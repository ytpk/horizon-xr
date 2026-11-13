//! Coplanar faces of full blocks merged into one quad that repeats its sprite per block:
//! fewer triangles from the same section, for GPUs whose frame is bound by geometry (a
//! headset's Adreno; the renderer turns it on with [`merge_faces`], nothing else does).
//!
//! Two faces merge only when the merged quad draws the very pixels they drew:
//! - both are whole block faces on the block grid, opaque (`OPAQUE_LAYER`), in one plane,
//!   facing one way, with their corners in the same order;
//! - each has one colour and one light at all four corners (so there is nothing to
//!   interpolate), and they are the same in both, as are the sprite layer and the tint;
//! - their UVs are the whole sprite, turned and mirrored the same way;
//! - no other quad lies over either of them (a grass block's side overlay is drawn at its
//!   base's depth, which only the same four corners give).
//!
//! A merged quad's UVs count blocks down from 1 (`MERGED_STEP`): the corner that had 1
//! keeps it, the one that had 0 gets `1 - blocks`. terrain.wgsl's `terrain_uv` decodes it
//! and `tile_uv` wraps it per block when the terrain shader is built for merged meshes
//! (gfx.rs's `terrain_merged`).
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use super::{OPAQUE_LAYER, TerrainVertex};

/// `TerrainVertex` UV of 1: the highest a face that was not merged has.
const ONE: u16 = 32768;
/// Fixed-point steps per block of a merged quad's UV below 0: `ONE + blocks * MERGED_STEP`
/// is `-blocks`, so 15 blocks below 0 (a 16 block face) fit.
pub const MERGED_STEP: u16 = 2048;

static SHRINK: AtomicBool = AtomicBool::new(false);
/// Whether crossed cutout quads (plants) are meshed over the part of their sprite that
/// draws (`BakedFace::shrunk`) from now on: fewer pixels for the cutout pass to throw away.
pub fn shrink_plants(on: bool) {
    SHRINK.store(on, Ordering::Relaxed);
}
/// Whether [`shrink_plants`] turned it on.
pub fn shrinking_plants() -> bool {
    SHRINK.load(Ordering::Relaxed)
}

static MERGE: AtomicBool = AtomicBool::new(false);
static AGGRESSIVE: AtomicBool = AtomicBool::new(false);
/// Allows lower-quality meshes: quantized flat light/colour and uniform texture orientation.
/// Still keeps sprites, emission, winding and overlaid faces separate.
pub fn aggressive_merging(on: bool) {
    AGGRESSIVE.store(on, Ordering::Relaxed);
}
/// Turns merging on or off for the meshes sorted from now on. The terrain shader built
/// while it is on reads merged UVs.
pub fn merge_faces(on: bool) {
    MERGE.store(on, Ordering::Relaxed);
}
/// Whether [`merge_faces`] turned merging on.
pub fn merging() -> bool {
    MERGE.load(Ordering::Relaxed)
}

/// What became of the solid quads given to [`merge`], summed over every call.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    /// Solid quads in.
    pub quads: u64,
    /// Of them, opaque ones (`OPAQUE_LAYER`).
    pub opaque: u64,
    /// Opaque quads that are no whole block face on the grid.
    pub not_block_face: u64,
    /// Whole opaque block faces under another quad.
    pub overlaid: u64,
    /// Whole opaque block faces whose light or colour differs between corners.
    pub shaded: u64,
    /// Whole opaque block faces whose UVs are not the whole sprite.
    pub partial_uv: u64,
    /// Faces that could merge and had no equal neighbour.
    pub alone: u64,
    /// Faces that went into a merged quad.
    pub merged: u64,
    /// Solid quads out.
    pub out: u64,
}
static STATS: [AtomicU64; 9] = [const { AtomicU64::new(0) }; 9];
/// The totals since the process started.
pub fn stats() -> Stats {
    let v: [u64; 9] = std::array::from_fn(|i| STATS[i].load(Ordering::Relaxed));
    Stats {
        quads: v[0],
        opaque: v[1],
        not_block_face: v[2],
        overlaid: v[3],
        shaded: v[4],
        partial_uv: v[5],
        alone: v[6],
        merged: v[7],
        out: v[8],
    }
}
fn record(s: &Stats) {
    for (total, v) in STATS.iter().zip([
        s.quads,
        s.opaque,
        s.not_block_face,
        s.overlaid,
        s.shaded,
        s.partial_uv,
        s.alone,
        s.merged,
        s.out,
    ]) {
        total.fetch_add(v, Ordering::Relaxed);
    }
}

