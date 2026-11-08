// SPDX-License-Identifier: LGPL-3.0-or-later
//! Ports parts of `org.vivecraft.common.utils.MathUtils`.
//! Copyright (c) Vivecraft contributors; LGPL-3.0-or-later (see LICENSE-LGPL-3.0).

use crate::network::FbtMode;
use glam::{Quat, Vec3};
use std::f32::consts::{FRAC_PI_2, PI, TAU};

pub const FORWARD: Vec3 = Vec3::new(0.0, 0.0, 1.0);
/// A device points along its local -Z.
pub const BACK: Vec3 = Vec3::new(0.0, 0.0, -1.0);
pub const LEFT: Vec3 = Vec3::new(1.0, 0.0, 0.0);
pub const RIGHT: Vec3 = Vec3::new(-1.0, 0.0, 0.0);
pub const UP: Vec3 = Vec3::new(0.0, 1.0, 0.0);
pub const DOWN: Vec3 = Vec3::new(0.0, -1.0, 0.0);

/// Java's `Math.signum`: zero stays zero.
pub fn signum(v: f32) -> f32 {
    if v > 0.0 {
        1.0
    } else if v < 0.0 {
        -1.0
    } else {
        v
    }
}

/// `getPerpendicularVec`.
pub fn perpendicular(v: Vec3) -> Vec3 {
    if (v.dot(UP).abs() / v.length()) != 1.0 {
        v.cross(UP).normalize()
    } else {
        v.cross(LEFT).normalize()
    }
}

/// `lerpMod`.
pub fn lerp_mod(from: f64, to: f64, percent: f64, modulus: f64) -> f64 {
    if (to - from).abs() < modulus / 2.0 {
        from + (to - from) * percent
    } else {
        from + (to - from - (to - from).signum() * modulus) * percent
    }
}

/// `angleDiff`: the signed difference of two angles in degrees.
pub fn angle_diff(a: f32, b: f32) -> f32 {
    let d = (a - b).abs() % 360.0;
    let r = if d > 180.0 { 360.0 - d } else { d };
    let diff = a - b;
    let sign = if (0.0..=180.0).contains(&diff) || (-360.0..=-180.0).contains(&diff) {
        1.0
    } else {
        -1.0
    };
    r * sign
}

/// `angleNormalize`: an angle in the 0..360 range.
pub fn angle_normalize(angle: f32) -> f32 {
    let mut a = angle % 360.0;
    if a < 0.0 {
        a += 360.0;
    }
    a
}

/// `wrapRadians`: into -PI..PI.
pub fn wrap_radians(value: f32) -> f32 {
    let mut w = value % TAU;
    if w >= PI {
        w -= TAU;
    }
    if w < -PI {
        w += TAU;
    }
    w
}

/// `rotLerpRad`.
pub fn rot_lerp_rad(delta: f32, start: f32, end: f32) -> f32 {
    start + delta * wrap_radians(end - start)
}

/// `applyDeadzone`: `[deadzone, 1]` mapped to `[0, 1]`, zero below it.
pub fn apply_deadzone(axis: f32, deadzone: f32) -> f32 {
    if axis.abs() > deadzone {
        let scalar = 1.0 / (1.0 - deadzone);
        (axis.abs() - deadzone) * scalar * signum(axis)
    } else {
        0.0
    }
}

/// `toDigital`: a stick as four digital directions (x right, y forward).
pub fn to_digital(x: f32, y: f32, deadzone: f32) -> (f32, f32) {
    let mut out = (0.0, 0.0);
    if (x * x + y * y).sqrt() > deadzone {
        let angle = x.atan2(y);
        let abs = angle.abs();
        let eighth = PI / 8.0;
        if abs >= eighth && abs <= PI - eighth {
            out.0 = signum(angle);
        }
        if abs < FRAC_PI_2 - eighth {
            out.1 = 1.0;
        } else if abs > FRAC_PI_2 + eighth {
            out.1 = -1.0;
        }
    }
    out
}

