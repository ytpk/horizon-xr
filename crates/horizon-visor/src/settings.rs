// SPDX-License-Identifier: MPL-2.0
//! `VRServerSettings` and the `SERVER_SETTINGS` payload (`ServerConfig`). Copyright (C)
//! Phoenix-Ra and Visor contributors; MPL-2.0 (see LICENSE-MPL-2.0).
//!
//! The payload is the YAML of the fields marked `@SendSettingToClient`, keyed by the Java field
//! names (`twoHandedVR`, `betterSwinging`, ...). The mod writes it with its config library
//! (`Config.toPlaintext`, whose exact layout is in the AtumConfig library, not read here);
//! VisorPlugin writes flat `key: value` lines. The parser takes any flat mapping.

/// `SupportedMovement`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SupportedMovement {
    Controller,
    Teleport,
    Both,
}

impl SupportedMovement {
    pub fn name(self) -> &'static str {
        match self {
            Self::Controller => "CONTROLLER",
            Self::Teleport => "TELEPORT",
            Self::Both => "BOTH",
        }
    }

    pub fn from_name(s: &str) -> Option<Self> {
        match s.trim().to_ascii_uppercase().as_str() {
            "CONTROLLER" => Some(Self::Controller),
            "TELEPORT" => Some(Self::Teleport),
            "BOTH" => Some(Self::Both),
            _ => None,
        }
    }
}

/// `SupportedHeightMode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SupportedHeightMode {
    MatchModel,
    RealSize,
    Both,
}

impl SupportedHeightMode {
    pub fn name(self) -> &'static str {
        match self {
            Self::MatchModel => "MATCH_MODEL",
            Self::RealSize => "REAL_SIZE",
            Self::Both => "BOTH",
        }
    }

    pub fn from_name(s: &str) -> Option<Self> {
        match s.trim().to_ascii_uppercase().as_str() {
            "MATCH_MODEL" => Some(Self::MatchModel),
            "REAL_SIZE" => Some(Self::RealSize),
            "BOTH" => Some(Self::Both),
            _ => None,
        }
    }
}

/// The static fields of `VRServerSettings`.
#[derive(Clone, Debug, PartialEq)]
pub struct ServerSettings {
    pub server_debug: bool,
    pub vr_only: bool,
    /// `two_handed`: the off hand acts too (`OFFHAND_SLOT`, `ACTIVE_HAND`).
    pub two_handed_vr: bool,
    /// `better_swinging`: `SWING_ATTACK` and `SWING_BLOCK` instead of vanilla clicks.
    pub better_swinging: bool,
    /// `roomscale_shield`: a raised shield blocks without holding use.
    pub roomscale_shield_blocking: bool,
    pub attacks_while_blocking: bool,
    /// Server only: ms before a swung block's damage resets.
    pub swinging_repair_delay: i64,
    /// Server only: the mining speed of swings.
    pub swinging_mining_speed: f32,
    pub room_crawling_supported: bool,
    pub room_climbing_supported: bool,
    pub pvp_vr_vs_vanilla: bool,
    pub pvp_vr_vs_vr: bool,
    pub notify_pvp_blocked: bool,
    pub creeper_swell_distance: f64,
    pub supported_movement: SupportedMovement,
    pub teleport_up_limit: i32,
    pub teleport_down_limit: i32,
    pub teleport_forward_limit: i32,
    pub supported_height_mode: SupportedHeightMode,
    pub body_trackers_supported: bool,
    pub hand_trackers_supported: bool,
    /// Server only.
    pub body_trackers_send_to_others: bool,
    /// Server only.
    pub hand_trackers_send_to_others: bool,
}

impl Default for ServerSettings {
    /// `VRServerSettings.resetToDefaults`: what a Visor server without a config file runs,
    /// and what a Visor client starts a local world with.
    fn default() -> Self {
        Self {
            server_debug: false,
            vr_only: false,
            two_handed_vr: true,
            better_swinging: true,
            roomscale_shield_blocking: true,
            attacks_while_blocking: true,
            swinging_repair_delay: 400,
            swinging_mining_speed: 1.5,
            room_crawling_supported: true,
            room_climbing_supported: true,
            pvp_vr_vs_vanilla: true,
            pvp_vr_vs_vr: true,
            notify_pvp_blocked: false,
            creeper_swell_distance: 1.75,
            supported_movement: SupportedMovement::Both,
            teleport_up_limit: 1,
            teleport_down_limit: 4,
            teleport_forward_limit: 16,
            supported_height_mode: SupportedHeightMode::Both,
            body_trackers_supported: true,
            hand_trackers_supported: true,
            body_trackers_send_to_others: true,
            hand_trackers_send_to_others: true,
        }
    }
}

