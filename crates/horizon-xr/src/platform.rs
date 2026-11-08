//! The display rate and the performance levels Horizon asks the headset's runtime for, as
//! pure decisions (the OpenXR calls are in `lib.rs`): which of the offered rates is nearest the
//! wanted one, whether 120 Hz is among them, and which level `HORIZON_XR_PERF` names.
use openxr as xr;

/// The offered rate nearest `wanted`, the lower one when two are as near.
pub fn nearest_rate(rates: &[f32], wanted: f32) -> Option<f32> {
    rates.iter().copied().min_by(|a, b| {
        (a - wanted)
            .abs()
            .total_cmp(&(b - wanted).abs())
            .then(a.total_cmp(b))
    })
}

/// Whether `hz` (within half a hertz) is offered.
pub fn offers(rates: &[f32], hz: f32) -> bool {
    rates.iter().any(|r| (r - hz).abs() < 0.5)
}

/// The log line that tells a tester what the headset can do and what Horizon chose.
pub fn rate_report(rates: &[f32], current: Option<f32>, wanted: f32) -> String {
    let chosen = nearest_rate(rates, wanted);
    format!(
        "refresh rates {rates:?}, now {current:?}, 120 Hz {}, wanted {wanted} -> {chosen:?}",
        if offers(rates, 120.0) {
            "offered"
        } else {
            "NOT offered"
        }
    )
}

/// `HORIZON_XR_PERF`: `low`, `sustained_low`, `sustained_high` (the default) or `boost`.
pub fn perf_level(text: Option<&str>) -> xr::sys::PerfSettingsLevelEXT {
    use xr::sys::PerfSettingsLevelEXT as L;
    match text.map(|t| t.trim().to_ascii_lowercase()).as_deref() {
        Some("low" | "power_savings") => L::POWER_SAVINGS,
        Some("sustained_low") => L::SUSTAINED_LOW,
        Some("boost") => L::BOOST,
        _ => L::SUSTAINED_HIGH,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_nearest_offered_rate_wins_and_ties_go_low() {
        let quest = [72.0, 80.0, 90.0, 120.0];
        assert_eq!(nearest_rate(&quest, 120.0), Some(120.0));
        assert_eq!(nearest_rate(&quest, 100.0), Some(90.0));
        // 105 is as near 90 as 120: the lower rate is the one a headset can hold.
        assert_eq!(nearest_rate(&quest, 105.0), Some(90.0));
        assert_eq!(nearest_rate(&[], 90.0), None);
    }

    #[test]
    fn the_report_says_whether_120_hz_is_there() {
        assert!(rate_report(&[72.0, 90.0], Some(72.0), 120.0).contains("120 Hz NOT offered"));
        let report = rate_report(&[72.0, 90.0, 120.0], Some(90.0), 120.0);
        assert!(report.contains("120 Hz offered"), "{report}");
        assert!(report.ends_with("-> Some(120.0)"), "{report}");
    }

    #[test]
    fn the_performance_level_defaults_to_sustained_high() {
        use xr::sys::PerfSettingsLevelEXT as L;
        assert_eq!(perf_level(None), L::SUSTAINED_HIGH);
        assert_eq!(perf_level(Some("BOOST")), L::BOOST);
        assert_eq!(perf_level(Some("sustained_low")), L::SUSTAINED_LOW);
        assert_eq!(perf_level(Some("nonsense")), L::SUSTAINED_HIGH);
    }
}
