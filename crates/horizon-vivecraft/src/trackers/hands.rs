// SPDX-License-Identifier: LGPL-3.0-or-later
//! Ports `EatingTracker`, `BackpackTracker` and `BowTracker`.
//! Copyright (c) Vivecraft contributors; LGPL-3.0-or-later (see LICENSE-LGPL-3.0).

use super::{Haptic, PlayerFlags};
use crate::modes::BowMode;
use crate::player::Histories;
use crate::settings::VrSettings;
use crate::vr_data::VrData;
use glam::Vec3;

/// `ItemUseAnimation` of the held item, as the eating tracker tells them apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UseAnimation {
    Eat,
    Drink,
    TootHorn,
    Bow,
    Other,
}

/// `EatingTracker`: bringing food or a drink to the mouth uses the item.
#[derive(Clone, Copy, Debug, Default)]
pub struct EatingTracker {
    eating: [bool; 2],
    eat_start_ms: u64,
}

const MOUTH_TO_EYE: f32 = 0.0;
const THRESHOLD: f32 = 0.25;
const EAT_TIME_MS: u64 = 2100;

/// What the eating tracker asks of the game in a tick.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EatingOutput {
    /// `useItem(hand)` to start eating with the hand (0 main, 1 off).
    pub use_item: [bool; 2],
    pub haptics: [Option<u32>; 2],
}

fn eatable(a: UseAnimation) -> bool {
    matches!(
        a,
        UseAnimation::Eat | UseAnimation::Drink | UseAnimation::TootHorn
    )
}

impl EatingTracker {
    pub fn item_in_use(&self) -> bool {
        self.eating[0] || self.eating[1]
    }
    pub fn eating(&self, hand: usize) -> bool {
        self.eating[hand.min(1)]
    }

    pub fn is_active(
        &self,
        s: &VrSettings,
        p: &PlayerFlags,
        bow_active: bool,
        held: [UseAnimation; 2],
    ) -> bool {
        !s.seated()
            && p.in_game
            && p.alive
            && !p.sleeping
            && !bow_active
            && (eatable(held[0]) || eatable(held[1]))
    }

    /// Leaving the tracker's gate clears it (`inactiveProcess`).
    pub fn inactive(&mut self) {
        self.eating = [false; 2];
    }

    /// One logic tick on the room data. `use_started[c]` is whether the game's `useItem`
    /// accepted the use; `remaining_ticks` the item's use ticks left (for the crunch haptics).
    pub fn tick(
        &mut self,
        room: &VrData,
        h: &Histories,
        now_ms: u64,
        held: [UseAnimation; 2],
        remaining_ticks: i64,
        use_started: impl Fn(usize) -> bool,
    ) -> EatingOutput {
        let mut out = EatingOutput::default();
        let hmd_pos = room.hmd().position_f();
        let mouth = room.hmd().custom_vector(Vec3::new(0.0, -MOUTH_TO_EYE, 0.0)) + hmd_pos;
        for c in 0..2 {
            let anim = held[c];
            if !eatable(anim) {
                continue;
            }
            let food = h.controller[c].average_position(now_ms, 0.333)
                + room.controller(c).direction() * 0.2;
            if mouth.distance(food) < THRESHOLD {
                let crunch = match anim {
                    UseAnimation::Drink => {
                        // A drink is tipped: not with the controller's up axis pointing up.
                        if room.controller(c).custom_vector(Vec3::Y).y > 0.0 {
                            self.eating[c] = false;
                            continue;
                        }
                        0
                    }
                    UseAnimation::Eat => 2,
                    _ => 1,
                };
                if !self.eating[c] && use_started(c) {
                    out.use_item[c] = true;
                    self.eating[c] = true;
                    self.eat_start_ms = now_ms;
                }
                if self.eating[c] && remaining_ticks > 0 && remaining_ticks % 5 <= crunch {
                    out.haptics[c] = Some(700);
                }
                if now_ms.saturating_sub(self.eat_start_ms) > EAT_TIME_MS {
                    self.eating[c] = false;
                }
            } else {
                self.eating[c] = false;
            }
        }
        out
    }
}

/// `BackpackTracker`: a hand reaching over the shoulder presses the configured binding.
#[derive(Clone, Copy, Debug, Default)]
pub struct BackpackTracker {
    pub was_in: [bool; 2],
}

impl BackpackTracker {
    pub fn is_active(
        &self,
        s: &VrSettings,
        p: &PlayerFlags,
        attacking_or_interacting: bool,
        bow_drawing: bool,
    ) -> bool {
        !s.seated()
            && s.bool("backpackSwitching")
            && p.in_game
            && p.alive
            && !p.sleeping
            && !attacking_or_interacting
            && !bow_drawing
    }

