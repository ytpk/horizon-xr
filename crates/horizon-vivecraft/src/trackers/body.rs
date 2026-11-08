// SPDX-License-Identifier: LGPL-3.0-or-later
//! Ports `SneakTracker`, `JumpTracker` (the realistic jump; the Climbey boots jump is with the
//! climb tracker) and `CrawlTracker`.
//! Copyright (c) Vivecraft contributors; LGPL-3.0-or-later (see LICENSE-LGPL-3.0).

use super::PlayerFlags;
use crate::modes::Tri;
use crate::player::{Histories, player_height};
use crate::settings::VrSettings;

/// Whether the player moves freely or simulates falling (`!getFreeMove() && !simulateFalling`
/// gates both the sneak and the jump tracker).
fn locomotion_allows(free_move: bool, s: &VrSettings) -> bool {
    free_move || s.bool("simulateFalling")
}

/// `SneakTracker`: crouching in the room sneaks.
#[derive(Clone, Copy, Debug, Default)]
pub struct SneakTracker {
    pub sneak_override: bool,
    /// Set while a roomscale dismount waits.
    pub sneak_counter: i32,
}

impl SneakTracker {
    pub fn is_active(&self, s: &VrSettings, p: &PlayerFlags, free_move: bool) -> bool {
        !s.seated()
            && locomotion_allows(free_move, s)
            && s.bool("realisticSneakEnabled")
            && p.in_game
            && p.alive
            && (p.on_ground || p.on_climbable)
            && !p.passenger
    }

    /// One logic tick.
    pub fn tick(&mut self, s: &VrSettings, p: &PlayerFlags, free_move: bool, h: &Histories) {
        if self.is_active(s, p, free_move) {
            if !p.paused_or_screen && self.sneak_counter > 0 {
                self.sneak_counter -= 1;
            }
            self.sneak_override =
                player_height(s) - h.hmd_pivot.latest().y > s.float("sneakThreshold");
        } else {
            self.sneak_override = false;
        }
    }
}

/// `JumpTracker` without the Climbey boots.
#[derive(Clone, Copy, Debug, Default)]
pub struct JumpTracker;

impl JumpTracker {
    pub fn is_active(&self, s: &VrSettings, p: &PlayerFlags, free_move: bool) -> bool {
        !s.seated()
            && locomotion_allows(free_move, s)
            && s.realistic_jump() != Tri::Off
            && p.alive
            && p.in_game
            && !(p.in_water || p.in_lava || !p.on_ground)
            && !p.shift_down
            && !p.passenger
    }

    /// One logic tick: whether the player jumps (`jumpFromGround`). Without Climbey boots on,
    /// a quick rise of the head pivot above the standing height does it; with them, only when
    /// `realisticJumpEnabled` is `ON`.
    pub fn tick(
        &self,
        s: &VrSettings,
        p: &PlayerFlags,
        free_move: bool,
        h: &Histories,
        now_ms: u64,
        climbey_boots: bool,
    ) -> bool {
        if !self.is_active(s, p, free_move) {
            return false;
        }
        (!climbey_boots || s.realistic_jump() == Tri::On)
            && f64::from(h.hmd_pivot.net_movement(now_ms, 0.25).y) > 0.1
            && h.hmd_pivot.latest().y - player_height(s) > s.float("jumpThreshold")
    }
}

/// `CrawlTracker`: lying low in the room crawls.
#[derive(Clone, Copy, Debug, Default)]
pub struct CrawlTracker {
    was_crawling: bool,
    pub crawling: bool,
    pub crawlsteresis: bool,
}

/// What the crawl tracker asks of the game after a tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CrawlEvent {
    None,
    /// The state changed: set the swimming pose when crawling, and tell the server
    /// (`CrawlPayloadC2S`) when it allows crawling.
    Changed {
        crawling: bool,
    },
}

impl CrawlTracker {
    pub fn is_active(&self, s: &VrSettings, p: &PlayerFlags, server_allows: bool) -> bool {
        !s.seated()
            && s.bool("allowCrawling")
            && server_allows
            && p.alive
            && !p.spectator
            && !p.sleeping
            && !p.passenger
    }

