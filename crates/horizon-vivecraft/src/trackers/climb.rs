// SPDX-License-Identifier: LGPL-3.0-or-later
//! Ports `ClimbTracker` (roomscale ladder and vine climbing, and Climbey's claws) and the
//! Climbey boots part of `JumpTracker`.
//! Copyright (c) Vivecraft contributors; LGPL-3.0-or-later (see LICENSE-LGPL-3.0).
//!
//! The tracker is a state machine: each logic tick it is given the hands' world places, what the
//! blocks at them are and the player, and answers with [`ClimbAction`]s the game applies (the
//! player's new position and motion, no gravity, a haptic pulse, the packets of the server).

use super::Haptic;
use crate::network::ClimbeyBlockmode;
use crate::player::{Aabb, Histories};
use crate::settings::VrSettings;
use crate::vr_data::VrData;
use glam::{DVec3, Vec3};

/// Which way a block's climbable side faces (`Direction`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    North,
    South,
    East,
    West,
}

/// What a block is to the climb tracker.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Climbable {
    #[default]
    No,
    /// A ladder or an open trapdoor over a ladder (`HorizontalDirectionalBlock` with `FACING`).
    Facing(Side),
    /// A vine: its sides that have a face and a solid block behind them.
    Vine {
        north: bool,
        east: bool,
        south: bool,
        west: bool,
    },
    /// Another block of the climbable tags: only grabbed whole, without auto grab.
    Other,
}

/// One block's facts.
#[derive(Clone, Debug, Default)]
pub struct ClimbBlock {
    pub climbable: Climbable,
    /// The bounds of its collision shape, relative to the block; `None` for no shape.
    pub shape: Option<Aabb>,
    /// The block's id, for the server's climbable list.
    pub id: String,
}

/// What the tracker asks of the world.
pub trait ClimbWorld {
    fn block(&self, pos: [i32; 3]) -> ClimbBlock;
    /// `level.noCollision(player, box)`.
    fn no_collision(&mut self, bb: Aabb) -> bool;
}

/// The player, as the tracker sees it.
#[derive(Clone, Copy, Debug)]
pub struct ClimbPlayer {
    pub pos: DVec3,
    /// The bounding box now.
    pub bb: Aabb,
    pub delta: DVec3,
    pub on_ground: bool,
    pub flying: bool,
    pub alive: bool,
    pub passenger: bool,
    pub in_game: bool,
    /// Jump boost amplifier + 1 (0 without the effect).
    pub jump_boost: i32,
    /// The server allows Climbey and a claw is in a hand.
    pub claws: bool,
    /// The server allows Climbey and jump boots are on.
    pub boots: bool,
    /// The move input is held (`zza` or `xxa`).
    pub moving: bool,
    pub free_move: bool,
}

/// The inputs of one tick.
pub struct ClimbInput<'a> {
    pub now_ms: u64,
    /// The data the logic tick works with, in the world and in the room.
    pub world: &'a VrData,
    pub room: &'a VrData,
    pub histories: &'a Histories,
    /// The Climbey grab binding per hand (the grips).
    pub grab: [bool; 2],
    /// The Climbey jump binding.
    pub jump_key: bool,
}

/// What the game does after a tick.
#[derive(Clone, Debug, PartialEq)]
pub enum ClimbAction {
    SetPos(DVec3),
    /// `xOld`, `yOld`, `zOld`: where the interpolation starts.
    SetPrevious(DVec3),
    SetDelta(DVec3),
    NoGravity(bool),
    ResetFall,
    Haptic(Haptic),
    /// `ClimbingPayloadC2S`.
    SendClimbing,
    /// `JumpingPayloadC2S`.
    SendJumping,
    /// `snapRoomOriginToPlayerEntity`.
    SnapRoom,
    /// `keyClimbeyGrab.unpressKey`.
    UnpressGrab(usize),
    /// The step sound of grabbing a block at this place.
    StepSound(DVec3),
}

