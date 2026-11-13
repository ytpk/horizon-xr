// SPDX-License-Identifier: LGPL-3.0-or-later
//! The Vivecraft server half: ports `org.vivecraft.server.ServerNetworking`,
//! `ServerVivePlayer`, `ServerVRPlayers`, `ServerUtil` (kick/format) and the
//! network related values of `ServerConfig`.
//! Copyright (c) Vivecraft contributors; LGPL-3.0-or-later (see LICENSE-LGPL-3.0).
//!
//! No I/O and no world here. [`VrServer::handle`] takes a decoded payload and returns the
//! payloads to answer with and the [`Action`]s the host must apply to its world (a
//! teleport, food exhaustion, the crawl pose); the host calls [`VrServer::tick_player`]
//! once per tick for each player and sends what [`VrServer::state_updates`] returns.

use crate::math;
use crate::network::{
    C2s, ClimbeyBlockmode, FbtMode, MAX_SUPPORTED_NETWORK_PROTOCOL, MIN_SUPPORTED_NETWORK_PROTOCOL,
    NetworkVersion, S2c, VrBodyPart, VrPlayerState,
};
use glam::{DVec3, Vec3};
use std::collections::{BTreeMap, HashMap};

/// The network related values of `ServerConfig` (`vivecraft-server-config.toml`) with
/// Vivecraft's defaults.
#[derive(Clone, Debug, PartialEq)]
pub struct ServerConfig {
    pub debug: bool,
    pub vr_only: bool,
    pub vive_only: bool,
    pub allow_op: bool,
    pub send_data_to_owner: bool,
    pub kick_players_on_setting_update: bool,
    pub vr_fun: bool,
    pub dual_wielding: bool,
    pub allow_attacks_while_blocking: bool,
    pub allow_faster_block_breaking: bool,
    pub allow_roomscale_shield_blocking: bool,
    /// `roomscaleShieldCooldown` ticks (0 off) after a hit of at least
    /// `roomscaleShieldCooldownDamageTrigger`.
    pub roomscale_shield_cooldown: i32,
    pub roomscale_shield_cooldown_damage_trigger: f64,
    pub climbey_enabled: bool,
    pub climbey_food_exhaustion: bool,
    pub climbey_blockmode: ClimbeyBlockmode,
    pub climbey_blocklist: Vec<String>,
    pub crawling_enabled: bool,
    pub teleport_enabled: bool,
    pub teleport_food_exhaustion: bool,
    pub teleport_limited_survival: bool,
    pub teleport_up_limit: i32,
    pub teleport_down_limit: i32,
    pub teleport_horizontal_limit: i32,
    pub worldscale_limited: bool,
    pub worldscale_min: f64,
    pub worldscale_max: f64,
    pub force_third_person_items: bool,
    pub force_third_person_items_custom: bool,
    pub vr_switching_enabled: bool,
    /// `bow.*`: the damage a standing or seated VR bow adds, and headshots.
    pub bow_standing_multiplier: f64,
    pub bow_seated_multiplier: f64,
    pub bow_standing_headshot_multiplier: f64,
    pub bow_seated_headshot_multiplier: f64,
    pub bow_vanilla_headshot_multiplier: f64,
    pub bow_headshot_indicator: HeadshotIndicator,
    /// `vrChanges.*` values that are not at their defaults, for `SERVER_VR_CHANGES`.
    pub vr_changes: BTreeMap<String, String>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            debug: false,
            vr_only: false,
            vive_only: false,
            allow_op: true,
            send_data_to_owner: false,
            kick_players_on_setting_update: false,
            vr_fun: true,
            dual_wielding: true,
            allow_attacks_while_blocking: true,
            allow_faster_block_breaking: true,
            allow_roomscale_shield_blocking: true,
            roomscale_shield_cooldown: 0,
            roomscale_shield_cooldown_damage_trigger: 3.0,
            climbey_enabled: true,
            climbey_food_exhaustion: true,
            climbey_blockmode: ClimbeyBlockmode::Disabled,
            climbey_blocklist: vec!["white_wool".into(), "dirt".into(), "grass_block".into()],
            crawling_enabled: true,
            teleport_enabled: true,
            teleport_food_exhaustion: false,
            teleport_limited_survival: false,
            teleport_up_limit: 4,
            teleport_down_limit: 4,
            teleport_horizontal_limit: 16,
            worldscale_limited: false,
            worldscale_min: 0.5,
            worldscale_max: 2.0,
            force_third_person_items: false,
            force_third_person_items_custom: false,
            vr_switching_enabled: true,
            bow_standing_multiplier: 2.0,
            bow_seated_multiplier: 1.0,
            bow_standing_headshot_multiplier: 3.0,
            bow_seated_headshot_multiplier: 2.0,
            bow_vanilla_headshot_multiplier: 1.0,
            bow_headshot_indicator: HeadshotIndicator::Both,
            vr_changes: BTreeMap::new(),
        }
    }
}

/// `HeadshotIndicator`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeadshotIndicator {
    None,
    Audio,
    Visual,
    Both,
}

/// How an arrow's damage changes with who shot it and where it hit
/// (`AbstractArrowMixin.vivecraft$damageMultiplier`): `shooter` is `Some(seated)` for a VR
/// player, `None` for a player without VR.
pub fn arrow_multiplier(config: &ServerConfig, shooter: Option<bool>, headshot: bool) -> f64 {
    let mut multiplier = 1.0;
    if headshot {
        multiplier = match shooter {
            Some(true) => config.bow_seated_headshot_multiplier,
            Some(false) => config.bow_standing_headshot_multiplier,
            None => config.bow_vanilla_headshot_multiplier,
        };
    }
    // With headshots off the regular multiplier still counts.
    match shooter {
        Some(true) => multiplier.max(config.bow_seated_multiplier),
        Some(false) => multiplier.max(config.bow_standing_multiplier),
        None => multiplier,
    }
}

