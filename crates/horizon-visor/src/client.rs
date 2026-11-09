// SPDX-License-Identifier: MPL-2.0
//! The client half of `visor:channel` (`ClientNetworking`, `ClientPacketHandler`,
//! `VRClientPlayers`). Copyright (C) Phoenix-Ra and Visor contributors; MPL-2.0 (see
//! LICENSE-MPL-2.0).
//!
//! The state sends only what changed since the last tick (`*LastSent` in Java) and a pose
//! every tick, and only after the server answered the HANDSHAKE.

use crate::body::{BodyType, DEFAULT_FULL_HEIGHT, DEFAULT_GUN_ANGLE};
use crate::buffer::PoseData;
use crate::payload::{C2s, S2c, StartTracking};
use crate::settings::ServerSettings;
use crate::{NETWORK_VERSION, VISOR_VERSION};
use std::collections::HashMap;

/// The local player's state as the server should see it (`VRLocalPlayerImpl`'s getters).
#[derive(Clone, Debug, PartialEq)]
pub struct LocalState {
    /// `getModelHeight` rounded to a centimetre (`body::round_height`).
    pub height: f32,
    pub world_scale: f32,
    pub rotation_y: f32,
    pub left_handed: bool,
    pub active_hand_main: bool,
    pub offhand_slot: i32,
    pub gun_angle: f32,
    pub body_type: BodyType,
    pub overlay_focused: bool,
    pub pose: PoseData,
}

/// What the server last heard (`ClientNetworking.*LastSent`).
#[derive(Clone, Debug, PartialEq)]
struct LastSent {
    height: f32,
    world_scale: f32,
    rotation_y: f32,
    offhand_slot: i32,
    gun_angle: f32,
    left_handed: bool,
    active_hand_main: bool,
    body_type: Option<BodyType>,
    overlay_focused: bool,
}

impl Default for LastSent {
    fn default() -> Self {
        Self {
            height: DEFAULT_FULL_HEIGHT,
            world_scale: 1.0,
            rotation_y: 0.0,
            offhand_slot: -1,
            gun_angle: DEFAULT_GUN_ANGLE,
            left_handed: false,
            active_hand_main: true,
            body_type: None,
            overlay_focused: false,
        }
    }
}

/// Another VR player as the server told us (`VRRemotePlayerImpl`).
#[derive(Clone, Debug, PartialEq)]
pub struct RemotePlayer {
    /// The pose before the last one and the last, for interpolation between ticks.
    pub prev_pose: PoseData,
    pub pose: PoseData,
    pub body_type: BodyType,
    pub left_handed: bool,
    pub rotation_y: f32,
    pub world_scale: f32,
    /// `getFullHeight`: the height the model is rendered at.
    pub full_height: f32,
    pub gun_angle: f32,
    pub overlay_focused: bool,
    /// Client ticks since the last pose, for diagnostics; silence does not end tracking.
    pub idle_ticks: u32,
}

impl RemotePlayer {
    fn from_start(t: &StartTracking) -> Self {
        Self {
            prev_pose: t.pose,
            pose: t.pose,
            body_type: BodyType::from_id(&t.body_type),
            left_handed: t.left_handed,
            rotation_y: t.rotation_y,
            world_scale: t.world_scale,
            full_height: t.full_height,
            gun_angle: t.gun_angle,
            overlay_focused: t.overlay_focused,
            idle_ticks: 0,
        }
    }

    /// `getFullHeightScale`: how much taller than a Minecraft player the model is drawn.
    pub fn height_scale(&self) -> f32 {
        self.full_height / DEFAULT_FULL_HEIGHT
    }
}