    /// One logic tick on the room data: the hands whose binding to press
    /// (`backpackMainHandKeybind` and `backpackOffhandKeybind`), with a haptic pulse.
    pub fn tick(&mut self, room: &VrData) -> [Option<Haptic>; 2] {
        let mut out = [None; 2];
        let rear = room.head_rear();
        let hmd_dir = room.hmd().direction();
        for c in 0..2 {
            let pos = room.controller(c).position();
            let dir = room.hand(c).direction();
            let delta = (rear - pos).as_vec3();
            let dot = dir.dot(Vec3::NEG_Y);
            let dot_delta = delta.dot(hmd_dir);
            let below = (rear.y - pos.y).abs() < 0.25;
            let behind = dot_delta > 0.0 && delta.length() > 0.05;
            let aim_down = dot > 0.6;
            let in_front = dot_delta < 0.0 && delta.length() > 0.25;
            let aim_up = dot < 0.0;
            if below && behind && aim_down {
                if !self.was_in[c] {
                    out[c] = Some(Haptic {
                        controller: c,
                        micros: 1500,
                    });
                    self.was_in[c] = true;
                }
            } else if in_front || aim_up {
                self.was_in[c] = false;
            }
        }
        out
    }
}

/// `BowTracker`: drawing a bow by the distance between the hands.
#[derive(Clone, Copy, Debug, Default)]
pub struct BowTracker {
    pub start_draw_ms: u64,
    was_drawing: bool,
    can_draw: bool,
    current_draw: f32,
    max_draw: f32,
    aim: Vec3,
    ts_notch_ms: f32,
    hap_counter: u32,
    last_hap_step: i32,
}

const MAX_DRAW_MS: u64 = 1100;
const NOTCH_DOT_THRESHOLD: f64 = 20.0;

/// What the game knows for a bow tick.
#[derive(Clone, Copy, Debug)]
pub struct BowInput {
    pub now_ms: u64,
    /// The bow in the main hand (else the off hand's), and ammo is found (the main hand's bow
    /// finds its own; the off hand's needs an arrow in the main hand).
    pub bow_in_main: bool,
    pub has_ammo: bool,
    /// The use binding is held (`bowModule.isPressed`).
    pub drawing: bool,
    /// The player's bounding box height, divided by its scale.
    pub bb_height: f32,
    pub bb_scale: f32,
    /// `reverseShootingEye` and the server supports reversed bows.
    pub reversed: bool,
    /// The item's use duration in ticks (72000 for a bow).
    pub use_duration: i32,
}

/// What the bow tracker asks of the game in a frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BowOutput {
    pub haptics: [Option<u32>; 2],
    /// `vivecraft$setItemInUseRemainingClient`: the item's use ticks left, for the pose.
    pub item_in_use_remaining: Option<i32>,
}

impl BowTracker {
    pub fn aim(&self) -> Vec3 {
        self.aim
    }
    pub fn draw_percent(&self) -> f32 {
        if self.max_draw == 0.0 {
            0.0
        } else {
            self.current_draw / self.max_draw
        }
    }
    pub fn is_notched(&self, drawing: bool) -> bool {
        self.can_draw || drawing
    }
    pub fn is_charged(&self, now_ms: u64) -> bool {
        now_ms.saturating_sub(self.start_draw_ms) >= MAX_DRAW_MS
    }

    /// `BowTracker.isBow`.
    pub fn is_bow(
        s: &VrSettings,
        is_vanilla_bow: bool,
        use_animation: UseAnimation,
        excluded: bool,
    ) -> bool {
        match s.bow_mode() {
            BowMode::Off => false,
            BowMode::Vanilla => is_vanilla_bow,
            BowMode::On => use_animation == UseAnimation::Bow && !excluded,
        }
    }

    pub fn inactive(&mut self) {
        self.was_drawing = false;
        self.can_draw = false;
    }

