// SPDX-License-Identifier: LGPL-3.0-or-later
//! The VR runtime of the game: Vivecraft's `VRPlayer` data and trackers
//! (`org.vivecraft.client_vr.gameplay`, ported in `horizon-vivecraft`) fed from the headset's
//! tracking and applied to the player. Copyright (c) Vivecraft contributors; LGPL-3.0-or-later.
//!
//! The room data (`room_pre`) comes straight from the runtime; the world data puts the room
//! where the game's camera has it: the room's rotation is the body's yaw (`body_yaw`), and its
//! origin is placed so the head rest position is under the player's eyes (the game walks the
//! player after the head, so this is Vivecraft's `snapRoomOriginToPlayerEntity` every frame).
use super::event_loop::EventLoopCtl;
use super::input_trace::InputOrigin;
use super::{Game, Hand};
use glam::{DVec3, Quat, Vec3};
use horizon_client::gamepad::{Button, PadState};
use horizon_client::headset::{self, RoomSnapshot};
use horizon_vivecraft::crosshair::Face;
use horizon_vivecraft::modes::FreeMove;
use horizon_vivecraft::network::{C2s, NetworkVersion};
use horizon_vivecraft::player::VrPlayer;
use horizon_vivecraft::player::{Aabb, RoomWalk, RoomWalkWorld, WalkingPlayer, room_walk_to};
use horizon_vivecraft::trackers::climb::{
    ClimbAction, ClimbBlock, ClimbInput, ClimbJump, ClimbPlayer, ClimbTracker, ClimbWorld,
    Climbable, Side,
};
use horizon_vivecraft::trackers::hands::{EatingTracker, UseAnimation};
use horizon_vivecraft::trackers::moves::{RunTracker, SwimTracker};
use horizon_vivecraft::trackers::scope::{BoatSteer, RowTracker, TelescopeTracker, steer_boat};
use horizon_vivecraft::trackers::swing::{
    BlockHit, BlockKind, EntityHit, HeldItem, SwingAction, SwingInput, SwingTracker, SwingWorld,
};
use horizon_vivecraft::trackers::teleport::{
    BlockHit as TeleportHit, Teleport, TeleportInput, TeleportLimits, TeleportPlayer,
    TeleportTracker, TeleportWorld,
};
use horizon_vivecraft::trackers::vehicle::{
    VehicleKind, VehicleTracker, VehicleView, steering_direction,
};
use horizon_vivecraft::trackers::{
    PlayerFlags,
    body::{JumpTracker, SneakTracker},
    hands::BackpackTracker,
};
use horizon_vivecraft::vr_data::{RoomPose, RoomPoses};
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::event::{DeviceId, ElementState, MouseButton, WindowEvent};

/// How long a tracker's button stays down so a tick sees it.
const JUMP_HOLD: Duration = Duration::from_millis(100);
const BACKPACK_HOLD: Duration = Duration::from_millis(120);

/// The state of the runtime between frames.
pub(super) struct VrRuntime {
    pub(super) player: VrPlayer,
    sneak: SneakTracker,
    jump: JumpTracker,
    room_jump: horizon_visor::tasks::movement::RoomJumpTask,
    last_room: Option<(u64, u64)>,
    backpack: BackpackTracker,
    pub(super) swing: SwingTracker,
    teleport: TeleportTracker,
    /// The teleport binding (the off trigger) is held and teleporting is on.
    teleport_binding: bool,
    eating: EatingTracker,
    scope: TelescopeTracker,
    swim: SwimTracker,
    run: RunTracker,
    /// `RowTracker`: the oar strengths while in a boat.
    row: RowTracker,
    vehicle: VehicleTracker,
    pub(super) keyboard: super::vr_keyboard::VrKeyboard,
    /// The health at the last tick, for the hit response.
    last_health: f32,
    pub(super) climb: ClimbTracker,
    climb_jump: ClimbJump,
    /// The swim tracker is a tick tracker: the time not yet run, in seconds.
    swim_time: f32,
    last_frame: Option<Instant>,
    jump_until: Option<Instant>,
    backpack_until: [Option<Instant>; 2],
    /// A jump was made and the head has not come back down.
    pub(super) jumped: bool,
    /// Visor's auto height: where the head rests (`HeightEstimator`), and whether the hint was said.
    height: horizon_visor::height::HeightEstimator,
    height_hint: bool,
    /// `TaskRoomSneak`'s state.
    visor_sneak: horizon_visor::tasks::movement::SneakTask,
}

impl Default for VrRuntime {
    fn default() -> Self {
        Self {
            height: horizon_visor::height::HeightEstimator::default(),
            height_hint: false,
            visor_sneak: Default::default(),
            player: VrPlayer::new(&headset::settings()),
            sneak: SneakTracker::default(),
            jump: JumpTracker,
            room_jump: Default::default(),
            last_room: None,
            backpack: BackpackTracker::default(),
            swing: SwingTracker::default(),
            teleport: TeleportTracker::default(),
            teleport_binding: false,
            eating: EatingTracker::default(),
            scope: TelescopeTracker::default(),
            swim: SwimTracker::default(),
            run: RunTracker::default(),
            row: RowTracker::default(),
            vehicle: VehicleTracker::default(),
            keyboard: Default::default(),
            last_health: 0.0,
            climb: ClimbTracker::default(),
            climb_jump: ClimbJump::default(),
            swim_time: 0.0,
            last_frame: None,
            jump_until: None,
            backpack_until: [None; 2],
            jumped: false,
        }
    }
}

/// `RoomPoses` from the runtime's snapshot; the main hand is the right unless the hands are
/// reversed.
fn room_poses(snap: &RoomSnapshot, left_handed: bool) -> RoomPoses {
    let pose = |p: (Vec3, Quat)| RoomPose { pos: p.0, rot: p.1 };
    let head = pose(snap.head);
    let main = usize::from(!left_handed);
    let order = [main, 1 - main];
    let pick = |poses: &[Option<(Vec3, Quat)>; 2]| {
        order.map(|i| poses[i].map_or(RoomPose::IDENTITY, pose))
    };
    RoomPoses {
        hmd: head,
        eyes: [head; 2],
        aim: pick(&snap.aim),
        hand: pick(&snap.grip),
        tracking: order.map(|i| snap.aim[i].is_some()),
    }
}

