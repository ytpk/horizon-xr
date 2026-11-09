// SPDX-License-Identifier: MPL-2.0
// Ports Visor: core/client/gui/overlays/builtin/keyboard/{KeyboardLayout,KeyboardLayouts,KeyboardRow,KeyboardKey}.java,
// core/client/gui/screens/VRKeyboardScreen.java (the buttons and where they sit), VROverlayKeyboard.java (state, pose).
//! Visor's keyboard as plain data: the nine layouts (`KeyboardLayout`: English, Russian, Ukrainian,
//! German, French, Spanish, Italian, Portuguese, Hungarian), the buttons of `VRKeyboardScreen.init`
//! with their rectangles in the overlay's GUI pixels, which button a point is on, and the small
//! state machine of what a press does (shift stays down until pressed again, the layout button
//! cycles, a held key repeats once a second has passed, then every tick). Copyright (C)
//! Phoenix-Ra and Visor contributors; MPL-2.0 (see LICENSE-MPL-2.0).

/// Where `VROverlayKeyboard` first puts the overlay, from the head (right, up, forward is -Z),
/// metres (`posOffset`), and its scale (the overlay's width in metres for a 427 pixel GUI).
pub const OFFSET: [f32; 3] = [0.0, -0.5, -0.6];
pub const SCALE: f32 = 0.5;
/// `VRKeyboardScreen.tick`: a pressed key's press repeats after this many ticks, then each tick.
pub const REPEAT_DELAY_TICKS: u32 = 20;

use glam::{Mat3, Quat, Vec3};

/// `KeyboardLayout`, in the enum's order (`next()` walks it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    English,
    Russian,
    Ukrainian,
    German,
    French,
    Spanish,
    Italian,
    Portuguese,
    Hungarian,
}

/// A layout's four rows (numbers, top, home, bottom) as the normal and the shifted symbols.
type Rows = [(&'static str, &'static str); 4];

impl Layout {
    pub const ALL: [Layout; 9] = [
        Layout::English,
        Layout::Russian,
        Layout::Ukrainian,
        Layout::German,
        Layout::French,
        Layout::Spanish,
        Layout::Italian,
        Layout::Portuguese,
        Layout::Hungarian,
    ];

