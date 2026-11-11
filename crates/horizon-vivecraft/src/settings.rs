// SPDX-License-Identifier: LGPL-3.0-or-later
//! Ports `org.vivecraft.client_vr.settings.VRSettings`: option names, defaults, ranges and
//! the `vivecraft-client-config.json` format (a flat JSON object of strings).
//! Copyright (c) Vivecraft contributors; LGPL-3.0-or-later (see LICENSE-LGPL-3.0).
//!
//! The table in `settings_table.rs` is generated from Vivecraft's `@SettingField` fields:
//! the OpenVR, mixed reality, mirror and external camera settings (out of scope on a
//! standalone headset) and the array settings (radial menu, quick commands, keyboard
//! layouts) are left out; those live in their own modules when they are ported.

use std::collections::BTreeMap;

/// Horizon's own settings file before the Vivecraft table.
const LEGACY_FILE: &str = "horizon-vr.txt";

#[path = "settings_table.rs"]
mod table;

pub use table::DEFS;

/// What a setting holds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Bool,
    Int,
    Float,
    Str,
    /// A `x,y,z` vector.
    Vec3,
    /// One of these names, as the Java enum constants are written.
    Enum(&'static [&'static str]),
}

/// A setting's definition.
#[derive(Clone, Copy, Debug)]
pub struct Def {
    /// The config key.
    pub key: &'static str,
    pub kind: Kind,
    /// The default as the config stores it.
    pub default: &'static str,
    /// The options screen's slider `(min, max, step)`. Note `worldScale` and
    /// `vrWorldRotationIncrement` sliders are index sliders in Vivecraft, mapped to values.
    pub range: Option<(f32, f32, f32)>,
}

/// The definition for a key.
pub fn def(key: &str) -> Option<&'static Def> {
    DEFS.iter().find(|d| d.key == key)
}

/// The values the enum options take, for the options screen.
pub fn enum_values(key: &str) -> Option<&'static [&'static str]> {
    match def(key)?.kind {
        Kind::Enum(v) => Some(v),
        _ => None,
    }
}

/// The look and feel of Visor 0.6.1, as values of this table's keys. Each line names the Visor
/// default it stands for (`VRClientSettings`, `VROverlayHUD`, the input actions); everything else
/// (the player's hand, height and body choices, the performance switches) is left as it is.
pub const VISOR_PROFILE: &[(&str, &str)] = &[
    // `rotationMode = HMD`: the stick walks where the head looks.
    ("vrFreeMoveMode", "HMD"),
    // `rotationFlyMode = OFFHAND`: flying goes where the off-hand controller points.
    ("vrFreeMoveFlyMode", "CONTROLLER"),
    // `worldRotationStep = 45` degrees per snap turn.
    ("vrWorldRotationIncrement", "45.0"),
    // `autoSprint = true`, `sprintThreshold = 0.9`.
    ("autoSprint", "true"),
    ("autoSprintThreshold", "0.9"),
    // The `hud` overlay: head-locked, 1 m wide, 1.2 m ahead and a little below the eyes.
    ("vrHudLockMode", "HEAD"),
    ("hudDistance", "1.2"),
    ("headHudScale", "1.0"),
    ("hudOpacity", "1.0"),
    // Chat and signs use the keyboard on the screen's panel (Visor's `showKeyboard`); the keyboard
    // in the room opens from the game menu, not by itself.
    ("autoOpenKeyboard", "OFF"),
    // The hotbar is the grip wheel (`TaskHotBar`), not touching a HUD on the arm.
    ("vrTouchHotbar", "false"),
    // Screens open in front of the face; there is no "over the block" placement.
    ("guiAppearOverBlock", "false"),
    ("menuAlwaysFollowFace", "false"),
    // Visor has no locomotion vignette, only the one near a block (`VRShaderInBlockVignette`).
    ("fovReduction", "false"),
    // Immersion defaults, all on (`room_sneak`, `room_jump`, `room_climb`, `room_swim`, ...).
    ("realisticSneakEnabled", "true"),
    ("realisticJumpEnabled", "ON"),
    ("realisticClimbEnabled", "true"),
    ("realisticSwimEnabled", "true"),
    ("realisticRowEnabled", "true"),
    ("realisticDismountEnabled", "true"),
    ("realisticBlockInteractEnabled", "true"),
    ("realisticEntityInteractEnabled", "true"),
    // `swing.speed_threshold = 3.0`, `in_block` vignette on.
    ("visorSwingSpeedThreshold", "3.0"),
    ("visorInBlockVignette", "true"),
];