impl Game<'_> {
    /// The state of the player the trackers gate on.
    pub(super) fn vr_flags(&self) -> PlayerFlags {
        let Some(p) = self.player.as_ref() else {
            return PlayerFlags::default();
        };
        PlayerFlags {
            alive: p.health.health() > 0.0,
            on_ground: p.ground.on_ground,
            on_climbable: p.last_climbable.is_some(),
            in_water: p.fluids.was_touching_water,
            in_lava: p.is_in_lava(),
            shift_down: p.shift_key_down,
            passenger: p.is_passenger,
            sleeping: p.sleeping,
            spectator: p.spectator,
            in_game: true,
            paused_or_screen: self.pad_in_menu(),
        }
    }

    /// `VRPlayer.freemoveDirection`: the move stick's (left, forward) turned by the free-move
    /// mode (the off controller by default, the head, the room, the waist) from the game's own
    /// yaw to where the player is to go, so the stick moves the way Vivecraft's does.
    pub(super) fn vr_free_move_stick(&self, stick: Option<[f32; 2]>) -> Option<[f32; 2]> {
        let [left, forward] = stick?;
        let settings = headset::settings();
        let Some(player) = self.player.as_ref() else {
            return stick;
        };
        if !headset::active() || settings.seated() || self.screen != super::Screen::Playing {
            return stick;
        }
        let run = self.pad.headset.vr.run;
        let dir = self.pad.headset.vr.player.freemove_direction(
            &settings,
            true,
            (f64::from(left), f64::from(forward)),
            1.0,
            player.flying,
            player.swimming,
            player.is_passenger,
            false,
            !player.ground.on_ground,
            (run.yaw() as f32, run.speed()),
            1.0,
        );
        // The game's yaw frame: forward is (-sin, cos), left (cos, sin).
        let yaw = f64::from(self.yaw).to_radians();
        let (sin, cos) = yaw.sin_cos();
        let mut out = [
            (dir.x * cos + dir.z * sin) as f32,
            (dir.x * -sin + dir.z * cos) as f32,
        ];
        // The speed multipliers are the movement code's.
        let length = out[0].hypot(out[1]);
        if length > 1.0 {
            out = out.map(|v| v / length);
        }
        Some(out)
    }

    pub(super) fn vr_clear_room_tracking(&mut self) {
        if self.pad.headset.vr.last_room.take().is_none() {
            return;
        }
        let vr = &mut self.pad.headset.vr;
        vr.player.histories = Default::default();
        vr.height.reset();
        self.visor.swing.clear();
        vr.room_jump.clear();
        vr.visor_sneak.clear();
        vr.sneak.sneak_override = false;
        vr.sneak.sneak_counter = 0;
        vr.backpack_until = [None; 2];
        vr.teleport_binding = false;
        vr.teleport.inactive();
        vr.jump_until = None;
        vr.jumped = false;
        headset::set_tracker_pad(PadState::default());
        self.visor_room_sneak(false);
        for effect in self.visor.climb.clear() {
            if let horizon_visor::tasks::climb::ClimbEffect::NoGravity(on) = effect
                && let Some(player) = self.player.as_mut()
            {
                player.no_gravity = on;
            }
        }
        if self.visor.crawl.clear().is_some() && self.visor_active() {
            self.visor_send(&horizon_visor::payload::C2s::Crawling(false));
        }
        if let Some(player) = self.player.as_mut() {
            player.room_crawling = false;
        }
    }

    /// Once per loop iteration, standing in a headset: polls Vivecraft's player data and runs
    /// the body trackers (sneak, jump, backpack), whose presses go to the pad.
    pub(super) fn vr_update(&mut self) {
        let Some(snap) = headset::room() else {
            self.vr_clear_room_tracking();
            return;
        };
        if let Some((last, epoch)) = self.pad.headset.vr.last_room {
            if last == snap.time_ms && epoch == snap.pose_epoch {
                return;
            }
            if epoch != snap.pose_epoch
                || snap.time_ms < last
                || snap.time_ms - last > 250
                || (!self.pad.headset.vr.player.histories.hmd.is_empty()
                    && self
                        .pad
                        .headset
                        .vr
                        .player
                        .histories
                        .hmd
                        .latest()
                        .distance(snap.head.0)
                        > 0.5)
            {
                self.vr_clear_room_tracking();
            }
        }
        self.pad.headset.vr.last_room = Some((snap.time_ms, snap.pose_epoch));
        let settings = headset::settings();
        let room = room_poses(&snap, settings.left_handed());
        let vr = &mut self.pad.headset.vr;
        vr.player.poll(snap.time_ms, room, &settings);
        // Include the current valid pose in a manual measurement.
        if headset::take_calibrate_height() {
            self.vr_calibrate_height(snap.time_ms);
        }
        self.visor_height_tick(snap.time_ms, (snap.head.1 * Vec3::NEG_Z).y);
        let settings = headset::settings();
        // World data: the room turned as the body, the rest under the player's eyes.
        let Some(eye) = self.eye_position() else {
            headset::set_tracker_pad(PadState::default());
            return;
        };
        let rotation = std::f32::consts::PI - headset::body_yaw().to_radians();
        // `VRPlayer.preTick`'s world scale; the XR layer scales the eyes and hands by the same.
        let scale = settings.float("worldScale").clamp(0.025, 100.0);
        headset::set_world_scale(scale);
        // A device is at `origin + room * scale`; the rest is under the eyes, lifted by the
        // standing height the scale adds (`horizon-xr`'s `world_offset`).
        let lift = glam::Vec3::new(0.0, (scale - 1.0) * snap.rest.y, 0.0);
        let origin = DVec3::from_array(eye) + lift.as_dvec3()
            - (Quat::from_rotation_y(rotation) * snap.rest * scale).as_dvec3();
        let vr = &mut self.pad.headset.vr;
        vr.player.room_origin = origin;
        vr.player.world_rotation = rotation.to_degrees();
        vr.player.world_scale = scale;
        vr.player.pre_tick(&settings, 1.0, false);
        vr.player.world_render = None;

        let flags = self.vr_flags();
        let boots = self.vivecraft.climbey
            && self.vr_named(
                self.armor[3].as_ref(),
                "minecraft:leather_boots",
                "Jump Boots",
            );
        let playing = self.player.is_some() && !flags.paused_or_screen;
        let visor = self.visor_mechanics();
        let calibrated = !settings.bool("visorHeightAuto")
            || settings.float("manualCalibration") >= horizon_visor::height::MIN_HEIGHT;
        let body_free = !flags.spectator
            && !flags.sleeping
            && self
                .player
                .as_ref()
                .is_some_and(|p| !p.flying && !p.fall_flying && p.auto_spin_attack_dmg <= 0.0)
            && !self.visor.climb.is_grabbed();
        let mut room_sneak = false;
        let free_move = true;
        let now = Instant::now();
        // Visor: the movement stick pushed forward aims the teleport arc and letting go goes
        // (`GameActionMovement` hands the stick to `TaskTeleport` in the teleport mode).
        let stick_forward = headset::move_stick()[1];
        let direct = self.vivecraft.net.server_allows_direct_teleport;
        // `VRServerPerms.noTeleportClient`: a server that did not announce teleports has none.
        let no_teleport_client = !self.vivecraft.net.server_supports_direct_teleport;
        let override_on = self.vivecraft.teleport_override;
        // Visor: the server's supported movement and the player's own choice decide.
        let visor_teleport = self.visor_teleport_enabled();
        let vr = &mut self.pad.headset.vr;
        vr.player.set_teleport_override(override_on);
        let teleport_on = visor_teleport.unwrap_or_else(|| {
            vr.player
                .teleport_enabled(&settings, no_teleport_client, direct)
        });
        vr.teleport_binding = playing && teleport_on && stick_forward > 0.5;
        headset::set_stick_teleports(playing && teleport_on);
        let mut pad = PadState::default();
        if playing && (!visor || calibrated) {
            vr.sneak
                .tick(&settings, &flags, free_move, &vr.player.histories);
            // `TaskRoomSneak`: the head's pivot against the standing height, both in the room
            // above the floor, and never while crawling. It is held, not clicked: the stick's
            // click is a toggle, so a held click latched the sneak on after one dip.
            room_sneak = body_free
                && vr.sneak.is_active(&settings, &flags, free_move)
                && vr.visor_sneak.tick(
                    vr.player.histories.hmd_pivot.latest().y,
                    horizon_vivecraft::player::player_height(&settings),
                    settings.float("visorSneakThreshold"),
                    self.visor.crawl.crawling,
                );
            let rising = if visor {
                let h = &vr.player.histories.hmd_pivot;
                let active = vr.jump.is_active(&settings, &flags, free_move)
                    && body_free
                    && !self.visor.crawl.crawling;
                vr.room_jump.tick(
                    active,
                    snap.time_ms,
                    h.latest().y,
                    horizon_vivecraft::player::player_height(&settings),
                    settings.float("visorJumpThreshold"),
                    h.net_movement(snap.time_ms, 0.25).y,
                    h.net_movement(snap.time_ms, 0.1).y / 0.1,
                )
            } else {
                vr.jump.tick(
                    &settings,
                    &flags,
                    free_move,
                    &vr.player.histories,
                    snap.time_ms,
                    boots,
                )
            };
            // One jump per rise: the head must come back down before the next.
            let pivot = vr.player.histories.hmd_pivot.latest().y;
            if vr.jumped && pivot < horizon_vivecraft::player::player_height(&settings) + 0.025 {
                vr.jumped = false;
            }
            if rising && !vr.jumped {
                vr.jumped = true;
                vr.jump_until = Some(now + JUMP_HOLD);
            }
            if vr.jump_until.is_some_and(|until| now < until) {
                pad.set(Button::South, true);
            }
            if vr.backpack.is_active(&settings, &flags, false, false) {
                let hits = vr.backpack.tick(&vr.player.room_pre);
                for (c, hit) in hits.into_iter().enumerate() {
                    if let Some(h) = hit {
                        vr.backpack_until[c] = Some(now + BACKPACK_HOLD);
                        headset::pulse(c == 0, 0.8, h.micros as f32 / 1.0e6 * 50.0);
                    }
                }
            }
            // The off hand's swap key (the main hand's binding is the game's drop).
            let off = 1;
            if vr.backpack_until[off].is_some_and(|until| now < until) {
                pad.set(Button::West, true);
            }
        } else {
            vr.sneak.sneak_override = false;
            vr.visor_sneak.clear();
            vr.room_jump.clear();
            vr.jump_until = None;
            vr.jumped = false;
        }
        headset::set_tracker_pad(pad);
        self.visor_room_sneak(room_sneak);
        self.vr_keyboard_tick(snap.time_ms);
    }
}

impl RoomWalkWorld for GameTeleport<'_, '_> {
    fn no_collision(&mut self, bb: Aabb) -> bool {
        TeleportWorld::no_collision(self, bb)
    }
}

