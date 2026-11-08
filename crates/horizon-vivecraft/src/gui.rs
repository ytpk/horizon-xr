// SPDX-License-Identifier: LGPL-3.0-or-later
//! Ports the placement of `org.vivecraft.client_vr.gameplay.screenhandlers.GuiHandler`: where a
//! screen opens in the room, where the HUD sits (head, hand or wrist lock), how a controller's
//! ray maps to the screen, and `InteractiveHotbarModule`'s touch bar.
//! Copyright (c) Vivecraft contributors; LGPL-3.0-or-later (see LICENSE-LGPL-3.0).
//!
//! Everything is in room space (the tracking space of the runtime). The quad is `QUAD_WIDTH`
//! metres wide at scale 1 and as tall as the GUI framebuffer's aspect says. The sizes and places are
//! Visor's (`VROverlayGameScreen`, `VROverlayHUD`, `VROverlayPose`: an overlay is as wide as its scale
//! in metres), not Vivecraft's 1.5 m quad.

use crate::modes::HudLock;
use crate::settings::VrSettings;
use crate::vr_data::RoomPose;
use glam::{Mat3, Quat, Vec2, Vec3};
use std::f32::consts::{FRAC_PI_2, PI};

/// The GUI quad's width at scale 1: Visor's `VROverlayPose` makes an overlay `scale` metres wide
/// (Vivecraft's `getTexCoordsForCursor` used 1.5).
pub const QUAD_WIDTH: f32 = 1.0;

/// How far below the eyes' line the head-locked HUD hangs per metre of its distance: Visor's HUD
/// overlay sits at `(0, -0.1, -1.2)` from the head (`VROverlayHUD`).
pub const HUD_DROP_PER_METRE: f32 = 0.1 / 1.2;

/// `GuiHandler.calculateScale`: the GUI scale factor for a framebuffer, and the largest one.
pub fn calculate_scale(scale_in: i32, force_unicode: bool, width: i32, height: i32) -> (i32, i32) {
    let (mut scale, mut max_scale) = (1, 1);
    while max_scale < width
        && max_scale < height
        && width / (max_scale + 1) >= 320
        && height / (max_scale + 1) >= 240
    {
        if scale < scale_in || scale_in == 0 {
            scale += 1;
        }
        max_scale += 1;
    }
    if force_unicode {
        if scale % 2 != 0 {
            scale += 1;
        }
        if max_scale % 2 != 0 {
            max_scale += 1;
        }
    }
    (scale, max_scale)
}

/// The framebuffer the GUI is drawn into (`updateResolution`).
pub fn gui_resolution(s: &VrSettings) -> (i32, i32) {
    if s.bool("doubleGUIResolution") {
        (2560, 1440)
    } else {
        (1280, 720)
    }
}

/// A screen in the room.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    /// The middle of the quad.
    pub pos: Vec3,
    /// Turns the quad's frame (+Z toward the viewer) into the room.
    pub rot: Quat,
    /// The quad is `QUAD_WIDTH * scale` wide.
    pub scale: f32,
    /// An offset in the quad's own frame the renderer adds (the wrist).
    pub local: Vec3,
}

impl Placement {
    /// The middle of the quad including `local`.
    pub fn centre(&self) -> Vec3 {
        self.pos + self.rot * self.local
    }
    pub fn width(&self) -> f32 {
        QUAD_WIDTH * self.scale
    }
}

/// Which screen is opening.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ScreenKind {
    Normal,
    Chat,
    /// A book or a sign being edited.
    Book,
    /// A container screen of the block (or entity) the player just used: its position in the
    /// room (`GUI_APPEAR_OVER_BLOCK_ACTIVE`).
    Container {
        source_room: Vec3,
    },
}

