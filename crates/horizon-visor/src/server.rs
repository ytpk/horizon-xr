// SPDX-License-Identifier: MPL-2.0
//! The server half of `visor:channel`: `ServerPacketHandler`, `ServerNetworking` and the player
//! state of `VRServerPlayerImpl` (the mod), with the same observable behaviour as VisorPlugin's
//! `InboundHandler` and `TrackerBroadcaster`. Copyright (C) Phoenix-Ra and Visor
//! contributors; MPL-2.0 (see LICENSE-MPL-2.0).
//!
//! The state machine has no I/O: [`VisorServer::handle`] returns the replies and the effects
//! on the player, [`VisorServer::state_updates`] the broadcast of one VR player's state.

use crate::body::{DEFAULT_FULL_HEIGHT, DEFAULT_GUN_ANGLE};
use crate::buffer::PoseData;
use crate::payload::{C2s, S2c, StartTracking};
use crate::settings::{ServerSettings, SupportedMovement};
use crate::{NETWORK_VERSION, VISOR_VERSION};
use std::collections::{HashMap, HashSet};

/// What a player has said about itself (`VRServerPlayerImpl`).
#[derive(Clone, Debug)]
pub struct VrState {
    /// The last `POSE_DATA`; nothing is broadcast before the first.
    pub pose: Option<PoseData>,
    /// The id string, `"null"` until the client says (`VRServerPlayerImpl.vrBodyType`).
    pub body_type: String,
    pub world_scale: f32,
    pub full_height: f32,
    pub rotation_y: f32,
    pub active_hand_main: bool,
    pub left_handed: bool,
    pub crawling: bool,
    pub offhand_slot: i32,
    pub gun_angle: f32,
    pub overlay_focused: bool,
    // `*LastSent`: what the trackers were told.
    body_type_sent: Option<String>,
    left_handed_sent: bool,
    rotation_y_sent: f32,
    world_scale_sent: f32,
    full_height_sent: f32,
    gun_angle_sent: f32,
    overlay_focused_sent: bool,
    known_trackers: HashSet<u128>,
}

impl Default for VrState {
    fn default() -> Self {
        Self {
            pose: None,
            body_type: "null".into(),
            world_scale: 1.0,
            full_height: DEFAULT_FULL_HEIGHT,
            rotation_y: 0.0,
            active_hand_main: true,
            left_handed: false,
            crawling: false,
            offhand_slot: 0,
            gun_angle: DEFAULT_GUN_ANGLE,
            overlay_focused: false,
            body_type_sent: None,
            left_handed_sent: false,
            rotation_y_sent: 0.0,
            world_scale_sent: 1.0,
            full_height_sent: DEFAULT_FULL_HEIGHT,
            gun_angle_sent: DEFAULT_GUN_ANGLE,
            overlay_focused_sent: false,
            known_trackers: HashSet::new(),
        }
    }
}

/// A player that said HANDSHAKE (`VisorServerPlayerImpl` or `VRServerPlayerImpl`).
#[derive(Clone, Debug)]
pub struct Session {
    pub visor_version: String,
    /// `Some` for a VR player.
    pub vr: Option<VrState>,
}

/// What the mod keeps in the player's data across joins (`visor$getRotationYCached`,
/// `visor$getOffhandSlotCached`; VisorPlugin: `vr_prefs.properties`). Kept in memory here.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Prefs {
    pub rotation_y: f32,
    pub offhand_slot: i32,
}

/// The file of the prefs (VisorPlugin's `PrefsFile`, `vr_prefs.properties`): a comment header,
/// then `uuid=rotationY,offhandSlot` for each player, with Java's `Float.toString`.
pub const PREFS_FILE: &str = "vr_prefs.properties";

/// `PrefsFile.save`: players in UUID order for a file that does not shuffle between saves.
pub fn prefs_text(prefs: &HashMap<u128, Prefs>) -> String {
    let mut out =
        String::from("# Visor VR player prefs: rotationY,offhandSlot\n# Don't touch this!\n");
    let mut all: Vec<_> = prefs.iter().collect();
    all.sort_by_key(|(uuid, _)| **uuid);
    for (uuid, p) in all {
        let r = p.rotation_y;
        let rotation = if r.fract() == 0.0 && r.abs() < 1e7 {
            format!("{r:.1}")
        } else {
            r.to_string()
        };
        out.push_str(&format!(
            "{}={},{}\n",
            uuid_text(*uuid),
            rotation,
            p.offhand_slot
        ));
    }
    out
}

