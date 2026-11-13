// SPDX-License-Identifier: MPL-2.0
// Ports Visor: core/client/gui/screens/VRPauseMenuScreen.java (the layout and the buttons), input/actions/ActionMenu.java.
//! Visor's game menu (`VRPauseMenuScreen`): Y (or the Menu button) in play opens a small column of
//! buttons with three tabs, in place of the Vivecraft ring of eight actions. Main: inventory,
//! height calibration, keyboard, chat, the pause menu, VR settings. Commands (for a player who may
//! run them): game mode, time and weather. Tools: hitboxes, chunk borders, reload chunks, clear
//! chat. A button at the foot leaves the world. Copyright (C) Phoenix-Ra and Visor contributors;
//! MPL-2.0 (see LICENSE-MPL-2.0).
use super::{Game, Screen};
use horizon_client::{graphics_style, headset, hud::HudBatch, keybinds, text::Text, ui};
use std::time::{Duration, Instant};

/// `COLUMN_W`: the width of the whole column, in GUI pixels.
const COLUMN_W: i32 = 204;
const BTN_H: i32 = 20;
const GAP: i32 = 4;
/// `TAB_W`, `BTN_HALF`, `BTN_QUARTER`, `BTN_THIRD`: the widths Java's integer division gives.
const TAB_W: i32 = (COLUMN_W - 8) / 3;
const BTN_HALF: i32 = (COLUMN_W - 4) / 2;
const BTN_QUARTER: i32 = (COLUMN_W - 3 * GAP) / 4;
const BTN_THIRD: i32 = (COLUMN_W - 2 * GAP) / 3;
const LABEL_H: i32 = 11;
const SECTION_GAP: i32 = 6;
/// The accent colour of the tab strip and the section labels (`0xFF6AE3EA`), the divider's grey.
const ACCENT: u32 = 0xFF6A_E3EA;
const DIVIDER: u32 = 0xFF55_5555;
/// How long "Calibrate Height" counts down before it measures (`PlayerHeightTracker`'s countdown).
const MEASURE_COUNTDOWN: Duration = Duration::from_secs(3);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(super) enum Tab {
    #[default]
    Main,
    Commands,
    Tools,
}

impl Tab {
    const ALL: [Tab; 3] = [Tab::Main, Tab::Commands, Tab::Tools];
    fn label(self) -> &'static str {
        match self {
            Tab::Main => "visor.screen.pause_menu.tab.main",
            Tab::Commands => "visor.screen.pause_menu.tab.commands",
            Tab::Tools => "visor.screen.pause_menu.tab.tools",
        }
    }
}

/// What a button does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Do {
    Tab(Tab),
    Inventory,
    MeasureHeight,
    Keyboard,
    Chat,
    PauseMenu,
    VrSettings,
    Command(&'static str),
    Hitboxes,
    ChunkBorders,
    ReloadChunks,
    ClearChat,
    Leave,
}

/// One button: where it is (GUI pixels), its label and what it does.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Button {
    pub(super) at: [i32; 4],
    pub(super) label: &'static str,
    pub(super) action: Do,
    /// The tab button of the tab that shows is not pressable.
    pub(super) active: bool,
}

/// The menu on a `w` by `h` screen: its buttons, the section labels `(x, y, key)` and the lines.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Layout {
    pub(super) buttons: Vec<Button>,
    pub(super) headers: Vec<(i32, i32, &'static str)>,
    pub(super) title_y: i32,
    pub(super) divider_y: i32,
    /// The accent line under the tab strip: x, y, width.
    pub(super) accent: [i32; 3],
    pub(super) left: i32,
}

/// `totalColumnHeight`.
fn column_height(tab: Tab) -> i32 {
    let content = match tab {
        Tab::Main => 3 * (BTN_H + GAP),
        Tab::Tools => 2 * (BTN_H + GAP),
        Tab::Commands => 3 * LABEL_H + 4 * (BTN_H + GAP) + 2 * SECTION_GAP,
    };
    9 + 16 + BTN_H + 6 + content + 6 + BTN_H
}

