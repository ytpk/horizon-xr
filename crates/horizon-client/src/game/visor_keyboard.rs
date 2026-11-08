// SPDX-License-Identifier: MPL-2.0
// Ports Visor: core/client/gui/overlays/builtin/keyboard/VROverlayKeyboard.java, KeyboardButton.java, core/client/gui/screens/VRKeyboardScreen.java, core/client/gui/overlays/builtin/VROverlayGameScreen.java (onScreenChanged), mixin/client/gui/KeyboardMixins.java (opening).
//! Visor's keyboard in a headset: its own overlay panel on the keyboard slot of the second quad
//! layer (`headset::OVERLAY_KEYBOARD`), hung under the screen where `VROverlayKeyboard` hangs it
//! (0.5 m below and 0.6 m ahead of the head, facing it, then fixed in the room), pressed with
//! either hand's pointer ray and trigger. What it types goes where the on-screen keyboard's
//! output went (`pad_osk_output`: chat, signs, books, search boxes, the anvil's name, the title's
//! fields). Copyright (C) Phoenix-Ra and Visor contributors; MPL-2.0 (see LICENSE-MPL-2.0).
use super::{Game, Screen};
use glam::{Quat, Vec3};
use horizon_client::{headset, hud::HudBatch, osk, ui};
use horizon_visor::keyboard::{self, Action, Button, Keyboard, Layout};
use winit::keyboard::KeyCode;

/// Visor's keyboard while it is shown.
#[derive(Default)]
pub(super) struct KeyboardState {
    shown: bool,
    keyboard: Keyboard,
    /// The player closed it (the red cross, or Escape) for this screen: it stays closed.
    dismissed: bool,
    /// The screen it opened for; another screen (or none) closes it
    /// (`VROverlayGameScreen.onScreenChanged`).
    screen: Option<(Screen, bool)>,
    /// Where it hangs, in the room (not from the rest point, which follows room-scale walking).
    room_at: Vec3,
    rotation: Quat,
    /// The size in GUI pixels of the image drawn last: where a ray on the panel is on the keyboard.
    batch: [f32; 2],
    /// Each hand's (main, off) ray: the key it is over and how far away the panel is.
    hover: [Option<(usize, f32)>; 2],
    /// Each hand's trigger was down last frame.
    down: [bool; 2],
}

