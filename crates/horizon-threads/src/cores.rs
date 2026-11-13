//! Keeping the render and simulation threads on a phone-class chip's big cores.
//!
//! A headset's chip (Snapdragon XR2) mixes fast and slow cores, and the scheduler may leave the
//! thread that draws the frame, or the one that ticks the world, on a slow core for a stretch: a
//! frame then comes late for no reason the game can see. Asked for (`HORIZON_PIN_BIG_CORES` or
//! the headset setting `horizonPinBigCores`), the calling thread is restricted to the cores that
//! are not in the chip's slowest tier. Chips whose cores are all alike are left alone.
//!
//! The speed of a core is its architectural `cpu_capacity`, or else its highest frequency
//! (`cpuinfo_max_freq`), on one consistent scale read from sysfs. These may be hidden, in which case nothing is
//! pinned and the caller is told why.
mod hybrid;
pub use hybrid::{hybrid_plan, pin_background, pin_worker};

/// What was asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Wanted {
    Off,
    /// The cores outside the slowest tier.
    Auto,
    /// These cores (`3-6,7`), whatever their speed.
    List(Vec<usize>),
}

impl Wanted {
    /// `0`/`off`/empty, `1`/`on`/`auto`, or a kernel CPU list such as `3-6,7`.
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "" | "0" | "off" | "false" => Some(Self::Off),
            "1" | "on" | "auto" | "true" => Some(Self::Auto),
            list => parse_cpu_list(list).map(Self::List),
        }
    }
}

/// A kernel CPU list: comma separated cores and `a-b` ranges (`0-2,5`), sorted and unique.
/// `None` for anything else (including a reversed range).
pub fn parse_cpu_list(text: &str) -> Option<Vec<usize>> {
    let mut cores = Vec::new();
    for part in text.split(',') {
        let part = part.trim();
        match part.split_once('-') {
            Some((from, to)) => {
                let (from, to): (usize, usize) =
                    (from.trim().parse().ok()?, to.trim().parse().ok()?);
                if from > to || to > 1023 {
                    return None;
                }
                cores.extend(from..=to);
            }
            None => cores.push(part.parse().ok().filter(|&c| c <= 1023)?),
        }
    }
    cores.sort_unstable();
    cores.dedup();
    (!cores.is_empty()).then_some(cores)
}

/// The cores outside the slowest tier of `capacities` (core, capacity), ascending. Empty when
/// every core has the same capacity (nothing to choose between).
pub fn big_cores(capacities: &[(usize, u64)]) -> Vec<usize> {
    let Some(slowest) = capacities.iter().map(|&(_, c)| c).min() else {
        return Vec::new();
    };
    let mut big: Vec<usize> = capacities
        .iter()
        .filter(|&&(_, c)| c > slowest)
        .map(|&(core, _)| core)
        .collect();
    big.sort_unstable();
    big
}

/// The cores a pinned thread may run on, among those the process is allowed.
pub fn restrict(cores: &[usize], allowed: &[usize]) -> Vec<usize> {
    cores
        .iter()
        .copied()
        .filter(|c| allowed.contains(c))
        .collect()
}

/// What pinning did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pinned {
    pub cores: Vec<usize>,
    /// The cores the process may use at all (`sched_getaffinity`).
    pub allowed: Vec<usize>,
    pub capacities: Vec<(usize, u64)>,
}

/// Restricts the calling thread as `wanted` says. `Ok(None)` when nothing was asked for or the
/// chip has one kind of core; an error says why the request could not be carried out.
#[cfg(any(target_os = "linux", target_os = "android"))]
pub fn pin_current(wanted: &Wanted) -> Result<Option<Pinned>, String> {
    let capacities = read_capacities();
    let allowed = allowed_cores()?;
    // Threads spawned after this call inherit its restricted mask. Preserve the original
    // allowance before pinning so opt-in background workers can still reach E cores.
    hybrid::remember_allowed(&allowed);
    let cores = match wanted {
        Wanted::Off => return Ok(None),
        Wanted::Auto => {
            if capacities.is_empty() {
                return Err("the kernel does not tell the cores' speed (no cpu_capacity or cpuinfo_max_freq)".into());
            }
            hybrid_plan().map_or_else(
                || restrict(&big_cores(&capacities), &allowed),
                |plan| restrict(&plan.foreground, &allowed),
            )
        }
        Wanted::List(list) => restrict(list, &allowed),
    };
    if cores == allowed {
        return Ok(None);
    }
    if cores.is_empty() {
        return match wanted {
            Wanted::List(_) => Err(format!(
                "none of the listed cores is allowed to this process (it may use {allowed:?})"
            )),
            _ => Ok(None),
        };
    }
    set_affinity(&cores)?;
    Ok(Some(Pinned {
        cores,
        allowed,
        capacities,
    }))
}

#[cfg(not(any(target_os = "linux", target_os = "android")))]
pub fn pin_current(_wanted: &Wanted) -> Result<Option<Pinned>, String> {
    Ok(None)
}