    /// The label of the language button (`KeyboardLayout.label`).
    pub fn label(self) -> &'static str {
        match self {
            Layout::English => "EN",
            Layout::Russian => "RU",
            Layout::Ukrainian => "UA",
            Layout::German => "DE",
            Layout::French => "FR",
            Layout::Spanish => "ES",
            Layout::Italian => "IT",
            Layout::Portuguese => "PT",
            Layout::Hungarian => "HU",
        }
    }

    /// `KeyboardLayouts.deserialize`: the layouts of a comma separated list of enum names
    /// (`ENGLISH,RUSSIAN`), unknown names skipped, repeats once.
    pub fn parse_list(raw: &str) -> Vec<Layout> {
        let mut out = Vec::new();
        for part in raw.split(',') {
            let name = part.trim().to_ascii_uppercase();
            if let Some(layout) = Layout::ALL
                .into_iter()
                .find(|l| format!("{l:?}").to_ascii_uppercase() == name)
                && !out.contains(&layout)
            {
                out.push(layout);
            }
        }
        out
    }

    /// `KeyboardLayouts.getEffectiveSelected`: the selected layouts, English when none, and the
    /// language's own after them when `auto` is on and it is not among them.
    pub fn effective(raw: &str, auto: bool, lang_code: &str) -> Vec<Layout> {
        let mut layouts = Layout::parse_list(raw);
        if let Some(own) = Layout::from_lang(lang_code).filter(|_| auto)
            && !layouts.contains(&own)
        {
            if layouts.is_empty() {
                layouts.push(Layout::English);
            }
            layouts.push(own);
        }
        layouts
    }

    /// `KeyboardLayout.next`.
    pub fn next(self) -> Layout {
        Layout::ALL[(self as usize + 1) % Layout::ALL.len()]
    }

    /// `KeyboardLayout.fromLangCode`: the layout of a language code (`ru_ru`).
    pub fn from_lang(code: &str) -> Option<Layout> {
        let lower = code.to_lowercase();
        [
            ("en", Layout::English),
            ("ru", Layout::Russian),
            ("uk", Layout::Ukrainian),
            ("de", Layout::German),
            ("fr", Layout::French),
            ("es", Layout::Spanish),
            ("it", Layout::Italian),
            ("pt", Layout::Portuguese),
            ("hu", Layout::Hungarian),
        ]
        .into_iter()
        .find(|(prefix, _)| lower.starts_with(prefix))
        .map(|(_, layout)| layout)
    }

    /// `KeyboardLayout`'s constants: `numbers`, `top`, `home` and `bottom`, each (normal, shift).
    fn rows(self) -> Rows {
        match self {
            Layout::English => [
                ("`1234567890-=", "~!@#$%^&*()_+"),
                ("qwertyuiop[]\\", "QWERTYUIOP{}|"),
                ("asdfghjkl;'", "ASDFGHJKL:\""),
                ("zxcvbnm,./", "ZXCVBNM<>?"),
            ],
            Layout::Russian => [
                ("ё1234567890-=", "Ё!\"№;%:?*()_+"),
                ("йцукенгшщзхъ\\", "ЙЦУКЕНГШЩЗХЪ/"),
                ("фывапролджэ", "ФЫВАПРОЛДЖЭ"),
                ("ячсмитьбю.", "ЯЧСМИТЬБЮ,"),
            ],
            Layout::Ukrainian => [
                ("'1234567890-=", "₴!\"№;%:?*()_+"),
                ("йцукенгшщзхїґ", "ЙЦУКЕНГШЩЗХЇҐ"),
                ("фівапролджє", "ФІВАПРОЛДЖЄ"),
                ("ячсмитьбю.", "ЯЧСМИТЬБЮ,"),
            ],
            Layout::German => [
                ("^1234567890ß´", "°!\"§$%&/()=?`"),
                ("qwertzuiopü+#", "QWERTZUIOPÜ*'"),
                ("asdfghjklöä", "ASDFGHJKLÖÄ"),
                ("yxcvbnm,.-", "YXCVBNM;:_"),
            ],
            Layout::French => [
                ("²&é\"'(-è_çà)=", "³1234567890°+"),
                ("azertyuiop^$*", "AZERTYUIOP¨£µ"),
                ("qsdfghjklmù", "QSDFGHJKLM%"),
                ("wxcvbn,;:!", "WXCVBN?./§"),
            ],
            Layout::Spanish => [
                ("º1234567890'¡", "ª!\"·$%&/()=?¿"),
                ("qwertyuiop`+ç", "QWERTYUIOP^*Ç"),
                ("asdfghjklñ´", "ASDFGHJKLÑ¨"),
                ("zxcvbnm,.-", "ZXCVBNM;:_"),
            ],
            Layout::Italian => [
                ("\\1234567890'ì", "|!\"£$%&/()=?^"),
                ("qwertyuiopè+ù", "QWERTYUIOPé*§"),
                ("asdfghjklòà", "ASDFGHJKLç°"),
                ("zxcvbnm,.-", "ZXCVBNM;:_"),
            ],
            Layout::Portuguese => [
                ("\\1234567890'«", "|!\"#$%&/()=?»"),
                ("qwertyuiop+´~", "QWERTYUIOP*`^"),
                ("asdfghjklçº", "ASDFGHJKLÇª"),
                ("zxcvbnm,.-", "ZXCVBNM;:_"),
            ],
            Layout::Hungarian => [
                ("0123456789öüó", "§'\"+!%/=()ÖÜÓ"),
                ("qwertzuiopőúű", "QWERTZUIOPŐÚŰ"),
                ("asdfghjkléá", "ASDFGHJKLÉÁ"),
                ("yxcvbnm,.-í", "YXCVBNM?:_Í"),
            ],
        }
    }

    /// The keys of each row as typed: `KeyboardLayouts.build`'s layer for `shifted`.
    pub fn keys(self, shifted: bool) -> [Vec<String>; 4] {
        self.rows().map(|(normal, shift)| {
            (if shifted { shift } else { normal })
                .chars()
                .map(|c| c.to_string())
                .collect()
        })
    }

    /// `KeyboardLayoutKeys.getMaxColumns`: the widest row (13 in every layout).
    pub fn max_columns(self) -> usize {
        self.keys(false).iter().map(Vec::len).max().unwrap_or(0)
    }
}