/// `onScreenChanged` for a screen opening in play: the quad `facing` (the head, or the
/// controller when `in_front_of_hand`) sees, 1 m ahead (`look * 0.5` of a 2 m vector).
/// `physical_keyboard_showing` keeps the quad above the head height plus 0.2.
pub fn place_screen(
    s: &VrSettings,
    kind: ScreenKind,
    facing: &RoomPose,
    hmd: &RoomPose,
    physical_keyboard_showing: bool,
) -> Placement {
    let mut scale = 1.0;
    let pos = match kind {
        ScreenKind::Container { source_room } if s.bool("guiAppearOverBlock") => {
            let distance = (source_room - hmd.pos).length();
            scale = distance.sqrt();
            // 1.1 plus a quarter of the scale above the block's (or entity's) base.
            source_room + Vec3::new(0.0, 1.1 + scale * 0.25, 0.0)
        }
        _ => {
            let offset = match kind {
                ScreenKind::Chat => Vec3::new(0.0, 0.5, -2.0),
                ScreenKind::Book => Vec3::new(0.0, 0.25, -2.0),
                _ => Vec3::new(0.0, 0.0, -2.0),
            };
            let look = facing.rot * offset;
            let mut p = facing.pos + look * 0.5;
            if s.bool("physicalKeyboard") && physical_keyboard_showing && p.y < facing.pos.y + 0.2 {
                p.y = facing.pos.y + 0.2;
            }
            p
        }
    };
    // Face the head (or the hand).
    let look = pos - facing.pos;
    let pitch = (look.y / look.length()).asin();
    let yaw = PI + look.x.atan2(look.z);
    Placement {
        pos,
        rot: Quat::from_rotation_y(yaw) * Quat::from_rotation_x(pitch),
        scale,
        local: Vec3::ZERO,
    }
}

/// `RadialHandler.orientOverlay`: the radial menu opens 0.6 m ahead of the controller that
/// opened it in hold mode (`radialModeHold`), else 1 m ahead of the head, facing it.
pub fn place_radial(s: &VrSettings, hmd: &RoomPose, controller: &RoomPose) -> Placement {
    let (pose, distance) = if s.bool("radialModeHold") {
        (controller, 1.2)
    } else {
        (hmd, 2.0)
    };
    let offset = pose.dir() * (distance * 0.5);
    let pitch = (offset.y / offset.length()).asin();
    let yaw = PI + offset.x.atan2(offset.z);
    Placement {
        pos: pose.pos + offset,
        rot: Quat::from_rotation_y(yaw) * Quat::from_rotation_x(pitch),
        scale: 1.0,
        local: Vec3::ZERO,
    }
}

/// `onScreenChanged`'s static screen (the menu room, and menus that do not follow the face):
/// 2.5 m... Vivecraft's `GUI_SCALE = 2`, in front of the play area's middle.
pub fn static_screen(play_area_depth: Option<f32>) -> Placement {
    Placement {
        pos: Vec3::new(
            0.02,
            1.3,
            -(play_area_depth.map_or(0.0, |d| d * 0.5)).max(1.5),
        ),
        rot: Quat::IDENTITY,
        scale: 2.0,
        local: Vec3::ZERO,
    }
}

/// The inputs of the HUD's placement (`extractGui` with no screen open).
#[derive(Clone, Copy, Debug)]
pub struct HudInput {
    pub hmd: RoomPose,
    /// The off hand's aim pose (the controller the HUD sits on).
    pub off_aim: RoomPose,
    /// The main hand's aim pose (the seated alternative mode).
    pub main_aim: RoomPose,
    /// The skin has slim arms.
    pub slim: bool,
    pub world_scale: f32,
}