impl ServerSettings {
    /// `VRServerSettings.joinedDedicatedServer`: what the client assumes on a server that
    /// has not (yet) said anything: the mechanics that need a server are off and movement is the
    /// controller. This is also Horizon's one profile for a vanilla server, whichever VR
    /// protocol the client picked.
    pub fn vanilla_server() -> Self {
        Self {
            vr_only: false,
            server_debug: false,
            room_crawling_supported: false,
            room_climbing_supported: false,
            pvp_vr_vs_vanilla: true,
            pvp_vr_vs_vr: true,
            notify_pvp_blocked: false,
            two_handed_vr: false,
            better_swinging: false,
            roomscale_shield_blocking: false,
            attacks_while_blocking: true,
            creeper_swell_distance: 1.75,
            supported_movement: SupportedMovement::Controller,
            supported_height_mode: SupportedHeightMode::MatchModel,
            body_trackers_supported: false,
            hand_trackers_supported: false,
            ..Self::default()
        }
    }

    /// VisorPlugin's `VisorSettings.defaults()`: the config file it writes on first start (every
    /// mechanic that needs server code off). Used by the tests of the plugin's wire shape and
    /// for a server file without keys.
    pub fn plugin_defaults() -> Self {
        Self {
            two_handed_vr: false,
            better_swinging: false,
            roomscale_shield_blocking: false,
            room_crawling_supported: false,
            room_climbing_supported: false,
            supported_movement: SupportedMovement::Controller,
            body_trackers_supported: false,
            hand_trackers_supported: false,
            ..Self::default()
        }
    }

