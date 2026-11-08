// SPDX-License-Identifier: LGPL-3.0-or-later
//! The client half of the networking: ports `org.vivecraft.client.network.ClientNetworking`
//! and the data side of `org.vivecraft.client.ClientVRPlayers`.
//! Copyright (c) Vivecraft contributors; LGPL-3.0-or-later (see LICENSE-LGPL-3.0).
//!
//! No I/O here: the game feeds [`ClientNet::handle`] the server's payloads, sends the
//! payloads the `send_*`/`player_updates` functions return, and reacts to [`Event`]s.

use crate::network::{
    C2s, ClimbeyBlockmode, FbtMode, MAX_SUPPORTED_NETWORK_PROTOCOL, MIN_SUPPORTED_NETWORK_PROTOCOL,
    NetworkVersion, S2c, VrBodyPart, VrPlayerState,
};
use glam::Vec3;
use std::collections::HashMap;

/// `AutoCalibration.DEFAULT_HEIGHT`.
pub const DEFAULT_HEIGHT: f32 = 1.52;

/// What the game does for a payload from the server.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// The server has the Vivecraft plugin (`VERSION`): show `vivecraft.messages.serverplugin`
    /// (with the version) and, in VR with no calibration, `vivecraft.messages.calibrateheight`.
    ServerPlugin { version: String },
    /// Climbey: allowed and the blocklist (block ids as the server sent them).
    Climbey {
        allowed: bool,
        blockmode: ClimbeyBlockmode,
        blocks: Vec<String>,
    },
    /// Teleport is not allowed here: turn the teleport override off and show the message.
    TeleportDisallowed,
    /// Server setting overrides (`SETTING_OVERRIDE`); keys are `name` or `name.min`/`name.max`.
    SettingOverride {
        overrides: Vec<(String, String)>,
        clear: bool,
    },
    /// `vivecraft.messages.novrhotswitching`.
    NoVrHotswitching,
    /// A haptic pulse for a body part.
    Haptic {
        body_part: VrBodyPart,
        duration: f32,
        frequency: f32,
        amplitude: f32,
        delay: f32,
    },
    /// Where the last hit came from (the haptic tracker's `setLastHitDirection`).
    DamageDirection(Vec3),
    /// The server told which VR changes are non default (`SERVER_VR_CHANGES`).
    ServerVrChanges,
    /// A player turned VR off: drop their VR state.
    DisableVr(u128),
}

/// A VR player's last received data (`ClientVRPlayers.RotInfo`'s source).
#[derive(Clone, Debug, PartialEq)]
pub struct RemoteVrPlayer {
    pub state: VrPlayerState,
    pub world_scale: f32,
    /// Clamped to 0.5..1.5, 1 when seated.
    pub height_scale: f32,
}

/// `ClientNetworking`'s static state.
#[derive(Clone, Debug)]
pub struct ClientNet {
    pub server_has_vivecraft: bool,
    pub server_wants_data: bool,
    pub server_supports_direct_teleport: bool,
    pub server_allows_direct_teleport: bool,
    pub server_allows_climbey: bool,
    pub server_allows_crawling: bool,
    pub server_allows_vr_switching: bool,
    pub server_allows_dual_wielding: bool,
    pub server_allows_attacking_while_blocking: bool,
    pub server_vr_changes: Option<HashMap<String, String>>,
    pub used_network_version: NetworkVersion,
    pub climb_blockmode: ClimbeyBlockmode,
    pub climb_blocklist: Vec<String>,
    /// Aim overrides for vanilla servers and the server's.
    pub aim_dir_override: Option<Vec3>,
    pub aim_pos_override: Option<Vec3>,
    pub body_part_client_override: Option<VrBodyPart>,
    pub is_last_body_part_aim: bool,
    pub head_aim_warning: bool,
    pub vr_switching_warning: bool,
    pub teleport_warning: bool,
    worldscale_last: f32,
    height_last: f32,
    last_sent_body_part: VrBodyPart,
    /// Other VR players, by UUID.
    pub players: HashMap<u128, RemoteVrPlayer>,
}

impl Default for ClientNet {
    fn default() -> Self {
        Self {
            server_has_vivecraft: false,
            server_wants_data: false,
            server_supports_direct_teleport: false,
            server_allows_direct_teleport: true,
            server_allows_climbey: false,
            server_allows_crawling: false,
            server_allows_vr_switching: false,
            server_allows_dual_wielding: false,
            server_allows_attacking_while_blocking: false,
            server_vr_changes: None,
            // Assume a legacy server by default, to not send invalid packets.
            used_network_version: NetworkVersion::Legacy,
            climb_blockmode: ClimbeyBlockmode::Disabled,
            climb_blocklist: Vec::new(),
            aim_dir_override: None,
            aim_pos_override: None,
            body_part_client_override: None,
            is_last_body_part_aim: false,
            head_aim_warning: false,
            vr_switching_warning: false,
            teleport_warning: false,
            worldscale_last: 0.0,
            height_last: 0.0,
            last_sent_body_part: VrBodyPart::MainHand,
            players: HashMap::new(),
        }
    }
}

