//! Dynamic render scale with hysteresis: when frames come late the eyes' images shrink a step,
//! and after a long quiet stretch they grow a step back. Visor does the same for its render
//! scale slider ("wait for the slider to stop before the targets are made again"); here the
//! trigger is the frame interval.
//!
//! The policy is plain data so it can be tested without a headset; `lib.rs` asks it once a
//! frame and, when it answers, makes the eyes' swapchains again. Off unless the player turns on
//! `horizonDynamicResolution` (or `HORIZON_XR_DYNRES=1`).

/// The scale factors the controller may pick, as multiples of the configured eye size.
pub const MIN_SCALE: f32 = 0.6;
pub const MAX_SCALE: f32 = 1.0;
/// One step down is 12 % of the side; one step up 6 %: down fast, up slowly.
const STEP_DOWN: f32 = 0.88;
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
    pub fn frame(&mut self, interval: f32, period: f32) -> Option<f32> {
        if period <= 0.0 {
            return None;
        }
        self.cooldown = self.cooldown.saturating_sub(1);
        let late = interval > period * LATE;
        self.seen += 1;
        if late {
            self.late += 1;
            self.quiet = 0;
        } else {
            self.quiet += 1;
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
    (((base as f32 * scale) as u32 / 8) * 8).max(64)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PERIOD: f32 = 1.0 / 90.0;

    #[test]
    fn steady_frames_never_change_the_scale_until_a_long_quiet_stretch_at_a_lower_scale() {
        let mut d = DynamicScale::default();
        for _ in 0..5_000 {
            assert_eq!(d.frame(PERIOD, PERIOD), None);
        }
        assert_eq!(d.scale(), MAX_SCALE);
    }

    #[test]
    fn a_run_of_late_frames_shrinks_it_one_step_and_then_waits() {
        let mut d = DynamicScale::default();
        let mut changes = Vec::new();
        for _ in 0..20 {
            if let Some(s) = d.frame(PERIOD * 2.0, PERIOD) {
                changes.push(s);
            }
        }
        // 4 late frames shrink once; the cooldown then holds the next change off for 120 frames.
        assert_eq!(changes.len(), 1);
        assert!((changes[0] - 0.88).abs() < 1e-6);
    }

    #[test]
    fn a_single_dropped_frame_does_not_shrink_it() {
        let mut d = DynamicScale::default();
        for i in 0..200 {
            let interval = if i % 31 == 0 { PERIOD * 3.0 } else { PERIOD };
            assert_eq!(d.frame(interval, PERIOD), None, "frame {i}");
        }
    }

    #[test]
    fn it_never_goes_below_the_floor_and_grows_back_after_900_quiet_frames() {
        let mut d = DynamicScale::default();
        for _ in 0..3_000 {
            d.frame(PERIOD * 2.0, PERIOD);
        }
        assert!(
            d.scale() >= MIN_SCALE - 1e-6 && d.scale() < 0.7,
            "{}",
            d.scale()
        );
        let low = d.scale();
        let mut grown = None;
        for i in 0..1_200 {
            if let Some(s) = d.frame(PERIOD, PERIOD) {
                grown = Some((i, s));
                break;
            }
        }
        let (at, s) = grown.expect("it grows back");
        assert!((899..=900).contains(&at), "{at}");
        assert!((s - low * 1.06).abs() < 1e-5);
    }

    #[test]
    fn sides_are_whole_multiples_of_eight() {
        assert_eq!(scaled_side(1680, 1.0), 1680);
        assert_eq!(scaled_side(1680, 0.88), 1472);
        assert_eq!(scaled_side(40, 0.5), 64);
    }
}
