//! The headset launcher's room as data: boxes with a material per face, built into one small static
//! mesh (`room_scene.rs` draws it), and the room's own textures, made here from a few colours so
//! that the room stands before any Mojang file exists. Once the game's files are prepared,
//! `load_real` reads the real block textures for the same materials and the room "turns real".
//!
//! The room is 6 by 5 blocks inside and 3 high, in the headset's stage space (metres, +Y up, -Z
//! ahead, the origin where the player stands, on the floor): x from -3 to 3, z from -3 (the TV
//! wall) to 2 (the sofa's wall). A block is one metre.
use glam::{Quat, Vec3};
use std::path::Path;

/// The TV's screen: a panel in the room's space, facing the player (+Z), 2.4 m wide.
pub const TV_CENTRE: Vec3 = Vec3::new(0.0, 1.45, -2.86);
pub const TV_WIDTH: f32 = 2.4;
pub const TV_ROT: Quat = Quat::IDENTITY;

/// The interior, as `(min, max)` corners.
pub const INTERIOR: ([f32; 3], [f32; 3]) = ([-3.0, 0.0, -3.0], [3.0, 3.0, 2.0]);

/// Where the lights are: two lanterns, the fire, the window and the TV.
pub const LANTERNS: [Vec3; 2] = [Vec3::new(-1.4, 2.3, -0.6), Vec3::new(1.4, 2.3, -0.6)];
pub const FIRE: Vec3 = Vec3::new(2.62, 0.4, -1.0);
pub const WINDOW: Vec3 = Vec3::new(-3.0, 1.6, -1.0);
pub const TV_LIGHT: Vec3 = Vec3::new(0.0, 1.45, -2.4);

/// A material: its texture layer, and what the pass does with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Mat {
    OakPlanks,
    SprucePlanks,
    LogSide,
    LogTop,
    DarkOak,
    WoolRed,
    WoolWhite,
    WoolBrown,
    BlackConcrete,
    Bricks,
    Glass,
    Books,
    Lantern,
    Fire,
    Pot,
    Fern,
    Dirt,
    Iron,
    Stone,
}

pub const MATERIALS: usize = 19;
/// Texture layers of the array (a few spare).
pub const LAYERS: usize = 24;

const EMISSIVE: u32 = 1 << 9;
const CUTOUT: u32 = 1 << 10;
const FLAME: u32 = 1 << 11;

/// The real block texture of each material, in `assets/minecraft/textures/block/`, where the
/// game has one (the lantern is a model's layout, not a block texture: it stays as it is).
pub const REAL: [(Mat, &str); 18] = [
    (Mat::OakPlanks, "oak_planks"),
    (Mat::SprucePlanks, "spruce_planks"),
    (Mat::LogSide, "stripped_spruce_log"),
    (Mat::LogTop, "stripped_spruce_log_top"),
    (Mat::DarkOak, "dark_oak_planks"),
    (Mat::WoolRed, "red_wool"),
    (Mat::WoolWhite, "white_wool"),
    (Mat::WoolBrown, "brown_wool"),
    (Mat::BlackConcrete, "black_concrete"),
    (Mat::Bricks, "bricks"),
    (Mat::Glass, "glass"),
    (Mat::Books, "bookshelf"),
    (Mat::Fire, "campfire_fire"),
    (Mat::Pot, "flower_pot"),
    (Mat::Fern, "fern"),
    (Mat::Dirt, "dirt"),
    (Mat::Iron, "iron_block"),
    (Mat::Stone, "stone_bricks"),
];

/// One vertex: position, texture coordinates in blocks (the texture repeats once a block) and
/// the material word: layer in bits 0 to 5, the face's normal (`+x -x +y -y +z -z`) in bits 6 to 8,
/// then the flags above.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub uv: [f32; 2],
    pub data: u32,
}

#[derive(Default)]
pub struct Mesh {
    pub opaque: (Vec<Vertex>, Vec<u32>),
    /// Glass, drawn last with blending and no depth write.
    pub glass: (Vec<Vertex>, Vec<u32>),
}

fn word(mat: Mat, normal: usize) -> u32 {
    let flags = match mat {
        Mat::Lantern => EMISSIVE,
        Mat::Fire => EMISSIVE | CUTOUT | FLAME,
        Mat::Fern => CUTOUT,
        _ => 0,
    };
    mat as u32 | (normal as u32) << 6 | flags
}

/// A box with one material per face (`+x -x +y -y +z -z`; `None` leaves a face out).
#[derive(Clone, Copy, Debug)]
pub struct Block {
    pub min: Vec3,
    pub max: Vec3,
    pub faces: [Option<Mat>; 6],
}

