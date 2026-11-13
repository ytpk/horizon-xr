// SPDX-License-Identifier: MPL-2.0
//! Visor's height measuring (`PlayerHeightTracker`, `HeightEstimator`, `HeightFormat`): with Auto
//! Height on, the game watches where the head rests and takes the height that 90 % of the quiet
//! samples stay under; "Measure Now" takes the head's average height over a second after a
//! three second countdown. Copyright (C) Phoenix-Ra and Visor contributors; MPL-2.0 (see
//! LICENSE-MPL-2.0).

/// `VRPlayer.DEFAULT_FULL_HEIGHT`: the standing head height Visor assumes before it has measured
/// one, metres.
pub const DEFAULT_FULL_HEIGHT: f32 = 1.52;
/// `VRClientSettings.MIN_HEIGHT`: below this a head height is not a standing one.
pub const MIN_HEIGHT: f32 = DEFAULT_FULL_HEIGHT / 4.0;
const MAX: f32 = 2.6;
const BIN: f32 = 0.01;
const DECAY: f32 = 0.99988;
/// How far the head may look up or down (the vertical part of its direction) and still count.
const LEVEL_LIMIT: f32 = 0.42;
/// `PlayerHeightTracker.DEAD_ZONE`: a change smaller than this share of the height is ignored.
const DEAD_ZONE: f32 = 0.02;
/// `HeightEstimator`'s mass a histogram needs before it speaks (about three seconds of samples at 20 Hz).
const MIN_MASS: f32 = 60.0;

/// `HeightEstimator`: a histogram of the head's resting heights that forgets slowly; the answer is
/// the 90th percentile.
#[derive(Clone, Debug)]
pub struct HeightEstimator {
    bin: Vec<f32>,
    mass: f32,
    last_ms: Option<u64>,
}

impl Default for HeightEstimator {
    fn default() -> Self {
        Self {
            bin: vec![0.0; ((MAX - MIN_HEIGHT) / BIN) as usize + 1],
            mass: 0.0,
            last_ms: None,
        }
    }
}

/// Visor samples once per game tick (20 Hz): the mass needed to speak is three seconds of
/// samples and the decay is per sample, so a caller that runs per frame must be thinned to this.
pub const SAMPLE_INTERVAL_MS: u64 = 50;

impl HeightEstimator {
    /// One tick: `speed` is the head's average speed over the last half second (m/s), `look_y` the
    /// vertical part of the direction it faces and `height` its average height over that half
    /// second. A moving head or one looking steeply up or down says nothing about the height.
    pub fn sample(&mut self, speed: f32, look_y: f32, height: f32) {
        if !speed.is_finite()
            || speed < 0.0
            || speed > 0.2
            || !look_y.is_finite()
            || look_y.abs() > LEVEL_LIMIT
        {
            return;
        }
        if !(MIN_HEIGHT..=MAX).contains(&height) {
            return;
        }
        for b in &mut self.bin {
            *b *= DECAY;
        }
        self.mass = self.mass * DECAY + 1.0;
        let i = ((height - MIN_HEIGHT) / BIN) as usize;
        if let Some(b) = self.bin.get_mut(i) {
            *b += 1.0;
        }
    }

    /// [`sample`](Self::sample) at most once per game tick (`now_ms` on any steady clock).
    pub fn sample_at(&mut self, now_ms: u64, speed: f32, look_y: f32, height: f32) {
        if self
            .last_ms
            .is_some_and(|last| now_ms >= last && now_ms - last < SAMPLE_INTERVAL_MS)
        {
            return;
        }
        self.last_ms = Some(now_ms);
        self.sample(speed, look_y, height);
    }

    /// The height 90 % of the samples are at or under, once there are enough of them.
    pub fn estimate(&self) -> Option<f32> {
        if self.mass < MIN_MASS {
            return None;
        }
        let target = self.mass * 0.9;
        let mut acc = 0.0;
        for (i, b) in self.bin.iter().enumerate() {
            acc += b;
            if acc >= target {
                return Some(MIN_HEIGHT + i as f32 * BIN);
            }
        }
        None
    }

    pub fn reset(&mut self) {
        self.bin.iter_mut().for_each(|b| *b = 0.0);
        self.mass = 0.0;
        self.last_ms = None;
    }
}

/// What the auto height wants done with the stored height.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Auto {
    /// Nothing.
    Keep,
    /// Store this height; `first` when none was measured before (Visor tells the player).
    Store { height: f32, first: bool },
}

