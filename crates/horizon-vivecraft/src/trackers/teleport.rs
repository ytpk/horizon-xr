// SPDX-License-Identifier: LGPL-3.0-or-later
//! Ports `TeleportTracker` and `VRMovementStyle`'s default style, `Arc` (hold the binding, aim
//! the arc, release to teleport). The other styles (Minimal, Beam, Tunnel, Grapple) are only
//! reachable by a debug hotkey in Vivecraft and are not ported.
//! Copyright (c) Vivecraft contributors; LGPL-3.0-or-later (see LICENSE-LGPL-3.0).

use super::PlayerFlags;
use crate::crosshair::Face;
use crate::player::Aabb;
use crate::vr_data::VrData;
use glam::{DVec3, Mat3, Vec3};

/// A block hit by the arc.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlockHit {
    pub pos: [i32; 3],
    pub location: DVec3,
    pub face: Face,
}

/// What the teleport tracker asks of the world.
pub trait TeleportWorld {
    /// `level.clip(ClipContext(from, to, COLLIDER, ANY fluid))`.
    fn clip(&mut self, from: DVec3, to: DVec3) -> Option<BlockHit>;
    /// `level.noCollision(player, bb)`.
    fn no_collision(&mut self, bb: Aabb) -> bool;
    /// The block at `pos` has a fluid.
    fn has_fluid(&self, pos: [i32; 3]) -> bool;
    /// A ladder, a vine or a `VIVECRAFT_CLIMBABLE` block.
    fn is_climbable(&self, pos: [i32; 3]) -> bool;
    /// The block's collision shape: its top (max Y within the block), or `None` if empty.
    fn collision_top(&self, pos: [i32; 3]) -> Option<f64>;
    /// Soul sand or a honey block (they sink the feet).
    fn is_sinking(&self, pos: [i32; 3]) -> bool;
    /// The two blocks are the same block type (`block == blockState.getBlock()`).
    fn same_block(&self, a: [i32; 3], b: [i32; 3]) -> bool;
}

/// `ClientNetworking`'s teleport limits.
#[derive(Clone, Copy, Debug, Default)]
pub struct TeleportLimits {
    pub limited_survival: bool,
    pub down: i32,
    pub up: i32,
    pub horiz: i32,
}

/// The player for the destination checks.
#[derive(Clone, Copy, Debug)]
pub struct TeleportPlayer {
    /// The feet (`getX`, `minY`, `getZ`).
    pub pos: DVec3,
    pub half_width: f64,
    pub height: f64,
    pub shift_down: bool,
    pub may_fly: bool,
    /// `getMuhJumpFactor` and `getMuhSpeedFactor`.
    pub jump_factor: f64,
    pub speed_factor: f64,
}

impl TeleportPlayer {
    fn bb_at(&self, feet: DVec3) -> Aabb {
        Aabb {
            min: DVec3::new(feet.x - self.half_width, feet.y, feet.z - self.half_width),
            max: DVec3::new(
                feet.x + self.half_width,
                feet.y + self.height,
                feet.z + self.half_width,
            ),
        }
    }
    fn bb(&self) -> Aabb {
        self.bb_at(self.pos)
    }
}

/// The inputs of a tick.
#[derive(Clone, Copy, Debug)]
pub struct TeleportInput {
    /// The teleport binding is held and teleporting is enabled (`keyTeleport.isDown() &&
    /// isTeleportEnabled()`).
    pub binding_down: bool,
    /// Seated, not free moving, and the move input is pushed (`seatedTeleport`).
    pub seated_teleport: bool,
}

/// How a teleport is carried out (`TeleportTracker.activeProcess`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Teleport {
    /// The server has no teleport permission for this client: send `/tp x y z`.
    Command(DVec3),
    /// Move the player; with a Vivecraft server that supports direct teleports, send the
    /// teleport packet with the next position (`vivecraft$setTeleported`).
    Direct { to: DVec3, direct_packet: bool },
}

