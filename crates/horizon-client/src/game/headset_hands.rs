// SPDX-License-Identifier: LGPL-3.0-or-later
// Derived from Vivecraft: render/VivecraftItemRendering.java, render/VRArmRenderer.java. Copyright (c) Vivecraft contributors; LGPL-3.0-or-later.
//! How a headset's hands hold things, after Vivecraft's `VRArmRenderer`
//! and `VivecraftItemRendering`: each hand's arm and item are placed in its
//! controller's frame (the grip's place, turned as the controller points:
//! -Z along the ray, +Y up), the arm reaching back along the controller and
//! the item turned the way a hand holds that kind of thing — a sword by the
//! hilt pointing ahead, a pickaxe as a tool, a block in the palm, food at the
//! fingers, a shield on the forearm.
use glam::{Mat4, Quat, Vec3};

/// How a held item sits in the hand (Vivecraft's `VivecraftItemTransformType`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Held {
    /// A flat item (an ingot, a sapling, a door).
    Item,
    /// A block item drawn as a block.
    Block,
    /// A torch: upright in the fist.
    Stick,
    /// Something to eat or drink.
    Food,
    /// Swords, and every tool: held by the handle along the ray.
    Tool,
    /// A fishing rod or a carrot (fungus) on a stick.
    Rod,
    /// An arrow: point first.
    Arrow,
    Shield,
    /// A trident.
    Trident,
    /// A spear (`kinetic_weapon`).
    Lance,
    Crossbow,
    /// A compass or a clock: face up.
    Compass,
    Horn,
    Mace,
    Spyglass,
    /// A bow standing (it shows in the off hand's controller).
    Bow,
    /// A map (`MAP`): held up in front of the hand.
    Map,
}

impl Held {
    /// The kind of a held `item` (its id), `block` its block's facts' Java
    /// classes when it places one and whether that draws as a whole block,
    /// and `food` whether it is eaten or drunk.
    pub(super) fn of(
        item: &str,
        block: Option<(&[std::sync::Arc<str>], bool)>,
        food: bool,
        map: bool,
    ) -> Self {
        let name = item.strip_prefix("minecraft:").unwrap_or(item);
        if food {
            return Held::Food;
        }
        if let Some((classes, whole)) = block {
            if classes.iter().any(|c| c.ends_with("TorchBlock")) {
                return Held::Stick;
            }
            return if whole { Held::Block } else { Held::Item };
        }
        if map {
            return Held::Map;
        }
        match name {
            "bow" => Held::Bow,
            "crossbow" => Held::Crossbow,
            "shield" => Held::Shield,
            "trident" => Held::Trident,
            "mace" => Held::Mace,
            "spyglass" => Held::Spyglass,
            "goat_horn" => Held::Horn,
            "compass" | "recovery_compass" | "clock" => Held::Compass,
            "fishing_rod" | "carrot_on_a_stick" | "warped_fungus_on_a_stick" => Held::Rod,
            "arrow" | "spectral_arrow" | "tipped_arrow" => Held::Arrow,
            _ if name.ends_with("_spear") => Held::Lance,
            _ if name.ends_with("_sword")
                || name.ends_with("_pickaxe")
                || name.ends_with("_axe")
                || name.ends_with("_shovel")
                || name.ends_with("_hoe")
                || matches!(
                    name,
                    "shears"
                        | "flint_and_steel"
                        | "brush"
                        | "stick"
                        | "bone"
                        | "blaze_rod"
                        | "breeze_rod"
                        | "debug_stick"
                        | "bamboo"
                ) =>
            {
                Held::Tool
            }
            _ => Held::Item,
        }
    }
}

fn rx(degrees: f32) -> Quat {
    Quat::from_rotation_x(degrees.to_radians())
}
fn ry(degrees: f32) -> Quat {
    Quat::from_rotation_y(degrees.to_radians())
}
fn rz(degrees: f32) -> Quat {
    Quat::from_rotation_z(degrees.to_radians())
}

/// A controller's frame for what its hand holds: its aim pose's place and
/// turn (camera-relative world; -Z along the ray, +Y up), which hand it is,
/// and the "gun angle" (degrees) between the aim and the grip, which items
/// turn back by as Vivecraft's do for controllers held like a pistol.
#[derive(Clone, Copy, Debug)]
pub(super) struct HandFrame {
    pub(super) at: Mat4,
    pub(super) left: bool,
    pub(super) gun: f32,
    /// The controller's turn from its aim onto its grip (`item_pose::aim_to_grip`), which
    /// Visor's item poses are re-anchored by.
    pub(super) aim_to_grip: Quat,
}

