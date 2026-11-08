// SPDX-License-Identifier: MPL-2.0
//! Horizon's port of Visor (https://github.com/VisorModStudio/Visor, commit
//! c01354fad8ac03a016c249239a04b07adeb36476, Visor 0.6.1) and of the server half of
//! VisorPlugin (https://github.com/VisorModStudio/VisorPlugin, commit
//! b4f3fc8be0e67f517c3e98c07ac1b5510eaf3d24, 0.2.0alpha).
//!
//! Copyright (C) 2025-2026 Phoenix-Ra and Visor contributors. This crate is a translation of
//! parts of Visor to Rust and is licensed under the Mozilla Public License 2.0
//! (LICENSE-MPL-2.0). The rest of Horizon is MIT.
//!
//! - [`payload`]: the `visor:channel` protocol, version 5, byte for byte
//!   (`org.vmstudio.visor.api.common.network`).
//! - [`buffer`]: the pose buffers (`PoseDataBuffer` and friends) with their bit masks.
//! - [`settings`]: `VRServerSettings` and the `SERVER_SETTINGS` text.
//! - [`client`]: the client half of the networking (`ClientNetworking`).
//! - [`server`]: the server half (`ServerPacketHandler`, `ServerNetworking`), as the mod and
//!   VisorPlugin both do it.
//! - [`body`]: `VRBodyType` ids and the height maths of the local player.

#![cfg_attr(test, allow(clippy::field_reassign_with_default))]

pub mod body;
pub mod body_rig;
pub mod buffer;
pub mod client;
pub mod height;
pub mod item_pose;
pub mod keyboard;
pub mod overlay;
pub mod payload;
pub mod presets;
pub mod server;
pub mod settings;
pub mod swing_effects;
pub mod tasks;

use std::fmt;

/// A payload that does not decode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);

impl Error {
    pub fn new(s: &str) -> Self {
        Error(s.to_string())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

impl From<horizon_vivecraft::Error> for Error {
    fn from(e: horizon_vivecraft::Error) -> Self {
        Error(e.0)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// `VisorNetwork.CORE_CHANNEL_ID`.
pub const CHANNEL: &str = "visor:channel";
/// `VisorNetwork.CORE_NETWORK_VERSION`: "5: since Visor 0.5.0". A server that gets another
/// number in the HANDSHAKE disconnects the player.
pub const NETWORK_VERSION: i32 = 5;
/// `gradle.properties` `mod_version` of the ported commit: the string the HANDSHAKE carries
/// (`ModLoader.getModVersion(VisorAPI.MOD_ID)`); servers only log it.
pub const VISOR_VERSION: &str = "0.6.1";
/// The channel plugin-message servers (Paper, Spigot) learn the client's channels from.
pub const REGISTER_CHANNEL: &str = "minecraft:register";

/// The `minecraft:register` payload that announces [`CHANNEL`] to a server.
pub fn register_payload() -> Vec<u8> {
    CHANNEL.as_bytes().to_vec()
}
