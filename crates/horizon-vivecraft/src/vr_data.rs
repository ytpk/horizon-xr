// SPDX-License-Identifier: LGPL-3.0-or-later
//! Ports `org.vivecraft.client_vr.VRData` (the room-to-world transform of the tracked poses),
//! `Vector3fHistory` and `QuaternionfHistory`.
//! Copyright (c) Vivecraft contributors; LGPL-3.0-or-later (see LICENSE-LGPL-3.0).
//!
//! Room space is the tracking space of the runtime (metres, +Y up, the device faces -Z at
//! identity). `VrData` turns it into world space as Vivecraft does: scale by the world scale,
//! rotate about Y by the world rotation, add the room origin. Skipped: the mixed reality camera,
//! the screenshot camera, telescopes' smoothing, and the full-body trackers (an Oculus Touch
//! setup has none), so `fbt_mode` is always arms only.

use crate::math::{self, BACK};
use crate::network::FbtMode;
use glam::{DVec3, Mat3, Quat, Vec3};
use std::collections::VecDeque;
use std::f32::consts::{FRAC_PI_2, PI};

/// A tracked device in room space (`VRData.VRDevicePose`'s fields).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RoomPose {
    pub pos: Vec3,
    pub rot: Quat,
}

impl RoomPose {
    pub const IDENTITY: Self = Self {
        pos: Vec3::ZERO,
        rot: Quat::IDENTITY,
    };
    /// `MathUtils.BACK` through the rotation: where the device points.
    pub fn dir(&self) -> Vec3 {
        self.rot * BACK
    }
}

/// What `MCVR` knows each frame (room space).
#[derive(Clone, Copy, Debug)]
pub struct RoomPoses {
    pub hmd: RoomPose,
    /// The eyes, left and right (position in room space).
    pub eyes: [RoomPose; 2],
    /// The controllers' aim poses, main and off hand (`c0` and `c1`).
    pub aim: [RoomPose; 2],
    /// The controllers' hand poses (`h0` and `h1`).
    pub hand: [RoomPose; 2],
    /// Whether each controller is tracked (`MCVR.isControllerTracking`).
    pub tracking: [bool; 2],
}

impl Default for RoomPoses {
    fn default() -> Self {
        Self {
            hmd: RoomPose::IDENTITY,
            eyes: [RoomPose::IDENTITY; 2],
            aim: [RoomPose::IDENTITY; 2],
            hand: [RoomPose::IDENTITY; 2],
            tracking: [false; 2],
        }
    }
}

/// The settings `VRData` reads.
#[derive(Clone, Copy, Debug, Default)]
pub struct DataSettings {
    pub seated: bool,
    pub reverse_hands: bool,
    pub aim_with_hmd: bool,
}

/// A device index (`VRData.getController`): 0 is the main hand, 1 the off hand.
pub type Hand = usize;

/// `VRData`: the poses with their room origin, rotation and scale.
#[derive(Clone, Copy, Debug)]
pub struct VrData {
    /// The room origin in the world.
    pub origin: DVec3,
    /// The room's rotation about Y, radians.
    pub rotation: f32,
    /// Positions are scaled by this.
    pub world_scale: f32,
    pub hmd: RoomPose,
    pub eyes: [RoomPose; 2],
    pub c: [RoomPose; 2],
    pub h: [RoomPose; 2],
    pub tracking: [bool; 2],
    pub settings: DataSettings,
}

/// A pose's world-space view: `VRData.VRDevicePose` bound to its data.
#[derive(Clone, Copy)]
pub struct DevicePose<'a> {
    data: &'a VrData,
    pose: &'a RoomPose,
}