impl Game<'_> {
    /// `VRPlayer.doPlayerMoveInRoom` for the step the head walked (`step`, world metres): the
    /// player's feet go there when the box is free (no sliding along walls) or up a block
    /// (`walkUpBlocks`), and nothing happens while sneaking, asleep, jumping, on a ladder,
    /// dead or riding. Returns how far the player moved.
    pub(super) fn vr_room_walk(&mut self, step: Vec3) -> [f64; 3] {
        let settings = headset::settings();
        let Some(p) = self.player.as_ref() else {
            return [0.0; 3];
        };
        let vr = &self.pad.headset.vr;
        let pos = DVec3::new(p.position.x, p.position.y, p.position.z);
        let walking = WalkingPlayer {
            pos,
            half_width: 0.3,
            height: 1.8,
            shift_down: p.shift_key_down,
            sleeping: self.sleeping.is_some(),
            alive: p.health.health() > 0.0,
            riding: self.riding.is_some(),
            fall_distance_zero: p.fall_distance == 0.0,
            on_climbable: p.last_climbable.is_some(),
            jumping_tracker: vr.jumped,
            grabbing_ladder: vr.climb.is_grabbing_ladder() || self.visor.climb.is_grabbed(),
            jump_factor_one: true,
            dimensions_width: 0.6,
        };
        if walking.shift_down
            || walking.sleeping
            || walking.jumping_tracker
            || walking.grabbing_ladder
            || !walking.alive
            || walking.riding
        {
            return [0.0; 3];
        }
        let target = pos + DVec3::new(f64::from(step.x), 0.0, f64::from(step.z));
        let mut world = GameTeleport(self);
        let walk = room_walk_to(&settings, &walking, &mut world, false, target.x, target.z);
        let to = match walk {
            RoomWalk::Moved { pos, .. } | RoomWalk::Stepped { pos, .. } => pos,
            RoomWalk::None | RoomWalk::Dismount => return [0.0; 3],
        };
        let Some(p) = self.player.as_mut() else {
            return [0.0; 3];
        };
        p.position.x = to.x;
        p.position.y = to.y;
        p.position.z = to.z;
        [to.x - pos.x, to.y - pos.y, to.z - pos.z]
    }
}

/// What the swing tracker asks of the game's world.
pub(super) struct GameSwing<'a, 'b>(pub(super) &'a mut Game<'b>);

fn face_of(normal: [i32; 3]) -> Face {
    match normal {
        [0, -1, 0] => Face::Down,
        [0, 1, 0] => Face::Up,
        [0, 0, -1] => Face::North,
        [0, 0, 1] => Face::South,
        [-1, 0, 0] => Face::West,
        _ => Face::East,
    }
}

impl SwingWorld for GameSwing<'_, '_> {
    fn entities(&mut self, bb: Aabb) -> Vec<EntityHit> {
        let game = &*self.0;
        game.mobs
            .iter()
            .filter(|m| !m.removed)
            .filter(|m| {
                let b = m.bounds();
                let (lo, hi) = (b.min_position(), b.max_position());
                hi.x >= bb.min.x
                    && lo.x <= bb.max.x
                    && hi.y >= bb.min.y
                    && lo.y <= bb.max.y
                    && hi.z >= bb.min.z
                    && lo.z <= bb.max.z
            })
            .map(|m| EntityHit {
                id: m.id,
                pickable: true,
                is_player: false,
                is_vehicle: Some(m.id) == game.riding,
            })
            .collect()
    }

    fn clip(&mut self, from: DVec3, to: DVec3) -> Option<BlockHit> {
        let delta = to - from;
        let length = delta.length();
        if length < 1e-6 {
            return None;
        }
        let dir = (delta / length).to_array();
        let hit = self.0.clip_ray(from.to_array(), dir, length)?;
        Some(BlockHit {
            pos: hit.pos,
            face: face_of(hit.face),
            location: DVec3::from_array(hit.point),
            // Match VoxelShape.clip's inside test, including starts on a face.
            inside: hit.face == [0; 3]
                || horizon_visor::tasks::swing::starts_inside_outline(
                    from,
                    to,
                    &self.0.outline_boxes(hit.pos),
                ),
        })
    }

    fn block_kind(&mut self, hit: &BlockHit) -> BlockKind {
        let game = &*self.0;
        let state = game.world.block(hit.pos[0], hit.pos[1], hit.pos[2]);
        let facts = game.res.facts().get(state);
        let is = |name: &str| facts.classes.iter().any(|c| c.ends_with(name));
        if is("NoteBlock") {
            BlockKind::Music
        } else if is("LadderBlock") || is("VineBlock") {
            BlockKind::Climbable
        } else {
            BlockKind::Normal
        }
    }
}

/// How the item in a hand swings: a sword or spear, a tool, or neither.
fn held_item(item: Option<&str>) -> HeldItem {
    let Some(item) = item else {
        return HeldItem {
            empty: true,
            ..Default::default()
        };
    };
    let name = item.strip_prefix("minecraft:").unwrap_or(item);
    let sword = name.ends_with("_sword") || name == "trident";
    let lance = name.ends_with("_spear");
    let tool = name.ends_with("_pickaxe")
        || name.ends_with("_axe")
        || name.ends_with("_shovel")
        || name.ends_with("_hoe")
        || matches!(
            name,
            "shears"
                | "stick"
                | "bone"
                | "blaze_rod"
                | "bamboo"
                | "torch"
                | "redstone_torch"
                | "debug_stick"
                | "flint_and_steel"
                | "brush"
                | "fishing_rod"
                | "arrow"
                | "carrot_on_a_stick"
                | "warped_fungus_on_a_stick"
        );
    HeldItem {
        empty: false,
        sword,
        lance,
        tool,
    }
}

/// `ItemUseAnimation` of an item the eating tracker cares about.
fn use_animation(item: &str, consumable: bool) -> UseAnimation {
    if !consumable {
        return UseAnimation::Other;
    }
    let name = item.strip_prefix("minecraft:").unwrap_or(item);
    if matches!(
        name,
        "potion" | "milk_bucket" | "honey_bottle" | "ominous_bottle"
    ) || name.ends_with("_potion")
    {
        UseAnimation::Drink
    } else if name == "goat_horn" {
        UseAnimation::TootHorn
    } else {
        UseAnimation::Eat
    }
}

