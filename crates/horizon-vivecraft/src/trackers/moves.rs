// SPDX-License-Identifier: LGPL-3.0-or-later
//! Ports `RunTracker` (run in place) and `SwimTracker` (swimming strokes).
//! Copyright (c) Vivecraft contributors; LGPL-3.0-or-later (see LICENSE-LGPL-3.0).

use super::PlayerFlags;
use crate::modes::FreeMove;
use crate::player::Histories;
use crate::settings::VrSettings;
use crate::vr_data::VrData;
use glam::Vec3;

/// `RunTracker`: arms swinging like running move the player.
#[derive(Clone, Copy, Debug, Default)]
pub struct RunTracker {
    /// The yaw the arms point to, radians.
    direction: f64,
    speed: f32,
}

impl RunTracker {
    pub fn yaw(&self) -> f64 {
        self.direction
    }
    pub fn speed(&self) -> f32 {
        self.speed
    }

    #[allow(clippy::too_many_arguments)]
    pub fn is_active(
        &self,
        s: &VrSettings,
        p: &PlayerFlags,
        free_move: bool,
        bow_notched: bool,
    ) -> bool {
        free_move
            && !s.seated()
            && s.free_move_mode(false) == FreeMove::RunInPlace
            && p.alive
            && p.in_game
            && !(!p.on_ground && (p.in_water || p.in_lava))
            && !p.on_climbable
            && !bow_notched
    }

    /// One logic tick, on the world data of the tick.
    pub fn tick(&mut self, active: bool, h: &Histories, now_ms: u64, world: &VrData) {
        if !active {
            self.speed = 0.0;
            return;
        }
        let c0 = h.controller[0].average_speed(now_ms, 0.33);
        let c1 = h.controller[1].average_speed(now_ms, 0.33);
        if self.speed > 0.0 {
            if c0 < 0.1 && c1 < 0.1 {
                self.speed = 0.0;
                return;
            }
        } else if c0 < 0.6 && c1 < 0.6 {
            self.speed = 0.0;
            return;
        }
        if (c0 - c1).abs() > 0.5 {
            self.speed = 0.0;
            return;
        }
        let v = (world.controller(0).direction() + world.controller(1).direction()) * 0.5;
        self.direction = f64::from((-v.x).atan2(v.z));
        self.speed = (c0 + c1) * 0.5 * 1.3;
        if self.speed > 0.1 {
            self.speed = 1.0;
        }
        if self.speed > 1.0 {
            self.speed = 1.3;
        }
    }
}

/// `SwimTracker`: sweeping the hands through water pushes the player.
#[derive(Clone, Copy, Debug, Default)]
pub struct SwimTracker {
    motion: Vec3,
    last_dist: f64,
}

const FRICTION: f32 = 0.9;
const SWIM_SPEED: f32 = 1.3;

/// What the swim tracker asks of the game.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Swim {
    /// `setSwimming`.
    pub swimming: bool,
    /// `setSprinting`.
    pub sprinting: bool,
    /// `player.push(...)`.
    pub push: Vec3,
}

impl SwimTracker {
    /// `move_input` is whether `zza` or `xxa` is nonzero.
    pub fn is_active(&self, s: &VrSettings, p: &PlayerFlags, move_input: bool) -> bool {
        !s.seated()
            && s.bool("realisticSwimEnabled")
            && !p.paused_or_screen
            && p.alive
            && p.in_game
            && (p.in_water || p.in_lava)
            && !move_input
    }