/// The angle (degrees) between a controller's aim and grip directions
/// (Vivecraft's `gunAngle`, from the tip and the hand grip components).
pub(super) fn gun_angle(grip: Quat, aim: Quat) -> f32 {
    let dot = (grip * Vec3::NEG_Z).dot(aim * Vec3::NEG_Z).abs().min(1.0);
    dot.acos().to_degrees()
}

/// What the player is doing with a held item, which
/// `applyFirstPersonItemTransforms` reads off the player.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct HeldUse {
    /// This hand's item is in use (a shield raised, a trident charging).
    pub(super) using: bool,
    /// The shield is on cooldown.
    pub(super) cooldown: bool,
    /// Ticks the item has been used, for a trident's charge.
    pub(super) ticks_used: f32,
    /// A riptide spin attack runs.
    pub(super) spinning: bool,
    /// A spear's stab swing runs (Visor slides the spear instead of swinging the hand).
    pub(super) stab: bool,
    /// Ticks left of the item's use (food shakes in the mouth).
    pub(super) remaining: i32,
    /// Milliseconds on a clock, for the jitter of a charged trident.
    pub(super) millis: u64,
}

/// Where a held item's first-person (right hand) display transform starts,
/// in its controller's frame: Vivecraft's `applyFirstPersonItemTransforms`.
/// `right` is the physical hand, `state` what the player does with the item,
/// `gun` the controller's gun angle (degrees).
pub(super) fn item_transform(held: Held, right: bool, state: HeldUse, gun: f32) -> Mat4 {
    let using = state.using;
    let mut outer = Mat4::IDENTITY;
    let mut scale = 0.7;
    let mut at = Vec3::new(-0.05, 0.005, 0.0);
    let mut turn = rx(-110.0 + gun);
    let mut before = Quat::IDENTITY;
    let side = if right { 1.0 } else { -1.0 };
    match held {
        Held::Tool => {}
        Held::Bow => {
            before = turn;
            turn = Quat::IDENTITY;
            at += Vec3::new(-0.0225, -0.25, 0.025 + 0.03 * gun / 40.0);
            scale = 1.0;
        }
        Held::Crossbow => {
            turn = ry(10.0);
            at += Vec3::new(0.01, -0.02, -0.02);
            scale = 0.5;
        }
        Held::Food => {
            turn = rz(180.0) * rx(-135.0);
            // Eating jitter.
            at += Vec3::new(0.08, 0.0, 0.02 + 0.006 * (state.remaining as f32).sin());
            scale = 0.4;
        }
        Held::Map => {
            turn = rx(-45.0);
            at = Vec3::new(0.0, 0.16, -0.075);
            scale = 0.75;
        }
        Held::Item => {
            turn = rz(180.0) * rx(-135.0);
            at += Vec3::new(0.08, 0.0, -0.08);
            scale = 0.4;
        }
        Held::Compass => {
            turn = ry(90.0) * rx(25.0);
            scale = 0.4;
        }
        Held::Block => {
            at += Vec3::new(0.05, 0.0, -0.1);
            turn *= rx(90.0 - gun);
            scale = 0.3;
        }
        Held::Stick => {
            turn = rx(-45.0 + gun);
            at += Vec3::new(0.0, -0.105 + 0.06 * gun / 40.0, -0.1);
        }
        Held::Horn => {
            turn = rx(-45.0 + gun);
            at += Vec3::new(0.0, -0.105 + 0.06 * gun / 40.0, -0.1);
            scale = 0.3;
        }
        Held::Shield => {
            scale = 0.4;
            // Clear of the HUD above the hand.
            at.y += 0.18;
            if right {
                turn *= rx(105.0 - gun);
                at.x += 0.11;
            } else {
                turn *= rx(115.0 - gun);
                at.x -= 0.015;
            }
            at.z += 0.1;
            if using {
                turn *= rx(side * 5.0) * rz(-5.0);
                at += if right {
                    Vec3::new(0.04, -0.12, -0.1)
                } else {
                    Vec3::new(0.19, -0.12, -0.11)
                };
                turn *= ry(side * 90.0);
            }
            turn *= ry(side * -90.0);
            if state.cooldown {
                turn *= rz(side * 10.0);
                at.y -= 0.0055;
                at.z -= 0.035;
            }
        }
        Held::Trident => {
            at += Vec3::new(-0.135, 0.0, 0.575);
            scale = 0.6;
            turn = Quat::IDENTITY;
            let mut charging = false;
            let mut progress = 0.0;
            if using {
                charging = true;
                progress = state.ticks_used.max(0.0);
                // THROW_THRESHOLD_TIME: charged, it jitters.
                if progress > 10.0 {
                    progress = 10.0;
                    at.x += 0.003 * (state.millis as f32).sin();
                }
            }
            if state.spinning {
                at.z -= 0.15;
                charging = true;
                outer = Mat4::from_quat(rz(-(state.millis as f32 / 50.0) * 50.0));
            }
            if !charging {
                at.y += 0.2 * gun / 40.0;
                turn *= rx(gun);
            }
            turn *= rx(-65.0);
            at.z += -0.75 + progress / 10.0 * 0.25;
        }
        Held::Rod => {
            turn *= rx(40.0);
            at += Vec3::new(0.05, -0.02 + gun / 40.0 * 0.1, -0.15);
            scale = 0.8;
        }
        Held::Arrow => {
            before = rz(-180.0);
            turn *= rx(-gun);
        }
        Held::Spyglass => {
            turn = Quat::IDENTITY;
            scale *= 0.625;
            at = Vec3::new(-0.03 * side * scale, 0.0, -0.1 * scale);
        }
        Held::Mace => {
            before = rx(gun);
            turn *= rx(-gun);
            at = Vec3::new(0.0, 0.0125, -0.06);
            scale = 0.56;
        }
        Held::Lance => {
            at.x = -0.1375;
            turn *= rx(30.0);
        }
    }
    outer
        * Mat4::from_quat(before)
        * Mat4::from_translation(at)
        * Mat4::from_quat(turn)
        * Mat4::from_scale(Vec3::splat(scale))
}