const MAX_STEPS: usize = 50;

/// `TeleportTracker`.
#[derive(Clone, Debug)]
pub struct TeleportTracker {
    energy: f32,
    destination: DVec3,
    side_hit: Option<Face>,
    progress: f64,
    pub distance: f64,
    timer: i32,
    arc: Vec<DVec3>,
}

impl Default for TeleportTracker {
    fn default() -> Self {
        Self {
            energy: 0.0,
            destination: DVec3::ZERO,
            side_hit: None,
            progress: 0.0,
            distance: 0.0,
            timer: 0,
            arc: Vec::new(),
        }
    }
}

impl TeleportTracker {
    pub fn energy(&self) -> f32 {
        self.energy
    }
    pub fn is_aiming(&self) -> bool {
        self.progress > 0.0
    }
    pub fn destination(&self) -> DVec3 {
        self.destination
    }
    pub fn side_hit(&self) -> Option<Face> {
        self.side_hit
    }
    /// The arc to draw, from the controller to its end.
    pub fn arc(&self) -> &[DVec3] {
        &self.arc
    }

    pub fn is_active(p: &PlayerFlags) -> bool {
        p.in_game && p.alive && !p.sleeping
    }

    /// `inactiveProcess`.
    pub fn inactive(&mut self) {
        self.destination = DVec3::ZERO;
        self.arc.clear();
        self.progress = 0.0;
    }

    /// One logic tick (`activeProcess`, Arc style). `supported` is `isTeleportSupported` (the
    /// permission), `server_direct` `SERVER_SUPPORTS_DIRECT_TELEPORT`.
    pub fn tick(
        &mut self,
        input: TeleportInput,
        passenger: bool,
        supported: bool,
        server_direct: bool,
    ) -> Option<Teleport> {
        if self.energy < 100.0 {
            self.energy += 1.0;
        }
        let mut do_teleport = false;
        if (input.binding_down || input.seated_teleport) && !passenger {
            // Arc style: teleport on release; the progress is just "aiming".
            self.timer += 1;
            self.progress = 1.0;
        } else {
            if self.progress >= 1.0 {
                do_teleport = true;
            }
            self.timer = 0;
            // The destination is kept for the execution below, then the progress resets.
            let destination = self.destination;
            self.progress = 0.0;
            if do_teleport && destination != DVec3::ZERO {
                return Some(self.execute(destination, supported, server_direct));
            }
        }
        None
    }

    fn execute(&mut self, destination: DVec3, supported: bool, server_direct: bool) -> Teleport {
        // `doTeleportCallback`
        self.timer = -1;
        if !supported {
            Teleport::Command(destination)
        } else {
            Teleport::Direct {
                to: destination,
                direct_packet: server_direct,
            }
        }
    }

    /// `doTeleportCallback`'s energy use: after teleporting under the survival limits.
    pub fn spend_energy(&mut self, limits: &TeleportLimits, distance: f64) {
        self.distance = distance;
        if limits.limited_survival {
            self.energy -= distance as f32 * 4.0;
        }
    }

    /// `updateTeleportDestinations`, every frame: aims the arc from the controller while the
    /// binding is held. `controller` is 0 seated, else 1 (the off hand).
    pub fn update_destinations(
        &mut self,
        world_render: &VrData,
        seated: bool,
        player: &TeleportPlayer,
        limits: &TeleportLimits,
        world: &mut impl TeleportWorld,
    ) {
        self.destination = DVec3::ZERO;
        if self.progress > 0.0 {
            self.update_arc(
                world_render,
                usize::from(!seated),
                seated,
                player,
                limits,
                world,
            );
        }
    }