impl VrData {
    /// `new VRData(origin, walkMul, worldScale, rotation)`: `walk_mul` scales the head's
    /// horizontal room position (the walking multiplier); the eyes and hands are shifted by
    /// the same offset.
    pub fn new(
        room: &RoomPoses,
        settings: DataSettings,
        origin: DVec3,
        walk_mul: f32,
        world_scale: f32,
        rotation: f32,
    ) -> Self {
        let hmd_raw = (room.eyes[0].pos + room.eyes[1].pos) * 0.5;
        let scaled = Vec3::new(hmd_raw.x * walk_mul, hmd_raw.y, hmd_raw.z * walk_mul);
        let offset = Vec3::new(scaled.x - hmd_raw.x, 0.0, scaled.z - hmd_raw.z);
        let shift = |p: RoomPose| RoomPose {
            pos: p.pos + offset,
            rot: p.rot,
        };
        Self {
            origin,
            rotation,
            world_scale,
            hmd: RoomPose {
                pos: scaled,
                rot: room.hmd.rot,
            },
            eyes: room.eyes.map(shift),
            c: room.aim.map(shift),
            h: room.hand.map(shift),
            tracking: room.tracking,
            settings,
        }
    }

    pub fn pose<'a>(&'a self, pose: &'a RoomPose) -> DevicePose<'a> {
        DevicePose { data: self, pose }
    }
    pub fn hmd(&self) -> DevicePose<'_> {
        self.pose(&self.hmd)
    }
    /// `getController`: the aim pose of the main (0) or off (1) hand.
    pub fn controller(&self, hand: Hand) -> DevicePose<'_> {
        self.pose(&self.c[hand.min(1)])
    }
    /// `getHand`: the hand pose.
    pub fn hand(&self, hand: Hand) -> DevicePose<'_> {
        self.pose(&self.h[hand.min(1)])
    }
    pub fn eye(&self, eye: usize) -> DevicePose<'_> {
        self.pose(&self.eyes[eye.min(1)])
    }
    /// `getAim`: the pose the crosshair aims with.
    pub fn aim(&self) -> DevicePose<'_> {
        if self.settings.aim_with_hmd {
            self.hmd()
        } else {
            self.controller(0)
        }
    }
    pub fn fbt_mode(&self) -> FbtMode {
        FbtMode::ArmsOnly
    }

    /// `getBodyYawRad`.
    pub fn body_yaw_rad(&self) -> f32 {
        let hands = if self.tracking == [true, true] {
            let head = self.hmd().position();
            let rel = |p: DVec3| (p - head).as_vec3();
            Some((
                rel(self.controller(0).position()),
                rel(self.controller(1).position()),
            ))
        } else {
            None
        };
        math::estimate_body_yaw_rad(
            self.settings.seated,
            self.settings.reverse_hands,
            FbtMode::ArmsOnly,
            self.hmd().direction(),
            hands.map(|h| h.0),
            hands.map(|h| h.1),
            || Vec3::ZERO,
        )
    }
    pub fn body_yaw(&self) -> f32 {
        self.body_yaw_rad().to_degrees()
    }

    /// `getFacingYawRad`: the way the hands face.
    pub fn facing_yaw_rad(&self) -> f32 {
        if self.settings.seated {
            return self.hmd().yaw_rad();
        }
        let between = (self.controller(1).position() - self.controller(0).position())
            .as_vec3()
            .normalize_or_zero();
        let arms = rotate_y(between, -FRAC_PI_2);
        if self.settings.reverse_hands {
            arms.x.atan2(-arms.z)
        } else {
            (-arms.x).atan2(arms.z)
        }
    }
    pub fn facing_yaw(&self) -> f32 {
        self.facing_yaw_rad().to_degrees()
    }

    fn head_pivot_offset(&self, scale: f32) -> Vec3 {
        self.hmd().matrix() * Vec3::new(0.0, -0.1 * scale, 0.1 * scale)
    }
    /// `getHeadPivot`: where the head rotates about, in the world.
    pub fn head_pivot(&self) -> DVec3 {
        self.hmd().position() + self.head_pivot_offset(self.world_scale).as_dvec3()
    }
    /// `getNewHeadPivot`: the pivot for another room origin and scale.
    pub fn new_head_pivot(&self, new_origin: DVec3, new_world_scale: f32) -> DVec3 {
        let eye = self.hmd().position() - self.origin + new_origin;
        eye + self.head_pivot_offset(new_world_scale).as_dvec3()
    }
    /// `getHeadRear`: a point behind the player's back.
    pub fn head_rear(&self) -> DVec3 {
        let off =
            self.hmd().matrix() * Vec3::new(0.0, -0.2 * self.world_scale, 0.2 * self.world_scale);
        self.hmd().position() + off.as_dvec3()
    }

    /// `VRPlayer.roomToWorldPos`.
    pub fn room_to_world(&self, pos: Vec3) -> DVec3 {
        self.origin + rotate_y(pos * self.world_scale, self.rotation).as_dvec3()
    }
    /// `VRPlayer.worldToRoomPos`.
    pub fn world_to_room(&self, pos: DVec3) -> Vec3 {
        rotate_y(
            ((pos - self.origin).as_vec3()) / self.world_scale,
            -self.rotation,
        )
    }
}

