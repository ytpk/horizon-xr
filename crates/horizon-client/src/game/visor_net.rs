// SPDX-License-Identifier: MPL-2.0
//! The game's side of Visor's `visor:channel`: the HANDSHAKE after login, the server's
//! settings, our pose and state every tick, and other players' poses; and the choice between
//! Visor and Vivecraft for a connection.
//!
//! Ports the glue of `org.vmstudio.visor.mixin.client.multiplayer.ClientPacketListenerMixin`
//! (HANDSHAKE at the tail of `handleLogin`) and `ClientNetworking.sendVRPlayerState` (Visor
//! 0.6.1, https://github.com/VisorModStudio/Visor, commit
//! c01354fad8ac03a016c249239a04b07adeb36476). Copyright (C) Phoenix-Ra and Visor
//! contributors; MPL-2.0, see crates/horizon-visor/LICENSE-MPL-2.0. The protocol is in the
//! `horizon-visor` crate; this file moves bytes and reads the headset.
use super::*;
use horizon_client::headset;
use horizon_client::text::Text;
use horizon_protocol::packets::{configuration::CustomPayload, play::Serverbound as S};
use horizon_visor::{
    body::{self, BodyType},
    buffer::{PoseData, PoseElement},
    client::{ClientNet, Event, LocalState},
    payload::{C2s, S2c},
    settings::{ServerSettings, SupportedHeightMode},
};

/// How many client ticks `Auto` waits for Visor's HANDSHAKE answer before it speaks Vivecraft.
/// A Visor server answers in the same round trip as the login; this covers a slow link.
pub(super) const AUTO_WAIT_TICKS: u32 = 40;

/// The protocol the player asked for (`horizonVrProtocol`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Preference {
    Auto,
    Visor,
    Vivecraft,
}

impl Preference {
    pub(super) fn of(settings: &horizon_vivecraft::settings::VrSettings) -> Self {
        match settings.enum_name("horizonVrProtocol") {
            "VISOR" => Self::Visor,
            "VIVECRAFT" => Self::Vivecraft,
            _ => Self::Auto,
        }
    }
}

/// The protocol a connection speaks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Protocol {
    /// Not connected: no VR protocol.
    #[default]
    None,
    /// Visor's HANDSHAKE went out; Vivecraft's VERSION waits for the answer (or the timeout).
    Auto,
    Visor,
    Vivecraft,
    /// Auto tried Vivecraft; a delayed Visor HANDSHAKE may still select Visor.
    Fallback,
}

impl Protocol {
    fn accepts(self, payload: &S2c) -> bool {
        match self {
            Self::None | Self::Vivecraft => false,
            Self::Fallback => matches!(payload, S2c::Handshake),
            Self::Auto | Self::Visor => true,
        }
    }
}

/// What the game keeps of the channel (`ClientNetworking`'s statics).
#[derive(Default)]
pub(super) struct VisorNet {
    pub(super) net: ClientNet,
    pub(super) protocol: Protocol,
    /// Client ticks since the login while the protocol is `Auto`.
    waited: u32,
    /// The hand that attacks and mines (`VRLocalPlayerImpl.activeHand`): the main hand.
    pub(super) active_main: bool,
    /// `TaskHotBar`: the wheels' choices, and the slot the off hand uses (`slot_offhand`, -1
    /// until chosen).
    pub(super) hotbar: horizon_visor::tasks::hotbar::HotbarTask,
    /// The login's HANDSHAKE was made with a headset running. A session that starts after the
    /// login (a world opened from the command line) says it late (`visor_late_login`).
    pub(super) logged_in_with_headset: bool,
    /// `TaskSwing` and `TaskRoomCrawl`.
    pub(super) swing: horizon_visor::tasks::swing::SwingTask,
    /// `TaskRoomClimb`.
    pub(super) climb: horizon_visor::tasks::climb::ClimbTask,
    pub(super) crawl: horizon_visor::tasks::movement::CrawlTask,
    /// `TaskSwing.pendingBreaks`: blocks a swing hit that the server may break.
    pub(super) pending_breaks:
        horizon_visor::swing_effects::PendingBreaks<horizon_world::registry::BlockStateId>,
}

