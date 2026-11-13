//! Optional visual thinning before lighting and vertex emission. World data is untouched.
use std::sync::atomic::{AtomicU32, Ordering};

static DENSITY: AtomicU32 = AtomicU32::new(100);

/// Percentage of crossed plant models to draw. A renderer sets this before workers mesh.
pub fn plant_density(percent: u32) {
    DENSITY.store(percent.min(100), Ordering::Relaxed);
}

/// One choice per horizontal world cell: both halves of a tall plant and every face of its
/// selected model stay together. It is independent of worker order, camera and remeshing.
pub(super) fn keep(x: i32, z: i32, percent: u32) -> bool {
    if percent >= 100 {
        return true;
    }
    if percent == 0 {
        return false;
    }
    let mut h = (x as u32).wrapping_mul(0x9e37_79b9) ^ (z as u32).wrapping_mul(0x85eb_ca6b);
    h ^= h >> 16;
    h = h.wrapping_mul(0x7feb_352d);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846c_a68b);
    h ^= h >> 16;
    // Multiply-high instead of modulo: a uniform fraction of all u32 hashes.
    ((u64::from(h) * 100) >> 32) < u64::from(percent)
}

pub(super) fn density() -> u32 {
    DENSITY.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thinning_is_nested_and_distributed_across_negative_and_positive_cells() {
        let mut counts = [0; 5];
        for x in -128..128 {
            for z in -128..128 {
                let decisions = [0, 25, 50, 75, 100].map(|p| keep(x, z, p));
                assert!(!decisions[0] && decisions[4]);
                assert!(decisions.windows(2).all(|v| !v[0] || v[1]));
                for (count, yes) in counts.iter_mut().zip(decisions) {
                    *count += usize::from(yes);
                }
            }
        }
        for (count, percent) in counts.into_iter().zip([0, 25, 50, 75, 100]) {
            assert!((count as f64 / 65536.0 * 100.0 - f64::from(percent)).abs() < 1.0);
        }
    }
}