impl Game<'_> {
    fn vr_click(
        &mut self,
        event_loop: &dyn EventLoopCtl,
        id: winit::window::WindowId,
        button: MouseButton,
        state: ElementState,
    ) {
        if button == MouseButton::Left {
            self.pad.headset.swing_type = super::headset_hands::SwingType::Attack;
        }
        self.handle_window_event(
            event_loop,
            id,
            WindowEvent::MouseInput {
                device_id: DeviceId::dummy(),
                state,
                button,
            },
            InputOrigin::User,
        );
    }

    /// `SwingTracker`: a fast swing attacks what the weapon reaches and mines what its tip
    /// crosses. An attack clicks with the aim on the hand that swung (the tick aims it); a
    /// mining swing holds attack on the block for `hits` ticks.
    pub(super) fn vr_swing(&mut self, event_loop: &dyn EventLoopCtl, id: winit::window::WindowId) {
        // Visor's `TaskSwing` swings unless the player picked Vivecraft (or the server is one).
        if self.visor_mechanics() {
            self.visor_swing(event_loop, id);
            return;
        }
        let settings = headset::settings();
        let flags = self.vr_flags();
        let hands = [headset::main_hand(), headset::off_hand()];
        if hands.iter().all(Option::is_none) || self.spectator() || self.player.is_none() {
            self.pad.headset.vr.swing.inactive();
            return;
        }
        let creative = self.creative;
        let jumping = self.pad.headset.vr.jumped;
        let mut swing = std::mem::take(&mut self.pad.headset.vr.swing);
        let active = swing.is_active(&settings, &flags, creative, false, true, false, jumping);
        if !active {
            swing.inactive();
            self.pad.headset.vr.swing = swing;
            return;
        }
        let world_data = self.pad.headset.vr.player.world_pre;
        let room_data = self.pad.headset.vr.player.room_pre;
        let held = [Hand::Main, Hand::Off].map(|hand| {
            let item = self.with_hand(hand, |g| g.held().map(|s| s.item.to_string()));
            held_item(item.as_deref())
        });
        let input = SwingInput {
            now_ms: self.pad.headset.vr.clock_ms(),
            world: &world_data,
            room: &room_data,
            held,
            creative,
            entity_reach: self.entity_reach,
            shift_down: flags.shift_down,
            grabbing_ladder: [false; 2],
            cannot_attack: [false; 2],
            can_break_climbable_when_sneaking: false,
        };
        let actions = swing.tick(&settings, &input, &mut GameSwing(self));
        self.pad.headset.vr.swing = swing;
        let mut attacked = false;
        for action in actions {
            match action {
                SwingAction::Attack { hand, .. } if !attacked => {
                    attacked = true;
                    // The tick aims the click from the hand that swung.
                    self.pad.headset.set_off_pending(hand == 1);
                    for state in [ElementState::Pressed, ElementState::Released] {
                        self.vr_click(event_loop, id, MouseButton::Left, state);
                    }
                }
                SwingAction::Mine { hand: 0, hit, hits } if self.pad.headset.trackers.idle() => {
                    let block = super::Hit {
                        pos: hit.pos,
                        point: hit.location.to_array(),
                        face: face_normal(hit.face),
                    };
                    self.forced_hit = Some(block);
                    self.pad
                        .headset
                        .trackers
                        .hold(MouseButton::Left, Duration::from_millis(50) * hits);
                    self.vr_click(event_loop, id, MouseButton::Left, ElementState::Pressed);
                }
                SwingAction::Tap { hand: 0, hit } if self.pad.headset.trackers.idle() => {
                    let block = super::Hit {
                        pos: hit.pos,
                        point: hit.location.to_array(),
                        face: face_normal(hit.face),
                    };
                    self.forced_hit = Some(block);
                    self.pad
                        .headset
                        .trackers
                        .hold(MouseButton::Left, Duration::from_millis(100));
                    self.vr_click(event_loop, id, MouseButton::Left, ElementState::Pressed);
                }
                SwingAction::Haptic(h) => {
                    headset::pulse(
                        h.controller == 0,
                        0.6,
                        (h.micros as f32 / 1.0e6 * 40.0).clamp(0.02, 0.1),
                    );
                }
                _ => {}
            }
        }
    }

    /// `EatingTracker`: food or a drink held to the mouth is used until it is eaten.
    pub(super) fn vr_eating(&mut self, event_loop: &dyn EventLoopCtl, id: winit::window::WindowId) {
        let settings = headset::settings();
        let flags = self.vr_flags();
        let held = [Hand::Main, Hand::Off].map(|hand| {
            let consumable = self.consumable_in(hand);
            let item = self.with_hand(hand, |g| g.held().map(|s| s.item.to_string()));
            item.map_or(UseAnimation::Other, |i| use_animation(&i, consumable))
        });
        let bow = self.headset_holds_bow();
        // Visor's `room_consume`: food or a drink held to the mouth is used.
        let active = settings.bool("visorRoomConsume")
            && self
                .pad
                .headset
                .vr
                .eating
                .is_active(&settings, &flags, bow, held);
        let room = self.pad.headset.vr.player.room_pre;
        let now = self.pad.headset.vr.clock_ms();
        let mut hand = None;
        if active {
            let out = {
                let vr = &mut self.pad.headset.vr;
                vr.eating
                    .tick(&room, &vr.player.histories, now, held, 0, |_| true)
            };
            for (c, haptic) in out.haptics.into_iter().enumerate() {
                if haptic.is_some() {
                    headset::pulse(c == 0, 0.7, 0.03);
                }
            }
            let eating = &self.pad.headset.vr.eating;
            hand = if eating.eating(0) {
                Some(Hand::Main)
            } else if eating.eating(1) {
                Some(Hand::Off)
            } else {
                None
            };
        } else {
            self.pad.headset.vr.eating.inactive();
        }
        // `TelescopeTracker`: a spyglass held to the eye is used while looked through.
        if hand.is_none() {
            let scope = [Hand::Main, Hand::Off].map(|h| {
                self.with_hand(h, |g| {
                    g.held().is_some_and(|s| &*s.item == "minecraft:spyglass")
                })
            });
            if self.pad.headset.vr.scope.is_active(&flags, bow, scope) {
                let in_use = [
                    self.used_hand() == Some(Hand::Main),
                    self.used_hand() == Some(Hand::Off),
                ];
                self.pad
                    .headset
                    .vr
                    .scope
                    .tick(&settings, &room, scope, in_use);
                let state = &self.pad.headset.vr.scope;
                hand = if state.viewing(0) {
                    Some(Hand::Main)
                } else if state.viewing(1) {
                    Some(Hand::Off)
                } else {
                    None
                };
            } else {
                self.pad.headset.vr.scope.inactive();
            }
        }
        self.headset_eating(event_loop, id, hand);
    }

    /// `HapticTracker.handleHit` on controllers (Vivecraft drives a haptic vest there): a loss of
    /// health buzzes both controllers, more on the side the hit came from when the server told
    /// (`DamageDirection`, in the player's frame).
    fn vr_hit_response(&mut self) {
        let Some(health) = self.player.as_ref().map(|p| p.health.health()) else {
            return;
        };
        let last = std::mem::replace(&mut self.pad.headset.vr.last_health, health);
        let lost = last - health;
        if last <= 0.0 || lost <= 0.0 {
            self.vivecraft.last_hit = None;
            return;
        }
        let strength = (lost / 6.0).clamp(0.25, 1.0);
        let side = self.vivecraft.last_hit.take().map(|dir| {
            // Rotated into the room: positive x is the right.
            let yaw = headset::body_yaw().to_radians();
            (dir.x * yaw.cos() + dir.z * yaw.sin()).signum()
        });
        for main in [true, false] {
            // The right hand is the main one unless the hands are reversed.
            let right = main != headset::settings().left_handed();
            let near = side.is_none_or(|s| (s > 0.0) == right);
            headset::pulse(main, if near { strength } else { strength * 0.4 }, 0.15);
        }
    }

    /// A stack named like Vivecraft's special items (`ViveItems.isClimbingClaws` and
    /// `isJumpBoots`: the item with the name `Climb Claws` or `Jump Boots`).
    fn vr_named(
        &self,
        stack: Option<&horizon_client::items::ItemStack>,
        item: &str,
        name: &str,
    ) -> bool {
        stack.is_some_and(|s| {
            &*s.item == item
                && super::combat::has_custom_name(s)
                && self.stack_display_name(s).plain() == name
        })
    }

    /// `ClimbTracker` and the Climbey boots' jump, once per tick: the hands on a ladder, a vine
    /// or (with the claws) any block hold the player up and pulling moves it.
    pub(super) fn vr_climb(&mut self) {
        if !headset::active() {
            return;
        }
        // Visor's own grab logic (`TaskRoomClimb`), not Vivecraft's tracker.
        if self.visor_mechanics() {
            self.visor_climb();
            return;
        }
        let settings = headset::settings();
        let flags = self.vr_flags();
        let climbey = self.vivecraft.climbey;
        let (claws, boots) = (
            climbey
                && (self.vr_named(self.held(), "minecraft:shears", "Climb Claws")
                    || self.vr_named(self.offhand_item(), "minecraft:shears", "Climb Claws")),
            climbey
                && self.vr_named(
                    self.armor[3].as_ref(),
                    "minecraft:leather_boots",
                    "Jump Boots",
                ),
        );
        let boost = self
            .potions
            .effects
            .amplifier("jump_boost")
            .map_or(0, |a| a + 1);
        let moving = self.pad_move().is_some_and(|m| m != [0.0, 0.0]);
        let Some(player) = self.player.as_ref() else {
            return;
        };
        let pos = DVec3::new(player.position.x, player.position.y, player.position.z);
        let cp = ClimbPlayer {
            pos,
            bb: Aabb {
                min: pos - DVec3::new(0.3, 0.0, 0.3),
                max: pos + DVec3::new(0.3, 1.8, 0.3),
            },
            delta: DVec3::new(player.velocity.x, player.velocity.y, player.velocity.z),
            on_ground: player.ground.on_ground,
            flying: player.flying,
            alive: flags.alive,
            passenger: flags.passenger,
            in_game: flags.in_game,
            jump_boost: boost,
            claws,
            boots,
            moving,
            free_move: true,
        };
        let buttons = headset::hand_buttons();
        let grab = [buttons[0].grip > 0.6, buttons[1].grip > 0.6];
        // Visor's jump is B (`GameActionJump`).
        let jump_key = buttons[0].secondary;
        let net = &self.vivecraft.net;
        let mut climb = std::mem::take(&mut self.pad.headset.vr.climb);
        climb.server_blockmode = net.climb_blockmode;
        if climb.blocklist != net.climb_blocklist {
            climb.blocklist = net.climb_blocklist.clone();
        }
        let mut climb_jump = std::mem::take(&mut self.pad.headset.vr.climb_jump);
        climb.idle(&settings, &cp);
        // Visor: room climbing is the server's (`roomClimbingSupported`) and the player's.
        let active = climb.is_active(&settings, &cp)
            && (!self.visor_mechanics() || self.vr_server_settings().room_climbing_supported);
        let jumping = boots && climb_jump_active(&settings, &flags, &cp);
        let mut actions = Vec::new();
        if active || jumping {
            let vr = &self.pad.headset.vr;
            let world = vr.player.world_pre;
            let room = vr.player.room_pre;
            let histories = vr.player.histories.clone();
            let input = ClimbInput {
                now_ms: vr.clock_ms(),
                world: &world,
                room: &room,
                histories: &histories,
                grab,
                jump_key,
            };
            if active {
                actions = climb.tick(&settings, &cp, &input, &mut GameClimb(self));
            }
            if jumping {
                actions.extend(climb_jump.tick(&cp, &input, &mut climb));
            }
        }
        if !active {
            // Idle: give gravity back once, whatever was held.
            actions.extend(climb.inactive());
        }
        if !jumping {
            climb_jump.inactive();
        }
        self.pad.headset.vr.climb_jump = climb_jump;
        self.pad.headset.vr.climb = climb;
        for action in actions {
            self.apply_climb(action);
        }
    }

    fn apply_climb(&mut self, action: ClimbAction) {
        match action {
            ClimbAction::SetPos(to) => {
                if let Some(player) = self.player.as_mut() {
                    let moved = [
                        to.x - player.position.x,
                        to.y - player.position.y,
                        to.z - player.position.z,
                    ];
                    player.position.x = to.x;
                    player.position.y = to.y;
                    player.position.z = to.z;
                    // The camera follows at once.
                    self.previous_position.x += moved[0];
                    self.previous_position.y += moved[1];
                    self.previous_position.z += moved[2];
                }
            }
            ClimbAction::SetPrevious(at) => {
                self.previous_position.x = at.x;
                self.previous_position.y = at.y;
                self.previous_position.z = at.z;
            }
            ClimbAction::SetDelta(v) => {
                if let Some(player) = self.player.as_mut() {
                    player.velocity.x = v.x;
                    player.velocity.y = v.y;
                    player.velocity.z = v.z;
                }
            }
            ClimbAction::NoGravity(on) => {
                if let Some(player) = self.player.as_mut() {
                    player.no_gravity = on;
                }
            }
            ClimbAction::ResetFall => {
                if let Some(player) = self.player.as_mut() {
                    player.fall_distance = 0.0;
                }
            }
            ClimbAction::Haptic(h) => {
                headset::pulse(
                    h.controller == 0,
                    0.6,
                    (h.micros as f32 / 1.0e6 * 40.0).clamp(0.02, 0.1),
                );
            }
            ClimbAction::SendClimbing => {
                if self.visor_mechanics() {
                    // `TaskRoomClimb`: the server resets the fall when climbing is supported.
                    if self.visor_active() {
                        self.visor_send(&horizon_visor::payload::C2s::Climbing);
                    }
                } else {
                    self.vivecraft_send(&C2s::Climbing);
                }
            }
            ClimbAction::SendJumping => {
                let net = &self.vivecraft.net;
                if NetworkVersion::ClimbeyJump.accepts(net.used_network_version) {
                    self.vivecraft_send(&C2s::Jumping);
                }
            }
            ClimbAction::SnapRoom | ClimbAction::UnpressGrab(_) | ClimbAction::StepSound(_) => {}
        }
    }

    /// `SwimTracker`: a tick tracker, run for the ticks that passed since the last frame.
    pub(super) fn vr_swim(&mut self) {
        let settings = headset::settings();
        let flags = self.vr_flags();
        let stick = headset::pad().map_or(0.0, |p| p.left[0].abs().max(p.left[1].abs()));
        let now = Instant::now();
        let vr = &mut self.pad.headset.vr;
        let dt = vr
            .last_frame
            .replace(now)
            .map_or(0.0, |t| now.duration_since(t).as_secs_f32().min(0.1));
        if !vr.swim.is_active(&settings, &flags, stick > 0.1) {
            vr.swim_time = 0.0;
            return;
        }
        vr.swim_time += dt;
        let world = vr.player.world_pre;
        let mut push = Vec3::ZERO;
        let mut swimming = false;
        while vr.swim_time >= 0.05 {
            vr.swim_time -= 0.05;
            let out = vr.swim.tick(&world);
            push += out.push;
            swimming = out.swimming;
        }
        let _ = swimming;
        if let Some(player) = self.player.as_mut() {
            player.velocity.x += f64::from(push.x);
            player.velocity.y += f64::from(push.y);
            player.velocity.z += f64::from(push.z);
        }
    }
}