const NORTH_BB: Aabb = bb(0.1, 0.0, 0.9, 0.9, 1.0, 1.1);
const SOUTH_BB: Aabb = bb(0.1, 0.0, -0.1, 0.9, 1.0, 0.1);
const WEST_BB: Aabb = bb(0.9, 0.0, 0.1, 1.1, 1.0, 0.9);
const EAST_BB: Aabb = bb(-0.1, 0.0, 0.1, 0.1, 1.0, 0.9);
const FULL_BB: Aabb = bb(0.0, 0.0, 0.0, 1.0, 1.0, 1.0);

const fn bb(x0: f64, y0: f64, z0: f64, x1: f64, y1: f64, z1: f64) -> Aabb {
    Aabb {
        min: DVec3::new(x0, y0, z0),
        max: DVec3::new(x1, y1, z1),
    }
}

fn intersects(a: &Aabb, b: &Aabb) -> bool {
    a.min.x < b.max.x
        && a.max.x > b.min.x
        && a.min.y < b.max.y
        && a.max.y > b.min.y
        && a.min.z < b.max.z
        && a.max.z > b.min.z
}

fn contains(a: &Aabb, p: DVec3) -> bool {
    p.x >= a.min.x
        && p.x < a.max.x
        && p.y >= a.min.y
        && p.y < a.max.y
        && p.z >= a.min.z
        && p.z < a.max.z
}

fn block_pos(p: DVec3) -> [i32; 3] {
    [p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32]
}

fn offset(a: &Aabb, pos: [i32; 3]) -> Aabb {
    a.moved(DVec3::new(
        f64::from(pos[0]),
        f64::from(pos[1]),
        f64::from(pos[2]),
    ))
}

/// `ClimbTracker`.
#[derive(Clone, Debug)]
pub struct ClimbTracker {
    pub blocklist: Vec<String>,
    pub server_blockmode: ClimbeyBlockmode,
    pub force_activate: bool,
    pub latch_start_controller: Option<usize>,
    latch_start: [DVec3; 2],
    latch_start_room: [Vec3; 2],
    latch_box: [Option<Aabb>; 2],
    gravity_override: bool,
    want_jump: bool,
    latched: [bool; 2],
    was_in_block: [bool; 2],
    was_button: [bool; 2],
    was_latched: [bool; 2],
    in_block: [bool; 2],
    grab_direction: [Option<Side>; 2],
    unset_flag: bool,
    /// The deterministic stand-in for `rand.nextInt(20)` is not needed: only dust uses it.
    _private: (),
}

impl Default for ClimbTracker {
    fn default() -> Self {
        Self {
            blocklist: Vec::new(),
            server_blockmode: ClimbeyBlockmode::Disabled,
            force_activate: false,
            latch_start_controller: None,
            latch_start: [DVec3::ZERO; 2],
            latch_start_room: [Vec3::ZERO; 2],
            latch_box: [None; 2],
            gravity_override: false,
            want_jump: false,
            latched: [false; 2],
            was_in_block: [false; 2],
            was_button: [false; 2],
            was_latched: [false; 2],
            in_block: [false; 2],
            grab_direction: [None; 2],
            unset_flag: false,
            _private: (),
        }
    }
}

impl ClimbTracker {
    pub fn is_grabbing_ladder(&self) -> bool {
        self.latched[0] || self.latched[1]
    }

    /// Sic: Vivecraft mixes `wasLatched[0]` and `latched[1]`.
    pub fn was_grabbing_ladder(&self) -> bool {
        self.was_latched[0] || self.latched[1]
    }

    pub fn is_grabbing(&self, controller: usize) -> bool {
        self.latched[controller]
    }

    /// `isClimbeyClimb`: the claws are equipped and the tracker is on.
    pub fn is_climbey(&self, s: &VrSettings, p: &ClimbPlayer) -> bool {
        self.is_active(s, p) && p.claws
    }

