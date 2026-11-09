// SPDX-License-Identifier: LGPL-3.0-or-later
//! Ports the options-screen half of `org.vivecraft.client_vr.settings.VRSettings.VrOptions`:
//! which config key an option edits, how its value is shown, how a slider maps to it and how a
//! button cycles it. The screens themselves are Horizon's own (`horizon-client-render`).
//! Copyright (c) Vivecraft contributors; LGPL-3.0-or-later (see LICENSE-LGPL-3.0).

use crate::settings::{Kind, VrSettings, def};

#[path = "options_table.rs"]
mod table;

pub use table::OPTS;

/// One `VrOptions` constant bound to its `@SettingField`.
#[derive(Clone, Copy, Debug)]
pub struct OptInfo {
    /// The config key of the setting it edits.
    pub key: &'static str,
    /// The `VrOptions` constant's name (`HUD_SCALE`): the lang key is `vivecraft.options.<name>`.
    pub name: &'static str,
    /// The float option's decimal places; -1 shows a percentage; -2 is not a float option.
    pub decimals: i8,
    /// The boolean option's lang keys for true and false.
    pub on: &'static str,
    pub off: &'static str,
    /// The enum option's lowercased Java class name (`hudlock`): the lang keys of its values.
    pub class: &'static str,
    /// The option's label key.
    pub lang: &'static str,
}

/// The option for a config key.
pub fn info(key: &str) -> Option<&'static OptInfo> {
    OPTS.iter().find(|o| o.key == key)
}

/// `OptionEnum.getLangKey`: the lang key of an enum constant.
pub fn enum_lang_key(class: &str, value: &str) -> String {
    match value.to_ascii_lowercase().as_str() {
        "yes" => "vivecraft.options.yes".into(),
        "no" => "vivecraft.options.no".into(),
        "on" => "options.on".into(),
        "off" => "options.off".into(),
        v => format!("vivecraft.options.{class}.{}", v.replace('_', "")),
    }
}

/// An option's value as the button shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Shown {
    Literal(String),
    /// A lang key.
    Lang(String),
    /// A lang key with one argument (`%s`).
    LangArg(String, String),
}

/// `WORLD_SCALE`'s slider is an index into these values.
const WORLD_SCALES: [f32; 30] = [
    0.1, 0.25, 0.5, 0.6, 0.7, 0.8, 0.9, 1.0, 1.1, 1.2, 1.3, 1.4, 1.5, 1.6, 1.7, 1.8, 1.9, 2.0, 3.0,
    4.0, 6.0, 8.0, 10.0, 12.0, 16.0, 20.0, 30.0, 50.0, 75.0, 100.0,
];
/// `WORLD_ROTATION_INCREMENT`'s slider is an index (from -1) into these; 0 is smooth turning.
const ROTATION_INCREMENTS: [f32; 6] = [0.0, 10.0, 36.0, 45.0, 90.0, 180.0];

/// `VrOptions.getOptionFloatValue`: the slider position for a value, as an index for the
/// index sliders.
fn to_slider_value(name: &str, value: f32) -> f32 {
    match name {
        "WORLD_SCALE" => WORLD_SCALES
            .iter()
            .position(|v| (v - value).abs() < 1e-4)
            .map_or(7.0, |i| i as f32),
        "WORLD_ROTATION_INCREMENT" => ROTATION_INCREMENTS
            .iter()
            .position(|v| (v - value).abs() < 1e-4)
            .map_or(2.0, |i| i as f32 - 1.0),
        _ => value,
    }
}

/// `VrOptions.setOptionFloatValue`: the value for a slider position.
fn from_slider_value(name: &str, v: f32) -> f32 {
    match name {
        "WORLD_SCALE" => WORLD_SCALES[(v.round() as usize).min(WORLD_SCALES.len() - 1)],
        "WORLD_ROTATION_INCREMENT" => {
            ROTATION_INCREMENTS[((v.round() as i32 + 1).clamp(0, 5)) as usize]
        }
        _ => v,
    }
}

/// How many steps a slider has, and where it stands (0..1); `None` for a button.
pub fn slider(s: &VrSettings, o: &OptInfo) -> Option<(f32, u32)> {
    let d = def(o.key)?;
    let (min, max, step) = d.range?;
    if matches!(d.kind, Kind::Bool | Kind::Enum(_) | Kind::Str | Kind::Vec3) {
        return None;
    }
    let value = match d.kind {
        Kind::Int => s.int(o.key) as f32,
        _ => s.float(o.key),
    };
    let v = to_slider_value(o.name, value);
    let steps = (((max - min) / step).round() as u32).max(1);
    Some((((v - min) / (max - min)).clamp(0.0, 1.0), steps))
}