/// What the climb tracker asks of the game's world.
pub(super) struct GameClimb<'a, 'b>(pub(super) &'a Game<'b>);

impl ClimbWorld for GameClimb<'_, '_> {
    fn block(&self, pos: [i32; 3]) -> ClimbBlock {
        let game = self.0;
        let state = game.world.block(pos[0], pos[1], pos[2]);
        let facts = game.res.facts().get(state);
        let props = game
            .pack
            .assets
            .registry
            .state(state)
            .map(|s| &s.properties);
        let prop = |key: &str| props.and_then(|p| p.get(key)).map(String::as_str);
        let side = |v: Option<&str>| match v? {
            "north" => Some(Side::North),
            "south" => Some(Side::South),
            "east" => Some(Side::East),
            "west" => Some(Side::West),
            _ => None,
        };
        let is = |suffix: &str| facts.classes.iter().any(|c| c.ends_with(suffix));
        let occludes = |dx: i32, dz: i32| {
            let near = game.world.block(pos[0] + dx, pos[1], pos[2] + dz);
            game.res.facts().get(near).can_occlude
        };
        let climbable = if is("LadderBlock") {
            side(prop("facing")).map_or(Climbable::No, Climbable::Facing)
        } else if is("VineBlock") {
            let face = |key: &str| prop(key) == Some("true");
            Climbable::Vine {
                north: face("north") && occludes(0, -1),
                east: face("east") && occludes(1, 0),
                south: face("south") && occludes(0, 1),
                west: face("west") && occludes(-1, 0),
            }
        } else if facts.name.ends_with("_trapdoor") && prop("open") == Some("true") {
            // `trapdoorUsableAsLadder`: open, over a ladder facing the same way.
            let below = game.world.block(pos[0], pos[1] - 1, pos[2]);
            let ladder = game.res.facts().get(below).name.as_ref() == "minecraft:ladder";
            let same = game
                .pack
                .assets
                .registry
                .state(below)
                .and_then(|s| s.properties.get("facing"))
                .map(String::as_str)
                == prop("facing");
            match (ladder && same, side(prop("facing"))) {
                (true, Some(s)) => Climbable::Facing(s),
                _ => Climbable::No,
            }
        } else if game.block_in_tag("minecraft:climbable", &facts.name) {
            side(prop("facing")).map_or(Climbable::Other, Climbable::Facing)
        } else {
            Climbable::No
        };
        let shape = game.block_collision_shape(pos).bounds().map(|b| Aabb {
            min: DVec3::new(b.min_position().x, b.min_position().y, b.min_position().z),
            max: DVec3::new(b.max_position().x, b.max_position().y, b.max_position().z),
        });
        ClimbBlock {
            climbable,
            shape,
            id: facts.name.to_string(),
        }
    }
    fn no_collision(&mut self, bb: Aabb) -> bool {
        TeleportWorld::no_collision(&mut GameTeleport(self.0), bb)
    }
}

/// The Climbey boots' jump runs under `JumpTracker.isActive`.
fn climb_jump_active(
    settings: &horizon_vivecraft::settings::VrSettings,
    flags: &PlayerFlags,
    cp: &ClimbPlayer,
) -> bool {
    !settings.seated()
        && settings.realistic_jump() != horizon_vivecraft::modes::Tri::Off
        && cp.alive
        && !(flags.in_water || flags.in_lava || !cp.on_ground)
        && !flags.shift_down
        && !cp.passenger
}

pub(super) fn face_normal(face: Face) -> [i32; 3] {
    match face {
        Face::Down => [0, -1, 0],
        Face::Up => [0, 1, 0],
        Face::North => [0, 0, -1],
        Face::South => [0, 0, 1],
        Face::West => [-1, 0, 0],
        Face::East => [1, 0, 0],
    }
}