    pub fn is_active(&self, s: &VrSettings, p: &ClimbPlayer) -> bool {
        if s.seated() {
            return false;
        }
        if !p.free_move && !s.bool("simulateFalling") {
            return false;
        }
        if !s.bool("realisticClimbEnabled") || !p.in_game || p.passenger {
            return false;
        }
        p.claws || !p.moving
    }

    /// Vivecraft's `isClimbableBlock` decided by the game; the tracker only uses the result.
    fn allowed(&self, block: &ClimbBlock) -> bool {
        match self.server_blockmode {
            ClimbeyBlockmode::Disabled => true,
            ClimbeyBlockmode::Whitelist => self.blocklist.contains(&block.id),
            ClimbeyBlockmode::Blacklist => !self.blocklist.contains(&block.id),
        }
    }

    /// `idleProcess`: which grab bindings are enabled is for the game (the grips); this keeps the
    /// forced activation after letting go of a ladder. Returns whether the grab binding is
    /// enabled per hand.
    pub fn idle(&mut self, s: &VrSettings, p: &ClimbPlayer) -> [bool; 2] {
        if !self.is_active(s, p) {
            self.was_latched = [false; 2];
        }
        if self.was_grabbing_ladder() && !self.is_grabbing_ladder() {
            self.force_activate = true;
        } else if p.on_ground || p.flying {
            self.force_activate = false;
        }
        let needs_button = self.is_climbey(s, p) || !s.bool("climbingAutoGrab");
        [0, 1].map(|c| {
            needs_button && (self.is_grabbing_ladder() || self.in_block[c] || self.force_activate)
        })
    }

    /// `inactiveProcess`.
    pub fn inactive(&mut self) -> Vec<ClimbAction> {
        self.latch_start_controller = None;
        self.latched = [false; 2];
        vec![ClimbAction::NoGravity(false)]
    }

