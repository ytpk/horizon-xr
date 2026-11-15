// SPDX-License-Identifier: LGPL-3.0-only
// Copyright (c) 2025-2026 Phoenix-Ra and VisorEssentials contributors.
//! Native port of VisorEssentials, upstream 88ffec507449eb1a97ccbf495cd331a46c6d635b.
//! Derived from RemoteOverlayRenderHelper.java; changed to camera-relative Rust geometry.
pub mod bow;
pub mod inventory;
pub mod network;
use glam::{Quat, Vec3};

/// The placeholder contains no remote screen contents (as in Essentials).
pub const PANEL_COLOR: u32 = 0x8C3C_2D28;
pub const TEXT_COLOR: u32 = 0x005C_6476;

/// A remote player's menu indicator. Positions are relative to the viewing camera.
#[derive(Clone, Copy, Debug)]
pub struct FocusPanel {
    pub center: Vec3,
    pub right: Vec3,
    pub up: Vec3,
    /// Points back toward the player who owns the panel.
    pub normal: Vec3,
}

impl FocusPanel {
    /// Essentials uses a 0.8 by 0.45 m panel, 0.45 m in front of the HMD,
    /// without roll. Looking vertically uses body yaw rather than an unstable heading.
    pub fn new(head: Vec3, orientation: Quat, body_yaw: f32) -> Option<Self> {
        if !head.is_finite() || !orientation.is_finite() || !body_yaw.is_finite() {
            return None;
        }
        let direction = (orientation * Vec3::NEG_Z).try_normalize()?;
        let forward = if direction.x.hypot(direction.z) < 1e-4 {
            Vec3::new(-body_yaw.sin(), 0.0, body_yaw.cos())
        } else {
            direction
        };
        let right = forward.cross(Vec3::Y).try_normalize()?;
        let up = right.cross(forward);
        Some(Self {
            center: head + forward * 0.45,
            right,
            up,
            normal: -forward,
        })
    }

    pub fn corners(self) -> [Vec3; 4] {
        let r = self.right * 0.4;
        let u = self.up * 0.225;
        [
            self.center - r - u,
            self.center + r - u,
            self.center + r + u,
            self.center - r + u,
        ]
    }

    /// Text faces the viewer on either side and sits 1 cm off the panel.
    pub fn label_axes(self) -> (Vec3, Vec3, Vec3) {
        let side = if (-self.center).dot(self.normal) < 0.0 {
            -1.0
        } else {
            1.0
        };
        (
            self.center + self.normal * side * 0.01,
            self.right * side,
            self.up,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panel_tracks_head_and_has_upstream_dimensions_without_roll() {
        let panel = FocusPanel::new(Vec3::new(2.0, 1.6, 0.0), Quat::IDENTITY, 0.0).unwrap();
        assert!((panel.center - Vec3::new(2.0, 1.6, -0.45)).length() < 1e-6);
        let [a, b, c, _] = panel.corners();
        assert!((a.distance(b) - 0.8).abs() < 1e-6);
        assert!((b.distance(c) - 0.45).abs() < 1e-6);
        let rolled = FocusPanel::new(Vec3::ZERO, Quat::from_rotation_z(1.0), 0.0).unwrap();
        assert!((rolled.up - Vec3::Y).length() < 1e-6);
    }

    #[test]
    fn looking_straight_up_uses_body_heading_and_invalid_poses_are_ignored() {
        let panel = FocusPanel::new(
            Vec3::ZERO,
            Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
            0.0,
        )
        .unwrap();
        assert!((panel.center - Vec3::new(0.0, 0.0, 0.45)).length() < 1e-6);
        assert!(FocusPanel::new(Vec3::splat(f32::NAN), Quat::IDENTITY, 0.0).is_none());
        assert!(FocusPanel::new(Vec3::ZERO, Quat::from_xyzw(0.0, 0.0, 0.0, 0.0), 0.0).is_none());
    }

    #[test]
    fn label_faces_viewer_on_both_sides_without_mirroring() {
        for z in [-2.0, 2.0] {
            let panel = FocusPanel::new(Vec3::new(0.0, 0.0, z), Quat::IDENTITY, 0.0).unwrap();
            let (at, right, up) = panel.label_axes();
            assert!(right.cross(up).dot(-at) > 0.0);
            assert!((at.distance(panel.center) - 0.01).abs() < 1e-6);
        }
    }
}
