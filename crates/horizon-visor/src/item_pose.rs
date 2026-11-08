// SPDX-License-Identifier: MPL-2.0
// Ports Visor: core/client/render/decoration/hand/VRItemPoseDefault.java (computeParams, getTransformType, spearMotion), VRHandRenderer.renderWorldArmWithItem (the chain before the pose).
//! How Visor holds each kind of item in a hand (`VRItemPoseDefault`): the item's class picks a
//! [`TransformType`], and each type has its own turn, shift and size, authored on a Quest 3S's aim
//! frame and re-anchored onto the grip by the controller's aim-to-grip turn. Copyright (C)
//! Phoenix-Ra and Visor contributors; MPL-2.0 (see LICENSE-MPL-2.0).
//!
//! The matrix a hand's item gets is `swing * hand_base() * pose.matrix()`, drawn with the third
//! person hand display transform (`ItemDisplayContext.THIRD_PERSON_*_HAND`).

use crate::body::DEFAULT_GUN_ANGLE;
use glam::{Mat4, Quat, Vec3};

/// `VRItemPoseDefault.TransformType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransformType {
    Default,
    Block3d,
    BlockStick,
    BlockItem,
    Shield,
    Sword,
    Mace,
    Tool,
    FishingRod,
    Bow,
    Trident,
    Spear,
    Map,
    Consumable,
    Crossbow,
    Compass,
    Horn,
    Stick,
    Torch,
}

/// What `getTransformType` asks of an item.
#[derive(Clone, Copy, Debug, Default)]
pub struct ItemFacts<'a> {
    /// The item's id, `minecraft:bone`.
    pub id: &'a str,
    /// Its use animation is EAT or DRINK.
    pub eats_or_drinks: bool,
    /// Its use animation is TOOT_HORN.
    pub toots: bool,
    /// A shield (`blocks_attacks`).
    pub shield: bool,
    /// A spear (`kinetic_weapon`).
    pub spear: bool,
    /// A digger tool (`tool` component) or a rod, flint and steel, a brush.
    pub tool: bool,
    /// What a block item places: whether its block is a torch, and whether its model is a 3D block
    /// (lit as one).
    pub block: Option<BlockFacts>,
    pub map: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct BlockFacts {
    pub torch_block: bool,
    pub gui_3d: bool,
}

fn name(id: &str) -> &str {
    id.strip_prefix("minecraft:").unwrap_or(id)
}

/// `isStick`.
fn is_stick(id: &str) -> bool {
    matches!(
        name(id),
        "debug_stick" | "bone" | "blaze_rod" | "bamboo" | "stick"
    )
}

/// `isTorch`.
fn is_torch(id: &str) -> bool {
    matches!(name(id), "torch" | "soul_torch" | "redstone_torch")
}

/// `getTransformType`: the first that applies.
pub fn transform_type(f: &ItemFacts) -> TransformType {
    let n = name(f.id);
    if f.eats_or_drinks {
        return TransformType::Consumable;
    }
    if f.shield {
        return TransformType::Shield;
    }
    if f.spear {
        return TransformType::Spear;
    }
    // `isTool`: digger tools, rods, flint and steel, brushes (and the stick, bone... are
    // `isStick`, which comes after).
    if f.tool
        || matches!(
            n,
            "fishing_rod"
                | "carrot_on_a_stick"
                | "warped_fungus_on_a_stick"
                | "flint_and_steel"
                | "brush"
        )
    {
        return if matches!(
            n,
            "fishing_rod" | "carrot_on_a_stick" | "warped_fungus_on_a_stick"
        ) {
            TransformType::FishingRod
        } else {
            TransformType::Tool
        };
    }
    if is_stick(f.id) {
        return TransformType::Stick;
    }
    if is_torch(f.id) {
        return TransformType::Torch;
    }
    if let Some(block) = f.block {
        return if block.torch_block {
            TransformType::BlockStick
        } else if block.gui_3d {
            TransformType::Block3d
        } else {
            TransformType::BlockItem
        };
    }
    if f.map {
        return TransformType::Map;
    }
    if n == "bow" {
        return TransformType::Bow;
    }
    if f.toots {
        return TransformType::Horn;
    }
    if n == "mace" {
        return TransformType::Mace;
    }
    if n.ends_with("_sword") {
        return TransformType::Sword;
    }
    if n == "trident" {
        return TransformType::Trident;
    }
    if n == "crossbow" {
        return TransformType::Crossbow;
    }
    if n == "compass" || n == "clock" {
        return TransformType::Compass;
    }
    TransformType::Default
}

