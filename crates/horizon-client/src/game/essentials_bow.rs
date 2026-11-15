// SPDX-License-Identifier: LGPL-3.0-only
// Copyright (c) 2025-2026 Phoenix-Ra and VisorEssentials contributors.
//! Native BowItemTask and EssentialsChannel integration; upstream revision in docs/modded.md.
use super::{Game, Hand, lan::LanHost};
use glam::Vec3;
use horizon_client::headset;
use horizon_protocol::packets::{
    configuration::CustomPayload,
    play::{self, Clientbound as C, Serverbound as S},
};
use starling_essentials::{
    bow::{Action, Input},
    network::{self, Payload},
};

#[derive(Default)]
pub(super) struct State {
    pub(super) bow: starling_essentials::bow::Bow,
    pub(super) supported: bool,
    saved_main: Option<bool>,
    pub(super) tension: std::collections::HashMap<u128, f32>,
    pub(super) registered: std::collections::HashSet<u128>,
    server_loaded: bool,
    server_bow: bool,
}
impl Game<'_> {
    pub(super) fn essentials_bow_wanted(&self) -> bool {
        headset::active()
            && !headset::settings().seated()
            && self.settings.vr.bool("essentialsBetterBow")
            && self.essentials.supported
    }
    pub(super) fn essentials_send(&self, payload: Payload) {
        self.send(S::CustomPayload(CustomPayload {
            channel: network::CHANNEL.into(),
            data: payload.write(),
        }));
    }
    pub(super) fn essentials_reset(&mut self) {
        self.essentials.bow.clear();
        self.essentials.supported = false;
        self.essentials.saved_main = None;
    }
    pub(super) fn essentials_payload(&mut self, data: &[u8]) {
        if let Some(enabled) = network::settings(data) {
            self.essentials.supported = enabled;
        }
    }
    pub(super) fn essentials_bow_frame(&mut self, playing: bool) -> bool {
        let hands = headset::main_hand().zip(headset::off_hand());
        let bow_main = self
            .stack_in(self.selected)
            .is_some_and(|s| &*s.item == "minecraft:bow");
        let bow_off = self
            .offhand_item()
            .is_some_and(|s| &*s.item == "minecraft:bow");
        if !self.essentials_bow_wanted()
            || !playing
            || (!bow_main && !bow_off)
            || hands.is_none()
            || self.sleeping.is_some()
            || self
                .player
                .as_ref()
                .is_none_or(|p| p.health.dead_or_dying())
        {
            if self.essentials.bow.clear().is_some() {
                self.essentials_bow_action(Action::Cancel);
            }
            return false;
        }
        let (main, off) = hands.expect("tracked hands");
        let holder = usize::from(!bow_main);
        let arrow = if bow_main {
            self.offhand_item()
        } else {
            self.stack_in(self.selected)
        };
        let arrow = arrow.is_some_and(|s| self.items().item_in_tag("minecraft:arrows", &s.item));
        let output = self.essentials.bow.frame(Input {
            now: self.pad.headset.vr.clock_ms(),
            holder,
            hands: [main.aim.0, off.aim.0],
            grip: if bow_main { main.grip.1 } else { off.grip.1 },
            height: self.player.as_ref().map_or(1.8, |p| p.dimensions.height),
            scale: headset::world_scale(),
            arrow_in_other_hand: arrow,
            pressed: ["questBindAttack", "questBindOffAttack"].iter().any(|key| {
                horizon_client_input::vr_bindings::held(
                    &self.settings.vr,
                    key,
                    headset::hand_buttons(),
                    false,
                )
            }),
        });
        for (i, micros) in output.haptics.into_iter().enumerate() {
            if micros > 0 {
                headset::pulse(
                    i == 0,
                    (micros as f32 / 3000.0).clamp(0.0, 1.0),
                    micros as f32 / 1_000_000.0,
                );
            }
        }
        if let Some(action) = output.action {
            self.essentials_bow_action(action);
        }
        self.essentials.bow.notched || self.essentials.bow.drawing
    }
    fn essentials_bow_action(&mut self, action: Action) {
        match action {
            Action::Start => {
                self.essentials.saved_main = Some(self.visor.active_main);
                self.visor.active_main = self.essentials.bow.holder == 0;
                self.visor_send(&horizon_visor::payload::C2s::ActiveHand {
                    main: self.visor.active_main,
                });
                let hand = if self.visor.active_main {
                    Hand::Main
                } else {
                    Hand::Off
                };
                self.right = true;
                self.remote_use_item(hand);
            }
            Action::Shoot(tension) => {
                // Tension precedes vanilla RELEASE_USE_ITEM on the same ordered connection.
                self.essentials_send(Payload::Tension(tension));
                if let Some(aim) = self.essentials_bow_angles() {
                    self.send(S::MovePlayerRot(play::MovePlayerRot {
                        yaw: headset::body_yaw() + aim[0],
                        pitch: aim[1],
                        flags: 0,
                    }));
                }
                self.release_remote_use();
                self.right = false;
                self.essentials_restore_hand();
            }
            Action::Cancel => {
                self.essentials_send(Payload::Cancel);
                self.using = None;
                self.right = false;
                self.essentials_restore_hand();
            }
        }
    }
    fn essentials_restore_hand(&mut self) {
        if let Some(main) = self.essentials.saved_main.take() {
            self.visor.active_main = main;
            self.visor_send(&horizon_visor::payload::C2s::ActiveHand { main });
        }
    }
    pub(super) fn essentials_bow_angles(&self) -> Option<[f32; 2]> {
        if !self.essentials_bow_wanted()
            || (!self.essentials.bow.drawing && self.essentials.saved_main.is_none())
        {
            return None;
        }
        let hand = if self.essentials.bow.holder == 0 {
            headset::main_hand()?
        } else {
            headset::off_hand()?
        };
        Some(headset::direction_angles(hand.aim.1 * Vec3::NEG_Z))
    }
    fn essentials_server_config(&mut self) {
        if self.essentials.server_loaded {
            return;
        }
        self.essentials.server_loaded = true;
        self.essentials.server_bow = true;
        let path = self.world_root.join("visor_essentials-server.yml");
        match std::fs::read(&path) {
            Ok(data) => {
                let mut framed = vec![0];
                framed.extend(data);
                self.essentials.server_bow = network::settings(&framed).unwrap_or(true);
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                if let Err(e) = std::fs::write(
                    &path,
                    "# VisorEssentials: allow bow power from physical draw distance\nbetterBow: true\n",
                ) {
                    log::warn!("Essentials settings: {e}");
                }
            }
            Err(e) => log::warn!("Essentials settings: {e}"),
        }
    }
    pub(super) fn essentials_server_hello(&mut self, host: &mut LanHost, conn: u32) {
        self.essentials_server_config();
        let Some(g) = host.hub.guest(conn) else {
            return;
        };
        if (self.essentials.registered.contains(&g.uuid) || g.channels.contains(network::CHANNEL))
            && self.visor_server.server.sessions.contains_key(&g.uuid)
        {
            host.hub.send_to(
                conn,
                &C::CustomPayload(CustomPayload {
                    channel: network::CHANNEL.into(),
                    data: network::settings_payload(self.essentials.server_bow),
                }),
            );
        }
    }
    pub(super) fn essentials_register(&mut self, host: &mut LanHost, conn: u32, data: &[u8]) {
        if data
            .split(|b| *b == 0)
            .any(|c| c == network::CHANNEL.as_bytes())
        {
            if let Some(g) = host.hub.guest(conn) {
                self.essentials.registered.insert(g.uuid);
            }
            self.essentials_server_hello(host, conn);
        }
    }
    pub(super) fn guest_essentials(&mut self, host: &mut LanHost, conn: u32, data: &[u8]) {
        self.essentials_server_config();
        let Some(uuid) = host.hub.guest(conn).map(|g| g.uuid) else {
            return;
        };
        if !self.essentials.server_bow || !self.visor_server.server.is_vr(uuid) {
            return;
        }
        match Payload::read(data) {
            Some(Payload::Tension(t)) => {
                self.as_guest(host, conn, |g| {
                    if g.using.is_some_and(|(slot, _)| {
                        g.stack_in(slot)
                            .is_some_and(|s| &*s.item == "minecraft:bow")
                    }) {
                        g.essentials.tension.insert(uuid, t);
                    }
                });
            }
            Some(Payload::Cancel) => {
                self.essentials.tension.remove(&uuid);
                self.as_guest(host, conn, |g| {
                    if g.using.is_some_and(|(slot, _)| {
                        g.stack_in(slot)
                            .is_some_and(|s| &*s.item == "minecraft:bow")
                    }) {
                        g.using = None;
                    }
                });
            }
            None => {}
        }
    }
}