/// `VRPauseMenuScreen.init` and `buildContent`: the layout for the `tab` on a `w` by `h` GUI screen.
/// `commands` is whether the Commands tab shows (the player may run commands).
pub(super) fn layout(tab: Tab, commands: bool, w: i32, h: i32) -> Layout {
    let tab = if tab == Tab::Commands && !commands {
        Tab::Main
    } else {
        tab
    };
    let cx = w / 2;
    let start_y = h / 2 - column_height(tab) / 2;
    let left = cx - COLUMN_W / 2;
    let right = left + BTN_HALF + GAP;
    let mut out = Layout {
        title_y: start_y,
        divider_y: start_y + 9 + 10,
        left,
        ..Layout::default()
    };
    let mut y = start_y + 9 + 16;
    for (i, t) in Tab::ALL.into_iter().enumerate() {
        if t == Tab::Commands && !commands {
            continue;
        }
        out.buttons.push(Button {
            at: [left + i as i32 * (TAB_W + 4), y, TAB_W, BTN_H],
            label: t.label(),
            action: Do::Tab(t),
            active: t != tab,
        });
    }
    out.accent = [left + tab as i32 * (TAB_W + 4), y - 2, TAB_W];
    y += BTN_H + GAP + 2;
    let half = |label, x, y, action| Button {
        at: [x, y, BTN_HALF, BTN_H],
        label,
        action,
        active: true,
    };
    match tab {
        Tab::Main => {
            out.buttons.extend([
                half(
                    "visor.screen.pause_menu.button.inventory",
                    left,
                    y,
                    Do::Inventory,
                ),
                half(
                    "visor.screen.pause_menu.button.measure_height",
                    right,
                    y,
                    Do::MeasureHeight,
                ),
                half(
                    "visor.screen.pause_menu.button.keyboard",
                    left,
                    y + 24,
                    Do::Keyboard,
                ),
                half(
                    "visor.screen.pause_menu.button.chat",
                    right,
                    y + 24,
                    Do::Chat,
                ),
                half(
                    "visor.screen.pause_menu.button.pause_menu",
                    left,
                    y + 48,
                    Do::PauseMenu,
                ),
                half(
                    "visor.screen.pause_menu.button.vr_settings",
                    right,
                    y + 48,
                    Do::VrSettings,
                ),
            ]);
            y += 3 * (BTN_H + GAP);
        }
        Tab::Commands => {
            out.headers
                .push((left, y, "visor.screen.pause_menu.section.game_mode"));
            y += LABEL_H;
            out.buttons.extend([
                half(
                    "visor.screen.pause_menu.button.survival",
                    left,
                    y,
                    Do::Command("gamemode survival"),
                ),
                half(
                    "visor.screen.pause_menu.button.creative",
                    right,
                    y,
                    Do::Command("gamemode creative"),
                ),
                half(
                    "visor.screen.pause_menu.button.spectator",
                    left,
                    y + 24,
                    Do::Command("gamemode spectator"),
                ),
                half(
                    "visor.screen.pause_menu.button.adventure",
                    right,
                    y + 24,
                    Do::Command("gamemode adventure"),
                ),
            ]);
            y += 2 * (BTN_H + GAP) + SECTION_GAP;
            out.headers
                .push((left, y, "visor.screen.pause_menu.section.time"));
            y += LABEL_H;
            let times = [
                ("visor.screen.pause_menu.button.dawn", "time set 0"),
                ("visor.screen.pause_menu.button.noon", "time set 6000"),
                ("visor.screen.pause_menu.button.dusk", "time set 12000"),
                ("visor.screen.pause_menu.button.night", "time set 18000"),
            ];
            for (i, (label, command)) in times.into_iter().enumerate() {
                out.buttons.push(Button {
                    at: [left + i as i32 * (BTN_QUARTER + GAP), y, BTN_QUARTER, BTN_H],
                    label,
                    action: Do::Command(command),
                    active: true,
                });
            }
            y += BTN_H + GAP + SECTION_GAP;
            out.headers
                .push((left, y, "visor.screen.pause_menu.section.weather"));
            y += LABEL_H;
            let weather = [
                ("visor.screen.pause_menu.button.clear", "weather clear"),
                ("visor.screen.pause_menu.button.rain", "weather rain"),
                ("visor.screen.pause_menu.button.thunder", "weather thunder"),
            ];
            for (i, (label, command)) in weather.into_iter().enumerate() {
                out.buttons.push(Button {
                    at: [left + i as i32 * (BTN_THIRD + GAP), y, BTN_THIRD, BTN_H],
                    label,
                    action: Do::Command(command),
                    active: true,
                });
            }
            y += BTN_H + GAP;
        }
        Tab::Tools => {
            out.buttons.extend([
                half(
                    "visor.screen.pause_menu.button.hitboxes",
                    left,
                    y,
                    Do::Hitboxes,
                ),
                half(
                    "visor.screen.pause_menu.button.chunk_borders",
                    right,
                    y,
                    Do::ChunkBorders,
                ),
                half(
                    "visor.screen.pause_menu.button.reload_chunks",
                    left,
                    y + 24,
                    Do::ReloadChunks,
                ),
                half(
                    "visor.screen.pause_menu.button.clear_chat",
                    right,
                    y + 24,
                    Do::ClearChat,
                ),
            ]);
            y += 2 * (BTN_H + GAP);
        }
    }
    y += 4;
    out.buttons.push(Button {
        at: [left, y, COLUMN_W, BTN_H],
        label: "menu.returnToMenu",
        action: Do::Leave,
        active: true,
    });
    out
}

