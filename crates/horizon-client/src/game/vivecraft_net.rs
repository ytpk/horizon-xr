// SPDX-License-Identifier: LGPL-3.0-or-later
//! The game's side of Vivecraft's `vivecraft:data` channel: the version handshake after
//! login, the server's settings, our pose every tick, and other players' poses.
//!
//! Ports the glue of `org.vivecraft.mixin.client_vr.multiplayer.ClientPacketListenerVRMixin`
//! (reset and VERSION on login and respawn) and `ClientNetworking.sendVRPlayerPositions`
//! (Vivecraft, https://github.com/Vivecraft/VivecraftMod, commit
//! 8b4d6b95a1891d9839f19b45c13e93415c02a31e). Copyright (c) Vivecraft contributors;
//! LGPL-3.0-or-later, see crates/horizon-vivecraft/LICENSE-LGPL-3.0. The protocol itself
//! is in the `horizon-vivecraft` crate; this file only moves bytes and reads the headset.
use super::*;
use glam::{Quat, Vec3};
use horizon_client::headset;
use horizon_client::text::Text;
use horizon_protocol::packets::{configuration::CustomPayload, play::Serverbound as S};
use horizon_vivecraft::{
    client::{ClientNet, Event},
    network::{self, C2s, FbtMode, NetworkVersion, Pose, S2c, VrBodyPart, VrPlayerState},
    overrides::ServerOverrides,
};

/// What the game keeps of the channel (`ClientNetworking`'s statics).
#[derive(Default)]
pub(super) struct VivecraftNet {
    pub(super) net: ClientNet,
    /// The calibrated height (metres); Vivecraft's default until calibrated.
    pub(super) user_height: Option<f32>,
    /// What the server forces over the player's own settings (`VRSettings.overrides`).
    pub(super) overrides: ServerOverrides,
    /// The settings the headset side sees: the player's, or with the overrides laid over.
    effective: Option<(
        std::sync::Arc<horizon_vivecraft::settings::VrSettings>,
        u32,
        std::sync::Arc<horizon_vivecraft::settings::VrSettings>,
    )>,
    overrides_version: u32,
    /// The last hit's direction, for the haptic tracker (`DamageDirection`).
    pub(super) last_hit: Option<Vec3>,
    /// Climbey: the server allows it and which blocks (`Event::Climbey`).
    pub(super) climbey: bool,
    /// The player turned teleporting off (`setTeleportOverride`).
    pub(super) teleport_override: bool,
}

