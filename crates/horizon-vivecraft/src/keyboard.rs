// SPDX-License-Identifier: LGPL-3.0-or-later
//! Ports `PhysicalKeyboard`, `KeyboardKeys`, `KeyboardTheme` and the keyboard half of
//! `KeyboardHandler` (placement): the 3D keyboard poked by the controllers. The layouts are
//! `VRSettings.getDefaultKeyboardLayouts`. Copyright (c) Vivecraft contributors;
//! LGPL-3.0-or-later (see LICENSE-LGPL-3.0).
//!
//! Keys live in keyboard space (metres, x right, y down the rows, z away from the player);
//! [`PhysicalKeyboard::to_local`] brings a controller there. Drawing and typing are the
//! game's: this module says which keys are down and what they do.

use crate::vr_data::RoomPose;
use glam::{Quat, Vec3};

pub const COLUMNS: i32 = 13;
pub const ROWS: i32 = 4;
pub const SPECIAL_KEY_WIDTH: i32 = 2;

const SPACING: f32 = 0.0064;
const KEY_WIDTH: f32 = 0.04;
const KEY_HEIGHT: f32 = 0.04;
const KEY_WIDTH_SPECIAL: f32 = KEY_WIDTH * 2.0 + SPACING;

/// `getDefaultKeyboardLayouts`: id, name and the regular and shifted characters (row by row,
/// `COLUMNS` to a row). `custom` and `system` are the English (US) keys here.
pub const LAYOUTS: &[(&str, &str, &str, &str)] = &[
    (
        "en_us",
        "English (US)",
        "`1234567890-=qwertyuiop[]\\asdfghjkl;':\"zxcvbnm,./?<>",
        "~!@#$%^&*()_+QWERTYUIOP{}|ASDFGHJKL;':\"ZXCVBNM,./?<>",
    ),
    (
        "en_gb",
        "English (UK)",
        "`1234567890-=qwertyuiop[]#asdfghjkl;:'\\zxcvbnm,./?<>",
        "¬!\"£$%^&*()_+QWERTYUIOP{}~ASDFGHJKL;:@|ZXCVBNM,./?<>",
    ),
    (
        "de_de",
        "German",
        "^1234567890ß´qwertzuiopü+#asdfghjklöä~|yxcvbnm,.-{}<",
        "°!\"€$%&/()=?`QWERTZUIOPÜ*'ASDFGHJKLÖÄ@\\YXCVBNM;:_[]>",
    ),
    (
        "fr_fr",
        "French",
        "²&é\"'(-è_çà)=azertyuiop^$*qsdfghjklmù@#wxcvbn,;:![]<",
        "²1234567890°+AZERTYUIOP¨£µQSDFGHJKLM%¤~WXCVBN?./\\{}>",
    ),
    (
        "fr_be",
        "French (Belgium)",
        "²&é\"'(\\è!çà)-azertyuiop^$µqsdfghjklmù@#wxcvbn,;:=[]<",
        "³1234567890°_AZERTYUIOP¨*£QSDFGHJKLM%¤~WXCVBN?./+{}>",
    ),
    (
        "be_by",
        "Belarusian",
        "ё1234567890-=йцукенгшўзх'\\фывапролджэ:\"ячсмітьбю.?<>",
        "Ё!\"№;%:?*()_+ЙЦУКЕНГШЎЗХ'/ФЫВАПРОЛДЖЭ:\"ЯЧСМІТЬБЮ,?<>",
    ),
    (
        "uk_ua",
        "Ukrainian",
        "'1234567890-=йцукенгшщзхїґфівапролджє/\"ячсмитьбю.?<>",
        "'!\"№;%:?*()_+ЙЦУКЕНГШЩЗХЇҐФІВАПРОЛДЖЄ\\\"ЯЧСМИТЬБЮ,?<>",
    ),
    (
        "ru_ru",
        "Russian",
        "ё1234567890-=йцукенгшщзхъ\\фывапролджэ:\"ячсмитьбю.?<>",
        "Ё!\"№;%:?*()_+ЙЦУКЕНГШЩЗХЪ/ФЫВАПРОЛДЖЭ:\"ЯЧСМИТЬБЮ,?<>",
    ),
];