/// The button under a GUI point.
pub(super) fn button_at(layout: &Layout, x: f32, y: f32) -> Option<usize> {
    layout.buttons.iter().position(|b| {
        b.active
            && x >= b.at[0] as f32
            && x < (b.at[0] + b.at[2]) as f32
            && y >= b.at[1] as f32
            && y < (b.at[1] + b.at[3]) as f32
    })
}

/// What the menu remembers between frames.
#[derive(Default)]
pub(super) struct GameMenu {
    tab: Tab,
    /// "Calibrate Height" is counting down to this moment.
    measuring: Option<Instant>,
}

impl Game<'_> {
    /// Whether the Commands tab shows: a single player's world with commands on, or a server
    /// (which refuses what the player may not run, as it does for any command).
    fn game_menu_commands(&self) -> bool {
        self.allow_commands() || self.remote.is_some()
    }

    fn game_menu_layout(&self, w: f32, h: f32) -> Layout {
        layout(
            self.pad.headset.game_menu.tab,
            self.game_menu_commands(),
            w as i32,
            h as i32,
        )
    }

    /// Y or Menu in play opens the menu (`ActionMenu.onPress`).
    pub(super) fn headset_game_menu_button(&mut self) {
        if headset::take_game_menu()
            && self.screen == Screen::Playing
            && self.player.is_some()
            && self.title.is_none()
        {
            self.pad.headset.game_menu = GameMenu::default();
            self.screen = Screen::GameMenu;
            self.keys.clear();
            self.left = false;
            self.right = false;
            self.mining = None;
            self.capture(false);
        }
    }

    pub(super) fn close_game_menu(&mut self) {
        self.pad.headset.game_menu.measuring = None;
        self.screen = Screen::Playing;
        self.capture(true);
    }

    /// Every frame while the menu shows: the calibration countdown ends in a measurement.
    fn game_menu_tick(&mut self) {
        let menu = &mut self.pad.headset.game_menu;
        if menu.measuring.is_some_and(|at| Instant::now() >= at) {
            menu.measuring = None;
            headset::request_calibrate_height();
        }
    }

    pub(super) fn draw_game_menu(&mut self, batch: &mut HudBatch) {
        horizon_client::widgets::begin_screen(|| {
            horizon_client::widgets::ScreenInfo::new("game_menu", "")
        });
        self.game_menu_tick();
        let layout = self.game_menu_layout(batch.width, batch.height);
        let hovered = button_at(&layout, self.cursor.0, self.cursor.1);
        let atlas = &self.gui.atlas;
        let title = Text::translate("visor.screen.pause_menu.title", &[]);
        let tw = HudBatch::text_width(atlas, &title.plain());
        let cx = (batch.width / 2.0).floor();
        batch.text_styled(
            atlas,
            &title,
            cx - (tw / 2.0).floor(),
            layout.title_y as f32,
            u32::MAX,
            true,
        );
        let line = |batch: &mut HudBatch, x: i32, y: i32, w: i32, color| {
            graphics_style::fill(batch, atlas, [x as f32, y as f32, w as f32, 1.0], color);
        };
        line(batch, layout.left, layout.divider_y, COLUMN_W, DIVIDER);
        line(
            batch,
            layout.accent[0],
            layout.accent[1],
            layout.accent[2],
            ACCENT,
        );
        for &(x, y, key) in &layout.headers {
            let text = Text::translate(key, &[]).with_color(ACCENT & 0x00FF_FFFF);
            let width = HudBatch::text_width(atlas, &text.plain()) as i32;
            line(batch, x, y + 4, 18, DIVIDER);
            line(batch, x + 22 + width, y + 4, COLUMN_W - 22 - width, DIVIDER);
            batch.text_styled(atlas, &text, (x + 20) as f32, y as f32, u32::MAX, false);
        }
        let measuring = self.pad.headset.game_menu.measuring;
        for (i, b) in layout.buttons.iter().enumerate() {
            let label = match (b.action, measuring) {
                (Do::MeasureHeight, Some(at)) => {
                    let left = at.saturating_duration_since(Instant::now()).as_secs() + 1;
                    Text::translate(
                        "visor.messages.height_measure_countdown",
                        &[Text::literal(left.to_string())],
                    )
                }
                (Do::Leave, _) if self.external_server() => Text::translate("menu.disconnect", &[]),
                _ => Text::translate(b.label, &[]),
            };
            ui::button(
                batch,
                atlas,
                b.at[0] as f32,
                b.at[1] as f32,
                b.at[2] as f32,
                b.at[3] as f32,
                &label,
                b.active,
                b.active && hovered == Some(i),
            );
        }
    }

    /// A click (window pixels) on the menu.
    pub(super) fn game_menu_click(&mut self, x: f64, y: f64) {
        let Some(gui) = self.gui_scale() else {
            return;
        };
        let layout = self.game_menu_layout(gui.width, gui.height);
        let Some(i) = button_at(&layout, x as f32 / gui.scale, y as f32 / gui.scale) else {
            return;
        };
        self.audio.play_button_click();
        self.run_game_menu(layout.buttons[i].action);
    }

    fn run_game_menu(&mut self, action: Do) {
        match action {
            Do::Tab(tab) => self.pad.headset.game_menu.tab = tab,
            Do::Inventory => {
                self.close_game_menu();
                self.press_binding("key.inventory");
            }
            Do::MeasureHeight => {
                let menu = &mut self.pad.headset.game_menu;
                if menu.measuring.is_none() {
                    menu.measuring = Some(Instant::now() + MEASURE_COUNTDOWN);
                }
            }
            Do::Keyboard => {
                self.close_game_menu();
                self.vr_keyboard_toggle();
            }
            Do::Chat => {
                self.close_game_menu();
                self.open_chat("");
            }
            Do::PauseMenu => {
                self.pause_focus = None;
                self.screen = Screen::Paused;
            }
            Do::VrSettings => {
                self.open_options_at(
                    false,
                    horizon_client::options_ui::Page::Vr(horizon_vivecraft::pages::PageId::Main),
                );
            }
            Do::Command(command) => self.send_command(command),
            Do::Hitboxes => self.menu_toggle_hitboxes(),
            Do::ChunkBorders => self.menu_toggle_chunk_borders(),
            Do::ReloadChunks => self.all_sections_changed(),
            Do::ClearChat => self.ui.chat.clear(),
            Do::Leave => {
                if let Err(e) = self.execute_pause_action(2) {
                    log::warn!("Game menu: leaving: {e}");
                }
            }
        }
    }

    /// Presses and releases the key or mouse button the game's binding `name` is bound to.
    pub(super) fn press_binding(&mut self, name: &str) {
        let bound = self.settings.keys.get(name).to_string();
        if let Some(key) = keybinds::key_code(&bound) {
            if let Err(e) = self.key_pressed(key) {
                log::warn!("Headset: game menu: {e}");
            }
        } else if let Some(button) = keybinds::mouse_button(&bound) {
            for pressed in [true, false] {
                if let Err(e) = self.play_mouse(button, pressed) {
                    log::warn!("Headset: game menu: {e}");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find(l: &Layout, action: Do) -> &Button {
        l.buttons.iter().find(|b| b.action == action).unwrap()
    }

    #[test]
    fn the_main_tab_is_two_columns_of_three_rows_with_the_leave_button_below() {
        // A 427 x 240 GUI. Total height 9 + 16 + 20 + 6 + 72 + 6 + 20 = 149, so the column starts at
        // 120 - 74 = 46; the tab strip is at 46 + 25 = 71 and the first row at 71 + 20 + 4 + 2 = 97.
        let l = layout(Tab::Main, true, 427, 240);
        assert_eq!(l.title_y, 46);
        assert_eq!(l.divider_y, 65);
        let left = 427 / 2 - 102;
        let inv = find(&l, Do::Inventory);
        assert_eq!(inv.at, [left, 97, 100, 20]);
        let height = find(&l, Do::MeasureHeight);
        assert_eq!(height.at, [left + 104, 97, 100, 20]);
        assert_eq!(find(&l, Do::Keyboard).at[1], 121);
        assert_eq!(find(&l, Do::VrSettings).at, [left + 104, 145, 100, 20]);
        // After the three rows (72) and 4 more: 97 + 76 = 173.
        assert_eq!(find(&l, Do::Leave).at, [left, 173, 204, 20]);
        // Three tab buttons 65 wide, the shown one inactive, the accent line under it.
        let tabs: Vec<_> = l
            .buttons
            .iter()
            .filter(|b| matches!(b.action, Do::Tab(_)))
            .collect();
        assert_eq!(tabs.len(), 3);
        assert_eq!(tabs[1].at, [left + 69, 71, 65, 20]);
        assert!(!tabs[0].active && tabs[1].active);
        assert_eq!(l.accent, [left, 69, 65]);
    }

    #[test]
    fn the_commands_tab_has_game_mode_time_and_weather_and_needs_the_right_to_run_them() {
        let l = layout(Tab::Commands, true, 427, 240);
        assert_eq!(l.headers.len(), 3);
        let commands: Vec<_> = l
            .buttons
            .iter()
            .filter_map(|b| match b.action {
                Do::Command(c) => Some(c),
                _ => None,
            })
            .collect();
        assert_eq!(
            commands,
            [
                "gamemode survival",
                "gamemode creative",
                "gamemode spectator",
                "gamemode adventure",
                "time set 0",
                "time set 6000",
                "time set 12000",
                "time set 18000",
                "weather clear",
                "weather rain",
                "weather thunder",
            ]
        );
        // Four time buttons 48 wide, three weather buttons 65 wide.
        assert_eq!(find(&l, Do::Command("time set 18000")).at[2], 48);
        assert_eq!(find(&l, Do::Command("weather thunder")).at[2], 65);
        // Without the right the tab is not there and the Main tab shows.
        let none = layout(Tab::Commands, false, 427, 240);
        assert!(
            none.buttons
                .iter()
                .all(|b| b.action != Do::Tab(Tab::Commands))
        );
        assert!(none.buttons.iter().any(|b| b.action == Do::Inventory));
    }

    #[test]
    fn a_click_finds_the_button_under_it_and_skips_the_shown_tab() {
        let l = layout(Tab::Main, true, 427, 240);
        let inv = find(&l, Do::Inventory);
        let i = button_at(&l, inv.at[0] as f32 + 5.0, inv.at[1] as f32 + 5.0).unwrap();
        assert_eq!(l.buttons[i].action, Do::Inventory);
        let shown = l
            .buttons
            .iter()
            .find(|b| b.action == Do::Tab(Tab::Main))
            .unwrap();
        assert_eq!(
            button_at(&l, shown.at[0] as f32 + 5.0, shown.at[1] as f32 + 5.0),
            None
        );
        assert_eq!(button_at(&l, 0.0, 0.0), None);
    }
}
