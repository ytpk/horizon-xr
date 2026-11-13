// SPDX-License-Identifier: LGPL-3.0-or-later
//! Ports `org.vivecraft.common.network.packet.c2s.*` and `VivecraftPayloadC2S.readPacket`.
//! Copyright (c) Vivecraft contributors; LGPL-3.0-or-later (see LICENSE-LGPL-3.0).

use super::pose::{read_vec3, write_vec3};
use super::{NetworkVersion, PayloadId, Pose, VrBodyPart, VrPlayerState};
use crate::wire::{Reader, Writer};
use crate::{Error, Result};
use glam::Vec3;

/// A client to server payload.
#[derive(Clone, Debug, PartialEq)]
pub enum C2s {
    /// `version` is e.g. `Vivecraft-26.2-fabric-1.3.15`; `legacy` is a client that sent
    /// no protocol range.
    Version {
        version: String,
        vr: bool,
        max_version: i32,
        min_version: i32,
        legacy: bool,
    },
    HeadData {
        seated: bool,
        hmd: Pose,
    },
    Controller0Data {
        left_handed: bool,
        main_hand: Pose,
    },
    Controller1Data {
        left_handed: bool,
        off_hand: Pose,
    },
    WorldScale(f32),
    Draw(f32),
    Teleport {
        x: f32,
        y: f32,
        z: f32,
    },
    Climbing,
    Jumping,
    Height(f32),
    /// `ACTIVEHAND`. `use_for_aim` is only on the wire from HEAD_AIM on.
    ActiveHand {
        body_part: VrBodyPart,
        use_for_aim: bool,
    },
    Crawl(bool),
    IsVrActive(bool),
    VrPlayerState(VrPlayerState),
    DamageDirection,
    AimDirectionOverride(Vec3),
    AimOverrideReset(i8),
    AimPositionOverride(Vec3),
    /// An identifier the server does not expect from a client, or none at all.
    Unknown,
}

impl C2s {
    pub fn id(&self) -> Option<PayloadId> {
        Some(match self {
            C2s::Version { .. } => PayloadId::Version,
            C2s::HeadData { .. } => PayloadId::HeadData,
            C2s::Controller0Data { .. } => PayloadId::Controller0Data,
            C2s::Controller1Data { .. } => PayloadId::Controller1Data,
            C2s::WorldScale(_) => PayloadId::WorldScale,
            C2s::Draw(_) => PayloadId::Draw,
            C2s::Teleport { .. } => PayloadId::Teleport,
            C2s::Climbing => PayloadId::Climbing,
            C2s::Jumping => PayloadId::Jumping,
            C2s::Height(_) => PayloadId::Height,
            C2s::ActiveHand { .. } => PayloadId::ActiveHand,
            C2s::Crawl(_) => PayloadId::Crawl,
            C2s::IsVrActive(_) => PayloadId::IsVrActive,
            C2s::VrPlayerState(_) => PayloadId::VrPlayerState,
            C2s::DamageDirection => PayloadId::DamageDirection,
            C2s::AimDirectionOverride(_) => PayloadId::AimDirectionOverride,
            C2s::AimOverrideReset(_) => PayloadId::AimOverrideReset,
            C2s::AimPositionOverride(_) => PayloadId::AimPositionOverride,
            C2s::Unknown => return None,
        })
    }

