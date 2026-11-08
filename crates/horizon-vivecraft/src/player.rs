// SPDX-License-Identifier: LGPL-3.0-or-later
//! Ports `org.vivecraft.client_vr.gameplay.VRPlayer` (the room origin, the world scale and
//! rotation, the player following the head in the room, the free move direction and the look
//! override) and the world rotation part of `MCVR.processBindings` (snap and smooth turning).
//! Copyright (c) Vivecraft contributors; LGPL-3.0-or-later (see LICENSE-LGPL-3.0).
//!
//! The Java tick structure is kept: `poll` (new tracking data), `pre_tick`, the trackers,
//! `post_tick`, and `pre_render` for the interpolated data the renderer uses. Skipped: the
//! wonder food world scale, the vehicle (horse) body yaw, KAT VR and Infinadeck.

use crate::modes::FreeMove;
use crate::settings::VrSettings;
use crate::vr_data::{DataSettings, RoomPoses, Vec3History, VrData, lerp_rotation, rotate_y};
use glam::{DVec3, Vec3};

/// `AutoCalibration.DEFAULT_HEIGHT`.
pub const DEFAULT_HEIGHT: f32 = 1.52;

/// `AutoCalibration.getPlayerHeight`: the calibrated eye height, or the default.
pub fn player_height(s: &VrSettings) -> f32 {
    let manual = s.float("manualCalibration");
    if !s.seated() && manual != -1.0 {
        manual
    } else {
        DEFAULT_HEIGHT
    }
}

/// The ten sample windows' owner: what `MCVR` keeps between polls.
#[derive(Clone, Debug, Default)]
pub struct Histories {
    /// The head pivot in room space (`hmdPivotHistory`).
    pub hmd_pivot: Vec3History,
    pub hmd: Vec3History,
    /// The controllers' aim positions in room space (`controllerHistory`).
    pub controller: [Vec3History; 2],
}

/// A player standing in the world, as the room walk sees it.
#[derive(Clone, Copy, Debug)]
pub struct WalkingPlayer {
    pub pos: DVec3,
    pub half_width: f64,
    pub height: f64,
    pub shift_down: bool,
    pub sleeping: bool,
    pub alive: bool,
    pub riding: bool,
    /// Falling distance is zero (walking up blocks needs a player on the ground).
    pub fall_distance_zero: bool,
    pub on_climbable: bool,
    pub jumping_tracker: bool,
    pub grabbing_ladder: bool,
    /// `vivecraft$getMuhJumpFactor() == 1`.
    pub jump_factor_one: bool,
    /// Half width of the player's current dimensions, for the climbing shrink.
    pub dimensions_width: f64,
}

/// An axis aligned box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    pub min: DVec3,
    pub max: DVec3,
}

impl Aabb {
    pub fn moved(self, by: DVec3) -> Self {
        Self {
            min: self.min + by,
            max: self.max + by,
        }
    }
}

/// What the room walk asks of the world.
pub trait RoomWalkWorld {
    /// `level.noCollision(player, bb)`.
    fn no_collision(&mut self, bb: Aabb) -> bool;
}

/// What the room walk did to the player.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RoomWalk {
    /// Nothing moved.
    None,
    /// The player's x and z follow the head (`setPosRaw`); y stays or follows the new box.
    Moved { pos: DVec3, bb: Aabb },
    /// A step up: the player and the room origin rise.
    Stepped {
        pos: DVec3,
        bb: Aabb,
        origin_up: f64,
    },
    /// Riding away: the sneak counter starts (roomscale dismount).
    Dismount,
}