/// `setOptionFloatValue` from the slider's 0..1 position. `server_range` is a server
/// override's `(min, max)` (`VrSettings` overrides), if one limits the option.
pub fn set_slider(s: &mut VrSettings, o: &OptInfo, at: f32, server_range: Option<(f32, f32)>) {
    let Some(d) = def(o.key) else {
        return;
    };
    let Some((min, max, step)) = d.range else {
        return;
    };
    let steps = ((max - min) / step).round().max(1.0);
    let raw = min + (at.clamp(0.0, 1.0) * steps).round() * (max - min) / steps;
    let mut v = from_slider_value(o.name, raw);
    if let Some((lo, hi)) = server_range {
        v = v.clamp(lo, hi);
    }
    match d.kind {
        Kind::Int => {
            s.set_int(o.key, v.round() as i32);
        }
        _ => {
            s.set_float(o.key, v);
        }
    }
    // `WORLD_ROTATION_INCREMENT.onOptionChange`
    if o.name == "WORLD_ROTATION_INCREMENT" {
        s.set_float("worldRotation", 0.0);
    }
}

/// `VrOptions.setOptionValue` for the non-slider options: toggles, or the next enum constant
/// (the previous one with `back`).
pub fn cycle(s: &mut VrSettings, o: &OptInfo, back: bool) {
    if let Some(layout) = o.name.strip_prefix("VISOR_KEYBOARD_LAYOUT_") {
        let mut selected: Vec<String> = s
            .string(o.key)
            .split(',')
            .map(|v| v.trim().to_ascii_uppercase())
            .filter(|v| !v.is_empty())
            .collect();
        if selected.iter().any(|v| v == layout) {
            // Keep at least one usable language; auto layout is optional.
            if selected.len() > 1 {
                selected.retain(|v| v != layout);
            }
        } else {
            selected.push(layout.to_owned());
        }
        s.set(o.key, &selected.join(","));
        return;
    }
    let Some(d) = def(o.key) else {
        return;
    };
    match d.kind {
        Kind::Bool => {
            s.set_bool(o.key, !s.bool(o.key));
        }
        Kind::Enum(values) => {
            let now = s.enum_name(o.key).to_string();
            let i = values.iter().position(|v| *v == now).unwrap_or(0);
            let n = values.len();
            let mut next = if back { (i + n - 1) % n } else { (i + 1) % n };
            // FREEMOVE_MODE skips AUTO, which is for flying only.
            if o.name == "FREEMOVE_MODE" && values[next] == "AUTO" {
                next = if back { (next + n - 1) % n } else { 0 };
            }
            s.set(o.key, values[next]);
        }
        _ => {}
    }
}

/// Sharing a physical control is allowed, but must be visible before gameplay resumes.
pub fn binding_conflict(s: &VrSettings, key: &str) -> bool {
    key.starts_with("questBind")
        && s.enum_name(key) != "NONE"
        && crate::settings::DEFS.iter().any(|d| {
            d.key != key && d.key.starts_with("questBind") && s.enum_name(d.key) == s.enum_name(key)
        })
}