/// `PrefsFile.load`: a line that does not parse is skipped.
pub fn load_prefs_text(text: &str) -> HashMap<u128, Prefs> {
    let mut out = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let Some((rotation, slot)) = value.split_once(',') else {
            continue;
        };
        let (Some(uuid), Ok(rotation_y), Ok(offhand_slot)) = (
            parse_uuid(key.trim()),
            rotation.trim().parse::<f32>(),
            slot.trim().parse::<i32>(),
        ) else {
            continue;
        };
        out.insert(
            uuid,
            Prefs {
                rotation_y,
                offhand_slot,
            },
        );
    }
    out
}

fn uuid_text(uuid: u128) -> String {
    let h = format!("{uuid:032x}");
    format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    )
}

fn parse_uuid(text: &str) -> Option<u128> {
    let hex: String = text.chars().filter(|c| *c != '-').collect();
    (hex.len() == 32).then(|| u128::from_str_radix(&hex, 16).ok())?
}

/// Why a player is sent away.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Disconnect {
    /// `visor.messages.network_mismatch` with the client's and the server's version.
    NetworkMismatch { client: i32, server: i32 },
}

/// An effect on the player a payload asks for, done by the host with the player swapped in.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Disconnect(Disconnect),
    /// `absSnapTo(x, y, z)` keeping the rotation (`TELEPORT`).
    Teleport {
        x: f32,
        y: f32,
        z: f32,
    },
    /// `resetFallDistance` (`CLIMBING`).
    ResetFall,
    /// `setForcedCrawl` / `Pose.SWIMMING` (`CRAWLING`); false lets the pose go back.
    Crawl(bool),
    /// `SWING_ATTACK`: the host attacks the entity from the hand if it is in reach.
    SwingAttack {
        entity_id: i32,
        shift: bool,
        main_hand: bool,
    },
    /// `SWING_BLOCK`: a hit on a block (`visor$handleVrBlockDamage`) and its ack.
    SwingBlock {
        pos: [i32; 3],
        direction: u8,
        main_hand: bool,
        sequence: i32,
    },
}

/// The result of one payload.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Out {
    pub replies: Vec<S2c>,
    pub actions: Vec<Action>,
}

/// The server's Visor state: the settings and the players.
#[derive(Debug, Default)]
pub struct VisorServer {
    pub settings: ServerSettings,
    pub sessions: HashMap<u128, Session>,
    pub prefs: HashMap<u128, Prefs>,
}

impl VisorServer {
    pub fn new(settings: ServerSettings) -> Self {
        Self {
            settings,
            ..Self::default()
        }
    }

    /// The player is a VR player of Visor (`getVRPlayer(player) != null`).
    pub fn is_vr(&self, uuid: u128) -> bool {
        self.sessions.get(&uuid).is_some_and(|s| s.vr.is_some())
    }

    pub fn vr_state(&self, uuid: u128) -> Option<&VrState> {
        self.sessions.get(&uuid)?.vr.as_ref()
    }

    /// Where the player's off hand is (`VROffhandEquipment.redirecting`): the hotbar slot it chose
    /// for a VR player on a `twoHandedVR` server (-1: none yet), otherwise `None`, the vanilla slot.
    pub fn offhand_redirect(&self, uuid: u128) -> Option<i32> {
        let vr = self.vr_state(uuid);
        crate::tasks::hotbar::offhand_redirect(
            self.settings.two_handed_vr,
            vr.is_some(),
            vr.map_or(-1, |v| v.offhand_slot),
        )
    }

    /// The hand that mines and attacks (`getActiveHand`): the off hand only in two-handed play.
    pub fn active_hand_is_main(&self, uuid: u128) -> bool {
        match self.vr_state(uuid) {
            Some(vr) if self.settings.two_handed_vr => vr.active_hand_main,
            _ => true,
        }
    }

    /// `VisorServerImpl.removePlayer`.
    pub fn remove(&mut self, uuid: u128) {
        self.sessions.remove(&uuid);
    }