impl VrRuntime {
    /// Milliseconds on the runtime's clock (the headset's frame time, as `Util.getMillis`).
    pub(super) fn clock_ms(&self) -> u64 {
        headset::room().map_or(0, |r| r.time_ms)
    }
}

/// What the teleport arc asks of the game's world.
struct GameTeleport<'a, 'b>(&'a Game<'b>);

impl GameTeleport<'_, '_> {
    /// The first water or lava surface along `from..to`, which the block ray passes
    /// (`ClipContext.Fluid.ANY`): where the ray first is below the fluid's top in a block
    /// with no fluid above it. The distance along the ray and the hit.
    fn fluid_surface(&self, from: DVec3, dir: DVec3, length: f64) -> Option<(f64, TeleportHit)> {
        let game = self.0;
        let mut t = 0.0;
        while t <= length {
            let p = from + dir * t;
            let pos = [p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32];
            let facts = game
                .res
                .facts()
                .get(game.world.block(pos[0], pos[1], pos[2]));
            if facts.fluid != horizon_client::blocks::Fluid::None {
                let above = game.world.block(pos[0], pos[1] + 1, pos[2]);
                let same = game.res.facts().get(above).fluid == facts.fluid;
                // `FluidState.getHeight`: a level out of nine, a falling or covered fluid is full.
                let height = if same || facts.fluid_falling {
                    1.0
                } else {
                    f64::from(facts.fluid_level) / 9.0
                };
                if !same && p.y - f64::from(pos[1]) <= height {
                    let location = DVec3::new(p.x, f64::from(pos[1]) + height, p.z);
                    return Some((
                        t,
                        TeleportHit {
                            pos,
                            location,
                            face: Face::Up,
                        },
                    ));
                }
            }
            t += 0.05;
        }
        None
    }
}

impl TeleportWorld for GameTeleport<'_, '_> {
    fn clip(&mut self, from: DVec3, to: DVec3) -> Option<TeleportHit> {
        let delta = to - from;
        let length = delta.length();
        if length < 1e-6 {
            return None;
        }
        let dir = delta / length;
        let block = self
            .0
            .clip_ray(from.to_array(), dir.to_array(), length)
            .map(|hit| {
                let location = DVec3::from_array(hit.point);
                (
                    from.distance(location),
                    TeleportHit {
                        pos: hit.pos,
                        location,
                        face: face_of(hit.face),
                    },
                )
            });
        // Fluids count as hits too: the arc can land on water.
        let fluid = self.fluid_surface(from, dir, block.as_ref().map_or(length, |b| b.0));
        match (block, fluid) {
            (_, Some((_, hit))) => Some(hit),
            (Some((_, hit)), None) => Some(hit),
            (None, None) => None,
        }
    }
    fn no_collision(&mut self, bb: Aabb) -> bool {
        let mut scene = horizon_client::collisions::SceneCollisions {
            world: &self.0.world,
            binding: self.0.collision,
            subject: None,
        };
        let bounds = horizon_core::geometry::Aabb::new(
            horizon_core::geometry::Vec3::new(bb.min.x, bb.min.y, bb.min.z),
            horizon_core::geometry::Vec3::new(bb.max.x, bb.max.y, bb.max.z),
        );
        horizon_client::collisions::no_collision_empty(&mut scene, bounds)
    }
    fn has_fluid(&self, pos: [i32; 3]) -> bool {
        let game = self.0;
        let state = game.world.block(pos[0], pos[1], pos[2]);
        game.res.facts().get(state).fluid != horizon_client::blocks::Fluid::None
    }
    fn is_climbable(&self, pos: [i32; 3]) -> bool {
        let game = self.0;
        let facts = game
            .res
            .facts()
            .get(game.world.block(pos[0], pos[1], pos[2]));
        facts
            .classes
            .iter()
            .any(|c| c.ends_with("LadderBlock") || c.ends_with("VineBlock"))
    }
    fn collision_top(&self, pos: [i32; 3]) -> Option<f64> {
        let shape = self.0.block_collision_shape(pos);
        (!shape.is_empty()).then(|| shape.max(horizon_core::geometry::Axis::Y))
    }
    fn is_sinking(&self, pos: [i32; 3]) -> bool {
        let game = self.0;
        let facts = game
            .res
            .facts()
            .get(game.world.block(pos[0], pos[1], pos[2]));
        matches!(
            &*facts.name,
            "minecraft:soul_sand" | "minecraft:honey_block"
        )
    }
    fn same_block(&self, a: [i32; 3], b: [i32; 3]) -> bool {
        let game = self.0;
        let facts = game.res.facts();
        facts.get(game.world.block(a[0], a[1], a[2])).name
            == facts.get(game.world.block(b[0], b[1], b[2])).name
    }
}