/// Where the overlay is first hung (`VROverlayKeyboard.orient`: the position anchor is the head
/// at `OFFSET`, the rotation faces the head from there): the position and the turn, given the
/// head's pose, in whatever space the head is in. The panel's front (+Z) looks back at the head,
/// its top is as upright as the head's own up lets it be.
pub fn hang(head: Vec3, head_rotation: Quat, scale: f32) -> (Vec3, Quat) {
    let at = head + head_rotation * (Vec3::from_array(OFFSET) * scale);
    let front = (head - at).try_normalize().unwrap_or(Vec3::Z);
    let right = Vec3::Y.cross(front).try_normalize().unwrap_or(Vec3::X);
    let up = front.cross(right);
    (at, Quat::from_mat3(&Mat3::from_cols(right, up, front)))
}

/// Where a ray meets a panel hung at `pos` turned by `rot`, `width` by `height` metres: shares
/// of its width and height from its top left, `None` off the quad, behind the ray or parallel to it,
/// and the distance along the ray.
pub fn ray_uv(
    origin: Vec3,
    dir: Vec3,
    pos: Vec3,
    rot: Quat,
    width: f32,
    height: f32,
) -> Option<([f32; 2], f32)> {
    let normal = rot * Vec3::Z;
    let along = dir.dot(normal);
    if along.abs() < 1e-5 {
        return None;
    }
    let t = (pos - origin).dot(normal) / along;
    if t <= 0.0 {
        return None;
    }
    let local = rot.inverse() * (origin + dir * t - pos);
    let (u, v) = (local.x / width + 0.5, 0.5 - local.y / height);
    ((0.0..=1.0).contains(&u) && (0.0..=1.0).contains(&v)).then_some(([u, v], t))
}

/// Where the keyboard's GUI pixels land in the panel's image, which is `batch` GUI pixels across and
/// tall: the keyboard (`bounds`) centred in it. Add this to a keyboard point for the image's.
pub fn placement(batch: [f32; 2], bounds: [i32; 4]) -> [f32; 2] {
    [
        ((batch[0] - bounds[2] as f32) / 2.0).floor() - bounds[0] as f32,
        ((batch[1] - bounds[3] as f32) / 2.0).floor() - bounds[1] as f32,
    ]
}

/// What a button does when pressed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// A key: types its text (`InputHelper.typeChars`).
    Type(String),
    Space,
    Backspace,
    Delete,
    Home,
    End,
    SelectAll,
    Enter,
    Tab,
    Escape,
    Up,
    Down,
    Left,
    Right,
    Cut,
    Copy,
    Paste,
    /// `SHIFT`: shift stays on until it is pressed again.
    Shift,
    /// The language button: the next layout.
    Language,
    /// The red cross.
    Close,
}

/// One button of `VRKeyboardScreen`: its rectangle `[x, y, w, h]` in GUI pixels (the screen's own,
/// before the overlay's), its label, what it does, whether it is live and whether holding it
/// repeats (`usePressTask`).
#[derive(Clone, Debug, PartialEq)]
pub struct Button {
    pub rect: [i32; 4],
    pub label: String,
    pub action: Action,
    pub active: bool,
    pub repeats: bool,
}

impl Button {
    pub fn contains(&self, x: f32, y: f32) -> bool {
        let [bx, by, w, h] = self.rect;
        x >= bx as f32 && y >= by as f32 && x < (bx + w) as f32 && y < (by + h) as f32
    }
}

const GRID_START: i32 = 32;
const KEY_GAP: i32 = 2;
const KEY_WIDTH: i32 = 25;
const KEY_HEIGHT: i32 = 20;
const SMALL_WIDTH: i32 = 30;
const SIDE_WIDTH: i32 = 35;
const SHIFT_WIDTH: i32 = 50;
const LANGUAGE_WIDTH: i32 = 38;

