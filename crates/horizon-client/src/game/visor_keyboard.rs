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
use horizon_visor::overlay::{Drag, Handle, Resize};
use std::sync::Arc;
use winit::keyboard::KeyCode;

#[derive(Clone, Copy, Debug)]
enum KeyboardGrab {
    Drag { hand: usize, drag: Drag },
    Resize { hand: usize, resize: Resize },
}
impl KeyboardGrab {
    fn hand(self) -> usize {
        match self {
            Self::Drag { hand, .. } | Self::Resize { hand, .. } => hand,
        }
    }
}

/// Handles inside the keyboard's existing quad, below the keys (no extra XR layer).
fn keyboard_handles(batch: [f32; 2], buttons: &[Button]) -> [[f32; 4]; 2] {
    let bounds = keyboard::bounds(buttons);
    let at = keyboard::placement(batch, bounds);
    let y = (at[1] + (bounds[1] + bounds[3]) as f32 + 4.0).min(batch[1] - 9.0);
    let centre = batch[0] * 0.5;
    [[centre - 45.0, y, 72.0, 7.0], [centre + 32.0, y, 7.0, 7.0]]
}

/// Visor's keyboard while it is shown.
#[derive(Default)]
pub(super) struct KeyboardState {
    shown: bool,
    keyboard: Keyboard,
    /// Rebuilt only on open, Shift or a layout change, not per hand/frame/tick.
    buttons: Arc<[Button]>,
    scale: f32,
    grab: Option<KeyboardGrab>,
    handle_hover: [Option<(Handle, f32)>; 2],
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
    repeat_hand: Option<usize>,
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
        let (at, rotation) = keyboard::hang(room.head.0, room.head.1, 1.0);
        let layouts = self.visor_keyboard_layouts();
        let screen = (self.screen, self.title.is_some());
        let state = &mut self.pad.headset.keyboard;
        state.keyboard = Keyboard::new(layouts);
        state.keyboard.reopen();
        state.buttons = state.keyboard.buttons().into();
        state.repeat_hand = None;
        state.scale = headset::settings().float("visorKeyboardScale");
        state.grab = None;
        state.handle_hover = [None; 2];
        state.shown = true;
        state.screen = Some(screen);
        state.room_at = at;
        state.rotation = rotation;
        state.hover = [None; 2];
        state.down = headset::hand_buttons().map(|b| b.trigger > 0.5);
    }

    /// The binding and the VR menu use the same keyboard. In gameplay open chat so
    /// typed text has an explicit destination; mandatory server dialogs stay in place.
    pub(super) fn visor_keyboard_toggle(&mut self) {
        if self.pad.headset.keyboard.shown {
            self.visor_keyboard_close(true);
        } else {
            if !self.pad_in_menu() {
                self.open_chat("");
            }
            self.visor_keyboard_open();
        }
    }

    /// `setVisible(false)`: the keyboard goes; `dismissed` keeps it away for this screen.
    pub(super) fn visor_keyboard_close(&mut self, dismissed: bool) {
        let state = &mut self.pad.headset.keyboard;
        state.shown = false;
        state.keyboard.release();
        state.repeat_hand = None;
        state.hover = [None; 2];
        state.down = [false; 2];
        state.dismissed = dismissed;
        state.grab = None;
        state.handle_hover = [None; 2];
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
        state.shown
            && (state.hover.iter().any(Option::is_some)
                || state.handle_hover.iter().any(Option::is_some)
                || state.grab.is_some())
    }

    /// Whether the main (`off` false) or the off hand's ray is over the keyboard: that hand's
    /// pointer does not click what lies behind the keyboard.
    pub(super) fn visor_keyboard_hand_over(&self, off: bool) -> bool {
        let state = &self.pad.headset.keyboard;
        state.shown
            && (state.hover[usize::from(off)].is_some()
                || state.handle_hover[usize::from(off)].is_some()
                || state.grab.is_some_and(|g| g.hand() == usize::from(off)))
    }

    /// The distance each hand's ray runs to the keyboard (main, off), for drawing the rays.
    pub(super) fn visor_keyboard_ray_lengths(&self) -> [Option<f32>; 2] {
        let state = &self.pad.headset.keyboard;
        if !state.shown {
            return [None; 2];
        }
        std::array::from_fn(|i| {
            state.hover[i]
                .map(|(_, d)| d)
                .or_else(|| state.handle_hover[i].map(|(_, d)| d))
        })
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
            state.scale * scale * state.batch[0] / horizon_visor::overlay::GUI_PIXELS_ACROSS;
        Some((
            (state.room_at - room.rest) * scale + Vec3::Y * ((scale - 1.0) * room.rest.y),
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
        let buttons = Arc::clone(&self.pad.headset.keyboard.buttons);
        let at = keyboard::placement(self.pad.headset.keyboard.batch, keyboard::bounds(&buttons));
        let batch = self.pad.headset.keyboard.batch;
        let hands = [headset::main_hand(), headset::off_hand()];
        let triggers = headset::hand_buttons().map(|b| b.trigger > 0.5);
        let handles = keyboard_handles(batch, &buttons);
        for i in 0..2 {
            let point = hands[i].and_then(|hand| {
                keyboard::ray_uv(
                    hand.aim.0,
                    hand.aim.1 * Vec3::NEG_Z,
                    pos,
                    rot,
                    width,
                    height,
                )
            });
            self.pad.headset.keyboard.handle_hover[i] = point.and_then(|([u, v], distance)| {
                let (x, y) = (u * batch[0], v * batch[1]);
                handles
                    .iter()
                    .position(|[l, t, w, h]| x >= *l && x < l + w && y >= *t && y < t + h)
                    .map(|n| (if n == 0 { Handle::Drag } else { Handle::Resize }, distance))
            });
        }
        if self.pad.headset.keyboard.grab.is_none() {
            for i in 0..2 {
                if !triggers[i] || self.pad.headset.keyboard.down[i] {
                    continue;
                }
                let Some((handle, _)) = self.pad.headset.keyboard.handle_hover[i] else {
                    continue;
                };
                let Some(hand) = hands[i] else {
                    continue;
                };
                self.pad.headset.keyboard.grab = match handle {
                    Handle::Drag => Some(KeyboardGrab::Drag {
                        hand: i,
                        drag: Drag::start(hand.aim.0, hand.aim.1, pos, rot, headset::world_scale()),
                    }),
                    Handle::Resize => hands[1 - i].map(|other| KeyboardGrab::Resize {
                        hand: i,
                        resize: Resize::start(
                            hand.grip.0,
                            other.grip.0,
                            self.pad.headset.keyboard.scale,
                        ),
                    }),
                };
                if self.pad.headset.keyboard.grab.is_some() {
                    headset::pulse(i == 0, 0.25, 0.03);
                    break;
                }
            }
        }
        if let Some(grab) = self.pad.headset.keyboard.grab {
            self.pad.headset.keyboard.keyboard.release();
            self.pad.headset.keyboard.repeat_hand = None;
            let i = grab.hand();
            let mut save = false;
            if triggers[i] && hands[i].is_some() {
                match grab {
                    KeyboardGrab::Drag { drag, .. } => {
                        let hand = hands[i].expect("tracked grabbing hand");
                        let scale = headset::world_scale();
                        let (at, rotation) = drag.pose(hand.aim.0, hand.aim.1, scale);
                        if let Some(room) = headset::room() {
                            self.pad.headset.keyboard.room_at =
                                room.rest + (at - Vec3::Y * ((scale - 1.0) * room.rest.y)) / scale;
                            self.pad.headset.keyboard.rotation = rotation;
                        }
                    }
                    KeyboardGrab::Resize { resize, .. } => {
                        if let (Some(a), Some(b)) = (hands[0], hands[1]) {
                            self.pad.headset.keyboard.scale =
                                resize.scale_now(a.grip.0, b.grip.0).clamp(0.25, 1.5);
                        } else {
                            self.pad.headset.keyboard.grab = None;
                            save = true;
                        }
                    }
                }
            } else {
                self.pad.headset.keyboard.grab = None;
                save = matches!(grab, KeyboardGrab::Resize { .. });
            }
            self.pad.headset.keyboard.down = triggers;
            self.pad.headset.keyboard.hover = [None; 2];
            if save {
                Arc::make_mut(&mut self.settings.vr)
                    .set_float("visorKeyboardScale", self.pad.headset.keyboard.scale);
                self.settings.save(&Self::options_path());
            }
            return;
        }
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
                    log::trace!(
                        "Horizon XR: the {} hand's ray is on the keyboard's {:?} key, {distance:.2} m away",
                        if i == 0 { "main" } else { "off" },
                        buttons[k].label
                    );
                }
                headset::pulse(i == 0, 0.12, 0.01);
            }
            let down = triggers[i];
            let was = std::mem::replace(&mut self.pad.headset.keyboard.down[i], down);
            if self.pad.headset.keyboard.repeat_hand == Some(i)
                && (!down || hover.map(|h| h.0) != self.pad.headset.keyboard.keyboard.held())
            {
                self.pad.headset.keyboard.keyboard.release();
                self.pad.headset.keyboard.repeat_hand = None;
            }
            if down && !was {
                if let Some((k, _)) = hover {
                    headset::pulse(i == 0, 0.35, 0.03);
                    let state = &mut self.pad.headset.keyboard;
                    let before = (state.keyboard.layout, state.keyboard.shift);
                    let action = state.keyboard.press(&buttons, k);
                    state.repeat_hand = state.keyboard.held().map(|_| i);
                    let changed = before != (state.keyboard.layout, state.keyboard.shift);
                    if changed {
                        state.buttons = state.keyboard.buttons().into();
                        state.hover = [None; 2];
                        state.down = triggers;
                    }
                    if let Some(action) = action {
                        self.visor_keyboard_act(action);
                    }
                    if changed {
                        return;
                    }
                }
            } else if !down && was && self.pad.headset.keyboard.repeat_hand == Some(i) {
                self.pad.headset.keyboard.keyboard.release();
                self.pad.headset.keyboard.repeat_hand = None;
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
        let buttons = Arc::clone(&self.pad.headset.keyboard.buttons);
        if let Some(action) = self.pad.headset.keyboard.keyboard.tick(&buttons) {
            self.visor_keyboard_act(action);
        }
    }

    /// What a button does to the game: the text and the editing keys go where the on-screen
    /// keyboard's output went; the cross closes it; Escape is the key.
    fn visor_keyboard_act(&mut self, action: Action) {
        let key = |game: &mut Self, code: KeyCode| {
            let shifted = game.pad.headset.keyboard.keyboard.shift;
            if shifted
                && matches!(
                    code,
                    KeyCode::ArrowLeft
                        | KeyCode::ArrowRight
                        | KeyCode::ArrowUp
                        | KeyCode::ArrowDown
                        | KeyCode::Home
                        | KeyCode::End
                )
            {
                game.pad_with_modifier(KeyCode::ShiftLeft, |g| g.visor_keyboard_key(code));
            } else {
                game.visor_keyboard_key(code);
            }
        };
        match action {
            Action::Type(text) => self.pad_osk_output(osk::Output::Text(text)),
            Action::Space => self.pad_osk_output(osk::Output::Text(" ".into())),
            Action::Backspace => self.pad_osk_output(osk::Output::Backspace),
            Action::Enter => self.pad_osk_output(osk::Output::Enter),
            Action::Escape => key(self, KeyCode::Escape),
            Action::Tab => key(self, KeyCode::Tab),
            Action::Delete => key(self, KeyCode::Delete),
            Action::Home => key(self, KeyCode::Home),
            Action::End => key(self, KeyCode::End),
            Action::SelectAll => {
                self.visor_keyboard_clipboard(horizon_client::clipboard::EditShortcut::SelectAll)
            }
            Action::Up => key(self, KeyCode::ArrowUp),
            Action::Down => key(self, KeyCode::ArrowDown),
            Action::Left => key(self, KeyCode::ArrowLeft),
            Action::Right => key(self, KeyCode::ArrowRight),
            Action::Paste => {
                self.visor_keyboard_clipboard(horizon_client::clipboard::EditShortcut::Paste)
            }
            Action::Cut => {
                self.visor_keyboard_clipboard(horizon_client::clipboard::EditShortcut::Cut)
            }
            Action::Copy => {
                self.visor_keyboard_clipboard(horizon_client::clipboard::EditShortcut::Copy)
            }
            Action::Close => self.visor_keyboard_close(true),
            Action::Shift | Action::Language => {}
        }
    }

    /// Use the same selection/clipboard paths as a physical keyboard; do not append paste text.
    fn visor_keyboard_clipboard(&mut self, shortcut: horizon_client::clipboard::EditShortcut) {
        if self.dialog_open() {
            if let Err(e) = self.dialog_key(KeyCode::KeyC, Some(shortcut), None) {
                log::warn!("Horizon: dialog keyboard: {e}");
            }
        } else if self.title.is_some() {
            self.title_shortcut(shortcut);
        } else if self.screen == Screen::Chat {
            self.chat_shortcut(shortcut);
        } else if self.sign_edit.is_some() {
            self.sign_shortcut(shortcut);
        } else if self.book.is_some() {
            self.book_shortcut(shortcut);
        } else if self.screen == Screen::Creative && self.creative_screen.tab().search {
            self.creative_search_shortcut(shortcut);
        } else if self.recipe_search_focused() {
            self.recipe_shortcut(shortcut);
        } else if self.station_typing() {
            self.station_shortcut(shortcut);
        } else if self.screen == Screen::Options {
            self.options_shortcut(shortcut);
        } else if shortcut == horizon_client::clipboard::EditShortcut::Paste {
            use horizon_client::clipboard::Clipboard;
            let text = self.clipboard.get();
            if !text.is_empty() {
                self.pad_osk_output(osk::Output::Text(text));
            }
        }
    }

    fn visor_keyboard_key(&mut self, code: KeyCode) {
        if self.dialog_open() {
            if let Err(e) = self.dialog_key(code, None, None) {
                log::warn!("Horizon: dialog keyboard: {e}");
            }
            return;
        }
        let shift = self.has_shift_down();
        let ctrl = self.has_control_down_with_quirk();
        if let Some(title) = self.title.as_mut()
            && title.field_key(code, ctrl, shift)
        {
            return;
        }
        if self.screen == Screen::Creative
            && self.creative_screen.tab().search
            && self.creative_screen.search.key(code, ctrl, shift)
        {
            return;
        }
        if self.station_typing() && self.station_key(code) {
            return;
        }
        if self.screen == Screen::Chat && self.title.is_none() {
            self.chat_key_code(code, None);
        } else if self.sign_edit.is_some() {
            self.sign_input(Some(code), None);
        } else if self.book.is_some() {
            self.book_input(Some(code), None);
        } else if self.screen == Screen::Options {
            if let Err(e) = self.options_key(code, None) {
                log::warn!("Horizon: keyboard key: {e}");
            }
        } else if let Err(e) = self.key_pressed(code, code) {
            log::warn!("Horizon: keyboard key: {e}");
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
        let buttons = &state.buttons;
        let at = keyboard::placement([batch.width, batch.height], keyboard::bounds(buttons));
        let atlas = &self.gui.atlas;
        for (i, button) in buttons.iter().enumerate() {
            let hovered = state.hover.iter().flatten().any(|(k, _)| *k == i);
            let held = state.keyboard.held() == Some(i);
            self.draw_keyboard_button(&mut batch, atlas, button, at, hovered || held);
        }
        for (i, rect) in keyboard_handles([batch.width, batch.height], buttons)
            .into_iter()
            .enumerate()
        {
            let handle = if i == 0 { Handle::Drag } else { Handle::Resize };
            let lit = state.grab.is_some()
                || state
                    .handle_hover
                    .iter()
                    .flatten()
                    .any(|(h, _)| *h == handle);
            horizon_client::graphics_style::fill(
                &mut batch,
                atlas,
                rect,
                if lit { 0xA078_DCFF } else { 0x55BE_BEBE },
            );
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
        let plain = button
            .label
            .strip_prefix("\u{a7}c")
            .unwrap_or(&button.label);
        let width = HudBatch::text_width(atlas, plain);
        let tx = x + ((w as f32 - width) / 2.0).floor();
        let ty = y + ((h as f32 - 8.0) / 2.0).floor();
        // The close button's red cross (`§cx`).
        let color = if button.label.starts_with('\u{a7}') {
            0xFF55_55FF
        } else {
            color
        };
        batch.text(atlas, plain, tx, ty, color, true);
    }
}