impl Block {
    fn all(min: [f32; 3], max: [f32; 3], mat: Mat) -> Self {
        Self {
            min: Vec3::from(min),
            max: Vec3::from(max),
            faces: [Some(mat); 6],
        }
    }
    fn sides(min: [f32; 3], max: [f32; 3], side: Mat, top: Mat) -> Self {
        Self {
            faces: [
                Some(side),
                Some(side),
                Some(top),
                Some(side),
                Some(side),
                Some(side),
            ],
            ..Self::all(min, max, side)
        }
    }
    fn face(min: [f32; 3], max: [f32; 3], face: usize, mat: Mat) -> Self {
        let mut faces = [None; 6];
        faces[face] = Some(mat);
        Self {
            faces,
            ..Self::all(min, max, mat)
        }
    }

    fn push(&self, mesh: &mut (Vec<Vertex>, Vec<u32>)) {
        let (a, b) = (self.min, self.max);
        for (face, mat) in self.faces.iter().enumerate() {
            let Some(mat) = mat else { continue };
            // The four corners of the face, counter-clockwise from outside; uv in blocks.
            let (corners, uv): ([Vec3; 4], [[f32; 2]; 4]) = match face {
                0 => (
                    [
                        Vec3::new(b.x, a.y, b.z),
                        Vec3::new(b.x, a.y, a.z),
                        Vec3::new(b.x, b.y, a.z),
                        Vec3::new(b.x, b.y, b.z),
                    ],
                    [[b.z, -a.y], [a.z, -a.y], [a.z, -b.y], [b.z, -b.y]],
                ),
                1 => (
                    [
                        Vec3::new(a.x, a.y, a.z),
                        Vec3::new(a.x, a.y, b.z),
                        Vec3::new(a.x, b.y, b.z),
                        Vec3::new(a.x, b.y, a.z),
                    ],
                    [[a.z, -a.y], [b.z, -a.y], [b.z, -b.y], [a.z, -b.y]],
                ),
                2 => (
                    [
                        Vec3::new(a.x, b.y, b.z),
                        Vec3::new(b.x, b.y, b.z),
                        Vec3::new(b.x, b.y, a.z),
                        Vec3::new(a.x, b.y, a.z),
                    ],
                    [[a.x, b.z], [b.x, b.z], [b.x, a.z], [a.x, a.z]],
                ),
                3 => (
                    [
                        Vec3::new(a.x, a.y, a.z),
                        Vec3::new(b.x, a.y, a.z),
                        Vec3::new(b.x, a.y, b.z),
                        Vec3::new(a.x, a.y, b.z),
                    ],
                    [[a.x, a.z], [b.x, a.z], [b.x, b.z], [a.x, b.z]],
                ),
                4 => (
                    [
                        Vec3::new(a.x, a.y, b.z),
                        Vec3::new(b.x, a.y, b.z),
                        Vec3::new(b.x, b.y, b.z),
                        Vec3::new(a.x, b.y, b.z),
                    ],
                    [[a.x, -a.y], [b.x, -a.y], [b.x, -b.y], [a.x, -b.y]],
                ),
                _ => (
                    [
                        Vec3::new(b.x, a.y, a.z),
                        Vec3::new(a.x, a.y, a.z),
                        Vec3::new(a.x, b.y, a.z),
                        Vec3::new(b.x, b.y, a.z),
                    ],
                    [[b.x, -a.y], [a.x, -a.y], [a.x, -b.y], [b.x, -b.y]],
                ),
            };
            let first = mesh.0.len() as u32;
            for (c, uv) in corners.iter().zip(uv) {
                mesh.0.push(Vertex {
                    pos: c.to_array(),
                    uv,
                    data: word(*mat, face),
                });
            }
            mesh.1
                .extend([first, first + 1, first + 2, first, first + 2, first + 3]);
        }
    }
}

/// A vertical quad between two floor points, `height` tall from `y`, uv 0..1 (a fern's leaf, a
/// flame): seen from both sides.
fn standing(
    mesh: &mut (Vec<Vertex>, Vec<u32>),
    from: [f32; 2],
    to: [f32; 2],
    y: f32,
    height: f32,
    mat: Mat,
) {
    let first = mesh.0.len() as u32;
    let data = word(mat, 2);
    let corner = |p: [f32; 2], y: f32, uv: [f32; 2]| Vertex {
        pos: [p[0], y, p[1]],
        uv,
        data,
    };
    mesh.0.extend([
        corner(from, y, [0.0, 1.0]),
        corner(to, y, [1.0, 1.0]),
        corner(to, y + height, [1.0, 0.0]),
        corner(from, y + height, [0.0, 0.0]),
    ]);
    mesh.1
        .extend([first, first + 1, first + 2, first, first + 2, first + 3]);
}

