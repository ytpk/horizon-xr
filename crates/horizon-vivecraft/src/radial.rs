// SPDX-License-Identifier: LGPL-3.0-or-later
//! Ports the data of `GuiRadialConfiguration` and `GuiQuickCommandEditor`: the radial menu's
//! two item sets (`vrRadialItems`, `vrRadialItemsAlt`, 14 slots each, a key binding's name or
//! empty) and the 12 quick commands (`vrQuickCommands`), kept as lines of the settings.
//! Copyright (c) Vivecraft contributors; LGPL-3.0-or-later (see LICENSE-LGPL-3.0).

use crate::settings::VrSettings;

/// The slots a set has (`getRadialItemsDefault`).
pub const SLOTS: usize = 14;
/// The quick commands (`getQuickCommandsDefaults`).
pub const QUICK_COMMANDS: usize = 12;

/// The key bindings the VR mod adds that the radial menu can run, in Vivecraft's order.
pub const VIVECRAFT_ACTIONS: &[&str] = &[
    "vivecraft.key.rotateLeft",
    "vivecraft.key.rotateRight",
    "vivecraft.key.quickTorch",
    "vivecraft.key.toggleHandheldCam",
    "vivecraft.key.togglePlayerList",
    "vivecraft.key.quickcommand1",
    "vivecraft.key.quickcommand2",
    "vivecraft.key.quickcommand3",
    "vivecraft.key.quickcommand4",
    "vivecraft.key.quickcommand5",
    "vivecraft.key.quickcommand6",
    "vivecraft.key.quickcommand7",
    "vivecraft.key.quickcommand8",
    "vivecraft.key.quickcommand9",
    "vivecraft.key.quickcommand10",
    "vivecraft.key.quickcommand11",
    "vivecraft.key.quickcommand12",
];

fn key(alt: bool) -> &'static str {
    if alt {
        "vrRadialItemsAlt"
    } else {
        "vrRadialItems"
    }
}

/// The set's 14 items (empty for a free slot).
pub fn items(s: &VrSettings, alt: bool) -> [String; SLOTS] {
    let mut out: [String; SLOTS] = Default::default();
    for (o, v) in out.iter_mut().zip(s.string(key(alt)).split('\n')) {
        *o = v.to_string();
    }
    out
}

pub fn set_item(s: &mut VrSettings, alt: bool, slot: usize, name: &str) {
    let mut v = items(s, alt);
    if let Some(item) = v.get_mut(slot) {
        *item = name.replace('\n', "");
        s.set(key(alt), &v.join("\n"));
    }
}

/// The set back to Vivecraft's default (the main set holds the eight stock actions).
pub fn reset(s: &mut VrSettings) {
    for alt in [false, true] {
        let default = crate::settings::def(key(alt)).map_or("", |d| d.default);
        s.set(key(alt), default);
    }
}

/// The item after `current` in `actions` (a free slot first, then each action), wrapping.
pub fn next<'a>(current: &str, actions: &'a [&'a str]) -> &'a str {
    if current.is_empty() {
        return actions.first().copied().unwrap_or("");
    }
    match actions.iter().position(|a| a.eq_ignore_ascii_case(current)) {
        Some(i) => actions.get(i + 1).copied().unwrap_or(""),
        None => "",
    }
}

/// The 12 quick commands.
pub fn quick_commands(s: &VrSettings) -> [String; QUICK_COMMANDS] {
    let mut out: [String; QUICK_COMMANDS] = Default::default();
    for (o, v) in out.iter_mut().zip(s.string("vrQuickCommands").split('\n')) {
        *o = v.to_string();
    }
    out
}

pub fn set_quick_command(s: &mut VrSettings, i: usize, command: &str) {
    let mut v = quick_commands(s);
    if let Some(c) = v.get_mut(i) {
        *c = command.replace('\n', " ");
        s.set("vrQuickCommands", &v.join("\n"));
    }
}

pub fn reset_quick_commands(s: &mut VrSettings) {
    let default = crate::settings::def("vrQuickCommands").map_or("", |d| d.default);
    s.set("vrQuickCommands", default);
}

/// The number (1 to 12) of a `vivecraft.key.quickcommandN` binding.
pub fn quick_command_number(binding: &str) -> Option<usize> {
    let n: usize = binding
        .strip_prefix("vivecraft.key.quickcommand")?
        .parse()
        .ok()?;
    (1..=QUICK_COMMANDS).contains(&n).then_some(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_are_vivecrafts_eight_actions_and_twelve_commands() {
        let s = VrSettings::default();
        let main = items(&s, false);
        assert_eq!(main[0], "key.drop");
        assert_eq!(main[7], "vivecraft.key.quickTorch");
        assert!(main[8..].iter().all(String::is_empty));
        assert!(items(&s, true).iter().all(String::is_empty));
        let q = quick_commands(&s);
        assert_eq!(
            (q[0].as_str(), q[11].as_str()),
            ("/gamemode survival", "praise the sun!")
        );
    }

    #[test]
    fn slots_and_commands_edit_and_round_trip_through_the_config() {
        let mut s = VrSettings::default();
        set_item(&mut s, true, 3, "key.inventory");
        set_quick_command(&mut s, 2, "/time set day");
        let mut loaded = VrSettings::default();
        loaded.load_json(&s.to_json());
        assert_eq!(items(&loaded, true)[3], "key.inventory");
        assert_eq!(quick_commands(&loaded)[2], "/time set day");
        reset(&mut loaded);
        assert!(items(&loaded, true).iter().all(String::is_empty));
        assert_eq!(items(&loaded, false)[1], "key.chat");
    }

    #[test]
    fn cycling_walks_the_actions_then_frees_the_slot() {
        let actions = ["a", "b"];
        assert_eq!(next("", &actions), "a");
        assert_eq!(next("a", &actions), "b");
        assert_eq!(next("b", &actions), "");
        assert_eq!(next("zzz", &actions), "");
        assert_eq!(
            quick_command_number("vivecraft.key.quickcommand12"),
            Some(12)
        );
        assert_eq!(quick_command_number("vivecraft.key.quickcommand13"), None);
    }
}