/// The kinds of entity `Utils.getEntityHeadHitbox` knows a head of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeadKind {
    /// The head is on top at the eyes (humanoids and the like).
    Eyes,
    /// Another living entity: the head is in front, along its body yaw.
    Front,
    /// Nothing (the ender dragon).
    None,
}

/// `getEntityHeadHitbox`'s choice by entity id.
pub fn head_kind(entity: &str, swimming: bool) -> HeadKind {
    let name = entity.strip_prefix("minecraft:").unwrap_or(entity);
    let upper = matches!(
        name,
        "zombie"
            | "husk"
            | "drowned"
            | "zombie_villager"
            | "zombified_piglin"
            | "piglin"
            | "piglin_brute"
            | "skeleton"
            | "stray"
            | "wither_skeleton"
            | "bogged"
            | "parched"
            | "witch"
            | "pillager"
            | "vindicator"
            | "evoker"
            | "illusioner"
            | "blaze"
            | "creeper"
            | "enderman"
            | "villager"
            | "wandering_trader"
            | "snow_golem"
            | "vex"
            | "strider"
            | "creaking"
    ) || (name == "player" && !swimming);
    if upper {
        HeadKind::Eyes
    } else if name == "ender_dragon" {
        HeadKind::None
    } else {
        HeadKind::Front
    }
}

/// `getEntityHeadHitbox`: the head's box as (min, max). `feet` is the entity's position,
/// `eye_height` its eyes' height, `width` its box width, `body_yaw` its body's yaw in degrees.
pub fn head_hitbox(
    kind: HeadKind,
    feet: DVec3,
    eye_height: f64,
    width: f64,
    baby: bool,
    body_yaw: f32,
    inflate: f64,
) -> Option<(DVec3, DVec3)> {
    let eyes = feet + DVec3::new(0.0, eye_height, 0.0);
    let (centre, half, grow) = match kind {
        HeadKind::None => return None,
        HeadKind::Eyes => {
            let mut size = width * 0.5;
            if baby {
                size *= 1.2;
            }
            // The box is `size - inflate` below and `size + inflate` above, then inflated.
            let lo = eyes - DVec3::new(size, size - inflate, size);
            let hi = eyes + DVec3::new(size, size + inflate, size);
            return Some((lo - DVec3::splat(inflate), hi + DVec3::splat(inflate)));
        }
        HeadKind::Front => {
            let y_rot = -body_yaw.to_radians();
            let head = eyes
                + DVec3::new(f64::from(y_rot.sin()), 0.0, f64::from(y_rot.cos())) * (width * 0.5);
            let mut size = width * 0.25;
            if baby {
                size *= 1.5;
            }
            (head, size, 0.0)
        }
    };
    let h = DVec3::splat(half);
    Some((
        centre - h - DVec3::splat(grow),
        centre + h + DVec3::splat(grow),
    ))
}

/// `AABB.clip`: where along `from..to` (0 at `from`, 1 at `to`) the segment first is in the
/// box, if it touches it.
pub fn segment_enters_box(from: DVec3, to: DVec3, min: DVec3, max: DVec3) -> Option<f64> {
    let d = to - from;
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for axis in 0..3 {
        let (a, dd, lo, hi) = (from[axis], d[axis], min[axis], max[axis]);
        if dd.abs() < 1e-12 {
            if a < lo || a > hi {
                return None;
            }
        } else {
            let (mut near, mut far) = ((lo - a) / dd, (hi - a) / dd);
            if near > far {
                std::mem::swap(&mut near, &mut far);
            }
            t0 = t0.max(near);
            t1 = t1.min(far);
            if t0 > t1 {
                return None;
            }
        }
    }
    Some(t0)
}

/// `AbstractArrowMixin.vivecraft$isHeadshot`: an arrow at `at` flying along `velocity` hits the
/// head box before it gets into the body box (`body`).
pub fn is_headshot(at: DVec3, velocity: DVec3, head: (DVec3, DVec3), body: (DVec3, DVec3)) -> bool {
    let far = at + velocity * 2.0;
    // Where the arrow gets into the body; without a hit, one velocity on.
    let hit = match segment_enters_box(at, far, body.0, body.1) {
        Some(t) => at + (far - at) * t,
        None => at + velocity,
    };
    segment_enters_box(at, hit, head.0, head.1).is_some()
}

/// What the host must do in the world for a payload.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// `TELEPORT`: `absSnapTo(x, y, z)` keeping the rotation.
    Teleport { x: f32, y: f32, z: f32 },
    /// `causeFoodExhaustion`.
    FoodExhaustion(f32),
    /// Climbing: `fallDistance = 0` and the anti-fly counter reset.
    ResetFall,
    /// The player's pose is swimming (crawling).
    CrawlPose,
    /// Equipment of the main hand changed because the active body part did: the host
    /// re-applies the attribute modifiers (`applyEquipmentChange`).
    SwapActiveItem { from: VrBodyPart, to: VrBodyPart },
    /// A system message to the player.
    Message(String),
    /// Disconnect the player.
    Kick(String),
}

/// What handling one payload produced.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Output {
    pub replies: Vec<S2c>,
    pub actions: Vec<Action>,
    /// Players that track this one must be told that VR went off for them
    /// (`IS_VR_ACTIVE false`).
    pub vr_off_broadcast: bool,
}

