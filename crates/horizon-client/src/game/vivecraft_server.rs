// SPDX-License-Identifier: LGPL-3.0-or-later
//! The server side of Vivecraft's `vivecraft:data` channel for the hub (LAN, the
//! integrated server that single player runs on, and the dedicated server): answers the
//! handshake and collects the guests' poses, then tells the players who see a VR player
//! where their body is.
//!
//! Ports the glue of `org.vivecraft.server.ServerNetworking.handlePacket` and
//! `sendVrPlayerStateToClients` and the `ServerGamePacketListenerImpl` mixin
//! (Vivecraft, https://github.com/Vivecraft/VivecraftMod, commit
//! 8b4d6b95a1891d9839f19b45c13e93415c02a31e). Copyright (c) Vivecraft contributors;
//! LGPL-3.0-or-later, see crates/horizon-vivecraft/LICENSE-LGPL-3.0. The logic is in the
//! `horizon-vivecraft` crate (`server`); this file moves bytes.
use super::commands::Target;
use super::{Game, lan::LanHost};
use horizon_protocol::packets::{configuration::CustomPayload, play::Clientbound as C};
use horizon_vivecraft::{
    network::{self, C2s, S2c},
    server::{self, Action, VrServer},
};

/// The hub's Vivecraft state.
#[derive(Default)]
pub(super) struct VivecraftServer {
    pub(super) server: VrServer,
    /// Headshots whose indicator is not sent yet: the shooter, where the arrow hit and the
    /// way it flew (`bowHeadshotIndicator`).
    headshots: std::cell::RefCell<Vec<(u128, [f64; 3], [f64; 3])>>,
}

fn payload(p: &S2c) -> C {
    C::CustomPayload(CustomPayload {
        channel: network::CHANNEL.into(),
        data: p.write(),
    })
}