/// What the player is doing with the item in this hand.
#[derive(Clone, Copy, Debug, Default)]
pub struct PoseState {
    /// `player.isUsingItem() && getUsedItemHand() == hand`.
    pub using: bool,
    /// `getUseItemRemainingTicks()`.
    pub remaining_ticks: i32,
    /// A trident: the ticks it has been charged (`useDuration - (remaining - partial + 1)`).
    pub charge: f32,
    /// A trident's riptide level and whether the player is in water or rain.
    pub riptide: f32,
    pub wet: bool,
    /// `isAutoSpinAttack()`.
    pub spinning: bool,
    /// `VisorState.TICK_COUNT`, `Util.getMillis()`.
    pub ticks: u64,
    pub millis: u64,
    /// A spear: the stab swing's progress (0 when not stabbing) and the kinetic motion.
    pub stab: f32,
    pub spear: SpearMotion,
    /// A bed (the tag `beds`) or a banner, for a 3D block.
    pub bed: bool,
    pub banner: bool,
}

/// `SpearMotion(slide, dip, sway)`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SpearMotion {
    pub slide: f32,
    pub dip: f32,
    pub sway: f32,
}

fn clamp01(x: f32) -> f32 {
    x.clamp(0.0, 1.0)
}

/// `progress(time, start, end)`.
fn progress(time: f32, start: f32, end: f32) -> f32 {
    if end <= start {
        return if time >= end { 1.0 } else { 0.0 };
    }
    clamp01((time - start) / (end - start))
}

fn in_out_sine(x: f32) -> f32 {
    -((std::f32::consts::PI * x).cos() - 1.0) / 2.0
}
fn out_back(x: f32) -> f32 {
    let (c1, c3) = (1.70158_f32, 2.70158_f32);
    1.0 + c3 * (x - 1.0).powi(3) + c1 * (x - 1.0).powi(2)
}
fn in_out_expo(x: f32) -> f32 {
    if x <= 0.0 {
        0.0
    } else if x >= 1.0 {
        1.0
    } else if x < 0.5 {
        2f32.powf(20.0 * x - 10.0) / 2.0
    } else {
        (2.0 - 2f32.powf(-20.0 * x + 10.0)) / 2.0
    }
}

/// The spear's stab swing (`spearMotion`'s first half): `attack` is the swing's progress, `reach`
/// the weapon's `forwardMovement` (0.38 without a `kinetic_weapon`).
pub fn stab_motion(attack: f32, reach: f32) -> SpearMotion {
    let wind_up = in_out_sine(progress(attack, 0.0, 0.05));
    let thrust = out_back(progress(attack, 0.05, 0.2));
    let retract = in_out_expo(progress(attack, 0.4, 1.0));
    SpearMotion {
        slide: reach * (thrust - retract) - 0.08 * (wind_up - thrust),
        dip: 0.0,
        sway: 0.0,
    }
}

/// `SPEAR_GRIP`.
const SPEAR_GRIP: Vec3 = Vec3::new(0.0, -0.2743, 0.121);

/// `PoseParams`: what `computeParams` makes.
#[derive(Clone, Copy, Debug)]
pub struct PoseParams {
    pub pre_rotation: Quat,
    pub rotation: Quat,
    pub offset: Vec3,
    pub scale: f32,
}