/// Every box of the room.
#[allow(clippy::vec_init_then_push)]
pub fn blocks() -> Vec<Block> {
    use Mat::*;
    let mut b = Vec::new();
    // The shell, 0.5 m thick: only the faces inside show.
    b.push(Block::face(
        [-3.0, -0.5, -3.0],
        [3.0, 0.0, 2.0],
        2,
        OakPlanks,
    ));
    b.push(Block::face([-3.0, 3.0, -3.0], [3.0, 3.5, 2.0], 3, DarkOak));
    b.push(Block::face(
        [-3.0, 0.0, -3.5],
        [3.0, 3.0, -3.0],
        4,
        SprucePlanks,
    ));
    b.push(Block::face(
        [-3.0, 0.0, 2.0],
        [3.0, 3.0, 2.5],
        5,
        SprucePlanks,
    ));
    b.push(Block::face(
        [3.0, 0.0, -3.0],
        [3.5, 3.0, 2.0],
        1,
        SprucePlanks,
    ));
    // The west wall around its window (z -2 to 0, y 1 to 2.2): all faces, so the reveal shows.
    for (min, max) in [
        ([-3.5, 0.0, -3.0], [-3.0, 1.0, 2.0]),
        ([-3.5, 2.2, -3.0], [-3.0, 3.0, 2.0]),
        ([-3.5, 1.0, -3.0], [-3.0, 2.2, -2.0]),
        ([-3.5, 1.0, 0.0], [-3.0, 2.2, 2.0]),
    ] {
        b.push(Block::all(min, max, SprucePlanks));
    }
    // The window's frame and its panes (glass is in `glass_blocks`).
    b.push(Block::all(
        [-3.3, 1.0, -1.04],
        [-3.2, 2.2, -0.96],
        SprucePlanks,
    ));
    b.push(Block::all(
        [-3.3, 1.56, -2.0],
        [-3.2, 1.64, 0.0],
        SprucePlanks,
    ));
    b.push(Block::all(
        [-3.3, 0.96, -2.0],
        [-2.85, 1.0, 0.0],
        SprucePlanks,
    ));
    // Stripped logs in the four corners.
    for (x, z) in [(-3.0, -3.0), (2.6, -3.0), (-3.0, 1.6), (2.6, 1.6)] {
        b.push(Block::sides(
            [x, 0.0, z],
            [x + 0.4, 3.0, z + 0.4],
            LogSide,
            LogTop,
        ));
    }
    // Dark oak beams across the ceiling.
    for z in [-1.8, 0.4] {
        b.push(Block::all([-3.0, 2.75, z], [3.0, 3.0, z + 0.3], DarkOak));
    }
    // The rug in the middle: red, then white, then red, each a carpet's thickness higher.
    b.push(Block::sides(
        [-2.0, 0.0, -1.7],
        [2.0, 0.04, 0.8],
        WoolRed,
        WoolRed,
    ));
    b.push(Block::sides(
        [-1.75, 0.04, -1.45],
        [1.75, 0.08, 0.55],
        WoolWhite,
        WoolWhite,
    ));
    b.push(Block::sides(
        [-1.5, 0.08, -1.2],
        [1.5, 0.12, 0.3],
        WoolRed,
        WoolRed,
    ));
    // The TV: a black frame on the wall, its screen where the panel hangs, a low dark oak bench.
    b.push(Block::all(
        [-1.4, 0.72, -3.0],
        [1.4, 2.18, -2.9],
        BlackConcrete,
    ));
    b.push(Block::face(
        [-1.2, 0.775, -2.9],
        [1.2, 2.125, -2.88],
        4,
        BlackConcrete,
    ));
    b.push(Block::all([-1.3, 0.0, -3.0], [1.3, 0.45, -2.55], DarkOak));
    b.push(Block::all(
        [-1.3, 0.45, -3.0],
        [1.3, 0.5, -2.5],
        SprucePlanks,
    ));
    // The sofa against the south wall, of slabs and stairs: brown frame, red cushions.
    b.push(Block::all([-2.0, 0.0, 1.0], [-1.5, 0.62, 2.0], WoolBrown));
    b.push(Block::all([1.5, 0.0, 1.0], [2.0, 0.62, 2.0], WoolBrown));
    b.push(Block::all([-1.5, 0.0, 1.0], [1.5, 0.3, 2.0], WoolBrown));
    b.push(Block::all([-1.5, 0.3, 1.78], [1.5, 1.0, 2.0], WoolBrown));
    for i in 0..3 {
        let x = -1.5 + i as f32;
        b.push(Block::all(
            [x + 0.02, 0.3, 1.0],
            [x + 0.98, 0.5, 1.78],
            WoolRed,
        ));
        b.push(Block::all(
            [x + 0.04, 0.5, 1.62],
            [x + 0.96, 0.95, 1.78],
            WoolRed,
        ));
    }
    // The bookshelf, two blocks high, books on the side facing the room.
    b.push(Block {
        faces: [
            Some(Books),
            Some(OakPlanks),
            Some(OakPlanks),
            None,
            Some(OakPlanks),
            Some(OakPlanks),
        ],
        ..Block::all([-3.0, 0.0, 0.0], [-2.0, 1.0, 1.0], OakPlanks)
    });
    b.push(Block {
        faces: [
            Some(Books),
            Some(OakPlanks),
            Some(OakPlanks),
            None,
            Some(OakPlanks),
            Some(OakPlanks),
        ],
        ..Block::all([-3.0, 1.0, 0.0], [-2.0, 2.0, 1.0], OakPlanks)
    });
    // The side table beside the sofa: a slab top on four legs, a flower pot with soil.
    b.push(Block::all([2.1, 0.7, 1.15], [2.9, 0.8, 1.85], OakPlanks));
    for (x, z) in [(2.12, 1.17), (2.78, 1.17), (2.12, 1.73), (2.78, 1.73)] {
        b.push(Block::all(
            [x, 0.0, z],
            [x + 0.1, 0.7, z + 0.1],
            SprucePlanks,
        ));
    }
    b.push(Block::all([2.4, 0.8, 1.4], [2.6, 1.0, 1.6], Pot));
    b.push(Block::face([2.42, 1.0, 1.42], [2.58, 1.0, 1.58], 2, Dirt));
    // The fireplace on the east wall: bricks round a 1 m opening, a mantel, a chimney to the
    // ceiling, a stone hearth, and logs in the opening.
    b.push(Block::all([2.3, 0.0, -2.0], [3.0, 2.4, -1.5], Bricks));
    b.push(Block::all([2.3, 0.0, -0.5], [3.0, 2.4, 0.0], Bricks));
    b.push(Block::all([2.3, 1.0, -1.5], [3.0, 1.4, -0.5], Bricks));
    b.push(Block::all([2.6, 1.4, -1.5], [3.0, 3.0, -0.5], Bricks));
    b.push(Block::all([2.2, 1.4, -2.0], [2.6, 1.5, 0.0], OakPlanks));
    b.push(Block::all([2.0, 0.0, -2.1], [2.3, 0.08, 0.1], Stone));
    b.push(Block::all([2.9, 0.0, -1.5], [3.0, 1.0, -0.5], Bricks));
    // Carved opening: logs and embers on the hearth (the bricks behind are the back wall).
    b.push(Block::all([2.4, 0.0, -1.35], [2.5, 0.12, -0.65], DarkOak));
    b.push(Block::all([2.55, 0.0, -1.25], [2.65, 0.12, -0.75], DarkOak));
    b.push(Block::all([2.62, 0.0, -1.3], [2.82, 0.08, -0.7], Iron));
    b
}