/// The part of `doPlayerMoveInRoom` after the gates: the player's feet go to `(x, z)` when
/// nothing is in the way, or up a step (`walkUpBlocks`, `realisticClimbEnabled`). A game
/// whose camera is not the head's calls this with its own target.
pub fn room_walk_to(
    s: &VrSettings,
    player: &WalkingPlayer,
    world: &mut impl RoomWalkWorld,
    can_roomscale_dismount: bool,
    x: f64,
    z: f64,
) -> RoomWalk {
    if s.bool("realisticDismountEnabled") && can_roomscale_dismount {
        return RoomWalk::Dismount;
    }
    let y = player.pos.y;
    let half = player.half_width;
    let bb = Aabb {
        min: DVec3::new(x - half, y, z - half),
        max: DVec3::new(x + half, y + player.height, z + half),
    };
    if world.no_collision(bb) {
        let y_new = if s.bool("simulateFalling") {
            player.pos.y
        } else {
            y
        };
        return RoomWalk::Moved {
            pos: DVec3::new(x, y_new, z),
            bb,
        };
    }
    let climbing = player.grabbing_ladder && s.bool("realisticClimbEnabled");
    if ((s.bool("walkUpBlocks") && player.jump_factor_one) || climbing) && player.fall_distance_zero
    {
        let shrink = player.dimensions_width * 0.45;
        let shrunk = half - shrink;
        let climb_bb = Aabb {
            min: DVec3::new(x - shrunk, bb.min.y, z - shrunk),
            max: DVec3::new(x + shrunk, bb.max.y, z + shrunk),
        };
        if !world.no_collision(climb_bb) {
            let extra = if player.on_climbable && s.bool("realisticClimbEnabled") {
                6
            } else {
                0
            };
            let mut bb = bb;
            for i in 0..=(10 + extra) {
                bb = bb.moved(DVec3::new(0.0, 0.1, 0.0));
                if world.no_collision(bb) {
                    let up = 0.1 * f64::from(i + 1);
                    return RoomWalk::Stepped {
                        pos: DVec3::new(x, bb.min.y, z),
                        bb,
                        origin_up: up,
                    };
                }
            }
        }
    }
    RoomWalk::None
}

/// The interpolated state the renderer draws: `VRPlayer.vrdata_world_*`.
#[derive(Clone, Debug)]
pub struct VrPlayer {
    pub room_origin: DVec3,
    pub world_scale: f32,
    pub room_scale_movement_delay: i32,
    /// `worldRotation`, degrees (Vivecraft keeps it in the settings).
    pub world_rotation: f32,
    pub room_pre: VrData,
    pub world_pre: VrData,
    pub world_post: VrData,
    pub world_render: Option<VrData>,
    pub histories: Histories,
    free_move_current: bool,
    teleport_override: bool,
    data_settings: DataSettings,
    room: RoomPoses,
}

impl VrPlayer {
    pub fn new(s: &VrSettings) -> Self {
        let data_settings = data_settings(s);
        let room = RoomPoses::default();
        let make = |origin, scale, rot| {
            VrData::new(
                &room,
                data_settings,
                origin,
                s.float("walkMultiplier"),
                scale,
                rot,
            )
        };
        let scale = s.float("worldScale");
        Self {
            room_origin: DVec3::ZERO,
            world_scale: scale,
            room_scale_movement_delay: 0,
            world_rotation: s.float("worldRotation"),
            room_pre: make(DVec3::ZERO, 1.0, 0.0),
            world_pre: make(DVec3::ZERO, scale, 0.0),
            world_post: make(DVec3::ZERO, scale, 0.0),
            world_render: None,
            histories: Histories::default(),
            free_move_current: true,
            teleport_override: false,
            data_settings,
            room,
        }
    }

    /// `getVRDataWorld`.
    pub fn world(&self) -> &VrData {
        self.world_render.as_ref().unwrap_or(&self.world_pre)
    }

    fn data(&self, s: &VrSettings, origin: DVec3, scale: f32, rotation: f32) -> VrData {
        VrData::new(
            &self.room,
            self.data_settings,
            origin,
            s.float("walkMultiplier"),
            scale,
            rotation,
        )
    }

    /// `MCVR.poll`'s history updates, then `VRPlayer.postPoll`: new tracking data in room space.
    pub fn poll(&mut self, now_ms: u64, room: RoomPoses, s: &VrSettings) {
        self.room = room;
        self.data_settings = data_settings(s);
        let hmd = room.hmd;
        let eye = (room.eyes[0].pos + room.eyes[1].pos) * 0.5;
        // `MCVR`: the pivot is the eye plus the head's (0, -0.1, 0.1).
        let pivot = hmd.rot * Vec3::new(0.0, -0.1, 0.1) + eye;
        self.histories.hmd_pivot.add(now_ms, pivot);
        self.histories.hmd.add(now_ms, eye);
        for c in 0..2 {
            self.histories.controller[c].add(now_ms, room.aim[c].pos);
        }
        self.room_pre = self.data(s, DVec3::ZERO, 1.0, 0.0);
    }

