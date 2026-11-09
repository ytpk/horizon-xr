// SPDX-License-Identifier: MPL-2.0
//! Visor's gameplay on the client: the swing task, crawling, teleport policy and the other
//! players' block cracks, with the server's settings deciding what is on.
//!
//! Ports the glue of `TaskSwing`, `TaskRoomCrawl`, `TaskTeleport` and `ClientPacketHandler`
//! (`BLOCK_DAMAGE`) (Visor 0.6.1, https://github.com/VisorModStudio/Visor, commit
//! c01354fad8ac03a016c249239a04b07adeb36476). Copyright (C) Phoenix-Ra and Visor contributors;
//! MPL-2.0, see crates/horizon-visor/LICENSE-MPL-2.0. The logic is in the `horizon-visor` crate
//! (`tasks`); this file reads the game and moves the results into it.
use super::event_loop::EventLoopCtl;
use super::visor_net::Preference;
use super::vr::{GameSwing, face_normal};
use super::{Game, Hand};
use glam::{DQuat, DVec3};
use horizon_client::headset;
use horizon_visor::{
    payload::C2s,
    tasks::{
        movement::{self, MovementMode},
        swing::{self, BlockHit, EntityRef, HandInput, ItemKind, SwingAction, SwingWorld},
    },
};
use horizon_vivecraft::{trackers::swing::BlockHit as ViveHit, vr_data::VrData};
use winit::event::{DeviceId, ElementState, MouseButton, WindowEvent};

/// `Direction.get3DDataValue` of a face normal: down, up, north, south, west, east.
pub(super) fn direction_3d(normal: [i32; 3]) -> u8 {
    match normal {
        [0, -1, 0] => 0,
        [0, 1, 0] => 1,
        [0, 0, -1] => 2,
        [0, 0, 1] => 3,
        [-1, 0, 0] => 4,
        _ => 5,
    }
}

/// How `TaskSwing.getItemProperties` sorts an item: weapons are swords, tridents and maces;
/// tools are the diggers, shears, a stick and other handheld things.
pub(super) fn item_kind(item: Option<&str>) -> ItemKind {
    let Some(item) = item else {
        return ItemKind::Empty;
    };
    let name = item.strip_prefix("minecraft:").unwrap_or(item);
    if name.ends_with("_sword") || name == "trident" || name == "mace" {
        return ItemKind::Weapon;
    }
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
                | "spectral_arrow"
                | "tipped_arrow"
                | "carrot_on_a_stick"
                | "warped_fungus_on_a_stick"
        );
    if tool {
        ItemKind::Tool
    } else {
        ItemKind::Other
    }
}

/// The world as the swing task looks at it.
struct VisorSwingWorld<'a, 'b>(GameSwing<'a, 'b>);

fn to_visor_hit(hit: ViveHit) -> BlockHit {
    BlockHit {
        pos: hit.pos,
        face: face_normal(hit.face),
        location: hit.location,
        inside: hit.inside,
    }
}

impl SwingWorld for VisorSwingWorld<'_, '_> {
    fn entities(&mut self, lo: DVec3, hi: DVec3) -> Vec<EntityRef> {
        let game = &*self.0.0;
        game.mobs
            .iter()
            .filter(|m| !m.removed)
            .filter(|m| {
                let b = m.bounds();
                let (blo, bhi) = (b.min_position(), b.max_position());
                bhi.x >= lo.x
                    && blo.x <= hi.x
                    && bhi.y >= lo.y
                    && blo.y <= hi.y
                    && bhi.z >= lo.z
                    && blo.z <= hi.z
            })
            .map(|m| EntityRef {
                id: m.id,
                pickable: true,
                is_vehicle: Some(m.id) == game.riding,
                is_player: game.remote_player_uuid(m.id).is_some(),
            })
            .collect()
    }

    fn clip(&mut self, from: DVec3, to: DVec3) -> Option<BlockHit> {
        use horizon_vivecraft::trackers::swing::SwingWorld as _;
        self.0.clip(from, to).map(to_visor_hit)
    }