/// What the game has to do with a payload.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// The server answered the HANDSHAKE: it speaks Visor.
    ServerSupportsVisor,
    /// `SERVER_SETTINGS` changed the settings.
    SettingsChanged,
    /// `ROTATION_Y`: the room rotation the server remembered for this player.
    RotationY(f32),
    /// `OFFHAND_SLOT`: the hotbar slot the server remembered for the off hand.
    OffhandSlot(i32),
    /// `BLOCK_DAMAGE`: another player's mining progress on a block (stage 0..9, other: cleared).
    BlockDamage {
        player: u128,
        pos: [i32; 3],
        stage: i32,
    },
    /// A VR player appeared or sent its whole state again.
    PlayerTracked(u128),
}

/// The state of the channel on the client (`ClientNetworking`'s statics).
#[derive(Debug)]
pub struct ClientNet {
    /// `serverSupportsVisor`: set by the server's HANDSHAKE answer.
    pub server_supports_visor: bool,
    /// `VRServerSettings`: the vanilla server profile until the server says otherwise.
    pub settings: ServerSettings,
    pub players: HashMap<u128, RemotePlayer>,
    last: LastSent,
}

impl Default for ClientNet {
    fn default() -> Self {
        Self {
            server_supports_visor: false,
            settings: ServerSettings::vanilla_server(),
            players: HashMap::new(),
            last: LastSent::default(),
        }
    }
}

impl ClientNet {
    /// `ClientPacketListenerMixin.visor$onLogin`: forget the last server, then the HANDSHAKE
    /// every server gets, whether it knows Visor or not.
    pub fn login(&mut self, vr_active: bool) -> C2s {
        self.reset();
        C2s::Handshake {
            vr_active,
            network_version: NETWORK_VERSION,
            visor_version: VISOR_VERSION.to_string(),
        }
    }

    /// `ClientNetworking.dispose` and `VRServerSettings.resetToDefaults` for a dedicated
    /// server (`joinedDedicatedServer`): the server has to prove it speaks Visor again.
    pub fn reset(&mut self) {
        self.server_supports_visor = false;
        self.settings = ServerSettings::vanilla_server();
        self.players.clear();
        self.last = LastSent::default();
    }

    /// `ClientNetworking.sendVRPlayerState`: the changes, then the pose. Empty until the
    /// server answered the HANDSHAKE.
    pub fn state_updates(&mut self, local: &LocalState) -> Vec<C2s> {
        if !self.server_supports_visor {
            return Vec::new();
        }
        let last = &mut self.last;
        let mut out = Vec::with_capacity(2);
        if local.height != last.height {
            out.push(C2s::FullHeight(local.height));
            last.height = local.height;
        }
        if local.world_scale != last.world_scale {
            out.push(C2s::WorldScale(local.world_scale));
            last.world_scale = local.world_scale;
        }
        if local.rotation_y != last.rotation_y {
            out.push(C2s::RotationY(local.rotation_y));
            last.rotation_y = local.rotation_y;
        }
        if local.left_handed != last.left_handed {
            out.push(C2s::LeftHanded(local.left_handed));
            last.left_handed = local.left_handed;
        }
        if local.active_hand_main != last.active_hand_main {
            out.push(C2s::ActiveHand {
                main: local.active_hand_main,
            });
            last.active_hand_main = local.active_hand_main;
        }
        if local.offhand_slot != last.offhand_slot {
            out.push(C2s::OffhandSlot(local.offhand_slot));
            last.offhand_slot = local.offhand_slot;
        }
        if local.gun_angle != last.gun_angle {
            out.push(C2s::GunAngle(local.gun_angle));
            last.gun_angle = local.gun_angle;
        }
        if last.body_type.as_ref() != Some(&local.body_type) {
            out.push(C2s::VrBodyType(local.body_type.id().to_string()));
            last.body_type = Some(local.body_type.clone());
        }
        if local.overlay_focused != last.overlay_focused {
            out.push(C2s::OverlayFocused(local.overlay_focused));
            last.overlay_focused = local.overlay_focused;
        }
        out.push(C2s::Pose(Box::new(local.pose)));
        out
    }