impl DevicePose<'_> {
    /// The position in the world.
    pub fn position(&self) -> DVec3 {
        let local = rotate_y(self.pose.pos * self.data.world_scale, self.data.rotation);
        self.data.origin + local.as_dvec3()
    }
    /// The position relative to the room origin, scaled and rotated (no origin).
    pub fn position_f(&self) -> Vec3 {
        rotate_y(self.pose.pos * self.data.world_scale, self.data.rotation)
    }
    /// `getScalePositionOffset`: the position change from another world scale.
    pub fn scale_position_offset(&self, new_scale: f32) -> Vec3 {
        let old = rotate_y(self.pose.pos * self.data.world_scale, self.data.rotation);
        let new = rotate_y(self.pose.pos * new_scale, self.data.rotation);
        new - old
    }
    pub fn direction(&self) -> Vec3 {
        rotate_y(self.pose.dir(), self.data.rotation)
    }
    /// Local `axis` in the world (`getCustomVector`).
    pub fn custom_vector(&self, axis: Vec3) -> Vec3 {
        rotate_y(self.pose.rot * axis, self.data.rotation)
    }
    /// Minecraft's yaw (0 faces south, +Z) in radians.
    pub fn yaw_rad(&self) -> f32 {
        let d = self.direction();
        (-d.x).atan2(d.z)
    }
    pub fn yaw(&self) -> f32 {
        self.yaw_rad().to_degrees()
    }
    /// Positive looks up (note: Minecraft's pitch is the negative of this).
    pub fn pitch_rad(&self) -> f32 {
        let d = self.direction();
        (d.y / d.length()).asin()
    }
    pub fn pitch(&self) -> f32 {
        self.pitch_rad().to_degrees()
    }
    pub fn roll_rad(&self) -> f32 {
        let m = Mat3::from_quat(self.pose.rot);
        -m.x_axis.y.atan2(m.y_axis.y)
    }
    pub fn roll(&self) -> f32 {
        self.roll_rad().to_degrees()
    }
    /// The device's orientation in the world.
    pub fn orientation(&self) -> Quat {
        Quat::from_rotation_y(self.data.rotation) * self.pose.rot
    }
    fn matrix(&self) -> Mat3 {
        Mat3::from_quat(self.orientation())
    }
}

/// JOML's `Vector3f.rotateY`.
pub fn rotate_y(v: Vec3, angle: f32) -> Vec3 {
    Quat::from_rotation_y(angle) * v
}

/// `VRPlayer.preRender`'s interpolation of the room between the tick before and after.
pub fn lerp_rotation(pre: f32, post: f32, partial: f32) -> f32 {
    let (mut start, mut end) = (pre, post);
    if (end - start).abs() > PI {
        if end > start {
            start += 2.0 * PI;
        } else {
            end += 2.0 * PI;
        }
    }
    start + (end - start) * partial
}

/// Samples with a time stamp in milliseconds: `Vector3fHistory`.
#[derive(Clone, Debug, Default)]
pub struct Vec3History {
    data: VecDeque<(u64, Vec3)>,
}

const HISTORY_CAPACITY: usize = 450;