    /// `activeProcess`: one logic tick.
    pub fn tick(
        &mut self,
        s: &VrSettings,
        p: &ClimbPlayer,
        input: &ClimbInput,
        world: &mut impl ClimbWorld,
    ) -> Vec<ClimbAction> {
        let mut out = Vec::new();
        let climbey = self.is_climbey(s, p);
        let auto_grab = s.bool("climbingAutoGrab");
        let mut button = [false; 2];
        let mut allowed = [false; 2];
        let mut controller_pos = [DVec3::ZERO; 2];
        let mut boxes: [Option<Aabb>; 2] = [None; 2];
        let mut nope = false;
        let mut grabbed = false;
        let mut jump = false;
        let mut ladder = false;

        for c in 0..2 {
            controller_pos[c] = input.world.controller(c).position();
            let controller_dir = input.world.controller(c).direction();
            self.in_block[c] = false;
            let mut pos = block_pos(controller_pos[c]);
            let mut block = world.block(pos);
            boxes[c] = block.shape;

            if !climbey {
                // Roomscale climbable.
                let near = controller_pos[c] - (controller_dir * 0.2).as_dvec3();
                let controller_bb = Aabb {
                    min: controller_pos[c].min(near),
                    max: controller_pos[c].max(near),
                };
                ladder = true;
                let mut ok = block.climbable != Climbable::No;
                if !ok {
                    // The other end of the hand's box.
                    let pos2 = block_pos(near);
                    let block2 = world.block(pos2);
                    if block2.climbable != Climbable::No {
                        ok = true;
                        pos = pos2;
                        block = block2;
                        controller_pos[c] = near;
                        boxes[c] = block.shape;
                    }
                }
                let was_ok = ok;
                if ok {
                    let mut bbs: Vec<(Aabb, Option<Side>)> = Vec::new();
                    match block.climbable {
                        Climbable::Facing(side) => {
                            let a = match side {
                                Side::East => EAST_BB,
                                Side::North => NORTH_BB,
                                Side::South => SOUTH_BB,
                                Side::West => WEST_BB,
                            };
                            bbs.push((a, Some(side)));
                        }
                        Climbable::Vine {
                            north,
                            east,
                            south,
                            west,
                        } => {
                            boxes[c] = Some(FULL_BB);
                            if north {
                                bbs.push((SOUTH_BB, Some(Side::South)));
                            }
                            if east {
                                bbs.push((WEST_BB, Some(Side::West)));
                            }
                            if south {
                                bbs.push((NORTH_BB, Some(Side::North)));
                            }
                            if west {
                                bbs.push((EAST_BB, Some(Side::East)));
                            }
                        }
                        _ => {
                            if !auto_grab {
                                bbs.push((FULL_BB, None));
                            }
                        }
                    }
                    for (a, side) in bbs {
                        if intersects(&controller_bb, &offset(&a, pos)) {
                            self.in_block[c] = true;
                            if side.is_some() {
                                self.grab_direction[c] = side;
                            }
                            break;
                        }
                    }
                }
                // Don't let go that easily when not auto grabbing.
                if !was_ok || (!self.in_block[c] && !auto_grab) {
                    let hand_to_latch = self.latch_start[c] - controller_pos[c];
                    if hand_to_latch.length() > 0.5 {
                        self.in_block[c] = false;
                    } else {
                        let latch_block = world.block(block_pos(self.latch_start[c]));
                        self.in_block[c] =
                            self.was_in_block[c] && latch_block.climbable != Climbable::No;
                    }
                }
                button[c] = self.in_block[c] && (auto_grab || input.grab[c]);
                allowed[c] = self.in_block[c];
            } else {
                // Climbey (the game lifts the player's ground contact while latched).
                button[c] = input.grab[c];
                self.in_block[c] = boxes[c]
                    .as_ref()
                    .is_some_and(|b| contains(&offset(b, pos), controller_pos[c]));
                if !self.in_block[c] {
                    let hand_to_latch = self.latch_start[c] - controller_pos[c];
                    if hand_to_latch.length() > 0.5 {
                        button[c] = false;
                    }
                }
                allowed[c] = self.allowed(&block);
            }

            self.was_latched[c] = self.latched[c];

            if !button[c] && self.latched[c] {
                // Let go.
                self.latched[c] = false;
                out.push(ClimbAction::UnpressGrab(c));
                jump = true;
            }

            if !self.latched[c] && !nope && allowed[c] {
                if !self.was_in_block[c] && self.in_block[c] {
                    // Indicate that it can be grabbed.
                    out.push(ClimbAction::Haptic(Haptic {
                        controller: c,
                        micros: 750,
                    }));
                }
                if (!self.was_in_block[c] && self.in_block[c] && button[c])
                    || (!self.was_button[c] && button[c] && self.in_block[c])
                {
                    // Grab.
                    grabbed = true;
                    self.want_jump = false;
                    self.latch_start[c] = controller_pos[c];
                    self.latch_start_room[c] = input.room.controller(c).position_f();
                    self.latch_start_controller = Some(c);
                    self.latch_box[c] = boxes[c];
                    self.latched[c] = true;
                    if c == 0 {
                        self.latched[1] = false;
                        nope = true;
                    } else {
                        self.latched[0] = false;
                    }
                    out.push(ClimbAction::Haptic(Haptic {
                        controller: c,
                        micros: 2000,
                    }));
                    out.push(ClimbAction::StepSound(self.latch_start[c]));
                }
            }
            self.was_button[c] = button[c];
            self.was_in_block[c] = self.in_block[c];
        }

        if !self.latched[0] && !self.latched[1] {
            // Let go with one hand: the other hand takes over.
            for c in 0..2 {
                if self.in_block[c] && button[c] && allowed[c] {
                    grabbed = true;
                    self.latch_start[c] = controller_pos[c];
                    self.latch_start_room[c] = input.room.controller(c).position_f();
                    self.latch_start_controller = Some(c);
                    self.latched[c] = true;
                    self.latch_box[c] = boxes[c];
                    self.want_jump = false;
                    out.push(ClimbAction::Haptic(Haptic {
                        controller: c,
                        micros: 2000,
                    }));
                }
            }
        }

        if !self.want_jump && !ladder {
            self.want_jump = input.jump_key && p.boots;
        }
        jump &= self.want_jump;

        if (self.latched[0] || self.latched[1]) && !self.gravity_override {
            self.unset_flag = true;
            out.push(ClimbAction::NoGravity(true));
            self.gravity_override = true;
        }
        if !self.latched[0] && !self.latched[1] && self.gravity_override {
            out.push(ClimbAction::NoGravity(false));
            self.gravity_override = false;
        }

        if !self.latched[0] && !self.latched[1] && !jump {
            if p.on_ground && self.unset_flag {
                self.unset_flag = false;
                out.push(ClimbAction::UnpressGrab(0));
                out.push(ClimbAction::UnpressGrab(1));
            }
            self.latch_start_controller = None;
            return out;
        }

        let Some(lc) = self.latch_start_controller else {
            return out;
        };
        let now = input.world.controller(lc).position();
        let start = input.world.room_to_world(self.latch_start_room[lc]);
        let delta = now - start;
        self.latch_start_room[lc] = input.room.controller(lc).position_f();

        if self.want_jump {
            // Bzzz.
            out.push(ClimbAction::Haptic(Haptic {
                controller: lc,
                micros: 200,
            }));
        }

        if !jump {
            let delta_movement = if grabbed {
                DVec3::ZERO
            } else {
                DVec3::new(p.delta.x, 0.0, p.delta.z)
            };
            out.push(ClimbAction::SetDelta(delta_movement));
            out.push(ClimbAction::ResetFall);
            let (x, y, z) = (p.pos.x, p.pos.y, p.pos.z);
            let mut new_x = x;
            let new_y = y - delta.y;
            let mut new_z = z;
            let pos = block_pos(self.latch_start[lc]);
            let mut new_y = new_y;
            if !ladder || !auto_grab {
                new_x = x - delta.x;
                new_z = z - delta.z;
            } else {
                let squeeze = f64::from(1.0 - input.world.world_scale.min(1.0));
                match self.grab_direction[lc] {
                    Some(dir @ (Side::North | Side::South)) => {
                        new_x = x - delta.x;
                        new_z = f64::from(pos[2]) + 0.5;
                        new_z += squeeze * if dir == Side::North { 0.5 } else { -0.5 };
                    }
                    Some(dir @ (Side::East | Side::West)) => {
                        new_z = z - delta.z;
                        new_x = f64::from(pos[0]) + 0.5;
                        new_x += squeeze * if dir == Side::West { 0.5 } else { -0.5 };
                    }
                    None => {}
                }
            }
            let hmd = input.room.head_pivot().y;
            let controller = input.room.controller(lc).position().y;
            // Getting off on top.
            if let Some(latch_box) = self.latch_box[lc]
                && !self.want_jump
                && controller <= hmd / 2.0
                && self.latch_start[lc].y > latch_box.max.y * 0.8 + f64::from(pos[1])
            {
                let mut horizontal = input.world.hmd().direction();
                horizontal.y = 0.0;
                let horizontal = horizontal.normalize_or_zero() * 0.1;
                let rise = latch_box.max.y + f64::from(pos[1]) - p.pos.y;
                let moved = p.bb.moved(DVec3::new(
                    f64::from(horizontal.x),
                    rise,
                    f64::from(horizontal.z),
                ));
                if world.no_collision(moved) {
                    new_x = p.pos.x + f64::from(horizontal.x);
                    new_y = latch_box.max.y + f64::from(pos[1]);
                    new_z = p.pos.z + f64::from(horizontal.z);
                    self.latch_start_controller = None;
                    self.latched = [false; 2];
                    self.was_in_block = [false; 2];
                    out.push(ClimbAction::NoGravity(false));
                }
            }
            let valid = find_valid_pos(p, DVec3::new(new_x, new_y, new_z), true, world, &mut out);
            out.push(ClimbAction::SetPos(valid));
            out.push(ClimbAction::SendClimbing);
        } else {
            // Jump!
            self.want_jump = false;
            let p1 = find_valid_pos(p, p.pos - delta, false, world, &mut out);
            let history = &input.histories.controller[lc];
            let mut movement = history.net_movement(input.now_ms, 0.3);
            let speed = history.average_speed(input.now_ms, 0.3);
            movement *= 0.66 * speed;
            let limit = 0.66;
            if movement.length() > limit {
                movement *= limit / movement.length();
            }
            if p.jump_boost > 0 {
                movement *= p.jump_boost as f32 + 0.5;
            }
            let movement = crate::vr_data::rotate_y(movement, input.world.rotation);
            let delta_movement = DVec3::new(
                -f64::from(movement.x),
                -f64::from(movement.y),
                -f64::from(movement.z),
            );
            out.push(ClimbAction::SetDelta(delta_movement));
            out.push(ClimbAction::SetPrevious(p1));
            out.push(ClimbAction::SetPos(p1));
            // Move the player one tick, but not into a wall.
            let moved = DVec3::new(
                p1.x + delta_movement.x,
                p1.y + delta_movement.y,
                p1.z + delta_movement.z,
            );
            let probe = ClimbPlayer {
                pos: p1,
                bb: p.bb.moved(p1 - p.pos),
                ..*p
            };
            let end = find_valid_pos(&probe, moved, false, world, &mut out);
            out.push(ClimbAction::SetPos(end));
            out.push(ClimbAction::SnapRoom);
            out.push(ClimbAction::SendJumping);
        }
        out
    }
}