    fn is_climbable(&mut self, pos: [i32; 3]) -> bool {
        let game = &*self.0.0;
        let facts = game
            .res
            .facts()
            .get(game.world.block(pos[0], pos[1], pos[2]));
        facts
            .classes
            .iter()
            .any(|c| c.ends_with("LadderBlock") || c.ends_with("VineBlock"))
    }

    fn weapon_can_break(&mut self, hit: &BlockHit) -> bool {
        let correct = {
            let game = &*self.0.0;
            let facts = game
                .res
                .facts()
                .get(game.world.block(hit.pos[0], hit.pos[1], hit.pos[2]));
            // The adapter runs with the swinging hand, including its tool components.
            !facts.requires_tool || game.items().correct_for_drops(game.held(), &facts.name)
        };
        correct || self.0.0.destroy_progress(hit.pos) >= 1.0
    }

    fn is_farmable(&mut self, hit: &BlockHit) -> bool {
        let game = &*self.0.0;
        let facts = game
            .res
            .facts()
            .get(game.world.block(hit.pos[0], hit.pos[1], hit.pos[2]));
        facts.classes.iter().any(|c| {
            c.ends_with("CropBlock") || c.ends_with("StemBlock") || c.ends_with("AttachedStemBlock")
        }) || game
            .held()
            .is_some_and(|stack| game.block_transform(stack, hit.pos, hit.face).is_some())
    }
}