    /// One frame on the world data (`activeProcess`).
    pub fn frame(&mut self, world: &VrData, input: BowInput) -> BowOutput {
        let mut out = BowOutput::default();
        let last_can_draw = self.can_draw;
        self.max_draw = input.bb_height * 0.22 / input.bb_scale;
        let (bow_hand, arrow_hand) = if input.reversed { (0, 1) } else { (1, 0) };
        let arrow_pos = world.controller(arrow_hand).position();
        let bow_pos = world.controller(bow_hand).position();
        let controllers_dist = bow_pos.distance(arrow_pos) as f32;
        let up = Vec3::new(0.0, world.world_scale, 0.0);
        let string_pos =
            bow_pos + (world.hand(bow_hand).custom_vector(up) * (self.max_draw * 0.5)).as_dvec3();
        let notch_dist = arrow_pos.distance(string_pos);
        self.aim = (bow_pos - arrow_pos).as_vec3().normalize_or_zero();
        let arrow_aim = world.controller(arrow_hand).custom_vector(Vec3::NEG_Z);
        let bow_aim = world.hand(bow_hand).custom_vector(Vec3::NEG_Y);
        let dot_rad = bow_aim.dot(arrow_aim);
        let dot_deg = if dot_rad > 0.999 {
            0.0
        } else {
            f64::from(dot_rad.acos()).to_degrees()
        };
        let notch_threshold = 0.15 * world.world_scale;
        let stage0 = input.use_duration;
        let stage1 = input.use_duration - 15;
        if input.has_ammo
            && notch_dist <= f64::from(notch_threshold)
            && dot_deg <= NOTCH_DOT_THRESHOLD
        {
            self.can_draw = true;
            self.ts_notch_ms = input.now_ms as f32;
            if !input.drawing {
                out.item_in_use_remaining = Some(stage0);
            }
        } else if !input.drawing && input.now_ms as f32 - self.ts_notch_ms > 500.0 {
            self.can_draw = false;
        }
        if input.drawing && !self.was_drawing {
            self.start_draw_ms = input.now_ms;
        }
        if !input.drawing && self.can_draw && !last_can_draw {
            out.haptics[arrow_hand] = Some(800);
            out.haptics[bow_hand] = Some(800);
        }
        if input.drawing {
            self.current_draw =
                ((controllers_dist - notch_threshold) / world.world_scale).min(self.max_draw);
            let percent = f64::from(self.draw_percent());
            let strength = if percent > 0.0 {
                (percent * 500.0) as u32 + 700
            } else {
                0
            };
            out.item_in_use_remaining = Some(if percent >= 1.0 {
                0
            } else if percent > 0.4 {
                stage1
            } else {
                stage0
            });
            let step = (percent * 4.0 * 4.0 * 3.0) as i32;
            if step % 2 == 0 && self.last_hap_step != step {
                out.haptics[arrow_hand] = Some(strength);
                if percent == 1.0 {
                    out.haptics[bow_hand] = Some(strength);
                }
            }
            if self.is_charged(input.now_ms) && self.hap_counter % 4 == 0 {
                out.haptics[bow_hand] = Some(200);
            }
            self.last_hap_step = step;
            self.hap_counter += 1;
        } else {
            self.hap_counter = 0;
            self.last_hap_step = 0;
        }
        self.was_drawing = input.drawing;
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vr_data::{DataSettings, RoomPose, RoomPoses};
    use glam::{DVec3, Quat};

    fn room(main: Vec3, off: Vec3, off_rot: Quat) -> VrData {
        let head = RoomPose {
            pos: Vec3::new(0.0, 1.6, 0.0),
            rot: Quat::IDENTITY,
        };
        let aim = [
            RoomPose {
                pos: main,
                rot: Quat::IDENTITY,
            },
            RoomPose {
                pos: off,
                rot: off_rot,
            },
        ];
        let room = RoomPoses {
            hmd: head,
            eyes: [head; 2],
            aim,
            hand: aim,
            tracking: [true; 2],
        };
        VrData::new(&room, DataSettings::default(), DVec3::ZERO, 1.0, 1.0, 0.0)
    }

    fn bow_input(now: u64, drawing: bool) -> BowInput {
        BowInput {
            now_ms: now,
            bow_in_main: false,
            has_ammo: true,
            drawing,
            bb_height: 1.8,
            bb_scale: 1.0,
            reversed: false,
            use_duration: 72000,
        }
    }

    #[test]
    fn a_bow_nocks_near_the_string_and_draws_by_the_hands_distance() {
        let mut t = BowTracker::default();
        // The bow hand (off) points along -Z with the grip's -Y... aligned with the arrow
        // hand (main): string 0.2 m from the bow hand along its up axis, arrow hand there.
        let bow_rot = Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
        let bow_hand = Vec3::new(0.0, 1.3, -0.5);
        let string = bow_hand + bow_rot * Vec3::Y * (1.8 * 0.22 * 0.5);
        let held = room(string, bow_hand, bow_rot);
        let out = t.frame(&held, bow_input(1000, false));
        assert!(t.is_notched(false), "not nocked, aim {:?}", t.aim());
        assert_eq!(out.haptics, [Some(800), Some(800)]);
        assert_eq!(out.item_in_use_remaining, Some(72000));
        // Drawing: pull the arrow hand back 0.3 m.
        let pulled = room(string + Vec3::new(0.0, 0.0, 0.3), bow_hand, bow_rot);
        let out = t.frame(&pulled, bow_input(1100, true));
        assert!(t.draw_percent() > 0.0);
        assert!(out.item_in_use_remaining.is_some());
        assert_eq!(t.start_draw_ms, 1100);
        assert!(!t.is_charged(1500));
        assert!(t.is_charged(2300));
        // The aim runs from the arrow hand through the bow hand.
        assert!(t.aim().z < 0.0);
    }

    #[test]
    fn no_ammo_means_no_nock_and_the_notch_lapses() {
        let mut t = BowTracker::default();
        let rot = Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
        let hand = Vec3::new(0.0, 1.3, -0.5);
        let r = room(hand + rot * Vec3::Y * 0.198, hand, rot);
        t.frame(
            &r,
            BowInput {
                has_ammo: false,
                ..bow_input(0, false)
            },
        );
        assert!(!t.is_notched(false));
        t.frame(&r, bow_input(100, false));
        assert!(t.is_notched(false));
        // Far away for over half a second.
        let far = room(Vec3::new(0.5, 1.0, 0.0), hand, rot);
        t.frame(&far, bow_input(300, false));
        assert!(t.is_notched(false));
        t.frame(&far, bow_input(700, false));
        assert!(!t.is_notched(false));
    }

    #[test]
    fn bow_modes_pick_the_bows() {
        let mut s = VrSettings::default();
        assert!(BowTracker::is_bow(&s, false, UseAnimation::Bow, false));
        assert!(!BowTracker::is_bow(&s, false, UseAnimation::Bow, true));
        s.set("bowMode", "VANILLA");
        assert!(!BowTracker::is_bow(&s, false, UseAnimation::Bow, false));
        assert!(BowTracker::is_bow(&s, true, UseAnimation::Other, false));
        s.set("bowMode", "OFF");
        assert!(!BowTracker::is_bow(&s, true, UseAnimation::Bow, false));
    }

    #[test]
    fn food_at_the_mouth_is_eaten_once_and_a_drink_needs_tipping() {
        let mut t = EatingTracker::default();
        let mut h = Histories::default();
        let mouth_hand = Vec3::new(0.0, 1.55, 0.1);
        for i in 0..5 {
            h.controller[0].add(i * 50, mouth_hand);
            h.controller[1].add(i * 50, Vec3::new(0.4, 1.0, -0.3));
        }
        // The main controller points along -Z: the food is 0.2 m ahead of its average position.
        let r = room(mouth_hand, Vec3::new(0.4, 1.0, -0.3), Quat::IDENTITY);
        let held = [UseAnimation::Eat, UseAnimation::Other];
        let out = t.tick(&r, &h, 200, held, 20, |c| c == 0);
        assert!(out.use_item[0] && !out.use_item[1]);
        assert!(t.item_in_use());
        // Already eating: no second use; the crunch buzzes.
        let out = t.tick(&r, &h, 300, held, 20, |_| true);
        assert!(!out.use_item[0]);
        assert_eq!(out.haptics[0], Some(700));
        // The meal lasts at most 2.1 s.
        t.tick(&r, &h, 2400, held, 20, |_| true);
        assert!(!t.eating(0));
        // Away from the mouth nothing happens.
        let away = room(
            Vec3::new(0.5, 1.0, -0.3),
            Vec3::new(0.4, 1.0, -0.3),
            Quat::IDENTITY,
        );
        let mut far_h = Histories::default();
        far_h.controller[0].add(0, Vec3::new(0.5, 1.0, -0.3));
        let mut t2 = EatingTracker::default();
        assert!(!t2.tick(&away, &far_h, 100, held, 20, |_| true).use_item[0]);
        // A potion held upright is not drunk.
        let mut t3 = EatingTracker::default();
        let out = t3.tick(
            &r,
            &h,
            200,
            [UseAnimation::Drink, UseAnimation::Other],
            20,
            |_| true,
        );
        assert!(!out.use_item[0]);
    }

    #[test]
    fn reaching_over_the_shoulder_presses_the_backpack_once() {
        let mut t = BackpackTracker::default();
        // Behind the head (+Z), pointing down, at head height minus a bit.
        let mut r = room(
            Vec3::new(0.0, 1.5, 0.3),
            Vec3::new(0.4, 1.0, -0.3),
            Quat::IDENTITY,
        );
        // Point the hand down: its direction (-Z local) must have dot(DOWN) > 0.6.
        let down = Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2);
        r.h[0].rot = down;
        r.c[0].rot = down;
        let out = t.tick(&r);
        assert_eq!(
            out[0],
            Some(Haptic {
                controller: 0,
                micros: 1500
            })
        );
        assert_eq!(out[1], None);
        // Still there: no second press.
        assert_eq!(t.tick(&r)[0], None);
        // Back in front: re-armed.
        let front = room(
            Vec3::new(0.0, 1.4, -0.5),
            Vec3::new(0.4, 1.0, -0.3),
            Quat::IDENTITY,
        );
        t.tick(&front);
        assert!(!t.was_in[0]);
    }
}