/// `PlayerHeightTracker.autoUpdate`: the first estimate is stored at once; later ones only when they
/// differ by more than the dead zone, and a lower one only while a screen is open or in the menu room
/// (`isSafeMoment`), so ducking through a door does not shrink the player. `stored` is `None` when
/// nothing was measured yet.
pub fn auto_update(estimate: Option<f32>, stored: Option<f32>, safe_moment: bool) -> Auto {
    let Some(estimate) = estimate.filter(|h| h.is_finite() && (MIN_HEIGHT..=MAX).contains(h))
    else {
        return Auto::Keep;
    };
    // A quiet seated/crawling launch is not an automatic standing calibration. Smaller
    // users can explicitly Measure Now; never guess their standing height from a low pose.
    if estimate < 1.2 {
        return Auto::Keep;
    }
    let Some(stored) = stored else {
        return Auto::Store {
            height: estimate,
            first: true,
        };
    };
    if (estimate - stored).abs() / stored < DEAD_ZONE {
        return Auto::Keep;
    }
    if estimate > stored || safe_moment {
        Auto::Store {
            height: estimate,
            first: false,
        }
    } else {
        Auto::Keep
    }
}

/// `HeightFormat`: a height as the player's units show it: `171 cm`, or `5 ft 7 in`.
pub fn format_height(metres: f32, imperial: bool) -> String {
    if imperial {
        let inches = (metres * 100.0 / 2.54).round() as i32;
        format!("{} ft {} in", inches / 12, inches % 12)
    } else {
        format!("{} cm", (metres * 100.0).round() as i32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_needs_three_seconds_of_quiet_samples_and_answers_with_the_90th_percentile() {
        let mut e = HeightEstimator::default();
        for _ in 0..59 {
            e.sample(0.0, 0.0, 1.70);
        }
        assert_eq!(e.estimate(), None);
        // The mass decays a little with every sample (0.99988), so 60 of them do not reach 60: 61 do.
        e.sample(0.0, 0.0, 1.70);
        assert_eq!(e.estimate(), None);
        e.sample(0.0, 0.0, 1.70);
        // Every sample in the 1.70 bin: its lower edge, 1.52 / 4 + 0.01 * 132 = 1.70.
        let got = e.estimate().unwrap();
        assert!((got - 1.70).abs() < 0.0101, "{got}");
        // Ducking half the time (1.20) does not drag the 90th percentile under the standing height:
        // 41 standing samples and 21 ducked ones (a third) put the 90 % line at the standing bin.
        let mut e = HeightEstimator::default();
        for _ in 0..41 {
            e.sample(0.0, 0.0, 1.70);
        }
        for _ in 0..21 {
            e.sample(0.0, 0.0, 1.20);
        }
        let got = e.estimate().unwrap();
        assert!((got - 1.70).abs() < 0.0101, "{got}");
    }

    #[test]
    fn a_moving_or_tilted_head_and_impossible_heights_say_nothing() {
        let mut e = HeightEstimator::default();
        for _ in 0..100 {
            e.sample(0.5, 0.0, 1.7);
            e.sample(0.0, 0.5, 1.7);
            e.sample(0.0, 0.0, 0.2);
            e.sample(0.0, 0.0, 3.0);
        }
        assert_eq!(e.estimate(), None);
    }

    #[test]
    fn the_first_height_is_stored_and_a_lower_one_waits_for_a_safe_moment() {
        assert_eq!(auto_update(None, None, false), Auto::Keep);
        assert_eq!(
            auto_update(Some(1.7), None, false),
            Auto::Store {
                height: 1.7,
                first: true
            }
        );
        // 1.70 against 1.68 is 1.2 %: inside the dead zone.
        assert_eq!(auto_update(Some(1.70), Some(1.68), false), Auto::Keep);
        // A taller one is taken at once, a shorter one only with a screen open.
        assert_eq!(
            auto_update(Some(1.80), Some(1.68), false),
            Auto::Store {
                height: 1.80,
                first: false
            }
        );
        assert_eq!(auto_update(Some(1.50), Some(1.68), false), Auto::Keep);
        assert_eq!(
            auto_update(Some(1.50), Some(1.68), true),
            Auto::Store {
                height: 1.50,
                first: false
            }
        );
    }

    /// A 1.75 m player's head pivot over time at 90 frames a second, through everything the game
    /// does with it: the estimator once per tick, `auto_update`, then Visor's crawl and sneak
    /// tasks reading the same room-space height. The poses they cause never reach the height.
    struct Sim {
        est: HeightEstimator,
        stored: Option<f32>,
        crawl: crate::tasks::movement::CrawlTask,
        sneak: crate::tasks::movement::SneakTask,
        now: u64,
        head: f32,
        history: Vec<(u64, f32)>,
    }

    impl Sim {
        /// Moves the head to `to` at 1 m/s, holds it for `seconds`; the last tick's (crawl, sneak).
        fn run(&mut self, to: f32, seconds: f32) -> Vec<(bool, bool)> {
            use crate::tasks::movement::{DEFAULT_CRAWL_THRESHOLD, DEFAULT_SNEAK_THRESHOLD};
            let mut seen = Vec::new();
            for _ in 0..(seconds * 90.0) as usize {
                self.now += 11;
                self.head += (to - self.head).clamp(-0.011, 0.011);
                self.history.push((self.now, self.head));
                // The average over the last half second and its speed, as `Vec3History` does.
                let recent: Vec<_> = self
                    .history
                    .iter()
                    .rev()
                    .take_while(|(t, _)| self.now - t <= 500)
                    .collect();
                let (new, old) = (recent[0], recent[recent.len() - 1]);
                let avg = recent.iter().map(|(_, y)| y).sum::<f32>() / recent.len() as f32;
                let speed = (new.1 - old.1).abs() / (0.001 * (new.0 - old.0).max(1) as f32);
                self.est.sample_at(self.now, speed, 0.0, avg);
                if let Auto::Store { height, .. } =
                    auto_update(self.est.estimate(), self.stored, false)
                {
                    self.stored = Some(height);
                }
                let full = self.stored.unwrap_or(DEFAULT_FULL_HEIGHT);
                self.crawl.tick(self.head, full, DEFAULT_CRAWL_THRESHOLD);
                self.sneak.tick(
                    self.head,
                    full,
                    DEFAULT_SNEAK_THRESHOLD,
                    self.crawl.crawling,
                );
                seen.push((self.crawl.crawling, self.sneak.sneaking));
            }
            seen
        }
    }

    #[test]
    fn a_175_cm_player_standing_crouching_crawling_and_standing_again() {
        use crate::tasks::movement::{
            CrawlTask, DEFAULT_CRAWL_THRESHOLD, DEFAULT_SNEAK_THRESHOLD, SneakTask,
        };
        const STAND: f32 = 1.75;
        let mut sim = Sim {
            est: HeightEstimator::default(),
            stored: None,
            crawl: CrawlTask::default(),
            sneak: SneakTask::default(),
            now: 0,
            head: STAND,
            history: Vec::new(),
        };
        // Standing upright: never sneaking or crawling, before and after the height is measured.
        assert!(sim.run(STAND, 6.0).iter().all(|&(c, s)| !c && !s));
        let measured = sim.stored.expect("the standing height is measured");
        assert!((measured - STAND).abs() < 0.02, "{measured}");
        // Crouching to 1.40 m (0.80 of the height): sneaking, not crawling, once it is down.
        assert_eq!(sim.run(1.40, 4.0).last(), Some(&(false, true)));
        // Crawling at 1.10 m (0.63): crawling, and a crawl is not a sneak.
        assert_eq!(sim.run(1.10, 4.0).last(), Some(&(true, false)));
        // Standing again: both let go, and the height is still the standing one.
        assert_eq!(sim.run(STAND, 4.0).last(), Some(&(false, false)));
        assert_eq!(sim.stored, Some(measured));
        // The thresholds on the measured height: 1.49 m stands, 1.48 sneaks, 1.22 m crawls.
        let mut s = SneakTask::default();
        assert!(!s.tick(1.49, 1.75, DEFAULT_SNEAK_THRESHOLD, false));
        assert!(s.tick(1.48, 1.75, DEFAULT_SNEAK_THRESHOLD, false));
        let mut c = CrawlTask::default();
        assert_eq!(c.tick(1.23, 1.75, DEFAULT_CRAWL_THRESHOLD), None);
        assert_eq!(c.tick(1.22, 1.75, DEFAULT_CRAWL_THRESHOLD), Some(true));
    }

    #[test]
    fn the_estimator_counts_ticks_not_frames() {
        // A head resting for a second (90 frames) is 20 ticks: it says nothing yet.
        let mut e = HeightEstimator::default();
        for t in 0..90 {
            e.sample_at(t * 11, 0.0, 0.0, 1.75);
        }
        assert_eq!(e.estimate(), None);
        // Three seconds is enough.
        for t in 90..400 {
            e.sample_at(t * 11, 0.0, 0.0, 1.75);
        }
        assert!(e.estimate().is_some());
    }

    #[test]
    fn auto_does_not_calibrate_a_low_starting_pose_or_non_finite_motion() {
        assert_eq!(auto_update(Some(0.85), None, true), Auto::Keep);
        assert_eq!(auto_update(Some(0.85), Some(1.7), true), Auto::Keep);
        let mut estimator = HeightEstimator::default();
        for _ in 0..100 {
            estimator.sample(f32::NAN, 0.0, 1.7);
            estimator.sample(0.0, f32::NAN, 1.7);
        }
        assert_eq!(estimator.estimate(), None);
    }

    #[test]
    fn heights_are_shown_in_the_players_units() {
        assert_eq!(format_height(1.714, false), "171 cm");
        // 1.70 m = 66.93 in = 5 ft 7 in.
        assert_eq!(format_height(1.70, true), "5 ft 7 in");
        // 1.83 m = 72.05 in = 6 ft 0 in.
        assert_eq!(format_height(1.83, true), "6 ft 0 in");
    }
}
