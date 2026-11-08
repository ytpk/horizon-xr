// SPDX-License-Identifier: LGPL-3.0-or-later
//! Ports `org.vivecraft.common.network.packet.s2c.*` and `VivecraftPayloadS2C.readPacket`.
//! Copyright (c) Vivecraft contributors; LGPL-3.0-or-later (see LICENSE-LGPL-3.0).

use super::pose::{read_vec3, write_vec3};
use super::{NetworkVersion, PayloadId, VrBodyPart, VrPlayerState};
use crate::wire::{Reader, Writer};
use crate::{Error, Result};
use glam::Vec3;
use std::collections::BTreeMap;

/// `ClimbeyBlockmode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum ClimbeyBlockmode {
    #[default]
    Disabled,
    Whitelist,
    Blacklist,
}

impl ClimbeyBlockmode {
    pub fn from_wire(b: i8) -> Option<Self> {
        match b {
            0 => Some(Self::Disabled),
            1 => Some(Self::Whitelist),
            2 => Some(Self::Blacklist),
            _ => None,
        }
    }
}

/// A server to client payload.
#[derive(Clone, Debug, PartialEq)]
pub enum S2c {
    Version(String),
    RequestData,
    /// Another player's pose, scale and height.
    UberPacket {
        player: u128,
        state: VrPlayerState,
        world_scale: f32,
        height_scale: f32,
    },
    /// `allowed` is only on the wire from OPTION_TOGGLE on; an old server sends none and
    /// always allowed.
    Teleport {
        allowed: bool,
        target: NetworkVersion,
    },
    Climbing {
        allowed: bool,
        blockmode: ClimbeyBlockmode,
        blocks: Option<Vec<String>>,
    },
    SettingOverride {
        overrides: BTreeMap<String, String>,
        clear: bool,
    },
    Crawl {
        allowed: bool,
        target: NetworkVersion,
    },
    NetworkVersion(NetworkVersion),
    VrSwitching(bool),
    IsVrActive {
        vr: bool,
        player: u128,
    },
    DualWielding(bool),
    Haptic {
        body_part: VrBodyPart,
        duration: f32,
        frequency: f32,
        amplitude: f32,
        delay: f32,
    },
    ServerVrChanges(BTreeMap<String, String>),
    DamageDirection(Vec3),
    AttackWhileBlocking(bool),
    Unknown,
}

impl S2c {
    pub fn id(&self) -> Option<PayloadId> {
        Some(match self {
            S2c::Version(_) => PayloadId::Version,
            S2c::RequestData => PayloadId::RequestData,
            S2c::UberPacket { .. } => PayloadId::UberPacket,
            S2c::Teleport { .. } => PayloadId::Teleport,
            S2c::Climbing { .. } => PayloadId::Climbing,
            S2c::SettingOverride { .. } => PayloadId::SettingOverride,
            S2c::Crawl { .. } => PayloadId::Crawl,
            S2c::NetworkVersion(_) => PayloadId::NetworkVersion,
            S2c::VrSwitching(_) => PayloadId::VrSwitching,
            S2c::IsVrActive { .. } => PayloadId::IsVrActive,
            S2c::DualWielding(_) => PayloadId::DualWielding,
            S2c::Haptic { .. } => PayloadId::Haptic,
            S2c::ServerVrChanges(_) => PayloadId::ServerVrChanges,
            S2c::DamageDirection(_) => PayloadId::DamageDirection,
            S2c::AttackWhileBlocking(_) => PayloadId::AttackWhileBlocking,
            S2c::Unknown => return None,
        })
    }

    pub fn write(&self) -> Vec<u8> {
        let mut w = Writer::new();
        let Some(id) = self.id() else {
            return w.data;
        };
        w.byte(id as u8);
        match self {
            S2c::Version(v) => w.utf(v),
            S2c::RequestData | S2c::Unknown => {}
            S2c::UberPacket {
                player,
                state,
                world_scale,
                height_scale,
            } => {
                w.uuid(*player);
                state.serialize(&mut w);
                w.float(*world_scale);
                w.float(*height_scale);
            }
            S2c::Teleport { allowed, target } | S2c::Crawl { allowed, target } => {
                // Old clients don't expect additional data.
                if NetworkVersion::OptionToggle.accepts(*target) {
                    w.bool(*allowed);
                }
            }
            S2c::Climbing {
                allowed,
                blockmode,
                blocks,
            } => {
                w.bool(*allowed);
                w.byte(*blockmode as u8);
                if let Some(blocks) = blocks {
                    for b in blocks {
                        w.utf(b);
                    }
                }
            }
            S2c::SettingOverride { overrides, clear } => {
                for (k, v) in overrides {
                    w.utf(k);
                    w.utf(v);
                }
                w.utf("clearOverrides");
                w.utf(if *clear { "true" } else { "false" });
            }
            S2c::NetworkVersion(v) => w.byte(v.protocol_version() as u8),
            S2c::VrSwitching(b) | S2c::DualWielding(b) | S2c::AttackWhileBlocking(b) => w.bool(*b),
            S2c::IsVrActive { vr, player } => {
                w.bool(*vr);
                w.uuid(*player);
            }
            S2c::Haptic {
                body_part,
                duration,
                frequency,
                amplitude,
                delay,
            } => {
                w.byte(*body_part as u8);
                w.float(*duration);
                w.float(*frequency);
                w.float(*amplitude);
                w.float(*delay);
            }
            S2c::ServerVrChanges(changes) => {
                for (k, v) in changes {
                    w.utf(k);
                    w.utf(v);
                }
            }
            S2c::DamageDirection(v) => write_vec3(&mut w, *v),
        }
        w.data
    }

