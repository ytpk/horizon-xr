// SPDX-License-Identifier: LGPL-3.0-or-later
//! The VR player model's joint math: `VRPlayerModel.animateVRModel` (the vanilla-shaped model
//! with connected arms, `playerModelType` VANILLA, which is Vivecraft's default) and the
//! helpers of `ModelUtils` it uses. A model part is a position (in 1/16 blocks, as
//! `ModelPart.x/y/z`), an Euler rotation applied Z, then Y, then X, and a scale.
//!
//! Not ported: the split-arm and split-leg models (extra hand and foot meshes), full-body
//! tracking (a player sending FBT is shown as arms-only) and the first-person GUI anchors of
//! `positionGUI`.
//! Copyright (c) Vivecraft contributors; LGPL-3.0-or-later (see LICENSE-LGPL-3.0).

use crate::client::RemoteVrPlayer;
use crate::math::{DOWN, RIGHT, estimate_body_yaw_rad};
use crate::network::{FbtMode, VrPlayerState};
use glam::{EulerRot, Mat3, Quat, Vec3};
use std::f32::consts::{FRAC_PI_2, PI};

/// One `ModelPart`.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Part {
    pub pos: Vec3,
    /// x, y, z rotation (radians).
    pub rot: Vec3,
    pub scale: Vec3,
}

impl Part {
    /// From `[x, y, z, xrot, yrot, zrot, xscale, yscale, zscale]`.
    pub fn from_array(a: [f32; 9]) -> Self {
        Self {
            pos: Vec3::new(a[0], a[1], a[2]),
            rot: Vec3::new(a[3], a[4], a[5]),
            scale: Vec3::new(a[6], a[7], a[8]),
        }
    }
    pub fn to_array(self) -> [f32; 9] {
        [
            self.pos.x,
            self.pos.y,
            self.pos.z,
            self.rot.x,
            self.rot.y,
            self.rot.z,
            self.scale.x,
            self.scale.y,
            self.scale.z,
        ]
    }
}

/// The parts of `PlayerModel` the VR model moves.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Parts {
    pub head: Part,
    pub body: Part,
    pub left_arm: Part,
    pub right_arm: Part,
    pub left_leg: Part,
    pub right_leg: Part,
}

/// `ClientVRPlayers.RotInfo`, the fields the arms-only model reads.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RotInfo {
    pub seated: bool,
    pub left_handed: bool,
    pub head_pos: Vec3,
    pub head_quat: Quat,
    pub main_hand_pos: Vec3,
    pub main_hand_quat: Quat,
    pub off_hand_pos: Vec3,
    pub off_hand_quat: Quat,
    pub world_scale: f32,
    pub height_scale: f32,
    /// A Visor player: the body faces where the head does (`RemotePlayerPose.calcBodyYaw` is
    /// `hmd.getYaw()`), not Vivecraft's estimate from the head and both hands.
    pub head_yaw_body: bool,
    /// A Visor player's gun angle (`OTHER_GUN_ANGLE`, degrees): the turn about X between the
    /// controller's aim and its grip, which turns the items it holds (`RemotePlayerPose`,
    /// `VRHandItemPose.getAimToGripRotation`). `None` for a Vivecraft player.
    pub gun_angle: Option<f32>,
}

impl RotInfo {
    pub fn of(player: &RemoteVrPlayer) -> Self {
        let s: &VrPlayerState = &player.state;
        Self {
            seated: s.seated,
            left_handed: s.left_handed,
            head_pos: s.hmd.position,
            head_quat: s.hmd.orientation,
            main_hand_pos: s.main_hand.position,
            main_hand_quat: s.main_hand.orientation,
            off_hand_pos: s.off_hand.position,
            off_hand_quat: s.off_hand.orientation,
            world_scale: player.world_scale,
            height_scale: player.height_scale,
            head_yaw_body: false,
            gun_angle: None,
        }
    }