impl ClientNet {
    /// `resetServerSettings`: on each login, respawn and dimension change.
    pub fn reset_server_settings(&mut self) {
        let players = std::mem::take(&mut self.players);
        drop(players);
        *self = Self::default();
        self.teleport_warning = true;
    }

    /// `sendVersionInfo`: the first payload of a Vivecraft client; `vr` is whether VR runs.
    pub fn version_payload(&self, vr: bool) -> C2s {
        C2s::Version {
            version: crate::version_identifier(),
            vr,
            max_version: MAX_SUPPORTED_NETWORK_PROTOCOL,
            min_version: MIN_SUPPORTED_NETWORK_PROTOCOL,
            legacy: false,
        }
    }

    /// `supportsReversedBow`.
    pub fn supports_reversed_bow(&self) -> bool {
        NetworkVersion::DualWielding.accepts(self.used_network_version)
            || !self.server_has_vivecraft
    }

    /// `handlePacket`. Returns what the game must do.
    pub fn handle(&mut self, payload: S2c) -> Vec<Event> {
        let mut events = Vec::new();
        match payload {
            S2c::Unknown => {}
            S2c::Version(version) => {
                self.server_has_vivecraft = true;
                self.teleport_warning = false;
                self.vr_switching_warning = true;
                self.head_aim_warning = true;
                events.push(Event::ServerPlugin { version });
            }
            S2c::IsVrActive { vr, player } => {
                if !vr {
                    self.players.remove(&player);
                    events.push(Event::DisableVr(player));
                }
            }
            S2c::RequestData => self.server_wants_data = true,
            S2c::Climbing {
                allowed,
                blockmode,
                blocks,
            } => {
                self.server_allows_climbey = allowed;
                self.climb_blockmode = blockmode;
                self.climb_blocklist = blocks.clone().unwrap_or_default();
                events.push(Event::Climbey {
                    allowed,
                    blockmode,
                    blocks: blocks.unwrap_or_default(),
                });
            }
            S2c::Teleport { allowed, .. } => {
                self.server_supports_direct_teleport = true;
                self.server_allows_direct_teleport = allowed;
                if !allowed {
                    events.push(Event::TeleportDisallowed);
                }
            }
            S2c::UberPacket {
                player,
                state,
                world_scale,
                height_scale,
            } => self.update_player(player, state, world_scale, height_scale),
            S2c::SettingOverride { overrides, clear } => {
                events.push(Event::SettingOverride {
                    overrides: overrides.into_iter().collect(),
                    clear,
                });
            }
            S2c::Crawl { allowed, .. } => self.server_allows_crawling = allowed,
            S2c::NetworkVersion(v) => {
                self.used_network_version = v;
                if NetworkVersion::HeadAim.accepts(v) {
                    self.head_aim_warning = false;
                }
            }
            S2c::VrSwitching(allowed) => {
                self.server_allows_vr_switching = allowed;
                if !allowed {
                    events.push(Event::NoVrHotswitching);
                }
                self.vr_switching_warning = false;
            }
            S2c::DualWielding(b) => self.server_allows_dual_wielding = b,
            S2c::ServerVrChanges(changes) => {
                self.server_vr_changes = Some(changes.into_iter().collect());
                events.push(Event::ServerVrChanges);
            }
            S2c::Haptic {
                body_part,
                duration,
                frequency,
                amplitude,
                delay,
            } => events.push(Event::Haptic {
                body_part,
                duration,
                frequency,
                amplitude,
                delay,
            }),
            S2c::DamageDirection(v) => events.push(Event::DamageDirection(v)),
            S2c::AttackWhileBlocking(b) => self.server_allows_attacking_while_blocking = b,
        }
        events
    }

    /// `ClientVRPlayers.update` for a received state.
    pub fn update_player(
        &mut self,
        player: u128,
        state: VrPlayerState,
        world_scale: f32,
        height_scale: f32,
    ) {
        let mut height_scale = height_scale.clamp(0.5, 1.5);
        if state.seated {
            height_scale = 1.0;
        }
        self.players.insert(
            player,
            RemoteVrPlayer {
                state,
                world_scale,
                height_scale,
            },
        );
    }