/// The characters of layout `id` (`custom` and `system` fall back to English (US)).
pub fn layout_chars(id: &str) -> (&'static str, &'static str) {
    let l = LAYOUTS.iter().find(|l| l.0 == id).unwrap_or(&LAYOUTS[0]);
    (l.2, l.3)
}

/// `VRSettings.getKeyboardLayout`: the id the settings point at (`keyboardLayoutOrder` and
/// `currentKeyboardLayout`).
pub fn current_layout_id(s: &crate::settings::VrSettings) -> &str {
    let i = usize::try_from(s.int("currentKeyboardLayout")).unwrap_or(0);
    s.string("keyboardLayoutOrder")
        .split('\n')
        .filter(|i| !i.is_empty())
        .nth(i)
        .unwrap_or("en_us")
}

/// `nextKeyboardLayout`: the settings' next layout index.
pub fn next_layout_index(s: &crate::settings::VrSettings) -> i32 {
    let n = s
        .string("keyboardLayoutOrder")
        .split('\n')
        .filter(|i| !i.is_empty())
        .count()
        .max(1) as i32;
    (s.int("currentKeyboardLayout") + 1).rem_euclid(n)
}

/// What a key does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyAction {
    Char(char),
    Space,
    Tab,
    Escape,
    Backspace,
    Enter,
    Up,
    Down,
    Left,
    Right,
    Paste,
    Shift,
    NextLayout,
}

/// What a key shows: its character, a language key (`vivecraft.keyboard.key.<name>`) or a
/// symbol.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyLabel {
    Char(char),
    Lang(&'static str),
    Symbol(&'static str),
}

/// `KeyboardKeys.Key`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Key {
    /// 0-499 regular, 500-999 shifted, 1000+ special.
    pub id: i32,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub label: KeyLabel,
    pub action: KeyAction,
}

const SHIFT_1: i32 = 1000;
const SHIFT_2: i32 = 1001;

impl Key {
    pub fn is_shift(&self) -> bool {
        self.id == SHIFT_1 || self.id == SHIFT_2
    }
    pub fn is_regular(&self) -> bool {
        self.id < 1000
    }
}

/// `KeyboardKeys.getSpecialKeys` (without cut and copy, which need the OS clipboard; and
/// without the layout selection screen's icon).
pub fn special_keys() -> Vec<Key> {
    let w = SPECIAL_KEY_WIDTH;
    let mut id = SHIFT_1;
    let mut key = |x: i32, y: i32, width: i32, label: KeyLabel, action: KeyAction| {
        let k = Key {
            id,
            x,
            y,
            width,
            height: 1,
            label,
            action,
        };
        id += 1;
        k
    };
    let wide = |name: &'static str| KeyLabel::Lang(name);
    vec![
        key(0, 4, 2, wide("shift"), KeyAction::Shift),
        key(w + COLUMNS, 4, 2, wide("shift"), KeyAction::Shift),
        key(
            w + (COLUMNS - 5) / 2,
            -1,
            5,
            KeyLabel::Char(' '),
            KeyAction::Space,
        ),
        key(0, 2, 2, wide("tab"), KeyAction::Tab),
        key(0, 1, 2, wide("esc"), KeyAction::Escape),
        key(w + COLUMNS, 1, 2, wide("backspace"), KeyAction::Backspace),
        key(w + COLUMNS, 3, 2, wide("enter"), KeyAction::Enter),
        key(
            w + COLUMNS + 1,
            5,
            1,
            KeyLabel::Symbol("\u{2191}"),
            KeyAction::Up,
        ),
        key(
            w + COLUMNS + 1,
            6,
            1,
            KeyLabel::Symbol("\u{2193}"),
            KeyAction::Down,
        ),
        key(
            w + COLUMNS,
            6,
            1,
            KeyLabel::Symbol("\u{2190}"),
            KeyAction::Left,
        ),
        key(
            w + COLUMNS + 2,
            6,
            1,
            KeyLabel::Symbol("\u{2192}"),
            KeyAction::Right,
        ),
        key(w * 3, 0, 2, wide("paste"), KeyAction::Paste),
        key(w * 4, 0, 2, wide("switch"), KeyAction::NextLayout),
    ]
}