    /// The pose between two received ones (`ClientVRPlayers.getRotationsForPlayer`).
    pub fn lerp(&self, to: &RotInfo, t: f32) -> RotInfo {
        RotInfo {
            seated: to.seated,
            left_handed: to.left_handed,
            head_pos: self.head_pos.lerp(to.head_pos, t),
            head_quat: self.head_quat.slerp(to.head_quat, t),
            main_hand_pos: self.main_hand_pos.lerp(to.main_hand_pos, t),
            main_hand_quat: self.main_hand_quat.slerp(to.main_hand_quat, t),
            off_hand_pos: self.off_hand_pos.lerp(to.off_hand_pos, t),
            off_hand_quat: self.off_hand_quat.slerp(to.off_hand_quat, t),
            world_scale: self.world_scale + (to.world_scale - self.world_scale) * t,
            height_scale: self.height_scale + (to.height_scale - self.height_scale) * t,
            head_yaw_body: to.head_yaw_body,
            gun_angle: match (self.gun_angle, to.gun_angle) {
                (Some(a), Some(b)) => Some(a + (b - a) * t),
                (_, b) => b,
            },
        }
    }

    /// `RotInfo.getBodyYawRad`: where the body faces, in the world's yaw.
    pub fn body_yaw_rad(&self) -> f32 {
        if self.head_yaw_body {
            return crate::math::head_yaw_rad(self.head_quat);
        }
        // Untracked hands sit on the same spot.
        let hands = (self.off_hand_pos.distance_squared(self.main_hand_pos) != 0.0)
            .then_some((self.main_hand_pos, self.off_hand_pos));
        estimate_body_yaw_rad(
            self.seated,
            self.left_handed,
            FbtMode::ArmsOnly,
            crate::math::direction(self.head_quat),
            hands.map(|h| h.0),
            hands.map(|h| h.1),
            || Vec3::ZERO,
        )
    }
}

/// The settings the model reads (`VRSettings.playerLimbsConnected` and the rest).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BodyOptions {
    pub limbs_connected: bool,
    pub limbs_limit: bool,
    pub arm_anim: bool,
    pub apply_world_scale: bool,
}

impl Default for BodyOptions {
    /// Vivecraft's defaults.
    fn default() -> Self {
        Self {
            limbs_connected: true,
            limbs_limit: false,
            arm_anim: true,
            apply_world_scale: false,
        }
    }
}

/// A VR player's pose as the last two ticks sent it, for drawing between them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VrBody {
    pub prev: RotInfo,
    pub cur: RotInfo,
    pub options: BodyOptions,
}

impl VrBody {
    /// A body that has just appeared.
    pub fn new(rot: RotInfo, options: BodyOptions) -> Self {
        Self {
            prev: rot,
            cur: rot,
            options,
        }
    }
    /// A new tick's pose.
    pub fn push(&mut self, rot: RotInfo, options: BodyOptions) {
        self.prev = self.cur;
        self.cur = rot;
        self.options = options;
    }
    /// The pose `partial` of the way from the last tick to this.
    pub fn rot(&self, partial: f32) -> RotInfo {
        self.prev.lerp(&self.cur, partial)
    }
    /// The yaw (degrees, the world's) the entity is turned by.
    pub fn body_yaw_degrees(&self, partial: f32) -> f32 {
        self.rot(partial).body_yaw_rad().to_degrees()
    }
}

/// How the swing of the player's arm moves the hand (`VRFirstPersonArmSwing`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SwingKind {
    #[default]
    Attack,
    Use,
    Interact,
}

/// What `animateVRModel` reads of the render state and the settings.
#[derive(Clone, Copy, Debug)]
pub struct Input {
    pub rot: RotInfo,
    pub slim: bool,
    pub crouching: bool,
    pub passenger: bool,
    pub auto_spin_attack: bool,
    /// `renderState.swimAmount`, or 1 when gliding.
    pub lay_amount: f32,
    /// Lying in the water, or gliding.
    pub swimming: bool,
    /// Head pitch (degrees).
    pub head_pitch: f32,
    /// `renderState.attackTime` 0..1.
    pub attack_time: f32,
    /// The arm the swing moves is the player's main hand.
    pub attack_is_main: bool,
    pub is_main_player: bool,
    pub body_scale: f32,
    pub arm_scale: f32,
    pub leg_scale: f32,
    pub swing: SwingKind,
    pub limbs_connected: bool,
    pub limbs_limit: bool,
    pub arm_anim: bool,
    pub apply_world_scale: bool,
}