impl Game<'_> {
    pub(super) fn vivecraft_send(&self, payload: &C2s) {
        let net = &self.vivecraft.net;
        if !net.may_send() {
            return;
        }
        self.send(S::CustomPayload(CustomPayload {
            channel: network::CHANNEL.into(),
            data: payload.write(net.used_network_version),
        }));
    }

    /// `LocalPlayerVRMixin.vivecraft$directTeleport`: the new position to a server that takes
    /// teleports.
    pub(super) fn vivecraft_send_teleport(&self, to: glam::DVec3) {
        self.vivecraft_send(&C2s::Teleport {
            x: to.x as f32,
            y: to.y as f32,
            z: to.z as f32,
        });
    }

    /// `ClientPacketListenerVRMixin.vivecraft$resetServerState`: login and respawn.
    /// Only a headset speaks the protocol (a flat screen is "NONVR" in Vivecraft, which
    /// Horizon does not announce).
    /// `send` says whether the VERSION goes out (the protocol is Vivecraft); otherwise only the
    /// state is forgotten.
    pub(super) fn vivecraft_reset(&mut self, send: bool) {
        self.vivecraft.net.reset_server_settings();
        self.vivecraft.overrides.reset_all();
        self.vivecraft.overrides_version += 1;
        if !send || !headset::active() {
            return;
        }
        let payload = self.vivecraft.net.version_payload(true);
        // VERSION is the one payload sent before the server answered.
        self.send(S::CustomPayload(CustomPayload {
            channel: network::CHANNEL.into(),
            data: payload.write(NetworkVersion::Legacy),
        }));
    }

    /// A payload from the server (`ClientNetworking.handlePacket`).
    pub(super) fn vivecraft_payload(&mut self, data: &[u8]) {
        // A delayed Visor answer may have superseded Auto's Vivecraft probe.
        if self.visor_active() {
            return;
        }
        let payload = match S2c::read(data) {
            Ok(p) => p,
            Err(e) => {
                log::debug!("Vivecraft: a bad payload from the server: {e}");
                return;
            }
        };
        for event in self.vivecraft.net.handle(payload) {
            match event {
                Event::ServerPlugin { version } => {
                    self.system_message(Text::tr("vivecraft.messages.serverplugin", &[&version]));
                    if headset::active() {
                        self.system_message(Text::tr("vivecraft.messages.calibrateheight", &[]));
                    }
                }
                Event::NoVrHotswitching => {
                    self.system_message(Text::tr("vivecraft.messages.novrhotswitching", &[]));
                }
                Event::Haptic {
                    body_part,
                    duration,
                    amplitude,
                    ..
                } => {
                    // The main hand's pulse; Vivecraft's frequency has no equivalent.
                    let main = body_part != VrBodyPart::OffHand;
                    headset::pulse(main, amplitude, duration);
                }
                Event::Climbey { allowed, .. } => self.vivecraft.climbey = allowed,
                Event::TeleportDisallowed => {
                    self.vivecraft.teleport_override = false;
                    self.system_message(Text::tr("vivecraft.messages.teleportdisabled", &[]));
                }
                Event::SettingOverride { overrides, clear } => {
                    self.vivecraft.overrides.apply(&overrides, clear);
                    self.vivecraft.overrides_version += 1;
                }
                Event::ServerVrChanges => {
                    // `showServerVrChangesMessage`: always, server only, or never.
                    if self.settings.vr.enum_name("showServerVrChangesMessage") != "NEVER" {
                        self.system_message(Text::tr(
                            "vivecraft.messages.nondefaultvrchanges",
                            &[&""],
                        ));
                    }
                }
                Event::DamageDirection(direction) => self.vivecraft.last_hit = Some(direction),
                Event::DisableVr(_) => {}
            }
        }
    }

    /// The settings the headset side sees: the player's, with the server's overrides over
    /// them (`VRSettings.overrides`).
    pub(super) fn vivecraft_effective_settings(
        &mut self,
    ) -> std::sync::Arc<horizon_vivecraft::settings::VrSettings> {
        let own = std::sync::Arc::clone(&self.settings.vr);
        let v = &mut self.vivecraft;
        if v.overrides == ServerOverrides::default() {
            return own;
        }
        if let Some((src, version, out)) = &v.effective
            && std::sync::Arc::ptr_eq(src, &own)
            && *version == v.overrides_version
        {
            return std::sync::Arc::clone(out);
        }
        let out = std::sync::Arc::new(v.overrides.effective(&own));
        v.effective = Some((own, v.overrides_version, std::sync::Arc::clone(&out)));
        out
    }

    /// `ClientVRPlayers.tick`: each VR player's last two poses go to its entity, whose model
    /// then follows them (`VRPlayerModel`); a player that left VR gets its vanilla model back.
    pub(super) fn vivecraft_bodies(&mut self) {
        use horizon_vivecraft::player_model::{BodyOptions, RotInfo, VrBody};
        let any_body = self.mobs.iter().any(|m| m.extra.vr_body.is_some());
        if self.vivecraft.net.players.is_empty() && self.visor.net.players.is_empty() && !any_body {
            return;
        }
        let vr = &self.settings.vr;
        let options = BodyOptions {
            limbs_connected: vr.bool("playerLimbsConnected"),
            limbs_limit: vr.bool("playerLimbsLimit"),
            arm_anim: vr.bool("playerArmAnim"),
            apply_world_scale: vr.bool("applyPlayerWorldscale"),
        };
        let uuids: Vec<(usize, u128)> = self
            .mobs
            .iter()
            .enumerate()
            .filter_map(|(i, m)| Some((i, self.remote_player_uuid(m.id)?)))
            .collect();
        for (i, uuid) in uuids {
            // A Vivecraft player or a Visor one (a server has one protocol per player).
            let rot = self
                .vivecraft
                .net
                .players
                .get(&uuid)
                .map(RotInfo::of)
                .or_else(|| {
                    self.visor
                        .net
                        .players
                        .get(&uuid)
                        .map(super::visor_net::visor_rot_info)
                });
            let body = &mut self.mobs[i].extra.vr_body;
            match rot {
                Some(rot) => match body {
                    Some(b) => b.push(rot, options),
                    None => *body = Some(VrBody::new(rot, options)),
                },
                None => *body = None,
            }
        }
    }

    /// The headset's orientation in the world: the room's rotation for the body's facing times
    /// the head's pose in the play space. Without the room's snapshot (the simulated headset),
    /// the head's yaw and pitch alone.
    pub(super) fn headset_head_rotation(&self) -> Quat {
        let body = headset::body_yaw();
        if let Some(room) = headset::room() {
            return (headset::body_rotation(body) * room.head.1).normalize();
        }
        let [yaw, pitch] = headset::head().unwrap_or([0.0, 0.0]);
        let (yaw, pitch) = ((body + yaw).to_radians(), pitch.to_radians());
        let look = Vec3::new(
            -yaw.sin() * pitch.cos(),
            -pitch.sin(),
            yaw.cos() * pitch.cos(),
        );
        Quat::from_rotation_arc(Vec3::NEG_Z, look)
    }

    /// The local head and hands as both VR protocols send them: positions relative to the
    /// player's feet, orientations in the world. `None` while the player or the headset is
    /// missing.
    pub(super) fn vr_local_pose(&self) -> Option<LocalPose> {
        if !headset::active() {
            return None;
        }
        let (Some(eye), Some(feet)) = (
            self.eye_position(),
            self.player.as_ref().map(|p| p.position),
        ) else {
            return None;
        };
        let body = headset::body_yaw();
        let to_world = |v: Vec3| headset::to_world(body, v);
        let rel = |world: [f64; 3]| {
            Vec3::new(
                (world[0] - feet.x) as f32,
                (world[1] - feet.y) as f32,
                (world[2] - feet.z) as f32,
            )
        };
        let head_offset = to_world(headset::head_offset().unwrap_or(Vec3::ZERO));
        let hmd_pos = rel([
            eye[0] + f64::from(head_offset.x),
            eye[1] + f64::from(head_offset.y),
            eye[2] + f64::from(head_offset.z),
        ]);
        // The head's own turn in the world, roll included. The player's `yaw` and `pitch` follow
        // the aim hand (what use and attack read), so they say nothing of where the head faces:
        // the server and the other players draw the head from this.
        let hmd = Pose::new(hmd_pos, self.headset_head_rotation());
        let settings = headset::settings();
        let hand = |off: bool| -> Pose {
            match if off {
                headset::off_hand()
            } else {
                headset::main_hand()
            } {
                Some(h) => {
                    let at = to_world(h.aim.0);
                    Pose::new(
                        rel([
                            eye[0] + f64::from(at.x),
                            eye[1] + f64::from(at.y),
                            eye[2] + f64::from(at.z),
                        ]),
                        headset::body_rotation(body) * h.aim.1,
                    )
                }
                // Untracked: both hands at the same place, as Vivecraft's body yaw
                // treats non tracking controllers.
                None => hmd,
            }
        };
        Some(LocalPose {
            hmd,
            main: hand(false),
            off: hand(true),
            left_handed: settings.left_handed(),
            seated: settings.seated(),
        })
    }

    /// `sendVRPlayerPositions`: the local pose to the server, once per tick.
    pub(super) fn vivecraft_tick(&mut self) {
        if !headset::active() || !self.vivecraft.net.server_wants_data {
            return;
        }
        let Some(pose) = self.vr_local_pose() else {
            return;
        };
        let state = VrPlayerState {
            seated: pose.seated,
            hmd: pose.hmd,
            left_handed: pose.left_handed,
            main_hand: pose.main,
            reverse_hands_legacy: pose.left_handed,
            off_hand: pose.off,
            fbt_mode: FbtMode::ArmsOnly,
            ..VrPlayerState::default()
        };
        let height = *self.vivecraft.user_height.get_or_insert(1.52);
        let sent = self
            .vivecraft
            .net
            .player_updates(&state, headset::world_scale(), height);
        for p in &sent {
            self.vivecraft_send(p);
        }
    }
}

/// The local head and hands (see `Game::vr_local_pose`).
pub(super) struct LocalPose {
    pub(super) hmd: Pose,
    pub(super) main: Pose,
    pub(super) off: Pose,
    pub(super) left_handed: bool,
    pub(super) seated: bool,
}
