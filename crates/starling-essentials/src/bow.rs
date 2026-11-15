// SPDX-License-Identifier: LGPL-3.0-only
// Copyright (c) 2025-2026 Phoenix-Ra and VisorEssentials contributors.
//! BowItemTask.java, translated to an explicit, monotonic-clock state machine.
use glam::{Quat, Vec3};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Action {
    Start,
    Shoot(f32),
    Cancel,
}
#[derive(Clone, Copy, Debug)]
pub struct Input {
    pub now: u64,
    pub holder: usize,
    pub hands: [Vec3; 2],
    pub grip: Quat,
    pub height: f32,
    pub scale: f32,
    pub arrow_in_other_hand: bool,
    pub pressed: bool,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Output {
    pub action: Option<Action>,
    pub haptics: [u32; 2],
}
#[derive(Default, Debug)]
pub struct Bow {
    pub drawing: bool,
    pub notched: bool,
    pub holder: usize,
    pub draw: f32,
    pressed: bool,
    last_near: Option<u64>,
    last_shoot: Option<u64>,
    last_step: u32,
}
impl Bow {
    pub fn clear(&mut self) -> Option<Action> {
        let action = self.drawing.then_some(Action::Cancel);
        let last_shoot = self.last_shoot;
        let pressed = self.pressed;
        let holder = self.holder;
        *self = Self {
            last_shoot,
            pressed,
            holder,
            ..Default::default()
        };
        action
    }
    pub fn frame(&mut self, i: Input) -> Output {
        let mut out = Output::default();
        if i.holder > 1
            || !i.scale.is_finite()
            || i.scale <= 0.0
            || !i.height.is_finite()
            || i.height <= 0.0
            || i.hands.iter().any(|p| !p.is_finite())
            || !i.grip.is_finite()
        {
            out.action = self.clear();
            return out;
        }
        if self.drawing && self.holder != i.holder {
            out.action = self.clear();
            return out;
        }
        self.holder = i.holder;
        let other = 1 - i.holder;
        let was_notched = self.notched;
        let maximum = i.height * 0.22;
        let near = i.hands[i.holder] + i.grip * Vec3::Y * (i.scale * maximum * 0.5);
        if i.arrow_in_other_hand && i.hands[other].distance(near) <= 0.2 * i.scale {
            self.last_near = Some(i.now);
            self.notched = true;
        } else if self.last_near.is_none_or(|t| i.now.saturating_sub(t) > 250) {
            self.notched = false;
        }
        if !self.drawing && self.notched && i.pressed && !self.pressed {
            self.drawing = true;
            out.action = Some(Action::Start);
            out.haptics = [800; 2];
        }
        // Java releases using the preceding frame's draw, before updating geometry.
        if self.drawing && !i.pressed {
            if self.pressed && self.draw >= 0.1 {
                out.action = Some(Action::Shoot(self.draw));
                out.haptics[i.holder] = 3000;
                out.haptics[other] = 500;
                self.last_shoot = Some(i.now);
            } else {
                out.action = Some(Action::Cancel);
            }
            self.drawing = false;
            self.draw = 0.0;
            self.last_step = 0;
        }
        self.pressed = i.pressed;
        if !self.drawing && self.notched && !was_notched {
            out.haptics = [800; 2];
        }
        if self.drawing {
            let distance = if self
                .last_shoot
                .is_none_or(|t| i.now.saturating_sub(t) > 500)
            {
                i.hands[0].distance(i.hands[1])
            } else {
                0.0
            };
            self.draw = (((distance - 0.2 * i.scale) / i.scale) / maximum).clamp(0.0, 1.0);
            let step = (self.draw * 10.0) as u32;
            if step % 2 == 0 && step != self.last_step {
                let pulse = if self.draw > 0.0 {
                    (self.draw * 500.0) as u32 + 700
                } else {
                    0
                };
                out.haptics[other] = pulse;
                if self.draw == 1.0 {
                    out.haptics[i.holder] = pulse;
                }
            }
            self.last_step = step;
        } else {
            self.last_step = 0;
        }
        out
    }
}

/// BowItemPose.applyPose: gun angle is authored in degrees and the roll is mirrored.
pub fn item_pose(main: bool, gun_angle: f32) -> glam::Mat4 {
    let sign = if main { 1.0 } else { -1.0 };
    glam::Mat4::from_translation(Vec3::new(sign * 0.1, 0.0, -0.1))
        * glam::Mat4::from_quat(
            Quat::from_rotation_z((sign * -10.0f32).to_radians())
                * Quat::from_rotation_x((gun_angle - 67.0).to_radians()),
        )
        * glam::Mat4::from_scale(Vec3::splat(1.4))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn input(now: u64, distance: f32, pressed: bool) -> Input {
        Input {
            now,
            holder: 0,
            hands: [Vec3::ZERO, Vec3::new(distance, 1.8 * 0.22 * 0.5, 0.0)],
            grip: Quat::IDENTITY,
            height: 1.8,
            scale: 1.0,
            arrow_in_other_hand: true,
            pressed,
        }
    }
    #[test]
    fn actual_distance_controls_shot_and_short_draw_cancels() {
        let mut b = Bow::default();
        b.frame(input(0, 0.0, false));
        assert_eq!(b.frame(input(1, 0.0, true)).action, Some(Action::Start));
        b.frame(input(100, 0.7, true));
        assert_eq!(b.draw, 1.0);
        assert_eq!(
            b.frame(input(101, 0.7, false)).action,
            Some(Action::Shoot(1.0))
        );
        b.frame(input(700, 0.0, false));
        b.frame(input(701, 0.0, true));
        assert_eq!(b.frame(input(702, 0.0, false)).action, Some(Action::Cancel));
    }
    #[test]
    fn either_hand_works_and_nocking_requires_an_arrow_and_a_fresh_press() {
        let mut b = Bow::default();
        let mut i = input(0, 0.0, true);
        i.arrow_in_other_hand = false;
        assert_eq!(b.frame(i).action, None);
        i.now = 1;
        i.arrow_in_other_hand = true;
        assert_eq!(b.frame(i).action, None);
        i.now = 2;
        i.pressed = false;
        b.frame(i);
        i.now = 3;
        i.pressed = true;
        assert_eq!(b.frame(i).action, Some(Action::Start));
        i.holder = 1;
        assert_eq!(b.frame(i).action, Some(Action::Cancel));
        i.pressed = false;
        i.hands.swap(0, 1);
        b.frame(i);
        i.pressed = true;
        assert_eq!(b.frame(i).action, Some(Action::Start));
        assert_eq!(b.holder, 1);
    }
    #[test]
    fn world_scale_does_not_change_physical_draw_and_cooldown_blocks_fast_shots() {
        let mut a = Bow::default();
        let mut b = Bow::default();
        for i in [
            input(0, 0.0, false),
            input(1, 0.0, true),
            input(100, 0.4, true),
        ] {
            a.frame(i);
            let mut j = i;
            j.scale = 2.0;
            j.hands = j.hands.map(|p| p * 2.0);
            b.frame(j);
        }
        assert!((a.draw - b.draw).abs() < 1e-6);
        a.frame(input(101, 0.4, false));
        a.frame(input(110, 0.0, false));
        a.frame(input(111, 0.0, true));
        a.frame(input(150, 0.7, true));
        assert_eq!(a.draw, 0.0);
        assert_eq!(a.frame(input(151, 0.7, false)).action, Some(Action::Cancel));
    }
}