/// `KeyboardKeys.getRegularKeys`: the grid of characters (`chars` is the layout's row-major
/// string) and its number of rows.
pub fn regular_keys(chars: &str, shift: bool) -> (Vec<Key>, i32) {
    let chars: Vec<char> = chars.chars().collect();
    let calc = chars.len() as f32 / COLUMNS as f32;
    let rows = if (ROWS as f32 - calc).abs() > 0.01 {
        calc.ceil() as i32
    } else {
        ROWS
    };
    let mut keys = Vec::with_capacity((rows * COLUMNS) as usize);
    for row in 0..rows {
        for column in 0..COLUMNS {
            let index = (row * COLUMNS + column) as usize;
            let ch = chars.get(index).copied().unwrap_or('\0');
            keys.push(Key {
                id: if shift {
                    index as i32 + 500
                } else {
                    index as i32
                },
                x: column + SPECIAL_KEY_WIDTH,
                y: 1 + row,
                width: 1,
                height: 1,
                label: KeyLabel::Char(ch),
                action: KeyAction::Char(ch),
            });
        }
    }
    (keys, rows)
}

/// `KeyboardTheme`: the colour (rgb) the theme gives key `id` at slot `(x, y)`.
pub fn theme_color(theme: &str, id: i32, x: i32, y: i32) -> [f32; 3] {
    match theme {
        "RED" => [1.0, 0.0, 0.0],
        "GREEN" => [0.0, 1.0, 0.0],
        "BLUE" => [0.0, 0.0, 1.0],
        "BLACK" => [0.0, 0.0, 0.0],
        "GRASS" => {
            if (0..2).contains(&y) {
                [0.321, 0.584, 0.184]
            } else {
                [0.607, 0.462, 0.325]
            }
        }
        "BEES" => {
            if x.rem_euclid(4) < 2 {
                [1.0, 1.0, 0.0]
            } else {
                [0.0, 0.0, 0.0]
            }
        }
        "AESTHETIC" => {
            if id >= 1000 {
                [0.0, 1.0, 1.0]
            } else {
                [1.0, 0.0, 1.0]
            }
        }
        "DOSE" => {
            if id % 2 == 0 {
                [0.5, 0.0, 1.0]
            } else {
                [0.0, 1.0, 0.0]
            }
        }
        // DEFAULT, and CUSTOM (keyboardtheme.txt is not read).
        _ => [1.0, 1.0, 1.0],
    }
}

/// One key in keyboard space.
#[derive(Clone, Copy, Debug)]
pub struct KeyButton {
    pub key: Key,
    /// Box corners, metres; z from 0 (the face the player sees) to the key's depth.
    pub min: Vec3,
    pub max: Vec3,
    pub pressed: bool,
}

impl KeyButton {
    /// `getRenderBoundingBox`: a pressed key sinks 1.2 cm (scaled).
    pub fn render_box(&self, scale: f32) -> (Vec3, Vec3) {
        if self.pressed {
            let d = Vec3::new(0.0, 0.0, 0.012 * scale);
            (self.min + d, self.max + d)
        } else {
            (self.min, self.max)
        }
    }

    /// `getCollisionBoundingBox`: a pressed key reaches 8 cm further so a hand stays on it.
    fn touches(&self, p: Vec3) -> bool {
        let max_z = if self.pressed {
            self.max.z + 0.08
        } else {
            self.max.z
        };
        p.x >= self.min.x
            && p.x < self.max.x
            && p.y >= self.min.y
            && p.y < self.max.y
            && p.z >= self.min.z
            && p.z < max_z
    }

    /// `getRenderColor` without the shift key's own colour: the theme's colour at half
    /// strength unless pressed, translucent.
    pub fn color(&self, theme: &str) -> [f32; 4] {
        let [r, g, b] = theme_color(theme, self.key.id, self.key.x, self.key.y);
        let k = if self.pressed { 1.0 } else { 0.5 };
        [r * k, g * k, b * k, 0.5]
    }
}

/// What the player did on the keyboard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyEvent {
    /// A key went down under a controller (0 main, 1 off); `repeat` while it is held.
    Press {
        action: KeyAction,
        controller: usize,
        repeat: bool,
    },
    Release(KeyAction),
}

/// `PhysicalKeyboard`.
#[derive(Clone, Debug, Default)]
pub struct PhysicalKeyboard {
    pub shift: bool,
    pub shift_sticky: bool,
    keys: Vec<KeyButton>,
    rows: i32,
    pub scale: f32,
    pressed_key: [Option<i32>; 2],
    press_time: [u64; 2],
    repeat_time: [u64; 2],
    shift_press_time: u64,
    last_pressed_shift: bool,
    reinit: bool,
    /// Counts the layouts built, so a drawer can tell its cache is old.
    pub generation: u32,
    regular: String,
    shifted: String,
}