    /// `sendVRPlayerPositions`: the payloads to send this tick. `state` is the local
    /// player's state (positions relative to the player's feet), `user_height` the
    /// calibrated height in metres. Nothing is sent until the server asked for data.
    pub fn player_updates(
        &mut self,
        state: &VrPlayerState,
        world_scale: f32,
        user_height: f32,
    ) -> Vec<C2s> {
        let mut out = Vec::new();
        if !self.server_wants_data {
            return out;
        }
        if world_scale != self.worldscale_last {
            out.push(C2s::WorldScale(world_scale));
            self.worldscale_last = world_scale;
        }
        if user_height != self.height_last {
            out.push(C2s::Height(user_height / DEFAULT_HEIGHT));
            self.height_last = user_height;
        }
        if self.used_network_version != NetworkVersion::Legacy {
            // Don't send FBT data to servers that cannot hold it.
            let mut state = state.clone();
            if !NetworkVersion::Fbt.accepts(self.used_network_version) {
                state = state.stripped_for(self.used_network_version);
            }
            out.push(C2s::VrPlayerState(state));
        } else {
            out.push(C2s::Controller0Data {
                left_handed: state.left_handed,
                main_hand: state.main_hand,
            });
            out.push(C2s::Controller1Data {
                left_handed: state.left_handed,
                off_hand: state.off_hand,
            });
            out.push(C2s::HeadData {
                seated: state.seated,
                hmd: state.hmd,
            });
        }
        out
    }

    /// Payloads are only sent to servers that said they have Vivecraft.
    pub fn may_send(&self) -> bool {
        self.server_has_vivecraft
    }

    /// `sendActiveHand`: `aim_device_hmd` is `vrSettings.aimDevice == HMD`.
    pub fn active_hand(
        &mut self,
        off_hand: bool,
        use_for_aim: bool,
        aim_device_hmd: bool,
    ) -> Option<C2s> {
        if !use_for_aim && aim_device_hmd {
            self.active_body_part(VrBodyPart::Head, true)
        } else {
            let part = if off_hand {
                VrBodyPart::OffHand
            } else {
                VrBodyPart::MainHand
            };
            self.active_body_part(part, use_for_aim)
        }
    }

    /// `sendActiveBodyPart`.
    pub fn active_body_part(&mut self, mut part: VrBodyPart, use_for_aim: bool) -> Option<C2s> {
        let mut out = None;
        if self.server_wants_data {
            let v = self.used_network_version;
            if (!NetworkVersion::HeadAim.accepts(v) && part == VrBodyPart::Head)
                || (!NetworkVersion::DualWielding.accepts(v)
                    && !part.available_in_mode(FbtMode::ArmsOnly))
            {
                // Old plugins only support main and off hand.
                part = VrBodyPart::MainHand;
            }
            // Only when the part differs from last time.
            if part != self.last_sent_body_part {
                out = Some(C2s::ActiveHand {
                    body_part: part,
                    use_for_aim,
                });
            }
        }
        self.last_sent_body_part = part;
        self.is_last_body_part_aim = use_for_aim;
        out
    }

    /// `getActiveBodyPart`.
    pub fn get_active_body_part(&self) -> VrBodyPart {
        self.body_part_client_override
            .unwrap_or(self.last_sent_body_part)
    }

    /// `resetAim`: with a server that has no data request, the look is restored client side.
    pub fn reset_aim(&mut self, ticks: i8) -> Option<C2s> {
        if !self.server_wants_data {
            self.aim_dir_override = None;
            self.aim_pos_override = None;
            None
        } else if NetworkVersion::AimOverride.accepts(self.used_network_version) {
            if ticks == 0 {
                self.aim_dir_override = None;
                self.aim_pos_override = None;
            }
            Some(C2s::AimOverrideReset(ticks))
        } else {
            None
        }
    }

    /// `overrideAimDir` for a server that wants data.
    pub fn override_aim_dir(&mut self, dir: Vec3) -> Option<C2s> {
        if self.server_wants_data && NetworkVersion::AimOverride.accepts(self.used_network_version)
        {
            self.aim_dir_override = Some(dir);
            Some(C2s::AimDirectionOverride(dir))
        } else {
            None
        }
    }