/// `VRSettings`' values; every key of [`DEFS`] has one.
#[derive(Clone, Debug, PartialEq)]
pub struct VrSettings {
    values: BTreeMap<&'static str, String>,
}

impl Default for VrSettings {
    fn default() -> Self {
        Self {
            values: DEFS
                .iter()
                .map(|d| {
                    let v = match (d.kind, d.default.parse::<f32>()) {
                        (Kind::Float, Ok(f)) => java_float(f),
                        _ => d.default.to_string(),
                    };
                    (d.key, v)
                })
                .collect(),
        }
    }
}

/// Quest uses a linear per-eye scale, not Vivecraft's desktop supersampling range.
pub const RENDER_SCALE_MIN: f32 = 0.25;
pub const RENDER_SCALE_MAX: f32 = 2.0;

pub fn render_scale(value: f32) -> f32 {
    if value.is_finite() && value > 0.0 {
        value.clamp(RENDER_SCALE_MIN, RENDER_SCALE_MAX)
    } else {
        1.0
    }
}

impl VrSettings {
    /// What a fresh Horizon install starts with: Vivecraft's defaults, but a render scale a
    /// standalone headset's GPU can sustain.
    pub fn horizon() -> Self {
        let mut s = Self::default();
        s.set_float("renderScaleFactor", 0.4);
        s.apply_visor_profile();
        s
    }

    /// Puts the values of [`VISOR_PROFILE`] in and marks the settings as brought to Visor's look.
    pub fn apply_visor_profile(&mut self) {
        for (key, value) in VISOR_PROFILE {
            let ok = self.set(key, value);
            debug_assert!(
                ok,
                "the Visor profile names a setting that does not take it: {key}"
            );
        }
        self.set_int("visorProfile", 1);
    }

    /// The file the options are kept in, in the game's home.
    pub const FILE: &'static str = "vivecraft-client-config.json";

    /// Horizon's defaults, then the config file in `dir` over them. A missing file leaves the
    /// defaults, except that a pre-Vivecraft `horizon-vr.txt` is migrated once.
    pub fn load(dir: &std::path::Path) -> Self {
        let mut s = Self::horizon();
        match std::fs::read_to_string(dir.join(Self::FILE)) {
            Ok(text) => {
                // A file without the marker is older than the Visor look.
                s.set_int("visorProfile", 0);
                s.load_json(&text);
                // A file written before the Visor look: keep it beside the new one, then move the
                // values the profile names over (the rest, the player's own choices, stay).
                if s.int("visorProfile") < 1 {
                    let _ = std::fs::write(dir.join(format!("{}.pre-visor", Self::FILE)), &text);
                    s.apply_visor_profile();
                }
            }
            Err(_) => {
                if let Ok(old) = std::fs::read_to_string(dir.join(LEGACY_FILE)) {
                    s.migrate_legacy(&old);
                }
            }
        }
        s
    }

    /// Writes the config once something differs from Horizon's defaults (or the file exists),
    /// so a flat screen's player gets no new file.
    pub fn save(&self, dir: &std::path::Path) {
        let path = dir.join(Self::FILE);
        if *self != Self::horizon() || path.exists() {
            let _ = std::fs::write(path, self.to_json());
        }
    }

