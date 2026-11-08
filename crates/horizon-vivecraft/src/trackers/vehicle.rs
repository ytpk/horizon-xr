// SPDX-License-Identifier: LGPL-3.0-or-later
//! Ports `VehicleTracker` (turning the world with the vehicle you ride, the steering direction
//! of mounts, the roomscale dismount gate) and the unused `HorseTracker`'s role (Vivecraft
//! never activates it: `isActive` starts with `if (true) return false`, so nothing is ported
//! of its reins). Copyright (c) Vivecraft contributors; LGPL-3.0-or-later.

use crate::settings::VrSettings;
use crate::vr_data::VrData;
use glam::Vec3;

/// The vehicle kinds the tracker tells apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VehicleKind {
    /// Horses, donkeys, mules, camels, (skeleton/zombie) horses: `AbstractHorse`.
    Horse,
    /// `AbstractBoat`.
    Boat,
    /// Pigs, striders and other `Mob`s (not a happy ghast) the client moves.
    Mob,
    Minecart,
    /// Happy ghasts and anything else: rotation follows the server's.
    Other,
}

/// `rotDiff_Degrees`: the signed shortest turn from `start` to `end`, in degrees.
pub fn rot_diff_degrees(start: f32, end: f32) -> f32 {
    let mut d = (end - start).rem_euclid(360.0);
    if d > 180.0 {
        d -= 360.0;
    }
    d
}

/// What the game tells the tracker about the vehicle each tick.
#[derive(Clone, Copy, Debug)]
pub struct VehicleView {
    pub kind: VehicleKind,
    pub yaw: f32,
    /// A mob/horse's body yaw (`yBodyRot`).
    pub body_yaw: f32,
    /// The client moves the mob/horse (`isLocalInstanceAuthoritative`).
    pub authoritative: bool,
    /// A horse is saddled.
    pub saddled: bool,
    /// A minecart's movement this tick (`pos - posOld`) and delta movement, horizontal.
    pub cart_moved: Vec3,
    pub cart_speed: f32,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct VehicleTracker {
    vehicle_initial_rotation: f32,
    pub rotation_cooldown: u32,
    pub dismount_cooldown: u32,
    minecart_counter: u32,
    riding: bool,
}

impl VehicleTracker {
    pub fn is_riding(&self) -> bool {
        self.riding
    }

    /// `isActive`: alive and in a game.
    pub fn is_active(alive: bool, in_game: bool) -> bool {
        alive && in_game
    }

    pub fn inactive(&mut self) {
        self.minecart_counter = 2;
        self.riding = false;
    }

    /// `onStartRiding`: returns how many degrees `worldRotation` is turned so the player faces
    /// along the vehicle (none for minecarts, which have no front).
    pub fn on_start_riding(
        &mut self,
        s: &VrSettings,
        vehicle_yaw: f32,
        minecart: bool,
        hmd_yaw: f32,
        world_rotation: f32,
    ) -> f32 {
        self.riding = true;
        self.dismount_cooldown = 5;
        if !s.bool("vehicleRotation") {
            return 0.0;
        }
        self.vehicle_initial_rotation = world_rotation;
        self.rotation_cooldown = 2;
        if minecart {
            return 0.0;
        }
        rot_diff_degrees(vehicle_yaw % 360.0, hmd_yaw)
    }

    /// `onStopRiding`: the swing tracker is disabled for 10 ticks and the sneak counter reset
    /// by the caller.
    pub fn on_stop_riding(&mut self) {
        self.riding = false;
    }

    /// `activeProcess` (one logic tick, not paused): the degrees `worldRotation` is turned by
    /// to follow the vehicle. `riding_vehicle` is None when the player rides nothing.
    pub fn tick(&mut self, s: &VrSettings, vehicle: Option<VehicleView>) -> f32 {
        self.dismount_cooldown = self.dismount_cooldown.saturating_sub(1);
        self.rotation_cooldown = self.rotation_cooldown.saturating_sub(1);
        let Some(v) = vehicle else {
            self.minecart_counter = 3;
            return 0.0;
        };
        if !(s.bool("vehicleRotation") && self.rotation_cooldown == 0) {
            self.minecart_counter = 3;
            self.vehicle_initial_rotation = v.yaw;
            return 0.0;
        }
        let mut target = v.yaw;
        match v.kind {
            VehicleKind::Horse => {
                if v.authoritative && v.saddled {
                    return 0.0;
                }
                target = v.body_yaw;
            }
            VehicleKind::Mob => {
                if v.authoritative {
                    return 0.0;
                }
                target = v.body_yaw;
            }
            _ => {}
        }
        let mut increment = 10.0f32;
        if v.kind == VehicleKind::Minecart {
            let turning = v.cart_moved.length() > 0.001;
            if turning {
                self.minecart_counter = self.minecart_counter.saturating_sub(1);
            } else {
                self.minecart_counter = 3;
            }
            target = if turning {
                -180.0 + (-v.cart_moved.x).atan2(v.cart_moved.z).to_degrees()
            } else {
                self.vehicle_initial_rotation
            };
            if self.minecart_counter > 0 {
                self.vehicle_initial_rotation = target;
            }
            increment = (200.0 * v.cart_speed * v.cart_speed).max(10.0);
        }
        let diff =
            rot_diff_degrees(target, self.vehicle_initial_rotation).clamp(-increment, increment);
        self.vehicle_initial_rotation = (self.vehicle_initial_rotation - diff) % 360.0;
        diff
    }