/// The hanging lanterns: a chain from the ceiling, an iron cap and base round an emissive core.
fn lanterns(mesh: &mut (Vec<Vertex>, Vec<u32>)) {
    for at in LANTERNS {
        let (x, y, z) = (at.x, at.y, at.z);
        for block in [
            Block::all(
                [x - 0.02, y + 0.3, z - 0.02],
                [x + 0.02, 2.75, z + 0.02],
                Mat::Iron,
            ),
            Block::all(
                [x - 0.1, y + 0.2, z - 0.1],
                [x + 0.1, y + 0.3, z + 0.1],
                Mat::Iron,
            ),
            Block::all(
                [x - 0.12, y, z - 0.12],
                [x + 0.12, y + 0.2, z + 0.12],
                Mat::Lantern,
            ),
            Block::all(
                [x - 0.14, y - 0.06, z - 0.14],
                [x + 0.14, y, z + 0.14],
                Mat::Iron,
            ),
        ] {
            block.push(mesh);
        }
    }
}

/// Builds the whole mesh.
pub fn mesh() -> Mesh {
    let mut mesh = Mesh::default();
    for block in blocks() {
        block.push(&mut mesh.opaque);
    }
    lanterns(&mut mesh.opaque);
    // The fern in the pot: two crossed leaves; the flames in the hearth: two crossed quads.
    let (c, r) = ([2.5, 1.5], 0.27);
    standing(
        &mut mesh.opaque,
        [c[0] - r, c[1] - r],
        [c[0] + r, c[1] + r],
        1.0,
        0.5,
        Mat::Fern,
    );
    standing(
        &mut mesh.opaque,
        [c[0] + r, c[1] - r],
        [c[0] - r, c[1] + r],
        1.0,
        0.5,
        Mat::Fern,
    );
    let (f, r) = ([FIRE.x, FIRE.z], 0.32);
    standing(
        &mut mesh.opaque,
        [f[0], f[1] - r],
        [f[0], f[1] + r],
        0.1,
        0.6,
        Mat::Fire,
    );
    standing(
        &mut mesh.opaque,
        [f[0] - r, f[1]],
        [f[0] + r, f[1]],
        0.1,
        0.6,
        Mat::Fire,
    );
    // The window's panes, a block high and wide in four lights.
    for (z0, z1) in [(-2.0, -1.04), (-0.96, 0.0)] {
        for (y0, y1) in [(1.0, 1.56), (1.64, 2.2)] {
            Block::all([-3.27, y0, z0], [-3.23, y1, z1], Mat::Glass).push(&mut mesh.glass);
        }
    }
    mesh
}

// ----- Textures ------------------------------------------------------------------------------

/// A 16 by 16 RGBA texture.
pub type Texels = Vec<u8>;

fn hash(x: u32, y: u32, salt: u32) -> u32 {
    let mut h = x
        .wrapping_mul(0x9E37_79B1)
        .wrapping_add(y.wrapping_mul(0x85EB_CA6B))
        .wrapping_add(salt.wrapping_mul(0xC2B2_AE35));
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^ (h >> 15)
}