    /// Takes over the values of Horizon's earlier home-made `horizon-vr.txt` (`key:value`
    /// lines) that have a Vivecraft counterpart.
    pub fn migrate_legacy(&mut self, text: &str) {
        for line in text.lines() {
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let (key, value) = (key.trim(), value.trim());
            let flag = value == "true";
            match key {
                "seated" => self.set_bool("seated", flag),
                "reverseHands" => self.set_bool("reverseHands", flag),
                "freeMove" => self.set(
                    "vrFreeMoveMode",
                    if value == "hmd" { "HMD" } else { "CONTROLLER" },
                ),
                "snapTurn" => value
                    .parse::<f32>()
                    .is_ok_and(|v| self.set_float("vrWorldRotationIncrement", v.max(0.0))),
                "vignette" => value
                    .parse::<f32>()
                    .is_ok_and(|v| self.set_bool("fovReduction", v > 0.0)),
                "hud" => self.set(
                    "vrHudLockMode",
                    match value {
                        "hand" => "HAND",
                        "head" => "HEAD",
                        _ => "WRIST",
                    },
                ),
                "hudOpacity" => value
                    .parse::<f32>()
                    .is_ok_and(|v| self.set_float("hudOpacity", v.clamp(0.15, 1.0))),
                "hudScale" => value
                    .parse::<f32>()
                    .is_ok_and(|v| self.set_float("headHudScale", v.clamp(0.35, 2.5))),
                "guiDistance" => value
                    .parse::<f32>()
                    .is_ok_and(|v| self.set_float("hudDistance", v.clamp(0.25, 5.0))),
                "renderScale" => value
                    .parse::<f32>()
                    .is_ok_and(|v| self.set_float("renderScaleFactor", render_scale(v))),
                "refreshRate" => value
                    .parse::<f32>()
                    .is_ok_and(|v| self.set_float("horizonRefreshRate", v.clamp(72.0, 120.0))),
                "swingAttack" => self.set("weaponCollisionNew", if flag { "ON" } else { "OFF" }),
                "swingBreak" => self.set_bool("blockCollision", flag),
                "touchInteract" => {
                    self.set_bool("realisticBlockInteractEnabled", flag)
                        && self.set_bool("realisticEntityInteractEnabled", flag)
                }
                "physicalCrouch" => self.set_bool("realisticSneakEnabled", flag),
                "physicalJump" => self.set("realisticJumpEnabled", if flag { "ON" } else { "OFF" }),
                "menuOverBlock" => self.set_bool("guiAppearOverBlock", flag),
                "swimStrokes" => self.set_bool("realisticSwimEnabled", flag),
                _ => false,
            };
        }
    }

    fn raw(&self, key: &str) -> &str {
        match self.values.get(key) {
            Some(v) => v,
            None => {
                debug_assert!(false, "unknown VR setting {key}");
                ""
            }
        }
    }

    /// The stored value as text.
    pub fn raw_value(&self, key: &str) -> String {
        self.raw(key).to_string()
    }

    pub fn bool(&self, key: &str) -> bool {
        self.raw(key) == "true"
    }
    pub fn int(&self, key: &str) -> i32 {
        self.raw(key).parse().unwrap_or(0)
    }
    pub fn float(&self, key: &str) -> f32 {
        self.raw(key).parse().unwrap_or(0.0)
    }
    pub fn string(&self, key: &str) -> &str {
        self.raw(key)
    }
    /// The enum constant's name, e.g. `WRIST`.
    pub fn enum_name(&self, key: &str) -> &str {
        self.raw(key)
    }
    pub fn vec3(&self, key: &str) -> [f32; 3] {
        let mut out = [0.0; 3];
        for (o, p) in out.iter_mut().zip(self.raw(key).split(',')) {
            *o = p.trim().parse().unwrap_or(0.0);
        }
        out
    }

