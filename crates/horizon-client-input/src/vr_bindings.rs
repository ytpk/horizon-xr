//! Rebindable Quest gameplay inputs. Main/off follow reverseHands; poses never affect buttons.
use crate::headset::{HandButtons, VrSettings};

/// `hands` is main, off, rather than physical left, right. Values preserve analog triggers.
pub fn value(settings: &VrSettings, action: &str, hands: [HandButtons; 2], menu: bool) -> f32 {
    let source = settings.enum_name(action);
    let button = match source {
        "MAIN_TRIGGER" => return hands[0].trigger,
        "OFF_TRIGGER" => return hands[1].trigger,
        "MAIN_GRIP" => return hands[0].grip,
        "OFF_GRIP" => return hands[1].grip,
        "MAIN_PRIMARY" => hands[0].primary,
        "MAIN_SECONDARY" => hands[0].secondary,
        "OFF_PRIMARY" => hands[1].primary,
        "OFF_SECONDARY" => hands[1].secondary,
        "MAIN_STICK" => hands[0].stick_click,
        "OFF_STICK" => hands[1].stick_click,
        "MENU" => menu,
        _ => false,
    };
    f32::from(u8::from(button))
}

pub fn held(settings: &VrSettings, action: &str, hands: [HandButtons; 2], menu: bool) -> bool {
    value(settings, action, hands, menu) > 0.5
}

/// A UI / bow grab consumes physical triggers only, not other rebound buttons.
pub fn uncaptured_value(
    settings: &VrSettings,
    action: &str,
    mut hands: [HandButtons; 2],
    captured: [bool; 2],
) -> f32 {
    for (hand, taken) in hands.iter_mut().zip(captured) {
        if taken {
            hand.trigger = 0.0;
        }
    }
    value(settings, action, hands, false)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn captured_triggers_do_not_suppress_other_hands_or_rebound_face_buttons() {
        let mut settings = VrSettings::default();
        let hands = [
            HandButtons {
                trigger: 0.8,
                primary: true,
                ..Default::default()
            },
            HandButtons {
                trigger: 0.9,
                ..Default::default()
            },
        ];
        assert_eq!(
            uncaptured_value(&settings, "questBindAttack", hands, [true, false]),
            0.0
        );
        assert_eq!(
            uncaptured_value(&settings, "questBindOffAttack", hands, [true, false]),
            0.9
        );
        settings.set("questBindAttack", "MAIN_PRIMARY");
        assert_eq!(
            uncaptured_value(&settings, "questBindAttack", hands, [true, true]),
            1.0
        );
    }

    #[test]
    fn defaults_and_rebinding_route_actual_hand_states_and_round_trip() {
        let mut settings = VrSettings::default();
        let hands = [
            HandButtons {
                secondary: true,
                trigger: 0.75,
                ..Default::default()
            },
            HandButtons {
                primary: true,
                ..Default::default()
            },
        ];
        assert!(held(&settings, "questBindJump", hands, false));
        assert!(!held(&settings, "questBindInventory", hands, false));
        assert_eq!(value(&settings, "questBindAttack", hands, false), 0.75);
        assert!(settings.set("questBindJump", "OFF_PRIMARY"));
        assert!(settings.set("questBindAttack", "NONE"));
        let mut restored = VrSettings::default();
        restored.load_json(&settings.to_json());
        assert!(held(&restored, "questBindJump", hands, false));
        assert!(!held(
            &restored,
            "questBindJump",
            [hands[0], HandButtons::default()],
            false
        ));
        assert_eq!(value(&restored, "questBindAttack", hands, false), 0.0);
        assert!(!restored.set("questBindJump", "INVALID"));
    }
}
