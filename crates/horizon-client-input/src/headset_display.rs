//! Display capabilities and quality settings shared with the Quest startup room.
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DisplayInfo {
    pub recommended: [u32; 2],
    pub maximum: [u32; 2],
    pub active: [u32; 2],
    pub rates: Vec<f32>,
    pub current_hz: Option<f32>,
    pub spacewarp_supported: bool,
    pub spacewarp_active: bool,
    pub scale_override: Option<f32>,
    pub foveated: bool,
    /// Device can attempt the native array path; runtime creation may still refuse it.
    pub foveated_stereo_capable: bool,
    pub super_resolution_supported: bool,
}

static INFO: Mutex<Option<Arc<DisplayInfo>>> = Mutex::new(None);

pub fn snapshot() -> Option<Arc<DisplayInfo>> {
    INFO.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

pub fn update(f: impl FnOnce(&mut DisplayInfo)) {
    let mut info = INFO.lock().unwrap_or_else(|e| e.into_inner());
    f(Arc::make_mut(
        info.get_or_insert_with(|| Arc::new(DisplayInfo::default())),
    ));
}

/// Scale each dimension, retaining the current port's linear scale convention.
pub fn eye_size(recommended: [u32; 2], maximum: [u32; 2], scale: f32) -> [u32; 2] {
    let scale = horizon_vivecraft::settings::render_scale(scale);
    std::array::from_fn(|i| ((recommended[i] as f32 * scale) as u32).clamp(64, maximum[i].max(64)))
}

/// OpenXR has already applied the eye scale; do not multiply it by the window scale.
pub fn post_render_scale(headset: bool, desktop_scale: f32) -> f32 {
    if headset {
        1.0
    } else {
        desktop_scale.clamp(0.25, 2.0)
    }
}

/// Wait for a resolution drag to settle before allocating new XR swapchains.
#[derive(Default)]
pub struct ScaleChange {
    pending: Option<(f32, std::time::Instant)>,
}
impl ScaleChange {
    pub fn ready(&mut self, desired: f32, active: f32, now: std::time::Instant) -> bool {
        if desired == active {
            self.pending = None;
            return true;
        }
        match self.pending {
            Some((value, at)) if value == desired => {
                now.saturating_duration_since(at) >= std::time::Duration::from_millis(150)
            }
            _ => {
                self.pending = Some((desired, now));
                false
            }
        }
    }
}

/// Explicit quality choices; do not change the saved value until the player presses a button.
pub fn step_scale(current: f32, direction: i8) -> f32 {
    const SCALES: [f32; 13] = [
        0.25, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1.0, 1.2, 1.4, 1.6, 1.8, 2.0,
    ];
    if direction > 0 {
        SCALES
            .into_iter()
            .find(|v| *v > current + 0.001)
            .unwrap_or(current)
    } else {
        SCALES
            .into_iter()
            .rev()
            .find(|v| *v < current - 0.001)
            .unwrap_or(current)
    }
}

/// Actual render cadences, including half-rate SpaceWarp when available.
pub fn render_rates(info: &DisplayInfo) -> Vec<f32> {
    horizon_vivecraft::refresh::supported(&info.rates, info.spacewarp_supported)
}

/// Pick the next actual runtime rate, never an intermediate unsupported slider value.
pub fn step_rate(current: f32, direction: i8, rates: &[f32]) -> f32 {
    let eligible = |v: &&f32| {
        v.is_finite()
            && **v > 0.0
            && if direction > 0 {
                **v > current + 0.1
            } else {
                **v < current - 0.1
            }
    };
    let values = rates.iter().filter(eligible).copied();
    if direction > 0 {
        values.min_by(f32::total_cmp).unwrap_or(current)
    } else {
        values.max_by(f32::total_cmp).unwrap_or(current)
    }
}

static GPU_FRAME: Mutex<Option<(std::time::Instant, f32)>> = Mutex::new(None);

/// Discard samples after a resolution or stereo-path change.
pub fn clear_gpu_frame() {
    *GPU_FRAME.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// An estimate for the complete stereo world render, not one eye or XR wait.
pub fn record_gpu_frame(milliseconds: f32) {
    if milliseconds.is_finite() && milliseconds > 0.0 {
        *GPU_FRAME.lock().unwrap_or_else(|e| e.into_inner()) =
            Some((std::time::Instant::now(), milliseconds));
    }
}

pub fn recent_gpu_seconds() -> Option<f32> {
    GPU_FRAME
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .filter(|(at, _)| at.elapsed() < std::time::Duration::from_millis(500))
        .map(|(_, ms)| ms * 0.001)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn slider_transients_do_not_resize_and_window_supersampling_is_not_applied_twice() {
        use std::time::{Duration, Instant};
        let now = Instant::now();
        let mut change = ScaleChange::default();
        assert!(!change.ready(2.0, 1.0, now));
        assert!(!change.ready(2.0, 1.0, now + Duration::from_millis(100)));
        // Drag returned to its saved value: no stale request for the maximum.
        assert!(change.ready(1.0, 1.0, now + Duration::from_millis(110)));
        assert!(!change.ready(1.2, 1.0, now + Duration::from_millis(120)));
        assert!(change.ready(1.2, 1.0, now + Duration::from_millis(270)));
        for factor in [0.5, 1.0, 2.0, 9.0] {
            assert_eq!(post_render_scale(true, factor), 1.0);
        }
        assert_eq!(post_render_scale(false, 1.5), 1.5);
        assert_eq!(eye_size([2000, 2200], [8192, 8192], 9.0), [4000, 4400]);
    }

    #[test]
    fn explicit_quality_steps_and_runtime_sizes_do_not_silently_reduce_quality() {
        assert_eq!(step_scale(0.4, 1), 0.5);
        assert_eq!(step_scale(0.4, -1), 0.25);
        assert_eq!(step_scale(2.0, 1), 2.0);
        assert_eq!(step_scale(3.0, 1), 3.0);
        assert_eq!(eye_size([2000, 2200], [4096, 4096], 0.4), [800, 880]);
        assert_eq!(eye_size([2000, 2200], [3000, 3200], 2.0), [3000, 3200]);
        assert_eq!(eye_size([2000, 2200], [4096, 4096], f32::NAN), [2000, 2200]);
    }
    #[test]
    fn only_offered_rates_are_selected_in_either_direction() {
        let rates = [120.0, 72.0, 90.0, 80.0, f32::NAN, -1.0];
        assert_eq!(step_rate(90.0, 1, &rates), 120.0);
        assert_eq!(step_rate(90.0, -1, &rates), 80.0);
        assert_eq!(step_rate(120.0, 1, &rates), 120.0);
        assert_eq!(step_rate(90.0, 1, &[72.0, 90.0]), 90.0);
        assert_eq!(step_rate(90.0, 1, &[]), 90.0);
    }
    #[test]
    fn render_cadences_follow_both_runtime_and_spacewarp_support() {
        let mut info = DisplayInfo {
            rates: vec![72., 90., 120.],
            ..Default::default()
        };
        assert_eq!(render_rates(&info), [72., 90., 120.]);
        info.spacewarp_supported = true;
        assert_eq!(render_rates(&info), [36., 45., 60., 72., 90., 120.]);
        assert_eq!(step_rate(45., -1, &render_rates(&info)), 36.);
        assert_eq!(step_rate(45., 1, &render_rates(&info)), 60.);
        assert_eq!(step_rate(72., -1, &render_rates(&info)), 60.);
    }
}