    fn update_arc(
        &mut self,
        data: &VrData,
        controller: usize,
        seated: bool,
        player: &TeleportPlayer,
        limits: &TeleportLimits,
        world: &mut impl TeleportWorld,
    ) {
        let pose = data.controller(controller);
        let start = pose.position();
        self.arc.clear();
        self.arc.push(start);
        // gravity: the controller's down axis, rolled, scaled
        let roll = pose.roll_rad();
        let m = Mat3::from_quat(pose.orientation()) * Mat3::from_rotation_z(roll);
        let gravity = m * Vec3::NEG_Y * 0.098;
        let mut velocity = pose.direction() * 0.5;
        let mut pos = start;
        let mut i = 1;
        while i < MAX_STEPS && (i * 4) as f32 <= self.energy {
            let new_pos = pos + velocity.as_dvec3();
            if let Some(hit) = world.clip(pos, new_pos) {
                self.arc.push(hit.location);
                self.check_and_set_destination(player, &hit, seated, limits, world);
                let diff = player.pos - self.destination;
                let y_diff = diff.y;
                self.distance = diff.length();
                let xz = (diff.x * diff.x + diff.z * diff.z).sqrt();
                let mut ok = !player.shift_down || y_diff <= 0.2;
                if !player.may_fly && limits.limited_survival {
                    let too_low = limits.down > 0 && y_diff > f64::from(limits.down) + 0.2;
                    let too_high =
                        limits.up > 0 && -y_diff > f64::from(limits.up) * player.jump_factor + 0.2;
                    let too_far = limits.horiz > 0
                        && xz > f64::from(limits.horiz) * player.speed_factor + 0.2;
                    if too_low || too_high || too_far {
                        ok = false;
                    }
                }
                if !ok {
                    self.destination = DVec3::ZERO;
                    self.distance = 0.0;
                }
                return;
            }
            pos = new_pos;
            self.arc.push(new_pos);
            velocity += gravity;
            i += 1;
        }
    }

    fn check_and_set_destination(
        &mut self,
        player: &TeleportPlayer,
        hit: &BlockHit,
        seated: bool,
        limits: &TeleportLimits,
        world: &mut impl TeleportWorld,
    ) -> bool {
        let [bx, by, bz] = hit.pos;
        if world.has_fluid(hit.pos) {
            let hit_vec = DVec3::new(hit.location.x, f64::from(by), hit.location.z);
            let feet = DVec3::new(player.pos.x, player.pos.y, player.pos.z);
            let mut bb = player.bb().moved(hit_vec - feet);
            let mut empty = world.no_collision(bb);
            if !empty {
                let center = DVec3::new(f64::from(bx) + 0.5, f64::from(by), f64::from(bz) + 0.5);
                bb = player.bb().moved(center - feet);
                empty = world.no_collision(bb);
            }
            let ex = if seated { 0.5 } else { 0.0 };
            if empty {
                self.destination = DVec3::new(
                    (bb.min.x + bb.max.x) * 0.5,
                    bb.min.y + ex,
                    (bb.min.z + bb.max.z) * 0.5,
                );
                self.side_hit = Some(hit.face);
                return true;
            }
        } else if hit.face != Face::Up {
            // Sides need the top of a block, unless it is climbable or the limits are off.
            if world.is_climbable(hit.pos) {
                let mut dest = DVec3::new(
                    f64::from(bx) + 0.5,
                    f64::from(by) + 0.5,
                    f64::from(bz) + 0.5,
                );
                if world.same_block([bx, by - 1, bz], hit.pos) {
                    dest.y -= 1.0;
                }
                self.destination = dest;
                self.side_hit = Some(hit.face);
                return true;
            }
            if !player.may_fly && limits.limited_survival {
                return false;
            }
        }
        let mut block = [bx, by - 1, bz];
        for _ in 0..2 {
            if let Some(height) = world.collision_top(block) {
                let hit_vec =
                    DVec3::new(hit.location.x, f64::from(block[1]) + height, hit.location.z);
                let offset = hit_vec - player.pos;
                let mut bb = player.bb().moved(offset);
                let ex = if world.is_sinking(block) { 0.05 } else { 0.0 };
                let grown = |bb: Aabb| Aabb {
                    min: bb.min - DVec3::new(0.0, 0.125 + ex, 0.0),
                    max: bb.max + DVec3::new(0.0, 0.125 + ex, 0.0),
                };
                let mut empty = world.no_collision(bb) && !world.no_collision(grown(bb));
                if !empty {
                    let center = DVec3::new(
                        f64::from(block[0]) + 0.5,
                        f64::from(block[1]) + height,
                        f64::from(block[2]) + 0.5,
                    );
                    bb = player.bb().moved(center - player.pos);
                    empty = world.no_collision(bb) && !world.no_collision(grown(bb));
                }
                if empty {
                    self.destination = DVec3::new(
                        (bb.min.x + bb.max.x) * 0.5,
                        f64::from(block[1]) + height,
                        (bb.min.z + bb.max.z) * 0.5,
                    );
                    return true;
                }
            }
            block[1] += 1;
        }
        false
    }