/// `VRKeyboardScreen.init`: every button of the keyboard for `layout`, shifted or not;
/// `several_layouts` is whether more than one layout is enabled (the language button is dead
/// otherwise).
pub fn buttons(layout: Layout, shifted: bool, several_layouts: bool) -> Vec<Button> {
    let rows = layout.keys(shifted);
    let max_columns = layout.max_columns() as i32;
    let grid_right = GRID_START + max_columns * (KEY_WIDTH + KEY_GAP);
    let mut out = Vec::new();
    let mut add = |rect: [i32; 4], label: &str, action: Action, repeats: bool| {
        out.push(Button {
            rect,
            label: label.to_string(),
            action,
            active: true,
            repeats,
        });
    };
    for (row, keys) in rows.iter().enumerate() {
        let start = GRID_START + ((max_columns - keys.len() as i32) * (KEY_WIDTH + KEY_GAP)) / 2;
        let y = GRID_START + row as i32 * (KEY_HEIGHT + KEY_GAP);
        for (column, key) in keys.iter().enumerate() {
            add(
                [
                    start + column as i32 * (KEY_WIDTH + KEY_GAP),
                    y,
                    KEY_WIDTH,
                    KEY_HEIGHT,
                ],
                key,
                Action::Type(key.clone()),
                true,
            );
        }
    }
    let bottom_y = GRID_START + rows.len() as i32 * (KEY_HEIGHT + KEY_GAP);
    let space_x = GRID_START + ((max_columns - 5) / 2) * (KEY_WIDTH + KEY_GAP);
    let top_y = GRID_START - (KEY_HEIGHT + KEY_GAP);
    add(
        [
            0,
            GRID_START + (rows.len() as i32 - 1) * (KEY_HEIGHT + KEY_GAP),
            if shifted {
                SHIFT_WIDTH + 2
            } else {
                SHIFT_WIDTH
            },
            KEY_HEIGHT,
        ],
        if shifted { "SHIFT" } else { "Shift" },
        Action::Shift,
        false,
    );
    add(
        [
            3 * (SIDE_WIDTH + KEY_GAP) + GRID_START,
            top_y,
            LANGUAGE_WIDTH,
            KEY_HEIGHT,
        ],
        layout.label(),
        Action::Language,
        false,
    );
    add(
        [space_x, bottom_y, 5 * (KEY_WIDTH + KEY_GAP), KEY_HEIGHT],
        " ",
        Action::Space,
        true,
    );
    add(
        [grid_right, GRID_START, SIDE_WIDTH, KEY_HEIGHT],
        "BKSP",
        Action::Backspace,
        true,
    );
    add(
        [
            grid_right,
            GRID_START + 2 * (KEY_HEIGHT + KEY_GAP),
            SIDE_WIDTH,
            KEY_HEIGHT,
        ],
        "ENTER",
        Action::Enter,
        false,
    );
    add(
        [
            0,
            GRID_START + KEY_HEIGHT + KEY_GAP,
            SMALL_WIDTH,
            KEY_HEIGHT,
        ],
        "TAB",
        Action::Tab,
        false,
    );
    add(
        [0, top_y, SMALL_WIDTH, KEY_HEIGHT],
        "\u{a7}cx",
        Action::Close,
        false,
    );
    add(
        [0, GRID_START, SMALL_WIDTH, KEY_HEIGHT],
        "ESC",
        Action::Escape,
        false,
    );
    let arrow = (max_columns - 1) * (KEY_WIDTH + KEY_GAP) + GRID_START;
    let below = bottom_y + KEY_HEIGHT + KEY_GAP;
    add(
        [arrow, bottom_y, KEY_WIDTH, KEY_HEIGHT],
        "\u{2191}",
        Action::Up,
        true,
    );
    add(
        [arrow, below, KEY_WIDTH, KEY_HEIGHT],
        "\u{2193}",
        Action::Down,
        true,
    );
    add(
        [
            (max_columns - 2) * (KEY_WIDTH + KEY_GAP) + GRID_START,
            below,
            KEY_WIDTH,
            KEY_HEIGHT,
        ],
        "\u{2190}",
        Action::Left,
        true,
    );
    add(
        [
            max_columns * (KEY_WIDTH + KEY_GAP) + GRID_START,
            below,
            KEY_WIDTH,
            KEY_HEIGHT,
        ],
        "\u{2192}",
        Action::Right,
        true,
    );
    for (i, (label, action)) in [
        ("CUT", Action::Cut),
        ("COPY", Action::Copy),
        ("PASTE", Action::Paste),
    ]
    .into_iter()
    .enumerate()
    {
        add(
            [
                i as i32 * (SIDE_WIDTH + KEY_GAP) + GRID_START,
                top_y,
                SIDE_WIDTH,
                KEY_HEIGHT,
            ],
            label,
            action,
            false,
        );
    }
    // Editing controls occupy the unused lower-left strip beside the arrows.
    for (i, (label, action, repeats)) in [
        ("DEL", Action::Delete, true),
        ("HOME", Action::Home, false),
        ("END", Action::End, false),
        ("ALL", Action::SelectAll, false),
    ]
    .into_iter()
    .enumerate()
    {
        add(
            [
                GRID_START + i as i32 * (SIDE_WIDTH + KEY_GAP),
                below,
                SIDE_WIDTH,
                KEY_HEIGHT,
            ],
            label,
            action,
            repeats,
        );
    }
    // The language button is dead with one layout (`languageButton.active`).
    for button in out.iter_mut().filter(|b| b.action == Action::Language) {
        button.active = several_layouts;
    }
    out
}