/// `ModelUtils.getBendProgress`: how far the player bends over, 0..1.
pub fn bend_progress(
    spin: bool,
    crouching: bool,
    passenger: bool,
    rot: &RotInfo,
    head_pivot: Vec3,
) -> f32 {
    if spin {
        return 0.0;
    }
    // The default eye height, 1.42, and the neck offset.
    let eye_height = 1.42 * rot.world_scale;
    let height_offset = (head_pivot.y - eye_height * rot.height_scale).clamp(-eye_height, 0.0);
    let mut progress = height_offset / -eye_height;
    if crouching {
        progress = progress.max(0.125);
    }
    if passenger {
        progress = progress.min(0.5);
    }
    progress
}

/// `ModelUtils.worldToModel`: a point in player local world space in the model's space.
pub fn world_to_model(
    point: Vec3,
    rot: &RotInfo,
    body_yaw: f32,
    spin: bool,
    use_world_scale: bool,
) -> Vec3 {
    let mut out = point;
    if spin {
        out.y += 1.0;
    }
    // The entity scale is 1 here: the world scale includes it either way.
    if use_world_scale {
        out /= rot.world_scale;
    }
    let scale = 0.9375 * rot.height_scale;
    out -= Vec3::new(0.0, 1.501 * scale, 0.0);
    out = Quat::from_rotation_y(-PI + body_yaw) * out;
    out *= 16.0 / scale;
    out * Vec3::new(-1.0, -1.0, 1.0)
}

/// `ModelUtils.worldToModelDirection`.
pub fn world_to_model_direction(dir: Vec3, body_yaw: f32) -> Vec3 {
    let d = Quat::from_rotation_y(-PI + body_yaw) * dir;
    Vec3::new(-d.x, -d.y, d.z)
}

/// `ModelUtils.pointAtModel`: JOML's `setLookAlong(-dir.x, -dir.y, dir.z, -up.x, -up.y, up.z)`,
/// transposed, then turned a quarter about X (model parts hang down).
pub fn point_at_model(dir: Vec3, up: Vec3) -> Mat3 {
    let d = Vec3::new(dir.x, dir.y, -dir.z).normalize();
    let u = Vec3::new(-up.x, -up.y, up.z);
    let left = u.cross(d).normalize();
    let upn = d.cross(left);
    Mat3::from_cols(left, upn, d) * Mat3::from_rotation_x(FRAC_PI_2)
}

/// `ModelUtils.pointAtModelWithLocal`: point at `dir` with the up of the target rotation.
pub fn point_at_model_with_local(target_rot: Quat, body_yaw: f32, dir: Vec3) -> Mat3 {
    let up = world_to_model_direction(target_rot * RIGHT, body_yaw);
    let up = dir.cross(up);
    point_at_model(dir, up)
}

/// `ModelUtils.pointModelAtModelForward`.
fn point_model_at_model_forward(part: &Part, target: Vec3) -> Mat3 {
    let dir = target - part.pos;
    let up = dir.cross(crate::math::LEFT);
    point_at_model(dir, up)
}

/// `ModelUtils.setRotation`: a world rotation as a model part's.
pub fn set_rotation(part: &mut Part, m: Mat3) {
    let (z, y, x) = m.to_euler(EulerRot::ZYX);
    part.rot = Vec3::new(-x, if y.is_nan() { 0.0 } else { -y }, z);
}