    /// `preTick`: the data the logic tick works with, and the world scale for this tick.
    /// `eye_height_scale` is `ScaleHelper.getEntityEyeHeightScale` of the player (1 normally),
    /// `in_menu_room` the menu room's fixed scale.
    pub fn pre_tick(&mut self, s: &VrSettings, eye_height_scale: f32, in_menu_room: bool) {
        self.world_pre = self.data(
            s,
            self.room_origin,
            self.world_scale,
            self.world_rotation.to_radians(),
        );
        if in_menu_room {
            self.world_scale = 1.0;
        } else {
            self.world_scale = (s.float("worldScale") * eye_height_scale).clamp(0.025, 100.0);
        }
        if s.seated() && !in_menu_room {
            self.world_rotation = s.float("worldRotation");
        }
    }

    /// `postTick`: keep the head in place across scale and rotation changes.
    pub fn post_tick(&mut self, s: &VrSettings) {
        // Translation due only to scale: move the room so the head stays where it is.
        let offset = self.world_pre.hmd().scale_position_offset(self.world_scale);
        self.room_origin -= offset.as_dvec3();
        // Rotate the room about the head pivot by the change of the world rotation.
        let end = self.world_rotation;
        let start = self.world_pre.rotation.to_degrees();
        let pivot = self
            .world_pre
            .new_head_pivot(self.room_origin, self.world_scale);
        self.rotate_origin_around(-end + start, pivot);
        self.world_post = self.data(
            s,
            self.room_origin,
            self.world_scale,
            self.world_rotation.to_radians(),
        );
    }

    /// `preRender`: the data between the tick before and after.
    pub fn pre_render(&mut self, s: &VrSettings, partial: f32) {
        let scale = self.world_pre.world_scale
            + (self.world_post.world_scale - self.world_pre.world_scale) * partial;
        let rotation = lerp_rotation(self.world_pre.rotation, self.world_post.rotation, partial);
        let origin = self.world_pre.origin
            + (self.world_post.origin - self.world_pre.origin) * f64::from(partial);
        self.world_render = Some(self.data(s, origin, scale, rotation));
    }

    pub fn set_room_origin(&mut self, origin: DVec3, reset: bool) {
        if reset {
            self.world_pre.origin = origin;
        }
        self.room_origin = origin;
    }

    /// `snapRoomOriginToPlayerEntity`: put the room so the head is over the entity.
    /// `y_offset` is `vivecraft$getRoomYOffsetFromPose`.
    pub fn snap_room_origin_to_player(
        &mut self,
        entity: DVec3,
        y_offset: f64,
        reset: bool,
        instant: bool,
    ) {
        let cam = if instant {
            self.world_pre
                .new_head_pivot(self.room_origin, self.world_scale)
                - self.room_origin
        } else {
            self.world_pre.head_pivot() - self.world_pre.origin
        };
        self.set_room_origin(
            DVec3::new(entity.x - cam.x, entity.y + y_offset, entity.z - cam.z),
            reset,
        );
    }

    /// `rotateOriginAround`: the room origin turned by `degrees` about `pivot`.
    pub fn rotate_origin_around(&mut self, degrees: f32, pivot: DVec3) {
        let rads = degrees.to_radians();
        if rads != 0.0 {
            let p = self.room_origin;
            let (sin, cos) = (f64::from(rads.sin()), f64::from(rads.cos()));
            self.room_origin = DVec3::new(
                cos * (p.x - pivot.x) - sin * (p.z - pivot.z) + pivot.x,
                p.y,
                sin * (p.x - pivot.x) + cos * (p.z - pivot.z) + pivot.z,
            );
        }
    }

    /// `doPlayerMoveInRoom`: the player's x and z follow the head as it moves in the room.
    pub fn do_player_move_in_room(
        &mut self,
        s: &VrSettings,
        player: &WalkingPlayer,
        world: &mut impl RoomWalkWorld,
        can_roomscale_dismount: bool,
    ) -> RoomWalk {
        if self.room_scale_movement_delay > 0 {
            self.room_scale_movement_delay -= 1;
            return RoomWalk::None;
        }
        if player.shift_down
            || player.sleeping
            || player.jumping_tracker
            || player.grabbing_ladder
            || !player.alive
            || player.riding
        {
            return RoomWalk::None;
        }
        let pivot = self
            .world_pre
            .new_head_pivot(self.room_origin, self.world_scale);
        let walk = room_walk_to(s, player, world, can_roomscale_dismount, pivot.x, pivot.z);
        if let RoomWalk::Stepped { origin_up, .. } = walk {
            self.room_origin.y += origin_up;
        }
        walk
    }