impl Vec3History {
    pub fn add(&mut self, now_ms: u64, v: Vec3) {
        if !v.is_finite() {
            return;
        }
        if let Some(last) = self.data.back_mut() {
            if now_ms == last.0 {
                last.1 = v;
                return;
            }
            if now_ms < last.0 {
                self.data.clear();
            }
        }
        self.data.push_back((now_ms, v));
        if self.data.len() > HISTORY_CAPACITY {
            self.data.pop_front();
        }
    }
    pub fn clear(&mut self) {
        self.data.clear();
    }
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
    pub fn latest(&self) -> Vec3 {
        self.data.back().map_or(Vec3::ZERO, |e| e.1)
    }
    /// The samples from the last `seconds`, newest first.
    fn recent(&self, now_ms: u64, seconds: f64) -> impl Iterator<Item = &(u64, Vec3)> {
        self.data
            .iter()
            .rev()
            .take_while(move |e| (now_ms.saturating_sub(e.0)) as f64 <= seconds * 1000.0)
    }
    /// `totalMovement`: the sum of the distances between consecutive samples... of Vivecraft:
    /// every sample's distance to the newest in the period.
    pub fn total_movement(&self, now_ms: u64, seconds: f64) -> f32 {
        let mut it = self.recent(now_ms, seconds);
        let Some(last) = it.next() else {
            return 0.0;
        };
        it.map(|e| last.1.distance(e.1)).sum()
    }
    /// `netMovement`: the newest sample minus the oldest in the period.
    pub fn net_movement(&self, now_ms: u64, seconds: f64) -> Vec3 {
        let mut it = self.recent(now_ms, seconds);
        let Some(last) = it.next() else {
            return Vec3::ZERO;
        };
        match it.last() {
            Some(first) => last.1 - first.1,
            None => Vec3::ZERO,
        }
    }
    /// Actual coverage of a recent window, used before trusting manual calibration.
    pub fn span_ms(&self, now_ms: u64, seconds: f64) -> u64 {
        let mut samples = self.recent(now_ms, seconds);
        let Some(newest) = samples.next() else {
            return 0;
        };
        samples
            .last()
            .map_or(0, |oldest| newest.0.saturating_sub(oldest.0))
    }