impl Game<'_> {
    /// `ServerVivePlayer.getAimDir` and `getAimPos` for a VR guest: its ray for this turn, and the
    /// angles of it, which stand for the guest's look while it acts (projectiles, picks).
    pub(super) fn guest_vr_aim(
        &mut self,
        uuid: u128,
        position: [f64; 3],
        yaw: f32,
        pitch: f32,
    ) -> Option<[f32; 2]> {
        // `AimFixHandler` fixes the aim of UseItem, UseItemOn and PlayerAction packets only.
        if !self.vr_aim_packet {
            return None;
        }
        let player = self.vivecraft_server.server.vive_player(uuid)?;
        if !player.is_vr() || player.vr_player_state().is_none() {
            return None;
        }
        let feet = glam::DVec3::from_array(position);
        let look = glam::DVec3::from_array(super::view_vector(yaw, pitch)).as_vec3();
        let dir = player.aim_dir(false, feet, look);
        let at = player.aim_pos(false, feet, look);
        self.vr_aim = Some((at.to_array(), dir.as_dvec3().to_array()));
        let angles = [
            (-dir.x).atan2(dir.z).to_degrees(),
            (-dir.y).clamp(-1.0, 1.0).asin().to_degrees(),
        ];
        [self.yaw, self.pitch] = angles;
        Some(angles)
    }

    /// `AbstractArrowMixin.vivecraft$damageMultiplier` for the player whose turn it is: how much
    /// more an arrow hurts `who` for a VR bow and for a hit on the head.
    pub(super) fn vr_arrow_multiplier(
        &self,
        arrow: &horizon_client::projectiles::Projectile,
        who: horizon_client::mobs::Who,
    ) -> f64 {
        use horizon_vivecraft::server;
        if arrow.owner != Some(horizon_client::mobs::Who::Player) {
            return 1.0;
        }
        let Some(uuid) = self.lan_acting_uuid else {
            return 1.0;
        };
        let server_state = &self.vivecraft_server.server;
        let shooter = server_state
            .vive_player(uuid)
            .filter(|p| p.is_vr())
            .map(|p| p.is_seated());
        let config = &server_state.config;
        let mut headshot = false;
        let horizon_client::mobs::Who::Mob(id) = who else {
            return server::arrow_multiplier(config, shooter, false);
        };
        if let Some(mob) = self.mobs.iter().find(|m| m.id == id) {
            let kind = server::head_kind(&mob.kind.id, false);
            let feet = glam::DVec3::new(mob.position.x, mob.position.y, mob.position.z);
            if let Some(head) = server::head_hitbox(
                kind,
                feet,
                mob.kind.eye_height,
                mob.kind.width,
                mob.is_baby(),
                mob.body_yaw,
                0.3,
            ) {
                let half = mob.kind.width * 0.5;
                let body = (
                    feet - glam::DVec3::new(half, 0.0, half),
                    feet + glam::DVec3::new(half, mob.kind.height, half),
                );
                let at = glam::DVec3::new(arrow.position.x, arrow.position.y, arrow.position.z);
                let velocity =
                    glam::DVec3::new(arrow.velocity.x, arrow.velocity.y, arrow.velocity.z);
                headshot = server::is_headshot(at, velocity, head, body);
            }
        }
        let multiplier = server::arrow_multiplier(config, shooter, headshot);
        if headshot
            && multiplier > 1.0
            && config.bow_headshot_indicator != server::HeadshotIndicator::None
        {
            self.vivecraft_server.headshots.borrow_mut().push((
                uuid,
                [arrow.position.x, arrow.position.y, arrow.position.z],
                [arrow.velocity.x, arrow.velocity.y, arrow.velocity.z],
            ));
        }
        multiplier
    }

    /// `AbstractArrowMixin.vivecraft$damageMultiplier`'s indicator: crit particles at the hit,
    /// flying back against the arrow, and the item break sound, to the shooter only.
    pub(super) fn vr_flush_headshots(&self, host: &LanHost) {
        use horizon_protocol::{entity_data::Particle, packets::play};
        let hits = std::mem::take(&mut *self.vivecraft_server.headshots.borrow_mut());
        if hits.is_empty() {
            return;
        }
        let indicator = self.vivecraft_server.server.config.bow_headshot_indicator;
        let particle = host
            .hub
            .registry_id("minecraft:particle_type", "minecraft:crit");
        for (uuid, at, velocity) in hits {
            let Some(shooter) = host.hub.guests.iter().find(|g| g.uuid == uuid) else {
                continue;
            };
            if let (Some(id), true) = (particle, indicator != server::HeadshotIndicator::Audio) {
                shooter.send(&C::LevelParticles(play::LevelParticles {
                    particle: Particle { id, data: None },
                    override_limiter: true,
                    always_show: true,
                    pos: at,
                    dist: velocity.map(|v| -v as f32),
                    max_speed: [0.1; 3],
                    count: 5,
                    randomization: 0,
                }));
            }
            if indicator != server::HeadshotIndicator::Visual {
                let seed = host.hub.next_sound_seed();
                // SoundSource.PLAYERS, 0.7 volume, 0.5 pitch, at the shooter.
                shooter.send(&host.hub.sound_packet(
                    "minecraft:entity.item.break",
                    shooter.position,
                    0.7,
                    0.5,
                    7,
                    seed,
                ));
            }
        }
    }

    /// `ServerPlayerMixin.vivecraft$roomscaleShieldBlockingItem`: a VR player holding a shield
    /// up against a hit from `from` blocks it. The slot of the shield and the dot product of
    /// its facing with the direction of the hit (above 0.5, a 120 degree cone). A projectile
    /// is judged by the direction from the hand to a point 1.5 m (plus half the player's
    /// width) back along its flight; other hits by the flat direction to the hit.
    /// `bypassed` says the shield's `bypassed_by` holds the damage type.
    pub(super) fn vr_roomscale_shield(
        &self,
        from: Option<[f64; 3]>,
        projectile: bool,
        bypassed: &dyn Fn(&horizon_client::items::BlocksAttacks) -> bool,
    ) -> Option<(usize, f64)> {
        use horizon_vivecraft::network::VrBodyPart;
        let server = &self.vivecraft_server.server;
        let from = from?;
        if !server.config.allow_roomscale_shield_blocking {
            return None;
        }
        let vive = self
            .lan_acting_uuid
            .and_then(|u| server.vive_player(u))
            .filter(|p| p.is_vr() && p.vr_player_state().is_some())?;
        let pos = self.player.as_ref()?.position;
        let feet = glam::DVec3::new(pos.x, pos.y, pos.z);
        let look = glam::DVec3::from_array(super::view_vector(self.yaw, self.pitch)).as_vec3();
        let to_hit =
            glam::Vec2::new((from[0] - pos.x) as f32, (from[2] - pos.z) as f32).try_normalize()?;
        // The projectile's check position: back along its flight (towards the player's
        // centre, as the arrow flies) by half the width plus 1.5 m from that centre.
        let centre = feet + glam::DVec3::new(0.0, 0.9, 0.0);
        let behind = (centre - glam::DVec3::from_array(from)).normalize_or_zero();
        let check = centre - behind * (0.3 + 1.5);
        for (part, slot) in [
            (VrBodyPart::MainHand, self.selected),
            (VrBodyPart::OffHand, super::shield::OFFHAND_SLOT),
        ] {
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
            let main = part == VrBodyPart::MainHand;
            let side = if vive.is_left_handed() == main {
                horizon_vivecraft::math::RIGHT
            } else {
                horizon_vivecraft::math::LEFT
            };
            let facing = vive.body_part_vector_custom(part, side, look);
            let dot = if projectile {
                let hand = vive.body_part_pos(part, false, feet, look);
                let dir = (check - hand).normalize_or_zero().as_vec3();
                f64::from(facing.dot(dir))
            } else {
                let Some(flat) = glam::Vec2::new(facing.x, facing.z).try_normalize() else {
                    continue;
                };
                f64::from(flat.dot(to_hit))
            };
            if dot > 0.5 {
                return Some((slot, dot));
            }
        }
        None
    }

    /// The cooldown a roomscale block puts on its shield: `roomscaleShieldCooldown` ticks for a
    /// hit of `damage` at least the trigger.
    pub(super) fn vr_shield_cooldown(&mut self, slot: usize, damage: f32) {
        let config = &self.vivecraft_server.server.config;
        let ticks = config.roomscale_shield_cooldown;
        if ticks <= 0 || f64::from(damage) < config.roomscale_shield_cooldown_damage_trigger {
            return;
        }
        let Some(stack) = self.stack_in(slot) else {
            return;
        };
        let group = horizon_client::cooldowns::group(
            stack,
            self.items()
                .get(&stack.item)
                .and_then(|i| i.use_cooldown.as_ref()),
        );
        self.cooldowns.add(group, ticks);
    }

    /// Dual wielding: the VR player whose turn it is acts with the item in its off hand.
    pub(super) fn vr_dual_wield_swapped(&self) -> bool {
        // Visor: `ACTIVE_HAND` on the off hand in two-handed play (`getWeaponItem`,
        // `getDestroySpeed`).
        if self.active_off && self.offhand_redirect.is_some() {
            return true;
        }
        let server = &self.vivecraft_server.server;
        server.config.dual_wielding
            && self
                .lan_acting_uuid
                .and_then(|u| server.vive_player(u))
                .is_some_and(|p| {
                    p.is_vr()
                        && p.active_item_body_part()
                            == horizon_vivecraft::network::VrBodyPart::OffHand
                })
    }

    /// `ProjectileMixin`: a drawn VR bow shoots at the draw its client sent.
    pub(super) fn vr_draw_factor(&self) -> f32 {
        self.lan_acting_uuid
            .and_then(|u| self.vivecraft_server.server.vive_player(u))
            .filter(|p| p.is_vr() && p.is_drawing())
            .map_or(1.0, |p| p.draw)
    }

    /// A guest's `vivecraft:data` payload.
    pub(super) fn guest_vivecraft(&mut self, host: &mut LanHost, conn: u32, data: &[u8]) {
        let Some(uuid) = host.hub.guest(conn).map(|g| g.uuid) else {
            return;
        };
        let packet = match C2s::read(data) {
            Ok(p) => p,
            Err(e) => {
                log::debug!("Vivecraft: a bad payload from a player: {e}");
                return;
            }
        };
        let out = self.vivecraft_server.server.handle(uuid, packet);
        for reply in &out.replies {
            host.hub.send_to(conn, &payload(reply));
        }
        if out.vr_off_broadcast {
            let off = payload(&S2c::IsVrActive {
                vr: false,
                player: uuid,
            });
            if let Some(id) = host.hub.guest(conn).map(|g| g.entity_id) {
                host.hub.send_to_tracking(id, &off);
            }
        }
        // `ServerNetworking.handlePacket`'s effects on the player, with the guest swapped in.
        for action in out.actions {
            match action {
                Action::Teleport { x, y, z } => {
                    // `absSnapTo(x, y, z)`, keeping the rotation.
                    let pos = [f64::from(x), f64::from(y), f64::from(z)];
                    let _ = self.as_guest(host, conn, |g| g.teleport_to(Target::Player, pos, None));
                }
                Action::FoodExhaustion(amount) => {
                    let _ = self.as_guest(host, conn, |g| {
                        if let Some(p) = g.player.as_mut()
                            && !g.creative
                        {
                            p.cause_food_exhaustion(amount);
                        }
                    });
                }
                // Climbing: the fall resets (the anti-fly counter is not kept).
                Action::ResetFall => {
                    let _ = self.as_guest(host, conn, |g| {
                        if let Some(p) = g.player.as_mut() {
                            p.fall_distance = 0.0;
                        }
                    });
                }
                Action::CrawlPose => {
                    let _ = self.as_guest(host, conn, |g| {
                        if let Some(p) = g.player.as_mut() {
                            let _ = p.set_pose(crate::pose::Pose::Swimming);
                        }
                    });
                }
                Action::Message(text) => {
                    host.hub.send_to(
                        conn,
                        &C::SystemChat(horizon_protocol::packets::play::SystemChat {
                            content: horizon_client::lan::data::component_nbt(
                                &horizon_client::text::Text::literal(text).component(),
                            ),
                            overlay: false,
                        }),
                    );
                }
                // The attribute swap of the dual wielding hand: the host's equipment code
                // reads the active body part (kept in `VrServer`) when it applies modifiers.
                Action::SwapActiveItem { .. } => {
                    let _ = self.as_guest(host, conn, |g| g.apply_equipment());
                }
                // `ServerUtil.kickIfNotAllowed` and the like: the player leaves with the reason.
                Action::Kick(reason) => {
                    let text = horizon_client::text::Text::literal(reason);
                    host.hub.kick(
                        conn,
                        horizon_client::lan::data::component_nbt(&text.component()),
                    );
                }
            }
        }
    }

    /// Once per server tick: the aim overrides time out, VR players' poses go to the
    /// players who see them, and players who left are forgotten.
    pub(super) fn flush_vivecraft(&mut self, host: &mut LanHost) {
        if self.vivecraft_server.server.players.is_empty() {
            return;
        }
        let here: Vec<(u128, i32)> = host
            .hub
            .guests
            .iter()
            .filter(|g| g.placed)
            .map(|g| (g.uuid, g.entity_id))
            .collect();
        let gone: Vec<u128> = self
            .vivecraft_server
            .server
            .players
            .keys()
            .copied()
            .filter(|u| !here.iter().any(|(h, _)| h == u))
            .collect();
        for uuid in gone {
            self.vivecraft_server.server.remove(uuid);
        }
        for (uuid, entity) in &here {
            self.vivecraft_server.server.tick_player(*uuid);
            let tracking: Vec<u128> = host
                .hub
                .guests
                .iter()
                .filter(|g| g.placed && g.tracker.is_tracking(*entity))
                .map(|g| g.uuid)
                .collect();
            for (to, p) in self.vivecraft_server.server.state_updates(*uuid, &tracking) {
                if let Some(g) = host.hub.guests.iter().find(|g| g.uuid == to) {
                    host.hub.send_to(g.conn, &payload(&p));
                }
            }
        }
    }
}
