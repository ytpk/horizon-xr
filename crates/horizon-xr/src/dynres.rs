//! Dynamic render scale with hysteresis: late, GPU-bound frames shrink the eyes a step,
//! and after a long quiet stretch they grow a step back. Visor does the same for its render
//! scale slider ("wait for the slider to stop before the targets are made again"); here the
//! trigger combines the frame interval and fresh stereo GPU timestamp feedback.
//!
//! The policy is plain data so it can be tested without a headset; `lib.rs` asks it once a
//! frame and, when it answers, makes the eyes' swapchains again. Off unless the player turns on
//! `horizonDynamicResolution` (or `HORIZON_XR_DYNRES=1`).

/// The scale factors the controller may pick, as multiples of the configured eye size.
pub const MIN_SCALE: f32 = 0.85;
pub const MAX_SCALE: f32 = 1.0;
/// Six percent steps, limited to 15 percent below the explicitly configured size.
const STEP_DOWN: f32 = 0.94;
const STEP_UP: f32 = 1.06;
/// A frame is late when its interval is more than this many display periods.
const LATE: f32 = 1.35;
/// Late frames in a window of `WINDOW` that shrink the image; the window slides.
const LATE_TO_SHRINK: u32 = 4;
const WINDOW: u32 = 30;
/// Frames of no late frame before a step up, and the pause after any change.
const QUIET_TO_GROW: u32 = 900;
const COOLDOWN: u32 = 120;

#[derive(Clone, Debug)]
pub struct DynamicScale {
    scale: f32,
    /// Late frames and frames seen in the current window.
    late: u32,
    seen: u32,
    quiet: u32,
    cooldown: u32,
}

impl Default for DynamicScale {
    fn default() -> Self {
        Self {
            scale: MAX_SCALE,
            late: 0,
            seen: 0,
            quiet: 0,
            cooldown: 0,
        }
    }
}

impl DynamicScale {
    #[cfg(test)]
    pub fn scale(&self) -> f32 {
        self.scale
    }

    /// One frame: `interval` is the time since the last frame began and `period` the display's.
    /// Returns the new scale when it changed.
    pub fn frame(&mut self, interval: f32, period: f32, gpu_time: Option<f32>) -> Option<f32> {
        let Some(gpu_time) = gpu_time.filter(|v| v.is_finite() && *v > 0.0) else {
            self.late = 0;
            self.quiet = 0;
            self.seen = 0;
            return None;
        };
        if !period.is_finite() || !interval.is_finite() || period <= 0.0 || interval <= 0.0 {
            return None;
        }
        self.cooldown = self.cooldown.saturating_sub(1);
        let late = interval > period * LATE && gpu_time > period * 0.85;
        self.seen += 1;
        if late {
            self.late += 1;
            self.quiet = 0;
        } else if gpu_time < period * 0.7 {
            self.quiet += 1;
        } else {
            self.quiet = 0;
        }
        if self.seen >= WINDOW {
            self.seen = 0;
            self.late = 0;
        }
        if self.cooldown == 0 && self.late >= LATE_TO_SHRINK && self.scale > MIN_SCALE {
            return Some(self.set((self.scale * STEP_DOWN).max(MIN_SCALE)));
        }
        if self.cooldown == 0 && self.quiet >= QUIET_TO_GROW && self.scale < MAX_SCALE {
            return Some(self.set((self.scale * STEP_UP).min(MAX_SCALE)));
        }
        None
    }

    /// Allocation failed: retain the resolution that is actually displayed,
    /// and wait before trying to allocate again.
    pub fn reject(&mut self, previous: Self) {
        *self = previous;
        self.late = 0;
        self.quiet = 0;
        self.seen = 0;
        self.cooldown = COOLDOWN;
    }

    fn set(&mut self, scale: f32) -> f32 {
        self.scale = scale;
        self.late = 0;
        self.seen = 0;
        self.quiet = 0;
        self.cooldown = COOLDOWN;
        scale
    }
}