/// `findValidPlayerPos`: the new place, or the nearest of the ways to keep part of the old.
fn find_valid_pos(
    p: &ClimbPlayer,
    new: DVec3,
    feedback: bool,
    world: &mut impl ClimbWorld,
    out: &mut Vec<ClimbAction>,
) -> DVec3 {
    let old = p.pos;
    for i in 0..8 {
        let mut check = new;
        match i {
            1 => check.y = old.y,
            2 => check.z = old.z,
            3 => check.x = old.x,
            4 => {
                check.z = old.z;
                check.x = old.x;
            }
            5 => {
                check.x = old.x;
                check.y = old.y;
            }
            6 => {
                check.y = old.y;
                check.z = old.z;
            }
            7 => check = old,
            _ => {}
        }
        let bb = p.bb.moved(check - old);
        if world.no_collision(bb) {
            if feedback && i > 1 {
                // Ouch!
                for controller in 0..2 {
                    out.push(ClimbAction::Haptic(Haptic {
                        controller,
                        micros: 100,
                    }));
                }
            }
            return check;
        }
    }
    old
}

/// The Climbey boots part of `JumpTracker`: grab with both hands' jump binding, pull, let go to
/// leap.
#[derive(Clone, Debug, Default)]
pub struct ClimbJump {
    latched: [bool; 2],
    latch_start: [Vec3; 2],
    latch_start_origin: [DVec3; 2],
    latch_start_player: [DVec3; 2],
}

