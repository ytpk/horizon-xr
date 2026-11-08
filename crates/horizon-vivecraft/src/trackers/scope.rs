// SPDX-License-Identifier: LGPL-3.0-or-later
//! Ports `TelescopeTracker` (a spyglass held to the eye is looked through) and `RowTracker`
//! (paddling a boat with the controllers).
//! Copyright (c) Vivecraft contributors; LGPL-3.0-or-later (see LICENSE-LGPL-3.0).

use super::PlayerFlags;
use crate::player::Histories;
use crate::settings::VrSettings;
use crate::vr_data::VrData;
use glam::Vec3;

const LENS_DIST_MAX: f32 = 0.05;
const LENS_DIST_MIN: f32 = 0.185;
const LENS_DOT_MAX: f32 = 0.9;
const LENS_DOT_MIN: f32 = 0.75;

/// Where a controller's lens is: behind the controller along its view axis, a little ahead
/// of it (`getLensOrigin`). The view axis is the controller's -Y.
fn lens_origin(room: &VrData, controller: usize) -> Vec3 {
    let c = room.controller(controller);
    c.position_f() + (view_vector(room, controller) * -0.2 + c.direction() * 0.05)
}

fn view_vector(room: &VrData, controller: usize) -> Vec3 {
    room.controller(controller).custom_vector(Vec3::NEG_Y)
}

/// `viewPercent(controller, pass)`: how well an eye lines up with the lens, 0..1.
pub fn eye_view_percent(room: &VrData, controller: usize, eye: usize) -> f32 {
    let eye_pose = room.eye(eye);
    let dist = (eye_pose.position_f() - lens_origin(room, controller)).length();
    let dot = eye_pose
        .direction()
        .dot(view_vector(room, controller))
        .abs();
    let dot_fact = if dot > LENS_DOT_MIN {
        if dot > LENS_DOT_MAX {
            1.0
        } else {
            (dot - LENS_DOT_MIN) / (LENS_DOT_MAX - LENS_DOT_MIN)
        }
    } else {
        0.0
    };
    let dist_fact = if dist < LENS_DIST_MIN {
        if dist < LENS_DIST_MAX {
            1.0
        } else {
            1.0 - (dist - LENS_DIST_MAX) / (LENS_DIST_MIN - LENS_DIST_MAX)
        }
    } else {
        0.0
    };
    dot_fact.min(dist_fact)
}

/// `viewPercent(controller)` standing: the better of the two eyes. Seated there is no fade:
/// 1 while the spyglass is in use.
pub fn view_percent(s: &VrSettings, room: &VrData, controller: usize, in_use: bool) -> f32 {
    if s.bool("renderAllPasses") {
        return 1.0;
    }
    if s.seated() {
        return if in_use { 1.0 } else { 0.0 };
    }
    eye_view_percent(room, controller, 0).max(eye_view_percent(room, controller, 1))
}

/// `TelescopeTracker`.
#[derive(Clone, Copy, Debug, Default)]
pub struct TelescopeTracker {
    viewing: [bool; 2],
}

impl TelescopeTracker {
    pub fn item_in_use(&self) -> bool {
        self.viewing[0] || self.viewing[1]
    }
    pub fn viewing(&self, hand: usize) -> bool {
        self.viewing[hand.min(1)]
    }

    pub fn is_active(
        &self,
        p: &PlayerFlags,
        bow_active: bool,
        telescope_in_hand: [bool; 2],
    ) -> bool {
        p.in_game && !bow_active && (telescope_in_hand[0] || telescope_in_hand[1])
    }

    pub fn inactive(&mut self) {
        self.viewing = [false; 2];
    }

    /// One logic tick: the hands whose spyglass starts to be used (`useItem`); a hand keeps
    /// the use going while it looks through, and drops it when it does not.
    pub fn tick(
        &mut self,
        s: &VrSettings,
        room: &VrData,
        telescope_in_hand: [bool; 2],
        in_use: [bool; 2],
    ) -> [bool; 2] {
        let mut start = [false; 2];
        for c in 0..2 {
            if telescope_in_hand[c] && view_percent(s, room, c, in_use[c]) > 0.0 {
                start[c] = !self.viewing[c];
                self.viewing[c] = true;
            } else {
                self.viewing[c] = false;
            }
        }
        start
    }
}

/// `RowTracker`: the oar strength of each controller while in a boat.
#[derive(Clone, Copy, Debug, Default)]
pub struct RowTracker {
    /// Left, right and both oars (`LOar`, `ROar`, `FOar`).
    pub left: f32,
    pub right: f32,
    pub both: f32,
}