impl Game<'_> {
    fn teleport_limits(&self) -> TeleportLimits {
        let s = headset::settings();
        // Visor: the limits are the server's (`teleportUpLimit` ...), and survival is limited.
        if self.visor_active() {
            let (up, down, forward) =
                horizon_visor::tasks::movement::teleport_limits(self.vr_server_settings());
            return TeleportLimits {
                limited_survival: true,
                down,
                up,
                horiz: forward,
            };
        }
        TeleportLimits {
            limited_survival: s.bool("limitedTeleport"),
            down: s.int("teleportLimitDown"),
            up: s.int("teleportLimitUp"),
            horiz: s.int("teleportLimitHoriz"),
        }
    }

    /// Every frame: the teleport arc follows the off hand while its trigger is held.
    pub(super) fn vr_teleport_frame(&mut self) {
        let Some(p) = self.player.as_ref() else {
            return;
        };
        let feet = DVec3::new(p.position.x, p.position.y, p.position.z);
        let player = TeleportPlayer {
            pos: feet,
            half_width: f64::from(p.dimensions.width) / 2.0,
            height: f64::from(p.dimensions.height),
            shift_down: p.shift_key_down,
            may_fly: p.may_fly,
            jump_factor: 1.0,
            speed_factor: 1.0,
        };
        let limits = self.teleport_limits();
        let settings = headset::settings();
        let world = self.pad.headset.vr.player.world_pre;
        let mut tracker = std::mem::take(&mut self.pad.headset.vr.teleport);
        tracker.update_destinations(
            &world,
            settings.seated(),
            &player,
            &limits,
            &mut GameTeleport(self),
        );
        self.pad.headset.vr.teleport = tracker;
    }

    /// `RowTracker` (a tick tracker): the oar strengths of the controllers while the player
    /// sits in a boat and does not push forward.
    fn vr_row(&mut self) {
        let settings = headset::settings();
        let flags = self.vr_flags();
        let forward = self
            .riding
            .and_then(|id| self.boats.iter().find(|b| b.id == id))
            .map(|b| b.input[2]);
        let bow = self.pad.headset.bow.notched();
        let vr = &mut self.pad.headset.vr;
        let now = vr.clock_ms();
        match forward {
            Some(forward) if vr.row.is_active(&settings, &flags, true, forward, bow) => {
                vr.row.tick(&vr.player.histories, now);
            }
            _ => vr.row.inactive(),
        }
    }

    /// What the player rides, as the vehicle tracker sees it.
    fn vr_vehicle_view(&self) -> Option<VehicleView> {
        let id = self.riding?;
        let blank = VehicleView {
            kind: VehicleKind::Other,
            yaw: 0.0,
            body_yaw: 0.0,
            authoritative: false,
            saddled: false,
            cart_moved: Vec3::ZERO,
            cart_speed: 0.0,
        };
        if let Some(m) = self.mobs.iter().find(|m| m.id == id) {
            let horse = horizon_client::mobs::equine::is_equine(m.species());
            let ghast = m.species() == horizon_client::mobs::Species::HappyGhast;
            return Some(VehicleView {
                kind: if horse {
                    VehicleKind::Horse
                } else if ghast {
                    VehicleKind::Other
                } else {
                    VehicleKind::Mob
                },
                yaw: m.yaw,
                body_yaw: m.yaw,
                authoritative: self.driven_mount().is_some_and(|d| d.id == id),
                saddled: m.extra.saddled,
                ..blank
            });
        }
        if let Some(c) = self.carts.iter().find(|c| c.id == id) {
            let moved = c.position.subtract(c.previous);
            return Some(VehicleView {
                kind: VehicleKind::Minecart,
                yaw: c.yaw,
                body_yaw: c.yaw,
                cart_moved: Vec3::new(moved.x as f32, moved.y as f32, moved.z as f32),
                cart_speed: (c.velocity.x.hypot(c.velocity.z)) as f32,
                ..blank
            });
        }
        // Boats turn the rider with them already (`positionRider`).
        None
    }

    /// `VehicleTracker` (a tick tracker): the world turns with the vehicle you ride.
    fn vr_vehicle(&mut self) {
        let settings = headset::settings();
        let flags = self.vr_flags();
        if !VehicleTracker::is_active(flags.alive, flags.in_game) {
            self.pad.headset.vr.vehicle.inactive();
            return;
        }
        let view = self.vr_vehicle_view();
        let vr = &mut self.pad.headset.vr;
        match (flags.passenger, vr.vehicle.is_riding()) {
            (true, false) => {
                let hmd_yaw = vr.player.world_pre.hmd().yaw();
                let start = view.map_or(0.0, |v| v.yaw);
                let turn = vr.vehicle.on_start_riding(
                    &settings,
                    start,
                    view.is_some_and(|v| v.kind == VehicleKind::Minecart),
                    hmd_yaw,
                    0.0,
                );
                // A turn of the world turns the body the other way.
                if turn != 0.0 && view.is_some() {
                    headset::snap_turn(-turn);
                }
            }
            (false, true) => vr.vehicle.on_stop_riding(),
            _ => {}
        }
        if !flags.passenger || settings.seated() {
            vr.vehicle.tick(&settings, None);
            return;
        }
        let turn = vr.vehicle.tick(&settings, view);
        if turn != 0.0 {
            headset::snap_turn(-turn);
        }
    }

    /// `VRPlayer.doPermanentLookOverride` for a passenger: the yaw and pitch (game degrees) the
    /// mount the client moves is steered with, when forward is held (`getSteeringDirection`).
    pub(super) fn vr_mount_look(&self, forward: bool) -> Option<(f32, f32)> {
        if !headset::active() {
            return None;
        }
        let settings = headset::settings();
        if settings.seated() {
            return None;
        }
        let view = self.vr_vehicle_view()?;
        let dir = steering_direction(
            &settings,
            &self.pad.headset.vr.player.world_pre,
            view.kind,
            forward,
            view.authoritative,
            None,
            Vec3::ZERO,
        )?;
        let len = dir.length();
        if len < 1.0e-4 {
            return None;
        }
        Some((
            (-dir.x).atan2(dir.z).to_degrees(),
            (-dir.y / len).asin().to_degrees(),
        ))
    }

    /// `AbstractBoatMixin`: how the VR player's controllers steer the boat they ride (standing
    /// mode only); None leaves vanilla's controls.
    pub(super) fn vr_boat_steer(
        &self,
        yaw: f32,
        delta_rotation: f32,
        up: bool,
    ) -> Option<BoatSteer> {
        if !headset::active() {
            return None;
        }
        let settings = headset::settings();
        if settings.seated() {
            return None;
        }
        let vr = &self.pad.headset.vr;
        let world = &vr.player.world_pre;
        let point = match settings.free_move_mode(false) {
            FreeMove::Hmd => world.hmd().yaw(),
            FreeMove::Waist => world.body_yaw(),
            _ => world.controller(1).yaw(),
        };
        Some(steer_boat(
            settings.bool("vehicleRotation"),
            up,
            point,
            yaw,
            delta_rotation,
            &vr.row,
        ))
    }

    /// Every game tick: the teleport tracker; a release carries the teleport out, to the
    /// server's teleport packet when it takes them, else to `/tp` (no permission) or just
    /// there (offline).
    pub(super) fn vr_tick(&mut self) {
        if !headset::active() {
            return;
        }
        let flags = self.vr_flags();
        self.vr_hit_response();
        // `RunTracker`: arms swinging like running (the run-in-place free move mode).
        {
            let settings = headset::settings();
            let bow = self.pad.headset.bow.notched();
            let vr = &mut self.pad.headset.vr;
            let active = vr.run.is_active(&settings, &flags, true, bow);
            let (histories, world, now) =
                (&vr.player.histories, vr.player.world_pre, vr.clock_ms());
            vr.run.tick(active, histories, now, &world);
        }
        self.vr_row();
        self.vr_vehicle();
        self.visor_crawl_tick(&flags);
        self.visor_confirm_breaks();
        if !TeleportTracker::is_active(&flags) {
            self.pad.headset.vr.teleport.inactive();
            return;
        }
        let passenger = flags.passenger;
        let direct_ok = self.vivecraft.net.server_supports_direct_teleport || self.visor_active();
        let vr = &mut self.pad.headset.vr;
        let input = TeleportInput {
            binding_down: vr.teleport_binding,
            seated_teleport: false,
        };
        let Some(done) = vr.teleport.tick(input, passenger, direct_ok, direct_ok) else {
            return;
        };
        let to = match done {
            Teleport::Command(to) | Teleport::Direct { to, .. } => to,
        };
        let from = self
            .player
            .as_ref()
            .map(|p| DVec3::new(p.position.x, p.position.y, p.position.z));
        let limits = self.teleport_limits();
        if let Some(from) = from {
            self.pad
                .headset
                .vr
                .teleport
                .spend_energy(&limits, from.distance(to));
        }
        match done {
            Teleport::Direct { to, direct_packet } => {
                self.teleport_to(super::commands::Target::Player, to.to_array(), None);
                if direct_packet {
                    if self.visor_active() {
                        self.visor_send_teleport(to);
                    } else {
                        self.vivecraft_send_teleport(to);
                    }
                }
            }
            Teleport::Command(to) => {
                self.send_command(&format!("tp {} {} {}", to.x, to.y, to.z));
            }
        }
        // `swingTracker.disableSwing = 3`
        self.pad.headset.vr.swing.disable_swing = 3;
        self.visor.swing.inactive_ticks = 3;
        if let Some(p) = self.player.as_mut() {
            p.fall_distance = 0.0;
        }
    }
}

impl Game<'_> {
    /// The teleport arc as lines (Vivecraft draws it as a textured beam): green to a
    /// destination the player may go to, red when there is none.
    pub(super) fn vr_teleport_lines(&self, lines: &mut Vec<horizon_client::gizmos::Line>) {
        let tracker = &self.pad.headset.vr.teleport;
        if !tracker.is_aiming() {
            return;
        }
        let ok = tracker.destination() != DVec3::ZERO;
        // Visor's arc (`HandEffectTeleport`): cyan, 0.9 of it where the player may fly, 0.8 where
        // survival limits the teleport, and a dim 0.3 at half strength where it cannot land.
        let may_fly = self.creative || self.spectator();
        let (strength, alpha) = match (ok, may_fly) {
            (false, _) => (0.3, 0x80),
            (true, true) => (0.9, 0xFF),
            (true, false) => (0.8, 0xFF),
        };
        // The light where it lands (the head's where it cannot), as `getMaxLocalRawBrightness`.
        let probe = if ok {
            tracker.destination()
        } else {
            self.eye_position()
                .map_or(DVec3::ZERO, |e| DVec3::new(e[0], e[1], e[2]))
        };
        let light = self.world.light(
            probe.x.floor() as i32,
            probe.y.floor() as i32,
            probe.z.floor() as i32,
        );
        let light = f32::from((light >> 4).max(light & 15)) / 15.0;
        let rgb = |s: f32| (255.0 * s * light).floor() as u32;
        let color = |a: u32| (a << 24) | (rgb(0.0) << 16) | (rgb(strength) << 8) | rgb(strength);
        let arc = tracker.arc();
        if arc.len() < 2 {
            return;
        }
        // `getArcPosInterpolated`: a point along the arc by its share of the whole.
        let along = |t: f64| {
            let at = t.clamp(0.0, 1.0) * (arc.len() - 1) as f64;
            let (i, f) = ((at.floor() as usize).min(arc.len() - 2), at.fract());
            arc[i].lerp(
                arc[i + 1],
                if at >= (arc.len() - 1) as f64 { 1.0 } else { f },
            )
        };
        // `timer` runs in seconds; the beam's dashes slide along the arc at `BEAM_ANIMATION_SPEED`
        // 2.4 times 0.6 segments a second, and stand still where it cannot land.
        let timer = f64::from(self.started.elapsed().as_secs_f32());
        let scroll = if ok { timer * 2.4 * 0.6 } else { 0.0 };
        let segments = arc.len() - 1;
        let step = 1.0 / segments as f64;
        for i in 0..segments {
            let progress = (i as f64 / segments as f64 + scroll * step).rem_euclid(1.0);
            lines.push(horizon_client::gizmos::Line {
                start: along(progress - step * 0.4).to_array(),
                end: along(progress).to_array(),
                color: color(alpha),
                width: 5.0,
            });
        }
        if ok {
            // The landing mark (`vr_teleport_point`): two square ripples spreading over a 0.8 m
            // quad, each a smooth band 0.02 wide that fades as it nears the edge.
            let d = tracker.destination() + DVec3::new(0.0, 0.01, 0.0);
            let smooth = |a: f64, b: f64, x: f64| {
                let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
                t * t * (3.0 - 2.0 * t)
            };
            const MAX: f64 = 0.6;
            for offset in [0.0, 0.5 * MAX] {
                let p = (timer * 0.15 + offset).rem_euclid(MAX);
                let fade = 1.0 - smooth(MAX - 0.3, MAX, p);
                let band = (fade * 255.0) as u32;
                if band < 4 {
                    continue;
                }
                // The band is 0.02 either side of `p` in the shader's scaled units, of which the quad's
                // half width (0.4 m) is 0.6.
                let e = 0.4 * p / MAX;
                let corners = [
                    DVec3::new(-e, 0.0, -e),
                    DVec3::new(e, 0.0, -e),
                    DVec3::new(e, 0.0, e),
                    DVec3::new(-e, 0.0, e),
                ];
                for k in 0..4 {
                    lines.push(horizon_client::gizmos::Line {
                        start: (d + corners[k]).to_array(),
                        end: (d + corners[(k + 1) % 4]).to_array(),
                        color: color(band),
                        width: 4.0,
                    });
                }
            }
        }
    }
}