/// `VRFirstPersonArmSwing`: how the hand moves while it swings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum SwingType {
    /// The hand tips forward and back.
    #[default]
    Attack,
    /// The hand pushes forward (using an item).
    Use,
    /// The hand turns sideways (touching things).
    Interact,
}

/// A hand's swing, `progress` 0..1 (Vivecraft's `transformFirstPersonVR`):
/// attack tips the hand forward and back about a point 0.2 behind the
/// controller, use pushes it ahead, interact turns it about its axis.
pub(super) fn swing(progress: f32, kind: SwingType, right: bool) -> Mat4 {
    if progress <= 0.0 {
        return Mat4::IDENTITY;
    }
    let pi = std::f32::consts::PI;
    let wave = if progress > 0.5 {
        (progress * pi + pi).sin()
    } else {
        (progress * 3.0 * pi).sin()
    };
    match kind {
        SwingType::Attack => {
            Mat4::from_translation(Vec3::new(0.0, 0.0, 0.2))
                * Mat4::from_quat(rx(wave * 30.0))
                * Mat4::from_translation(Vec3::new(0.0, 0.0, -0.2))
        }
        SwingType::Interact => Mat4::from_quat(rz(if right { -1.0 } else { 1.0 } * wave * 45.0)),
        SwingType::Use => {
            let forward = if progress > 0.25 {
                (progress / 2.0 * pi + pi).sin()
            } else {
                (progress * 2.0 * pi).sin()
            };
            Mat4::from_translation(Vec3::new(0.0, 0.0, -(1.0 + forward) * 0.1))
        }
    }
}