/// 0..1 from the hash.
fn noise(x: u32, y: u32, salt: u32) -> f32 {
    (hash(x, y, salt) & 0xFFFF) as f32 / 65535.0
}

fn shade(c: [f32; 3], k: f32) -> [f32; 3] {
    [c[0] * k, c[1] * k, c[2] * k]
}

fn texture(f: impl Fn(u32, u32) -> [f32; 4]) -> Texels {
    let mut out = Vec::with_capacity(16 * 16 * 4);
    for y in 0..16 {
        for x in 0..16 {
            let c = f(x, y);
            out.extend(c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8));
        }
    }
    out
}

fn planks(base: [f32; 3], salt: u32) -> Texels {
    texture(|x, y| {
        let row = y / 4;
        // Each board is its own tone, and the boards are staggered.
        let shift = hash(row, 0, salt) % 16;
        let board = (x + shift) / 8;
        let tone = 0.86 + 0.16 * noise(board, row, salt + 1);
        let grain = 0.95 + 0.07 * noise(x, row, salt + 2)
            - 0.04 * f32::from(u8::from((x + shift).is_multiple_of(8)));
        let seam = if y % 4 == 3 { 0.72 } else { 1.0 };
        let k = tone * grain * seam * (0.97 + 0.06 * noise(x, y, salt + 3));
        let c = shade(base, k);
        [c[0], c[1], c[2], 1.0]
    })
}

fn wool(base: [f32; 3], salt: u32) -> Texels {
    texture(|x, y| {
        let k = 0.88 + 0.2 * noise(x, y, salt);
        let c = shade(base, k);
        [c[0], c[1], c[2], 1.0]
    })
}

/// A material's own texture, made here.
pub fn procedural(mat: Mat) -> Texels {
    match mat {
        Mat::OakPlanks => planks([0.74, 0.57, 0.33], 10),
        Mat::SprucePlanks => planks([0.45, 0.33, 0.19], 20),
        Mat::DarkOak => planks([0.26, 0.17, 0.09], 30),
        Mat::LogSide => texture(|x, y| {
            let k = 0.84 + 0.13 * noise(x, 0, 41) + 0.04 * noise(x, y, 42);
            let c = shade([0.62, 0.46, 0.27], k);
            [c[0], c[1], c[2], 1.0]
        }),
        Mat::LogTop => texture(|x, y| {
            let (dx, dy) = (x as f32 - 7.5, y as f32 - 7.5);
            let ring = ((dx * dx + dy * dy).sqrt() * 0.9).sin() * 0.5 + 0.5;
            let c = shade([0.66, 0.5, 0.3], 0.82 + 0.2 * ring);
            [c[0], c[1], c[2], 1.0]
        }),
        Mat::WoolRed => wool([0.62, 0.12, 0.1], 50),
        Mat::WoolWhite => wool([0.9, 0.9, 0.86], 51),
        Mat::WoolBrown => wool([0.38, 0.24, 0.14], 52),
        Mat::BlackConcrete => texture(|x, y| {
            let k = 0.05 + 0.025 * noise(x, y, 60);
            [k, k, k * 1.1, 1.0]
        }),
        Mat::Bricks => texture(|x, y| {
            let row = y / 4;
            let mortar = y % 4 == 3 || (x + if row % 2 == 0 { 0 } else { 4 }) % 8 == 7;
            if mortar {
                let k = 0.55 + 0.1 * noise(x, y, 70);
                [k, k * 0.97, k * 0.92, 1.0]
            } else {
                let k = 0.85 + 0.25 * noise(x / 4, row, 71) + 0.06 * noise(x, y, 72);
                let c = shade([0.55, 0.22, 0.17], k);
                [c[0], c[1], c[2], 1.0]
            }
        }),
        Mat::Glass => texture(|x, y| {
            let edge = x == 0 || y == 0 || x == 15 || y == 15;
            let shine = (x + y == 5 || x + y == 6 || x + y == 9) && x > 2 && y > 2 && x < 9;
            if edge {
                [0.72, 0.85, 0.9, 0.55]
            } else if shine {
                [0.95, 0.98, 1.0, 0.4]
            } else {
                [0.7, 0.85, 0.95, 0.16]
            }
        }),
        Mat::Books => texture(|x, y| {
            // Boards on rows 0 to 1, 7 to 8 and 15; books between, a spine of its own colour.
            if y < 2 || (7..9).contains(&y) || y == 15 {
                let k = 0.9 + 0.1 * noise(x, y, 80);
                let c = shade([0.74, 0.57, 0.33], k);
                return [c[0], c[1], c[2], 1.0];
            }
            let shelf = u32::from(y > 8);
            let book = x / 2;
            let tint = hash(book, shelf, 81) % 6;
            let base = [
                [0.55, 0.12, 0.1],
                [0.15, 0.3, 0.5],
                [0.2, 0.42, 0.2],
                [0.5, 0.36, 0.14],
                [0.35, 0.2, 0.45],
                [0.6, 0.55, 0.4],
            ][tint as usize];
            let tall = 4 + hash(book, shelf, 82) % 3;
            let top = if shelf == 0 { 2 } else { 9 };
            if y >= top + (6 - tall).min(3) || y < top {
                let k = 0.85 + 0.15 * noise(x, y, 83);
                let c = shade(base, k);
                [c[0], c[1], c[2], 1.0]
            } else {
                [0.12, 0.09, 0.06, 1.0]
            }
        }),
        Mat::Lantern => texture(|x, y| {
            let frame = !(2..=13).contains(&x) || !(2..=13).contains(&y) || x == 7 || x == 8;
            if frame {
                [0.16, 0.14, 0.12, 1.0]
            } else {
                let k = 0.9 + 0.1 * noise(x, y, 90);
                [1.0 * k, 0.74 * k, 0.32 * k, 1.0]
            }
        }),
        Mat::Fire => texture(|x, y| {
            // A flame: wide at the foot, narrow at the tip, a bright heart.
            let h = 15.0 - y as f32;
            let half = 6.5 - h * 0.38 + 1.8 * noise(y, 0, 100) - 0.9;
            let d = (x as f32 - 7.5).abs();
            if d > half.max(0.0) || h < 0.0 {
                [0.0, 0.0, 0.0, 0.0]
            } else if d < half * 0.45 && h < 11.0 {
                [1.0, 0.92, 0.5, 1.0]
            } else {
                [1.0, 0.5 + 0.2 * noise(x, y, 101), 0.1, 1.0]
            }
        }),
        Mat::Pot => texture(|x, y| {
            let rim = y < 3;
            let k = if rim { 0.8 } else { 0.95 } + 0.1 * noise(x, y, 110);
            let c = shade([0.55, 0.28, 0.2], k);
            [c[0], c[1], c[2], 1.0]
        }),
        Mat::Fern => texture(|x, y| {
            // Fronds out of a point at the bottom middle.
            let (dx, dy) = ((x as f32 - 7.5).abs(), 15.0 - y as f32);
            let frond = (dy > 0.5 && dx < dy * 0.7 + 1.0) && ((x + y) % 3 != 0 || dx < 1.5);
            if frond && dy < 15.0 - (dx * 0.6) {
                let k = 0.75 + 0.35 * noise(x, y, 120);
                [0.2 * k, 0.55 * k, 0.16 * k, 1.0]
            } else {
                [0.0, 0.0, 0.0, 0.0]
            }
        }),
        Mat::Dirt => texture(|x, y| {
            let k = 0.7 + 0.35 * noise(x, y, 130);
            let c = shade([0.4, 0.28, 0.18], k);
            [c[0], c[1], c[2], 1.0]
        }),
        Mat::Iron => texture(|x, y| {
            let k = 0.4 + 0.07 * noise(x, y, 140);
            [k, k, k * 1.04, 1.0]
        }),
        Mat::Stone => texture(|x, y| {
            let mortar = y % 8 == 7 || (x + if y / 8 == 0 { 0 } else { 8 }) % 16 == 15;
            let k = if mortar {
                0.36
            } else {
                0.52 + 0.1 * noise(x / 4, y / 4, 150) + 0.05 * noise(x, y, 151)
            };
            [k, k, k * 1.02, 1.0]
        }),
    }
}