/// `ServerVivePlayer`. Positions are relative to the player's position, which the host
/// passes in (`feet`) when a world position is wanted.
#[derive(Clone, Debug)]
pub struct ServerVivePlayer {
    state: Option<VrPlayerState>,
    body_yaw: Option<f32>,
    /// How much the roomscale bow is drawn.
    pub draw: f32,
    pub world_scale: f32,
    pub height_scale: f32,
    pub active_body_part: VrBodyPart,
    pub aim_dir_override: Option<Vec3>,
    pub aim_pos_override: Option<DVec3>,
    /// Ticks until the aim override resets.
    pub aim_reset: i32,
    pub delayed_destroy_body_part: Option<VrBodyPart>,
    pub use_body_part_for_aim: bool,
    pub crawling: bool,
    is_vr: bool,
    pub network_version: NetworkVersion,
    /// The version string the client sent on join.
    pub version: String,
    pub wants_damage_direction: bool,
}

impl Default for ServerVivePlayer {
    fn default() -> Self {
        Self {
            state: None,
            body_yaw: None,
            draw: 0.0,
            world_scale: 1.0,
            height_scale: 1.0,
            active_body_part: VrBodyPart::MainHand,
            aim_dir_override: None,
            aim_pos_override: None,
            aim_reset: 0,
            delayed_destroy_body_part: None,
            use_body_part_for_aim: false,
            crawling: false,
            is_vr: false,
            network_version: NetworkVersion::from_protocol_version(MAX_SUPPORTED_NETWORK_PROTOCOL)
                .unwrap(),
            version: String::new(),
            wants_damage_direction: false,
        }
    }
}

impl ServerVivePlayer {
    pub fn is_vr(&self) -> bool {
        self.is_vr
    }
    pub fn set_vr(&mut self, vr: bool) {
        self.is_vr = vr;
    }
    pub fn is_seated(&self) -> bool {
        self.state.as_ref().is_some_and(|s| s.seated)
    }
    pub fn is_left_handed(&self) -> bool {
        self.state.as_ref().is_some_and(|s| s.left_handed)
    }
    pub fn vr_player_state(&self) -> Option<&VrPlayerState> {
        self.state.as_ref()
    }
    /// `setVrPlayerState`.
    pub fn set_vr_player_state(&mut self, state: VrPlayerState) {
        self.state = Some(state);
        self.body_yaw = None;
    }
    /// `isDrawing`.
    pub fn is_drawing(&self) -> bool {
        !self.is_seated() && self.draw > 0.0
    }
    /// `getActiveItemBodyPart`: the bow is drawn with the main hand's item.
    pub fn active_item_body_part(&self) -> VrBodyPart {
        if self.is_drawing() {
            VrBodyPart::MainHand
        } else {
            self.active_body_part
        }
    }

    /// `getBodyPartVectorCustom`: `direction` rotated by the part's orientation; `look`
    /// is the player's look vector when no VR state is known.
    pub fn body_part_vector_custom(&self, part: VrBodyPart, direction: Vec3, look: Vec3) -> Vec3 {
        let Some(state) = &self.state else {
            return look;
        };
        let part = if state.seated || !part.available_in_mode(state.fbt_mode) {
            VrBodyPart::MainHand
        } else {
            part
        };
        state.body_part_pose(part).unwrap().orientation * direction
    }

    /// `getBodyPartDir`.
    pub fn body_part_dir(&self, part: VrBodyPart, look: Vec3) -> Vec3 {
        self.body_part_vector_custom(part, math::BACK, look)
    }

    /// `getHMDDir`.
    pub fn hmd_dir(&self, look: Vec3) -> Vec3 {
        match &self.state {
            Some(s) => s.hmd.orientation * math::BACK,
            None => look,
        }
    }

    /// `getHMDPos`; `feet` is the player's position.
    pub fn hmd_pos(&self, feet: DVec3) -> DVec3 {
        match &self.state {
            Some(s) => feet + s.hmd.position.as_dvec3(),
            None => feet + DVec3::new(0.0, 1.62, 0.0),
        }
    }

    /// `getBodyPartPos`.
    pub fn body_part_pos(
        &self,
        part: VrBodyPart,
        real_position: bool,
        feet: DVec3,
        look: Vec3,
    ) -> DVec3 {
        let Some(state) = &self.state else {
            return feet + DVec3::new(0.0, 1.62, 0.0);
        };
        let part = if part.available_in_mode(state.fbt_mode) {
            part
        } else {
            VrBodyPart::MainHand
        };
        // In seated the real position is at the head, so the visual position is rebuilt.
        if state.seated && part.is_hand() && !real_position {
            let dir = self.hmd_dir(look);
            let rot = std::f32::consts::PI / 180.0
                * if part == VrBodyPart::MainHand {
                    -35.0
                } else {
                    35.0
                };
            let (s, c) = rot.sin_cos();
            // Vec3.yRot
            let d = DVec3::new(
                f64::from(dir.x * c + dir.z * s),
                0.0,
                f64::from(-dir.x * s + dir.z * c),
            )
            .normalize();
            let ws = f64::from(self.world_scale);
            return self.hmd_pos(feet) + DVec3::new(d.x * 0.3 * ws, -0.4 * ws, d.z * 0.3 * ws);
        }
        feet + state.body_part_pose(part).unwrap().position.as_dvec3()
    }

    /// `getAimDir`.
    pub fn aim_dir(&self, ignore_use_for_aim: bool, feet: DVec3, look: Vec3) -> Vec3 {
        if let Some(d) = self.aim_dir_override {
            d
        } else if self.is_drawing() {
            let a = self.body_part_pos(self.active_body_part.opposite(), false, feet, look);
            let b = self.body_part_pos(self.active_body_part, false, feet, look);
            (a - b).normalize().as_vec3()
        } else if ignore_use_for_aim || self.use_body_part_for_aim {
            self.body_part_dir(self.active_body_part, look)
        } else {
            self.body_part_dir(VrBodyPart::MainHand, look)
        }
    }