    /// `ServerPacketHandler.handlePacket`.
    pub fn handle(&mut self, uuid: u128, payload: C2s) -> Out {
        let mut out = Out::default();
        if self.sessions.get(&uuid).is_some_and(|s| s.vr.is_none())
            && let C2s::Handshake {
                vr_active: true,
                network_version,
                visor_version,
            } = &payload
        {
            self.handshake(
                uuid,
                true,
                *network_version,
                visor_version.clone(),
                &mut out,
            );
            return out;
        }
        let settings = &self.settings;
        let Some(session) = self.sessions.get_mut(&uuid) else {
            // A player that has not said HANDSHAKE can say nothing else.
            if let C2s::Handshake {
                vr_active,
                network_version,
                visor_version,
            } = payload
            {
                self.handshake(uuid, vr_active, network_version, visor_version, &mut out);
            }
            return out;
        };
        // A non-VR session and a second HANDSHAKE do nothing.
        let Some(vr) = session.vr.as_mut() else {
            return out;
        };
        match payload {
            C2s::Pose(pose) => vr.pose = Some(*pose),
            C2s::LeftHanded(v) => vr.left_handed = v,
            C2s::ActiveHand { main } => vr.active_hand_main = main,
            C2s::OffhandSlot(slot) => {
                vr.offhand_slot = slot;
                self.prefs.entry(uuid).or_default().offhand_slot = slot;
            }
            C2s::VrBodyType(s) => vr.body_type = s,
            C2s::WorldScale(v) => vr.world_scale = v,
            C2s::FullHeight(v) => vr.full_height = v,
            C2s::RotationY(v) => {
                vr.rotation_y = v;
                self.prefs.entry(uuid).or_default().rotation_y = v;
            }
            C2s::GunAngle(v) => vr.gun_angle = v,
            C2s::OverlayFocused(v) => vr.overlay_focused = v,
            C2s::Crawling(v) if settings.room_crawling_supported => {
                vr.crawling = v;
                out.actions.push(Action::Crawl(v));
            }
            C2s::Climbing if settings.room_climbing_supported => {
                out.actions.push(Action::ResetFall)
            }
            C2s::Teleport { x, y, z }
                if settings.supported_movement != SupportedMovement::Controller =>
            {
                out.actions.push(Action::Teleport { x, y, z });
            }
            C2s::SwingAttack {
                entity_id,
                shift,
                main_hand,
            } if settings.better_swinging => out.actions.push(Action::SwingAttack {
                entity_id,
                shift,
                main_hand,
            }),
            C2s::SwingBlock {
                pos,
                direction,
                main_hand,
                sequence,
            } if settings.better_swinging => out.actions.push(Action::SwingBlock {
                pos,
                direction,
                main_hand,
                sequence,
            }),
            _ => {}
        }
        out
    }

    /// `ServerPacketHandler.handleHandshake`.
    fn handshake(
        &mut self,
        uuid: u128,
        vr_active: bool,
        network_version: i32,
        visor_version: String,
        out: &mut Out,
    ) {
        if network_version != NETWORK_VERSION {
            out.actions
                .push(Action::Disconnect(Disconnect::NetworkMismatch {
                    client: network_version,
                    server: NETWORK_VERSION,
                }));
            return;
        }
        self.sessions.insert(
            uuid,
            Session {
                visor_version,
                vr: vr_active.then(|| {
                    let prefs = self.prefs.get(&uuid).copied().unwrap_or_default();
                    VrState {
                        rotation_y: prefs.rotation_y,
                        rotation_y_sent: 0.0,
                        offhand_slot: prefs.offhand_slot,
                        ..VrState::default()
                    }
                }),
            },
        );
        out.replies.push(S2c::Handshake);
        out.replies
            .push(S2c::ServerSettings(self.settings.to_client_text()));
        if vr_active {
            let prefs = self.prefs.get(&uuid).copied().unwrap_or_default();
            out.replies.push(S2c::RotationY(prefs.rotation_y));
            if self.settings.two_handed_vr {
                out.replies.push(S2c::OffhandSlot(prefs.offhand_slot));
            }
        }
    }