    /// One logic tick. `eye_height_scale` is the player's `ScaleHelper` factor, `swimming_pose`
    /// whether the entity is in the swimming pose.
    pub fn tick(
        &mut self,
        s: &VrSettings,
        p: &PlayerFlags,
        server_allows: bool,
        h: &Histories,
        now_ms: u64,
        world_scale: f32,
        eye_height_scale: f32,
        swimming_pose: bool,
    ) -> CrawlEvent {
        if self.is_active(s, p, server_allows) {
            let scaled = world_scale / eye_height_scale;
            self.crawling = h.hmd_pivot.average_position(now_ms, 0.2).y * scaled + 0.1
                < s.float("crawlThreshold");
        } else {
            self.crawling = false;
            self.crawlsteresis = false;
        }
        let mut event = CrawlEvent::None;
        if self.crawling != self.was_crawling {
            if self.crawling {
                self.crawlsteresis = true;
            }
            if server_allows {
                event = CrawlEvent::Changed {
                    crawling: self.crawling,
                };
            }
            self.was_crawling = self.crawling;
        }
        if !self.crawling && !swimming_pose {
            self.crawlsteresis = false;
        }
        event
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;

    fn standing() -> PlayerFlags {
        PlayerFlags {
            alive: true,
            on_ground: true,
            in_game: true,
            ..Default::default()
        }
    }

    fn at(h: &mut Histories, ms: u64, y: f32) {
        h.hmd_pivot.add(ms, Vec3::new(0.0, y, 0.0));
    }

    #[test]
    fn crouching_below_the_threshold_sneaks_only_while_the_tracker_is_active() {
        let s = VrSettings::default();
        let mut t = SneakTracker::default();
        let mut h = Histories::default();
        // 1.52 m calibrated height; 0.4 m lower sneaks (default threshold).
        at(&mut h, 0, 1.0);
        t.tick(&s, &standing(), true, &h);
        assert!(t.sneak_override);
        at(&mut h, 50, 1.45);
        t.tick(&s, &standing(), true, &h);
        assert!(!t.sneak_override);
        // In the air the override is dropped.
        at(&mut h, 100, 1.0);
        t.tick(
            &s,
            &PlayerFlags {
                on_ground: false,
                ..standing()
            },
            true,
            &h,
        );
        assert!(!t.sneak_override);
        // Seated players do not crouch in the room.
        let mut seated = VrSettings::default();
        seated.set_bool("seated", true);
        t.tick(&seated, &standing(), true, &h);
        assert!(!t.sneak_override);
    }

    #[test]
    fn a_quick_rise_above_the_standing_height_jumps() {
        let mut s = VrSettings::default();
        s.set("realisticJumpEnabled", "ON");
        let mut h = Histories::default();
        at(&mut h, 0, 1.5);
        at(&mut h, 100, 1.55);
        at(&mut h, 200, 1.65);
        let jump = JumpTracker;
        assert!(jump.tick(&s, &standing(), true, &h, 200, false));
        // Slowly rising does not (net movement under 0.1 m).
        let mut slow = Histories::default();
        at(&mut slow, 0, 1.58);
        at(&mut slow, 200, 1.62);
        assert!(!jump.tick(&s, &standing(), true, &slow, 200, false));
        // Off: never.
        s.set("realisticJumpEnabled", "OFF");
        assert!(!jump.tick(&s, &standing(), true, &h, 200, false));
    }

    #[test]
    fn crawling_tells_the_server_once_per_change() {
        let mut s = VrSettings::default();
        s.set_bool("allowCrawling", true);
        let mut t = CrawlTracker::default();
        let mut h = Histories::default();
        for ms in [0, 50, 100] {
            at(&mut h, ms, 0.5);
        }
        let e = t.tick(&s, &standing(), true, &h, 100, 1.0, 1.0, false);
        assert_eq!(e, CrawlEvent::Changed { crawling: true });
        assert!(t.crawlsteresis);
        assert_eq!(
            t.tick(&s, &standing(), true, &h, 100, 1.0, 1.0, true),
            CrawlEvent::None
        );
        for ms in [200, 250, 300] {
            at(&mut h, ms, 1.6);
        }
        let e = t.tick(&s, &standing(), true, &h, 300, 1.0, 1.0, false);
        assert_eq!(e, CrawlEvent::Changed { crawling: false });
        assert!(!t.crawlsteresis);
        // A server that does not allow it hears nothing and nothing crawls.
        let mut off = CrawlTracker::default();
        assert_eq!(
            off.tick(&s, &standing(), false, &h, 100, 1.0, 1.0, false),
            CrawlEvent::None
        );
        assert!(!off.crawling);
    }
}