/// `ModelUtils.swingAnimation` (without the pivot): the swing's rotation on `m` and the hand's
/// offset.
pub fn swing_animation(
    is_right: bool,
    attack_time: f32,
    is_main_player: bool,
    kind: SwingKind,
    m: &mut Mat3,
) -> Vec3 {
    let mut offset = Vec3::ZERO;
    if attack_time > 0.0 {
        let wave = if attack_time > 0.5 {
            (attack_time * PI + PI).sin()
        } else {
            (attack_time * 3.0 * PI).sin()
        };
        if !is_main_player || kind == SwingKind::Attack {
            *m *= Mat3::from_rotation_x(wave * 30.0f32.to_radians());
        } else {
            match kind {
                SwingKind::Use => {
                    let movement = if attack_time > 0.25 {
                        (attack_time * FRAC_PI_2 + PI).sin()
                    } else {
                        (attack_time * 2.0 * PI).sin()
                    };
                    offset = *m * DOWN * ((1.0 + movement) * 1.6);
                }
                SwingKind::Interact => {
                    let angle = if is_right { -40.0f32 } else { 40.0 };
                    *m *= Mat3::from_rotation_y(angle.to_radians() * wave);
                }
                SwingKind::Attack => {}
            }
        }
    }
    offset
}

/// `ModelUtils.applySwimRotationOffset`.
fn apply_swim_rotation_offset(input: &Input, x_rot: f32, parts: &mut [&mut Part]) {
    let (sin, cos) = x_rot.sin_cos();
    let rotate_x = |v: Vec3| Vec3::new(v.x, v.y * cos - v.z * sin, v.y * sin + v.z * cos);
    let global = if input.swimming && !input.auto_spin_attack {
        let mut v = rotate_x(Vec3::new(0.0, 17.06125, 5.125));
        v.y += 2.0;
        v
    } else {
        Vec3::ZERO
    };
    for part in parts {
        let mut v = part.pos - global;
        v.y -= 24.0;
        v = rotate_x(v);
        v.y += 24.0;
        part.pos = v;
    }
}

/// `VRPlayerModel.positionConnectedArm`.
fn position_connected_arm(
    arm: &mut Part,
    arm_pos: Vec3,
    arm_rot: Quat,
    input: &Input,
    body_yaw: f32,
    x_rot: f32,
    z_rot_offset: f32,
    apply_attack: bool,
    is_right: bool,
) {
    let rot = &input.rot;
    let mut dir = world_to_model(
        arm_pos,
        rot,
        body_yaw,
        input.auto_spin_attack,
        input.is_main_player || input.apply_world_scale,
    );
    dir -= arm.pos;
    // The shoulders move up with the arms up: the pivot is slightly off.
    arm.pos.y -= 2.0 * (-dir.y / dir.length()).max(0.0);
    let mut m = point_at_model_with_local(arm_rot, body_yaw, dir);
    let controller_dist = dir.length();
    if !input.limbs_limit && controller_dist > 10.0 {
        arm.pos += dir.normalize() * (controller_dist - 10.0);
        m *= Mat3::from_rotation_z(-z_rot_offset);
    } else {
        m *= Mat3::from_rotation_z(-z_rot_offset * (10.0 / controller_dist).min(1.0));
    }
    if input.arm_anim && apply_attack {
        let off = swing_animation(
            is_right,
            input.attack_time,
            input.is_main_player,
            input.swing,
            &mut m,
        );
        arm.pos.x -= off.x;
        arm.pos.y -= off.y;
        arm.pos.z += off.z;
    }
    m = Mat3::from_rotation_x(-x_rot) * m;
    set_rotation(arm, m);
}