/// The player model's arm (its model part, pivot included) in a controller's
/// frame, after Vivecraft's `vrPlayerArm`: at 0.4 of its size, its hand end at
/// the controller and the shoulder end back toward the player, the outside
/// of the arm outward. `slim` arms are a pixel narrower.
pub(super) fn arm_transform(right: bool, slim: bool) -> Mat4 {
    let side = if right { 1.0 } else { -1.0 };
    let middle = if slim { 0.34375 } else { 0.375 };
    Mat4::from_scale(Vec3::splat(0.4))
        * Mat4::from_translation(Vec3::new(-middle * side, 0.0, 0.75))
        * Mat4::from_quat(rx(-90.0))
        * Mat4::from_quat(ry(180.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn items_fall_into_the_ways_they_are_held() {
        assert_eq!(
            Held::of("minecraft:diamond_sword", None, false, false),
            Held::Tool
        );
        assert_eq!(
            Held::of("minecraft:iron_pickaxe", None, false, false),
            Held::Tool
        );
        assert_eq!(Held::of("minecraft:bread", None, true, false), Held::Food);
        assert_eq!(
            Held::of("minecraft:shield", None, false, false),
            Held::Shield
        );
        assert_eq!(
            Held::of("minecraft:diamond", None, false, false),
            Held::Item
        );
        assert_eq!(
            Held::of("minecraft:filled_map", None, false, true),
            Held::Map
        );
        let torch: [std::sync::Arc<str>; 1] = ["BaseTorchBlock".into()];
        assert_eq!(
            Held::of("minecraft:torch", Some((&torch, false)), false, false),
            Held::Stick
        );
        assert_eq!(
            Held::of("minecraft:stone", Some((&[], true)), false, false),
            Held::Block
        );
        assert_eq!(
            Held::of("minecraft:poppy", Some((&[], false)), false, false),
            Held::Item
        );
    }

    #[test]
    fn the_arm_reaches_back_from_the_controller() {
        // A wide right arm's part: x -8..-4, y 0..12 (the hand end at 12), z -2..2 pixels.
        let m = arm_transform(true, false);
        let hand = m.transform_point3(Vec3::new(-6.0, 12.0, 0.0) / 16.0);
        let shoulder = m.transform_point3(Vec3::new(-6.0, 0.0, 0.0) / 16.0);
        assert!(hand.length() < 1e-4, "{hand}");
        // 0.3 m behind the controller (+Z), straight back.
        assert!(
            (shoulder - Vec3::new(0.0, 0.0, 0.3)).length() < 1e-4,
            "{shoulder}"
        );
    }

    #[test]
    fn a_swing_comes_back_to_rest() {
        assert_eq!(swing(0.0, SwingType::Use, true), Mat4::IDENTITY);
        let end = swing(1.0, SwingType::Attack, true).transform_point3(Vec3::ZERO);
        assert!(end.length() < 1e-4, "{end}");
        let mid =
            swing(1.0 / 6.0, SwingType::Attack, true).transform_point3(Vec3::new(0.0, 0.0, -0.3));
        assert!(mid.y > 0.05, "the swing lifts the hand's tip: {mid}");
    }

    #[test]
    fn a_map_is_held_up_in_front_of_the_hand() {
        // MAP: rotation -45 about X, translate (0, 0.16, -0.075), scale 0.75.
        let m = item_transform(Held::Map, true, HeldUse::default(), 40.0);
        let origin = m.transform_point3(Vec3::ZERO);
        assert!(
            (origin - Vec3::new(0.0, 0.16, -0.075)).length() < 1e-5,
            "{origin}"
        );
        // The map's up axis tips forward 45 degrees, scaled to 0.75.
        let up = m.transform_vector3(Vec3::Y);
        assert!((up.length() - 0.75).abs() < 1e-5);
        assert!(
            (up.z + 0.75 * 45f32.to_radians().sin()).abs() < 1e-5,
            "{up}"
        );
    }

    #[test]
    fn a_use_swing_pushes_the_hand_ahead_and_an_interact_swing_turns_it() {
        // progress 0.25: sin(0.5 pi) = 1, so -(1 + 1) * 0.1 along Z.
        let push = swing(0.25, SwingType::Use, true).transform_point3(Vec3::ZERO);
        assert!((push - Vec3::new(0.0, 0.0, -0.2)).length() < 1e-5, "{push}");
        // The right hand turns by -45 degrees about Z at the wave's peak.
        let turned = swing(1.0 / 6.0, SwingType::Interact, true).transform_vector3(Vec3::X);
        assert!(turned.y < -0.6, "{turned}");
    }

    #[test]
    fn a_trident_tips_up_by_the_gun_angle_unless_it_charges() {
        let rest = item_transform(Held::Trident, true, HeldUse::default(), 40.0);
        let charging = item_transform(
            Held::Trident,
            true,
            HeldUse {
                using: true,
                ticks_used: 5.0,
                ..HeldUse::default()
            },
            40.0,
        );
        let at = |m: Mat4| m.transform_point3(Vec3::ZERO);
        // Resting it rises by 0.2 * gun / 40; charging it is drawn back by
        // progress / 10 * 0.25 (0.125 at 5 ticks) from -0.175.
        assert!((at(rest).y - at(charging).y - 0.2).abs() < 1e-4);
        assert!((at(rest).z + 0.175).abs() < 1e-4, "{}", at(rest));
        assert!((at(charging).z + 0.05).abs() < 1e-4, "{}", at(charging));
    }
}