    /// `canRoomscaleDismount`: standing still on a vehicle that rests on the ground.
    pub fn can_roomscale_dismount(
        &self,
        moving: bool,
        passenger: bool,
        vehicle_on_ground: bool,
    ) -> bool {
        !moving && passenger && vehicle_on_ground && self.dismount_cooldown == 0
    }
}

/// `getSteeringDirection` for a horse or a boat (with forward held): the free-move direction;
/// for a mob the client moves with a food stick in a hand: from the vehicle toward the stick's
/// controller; others get the free-move direction too. None: leave the vehicle alone.
pub fn steering_direction(
    s: &VrSettings,
    world: &VrData,
    kind: VehicleKind,
    forward_held: bool,
    authoritative: bool,
    food_stick_controller: Option<usize>,
    vehicle_position: Vec3,
) -> Option<Vec3> {
    let free_move = || {
        use crate::modes::FreeMove;
        match s.free_move_mode(false) {
            FreeMove::Hmd => world.hmd().direction(),
            FreeMove::Waist => {
                let yaw = world.body_yaw_rad();
                Vec3::new(0.0, 0.0, 1.0)
                    .rotate_x_vr(world.hmd().pitch_rad())
                    .rotate_y_vr(-yaw)
            }
            _ => world.controller(0).direction(),
        }
    };
    match kind {
        VehicleKind::Horse | VehicleKind::Boat => forward_held.then(free_move),
        _ if authoritative => match (kind, food_stick_controller) {
            (VehicleKind::Mob, Some(c)) => {
                let con = world.controller(c);
                Some(
                    ((con.position_f() - vehicle_position) + con.direction() * 0.3)
                        .normalize_or_zero(),
                )
            }
            _ => Some(free_move()),
        },
        _ => None,
    }
}

/// Small helpers for the Waist mode, in Vivecraft's `Vector3f.rotateX/rotateY` conventions.
trait VrRotate {
    fn rotate_x_vr(self, pitch: f32) -> Vec3;
    fn rotate_y_vr(self, angle: f32) -> Vec3;
}

impl VrRotate for Vec3 {
    fn rotate_x_vr(self, pitch: f32) -> Vec3 {
        // JOML rotateX(-pitch)
        let (sin, cos) = (-pitch).sin_cos();
        Vec3::new(
            self.x,
            self.y * cos - self.z * sin,
            self.y * sin + self.z * cos,
        )
    }
    fn rotate_y_vr(self, angle: f32) -> Vec3 {
        // JOML rotateY(angle)
        let (sin, cos) = angle.sin_cos();
        Vec3::new(
            self.x * cos + self.z * sin,
            self.y,
            -self.x * sin + self.z * cos,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(kind: VehicleKind, yaw: f32) -> VehicleView {
        VehicleView {
            kind,
            yaw,
            body_yaw: yaw,
            authoritative: false,
            saddled: false,
            cart_moved: Vec3::ZERO,
            cart_speed: 0.0,
        }
    }

    #[test]
    fn rot_diff_takes_the_short_way() {
        assert_eq!(rot_diff_degrees(350.0, 10.0), 20.0);
        assert_eq!(rot_diff_degrees(10.0, 350.0), -20.0);
    }

    #[test]
    fn the_world_follows_a_turning_vehicle_in_steps_of_ten_degrees() {
        let s = VrSettings::default();
        let mut t = VehicleTracker::default();
        t.on_start_riding(&s, 0.0, false, 0.0, 0.0);
        t.rotation_cooldown = 0;
        // Settles on the first yaw it sees, then the vehicle swings 90 degrees.
        assert_eq!(t.tick(&s, Some(view(VehicleKind::Other, 0.0))), 0.0);
        let d = t.tick(&s, Some(view(VehicleKind::Other, 90.0)));
        assert!((d - (-10.0)).abs() < 1e-4 || (d - 10.0).abs() < 1e-4, "{d}");
        // It never exceeds the step, and a mob the client moves is left alone.
        let mut m = view(VehicleKind::Mob, 45.0);
        m.authoritative = true;
        assert_eq!(t.tick(&s, Some(m)), 0.0);
    }

    #[test]
    fn dismount_waits_for_the_cooldown() {
        let s = VrSettings::default();
        let mut t = VehicleTracker::default();
        t.on_start_riding(&s, 0.0, true, 0.0, 0.0);
        assert!(!t.can_roomscale_dismount(false, true, true));
        for _ in 0..5 {
            t.tick(&s, None);
        }
        assert!(t.can_roomscale_dismount(false, true, true));
        assert!(!t.can_roomscale_dismount(true, true, true));
    }
}