    /// `VisorServer.broadcastSettings` (VisorPlugin's `reload`): the settings to everybody.
    pub fn settings_payload(&self) -> S2c {
        S2c::ServerSettings(self.settings.to_client_text())
    }

    /// `ServerNetworking.sendVRStatePacketOf`: what the players that see `uuid` hear about it
    /// this tick. `tracking` lists the players that currently track the VR player (any
    /// player with a Visor session counts, VR or not; the player itself is left out). The
    /// result is (receiver, payload) in the order of the Java.
    pub fn state_updates(&mut self, uuid: u128, tracking: &[u128]) -> Vec<(u128, S2c)> {
        let settings_trackers = self.settings.body_trackers_send_to_others;
        let settings_hands = self.settings.hand_trackers_send_to_others;
        let receivers: Vec<u128> = tracking
            .iter()
            .copied()
            .filter(|t| *t != uuid && self.sessions.contains_key(t))
            .collect();
        let Some(vr) = self.sessions.get_mut(&uuid).and_then(|s| s.vr.as_mut()) else {
            return Vec::new();
        };
        let Some(pose) = vr.pose else {
            return Vec::new();
        };
        let for_others = pose.for_others(settings_trackers, settings_hands);

        let current: HashSet<u128> = receivers.iter().copied().collect();
        let new: HashSet<u128> = current.difference(&vr.known_trackers).copied().collect();
        vr.known_trackers.retain(|k| current.contains(k));
        vr.known_trackers.extend(current.iter().copied());

        let mut out = Vec::new();
        if !new.is_empty() {
            let snapshot = S2c::OtherStartTracking(Box::new(StartTracking {
                player: uuid,
                pose: for_others,
                body_type: vr.body_type.clone(),
                left_handed: vr.left_handed,
                rotation_y: vr.rotation_y,
                world_scale: vr.world_scale,
                full_height: vr.full_height,
                gun_angle: vr.gun_angle,
                overlay_focused: vr.overlay_focused,
            }));
            for r in receivers.iter().filter(|r| new.contains(r)) {
                out.push((*r, snapshot.clone()));
            }
        }
        // Updated values go to the old trackers; the new ones got them in the snapshot.
        let to_old = |payload: S2c, out: &mut Vec<(u128, S2c)>| {
            for r in receivers.iter().filter(|r| !new.contains(r)) {
                out.push((*r, payload.clone()));
            }
        };
        if vr.body_type_sent.as_deref() != Some(vr.body_type.as_str()) {
            to_old(
                S2c::OtherBodyType {
                    player: uuid,
                    body_type: vr.body_type.clone(),
                },
                &mut out,
            );
            vr.body_type_sent = Some(vr.body_type.clone());
        }
        if vr.left_handed != vr.left_handed_sent {
            to_old(
                S2c::OtherLeftHanded {
                    player: uuid,
                    left_handed: vr.left_handed,
                },
                &mut out,
            );
            vr.left_handed_sent = vr.left_handed;
        }
        if vr.rotation_y != vr.rotation_y_sent {
            to_old(
                S2c::OtherRotationY {
                    player: uuid,
                    rotation_y: vr.rotation_y,
                },
                &mut out,
            );
            vr.rotation_y_sent = vr.rotation_y;
        }
        if vr.world_scale != vr.world_scale_sent {
            to_old(
                S2c::OtherWorldScale {
                    player: uuid,
                    world_scale: vr.world_scale,
                },
                &mut out,
            );
            vr.world_scale_sent = vr.world_scale;
        }
        if vr.full_height != vr.full_height_sent {
            to_old(
                S2c::OtherFullHeight {
                    player: uuid,
                    full_height: vr.full_height,
                },
                &mut out,
            );
            vr.full_height_sent = vr.full_height;
        }
        // The Java does not leave the new trackers out of this one ("gunAngle is NOT part of
        // the new-tracker burst", though the snapshot does carry it): they hear it twice.
        if vr.gun_angle != vr.gun_angle_sent {
            for r in &receivers {
                out.push((
                    *r,
                    S2c::OtherGunAngle {
                        player: uuid,
                        gun_angle: vr.gun_angle,
                    },
                ));
            }
            vr.gun_angle_sent = vr.gun_angle;
        }
        if vr.overlay_focused != vr.overlay_focused_sent {
            to_old(
                S2c::OtherOverlayFocused {
                    player: uuid,
                    overlay_focused: vr.overlay_focused,
                },
                &mut out,
            );
            vr.overlay_focused_sent = vr.overlay_focused;
        }
        to_old(
            S2c::OtherPose {
                player: uuid,
                pose: Box::new(for_others),
            },
            &mut out,
        );
        out
    }

