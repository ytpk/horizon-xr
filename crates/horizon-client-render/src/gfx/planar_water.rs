//! Exact fast-path proof for disjoint horizontal terrain quads. Opposite
//! windings are checked separately because back-face culling separates them.
use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap};

pub struct Plane {
    pub y: f32,
    pub up: Vec<bool>,
}

fn key(v: f32) -> u32 {
    let bits = if v == 0.0 { 0 } else { v.to_bits() };
    if bits >> 31 == 0 {
        bits ^ (1 << 31)
    } else {
        !bits
    }
}

pub fn classify(vertices: &[[f32; 3]]) -> Option<Plane> {
    let y = vertices.first()?[1];
    if !vertices.len().is_multiple_of(4) {
        return None;
    }
    let mut rectangles = [Vec::new(), Vec::new()];
    let mut up = Vec::with_capacity(vertices.len() / 4);
    for q in vertices.as_chunks::<4>().0 {
        if q.iter()
            .any(|p| !p.iter().all(|v| v.is_finite()) || p[1] != y)
        {
            return None;
        }
        let min_x = q.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min);
        let max_x = q.iter().map(|p| p[0]).fold(f32::NEG_INFINITY, f32::max);
        let min_z = q.iter().map(|p| p[2]).fold(f32::INFINITY, f32::min);
        let max_z = q.iter().map(|p| p[2]).fold(f32::NEG_INFINITY, f32::max);
        if min_x >= max_x || min_z >= max_z {
            return None;
        }
        let mut corners = 0u8;
        for p in q {
            let x = if p[0] == min_x {
                0
            } else if p[0] == max_x {
                1
            } else {
                return None;
            };
            let z = if p[2] == min_z {
                0
            } else if p[2] == max_z {
                1
            } else {
                return None;
            };
            corners |= 1 << (x * 2 + z);
        }
        if corners != 15 {
            return None;
        }
        let normal_y = |a: [f32; 3], b: [f32; 3], c: [f32; 3]| {
            (b[2] - a[2]) * (c[0] - a[0]) - (b[0] - a[0]) * (c[2] - a[2])
        };
        let n = normal_y(q[0], q[1], q[2]);
        let n2 = normal_y(q[2], q[3], q[0]);
        if !n.is_finite() || !n2.is_finite() || n == 0.0 || n2 == 0.0 || (n > 0.0) != (n2 > 0.0) {
            return None;
        }
        let facing = n > 0.0;
        up.push(facing);
        rectangles[usize::from(facing)].push((key(min_x), key(max_x), key(min_z), key(max_z)));
    }
    for rects in &mut rectangles {
        rects.sort_unstable();
        let mut active = BTreeMap::<u32, u32>::new();
        let mut expiry = BinaryHeap::new();
        for &(x0, x1, z0, z1) in rects.iter() {
            while let Some(&Reverse((end_x, start_z))) = expiry.peek() {
                if end_x > x0 {
                    break;
                }
                expiry.pop();
                active.remove(&start_z);
            }
            if active
                .range(..=z0)
                .next_back()
                .is_some_and(|(_, &end)| end > z0)
                || active
                    .range(z0..)
                    .next()
                    .is_some_and(|(&start, _)| start < z1)
            {
                return None;
            }
            active.insert(z0, z1);
            expiry.push(Reverse((x1, z0)));
        }
    }
    Some(Plane { y, up })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn quad(x: f32, z: f32) -> [[f32; 3]; 4] {
        [
            [x, 1.0, z],
            [x, 1.0, z + 1.0],
            [x + 1.0, 1.0, z + 1.0],
            [x + 1.0, 1.0, z],
        ]
    }
    #[test]
    fn adjacent_quads_and_their_back_faces_are_safe_but_overlaps_are_not() {
        let mut v = Vec::new();
        for x in -16..16 {
            for z in -16..16 {
                let q = quad(x as f32, z as f32);
                v.extend(q);
                v.extend(q.into_iter().rev());
            }
        }
        let p = classify(&v).unwrap();
        assert_eq!(p.up.iter().filter(|&&u| u).count(), 1024);
        v.extend(quad(0.25, 0.25));
        assert!(classify(&v).is_none());
    }
    #[test]
    fn sweep_agrees_with_pairwise_overlap_oracle() {
        let mut seed = 4723u32;
        let mut cases = [false; 2];
        for _ in 0..300 {
            let mut rectangles = Vec::new();
            let mut vertices = Vec::new();
            for _ in 0..16 {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                let x = ((seed >> 16) % 64) as f32 * 0.25 - 8.0;
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                let z = ((seed >> 16) % 64) as f32 * 0.25 - 8.0;
                rectangles.push([x, z]);
                vertices.extend(quad(x, z));
            }
            let overlap = rectangles.iter().enumerate().any(|(i, a)| {
                rectangles[..i].iter().any(|b| {
                    a[0] < b[0] + 1.0 && b[0] < a[0] + 1.0 && a[1] < b[1] + 1.0 && b[1] < a[1] + 1.0
                })
            });
            cases[usize::from(overlap)] = true;
            assert_eq!(classify(&vertices).is_some(), !overlap);
        }
        assert_eq!(cases, [true, true]);
    }

    #[test]
    fn sloping_layered_bow_tie_and_invalid_geometry_use_the_regular_sort() {
        let q = quad(0.0, 0.0);
        let mut slope = q;
        slope[0][1] += 0.001;
        assert!(classify(&slope).is_none());
        let mut layered = q.to_vec();
        layered.extend(q.map(|mut p| {
            p[1] += 1.0;
            p
        }));
        assert!(classify(&layered).is_none());
        assert!(classify(&[q[0], q[2], q[1], q[3]]).is_none());
        let mut invalid = q;
        invalid[0][0] = f32::NAN;
        assert!(classify(&invalid).is_none());
    }
}