impl RowTracker {
    pub fn is_active(
        &self,
        s: &VrSettings,
        p: &PlayerFlags,
        in_boat: bool,
        forward_key: bool,
        bow_notched: bool,
    ) -> bool {
        !s.seated()
            && s.bool("realisticRowEnabled")
            && p.alive
            && p.in_game
            && !forward_key
            && in_boat
            && !bow_notched
    }

    pub fn is_rowing(&self) -> bool {
        self.right + self.left + self.both > 0.0
    }

    pub fn inactive(&mut self) {
        *self = Self::default();
    }

    /// One logic tick (`activeProcess`): the speed of each controller over half a second,
    /// above 0.5 m/s, up to 2 m/s; both together row straight.
    pub fn tick(&mut self, h: &Histories, now_ms: u64) {
        let (min, max) = (0.5, 2.0);
        let c0 = h.controller[0].average_speed(now_ms, 0.5);
        let c1 = h.controller[1].average_speed(now_ms, 0.5);
        self.right = (c0 - min).max(0.0);
        self.left = (c1 - min).max(0.0);
        self.both = if self.right > 0.0 && self.left > 0.0 {
            (self.right + self.left) / 2.0
        } else {
            0.0
        };
        self.both = self.both.min(max);
        self.right = self.right.min(max);
        self.left = self.left.min(max);
    }
}

/// What Vivecraft's `AbstractBoatMixin` changes in `controlBoat` for a boat the VR player
/// rows or steers.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BoatSteer {
    /// The boat is turned to this yaw (point to move).
    pub yaw: Option<f32>,
    /// Added to the boat's `deltaRotation`.
    pub delta_rotation: f32,
    /// Replaces the acceleration.
    pub acceleration: Option<f32>,
    /// Paddle flags set by the rowing.
    pub left: bool,
    pub right: bool,
    pub up: bool,
}