impl ClimbJump {
    pub fn is_jumping(&self) -> bool {
        self.latched[0] || self.latched[1]
    }

    pub fn inactive(&mut self) {
        self.latched = [false; 2];
    }

    /// `activeProcess` with the boots on. `jump_key` is the binding per hand (it is one
    /// binding in Vivecraft: both hands read the same state).
    pub fn tick(
        &mut self,
        p: &ClimbPlayer,
        input: &ClimbInput,
        climb: &mut ClimbTracker,
    ) -> Vec<ClimbAction> {
        let mut out = Vec::new();
        let ok = [input.jump_key; 2];
        let mut jump = false;
        let now = input
            .room
            .controller(0)
            .position_f()
            .lerp(input.room.controller(1).position_f(), 0.5);
        for c in 0..2 {
            if !ok[c] && self.latched[c] {
                out.push(ClimbAction::Haptic(Haptic {
                    controller: c,
                    micros: 200,
                }));
                jump = true;
            }
            if ok[c] && !self.latched[c] {
                self.latch_start[c] = now;
                self.latch_start_origin[c] = input.world.origin;
                self.latch_start_player[c] = p.pos;
                out.push(ClimbAction::Haptic(Haptic {
                    controller: c,
                    micros: 1000,
                }));
            }
        }
        self.latched = ok;
        let delta = crate::vr_data::rotate_y(now - self.latch_start[0], input.world.rotation);
        if !jump && self.is_jumping() {
            for controller in 0..2 {
                out.push(ClimbAction::Haptic(Haptic {
                    controller,
                    micros: 200,
                }));
            }
        }
        if jump {
            climb.force_activate = true;
            let h = &input.histories.controller;
            let movement =
                h[0].net_movement(input.now_ms, 0.3) + h[1].net_movement(input.now_ms, 0.3);
            let speed =
                h[0].average_speed(input.now_ms, 0.3) + h[1].average_speed(input.now_ms, 0.3) * 0.5;
            let mut movement = movement * (0.33 * speed);
            let limit = 0.66;
            if movement.length() > limit {
                movement *= limit / movement.length();
            }
            if p.jump_boost > 0 {
                movement *= p.jump_boost as f32 + 0.5;
            }
            let movement = crate::vr_data::rotate_y(movement, input.world.rotation);
            let mut last = p.pos - delta.as_dvec3();
            if delta.y < 0.0 && movement.y < 0.0 {
                let v = DVec3::new(
                    p.delta.x - f64::from(movement.x) * 1.25,
                    f64::from(-movement.y),
                    p.delta.z - f64::from(movement.z) * 1.25,
                );
                out.push(ClimbAction::SetDelta(v));
                out.push(ClimbAction::SetPrevious(last));
                last += v;
                out.push(ClimbAction::SetPos(last));
                out.push(ClimbAction::SnapRoom);
                out.push(ClimbAction::SendJumping);
            } else {
                out.push(ClimbAction::SnapRoom);
            }
        }
        out
    }