/// `extractGui`'s HUD placement: `lock` is the setting (seated always locks to the head).
/// Returns the placement and whether the hotbar can be touched (`HUD_POPUP`).
pub fn place_hud(s: &VrSettings, i: &HudInput) -> (Placement, bool) {
    let side = if s.left_handed() { -1.0 } else { 1.0 };
    if s.seated() || s.hud_lock() == HudLock::Head {
        let (rot, dir) = if s.seated() && s.bool("seatedHudAltMode") {
            (i.main_aim.rot, i.main_aim.dir())
        } else {
            (i.hmd.rot, i.hmd.dir())
        };
        let distance = i.world_scale * s.float("hudDistance");
        // Visor's HUD hangs a little below the eyes' line, in the head's own frame.
        let pos = if s.seated() && s.bool("seatedHudAltMode") {
            i.hmd.pos + dir * distance
        } else {
            i.hmd.pos + rot * Vec3::new(0.0, -HUD_DROP_PER_METRE * distance, -distance)
        };
        return (
            Placement {
                pos,
                rot,
                scale: s.float("headHudScale"),
                local: Vec3::ZERO,
            },
            false,
        );
    }
    let mut rot = i.off_aim.rot;
    let pos = i.off_aim.pos;
    if s.hud_lock() == HudLock::Hand {
        rot = rot * Quat::from_rotation_x(PI * -0.2) * Quat::from_rotation_y(PI * 0.1 * side);
        (
            Placement {
                pos,
                rot,
                scale: 0.58823526,
                local: Vec3::new(0.0, 0.32 * i.world_scale, 0.0),
            },
            true,
        )
    } else {
        let x_offset = -0.136;
        let y_offset = if i.slim { 0.085 } else { 0.055 };
        let y_scale_offset = if i.slim { 0.0825 } else { 0.1125 };
        let arm_scale = 0.42;
        let local = Vec3::new(
            x_offset * side,
            y_offset + y_scale_offset * (1.0 - arm_scale),
            (0.12 + 0.02 * s.float("vrHudWristOffset")) * arm_scale,
        ) * i.world_scale;
        rot =
            rot * Quat::from_rotation_z(FRAC_PI_2 * side) * Quat::from_rotation_y(FRAC_PI_2 * side);
        (
            Placement {
                pos,
                rot,
                scale: 0.4,
                local,
            },
            true,
        )
    }
}

/// `getTexCoordsForCursor`: where a controller's ray meets a screen, 0..1 from its top left
/// (outside 0..1 or `(-1, -1)` when it misses). `aspect` is the scaled GUI's height over width.
pub fn cursor_uv(placement: &Placement, controller: &RoomPose, aspect: f32) -> Vec2 {
    let m = Mat3::from_quat(placement.rot);
    let normal = m * Vec3::Z;
    let right = m * Vec3::X;
    let up = m * Vec3::Y;
    let dir = controller.dir();
    let facing = normal.dot(dir);
    if facing.abs() > 1.0e-5 {
        let top_left = placement.pos - up * 0.5 - right * 0.5;
        let dist = -normal.dot(controller.pos - top_left) / facing;
        if dist > 0.0 {
            let on_plane = controller.pos + dir * dist - top_left;
            let (mut u, mut v) = (on_plane.dot(right), on_plane.dot(up));
            u = (u - 0.5) / QUAD_WIDTH / placement.scale + 0.5;
            v = (v - 0.5) / aspect / QUAD_WIDTH / placement.scale + 0.5;
            return Vec2::new(u, 1.0 - v);
        }
    }
    Vec2::new(-1.0, -1.0)
}

