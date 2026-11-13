//! Render cadence: low values explicitly request Application SpaceWarp at twice the rate.
pub const RATES: [f32; 13] = [
    30., 36., 40., 45., 48., 50., 60., 72., 80., 90., 96., 100., 120.,
];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mode {
    pub render: f32,
    pub display: f32,
    pub spacewarp: bool,
}

pub fn mode(value: f32) -> Mode {
    // Preserve old native slider values (e.g. 108): OpenXR must choose its nearest
    // advertised rate from the original request, rather than first snapping to 100
    // and inadvertently changing a former 120 Hz request into 90 Hz.
    if value.is_finite() && value >= 72. {
        return Mode {
            render: value,
            display: value,
            spacewarp: false,
        };
    }
    let render = RATES
        .into_iter()
        .min_by(|a, b| (a - value).abs().total_cmp(&(b - value).abs()))
        .filter(|_| value.is_finite())
        .unwrap_or(90.);
    let spacewarp = render < 72.;
    Mode {
        render,
        display: if spacewarp { render * 2. } else { render },
        spacewarp,
    }
}

// A launch profile sets the initial cadence. An explicit UI edit takes precedence
// for this process, so the existing slider remains usable with those profiles.
static INTERACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn edited_in_ui() {
    INTERACTIVE.store(true, std::sync::atomic::Ordering::Relaxed);
}

pub fn requested(saved: f32) -> Mode {
    select(
        saved,
        std::env::var("HORIZON_XR_HZ").ok().as_deref(),
        INTERACTIVE.load(std::sync::atomic::Ordering::Relaxed),
    )
}

fn select(saved: f32, launch: Option<&str>, interactive: bool) -> Mode {
    let launch = launch
        .and_then(|s| s.parse::<f32>().ok())
        .filter(|v| v.is_finite());
    mode(if interactive {
        saved
    } else {
        launch.unwrap_or(saved)
    })
}

pub fn supported(rates: &[f32], spacewarp: bool) -> Vec<f32> {
    RATES
        .into_iter()
        .filter(|&rate| {
            let m = mode(rate);
            (!m.spacewarp || spacewarp)
                && rates
                    .iter()
                    .any(|r| r.is_finite() && (*r - m.display).abs() < 0.1)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn half_rates_are_distinct_from_native_and_respect_runtime_support() {
        for (render, display) in [
            (30., 60.),
            (36., 72.),
            (40., 80.),
            (45., 90.),
            (48., 96.),
            (50., 100.),
            (60., 120.),
        ] {
            assert_eq!(
                mode(render),
                Mode {
                    render,
                    display,
                    spacewarp: true
                }
            );
        }
        assert_eq!(
            supported(&[72., 90., 120.], true),
            [36., 45., 60., 72., 90., 120.]
        );
        assert_eq!(supported(&[72., 90.], false), [72., 90.]);
        assert_eq!(mode(f32::NAN).render, 90.);
        assert_eq!(
            mode(108.),
            Mode {
                render: 108.,
                display: 108.,
                spacewarp: false
            }
        );
        let mut legacy = crate::settings::VrSettings::default();
        legacy.migrate_legacy("refreshRate:108\n");
        assert_eq!(legacy.float("horizonRefreshRate"), 108.);
    }
    #[test]
    fn the_existing_slider_roundtrips_all_modes_and_keeps_native_defaults() {
        let option = crate::options::OPTS
            .iter()
            .find(|o| o.key == "horizonRefreshRate")
            .unwrap();
        let mut settings = crate::settings::VrSettings::default();
        assert_eq!(settings.float(option.key), 90.);
        for (i, expected) in RATES.iter().enumerate() {
            crate::options::set_slider(&mut settings, option, i as f32 / 12., None);
            assert_eq!(settings.float(option.key), *expected);
            let (position, steps) = crate::options::slider(&settings, option).unwrap();
            assert!((position - i as f32 / 12.).abs() < 0.0001);
            assert_eq!(steps, 12);
            let mut loaded = crate::settings::VrSettings::default();
            loaded.migrate_legacy(&format!("refreshRate:{expected}\n"));
            assert_eq!(loaded.float(option.key), *expected);
        }
    }
    #[test]
    fn launch_cadence_is_visible_but_explicit_slider_edits_take_over() {
        assert_eq!(select(90., Some("36"), false).render, 36.);
        assert_eq!(select(45., Some("36"), true).render, 45.);
        assert!(!select(90., Some("36"), true).spacewarp);
        assert_eq!(select(120., Some("NaN"), false).render, 120.);
        assert_eq!(select(90., None, false).render, 90.);
    }
}