impl PhysicalKeyboard {
    pub fn keys(&self) -> &[KeyButton] {
        &self.keys
    }

    pub fn has_keys(&self) -> bool {
        !self.keys.is_empty()
    }

    /// The (unshifted, shifted) characters in use have changed or the scale did: build again.
    pub fn set_layout(&mut self, regular: &str, shifted: &str, scale: f32) {
        if self.regular != regular || self.shifted != shifted || self.scale != scale {
            self.regular = regular.to_string();
            self.shifted = shifted.to_string();
            self.scale = scale;
            self.reinit = true;
        }
    }

    /// `show`: a sticky shift stays, else the keys are unshifted.
    pub fn show(&mut self) {
        if !self.shift_sticky {
            self.shift = false;
        }
        self.reinit = true;
    }

    pub fn needs_rebuild(&self) -> bool {
        self.reinit
    }

    /// `getCenterPos`: the point of keyboard space that sits at the keyboard's position.
    pub fn center(&self) -> Vec3 {
        let (kw, kh, kws, sp) = (
            KEY_WIDTH * self.scale,
            KEY_HEIGHT * self.scale,
            KEY_WIDTH_SPECIAL * self.scale,
            SPACING * self.scale,
        );
        let columns = COLUMNS as f32;
        Vec3::new(
            ((kw + sp) * (columns + columns % 2.0 / 2.0) + (kws + sp) * 2.0) / 2.0,
            (kh + sp) * (self.rows + 1) as f32,
            0.0,
        )
    }

    /// `init`: lays the keys out; the keys held stay held.
    pub fn rebuild(&mut self) {
        let held = self.pressed_key;
        self.keys.clear();
        let (spacing, kw, kh) = (
            SPACING * self.scale,
            KEY_WIDTH * self.scale,
            KEY_HEIGHT * self.scale,
        );
        let chars = if self.shift {
            &self.shifted
        } else {
            &self.regular
        };
        let (regular, rows) = regular_keys(chars, self.shift);
        self.rows = rows;
        let depth = 0.028 * self.scale;
        let add = |key: Key, rows: i32, keys: &mut Vec<KeyButton>| {
            let y = if key.y < 0 { rows - key.y } else { key.y };
            let (x0, y0) = (
                key.x as f32 * (kw + spacing),
                (y - 1) as f32 * (kh + spacing),
            );
            let w = key.width as f32 * kw + (key.width - 1) as f32 * spacing;
            let h = key.height as f32 * kh + (key.height - 1) as f32 * spacing;
            keys.push(KeyButton {
                key,
                min: Vec3::new(x0, y0, 0.0),
                max: Vec3::new(x0 + w, y0 + h, depth),
                pressed: false,
            });
        };
        let mut keys = Vec::new();
        for key in regular {
            add(key, rows, &mut keys);
        }
        for key in special_keys() {
            add(key, rows, &mut keys);
        }
        self.keys = keys;
        for held_id in held.into_iter().flatten() {
            if let Some(k) = self.keys.iter_mut().find(|k| k.key.id == held_id) {
                k.pressed = true;
            }
        }
        self.reinit = false;
        self.generation = self.generation.wrapping_add(1);
    }

    pub fn set_shift(&mut self, shift: bool, sticky: bool) {
        if shift != self.shift || sticky != self.shift_sticky {
            self.shift = shift;
            self.shift_sticky = shift && sticky;
            self.reinit = true;
        }
    }

    /// The colour of a key including the active shift key's own (`init`'s override).
    pub fn key_color(&self, k: &KeyButton, theme: &str) -> [f32; 4] {
        if k.key.is_shift() && self.shift {
            let v = if k.pressed { 1.0 } else { 0.5 };
            return [if self.shift_sticky { v } else { 0.0 }, v, 0.0, 0.5];
        }
        k.color(theme)
    }

    /// Releases every key held (`unpressAllKeys`).
    pub fn unpress_all(&mut self, events: &mut Vec<KeyEvent>) {
        for k in &mut self.keys {
            if k.pressed {
                k.pressed = false;
                events.push(KeyEvent::Release(k.key.action));
            }
        }
        self.pressed_key = [None; 2];
    }

