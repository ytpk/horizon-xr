// SPDX-License-Identifier: MPL-2.0
// Ports Visor: core/client/tasks/types/movement/TaskRoomClimb.java.
//! Visor's room climbing (`TaskRoomClimb`): a hand that reaches into a ladder's or a vine's face
//! grabs it, and the hand pulling moves the player (the hand stays where it is in the room, so the
//! body goes the other way). Per hand it keeps whether the hand is in a grab volume (the wall plane
//! of the block's face thickened by 0.1, `wallSlab`), whether it was last tick and whether it holds;
//! the player is placed against the face (`anchorFace`), pushed out over a ledge when the hand is
//! low and over the block's top (`TOP_OF_BLOCK_FRACTION`), and the new place is tried axis by axis
//! (`AXIS_RETREATS`) until it does not collide. Copyright (C) Phoenix-Ra and Visor contributors;
//! MPL-2.0 (see LICENSE-MPL-2.0).
//!
//! The world is the game's, through [`horizon_vivecraft::trackers::climb::ClimbWorld`] (block
//! facts and the collision test); Visor grabs only ladders (`LadderBlock`) and vines
//! (`VineBlock`), not the other blocks of the `climbable` tag Vivecraft's tracker takes.

use glam::{DVec3, Vec3};
use horizon_vivecraft::{
    player::Aabb,
    trackers::climb::{ClimbBlock, ClimbWorld, Climbable, Side},
    vr_data::VrData,
};

/// `HAND_DIRECTION_OFFSET`: the hand's box reaches this far back along the controller.
pub const HAND_DIRECTION_OFFSET: f64 = 0.2;
/// `HAND_DISTANCE_THRESHOLD`: a hand this far from where it grabbed lets go.
pub const HAND_DISTANCE_THRESHOLD: f64 = 0.5;
/// `COLLISION_BOX_OFFSET`: how thick the grab slab of a face is (each side of the plane).
pub const COLLISION_BOX_OFFSET: f64 = 0.1;
/// `TOP_OF_BLOCK_FRACTION`.
pub const TOP_OF_BLOCK_FRACTION: f64 = 0.82;

const AXIS_X: u8 = 1;
const AXIS_Y: u8 = 2;
const AXIS_Z: u8 = 4;
/// `AXIS_RETREATS`: the axes put back to where they were, one or two at a time.
const AXIS_RETREATS: [u8; 6] = [
    AXIS_Y,
    AXIS_Z,
    AXIS_X,
    AXIS_X | AXIS_Z,
    AXIS_X | AXIS_Y,
    AXIS_Y | AXIS_Z,
];