/// `VRPlayerModel.animateVRModel` for the arms-only, vanilla-shaped model: moves `parts` (the
/// vanilla pose already applied) to where the player's head and hands are. Returns the body
/// yaw (radians) the entity is turned by.
pub fn animate(parts: &mut Parts, input: &Input) -> f32 {
    let rot = input.rot;
    let body_yaw = rot.body_yaw_rad();
    let laying = input.lay_amount > 0.0;
    let x_rot = if input.swimming {
        input.lay_amount * (-FRAC_PI_2 - input.head_pitch.to_radians())
    } else {
        input.lay_amount * -FRAC_PI_2
    };
    let side_offset = 4.0 * input.body_scale + input.arm_scale;

    // The head pivot.
    let mut pivot = if input.swimming {
        Vec3::ZERO
    } else {
        rot.head_quat * Vec3::new(0.0, -0.2, 0.1) * (rot.height_scale * rot.world_scale)
    };
    pivot += rot.head_pos;
    let progress = bend_progress(
        input.auto_spin_attack,
        input.crouching && !input.swimming,
        input.passenger,
        &rot,
        pivot,
    );
    let height_offset = 22.0 * progress;

    // Rotate the head.
    let m = Mat3::from_rotation_x(-x_rot)
        * Mat3::from_rotation_y(body_yaw + PI)
        * Mat3::from_quat(rot.head_quat);
    set_rotation(&mut parts.head, m);
    let mut head = world_to_model(pivot, &rot, body_yaw, input.auto_spin_attack, true);
    if input.swimming {
        head.z += 3.0;
    }
    parts.head.pos = head;
    parts.body.pos = head;

    // The body: only the arms are tracked, so it turns in place.
    if input.passenger {
        let mut m =
            point_model_at_model_forward(&parts.body, Vec3::new(0.0, 14.0, 2.0 + height_offset));
        m = Mat3::from_rotation_x(-x_rot) * m;
        set_rotation(&mut parts.body, m);
    } else {
        parts.body.rot = Vec3::new(PI * (parts.body.pos.y / 22.0).max(0.0), 0.0, 0.0);
        if laying {
            let body_x_rot = if input.swimming {
                -x_rot
            } else {
                let above_ground = (height_offset - 11.0) / 11.0;
                progress * (PI - FRAC_PI_2 * (1.0 + 0.3 * (1.0 - above_ground)))
            };
            parts.body.rot.x += input.lay_amount * (body_x_rot - parts.body.rot.x);
            parts.head.pos.y -= 2.0 * input.lay_amount;
            parts.body.pos.y -= 2.0 * input.lay_amount;
        }
    }

    // The arms hang from the shoulders, which follow the body's lean.
    let cos_body = parts.body.rot.x.cos();
    parts.left_arm.pos.x = parts.body.pos.x + side_offset;
    parts.right_arm.pos.x = parts.body.pos.x - side_offset;
    parts.left_arm.pos.y = 2.0 * cos_body + parts.body.pos.y;
    parts.left_arm.pos.z = parts.body.pos.z;
    parts.right_arm.pos.y = parts.left_arm.pos.y;
    parts.right_arm.pos.z = parts.left_arm.pos.z;

    // The legs.
    parts.left_leg.pos.x = 1.9;
    parts.right_leg.pos.x = -1.9;
    if input.passenger {
        parts.left_leg.pos.z = height_offset;
        parts.right_leg.pos.z = height_offset;
    } else if laying {
        if input.swimming {
            let v = Vec3::new(0.0, 12.0, 0.0);
            let (sin, cos) = (-x_rot).sin_cos();
            parts.left_leg.pos.y = parts.body.pos.y + (v.y * cos - v.z * sin);
            parts.left_leg.pos.z = parts.body.pos.z + (v.y * sin + v.z * cos);
        } else {
            let cos2 = cos_body * cos_body;
            parts.left_leg.pos.y += 10.25 - 2.0 * cos2;
            parts.left_leg.pos.z = parts.body.pos.z + 13.0 - cos2 * 8.0;
        }
        parts.left_leg.pos.x += parts.body.pos.x;
        parts.right_leg.pos.x += parts.body.pos.x;
        parts.right_leg.pos.y = parts.left_leg.pos.y;
        parts.right_leg.pos.z = parts.left_leg.pos.z;
    } else {
        parts.left_leg.pos.x += parts.body.pos.x;
        parts.right_leg.pos.x += parts.body.pos.x;
    }
    if !input.passenger && input.lay_amount < 1.0 {
        // The legs move back with the bend.
        let new_y = 12.0 + parts.body.pos.y.min(0.0);
        let new_z = parts.body.pos.z + 10.0 * parts.body.rot.x.sin();
        let a = input.lay_amount;
        parts.left_leg.pos.y = new_y + a * (parts.left_leg.pos.y - new_y);
        parts.left_leg.pos.z = new_z + a * (parts.left_leg.pos.z - new_z);
        parts.right_leg.pos.y = parts.left_leg.pos.y;
        parts.right_leg.pos.z = parts.left_leg.pos.z;
    }

    // The arms reach the hands (untracked hands sit on one spot: the vanilla arms stay).
    if (!rot.seated || input.is_main_player)
        && rot.off_hand_pos.distance_squared(rot.main_hand_pos) > 0.0
    {
        let left_handed = rot.left_handed;
        let main_is_left = left_handed;
        let (mut main, mut off) = if left_handed {
            (parts.left_arm, parts.right_arm)
        } else {
            (parts.right_arm, parts.left_arm)
        };
        if input.limbs_connected {
            // The rotation point is not in the arm's centre: turn a little (half or one pixel at
            // full distance) so the hand lines up.
            let offset = (if left_handed { -1.0 } else { 1.0 })
                * (if input.slim { 0.016 } else { 0.032 })
                * PI
                * input.arm_scale;
            position_connected_arm(
                &mut main,
                rot.main_hand_pos,
                rot.main_hand_quat,
                input,
                body_yaw,
                x_rot,
                offset,
                input.attack_is_main,
                !main_is_left,
            );
            position_connected_arm(
                &mut off,
                rot.off_hand_pos,
                rot.off_hand_quat,
                input,
                body_yaw,
                x_rot,
                -offset,
                !input.attack_is_main,
                main_is_left,
            );
        } else {
            // Floating arms: placed at the hand, 10 along the hand's own axis.
            let x_offset =
                (if input.slim { 0.5 } else { 1.0 }) * (if left_handed { -1.0 } else { 1.0 });
            for (arm, pos, quat, dx, applies, right) in [
                (
                    &mut main,
                    rot.main_hand_pos,
                    rot.main_hand_quat,
                    -x_offset,
                    input.attack_is_main,
                    !main_is_left,
                ),
                (
                    &mut off,
                    rot.off_hand_pos,
                    rot.off_hand_quat,
                    x_offset,
                    !input.attack_is_main,
                    main_is_left,
                ),
            ] {
                let at = world_to_model(
                    pos,
                    &rot,
                    body_yaw,
                    input.auto_spin_attack,
                    input.is_main_player || input.apply_world_scale,
                );
                let mut m = Mat3::from_quat(quat);
                m = Mat3::from_rotation_y(body_yaw + PI) * m * Mat3::from_rotation_x(FRAC_PI_2);
                let v = m * Vec3::new(dx * input.arm_scale, 10.0 * input.arm_scale, 0.0);
                arm.pos = at - Vec3::new(v.x, v.y, -v.z);
                if input.arm_anim && applies {
                    // The pivot of the swing is 2 below the arm's top.
                    let before = m * Vec3::new(0.0, 2.0 * input.arm_scale, 0.0);
                    let off = swing_animation(
                        right,
                        input.attack_time,
                        input.is_main_player,
                        input.swing,
                        &mut m,
                    );
                    let after = m * Vec3::new(0.0, 2.0 * input.arm_scale, 0.0);
                    arm.pos.x -= off.x;
                    arm.pos.y -= off.y;
                    arm.pos.z += off.z;
                    arm.pos.x += before.x - after.x;
                    arm.pos.y += before.y - after.y;
                    arm.pos.z -= before.z - after.z;
                }
                m = Mat3::from_rotation_x(-x_rot) * m;
                set_rotation(arm, m);
                arm.scale.y = input.arm_scale;
            }
        }
        if left_handed {
            parts.left_arm = main;
            parts.right_arm = off;
        } else {
            parts.right_arm = main;
            parts.left_arm = off;
        }
    }

    if laying {
        // The body's lean continues into the water.
        parts.body.rot.x += x_rot;
        let mut list = [
            &mut parts.head,
            &mut parts.body,
            &mut parts.left_arm,
            &mut parts.right_arm,
            &mut parts.left_leg,
            &mut parts.right_leg,
        ];
        apply_swim_rotation_offset(input, x_rot, &mut list);
    }

    parts.left_arm.scale.x = input.arm_scale;
    parts.left_arm.scale.z = input.arm_scale;
    parts.right_arm.scale.x = input.arm_scale;
    parts.right_arm.scale.z = input.arm_scale;
    parts.body.scale.x = input.body_scale;
    parts.body.scale.z = input.body_scale;
    parts.left_leg.scale.x = input.leg_scale;
    parts.left_leg.scale.z = input.leg_scale;
    parts.right_leg.scale.x = input.leg_scale;
    parts.right_leg.scale.z = input.leg_scale;

    // The spin attack moves the model one block up.
    if input.auto_spin_attack {
        for p in [
            &mut parts.head,
            &mut parts.body,
            &mut parts.left_arm,
            &mut parts.right_arm,
            &mut parts.left_leg,
            &mut parts.right_leg,
        ] {
            p.pos.y += 24.0;
        }
    }
    body_yaw
}