    /// `getAimPos`.
    pub fn aim_pos(&self, ignore_use_for_aim: bool, feet: DVec3, look: Vec3) -> DVec3 {
        if let Some(p) = self.aim_pos_override {
            p
        } else if ignore_use_for_aim || self.use_body_part_for_aim {
            self.body_part_pos(self.active_body_part, false, feet, look)
        } else {
            self.body_part_pos(VrBodyPart::MainHand, false, feet, look)
        }
    }

    /// `getBodyYawRad`; `player_yaw_deg` is used when no state is known.
    pub fn body_yaw_rad(&mut self, player_yaw_deg: f32) -> f32 {
        if let Some(y) = self.body_yaw {
            return y;
        }
        let Some(state) = &self.state else {
            return player_yaw_deg.to_radians();
        };
        // Hands that do not track are invalid.
        let hands = (state
            .main_hand
            .position
            .distance_squared(state.off_hand.position)
            != 0.0)
            .then_some((state.main_hand.position, state.off_hand.position));
        let waist = state.waist;
        let yaw = math::estimate_body_yaw_rad(
            state.seated,
            state.left_handed,
            state.fbt_mode,
            state.hmd.orientation * math::BACK,
            hands.map(|h| h.0),
            hands.map(|h| h.1),
            || waist.map_or(Vec3::Z, |w| w.orientation * math::BACK),
        );
        self.body_yaw = Some(yaw);
        yaw
    }

    /// `tick`: the aim override times out.
    pub fn tick(&mut self) {
        if self.aim_reset > 0 {
            self.aim_reset -= 1;
            if self.aim_reset == 0 {
                self.aim_dir_override = None;
                self.aim_pos_override = None;
            }
        }
    }
}

/// `ServerUtil.formatMessage` for the `&name` placeholders (the `%s` form needs Java's
/// formatter and is not ported).
pub fn format_message(message: &str, player_name: &str, other: &[(&str, &str)]) -> String {
    let mut out = message.replace("&player", player_name);
    for (k, v) in other {
        out = out.replace(k, v);
    }
    out
}

/// The server's Vivecraft players (`MinecraftServerExt.vivecraft$getPlayersWithVivecraft`)
/// and config.
#[derive(Clone, Debug, Default)]
pub struct VrServer {
    pub config: ServerConfig,
    pub players: HashMap<u128, ServerVivePlayer>,
    /// The legacy clients' three packets, until all are there.
    legacy: HashMap<u128, LegacyPackets>,
}

#[derive(Clone, Debug, Default)]
struct LegacyPackets {
    head: Option<(bool, crate::network::Pose)>,
    c0: Option<(bool, crate::network::Pose)>,
    c1: Option<(bool, crate::network::Pose)>,
}

impl VrServer {
    pub fn new(config: ServerConfig) -> Self {
        Self {
            config,
            ..Self::default()
        }
    }

    /// `getVivePlayer`.
    pub fn vive_player(&self, uuid: u128) -> Option<&ServerVivePlayer> {
        self.players.get(&uuid)
    }

    /// `isVRPlayer`.
    pub fn is_vr_player(&self, uuid: u128) -> bool {
        self.players.get(&uuid).is_some_and(ServerVivePlayer::is_vr)
    }

    /// The player left: forget them.
    pub fn remove(&mut self, uuid: u128) {
        self.players.remove(&uuid);
        self.legacy.remove(&uuid);
    }

    /// `getClimbeyServerPayload`. `block_exists` says if a block id is a real block other
    /// than air (only those are sent).
    pub fn climbey_payload(&self, block_exists: &dyn Fn(&str) -> bool) -> S2c {
        let blocks = (self.config.climbey_blockmode != ClimbeyBlockmode::Disabled).then(|| {
            self.config
                .climbey_blocklist
                .iter()
                .filter(|b| block_exists(b))
                .cloned()
                .collect()
        });
        S2c::Climbing {
            allowed: self.config.climbey_enabled,
            blockmode: self.config.climbey_blockmode,
            blocks,
        }
    }

    /// `getVRSwitchingPayload`.
    pub fn vr_switching_payload(&self) -> S2c {
        S2c::VrSwitching(self.config.vr_switching_enabled && !self.config.vr_only)
    }

    fn survival_teleport_override(&self) -> S2c {
        S2c::SettingOverride {
            overrides: BTreeMap::from([
                ("limitedTeleport".into(), "true".into()),
                (
                    "teleportLimitUp".into(),
                    self.config.teleport_up_limit.to_string(),
                ),
                (
                    "teleportLimitDown".into(),
                    self.config.teleport_down_limit.to_string(),
                ),
                (
                    "teleportLimitHoriz".into(),
                    self.config.teleport_horizontal_limit.to_string(),
                ),
            ]),
            clear: !self.config.teleport_limited_survival,
        }
    }

    fn world_scale_override(&self) -> S2c {
        S2c::SettingOverride {
            overrides: BTreeMap::from([
                (
                    "worldScale.min".into(),
                    java_double(self.config.worldscale_min),
                ),
                (
                    "worldScale.max".into(),
                    java_double(self.config.worldscale_max),
                ),
            ]),
            clear: !self.config.worldscale_limited,
        }
    }