impl Game<'_> {
    /// Visor's keyboard is the headset's keyboard (`visorKeyboard`; off brings Horizon's own
    /// back).
    pub(super) fn visor_keyboard_wanted(&self) -> bool {
        headset::active() && headset::settings().bool("visorKeyboard")
    }

    /// The layouts the language button walks (`getEffectiveSelected`).
    fn visor_keyboard_layouts(&self) -> Vec<Layout> {
        let settings = headset::settings();
        Layout::effective(
            settings.string("visorKeyboardLayouts"),
            settings.bool("visorKeyboardAutoLayout"),
            &self.settings.language,
        )
    }

    /// `VROverlayKeyboard.setVisible(true, screen)`: shown for the open screen, hung ahead of the
    /// head, shift off and the first layout.
    pub(super) fn visor_keyboard_open(&mut self) {
        if self.pad.headset.keyboard.shown {
            return;
        }
        let Some(room) = headset::room() else {
            return;
        };
        let (at, rotation) = keyboard::hang(room.head.0, room.head.1, headset::world_scale());
        let layouts = self.visor_keyboard_layouts();
        let screen = (self.screen, self.title.is_some());
        let state = &mut self.pad.headset.keyboard;
        state.keyboard = Keyboard::new(layouts);
        state.keyboard.reopen();
        state.shown = true;
        state.screen = Some(screen);
        state.room_at = at;
        state.rotation = rotation;
        state.hover = [None; 2];
        state.down = [false; 2];
    }

    /// `setVisible(false)`: the keyboard goes; `dismissed` keeps it away for this screen.
    pub(super) fn visor_keyboard_close(&mut self, dismissed: bool) {
        let state = &mut self.pad.headset.keyboard;
        state.shown = false;
        state.keyboard.release();
        state.hover = [None; 2];
        state.down = [false; 2];
        state.dismissed = dismissed;
        headset::set_overlay_at(headset::OVERLAY_KEYBOARD, None);
    }

    /// Each frame with a screen open: it closes with the screen it was opened for, and opens with
    /// the chat, a sign or a book (`VROverlayGameScreen.onScreenChanged`, the sign and book
    /// screens' `init`) once per screen. A click on a text field opens it too
    /// (`visor_keyboard_clicked`).
    pub(super) fn visor_keyboard_auto(&mut self, typing: bool) {
        let here = (self.screen, self.title.is_some());
        let state = &mut self.pad.headset.keyboard;
        if state.screen != Some(here) {
            // Another screen: the keyboard of the old one is gone, and the new one is a fresh start.
            let was = state.shown;
            state.screen = Some(here);
            state.dismissed = false;
            if was {
                self.visor_keyboard_close(false);
            }
        }
        if typing && !self.pad.headset.keyboard.dismissed && !self.pad.headset.keyboard.shown {
            self.visor_keyboard_open();
        }
    }

    /// `KeyboardMixins.EditBoxMixin`: a click on a text field opens the keyboard.
    pub(super) fn visor_keyboard_clicked(&mut self) {
        if !self.pad.headset.keyboard.shown {
            self.visor_keyboard_open();
        }
    }

    /// No screen: the keyboard goes with it.
    pub(super) fn visor_keyboard_no_screen(&mut self) {
        if self.pad.headset.keyboard.shown || self.pad.headset.keyboard.screen.is_some() {
            self.visor_keyboard_close(false);
            self.pad.headset.keyboard.screen = None;
        }
    }

    /// Whether a hand's ray is over the keyboard: a trigger press there is no click on the screen
    /// beside it.
    pub(super) fn visor_keyboard_hovered(&self) -> bool {
        let state = &self.pad.headset.keyboard;
        state.shown && state.hover.iter().any(Option::is_some)
    }

    /// Whether the main (`off` false) or the off hand's ray is over the keyboard: that hand's
    /// pointer does not click what lies behind the keyboard.
    pub(super) fn visor_keyboard_hand_over(&self, off: bool) -> bool {
        let state = &self.pad.headset.keyboard;
        state.shown && state.hover[usize::from(off)].is_some()
    }

    /// The distance each hand's ray runs to the keyboard (main, off), for drawing the rays.
    pub(super) fn visor_keyboard_ray_lengths(&self) -> [Option<f32>; 2] {
        let state = &self.pad.headset.keyboard;
        if !state.shown {
            return [None; 2];
        }
        state.hover.map(|h| h.map(|(_, distance)| distance))
    }

    /// The panel's place in the play space (from the rest point) and its size in metres.
    fn visor_keyboard_panel(&self) -> Option<(Vec3, Quat, f32, f32)> {
        let state = &self.pad.headset.keyboard;
        let room = headset::room()?;
        let scale = headset::world_scale();
        if state.batch[0] <= 0.0 {
            return None;
        }
        let width =
            keyboard::SCALE * scale * state.batch[0] / horizon_visor::overlay::GUI_PIXELS_ACROSS;
        Some((
            state.room_at - room.rest,
            state.rotation,
            width,
            width * 9.0 / 16.0,
        ))
    }

    /// Each frame while it is shown: where each hand's ray meets a key (hover, with a pulse when it
    /// changes), and the triggers that press and release (`KeyboardButton.onPress`/`onRelease`).
    pub(super) fn visor_keyboard_frame(&mut self) {
        if !self.pad.headset.keyboard.shown {
            return;
        }
        let Some((pos, rot, width, height)) = self.visor_keyboard_panel() else {
            return;
        };
        let buttons = self.pad.headset.keyboard.keyboard.buttons();
        let at = keyboard::placement(self.pad.headset.keyboard.batch, keyboard::bounds(&buttons));
        let batch = self.pad.headset.keyboard.batch;
        let hands = [headset::main_hand(), headset::off_hand()];
        let triggers = headset::hand_buttons().map(|b| b.trigger > 0.5);
        for i in 0..2 {
            let hover = hands[i].and_then(|hand| {
                let origin = hand.aim.0;
                let dir = hand.aim.1 * Vec3::NEG_Z;
                let ([u, v], distance) = keyboard::ray_uv(origin, dir, pos, rot, width, height)?;
                let (x, y) = (u * batch[0] - at[0], v * batch[1] - at[1]);
                keyboard::button_at(&buttons, x, y).map(|k| (k, distance))
            });
            let previous = self.pad.headset.keyboard.hover[i];
            self.pad.headset.keyboard.hover[i] = hover;
            // A pulse when the ray comes onto another key (the main hand is `true`).
            if hover.map(|h| h.0) != previous.map(|h| h.0) && hover.is_some() {
                if let Some((k, distance)) = hover {
                    log::info!(
                        "Horizon XR: the {} hand's ray is on the keyboard's {:?} key, {distance:.2} m away",
                        if i == 0 { "main" } else { "off" },
                        buttons[k].label
                    );
                }
                headset::pulse(i == 0, 0.12, 0.01);
            }
            let down = triggers[i];
            let was = std::mem::replace(&mut self.pad.headset.keyboard.down[i], down);
            if down && !was {
                if let Some((k, _)) = hover {
                    headset::pulse(i == 0, 0.35, 0.03);
                    let action = self.pad.headset.keyboard.keyboard.press(&buttons, k);
                    if let Some(action) = action {
                        self.visor_keyboard_act(action);
                    }
                }
            } else if !down && was {
                self.pad.headset.keyboard.keyboard.release();
            }
            if !self.pad.headset.keyboard.shown {
                return;
            }
        }
    }

    /// One tick (`VRKeyboardScreen.tick`): a key held for a second repeats every tick.
    pub(super) fn visor_keyboard_tick(&mut self) {
        if !self.pad.headset.keyboard.shown {
            return;
        }
        let buttons = self.pad.headset.keyboard.keyboard.buttons();
        if let Some(action) = self.pad.headset.keyboard.keyboard.tick(&buttons) {
            self.visor_keyboard_act(action);
        }
    }

    /// What a button does to the game: the text and the editing keys go where the on-screen
    /// keyboard's output went; the cross closes it; Escape is the key.
    fn visor_keyboard_act(&mut self, action: Action) {
        let key = |game: &mut Self, code: KeyCode| {
            if let Err(e) = game.key_pressed(code, code) {
                log::warn!("Horizon: keyboard key: {e}");
            }
        };
        match action {
            Action::Type(text) => self.pad_osk_output(osk::Output::Text(text)),
            Action::Space => self.pad_osk_output(osk::Output::Text(" ".into())),
            Action::Backspace => self.pad_osk_output(osk::Output::Backspace),
            Action::Enter => self.pad_osk_output(osk::Output::Enter),
            Action::Escape => key(self, KeyCode::Escape),
            Action::Tab => key(self, KeyCode::Tab),
            Action::Up => key(self, KeyCode::ArrowUp),
            Action::Down => key(self, KeyCode::ArrowDown),
            Action::Left => key(self, KeyCode::ArrowLeft),
            Action::Right => key(self, KeyCode::ArrowRight),
            Action::Paste => {
                use horizon_client::clipboard::Clipboard;
                let text = self.clipboard.get();
                if !text.is_empty() {
                    self.pad_osk_output(osk::Output::Text(text));
                }
            }
            // Horizon's fields keep no selection a button could cut or copy.
            Action::Cut | Action::Copy => {}
            Action::Close => self.visor_keyboard_close(true),
            Action::Shift | Action::Language => {}
        }
    }

    /// The keyboard's overlay panel and its picture, or none (`headset::set_overlay_at`).
    pub(super) fn visor_keyboard_overlay(&mut self, size: [u32; 2]) -> Option<HudBatch> {
        if !self.pad.headset.keyboard.shown || !headset::active() {
            headset::set_overlay_at(headset::OVERLAY_KEYBOARD, None);
            return None;
        }
        let mut batch = HudBatch::new(size);
        self.pad.headset.keyboard.batch = [batch.width, batch.height];
        let Some((pos, rot, width, _)) = self.visor_keyboard_panel() else {
            headset::set_overlay_at(headset::OVERLAY_KEYBOARD, None);
            return None;
        };
        headset::set_overlay_at(
            headset::OVERLAY_KEYBOARD,
            Some(headset::OverlaySpec::new(pos, rot, width)),
        );
        let state = &self.pad.headset.keyboard;
        let buttons = state.keyboard.buttons();
        let at = keyboard::placement([batch.width, batch.height], keyboard::bounds(&buttons));
        let atlas = &self.gui.atlas;
        for (i, button) in buttons.iter().enumerate() {
            let hovered = state.hover.iter().flatten().any(|(k, _)| *k == i);
            let held = state.keyboard.held() == Some(i);
            self.draw_keyboard_button(&mut batch, atlas, button, at, hovered || held);
        }
        Some(batch)
    }

    /// `KeyboardButton`: a vanilla button, lit while a ray is on it or it is held, grey when dead,
    /// its label centred.
    fn draw_keyboard_button(
        &self,
        batch: &mut HudBatch,
        atlas: &horizon_client::hud::HudAtlas,
        button: &Button,
        at: [f32; 2],
        lit: bool,
    ) {
        let [x, y, w, h] = button.rect;
        let (x, y) = (x as f32 + at[0], y as f32 + at[1]);
        let sprite = if !button.active {
            "gui/sprites/widget/button_disabled"
        } else if lit {
            "gui/sprites/widget/button_highlighted"
        } else {
            "gui/sprites/widget/button"
        };
        ui::nine_slice(batch, atlas, sprite, x, y, w as f32, h as f32, u32::MAX);
        let color = if !button.active {
            0xFFA0_A0A0
        } else if lit {
            0xFFA0_FFFF
        } else {
            u32::MAX
        };
        let plain: String = button.label.replace("\u{a7}c", "");
        let width = HudBatch::text_width(atlas, &plain);
        let tx = x + ((w as f32 - width) / 2.0).floor();
        let ty = y + ((h as f32 - 8.0) / 2.0).floor();
        // The close button's red cross (`§cx`).
        let color = if button.label.starts_with('\u{a7}') {
            0xFF55_55FF
        } else {
            color
        };
        batch.text(atlas, &plain, tx, ty, color, true);
    }
}