    /// `averageSpeed` in metres a second.
    pub fn average_speed(&self, now_ms: u64, seconds: f64) -> f32 {
        let mut total = 0.0;
        let mut count = 0;
        let mut last: Option<&(u64, Vec3)> = None;
        for e in self.recent(now_ms, seconds) {
            if let Some(l) = last {
                let dt = 0.001 * l.0.saturating_sub(e.0) as f32;
                if dt > 0.0 {
                    count += 1;
                    total += l.1.distance(e.1) / dt;
                }
            }
            last = Some(e);
        }
        if count == 0 {
            total
        } else {
            total / count as f32
        }
    }
    /// `averagePosition`.
    pub fn average_position(&self, now_ms: u64, seconds: f64) -> Vec3 {
        let mut sum = Vec3::ZERO;
        let mut n = 0;
        for e in self.recent(now_ms, seconds) {
            sum += e.1;
            n += 1;
        }
        if n == 0 { Vec3::ZERO } else { sum / n as f32 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data(rotation: f32, scale: f32) -> VrData {
        let hmd = RoomPose {
            pos: Vec3::new(0.5, 1.7, -0.25),
            rot: Quat::IDENTITY,
        };
        let room = RoomPoses {
            hmd,
            eyes: [hmd; 2],
            aim: [
                RoomPose {
                    pos: Vec3::new(0.8, 1.2, -0.75),
                    rot: Quat::IDENTITY,
                },
                RoomPose {
                    pos: Vec3::new(0.2, 1.2, -0.75),
                    rot: Quat::IDENTITY,
                },
            ],
            hand: [RoomPose::IDENTITY; 2],
            tracking: [true, true],
        };
        VrData::new(
            &room,
            DataSettings::default(),
            DVec3::new(10.0, 64.0, 20.0),
            1.0,
            scale,
            rotation,
        )
    }

    #[test]
    fn history_coalesces_duplicate_times_and_keeps_long_uptime_speed_finite() {
        let mut h = Vec3History::default();
        let t = 100_000_000;
        h.add(t, Vec3::ZERO);
        h.add(t, Vec3::X * 0.01);
        h.add(t + 5, Vec3::X * 0.015);
        assert!((h.average_speed(t + 5, 0.5) - 1.0).abs() < 1e-5);
        h.add(t + 10, Vec3::splat(f32::NAN));
        assert!(h.latest().is_finite());
        h.add(0, Vec3::Y);
        assert_eq!(h.net_movement(0, 0.5), Vec3::ZERO);
    }

    #[test]
    fn room_and_world_round_trip_with_scale_and_rotation() {
        let d = data(0.7, 2.0);
        let p = Vec3::new(0.4, 1.0, -0.9);
        let back = d.world_to_room(d.room_to_world(p));
        assert!((back - p).length() < 1e-5, "{back}");
        // The head's world position is the room position through the same map.
        assert!((d.hmd().position() - d.room_to_world(d.hmd.pos)).length() < 1e-9);
    }

    #[test]
    fn a_device_facing_minus_z_has_yaw_180_and_turns_with_the_room() {
        let d = data(0.0, 1.0);
        assert!((d.hmd().yaw().abs() - 180.0).abs() < 1e-3);
        // Rotating the room a quarter turn left points the head along -X: yaw 90.
        let d = data(FRAC_PI_2, 1.0);
        assert!((d.hmd().yaw() - 90.0).abs() < 1e-3, "{}", d.hmd().yaw());
        assert!(d.hmd().pitch().abs() < 1e-3);
    }

    #[test]
    fn the_head_pivot_sits_behind_and_below_the_eyes_scaled_by_the_world() {
        let d = data(0.0, 2.0);
        let eye = d.hmd().position();
        let pivot = d.head_pivot();
        assert!((pivot.y - (eye.y - 0.2)).abs() < 1e-6);
        assert!((pivot.z - (eye.z + 0.2)).abs() < 1e-6);
        // Moving the room moves the pivot with it.
        let moved = d.new_head_pivot(d.origin + DVec3::new(1.0, 0.0, 0.0), 2.0);
        assert!((moved.x - pivot.x - 1.0).abs() < 1e-9);
    }

    #[test]
    fn the_scale_offset_is_the_position_change() {
        let d = data(0.3, 1.0);
        let off = d.hmd().scale_position_offset(3.0);
        let want = rotate_y(d.hmd.pos * 2.0, 0.3);
        assert!((off - want).length() < 1e-5);
    }

    #[test]
    fn hands_in_front_of_a_head_face_the_body_the_way_the_head_does() {
        let d = data(0.0, 1.0);
        assert!((d.body_yaw().abs() - 180.0).abs() < 1.0, "{}", d.body_yaw());
    }

    #[test]
    fn rotation_interpolates_the_short_way() {
        let r = lerp_rotation(3.0, -3.0, 0.5);
        // Halfway between 3 and -3 rad the short way is across pi.
        assert!((r.abs() - PI).abs() < 0.2 || (r - PI).abs() < 0.2, "{r}");
        assert!((lerp_rotation(0.0, 1.0, 0.25) - 0.25).abs() < 1e-6);
    }

    #[test]
    fn history_measures_the_recent_samples_only() {
        let mut h = Vec3History::default();
        for i in 0..10u64 {
            h.add(i * 100, Vec3::new(i as f32 * 0.1, 0.0, 0.0));
        }
        // 1 m/s along x over the last half second.
        assert!((h.average_speed(900, 0.5) - 1.0).abs() < 1e-4);
        assert!((h.net_movement(900, 0.5).x - 0.5).abs() < 1e-4);
        assert!(h.total_movement(900, 0.25) > 0.2);
        assert!((h.average_position(900, 0.05).x - 0.9).abs() < 1e-5);
        assert_eq!(Vec3History::default().latest(), Vec3::ZERO);
    }
}