    /// Sets a value if it is valid for the key's kind (a bad enum name or number is
    /// refused). Render scale is normalized to the supported headset range; other numeric
    /// settings retain their parsed value, including values beyond UI slider limits.
    pub fn set(&mut self, key: &str, value: &str) -> bool {
        let Some(d) = def(key) else {
            return false;
        };
        let stored = match d.kind {
            Kind::Bool => match value {
                "true" | "false" => value.to_string(),
                _ => return false,
            },
            Kind::Int => match value.parse::<i32>() {
                Ok(v) => v.to_string(),
                Err(_) => return false,
            },
            Kind::Float => match value.parse::<f32>() {
                Ok(v) if v.is_finite() => java_float(if key == "renderScaleFactor" {
                    render_scale(v)
                } else {
                    v
                }),
                _ => return false,
            },
            Kind::Str => value.to_string(),
            Kind::Vec3 => {
                let parts: Vec<_> = value.split(',').map(|p| p.trim().parse::<f32>()).collect();
                if parts.len() != 3 || parts.iter().any(Result::is_err) {
                    return false;
                }
                value.to_string()
            }
            Kind::Enum(names) => {
                if !names.contains(&value) {
                    return false;
                }
                value.to_string()
            }
        };
        self.values.insert(d.key, stored);
        true
    }

    pub fn set_bool(&mut self, key: &str, v: bool) -> bool {
        self.set(key, if v { "true" } else { "false" })
    }
    pub fn set_float(&mut self, key: &str, v: f32) -> bool {
        self.set(key, &java_float(v))
    }
    pub fn set_int(&mut self, key: &str, v: i32) -> bool {
        self.set(key, &v.to_string())
    }

    /// `vivecraft-client-config.json`: a flat object, unknown keys kept out, bad values
    /// ignored (the default stays).
    pub fn load_json(&mut self, text: &str) -> usize {
        let mut loaded = 0;
        for (k, v) in parse_flat_json(text) {
            if self.set(&k, &v) {
                loaded += 1;
            }
        }
        loaded
    }

    /// Writes the config; only the values that differ from the defaults would be needed,
    /// but Vivecraft writes them all.
    pub fn to_json(&self) -> String {
        let mut s = String::from("{\n");
        let n = DEFS.len();
        for (i, d) in DEFS.iter().enumerate() {
            s.push_str("  \"");
            s.push_str(d.key);
            s.push_str("\": \"");
            for c in self.raw(d.key).chars() {
                match c {
                    '"' => s.push_str("\\\""),
                    '\\' => s.push_str("\\\\"),
                    '\n' => s.push_str("\\n"),
                    c => s.push(c),
                }
            }
            s.push('"');
            if i + 1 < n {
                s.push(',');
            }
            s.push('\n');
        }
        s.push('}');
        s
    }
}

/// Java's `Float.toString` for the finite values settings hold: always a decimal point.
fn java_float(v: f32) -> String {
    let s = format!("{v}");
    if s.contains('.') || s.contains('e') || s.contains('E') {
        s
    } else {
        format!("{s}.0")
    }
}

