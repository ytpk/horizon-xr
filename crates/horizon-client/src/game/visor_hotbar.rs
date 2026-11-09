// SPDX-License-Identifier: MPL-2.0
// Ports Visor: core/client/tasks/types/TaskHotBar.java, gui/overlays/builtin/hotbar/VROverlayHotBar.java, input/actions/game/GameActionHotBar.java.
//! Visor's hotbar wheel. The HUD has no row of item slots (`hud_disabled`); a grip held opens a
//! wheel of the nine hotbar slots, the hand's aim picks one (the middle is slot 0, the others run
//! clockwise from the top) and letting go closes it. Copyright (C) Phoenix-Ra and Visor
//! contributors; MPL-2.0 (see LICENSE-MPL-2.0).
//!
//! Each hand has its own room overlay, 0.6 m ahead of where it opened its wheel.
//! Both wheels remain visible and selectable while both grips are held.
use super::Game;
use glam::{Quat, Vec3};
use horizon_client::{graphics_style, headset, hud::HudBatch, visor_art};
use horizon_visor::tasks::hotbar::{
    HotbarInput, ITEM_AT, Tuning, WHEEL_SIZE, slice_from_pos, wheel_point,
};

/// The slot a wheel points at, if it is open.
fn slices_of(wheels: &Wheels, hand: usize) -> Option<usize> {
    wheels.hands[hand].and_then(|w| w.slot)
}

impl Wheel {
    /// Where the panel hangs (`VROverlayHotBar.onEnable`): across the ray the hand opened it along,
    /// `WHEEL_DISTANCE` ahead.
    pub(super) fn panel(&self) -> (Vec3, Quat) {
        super::visor_overlay::upright_panel(
            self.origin,
            self.rot * Vec3::NEG_Z,
            horizon_visor::tasks::hotbar::WHEEL_DISTANCE,
        )
    }
}

/// A wheel that is open: where the hand was when it opened, and the slot it points at.
#[derive(Clone, Copy, Debug)]
pub(super) struct Wheel {
    origin: Vec3,
    rot: Quat,
    slot: Option<usize>,
}

/// The two independent wheels (`VROverlayHotBar.ID_MAIN` and `ID_OFFHAND`).
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Wheels {
    pub(super) hands: [Option<Wheel>; 2],
}