/// The bounding box of the buttons (`VRKeyboardScreen`'s cursor bounds): `[x, y, w, h]`.
pub fn bounds(buttons: &[Button]) -> [i32; 4] {
    let min_x = buttons.iter().map(|b| b.rect[0]).min().unwrap_or(0);
    let min_y = buttons.iter().map(|b| b.rect[1]).min().unwrap_or(0);
    let max_x = buttons
        .iter()
        .map(|b| b.rect[0] + b.rect[2])
        .max()
        .unwrap_or(0);
    let max_y = buttons
        .iter()
        .map(|b| b.rect[1] + b.rect[3])
        .max()
        .unwrap_or(0);
    [min_x, min_y, max_x - min_x, max_y - min_y]
}

/// The button under a point of the keyboard's GUI pixels, if any (`AbstractWidget.isMouseOver`).
pub fn button_at(buttons: &[Button], x: f32, y: f32) -> Option<usize> {
    buttons.iter().position(|b| b.contains(x, y))
}

/// `VROverlayKeyboard` and `VRKeyboardScreen`: the layout, the shift, the key held down and the
/// ticks it has been held.
#[derive(Clone, Debug)]
pub struct Keyboard {
    pub layout: Layout,
    pub shift: bool,
    /// The layouts the player has turned on (`getEffectiveSelected`; English when none is).
    pub enabled: Vec<Layout>,
    /// The button held and the ticks it has been held, for the repeat.
    held: Option<(usize, u32)>,
}

impl Default for Keyboard {
    fn default() -> Self {
        Self::new(vec![Layout::English])
    }
}

impl Keyboard {
    /// A keyboard with `enabled` layouts (the first is the one it opens on).
    pub fn new(mut enabled: Vec<Layout>) -> Self {
        if enabled.is_empty() {
            enabled.push(Layout::English);
        }
        Self {
            layout: enabled[0],
            shift: false,
            enabled,
            held: None,
        }
    }

    /// `setVisible(true)`: shift off, the first layout.
    pub fn reopen(&mut self) {
        self.shift = false;
        self.layout = self.enabled[0];
        self.held = None;
    }

    pub fn buttons(&self) -> Vec<Button> {
        buttons(self.layout, self.shift, self.enabled.len() > 1)
    }

    /// `cycleLayout`.
    fn cycle_layout(&mut self) {
        let at = self.enabled.iter().position(|l| *l == self.layout);
        self.layout = match at {
            Some(i) => self.enabled[(i + 1) % self.enabled.len()],
            None => self.enabled[0],
        };
    }