impl Game<'_> {
    /// Vivecraft's debug axes: `renderDeviceAxes` for the local devices and
    /// `renderVrPlayerAxes` for the VR players the server sent (red X, green Y, blue Z, 0.2 m).
    /// The remote bodies are not drawn as Vivecraft's VR player model yet; these axes show
    /// where the received head and hands are.
    pub(super) fn vr_axes_lines(&self, lines: &mut Vec<horizon_client::gizmos::Line>) {
        let settings = headset::settings();
        let mut axes = |at: DVec3, rot: Quat| {
            for (axis, color) in [
                (Vec3::X, 0xFFFF_2020_u32),
                (Vec3::Y, 0xFF20_FF20),
                (Vec3::Z, 0xFF20_20FF),
            ] {
                lines.push(horizon_client::gizmos::Line {
                    start: at.to_array(),
                    end: (at + (rot * axis * 0.2).as_dvec3()).to_array(),
                    color,
                    width: 3.0,
                });
            }
        };
        if settings.bool("renderDeviceAxes") {
            let w = self.pad.headset.vr.player.world_pre;
            axes(w.hmd().position(), w.hmd().orientation());
            for c in 0..2 {
                axes(w.controller(c).position(), w.controller(c).orientation());
            }
        }
        if settings.bool("renderVrPlayerAxes") {
            for mob in self.mobs.iter().filter(|m| !m.removed) {
                let Some(uuid) = self.remote_player_uuid(mob.id) else {
                    continue;
                };
                let Some(player) = self.vivecraft.net.players.get(&uuid) else {
                    continue;
                };
                let feet = DVec3::new(mob.position.x, mob.position.y, mob.position.z);
                let state = &player.state;
                for pose in [state.hmd, state.main_hand, state.off_hand] {
                    axes(
                        feet + (pose.position * player.world_scale).as_dvec3(),
                        pose.orientation,
                    );
                }
            }
        }
    }
}

impl Game<'_> {
    /// "Measure Now" (`PlayerHeightTracker.finishMeasure`): the player's height is the head pivot's
    /// average height over the last second.
    fn vr_calibrate_height(&mut self, now_ms: u64) {
        let y = self
            .pad
            .headset
            .vr
            .player
            .histories
            .hmd_pivot
            .average_position(now_ms, 1.0)
            .y;
        let history = &self.pad.headset.vr.player.histories.hmd_pivot;
        let still =
            history.span_ms(now_ms, 1.0) >= 800 && history.average_speed(now_ms, 1.0) <= 0.15;
        if !still || !y.is_finite() || !(horizon_visor::height::MIN_HEIGHT..=2.6).contains(&y) {
            self.system_message(horizon_client::text::Text::translate(
                "visor.messages.height_calibration_failed",
                &[],
            ));
            return;
        }
        self.visor_store_height(y);
        self.pad.headset.vr.height.reset();
        let shown = self.visor_height_text(y);
        self.system_message(horizon_client::text::Text::translate(
            "visor.messages.height_measured",
            &[horizon_client::text::Text::literal(shown)],
        ));
    }

    /// A height as the player's units show it (`HeightFormat`).
    fn visor_height_text(&self, metres: f32) -> String {
        horizon_visor::height::format_height(
            metres,
            headset::settings().enum_name("visorHeightUnits") == "IMPERIAL",
        )
    }

    /// Holds the sneak key for the room's sneak while the head is low and lets it go when it
    /// is back up. It is its own key source: the stick's click toggles, and a toggle released
    /// nothing.
    fn visor_room_sneak(&mut self, wanted: bool) {
        use winit::keyboard::KeyCode;
        // Starling's `keys` is one set of keys, without the device that holds each: the room's
        // sneak remembers that it is the one holding the key.
        let held = self.room_sneaking;
        if wanted != held {
            log::info!(
                "Visor: the room {} (head pivot {:.2} m)",
                if wanted { "sneaks" } else { "stands up" },
                self.pad.headset.vr.player.histories.hmd_pivot.latest().y
            );
        }
        if wanted && !held {
            self.keys.insert(KeyCode::ShiftLeft);
            self.room_sneaking = true;
        } else if !wanted && held {
            self.keys.remove(&KeyCode::ShiftLeft);
            self.room_sneaking = false;
        }
    }

    /// Stores the standing height (`VRClientSettings.setFullHeight`) and saves it.
    fn visor_store_height(&mut self, height: f32) {
        Arc::make_mut(&mut self.settings.vr).set_float("manualCalibration", height);
        self.pad.headset.vr.room_jump.clear();
        self.pad.headset.vr.jump_until = None;
        self.pad.headset.vr.jumped = false;
        self.settings.save(&Self::options_path());
    }

    /// Once per tick in a headset. Visor's auto height (`PlayerHeightTracker.tick`): the head's
    /// resting height is watched, and the standing height follows it. The room thresholds are
    /// shares of that height (`room_sneak.threshold` 0.85, `room_jump.threshold` 1.05): the
    /// sneak and jump trackers read them as the metres below and above it.
    fn visor_height_tick(&mut self, now_ms: u64, look_y: f32) {
        let settings = headset::settings();
        let stored = (settings.float("manualCalibration") != -1.0)
            .then(|| settings.float("manualCalibration"));
        let full = horizon_vivecraft::player::player_height(&settings);
        let sneak = (1.0 - settings.float("visorSneakThreshold")) * full;
        let jump = (settings.float("visorJumpThreshold") - 1.0) * full;
        for (key, value) in [("sneakThreshold", sneak), ("jumpThreshold", jump)] {
            if (settings.float(key) - value).abs() > 1e-4 {
                Arc::make_mut(&mut self.settings.vr).set_float(key, value);
            }
        }
        if !settings.bool("visorHeightAuto") || settings.seated() {
            return;
        }
        let h = &self.pad.headset.vr.player.histories.hmd_pivot;
        let (speed, height) = (
            h.average_speed(now_ms, 0.5),
            h.average_position(now_ms, 0.5).y,
        );
        let vr = &mut self.pad.headset.vr;
        // What the body does must not teach the height: a crouch or a crawl is not a standing rest.
        if stored.is_none() || (!vr.visor_sneak.sneaking && !self.visor.crawl.crawling) {
            vr.height.sample_at(now_ms, speed, look_y, height);
        }
        // `isSafeMoment`: a screen is open or the title shows.
        let safe = self.screen != super::Screen::Playing || self.title.is_some();
        let hint = !self.pad.headset.vr.height_hint && stored.is_none() && self.player.is_some();
        match horizon_visor::height::auto_update(
            self.pad.headset.vr.height.estimate(),
            stored,
            safe,
        ) {
            horizon_visor::height::Auto::Store { height, first } => {
                self.visor_store_height(height);
                if first && self.player.is_some() {
                    let shown = self.visor_height_text(height);
                    self.system_message(horizon_client::text::Text::translate(
                        "visor.messages.height_auto_set",
                        &[horizon_client::text::Text::literal(shown)],
                    ));
                }
            }
            horizon_visor::height::Auto::Keep if hint => {
                self.pad.headset.vr.height_hint = true;
                self.system_message(horizon_client::text::Text::translate(
                    "visor.messages.height_auto_hint",
                    &[],
                ));
            }
            horizon_visor::height::Auto::Keep => {}
        }
    }
}