impl Game<'_> {
    /// BowAimCrosshairRenderer: a 64-block collider/entity ray along the holding hand,
    /// 1 cm off the surface, with draw-ready/miss and local-light brightness.
    pub(super) fn essentials_bow_crosshair(&mut self, camera: [f64; 3]) -> bool {
        if !self.essentials_bow_wanted()
            || (!self.essentials.bow.notched && !self.essentials.bow.drawing)
        {
            return false;
        }
        let hand = if self.essentials.bow.holder == 0 {
            headset::main_hand()
        } else {
            headset::off_hand()
        };
        let Some(hand) = hand else {
            return true;
        };
        let body = headset::body_rotation(headset::body_yaw());
        let origin = glam::DVec3::from_array(camera) + (body * hand.aim.0).as_dvec3();
        let dir = (body * hand.aim.1 * Vec3::NEG_Z)
            .normalize_or_zero()
            .as_dvec3();
        let start = origin + dir;
        let mut end = origin + dir * 64.0;
        let mut hit = false;
        if let Some(p) = self.essentials_collider_hit(start, end) {
            end = p;
            hit = true;
        }
        // Use the game's complete pickable-entity set, bounded by the collider hit,
        // while leaving the gameplay ray and interaction reach unchanged afterward.
        let saved = (
            self.vr_aim,
            self.forced_hit,
            self.entity_reach,
            self.block_reach,
        );
        self.vr_aim = Some((start.to_array(), dir.to_array()));
        self.forced_hit = Some(super::Hit {
            pos: end.floor().as_ivec3().to_array(),
            face: [0; 3],
            point: end.to_array(),
        });
        self.entity_reach = start.distance(end);
        self.block_reach = self.entity_reach;
        let entity = self.targeted_entity_distance();
        (
            self.vr_aim,
            self.forced_hit,
            self.entity_reach,
            self.block_reach,
        ) = saved;
        if let Some(distance) = entity {
            end = start + dir * distance;
            hit = true;
        }
        let light = self.world.light(
            end.x.floor() as i32,
            end.y.floor() as i32,
            end.z.floor() as i32,
        );
        let sky = i32::from(light >> 4) - self.sky_darken();
        let raw = sky.max(i32::from(light & 15)).clamp(0, 15);
        let mut brightness = (raw as f32 / 15.0).max(0.15);
        if !hit {
            brightness *= 0.5;
        }
        if !self.essentials.bow.drawing {
            brightness *= 0.4;
        }
        let scale =
            0.06 * headset::world_scale().sqrt() * (origin.distance(end) as f32 / 4.5).max(1.0);
        let forward = dir.as_vec3();
        let right = forward.cross(Vec3::Y).try_normalize().unwrap_or(Vec3::X);
        let up = right.cross(forward);
        let at = (end - dir * 0.01 - glam::DVec3::from_array(camera)).as_vec3();
        let Some(sprite) = self.gui.atlas.sprites.get("hud/crosshair").copied() else {
            return true;
        };
        let c = (brightness * 255.0) as u32;
        let color = 0xFF00_0000 | c << 16 | c << 8 | c;
        let corner = |x: f32, y: f32, u: f32, v: f32| horizon_client::mesher::Vertex {
            pos: (at + (right * x + up * y) * scale).to_array(),
            uv: [u, v],
            color,
            layer: 0,
            light: 0xF0F0,
            normal: [0; 4],
            glint_uv: [0.0; 2],
        };
        let corners = [
            corner(-1.0, 1.0, sprite.u0, sprite.v0),
            corner(1.0, 1.0, sprite.u1, sprite.v0),
            corner(1.0, -1.0, sprite.u1, sprite.v1),
            corner(-1.0, -1.0, sprite.u0, sprite.v1),
        ];
        self.hand_crosshair.extend(corners);
        self.hand_crosshair.extend(corners.into_iter().rev());
        true
    }
}