/// Planes per direction: a section's faces lie on 0 to 16.
const PLANES: usize = 17;
const CELLS: usize = 6 * PLANES * 256;

/// A whole block face on the grid: its cell in the scratch grids, and which of its corners
/// are at the higher `u` (bits 0 to 3) and the higher `w` (bits 4 to 7), `u` and `w` the
/// axes after the plane's.
fn block_face(q: &[TerrainVertex; 4]) -> Option<(usize, u8)> {
    let p = q.map(|v| v.pos);
    let axis = (0..3).find(|&a| p.iter().all(|v| v[a] == p[0][a]))?;
    let (u, w) = ((axis + 1) % 3, (axis + 2) % 3);
    let whole = |v: f32, most: f32| (v.fract() == 0.0 && (0.0..=most).contains(&v)).then_some(v);
    let plane = whole(p[0][axis], 16.0)? as usize;
    let low = |k: usize| p.iter().map(|v| v[k]).fold(f32::INFINITY, f32::min);
    let (u0, w0) = (whole(low(u), 15.0)?, whole(low(w), 15.0)?);
    let mut corners = 0u8;
    for (i, v) in p.iter().enumerate() {
        let high = |c: f32, low: f32| match c - low {
            0.0 => Some(false),
            1.0 => Some(true),
            _ => None,
        };
        corners |= u8::from(high(v[u], u0)?) << i | u8::from(high(v[w], w0)?) << (4 + i);
    }
    // The four corners of the square, in order round it: both triangles (0, 1, 2 and
    // 0, 2, 3) then face the same way.
    let at = |i: usize| (corners >> i & 1, corners >> (4 + i) & 1);
    let round = (0..4).all(|i| {
        let (a, b) = (at(i), at((i + 1) % 4));
        (a.0 != b.0) != (a.1 != b.1)
    });
    if !round || at(0) == at(2) {
        return None;
    }
    // Counter-clockwise seen from the positive side of the axis faces it (sides.rs).
    let step = |i: usize| {
        let (a, b) = (at(i), at((i + 1) % 4));
        (
            i32::from(b.0) - i32::from(a.0),
            i32::from(b.1) - i32::from(a.1),
        )
    };
    let (first, second) = (step(0), step(1));
    let positive = first.0 * second.1 - first.1 * second.0 > 0;
    let direction = axis * 2 + usize::from(positive);
    Some((
        ((direction * PLANES + plane) * 16 + w0 as usize) * 16 + u0 as usize,
        corners,
    ))
}

/// What two faces must share to merge, beside lying side by side in one plane.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Key {
    corners: u8,
    layer: u16,
    light: u16,
    color: u32,
    uv: [[u16; 2]; 4],
    tint: u32,
}

/// The face's key, or why it has none.
enum Face {
    Mergeable(Key),
    Shaded,
    PartialUv,
}
fn face(q: &[TerrainVertex; 4], corners: u8, tint: u32) -> Face {
    if q.iter()
        .any(|v| v.color != q[0].color || v.light != q[0].light)
    {
        return Face::Shaded;
    }
    // Each UV component is 0 at one end and 1 at the other of one of the square's axes,
    // and the two components take one axis each.
    let mut axes = [0u8; 2];
    for (c, axis) in axes.iter_mut().enumerate() {
        let mut ones = 0u8;
        for (i, v) in q.iter().enumerate() {
            match v.uv[c] {
                0 => {}
                ONE => ones |= 1 << i,
                _ => return Face::PartialUv,
            }
        }
        let (along_u, along_w) = (corners & 15, corners >> 4);
        *axis = if ones == along_u || ones == !along_u & 15 {
            1
        } else if ones == along_w || ones == !along_w & 15 {
            2
        } else {
            return Face::PartialUv;
        };
    }
    if axes[0] == axes[1] {
        return Face::PartialUv;
    }
    Face::Mergeable(Key {
        corners,
        layer: q[0].layer,
        light: q[0].light,
        color: q[0].color,
        uv: q.map(|v| v.uv),
        tint,
    })
}

