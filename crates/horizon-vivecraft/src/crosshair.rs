// SPDX-License-Identifier: LGPL-3.0-or-later
//! Ports the crosshair of `org.vivecraft.client_vr.render.helpers.VREffectsHelper`
//! (`shouldRenderCrosshair`, `extractCrosshairState`): when the in-world crosshair shows, how it
//! lies on what the aim ray hits, and how large it is.
//! Copyright (c) Vivecraft contributors; LGPL-3.0-or-later (see LICENSE-LGPL-3.0).

use crate::modes::PointerElement;
use crate::settings::VrSettings;
use glam::{Mat3, Vec3};

/// What `shouldRenderCrosshair` asks about the game.
#[derive(Clone, Copy, Debug, Default)]
pub struct CrosshairContext {
    /// A level is loaded.
    pub in_level: bool,
    /// A screen is open (or `GuiHandler.GUI_POS_ROOM` is set).
    pub screen_open: bool,
    /// F1: the HUD is hidden.
    pub hud_hidden: bool,
    pub keyboard_showing: bool,
    /// The radial menu is open on the right (main) controller.
    pub radial_on_main: bool,
    pub bow_notched: bool,
    /// The interact or climb-grab binding is down on the main controller.
    pub interact_or_grab_down: bool,
    pub teleport_aiming: bool,
    pub grabbing_ladder: bool,
    pub world_scale: f32,
}

/// `VREffectsHelper.shouldRenderCrosshair`.
pub fn should_render(s: &VrSettings, c: &CrosshairContext) -> bool {
    if !c.in_level || c.screen_open {
        return false;
    }
    match s.crosshair_mode() {
        PointerElement::Never => return false,
        PointerElement::WithHud if c.hud_hidden => return false,
        _ => {}
    }
    !(c.keyboard_showing
        || c.radial_on_main
        || c.bow_notched
        || c.interact_or_grab_down
        || c.teleport_aiming
        || c.grabbing_ladder
        || c.world_scale > 15.0)
}

/// Which face of a block the ray hit (`Direction`), or none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Face {
    Down,
    Up,
    North,
    South,
    West,
    East,
}

/// Where and how the crosshair lies.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CrosshairQuad {
    /// The quad's right and up axes in the world; it spans `+-scale` along each.
    pub right: Vec3,
    pub up: Vec3,
    /// Half the quad's side, in metres.
    pub scale: f32,
    /// 0.5 on a miss, 1 on a hit.
    pub brightness: f32,
    /// Draws through blocks (`!useCrosshairOcclusion`).
    pub through_blocks: bool,
}

/// `extractCrosshairState`'s geometry. `aim_yaw` and `aim_pitch` are the aim controller's in
/// Minecraft degrees (pitch positive looks down), `distance` the aim origin's distance to the
/// hit point.
pub fn quad(
    s: &VrSettings,
    face: Option<Face>,
    hit: bool,
    aim_yaw: f32,
    aim_pitch: f32,
    distance: f32,
    world_scale: f32,
) -> CrosshairQuad {
    let ry = |deg: f32| Mat3::from_rotation_y(deg.to_radians());
    let rx = |deg: f32| Mat3::from_rotation_x(deg.to_radians());
    let rotation = match face {
        Some(Face::Down) => ry(-aim_yaw) * rx(-90.0),
        Some(Face::Up) => ry(-aim_yaw) * rx(90.0),
        Some(Face::West) => ry(90.0),
        Some(Face::East) => ry(-90.0),
        Some(Face::South) => ry(180.0),
        Some(Face::North) => Mat3::IDENTITY,
        None => ry(-aim_yaw) * rx(-aim_pitch),
    };
    let mut scale = 0.125 * s.float("crosshairScale") * world_scale.sqrt();
    if s.bool("crosshairScalesWithDistance") {
        scale *= 0.3 + 0.2 * distance;
    }
    CrosshairQuad {
        right: rotation * Vec3::X,
        up: rotation * Vec3::Y,
        scale,
        brightness: if hit { 1.0 } else { 0.5 },
        through_blocks: !s.bool("useCrosshairOcclusion"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_hides_for_the_things_vivecraft_hides_it_for() {
        let s = VrSettings::default();
        let ok = CrosshairContext {
            in_level: true,
            world_scale: 1.0,
            ..Default::default()
        };
        assert!(should_render(&s, &ok));
        assert!(!should_render(
            &s,
            &CrosshairContext {
                bow_notched: true,
                ..ok
            }
        ));
        assert!(!should_render(
            &s,
            &CrosshairContext {
                world_scale: 16.0,
                ..ok
            }
        ));
        let mut never = VrSettings::default();
        never.set("renderInGameCrosshairMode", "NEVER");
        assert!(!should_render(&never, &ok));
        let mut with_hud = VrSettings::default();
        with_hud.set("renderInGameCrosshairMode", "WITH_HUD");
        assert!(!should_render(
            &with_hud,
            &CrosshairContext {
                hud_hidden: true,
                ..ok
            }
        ));
    }

    #[test]
    fn it_lies_flat_on_the_face_and_grows_with_distance() {
        let mut s = VrSettings::default();
        s.set_bool("crosshairScalesWithDistance", true);
        let up = quad(&s, Some(Face::Up), true, 0.0, 0.0, 2.0, 1.0);
        // Lying on top of a block: the quad's up axis runs along the ground.
        assert!(up.up.y.abs() < 1e-5 && up.right.y.abs() < 1e-5);
        let near = quad(&s, None, false, 0.0, 0.0, 1.0, 1.0);
        let far = quad(&s, None, false, 0.0, 0.0, 5.0, 1.0);
        assert!(far.scale > near.scale);
        assert_eq!(near.brightness, 0.5);
        // 0.125 * crosshairScale (default) at 1 m: 0.3 + 0.2 of it.
        let expected = 0.125 * s.float("crosshairScale") * 0.5;
        assert!((near.scale - expected).abs() < 1e-6);
    }
}
