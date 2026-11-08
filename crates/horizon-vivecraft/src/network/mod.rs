// SPDX-License-Identifier: LGPL-3.0-or-later
//! `org.vivecraft.common.network`: the `vivecraft:data` channel.
//! Copyright (c) Vivecraft contributors; LGPL-3.0-or-later (see LICENSE-LGPL-3.0).

mod body;
mod c2s;
mod pose;
mod s2c;
mod version;

pub use body::{FbtMode, VrBodyPart};
pub use c2s::C2s;
pub use pose::{Pose, TrackerPose, VrPlayerState};
pub use s2c::{ClimbeyBlockmode, S2c};
pub use version::NetworkVersion;

/// `CommonNetworkHelper.CHANNEL`.
pub const CHANNEL: &str = "vivecraft:data";
/// The channel plugin-message servers (Paper, Spigot, Fabric API, NeoForge) learn the
/// client's channels from: a NUL separated list.
pub const REGISTER_CHANNEL: &str = "minecraft:register";

/// `CommonNetworkHelper.MAX_SUPPORTED_NETWORK_PROTOCOL`.
pub const MAX_SUPPORTED_NETWORK_PROTOCOL: i32 = NetworkVersion::ClimbeyJump as i32 - 1;
/// `CommonNetworkHelper.MIN_SUPPORTED_NETWORK_PROTOCOL`.
pub const MIN_SUPPORTED_NETWORK_PROTOCOL: i32 = 0;

/// The `minecraft:register` payload that announces [`CHANNEL`] to a server.
pub fn register_payload() -> Vec<u8> {
    CHANNEL.as_bytes().to_vec()
}

/// `PayloadIdentifier`, in Vivecraft's declaration order (the ordinal is the wire id).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum PayloadId {
    Version,
    RequestData,
    HeadData,
    Controller0Data,
    Controller1Data,
    WorldScale,
    Draw,
    MoveMode,
    UberPacket,
    Teleport,
    Climbing,
    SettingOverride,
    Height,
    ActiveHand,
    Crawl,
    NetworkVersion,
    VrSwitching,
    IsVrActive,
    VrPlayerState,
    DualWielding,
    Haptic,
    ServerVrChanges,
    DamageDirection,
    AttackWhileBlocking,
    AimDirectionOverride,
    AimOverrideReset,
    AimPositionOverride,
    Jumping,
}

impl PayloadId {
    pub const ALL: [PayloadId; 28] = [
        PayloadId::Version,
        PayloadId::RequestData,
        PayloadId::HeadData,
        PayloadId::Controller0Data,
        PayloadId::Controller1Data,
        PayloadId::WorldScale,
        PayloadId::Draw,
        PayloadId::MoveMode,
        PayloadId::UberPacket,
        PayloadId::Teleport,
        PayloadId::Climbing,
        PayloadId::SettingOverride,
        PayloadId::Height,
        PayloadId::ActiveHand,
        PayloadId::Crawl,
        PayloadId::NetworkVersion,
        PayloadId::VrSwitching,
        PayloadId::IsVrActive,
        PayloadId::VrPlayerState,
        PayloadId::DualWielding,
        PayloadId::Haptic,
        PayloadId::ServerVrChanges,
        PayloadId::DamageDirection,
        PayloadId::AttackWhileBlocking,
        PayloadId::AimDirectionOverride,
        PayloadId::AimOverrideReset,
        PayloadId::AimPositionOverride,
        PayloadId::Jumping,
    ];

    /// The id for a wire byte read as a signed `readByte`, `None` when unknown.
    pub fn from_wire(b: i8) -> Option<PayloadId> {
        usize::try_from(b)
            .ok()
            .and_then(|i| Self::ALL.get(i).copied())
    }
}
