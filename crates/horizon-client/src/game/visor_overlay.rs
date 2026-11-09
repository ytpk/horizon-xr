// SPDX-License-Identifier: MPL-2.0
// Ports Visor: core/client/gui/overlays/builtin/VROverlayMovementState.java and hotbar/VROverlayHotBar.java (where they hang), api/client/gui/overlays/VROverlayHelper.java.
//! The second panel of a headset's play frame (`headset::set_overlay`): overlays that hang in
//! the room rather than ride on the HUD. Copyright (C) Phoenix-Ra and Visor contributors;
//! MPL-2.0 (see LICENSE-MPL-2.0).
//!
//! Two of Visor's overlays live here: the hotbar wheel, 0.6 m in front of the hand that opened it,
//! and the movement state icon on the off hand (the Hand HUD preset's `movement_state`:
//! `defaults_hand_hud.yml`). Visor's overlay is a GUI at 427 pixels to the metre; the panel here
//! is a 1280 by 720 image drawn at the game's GUI scale, so its width in metres is its GUI width
//! over 427.
use super::Game;
use glam::{Mat3, Quat, Vec3};
use horizon_client::{graphics_style, headset, hud::HudBatch};
use horizon_visor::tasks::hotbar::PIXELS_PER_METRE;
use std::sync::Arc;

/// `movement_state.png`'s size and the icons' `[u, v]` (each 50 by 50): run, sneak, crawl, climb,
/// swim, fall flying, flying, fast flying.
const TEXTURE: [f32; 2] = [162.0, 212.0];
const ICON: f32 = 50.0;
const ICON_AT: [[f32; 2]; 8] = [
    [0.0, 0.0],
    [56.0, 0.0],
    [112.0, 0.0],
    [0.0, 72.0],
    [56.0, 72.0],
    [112.0, 72.0],
    [0.0, 144.0],
    [56.0, 144.0],
];

/// The icon of the movement state the player is in, as `VROverlayMovementState.onRender` picks it
/// (a later state wins over an earlier one).
pub(super) fn movement_icon(
    sprinting: bool,
    sneaking: bool,
    swimming: bool,
    crawling: bool,
    flying: bool,
    fall_flying: bool,
    climbing: bool,
) -> Option<usize> {
    let mut icon = None;
    if sprinting {
        icon = Some(0);
    }
    if sneaking {
        icon = Some(1);
    }
    if swimming {
        icon = Some(4);
    }
    if crawling {
        icon = Some(2);
    }
    if flying {
        icon = Some(if sprinting { 7 } else { 6 });
    }
    if fall_flying {
        icon = Some(5);
    }
    if climbing {
        icon = Some(3);
    }
    icon
}

/// The upright panel across a ray `distance` ahead of `origin` along `forward`, facing back at the
/// hand (`VROverlayHotBar`'s `applyPose` with the hand's position and `orientPosOffset` -0.6 m):
/// its +X is the ray's right, +Y up, +Z back toward where the ray starts. The same plane
/// `horizon_visor::tasks::hotbar::wheel_point` takes the ray against.
pub(super) fn upright_panel(origin: Vec3, forward: Vec3, distance: f32) -> (Vec3, Quat) {
    let right = forward.cross(Vec3::Y).try_normalize().unwrap_or(Vec3::X);
    let up = right.cross(forward);
    (
        origin + forward * distance,
        Quat::from_mat3(&Mat3::from_cols(right, up, -forward)),
    )
}