/// `InteractiveHotbarModule`: which hotbar slot (0..8, 9 the off hand's) the main hand
/// touches. `scale` is the HUD's applied scale times the window's GUI scale over the largest
/// one, `hotbar_rot` the HUD's rotation and `hotbar_pos` its middle.
pub fn touched_hotbar_slot(
    s: &VrSettings,
    hotbar_pos: Vec3,
    hotbar_rot: Quat,
    local_offset: Vec3,
    scale: f32,
    main_hand: Vec3,
    spectator: bool,
) -> Option<usize> {
    let m = Mat3::from_quat(hotbar_rot);
    let start = m * (local_offset + Vec3::new(-0.32 * scale, -0.38 * scale, 0.0)) + hotbar_pos;
    let line = start - (m * Vec3::X * (0.64 * scale) + start);
    let to_bar = start - main_hand;
    let dist = to_bar.cross(line).length() / line.length();
    if dist > 0.06 {
        return None;
    }
    let fact = to_bar.dot(line) / line.length_squared();
    if fact < -1.0 {
        return None;
    }
    let point = main_hand - (line * fact - to_bar);
    let size = line.length();
    let mut len = start.distance(point);
    if fact < 0.0 {
        len = -len;
    }
    let pos = len / size * 9.0;
    let slot = pos.floor() as i32;
    if slot > 8 {
        (!spectator && s.left_handed() && (9.5..=10.5).contains(&pos)).then_some(9)
    } else if slot < 0 {
        (!spectator && !s.left_handed() && (-1.5..=-0.5).contains(&pos)).then_some(9)
    } else {
        Some(slot as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn head() -> RoomPose {
        RoomPose {
            pos: Vec3::new(0.0, 1.7, 0.0),
            rot: Quat::IDENTITY,
        }
    }

    #[test]
    fn a_screen_opens_a_metre_ahead_at_head_height_facing_the_head() {
        let s = VrSettings::default();
        let p = place_screen(&s, ScreenKind::Normal, &head(), &head(), false);
        assert!(
            (p.pos - Vec3::new(0.0, 1.7, -1.0)).length() < 1e-5,
            "{}",
            p.pos
        );
        assert_eq!(p.scale, 1.0);
        // Facing the head: its normal points back at +Z.
        assert!(((p.rot * Vec3::Z) - Vec3::Z).length() < 1e-5);
        // Chat sits a quarter metre higher, tilted down toward the eyes.
        let chat = place_screen(&s, ScreenKind::Chat, &head(), &head(), false);
        assert!((chat.pos.y - 1.95).abs() < 1e-5);
        assert!((chat.rot * Vec3::Z).y < 0.0);
        assert_eq!(chat.width(), 1.0);
    }

    #[test]
    fn the_radial_menu_opens_at_the_hand_in_hold_mode_and_at_the_face_otherwise() {
        let mut s = VrSettings::default();
        let hand = RoomPose {
            pos: Vec3::new(0.3, 1.2, -0.2),
            rot: Quat::IDENTITY,
        };
        let p = place_radial(&s, &head(), &hand);
        assert!(
            (p.pos - Vec3::new(0.3, 1.2, -0.8)).length() < 1e-5,
            "{}",
            p.pos
        );
        s.set_bool("radialModeHold", false);
        let p = place_radial(&s, &head(), &hand);
        assert!(
            (p.pos - Vec3::new(0.0, 1.7, -1.0)).length() < 1e-5,
            "{}",
            p.pos
        );
    }

    #[test]
    fn a_screen_follows_where_the_head_looks() {
        let s = VrSettings::default();
        let left = RoomPose {
            pos: head().pos,
            rot: Quat::from_rotation_y(FRAC_PI_2),
        };
        let p = place_screen(&s, ScreenKind::Normal, &left, &left, false);
        assert!(
            (p.pos - Vec3::new(-1.0, 1.7, 0.0)).length() < 1e-5,
            "{}",
            p.pos
        );
        // Its normal points back at the head (+X from there).
        assert!(
            ((p.rot * Vec3::Z) - Vec3::X).length() < 1e-4,
            "{}",
            p.rot * Vec3::Z
        );
    }

    #[test]
    fn a_container_opens_over_its_block_scaled_by_the_distance() {
        let s = VrSettings::default();
        let block = Vec3::new(0.5, 0.0, -4.5);
        let p = place_screen(
            &s,
            ScreenKind::Container { source_room: block },
            &head(),
            &head(),
            false,
        );
        let distance = (block - head().pos).length();
        assert!((p.scale - distance.sqrt()).abs() < 1e-5);
        assert!((p.pos.y - (1.1 + p.scale * 0.25)).abs() < 1e-5);
        let mut off = VrSettings::default();
        off.set_bool("guiAppearOverBlock", false);
        let q = place_screen(
            &off,
            ScreenKind::Container { source_room: block },
            &head(),
            &head(),
            false,
        );
        assert_eq!(q.scale, 1.0);
    }

    #[test]
    fn the_cursor_is_where_the_ray_meets_the_quad() {
        let s = VrSettings::default();
        let p = place_screen(&s, ScreenKind::Normal, &head(), &head(), false);
        let aspect = 9.0 / 16.0;
        // Straight ahead from the head: the middle.
        let uv = cursor_uv(&p, &head(), aspect);
        assert!((uv - Vec2::new(0.5, 0.5)).length() < 1e-4, "{uv}");
        // 0.25 m to the right of the middle on a 1 m quad (Visor's width): u = 0.75.
        let right = RoomPose {
            pos: head().pos + Vec3::new(0.25, 0.0, 0.0),
            rot: Quat::IDENTITY,
        };
        let uv = cursor_uv(&p, &right, aspect);
        assert!((uv.x - 0.75).abs() < 1e-4, "{uv}");
        // Pointing away misses.
        let away = RoomPose {
            pos: head().pos,
            rot: Quat::from_rotation_y(PI),
        };
        assert_eq!(cursor_uv(&p, &away, aspect), Vec2::new(-1.0, -1.0));
    }

    #[test]
    fn the_hud_follows_the_head_the_hand_or_the_wrist() {
        let mut s = VrSettings::default();
        let input = HudInput {
            hmd: head(),
            off_aim: RoomPose {
                pos: Vec3::new(-0.3, 1.1, -0.3),
                rot: Quat::IDENTITY,
            },
            main_aim: RoomPose::IDENTITY,
            slim: false,
            world_scale: 1.0,
        };
        // Wrist (the default): 0.4 scale on the off hand, popping up for the touched hotbar.
        let (wrist, popup) = place_hud(&s, &input);
        assert_eq!(wrist.scale, 0.4);
        assert!(popup);
        assert!((wrist.pos - input.off_aim.pos).length() < 1e-6);
        assert!((wrist.local.x + 0.136).abs() < 1e-6);
        s.set("vrHudLockMode", "HAND");
        let (hand, _) = place_hud(&s, &input);
        assert!((hand.scale - 0.58823526).abs() < 1e-6);
        assert!((hand.centre().y - 1.1).abs() < 0.5);
        s.set("vrHudLockMode", "HEAD");
        let (head_hud, popup) = place_hud(&s, &input);
        assert!(!popup);
        // hudDistance (1.25) ahead of the eyes along -Z, 0.1 m per 1.2 m below their line (Visor's
        // HUD at (0, -0.1, -1.2)).
        assert!(
            (head_hud.pos - Vec3::new(0.0, 1.7 - 0.1 / 1.2 * 1.25, -1.25)).length() < 1e-5,
            "{}",
            head_hud.pos
        );
        assert_eq!(head_hud.scale, 1.0);
        // Left handed flips the side of the wrist.
        s.set("vrHudLockMode", "WRIST");
        s.set_bool("reverseHands", true);
        assert!(place_hud(&s, &input).0.local.x > 0.0);
    }

    #[test]
    fn scales_follow_the_resolution() {
        assert_eq!(calculate_scale(0, false, 1280, 720), (3, 3));
        assert_eq!(calculate_scale(2, false, 1280, 720), (2, 3));
        assert_eq!(calculate_scale(0, false, 2560, 1440), (6, 6));
        assert_eq!(calculate_scale(1, false, 2560, 1440).0, 1);
        assert_eq!(gui_resolution(&VrSettings::default()), (1280, 720));
    }

    #[test]
    fn touching_the_hotbar_picks_a_slot_and_beyond_the_ends_the_off_hand() {
        let s = VrSettings::default();
        let rot = Quat::IDENTITY;
        let pos = Vec3::new(0.0, 1.0, 0.0);
        // At scale 1 the bar runs 0.64 m along the quad's -X (screen left to right is +X, but
        // Vivecraft's LEFT constant is +X in its frame): start at x = -0.32.
        let start = pos + Vec3::new(-0.32, -0.38, 0.0);
        let slot = |hand: Vec3| touched_hotbar_slot(&s, pos, rot, Vec3::ZERO, 1.0, hand, false);
        let at = |fraction: f32| start + Vec3::new(0.64 * fraction, 0.0, 0.0);
        assert!(slot(at(0.5)).is_some());
        assert_eq!(slot(start + Vec3::new(0.0, 0.5, 0.0)), None);
        assert_eq!(slot(Vec3::new(5.0, 5.0, 5.0)), None);
    }
}