impl Game<'_> {
    /// Visor's rules govern the local mechanics unless the player picked Vivecraft or the server
    /// turned out to be a Vivecraft one (`horizonVrProtocol`).
    pub(super) fn visor_mechanics(&self) -> bool {
        Preference::of(&headset::settings()) != Preference::Vivecraft
            && !self.vivecraft.net.server_has_vivecraft
    }

    /// Visor's `TaskTeleport` is on: the server allows teleporting (`supportedMovement`) and the
    /// player's `visorMovementMode` agrees when the server allows both. `None` for the Vivecraft
    /// rules.
    pub(super) fn visor_teleport_enabled(&self) -> Option<bool> {
        if !self.visor_mechanics() {
            return None;
        }
        let settings = headset::settings();
        let own = match settings.enum_name("visorMovementMode") {
            "TELEPORT" => MovementMode::Teleport,
            _ => MovementMode::Controller,
        };
        let (passenger, flying) = self
            .player
            .as_ref()
            .map_or((false, false), |p| (p.is_passenger, p.flying));
        Some(
            movement::move_mode(own, self.vr_server_settings(), passenger, flying)
                == MovementMode::Teleport
                && !settings.seated(),
        )
    }

    /// `LocalPlayerMixin`: the teleport packet.
    pub(super) fn visor_send_teleport(&self, to: DVec3) {
        self.visor_send(&C2s::Teleport {
            x: to.x as f32,
            y: to.y as f32,
            z: to.z as f32,
        });
    }

    /// `TaskRoomCrawl`, once per tick: crawling when the head is low, if the server and the
    /// player allow it; the server hears `CRAWLING`.
    pub(super) fn visor_crawl_tick(&mut self, flags: &horizon_vivecraft::trackers::PlayerFlags) {
        if !self.visor_mechanics() {
            return;
        }
        let settings = headset::settings();
        let allowed = self.vr_server_settings().room_crawling_supported
            && settings.bool("allowCrawling")
            && !settings.seated()
            && flags.alive
            && headset::room().is_some()
            && (!settings.bool("visorHeightAuto")
                || settings.float("manualCalibration") >= horizon_visor::height::MIN_HEIGHT)
            && self
                .player
                .as_ref()
                .is_some_and(|p| !p.flying && !p.fall_flying && p.auto_spin_attack_dmg <= 0.0)
            && !flags.in_water
            && !flags.in_lava
            && !flags.passenger
            && !flags.sleeping
            && !flags.spectator
            && !flags.paused_or_screen;
        let change = if allowed {
            let vr = &self.pad.headset.vr;
            let actual = vr
                .player
                .histories
                .hmd_pivot
                .average_position(vr.clock_ms(), 0.1)
                .y;
            let now_ms = vr.clock_ms();
            let full = horizon_vivecraft::player::player_height(&settings);
            self.visor
                .crawl
                .tick_at(now_ms, actual, full, settings.float("visorCrawlThreshold"))
        } else {
            self.visor.crawl.clear()
        };
        if let Some(on) = change {
            log::info!(
                "Visor: the room {} (head pivot {:.2} m)",
                if on { "crawls" } else { "stops crawling" },
                self.pad.headset.vr.player.histories.hmd_pivot.latest().y
            );
        }
        if let Some(on) = change
            && self.visor_active()
        {
            self.visor_send(&C2s::Crawling(on));
        }
        // The local body crawls too (the server forces the same pose for everybody else).
        let crawling = self.visor.crawl.crawling;
        if let Some(p) = self.player.as_mut() {
            p.room_crawling = crawling;
        }
    }

    /// `TaskRoomClimb.onRun`, once per tick: the hands in a ladder's or a vine's face hold the
    /// player up and pulling moves it. Active when the player has room climbing on, the server
    /// supports it (`roomClimbingSupported`), the body is free (`RoomTasksUtils.isBodyFree`) and no
    /// move key or stick is pushed; the grab sends `CLIMBING` to a server that is not the
    /// player's own (and resets the fall there).
    pub(super) fn visor_climb(&mut self) {
        use horizon_visor::tasks::climb::{ClimbBody, ClimbEffect};
        let settings = headset::settings();
        let flags = self.vr_flags();
        let moving = self.pad_move().is_some_and(|m| m != [0.0, 0.0]);
        let free = flags.alive
            && !flags.passenger
            && self.sleeping.is_none()
            && !self.spectator()
            && flags.in_game
            && self
                .player
                .as_ref()
                .is_some_and(|p| !p.flying && !p.fall_flying && p.auto_spin_attack_dmg <= 0.0);
        let active = settings.bool("realisticClimbEnabled")
            && headset::room().is_some()
            && self.vr_server_settings().room_climbing_supported
            && free
            && !moving
            && self.screen == super::Screen::Playing;
        let mut task = std::mem::take(&mut self.visor.climb);
        let effects = if active {
            let Some(p) = self.player.as_ref() else {
                self.visor.climb = task;
                return;
            };
            let body = ClimbBody {
                pos: DVec3::new(p.position.x, p.position.y, p.position.z),
                delta: DVec3::new(p.velocity.x, p.velocity.y, p.velocity.z),
                half_width: f64::from(p.dimensions.width) / 2.0,
                height: f64::from(p.dimensions.height),
                world_scale: f64::from(headset::world_scale()),
            };
            let vr = &self.pad.headset.vr;
            let (world, room) = (vr.player.world_pre, vr.player.room_pre);
            let mut level = super::vr::GameClimb(self);
            task.tick(body, &world, &room, &mut level)
        } else {
            task.clear()
        };
        self.visor.climb = task;
        for effect in effects {
            match effect {
                ClimbEffect::SetPos(to) => {
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
                ClimbEffect::SetDelta(v) => {
                    if let Some(player) = self.player.as_mut() {
                        player.velocity.x = v.x;
                        player.velocity.y = v.y;
                        player.velocity.z = v.z;
                    }
                }
                ClimbEffect::NoGravity(on) => {
                    if let Some(player) = self.player.as_mut() {
                        player.no_gravity = on;
                    }
                }
                ClimbEffect::ResetFall => {
                    if let Some(player) = self.player.as_mut() {
                        player.fall_distance = 0.0;
                    }
                }
                ClimbEffect::Haptic(hand) => headset::pulse(hand == 0, 0.6, 0.04),
                ClimbEffect::StepSound(_) => {}
                ClimbEffect::SendClimbing => {
                    if self.visor_active() {
                        self.visor_send(&C2s::Climbing);
                    }
                }
            }
        }
    }

    /// `TaskSwing.onRun`: the hands swing. The server's `better_swinging` turns the swings into
    /// `SWING_ATTACK` and `SWING_BLOCK`; otherwise they click as the vanilla game does.
    pub(super) fn visor_swing(
        &mut self,
        event_loop: &dyn EventLoopCtl,
        id: winit::window::WindowId,
    ) {
        let settings = headset::settings();
        let flags = self.vr_flags();
        let hands = [headset::main_hand(), headset::off_hand()];
        if hands.iter().all(Option::is_none) || self.spectator() || self.player.is_none() {
            self.visor.swing.clear();
            return;
        }
        let server = self.vr_server_settings().clone();
        let blocking = self.used_hand().is_some_and(|h| {
            let slot = match h {
                Hand::Main => self.selected,
                Hand::Off => super::shield::OFFHAND_SLOT,
            };
            self.stack_in(slot)
                .is_some_and(|s| self.blocks_attacks(s).is_some())
        });
        // `TaskSwing.isActive`: creative is controlled by Visor's own setting.
        let grabbed = self.visor.climb.is_grabbed();
        let active = swing::is_active(
            &flags,
            self.creative,
            settings.bool("visorSwingInCreative"),
            blocking,
            server.attacks_while_blocking,
            grabbed,
        ) && settings.enum_name("weaponCollisionNew") != "OFF"
            && !settings.seated();
        if !active || self.visor.swing.waiting() {
            self.visor.swing.clear();
            return;
        }
        let world_data: VrData = self.pad.headset.vr.player.world_pre;
        let room_data: VrData = self.pad.headset.vr.player.room_pre;
        let now_ms = self.pad.headset.vr.clock_ms();
        let threshold = swing::speed_threshold(settings.float("visorSwingSpeedThreshold"));
        let grabbed =
            self.pad.headset.vr.climb.is_grabbing_ladder() || self.visor.climb.is_grabbed();
        let mut task = std::mem::take(&mut self.visor.swing);
        let mut all: Vec<SwingAction> = Vec::new();
        for hand in 0..2 {
            let item = [Hand::Main, Hand::Off][hand];
            let name = self.with_hand(item, |g| g.held().map(|s| s.item.to_string()));
            let kind = item_kind(name.as_deref());
            let pose = world_data.controller(hand);
            let q = pose.orientation();
            let input = HandInput {
                now_ms,
                pos: pose.position(),
                dir: pose.direction().as_dvec3(),
                rot: DQuat::from_xyzw(
                    f64::from(q.x),
                    f64::from(q.y),
                    f64::from(q.z),
                    f64::from(q.w),
                ),
                room_pos: room_data.controller(hand).position_f().as_dvec3(),
                room_dir: room_data.controller(hand).direction().as_dvec3(),
                head: world_data.hmd().position(),
                kind,
                reach: self.entity_reach,
                world_scale: world_data.world_scale,
                farming_tool: name.as_deref().is_some_and(|n| n.ends_with("_hoe")),
            };
            task.track(hand, &input);
            // A hand busy with an item swings nothing.
            if self.used_hand() == Some(item) {
                task.busy(hand);
                continue;
            }
            let actions = self.with_hand(item, |g| {
                let mut world = VisorSwingWorld(GameSwing(g));
                task.tick(hand, &input, threshold, grabbed, &mut world)
            });
            all.extend(actions);
        }
        self.visor.swing = task;
        self.visor_apply_swings(event_loop, id, all, &server);
    }

    fn visor_apply_swings(
        &mut self,
        event_loop: &dyn EventLoopCtl,
        id: winit::window::WindowId,
        actions: Vec<SwingAction>,
        server: &horizon_visor::settings::ServerSettings,
    ) {
        use horizon_visor::swing_effects;
        let better = swing_effects::better(server.better_swinging, self.visor_active());
        for action in actions {
            match action {
                SwingAction::Attack { hand, entity } => {
                    // `switchActiveHand`: the swinging hand attacks and mines.
                    if server.two_handed_vr {
                        self.visor.active_main = hand == 0;
                    }
                    self.pad.headset.swing_type = super::headset_hands::SwingType::Attack;
                    let feedback = swing_effects::entity_feedback(better);
                    if feedback.send_swing_attack {
                        let shift = self.player.as_ref().is_some_and(|p| p.shift_key_down);
                        self.visor_send(&C2s::SwingAttack {
                            entity_id: entity as i32,
                            shift,
                            main_hand: hand == 0,
                        });
                    }
                    if feedback.local_attack {
                        // `visor$swingAttack` + `resetAttackStrengthTicker`: the attacker's own
                        // side of `Player.attack`. The sounds are the server's.
                        self.visor_attack_locally(entity);
                    } else if feedback.vanilla_attack {
                        // `MC.gameMode.attack(player, entity)`: that entity, whatever the aim
                        // ray points at.
                        self.pad.headset.set_off_pending(hand == 1);
                        self.visor_attack_entity(entity);
                    }
                }
                SwingAction::Mine { hand, hit, hits } => {
                    if server.two_handed_vr {
                        self.visor.active_main = hand == 0;
                    }
                    self.pad.headset.swing_type = super::headset_hands::SwingType::Attack;
                    let feedback = swing_effects::block_feedback(better, hits);
                    if feedback.send_swing_block {
                        for _ in 0..hits {
                            let sequence = self.next_sequence();
                            self.visor_send(&C2s::SwingBlock {
                                pos: hit.pos,
                                direction: direction_3d(hit.face),
                                main_hand: hand == 0,
                                sequence,
                            });
                        }
                    } else if feedback.vanilla_dig && hand == 0 && self.pad.headset.trackers.idle()
                    {
                        self.visor_hold_block(event_loop, id, hit, hits, MouseButton::Left);
                    }
                    self.visor_block_feedback(hit.pos, hit.location.to_array(), &feedback);
                }
                SwingAction::UseOn { hand, hit } => {
                    self.pad.headset.set_off_pending(hand == 1);
                    self.visor_hold_block(event_loop, id, hit, 1, MouseButton::Right);
                }
                SwingAction::Haptic { hand, micros } => {
                    let (strength, seconds) = swing_effects::haptic_pulse(micros);
                    headset::pulse(hand == 0, strength, seconds);
                }
            }
        }
    }

    /// `TaskSwing.attackBetter`'s local half: `player.attack(entity)` on this client and
    /// `resetAttackStrengthTicker`.
    fn visor_attack_locally(&mut self, entity: u32) {
        if let Some(i) = self.mobs.iter().position(|m| m.id == entity && !m.removed) {
            self.remote_player_attack(i);
        }
        self.reset_attack_strength();
    }

    /// `TaskSwing.attackVanilla`: `MultiPlayerGameMode.attack(player, entity)` on the entity the
    /// swing found, then the arm swings. The attack, hit and crit sounds are the server's, as in
    /// vanilla.
    fn visor_attack_entity(&mut self, entity: u32) {
        let Some(i) = self.mobs.iter().position(|m| m.id == entity && !m.removed) else {
            return;
        };
        use horizon_protocol::packets::{
            Empty,
            play::{self, Serverbound as S},
        };
        self.send(S::Attack(play::Attack { id: entity as i32 }));
        self.remote_player_attack(i);
        self.reset_attack_strength();
        self.swing_arm();
        self.send(S::Punch(Empty));
    }

    /// What `swingMining` adds on this client (`swing_effects::block_feedback`): the block's
    /// hit sound for a server that sends none, the dust at the hit, and the watch for the
    /// break the server confirms.
    fn visor_block_feedback(
        &mut self,
        pos: [i32; 3],
        at: [f64; 3],
        feedback: &horizon_visor::swing_effects::BlockFeedback,
    ) {
        let state = self.world.block(pos[0], pos[1], pos[2]);
        if self.res.facts().is_air(state) {
            return;
        }
        if feedback.hit_sound {
            let sounds = self.res.facts().get(state).sounds.clone();
            let (volume, pitch) =
                horizon_visor::swing_effects::hit_sound(sounds.volume, sounds.pitch);
            self.audio.play_unshared(
                &sounds.hit,
                Some(pos.map(|v| f64::from(v) + 0.5)),
                volume,
                pitch,
            );
        }
        self.visor_block_dust(pos, at, state, feedback.dust);
        if feedback.watch_break {
            let tick = self.world.game_time.max(0) as u64;
            self.visor.pending_breaks.watch(pos, state, tick);
        }
    }

    /// `TaskSwing.blockDust`: `count` terrain crumbs at the hit, no push of their own, 0.6 size.
    fn visor_block_dust(
        &mut self,
        pos: [i32; 3],
        at: [f64; 3],
        state: horizon_world::registry::BlockStateId,
        count: u32,
    ) {
        if self.settings.extras.hide_block_particles {
            return;
        }
        let Some((sprite, tint)) = self.particle_sprite(state, pos) else {
            return;
        };
        for _ in 0..count {
            let mut particle = horizon_client::particles::Particle::terrain(
                self.particles.random(),
                at,
                [0.0; 3],
                sprite.clone(),
                tint,
            );
            particle.scale(horizon_visor::swing_effects::DUST_SIZE);
            self.particles.push(particle);
        }
    }

    /// `TaskSwing.confirmPendingBreaks`, every tick: a block a swing hit that the server has
    /// broken gets its break sound and particles here (the breaker's client plays them for a
    /// vanilla dig, and a swing digs nothing locally).
    pub(super) fn visor_confirm_breaks(&mut self) {
        if self.visor.pending_breaks.is_empty() {
            return;
        }
        let tick = self.world.game_time.max(0) as u64;
        let world = &self.world;
        let broken = self
            .visor
            .pending_breaks
            .confirm(tick, |p| world.block(p[0], p[1], p[2]));
        for (pos, state) in broken {
            self.break_sound_and_particles(pos, state);
        }
    }

    /// A click on the block the tip crossed: the game's own digging, held for `hits` ticks.
    fn visor_hold_block(
        &mut self,
        event_loop: &dyn EventLoopCtl,
        id: winit::window::WindowId,
        hit: BlockHit,
        hits: u32,
        button: MouseButton,
    ) {
        self.forced_hit = Some(super::Hit {
            pos: hit.pos,
            point: hit.location.to_array(),
            face: hit.face,
        });
        self.pad
            .headset
            .trackers
            .hold(button, std::time::Duration::from_millis(50) * hits);
        self.visor_click(event_loop, id, button, ElementState::Pressed);
    }

    fn visor_click(
        &mut self,
        event_loop: &dyn EventLoopCtl,
        id: winit::window::WindowId,
        button: MouseButton,
        state: ElementState,
    ) {
        self.handle_window_event(
            event_loop,
            id,
            WindowEvent::MouseInput {
                device_id: DeviceId::dummy(),
                state,
                button,
            },
            super::input_trace::InputOrigin::User,
        );
    }

    /// `ClientPacketHandler.BLOCK_DAMAGE`: another player's swung hit on a block, drawn as the
    /// game draws a crack. Stages 0 to 9 are progress; -1 clears the crack and -2 only the VR data.
    pub(super) fn visor_block_damage(&mut self, player: u128, pos: [i32; 3], stage: i32) {
        let Some(entity) = self.remote_player_entity(player) else {
            return;
        };
        match stage {
            -2 => {}
            -1 => self.destroy_block_progress(entity, pos, -1),
            s => self.destroy_block_progress(entity, pos, s.clamp(0, 9)),
        }
    }
}