#[cfg(test)]
mod tests {
    use super::*;

    fn standing() -> (Parts, Input) {
        let rest = |x: f32, y: f32| Part {
            pos: Vec3::new(x, y, 0.0),
            rot: Vec3::ZERO,
            scale: Vec3::ONE,
        };
        let parts = Parts {
            head: rest(0.0, 0.0),
            body: rest(0.0, 0.0),
            left_arm: rest(5.0, 2.0),
            right_arm: rest(-5.0, 2.0),
            left_leg: rest(1.9, 12.0),
            right_leg: rest(-1.9, 12.0),
        };
        // Facing +Z (yaw 0), head at the default eye height, hands in front of the chest.
        let face = Quat::from_rotation_y(PI);
        let rot = RotInfo {
            seated: false,
            left_handed: false,
            head_pos: Vec3::new(0.0, 1.62, 0.0),
            head_quat: face,
            main_hand_pos: Vec3::new(-0.25, 1.1, 0.4),
            main_hand_quat: face,
            off_hand_pos: Vec3::new(0.25, 1.1, 0.4),
            off_hand_quat: face,
            world_scale: 1.0,
            height_scale: 1.0,
            head_yaw_body: false,
            gun_angle: None,
        };
        let input = Input {
            rot,
            slim: false,
            crouching: false,
            passenger: false,
            auto_spin_attack: false,
            lay_amount: 0.0,
            swimming: false,
            head_pitch: 0.0,
            attack_time: 0.0,
            attack_is_main: false,
            is_main_player: false,
            body_scale: 1.0,
            arm_scale: 1.0,
            leg_scale: 1.0,
            swing: SwingKind::Attack,
            limbs_connected: true,
            limbs_limit: false,
            arm_anim: true,
            apply_world_scale: false,
        };
        (parts, input)
    }