    /// `updateFreeMove`: the heuristic of the locomotion type used last.
    pub fn update_free_move(&mut self, teleport_aiming: bool, move_input: bool) {
        if teleport_aiming {
            self.free_move_current = false;
        }
        if move_input {
            self.free_move_current = true;
        }
    }

    /// `isTeleportEnabled`: `server_allows_direct_teleport` is `SERVER_ALLOWS_DIRECT_TELEPORT`,
    /// `no_teleport_client` the permission file's `noTeleportClient`.
    pub fn teleport_enabled(
        &self,
        s: &VrSettings,
        no_teleport_client: bool,
        server_allows_direct_teleport: bool,
    ) -> bool {
        let enabled =
            (!no_teleport_client || self.teleport_override) && server_allows_direct_teleport;
        if s.seated() {
            enabled
        } else {
            enabled && !s.bool("forceStandingFreeMove")
        }
    }

    pub fn teleport_overridden(&self) -> bool {
        self.teleport_override
    }
    pub fn set_teleport_override(&mut self, on: bool) {
        self.teleport_override = on;
    }

    /// `getFreeMove`.
    pub fn free_move(&self, s: &VrSettings, teleport_enabled: bool) -> bool {
        if s.seated() {
            s.bool("seatedFreeMove") || !teleport_enabled
        } else {
            self.free_move_current || s.bool("forceStandingFreeMove")
        }
    }

    /// `freemoveDirection`: the movement the move input makes. `relative` is (strafe, forward),
    /// `speed` the player's speed attribute. `run_in_place` is the run tracker's (yaw, speed).
    #[allow(clippy::too_many_arguments)]
    pub fn freemove_direction(
        &self,
        s: &VrSettings,
        free_move: bool,
        relative: (f64, f64),
        speed: f32,
        flying: bool,
        swimming: bool,
        passenger: bool,
        sprinting: bool,
        jumping: bool,
        run_in_place: (f32, f32),
        inertia: f32,
    ) -> DVec3 {
        if !free_move {
            return DVec3::ZERO;
        }
        let (strafe, forward) = relative;
        let mut horizontal = strafe * strafe + forward * forward;
        if horizontal < 1.0e-4 {
            return DVec3::ZERO;
        }
        horizontal = horizontal.sqrt();
        if horizontal < 1.0 {
            horizontal = 1.0;
        }
        let k = f64::from(speed) / horizontal;
        let mut dir = Vec3::new((strafe * k) as f32, 0.0, (forward * k) as f32);
        let fly_or_swim = !passenger && (flying || swimming);
        let d = &self.world_pre;
        if s.seated() {
            // Vivecraft's `seatedUseHMD` picks controller 1 (sic), else 0.
            let pose = d.controller(usize::from(s.bool("seatedhmd")));
            if fly_or_swim {
                dir = rotate_x(dir, pose.pitch_rad());
            }
            dir = rotate_y(dir, -pose.yaw_rad());
        } else {
            let mode = s.free_move_mode(!passenger && flying);
            if fly_or_swim {
                dir = match mode {
                    FreeMove::Controller => rotate_x(dir, d.controller(1).pitch_rad()),
                    FreeMove::Hmd | FreeMove::RunInPlace | FreeMove::Room | FreeMove::Waist => {
                        rotate_x(dir, d.hmd().pitch_rad())
                    }
                    FreeMove::Auto => dir,
                };
            }
            if jumping {
                dir = rotate_y(dir, -d.hmd().yaw_rad());
            } else {
                dir = match mode {
                    FreeMove::Controller => rotate_y(dir, -d.controller(1).yaw_rad()),
                    FreeMove::Hmd => rotate_y(dir, -d.hmd().yaw_rad()),
                    FreeMove::RunInPlace => rotate_y(dir, -run_in_place.0) * run_in_place.1,
                    FreeMove::Room => rotate_y(dir, (180.0 + self.world_rotation).to_radians()),
                    FreeMove::Waist => rotate_y(dir, -d.body_yaw_rad()),
                    FreeMove::Auto => dir,
                };
            }
        }
        let multiplier = if sprinting && s.float("sprintMovementSpeedMultiplier") > 0.145 {
            s.float("sprintMovementSpeedMultiplier")
        } else {
            s.float("movementSpeedMultiplier")
        };
        let dir = dir * multiplier;
        let add = f64::from(inertia);
        let y_add = if flying { 5.0 } else { 1.0 };
        DVec3::new(
            f64::from(dir.x) * add,
            f64::from(dir.y) * y_add,
            f64::from(dir.z) * add,
        )
    }