/// `wallSlab(facing)`: the wall plane of the face a block shows towards `facing` (the block's
/// `FACING` points away from its wall, so the wall is on the opposite side), thickened to +-0.1
/// and inset by 0.1 along the other horizontal axis.
pub fn wall_slab(facing: Side) -> Aabb {
    let (near, far) = (COLLISION_BOX_OFFSET, 1.0 - COLLISION_BOX_OFFSET);
    // The wall: the side opposite `facing`, and its plane (1 on a positive side, else 0).
    let (x_axis, plane) = match facing {
        Side::North => (false, 1.0),
        Side::South => (false, 0.0),
        Side::East => (true, 0.0),
        Side::West => (true, 1.0),
    };
    let (low, high) = (plane - COLLISION_BOX_OFFSET, plane + COLLISION_BOX_OFFSET);
    if x_axis {
        Aabb {
            min: DVec3::new(low, 0.0, near),
            max: DVec3::new(high, 1.0, far),
        }
    } else {
        Aabb {
            min: DVec3::new(near, 0.0, low),
            max: DVec3::new(far, 1.0, high),
        }
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

fn block_pos(p: DVec3) -> [i32; 3] {
    [p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32]
}

fn moved(a: &Aabb, pos: [i32; 3]) -> Aabb {
    a.moved(DVec3::new(
        f64::from(pos[0]),
        f64::from(pos[1]),
        f64::from(pos[2]),
    ))
}

/// Visor grabs a ladder (`LadderBlock`) or a vine; `block`'s facts say which.
fn is_ladder(block: &ClimbBlock) -> Option<Side> {
    match block.climbable {
        Climbable::Facing(side) if block.id == "minecraft:ladder" => Some(side),
        _ => None,
    }
}

/// `isClimbableBlock`.
pub fn is_climbable(block: &ClimbBlock) -> bool {
    is_ladder(block).is_some() || matches!(block.climbable, Climbable::Vine { .. })
}

#[derive(Clone, Copy, Debug)]
struct HandState {
    anchored: bool,
    inside: bool,
    was_inside: bool,
    face: Option<Side>,
    anchored_pos: DVec3,
    anchored_pos_room: DVec3,
    hand_shape: Option<Aabb>,
    anchored_shape: Option<Aabb>,
}

impl Default for HandState {
    fn default() -> Self {
        Self {
            anchored: false,
            inside: false,
            was_inside: false,
            face: None,
            anchored_pos: DVec3::ZERO,
            anchored_pos_room: DVec3::ZERO,
            hand_shape: None,
            anchored_shape: None,
        }
    }
}

/// The player as the task moves it.
#[derive(Clone, Copy, Debug)]
pub struct ClimbBody {
    pub pos: DVec3,
    pub delta: DVec3,
    /// The box's half width and height.
    pub half_width: f64,
    pub height: f64,
    /// `ClientContext.localPlayer.getWorldScale()`.
    pub world_scale: f64,
}

impl ClimbBody {
    fn bb_at(&self, at: DVec3) -> Aabb {
        Aabb {
            min: DVec3::new(at.x - self.half_width, at.y, at.z - self.half_width),
            max: DVec3::new(
                at.x + self.half_width,
                at.y + self.height,
                at.z + self.half_width,
            ),
        }
    }
}

/// What the game does after a tick (`player.setPos`, `setDeltaMovement`, `setNoGravity`...).
#[derive(Clone, Debug, PartialEq)]
pub enum ClimbEffect {
    SetPos(DVec3),
    SetDelta(DVec3),
    NoGravity(bool),
    ResetFall,
    /// `HapticFeedback.CLIMB_GRAB` on a hand (0 main, 1 off).
    Haptic(usize),
    /// `visor$stepSound(blockPos, position)`.
    StepSound(DVec3),
    /// `ClimbingPayloadToServer` (on a server that is not the player's own: the game's own
    /// integrated server gets it too, which resets the fall there).
    SendClimbing,
}

/// `TaskRoomClimb`.
#[derive(Clone, Debug, Default)]
pub struct ClimbTask {
    hands: [HandState; 2],
    anchored_hand: Option<usize>,
    gravity_disabled: bool,
    anchored_this_tick: bool,
}

impl ClimbTask {
    pub fn anchored_hand(&self) -> Option<usize> {
        self.anchored_hand
    }

    /// `isGrabbed()`.
    pub fn is_grabbed(&self) -> bool {
        self.hands[0].anchored || self.hands[1].anchored
    }

    /// `isGrabbed(hand)`.
    pub fn is_grabbed_by(&self, hand: usize) -> bool {
        self.hands[hand].anchored
    }

    /// `onClear`: gravity comes back.
    pub fn clear(&mut self) -> Vec<ClimbEffect> {
        for h in &mut self.hands {
            h.anchored = false;
        }
        self.anchored_hand = None;
        if std::mem::take(&mut self.gravity_disabled) {
            vec![ClimbEffect::NoGravity(false)]
        } else {
            Vec::new()
        }
    }

    /// `onRun`. `world` is the tick pose (hands in the world), `room` the room pose; `head_pivot_y`
    /// and `hmd_dir` are the head's pivot height in the room and its direction in the world.
    pub fn tick(
        &mut self,
        body: ClimbBody,
        world: &VrData,
        room: &VrData,
        level: &mut impl ClimbWorld,
    ) -> Vec<ClimbEffect> {
        let mut out = Vec::new();
        self.anchored_this_tick = false;
        for h in &mut self.hands {
            h.inside = false;
        }
        let mut hand_positions = [DVec3::ZERO; 2];
        for hand in 0..2 {
            let pose = world.controller(hand);
            let position = pose.position();
            let direction = pose.direction().as_dvec3();
            hand_positions[hand] = position;
            let start = position - direction * HAND_DIRECTION_OFFSET;
            let hand_box = Aabb {
                min: position.min(start),
                max: position.max(start),
            };
            let mut at = block_pos(position);
            let mut block = level.block(at);
            self.hands[hand].hand_shape = block.shape;
            let mut climbable = is_climbable(&block);
            if !climbable {
                let at_start = block_pos(start);
                let block_start = level.block(at_start);
                if is_climbable(&block_start) && block_start.shape.is_some() {
                    at = at_start;
                    hand_positions[hand] = start;
                    climbable = true;
                    self.hands[hand].hand_shape = block_start.shape;
                    block = block_start;
                }
            }
            self.update_hand(
                hand,
                at,
                &block,
                &hand_box,
                &hand_positions,
                climbable,
                body,
                room,
                level,
                &mut out,
            );
        }
        let mut anchored = self.is_grabbed();
        if !anchored {
            self.handle_not_anchored(&hand_positions, room, &mut out);
            anchored = self.is_grabbed();
        }
        if !anchored {
            self.anchored_hand = None;
            if std::mem::take(&mut self.gravity_disabled) {
                out.push(ClimbEffect::NoGravity(false));
            }
            return out;
        }
        if !self.gravity_disabled {
            out.push(ClimbEffect::NoGravity(true));
            self.gravity_disabled = true;
        }
        self.apply_anchor(body, world, room, level, &mut out);
        out
    }

    /// `handleNotAnchored`: a hand already inside a face holds it.
    fn handle_not_anchored(
        &mut self,
        hand_positions: &[DVec3; 2],
        room: &VrData,
        out: &mut Vec<ClimbEffect>,
    ) {
        for hand in 0..2 {
            if !self.hands[hand].inside {
                continue;
            }
            self.anchored_this_tick = true;
            let s = &mut self.hands[hand];
            s.anchored_pos = hand_positions[hand];
            s.anchored_pos_room = room.controller(hand).position_f().as_dvec3();
            self.anchored_hand = Some(hand);
            s.anchored_shape = s.hand_shape;
            s.anchored = true;
            out.push(ClimbEffect::Haptic(hand));
            return;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn update_hand(
        &mut self,
        hand: usize,
        at: [i32; 3],
        block: &ClimbBlock,
        hand_box: &Aabb,
        hand_positions: &[DVec3; 2],
        climbable: bool,
        _body: ClimbBody,
        room: &VrData,
        level: &mut impl ClimbWorld,
        out: &mut Vec<ClimbEffect>,
    ) {
        if climbable {
            self.update_anchor_face(hand, at, block, hand_box, level);
        } else {
            let s = &mut self.hands[hand];
            let distance = (s.anchored_pos - hand_positions[hand]).length();
            if distance > HAND_DISTANCE_THRESHOLD {
                s.inside = false;
            } else {
                let held = level.block(block_pos(s.anchored_pos));
                s.inside = s.was_inside && is_climbable(&held);
            }
        }
        let s = &mut self.hands[hand];
        // Detach if the hand leaves the climbable block.
        if !s.inside && s.anchored {
            s.anchored = false;
        }
        // A hand that has just entered a climbable area is the new anchor.
        let just_in = !s.was_inside && s.inside;
        let can_anchor = !s.anchored && s.inside;
        if !self.anchored_this_tick && can_anchor && just_in {
            self.anchored_this_tick = true;
            let s = &mut self.hands[hand];
            s.anchored_pos = hand_positions[hand];
            s.anchored_pos_room = room.controller(hand).position_f().as_dvec3();
            s.anchored_shape = s.hand_shape;
            s.anchored = true;
            self.anchored_hand = Some(hand);
            // Detach the other hand.
            self.hands[1 - hand].anchored = false;
            out.push(ClimbEffect::Haptic(hand));
            out.push(ClimbEffect::StepSound(self.hands[hand].anchored_pos));
        }
        let s = &mut self.hands[hand];
        s.was_inside = s.inside;
    }

    /// `updateAnchorFace`.
    fn update_anchor_face(
        &mut self,
        hand: usize,
        at: [i32; 3],
        block: &ClimbBlock,
        hand_box: &Aabb,
        level: &mut impl ClimbWorld,
    ) {
        let s = &mut self.hands[hand];
        if let Some(face) = is_ladder(block) {
            if intersects(hand_box, &moved(&wall_slab(face), at)) {
                s.inside = true;
                s.anchor_face_set(face);
            }
            return;
        }
        if let Climbable::Vine {
            north,
            east,
            south,
            west,
        } = block.climbable
        {
            s.hand_shape = Some(Aabb {
                min: DVec3::ZERO,
                max: DVec3::ONE,
            });
            // The order of the Java: east, west, north, south; each flag already means the face
            // has a block that occludes behind it. The face is on the side the flag names, the slab
            // the one of the opposite `facing`.
            for (flag, facing) in [
                (east, Side::West),
                (west, Side::East),
                (north, Side::South),
                (south, Side::North),
            ] {
                if flag && intersects(hand_box, &moved(&wall_slab(facing), at)) {
                    s.inside = true;
                    s.anchor_face_set(facing);
                    return;
                }
            }
        }
        let _ = level;
    }

    /// `applyAnchorState`.
    fn apply_anchor(
        &mut self,
        body: ClimbBody,
        world: &VrData,
        room: &VrData,
        level: &mut impl ClimbWorld,
        out: &mut Vec<ClimbEffect>,
    ) {
        let Some(hand) = self.anchored_hand else {
            return;
        };
        let state = self.hands[hand];
        let hand_tick = world.controller(hand).position();
        let anchor_tick = world.room_to_world(state.anchored_pos_room.as_vec3());
        let delta = hand_tick - anchor_tick;
        self.hands[hand].anchored_pos_room = room.controller(hand).position_f().as_dvec3();
        // Horizontal movement is kept when an anchor was made this tick.
        out.push(ClimbEffect::SetDelta(DVec3::new(
            if self.anchored_this_tick {
                body.delta.x
            } else {
                0.0
            },
            0.0,
            if self.anchored_this_tick {
                body.delta.z
            } else {
                0.0
            },
        )));
        out.push(ClimbEffect::ResetFall);
        let (px, py, pz) = (body.pos.x, body.pos.y, body.pos.z);
        let (mut nx, mut ny, mut nz) = (px, py - delta.y, pz);
        let block = block_pos(state.anchored_pos);
        let squash = 1.0 - body.world_scale.min(1.0);
        match state.face {
            Some(face @ (Side::North | Side::South)) => {
                nx = px - delta.x;
                nz = f64::from(block[2]) + 0.5;
                nz += squash * if face == Side::North { 0.5 } else { -0.5 };
            }
            Some(face @ (Side::East | Side::West)) => {
                nz = pz - delta.z;
                nx = f64::from(block[0]) + 0.5;
                nx += squash * if face == Side::West { 0.5 } else { -0.5 };
            }
            None => {}
        }
        let head_pivot_y = {
            let hmd = room.hmd();
            f64::from(hmd.position_f().y)
                + f64::from((hmd.orientation() * Vec3::new(0.0, -0.1, 0.1)).y)
        };
        let hand_room_y = f64::from(room.controller(hand).position_f().y);
        let mut pos_now = body.pos;
        // A low hand over the block's top: the player is pushed out over the ledge.
        if let Some(shape) = state.anchored_shape
            && hand_room_y <= head_pivot_y / 2.0
            && state.anchored_pos.y > shape.max.y * TOP_OF_BLOCK_FRACTION + f64::from(block[1])
        {
            let hmd_dir = world.hmd().direction().as_dvec3();
            let flat = (hmd_dir * 0.1 * DVec3::new(1.0, 0.0, 0.0)).normalize_or_zero() * 0.1;
            let top = shape.max.y + f64::from(block[1]);
            let at = body.bb_at(body.pos + DVec3::new(flat.x, top - body.pos.y, flat.z));
            if level.no_collision(at) {
                nx = body.pos.x + flat.x;
                ny = top;
                nz = body.pos.z + flat.z;
                self.anchored_hand = None;
                for h in &mut self.hands {
                    h.anchored = false;
                    h.was_inside = false;
                }
                // `player.setNoGravity(false)`; `gravityDisabled` stays as the Java leaves it.
                out.push(ClimbEffect::NoGravity(false));
            }
        }
        // Resolve collision with the new coordinates.
        let free = self.try_resolve(body, DVec3::new(nx, ny, nz), level, &mut pos_now);
        out.push(ClimbEffect::SetPos(if free { pos_now } else { body.pos }));
        out.push(ClimbEffect::SendClimbing);
    }

    /// `tryResolveCollision`: the new place, else one or two axes of the old.
    fn try_resolve(
        &self,
        body: ClimbBody,
        new: DVec3,
        level: &mut impl ClimbWorld,
        result: &mut DVec3,
    ) -> bool {
        if level.no_collision(body.bb_at(new)) {
            *result = new;
            return true;
        }
        for retreat in AXIS_RETREATS {
            let at = DVec3::new(
                if retreat & AXIS_X != 0 {
                    body.pos.x
                } else {
                    new.x
                },
                if retreat & AXIS_Y != 0 {
                    body.pos.y
                } else {
                    new.y
                },
                if retreat & AXIS_Z != 0 {
                    body.pos.z
                } else {
                    new.z
                },
            );
            if level.no_collision(body.bb_at(at)) {
                *result = at;
                return true;
            }
        }
        false
    }
}

impl HandState {
    fn anchor_face_set(&mut self, face: Side) {
        self.face = Some(face);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_vivecraft::vr_data::{DataSettings, RoomPose, RoomPoses};

    /// A ladder at (5, 0..10, 5) with its wall to the north (z = 5, the `facing` south).
    struct Wall {
        solid_from_y: Option<f64>,
    }
    impl ClimbWorld for Wall {
        fn block(&self, pos: [i32; 3]) -> ClimbBlock {
            if pos[0] == 5 && pos[2] == 5 && (0..10).contains(&pos[1]) {
                ClimbBlock {
                    climbable: Climbable::Facing(Side::South),
                    shape: Some(Aabb {
                        min: DVec3::new(0.0, 0.0, 0.8),
                        max: DVec3::new(1.0, 1.0, 1.0),
                    }),
                    id: "minecraft:ladder".into(),
                }
            } else {
                ClimbBlock::default()
            }
        }
        fn no_collision(&mut self, bb: Aabb) -> bool {
            self.solid_from_y.is_none_or(|y| bb.min.y < y)
        }
    }

    fn data(hand: Vec3, head: Vec3) -> VrData {
        let pose = |pos| RoomPose {
            pos,
            rot: glam::Quat::IDENTITY,
        };
        let room = RoomPoses {
            hmd: pose(head),
            eyes: [pose(head), pose(head)],
            aim: [pose(hand), pose(Vec3::new(9.0, 9.0, 9.0))],
            hand: [pose(hand), pose(Vec3::new(9.0, 9.0, 9.0))],
            tracking: [true, true],
        };
        VrData::new(&room, DataSettings::default(), DVec3::ZERO, 1.0, 1.0, 0.0)
    }

    fn body(y: f64) -> ClimbBody {
        ClimbBody {
            pos: DVec3::new(5.5, y, 4.0),
            delta: DVec3::new(0.0, -0.3, 0.0),
            half_width: 0.3,
            height: 1.8,
            world_scale: 1.0,
        }
    }

    #[test]
    fn the_grab_slab_is_the_walls_plane_thickened_and_inset() {
        // A ladder facing south has its wall to the north: z in [-0.1, 0.1].
        let south = wall_slab(Side::South);
        assert_eq!((south.min.z, south.max.z), (-0.1, 0.1));
        assert_eq!((south.min.x, south.max.x), (0.1, 0.9));
        // Facing west: the wall is east, x in [0.9, 1.1].
        let west = wall_slab(Side::West);
        assert_eq!((west.min.x, west.max.x), (0.9, 1.1));
        assert_eq!((west.min.z, west.max.z), (0.1, 0.9));
        let north = wall_slab(Side::North);
        assert_eq!((north.min.z, north.max.z), (0.9, 1.1));
        let east = wall_slab(Side::East);
        assert_eq!((east.min.x, east.max.x), (-0.1, 0.1));
    }

    #[test]
    fn a_hand_in_the_slab_grabs_and_a_pull_moves_the_player_the_other_way() {
        let mut task = ClimbTask::default();
        let mut level = Wall { solid_from_y: None };
        // The room is the world here: the hand at the ladder's wall plane (z = 5.05), the player
        // 1 m in front of it. The ladder is the block at z = 5 and faces south (+z).
        let first = task.tick(
            body(3.0),
            &data(Vec3::new(5.5, 4.5, 5.05), Vec3::new(5.5, 4.6, 4.0)),
            &data(Vec3::new(5.5, 4.5, 5.05), Vec3::new(5.5, 4.6, 4.0)),
            &mut level,
        );
        assert!(task.is_grabbed_by(0));
        assert!(first.contains(&ClimbEffect::NoGravity(true)));
        assert!(first.contains(&ClimbEffect::Haptic(0)));
        assert!(first.contains(&ClimbEffect::SendClimbing));
        assert!(first.contains(&ClimbEffect::ResetFall));
        // The player is put against the face's block (z centre 5.5 for the south face).
        let pos = first.iter().find_map(|e| match e {
            ClimbEffect::SetPos(p) => Some(*p),
            _ => None,
        });
        assert!((pos.unwrap().z - 5.5).abs() < 1e-9, "{pos:?}");
        // The hand then moves 0.1 m up in the world (the player pulls): the body goes 0.1 m down.
        let up = Vec3::new(5.5, 4.6, 5.05);
        let second = task.tick(
            body(3.0),
            &data(up, Vec3::new(5.5, 4.6, 4.0)),
            &data(up, Vec3::new(5.5, 4.6, 4.0)),
            &mut level,
        );
        let pos = second.iter().find_map(|e| match e {
            ClimbEffect::SetPos(p) => Some(*p),
            _ => None,
        });
        assert!((pos.unwrap().y - 2.9).abs() < 1e-6, "{pos:?}");
    }

    #[test]
    fn a_hand_that_leaves_lets_go_and_gravity_comes_back() {
        let mut task = ClimbTask::default();
        let mut level = Wall { solid_from_y: None };
        let near = data(Vec3::new(5.5, 4.5, 5.05), Vec3::new(5.5, 4.6, 4.0));
        task.tick(body(3.0), &near, &near, &mut level);
        assert!(task.is_grabbed());
        let away = data(Vec3::new(2.0, 4.0, 2.0), Vec3::new(5.5, 4.6, 4.0));
        let out = task.tick(body(3.0), &away, &away, &mut level);
        assert!(!task.is_grabbed());
        assert!(out.contains(&ClimbEffect::NoGravity(false)));
        assert_eq!(task.anchored_hand(), None);
    }

    #[test]
    fn a_blocked_pull_retreats_axis_by_axis_and_gives_up_when_boxed_in() {
        let mut task = ClimbTask::default();
        // Solid from y = 3.0 up: a body that would move down is free, up is not. A pull that
        // lowers the body works; with the floor at once everything collides, the body stays.
        let mut level = Wall {
            solid_from_y: Some(3.05),
        };
        let near = data(Vec3::new(5.5, 4.5, 5.05), Vec3::new(5.5, 4.6, 4.0));
        task.tick(body(3.0), &near, &near, &mut level);
        let mut closed = Wall {
            solid_from_y: Some(-100.0),
        };
        let up = data(Vec3::new(5.5, 4.6, 5.05), Vec3::new(5.5, 4.6, 4.0));
        let out = task.tick(body(3.0), &up, &up, &mut closed);
        let pos = out.iter().find_map(|e| match e {
            ClimbEffect::SetPos(p) => Some(*p),
            _ => None,
        });
        assert_eq!(pos, Some(DVec3::new(5.5, 3.0, 4.0)), "boxed in: stays put");
    }

    #[test]
    fn only_ladders_and_vines_are_grabbed() {
        let scaffold = ClimbBlock {
            climbable: Climbable::Other,
            shape: None,
            id: "minecraft:scaffolding".into(),
        };
        assert!(!is_climbable(&scaffold));
        let trapdoor = ClimbBlock {
            climbable: Climbable::Facing(Side::North),
            shape: None,
            id: "minecraft:oak_trapdoor".into(),
        };
        assert!(!is_climbable(&trapdoor));
        let vine = ClimbBlock {
            climbable: Climbable::Vine {
                north: true,
                east: false,
                south: false,
                west: false,
            },
            shape: None,
            id: "minecraft:vine".into(),
        };
        assert!(is_climbable(&vine));
    }
}