    /// `VivecraftPayloadS2C.readPacket`.
    pub fn read(data: &[u8]) -> Result<Self> {
        let mut r = Reader::new(data);
        let Some(id) = PayloadId::from_wire(r.byte()?) else {
            return Ok(S2c::Unknown);
        };
        let map = |r: &mut Reader| -> Result<BTreeMap<String, String>> {
            let mut m = BTreeMap::new();
            while r.remaining() > 0 {
                let k = r.utf()?;
                let v = r.utf()?;
                m.insert(k, v);
            }
            Ok(m)
        };
        Ok(match id {
            PayloadId::Version => S2c::Version(r.utf()?),
            PayloadId::RequestData => S2c::RequestData,
            PayloadId::UberPacket => S2c::UberPacket {
                player: r.uuid()?,
                state: VrPlayerState::deserialize(&mut r, 8)?,
                world_scale: r.float()?,
                height_scale: r.float()?,
            },
            PayloadId::Teleport => {
                if r.remaining() > 0 {
                    S2c::Teleport {
                        allowed: r.bool()?,
                        target: NetworkVersion::OptionToggle,
                    }
                } else {
                    S2c::Teleport {
                        allowed: true,
                        target: NetworkVersion::Legacy,
                    }
                }
            }
            PayloadId::Climbing => {
                let allowed = r.bool()?;
                // Very old plugins sent no block list.
                if r.remaining() > 0 {
                    let blockmode = ClimbeyBlockmode::from_wire(r.byte()?)
                        .ok_or_else(|| Error::new("vivecraft payload: bad blockmode"))?;
                    let mut blocks = Vec::new();
                    while r.remaining() > 0 {
                        blocks.push(r.utf()?);
                    }
                    S2c::Climbing {
                        allowed,
                        blockmode,
                        blocks: Some(blocks),
                    }
                } else {
                    S2c::Climbing {
                        allowed,
                        blockmode: ClimbeyBlockmode::Disabled,
                        blocks: None,
                    }
                }
            }
            PayloadId::SettingOverride => {
                let overrides = map(&mut r)?;
                let clear = overrides.get("clearOverrides").is_some_and(|v| v == "true");
                S2c::SettingOverride { overrides, clear }
            }
            PayloadId::Crawl => {
                if r.remaining() > 0 {
                    S2c::Crawl {
                        allowed: r.bool()?,
                        target: NetworkVersion::OptionToggle,
                    }
                } else {
                    S2c::Crawl {
                        allowed: true,
                        target: NetworkVersion::Legacy,
                    }
                }
            }
            PayloadId::NetworkVersion => {
                let protocol = i32::from(r.byte()? as u8);
                S2c::NetworkVersion(
                    NetworkVersion::from_protocol_version(protocol)
                        .ok_or_else(|| Error::new("vivecraft payload: bad network version"))?,
                )
            }
            PayloadId::VrSwitching => S2c::VrSwitching(r.bool()?),
            PayloadId::IsVrActive => S2c::IsVrActive {
                vr: r.bool()?,
                player: r.uuid()?,
            },
            PayloadId::DualWielding => S2c::DualWielding(r.bool()?),
            PayloadId::Haptic => S2c::Haptic {
                body_part: VrBodyPart::from_wire(r.byte()?)
                    .ok_or_else(|| Error::new("vivecraft payload: bad body part"))?,
                duration: r.float()?,
                frequency: r.float()?,
                amplitude: r.float()?,
                delay: r.float()?,
            },
            PayloadId::ServerVrChanges => S2c::ServerVrChanges(map(&mut r)?),
            PayloadId::DamageDirection => S2c::DamageDirection(read_vec3(&mut r)?),
            PayloadId::AttackWhileBlocking => S2c::AttackWhileBlocking(r.bool()?),
            _ => S2c::Unknown,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::Pose;

    #[test]
    fn version_uses_a_length_prefixed_string() {
        let bytes = S2c::Version("Vivecraft-26.2-fabric-1.3.15".into()).write();
        assert_eq!(bytes[0], 0);
        assert_eq!(bytes[1], 28);
        assert_eq!(
            S2c::read(&bytes).unwrap(),
            S2c::Version("Vivecraft-26.2-fabric-1.3.15".into())
        );
    }

    #[test]
    fn network_version_is_the_protocol_number() {
        assert_eq!(
            S2c::NetworkVersion(NetworkVersion::ClimbeyJump).write(),
            vec![15, 9]
        );
        assert_eq!(
            S2c::read(&[15, 9]).unwrap(),
            S2c::NetworkVersion(NetworkVersion::ClimbeyJump)
        );
        // A legacy number (-1 as 0xFF) is outside what the client can name.
        assert!(S2c::read(&[15, 0xff]).is_err());
    }

    #[test]
    fn teleport_and_crawl_gain_a_flag_with_option_toggle() {
        let new = S2c::Teleport {
            allowed: false,
            target: NetworkVersion::OptionToggle,
        };
        assert_eq!(new.write(), vec![9, 0]);
        assert_eq!(S2c::read(&[9, 0]).unwrap(), new);
        let old = S2c::Teleport {
            allowed: true,
            target: NetworkVersion::HeadAim,
        };
        assert_eq!(old.write(), vec![9]);
        assert_eq!(
            S2c::read(&[9]).unwrap(),
            S2c::Teleport {
                allowed: true,
                target: NetworkVersion::Legacy
            }
        );
        assert_eq!(
            S2c::Crawl {
                allowed: true,
                target: NetworkVersion::OptionToggle
            }
            .write(),
            vec![14, 1]
        );
    }

    #[test]
    fn climbing_lists_blocks_as_strings_to_the_end() {
        let p = S2c::Climbing {
            allowed: true,
            blockmode: ClimbeyBlockmode::Whitelist,
            blocks: Some(vec!["white_wool".into(), "dirt".into()]),
        };
        let bytes = p.write();
        assert_eq!(&bytes[..3], &[10, 1, 1]);
        assert_eq!(bytes[3], 10);
        assert_eq!(S2c::read(&bytes).unwrap(), p);
        assert_eq!(
            S2c::read(&[10, 1]).unwrap(),
            S2c::Climbing {
                allowed: true,
                blockmode: ClimbeyBlockmode::Disabled,
                blocks: None
            }
        );
        // No blocks sent at all after the blockmode.
        assert_eq!(
            S2c::read(&[10, 1, 0]).unwrap(),
            S2c::Climbing {
                allowed: true,
                blockmode: ClimbeyBlockmode::Disabled,
                blocks: Some(vec![])
            }
        );
    }

    #[test]
    fn setting_override_ends_with_clear_overrides() {
        let mut overrides = BTreeMap::new();
        overrides.insert("limitedTeleport".to_string(), "true".to_string());
        let p = S2c::SettingOverride {
            overrides,
            clear: false,
        };
        let bytes = p.write();
        let tail = [&[14u8][..], b"clearOverrides", &[5u8][..], b"false"].concat();
        assert!(bytes.ends_with(&tail));
        match S2c::read(&bytes).unwrap() {
            S2c::SettingOverride { overrides, clear } => {
                assert!(!clear);
                assert_eq!(overrides["limitedTeleport"], "true");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn uber_packet_reads_the_fbt_part_only_before_the_two_trailing_floats() {
        let state = VrPlayerState {
            hmd: Pose::new(Vec3::new(0.0, 1.7, 0.0), glam::Quat::IDENTITY),
            ..VrPlayerState::default()
        };
        let p = S2c::UberPacket {
            player: 0x1122_3344_5566_7788_99aa_bbcc_ddee_ff00,
            state,
            world_scale: 1.5,
            height_scale: 0.9,
        };
        let bytes = p.write();
        assert_eq!(S2c::read(&bytes).unwrap(), p);
        let with_fbt = S2c::UberPacket {
            player: 7,
            state: VrPlayerState {
                fbt_mode: crate::network::FbtMode::ArmsLegs,
                waist: Some(Pose::default()),
                right_foot: Some(Pose::default()),
                left_foot: Some(Pose::default()),
                ..VrPlayerState::default()
            },
            world_scale: 1.0,
            height_scale: 1.0,
        };
        assert_eq!(S2c::read(&with_fbt.write()).unwrap(), with_fbt);
    }

    #[test]
    fn small_payloads_round_trip() {
        for p in [
            S2c::RequestData,
            S2c::VrSwitching(true),
            S2c::DualWielding(false),
            S2c::AttackWhileBlocking(true),
            S2c::IsVrActive {
                vr: false,
                player: 42,
            },
            S2c::Haptic {
                body_part: VrBodyPart::OffHand,
                duration: 0.1,
                frequency: 160.0,
                amplitude: 1.0,
                delay: 0.0,
            },
            S2c::DamageDirection(Vec3::new(0.0, 0.0, -1.0)),
            S2c::ServerVrChanges(BTreeMap::from([(
                "vrChanges.dualWielding".into(),
                "false".into(),
            )])),
        ] {
            assert_eq!(S2c::read(&p.write()).unwrap(), p);
        }
    }

    #[test]
    fn identifiers_are_the_ordinals() {
        assert_eq!(S2c::RequestData.write(), vec![1]);
        assert_eq!(S2c::VrSwitching(true).write(), vec![16, 1]);
        assert_eq!(S2c::DualWielding(true).write(), vec![19, 1]);
        assert_eq!(S2c::AttackWhileBlocking(true).write(), vec![23, 1]);
        assert_eq!(S2c::read(&[200]).unwrap(), S2c::Unknown);
        assert_eq!(S2c::read(&[26]).unwrap(), S2c::Unknown);
    }
}