impl Game<'_> {
    /// Once per frame, standing in a headset: the overlay panels the frame shows and what each
    /// draws (`[hand overlay, chat, screen handles]`), or none (and the headset is told so).
    pub(super) fn headset_overlay(
        &mut self,
        size: [u32; 2],
    ) -> [Option<HudBatch>; headset::OVERLAYS] {
        [
            self.hand_overlay(size, 0),
            self.chat_overlay(size),
            self.screen_handles_overlay(size),
            self.visor_keyboard_overlay(size),
            self.hand_overlay(size, 1),
        ]
    }

    /// Visor's chat overlay is on (`VROverlayChat`, a head-locked panel): the HUD draws no chat.
    pub(super) fn chat_overlay_wanted(&self) -> bool {
        headset::active() && self.title.is_none() && self.screen == super::Screen::Playing
    }

    /// `VROverlayChat`: the chat lines on a panel of their own, head-locked at (0.392, 0.214,
    /// -1.706) and 1.2 wide (`OverlayOptionsPose` of the template: position anchor HMD, rotation
    /// anchor HMD, rotation offset 0). Shown while there is something to read and the chat is not
    /// hidden; with the chat screen open the chat is on the screen's panel instead.
    fn chat_overlay(&mut self, size: [u32; 2]) -> Option<HudBatch> {
        let settings = headset::settings();
        if !self.chat_overlay_wanted() || self.pad_in_menu() || !settings.bool("visorChatVisible") {
            headset::set_overlay_at(headset::OVERLAY_CHAT, None);
            return None;
        }
        let mut batch = HudBatch::new(size);
        self.draw_chat(&mut batch);
        let room = headset::room().filter(|_| !batch.vertices.is_empty());
        let Some(room) = room else {
            headset::set_overlay_at(headset::OVERLAY_CHAT, None);
            return None;
        };
        let scale = headset::world_scale();
        let head = (room.head.0 - room.rest) * scale + Vec3::Y * ((scale - 1.0) * room.rest.y);
        let rot = room.head.1;
        // A GUI of 427 pixels across is `scale` metres wide (`PIXELS_PER_METRE`).
        let width = settings.float("visorChatScale") * scale * batch.width
            / horizon_visor::overlay::GUI_PIXELS_ACROSS;
        headset::set_overlay_at(
            headset::OVERLAY_CHAT,
            Some(headset::OverlaySpec::new(
                head + rot
                    * (Vec3::new(
                        settings.float("visorChatX"),
                        settings.float("visorChatY"),
                        settings.float("visorChatZ"),
                    ) * scale),
                rot,
                width,
            )),
        );
        Some(batch)
    }

    /// The hotbar wheel and the movement state icon (`headset::set_overlay`).
    fn hand_overlay(&mut self, size: [u32; 2], hand: usize) -> Option<HudBatch> {
        let slot = if hand == 0 {
            headset::OVERLAY_HAND
        } else {
            headset::OVERLAY_OFFHAND
        };
        if !headset::active() || self.pad_in_menu() || self.title.is_some() || self.spectator() {
            headset::set_overlay_at(slot, None);
            return None;
        }
        // The hotbar wheel, where the hand was when the grip went down.
        if let Some(wheel) = self.pad.headset.wheels.hands[hand] {
            let mut batch = HudBatch::new(size);
            self.draw_visor_wheel(&mut batch, hand);
            let (pos, rot) = wheel.panel();
            headset::set_overlay_at(
                slot,
                Some(headset::OverlaySpec::new(
                    pos,
                    rot,
                    batch.width / PIXELS_PER_METRE,
                )),
            );
            return Some(batch);
        }
        // The movement state on the off hand, with the HUD on the wrist (the Hand HUD preset).
        if hand == 0
            && headset::settings().enum_name("vrHudLockMode") == "WRIST"
            && self.riding.is_none()
            && let Some(hand) = headset::off_hand()
            && let Some(p) = self.player.as_ref()
            && let Some(icon) = movement_icon(
                p.sprinting,
                p.shift_key_down,
                p.swimming,
                p.room_crawling,
                p.flying,
                p.fall_flying,
                self.pad.headset.vr.climb.is_grabbing_ladder() || self.visor.climb.is_grabbed(),
            )
        {
            let mut batch = HudBatch::new(size);
            let (cx, cy) = (batch.width / 2.0, batch.height / 2.0);
            let side = batch.height * 0.6;
            let at = ICON_AT[icon];
            batch.sprite_sub(
                &self.gui.atlas,
                "horizon/visor/movement_state",
                [at[0], at[1], ICON, ICON],
                TEXTURE,
                [cx - side / 2.0, cy - side / 2.0, side, side],
                u32::MAX,
            );
            // `defaults_hand_hud.yml`: offset (0.06, -0.0178, 0.2628) from the off hand, turned a
            // quarter about Y from it, scale 0.05 of a 400 pixel overlay; the icon is 60 % of the
            // panel's height, 0.03 m tall.
            let width = 0.05 * (batch.width / 400.0);
            let pos = hand.grip.0 + hand.grip.1 * Vec3::new(0.06, -0.0178, 0.2628);
            let rot = hand.grip.1 * Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
            headset::set_overlay_at(slot, Some(headset::OverlaySpec::new(pos, rot, width)));
            return Some(batch);
        }
        headset::set_overlay_at(slot, None);
        None
    }

    /// The handles under a screen's panel (`RenderGuiHelper.drawHandles`): the drag bar and the
    /// resize square, grey, light grey while dragged and light blue while resized. The headset
    /// places the strip they are drawn on under the panel; this is its picture. A resize the
    /// player ended is kept in the settings.
    fn screen_handles_overlay(&mut self, size: [u32; 2]) -> Option<HudBatch> {
        if let Some(width) = headset::take_screen_scale() {
            Arc::make_mut(&mut self.settings.vr).set_float("visorScreenScale", width);
            self.settings.save(&Self::options_path());
        }
        let handles = headset::screen_handles();
        if !handles.visible || !headset::active() {
            return None;
        }
        // `barColor`.
        let color: u32 = if handles.resizing {
            0xA078_DCFF
        } else if handles.dragging {
            0x6EDC_DCDC
        } else {
            0x55BE_BEBE
        };
        let mut batch = HudBatch::new(size);
        let (w, h) = (batch.width, batch.height);
        let (rects, _) = horizon_visor::overlay::handle_rects(true);
        for [l, t, r, b] in rects {
            graphics_style::fill(
                &mut batch,
                &self.gui.atlas,
                [l * w, t * h, (r - l) * w, (b - t) * h],
                color,
            );
        }
        Some(batch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_movement_state_takes_the_last_that_applies() {
        // Sprinting: run. Sneaking over it: sneak. Flying fast: the fast icon; climbing beats all.
        assert_eq!(
            movement_icon(true, false, false, false, false, false, false),
            Some(0)
        );
        assert_eq!(
            movement_icon(true, true, false, false, false, false, false),
            Some(1)
        );
        assert_eq!(
            movement_icon(true, false, false, false, true, false, false),
            Some(7)
        );
        assert_eq!(
            movement_icon(false, false, false, false, true, false, false),
            Some(6)
        );
        assert_eq!(
            movement_icon(true, true, true, true, true, true, true),
            Some(3)
        );
        assert_eq!(
            movement_icon(false, false, false, false, false, false, false),
            None
        );
    }

    #[test]
    fn the_wheel_hangs_across_the_hands_ray_facing_back_at_it() {
        let (at, rot) = upright_panel(Vec3::new(0.1, -0.3, -0.2), Vec3::NEG_Z, 0.6);
        assert!((at - Vec3::new(0.1, -0.3, -0.8)).length() < 1e-5, "{at}");
        // Its front (+Z) looks back toward the hand, its right is the room's, its top is up.
        assert!((rot * Vec3::Z - Vec3::Z).length() < 1e-5);
        assert!((rot * Vec3::X - Vec3::X).length() < 1e-5);
        assert!((rot * Vec3::Y - Vec3::Y).length() < 1e-5);
        // A ray tipped up 30 degrees keeps the panel upright: its top stays vertical.
        let forward = Quat::from_rotation_x(30f32.to_radians()) * Vec3::NEG_Z;
        let (_, rot) = upright_panel(Vec3::ZERO, forward, 0.6);
        assert!((rot * Vec3::X).y.abs() < 1e-5);
    }
}