impl Game<'_> {
    fn visor_wheel_tuning() -> Tuning {
        let s = headset::settings();
        Tuning {
            center_radius: s.float("visorHotbarCenterRadius"),
            hysteresis: s.float("visorHotbarHysteresis"),
        }
    }

    /// Two-handed play is on: the server is Visor, plays `twoHandedVR`, and this is a headset.
    pub(super) fn visor_two_handed(&self) -> bool {
        headset::active() && self.visor_active() && self.vr_server_settings().two_handed_vr
    }

    /// Once per frame: where the off hand is (`VROffhandEquipment`) and which hand is active.
    /// With the mode off the vanilla off hand stays, exactly as before.
    pub(super) fn visor_update_two_handed(&mut self) {
        let two = self.visor_two_handed();
        let redirect = horizon_visor::tasks::hotbar::offhand_redirect(
            two,
            two,
            self.visor.hotbar.slot_offhand,
        );
        if redirect != self.offhand_redirect {
            self.offhand_redirect = redirect;
            // The hand shows another stack: the swap animation, as a different item.
            self.hands_replaced[1] = true;
        }
        let active_off = two && !self.visor.active_main;
        if active_off != self.active_off {
            self.active_off = active_off;
            // The attribute modifiers of the hand that acts (`vr_dual_wield_swapped`).
            if self.player.is_some() {
                self.apply_equipment();
            }
        }
        headset::set_two_handed(two);
    }

    /// `MouseClickHandler.onPress`: a click with one hand only makes it the active hand
    /// (`setActiveHand`); in two-handed play only. The off hand's clicks are the left trigger and
    /// X, the main hand's the right trigger and A.
    pub(super) fn visor_active_hand_click(&mut self) {
        let two = self.visor_two_handed();
        let hands = headset::hand_buttons();
        let settings = headset::settings();
        let held = |key| horizon_client_input::vr_bindings::held(&settings, key, hands, false);
        let down = [
            held("questBindAttack") || held("questBindUse"),
            (two && held("questBindOffAttack")) || held("questBindOffUse"),
        ];
        let before = std::mem::replace(&mut self.pad.headset.clicks, down);
        if !two || self.screen != super::Screen::Playing {
            return;
        }
        if down[0] && !before[0] && !down[1] {
            self.visor.active_main = true;
        } else if down[1] && !before[1] && !down[0] {
            self.visor.active_main = false;
        }
    }

    /// Once per loop in play: `TaskHotBar.onRun` with the two grips. A grip held opens that
    /// hand's wheel at the hand, the hand's aim moves its choice, letting go closes it. The main
    /// wheel selects the hotbar slot, the off wheel (two-handed play) the slot of the off hand's
    /// item.
    pub(super) fn headset_hotbar_wheel(&mut self) {
        let in_world = self.screen == super::Screen::Playing && !self.spectator();
        let hands = [headset::main_hand(), headset::off_hand()];
        let grips = headset::hand_buttons();
        let settings = headset::settings();
        let pressed = ["questBindMainHotbar", "questBindOffHotbar"].map(|key| {
            horizon_client_input::vr_bindings::held(&settings, key, grips, false) && in_world
        });
        let pressed: [bool; 2] = std::array::from_fn(|h| pressed[h] && hands[h].is_some());
        if !in_world {
            self.visor.hotbar.clear();
        }
        let two = self.visor_two_handed();
        // `VROverlayHotBar.getSelectedSlice` of each wheel: the hand's aim moves it; a wheel that
        // is just opening starts on the centre (and the off hand's on 1 if the main hand's is
        // on 0, `onRender`).
        let mut slices = [None; 2];
        let tuning = Self::visor_wheel_tuning();
        for h in 0..2 {
            let other = slices_of(&self.pad.headset.wheels, 1 - h);
            slices[h] = match (self.pad.headset.wheels.hands[h], hands[h]) {
                (Some(wheel), Some(hand)) => {
                    let (pos, rot) = hand.aim;
                    let at = wheel_point(wheel.origin, wheel.rot, pos, rot * Vec3::NEG_Z);
                    let disabled = other.map(|slot| [slot]);
                    at.and_then(|(x, y)| {
                        slice_from_pos(
                            x,
                            y,
                            wheel.slot,
                            disabled.as_ref().map_or(&[], |s| s.as_slice()),
                            tuning,
                        )
                    })
                    .or(wheel.slot)
                }
                _ if pressed[h] && (h == 0 || two) => Some(0),
                _ => None,
            };
        }
        if pressed[0] && pressed[1] && slices[0] == Some(0) && slices[1] == Some(0) {
            slices[1] = Some(1);
        }
        let out = self.visor.hotbar.tick(HotbarInput {
            two_handed: two,
            selected: self.selected,
            main_in_world: in_world,
            off_in_world: in_world,
            main_pressed: pressed[0],
            off_pressed: pressed[1] && two,
            main_slice: slices[0],
            off_slice: slices[1],
        });
        if let Some(slot) = out.select
            && slot != self.selected
        {
            self.hotbar_presses.push(slot);
        }
        // `HapticFeedback.HOTBAR_SHOW` and `HOTBAR_HIDE`.
        for (h, haptic) in [(0, out.main_haptic), (1, out.off_haptic)] {
            match haptic {
                Some(true) => headset::pulse(h == 0, 0.35, 0.04),
                Some(false) => headset::pulse(h == 0, 0.25, 0.03),
                None => {}
            }
        }
        for (h, enabled) in [(0, out.main_wheel), (1, out.off_wheel)] {
            match enabled {
                Some(true) => {
                    if let Some(hand) = hands[h] {
                        let (origin, rot) = hand.aim;
                        self.pad.headset.wheels.hands[h] = Some(Wheel {
                            origin,
                            rot,
                            slot: slices[h],
                        });
                    }
                }
                Some(false) => self.pad.headset.wheels.hands[h] = None,
                None => {}
            }
        }
        for h in 0..2 {
            if let Some(w) = self.pad.headset.wheels.hands[h].as_mut() {
                // `UI_SLICE_CHANGE`.
                if slices[h].is_some() && slices[h] != w.slot {
                    headset::pulse(h == 0, 0.15, 0.015);
                }
                w.slot = slices[h].or(w.slot);
            }
        }
    }

    /// The wheel on the HUD, as `VROverlayHotBar.renderRadialImage` draws it from Visor's own
    /// pictures: the backdrop for the slot the hand points at, the slot numbers, the items, the
    /// name of the pointed slot under it, and the marks of the slot in the main hand (and, in
    /// two-handed play, the off hand's).
    pub(super) fn draw_visor_wheel(&self, batch: &mut HudBatch, hand: usize) {
        let Some(wheel) = self.pad.headset.wheels.hands[hand] else {
            return;
        };
        let atlas = &self.gui.atlas;
        let (cx, cy) = ((batch.width / 2.0).floor(), (batch.height / 2.0).floor());
        let (x0, y0) = (cx - WHEEL_SIZE as f32 / 2.0, cy - WHEEL_SIZE as f32 / 2.0);
        let numbers = headset::settings().bool("visorHotbarSlotNumbers");
        let size = WHEEL_SIZE as f32;
        batch.sprite_scaled(
            atlas,
            visor_art::hotbar_background(wheel.slot),
            x0,
            y0,
            size,
            size,
            u32::MAX,
        );
        // `SLOT_NUMBERS_IMAGE_DIFF`: the 112 pixel sheet is centred on the 98 pixel wheel.
        let diff = 7.0;
        if numbers {
            batch.sprite_scaled(
                atlas,
                "horizon/visor/hotbar_slot_numbers",
                x0 - diff,
                y0 - diff,
                size + 2.0 * diff,
                size + 2.0 * diff,
                u32::MAX,
            );
        }
        for (slot, at) in ITEM_AT.iter().enumerate() {
            if let Some(stack) = self.stack_in(slot) {
                self.draw_item(batch, stack, x0 + at[0] as f32, y0 + at[1] as f32);
            }
        }
        // The name of the slot the hand points at, under the wheel.
        if let Some(stack) = wheel.slot.and_then(|s| self.stack_in(s)) {
            let name = self.stack_display_name(stack);
            let w = HudBatch::text_width(atlas, &name.plain());
            let gap = if numbers { 4.0 + diff } else { 4.0 };
            let (tx, ty) = (cx - (w / 2.0).floor(), y0 + size + gap);
            graphics_style::fill(
                batch,
                atlas,
                [tx - 3.0, ty - 2.0, w + 6.0, 8.0 + 4.0],
                0x8000_0000,
            );
            batch.text_styled(atlas, &name, tx, ty, u32::MAX, true);
        }
        // The marks: 26 pixels, five up and left of the item.
        let mark = |batch: &mut HudBatch, slot: usize, plain: &str, centre: &str| {
            let at = ITEM_AT[slot];
            batch.sprite_scaled(
                atlas,
                if slot == 0 { centre } else { plain },
                x0 + at[0] as f32 - 5.0,
                y0 + at[1] as f32 - 5.0,
                26.0,
                26.0,
                u32::MAX,
            );
        };
        mark(
            batch,
            self.selected.min(8),
            "horizon/visor/hotbar_main_selected0",
            "horizon/visor/hotbar_main_selected1",
        );
        if self.vr_server_settings().two_handed_vr
            && let Some(off) = self.visor_offhand_slot()
        {
            mark(
                batch,
                off,
                "horizon/visor/hotbar_offhand_selected0",
                "horizon/visor/hotbar_offhand_selected1",
            );
        }
    }
}