    /// `state_updates`, the receivers grouped by the payload they hear: the same payload for
    /// many players is made (and encoded by the caller) once. The order of the payloads is the
    /// Java's, and so is each receiver's order of them.
    pub fn state_updates_grouped(
        &mut self,
        uuid: u128,
        tracking: &[u128],
    ) -> Vec<(Vec<u128>, S2c)> {
        let mut out: Vec<(Vec<u128>, S2c)> = Vec::new();
        for (to, payload) in self.state_updates(uuid, tracking) {
            // The same payload goes to neighbours one after the other in `state_updates`.
            match out.last_mut() {
                Some((receivers, last)) if *last == payload => receivers.push(to),
                _ => out.push((vec![to], payload)),
            }
        }
        out
    }

    /// The text of the log line `Visor: <name> joined (<version>), VR=<bool>` (`serverDebug`).
    pub fn describe(&self, uuid: u128) -> Option<String> {
        let s = self.sessions.get(&uuid)?;
        Some(format!(
            "{} (client Visor {}, server {VISOR_VERSION}), VR={}",
            if s.vr.is_some() { "VR" } else { "non-VR" },
            s.visor_version,
            s.vr.is_some()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::PoseElement;
    use glam::{Quat, Vec3};

    fn hello(server: &mut VisorServer, uuid: u128, vr: bool) -> Out {
        server.handle(
            uuid,
            C2s::Handshake {
                vr_active: vr,
                network_version: 5,
                visor_version: "0.6.1".into(),
            },
        )
    }

    #[test]
    fn the_prefs_file_is_the_plugins_and_reads_back() {
        let mut prefs = HashMap::new();
        prefs.insert(
            0x0123_4567_89ab_cdef_0123_4567_89ab_cdef,
            Prefs {
                rotation_y: 1.5,
                offhand_slot: 3,
            },
        );
        prefs.insert(
            0x00000000_0000_0000_0000_000000000001,
            Prefs {
                rotation_y: 0.0,
                offhand_slot: -1,
            },
        );
        let text = prefs_text(&prefs);
        assert_eq!(
            text,
            "# Visor VR player prefs: rotationY,offhandSlot\n# Don't touch this!\n\
             00000000-0000-0000-0000-000000000001=0.0,-1\n\
             01234567-89ab-cdef-0123-456789abcdef=1.5,3\n"
        );
        assert_eq!(load_prefs_text(&text), prefs);
        // Lines that do not parse are skipped, as `PrefsFile.load` skips them.
        assert!(load_prefs_text("x=1,2\n00000000-0000-0000-0000-000000000001=a,1\n").is_empty());
    }

    // ServerPacketHandler.handleHandshake: HANDSHAKE, SERVER_SETTINGS, ROTATION_Y, then
    // OFFHAND_SLOT when `two_handed`.
    #[test]
    fn a_vr_handshake_gets_four_answers_with_two_handed() {
        let mut s = VisorServer::new(ServerSettings::default());
        s.prefs.insert(
            1,
            Prefs {
                rotation_y: 1.5,
                offhand_slot: 3,
            },
        );
        let out = hello(&mut s, 1, true);
        assert_eq!(out.replies.len(), 4);
        assert_eq!(out.replies[0], S2c::Handshake);
        assert_eq!(out.replies[1], s.settings_payload());
        assert_eq!(out.replies[2], S2c::RotationY(1.5));
        assert_eq!(out.replies[3], S2c::OffhandSlot(3));
        assert!(s.is_vr(1));
    }

    // `VROffhandEquipment.redirecting` and `getActiveHand` on the server: only a VR player of a
    // `twoHandedVR` server has its off hand in a hotbar slot, and only then can the off hand be
    // the active one.
    #[test]
    fn the_server_redirects_the_off_hand_of_a_vr_player_of_a_two_handed_server() {
        let mut s = VisorServer::new(ServerSettings::default());
        hello(&mut s, 1, true);
        hello(&mut s, 2, false);
        assert_eq!(
            s.offhand_redirect(1),
            Some(0),
            "the default slot until chosen"
        );
        s.handle(1, C2s::OffhandSlot(6));
        assert_eq!(s.offhand_redirect(1), Some(6));
        s.handle(1, C2s::OffhandSlot(-1));
        assert_eq!(s.offhand_redirect(1), Some(-1));
        // A non-VR player, a stranger and a server with the mode off keep the vanilla slot.
        assert_eq!(s.offhand_redirect(2), None);
        assert_eq!(s.offhand_redirect(99), None);
        assert!(s.active_hand_is_main(1));
        s.handle(1, C2s::ActiveHand { main: false });
        assert!(!s.active_hand_is_main(1));
        s.settings.two_handed_vr = false;
        assert_eq!(s.offhand_redirect(1), None);
        assert!(
            s.active_hand_is_main(1),
            "the off hand is never active when two-handed is off"
        );
    }

    #[test]
    fn without_two_handed_the_offhand_slot_is_not_sent_and_non_vr_gets_two() {
        let mut s = VisorServer::new(ServerSettings::plugin_defaults());
        assert_eq!(hello(&mut s, 1, true).replies.len(), 3);
        let out = hello(&mut s, 2, false);
        assert_eq!(out.replies, vec![S2c::Handshake, s.settings_payload()]);
        assert!(!s.is_vr(2) && s.sessions.contains_key(&2));
    }

    #[test]
    fn a_headset_starting_after_login_can_upgrade_the_non_vr_session() {
        let mut server = VisorServer::default();
        hello(&mut server, 1, false);
        assert!(!server.is_vr(1));
        assert!(
            hello(&mut server, 1, true)
                .replies
                .contains(&S2c::Handshake)
        );
        assert!(server.is_vr(1));
        let pose = PoseData::default();
        server.handle(1, C2s::Pose(Box::new(pose)));
        assert_eq!(server.vr_state(1).unwrap().pose, Some(pose));
        assert_eq!(hello(&mut server, 1, true), Out::default());
        assert_eq!(server.vr_state(1).unwrap().pose, Some(pose));
    }

    #[test]
    fn a_flat_client_receives_vr_poses_and_recovers_after_network_silence() {
        use crate::client::ClientNet;
        let mut server = VisorServer::default();
        let mut client = ClientNet::default();
        let hello_flat = client.login(false);
        for packet in server.handle(2, hello_flat).replies {
            client.handle(packet);
        }
        hello(&mut server, 1, true);
        server.handle(1, C2s::FullHeight(3.04));
        server.handle(1, C2s::Pose(Box::default()));
        for (_, packet) in server.state_updates(1, &[2]) {
            client.handle(packet);
        }
        for _ in 0..200 {
            client.tick();
        }
        let mut moved = PoseData::default();
        moved.hmd.position.y = 2.9;
        server.handle(1, C2s::Pose(Box::new(moved)));
        let packets = server.state_updates(1, &[2]);
        assert!(
            packets
                .iter()
                .all(|(_, p)| !matches!(p, S2c::OtherStartTracking(_)))
        );
        for (_, packet) in packets {
            client.handle(packet);
        }
        assert_eq!(client.players[&1].pose.hmd.position.y, 2.9);
        assert_eq!(client.players[&1].height_scale(), 2.0);
        client.remove_player(1);
        assert!(client.players.is_empty());
    }

    #[test]
    fn another_network_version_is_disconnected_with_both_numbers() {
        let mut s = VisorServer::new(ServerSettings::default());
        let out = s.handle(
            1,
            C2s::Handshake {
                vr_active: true,
                network_version: 4,
                visor_version: "0.5.0".into(),
            },
        );
        assert!(out.replies.is_empty());
        assert_eq!(
            out.actions,
            vec![Action::Disconnect(Disconnect::NetworkMismatch {
                client: 4,
                server: 5
            })]
        );
        assert!(!s.sessions.contains_key(&1));
    }

    #[test]
    fn payloads_before_the_handshake_and_from_non_vr_players_do_nothing() {
        let mut s = VisorServer::new(ServerSettings::default());
        assert_eq!(
            s.handle(
                1,
                C2s::Teleport {
                    x: 1.0,
                    y: 2.0,
                    z: 3.0
                }
            ),
            Out::default()
        );
        hello(&mut s, 2, false);
        assert_eq!(
            s.handle(
                2,
                C2s::Teleport {
                    x: 1.0,
                    y: 2.0,
                    z: 3.0
                }
            ),
            Out::default()
        );
        hello(&mut s, 3, true);
        // A second HANDSHAKE changes nothing.
        assert_eq!(hello(&mut s, 3, true), Out::default());
    }

    // ServerPacketHandler: each of these is behind its server setting.
    #[test]
    fn game_payloads_follow_the_settings() {
        let mut cfg = ServerSettings::default();
        cfg.supported_movement = SupportedMovement::Controller;
        cfg.better_swinging = false;
        cfg.room_climbing_supported = false;
        let mut s = VisorServer::new(cfg);
        hello(&mut s, 1, true);
        for p in [
            C2s::Teleport {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            C2s::Climbing,
            C2s::SwingAttack {
                entity_id: 1,
                shift: false,
                main_hand: true,
            },
            C2s::SwingBlock {
                pos: [0; 3],
                direction: 0,
                main_hand: true,
                sequence: 0,
            },
        ] {
            assert_eq!(s.handle(1, p), Out::default());
        }
        // Crawling is still on by default.
        assert_eq!(
            s.handle(1, C2s::Crawling(true)).actions,
            vec![Action::Crawl(true)]
        );

        s.settings = ServerSettings::default();
        assert_eq!(
            s.handle(
                1,
                C2s::Teleport {
                    x: 1.0,
                    y: 2.0,
                    z: 3.0
                }
            )
            .actions,
            vec![Action::Teleport {
                x: 1.0,
                y: 2.0,
                z: 3.0
            }]
        );
        assert_eq!(s.handle(1, C2s::Climbing).actions, vec![Action::ResetFall]);
        assert_eq!(
            s.handle(
                1,
                C2s::SwingBlock {
                    pos: [1, 2, 3],
                    direction: 4,
                    main_hand: false,
                    sequence: 9
                }
            )
            .actions,
            vec![Action::SwingBlock {
                pos: [1, 2, 3],
                direction: 4,
                main_hand: false,
                sequence: 9
            }]
        );
    }

    #[test]
    fn rotation_and_offhand_slot_are_remembered_for_the_next_join() {
        let mut s = VisorServer::new(ServerSettings::default());
        hello(&mut s, 1, true);
        s.handle(1, C2s::RotationY(2.5));
        s.handle(1, C2s::OffhandSlot(6));
        s.remove(1);
        let out = hello(&mut s, 1, true);
        assert_eq!(out.replies[2], S2c::RotationY(2.5));
        assert_eq!(out.replies[3], S2c::OffhandSlot(6));
    }

    fn pose_at(y: f32) -> C2s {
        let mut p = PoseData::default();
        p.hmd = PoseElement::new(Vec3::new(0.0, y, 0.0), Quat::IDENTITY);
        C2s::Pose(Box::new(p))
    }

    #[test]
    fn nothing_is_broadcast_before_the_first_pose() {
        let mut s = VisorServer::new(ServerSettings::default());
        hello(&mut s, 1, true);
        hello(&mut s, 2, true);
        assert!(s.state_updates(1, &[2]).is_empty());
    }

    // A new tracker gets the snapshot only; an old one gets the pose, and a changed value once.
    #[test]
    fn trackers_get_a_snapshot_then_changes_and_poses() {
        let mut s = VisorServer::new(ServerSettings::default());
        hello(&mut s, 1, true);
        hello(&mut s, 2, true);
        hello(&mut s, 3, false);
        s.handle(1, pose_at(1.6));

        let first = s.state_updates(1, &[2, 3, 1]);
        assert_eq!(first.len(), 2);
        for (to, p) in &first {
            assert!([2, 3].contains(to));
            let S2c::OtherStartTracking(t) = p else {
                panic!("not a snapshot: {p:?}")
            };
            assert_eq!(t.player, 1);
            assert_eq!(t.pose.hmd.position.y, 1.6);
            assert_eq!(t.body_type, "null");
        }

        // The next tick: the pose alone, to both.
        let second = s.state_updates(1, &[2, 3]);
        assert_eq!(second.len(), 2);
        assert!(
            second
                .iter()
                .all(|(_, p)| matches!(p, S2c::OtherPose { .. }))
        );

        // A body type and a new tracker (4): 4 gets the snapshot, 2 and 3 the change and the pose.
        s.handle(1, C2s::VrBodyType("full_body".into()));
        hello(&mut s, 4, false);
        let third = s.state_updates(1, &[2, 3, 4]);
        let to4: Vec<_> = third.iter().filter(|(to, _)| *to == 4).collect();
        assert_eq!(to4.len(), 1);
        assert!(matches!(to4[0].1, S2c::OtherStartTracking(_)));
        let to2: Vec<_> = third
            .iter()
            .filter(|(to, _)| *to == 2)
            .map(|(_, p)| p.clone())
            .collect();
        assert_eq!(
            to2,
            vec![
                S2c::OtherBodyType {
                    player: 1,
                    body_type: "full_body".into()
                },
                S2c::OtherPose {
                    player: 1,
                    pose: Box::new(PoseData::default().with_hmd_y(1.6))
                },
            ]
        );
        // Once only.
        assert_eq!(s.state_updates(1, &[2, 3, 4]).len(), 3);
        // A tracker that went away and came back gets the snapshot again.
        s.state_updates(1, &[3, 4]);
        let back = s.state_updates(1, &[2, 3, 4]);
        assert!(
            back.iter()
                .any(|(to, p)| *to == 2 && matches!(p, S2c::OtherStartTracking(_)))
        );
    }

    #[test]
    fn grouped_updates_carry_each_payload_once_for_all_who_hear_it() {
        let mut s = VisorServer::new(ServerSettings::default());
        for id in 1..=4 {
            hello(&mut s, id, id == 1);
        }
        s.handle(1, pose_at(1.6));
        // Three new trackers: one snapshot for the three.
        let first = s.state_updates_grouped(1, &[2, 3, 4]);
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].0, vec![2, 3, 4]);
        // The next tick: one pose for the three.
        let second = s.state_updates_grouped(1, &[2, 3, 4]);
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].0, vec![2, 3, 4]);
        assert!(matches!(second[0].1, S2c::OtherPose { .. }));
        // A body type change: the change for the three, then the pose for the three.
        s.handle(1, C2s::VrBodyType("full_body".into()));
        let third = s.state_updates_grouped(1, &[2, 3, 4]);
        assert_eq!(third.len(), 2);
        assert!(third.iter().all(|(r, _)| r.len() == 3));
    }