/// `getButtonDisplayString`'s value half.
pub fn shown(s: &VrSettings, o: &OptInfo) -> Shown {
    if let Some(layout) = o.name.strip_prefix("VISOR_KEYBOARD_LAYOUT_") {
        let selected = s
            .string(o.key)
            .split(',')
            .any(|v| v.trim().eq_ignore_ascii_case(layout));
        return Shown::Lang(if selected { o.on } else { o.off }.into());
    }
    let Some(d) = def(o.key) else {
        return Shown::Literal(String::new());
    };
    let degrees = |v: f32| Shown::Literal(format!("{v:.0}\u{b0}"));
    match o.name {
        "HUD_OPACITY" if s.float(o.key) > 0.99 => {
            return Shown::Lang("vivecraft.options.opaque".into());
        }
        "HUD_DISTANCE" => return Shown::Literal(format!("{:.2}m", s.float(o.key))),
        "WORLD_SCALE" => return Shown::Literal(format!("{:.2}x", s.float(o.key))),
        "SPRINT_MOVEMENT_MULTIPLIER" if s.float(o.key) <= 0.145 => {
            return Shown::Lang("vivecraft.options.sprintmovementmultiplier.same".into());
        }
        "WORLD_ROTATION" | "KEYHOLE" | "MIXED_REALITY_FOV" | "HANDHELD_CAMERA_FOV" => {
            return degrees(s.float(o.key));
        }
        "WORLD_ROTATION_INCREMENT" => {
            let v = s.float(o.key);
            return if v == 0.0 {
                Shown::Lang("vivecraft.options.smooth".into())
            } else {
                degrees(v)
            };
        }
        "TELEPORT_UP_LIMIT" | "TELEPORT_DOWN_LIMIT" | "TELEPORT_HORIZ_LIMIT" => {
            let v = s.int(o.key);
            return if v > 0 {
                Shown::LangArg("vivecraft.options.teleportlimit".into(), v.to_string())
            } else {
                Shown::Lang("options.off".into())
            };
        }
        "GUI_SCALE" => {
            let v = s.int(o.key);
            return if v == 0 {
                Shown::Lang("options.guiScale.auto".into())
            } else if s.bool("doubleGUIResolution") {
                Shown::Literal(v.to_string())
            } else {
                Shown::Literal(((v as f32 * 0.5).ceil() as i32).to_string())
            };
        }
        "HORIZON_REFRESH_RATE" => {
            return Shown::LangArg("horizon.vr.hertz".into(), format!("{:.0}", s.float(o.key)));
        }
        _ => {}
    }
    match d.kind {
        Kind::Bool => Shown::Lang(if s.bool(o.key) { o.on } else { o.off }.into()),
        Kind::Enum(_) => Shown::Lang(enum_lang_key(o.class, s.enum_name(o.key))),
        Kind::Int => Shown::Literal(s.int(o.key).to_string()),
        Kind::Float if o.decimals < 0 => {
            Shown::Literal(format!("{}%", (s.float(o.key) * 100.0).round() as i32))
        }
        Kind::Float => Shown::Literal(format!("{:.*}", o.decimals as usize, s.float(o.key))),
        Kind::Str => Shown::Literal(s.string(o.key).into()),
        Kind::Vec3 => Shown::Literal(s.string(o.key).into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_quest_bindings_are_reported_and_unbinding_clears_the_conflict() {
        let mut s = VrSettings::default();
        assert!(!binding_conflict(&s, "questBindJump"));
        s.set("questBindInventory", "MAIN_SECONDARY");
        assert!(binding_conflict(&s, "questBindJump"));
        assert!(binding_conflict(&s, "questBindInventory"));
        s.set("questBindJump", "NONE");
        assert!(!binding_conflict(&s, "questBindJump"));
        assert!(!binding_conflict(&s, "questBindInventory"));
    }

    #[test]
    fn every_option_binds_a_known_setting() {
        for o in OPTS {
            assert!(def(o.key).is_some(), "{}", o.key);
        }
        assert_eq!(info("hudOpacity").unwrap().name, "HUD_OPACITY");
        assert_eq!(info("vrHudLockMode").unwrap().class, "hudlock");
    }

    #[test]
    fn index_sliders_map_to_vivecrafts_values() {
        let mut s = VrSettings::default();
        let scale = info("worldScale").unwrap();
        // 1.0x is index 7 of 29.
        let (at, steps) = slider(&s, scale).unwrap();
        assert_eq!(steps, 29);
        assert!((at - 7.0 / 29.0).abs() < 1e-5);
        set_slider(&mut s, scale, 18.0 / 29.0, None);
        assert_eq!(s.float("worldScale"), 3.0);
        set_slider(&mut s, scale, 1.0, None);
        assert_eq!(s.float("worldScale"), 100.0);
        let rot = info("vrWorldRotationIncrement").unwrap();
        // 45 degrees is index 2 of -1..4.
        assert!((slider(&s, rot).unwrap().0 - 3.0 / 5.0).abs() < 1e-5);
        set_slider(&mut s, rot, 0.0, None);
        assert_eq!(s.float("vrWorldRotationIncrement"), 0.0);
        assert_eq!(
            shown(&s, rot),
            Shown::Lang("vivecraft.options.smooth".into())
        );
    }

    #[test]
    fn displays_and_cycles_follow_the_options() {
        let mut s = VrSettings::default();
        let hud = info("vrHudLockMode").unwrap();
        assert_eq!(
            shown(&s, hud),
            Shown::Lang("vivecraft.options.hudlock.wrist".into())
        );
        cycle(&mut s, hud, false);
        assert_eq!(s.enum_name("vrHudLockMode"), "HAND");
        cycle(&mut s, hud, true);
        cycle(&mut s, hud, true);
        assert_eq!(s.enum_name("vrHudLockMode"), "HEAD");
        let free = info("vrFreeMoveMode").unwrap();
        s.set("vrFreeMoveMode", "WAIST");
        cycle(&mut s, free, false);
        assert_eq!(s.enum_name("vrFreeMoveMode"), "CONTROLLER");
        let dist = info("hudDistance").unwrap();
        s.set_float("hudDistance", 2.0);
        assert_eq!(shown(&s, dist), Shown::Literal("2.00m".into()));
        s.set_int("teleportLimitUp", 0);
        assert_eq!(
            shown(&s, info("teleportLimitUp").unwrap()),
            Shown::Lang("options.off".into())
        );
    }

    #[test]
    fn a_server_range_limits_a_slider() {
        let mut s = VrSettings::default();
        let o = info("movementSpeedMultiplier").unwrap();
        set_slider(&mut s, o, 1.0, Some((0.2, 0.5)));
        assert!((s.float("movementSpeedMultiplier") - 0.5).abs() < 1e-5);
    }
}