/// A flat JSON object whose values are strings, numbers or booleans.
fn parse_flat_json(text: &str) -> Vec<(String, String)> {
    let b: Vec<char> = text.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    let skip_ws = |i: &mut usize| {
        while *i < b.len() && b[*i].is_whitespace() {
            *i += 1;
        }
    };
    let string = |i: &mut usize| -> Option<String> {
        if b.get(*i) != Some(&'"') {
            return None;
        }
        *i += 1;
        let mut s = String::new();
        while *i < b.len() {
            match b[*i] {
                '"' => {
                    *i += 1;
                    return Some(s);
                }
                '\\' => {
                    *i += 1;
                    match b.get(*i)? {
                        'n' => s.push('\n'),
                        't' => s.push('\t'),
                        'u' => {
                            let hex: String = b.get(*i + 1..*i + 5)?.iter().collect();
                            s.push(char::from_u32(u32::from_str_radix(&hex, 16).ok()?)?);
                            *i += 4;
                        }
                        c => s.push(*c),
                    }
                    *i += 1;
                }
                c => {
                    s.push(c);
                    *i += 1;
                }
            }
        }
        None
    };
    skip_ws(&mut i);
    if b.get(i) != Some(&'{') {
        return out;
    }
    i += 1;
    loop {
        skip_ws(&mut i);
        let Some(key) = string(&mut i) else {
            return out;
        };
        skip_ws(&mut i);
        if b.get(i) != Some(&':') {
            return out;
        }
        i += 1;
        skip_ws(&mut i);
        let value = if b.get(i) == Some(&'"') {
            string(&mut i)
        } else {
            let start = i;
            while i < b.len() && !matches!(b[i], ',' | '}') && !b[i].is_whitespace() {
                i += 1;
            }
            Some(b[start..i].iter().collect())
        };
        let Some(value) = value else {
            return out;
        };
        out.push((key, value));
        skip_ws(&mut i);
        match b.get(i) {
            Some(',') => i += 1,
            _ => return out,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_install_starts_with_visors_defaults() {
        let s = VrSettings::horizon();
        // Visor 0.6.1: the stick walks where the head looks, 45 degree snap turns, the HUD on the
        // head 1.2 m ahead, the grip wheel for the hotbar.
        assert_eq!(s.enum_name("vrFreeMoveMode"), "HMD");
        assert_eq!(s.enum_name("vrFreeMoveFlyMode"), "CONTROLLER");
        assert_eq!(s.float("vrWorldRotationIncrement"), 45.0);
        assert_eq!(s.enum_name("vrHudLockMode"), "HEAD");
        assert_eq!(s.float("hudDistance"), 1.2);
        assert!(!s.bool("vrTouchHotbar"));
        assert!(!s.bool("guiAppearOverBlock"));
        assert!(!s.bool("fovReduction"));
        assert_eq!(s.int("visorProfile"), 1);
        // The table itself is still Vivecraft's.
        assert_eq!(VrSettings::default().enum_name("vrHudLockMode"), "WRIST");
    }

    #[test]
    fn an_older_file_is_moved_to_visors_look_once_and_kept_beside_it() {
        let dir =
            std::env::temp_dir().join(format!("horizon-visor-profile-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // A Vivecraft-era file: wrist HUD, 30 degree turns, and the player's own choices.
        let old = r#"{"vrHudLockMode":"WRIST","vrWorldRotationIncrement":"30.0","hudDistance":"1.6","reverseHands":"true","visorHeightMode":"REAL_SIZE","renderScaleFactor":"0.7"}"#;
        std::fs::write(dir.join(VrSettings::FILE), old).unwrap();
        let s = VrSettings::load(&dir);
        assert_eq!(s.enum_name("vrHudLockMode"), "HEAD");
        assert_eq!(s.float("vrWorldRotationIncrement"), 45.0);
        assert_eq!(s.float("hudDistance"), 1.2);
        // Hands, height mode and render scale are the player's.
        assert!(s.bool("reverseHands"));
        assert_eq!(s.enum_name("visorHeightMode"), "REAL_SIZE");
        assert!((s.float("renderScaleFactor") - 0.7).abs() < 1e-6);
        let kept =
            std::fs::read_to_string(dir.join(format!("{}.pre-visor", VrSettings::FILE))).unwrap();
        assert_eq!(kept, old);
        // Saved and loaded again: the player's later choice of a wrist HUD is not undone.
        let mut s = s;
        s.set("vrHudLockMode", "WRIST");
        s.save(&dir);
        let again = VrSettings::load(&dir);
        assert_eq!(again.enum_name("vrHudLockMode"), "WRIST");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn defaults_are_vivecrafts() {
        let s = VrSettings::default();
        assert!(!s.bool("seated"));
        assert_eq!(s.enum_name("vrFreeMoveMode"), "CONTROLLER");
        assert_eq!(s.enum_name("vrFreeMoveFlyMode"), "AUTO");
        assert_eq!(s.enum_name("vrHudLockMode"), "WRIST");
        assert_eq!(s.float("headHudScale"), 1.0);
        assert_eq!(s.float("hudDistance"), 1.25);
        assert_eq!(s.float("vrWorldRotationIncrement"), 45.0);
        assert_eq!(s.int("teleportLimitUp"), 1);
        assert_eq!(s.int("teleportLimitDown"), 4);
        assert_eq!(s.int("teleportLimitHoriz"), 16);
        assert!(s.bool("analogMovement"));
        assert!(s.bool("autoSprint"));
        assert_eq!(s.float("autoSprintThreshold"), 0.9);
        assert_eq!(s.enum_name("bowMode"), "ON");
        assert_eq!(s.int("vrRadialButtons"), 8);
        assert_eq!(s.string("chatNotificationSound"), "block.note_block.bell");
        assert_eq!(s.vec3("originOffset"), [0.0; 3]);
        assert_eq!(s.float("jumpThreshold"), 0.05);
        assert_eq!(s.float("sneakThreshold"), 0.4);
        assert_eq!(s.float("crawlThreshold"), 0.82);
    }

    #[test]
    fn every_default_is_valid_for_its_kind() {
        let mut s = VrSettings::default();
        for d in DEFS {
            assert!(s.set(d.key, d.default), "{}", d.key);
        }
    }

    #[test]
    fn the_old_home_made_file_migrates() {
        let mut s = VrSettings::horizon();
        s.migrate_legacy(
            "seated:true\nsnapTurn:90\nhud:head\nphysicalJump:false\nlaser:off\nrenderScale:0.5\n",
        );
        assert!(s.seated());
        assert_eq!(s.float("vrWorldRotationIncrement"), 90.0);
        assert_eq!(s.hud_lock(), crate::modes::HudLock::Head);
        assert_eq!(s.enum_name("realisticJumpEnabled"), "OFF");
        assert_eq!(s.float("renderScaleFactor"), 0.5);
    }

    #[test]
    fn keys_are_unique() {
        let mut keys: Vec<_> = DEFS.iter().map(|d| d.key).collect();
        keys.sort_unstable();
        let n = keys.len();
        keys.dedup();
        assert_eq!(keys.len(), n);
    }

    #[test]
    fn bad_values_are_refused() {
        let mut s = VrSettings::default();
        assert!(!s.set("vrHudLockMode", "ELBOW"));
        assert!(!s.set("seated", "yes"));
        assert!(!s.set("headHudScale", "NaN"));
        assert!(!s.set("nope", "1"));
        assert!(s.set("vrHudLockMode", "HEAD"));
        assert_eq!(s.enum_name("vrHudLockMode"), "HEAD");
    }

    #[test]
    fn json_round_trips_and_ignores_the_unknown() {
        let mut s = VrSettings::default();
        s.set_bool("seated", true);
        s.set("vrFreeMoveMode", "HMD");
        s.set_float("hudDistance", 2.0);
        s.set("backpackMainHandKeybind", "key.\"x\"");
        let json = s.to_json();
        let mut back = VrSettings::default();
        assert_eq!(back.load_json(&json), DEFS.len());
        assert_eq!(back, s);
        let mut other = VrSettings::default();
        other.load_json(
            r#"{"seated": true, "headHudScale": 1.5, "somethingNew": "x", "vrHudLockMode": "BAD"}"#,
        );
        assert!(other.bool("seated"));
        assert_eq!(other.float("headHudScale"), 1.5);
        assert_eq!(other.enum_name("vrHudLockMode"), "WRIST");
    }

    #[test]
    fn slider_ranges_are_the_vr_options() {
        assert_eq!(def("headHudScale").unwrap().range, Some((0.35, 2.5, 0.01)));
        assert_eq!(
            def("movementSpeedMultiplier").unwrap().range,
            Some((0.15, 1.3, 0.01))
        );
        assert_eq!(
            def("vrRadialButtons").unwrap().range,
            Some((4.0, 14.0, 2.0))
        );
        assert!(def("seated").unwrap().range.is_none());
    }
}