    /// The payload's bytes. `used` is `ClientNetworking.USED_NETWORK_VERSION`: the
    /// `ACTIVEHAND` flag is only written when it accepts HEAD_AIM.
    pub fn write(&self, used: NetworkVersion) -> Vec<u8> {
        let mut w = Writer::new();
        let Some(id) = self.id() else {
            return w.data;
        };
        w.byte(id as u8);
        match self {
            C2s::Version {
                version,
                vr,
                max_version,
                min_version,
                ..
            } => {
                // "%s %s\n%d\n%d", raw UTF-8 bytes without a length (legacy compatible).
                w.bytes(
                    format!(
                        "{} {}\n{}\n{}",
                        version,
                        if *vr { "VR" } else { "NONVR" },
                        max_version,
                        min_version
                    )
                    .as_bytes(),
                );
            }
            C2s::HeadData { seated, hmd } => {
                w.bool(*seated);
                hmd.serialize(&mut w);
            }
            C2s::Controller0Data {
                left_handed,
                main_hand,
            } => {
                w.bool(*left_handed);
                main_hand.serialize(&mut w);
            }
            C2s::Controller1Data {
                left_handed,
                off_hand,
            } => {
                w.bool(*left_handed);
                off_hand.serialize(&mut w);
            }
            C2s::WorldScale(v) | C2s::Draw(v) | C2s::Height(v) => w.float(*v),
            C2s::Teleport { x, y, z } => {
                w.float(*x);
                w.float(*y);
                w.float(*z);
            }
            C2s::Climbing | C2s::Jumping | C2s::DamageDirection | C2s::Unknown => {}
            C2s::ActiveHand {
                body_part,
                use_for_aim,
            } => {
                w.byte(*body_part as u8);
                if NetworkVersion::HeadAim.accepts(used) {
                    w.bool(*use_for_aim);
                }
            }
            C2s::Crawl(b) | C2s::IsVrActive(b) => w.bool(*b),
            C2s::VrPlayerState(s) => s.serialize(&mut w),
            C2s::AimDirectionOverride(v) | C2s::AimPositionOverride(v) => write_vec3(&mut w, *v),
            C2s::AimOverrideReset(t) => w.byte(*t as u8),
        }
        w.data
    }