    /// `ServerNetworking.handlePacket`. `uuid` is the sending player's.
    pub fn handle(&mut self, uuid: u128, payload: C2s) -> Output {
        let mut out = Output::default();
        if matches!(payload, C2s::Unknown) {
            return out;
        }
        // Clients are expected to send VERSION first.
        let is_version = matches!(payload, C2s::Version { .. });
        if !is_version && !self.players.contains_key(&uuid) {
            return out;
        }
        match payload {
            C2s::Unknown => {}
            C2s::Version {
                version,
                vr,
                max_version,
                min_version,
                legacy,
            } => {
                let mut player = ServerVivePlayer {
                    version: version.clone(),
                    ..ServerVivePlayer::default()
                };
                if !legacy {
                    if MIN_SUPPORTED_NETWORK_PROTOCOL <= max_version
                        && min_version <= MAX_SUPPORTED_NETWORK_PROTOCOL
                    {
                        player.network_version = NetworkVersion::from_protocol_version(
                            max_version.min(MAX_SUPPORTED_NETWORK_PROTOCOL),
                        )
                        .unwrap_or(NetworkVersion::Legacy);
                    } else {
                        out.actions.push(Action::Message(
                            "Unsupported vivecraft version, VR features will not work".into(),
                        ));
                        return out;
                    }
                } else {
                    player.network_version = NetworkVersion::Legacy;
                }
                player.set_vr(vr);
                let nv = player.network_version;
                self.players.insert(uuid, player);
                let c = &self.config;
                out.replies.push(S2c::Version(crate::version_identifier()));
                out.replies.push(S2c::RequestData);
                if c.climbey_enabled {
                    // The host filters unknown blocks with `climbey_payload`; with no
                    // registry here every listed block is sent.
                    out.replies.push(self.climbey_payload(&|_| true));
                }
                if c.teleport_enabled || NetworkVersion::OptionToggle.accepts(nv) {
                    out.replies.push(S2c::Teleport {
                        allowed: c.teleport_enabled,
                        target: nv,
                    });
                }
                if c.teleport_limited_survival {
                    out.replies.push(self.survival_teleport_override());
                }
                if c.worldscale_limited {
                    out.replies.push(self.world_scale_override());
                }
                if c.force_third_person_items {
                    out.replies.push(S2c::SettingOverride {
                        overrides: BTreeMap::from([("thirdPersonItems".into(), "true".into())]),
                        clear: !c.force_third_person_items,
                    });
                }
                if c.force_third_person_items_custom {
                    out.replies.push(S2c::SettingOverride {
                        overrides: BTreeMap::from([(
                            "thirdPersonItemsCustom".into(),
                            "true".into(),
                        )]),
                        clear: !c.force_third_person_items_custom,
                    });
                }
                if c.crawling_enabled {
                    out.replies.push(S2c::Crawl {
                        allowed: true,
                        target: nv,
                    });
                }
                out.replies.push(self.vr_switching_payload());
                if NetworkVersion::DualWielding.accepts(nv) {
                    out.replies.push(S2c::DualWielding(c.dual_wielding));
                }
                if NetworkVersion::ServerVrChanges.accepts(nv) && !c.vr_changes.is_empty() {
                    out.replies.push(S2c::ServerVrChanges(c.vr_changes.clone()));
                }
                if NetworkVersion::OptionToggle.accepts(nv) {
                    out.replies
                        .push(S2c::AttackWhileBlocking(c.allow_attacks_while_blocking));
                }
                out.replies.push(S2c::NetworkVersion(nv));
            }
            C2s::IsVrActive(vr) => {
                let p = self.players.get_mut(&uuid).unwrap();
                if p.is_vr() != vr {
                    p.set_vr(!p.is_vr());
                    if !p.is_vr() {
                        out.vr_off_broadcast = true;
                    }
                }
            }
            C2s::Draw(d) => self.players.get_mut(&uuid).unwrap().draw = d,
            C2s::VrPlayerState(s) => self.players.get_mut(&uuid).unwrap().set_vr_player_state(s),
            C2s::WorldScale(v) => self.players.get_mut(&uuid).unwrap().world_scale = v,
            C2s::Height(v) => self.players.get_mut(&uuid).unwrap().height_scale = v,
            C2s::Teleport { x, y, z } => {
                if self.config.teleport_enabled {
                    out.actions.push(Action::Teleport { x, y, z });
                    // The host compares y with the player's to add the exhaustion.
                    if self.config.teleport_food_exhaustion {
                        out.actions.push(Action::FoodExhaustion(0.05));
                    }
                }
            }
            C2s::Climbing => {
                if self.config.climbey_enabled {
                    out.actions.push(Action::ResetFall);
                    if self.config.climbey_food_exhaustion {
                        // Only while holding climbing claws: the host checks the items.
                        out.actions.push(Action::FoodExhaustion(0.005));
                    }
                }
            }
            C2s::Jumping => {
                if self.config.climbey_food_exhaustion {
                    out.actions.push(Action::FoodExhaustion(0.3));
                }
            }
            C2s::ActiveHand {
                body_part,
                use_for_aim,
            } => {
                let dual = self.config.dual_wielding;
                let p = self.players.get_mut(&uuid).unwrap();
                let mut new = body_part;
                if p.is_seated() && new != VrBodyPart::Head {
                    new = VrBodyPart::MainHand;
                }
                p.use_body_part_for_aim = use_for_aim;
                if p.active_body_part != new
                    && !p.is_drawing()
                    && dual
                    && NetworkVersion::DualWielding.accepts(p.network_version)
                {
                    let from = p.active_body_part;
                    p.active_body_part = new;
                    out.actions.push(Action::SwapActiveItem { from, to: new });
                } else {
                    // Still updated, the bow uses it.
                    p.active_body_part = new;
                }
            }
            C2s::Crawl(crawling) => {
                if self.config.crawling_enabled {
                    self.players.get_mut(&uuid).unwrap().crawling = crawling;
                    if crawling {
                        out.actions.push(Action::CrawlPose);
                    }
                }
            }
            C2s::DamageDirection => {
                self.players.get_mut(&uuid).unwrap().wants_damage_direction = true
            }
            C2s::AimOverrideReset(ticks) => {
                let p = self.players.get_mut(&uuid).unwrap();
                if ticks == 0 {
                    p.aim_dir_override = None;
                    p.aim_pos_override = None;
                }
                p.aim_reset = i32::from(ticks);
            }
            C2s::AimDirectionOverride(d) => {
                self.players.get_mut(&uuid).unwrap().aim_dir_override = Some(d)
            }
            C2s::AimPositionOverride(rel) => {
                // Relative to the player; the host adds the player's position.
                // Stored relative here: `aim_pos` callers add `feet`.
                self.players.get_mut(&uuid).unwrap().aim_pos_override = Some(rel.as_dvec3());
            }
            C2s::HeadData { seated, hmd } => {
                let l = self.legacy.entry(uuid).or_default();
                l.head = Some((seated, hmd));
                self.assemble_legacy(uuid);
            }
            C2s::Controller0Data {
                left_handed,
                main_hand,
            } => {
                let l = self.legacy.entry(uuid).or_default();
                l.c0 = Some((left_handed, main_hand));
                self.assemble_legacy(uuid);
            }
            C2s::Controller1Data {
                left_handed,
                off_hand,
            } => {
                let l = self.legacy.entry(uuid).or_default();
                l.c1 = Some((left_handed, off_hand));
                self.assemble_legacy(uuid);
            }
        }
        out
    }