    /// A point of the room as keyboard space has it, for a keyboard at `pos`/`rot` in the room.
    pub fn to_local(&self, pos: Vec3, rot: Quat, room_point: Vec3) -> Vec3 {
        self.center() + rot.conjugate() * (room_point - pos)
    }

    /// `KeyboardHandler.processBindings` for the keyboard: the shift binding held makes a
    /// sticky shift while it is.
    pub fn shift_binding(&mut self, down: bool) {
        if down && !self.last_pressed_shift {
            self.set_shift(true, true);
            self.last_pressed_shift = true;
        } else if !down && self.last_pressed_shift {
            self.set_shift(false, false);
            self.last_pressed_shift = false;
        }
    }

    /// `process`: the controllers' tips (keyboard space, `None` untracked) press and release
    /// keys. A key goes down after 150 ms on a new one, repeats after 500 ms every 100 ms.
    pub fn process(&mut self, now_ms: u64, tips: [Option<Vec3>; 2], events: &mut Vec<KeyEvent>) {
        if self.reinit {
            self.rebuild();
        }
        for c in 0..2 {
            let touched = tips[c].and_then(|p| self.keys.iter().position(|k| k.touches(p)));
            let touched_id = touched.map(|i| self.keys[i].key.id);
            if let Some(i) = touched {
                if touched_id != self.pressed_key[c]
                    && now_ms.saturating_sub(self.press_time[c]) >= 150
                {
                    if let Some(prev) = self.pressed_key[c].take() {
                        self.release(prev, events);
                    }
                    self.press(i, c, false, now_ms, events);
                    self.pressed_key[c] = touched_id;
                    self.press_time[c] = now_ms;
                    self.repeat_time[c] = now_ms;
                } else if touched_id == self.pressed_key[c]
                    && now_ms.saturating_sub(self.press_time[c]) >= 500
                    && now_ms.saturating_sub(self.repeat_time[c]) >= 100
                {
                    self.press(i, c, true, now_ms, events);
                    self.repeat_time[c] = now_ms;
                }
            } else if let Some(prev) = self.pressed_key[c].take() {
                self.release(prev, events);
                self.press_time[c] = now_ms;
            }
        }
    }

    fn release(&mut self, id: i32, events: &mut Vec<KeyEvent>) {
        if let Some(k) = self.keys.iter_mut().find(|k| k.key.id == id) {
            k.pressed = false;
            events.push(KeyEvent::Release(k.key.action));
        }
    }

    fn press(
        &mut self,
        i: usize,
        controller: usize,
        repeat: bool,
        now_ms: u64,
        events: &mut Vec<KeyEvent>,
    ) {
        let key = self.keys[i].key;
        self.keys[i].pressed = true;
        events.push(KeyEvent::Press {
            action: key.action,
            controller,
            repeat,
        });
        if key.is_regular() && !self.shift_sticky {
            // The layout's `afterPress`: a typed character ends a one-shot shift.
            self.set_shift(false, false);
        } else if key.is_shift() && !repeat {
            // A second tap within 400 ms makes the shift stick.
            if self.shift
                && !self.shift_sticky
                && now_ms.saturating_sub(self.shift_press_time) < 400
            {
                self.set_shift(true, true);
            } else {
                self.set_shift(!self.shift, false);
            }
            self.shift_press_time = now_ms;
        }
    }
}