/// `AbstractBoatMixin.vivecraft$modifyAcceleration` (standing mode only): with forward held the
/// boat goes where the free-move controller (or head) points (`point_yaw`), with tank
/// controls when `vehicleRotation` is on; else rowing turns it by the oar strengths.
/// `delta_rotation` is the boat's current one.
pub fn steer_boat(
    vehicle_rotation: bool,
    input_up: bool,
    point_yaw: f32,
    boat_yaw: f32,
    delta_rotation: f32,
    row: &RowTracker,
) -> BoatSteer {
    let mut out = BoatSteer::default();
    if input_up {
        if vehicle_rotation {
            let mut end = boat_yaw % 360.0;
            let mut start = point_yaw;
            let diff = (end - start).abs();
            if diff > 180.0 {
                if end > start {
                    start += 360.0;
                } else {
                    end += 360.0;
                }
            }
            let diff = end - start;
            out.acceleration = Some(if diff.abs() < 30.0 {
                0.04
            } else if diff.abs() > 150.0 {
                -0.005
            } else if diff != 0.0 {
                0.005
            } else {
                0.0
            });
            if diff.abs() > 10.0 && diff.abs() < 150.0 {
                out.delta_rotation =
                    -diff.signum() * ((diff.abs() - 25.0).max(0.0) / 40.0).min(1.0);
            }
        } else {
            out.yaw = Some(point_yaw);
        }
    } else if row.is_rowing() {
        out.delta_rotation = row.left / 1.5 - row.right / 1.5;
        let total = delta_rotation + out.delta_rotation;
        out.left = total < 0.0;
        out.right = total > 0.0;
        let acceleration = (0.06 * row.both).min(0.04);
        out.acceleration = Some(acceleration);
        out.up = acceleration > 0.0;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vr_data::{DataSettings, RoomPose, RoomPoses};
    use glam::{DVec3, Quat};

    /// A spyglass in the right hand, its view axis (-Y) pointing back along +Z... the lens is
    /// 0.2 m behind the controller along -view, i.e. toward the eyes when the controller is
    /// held with its -Y toward the face.
    fn room(hand: Vec3, hand_rot: Quat) -> VrData {
        let head = RoomPose {
            pos: Vec3::new(0.0, 1.7, 0.0),
            rot: Quat::IDENTITY,
        };
        let aim = [
            RoomPose {
                pos: hand,
                rot: hand_rot,
            },
            RoomPose::IDENTITY,
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

    #[test]
    fn a_spyglass_at_the_eye_is_looked_through_and_one_at_the_hip_is_not() {
        let s = VrSettings::default();
        // The controller's -Y axis along the eye's view (-Z): rotate X by +90 degrees takes
        // -Y to -Z. The lens origin is 0.2 m behind it (+Z) plus 0.05 along its direction.
        let rot = Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
        let eye = Vec3::new(0.0, 1.7, 0.0);
        // Put the controller so the lens (hand + 0.2 back) sits on the eye.
        let at_eye = room(eye + Vec3::new(0.0, 0.0, -0.2), rot);
        let up = view_percent(&s, &at_eye, 0, false);
        assert!(up > 0.0, "{up}");
        let hip = room(Vec3::new(0.3, 0.9, -0.2), rot);
        assert_eq!(view_percent(&s, &hip, 0, false), 0.0);
        // Seated: only while in use.
        let mut seated = VrSettings::default();
        seated.set_bool("seated", true);
        assert_eq!(view_percent(&seated, &hip, 0, true), 1.0);
        assert_eq!(view_percent(&seated, &hip, 0, false), 0.0);
    }

    #[test]
    fn looking_through_starts_the_use_once() {
        let s = VrSettings::default();
        let rot = Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
        let looking = room(Vec3::new(0.0, 1.7, -0.2), rot);
        let mut t = TelescopeTracker::default();
        assert_eq!(
            t.tick(&s, &looking, [true, false], [false; 2]),
            [true, false]
        );
        assert!(t.item_in_use());
        assert_eq!(
            t.tick(&s, &looking, [true, false], [true, false]),
            [false, false]
        );
        let away = room(Vec3::new(0.3, 0.9, -0.2), rot);
        t.tick(&s, &away, [true, false], [true, false]);
        assert!(!t.item_in_use());
    }

    #[test]
    fn rowing_is_the_speed_over_the_minimum_capped() {
        let mut h = Histories::default();
        // Right controller 3 m/s, left 1 m/s.
        for i in 0..10u64 {
            h.controller[0].add(i * 50, Vec3::new(i as f32 * 0.15, 0.0, 0.0));
            h.controller[1].add(i * 50, Vec3::new(i as f32 * 0.05, 0.0, 0.0));
        }
        let mut t = RowTracker::default();
        t.tick(&h, 450);
        assert_eq!(t.right, 2.0);
        assert!((t.left - 0.5).abs() < 1e-3, "{}", t.left);
        // The oars average before they are capped: (2.5 + 0.5) / 2.
        assert!((t.both - 1.5).abs() < 1e-3, "{}", t.both);
        assert!(t.is_rowing());
        t.inactive();
        assert!(!t.is_rowing());
    }

    #[test]
    fn rowing_both_oars_goes_straight_and_one_oar_turns() {
        let both = RowTracker {
            left: 1.0,
            right: 1.0,
            both: 1.0,
        };
        let s = steer_boat(false, false, 0.0, 0.0, 0.0, &both);
        assert_eq!(s.acceleration, Some(0.04));
        assert!(s.up && !s.left && !s.right);
        let one = RowTracker {
            left: 0.0,
            right: 1.5,
            both: 0.0,
        };
        let s = steer_boat(false, false, 0.0, 0.0, 0.0, &one);
        assert_eq!(s.acceleration, Some(0.0));
        assert!((s.delta_rotation + 1.0).abs() < 1e-6 && s.left && !s.up);
    }

    #[test]
    fn forward_points_the_boat_or_uses_tank_controls() {
        let idle = RowTracker::default();
        let s = steer_boat(false, true, 90.0, 0.0, 0.0, &idle);
        assert_eq!(s.yaw, Some(90.0));
        // Tank controls: facing roughly the target accelerates, away brakes, in between creeps.
        let s = steer_boat(true, true, 10.0, 0.0, 0.0, &idle);
        assert_eq!(s.acceleration, Some(0.04));
        let s = steer_boat(true, true, 180.0, 0.0, 0.0, &idle);
        assert_eq!(s.acceleration, Some(-0.005));
        let s = steer_boat(true, true, 90.0, 0.0, 0.0, &idle);
        assert_eq!(s.acceleration, Some(0.005));
        assert!(s.delta_rotation > 0.0);
        // Not rowing and not forward: nothing changes.
        assert_eq!(
            steer_boat(false, false, 0.0, 0.0, 0.0, &idle),
            BoatSteer::default()
        );
    }
}