/// `face` stretched over `across` blocks along `u` and `up` along `w`, from the cell
/// (`u0`, `w0`).
fn stretched(
    face: &[TerrainVertex; 4],
    key: &Key,
    (u0, w0): (usize, usize),
    (across, up): (usize, usize),
) -> [TerrainVertex; 4] {
    let axis = (0..3)
        .find(|&a| face.iter().all(|v| v.pos[a] == face[0].pos[a]))
        .expect("a block face lies in a plane");
    let (u, w) = ((axis + 1) % 3, (axis + 2) % 3);
    let (along_u, along_w) = (key.corners & 15, key.corners >> 4);
    std::array::from_fn(|i| {
        let mut v = face[i];
        let (high_u, high_w) = (along_u >> i & 1 != 0, along_w >> i & 1 != 0);
        v.pos[u] = (u0 + if high_u { across } else { 0 }) as f32;
        v.pos[w] = (w0 + if high_w { up } else { 0 }) as f32;
        for c in 0..2 {
            // The axis this component runs along: the one its value changes with.
            let ones: u8 = (0..4).fold(0, |m, k| m | u8::from(face[k].uv[c] == ONE) << k);
            let blocks = if ones == along_u || ones == !along_u & 15 {
                across
            } else {
                up
            };
            if v.uv[c] == 0 && blocks > 1 {
                v.uv[c] = ONE + (blocks as u16 - 1) * MERGED_STEP;
            }
        }
        v
    })
}

#[derive(Default)]
struct Scratch {
    /// Quads (solid and translucent) lying on each cell.
    count: Vec<u8>,
    /// The mergeable solid quad of each cell, plus one; 0 for none.
    quad: Vec<u32>,
    keys: Vec<Option<Key>>,
    /// Per solid quad: 0 kept as it is, 1 dropped, 2 replaced by `merged[i]`.
    fate: Vec<u8>,
    merged: Vec<(u32, [TerrainVertex; 4])>,
    out: Vec<TerrainVertex>,
    tints: Vec<u32>,
}
thread_local! {
    static SCRATCH: std::cell::RefCell<Scratch> = Default::default();
}

/// Merges `solid`'s mergeable block faces (see the module), each merged quad taking the
/// place of its first face; every other quad stays where it was. `tints` (one per solid
/// quad, or none) follows. `translucent` only tells which faces lie under another quad.
pub fn merge(
    solid: &mut Vec<TerrainVertex>,
    tints: &mut Vec<u32>,
    translucent: &[TerrainVertex],
) -> Stats {
    let aggressive = AGGRESSIVE.load(Ordering::Relaxed);
    let stats =
        SCRATCH.with_borrow_mut(|s| merge_quality(solid, tints, translucent, s, aggressive));
    record(&stats);
    stats
}

#[cfg(test)]
fn merge_with(
    solid: &mut Vec<TerrainVertex>,
    tints: &mut Vec<u32>,
    translucent: &[TerrainVertex],
    s: &mut Scratch,
) -> Stats {
    merge_quality(solid, tints, translucent, s, false)
}

