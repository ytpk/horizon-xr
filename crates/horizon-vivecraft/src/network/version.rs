// SPDX-License-Identifier: LGPL-3.0-or-later
//! Ports `org.vivecraft.common.network.NetworkVersion`.
//! Copyright (c) Vivecraft contributors; LGPL-3.0-or-later (see LICENSE-LGPL-3.0).

/// The protocol versions; the protocol number is the ordinal minus one (legacy is -1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum NetworkVersion {
    /// Clients that used the old installer.
    Legacy,
    /// The client sends its data in a single packet and switches back to non-VR.
    NewNetworking,
    /// Full body tracker data.
    Fbt,
    /// The dual wielding packet and server logic.
    DualWielding,
    /// The head as a valid active body part, and a `useForAim` flag.
    HeadAim,
    /// Haptic events to the client.
    HapticPacket,
    /// The client learns which VR changes are on non default values.
    ServerVrChanges,
    /// Damage directions.
    DamageDirection,
    /// Settings that toggle after the initial connection.
    OptionToggle,
    /// Overrides of the aim direction and position.
    AimOverride,
    /// The client tells the server about a climbey jump.
    ClimbeyJump,
}

impl NetworkVersion {
    pub const ALL: [NetworkVersion; 11] = [
        Self::Legacy,
        Self::NewNetworking,
        Self::Fbt,
        Self::DualWielding,
        Self::HeadAim,
        Self::HapticPacket,
        Self::ServerVrChanges,
        Self::DamageDirection,
        Self::OptionToggle,
        Self::AimOverride,
        Self::ClimbeyJump,
    ];

    /// `fromProtocolVersion`: `None` for a number no version has.
    pub fn from_protocol_version(protocol: i32) -> Option<Self> {
        usize::try_from(protocol + 1)
            .ok()
            .and_then(|i| Self::ALL.get(i).copied())
    }

    /// `protocolVersion`: what travels between server and client.
    pub fn protocol_version(self) -> i32 {
        self as i32 - 1
    }

    /// `accepts`: `other` supports the features of `self`.
    pub fn accepts(self, other: NetworkVersion) -> bool {
        self <= other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_numbers_start_at_minus_one() {
        assert_eq!(NetworkVersion::Legacy.protocol_version(), -1);
        assert_eq!(NetworkVersion::NewNetworking.protocol_version(), 0);
        assert_eq!(NetworkVersion::ClimbeyJump.protocol_version(), 9);
        assert_eq!(crate::network::MAX_SUPPORTED_NETWORK_PROTOCOL, 9);
        assert_eq!(
            NetworkVersion::from_protocol_version(4),
            Some(NetworkVersion::HapticPacket)
        );
        assert_eq!(NetworkVersion::from_protocol_version(10), None);
        assert_eq!(NetworkVersion::from_protocol_version(-2), None);
    }

    #[test]
    fn accepts_is_an_ordinal_comparison() {
        assert!(NetworkVersion::Fbt.accepts(NetworkVersion::AimOverride));
        assert!(!NetworkVersion::AimOverride.accepts(NetworkVersion::Fbt));
    }
}
