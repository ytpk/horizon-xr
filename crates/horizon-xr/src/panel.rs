// SPDX-License-Identifier: LGPL-3.0-or-later
//! A quad in the play space the game's screen or HUD is shown on, placed by Vivecraft's
//! `GuiHandler` (`horizon_vivecraft::gui`): the menu panel opens where the player looks and
//! stays; the HUD sits on the head, the hand or the wrist. Copyright (c) Vivecraft contributors.
use glam::{Quat, Vec3};
use horizon_vivecraft::gui::{Placement, QUAD_WIDTH};
use openxr as xr;

/// The screen's height over its width (the GUI framebuffer is 16:9).
const ASPECT: f32 = 9.0 / 16.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Panel {
    centre: Vec3,
    /// Turns the panel's own frame (+Z toward the viewer) into the play space.
    orientation: Quat,
    width: f32,
}

impl Panel {
    /// The quad Vivecraft's `GuiHandler` puts there.
    pub fn placed(p: &Placement) -> Self {
        Self {
            centre: p.centre(),
            orientation: p.rot,
            width: QUAD_WIDTH * p.scale,
        }
    }

    /// A panel at a fixed place (the room's TV).
    pub fn fixed(centre: Vec3, orientation: Quat, width: f32) -> Self {
        Self {
            centre,
            orientation,
            width,
        }
    }

    fn height(&self) -> f32 {
        self.width * ASPECT
    }

    pub fn centre(&self) -> Vec3 {
        self.centre
    }
    pub fn orientation(&self) -> Quat {
        self.orientation
    }
    pub fn width(&self) -> f32 {
        self.width
    }
    /// Where the panel is now (a hand drags it).
    pub fn set_pose(&mut self, centre: Vec3, orientation: Quat) {
        self.centre = centre;
        self.orientation = orientation;
    }
    pub fn set_width(&mut self, width: f32) {
        self.width = width;
    }
    /// The strip under the panel its handles are in (`horizon_visor::overlay`): the middle, the
    /// turn, the width and the height.
    pub fn handle_strip(&self) -> (Vec3, Quat, f32, f32) {
        let h = self.height();
        let strip = h * horizon_visor::overlay::STRIP_HEIGHT;
        (
            self.centre + self.orientation * Vec3::new(0.0, -(h * 0.5 + strip * 0.5), 0.0),
            self.orientation,
            self.width,
            strip,
        )
    }
    /// Where a ray meets the panel's plane in front of it, as `hit_along` but not limited to the
    /// picture: `u` and `v` run past 0..1 where the handles are.
    pub fn plane_hit(&self, origin: Vec3, direction: Vec3) -> Option<[f32; 2]> {
        let normal = self.orientation * Vec3::Z;
        let facing = direction.dot(normal);
        if facing >= -1e-4 {
            return None;
        }
        let t = (self.centre - origin).dot(normal) / facing;
        if t <= 0.0 {
            return None;
        }
        let local = self.orientation.conjugate() * (origin + direction * t - self.centre);
        Some([local.x / self.width + 0.5, 0.5 - local.y / self.height()])
    }

    pub fn pose(&self) -> xr::Posef {
        let q = self.orientation;
        xr::Posef {
            orientation: xr::Quaternionf {
                x: q.x,
                y: q.y,
                z: q.z,
                w: q.w,
            },
            position: xr::Vector3f {
                x: self.centre.x,
                y: self.centre.y,
                z: self.centre.z,
            },
        }
    }

    pub fn size(&self) -> xr::Extent2Df {
        xr::Extent2Df {
            width: self.width,
            height: self.height(),
        }
    }

    /// Where a ray from `origin` along `direction` meets the panel's front,
    /// 0..1 from its top left, and how far along the (unit) `direction` it
    /// is; `None` when it misses or points away.
    pub fn hit_along(&self, origin: Vec3, direction: Vec3) -> Option<([f32; 2], f32)> {
        let normal = self.orientation * Vec3::Z;
        let facing = direction.dot(normal);
        if facing >= -1e-4 {
            return None;
        }
        let t = (self.centre - origin).dot(normal) / facing;
        if t <= 0.0 {
            return None;
        }
        let local = self.orientation.conjugate() * (origin + direction * t - self.centre);
        let u = local.x / self.width + 0.5;
        let v = 0.5 - local.y / self.height();
        ((0.0..=1.0).contains(&u) && (0.0..=1.0).contains(&v)).then_some(([u, v], t))
    }
    #[cfg(test)]
    fn hit(&self, origin: Vec3, direction: Vec3) -> Option<[f32; 2]> {
        self.hit_along(origin, direction).map(|(at, _)| at)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_vivecraft::gui::{ScreenKind, place_screen};
    use horizon_vivecraft::settings::VrSettings;
    use horizon_vivecraft::vr_data::RoomPose;

    #[test]
    fn a_ray_from_the_head_straight_ahead_hits_the_middle_of_a_screen_a_metre_off() {
        let head = RoomPose {
            pos: Vec3::new(0.2, 1.7, -0.3),
            rot: Quat::IDENTITY,
        };
        let s = VrSettings::default();
        let panel = Panel::placed(&place_screen(&s, ScreenKind::Normal, &head, &head, false));
        let [u, v] = panel.hit(head.pos, Vec3::NEG_Z).unwrap();
        assert!((u - 0.5).abs() < 1e-4 && (v - 0.5).abs() < 1e-4, "{u} {v}");
        // It is 1 m wide (Visor's overlay size): 0.25 m to the right of the middle is u = 0.75.
        let [u, _] = panel
            .hit(head.pos + Vec3::new(0.25, 0.0, 0.0), Vec3::NEG_Z)
            .unwrap();
        assert!((u - 0.75).abs() < 1e-4, "{u}");
    }

    #[test]
    fn the_panel_turns_with_the_head_and_misses_what_points_away() {
        let turned = RoomPose {
            pos: Vec3::ZERO,
            rot: Quat::from_rotation_y(90f32.to_radians()),
        };
        let s = VrSettings::default();
        let panel = Panel::placed(&place_screen(
            &s,
            ScreenKind::Normal,
            &turned,
            &turned,
            false,
        ));
        let [u, _] = panel
            .hit(Vec3::ZERO, Vec3::new(-1.0, 0.0, -0.2).normalize())
            .unwrap();
        assert!(u > 0.5, "{u}");
        assert_eq!(panel.hit(Vec3::ZERO, Vec3::X), None);
    }
}