/// `KeyboardHandler.orientOverlay` for the physical keyboard: 0.5 m below the head and 0.3 m
/// ahead of it, tilted back like a real one. Returns the keyboard's room position and rotation.
pub fn orient(hmd: &RoomPose) -> (Vec3, Quat) {
    let d = hmd.dir();
    let yaw = (-d.x).atan2(d.z);
    let offset = Quat::from_rotation_y(-yaw) * Vec3::new(0.0, -0.5, 0.3);
    let rot = Quat::from_rotation_y(std::f32::consts::PI - yaw)
        * Quat::from_rotation_x(std::f32::consts::PI * 0.8);
    (hmd.pos + offset, rot)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keyboard() -> PhysicalKeyboard {
        let (r, s) = layout_chars("en_us");
        let mut k = PhysicalKeyboard::default();
        k.set_layout(r, s, 1.0);
        k.show();
        k.rebuild();
        k
    }

    /// The tip that touches the middle of key `id`, 5 mm inside its face.
    fn tip_on(k: &PhysicalKeyboard, id: i32) -> Vec3 {
        let b = k.keys().iter().find(|b| b.key.id == id).unwrap();
        (b.min + b.max) / 2.0 * Vec3::new(1.0, 1.0, 0.0) + Vec3::new(0.0, 0.0, 0.005)
    }

    #[test]
    fn the_english_grid_is_four_rows_of_thirteen() {
        for (_, _, regular, shifted) in LAYOUTS {
            assert_eq!(regular.chars().count(), 52, "{regular}");
            assert_eq!(shifted.chars().count(), 52, "{shifted}");
        }
        let (keys, rows) = regular_keys(layout_chars("en_us").0, false);
        assert_eq!((keys.len(), rows), (52, 4));
        // q is the first key of the second row, after the two special columns.
        let q = keys
            .iter()
            .find(|k| k.action == KeyAction::Char('q'))
            .unwrap();
        assert_eq!((q.x, q.y), (SPECIAL_KEY_WIDTH, 2));
    }

    #[test]
    fn touching_a_key_types_it_after_the_delay_and_leaving_releases_it() {
        let mut k = keyboard();
        let q = keys_id(&k, 'q');
        let mut events = Vec::new();
        k.process(1000, [Some(tip_on(&k, q)), None], &mut events);
        assert_eq!(
            events,
            vec![KeyEvent::Press {
                action: KeyAction::Char('q'),
                controller: 0,
                repeat: false
            }]
        );
        // Held on it: nothing, then repeats after half a second.
        events.clear();
        let tip = tip_on(&k, q);
        k.process(1200, [Some(tip), None], &mut events);
        assert!(events.is_empty());
        k.process(1600, [Some(tip), None], &mut events);
        assert!(matches!(events[..], [KeyEvent::Press { repeat: true, .. }]));
        events.clear();
        k.process(1700, [None, None], &mut events);
        assert_eq!(events, vec![KeyEvent::Release(KeyAction::Char('q'))]);
    }

    fn keys_id(k: &PhysicalKeyboard, ch: char) -> i32 {
        k.keys()
            .iter()
            .find(|b| b.key.action == KeyAction::Char(ch))
            .unwrap()
            .key
            .id
    }

    #[test]
    fn shift_is_one_shot_and_a_double_tap_sticks() {
        let mut k = keyboard();
        let shift = SHIFT_1;
        let mut ev = Vec::new();
        k.process(1000, [Some(tip_on(&k, shift)), None], &mut ev);
        assert!(k.shift && !k.shift_sticky);
        // The next process rebuilds with the shifted characters; a letter types and unshifts.
        k.process(1300, [None, None], &mut ev);
        assert!(
            k.keys()
                .iter()
                .any(|b| b.key.action == KeyAction::Char('Q'))
        );
        let q = k
            .keys()
            .iter()
            .find(|b| b.key.action == KeyAction::Char('Q'))
            .unwrap()
            .key
            .id;
        k.process(1500, [Some(tip_on(&k, q)), None], &mut ev);
        assert!(!k.shift);
        // Double tap within 400 ms of the previous press: sticky.
        let mut k = keyboard();
        k.process(1000, [Some(tip_on(&k, shift)), None], &mut ev);
        k.process(1160, [None, None], &mut ev);
        k.process(1320, [Some(tip_on(&k, shift)), None], &mut ev);
        assert!(k.shift_sticky);
    }

    #[test]
    fn the_keyboard_sits_below_and_ahead_of_the_head_face_up() {
        let hmd = RoomPose {
            pos: Vec3::new(0.0, 1.7, 0.0),
            rot: Quat::IDENTITY,
        };
        let (pos, rot) = orient(&hmd);
        // A head with no turn looks along -Z in the room frame (yaw pi in Vivecraft's terms).
        assert!((pos.y - 1.2).abs() < 1e-5);
        assert!((pos - hmd.pos).length() > 0.5);
        // A key's face looks up and toward the player (keyboard -Z).
        let toward_player = rot * Vec3::NEG_Z;
        assert!(toward_player.y > 0.3, "{toward_player}");
        let to_head = (hmd.pos - pos).normalize();
        assert!(toward_player.dot(to_head) > 0.5);
    }
}
