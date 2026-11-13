//! An opt-in worker partition for heterogeneous Android/Linux CPUs. No fixed CPU IDs.
use std::sync::OnceLock;

#[cfg(any(target_os = "linux", target_os = "android"))]
static INITIAL: OnceLock<Vec<usize>> = OnceLock::new();
static PLAN: OnceLock<Option<HybridPlan>> = OnceLock::new();

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HybridPlan {
    /// Two fastest available P cores, shared by render, simulation and submit.
    pub foreground: Vec<usize>,
    /// Remaining P cores first, then E cores, one background worker per core.
    pub workers: Vec<usize>,
    /// Async visibility searches and texture compression may use any E core.
    pub efficiency: Vec<usize>,
}

#[cfg(any(target_os = "linux", target_os = "android"))]
pub(super) fn remember_allowed(allowed: &[usize]) {
    let _ = INITIAL.set(allowed.to_vec());
}

#[cfg(any(target_os = "linux", target_os = "android", test))]
fn plan(capacities: &[(usize, u64)], allowed: &[usize]) -> Option<HybridPlan> {
    // A partial or mixed-scale topology is not sufficient evidence for pinning.
    let mut ranked: Vec<_> = allowed
        .iter()
        .map(|&cpu| {
            capacities
                .iter()
                .find(|&&(c, capacity)| c == cpu && capacity > 0)
                .copied()
        })
        .collect::<Option<_>>()?;
    ranked.sort_unstable_by_key(|&(cpu, capacity)| (std::cmp::Reverse(capacity), cpu));
    let slowest = ranked.last()?.1;
    let big = ranked
        .iter()
        .take_while(|&&(_, capacity)| capacity > slowest)
        .count();
    // Keep a fast worker for urgent jobs as well as two foreground cores.
    if big < 3 || big == ranked.len() {
        return None;
    }
    Some(HybridPlan {
        foreground: ranked[..2].iter().map(|&(cpu, _)| cpu).collect(),
        workers: ranked[2..].iter().map(|&(cpu, _)| cpu).collect(),
        efficiency: ranked[big..].iter().map(|&(cpu, _)| cpu).collect(),
    })
}

/// `HORIZON_HYBRID_WORKERS=1`: capture topology once, before the foreground is pinned.
/// Hidden/partial sysfs, homogeneous CPUs and unsupported platforms retain normal scheduling.
pub fn hybrid_plan() -> Option<&'static HybridPlan> {
    PLAN.get_or_init(|| {
        if std::env::var("HORIZON_HYBRID_WORKERS").as_deref() != Ok("1") {
            return None;
        }
        #[cfg(any(target_os = "linux", target_os = "android"))]
        {
            let allowed = INITIAL.get_or_init(|| super::allowed_cores().unwrap_or_default());
            plan(&super::read_capacities(), allowed)
        }
        #[cfg(not(any(target_os = "linux", target_os = "android")))]
        {
            None
        }
    })
    .as_ref()
}

/// Called once per shared-pool worker. Restores access lost through inherited affinity.
/// Failure leaves the inherited mask intact; no frame ever waits for this operation.
pub fn pin_worker(worker: usize) -> Result<Option<usize>, String> {
    let Some(plan) = hybrid_plan() else {
        return Ok(None);
    };
    pin_to_worker(plan, worker).map(Some)
}

fn pin_to_worker(plan: &HybridPlan, worker: usize) -> Result<usize, String> {
    let cpu = plan.workers[worker % plan.workers.len()];
    #[cfg(any(target_os = "linux", target_os = "android"))]
    super::set_affinity(&[cpu])?;
    Ok(cpu)
}

/// Optional affinity for async work whose result the frame does not wait for.
pub fn pin_background() -> Result<(), String> {
    let Some(plan) = hybrid_plan() else {
        return Ok(());
    };
    #[cfg(any(target_os = "linux", target_os = "android"))]
    super::set_affinity(&plan.efficiency)?;
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    let _ = plan;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hybrid_workers_use_both_tiers_without_the_two_foreground_cores() {
        let capacities = [
            (0, 280),
            (1, 280),
            (2, 280),
            (3, 280),
            (4, 760),
            (5, 760),
            (6, 760),
            (7, 1024),
        ];
        let p = plan(&capacities, &[0, 1, 2, 3, 4, 5, 6, 7]).unwrap();
        assert_eq!(p.foreground, [7, 4]);
        assert_eq!(p.workers, [5, 6, 0, 1, 2, 3]);
        assert!(p.workers.iter().all(|cpu| !p.foreground.contains(cpu)));
        // A Quest process need not be allowed every physical core.
        let p = plan(&capacities, &[1, 4, 5, 6]).unwrap();
        assert_eq!(p.foreground, [4, 5]);
        assert_eq!(p.workers, [6, 1]);
    }

    #[test]
    fn unknown_topology_and_insufficient_fast_cores_keep_os_scheduling() {
        assert!(plan(&[], &[0, 1]).is_none());
        assert!(plan(&[(0, 1024), (1, 1024)], &[0, 1]).is_none());
        assert!(plan(&[(0, 200), (1, 800), (2, 800)], &[0, 1, 2]).is_none());
        assert!(plan(&[(0, 200), (1, 800), (2, 800), (3, 800)], &[0, 1, 2, 3, 4]).is_none());
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    #[test]
    fn a_worker_can_restore_a_core_excluded_by_its_parents_affinity() {
        std::thread::spawn(|| {
            let allowed = super::super::allowed_cores().unwrap();
            if allowed.len() < 2 {
                return;
            }
            let foreground = allowed[0];
            let background = allowed[1];
            super::super::set_affinity(&[foreground]).unwrap();
            std::thread::spawn(move || {
                assert_eq!(super::super::allowed_cores().unwrap(), [foreground]);
                let plan = HybridPlan {
                    foreground: vec![foreground],
                    workers: vec![background],
                    efficiency: vec![background],
                };
                assert_eq!(pin_to_worker(&plan, 0).unwrap(), background);
                assert_eq!(super::super::allowed_cores().unwrap(), [background]);
            })
            .join()
            .unwrap();
        })
        .join()
        .unwrap();
    }
}