    /// `doPermanentLookOverride`: the yaw and pitch (Minecraft degrees) the player entity gets.
    pub fn look_override(
        &self,
        s: &VrSettings,
        state: LookState,
        look_at: Option<(DVec3, DVec3)>,
    ) -> Option<(f32, f32)> {
        let d = &self.world_post;
        match state {
            LookState::Passenger { steering_dir } => steering_dir.map(|dir| {
                (
                    (-dir.x).atan2(dir.z).to_degrees(),
                    (-dir.y / dir.length()).asin().to_degrees(),
                )
            }),
            LookState::Blocking { hand } => {
                let c = d.controller(hand);
                Some((c.yaw(), -c.pitch()))
            }
            LookState::Sprinting { fall_flying } => {
                let mode = s.free_move_mode(fall_flying);
                Some(match mode {
                    FreeMove::Controller => (d.controller(1).yaw(), -d.controller(1).pitch()),
                    FreeMove::Waist => (d.body_yaw(), -d.hmd().pitch()),
                    _ => (d.hmd().yaw(), -d.hmd().pitch()),
                })
            }
            LookState::Normal => Some(match look_at {
                // Look AT the crosshair by default, most compatible with mods:
                // (eye position, crosshair position).
                Some((eye, target)) => {
                    let back = eye - target;
                    let what = (back.y / back.length()).clamp(-1.0, 1.0);
                    (
                        back.x.atan2(-back.z).to_degrees() as f32,
                        what.asin().to_degrees() as f32,
                    )
                }
                None => (d.hmd().yaw(), -d.hmd().pitch()),
            }),
        }
    }
}

/// What the player is doing, for `doPermanentLookOverride`.
#[derive(Clone, Copy, Debug)]
pub enum LookState {
    Passenger {
        steering_dir: Option<Vec3>,
    },
    /// Blocking with a shield in `hand` (0 main, 1 off).
    Blocking {
        hand: usize,
    },
    /// Sprint-jumping, flying with an elytra, or swimming forward.
    Sprinting {
        fall_flying: bool,
    },
    Normal,
}

fn rotate_x(v: Vec3, angle: f32) -> Vec3 {
    glam::Quat::from_rotation_x(angle) * v
}

fn data_settings(s: &VrSettings) -> DataSettings {
    DataSettings {
        seated: s.seated(),
        reverse_hands: s.left_handed(),
        aim_with_hmd: s.aim_with_hmd(),
    }
}

/// The state `MCVR.processBindings` keeps for turning.
#[derive(Clone, Copy, Debug, Default)]
pub struct Turning {
    /// A snap turn waits for the stick to come back.
    snap_held: bool,
}

/// The turn inputs of a frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct TurnInput {
    /// The turn stick's x (the rotate axis), -1..1.
    pub axis: f32,
    /// Seconds since the last call (smooth turning is per tick in Vivecraft: 20 a second).
    pub dt: f32,
}

