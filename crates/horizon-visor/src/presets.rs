// SPDX-License-Identifier: MPL-2.0
//! The built-in settings presets of Visor (`core/client/settings/presets`: `defaults` and
//! `defaults_hand_hud`) for the settings Horizon has. Copyright (C) Phoenix-Ra and Visor
//! contributors; MPL-2.0 (see LICENSE-MPL-2.0).
//!
//! Visor's presets also reset its overlay layout and action sets; Horizon's overlays are its own
//! (the HUD placement is the `vrHudLockMode` setting), so a preset here is a list of settings.

use horizon_vivecraft::settings::VrSettings;

/// A preset: the buttons' label and the settings it sets.
#[derive(Clone, Copy, Debug)]
pub struct Preset {
    pub id: &'static str,
    /// The label of the button that applies it.
    pub name_key: &'static str,
    pub values: &'static [(&'static str, &'static str)],
}

/// `VRSettingsPresetDefaults`: Visor's defaults. The look-and-feel values are
/// [`horizon_vivecraft::settings::VISOR_PROFILE`] (walking by the head, 45 degree turns, the HUD on the
/// head, the grip wheel); these are the ones that go with them (the controller's movement, the
/// height following the model, hands only, no tracking, the world one to one, the protocol chosen by the
/// server).
pub const DEFAULTS: Preset = Preset {
    id: "defaults",
    name_key: "visor.options.presets.types.defaults.name",
    values: &[
        ("horizonVrProtocol", "AUTO"),
        ("visorMovementMode", "CONTROLLER"),
        ("visorHeightMode", "MATCH_MODEL"),
        ("visorBodyType", "HANDS_ONLY"),
        ("visorHandTracking", "false"),
        ("visorBodyTracking", "false"),
        ("worldScale", "1.0"),
    ],
};

/// `VRSettingsPresetHandHud`: the HUD elements ride on the off hand (the `hud_health` and `hud_up`
/// overlays of `defaults_hand_hud.yml` sit on the off hand): here the HUD goes on the wrist.
pub const HAND_HUD: Preset = Preset {
    id: "defaults_hand_hud",
    name_key: "visor.options.presets.types.defaults_hand_hud.name",
    values: &[("vrHudLockMode", "WRIST")],
};

pub const QUEST_CONTROLS: Preset = Preset {
    id: "quest_controls",
    name_key: "horizon.vr.bindings.reset",
    values: &[
        ("questBindOffAttack", "OFF_TRIGGER"),
        ("questBindJump", "MAIN_SECONDARY"),
        ("questBindAttack", "MAIN_TRIGGER"),
        ("questBindUse", "MAIN_PRIMARY"),
        ("questBindOffUse", "OFF_PRIMARY"),
        ("questBindInventory", "NONE"),
        ("questBindDrop", "NONE"),
        ("questBindSwap", "NONE"),
        ("questBindChat", "NONE"),
        ("questBindKeyboard", "NONE"),
        ("questBindSneak", "MAIN_STICK"),
        ("questBindSprint", "NONE"),
        ("questBindMainHotbar", "MAIN_GRIP"),
        ("questBindOffHotbar", "OFF_GRIP"),
        ("questBindMenu", "OFF_SECONDARY"),
        ("questMoveStick", "OFF"),
        ("questTurnStick", "MAIN"),
    ],
};
pub const ALL: [Preset; 3] = [DEFAULTS, HAND_HUD, QUEST_CONTROLS];

pub fn by_id(id: &str) -> Option<&'static Preset> {
    ALL.iter().find(|p| p.id == id)
}

/// Lays the preset over the settings; false when the id is unknown.
pub fn apply(id: &str, settings: &mut VrSettings) -> bool {
    let Some(preset) = by_id(id) else {
        return false;
    };
    if id == DEFAULTS.id {
        settings.apply_visor_profile();
    }
    for (key, value) in preset.values {
        settings.set(key, value);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restoring_quest_controls_preserves_height_world_scale_and_handedness() {
        let mut s = VrSettings::default();
        s.set_float("manualCalibration", 1.8);
        s.set_float("worldScale", 2.0);
        s.set_bool("reverseHands", true);
        s.set("questBindJump", "OFF_PRIMARY");
        s.set("questMoveStick", "MAIN");
        assert!(apply("quest_controls", &mut s));
        assert_eq!(s.enum_name("questBindJump"), "MAIN_SECONDARY");
        assert_eq!(s.enum_name("questMoveStick"), "OFF");
        assert_eq!(s.float("manualCalibration"), 1.8);
        assert_eq!(s.float("worldScale"), 2.0);
        assert!(s.left_handed());
    }

    #[test]
    fn every_key_of_every_preset_is_a_setting_with_an_accepted_value() {
        for preset in ALL {
            let mut s = VrSettings::horizon();
            for (key, value) in preset.values {
                assert!(
                    horizon_vivecraft::settings::def(key).is_some(),
                    "{} has no setting {key}",
                    preset.id
                );
                assert!(s.set(key, value), "{key} refuses {value}");
            }
        }
    }

    #[test]
    fn the_hand_hud_preset_moves_the_hud_and_defaults_bring_it_back() {
        let mut s = VrSettings::horizon();
        assert!(apply("defaults_hand_hud", &mut s));
        assert_eq!(s.enum_name("vrHudLockMode"), "WRIST");
        s.set("visorHeightMode", "REAL_SIZE");
        s.set("visorSwingSpeedThreshold", "5.5");
        s.set("vrWorldRotationIncrement", "30.0");
        assert!(apply("defaults", &mut s));
        // Visor's defaults: the HUD on the head, 45 degree turns, the swing threshold 3.
        assert_eq!(s.enum_name("vrHudLockMode"), "HEAD");
        assert_eq!(s.float("vrWorldRotationIncrement"), 45.0);
        assert_eq!(s.enum_name("visorHeightMode"), "MATCH_MODEL");
        assert_eq!(s.float("visorSwingSpeedThreshold"), 3.0);
        assert!(!apply("nope", &mut s));
    }
}