    /// `overrideAimPos`: `pos` relative to the player's position.
    pub fn override_aim_pos(&mut self, relative: Vec3, absolute: Vec3) -> Option<C2s> {
        if self.server_wants_data && NetworkVersion::AimOverride.accepts(self.used_network_version)
        {
            self.aim_pos_override = Some(absolute);
            Some(C2s::AimPositionOverride(relative))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::Pose;

    fn state() -> VrPlayerState {
        VrPlayerState {
            hmd: Pose::new(Vec3::new(0.0, 1.7, 0.0), glam::Quat::IDENTITY),
            ..VrPlayerState::default()
        }
    }

    #[test]
    fn nothing_is_sent_before_the_server_asks() {
        let mut net = ClientNet::default();
        assert!(net.player_updates(&state(), 1.0, 1.7).is_empty());
        net.handle(S2c::RequestData);
        net.handle(S2c::NetworkVersion(NetworkVersion::ClimbeyJump));
        let sent = net.player_updates(&state(), 1.0, 1.7);
        assert!(matches!(sent[0], C2s::WorldScale(_)));
        assert!(matches!(sent[1], C2s::Height(h) if (h - 1.7 / 1.52).abs() < 1e-6));
        assert!(matches!(sent[2], C2s::VrPlayerState(_)));
        // Scale and height only go out when they change.
        let sent = net.player_updates(&state(), 1.0, 1.7);
        assert_eq!(sent.len(), 1);
    }

    #[test]
    fn legacy_servers_get_three_packets() {
        let mut net = ClientNet::default();
        net.handle(S2c::RequestData);
        let sent = net.player_updates(&state(), 1.0, 1.52);
        let ids: Vec<_> = sent.iter().map(|p| p.id().unwrap() as u8).collect();
        assert_eq!(ids, vec![5, 12, 3, 4, 2]);
    }

    #[test]
    fn the_handshake_state_follows_the_server() {
        let mut net = ClientNet::default();
        assert!(!net.may_send());
        let ev = net.handle(S2c::Version("x".into()));
        assert!(net.may_send());
        assert_eq!(
            ev,
            vec![Event::ServerPlugin {
                version: "x".into()
            }]
        );
        net.handle(S2c::NetworkVersion(NetworkVersion::DualWielding));
        assert!(net.head_aim_warning);
        net.handle(S2c::NetworkVersion(NetworkVersion::HeadAim));
        assert!(!net.head_aim_warning);
        let ev = net.handle(S2c::Teleport {
            allowed: false,
            target: NetworkVersion::OptionToggle,
        });
        assert_eq!(ev, vec![Event::TeleportDisallowed]);
        net.reset_server_settings();
        assert!(!net.server_has_vivecraft);
        assert_eq!(net.used_network_version, NetworkVersion::Legacy);
    }

    #[test]
    fn the_active_hand_goes_out_once_and_falls_back_on_old_servers() {
        let mut net = ClientNet::default();
        net.handle(S2c::RequestData);
        net.handle(S2c::NetworkVersion(NetworkVersion::ClimbeyJump));
        let p = net.active_hand(true, false, false);
        assert_eq!(
            p,
            Some(C2s::ActiveHand {
                body_part: VrBodyPart::OffHand,
                use_for_aim: false
            })
        );
        assert_eq!(net.active_hand(true, false, false), None);
        // The HMD aim device sends the head.
        assert_eq!(
            net.active_hand(false, false, true),
            Some(C2s::ActiveHand {
                body_part: VrBodyPart::Head,
                use_for_aim: true
            })
        );
        let mut old = ClientNet::default();
        old.handle(S2c::RequestData);
        old.handle(S2c::NetworkVersion(NetworkVersion::NewNetworking));
        assert_eq!(
            old.active_body_part(VrBodyPart::Head, true),
            None,
            "falls back to the main hand"
        );
    }

    #[test]
    fn other_players_height_is_clamped() {
        let mut net = ClientNet::default();
        net.handle(S2c::UberPacket {
            player: 1,
            state: state(),
            world_scale: 2.0,
            height_scale: 3.0,
        });
        assert_eq!(net.players[&1].height_scale, 1.5);
        let mut seated = state();
        seated.seated = true;
        net.handle(S2c::UberPacket {
            player: 2,
            state: seated,
            world_scale: 1.0,
            height_scale: 0.7,
        });
        assert_eq!(net.players[&2].height_scale, 1.0);
        net.handle(S2c::IsVrActive {
            vr: false,
            player: 1,
        });
        assert!(!net.players.contains_key(&1));
    }

    #[test]
    fn the_version_payload_names_vivecraft_and_the_protocol_range() {
        let net = ClientNet::default();
        let bytes = net.version_payload(true).write(NetworkVersion::Legacy);
        assert_eq!(
            &bytes[1..],
            b"Vivecraft-26.2-fabric-1.3.15 VR\n9\n0".as_slice()
        );
    }
}