    /// While latched, the room (not the player) moves with the hands: the new room origin.
    pub fn room_origin(&self, p: &ClimbPlayer, input: &ClimbInput) -> Option<DVec3> {
        if !self.is_jumping() {
            return None;
        }
        let now = input
            .room
            .controller(0)
            .position_f()
            .lerp(input.room.controller(1).position_f(), 0.5);
        let delta = crate::vr_data::rotate_y(now - self.latch_start[0], input.world.rotation);
        Some(self.latch_start_origin[0] - self.latch_start_player[0] + p.pos - delta.as_dvec3())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vr_data::{DataSettings, RoomPose, RoomPoses};
    use glam::Quat;

    /// A ladder column at x = 5, z = 5 on the north side: a wall of one ladder block high 10.
    struct Wall;
    impl ClimbWorld for Wall {
        fn block(&self, pos: [i32; 3]) -> ClimbBlock {
            if pos[0] == 5 && pos[2] == 5 && (0..10).contains(&pos[1]) {
                ClimbBlock {
                    climbable: Climbable::Facing(Side::South),
                    shape: None,
                    id: "minecraft:ladder".into(),
                }
            } else {
                ClimbBlock::default()
            }
        }
        fn no_collision(&mut self, _: Aabb) -> bool {
            true
        }
    }

    fn data(hand_y: f32) -> VrData {
        let pose = |pos: Vec3| RoomPose {
            pos,
            rot: Quat::IDENTITY,
        };
        let room = RoomPoses {
            hmd: pose(Vec3::new(0.0, 1.7, 0.0)),
            eyes: [pose(Vec3::new(0.0, 1.7, 0.0)); 2],
            // The hands point along -Z: the near end of the hand's box is 0.2 in front.
            aim: [
                pose(Vec3::new(0.0, hand_y, 0.0)),
                pose(Vec3::new(0.5, 1.0, 0.0)),
            ],
            hand: [RoomPose::IDENTITY; 2],
            tracking: [true; 2],
        };
        VrData::new(
            &room,
            DataSettings {
                seated: false,
                reverse_hands: false,
                aim_with_hmd: false,
            },
            DVec3::new(5.4, 0.0, 5.05),
            1.0,
            1.0,
            0.0,
        )
    }

    fn player(y: f64) -> ClimbPlayer {
        let pos = DVec3::new(5.4, y, 5.05);
        ClimbPlayer {
            pos,
            bb: Aabb {
                min: pos - DVec3::new(0.3, 0.0, 0.3),
                max: pos + DVec3::new(0.3, 1.8, 0.3),
            },
            delta: DVec3::ZERO,
            on_ground: true,
            flying: false,
            alive: true,
            passenger: false,
            in_game: true,
            jump_boost: 0,
            claws: false,
            boots: false,
            moving: false,
            free_move: true,
        }
    }

    fn input<'a>(world: &'a VrData, room: &'a VrData, h: &'a Histories) -> ClimbInput<'a> {
        ClimbInput {
            now_ms: 1000,
            world,
            room,
            histories: h,
            grab: [false; 2],
            jump_key: false,
        }
    }

    #[test]
    fn a_hand_in_a_ladder_grabs_it_and_pulling_down_lifts_the_player() {
        let s = VrSettings::default();
        let mut tracker = ClimbTracker::default();
        let h = Histories::default();
        let p = player(0.0);
        // Hand 0 at world (5.4, 1.0, 5.05): the ladder's plate is at z = 5.9..6.1? South side: z -0.1..0.1.
        let w0 = data(1.5);
        let actions = tracker.tick(&s, &p, &input(&w0, &w0, &h), &mut Wall);
        assert!(tracker.is_grabbing(0), "{actions:?}");
        assert!(actions.contains(&ClimbAction::NoGravity(true)));
        // The hand pulls down 20 cm: the player rises by 20 cm.
        let w1 = data(1.3);
        let actions = tracker.tick(&s, &p, &input(&w1, &w1, &h), &mut Wall);
        let up = actions
            .iter()
            .find_map(|a| match a {
                ClimbAction::SetPos(v) => Some(v.y),
                _ => None,
            })
            .expect("a new position");
        assert!((up - 0.2).abs() < 1e-3, "{up}");
        assert!(actions.contains(&ClimbAction::SendClimbing));
    }

    #[test]
    fn walking_with_the_stick_is_not_climbing() {
        let s = VrSettings::default();
        let tracker = ClimbTracker::default();
        let mut p = player(0.0);
        p.moving = true;
        assert!(!tracker.is_active(&s, &p));
        p.moving = false;
        assert!(tracker.is_active(&s, &p));
        p.claws = true;
        p.moving = true;
        assert!(tracker.is_active(&s, &p));
    }

    #[test]
    fn letting_go_of_a_ladder_gives_gravity_back() {
        let s = VrSettings::default();
        let mut tracker = ClimbTracker::default();
        let h = Histories::default();
        let p = player(0.0);
        let w0 = data(1.5);
        tracker.tick(&s, &p, &input(&w0, &w0, &h), &mut Wall);
        assert!(tracker.is_grabbing(0));
        // The hand leaves the ladder: 2 m away.
        let mut far = data(1.0);
        far.c[0].pos.x = 3.0;
        let actions = tracker.tick(&s, &p, &input(&far, &far, &h), &mut Wall);
        assert!(!tracker.is_grabbing(0));
        assert!(actions.contains(&ClimbAction::NoGravity(false)));
    }
}