    fn assemble_legacy(&mut self, uuid: u128) {
        let Some(l) = self.legacy.get(&uuid) else {
            return;
        };
        if let (Some((seated, hmd)), Some((lh0, main)), Some((lh1, off))) = (l.head, l.c0, l.c1) {
            let state = VrPlayerState {
                seated,
                hmd,
                left_handed: lh0,
                main_hand: main,
                reverse_hands_legacy: lh1,
                off_hand: off,
                fbt_mode: FbtMode::ArmsOnly,
                ..VrPlayerState::default()
            };
            if let Some(p) = self.players.get_mut(&uuid) {
                p.set_vr_player_state(state);
            }
            self.legacy.remove(&uuid);
        }
    }

    /// Once per tick per player (`ServerPlayerMixin.doTick`): aim override timeouts.
    pub fn tick_player(&mut self, uuid: u128) {
        if let Some(p) = self.players.get_mut(&uuid) {
            p.tick();
        }
    }

    /// `sendVrPlayerStateToClients`: the payload for each player that tracks `uuid`:
    /// `(receiver, payload)`. The legacy variant is chosen by the sender's network
    /// version, as Vivecraft does. `tracking` are the players that see `uuid`;
    /// `uuid` itself gets one only if the config says so.
    pub fn state_updates(&self, uuid: u128, tracking: &[u128]) -> Vec<(u128, S2c)> {
        let Some(p) = self.players.get(&uuid) else {
            return Vec::new();
        };
        let Some(state) = p.vr_player_state() else {
            return Vec::new();
        };
        if !p.is_vr() {
            return Vec::new();
        }
        let payload = if p.network_version == NetworkVersion::Legacy {
            S2c::UberPacket {
                player: uuid,
                state: state.stripped_for(NetworkVersion::Legacy),
                world_scale: p.world_scale,
                height_scale: p.height_scale,
            }
        } else {
            S2c::UberPacket {
                player: uuid,
                state: state.clone(),
                world_scale: p.world_scale,
                height_scale: p.height_scale,
            }
        };
        let mut out: Vec<(u128, S2c)> = tracking
            .iter()
            .copied()
            .filter(|t| *t != uuid && self.players.contains_key(t))
            .map(|t| (t, payload.clone()))
            .collect();
        if self.config.send_data_to_owner {
            out.push((uuid, payload));
        }
        out
    }

    /// `ServerUtil.kickIfNotAllowed` for the VR only and Vivecraft only settings; the
    /// minimum version check is not ported (no Vivecraft version parser here).
    pub fn kick_reason(&self, uuid: u128, is_op: bool, name: &str) -> Option<String> {
        let allowed = self.config.allow_op && is_op;
        let p = self.players.get(&uuid);
        if !allowed && self.config.vr_only && p.is_none_or(|p| !p.is_vr()) {
            return Some(format_message(
                "This server is configured for VR players only.",
                name,
                &[],
            ));
        }
        if !allowed && self.config.vive_only && p.is_none() {
            return Some(format_message(
                "This server is configured for Vivecraft players only.",
                name,
                &[],
            ));
        }
        None
    }
}