/// `normalizedDotXZ`.
pub fn normalized_dot_xz(a: Vec3, b: Vec3) -> f32 {
    (a.x * b.x + a.z * b.z) / ((a.x * a.x + a.z * a.z) * (b.x * b.x + b.z * b.z)).sqrt()
}

/// JOML's `Vector3f.rotateY`.
fn rotate_y(v: Vec3, angle: f32) -> Vec3 {
    let (s, c) = angle.sin_cos();
    Vec3::new(v.x * c + v.z * s, v.y, -v.x * s + v.z * c)
}

/// JOML's `lerp(other, t)`.
fn lerp(a: Vec3, b: Vec3, t: f32) -> Vec3 {
    a + (b - a) * t
}

/// `bodyYawRad`: the body yaw from two controller positions and the head direction.
fn body_yaw_rad(right_hand: Vec3, left_hand: Vec3, head_dir: Vec3) -> f32 {
    // When the hands are in front of the head.
    let dir = left_hand + right_hand;
    let h_dot = normalized_dot_xz(dir, head_dir);
    // When the hands are behind the head: the left controller is on the left side.
    let mut arms_forward = rotate_y(left_hand - right_hand, -FRAC_PI_2);
    // Crossed hands flip.
    if arms_forward.dot(head_dir) < 0.0 {
        arms_forward = -arms_forward;
    }
    // Mix them by how far they are to the side, to avoid jumping.
    let dir = lerp(arms_forward, dir, h_dot.max(0.0));
    // Average with the head direction.
    let dir = lerp(dir.normalize(), head_dir, 0.5);
    (-dir.x).atan2(dir.z)
}

/// Visor's `VRPoseImpl.extractAngles` yaw of a pose (`VRPose.getYaw`, radians, Minecraft's sense:
/// `atan2(-x, z)` of its forward): the head's heading. Looking nearly straight up or down, the
/// heading comes from where the pose's up vector leans, as in Visor.
pub fn head_yaw_rad(orientation: Quat) -> f32 {
    let dir = orientation * Vec3::NEG_Z;
    if dir.x * dir.x + dir.z * dir.z > 0.05 {
        return (-dir.x).atan2(dir.z);
    }
    let mut q = orientation.normalize();
    if q.w < 0.0 {
        q = -q;
    }
    let twist_sq = q.z * q.z + q.w * q.w;
    if twist_sq <= 1e-8 {
        return 0.0;
    }
    let n = twist_sq.sqrt();
    let swing = q * Quat::from_xyzw(0.0, 0.0, -q.z / n, q.w / n);
    let up = swing * Vec3::Y;
    if dir.y >= 0.0 {
        up.x.atan2(-up.z)
    } else {
        (-up.x).atan2(up.z)
    }
}

/// `estimateBodyYawRad`: the body's yaw in radians. `waist_dir` is only called with FBT.
pub fn estimate_body_yaw_rad(
    seated: bool,
    swapped_hands: bool,
    fbt_mode: FbtMode,
    head_direction: Vec3,
    main_hand_pos: Option<Vec3>,
    off_hand_pos: Option<Vec3>,
    waist_dir: impl FnOnce() -> Vec3,
) -> f32 {
    let hands = main_hand_pos.zip(off_hand_pos);
    if seated || (fbt_mode == FbtMode::ArmsOnly && hands.is_none()) {
        (-head_direction.x).atan2(head_direction.z)
    } else if fbt_mode != FbtMode::ArmsOnly {
        let dir = lerp(waist_dir(), head_direction, 0.5);
        (-dir.x).atan2(dir.z)
    } else {
        let (main, off) = hands.unwrap();
        body_yaw_rad(
            if swapped_hands { off } else { main },
            if swapped_hands { main } else { off },
            head_direction,
        )
    }
}