/// The room's texture array: every layer made here.
pub fn procedural_layers() -> Vec<Texels> {
    use Mat::*;
    let all = [
        OakPlanks,
        SprucePlanks,
        LogSide,
        LogTop,
        DarkOak,
        WoolRed,
        WoolWhite,
        WoolBrown,
        BlackConcrete,
        Bricks,
        Glass,
        Books,
        Lantern,
        Fire,
        Pot,
        Fern,
        Dirt,
        Iron,
        Stone,
    ];
    assert_eq!(all.len(), MATERIALS);
    let mut layers = vec![vec![255u8; 16 * 16 * 4]; LAYERS];
    for mat in all {
        layers[mat as usize] = procedural(mat);
    }
    layers
}

/// A PNG's top left 16 by 16 pixels as RGBA (an animated strip's first frame, a model sheet's
/// corner); none if the file is missing, not a PNG or smaller than that.
pub fn read_block_texture(path: &Path) -> Option<Texels> {
    let mut decoder = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(path).ok()?));
    // The game's block textures are mostly palette images of 2 to 8 bits: out to 8-bit RGBA.
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0u8; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    let channels = match info.color_type {
        png::ColorType::Rgba => 4,
        png::ColorType::Rgb => 3,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Grayscale => 1,
        png::ColorType::Indexed => return None,
    };
    if info.bit_depth != png::BitDepth::Eight || info.width < 16 || info.height < 16 {
        return None;
    }
    let mut out = Vec::with_capacity(16 * 16 * 4);
    for y in 0..16usize {
        for x in 0..16usize {
            let at = (y * info.width as usize + x) * channels;
            let p = &buf[at..at + channels];
            out.extend(match channels {
                4 => [p[0], p[1], p[2], p[3]],
                3 => [p[0], p[1], p[2], 255],
                2 => [p[0], p[0], p[0], p[1]],
                _ => [p[0], p[0], p[0], 255],
            });
        }
    }
    Some(out)
}