    /// `getInterpolatedArcPosition`: the point `progress` (0..1) along the arc.
    pub fn arc_position(&self, progress: f32) -> DVec3 {
        let n = self.arc.len();
        if n == 0 {
            return DVec3::ZERO;
        }
        if n == 1 || progress <= 0.0 {
            self.arc[0]
        } else if progress >= 1.0 {
            self.arc[n - 1]
        } else {
            let at = progress * (n - 1) as f32;
            let step = at.floor() as usize;
            let t = f64::from(at - step as f32);
            self.arc[step] + (self.arc[step + 1] - self.arc[step]) * t
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vr_data::{DataSettings, RoomPose, RoomPoses};
    use glam::Quat;

    /// A flat floor of full blocks below y = 64.
    struct Floor;
    impl TeleportWorld for Floor {
        fn clip(&mut self, from: DVec3, to: DVec3) -> Option<BlockHit> {
            if from.y >= 64.0 && to.y < 64.0 {
                let t = (from.y - 64.0) / (from.y - to.y);
                let at = from + (to - from) * t;
                return Some(BlockHit {
                    pos: [at.x.floor() as i32, 63, at.z.floor() as i32],
                    location: at,
                    face: Face::Up,
                });
            }
            None
        }
        fn no_collision(&mut self, bb: Aabb) -> bool {
            bb.min.y >= 64.0 - 1e-9
        }
        fn has_fluid(&self, _: [i32; 3]) -> bool {
            false
        }
        fn is_climbable(&self, _: [i32; 3]) -> bool {
            false
        }
        fn collision_top(&self, pos: [i32; 3]) -> Option<f64> {
            (pos[1] <= 63).then_some(1.0)
        }
        fn is_sinking(&self, _: [i32; 3]) -> bool {
            false
        }
        fn same_block(&self, _: [i32; 3], _: [i32; 3]) -> bool {
            false
        }
    }

    fn player() -> TeleportPlayer {
        TeleportPlayer {
            pos: DVec3::new(0.0, 64.0, 0.0),
            half_width: 0.3,
            height: 1.8,
            shift_down: false,
            may_fly: false,
            jump_factor: 1.0,
            speed_factor: 1.0,
        }
    }

    /// An off-hand controller at hip height pointing slightly up and forward (-Z).
    fn aiming(pitch_up: f32) -> VrData {
        let rot = Quat::from_rotation_x(pitch_up);
        let hand = RoomPose {
            pos: Vec3::new(-0.2, 1.2, -0.3),
            rot,
        };
        let head = RoomPose {
            pos: Vec3::new(0.0, 1.7, 0.0),
            rot: Quat::IDENTITY,
        };
        let room = RoomPoses {
            hmd: head,
            eyes: [head; 2],
            aim: [hand; 2],
            hand: [hand; 2],
            tracking: [true; 2],
        };
        VrData::new(
            &room,
            DataSettings::default(),
            DVec3::new(0.0, 64.0, 0.0),
            1.0,
            1.0,
            0.0,
        )
    }

    fn hold() -> TeleportInput {
        TeleportInput {
            binding_down: true,
            seated_teleport: false,
        }
    }
    fn release() -> TeleportInput {
        TeleportInput {
            binding_down: false,
            seated_teleport: false,
        }
    }

    #[test]
    fn the_arc_lands_on_the_floor_ahead_and_a_release_teleports_there() {
        let mut t = TeleportTracker::default();
        for _ in 0..100 {
            t.tick(release(), false, true, true);
        }
        assert_eq!(t.energy(), 100.0);
        let limits = TeleportLimits::default();
        t.tick(hold(), false, true, true);
        assert!(t.is_aiming());
        t.update_destinations(&aiming(0.4), false, &player(), &limits, &mut Floor);
        let dest = t.destination();
        assert!(dest != DVec3::ZERO, "arc {:?}", t.arc());
        assert!((dest.y - 64.0).abs() < 1e-6);
        assert!(dest.z < -0.5, "{dest}");
        assert!(t.arc().len() > 2);
        // Releasing teleports to the destination, with the direct packet on a supporting server.
        let tp = t.tick(release(), false, true, true);
        assert_eq!(
            tp,
            Some(Teleport::Direct {
                to: dest,
                direct_packet: true
            })
        );
        assert!(!t.is_aiming());
        // Without permission it is a command.
        t.tick(hold(), false, false, false);
        t.update_destinations(&aiming(0.4), false, &player(), &limits, &mut Floor);
        let d2 = t.destination();
        assert_eq!(
            t.tick(release(), false, false, false),
            Some(Teleport::Command(d2))
        );
    }

    #[test]
    fn a_passenger_cannot_aim_and_nothing_aimed_means_no_teleport() {
        let mut t = TeleportTracker::default();
        t.tick(hold(), true, true, true);
        assert!(!t.is_aiming());
        t.tick(hold(), false, true, true);
        // Pointing at the sky: no destination, so release does nothing.
        t.update_destinations(
            &aiming(1.2),
            false,
            &player(),
            &TeleportLimits::default(),
            &mut Floor,
        );
        assert_eq!(t.destination(), DVec3::ZERO);
        assert_eq!(t.tick(release(), false, true, true), None);
    }

    #[test]
    fn survival_limits_refuse_far_destinations_and_cost_energy() {
        let mut t = TeleportTracker::default();
        for _ in 0..100 {
            t.tick(release(), false, true, true);
        }
        let limits = TeleportLimits {
            limited_survival: true,
            down: 4,
            up: 1,
            horiz: 2,
        };
        t.tick(hold(), false, true, true);
        t.update_destinations(&aiming(0.4), false, &player(), &limits, &mut Floor);
        // The floor lands more than 2 m away horizontally.
        assert_eq!(t.destination(), DVec3::ZERO);
        // Creative flight ignores the limits.
        let flyer = TeleportPlayer {
            may_fly: true,
            ..player()
        };
        t.update_destinations(&aiming(0.4), false, &flyer, &limits, &mut Floor);
        assert!(t.destination() != DVec3::ZERO);
        // The energy only pays for the distance when limited.
        let before = t.energy();
        t.spend_energy(&limits, 3.0);
        assert_eq!(t.energy(), before - 12.0);
        t.spend_energy(&TeleportLimits::default(), 3.0);
        assert_eq!(t.energy(), before - 12.0);
    }

    #[test]
    fn the_arc_position_interpolates_along_its_points() {
        let t = TeleportTracker {
            arc: vec![
                DVec3::ZERO,
                DVec3::new(2.0, 0.0, 0.0),
                DVec3::new(2.0, 2.0, 0.0),
            ],
            ..Default::default()
        };
        assert_eq!(t.arc_position(0.25), DVec3::new(1.0, 0.0, 0.0));
        assert_eq!(t.arc_position(0.75), DVec3::new(2.0, 1.0, 0.0));
        assert_eq!(t.arc_position(2.0), DVec3::new(2.0, 2.0, 0.0));
    }
}