/// A device's direction for a given orientation.
pub fn direction(orientation: Quat) -> Vec3 {
    orientation * BACK
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_heads_yaw_is_minecrafts_heading_of_where_it_faces() {
        // Identity faces -Z: Minecraft's yaw 180. Facing +X is yaw -90, +Z is 0.
        let close = |a: f32, b: f32| (wrap_radians(a - b)).abs() < 1e-4;
        assert!(close(head_yaw_rad(Quat::IDENTITY), PI));
        assert!(close(head_yaw_rad(Quat::from_rotation_y(PI)), 0.0));
        assert!(close(
            head_yaw_rad(Quat::from_rotation_y(-FRAC_PI_2)),
            -FRAC_PI_2
        ));
        // Nodding down 30 degrees does not change the heading; a roll does not either.
        let nodded = Quat::from_rotation_y(PI) * Quat::from_rotation_x(-0.5);
        assert!(close(head_yaw_rad(nodded), 0.0));
        let rolled = nodded * Quat::from_rotation_z(0.4);
        assert!(close(head_yaw_rad(rolled), 0.0));
        // Looking straight down with the top of the head towards +X: the heading follows it.
        let down = Quat::from_rotation_y(-FRAC_PI_2) * Quat::from_rotation_x(-FRAC_PI_2);
        assert!(head_yaw_rad(down).is_finite());
    }

    #[test]
    fn angles_wrap_and_diff_like_java() {
        assert_eq!(angle_normalize(-90.0), 270.0);
        assert_eq!(angle_normalize(720.0), 0.0);
        assert_eq!(angle_diff(10.0, 350.0), 20.0);
        assert_eq!(angle_diff(350.0, 10.0), -20.0);
        assert!((wrap_radians(3.0 * PI / 2.0) - (-FRAC_PI_2)).abs() < 1e-6);
    }

    #[test]
    fn deadzone_rescales() {
        assert_eq!(apply_deadzone(0.1, 0.2), 0.0);
        assert!((apply_deadzone(0.6, 0.2) - 0.5).abs() < 1e-6);
        assert!((apply_deadzone(-1.0, 0.2) + 1.0).abs() < 1e-6);
    }

    #[test]
    fn digital_stick_has_eight_sectors() {
        assert_eq!(to_digital(0.0, 1.0, 0.5), (0.0, 1.0));
        assert_eq!(to_digital(0.0, -1.0, 0.5), (0.0, -1.0));
        assert_eq!(to_digital(1.0, 0.0, 0.5), (1.0, 0.0));
        assert_eq!(to_digital(-0.7, 0.7, 0.5), (-1.0, 1.0));
        assert_eq!(to_digital(0.1, 0.1, 0.5), (0.0, 0.0));
    }

    #[test]
    fn a_seated_body_faces_where_the_head_does() {
        // The head looks along +X: Minecraft's yaw of -90 degrees.
        let yaw =
            estimate_body_yaw_rad(true, false, FbtMode::ArmsOnly, Vec3::X, None, None, || {
                Vec3::ZERO
            });
        assert!((yaw + FRAC_PI_2).abs() < 1e-6);
    }

    #[test]
    fn standing_with_hands_in_front_follows_the_head() {
        // Looking along -Z (yaw 180), hands in front of the chest and a bit apart.
        let head = Vec3::new(0.0, 0.0, -1.0);
        let right = Vec3::new(-0.2, 1.0, -0.4);
        let left = Vec3::new(0.2, 1.0, -0.4);
        let yaw = estimate_body_yaw_rad(
            false,
            false,
            FbtMode::ArmsOnly,
            head,
            Some(right),
            Some(left),
            || Vec3::ZERO,
        );
        assert!((yaw.abs() - PI).abs() < 1e-4, "{yaw}");
    }

    #[test]
    fn a_pose_points_along_minus_z() {
        assert!((direction(Quat::IDENTITY) - BACK).length() < 1e-6);
        let turned = Quat::from_rotation_y(FRAC_PI_2);
        // A quarter turn left points the device along -X.
        assert!((direction(turned) - Vec3::new(-1.0, 0.0, 0.0)).length() < 1e-6);
    }
}