    /// `ClientPacketHandler.handlePacket`.
    pub fn handle(&mut self, payload: S2c) -> Option<Event> {
        match payload {
            S2c::Handshake => {
                // `receivedHandShake`: the server has to hear everything again.
                self.server_supports_visor = true;
                self.last = LastSent::default();
                Some(Event::ServerSupportsVisor)
            }
            S2c::ServerSettings(text) => {
                self.settings.apply_text(&text);
                Some(Event::SettingsChanged)
            }
            S2c::RotationY(v) => Some(Event::RotationY(v)),
            S2c::OffhandSlot(v) => Some(Event::OffhandSlot(v)),
            S2c::BlockDamage {
                player,
                pos,
                destroy_stage,
            } => Some(Event::BlockDamage {
                player,
                pos,
                stage: destroy_stage,
            }),
            S2c::OtherStartTracking(t) => {
                // A snapshot can come again for a player we know: it replaces everything.
                self.players.insert(t.player, RemotePlayer::from_start(&t));
                Some(Event::PlayerTracked(t.player))
            }
            S2c::OtherPose { player, pose } => {
                if let Some(p) = self.players.get_mut(&player) {
                    p.prev_pose = p.pose;
                    p.pose = *pose;
                    p.idle_ticks = 0;
                }
                None
            }
            S2c::OtherBodyType { player, body_type } => {
                self.with(player, |p| p.body_type = BodyType::from_id(&body_type))
            }
            S2c::OtherLeftHanded {
                player,
                left_handed,
            } => self.with(player, |p| p.left_handed = left_handed),
            S2c::OtherRotationY { player, rotation_y } => {
                self.with(player, |p| p.rotation_y = rotation_y)
            }
            S2c::OtherWorldScale {
                player,
                world_scale,
            } => self.with(player, |p| p.world_scale = world_scale),
            S2c::OtherFullHeight {
                player,
                full_height,
            } => self.with(player, |p| p.full_height = full_height),
            S2c::OtherGunAngle { player, gun_angle } => {
                self.with(player, |p| p.gun_angle = gun_angle)
            }
            S2c::OtherOverlayFocused {
                player,
                overlay_focused,
            } => self.with(player, |p| p.overlay_focused = overlay_focused),
            S2c::Unknown(_) => None,
        }
    }

    fn with(&mut self, player: u128, f: impl FnOnce(&mut RemotePlayer)) -> Option<Event> {
        if let Some(p) = self.players.get_mut(&player) {
            f(p);
        }
        None
    }

    /// `VRClientPlayers.removePlayer`: end tracking when the entity leaves the level.
    pub fn remove_player(&mut self, uuid: u128) {
        self.players.remove(&uuid);
    }