/// Normalize only whole, non-overlaid opaque faces. Never move positions or change sprites.
fn simplify(q: &mut [TerrainVertex; 4], corners: u8, tint: u32) -> Face {
    // Preserve emission and spare alpha bits; unlike RGB, they are discrete flags.
    let alpha = q[0].color & 0xff00_0000;
    let uniform = q.map(|v| TerrainVertex {
        color: q[0].color,
        light: q[0].light,
        ..v
    });
    if q.iter()
        .any(|v| v.color & 0xff00_0000 != alpha || v.layer != q[0].layer)
    {
        return Face::Shaded;
    }
    match face(&uniform, corners, tint) {
        Face::Mergeable(_) => {}
        other => return other,
    }
    // Baked block rotations can change which corner starts the quad as well as its UVs.
    // Canonicalize the start without reversing the winding; flat lighting has no diagonal.
    let first = (0..4)
        .find(|&i| corners >> i & 1 == 0 && corners >> (4 + i) & 1 == 0)
        .expect("a whole face has a low/low corner");
    q.rotate_left(first);
    let rotate = |bits: u8| ((bits >> first) | (bits << (4 - first))) & 15;
    let corners = rotate(corners & 15) | (rotate(corners >> 4) << 4);
    let mut color = alpha;
    for shift in [0, 8, 16] {
        let mean = q.iter().map(|v| (v.color >> shift) & 255).sum::<u32>() / 4;
        color |= ((mean + 16) / 32 * 32).min(255) << shift;
    }
    let mut light = 0u16;
    for shift in [0, 8] {
        let mean = q
            .iter()
            .map(|v| u32::from(v.light >> shift & 255))
            .sum::<u32>()
            / 4;
        // Four-light-level steps, retaining level 15 rather than exceeding the lightmap.
        light |= (((mean + 32) / 64 * 64).min(240) as u16) << shift;
    }
    for (i, v) in q.iter_mut().enumerate() {
        v.color = color;
        v.light = light;
        v.uv = [
            u16::from(corners >> i & 1) * ONE,
            u16::from(corners >> (4 + i) & 1) * ONE,
        ];
    }
    Face::Mergeable(Key {
        corners,
        layer: q[0].layer,
        light,
        color,
        uv: q.map(|v| v.uv),
        tint,
    })
}

fn merge_quality(
    solid: &mut Vec<TerrainVertex>,
    tints: &mut Vec<u32>,
    translucent: &[TerrainVertex],
    s: &mut Scratch,
    aggressive: bool,
) -> Stats {
    let quads = solid.as_chunks::<4>().0;
    let mut stats = Stats {
        quads: quads.len() as u64,
        out: quads.len() as u64,
        ..Stats::default()
    };
    if quads.len() < 2 {
        stats.opaque = quads
            .iter()
            .filter(|q| q[0].layer & OPAQUE_LAYER != 0)
            .count() as u64;
        stats.alone = stats.opaque;
        return stats;
    }
    let tinted = tints.len() == quads.len();
    s.count.clear();
    s.count.resize(CELLS, 0);
    s.quad.clear();
    s.quad.resize(CELLS, 0);
    s.keys.clear();
    s.keys.resize(quads.len(), None);
    for q in quads.iter().chain(translucent.as_chunks::<4>().0) {
        if let Some((cell, _)) = block_face(q) {
            s.count[cell] = s.count[cell].saturating_add(1);
        }
    }
    let mut any = false;
    for (i, q) in solid.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        if q[0].layer & OPAQUE_LAYER == 0 {
            continue;
        }
        stats.opaque += 1;
        let Some((cell, corners)) = block_face(q) else {
            stats.not_block_face += 1;
            continue;
        };
        if s.count[cell] != 1 {
            stats.overlaid += 1;
            continue;
        }
        let tint = if tinted { tints[i] } else { 0 };
        let candidate = if aggressive {
            simplify(q, corners, tint)
        } else {
            face(q, corners, tint)
        };
        match candidate {
            Face::Mergeable(key) => {
                s.keys[i] = Some(key);
                s.quad[cell] = i as u32 + 1;
                any = true;
            }
            Face::Shaded => stats.shaded += 1,
            Face::PartialUv => stats.partial_uv += 1,
        }
    }
    if !any {
        return stats;
    }
    let quads = solid.as_chunks::<4>().0;
    s.fate.clear();
    s.fate.resize(quads.len(), 0);
    s.merged.clear();
    for plane in 0..6 * PLANES {
        let base = plane * 256;
        if s.quad[base..base + 256].iter().all(|&q| q == 0) {
            continue;
        }
        for w0 in 0..16 {
            for u0 in 0..16 {
                let first = s.quad[base + w0 * 16 + u0];
                if first == 0 {
                    continue;
                }
                let first = first as usize - 1;
                let key = s.keys[first].expect("a mergeable face has a key");
                let same = |quad: u32| quad != 0 && s.keys[quad as usize - 1] == Some(key);
                let mut across = 1;
                while u0 + across < 16 && same(s.quad[base + w0 * 16 + u0 + across]) {
                    across += 1;
                }
                let mut up = 1;
                while w0 + up < 16
                    && (0..across).all(|k| same(s.quad[base + (w0 + up) * 16 + u0 + k]))
                {
                    up += 1;
                }
                if across * up == 1 {
                    s.quad[base + w0 * 16 + u0] = 0;
                    stats.alone += 1;
                    continue;
                }
                // The merged quad stands where the first of its faces in mesh order was.
                let mut place = first;
                for dw in 0..up {
                    for du in 0..across {
                        let cell = base + (w0 + dw) * 16 + u0 + du;
                        let quad = s.quad[cell] as usize - 1;
                        s.quad[cell] = 0;
                        s.fate[quad] = 1;
                        place = place.min(quad);
                    }
                }
                s.fate[place] = 2;
                s.merged.push((
                    place as u32,
                    stretched(&quads[first], &key, (u0, w0), (across, up)),
                ));
                stats.merged += (across * up) as u64;
                stats.out -= (across * up) as u64 - 1;
            }
        }
    }
    if s.merged.is_empty() {
        return stats;
    }
    s.merged.sort_unstable_by_key(|m| m.0);
    s.out.clear();
    s.tints.clear();
    let mut merged = s.merged.iter();
    for (i, q) in quads.iter().enumerate() {
        let quad = match s.fate[i] {
            0 => q,
            1 => continue,
            _ => &merged.next().expect("one merged quad per place").1,
        };
        s.out.extend_from_slice(quad);
        if tinted {
            s.tints.push(tints[i]);
        }
    }
    std::mem::swap(solid, &mut s.out);
    if tinted {
        std::mem::swap(tints, &mut s.tints);
    }
    stats
}