/// Java's `String.valueOf(double)` for the values the world scale override sends
/// (`1.0`, `0.5`, `100.0`): a decimal point is always there.
fn java_double(v: f64) -> String {
    let s = format!("{v}");
    if s.contains('.') || s.contains('e') || s.contains("inf") || s.contains("NaN") {
        s
    } else {
        format!("{s}.0")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::Pose;
    use glam::Quat;

    const A: u128 = 1;
    const B: u128 = 2;

    fn join(server: &mut VrServer, uuid: u128, max: i32) -> Output {
        server.handle(
            uuid,
            C2s::Version {
                version: "Vivecraft-26.2-fabric-1.3.15".into(),
                vr: true,
                max_version: max,
                min_version: 0,
                legacy: false,
            },
        )
    }

    fn ids(out: &Output) -> Vec<u8> {
        out.replies.iter().map(|r| r.id().unwrap() as u8).collect()
    }

    #[test]
    fn a_new_client_gets_vivecraft_servers_default_replies_in_order() {
        let mut s = VrServer::default();
        let out = join(&mut s, A, 9);
        // VERSION, REQUESTDATA, CLIMBING, TELEPORT, CRAWL, VR_SWITCHING, DUAL_WIELDING,
        // ATTACK_WHILE_BLOCKING, NETWORK_VERSION: no vrChanges, no overrides by default.
        assert_eq!(ids(&out), vec![0, 1, 10, 9, 14, 16, 19, 23, 15]);
        assert_eq!(
            out.replies[8],
            S2c::NetworkVersion(NetworkVersion::ClimbeyJump)
        );
        assert!(s.is_vr_player(A));
    }

    #[test]
    fn the_server_uses_the_lower_of_the_two_versions() {
        let mut s = VrServer::default();
        let out = join(&mut s, A, 3);
        assert_eq!(
            *out.replies.last().unwrap(),
            S2c::NetworkVersion(NetworkVersion::from_protocol_version(3).unwrap())
        );
        // DUAL_WIELDING is protocol 2: accepted at 3, but no OPTION_TOGGLE flag.
        assert!(ids(&out).contains(&19));
        assert!(!ids(&out).contains(&23));
        assert!(
            matches!(out.replies[3], S2c::Teleport { target, .. } if target.protocol_version() == 3)
        );
    }

    #[test]
    fn a_client_with_no_overlap_gets_a_message_and_no_state() {
        let mut s = VrServer::default();
        let out = s.handle(
            A,
            C2s::Version {
                version: "v".into(),
                vr: true,
                max_version: -1,
                min_version: -1,
                legacy: false,
            },
        );
        assert!(out.replies.is_empty());
        assert!(matches!(out.actions[0], Action::Message(_)));
        assert!(!s.players.contains_key(&A));
    }

    #[test]
    fn packets_before_version_are_dropped() {
        let mut s = VrServer::default();
        let out = s.handle(A, C2s::Draw(1.0));
        assert_eq!(out, Output::default());
        assert!(s.vive_player(A).is_none());
    }

    #[test]
    fn config_gates_the_actions() {
        let mut s = VrServer::default();
        join(&mut s, A, 9);
        let out = s.handle(
            A,
            C2s::Teleport {
                x: 1.0,
                y: 2.0,
                z: 3.0,
            },
        );
        assert_eq!(
            out.actions,
            vec![Action::Teleport {
                x: 1.0,
                y: 2.0,
                z: 3.0
            }]
        );
        s.config.teleport_enabled = false;
        assert!(
            s.handle(
                A,
                C2s::Teleport {
                    x: 1.0,
                    y: 2.0,
                    z: 3.0
                }
            )
            .actions
            .is_empty()
        );
        assert_eq!(
            s.handle(A, C2s::Jumping).actions,
            vec![Action::FoodExhaustion(0.3)]
        );
        s.config.crawling_enabled = false;
        s.handle(A, C2s::Crawl(true));
        assert!(!s.vive_player(A).unwrap().crawling);
    }

    #[test]
    fn world_scale_limits_are_sent_as_java_doubles() {
        let mut s = VrServer::default();
        s.config.worldscale_limited = true;
        let out = join(&mut s, A, 9);
        let ov = out.replies.iter().find_map(|r| match r {
            S2c::SettingOverride { overrides, clear } => Some((overrides.clone(), *clear)),
            _ => None,
        });
        let (ov, clear) = ov.unwrap();
        assert!(!clear);
        assert_eq!(ov["worldScale.min"], "0.5");
        assert_eq!(ov["worldScale.max"], "2.0");
    }

    #[test]
    fn legacy_clients_assemble_a_state_from_three_packets() {
        let mut s = VrServer::default();
        s.handle(
            A,
            C2s::Version {
                version: "old".into(),
                vr: true,
                max_version: -1,
                min_version: -1,
                legacy: true,
            },
        );
        let pose = Pose::new(Vec3::new(0.0, 1.6, 0.0), Quat::IDENTITY);
        s.handle(
            A,
            C2s::HeadData {
                seated: false,
                hmd: pose,
            },
        );
        s.handle(
            A,
            C2s::Controller0Data {
                left_handed: false,
                main_hand: pose,
            },
        );
        assert!(s.vive_player(A).unwrap().vr_player_state().is_none());
        s.handle(
            A,
            C2s::Controller1Data {
                left_handed: false,
                off_hand: pose,
            },
        );
        assert!(s.vive_player(A).unwrap().vr_player_state().is_some());
    }

    #[test]
    fn states_go_to_tracking_vivecraft_players_only() {
        let mut s = VrServer::default();
        join(&mut s, A, 9);
        join(&mut s, B, 9);
        let pose = Pose::new(Vec3::new(0.0, 1.6, 0.0), Quat::IDENTITY);
        s.handle(
            A,
            C2s::VrPlayerState(VrPlayerState {
                hmd: pose,
                main_hand: pose,
                off_hand: pose,
                ..Default::default()
            }),
        );
        let sent = s.state_updates(A, &[A, B, 99]);
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].0, B);
        // VR off: nothing, and the others hear about it.
        let out = s.handle(A, C2s::IsVrActive(false));
        assert!(out.vr_off_broadcast);
        assert!(s.state_updates(A, &[B]).is_empty());
    }

    #[test]
    fn active_hand_swaps_the_item_only_with_dual_wielding() {
        let mut s = VrServer::default();
        join(&mut s, A, 9);
        let out = s.handle(
            A,
            C2s::ActiveHand {
                body_part: VrBodyPart::OffHand,
                use_for_aim: true,
            },
        );
        assert_eq!(
            out.actions,
            vec![Action::SwapActiveItem {
                from: VrBodyPart::MainHand,
                to: VrBodyPart::OffHand
            }]
        );
        s.config.dual_wielding = false;
        let out = s.handle(
            A,
            C2s::ActiveHand {
                body_part: VrBodyPart::MainHand,
                use_for_aim: false,
            },
        );
        assert!(out.actions.is_empty());
        assert_eq!(
            s.vive_player(A).unwrap().active_body_part,
            VrBodyPart::MainHand
        );
    }

    #[test]
    fn aim_follows_the_hand_and_the_bow_draws_between_hands() {
        let mut s = VrServer::default();
        join(&mut s, A, 9);
        let main = Pose::new(
            Vec3::new(0.0, 1.0, -0.5),
            Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
        );
        let off = Pose::new(Vec3::new(0.0, 1.0, 0.0), Quat::IDENTITY);
        s.handle(
            A,
            C2s::VrPlayerState(VrPlayerState {
                hmd: off,
                main_hand: main,
                off_hand: off,
                ..Default::default()
            }),
        );
        let p = s.players.get_mut(&A).unwrap();
        let feet = DVec3::new(10.0, 64.0, 10.0);
        let look = Vec3::Z;
        // A quarter turn left points the hand along -X.
        assert!((p.aim_dir(false, feet, look) - Vec3::new(-1.0, 0.0, 0.0)).length() < 1e-5);
        assert_eq!(p.aim_pos(false, feet, look), DVec3::new(10.0, 65.0, 9.5));
        // Drawing: from the active hand to the other one.
        p.draw = 0.5;
        let d = p.aim_dir(false, feet, look);
        assert!((d - Vec3::new(0.0, 0.0, 1.0)).length() < 1e-5, "{d}");
        // An override wins, and times out.
        p.aim_dir_override = Some(Vec3::Y);
        p.aim_reset = 2;
        p.tick();
        assert_eq!(p.aim_dir(false, feet, look), Vec3::Y);
        p.tick();
        assert!(p.aim_dir_override.is_none());
    }

    #[test]
    fn without_a_state_the_player_looks_where_they_look() {
        let p = ServerVivePlayer::default();
        let look = Vec3::new(0.0, 0.0, 1.0);
        assert_eq!(p.aim_dir(false, DVec3::ZERO, look), look);
        assert_eq!(p.hmd_pos(DVec3::new(0.0, 64.0, 0.0)).y, 65.62);
    }

    #[test]
    fn vr_only_kicks_non_vr_unless_op() {
        let mut s = VrServer::default();
        s.config.vr_only = true;
        assert!(s.kick_reason(A, false, "x").is_some());
        assert!(s.kick_reason(A, true, "x").is_none());
        join(&mut s, A, 9);
        assert!(s.kick_reason(A, false, "x").is_none());
        // VR switching is off when the server is VR only.
        assert_eq!(s.vr_switching_payload(), S2c::VrSwitching(false));
    }

    #[test]
    fn arrows_do_more_damage_from_a_standing_vr_bow_and_to_the_head() {
        let c = ServerConfig::default();
        assert_eq!(arrow_multiplier(&c, Some(false), false), 2.0);
        assert_eq!(arrow_multiplier(&c, Some(true), false), 1.0);
        assert_eq!(arrow_multiplier(&c, Some(false), true), 3.0);
        assert_eq!(arrow_multiplier(&c, Some(true), true), 2.0);
        assert_eq!(arrow_multiplier(&c, None, true), 1.0);
        assert_eq!(arrow_multiplier(&c, None, false), 1.0);
    }

    #[test]
    fn a_zombie_has_its_head_at_the_eyes_and_a_cow_in_front() {
        let feet = DVec3::new(0.0, 64.0, 0.0);
        assert_eq!(head_kind("minecraft:zombie", false), HeadKind::Eyes);
        assert_eq!(head_kind("minecraft:player", true), HeadKind::Front);
        assert_eq!(head_kind("minecraft:ender_dragon", false), HeadKind::None);
        let (lo, hi) = head_hitbox(HeadKind::Eyes, feet, 1.74, 0.6, false, 0.0, 0.3).unwrap();
        // Eyes at 65.74, half width 0.3, inflated 0.3 each way.
        assert!((lo.y - (65.74 - 0.3 + 0.3 - 0.3)).abs() < 1e-9, "{lo}");
        assert!((hi.y - (65.74 + 0.3 + 0.3 + 0.3)).abs() < 1e-9, "{hi}");
        assert!((hi.x - 0.6).abs() < 1e-9);
        // A cow facing south (yaw 0) carries its head 0.45 ahead of its eyes.
        let (lo, hi) = head_hitbox(HeadKind::Front, feet, 1.3, 0.9, false, 0.0, 0.3).unwrap();
        let centre = (lo + hi) * 0.5;
        assert!(
            (centre.z - 0.45).abs() < 1e-6 && (centre.y - 65.3).abs() < 1e-6,
            "{centre}"
        );
    }

    #[test]
    fn an_arrow_into_the_face_is_a_headshot_one_into_the_legs_is_not() {
        let body = (DVec3::new(-0.3, 64.0, -0.3), DVec3::new(0.3, 65.95, 0.3));
        let head = head_hitbox(
            HeadKind::Eyes,
            DVec3::new(0.0, 64.0, 0.0),
            1.74,
            0.6,
            false,
            0.0,
            0.3,
        )
        .unwrap();
        let velocity = DVec3::new(0.0, 0.0, -3.0);
        assert!(is_headshot(
            DVec3::new(0.0, 65.7, 5.0),
            velocity,
            head,
            body
        ));
        assert!(!is_headshot(
            DVec3::new(0.0, 64.3, 5.0),
            velocity,
            head,
            body
        ));
    }
}