    /// `VivecraftPayloadC2S.readPacket`: an identifier the server does not handle gives
    /// [`C2s::Unknown`]; a payload that is cut short is an error.
    pub fn read(data: &[u8]) -> Result<Self> {
        let mut r = Reader::new(data);
        let Some(id) = PayloadId::from_wire(r.byte()?) else {
            return Ok(C2s::Unknown);
        };
        Ok(match id {
            PayloadId::Version => {
                let text = String::from_utf8_lossy(r.rest()).into_owned();
                let parts: Vec<&str> = text.split('\n').collect();
                // Java's String.split drops trailing empty strings; only the count of
                // parts (>= 3) decides here, which the same trimming does not change
                // for a well formed payload.
                let vr = !parts[0].contains("NONVR");
                if parts.len() >= 3 {
                    let max_version = parts[1]
                        .trim()
                        .parse()
                        .map_err(|_| Error::new("vivecraft payload: bad max version"))?;
                    let min_version = parts[2]
                        .trim()
                        .parse()
                        .map_err(|_| Error::new("vivecraft payload: bad min version"))?;
                    C2s::Version {
                        version: parts[0].to_string(),
                        vr,
                        max_version,
                        min_version,
                        legacy: false,
                    }
                } else {
                    C2s::Version {
                        version: parts[0].to_string(),
                        vr,
                        max_version: NetworkVersion::Legacy.protocol_version(),
                        min_version: NetworkVersion::Legacy.protocol_version(),
                        legacy: true,
                    }
                }
            }
            PayloadId::HeadData => C2s::HeadData {
                seated: r.bool()?,
                hmd: Pose::deserialize(&mut r)?,
            },
            PayloadId::Controller0Data => C2s::Controller0Data {
                left_handed: r.bool()?,
                main_hand: Pose::deserialize(&mut r)?,
            },
            PayloadId::Controller1Data => C2s::Controller1Data {
                left_handed: r.bool()?,
                off_hand: Pose::deserialize(&mut r)?,
            },
            PayloadId::WorldScale => C2s::WorldScale(r.float()?),
            PayloadId::Draw => C2s::Draw(r.float()?),
            PayloadId::Teleport => C2s::Teleport {
                x: r.float()?,
                y: r.float()?,
                z: r.float()?,
            },
            PayloadId::Climbing => C2s::Climbing,
            PayloadId::Jumping => C2s::Jumping,
            PayloadId::Height => C2s::Height(r.float()?),
            PayloadId::ActiveHand => {
                let body_part = VrBodyPart::from_wire(r.byte()?)
                    .ok_or_else(|| Error::new("vivecraft payload: bad body part"))?;
                let use_for_aim = if r.remaining() > 0 { r.bool()? } else { false };
                C2s::ActiveHand {
                    body_part,
                    use_for_aim,
                }
            }
            PayloadId::Crawl => C2s::Crawl(r.bool()?),
            PayloadId::IsVrActive => C2s::IsVrActive(r.bool()?),
            PayloadId::VrPlayerState => C2s::VrPlayerState(VrPlayerState::deserialize(&mut r, 0)?),
            PayloadId::DamageDirection => C2s::DamageDirection,
            PayloadId::AimDirectionOverride => C2s::AimDirectionOverride(read_vec3(&mut r)?),
            PayloadId::AimOverrideReset => C2s::AimOverrideReset(r.byte()?),
            PayloadId::AimPositionOverride => C2s::AimPositionOverride(read_vec3(&mut r)?),
            _ => C2s::Unknown,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::FbtMode;
    use glam::Quat;

    fn pose(x: f32) -> Pose {
        Pose::new(
            Vec3::new(x, x + 1.0, x + 2.0),
            Quat::from_xyzw(0.0, 0.0, 0.0, 1.0),
        )
    }

    #[test]
    fn version_is_text_not_length_prefixed() {
        let p = C2s::Version {
            version: "Vivecraft-26.2-fabric-1.3.15".into(),
            vr: true,
            max_version: 9,
            min_version: 0,
            legacy: false,
        };
        let bytes = p.write(NetworkVersion::Legacy);
        // The identifier byte (VERSION is 0), then the text VersionPayloadC2S.write makes.
        assert_eq!(bytes[0], 0);
        assert_eq!(
            &bytes[1..],
            b"Vivecraft-26.2-fabric-1.3.15 VR\n9\n0".as_slice()
        );
        // The read keeps the " VR" in the version, as VersionPayloadC2S.read does.
        match C2s::read(&bytes).unwrap() {
            C2s::Version {
                version,
                vr,
                max_version,
                min_version,
                legacy,
            } => {
                assert_eq!(version, "Vivecraft-26.2-fabric-1.3.15 VR");
                assert!(vr && !legacy);
                assert_eq!((max_version, min_version), (9, 0));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_legacy_version_has_no_range() {
        let mut bytes = vec![0u8];
        bytes.extend_from_slice(b"Vivecraft 1.19.2 (jrbudda)-NONVR-fabric-1.0");
        match C2s::read(&bytes).unwrap() {
            C2s::Version {
                vr,
                legacy,
                max_version,
                ..
            } => {
                assert!(!vr);
                assert!(legacy);
                assert_eq!(max_version, -1);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_player_state_is_hmd_hands_in_vivecrafts_order() {
        let s = VrPlayerState {
            seated: true,
            hmd: pose(1.0),
            left_handed: true,
            main_hand: pose(2.0),
            reverse_hands_legacy: true,
            off_hand: pose(3.0),
            ..VrPlayerState::default()
        };
        let bytes = C2s::VrPlayerState(s.clone()).write(NetworkVersion::ClimbeyJump);
        // 1 id + (1 + 28) + (1 + 28) + (1 + 28) with 7 floats a pose.
        assert_eq!(bytes.len(), 1 + 3 * 29);
        assert_eq!(bytes[0], PayloadId::VrPlayerState as u8);
        assert_eq!(bytes[1], 1, "seated");
        // hmd position x = 1.0, then y = 2.0 ...
        assert_eq!(&bytes[2..6], &1.0f32.to_be_bytes());
        // the quaternion is w, x, y, z: w is 1.0 for the identity.
        assert_eq!(&bytes[14..18], &1.0f32.to_be_bytes());
        assert_eq!(C2s::read(&bytes).unwrap(), C2s::VrPlayerState(s));
    }

    #[test]
    fn fbt_data_follows_the_hands() {
        let z = pose(0.5);
        let s = VrPlayerState {
            fbt_mode: FbtMode::WithJoints,
            waist: Some(z),
            right_foot: Some(z),
            left_foot: Some(z),
            right_knee: Some(z),
            left_knee: Some(z),
            right_elbow: Some(z),
            left_elbow: Some(z),
            hmd: pose(0.0),
            main_hand: pose(0.0),
            off_hand: pose(0.0),
            ..VrPlayerState::default()
        };
        let bytes = C2s::VrPlayerState(s.clone()).write(NetworkVersion::ClimbeyJump);
        assert_eq!(bytes.len(), 1 + 3 * 29 + 1 + 7 * 28);
        assert_eq!(C2s::read(&bytes).unwrap(), C2s::VrPlayerState(s.clone()));
        let legacy = s.stripped_for(NetworkVersion::NewNetworking);
        assert_eq!(legacy.fbt_mode, FbtMode::ArmsOnly);
        assert!(legacy.waist.is_none());
    }

    #[test]
    fn active_hand_flag_depends_on_the_version() {
        let p = C2s::ActiveHand {
            body_part: VrBodyPart::OffHand,
            use_for_aim: true,
        };
        assert_eq!(p.write(NetworkVersion::DualWielding), vec![13, 1]);
        assert_eq!(p.write(NetworkVersion::HeadAim), vec![13, 1, 1]);
        assert_eq!(
            C2s::read(&[13, 1]).unwrap(),
            C2s::ActiveHand {
                body_part: VrBodyPart::OffHand,
                use_for_aim: false
            }
        );
    }

    #[test]
    fn small_payloads_round_trip() {
        for p in [
            C2s::WorldScale(2.0),
            C2s::Draw(0.75),
            C2s::Height(1.1),
            C2s::Teleport {
                x: 1.0,
                y: 64.0,
                z: -3.5,
            },
            C2s::Climbing,
            C2s::Jumping,
            C2s::Crawl(true),
            C2s::IsVrActive(false),
            C2s::DamageDirection,
            C2s::AimDirectionOverride(Vec3::new(0.0, 1.0, 0.0)),
            C2s::AimOverrideReset(5),
            C2s::AimPositionOverride(Vec3::new(1.0, 2.0, 3.0)),
        ] {
            assert_eq!(C2s::read(&p.write(NetworkVersion::ClimbeyJump)).unwrap(), p);
        }
    }

    #[test]
    fn identifiers_are_the_ordinals() {
        assert_eq!(C2s::WorldScale(1.0).write(NetworkVersion::Legacy)[0], 5);
        assert_eq!(C2s::Draw(1.0).write(NetworkVersion::Legacy)[0], 6);
        assert_eq!(
            C2s::Teleport {
                x: 0.0,
                y: 0.0,
                z: 0.0
            }
            .write(NetworkVersion::Legacy)[0],
            9
        );
        assert_eq!(C2s::Climbing.write(NetworkVersion::Legacy), vec![10]);
        assert_eq!(C2s::Height(1.0).write(NetworkVersion::Legacy)[0], 12);
        assert_eq!(C2s::Crawl(true).write(NetworkVersion::Legacy), vec![14, 1]);
        assert_eq!(
            C2s::IsVrActive(true).write(NetworkVersion::Legacy),
            vec![17, 1]
        );
        assert_eq!(C2s::DamageDirection.write(NetworkVersion::Legacy), vec![22]);
        assert_eq!(C2s::Jumping.write(NetworkVersion::Legacy), vec![27]);
    }

    #[test]
    fn unknown_identifiers_are_ignored() {
        assert_eq!(C2s::read(&[100, 1, 2]).unwrap(), C2s::Unknown);
        assert_eq!(C2s::read(&[0xff]).unwrap(), C2s::Unknown);
        // An id a server sends but never expects from a client.
        assert_eq!(C2s::read(&[1]).unwrap(), C2s::Unknown);
    }
}