impl VisorNet {
    pub(super) fn new() -> Self {
        Self {
            active_main: true,
            ..Self::default()
        }
    }
}

impl Game<'_> {
    pub(super) fn visor_send(&self, payload: &C2s) {
        self.send(S::CustomPayload(CustomPayload {
            channel: horizon_visor::CHANNEL.into(),
            data: payload.write(),
        }));
    }

    /// The settings of the server as the mechanics read them: Visor's when it speaks Visor,
    /// otherwise the one profile for a vanilla server (or a Vivecraft one, whose own rules the
    /// Vivecraft code applies).
    /// The hotbar slot the off hand holds in two-handed play (`TaskHotBar.getSlotOffhand`), none
    /// while no slot is chosen.
    pub(super) fn visor_offhand_slot(&self) -> Option<usize> {
        usize::try_from(self.visor.hotbar.slot_offhand)
            .ok()
            .filter(|&s| s < 9)
    }

    pub(super) fn vr_server_settings(&self) -> &ServerSettings {
        &self.visor.net.settings
    }

    /// The headset's session began after the login (the world opened as the game started): the
    /// protocols are spoken now, as the login would have.
    pub(super) fn visor_late_login(&mut self) {
        if headset::active() && !self.visor.logged_in_with_headset && self.remote.is_some() {
            self.vr_login();
        }
    }

    /// Visor is the protocol and the server answered.
    pub(super) fn visor_active(&self) -> bool {
        self.visor.protocol == Protocol::Visor && self.visor.net.server_supports_visor
    }

    /// `ClientPacketListenerMixin.visor$onLogin` and Vivecraft's reset: both channels forget the
    /// last server; a headset says its HANDSHAKE (Visor) and, when the player or the server
    /// does not take Visor, its VERSION (Vivecraft).
    pub(super) fn vr_login(&mut self) {
        let preference = Preference::of(&headset::settings());
        self.visor.logged_in_with_headset = headset::active();
        self.visor.waited = 0;
        self.visor.active_main = true;
        self.visor.hotbar = Default::default();
        self.visor.net.reset();
        self.visor.pending_breaks.clear();
        self.visor.protocol = Protocol::None;
        headset::set_tracking_wanted(false, false);
        match preference {
            Preference::Vivecraft => {
                self.visor.protocol = Protocol::Vivecraft;
                self.vivecraft_reset(true);
            }
            Preference::Visor | Preference::Auto => {
                self.vivecraft_reset(false);
                self.visor.protocol = if preference == Preference::Auto {
                    Protocol::Auto
                } else {
                    Protocol::Visor
                };
                let hello = self.visor.net.login(headset::active());
                self.visor_send(&hello);
                log::info!(
                    "VR protocol: Visor HANDSHAKE sent (network {}, preference {preference:?})",
                    horizon_visor::NETWORK_VERSION
                );
            }
        }
    }

    /// A respawn (`vivecraft$resetServerState` on `handleRespawn`); Visor sends its HANDSHAKE
    /// on login only.
    pub(super) fn vr_respawn(&mut self) {
        if matches!(
            self.visor.protocol,
            Protocol::Vivecraft | Protocol::Fallback
        ) {
            self.vivecraft_reset(true);
        } else {
            self.vivecraft_reset(false);
        }
    }

    /// A payload on `visor:channel` from the server (`ClientPacketHandler.handlePacket`).
    pub(super) fn visor_payload(&mut self, data: &[u8]) {
        let payload = match S2c::read(data) {
            Ok(p) => p,
            Err(e) => {
                log::debug!("Visor: a bad payload from the server: {e}");
                return;
            }
        };
        let is_handshake = payload == S2c::Handshake;
        // An explicit Vivecraft choice never said HANDSHAKE. Auto's fallback did, so
        // accept its delayed answer, but no state packets before that answer.
        if !self.visor.protocol.accepts(&payload) {
            return;
        }
        let Some(event) = self.visor.net.handle(payload) else {
            return;
        };
        if is_handshake {
            self.visor.protocol = Protocol::Visor;
            self.vivecraft_reset(false);
        }
        match event {
            Event::ServerSupportsVisor => {
                log::info!(
                    "VR protocol: the server speaks Visor (network {})",
                    horizon_visor::NETWORK_VERSION
                );
                // `receivedHandShake`: a remote server says so in the chat.
                if self.integrated.is_none() {
                    self.system_message(Text::tr("visor.messages.server_supports", &[]));
                }
            }
            Event::SettingsChanged => {
                log::info!("Visor: server settings {:?}", self.visor.net.settings);
                self.visor_settings_changed();
            }
            Event::RotationY(rotation) => {
                // `setRotationY`: the room faces where it did when the player left.
                headset::set_body_yaw(180.0 - rotation.to_degrees());
            }
            Event::OffhandSlot(slot) => {
                // `TaskHotBar.setOffhandSlot`: the slot the server remembered; the main hand
                // steps aside if it holds that slot.
                let mut selected = self.selected;
                self.visor.hotbar.set_offhand_slot(slot, &mut selected);
                if selected != self.selected {
                    self.hotbar_presses.push(selected);
                }
            }
            Event::BlockDamage { player, pos, stage } => {
                self.visor_block_damage(player, pos, stage)
            }
            Event::PlayerTracked(_) => {}
        }
    }

    /// Where the player stands for the server: what `ClientNetworking.sendVRPlayerState`
    /// puts in `LocalState`.
    fn visor_local_state(&self, pose: &super::vivecraft_net::LocalPose) -> LocalState {
        let settings = headset::settings();
        let measured = horizon_vivecraft::player::player_height(&settings);
        let world_scale = headset::world_scale();
        let height = body::round_height(visor_model_height(
            self.visor_height_mode(),
            measured,
            world_scale,
        ));
        let to_element =
            |p: horizon_vivecraft::network::Pose| PoseElement::new(p.position, p.orientation);
        let mut data = PoseData {
            hmd: to_element(pose.hmd),
            main_hand: to_element(pose.main),
            off_hand: to_element(pose.off),
            ..PoseData::default()
        };
        self.visor_tracked_body(&mut data);
        LocalState {
            height,
            world_scale,
            rotation_y: std::f32::consts::PI - headset::body_yaw().to_radians(),
            left_handed: pose.left_handed,
            active_hand_main: self.visor.active_main,
            offhand_slot: self.visor.hotbar.slot_offhand,
            gun_angle: self.visor_gun_angle(),
            body_type: match settings.enum_name("visorBodyType") {
                "FULL_BODY" => BodyType::FullBody,
                _ => BodyType::HandsOnly,
            },
            overlay_focused: self.screen != super::Screen::Playing,
            pose: data,
        }
    }

    /// `VRClientSettings.getHeightMode`: the server's supported mode wins over the player's.
    pub(super) fn visor_height_mode(&self) -> SupportedHeightMode {
        match self.vr_server_settings().supported_height_mode {
            SupportedHeightMode::Both => match headset::settings().enum_name("visorHeightMode") {
                "REAL_SIZE" => SupportedHeightMode::RealSize,
                _ => SupportedHeightMode::MatchModel,
            },
            forced => forced,
        }
    }

    /// `XrRawPoseHandler.extractGunAngle` of the left controller.
    fn visor_gun_angle(&self) -> f32 {
        let left = if headset::settings().left_handed() {
            headset::main_hand()
        } else {
            headset::off_hand()
        };
        // From the turn as the item poses use it: a runtime that gives the aim as the grip has
        // the default (`updateAimToGrip`), so a player sends 60, not 0.
        left.map_or(body::DEFAULT_GUN_ANGLE, |h| {
            body::gun_angle_of(horizon_visor::item_pose::aim_to_grip(h.aim.1, h.grip.1))
        })
    }

    /// One client tick: Auto's wait, the state to the server, and other players.
    pub(super) fn visor_tick(&mut self) {
        self.visor_in_block_tick();
        if self.visor.protocol == Protocol::Auto {
            self.visor.waited += 1;
            if self.visor.waited > AUTO_WAIT_TICKS {
                // No Visor answer: this is not a Visor server; speak Vivecraft.
                log::info!(
                    "VR protocol: no Visor answer in {AUTO_WAIT_TICKS} ticks, speaking Vivecraft"
                );
                self.visor.protocol = Protocol::Fallback;
                self.vivecraft_reset(true);
            }
        }
        if self.visor.protocol != Protocol::Visor {
            return;
        }
        self.visor.net.tick();
        if !self.visor_active() || !headset::active() {
            return;
        }
        let Some(pose) = self.vr_local_pose() else {
            return;
        };
        let state = self.visor_local_state(&pose);
        for p in self.visor.net.state_updates(&state) {
            self.visor_send(&p);
        }
    }

    /// The player's own request to turn a protocol off at runtime is a settings change: the next
    /// login picks it up. Nothing to do on the open connection.
    fn visor_settings_changed(&mut self) {
        // Mechanics read `vr_server_settings` when they run; nothing is cached here.
    }
}