impl PoseParams {
    /// `applyPose`: `rotate(pre)`, `translate`, `rotate(rotation)`, `scale`.
    pub fn matrix(&self) -> Mat4 {
        Mat4::from_quat(self.pre_rotation)
            * Mat4::from_translation(self.offset)
            * Mat4::from_quat(self.rotation)
            * Mat4::from_scale(Vec3::splat(self.scale))
    }
}

fn xp(deg: f32) -> Quat {
    Quat::from_rotation_x(deg.to_radians())
}
fn yp(deg: f32) -> Quat {
    Quat::from_rotation_y(deg.to_radians())
}
fn zp(deg: f32) -> Quat {
    Quat::from_rotation_z(deg.to_radians())
}

/// `XrRawPoseHandler.updateAimToGrip`: the controller's turn from its aim onto its grip, `aim⁻¹ *
/// grip`. A runtime that gives the aim pose as the grip too (the two within 10 degrees) has the
/// default, a turn of `DEFAULT_GUN_ANGLE` about X. The item poses are authored for a Quest 3S's
/// aim frame and re-anchored by this turn, so it is the controller's own, never the identity.
pub fn aim_to_grip(aim: Quat, grip: Quat) -> Quat {
    let delta = (aim.inverse() * grip).normalize();
    let angle = 2.0 * delta.w.abs().min(1.0).acos();
    if angle < 10.0_f32.to_radians() {
        xp(DEFAULT_GUN_ANGLE)
    } else {
        delta
    }
}