    #[test]
    fn a_changed_gun_angle_reaches_new_trackers_twice() {
        let mut s = VisorServer::new(ServerSettings::default());
        hello(&mut s, 1, true);
        hello(&mut s, 2, true);
        s.handle(1, pose_at(1.6));
        s.handle(1, C2s::GunAngle(30.0));
        let out = s.state_updates(1, &[2]);
        assert!(matches!(out[0].1, S2c::OtherStartTracking(_)));
        assert_eq!(
            out[1].1,
            S2c::OtherGunAngle {
                player: 1,
                gun_angle: 30.0
            }
        );
    }

    #[test]
    fn trackers_and_joints_stay_home_unless_the_server_shares_them() {
        let mut cfg = ServerSettings::default();
        cfg.body_trackers_send_to_others = false;
        let mut s = VisorServer::new(cfg);
        hello(&mut s, 1, true);
        hello(&mut s, 2, false);
        let mut pose = PoseData::default();
        pose.trackers.0[0] = Some(PoseElement::IDENTITY);
        pose.hands.left.0[1] = Some(PoseElement::IDENTITY);
        s.handle(1, C2s::Pose(Box::new(pose)));
        let out = s.state_updates(1, &[2]);
        let S2c::OtherStartTracking(t) = &out[0].1 else {
            panic!()
        };
        assert!(t.pose.trackers.is_empty());
        assert!(!t.pose.hands.left.is_empty());
    }

    impl PoseData {
        fn with_hmd_y(mut self, y: f32) -> Self {
            self.hmd.position.y = y;
            self
        }
    }
}