#[cfg(test)]
mod tests {
    use super::*;

    const STONE: u16 = 7 | OPAQUE_LAYER;
    const WHITE: u32 = 0xFFFF_FFFF;
    const DAY: u16 = 0x00F0;

    #[test]
    fn aggressive_merging_flattens_shading_and_rotation_without_changing_coverage() {
        let mut solid = Vec::new();
        for z in 0..16 {
            for x in 0..16 {
                let mut q = top(x, 0, z, STONE, WHITE, DAY);
                let uv = q.map(|v| v.uv);
                for (i, v) in q.iter_mut().enumerate() {
                    v.color = [0xffe0_e0e0, 0xffe8_e8e8, 0xfff0_f0f0, 0xffe8_e8e8][i];
                    v.light = [0x20e0, 0x20d0, 0x20e0, 0x20d0][i];
                    v.uv = uv[(i + (x + z) as usize) % 4];
                }
                q.rotate_left(usize::from((3 * x + z) % 4));
                solid.extend(q);
            }
        }
        let mut exact = solid.clone();
        merge_quality(&mut exact, &mut vec![], &[], &mut Scratch::default(), false);
        assert_eq!(
            bytemuck::cast_slice::<_, u8>(&exact),
            bytemuck::cast_slice::<_, u8>(&solid),
            "exact mode keeps AO and texture variation"
        );
        let stats = merge_quality(&mut solid, &mut vec![], &[], &mut Scratch::default(), true);
        assert_eq!((stats.quads, stats.out), (256, 1));
        let q: &[TerrainVertex; 4] = solid.as_slice().try_into().unwrap();
        assert_eq!(
            q.map(|v| v.pos),
            [[0., 1., 0.], [0., 1., 16.], [16., 1., 16.], [16., 1., 0.]]
        );
        assert!(
            q.iter()
                .all(|v| v.layer == STONE && v.color == 0xffe0_e0e0 && v.light == 0x40c0)
        );
        assert_eq!(
            q.map(|v| v.uv.map(decoded)),
            [[-15., -15.], [1., -15.], [1., 1.], [-15., 1.]]
        );
    }