impl Turning {
    /// Rotation of the world: returns the change of `worldRotation` in degrees (positive adds).
    /// Smooth: `worldRotationXSensitivity * 10 * axis` per tick, turning the world the opposite
    /// way; snap: `vrWorldRotationIncrement` once the stick passes 0.5, until it is released.
    pub fn update(&mut self, s: &VrSettings, input: TurnInput) -> f32 {
        let increment = s.float("vrWorldRotationIncrement");
        let ax = input.axis;
        if increment == 0.0 {
            self.snap_held = false;
            if ax != 0.0 {
                let ticks = input.dt * 20.0;
                return -(s.float("worldRotationXSensitivity") * 10.0 * ax) * ticks;
            }
            0.0
        } else if ax.abs() > 0.5 {
            if self.snap_held {
                0.0
            } else {
                self.snap_held = true;
                -increment * ax.signum()
            }
        } else {
            // `consumeClick`: a new press needs the stick to go back first.
            if ax.abs() < 0.3 {
                self.snap_held = false;
            }
            0.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vr_data::RoomPose;
    use glam::Quat;

    fn poses(head: Vec3) -> RoomPoses {
        let eye = RoomPose {
            pos: head,
            rot: Quat::IDENTITY,
        };
        RoomPoses {
            hmd: eye,
            eyes: [eye; 2],
            aim: [RoomPose {
                pos: head + Vec3::new(0.3, -0.5, -0.3),
                rot: Quat::IDENTITY,
            }; 2],
            hand: [RoomPose::IDENTITY; 2],
            tracking: [true; 2],
        }
    }

    struct Open;
    impl RoomWalkWorld for Open {
        fn no_collision(&mut self, _: Aabb) -> bool {
            true
        }
    }

    fn walker(pos: DVec3) -> WalkingPlayer {
        WalkingPlayer {
            pos,
            half_width: 0.3,
            height: 1.8,
            shift_down: false,
            sleeping: false,
            alive: true,
            riding: false,
            fall_distance_zero: true,
            on_climbable: false,
            jumping_tracker: false,
            grabbing_ladder: false,
            jump_factor_one: true,
            dimensions_width: 0.6,
        }
    }

    #[test]
    fn the_player_follows_the_head_in_the_room() {
        let s = VrSettings::default();
        let mut p = VrPlayer::new(&s);
        p.poll(0, poses(Vec3::new(0.4, 1.6, 0.1)), &s);
        p.room_origin = DVec3::new(10.0, 64.0, 10.0);
        p.pre_tick(&s, 1.0, false);
        let walk =
            p.do_player_move_in_room(&s, &walker(DVec3::new(10.0, 64.0, 10.0)), &mut Open, false);
        let RoomWalk::Moved { pos, .. } = walk else {
            panic!("{walk:?}");
        };
        // The player's feet are under the head pivot (0.1 m behind the eyes' z).
        assert!((pos.x - 10.4).abs() < 1e-5, "{pos}");
        assert!((pos.z - (10.0 + 0.1 + 0.1)).abs() < 1e-5, "{pos}");
    }

    #[test]
    fn sneaking_or_riding_stops_the_room_walk() {
        let s = VrSettings::default();
        let mut p = VrPlayer::new(&s);
        p.poll(0, poses(Vec3::new(0.4, 1.6, 0.1)), &s);
        p.pre_tick(&s, 1.0, false);
        let mut w = walker(DVec3::ZERO);
        w.shift_down = true;
        assert_eq!(
            p.do_player_move_in_room(&s, &w, &mut Open, false),
            RoomWalk::None
        );
        p.room_scale_movement_delay = 2;
        assert_eq!(
            p.do_player_move_in_room(&s, &walker(DVec3::ZERO), &mut Open, false),
            RoomWalk::None
        );
        assert_eq!(p.room_scale_movement_delay, 1);
    }

    #[test]
    fn walking_into_a_ledge_steps_up_when_allowed() {
        let mut s = VrSettings::default();
        s.set_bool("walkUpBlocks", true);
        let mut p = VrPlayer::new(&s);
        p.poll(0, poses(Vec3::new(1.5, 1.6, 0.0)), &s);
        p.pre_tick(&s, 1.0, false);
        // The ground is at y = 64; at x >= 1 it is a block higher.
        struct Ledge;
        impl RoomWalkWorld for Ledge {
            fn no_collision(&mut self, bb: Aabb) -> bool {
                let ground = if bb.max.x > 1.0 { 65.0 } else { 64.0 };
                bb.min.y >= ground
            }
        }
        let walk =
            p.do_player_move_in_room(&s, &walker(DVec3::new(0.0, 64.0, 0.0)), &mut Ledge, false);
        match walk {
            RoomWalk::Stepped { pos, origin_up, .. } => {
                assert!((pos.y - 65.0).abs() < 0.11, "{pos}");
                assert!(origin_up >= 1.0);
            }
            other => panic!("{other:?}"),
        }
        s.set_bool("walkUpBlocks", false);
        let mut p = VrPlayer::new(&s);
        p.poll(0, poses(Vec3::new(1.5, 1.6, 0.0)), &s);
        p.pre_tick(&s, 1.0, false);
        assert_eq!(
            p.do_player_move_in_room(&s, &walker(DVec3::new(0.0, 64.0, 0.0)), &mut Ledge, false),
            RoomWalk::None
        );
    }

    #[test]
    fn a_snap_turn_turns_once_per_flick_and_the_head_stays_put() {
        let s = VrSettings::default();
        let mut t = Turning::default();
        let flick = TurnInput {
            axis: 1.0,
            dt: 0.011,
        };
        assert_eq!(t.update(&s, flick), -45.0);
        assert_eq!(t.update(&s, flick), 0.0);
        t.update(
            &s,
            TurnInput {
                axis: 0.0,
                dt: 0.011,
            },
        );
        assert_eq!(
            t.update(
                &s,
                TurnInput {
                    axis: -0.9,
                    dt: 0.011
                }
            ),
            45.0
        );

        // The world turns about the head: the pivot does not move.
        let mut p = VrPlayer::new(&s);
        p.poll(0, poses(Vec3::new(0.5, 1.6, 0.2)), &s);
        p.room_origin = DVec3::new(3.0, 64.0, 4.0);
        p.pre_tick(&s, 1.0, false);
        let before = p.world_pre.new_head_pivot(p.room_origin, p.world_scale);
        p.world_rotation += 45.0;
        p.post_tick(&s);
        let after = p.world_post.head_pivot();
        assert!((before - after).length() < 1e-4, "{before} {after}");
        assert!((p.world_post.rotation - 45f32.to_radians()).abs() < 1e-6);
    }

    #[test]
    fn smooth_turning_scales_with_the_stick_and_time() {
        let mut s = VrSettings::default();
        s.set_float("vrWorldRotationIncrement", 0.0);
        let mut t = Turning::default();
        // One tick (0.05 s) at full tilt: sensitivity * 10 degrees, the world turning against.
        let d = t.update(
            &s,
            TurnInput {
                axis: 1.0,
                dt: 0.05,
            },
        );
        assert!((d + 10.0).abs() < 1e-4, "{d}");
        assert_eq!(
            t.update(
                &s,
                TurnInput {
                    axis: 0.0,
                    dt: 0.05
                }
            ),
            0.0
        );
    }

    #[test]
    fn free_move_follows_the_off_hand_or_the_head() {
        let mut s = VrSettings::default();
        let mut p = VrPlayer::new(&s);
        // The off hand points along +X in the room: a quarter turn right of -Z.
        let mut room = poses(Vec3::new(0.0, 1.6, 0.0));
        room.aim[1].rot = Quat::from_rotation_y(-std::f32::consts::FRAC_PI_2);
        p.poll(0, room, &s);
        p.pre_tick(&s, 1.0, false);
        let go = |p: &VrPlayer, s: &VrSettings| {
            p.freemove_direction(
                s,
                true,
                (0.0, 1.0),
                0.1,
                false,
                false,
                false,
                false,
                false,
                (0.0, 1.0),
                1.0,
            )
        };
        let v = go(&p, &s);
        // Controller mode: forward goes where the off hand points (+X).
        assert!(v.x > 0.09 && v.z.abs() < 1e-4, "{v}");
        s.set("vrFreeMoveMode", "HMD");
        let v = go(&p, &s);
        // The head faces -Z.
        assert!(v.z < -0.09 && v.x.abs() < 1e-4, "{v}");
        assert_eq!(
            p.freemove_direction(
                &s,
                false,
                (0.0, 1.0),
                0.1,
                false,
                false,
                false,
                false,
                false,
                (0.0, 1.0),
                1.0
            ),
            DVec3::ZERO
        );
    }

    #[test]
    fn the_player_looks_where_the_head_does_unless_blocking() {
        let s = VrSettings::default();
        let mut p = VrPlayer::new(&s);
        p.poll(0, poses(Vec3::new(0.0, 1.6, 0.0)), &s);
        p.pre_tick(&s, 1.0, false);
        p.post_tick(&s);
        let (yaw, pitch) = p.look_override(&s, LookState::Normal, None).unwrap();
        assert!((yaw.abs() - 180.0).abs() < 1e-3 && pitch.abs() < 1e-3);
        let (yaw, _) = p
            .look_override(&s, LookState::Blocking { hand: 1 }, None)
            .unwrap();
        assert!((yaw.abs() - 180.0).abs() < 1e-3);
        assert_eq!(player_height(&s), DEFAULT_HEIGHT);
    }
}