/// Architectural capacity from sysfs, else maximum frequency; one scale for every core.
#[cfg(any(target_os = "linux", target_os = "android"))]
pub fn read_capacities() -> Vec<(usize, u64)> {
    let mut capacities = Vec::new();
    let mut frequencies = Vec::new();
    let mut count = 0;
    for core in 0..1024 {
        let base = format!("/sys/devices/system/cpu/cpu{core}");
        if !std::path::Path::new(&base).exists() {
            break;
        }
        count += 1;
        let read = |name: &str| {
            std::fs::read_to_string(format!("{base}/{name}"))
                .ok()
                .and_then(|text| text.trim().parse::<u64>().ok())
                .filter(|&value| value > 0)
        };
        if let Some(value) = read("cpu_capacity") {
            capacities.push((core, value));
        }
        if let Some(value) = read("cpufreq/cpuinfo_max_freq") {
            frequencies.push((core, value));
        }
    }
    // Architectural capacity distinguishes P/E cores more reliably than clock alone.
    // Never compare capacity units on one CPU with kHz on another.
    if capacities.len() == count {
        capacities
    } else if frequencies.len() == count {
        frequencies
    } else {
        Vec::new()
    }
}

/// The cores the process may run on now (`sched_getaffinity`).
#[cfg(any(target_os = "linux", target_os = "android"))]
pub(super) fn allowed_cores() -> Result<Vec<usize>, String> {
    // SAFETY: a zeroed cpu_set_t is the empty set; the call fills it for the calling thread.
    let set = unsafe {
        let mut set: libc::cpu_set_t = std::mem::zeroed();
        if libc::sched_getaffinity(0, std::mem::size_of::<libc::cpu_set_t>(), &mut set) != 0 {
            return Err(format!(
                "sched_getaffinity: {}",
                std::io::Error::last_os_error()
            ));
        }
        set
    };
    // SAFETY: CPU_ISSET only reads the set at an index inside it.
    Ok((0..libc::CPU_SETSIZE as usize)
        .filter(|&c| unsafe { libc::CPU_ISSET(c, &set) })
        .collect())
}

#[cfg(any(target_os = "linux", target_os = "android"))]
pub(super) fn set_affinity(cores: &[usize]) -> Result<(), String> {
    // SAFETY: builds a set of small core numbers and hands it to the kernel for this thread (pid 0).
    unsafe {
        let mut set: libc::cpu_set_t = std::mem::zeroed();
        for &core in cores {
            if core >= libc::CPU_SETSIZE as usize {
                return Err("CPU index exceeds cpu_set_t".into());
            }
            libc::CPU_SET(core, &mut set);
        }
        if libc::sched_setaffinity(0, std::mem::size_of::<libc::cpu_set_t>(), &set) != 0 {
            return Err(format!(
                "sched_setaffinity: {}",
                std::io::Error::last_os_error()
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_lists_follow_the_kernels_format() {
        assert_eq!(parse_cpu_list("0-2,5"), Some(vec![0, 1, 2, 5]));
        assert_eq!(parse_cpu_list(" 7 , 3-4 ,4"), Some(vec![3, 4, 7]));
        assert_eq!(parse_cpu_list("4-2"), None);
        assert_eq!(parse_cpu_list("a"), None);
        assert_eq!(parse_cpu_list(""), None);
    }

    #[test]
    fn the_request_is_a_switch_or_a_list() {
        assert_eq!(Wanted::parse("0"), Some(Wanted::Off));
        assert_eq!(Wanted::parse("AUTO"), Some(Wanted::Auto));
        assert_eq!(Wanted::parse("3-4"), Some(Wanted::List(vec![3, 4])));
        assert_eq!(Wanted::parse("fast"), None);
    }

    #[test]
    fn the_slowest_tier_is_left_out() {
        // A 1 + 4 + 3 chip as the Snapdragon 8 Gen 2 reports it: little 0-2 (slow), mid 3-6, prime 7.
        let caps: Vec<(usize, u64)> = (0..8)
            .map(|c| {
                (
                    c,
                    if c < 3 {
                        280
                    } else if c < 7 {
                        760
                    } else {
                        1024
                    },
                )
            })
            .collect();
        assert_eq!(big_cores(&caps), vec![3, 4, 5, 6, 7]);
    }

    #[test]
    fn a_chip_of_alike_cores_is_not_pinned() {
        let caps: Vec<(usize, u64)> = (0..6).map(|c| (c, 1024)).collect();
        assert!(big_cores(&caps).is_empty());
        assert!(big_cores(&[]).is_empty());
    }

    #[test]
    fn pinning_keeps_only_cores_the_process_may_use() {
        assert_eq!(restrict(&[3, 4, 7], &[0, 1, 2, 3, 4, 5, 6]), vec![3, 4]);
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    #[test]
    fn a_listed_core_the_process_has_is_pinned_to_it() {
        // Run on a thread of its own: the affinity is the thread's.
        let result = std::thread::spawn(|| {
            let allowed = allowed_cores().unwrap();
            let first = allowed[0];
            if allowed.len() < 2 {
                return None;
            }
            let pinned = pin_current(&Wanted::List(vec![first])).unwrap().unwrap();
            Some((pinned.cores, allowed_cores().unwrap(), first))
        })
        .join()
        .unwrap();
        if let Some((cores, now, first)) = result {
            assert_eq!(cores, vec![first]);
            assert_eq!(now, vec![first]);
        }
    }
}