    /// `ServerConfig.updateSettings`: lay the keys of a flat YAML mapping over the settings.
    /// A key that is missing keeps its value; one that does not parse is skipped (the Java
    /// throws and drops the whole update; skipping keeps the rest, which is the safer end).
    pub fn apply_text(&mut self, text: &str) {
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with("---") {
                continue;
            }
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let value = value
                .split(" #")
                .next()
                .unwrap_or("")
                .trim()
                .trim_matches(['"', '\'']);
            self.set(key.trim().trim_matches(['"', '\'']), value);
        }
    }

    /// One field by its Java name; false when the key is unknown or the value does not fit.
    pub fn set(&mut self, key: &str, value: &str) -> bool {
        let flag = |v: &str| match v {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        };
        macro_rules! boolean {
            ($f:ident) => {
                flag(value).map(|v| self.$f = v).is_some()
            };
        }
        match key {
            "serverDebug" => boolean!(server_debug),
            "vrOnly" => boolean!(vr_only),
            "twoHandedVR" => boolean!(two_handed_vr),
            "betterSwinging" => boolean!(better_swinging),
            "roomscaleShieldBlocking" => boolean!(roomscale_shield_blocking),
            "attacksWhileBlocking" => boolean!(attacks_while_blocking),
            "swingingRepairDelay" => parse_into(value, &mut self.swinging_repair_delay),
            "swingingMiningSpeed" => parse_into(value, &mut self.swinging_mining_speed),
            "roomCrawlingSupported" => boolean!(room_crawling_supported),
            "roomClimbingSupported" => boolean!(room_climbing_supported),
            "pvpVRvsVanilla" => boolean!(pvp_vr_vs_vanilla),
            "pvpVRvsVR" => boolean!(pvp_vr_vs_vr),
            "notifyPvpBlocked" => boolean!(notify_pvp_blocked),
            "creeperSwellDistance" => parse_into(value, &mut self.creeper_swell_distance),
            "supportedMovement" => SupportedMovement::from_name(value)
                .map(|v| self.supported_movement = v)
                .is_some(),
            "teleportUpLimit" => parse_into(value, &mut self.teleport_up_limit),
            "teleportDownLimit" => parse_into(value, &mut self.teleport_down_limit),
            "teleportForwardLimit" => parse_into(value, &mut self.teleport_forward_limit),
            "supportedHeightMode" => SupportedHeightMode::from_name(value)
                .map(|v| self.supported_height_mode = v)
                .is_some(),
            "bodyTrackersSupported" => boolean!(body_trackers_supported),
            "handTrackersSupported" => boolean!(hand_trackers_supported),
            "bodyTrackersSendToOthers" => boolean!(body_trackers_send_to_others),
            "handTrackersSendToOthers" => boolean!(hand_trackers_send_to_others),
            _ => false,
        }
    }

    /// `ServerConfig.getSettingsForClient().toPlaintext()`: the fields marked
    /// `@SendSettingToClient`, in declaration order, as flat YAML.
    pub fn to_client_text(&self) -> String {
        format!(
            "serverDebug: {}\nvrOnly: {}\ntwoHandedVR: {}\nbetterSwinging: {}\n\
             roomscaleShieldBlocking: {}\nattacksWhileBlocking: {}\nroomCrawlingSupported: {}\n\
             roomClimbingSupported: {}\npvpVRvsVanilla: {}\npvpVRvsVR: {}\nnotifyPvpBlocked: {}\n\
             creeperSwellDistance: {}\nsupportedMovement: {}\nteleportUpLimit: {}\n\
             teleportDownLimit: {}\nteleportForwardLimit: {}\nsupportedHeightMode: {}\n\
             bodyTrackersSupported: {}\nhandTrackersSupported: {}\n",
            self.server_debug,
            self.vr_only,
            self.two_handed_vr,
            self.better_swinging,
            self.roomscale_shield_blocking,
            self.attacks_while_blocking,
            self.room_crawling_supported,
            self.room_climbing_supported,
            self.pvp_vr_vs_vanilla,
            self.pvp_vr_vs_vr,
            self.notify_pvp_blocked,
            java_double(self.creeper_swell_distance),
            self.supported_movement.name(),
            self.teleport_up_limit,
            self.teleport_down_limit,
            self.teleport_forward_limit,
            self.supported_height_mode.name(),
            self.body_trackers_supported,
            self.hand_trackers_supported,
        )
    }

    /// The text of the whole server file (`server_settings.yml`) as AtumConfig dumps it
    /// (`ServerConfig.applySettingsTo` then `config.save()`): every field of `VRServerSettings` in
    /// declaration order as block-style YAML (SnakeYAML, indent 2), the `@ConfigComment` lines
    /// above their key as `# ` comments, enums by name, a `long` and an `int` bare, a `float` and a
    /// `double` with their decimal point.
    pub fn to_file_text(&self) -> String {
        let mut out = String::new();
        let mut field = |comment: &[&str], key: &str, value: String| {
            for line in comment {
                out.push_str("# ");
                out.push_str(line);
                out.push('\n');
            }
            out.push_str(&format!("{key}: {value}\n"));
        };
        field(&[], "serverDebug", self.server_debug.to_string());
        field(&[], "vrOnly", self.vr_only.to_string());
        field(&[], "twoHandedVR", self.two_handed_vr.to_string());
        field(&[], "betterSwinging", self.better_swinging.to_string());
        field(
            &["Block with a raised shield without holding use"],
            "roomscaleShieldBlocking",
            self.roomscale_shield_blocking.to_string(),
        );
        field(
            &[],
            "attacksWhileBlocking",
            self.attacks_while_blocking.to_string(),
        );
        field(
            &[],
            "swingingRepairDelay",
            self.swinging_repair_delay.to_string(),
        );
        field(
            &[],
            "swingingMiningSpeed",
            java_double(f64::from(self.swinging_mining_speed)),
        );
        field(
            &[],
            "roomCrawlingSupported",
            self.room_crawling_supported.to_string(),
        );
        field(
            &[],
            "roomClimbingSupported",
            self.room_climbing_supported.to_string(),
        );
        field(&[], "pvpVRvsVanilla", self.pvp_vr_vs_vanilla.to_string());
        field(&[], "pvpVRvsVR", self.pvp_vr_vs_vr.to_string());
        field(&[], "notifyPvpBlocked", self.notify_pvp_blocked.to_string());
        field(
            &[],
            "creeperSwellDistance",
            java_double(self.creeper_swell_distance),
        );
        field(
            &["Movement allowed for VR players: CONTROLLER, TELEPORT or BOTH"],
            "supportedMovement",
            self.supported_movement.name().to_string(),
        );
        field(
            &["Teleport distance limits in blocks"],
            "teleportUpLimit",
            self.teleport_up_limit.to_string(),
        );
        field(
            &[],
            "teleportDownLimit",
            self.teleport_down_limit.to_string(),
        );
        field(
            &[],
            "teleportForwardLimit",
            self.teleport_forward_limit.to_string(),
        );
        field(
            &[
                "Height mode allowed for VR players: MATCH_MODEL (player model scale not modified), REAL_SIZE (player model scale modified) or BOTH",
            ],
            "supportedHeightMode",
            self.supported_height_mode.name().to_string(),
        );
        field(
            &["[TRACKERS] - Potentially, more network consumption when enabled"],
            "bodyTrackersSupported",
            self.body_trackers_supported.to_string(),
        );
        field(
            &[],
            "handTrackersSupported",
            self.hand_trackers_supported.to_string(),
        );
        field(
            &[
                "Send body tracker data to other players (if enabled, players with Visor mod and within VR player visibility range receive such data)",
            ],
            "bodyTrackersSendToOthers",
            self.body_trackers_send_to_others.to_string(),
        );
        field(
            &[
                "Send hand tracking data to other players (if enabled, players with Visor mod and within VR player visibility range receive such data)",
            ],
            "handTrackersSendToOthers",
            self.hand_trackers_send_to_others.to_string(),
        );
        out
    }
}