    #[test]
    fn aggressive_merging_preserves_overlays_partial_uvs_cutout_and_emission_boundaries() {
        let a = top(0, 0, 0, STONE, 0xffe8_e8e8, DAY);
        let mut partial = top(1, 0, 0, STONE, WHITE, DAY);
        partial.iter_mut().for_each(|v| v.uv[0] /= 2);
        let overlay = top(0, 0, 0, 7, 0xff12_3456, DAY);
        let mut solid = [a, partial, overlay].as_flattened().to_vec();
        let original = solid.clone();
        merge_quality(&mut solid, &mut vec![], &[], &mut Scratch::default(), true);
        assert_eq!(
            bytemuck::cast_slice::<_, u8>(&solid),
            bytemuck::cast_slice::<_, u8>(&original)
        );
        let mut solid = [a, top(1, 0, 0, STONE, 0xfce8_e8e8, DAY)]
            .as_flattened()
            .to_vec();
        merge_quality(&mut solid, &mut vec![], &[], &mut Scratch::default(), true);
        assert_eq!(solid.len(), 8, "different emission tiers must not merge");
        assert_eq!(solid[0].color >> 24, 255);
        assert_eq!(solid[4].color >> 24, 252);
    }

    #[test]
    #[ignore = "synthetic geometry/CPU benchmark; no game resources or GPU FPS"]
    fn quality_mesh_benchmark() {
        let mut source = Vec::new();
        for z in 0..16 {
            for x in 0..16 {
                let mut q = top(x, 0, z, STONE, WHITE, DAY);
                q[1].color = 0xfff0_f0f0;
                let uv = q.map(|v| v.uv);
                for (i, v) in q.iter_mut().enumerate() {
                    v.uv = uv[(i + usize::from((x + z) % 4)) % 4];
                }
                source.extend(q);
            }
        }
        for aggressive in [false, true] {
            let mut scratch = Scratch::default();
            let mut work = Vec::new();
            let mut output = 0;
            let started = std::time::Instant::now();
            for _ in 0..1000 {
                work.clear();
                work.extend_from_slice(&source);
                let stats = merge_quality(&mut work, &mut vec![], &[], &mut scratch, aggressive);
                output = stats.out;
                std::hint::black_box(&work);
            }
            println!(
                "aggressive={aggressive} quads_in=256 quads_out={output} iterations=1000 elapsed_us={}",
                started.elapsed().as_micros()
            );
        }
    }

    /// The top face of the block at (`x`, `y`, `z`), its sprite the usual way up: u along
    /// +X, v along +Z.
    fn top(x: u8, y: u8, z: u8, layer: u16, color: u32, light: u16) -> [TerrainVertex; 4] {
        let (x, y, z) = (f32::from(x), f32::from(y) + 1.0, f32::from(z));
        [
            ([x, y, z], [0.0, 0.0]),
            ([x, y, z + 1.0], [0.0, 1.0]),
            ([x + 1.0, y, z + 1.0], [1.0, 1.0]),
            ([x + 1.0, y, z], [1.0, 0.0]),
        ]
        .map(|(pos, uv)| TerrainVertex::new(pos, uv, layer, color, light))
    }
    /// The north (-Z) face of the block at (`x`, `y`, `z`): u runs toward -X, v down.
    fn north(x: u8, y: u8, z: u8) -> [TerrainVertex; 4] {
        let (x, y, z) = (f32::from(x), f32::from(y), f32::from(z));
        [
            ([x + 1.0, y + 1.0, z], [0.0, 0.0]),
            ([x + 1.0, y, z], [0.0, 1.0]),
            ([x, y, z], [1.0, 1.0]),
            ([x, y + 1.0, z], [1.0, 0.0]),
        ]
        .map(|(pos, uv)| TerrainVertex::new(pos, uv, STONE, WHITE, DAY))
    }
    fn merged(faces: &[[TerrainVertex; 4]]) -> (Vec<[TerrainVertex; 4]>, Stats) {
        let mut solid = faces.as_flattened().to_vec();
        let stats = merge_with(&mut solid, &mut Vec::new(), &[], &mut Scratch::default());
        (solid.as_chunks::<4>().0.to_vec(), stats)
    }
    /// What terrain.wgsl's `terrain_uv` makes of a section's packed UV with merging on.
    fn decoded(uv: u16) -> f32 {
        if uv > ONE {
            -f32::from(uv - ONE) / f32::from(MERGED_STEP)
        } else {
            f32::from(uv) / f32::from(ONE)
        }
    }
    fn corners(q: &[TerrainVertex; 4]) -> Vec<([f32; 3], [f32; 2])> {
        q.iter().map(|v| (v.pos, v.uv.map(decoded))).collect()
    }