/// The size of one side at `scale`, in whole pixels (a multiple of 8, as encoders like).
pub fn scaled_side(base: u32, scale: f32) -> u32 {
    ((base as f32 * scale).ceil() as u32)
        .div_ceil(8)
        .saturating_mul(8)
        .min(base)
        .max(64)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PERIOD: f32 = 1.0 / 90.0;

    #[test]
    fn steady_frames_never_change_the_scale_until_a_long_quiet_stretch_at_a_lower_scale() {
        let mut d = DynamicScale::default();
        for _ in 0..5_000 {
            assert_eq!(d.frame(PERIOD, PERIOD, Some(PERIOD * 0.5)), None);
        }
        assert_eq!(d.scale(), MAX_SCALE);
    }

    #[test]
    fn a_run_of_late_frames_shrinks_it_one_step_and_then_waits() {
        let mut d = DynamicScale::default();
        let mut changes = Vec::new();
        for _ in 0..20 {
            if let Some(s) = d.frame(PERIOD * 2.0, PERIOD, Some(PERIOD * 1.1)) {
                changes.push(s);
            }
        }
        // 4 late frames shrink once; the cooldown then holds the next change off for 120 frames.
        assert_eq!(changes.len(), 1);
        assert!((changes[0] - 0.94).abs() < 1e-6);
    }

    #[test]
    fn a_single_dropped_frame_does_not_shrink_it() {
        let mut d = DynamicScale::default();
        for i in 0..200 {
            let interval = if i % 31 == 0 { PERIOD * 3.0 } else { PERIOD };
            assert_eq!(
                d.frame(interval, PERIOD, Some(PERIOD * 1.1)),
                None,
                "frame {i}"
            );
        }
    }

    #[test]
    fn it_never_goes_below_the_floor_and_grows_back_after_900_quiet_frames() {
        let mut d = DynamicScale::default();
        for _ in 0..3_000 {
            d.frame(PERIOD * 2.0, PERIOD, Some(PERIOD * 1.1));
        }
        assert!(
            d.scale() >= MIN_SCALE - 1e-6 && d.scale() <= 0.85,
            "{}",
            d.scale()
        );
        let low = d.scale();
        let mut grown = None;
        for i in 0..1_200 {
            if let Some(s) = d.frame(PERIOD, PERIOD, Some(PERIOD * 0.5)) {
                grown = Some((i, s));
                break;
            }
        }
        let (at, s) = grown.expect("it grows back");
        assert!((899..=900).contains(&at), "{at}");
        assert!((s - low * 1.06).abs() < 1e-5);
    }

    #[test]
    fn cpu_bound_and_untimed_frames_do_not_reduce_resolution() {
        for gpu in [None, Some(PERIOD * 0.3), Some(f32::NAN)] {
            let mut d = DynamicScale::default();
            for _ in 0..2000 {
                assert_eq!(d.frame(PERIOD * 3.0, PERIOD, gpu), None);
            }
            assert_eq!(d.scale(), 1.0);
        }
    }

    #[test]
    fn failed_allocations_keep_the_previous_scale_and_back_off() {
        let mut d = DynamicScale::default();
        for _ in 0..3 {
            d.frame(PERIOD * 2.0, PERIOD, Some(PERIOD * 1.1));
        }
        let previous = d.clone();
        assert_eq!(
            d.frame(PERIOD * 2.0, PERIOD, Some(PERIOD * 1.1)),
            Some(0.94)
        );
        d.reject(previous);
        assert_eq!(d.scale(), 1.0);
        for _ in 0..100 {
            assert_eq!(d.frame(PERIOD * 2.0, PERIOD, Some(PERIOD * 1.1)), None);
        }
    }

    #[test]
    fn sides_are_whole_multiples_of_eight() {
        assert_eq!(scaled_side(1680, 1.0), 1680);
        assert_eq!(scaled_side(1680, 0.88), 1480);
        assert_eq!(scaled_side(40, 0.5), 64);
    }
}