    #[test]
    fn a_model_point_goes_to_the_world_and_back() {
        let (_, input) = standing();
        // The model's origin (the neck of a 1.8 m player) is at 1.501 * 0.9375 = 1.407 m.
        let at = world_to_model(
            Vec3::new(0.0, 1.501 * 0.9375, 0.0),
            &input.rot,
            0.0,
            false,
            true,
        );
        assert!(at.length() < 1e-4, "{at}");
        // A point 16 model pixels below the neck is 0.9375 m down.
        let down = world_to_model(
            Vec3::new(0.0, 1.501 * 0.9375 - 0.9375, 0.0),
            &input.rot,
            0.0,
            false,
            true,
        );
        assert!((down - Vec3::new(0.0, 16.0, 0.0)).length() < 1e-3, "{down}");
    }

    #[test]
    fn the_head_sits_at_the_head_and_the_body_under_it() {
        let (mut parts, input) = standing();
        animate(&mut parts, &input);
        // The pivot is 20 cm below and 10 cm behind the eyes: 1.42 m up, close to the neck.
        assert!(parts.head.pos.y.abs() < 1.5, "{:?}", parts.head);
        assert_eq!(parts.head.pos, parts.body.pos);
        // Upright: no bend, legs at the hips.
        assert!(parts.body.rot.x.abs() < 0.2);
        assert!((parts.left_leg.pos.y - 12.0).abs() < 1.0);
    }