    /// A 3 by 2 patch of equal top faces becomes one quad over the same rectangle, its
    /// corners in the faces' order, whose UV crosses one whole sprite per block and ends
    /// on 1 where the faces' did.
    #[test]
    fn equal_faces_in_a_plane_become_one_quad_repeating_the_sprite() {
        let faces: Vec<_> = [(2, 5), (3, 5), (4, 5), (2, 6), (3, 6), (4, 6)]
            .map(|(x, z)| top(x, 9, z, STONE, WHITE, DAY))
            .to_vec();
        let (out, stats) = merged(&faces);
        assert_eq!(out.len(), 1);
        assert_eq!(
            corners(&out[0]),
            [
                ([2.0, 10.0, 5.0], [-2.0, -1.0]),
                ([2.0, 10.0, 7.0], [-2.0, 1.0]),
                ([5.0, 10.0, 7.0], [1.0, 1.0]),
                ([5.0, 10.0, 5.0], [1.0, -1.0]),
            ]
        );
        assert!(
            out[0]
                .iter()
                .all(|v| v.layer == STONE && v.color == WHITE && v.light == DAY)
        );
        assert_eq!((stats.quads, stats.merged, stats.out), (6, 6, 1));
    }

    /// A mirrored sprite (u falling as x rises) keeps its direction: the corner that had
    /// u = 0 goes below 0, the one that had 1 keeps 1.
    #[test]
    fn a_mirrored_face_stays_mirrored() {
        let (out, _) = merged(&[north(4, 3, 0), north(5, 3, 0), north(6, 3, 0)]);
        assert_eq!(out.len(), 1);
        assert_eq!(
            corners(&out[0]),
            [
                ([7.0, 4.0, 0.0], [-2.0, 0.0]),
                ([7.0, 3.0, 0.0], [-2.0, 1.0]),
                ([4.0, 3.0, 0.0], [1.0, 1.0]),
                ([4.0, 4.0, 0.0], [1.0, 0.0]),
            ]
        );
    }

    /// Whatever would change a pixel keeps two faces apart: another sprite, colour, light
    /// or tint, a turned sprite, a face shaded across its corners, a cutout face, a face in
    /// another plane or facing the other way, and a face under an overlay.
    #[test]
    fn faces_that_differ_stay_apart() {
        let base = top(0, 0, 0, STONE, WHITE, DAY);
        let beside = |f: &dyn Fn(&mut [TerrainVertex; 4])| {
            let mut other = top(1, 0, 0, STONE, WHITE, DAY);
            f(&mut other);
            merged(&[base, other]).0.len()
        };
        assert_eq!(beside(&|_| {}), 1, "the control: equal faces do merge");
        assert_eq!(
            beside(&|q| q.iter_mut().for_each(|v| v.layer = 8 | OPAQUE_LAYER)),
            2
        );
        assert_eq!(beside(&|q| q.iter_mut().for_each(|v| v.layer = 7)), 2);
        assert_eq!(
            beside(&|q| q.iter_mut().for_each(|v| v.color = 0xFF80_8080)),
            2
        );
        assert_eq!(beside(&|q| q.iter_mut().for_each(|v| v.light = 0x00E0)), 2);
        assert_eq!(beside(&|q| q[2].color = 0xFFCC_CCCC), 2);
        assert_eq!(beside(&|q| q[1].light = 0x00D0), 2);
        assert_eq!(beside(&|q| q.iter_mut().for_each(|v| v.pos[1] = 2.0)), 2);
        assert_eq!(beside(&|q| q.iter_mut().for_each(|v| v.pos[2] += 2.0)), 2);
        // The sprite a quarter turn round.
        assert_eq!(
            beside(&|q| {
                let uv = q.map(|v| v.uv);
                for (i, v) in q.iter_mut().enumerate() {
                    v.uv = uv[(i + 1) % 4];
                }
            }),
            2
        );
        // The same square seen from below.
        assert_eq!(beside(&|q| q.swap(1, 3)), 2);
        // Half the sprite.
        assert_eq!(beside(&|q| q.iter_mut().for_each(|v| v.uv[0] /= 2)), 2);
        // Off the grid, as an offset plant's quads are.
        assert_eq!(beside(&|q| q.iter_mut().for_each(|v| v.pos[0] += 0.25)), 2);

        let mut tints = vec![0x55AA55, 0x55AA56];
        let mut solid = [base, top(1, 0, 0, STONE, WHITE, DAY)]
            .as_flattened()
            .to_vec();
        merge_with(&mut solid, &mut tints, &[], &mut Scratch::default());
        assert_eq!((solid.len(), tints.len()), (8, 2));

        // An overlay on the second face (a grass block's side): neither merges with it,
        // and the overlay still follows its base.
        let overlay = top(1, 0, 0, 20, 0xFF40_C040, DAY);
        let third = top(2, 0, 0, STONE, WHITE, DAY);
        let (out, stats) = merged(&[base, top(1, 0, 0, STONE, WHITE, DAY), overlay, third]);
        assert_eq!(out.len(), 4);
        assert_eq!(out[2][0].layer, 20);
        assert_eq!(stats.overlaid, 1);
        // The same overlay in the translucent list.
        let mut solid = [base, top(1, 0, 0, STONE, WHITE, DAY)]
            .as_flattened()
            .to_vec();
        merge_with(
            &mut solid,
            &mut Vec::new(),
            &overlay,
            &mut Scratch::default(),
        );
        assert_eq!(solid.len(), 8);
    }