    /// Network silence may be a stalled server: keep the snapshot so later deltas work.
    pub fn tick(&mut self) {
        for p in self.players.values_mut() {
            p.idle_ticks = p.idle_ticks.saturating_add(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::SupportedMovement;

    fn local() -> LocalState {
        LocalState {
            height: DEFAULT_FULL_HEIGHT,
            world_scale: 1.0,
            rotation_y: 0.0,
            left_handed: false,
            active_hand_main: true,
            offhand_slot: -1,
            gun_angle: DEFAULT_GUN_ANGLE,
            body_type: BodyType::HandsOnly,
            overlay_focused: false,
            pose: PoseData::default(),
        }
    }

    #[test]
    fn nothing_is_sent_before_the_server_answers() {
        let mut net = ClientNet::default();
        assert!(net.state_updates(&local()).is_empty());
        let hs = net.login(true);
        assert_eq!(
            hs,
            C2s::Handshake {
                vr_active: true,
                network_version: 5,
                visor_version: "0.6.1".into()
            }
        );
        assert!(net.state_updates(&local()).is_empty());
    }

    // After the HANDSHAKE the first tick tells the server the body type (it was never sent)
    // and the pose; the defaults stay quiet. Later ticks send the pose only, until a value moves.
    #[test]
    fn a_tick_sends_the_pose_and_what_changed() {
        let mut net = ClientNet::default();
        net.login(true);
        assert_eq!(net.handle(S2c::Handshake), Some(Event::ServerSupportsVisor));
        let first = net.state_updates(&local());
        assert_eq!(first.len(), 2);
        assert_eq!(first[0], C2s::VrBodyType("hands_only".into()));
        assert!(matches!(first[1], C2s::Pose(_)));

        assert_eq!(net.state_updates(&local()).len(), 1);

        let mut moved = local();
        moved.world_scale = 2.0;
        moved.left_handed = true;
        moved.offhand_slot = 4;
        moved.height = 1.8;
        let out = net.state_updates(&moved);
        // The order of ClientNetworking.sendVRPlayerState.
        assert_eq!(
            &out[..4],
            &[
                C2s::FullHeight(1.8),
                C2s::WorldScale(2.0),
                C2s::LeftHanded(true),
                C2s::OffhandSlot(4)
            ]
        );
        assert_eq!(out.len(), 5);
        assert_eq!(net.state_updates(&moved).len(), 1);
    }

    #[test]
    fn a_second_handshake_makes_the_client_say_everything_again() {
        let mut net = ClientNet::default();
        net.handle(S2c::Handshake);
        let mut s = local();
        s.world_scale = 2.0;
        net.state_updates(&s);
        assert_eq!(net.state_updates(&s).len(), 1);
        net.handle(S2c::Handshake);
        assert_eq!(net.state_updates(&s).len(), 3);
    }

    #[test]
    fn settings_text_changes_the_vanilla_profile() {
        let mut net = ClientNet::default();
        assert_eq!(
            net.settings.supported_movement,
            SupportedMovement::Controller
        );
        assert_eq!(
            net.handle(S2c::ServerSettings(
                "supportedMovement: BOTH\nbetterSwinging: true\n".into()
            )),
            Some(Event::SettingsChanged)
        );
        assert_eq!(net.settings.supported_movement, SupportedMovement::Both);
        assert!(net.settings.better_swinging);
        // A new login goes back to the vanilla profile.
        net.login(true);
        assert!(!net.settings.better_swinging);
    }

    #[test]
    fn other_players_are_tracked_updated_and_forgotten() {
        let mut net = ClientNet::default();
        let start = StartTracking {
            player: 7,
            pose: PoseData::default(),
            body_type: "full_body".into(),
            left_handed: false,
            rotation_y: 0.0,
            world_scale: 1.0,
            full_height: 1.52,
            gun_angle: 60.0,
            overlay_focused: false,
        };
        assert_eq!(
            net.handle(S2c::OtherStartTracking(Box::new(start))),
            Some(Event::PlayerTracked(7))
        );
        net.handle(S2c::OtherFullHeight {
            player: 7,
            full_height: 3.04,
        });
        net.handle(S2c::OtherLeftHanded {
            player: 7,
            left_handed: true,
        });
        let mut moved = PoseData::default();
        moved.hmd.position.y = 1.6;
        net.handle(S2c::OtherPose {
            player: 7,
            pose: Box::new(moved),
        });
        // Packets about a player we were never told of are dropped.
        net.handle(S2c::OtherLeftHanded {
            player: 8,
            left_handed: true,
        });
        let p = &net.players[&7];
        assert_eq!(p.body_type, BodyType::FullBody);
        assert_eq!(p.height_scale(), 2.0);
        assert!(p.left_handed);
        assert_eq!(p.pose.hmd.position.y, 1.6);
        assert_eq!(p.prev_pose.hmd.position.y, 0.0);
        assert!(!net.players.contains_key(&8));
        for _ in 0..200 {
            net.tick();
        }
        moved.hmd.position.y = 1.7;
        net.handle(S2c::OtherPose {
            player: 7,
            pose: Box::new(moved),
        });
        assert_eq!(net.players[&7].pose.hmd.position.y, 1.7);
        assert_eq!(net.players[&7].full_height, 3.04);
        assert!(net.players[&7].left_handed);
        net.remove_player(7);
        assert!(net.players.is_empty());
    }
}
