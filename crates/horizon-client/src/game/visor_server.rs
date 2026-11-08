// SPDX-License-Identifier: MPL-2.0
//! The server side of Visor's `visor:channel` for the hub (LAN, the integrated server that
//! single player runs on, and the dedicated server): answers the HANDSHAKE, applies the game
//! payloads of the guests (teleport, climbing, crawling, swung attacks and blocks), and tells the
//! players who see a VR player where their body is.
//!
//! Ports the glue of `org.vmstudio.visor.core.server.network.ServerPacketHandler`,
//! `ServerNetworking.sendVRStatePacketOf` and `ServerPlayerGameModeMixin.visor$handleVrBlockDamage`
//! (Visor 0.6.1, https://github.com/VisorModStudio/Visor, commit
//! c01354fad8ac03a016c249239a04b07adeb36476), checked against VisorPlugin's `InboundHandler` and
//! `TrackerBroadcaster` (commit b4f3fc8be0e67f517c3e98c07ac1b5510eaf3d24). Copyright (C)
//! Phoenix-Ra and Visor contributors; MPL-2.0, see crates/horizon-visor/LICENSE-MPL-2.0. The
//! state machine is in the `horizon-visor` crate (`server`); this file moves bytes.
use super::commands::Target;
use super::{Game, Hand, lan::LanHost};
use horizon_protocol::packets::{configuration::CustomPayload, play, play::Clientbound as C};
use horizon_visor::{
    payload::{C2s, S2c},
    server::{Action, Disconnect, VisorServer},
    settings::ServerSettings,
};
use std::collections::HashMap;

/// The ticks a non-VR player has on a `vrOnly` server (`ServerNetworking.kickDelayedIfNoVR`: 5 s).
const VR_ONLY_GRACE_TICKS: u64 = 100;
/// How many blocks away a crack is still sent (`d * d + e * e + f * f < 1024.0`).
const CRACK_RANGE: f64 = 32.0;
/// `ServerPlayerGameModeMixin.visor$MINING_BONUS_TICKS`.
const MINING_BONUS_TICKS: u64 = 2;

/// The damage a player's swings did to a block (`visor$blockDamage` and `visor$blockDamageStart`).
struct BlockSwing {
    /// Ticks until the damage is forgotten (`swingingRepairDelay`, in ms in the settings but
    /// counted in ticks by the Java).
    delay: i64,
    damage: f32,
    start: u64,
}

/// The hub's Visor state.
#[derive(Default)]
pub(super) struct VisorServerState {
    pub(super) server: VisorServer,
    /// The tick a guest was first seen, for the `vrOnly` check.
    seen: HashMap<u128, u64>,
    /// Block damage of swings, by player and block.
    swings: HashMap<(u128, [i32; 3]), BlockSwing>,
    /// `server_settings.yml` was looked at.
    loaded: bool,
    /// `vr_prefs.properties` was read, and what it last held (to write it when a player's
    /// rotation or off-hand slot changed).
    prefs_loaded: bool,
    prefs_saved: HashMap<u128, horizon_visor::server::Prefs>,
}

fn payload(p: &S2c) -> C {
    C::CustomPayload(CustomPayload {
        channel: horizon_visor::CHANNEL.into(),
        data: p.write(),
    })
}