/// `computeParams`. `hand_dir` is +1 for the main hand, -1 for the off hand; `aim_to_grip` the
/// controller's turn from its aim onto its grip (`aim_to_grip` above; the sender's gun angle
/// about X for another player).
pub fn compute(kind: TransformType, hand_dir: f32, aim_to_grip: Quat, s: &PoseState) -> PoseParams {
    let mut scale = 0.8_f32;
    let (mut pre_yaw, mut pre_pitch, pre_roll) = (0.0_f32, 0.0_f32, 0.0_f32);
    let (mut tx, mut ty, mut tz) = (0.0_f32, 0.0_f32, 0.0_f32);
    let (mut yaw, mut pitch, mut roll) = (0.0_f32, 0.0_f32, 0.0_f32);
    match kind {
        TransformType::BlockItem | TransformType::Default => {
            scale = 1.0;
            pre_yaw = -20.0;
            tx = -0.055;
            ty = -0.1;
            tz = -0.2;
            yaw = 0.0;
            pitch = 90.0;
        }
        TransformType::Block3d => {
            scale = 0.7;
            ty = 0.005 - 0.05;
            tz -= 0.13;
            if s.bed {
                yaw = -50.0 + 20.0;
            } else if s.banner {
                scale = 1.4;
                ty = 0.0;
                yaw = 0.0;
                pitch = 180.0;
            } else {
                yaw = -50.0 - 40.0;
            }
        }
        TransformType::Consumable
        | TransformType::Compass
        | TransformType::BlockStick
        | TransformType::Horn => {
            ty = 0.005;
            tz += 0.006 * (s.remaining_ticks as f32).sin() + 0.02;
            roll = 180.0;
            yaw = -135.0;
        }
        TransformType::Tool => {
            // Brush and flint and steel have their own; the rest is the digger's.
            scale = 1.45;
            ty = 0.005 - 0.1;
            tz -= 0.08;
            yaw = -25.0;
        }
        TransformType::Stick => {
            ty = 0.005;
            tz = -0.05;
            yaw = -20.0;
        }
        TransformType::Torch => {
            scale = 1.8;
            pre_yaw = -10.0;
            tx = -0.11;
            tz = 0.08;
            yaw = 0.0;
            pitch = 90.0;
        }
        TransformType::Map => {
            scale = 1.0;
            tx = 0.0;
            ty = 0.16;
            tz = -0.075;
            yaw = -45.0;
        }
        TransformType::FishingRod => {
            scale = 1.45;
            yaw = -50.0;
        }
        TransformType::Crossbow => {
            scale = 0.9;
            tx = hand_dir * -0.065;
            yaw = 0.0;
            pitch = hand_dir * 15.0;
        }
        TransformType::Bow => {
            scale = 0.9;
            tx = hand_dir * 0.075;
            ty = 0.1;
            tz = -0.1;
            yaw = -7.0;
            roll = hand_dir * -10.0;
        }
        TransformType::Sword => {
            scale = 1.3;
            tz -= 0.08;
            ty = 0.005 - 0.04;
            yaw = -25.0;
        }
        TransformType::Mace => {
            scale = 1.1;
            tz -= 0.1;
            ty = 0.005 - 0.04;
            yaw = -25.0;
        }
        TransformType::Shield => {
            scale = 1.0;
            if s.using {
                tx = if hand_dir == 1.0 { -0.25 } else { 0.17 };
                ty = if hand_dir == 1.0 { -0.04 } else { 0.0 };
                yaw = -45.0;
                pitch = hand_dir * 45.0;
            }
        }
        TransformType::Trident => {
            scale = 1.3;
            pre_yaw = 90.0;
            let mut progress = 0.0_f32;
            if s.using && s.remaining_ticks > 0 {
                if s.riptide <= 0.0 || s.wet {
                    progress = s.charge;
                    // `TridentItem.THROW_THRESHOLD_TIME`.
                    if progress > 10.0 {
                        let rotation_progress = progress - 10.0;
                        progress = 10.0;
                        if s.riptide > 0.0 && s.wet {
                            pitch = -rotation_progress * 10.0 * s.riptide;
                        }
                        tx += 0.005 * (s.millis as f32).sin();
                    }
                }
                tx += hand_dir * 0.01;
                ty = 0.005 - 0.55 + progress / 10.0 * 0.25;
            } else if s.spinning && s.riptide > 0.0 {
                pre_yaw = -90.0;
                tx = hand_dir * -0.02;
                ty = 0.005 + 0.75;
                pitch = ((-(s.ticks as f32) * 50.0) % 360.0) - s.charge * 10.0 * s.riptide;
            } else {
                pre_yaw = -30.0;
                tx = hand_dir * -0.02;
                ty = 0.2;
                tz = -0.05;
                pitch = hand_dir * 30.0;
            }
        }
        TransformType::Spear => {
            scale = 1.3;
            yaw = -90.0;
            roll = -90.0;
            let m = s.spear;
            pre_yaw = m.dip;
            pre_pitch = m.sway;
            // The pivot of the grip turned by the dip and sway, minus the grip.
            let turn = (yp(pre_pitch) * xp(pre_yaw)).conjugate();
            let pivot = turn * SPEAR_GRIP - SPEAR_GRIP;
            tx = -0.1625 + pivot.x;
            ty = -0.2743 + pivot.y;
            tz = -0.0773 + pivot.z - m.slide;
        }
    }
    // The brush and flint and steel are tools with poses of their own; `compute_tool` below
    // overrides the digger's when the caller says which.
    let aim_locked = kind == TransformType::Spear;
    if !aim_locked {
        yaw -= DEFAULT_GUN_ANGLE;
    }
    let pre_rotation = zp(pre_roll) * yp(pre_pitch) * xp(pre_yaw);
    let mut rotation = zp(roll) * yp(pitch) * xp(yaw);
    if !aim_locked {
        rotation *= aim_to_grip;
    }
    PoseParams {
        pre_rotation,
        rotation,
        offset: Vec3::new(tx, ty, tz),
        scale,
    }
}

/// The two tools that differ from the digger's (`TOOL` with a `BrushItem` or a
/// `FlintAndSteelItem`).
pub fn compute_tool(item_id: &str, hand_dir: f32, aim_to_grip: Quat, s: &PoseState) -> PoseParams {
    let n = name(item_id);
    let base = compute(TransformType::Tool, hand_dir, aim_to_grip, s);
    let (scale, pre_yaw, offset, yaw, pitch, roll) = match n {
        "brush" => (0.9, 0.0, Vec3::ZERO, -90.0, -40.0, 90.0),
        "flint_and_steel" => (1.0, -15.0, Vec3::new(0.06, 0.0, -0.25), 0.0, -90.0, 0.0),
        _ => return base,
    };
    let yaw = yaw - DEFAULT_GUN_ANGLE;
    PoseParams {
        pre_rotation: xp(pre_yaw),
        rotation: zp(roll) * yp(pitch) * xp(yaw) * aim_to_grip,
        offset,
        scale,
    }
}