    #[test]
    fn the_arms_point_at_the_hands() {
        let (mut parts, input) = standing();
        animate(&mut parts, &input);
        // The right arm's hand end is 12 pixels along its length (-Y of the part, rotated):
        // where the tracked hand is, in model space, up to the arm's 10 pixel reach.
        let hand = world_to_model(
            input.rot.main_hand_pos,
            &input.rot,
            input.rot.body_yaw_rad(),
            false,
            true,
        );
        let m = Mat3::from_euler(
            EulerRot::ZYX,
            parts.right_arm.rot.z,
            parts.right_arm.rot.y,
            parts.right_arm.rot.x,
        );
        let tip = parts.right_arm.pos + m * Vec3::new(0.0, 10.0, 0.0);
        assert!((tip - hand).length() < 4.0, "tip {tip} hand {hand}");
        assert_eq!(parts.right_arm.scale.x, 1.0);
    }

    #[test]
    fn an_untracked_pair_of_hands_leaves_the_vanilla_arms() {
        let (mut parts, mut input) = standing();
        input.rot.off_hand_pos = input.rot.main_hand_pos;
        let before = (parts.left_arm.rot, parts.right_arm.rot);
        animate(&mut parts, &input);
        assert_eq!((parts.left_arm.rot, parts.right_arm.rot), before);
    }

    #[test]
    fn a_visor_body_faces_where_the_head_does_whatever_the_hands_do() {
        let (_, mut input) = standing();
        // The head turned 90 degrees (Minecraft yaw 90 faces -X); the hands stay in front.
        input.rot.head_quat = Quat::from_rotation_y(PI - FRAC_PI_2);
        input.rot.head_yaw_body = true;
        let yaw = input.rot.body_yaw_rad();
        assert!((yaw - FRAC_PI_2).abs() < 1e-4, "{yaw}");
        input.rot.head_yaw_body = false;
        assert!((input.rot.body_yaw_rad() - FRAC_PI_2).abs() > 0.05);
    }

    #[test]
    fn body_yaw_follows_where_the_head_and_hands_face() {
        let (_, input) = standing();
        let yaw = input.rot.body_yaw_rad();
        assert!(yaw.abs() < 0.3, "{yaw}");
    }

    #[test]
    fn crouching_bends_the_body_and_a_seat_lowers_the_legs() {
        let (mut standing_parts, mut input) = standing();
        input.rot.head_pos.y = 1.0;
        animate(&mut standing_parts, &input);
        assert!(
            standing_parts.body.rot.x > 0.5,
            "{:?}",
            standing_parts.body.rot
        );
        let (mut seated, mut input) = standing();
        input.rot.head_pos.y = 1.0;
        input.passenger = true;
        animate(&mut seated, &input);
        assert!(seated.left_leg.pos.z > 5.0, "{:?}", seated.left_leg.pos);
    }

    #[test]
    fn an_interact_swing_turns_the_hand_a_use_swing_pushes_it() {
        let mut m = Mat3::IDENTITY;
        let off = swing_animation(true, 1.0 / 6.0, true, SwingKind::Interact, &mut m);
        assert_eq!(off, Vec3::ZERO);
        assert!(m != Mat3::IDENTITY);
        let mut m = Mat3::IDENTITY;
        let off = swing_animation(true, 0.25, true, SwingKind::Use, &mut m);
        assert!((off - Vec3::new(0.0, -3.2, 0.0)).length() < 1e-4, "{off}");
    }
}
