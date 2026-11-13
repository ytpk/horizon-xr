// SPDX-License-Identifier: LGPL-3.0-or-later
//! Horizon's port of Vivecraft (https://github.com/Vivecraft/VivecraftMod, commit
//! 8b4d6b95a1891d9839f19b45c13e93415c02a31e, mod 1.3.15, Minecraft 26.2).
//!
//! Copyright (c) Vivecraft contributors. This crate is a translation of parts of Vivecraft
//! to Rust and is licensed under the GNU Lesser General Public License, version 3 or (at
//! your option) any later version (LICENSE-LGPL-3.0). The rest of Horizon is MIT.
//!
//! - [`network`]: the `vivecraft:data` protocol, byte for byte (`org.vivecraft.common.network`).
//! - [`server`]: the Vivecraft server half (`org.vivecraft.server`): players, config, handshake.
//! - [`client`]: the client half of the networking (`org.vivecraft.client.network.ClientNetworking`).
//! - [`math`]: pose math shared by both (`org.vivecraft.common.utils.MathUtils`).
//! - [`settings`]: `VRSettings`: option names, defaults, ranges.

pub mod client;
pub mod crosshair;
pub mod gui;
pub mod keyboard;
pub mod math;
pub mod modes;
pub mod network;
pub mod options;
pub mod overrides;
pub mod pages;
pub mod player;
pub mod player_model;
pub mod radial;
pub mod refresh;
pub mod server;
pub mod settings;
pub mod trackers;
pub mod vr_data;
pub mod wire;

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

pub type Result<T> = std::result::Result<T, Error>;

/// Vivecraft's version, as `gradle.properties` says (`mod_version`).
pub const VIVECRAFT_VERSION: &str = "1.3.15";
/// The Minecraft version Vivecraft ported here targets.
pub const VIVECRAFT_MC_VERSION: &str = "26.2";
/// `CommonDataHolder.versionIdentifier`: `Vivecraft-<mc>-<loader>-<mod>`. Horizon
/// identifies as a Fabric build, the loader every Vivecraft server accepts.
pub fn version_identifier() -> String {
    format!("Vivecraft-{VIVECRAFT_MC_VERSION}-fabric-{VIVECRAFT_VERSION}")
}