/// `renderWorldArmWithItem` up to the pose: the entity scale, the controller to model turn,
/// the arms' scale about the grip, the turn into the item's frame and the shift.
pub fn hand_base(arms_scale: f32) -> Mat4 {
    // `gripY`.
    let grip_y = 0.65;
    Mat4::from_scale(Vec3::splat(0.9375))
        * Mat4::from_quat(xp(-90.0))
        * Mat4::from_quat(yp(180.0))
        * Mat4::from_scale(Vec3::new(arms_scale, 1.0, arms_scale))
        * Mat4::from_translation(Vec3::new(0.0, grip_y, 0.0))
        * Mat4::from_scale(Vec3::new(1.0, arms_scale, 1.0))
        * Mat4::from_translation(Vec3::new(0.0, -grip_y, 0.0))
        * Mat4::from_quat(xp(-90.0))
        * Mat4::from_quat(yp(180.0))
        * Mat4::from_translation(Vec3::new(0.0, 0.125, 0.625))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(id: &str) -> ItemFacts<'_> {
        ItemFacts {
            id,
            ..ItemFacts::default()
        }
    }

    #[test]
    fn the_class_of_an_item_picks_the_type_in_visors_order() {
        use TransformType as T;
        assert_eq!(transform_type(&facts("minecraft:diamond_sword")), T::Sword);
        assert_eq!(transform_type(&facts("minecraft:mace")), T::Mace);
        assert_eq!(transform_type(&facts("minecraft:bow")), T::Bow);
        assert_eq!(transform_type(&facts("minecraft:crossbow")), T::Crossbow);
        assert_eq!(transform_type(&facts("minecraft:trident")), T::Trident);
        assert_eq!(transform_type(&facts("minecraft:compass")), T::Compass);
        assert_eq!(transform_type(&facts("minecraft:clock")), T::Compass);
        // A recovery compass is not a `CompassItem`.
        assert_eq!(
            transform_type(&facts("minecraft:recovery_compass")),
            T::Default
        );
        assert_eq!(transform_type(&facts("minecraft:bone")), T::Stick);
        assert_eq!(transform_type(&facts("minecraft:breeze_rod")), T::Default);
        assert_eq!(transform_type(&facts("minecraft:torch")), T::Torch);
        assert_eq!(
            transform_type(&facts("minecraft:fishing_rod")),
            T::FishingRod
        );
        assert_eq!(transform_type(&facts("minecraft:flint_and_steel")), T::Tool);
        let pick = ItemFacts {
            tool: true,
            ..facts("minecraft:iron_pickaxe")
        };
        assert_eq!(transform_type(&pick), T::Tool);
        // Eating beats everything; a shield beats a tool.
        let apple = ItemFacts {
            eats_or_drinks: true,
            ..facts("minecraft:apple")
        };
        assert_eq!(transform_type(&apple), T::Consumable);
        let shield = ItemFacts {
            shield: true,
            tool: true,
            ..facts("minecraft:shield")
        };
        assert_eq!(transform_type(&shield), T::Shield);
        let block = |torch, gui| ItemFacts {
            block: Some(BlockFacts {
                torch_block: torch,
                gui_3d: gui,
            }),
            ..facts("minecraft:stone")
        };
        assert_eq!(transform_type(&block(false, true)), T::Block3d);
        assert_eq!(transform_type(&block(false, false)), T::BlockItem);
        assert_eq!(transform_type(&block(true, false)), T::BlockStick);
        assert_eq!(transform_type(&facts("minecraft:diamond")), T::Default);
    }

    #[test]
    fn a_sword_sits_at_the_numbers_of_the_java() {
        let p = compute(
            TransformType::Sword,
            1.0,
            Quat::IDENTITY,
            &PoseState::default(),
        );
        assert_eq!(p.scale, 1.3);
        assert!((p.offset - Vec3::new(0.0, 0.005 - 0.04, -0.08)).length() < 1e-6);
        // yaw -25 less the default gun angle 60: a turn of -85 about X, nothing else.
        assert!(p.rotation.dot(xp(-85.0)).abs() > 1.0 - 1e-6);
        assert!(p.pre_rotation.dot(Quat::IDENTITY).abs() > 1.0 - 1e-6);
    }

    #[test]
    fn the_hands_mirror_where_the_java_mirrors_them() {
        let bow = |dir| {
            compute(
                TransformType::Bow,
                dir,
                Quat::IDENTITY,
                &PoseState::default(),
            )
        };
        assert!((bow(1.0).offset.x - 0.075).abs() < 1e-6);
        assert!((bow(-1.0).offset.x + 0.075).abs() < 1e-6);
        let cross = compute(
            TransformType::Crossbow,
            -1.0,
            Quat::IDENTITY,
            &PoseState::default(),
        );
        assert!((cross.offset.x - 0.065).abs() < 1e-6);
        // A raised shield goes the other side in the off hand.
        let up = PoseState {
            using: true,
            ..PoseState::default()
        };
        let main = compute(TransformType::Shield, 1.0, Quat::IDENTITY, &up);
        let off = compute(TransformType::Shield, -1.0, Quat::IDENTITY, &up);
        assert_eq!((main.offset.x, off.offset.x), (-0.25, 0.17));
        // Lowered, it has no pose of its own.
        let down = compute(
            TransformType::Shield,
            1.0,
            Quat::IDENTITY,
            &PoseState::default(),
        );
        assert_eq!(down.offset, Vec3::ZERO);
    }

    #[test]
    fn a_spear_ignores_the_gun_angle_and_slides_along_its_stab() {
        let idle = compute(TransformType::Spear, 1.0, xp(30.0), &PoseState::default());
        // No gun angle and no aim-to-grip turn: yaw -90 and roll -90 only.
        assert!(idle.rotation.dot(zp(-90.0) * xp(-90.0)).abs() > 1.0 - 1e-6);
        assert!((idle.offset - Vec3::new(-0.1625, -0.2743, -0.0773)).length() < 1e-5);
        let stab = PoseState {
            spear: stab_motion(0.2, 0.38),
            ..PoseState::default()
        };
        let thrust = compute(TransformType::Spear, 1.0, Quat::IDENTITY, &stab);
        assert!(thrust.offset.z < idle.offset.z - 0.2, "{}", thrust.offset.z);
        // Back at rest the slide is gone.
        let rest = stab_motion(1.0, 0.38);
        assert!(rest.slide.abs() < 1e-3);
    }

    #[test]
    fn food_shakes_in_the_mouth_with_the_ticks_left() {
        let at = |remaining| {
            compute(
                TransformType::Consumable,
                1.0,
                Quat::IDENTITY,
                &PoseState {
                    remaining_ticks: remaining,
                    ..PoseState::default()
                },
            )
            .offset
            .z
        };
        assert!((at(0) - 0.02).abs() < 1e-6);
        assert!((at(1) - (0.02 + 0.006 * 1f32.sin())).abs() < 1e-6);
    }

    #[test]
    fn the_brush_and_flint_have_poses_of_their_own() {
        let brush = compute_tool(
            "minecraft:brush",
            1.0,
            Quat::IDENTITY,
            &PoseState::default(),
        );
        assert_eq!(brush.scale, 0.9);
        let flint = compute_tool(
            "minecraft:flint_and_steel",
            1.0,
            Quat::IDENTITY,
            &PoseState::default(),
        );
        assert!((flint.offset - Vec3::new(0.06, 0.0, -0.25)).length() < 1e-6);
        let pick = compute_tool(
            "minecraft:iron_pickaxe",
            1.0,
            Quat::IDENTITY,
            &PoseState::default(),
        );
        assert_eq!(pick.scale, 1.45);
    }

    #[test]
    fn a_remote_players_gun_angle_is_the_aim_to_grip_turn_of_the_item() {
        // Visor takes the default gun angle (60) off every item's yaw and a remote sender's own
        // angle is turned back in: a sender at the default holds the sword at the yaw of its table
        // (-25), a sender whose grip equals its aim (0) holds it 60 degrees lower.
        let sword =
            |gun: f32| compute(TransformType::Sword, 1.0, xp(gun), &PoseState::default()).rotation;
        assert!(sword(DEFAULT_GUN_ANGLE).dot(xp(-25.0)).abs() > 1.0 - 1e-6);
        assert!(sword(0.0).dot(xp(-85.0)).abs() > 1.0 - 1e-6);
        assert!(sword(20.0).dot(xp(-65.0)).abs() > 1.0 - 1e-6);
    }

    #[test]
    fn a_runtime_that_gives_the_aim_as_the_grip_gets_the_default_turn() {
        let aim = Quat::from_rotation_y(0.7) * Quat::from_rotation_x(-0.2);
        // The grip is the aim: Visor's fallback, a turn of the default gun angle about X.
        let delta = aim_to_grip(aim, aim);
        assert!(delta.angle_between(xp(60.0)) < 1e-3);
        // A grip 3 degrees off is "the aim as the grip" too.
        assert!(aim_to_grip(aim, aim * xp(3.0)).angle_between(xp(60.0)) < 1e-3);
    }

    #[test]
    fn a_real_grip_is_kept_as_aim_inverse_times_grip_whichever_way_the_sign_goes() {
        let aim = Quat::from_rotation_y(1.1) * Quat::from_rotation_z(0.3);
        // A grip 35 degrees about X from the aim, and one 20 degrees about Y: the turn between
        // them is what comes back, to the last bit, in the aim's own frame.
        for turn in [xp(35.0), yp(20.0), xp(-40.0) * zp(15.0)] {
            let got = aim_to_grip(aim, aim * turn);
            assert!(got.angle_between(turn) < 1e-3, "{turn:?} {got:?}");
            // `q` and `-q` are the same turn.
            assert!(aim_to_grip(aim, -(aim * turn)).angle_between(turn) < 1e-3);
        }
    }

    #[test]
    fn a_sword_is_authored_in_the_aim_frame_and_the_aim_to_grip_turn_is_what_makes_it_stand() {
        // Authored: the sword's pose is `X(-25)` about the item's hilt, on a controller whose grip
        // is turned 60 degrees about X from its aim (the Quest's, and the default). The turn
        // `X(-25 - 60) * X(60)` is that authored turn, whatever the gun angle was taken out for.
        let state = PoseState::default();
        let with_grip = compute(TransformType::Sword, 1.0, xp(60.0), &state);
        assert!(with_grip.rotation.angle_between(xp(-25.0)) < 1e-3);
        // With the identity (what the first person path passed before) it is 60 degrees short of
        // that: the blade lies along the arm.
        let with_none = compute(TransformType::Sword, 1.0, Quat::IDENTITY, &state);
        assert!((with_none.rotation.angle_between(xp(-25.0)).to_degrees() - 60.0).abs() < 1e-3);
        // A grip 40 degrees from the aim holds it 20 degrees away from the authored pose.
        let forty = compute(TransformType::Sword, 1.0, xp(40.0), &state);
        assert!((forty.rotation.angle_between(xp(-25.0)).to_degrees() - 20.0).abs() < 1e-3);
        // A spear is locked to the aim: no turn at all.
        let spear = compute(TransformType::Spear, 1.0, xp(40.0), &state);
        let spear_none = compute(TransformType::Spear, 1.0, Quat::IDENTITY, &state);
        assert!(spear.rotation.angle_between(spear_none.rotation) < 1e-3);
    }
}