/// `VRCameraOverlaps.updateCameraOverlaps`' proximity: 1 inside a solid block, falling to 0 at
/// `IN_BLOCK_EFFECT_START` (0.3 m) from its surface. `solid` says whether a block is solid
/// (a full collision box and a full render cube, as an opaque wall).
pub(super) fn block_proximity(eye: [f64; 3], solid: &dyn Fn(i32, i32, i32) -> bool) -> f32 {
    const START: f64 = 0.3;
    let cell = eye.map(|v| v.floor() as i32);
    if solid(cell[0], cell[1], cell[2]) {
        return 1.0;
    }
    let mut nearest = START;
    for axis in 0..3 {
        let into = eye[axis] - eye[axis].floor();
        for (step, distance) in [(-1, into), (1, 1.0 - into)] {
            if distance >= nearest {
                continue;
            }
            let mut at = cell;
            at[axis] += step;
            if solid(at[0], at[1], at[2]) {
                nearest = distance;
            }
        }
    }
    (1.0 - nearest / START).max(0.0) as f32
}

impl Game<'_> {
    /// Tells the XR layer how close the eyes are to a block (`visorInBlockVignette`).
    pub(super) fn visor_in_block_tick(&self) {
        if !headset::active() || !headset::settings().bool("visorInBlockVignette") {
            headset::set_in_block(0.0);
            return;
        }
        let proximity = match self.eye_position() {
            Some(eye)
                if self
                    .player
                    .as_ref()
                    .is_some_and(|p| p.health.health() > 0.0)
                    && !self.spectator() =>
            {
                block_proximity(eye, &|x, y, z| {
                    let f = self.res.facts().get(self.world.block(x, y, z));
                    f.full_collision && f.solid_render
                })
            }
            _ => 0.0,
        };
        headset::set_in_block(proximity);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // TaskSwing.isTool: HANDHELD_MISC, diggers (pickaxes, axes, shovels, hoes), shears, flint and
    // steel, brush, fishing rod, food on a stick, ArrowItem (all three arrows), debug stick;
    // weapons are swords, trident, mace.
    #[test]
    fn items_are_sorted_as_task_swing_sorts_them() {
        assert_eq!(item_kind(None), ItemKind::Empty);
        for w in [
            "minecraft:diamond_sword",
            "minecraft:trident",
            "minecraft:mace",
        ] {
            assert_eq!(item_kind(Some(w)), ItemKind::Weapon, "{w}");
        }
        for t in [
            "minecraft:iron_pickaxe",
            "minecraft:stick",
            "minecraft:tipped_arrow",
            "minecraft:brush",
            "minecraft:shears",
        ] {
            assert_eq!(item_kind(Some(t)), ItemKind::Tool, "{t}");
        }
        assert_eq!(item_kind(Some("minecraft:bread")), ItemKind::Other);
    }

    #[test]
    fn the_proximity_is_one_in_a_block_and_zero_in_the_open() {
        let wall_at = |x: i32| move |bx: i32, _: i32, _: i32| bx == x;
        assert_eq!(block_proximity([5.5, 64.5, 0.5], &wall_at(5)), 1.0);
        assert_eq!(block_proximity([2.5, 64.5, 0.5], &wall_at(5)), 0.0);
    }

    #[test]
    fn it_grows_as_the_eyes_near_a_wall() {
        // A wall in block x = 5; the eyes 0.15 m from its face (x = 4.85) are half way.
        let wall = |bx: i32, _: i32, _: i32| bx == 5;
        let half = block_proximity([4.85, 64.5, 0.5], &wall);
        assert!((half - 0.5).abs() < 1e-4, "{half}");
        // 0.3 m or more away: nothing. At the face: almost one.
        assert_eq!(block_proximity([4.6, 64.5, 0.5], &wall), 0.0);
        assert!(block_proximity([4.99, 64.5, 0.5], &wall) > 0.95);
        // A floor below counts too.
        let floor = |_: i32, by: i32, _: i32| by == 63;
        let near = block_proximity([0.5, 64.1, 0.5], &floor);
        assert!((near - (1.0 - 0.1 / 0.3)).abs() < 1e-4, "{near}");
    }
}