    /// Merged quads cover exactly the cells their faces covered, once each, and the quads
    /// that did not merge stay in their order with the merged ones where their first
    /// face was.
    #[test]
    fn an_l_shaped_patch_is_covered_once_and_order_is_kept() {
        let plant = [
            [0.2, 0.0, 0.2],
            [0.8, 0.0, 0.8],
            [0.8, 1.0, 0.8],
            [0.2, 1.0, 0.2],
        ]
        .map(|pos| TerrainVertex::new(pos, [0.0, 0.0], 30, WHITE, DAY));
        let cells = [(0, 0), (1, 0), (2, 0), (0, 1), (0, 2), (5, 5)];
        let mut faces = vec![plant];
        faces.extend(cells.map(|(x, z)| top(x, 0, z, STONE, WHITE, DAY)));
        faces.insert(3, north(9, 9, 9));
        let (out, stats) = merged(&faces);
        let mut covered = std::collections::BTreeMap::new();
        for q in out
            .iter()
            .filter(|q| q[0].layer == STONE && q[0].pos[1] == 1.0)
        {
            let low = |k: usize| q.iter().map(|v| v.pos[k]).fold(f32::INFINITY, f32::min) as u8;
            let high = |k: usize| q.iter().map(|v| v.pos[k]).fold(0.0, f32::max) as u8;
            for x in low(0)..high(0) {
                for z in low(2)..high(2) {
                    *covered.entry((x, z)).or_insert(0) += 1;
                }
            }
        }
        assert_eq!(
            covered.into_iter().collect::<Vec<_>>(),
            [(0, 0), (0, 1), (0, 2), (1, 0), (2, 0), (5, 5)].map(|c| (c, 1))
        );
        assert_eq!(out[0][0].layer, 30, "the plant is still first");
        assert_eq!(out.len(), 5, "plant, two merged quads, north face, single");
        assert_eq!(
            out[3][0].pos,
            [10.0, 10.0, 9.0],
            "the north face follows what it followed"
        );
        assert_eq!(
            out[4][0].pos,
            [5.0, 1.0, 5.0],
            "the single face is last and as it was"
        );
        assert_eq!((stats.merged, stats.alone, stats.out), (5, 2, 5));
    }

    /// A whole 16 by 16 layer is one quad, and its lowest UV still fits the encoding.
    #[test]
    fn a_whole_layer_is_one_quad() {
        let faces: Vec<_> = (0..16)
            .flat_map(|z| (0..16).map(move |x| top(x, 15, z, STONE, WHITE, DAY)))
            .collect();
        let (out, _) = merged(&faces);
        assert_eq!(out.len(), 1);
        let uvs: Vec<[f32; 2]> = out[0].iter().map(|v| v.uv.map(decoded)).collect();
        assert_eq!(
            uvs,
            [[-15.0, -15.0], [-15.0, 1.0], [1.0, 1.0], [1.0, -15.0]]
        );
        assert_eq!(out[0][2].pos, [16.0, 16.0, 16.0]);
    }
}