impl Game<'_> {
    /// The settings file of a dedicated server (`server_settings.yml` beside server.properties,
    /// as `ServerConfig.onServerInit` does); other hubs run the defaults. A missing file is
    /// written with the defaults, so it can be edited.
    fn visor_load_settings(&mut self) {
        if self.visor_server.loaded {
            return;
        }
        self.visor_server.loaded = true;
        if self.options.dedicated.is_none() {
            return;
        }
        let path = std::path::Path::new("server_settings.yml");
        match std::fs::read_to_string(path) {
            Ok(text) => {
                let mut settings = ServerSettings::default();
                settings.apply_text(&text);
                self.visor_server.server.settings = settings;
            }
            Err(_) => {
                let text = ServerSettings::default().to_file_text();
                if let Err(e) = std::fs::write(path, text) {
                    log::warn!("Visor: could not write {}: {e}", path.display());
                }
            }
        }
    }

    /// The players' rotation and off-hand slot, kept across joins (`visor$rotation_y` and
    /// `visor$offhand_slot` in the mod's player data; VisorPlugin's `vr_prefs.properties`, which
    /// is what is written here, in the world's folder): read once, written every five seconds
    /// when something changed.
    fn visor_prefs(&mut self, host: &LanHost) {
        use horizon_visor::server::{PREFS_FILE, load_prefs_text, prefs_text};
        let path = self.world_root.join(PREFS_FILE);
        if !self.visor_server.prefs_loaded {
            self.visor_server.prefs_loaded = true;
            if let Ok(text) = std::fs::read_to_string(&path) {
                let loaded = load_prefs_text(&text);
                for (uuid, prefs) in &loaded {
                    self.visor_server
                        .server
                        .prefs
                        .entry(*uuid)
                        .or_insert(*prefs);
                }
                self.visor_server.prefs_saved = loaded;
            }
        }
        let state = &mut self.visor_server;
        if !host.hub.ticks.is_multiple_of(100) || state.server.prefs == state.prefs_saved {
            return;
        }
        match std::fs::write(&path, prefs_text(&state.server.prefs)) {
            Ok(()) => state.prefs_saved = state.server.prefs.clone(),
            Err(e) => log::warn!("Visor: could not write {}: {e}", path.display()),
        }
    }

    /// A guest's `visor:channel` payload.
    pub(super) fn guest_visor(&mut self, host: &mut LanHost, conn: u32, data: &[u8]) {
        self.visor_load_settings();
        self.visor_prefs(host);
        let Some((uuid, name)) = host.hub.guest(conn).map(|g| (g.uuid, g.name.clone())) else {
            return;
        };
        let packet = match C2s::read(data) {
            Ok(p) => p,
            Err(e) => {
                log::debug!("Visor: a bad payload from {name}: {e}");
                return;
            }
        };
        let hello = matches!(packet, C2s::Handshake { .. });
        let out = self.visor_server.server.handle(uuid, packet);
        if hello
            && self.visor_server.server.settings.server_debug
            && let Some(line) = self.visor_server.server.describe(uuid)
        {
            log::info!("Visor: {name} joined with {line}");
        }
        self.visor_sync_hands(host);
        for reply in &out.replies {
            host.hub.send_to(conn, &payload(reply));
        }
        for action in out.actions {
            self.visor_action(host, conn, uuid, action);
        }
    }

    /// The effects of one payload on the guest, with the guest swapped in.
    fn visor_action(&mut self, host: &mut LanHost, conn: u32, uuid: u128, action: Action) {
        match action {
            Action::Disconnect(Disconnect::NetworkMismatch { client, server }) => {
                // `visor.messages.network_mismatch`: the player reads its own language.
                let text = horizon_client::text::Text::tr(
                    "visor.messages.network_mismatch",
                    &[&client, &server],
                );
                host.hub.kick(
                    conn,
                    horizon_client::lan::data::component_nbt(&text.component()),
                );
            }
            Action::Teleport { x, y, z } => {
                // `absSnapTo(x, y, z)`, keeping the rotation. The Java does not check the
                // limits either: they are the client's.
                let pos = [f64::from(x), f64::from(y), f64::from(z)];
                let _ = self.as_guest(host, conn, |g| g.teleport_to(Target::Player, pos, None));
            }
            Action::ResetFall => {
                let _ = self.as_guest(host, conn, |g| {
                    if let Some(p) = g.player.as_mut() {
                        p.fall_distance = 0.0;
                    }
                });
            }
            Action::Crawl(on) => {
                if on {
                    let _ = self.as_guest(host, conn, |g| {
                        if let Some(p) = g.player.as_mut() {
                            let _ = p.set_pose(crate::pose::Pose::Swimming);
                        }
                    });
                }
            }
            Action::SwingAttack {
                entity_id,
                shift,
                main_hand,
            } => {
                let prior = self.visor_swing_hand(host, conn, main_hand);
                self.visor_swing_attack(host, conn, entity_id, shift, main_hand);
                self.visor_swing_hand_restore(host, conn, prior);
            }
            Action::SwingBlock {
                pos,
                main_hand,
                sequence,
                ..
            } => {
                let prior = self.visor_swing_hand(host, conn, main_hand);
                self.visor_swing_block(host, conn, uuid, pos, main_hand, sequence);
                self.visor_swing_hand_restore(host, conn, prior);
            }
        }
    }

    /// `visor$swingAttack(entity, hand)` and the block swings name their hand: for the whole
    /// action that hand acts, whatever `ACTIVE_HAND` says. Returns what to put back.
    fn visor_swing_hand(&mut self, host: &mut LanHost, conn: u32, main_hand: bool) -> Option<bool> {
        let two = self.visor_server.server.settings.two_handed_vr;
        let g = host.hub.guests.iter_mut().find(|g| g.conn == conn)?;
        Some(std::mem::replace(&mut g.active_hand_off, two && !main_hand))
    }

    fn visor_swing_hand_restore(&mut self, host: &mut LanHost, conn: u32, prior: Option<bool>) {
        if let (Some(prior), Some(g)) = (prior, host.hub.guests.iter_mut().find(|g| g.conn == conn))
        {
            g.active_hand_off = prior;
        }
    }

    /// Where each guest's off hand is and which hand is active, from what the Visor server knows
    /// (`VROffhandEquipment.redirecting`, `getActiveHand`). Run when a payload came in and every
    /// tick, so a swing, a use or a dig in between reads the current slot.
    pub(super) fn visor_sync_hands(&mut self, host: &mut LanHost) {
        let server = &self.visor_server.server;
        for g in host.hub.guests.iter_mut() {
            g.offhand_redirect = server.offhand_redirect(g.uuid);
            g.active_hand_off = g.offhand_redirect.is_some() && !server.active_hand_is_main(g.uuid);
        }
    }

    /// `SWING_ATTACK`: `serverPlayer.setShiftKeyDown(shift)`, then the attack from the hand.
    /// Spectators do nothing; the target has to be in reach (`guest_attack` checks it) and
    /// not an item, an experience orb or an arrow (the Java disconnects the player).
    fn visor_swing_attack(
        &mut self,
        host: &mut LanHost,
        conn: u32,
        entity_id: i32,
        shift: bool,
        main_hand: bool,
    ) {
        let Some(gi) = host.hub.guests.iter().position(|g| g.conn == conn) else {
            return;
        };
        if host.hub.guests[gi].spectator() {
            return;
        }
        // An item, an experience orb or an arrow in reach is not a target: the player that swings at
        // one is sent away (`multiplayer.disconnect.invalid_entity_attacked`), as vanilla does for an
        // Interact packet; one out of reach is ignored.
        let kind = self
            .level_entity(entity_id)
            .map(|t| self.target_type(t).to_string());
        if kind.as_deref().is_some_and(|k| {
            matches!(
                k,
                "minecraft:item"
                    | "minecraft:experience_orb"
                    | "minecraft:arrow"
                    | "minecraft:spectral_arrow"
                    | "minecraft:trident"
            )
        }) {
            let in_reach = self.guest_target_box(host, gi, entity_id).is_some_and(|b| {
                self.inside_border(&b)
                    && self.as_guest(host, conn, |g| g.within_attack_range(&b)) == Some(true)
            });
            if in_reach {
                log::warn!("Visor: a player tried to attack an invalid entity");
                let text = horizon_client::text::Text::tr(
                    "multiplayer.disconnect.invalid_entity_attacked",
                    &[],
                );
                host.hub.kick(
                    conn,
                    horizon_client::lan::data::component_nbt(&text.component()),
                );
            }
            return;
        }
        // `noAttackWhileShieldUp`: with `attacksWhileBlocking` off, a raised shield attacks nothing.
        if !self.visor_server.server.settings.attacks_while_blocking {
            let blocking = self
                .as_guest(host, conn, |g| {
                    g.used_hand().is_some_and(|h| {
                        let slot = match h {
                            Hand::Main => g.selected,
                            Hand::Off => super::shield::OFFHAND_SLOT,
                        };
                        g.stack_in(slot)
                            .is_some_and(|s| g.blocks_attacks(s).is_some())
                    })
                })
                .unwrap_or(false);
            if blocking {
                return;
            }
        }
        let _ = self.as_guest(host, conn, |g| {
            if let Some(p) = g.player.as_mut() {
                p.shift_key_down = shift;
            }
        });
        // `visor$swingAttack(entity, hand)`: the hand acts for the whole attack.
        let hand = if main_hand || !self.visor_server.server.settings.two_handed_vr {
            Hand::Main
        } else {
            Hand::Off
        };
        let prior = std::mem::replace(&mut self.hand, hand);
        self.guest_attack(host, conn, entity_id);
        self.hand = prior;
    }

    /// `SWING_BLOCK`: `visor$handleVrBlockDamage`. Each hit adds the block's per-tick destroy
    /// progress, capped by what the time since the first hit allows (`elapsed + 2` ticks at
    /// `swingingMiningSpeed`); a full block breaks, otherwise its crack goes to the others and
    /// the VR players hear `BLOCK_DAMAGE`.
    fn visor_swing_block(
        &mut self,
        host: &mut LanHost,
        conn: u32,
        uuid: u128,
        pos: [i32; 3],
        main_hand: bool,
        sequence: i32,
    ) {
        let Some(gi) = host.hub.guests.iter().position(|g| g.conn == conn) else {
            return;
        };
        host.hub.ack(conn, sequence);
        if host.hub.guests[gi].spectator() {
            return;
        }
        let entity = host.hub.guests[gi].entity_id;
        let restore = |this: &Self, host: &LanHost| {
            let state = this.world.block(pos[0], pos[1], pos[2]).0 as i32;
            host.hub
                .send_to(conn, &C::BlockUpdate(play::BlockUpdate { pos, state }));
        };
        // "too far", "may not interact" and spawn protection: the block goes back.
        if !host.hub.guests[gi].may_reach_block(pos)
            || !self.border.contains(f64::from(pos[0]), f64::from(pos[2]))
            || self.under_spawn_protection(host, gi, pos)
        {
            restore(self, host);
            return;
        }
        let hand = if main_hand || !self.visor_server.server.settings.two_handed_vr {
            Hand::Main
        } else {
            Hand::Off
        };
        let creative = host.hub.guests[gi].creative();
        let restricted = self
            .as_guest(host, conn, |g| g.block_action_restricted(pos))
            .unwrap_or(false);
        if creative {
            self.guest_destroy(host, conn, pos);
            self.visor_swing_clean(host, conn, uuid, entity, pos, false);
            return;
        }
        if restricted {
            restore(self, host);
            return;
        }
        if self
            .res
            .facts()
            .is_air(self.world.block(pos[0], pos[1], pos[2]))
        {
            return;
        }
        // `blockState.attack(level, pos, player)` (notes, buttons...), then the progress.
        let prior = std::mem::replace(&mut self.hand, hand);
        let step = self
            .as_guest(host, conn, |g| {
                let _ = g.attack_block(pos);
                g.destroy_progress(pos)
            })
            .unwrap_or(0.0);
        self.hand = prior;
        let ticks = host.hub.ticks;
        let settings = self.visor_server.server.settings.clone();
        let entry = self
            .visor_server
            .swings
            .entry((uuid, pos))
            .or_insert(BlockSwing {
                delay: settings.swinging_repair_delay,
                damage: 0.0,
                start: ticks,
            });
        let elapsed = ticks.saturating_sub(entry.start);
        let speed = settings.swinging_mining_speed.max(0.1);
        let allowed = step * (elapsed + MINING_BONUS_TICKS) as f32 * speed;
        let saved = entry.damage;
        let damage = saved.max((saved + step).min(allowed));
        if damage >= 1.0 {
            // "instant mine": the block breaks and the damage is forgotten.
            self.guest_destroy(host, conn, pos);
            self.visor_swing_clean(host, conn, uuid, entity, pos, false);
            return;
        }
        entry.damage = damage;
        entry.delay = settings.swinging_repair_delay;
        let stage = (damage * 10.0) as i32;
        self.destroy_block_progress(entity, pos, stage);
        let at = [f64::from(pos[0]), f64::from(pos[1]), f64::from(pos[2])];
        let crack = C::BlockDestruction(play::BlockDestruction {
            id: entity,
            pos,
            progress: stage as u8,
        });
        host.hub.broadcast_near(at, 64.0, &crack, Some(conn));
        // The breaker's own crack: `destroyBlockProgress` leaves the breaker out.
        let me = host.hub.guests[gi].position;
        let d = [at[0] - me[0], at[1] - me[1], at[2] - me[2]];
        if d[0] * d[0] + d[1] * d[1] + d[2] * d[2] < CRACK_RANGE * CRACK_RANGE {
            host.hub.send_to(conn, &crack);
        }
        self.visor_to_trackers(
            host,
            entity,
            uuid,
            &S2c::BlockDamage {
                player: uuid,
                pos,
                destroy_stage: stage,
            },
            true,
        );
    }

    /// `visor$sendSwingDamageCleanUp`: the damage is forgotten and its crack goes (`-1`).
    fn visor_swing_clean(
        &mut self,
        host: &mut LanHost,
        conn: u32,
        uuid: u128,
        entity: i32,
        pos: [i32; 3],
        only_vr_data: bool,
    ) {
        self.visor_server.swings.remove(&(uuid, pos));
        self.visor_to_trackers(
            host,
            entity,
            uuid,
            &S2c::BlockDamage {
                player: uuid,
                pos,
                destroy_stage: if only_vr_data { -2 } else { -1 },
            },
            true,
        );
        if !only_vr_data {
            self.guest_crack_gone(host, conn, pos);
            host.hub.send_to(
                conn,
                &C::BlockDestruction(play::BlockDestruction {
                    id: entity,
                    pos,
                    progress: 255,
                }),
            );
        }
    }

    /// `sendPacketToTrackedVRPlayers`: the players with a Visor session that see `entity`, and
    /// the player itself when `send_self`.
    fn visor_to_trackers(&self, host: &LanHost, entity: i32, uuid: u128, p: &S2c, send_self: bool) {
        let packet = payload(p);
        for g in host.hub.guests.iter().filter(|g| g.placed) {
            let mine = g.uuid == uuid;
            let sees = g.tracker.is_tracking(entity)
                && self.visor_server.server.sessions.contains_key(&g.uuid);
            if (mine && send_self) || (!mine && sees) {
                host.hub.send_to(g.conn, &packet);
            }
        }
    }

    /// `ServerPlayerMixin.visor$poseBlocks`: a Visor VR player holding a shield up against a hit
    /// blocks it without using the shield (`roomscale_shield`). The slot of the shield and the
    /// coverage (above 0.5, a 120 degree cone). A projectile is judged by the direction from the
    /// hand to where it came from, other hits by the flat direction; the Java moves a
    /// projectile's source one block back along its flight, which is not known here (the source is
    /// its position).
    pub(super) fn visor_roomscale_shield(
        &self,
        from: Option<[f64; 3]>,
        projectile: bool,
        bypassed: &dyn Fn(&horizon_client::items::BlocksAttacks) -> bool,
    ) -> Option<(usize, f64)> {
        use horizon_visor::buffer::PoseElement;
        let server = &self.visor_server.server;
        if !server.settings.roomscale_shield_blocking {
            return None;
        }
        let from = glam::DVec3::from_array(from?);
        let uuid = self.lan_acting_uuid?;
        let vr = server.vr_state(uuid)?;
        let pose = vr.pose?;
        let pos = self.player.as_ref()?.position;
        let feet = glam::DVec3::new(pos.x, pos.y, pos.z);
        for (hand, slot, element) in [
            (Hand::Main, self.selected, pose.main_hand),
            (Hand::Off, super::shield::OFFHAND_SLOT, pose.off_hand),
        ] {
            let PoseElement {
                position,
                orientation,
            } = element;
            let Some(stack) = self.stack_in(slot) else {
                continue;
            };
            let Some(component) = self.blocks_attacks(stack) else {
                continue;
            };
            if bypassed(&component) {
                continue;
            }
            let group = horizon_client::cooldowns::group(
                stack,
                self.items()
                    .get(&stack.item)
                    .and_then(|i| i.use_cooldown.as_ref()),
            );
            if self.cooldowns.is_on_cooldown(&group) {
                continue;
            }
            let palm_faces_left = (hand == Hand::Main) == vr.left_handed;
            let axis = if palm_faces_left {
                glam::Vec3::NEG_X
            } else {
                glam::Vec3::X
            };
            let facing = (orientation * axis).as_dvec3();
            let coverage = if projectile {
                let hand_pos = feet + position.as_dvec3();
                facing.dot((from - hand_pos).normalize_or_zero())
            } else {
                let to = glam::DVec2::new(from.x - feet.x, from.z - feet.z).normalize_or_zero();
                let flat = glam::DVec2::new(facing.x, facing.z).normalize_or_zero();
                flat.dot(to)
            };
            if coverage > 0.5 {
                return Some((slot, coverage));
            }
        }
        None
    }

    /// `ServerPlayerMixin.visor$blockPvpDamage`: VR against VR and VR against other players can
    /// each be switched off. True when this hit is not allowed; the attacker is told when
    /// `notifyPvpBlocked`.
    pub(super) fn visor_pvp_blocked(&self, host: &LanHost, attacker: u32, victim: u32) -> bool {
        let server = &self.visor_server.server;
        let (Some(a), Some(v)) = (host.hub.guest(attacker), host.hub.guest(victim)) else {
            return false;
        };
        let (a_vr, v_vr) = (server.is_vr(a.uuid), server.is_vr(v.uuid));
        let both = a_vr && v_vr;
        let one = a_vr != v_vr;
        let blocked =
            (both && !server.settings.pvp_vr_vs_vr) || (one && !server.settings.pvp_vr_vs_vanilla);
        if blocked && server.settings.notify_pvp_blocked {
            let key = if both {
                "visor.messages.pvp_blocked_vr"
            } else {
                "visor.messages.pvp_blocked_non_vr"
            };
            let text = horizon_client::text::Text::tr(key, &[]);
            host.hub.send_to(
                attacker,
                &C::SystemChat(play::SystemChat {
                    content: horizon_client::lan::data::component_nbt(&text.component()),
                    overlay: false,
                }),
            );
        }
        blocked
    }

    /// Once per server tick: swung block damage decays, the `vrOnly` rule, VR players' poses go
    /// to the players who see them, and players who left are forgotten.
    pub(super) fn flush_visor(&mut self, host: &mut LanHost) {
        self.visor_prefs(host);
        self.visor_sync_hands(host);
        let state = &self.visor_server;
        if state.server.sessions.is_empty()
            && !(state.server.settings.vr_only && self.options.dedicated.is_some())
            && state.swings.is_empty()
        {
            return;
        }
        self.visor_load_settings();
        let ticks = host.hub.ticks;
        let here: Vec<(u128, i32, u32, u8)> = host
            .hub
            .guests
            .iter()
            .filter(|g| g.placed)
            .map(|g| (g.uuid, g.entity_id, g.conn, g.permission))
            .collect();
        // Gone players.
        let gone: Vec<u128> = self
            .visor_server
            .server
            .sessions
            .keys()
            .copied()
            .filter(|u| !here.iter().any(|(h, ..)| h == u))
            .collect();
        for uuid in gone {
            self.visor_server.server.remove(uuid);
            self.visor_server.swings.retain(|(u, _), _| *u != uuid);
        }
        self.visor_server
            .seen
            .retain(|u, _| here.iter().any(|(h, ..)| h == u));
        // `ServerNetworking.kickDelayedIfNoVR`: a non-VR player on a `vrOnly` server leaves after
        // five seconds, unless it is an operator.
        if self.visor_server.server.settings.vr_only {
            for (uuid, _, conn, permission) in &here {
                let first = *self.visor_server.seen.entry(*uuid).or_insert(ticks);
                if ticks.saturating_sub(first) >= VR_ONLY_GRACE_TICKS
                    && *permission < 2
                    && !self.visor_server.server.is_vr(*uuid)
                {
                    let text = horizon_client::text::Text::tr("visor.messages.server_vr_only", &[]);
                    host.hub.kick(
                        *conn,
                        horizon_client::lan::data::component_nbt(&text.component()),
                    );
                }
            }
        }
        // `visor$tickDecayVrDamage`: damage not renewed is forgotten (and a block that is gone).
        let expired: Vec<(u128, [i32; 3])> = self
            .visor_server
            .swings
            .iter_mut()
            .filter_map(|(key, s)| {
                let air = self
                    .res
                    .facts()
                    .is_air(self.world.block(key.1[0], key.1[1], key.1[2]));
                if air || s.delay <= 0 {
                    Some(*key)
                } else {
                    s.delay -= 1;
                    None
                }
            })
            .collect();
        for (uuid, pos) in expired {
            if let Some((_, entity, conn, _)) = here.iter().find(|(u, ..)| *u == uuid).copied() {
                self.visor_swing_clean(host, conn, uuid, entity, pos, false);
            } else {
                self.visor_server.swings.remove(&(uuid, pos));
            }
        }
        // `visor$tickBodyPose`: a crawling player keeps the swimming pose.
        for (uuid, _, conn, _) in &here {
            if self
                .visor_server
                .server
                .vr_state(*uuid)
                .is_some_and(|v| v.crawling)
            {
                let _ = self.as_guest(host, *conn, |g| {
                    if let Some(p) = g.player.as_mut() {
                        let _ = p.set_pose(crate::pose::Pose::Swimming);
                    }
                });
            }
        }
        // `sendVRStatePacketOf` for every VR player.
        for (uuid, entity, ..) in &here {
            if !self.visor_server.server.is_vr(*uuid) {
                continue;
            }
            let tracking: Vec<u128> = host
                .hub
                .guests
                .iter()
                .filter(|g| g.placed && g.tracker.is_tracking(*entity))
                .map(|g| g.uuid)
                .collect();
            // One packet for the players who hear the same payload, not one made for each.
            for (receivers, p) in self
                .visor_server
                .server
                .state_updates_grouped(*uuid, &tracking)
            {
                let packet = payload(&p);
                for to in receivers {
                    if let Some(g) = host.hub.guests.iter().find(|g| g.uuid == to) {
                        host.hub.send_to(g.conn, &packet);
                    }
                }
            }
        }
    }
}