fn parse_into<T: std::str::FromStr>(value: &str, out: &mut T) -> bool {
    match value.parse() {
        Ok(v) => {
            *out = v;
            true
        }
        Err(_) => false,
    }
}

/// A Java `double` printed with a decimal point (`1.0`, not `1`).
fn java_double(v: f64) -> String {
    if v.fract() == 0.0 {
        format!("{v:.1}")
    } else {
        v.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The text VisorPlugin's `VisorSettings.toClientYaml()` builds (read from the Java).
    const PLUGIN_TEXT: &str = "vrOnly: false\nserverDebug: false\ntwoHandedVR: true\n\
        betterSwinging: true\nroomscaleShieldBlocking: false\nattacksWhileBlocking: true\n\
        roomCrawlingSupported: true\nroomClimbingSupported: false\npvpVRvsVanilla: true\n\
        pvpVRvsVR: false\nnotifyPvpBlocked: true\ncreeperSwellDistance: 1.75\n\
        supportedMovement: TELEPORT\nteleportUpLimit: 2\nteleportDownLimit: 5\n\
        teleportForwardLimit: 20\nbodyTrackersSupported: true\nhandTrackersSupported: false\n";

    #[test]
    fn the_plugins_text_sets_every_field_and_leaves_the_height_mode() {
        let mut s = ServerSettings::vanilla_server();
        s.apply_text(PLUGIN_TEXT);
        assert!(s.two_handed_vr && s.better_swinging && !s.roomscale_shield_blocking);
        assert!(s.room_crawling_supported && !s.room_climbing_supported);
        assert!(s.pvp_vr_vs_vanilla && !s.pvp_vr_vs_vr && s.notify_pvp_blocked);
        assert_eq!(s.supported_movement, SupportedMovement::Teleport);
        assert_eq!(
            (
                s.teleport_up_limit,
                s.teleport_down_limit,
                s.teleport_forward_limit
            ),
            (2, 5, 20)
        );
        assert!(s.body_trackers_supported && !s.hand_trackers_supported);
        // The plugin never sends `supportedHeightMode`: the client keeps what it had.
        assert_eq!(s.supported_height_mode, SupportedHeightMode::MatchModel);
    }

    #[test]
    fn a_mod_text_with_quotes_comments_and_unknown_keys_is_read() {
        let mut s = ServerSettings::vanilla_server();
        s.apply_text(
            "---\n# Visor\nvrOnly: true  # comment\nsupportedMovement: 'BOTH'\n\
             supportedHeightMode: \"REAL_SIZE\"\nsomethingNew: 3\nteleportUpLimit: bad\n",
        );
        assert!(s.vr_only);
        assert_eq!(s.supported_movement, SupportedMovement::Both);
        assert_eq!(s.supported_height_mode, SupportedHeightMode::RealSize);
        // A bad number is skipped, the default stays.
        assert_eq!(s.teleport_up_limit, 1);
    }

    #[test]
    fn the_client_text_round_trips() {
        let mut s = ServerSettings::default();
        s.supported_movement = SupportedMovement::Teleport;
        s.creeper_swell_distance = 2.0;
        s.room_climbing_supported = false;
        let text = s.to_client_text();
        assert!(text.contains("creeperSwellDistance: 2.0\n"));
        assert!(text.starts_with("serverDebug: false\nvrOnly: false\n"));
        let mut back = ServerSettings::vanilla_server();
        back.apply_text(&text);
        // The server only fields are not in the client text.
        back.swinging_repair_delay = s.swinging_repair_delay;
        back.swinging_mining_speed = s.swinging_mining_speed;
        back.body_trackers_send_to_others = s.body_trackers_send_to_others;
        back.hand_trackers_send_to_others = s.hand_trackers_send_to_others;
        assert_eq!(back, s);
    }

    // `joinedDedicatedServer` from the Java source.
    #[test]
    fn the_vanilla_server_profile_turns_the_server_mechanics_off() {
        let s = ServerSettings::vanilla_server();
        assert!(!s.room_crawling_supported && !s.room_climbing_supported);
        assert!(!s.two_handed_vr && !s.better_swinging && !s.roomscale_shield_blocking);
        assert!(s.attacks_while_blocking);
        assert_eq!(s.supported_movement, SupportedMovement::Controller);
        assert_eq!(s.supported_height_mode, SupportedHeightMode::MatchModel);
        assert!(!s.body_trackers_supported && !s.hand_trackers_supported);
        assert_eq!(s.teleport_forward_limit, 16);
    }

    #[test]
    fn the_server_file_is_what_atumconfig_dumps_for_the_defaults() {
        // `server_settings.yml` of a fresh Visor server (declaration order, `# ` comments from
        // `@ConfigComment`, SnakeYAML's block style).
        let expected = "serverDebug: false\nvrOnly: false\ntwoHandedVR: true\nbetterSwinging: true\n\
# Block with a raised shield without holding use\nroomscaleShieldBlocking: true\n\
attacksWhileBlocking: true\nswingingRepairDelay: 400\nswingingMiningSpeed: 1.5\n\
roomCrawlingSupported: true\nroomClimbingSupported: true\npvpVRvsVanilla: true\n\
pvpVRvsVR: true\nnotifyPvpBlocked: false\ncreeperSwellDistance: 1.75\n\
# Movement allowed for VR players: CONTROLLER, TELEPORT or BOTH\nsupportedMovement: BOTH\n\
# Teleport distance limits in blocks\nteleportUpLimit: 1\nteleportDownLimit: 4\n\
teleportForwardLimit: 16\n\
# Height mode allowed for VR players: MATCH_MODEL (player model scale not modified), REAL_SIZE (player model scale modified) or BOTH\n\
supportedHeightMode: BOTH\n\
# [TRACKERS] - Potentially, more network consumption when enabled\nbodyTrackersSupported: true\n\
handTrackersSupported: true\n\
# Send body tracker data to other players (if enabled, players with Visor mod and within VR player visibility range receive such data)\n\
bodyTrackersSendToOthers: true\n\
# Send hand tracking data to other players (if enabled, players with Visor mod and within VR player visibility range receive such data)\n\
handTrackersSendToOthers: true\n";
        assert_eq!(ServerSettings::default().to_file_text(), expected);
    }

    #[test]
    fn the_whole_file_text_keeps_the_server_fields() {
        let mut s = ServerSettings::default();
        s.swinging_repair_delay = 250;
        s.hand_trackers_send_to_others = false;
        let mut back = ServerSettings::vanilla_server();
        back.apply_text(&s.to_file_text());
        assert_eq!(back, s);
    }
}