    pub fn tick(&mut self, world: &VrData) -> Swim {
        let right = world.controller(0).position();
        let left = world.controller(1).position();
        let middle = (left - right) * 0.5 + right;
        let hmd_pos = world.head_pivot() - glam::DVec3::new(0.0, 0.3, 0.0);
        let move_dir =
            ((middle - hmd_pos).as_vec3().normalize_or_zero() + world.hmd().direction()) * 0.5;
        let back = Vec3::new(0.0, 0.0, -1.0);
        let controller_dir = (world.controller(0).custom_vector(back)
            + world.controller(1).custom_vector(back))
            * 0.5;
        let dir_factor = (controller_dir + move_dir).length() * 0.5;
        let distance = hmd_pos.distance(middle);
        let delta = self.last_dist - distance;
        if delta > 0.0 {
            self.motion += move_dir * (delta as f32 * SWIM_SPEED * dir_factor) * 0.15;
        }
        self.last_dist = distance;
        let out = Swim {
            swimming: self.motion.length() > 0.3,
            sprinting: self.motion.length() > 1.0,
            push: self.motion,
        };
        self.motion *= FRICTION;
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vr_data::{DataSettings, RoomPose, RoomPoses};
    use glam::{DVec3, Quat};

    fn world(hand_y: f32, hand_z: f32) -> VrData {
        let head = RoomPose {
            pos: Vec3::new(0.0, 1.6, 0.0),
            rot: Quat::IDENTITY,
        };
        let hand = |x: f32| RoomPose {
            pos: Vec3::new(x, hand_y, hand_z),
            rot: Quat::IDENTITY,
        };
        let room = RoomPoses {
            hmd: head,
            eyes: [head; 2],
            aim: [hand(0.2), hand(-0.2)],
            hand: [hand(0.2), hand(-0.2)],
            tracking: [true; 2],
        };
        VrData::new(&room, DataSettings::default(), DVec3::ZERO, 1.0, 1.0, 0.0)
    }

    #[test]
    fn running_in_place_needs_both_arms_pumping_alike() {
        let mut h = Histories::default();
        let mut t = RunTracker::default();
        // Both controllers move at 1 m/s.
        for i in 0..8u64 {
            let x = i as f32 * 0.05;
            h.controller[0].add(i * 50, Vec3::new(x, 1.0, 0.0));
            h.controller[1].add(i * 50, Vec3::new(x, 1.0, 0.0));
        }
        t.tick(true, &h, 350, &world(1.0, -0.4));
        assert_eq!(t.speed(), 1.0);
        // Facing -Z: yaw 180 degrees.
        assert!((t.yaw().abs() - std::f64::consts::PI).abs() < 1e-5);
        // One arm still: the run stops.
        let mut lopsided = Histories::default();
        for i in 0..8u64 {
            lopsided.controller[0].add(i * 50, Vec3::new(i as f32 * 0.05, 1.0, 0.0));
            lopsided.controller[1].add(i * 50, Vec3::new(0.0, 1.0, 0.0));
        }
        let mut t2 = RunTracker::default();
        t2.tick(true, &lopsided, 350, &world(1.0, -0.4));
        assert_eq!(t2.speed(), 0.0);
        t.tick(false, &h, 350, &world(1.0, -0.4));
        assert_eq!(t.speed(), 0.0);
    }

    #[test]
    fn drawing_the_hands_in_pushes_and_the_push_fades() {
        let mut t = SwimTracker::default();
        // Hands out ahead, then drawn toward the chest: a push along the way the head looks.
        let first = t.tick(&world(1.2, -0.9));
        assert_eq!(first.push, Vec3::ZERO);
        let drawn = t.tick(&world(1.2, -0.4));
        assert!(drawn.push.length() > 0.0, "{drawn:?}");
        let fading = t.tick(&world(1.2, -0.4));
        assert!(fading.push.length() < drawn.push.length());
    }

    #[test]
    fn swimming_needs_water_and_a_still_stick() {
        let mut s = VrSettings::default();
        s.set_bool("realisticSwimEnabled", true);
        let wet = PlayerFlags {
            alive: true,
            in_game: true,
            in_water: true,
            ..Default::default()
        };
        let t = SwimTracker::default();
        assert!(t.is_active(&s, &wet, false));
        assert!(!t.is_active(&s, &wet, true));
        assert!(!t.is_active(
            &s,
            &PlayerFlags {
                in_water: false,
                ..wet
            },
            false
        ));
    }
}