/// The real textures of `REAL` that `resources` (the prepared game files) holds, as
/// `(layer, texels)`. A grey block texture (wool, leaves in the game are tinted by the game; the
/// ones used here are not) needs no tint.
pub fn load_real(resources: &Path) -> Vec<(usize, Texels)> {
    let dir = resources.join("assets/minecraft/textures/block");
    REAL.iter()
        .filter_map(|(mat, name)| {
            let mut texels = read_block_texture(&dir.join(format!("{name}.png")))?;
            // A grey leaf is tinted green by the game's biome colours: a fixed green here.
            if *mat == Mat::Fern {
                for px in texels.chunks_mut(4) {
                    for (c, k) in px.iter_mut().zip([0.5, 0.76, 0.32]) {
                        *c = (f32::from(*c) * k) as u8;
                    }
                }
            }
            Some((*mat as usize, texels))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inside(b: &Block, p: Vec3) -> bool {
        p.cmpgt(b.min).all() && p.cmplt(b.max).all()
    }

    #[test]
    fn the_player_at_the_origin_stands_in_free_space_and_the_eyes_too() {
        for b in blocks() {
            for y in [0.2, 0.9, 1.65] {
                assert!(!inside(&b, Vec3::new(0.0, y, 0.0)), "{b:?} at height {y}");
            }
        }
    }

    #[test]
    fn the_tv_panel_hangs_on_the_far_wall_facing_the_player_inside_its_frame() {
        let half = TV_WIDTH / 2.0;
        let height = TV_WIDTH * 9.0 / 16.0;
        assert!(TV_CENTRE.z < -2.5 && TV_CENTRE.z > INTERIOR.0[2]);
        assert_eq!(
            TV_ROT * Vec3::Z,
            Vec3::Z,
            "its front looks at the player at +Z"
        );
        // The black frame, the screen back and nothing else cover the panel's rectangle.
        let frame = blocks()
            .into_iter()
            .find(|b| b.faces.iter().all(|f| *f == Some(Mat::BlackConcrete)))
            .expect("a black concrete frame");
        assert!(frame.min.x < TV_CENTRE.x - half && frame.max.x > TV_CENTRE.x + half);
        assert!(
            frame.min.y < TV_CENTRE.y - height / 2.0 && frame.max.y > TV_CENTRE.y + height / 2.0
        );
        assert!(
            frame.max.z <= TV_CENTRE.z + 0.001,
            "the panel is in front of the frame"
        );
        // Straight ahead of a standing player's eyes within 25 degrees up or down: a TV at
        // sitting eye height that does not make the neck work.
        for eye in [1.2f32, 1.65] {
            let pitch = ((TV_CENTRE.y - eye) / (0.0 - TV_CENTRE.z))
                .atan()
                .to_degrees();
            assert!(pitch.abs() < 25.0, "{pitch}");
        }
    }

    #[test]
    fn the_room_is_six_by_five_blocks_and_three_high() {
        let (min, max) = INTERIOR;
        assert_eq!(
            [max[0] - min[0], max[1] - min[1], max[2] - min[2]],
            [6.0, 3.0, 5.0]
        );
        // Nothing but the shell (and the window's reveal in it) goes past the interior.
        for b in blocks() {
            let shell = b.min.x < -3.0
                || b.max.x > 3.0
                || b.min.z < -3.0
                || b.max.z > 2.0
                || b.min.y < 0.0
                || b.max.y > 3.0;
            let furniture_in =
                b.min.x >= -3.0 && b.max.x <= 3.0 && b.min.z >= -3.0 && b.max.z <= 2.0;
            assert!(shell || furniture_in, "{b:?}");
        }
        // Six blocks across, five deep: the shell's inner faces are where the interior ends.
        let floor = blocks()
            .into_iter()
            .find(|b| b.faces[2] == Some(Mat::OakPlanks) && b.min.y < 0.0)
            .unwrap();
        assert_eq!(
            [floor.max.x - floor.min.x, floor.max.z - floor.min.z],
            [6.0, 5.0]
        );
    }

    #[test]
    fn the_sofa_faces_the_tv_with_red_cushions_in_reach_of_the_player() {
        let reds: Vec<_> = blocks()
            .into_iter()
            .filter(|b| {
                b.faces.iter().all(|f| *f == Some(Mat::WoolRed)) && b.max.y > 0.4 && b.min.y >= 0.3
            })
            .collect();
        assert_eq!(reds.len(), 6, "three seat and three back cushions");
        assert!(
            reds.iter().all(|b| b.min.z >= 1.0),
            "south of the player, TV to the north"
        );
        // The seat is a normal one: its cushion top is 0.4 to 0.55 m up.
        assert!(reds.iter().any(|b| (b.max.y - 0.5).abs() < 0.06));
        // The back is behind the seat, further from the TV.
        let seat = reds
            .iter()
            .filter(|b| b.max.y < 0.6)
            .map(|b| b.max.z)
            .fold(0.0, f32::max);
        let back = reds
            .iter()
            .filter(|b| b.max.y > 0.6)
            .map(|b| b.max.z)
            .fold(0.0, f32::max);
        assert!(back > seat - 0.2);
    }

    #[test]
    fn the_mesh_has_the_materials_the_brief_names_and_indexes_within_its_vertices() {
        let m = mesh();
        for (v, i) in [&m.opaque, &m.glass] {
            assert!(i.iter().all(|&i| (i as usize) < v.len()));
            assert_eq!(i.len() % 3, 0);
        }
        let used: std::collections::HashSet<u32> = m.opaque.0.iter().map(|v| v.data & 63).collect();
        for mat in [
            Mat::OakPlanks,
            Mat::SprucePlanks,
            Mat::LogSide,
            Mat::DarkOak,
            Mat::WoolRed,
            Mat::WoolWhite,
            Mat::BlackConcrete,
            Mat::Bricks,
            Mat::Books,
            Mat::Lantern,
            Mat::Fire,
            Mat::Pot,
            Mat::Fern,
        ] {
            assert!(used.contains(&(mat as u32)), "{mat:?}");
        }
        assert!(m.glass.0.iter().all(|v| v.data & 63 == Mat::Glass as u32));
        // A small static mesh: well under what a frame budget cares about.
        assert!(m.opaque.0.len() < 4000, "{}", m.opaque.0.len());
    }

    #[test]
    fn the_textures_are_16_by_16_and_the_cutouts_have_holes_and_the_rest_are_solid() {
        let layers = procedural_layers();
        assert_eq!(layers.len(), LAYERS);
        assert!(layers.iter().all(|t| t.len() == 16 * 16 * 4));
        let alpha = |m: Mat| -> Vec<u8> { layers[m as usize].chunks(4).map(|p| p[3]).collect() };
        for cutout in [Mat::Fern, Mat::Fire] {
            let a = alpha(cutout);
            assert!(a.contains(&0) && a.contains(&255), "{cutout:?}");
        }
        for solid in [
            Mat::OakPlanks,
            Mat::WoolRed,
            Mat::Bricks,
            Mat::BlackConcrete,
        ] {
            assert!(alpha(solid).iter().all(|&v| v == 255), "{solid:?}");
        }
        // The wool is red where the room says red, and the planks are warmer than the rug is white.
        let px = |m: Mat| &layers[m as usize][0..3];
        assert!(px(Mat::WoolRed)[0] > 100 && px(Mat::WoolRed)[1] < 60);
        assert!(px(Mat::WoolWhite)[1] > 180);
    }

    #[test]
    fn a_palette_texture_of_4_bits_is_read_too() {
        let dir = std::env::temp_dir().join(format!("room-layout-indexed-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("assets/minecraft/textures/block")).unwrap();
        // 16 by 16, 4 bits a pixel (two to a byte), palette: 0 is orange, 1 is green; the top
        // row is index 1, the rest index 0.
        let rows: Vec<u8> = (0..16)
            .flat_map(|y| std::iter::repeat_n(if y == 0 { 0x11u8 } else { 0x00 }, 8))
            .collect();
        let path = dir.join("assets/minecraft/textures/block/dirt.png");
        let mut enc = png::Encoder::new(std::fs::File::create(&path).unwrap(), 16, 16);
        enc.set_color(png::ColorType::Indexed);
        enc.set_depth(png::BitDepth::Four);
        enc.set_palette(vec![230, 120, 20, 10, 200, 10]);
        enc.write_header().unwrap().write_image_data(&rows).unwrap();
        let real = load_real(&dir);
        assert_eq!(real.len(), 1, "the palette image was not read");
        assert_eq!(&real[0].1[0..4], &[10, 200, 10, 255], "top row green");
        assert_eq!(
            &real[0].1[16 * 4..16 * 4 + 4],
            &[230, 120, 20, 255],
            "second row orange"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_real_texture_is_cropped_to_its_top_left_16_by_16() {
        let dir = std::env::temp_dir().join(format!("room-layout-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("assets/minecraft/textures/block")).unwrap();
        // A 16 by 48 strip (an animated texture): the first frame is red, then blue.
        let mut pixels = Vec::new();
        for y in 0..48 {
            for _ in 0..16 {
                pixels.extend(if y < 16 {
                    [200u8, 10, 10, 255]
                } else {
                    [10, 10, 200, 255]
                });
            }
        }
        let path = dir.join("assets/minecraft/textures/block/campfire_fire.png");
        let mut enc = png::Encoder::new(std::fs::File::create(&path).unwrap(), 16, 48);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header()
            .unwrap()
            .write_image_data(&pixels)
            .unwrap();
        let real = load_real(&dir);
        assert_eq!(real.len(), 1);
        assert_eq!(real[0].0, Mat::Fire as usize);
        assert_eq!(&real[0].1[0..4], &[200, 10, 10, 255]);
        assert_eq!(real[0].1.len(), 16 * 16 * 4);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