    /// A press of button `i`: what the game types or does for it, if anything. Shift and the
    /// language button change the keyboard itself and return `None`; a button that repeats is held
    /// until `release` (the repeat starts after a second).
    pub fn press(&mut self, buttons: &[Button], i: usize) -> Option<Action> {
        let button = buttons.get(i)?;
        if !button.active {
            return None;
        }
        self.held = button.repeats.then_some((i, 0));
        match &button.action {
            Action::Shift => {
                self.shift = !self.shift;
                None
            }
            Action::Language => {
                self.cycle_layout();
                None
            }
            // `pressNavigationKey`: the arrows go with shift when it is on.
            other => Some(other.clone()),
        }
    }

    /// The press ended (the trigger let go, or the pointer left the key).
    pub fn release(&mut self) {
        self.held = None;
    }

    /// One tick of `VRKeyboardScreen.tick`: after `REPEAT_DELAY_TICKS` ticks held, the press runs
    /// again every tick.
    pub fn tick(&mut self, buttons: &[Button]) -> Option<Action> {
        let (i, ticks) = self.held.as_mut()?;
        if *ticks < REPEAT_DELAY_TICKS {
            *ticks += 1;
            return None;
        }
        buttons.get(*i).map(|b| b.action.clone())
    }

    pub fn held(&self) -> Option<usize> {
        self.held.map(|(i, _)| i)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find<'a>(buttons: &'a [Button], label: &str) -> &'a Button {
        buttons.iter().find(|b| b.label == label).expect(label)
    }

    #[test]
    fn editing_keys_are_reachable_and_do_not_overlap_in_any_layout() {
        for layout in Layout::ALL {
            let keys = buttons(layout, false, true);
            for action in [Action::Delete, Action::Home, Action::End, Action::SelectAll] {
                let key = keys.iter().find(|b| b.action == action).unwrap();
                let [x, y, w, h] = key.rect;
                let selected = button_at(&keys, (x + w / 2) as f32, (y + h / 2) as f32).unwrap();
                assert_eq!(keys[selected].action, action);
            }
            for (i, a) in keys.iter().enumerate() {
                for b in &keys[i + 1..] {
                    assert!(
                        !(a.rect[0] < b.rect[0] + b.rect[2]
                            && b.rect[0] < a.rect[0] + a.rect[2]
                            && a.rect[1] < b.rect[1] + b.rect[3]
                            && b.rect[1] < a.rect[1] + a.rect[3]),
                        "overlap: {:?} {:?}",
                        a.label,
                        b.label
                    );
                }
            }
        }
    }

    #[test]
    fn the_qwerty_and_russian_rows_are_visors_and_shift_changes_every_key() {
        let en = Layout::English.keys(false);
        assert_eq!(en[0].concat(), "`1234567890-=");
        assert_eq!(en[1].concat(), "qwertyuiop[]\\");
        assert_eq!(en[2].concat(), "asdfghjkl;'");
        assert_eq!(en[3].concat(), "zxcvbnm,./");
        let shifted = Layout::English.keys(true);
        assert_eq!(shifted[1].concat(), "QWERTYUIOP{}|");
        assert_eq!(shifted[3].concat(), "ZXCVBNM<>?");
        let ru = Layout::Russian.keys(false);
        assert_eq!(ru[1].concat(), "йцукенгшщзхъ\\");
        assert_eq!(ru[2].concat(), "фывапролджэ");
        assert_eq!(ru[3].concat(), "ячсмитьбю.");
        assert_eq!(Layout::Russian.keys(true)[0].concat(), "Ё!\"№;%:?*()_+");
        // Both layers of every row type the same number of keys, and the keyboard is 13 wide.
        for layout in Layout::ALL {
            let (a, b) = (layout.keys(false), layout.keys(true));
            for row in 0..4 {
                assert_eq!(a[row].len(), b[row].len(), "{layout:?} {row}");
            }
            assert_eq!(layout.max_columns(), 13, "{layout:?}");
        }
    }

    #[test]
    fn the_layout_button_walks_the_enabled_layouts_and_follows_the_language() {
        assert_eq!(Layout::English.next(), Layout::Russian);
        assert_eq!(Layout::Hungarian.next(), Layout::English);
        assert_eq!(Layout::from_lang("ru_ru"), Some(Layout::Russian));
        assert_eq!(Layout::from_lang("uk_ua"), Some(Layout::Ukrainian));
        assert_eq!(Layout::from_lang("ja_jp"), None);
        let mut kb = Keyboard::new(vec![Layout::English, Layout::Russian]);
        let buttons = kb.buttons();
        let lang = buttons
            .iter()
            .position(|b| b.action == Action::Language)
            .unwrap();
        assert!(buttons[lang].active);
        assert_eq!(kb.press(&buttons, lang), None);
        assert_eq!(kb.layout, Layout::Russian);
        assert_eq!(kb.press(&kb.buttons(), lang), None);
        assert_eq!(kb.layout, Layout::English);
        // With one layout the button is dead.
        assert!(!Keyboard::default().buttons()[lang].active);
    }

    #[test]
    fn buttons_sit_where_the_screen_puts_them() {
        let b = buttons(Layout::English, false, false);
        // The `q` key: the top row has 13 keys, so it starts at the grid (32), one row down (54).
        assert_eq!(find(&b, "q").rect, [32, 54, 25, 20]);
        // The home row has 11 keys: indented by one key.
        assert_eq!(find(&b, "a").rect, [32 + 27, 76, 25, 20]);
        assert_eq!(find(&b, "BKSP").rect, [383, 32, 35, 20]);
        assert_eq!(find(&b, "ENTER").rect, [383, 76, 35, 20]);
        assert_eq!(find(&b, " ").rect, [140, 120, 135, 20]);
        assert_eq!(find(&b, "Shift").rect, [0, 98, 50, 20]);
        assert_eq!(find(&b, "\u{2192}").rect, [383, 142, 25, 20]);
        // Shifted, the shift button is two pixels wider and says so.
        assert_eq!(
            find(&buttons(Layout::English, true, false), "SHIFT").rect,
            [0, 98, 52, 20]
        );
        // The cursor bounds: from the close button's corner to the right arrow's.
        assert_eq!(bounds(&b), [0, 10, 418, 152]);
    }

    #[test]
    fn a_point_is_on_the_key_it_is_in_and_the_gaps_belong_to_none() {
        let b = buttons(Layout::English, false, false);
        let q = find(&b, "q").rect;
        let hit = |x: i32, y: i32| {
            button_at(&b, x as f32 + 0.5, y as f32 + 0.5).map(|i| b[i].label.clone())
        };
        assert_eq!(hit(q[0], q[1]).as_deref(), Some("q"));
        assert_eq!(hit(q[0] + 24, q[1] + 19).as_deref(), Some("q"));
        // The two pixel gap to the right is no key; the next one is `w`.
        assert_eq!(hit(q[0] + 25, q[1]), None);
        assert_eq!(hit(q[0] + 27, q[1]).as_deref(), Some("w"));
        assert_eq!(hit(500, 500), None);
    }

    #[test]
    fn shift_stays_until_pressed_again_and_the_keys_follow_it() {
        let mut kb = Keyboard::default();
        let buttons = kb.buttons();
        let shift = buttons
            .iter()
            .position(|b| b.action == Action::Shift)
            .unwrap();
        assert_eq!(kb.press(&buttons, shift), None);
        assert!(kb.shift);
        let upper = kb.buttons();
        let q = upper
            .iter()
            .find(|b| b.rect[0] == 32 && b.rect[1] == 54)
            .unwrap();
        assert_eq!(q.action, Action::Type("Q".into()));
        // Typing does not let it go (Visor's shift is a toggle, not a one-shot).
        let qi = upper.iter().position(|b| b == q).unwrap();
        assert_eq!(kb.press(&upper, qi), Some(Action::Type("Q".into())));
        assert!(kb.shift);
        let shift = upper
            .iter()
            .position(|b| b.action == Action::Shift)
            .unwrap();
        kb.press(&upper, shift);
        assert!(!kb.shift);
    }

    #[test]
    fn a_ray_at_a_key_of_the_hung_panel_presses_that_key() {
        // Head at 1.7 m looking ahead (-Z); the panel hangs 0.5 m lower and 0.6 m ahead of it.
        let head = Vec3::new(0.0, 1.7, 0.0);
        let (pos, rot) = hang(head, Quat::IDENTITY, 1.0);
        assert!((pos - Vec3::new(0.0, 1.2, -0.6)).length() < 1e-5);
        // It faces the head: its front looks up and back at it.
        let front = rot * Vec3::Z;
        assert!((front - (head - pos).normalize()).length() < 1e-5);
        assert!(
            (rot * Vec3::X - Vec3::X).length() < 1e-4,
            "upright, not rolled"
        );
        // A 640 by 360 GUI image: 0.5 m is 427 pixels, so it is 0.75 m wide.
        let batch = [640.0, 360.0];
        let width = SCALE * batch[0] / 427.0;
        let height = width * 9.0 / 16.0;
        let buttons = buttons(Layout::English, false, false);
        let at = placement(batch, bounds(&buttons));
        let q = buttons.iter().find(|b| b.label == "q").unwrap();
        // The point of the panel where `q`'s middle is.
        let image = [
            q.rect[0] as f32 + q.rect[2] as f32 / 2.0 + at[0],
            q.rect[1] as f32 + q.rect[3] as f32 / 2.0 + at[1],
        ];
        let local = Vec3::new(
            (image[0] / batch[0] - 0.5) * width,
            (0.5 - image[1] / batch[1]) * height,
            0.0,
        );
        let target = pos + rot * local;
        // A hand 0.3 m below the head, aiming at that point.
        let hand = Vec3::new(0.2, 1.4, -0.1);
        let ([u, v], t) =
            ray_uv(hand, (target - hand).normalize(), pos, rot, width, height).unwrap();
        assert!((t - (target - hand).length()).abs() < 1e-4);
        let (x, y) = (u * batch[0] - at[0], v * batch[1] - at[1]);
        assert_eq!(
            button_at(&buttons, x, y).map(|i| buttons[i].label.as_str()),
            Some("q")
        );
        // Aiming away from it, or from behind it, finds nothing.
        assert!(ray_uv(hand, Vec3::Y, pos, rot, width, height).is_none());
        assert!(ray_uv(hand, (hand - target).normalize(), pos, rot, width, height).is_none());
    }

    #[test]
    fn layouts_are_read_from_the_setting_and_the_language_is_added() {
        assert_eq!(
            Layout::parse_list("ENGLISH, russian,bogus,ENGLISH"),
            [Layout::English, Layout::Russian]
        );
        assert_eq!(
            Layout::effective("ENGLISH", true, "ru_ru"),
            [Layout::English, Layout::Russian]
        );
        assert_eq!(
            Layout::effective("ENGLISH", false, "ru_ru"),
            [Layout::English]
        );
        assert_eq!(
            Layout::effective("", true, "de_de"),
            [Layout::English, Layout::German]
        );
        assert_eq!(
            Layout::effective("RUSSIAN", true, "ru_ru"),
            [Layout::Russian]
        );
        // No layout at all: the keyboard still has English.
        assert_eq!(
            Keyboard::new(Layout::effective("", false, "ja_jp")).layout,
            Layout::English
        );
    }

    #[test]
    fn a_held_key_repeats_after_a_second_then_every_tick_and_stops_on_release() {
        let mut kb = Keyboard::default();
        let buttons = kb.buttons();
        let bksp = buttons
            .iter()
            .position(|b| b.action == Action::Backspace)
            .unwrap();
        assert_eq!(kb.press(&buttons, bksp), Some(Action::Backspace));
        for _ in 0..REPEAT_DELAY_TICKS {
            assert_eq!(kb.tick(&buttons), None);
        }
        assert_eq!(kb.tick(&buttons), Some(Action::Backspace));
        assert_eq!(kb.tick(&buttons), Some(Action::Backspace));
        kb.release();
        assert_eq!(kb.tick(&buttons), None);
        // Enter does not repeat (`usePressTask(false)`).
        let enter = buttons
            .iter()
            .position(|b| b.action == Action::Enter)
            .unwrap();
        assert_eq!(kb.press(&buttons, enter), Some(Action::Enter));
        for _ in 0..40 {
            assert_eq!(kb.tick(&buttons), None);
        }
    }
}