/// `VRLocalPlayerImpl.getModelHeight`: `DEFAULT_FULL_HEIGHT * modelHeightRatio * worldScale`,
/// where the ratio is the measured height over the default in `REAL_SIZE` and 1 in `MATCH_MODEL`.
pub(super) fn visor_model_height(
    mode: SupportedHeightMode,
    measured: f32,
    world_scale: f32,
) -> f32 {
    let ratio = match mode {
        SupportedHeightMode::RealSize => body::model_height_ratio(measured),
        _ => 1.0,
    };
    body::DEFAULT_FULL_HEIGHT * ratio * world_scale
}

/// A remote Visor player's pose as the arms-only model takes it.
pub(super) fn visor_rot_info(
    p: &horizon_visor::client::RemotePlayer,
) -> horizon_vivecraft::player_model::RotInfo {
    horizon_vivecraft::player_model::RotInfo {
        seated: false,
        left_handed: p.left_handed,
        head_pos: p.pose.hmd.position,
        head_quat: p.pose.hmd.orientation,
        main_hand_pos: p.pose.main_hand.position,
        main_hand_quat: p.pose.main_hand.orientation,
        off_hand_pos: p.pose.off_hand.position,
        off_hand_quat: p.pose.off_hand.orientation,
        // The height Visor sends is the rendered model's, world scale included, and the pose
        // offsets are scaled already: the model takes the height alone.
        world_scale: 1.0,
        height_scale: p.height_scale(),
        head_yaw_body: true,
        gun_angle: Some(p.gun_angle),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_accepts_a_delayed_handshake_but_an_explicit_choice_does_not() {
        assert!(Protocol::Fallback.accepts(&S2c::Handshake));
        assert!(!Protocol::Fallback.accepts(&S2c::ServerSettings(String::new())));
        assert!(!Protocol::Vivecraft.accepts(&S2c::Handshake));
        assert!(!Protocol::None.accepts(&S2c::Handshake));
        assert!(Protocol::Visor.accepts(&S2c::ServerSettings(String::new())));
    }

    #[test]
    fn remote_model_keeps_small_and_large_visor_scales() {
        use horizon_visor::payload::StartTracking;
        let mut net = ClientNet::default();
        for (height, expected) in [(0.38, 0.25), (3.04, 2.0)] {
            net.handle(S2c::OtherStartTracking(Box::new(StartTracking {
                player: 1,
                pose: PoseData::default(),
                body_type: "hands_only".into(),
                left_handed: false,
                rotation_y: 0.0,
                world_scale: expected,
                full_height: height,
                gun_angle: 60.0,
                overlay_focused: false,
            })));
            assert_eq!(visor_rot_info(&net.players[&1]).height_scale, expected);
        }
    }

    #[test]
    fn model_height_follows_the_mode_and_the_world_scale() {
        use SupportedHeightMode::*;
        assert!((visor_model_height(MatchModel, 1.8, 1.0) - 1.52).abs() < 1e-6);
        assert!((visor_model_height(RealSize, 1.8, 1.0) - 1.8).abs() < 1e-6);
        assert!((visor_model_height(RealSize, 1.8, 2.0) - 3.6).abs() < 1e-6);
        assert!((visor_model_height(MatchModel, 1.8, 0.5) - 0.76).abs() < 1e-6);
    }
}
