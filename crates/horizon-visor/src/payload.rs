// SPDX-License-Identifier: MPL-2.0
//! `VisorCorePayloadID` and the payloads of `org.vmstudio.visor.api.common.network`
//! (`toserver`, `toclient`, both `vrstate` folders). Copyright (C) Phoenix-Ra and Visor
//! contributors; MPL-2.0 (see LICENSE-MPL-2.0).
//!
//! Every payload on `visor:channel` starts with one byte, the ordinal of
//! [`PayloadId`], and the rest is the payload. A string that ends a payload takes the
//! whole rest of the buffer (`VisorPayload.readString`); everywhere else it is `writeUtf`.

use crate::buffer::PoseData;
use crate::{Error, Result};
use horizon_vivecraft::wire::{Reader, Writer};

/// `VisorCorePayloadID`, in declaration order: the ordinal is the wire id. There are 27
/// (`visor-port-plan.md` said 28; the enum in c01354f has 27).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum PayloadId {
    // To both
    Handshake,
    RotationY,
    OffhandSlot,
    // To the client
    ServerSettings,
    BlockDamage,
    OtherVrStartTracking,
    OtherVrPoseData,
    OtherVrRotationY,
    OtherVrLeftHanded,
    OtherVrBodyType,
    OtherVrWorldScale,
    OtherGunAngle,
    OtherVrFullHeight,
    // To the server
    FullHeight,
    PoseData,
    VrBodyType,
    LeftHanded,
    ActiveHand,
    WorldScale,
    GunAngle,
    Crawling,
    Climbing,
    Teleport,
    SwingAttack,
    SwingBlock,
    OverlayFocused,
    OtherVrOverlayFocused,
}

impl PayloadId {
    pub const ALL: [PayloadId; 27] = [
        PayloadId::Handshake,
        PayloadId::RotationY,
        PayloadId::OffhandSlot,
        PayloadId::ServerSettings,
        PayloadId::BlockDamage,
        PayloadId::OtherVrStartTracking,
        PayloadId::OtherVrPoseData,
        PayloadId::OtherVrRotationY,
        PayloadId::OtherVrLeftHanded,
        PayloadId::OtherVrBodyType,
        PayloadId::OtherVrWorldScale,
        PayloadId::OtherGunAngle,
        PayloadId::OtherVrFullHeight,
        PayloadId::FullHeight,
        PayloadId::PoseData,
        PayloadId::VrBodyType,
        PayloadId::LeftHanded,
        PayloadId::ActiveHand,
        PayloadId::WorldScale,
        PayloadId::GunAngle,
        PayloadId::Crawling,
        PayloadId::Climbing,
        PayloadId::Teleport,
        PayloadId::SwingAttack,
        PayloadId::SwingBlock,
        PayloadId::OverlayFocused,
        PayloadId::OtherVrOverlayFocused,
    ];

    /// `VisorCorePayloadID.fromOrdinal`: the byte as unsigned; `None` past the last id.
    pub fn from_wire(b: u8) -> Option<PayloadId> {
        Self::ALL.get(usize::from(b)).copied()
    }
}

/// `FriendlyByteBuf.writeBlockPos`: x in 26 bits, z in 26, y in 12 (`BlockPos.asLong`).
pub fn pack_block_pos(pos: [i32; 3]) -> i64 {
    ((i64::from(pos[0]) & 0x3FF_FFFF) << 38)
        | ((i64::from(pos[2]) & 0x3FF_FFFF) << 12)
        | (i64::from(pos[1]) & 0xFFF)
}

/// `BlockPos.of(long)`: the three fields sign extended.
pub fn unpack_block_pos(v: i64) -> [i32; 3] {
    [
        (v >> 38) as i32,
        ((v << 52) >> 52) as i32,
        ((v << 26) >> 38) as i32,
    ]
}

/// `Direction.get3DDataValue`: down, up, north, south, west, east.
pub const DIRECTION_COUNT: u8 = 6;

fn id_byte(w: &mut Writer, id: PayloadId) {
    w.byte(id as u8);
}

/// A payload from the client (`VisorPayloadToServer`).
#[derive(Clone, Debug, PartialEq)]
pub enum C2s {
    /// `HandshakePayloadToServer`: after login, to every server.
    Handshake {
        vr_active: bool,
        network_version: i32,
        visor_version: String,
    },
    ActiveHand {
        main: bool,
    },
    Crawling(bool),
    FullHeight(f32),
    GunAngle(f32),
    LeftHanded(bool),
    OffhandSlot(i32),
    Pose(Box<PoseData>),
    RotationY(f32),
    /// The id of the `VRBodyType` as the rest of the buffer.
    VrBodyType(String),
    WorldScale(f32),
    Climbing,
    SwingAttack {
        entity_id: i32,
        shift: bool,
        main_hand: bool,
    },
    SwingBlock {
        pos: [i32; 3],
        /// `Direction.get3DDataValue`.
        direction: u8,
        main_hand: bool,
        sequence: i32,
    },
    OverlayFocused(bool),
    Teleport {
        x: f32,
        y: f32,
        z: f32,
    },
    /// A valid id that is not a client payload (`UnknownPayloadToServer`): ignored.
    Unknown(u8),
}

impl C2s {
    pub fn id(&self) -> Option<PayloadId> {
        Some(match self {
            C2s::Handshake { .. } => PayloadId::Handshake,
            C2s::ActiveHand { .. } => PayloadId::ActiveHand,
            C2s::Crawling(_) => PayloadId::Crawling,
            C2s::FullHeight(_) => PayloadId::FullHeight,
            C2s::GunAngle(_) => PayloadId::GunAngle,
            C2s::LeftHanded(_) => PayloadId::LeftHanded,
            C2s::OffhandSlot(_) => PayloadId::OffhandSlot,
            C2s::Pose(_) => PayloadId::PoseData,
            C2s::RotationY(_) => PayloadId::RotationY,
            C2s::VrBodyType(_) => PayloadId::VrBodyType,
            C2s::WorldScale(_) => PayloadId::WorldScale,
            C2s::Climbing => PayloadId::Climbing,
            C2s::SwingAttack { .. } => PayloadId::SwingAttack,
            C2s::SwingBlock { .. } => PayloadId::SwingBlock,
            C2s::OverlayFocused(_) => PayloadId::OverlayFocused,
            C2s::Teleport { .. } => PayloadId::Teleport,
            C2s::Unknown(_) => return None,
        })
    }

    pub fn write(&self) -> Vec<u8> {
        let mut w = Writer::new();
        let Some(id) = self.id() else {
            // `UnknownPayloadToServer.write` writes nothing.
            return w.data;
        };
        id_byte(&mut w, id);
        match self {
            C2s::Handshake {
                vr_active,
                network_version,
                visor_version,
            } => {
                w.bool(*vr_active);
                w.int(*network_version);
                w.bytes(visor_version.as_bytes());
            }
            C2s::ActiveHand { main } => w.bool(*main),
            C2s::Crawling(v) | C2s::LeftHanded(v) | C2s::OverlayFocused(v) => w.bool(*v),
            C2s::FullHeight(v) | C2s::GunAngle(v) | C2s::RotationY(v) | C2s::WorldScale(v) => {
                w.float(*v);
            }
            C2s::OffhandSlot(v) => w.int(*v),
            C2s::Pose(p) => p.write(&mut w),
            C2s::VrBodyType(s) => w.bytes(s.as_bytes()),
            C2s::Climbing => {}
            C2s::SwingAttack {
                entity_id,
                shift,
                main_hand,
            } => {
                w.int(*entity_id);
                w.bool(*shift);
                w.bool(*main_hand);
            }
            C2s::SwingBlock {
                pos,
                direction,
                main_hand,
                sequence,
            } => {
                w.long(pack_block_pos(*pos));
                w.byte(*direction);
                w.bool(*main_hand);
                w.int(*sequence);
            }
            C2s::Teleport { x, y, z } => {
                w.float(*x);
                w.float(*y);
                w.float(*z);
            }
            C2s::Unknown(_) => {}
        }
        w.data
    }

    /// `VisorCorePayloadID.readToServer`.
    pub fn read(data: &[u8]) -> Result<C2s> {
        let mut r = Reader::new(data);
        let raw = r.ubyte()?;
        let Some(id) = PayloadId::from_wire(raw) else {
            return Err(Error(format!("visor payload: unknown id {raw}")));
        };
        Ok(match id {
            PayloadId::Handshake => C2s::Handshake {
                vr_active: r.bool()?,
                network_version: r.int()?,
                visor_version: rest_string(&mut r),
            },
            PayloadId::ActiveHand => C2s::ActiveHand { main: r.bool()? },
            PayloadId::Crawling => C2s::Crawling(r.bool()?),
            PayloadId::FullHeight => C2s::FullHeight(r.float()?),
            PayloadId::GunAngle => C2s::GunAngle(r.float()?),
            PayloadId::LeftHanded => C2s::LeftHanded(r.bool()?),
            PayloadId::OffhandSlot => C2s::OffhandSlot(r.int()?),
            PayloadId::PoseData => C2s::Pose(Box::new(PoseData::read(&mut r)?)),
            PayloadId::RotationY => C2s::RotationY(r.float()?),
            PayloadId::VrBodyType => C2s::VrBodyType(rest_string(&mut r)),
            PayloadId::WorldScale => C2s::WorldScale(r.float()?),
            PayloadId::Climbing => C2s::Climbing,
            PayloadId::SwingAttack => C2s::SwingAttack {
                entity_id: r.int()?,
                shift: r.bool()?,
                main_hand: r.bool()?,
            },
            PayloadId::SwingBlock => C2s::SwingBlock {
                pos: unpack_block_pos(r.long()?),
                // `Direction.from3DDataValue(readUnsignedByte)`: `abs(value % 6)`.
                direction: r.ubyte()? % DIRECTION_COUNT,
                main_hand: r.bool()?,
                sequence: r.int()?,
            },
            PayloadId::OverlayFocused => C2s::OverlayFocused(r.bool()?),
            PayloadId::Teleport => C2s::Teleport {
                x: r.float()?,
                y: r.float()?,
                z: r.float()?,
            },
            other => C2s::Unknown(other as u8),
        })
    }
}

/// `VisorPayload.readString`: everything left, as UTF-8.
fn rest_string(r: &mut Reader) -> String {
    String::from_utf8_lossy(r.rest()).into_owned()
}

/// What `OTHER_VR_START_TRACKING` carries: one player's whole state, in the order of
/// `VROtherStartTrackingPayloadToClient.onWrite`.
#[derive(Clone, Debug, PartialEq)]
pub struct StartTracking {
    pub player: u128,
    pub pose: PoseData,
    pub body_type: String,
    pub left_handed: bool,
    pub rotation_y: f32,
    pub world_scale: f32,
    pub full_height: f32,
    pub gun_angle: f32,
    pub overlay_focused: bool,
}

/// A payload from the server (`VisorPayloadToClient`).
#[derive(Clone, Debug, PartialEq)]
pub enum S2c {
    /// `HandshakePayloadToClient`: no body; the server supports Visor.
    Handshake,
    /// `SettingsPayloadToClient`: the YAML text of the settings (the rest of the buffer).
    ServerSettings(String),
    RotationY(f32),
    OffhandSlot(i32),
    BlockDamage {
        player: u128,
        pos: [i32; 3],
        destroy_stage: i32,
    },
    OtherStartTracking(Box<StartTracking>),
    OtherPose {
        player: u128,
        pose: Box<PoseData>,
    },
    OtherBodyType {
        player: u128,
        body_type: String,
    },
    OtherLeftHanded {
        player: u128,
        left_handed: bool,
    },
    OtherRotationY {
        player: u128,
        rotation_y: f32,
    },
    OtherWorldScale {
        player: u128,
        world_scale: f32,
    },
    OtherFullHeight {
        player: u128,
        full_height: f32,
    },
    OtherGunAngle {
        player: u128,
        gun_angle: f32,
    },
    OtherOverlayFocused {
        player: u128,
        overlay_focused: bool,
    },
    /// A valid id that is not a server payload (`UnknownPayloadToClient`): ignored.
    Unknown(u8),
}

impl S2c {
    pub fn id(&self) -> Option<PayloadId> {
        Some(match self {
            S2c::Handshake => PayloadId::Handshake,
            S2c::ServerSettings(_) => PayloadId::ServerSettings,
            S2c::RotationY(_) => PayloadId::RotationY,
            S2c::OffhandSlot(_) => PayloadId::OffhandSlot,
            S2c::BlockDamage { .. } => PayloadId::BlockDamage,
            S2c::OtherStartTracking(_) => PayloadId::OtherVrStartTracking,
            S2c::OtherPose { .. } => PayloadId::OtherVrPoseData,
            S2c::OtherBodyType { .. } => PayloadId::OtherVrBodyType,
            S2c::OtherLeftHanded { .. } => PayloadId::OtherVrLeftHanded,
            S2c::OtherRotationY { .. } => PayloadId::OtherVrRotationY,
            S2c::OtherWorldScale { .. } => PayloadId::OtherVrWorldScale,
            S2c::OtherFullHeight { .. } => PayloadId::OtherVrFullHeight,
            S2c::OtherGunAngle { .. } => PayloadId::OtherGunAngle,
            S2c::OtherOverlayFocused { .. } => PayloadId::OtherVrOverlayFocused,
            S2c::Unknown(_) => return None,
        })
    }

    pub fn write(&self) -> Vec<u8> {
        let mut w = Writer::new();
        let Some(id) = self.id() else {
            return w.data;
        };
        id_byte(&mut w, id);
        match self {
            S2c::Handshake | S2c::Unknown(_) => {}
            S2c::ServerSettings(text) => w.bytes(text.as_bytes()),
            S2c::RotationY(v) => w.float(*v),
            S2c::OffhandSlot(v) => w.int(*v),
            S2c::BlockDamage {
                player,
                pos,
                destroy_stage,
            } => {
                w.uuid(*player);
                w.long(pack_block_pos(*pos));
                w.int(*destroy_stage);
            }
            S2c::OtherStartTracking(t) => {
                w.uuid(t.player);
                t.pose.write(&mut w);
                w.utf(&t.body_type);
                w.bool(t.left_handed);
                w.float(t.rotation_y);
                w.float(t.world_scale);
                w.float(t.full_height);
                w.float(t.gun_angle);
                w.bool(t.overlay_focused);
            }
            S2c::OtherPose { player, pose } => {
                w.uuid(*player);
                pose.write(&mut w);
            }
            S2c::OtherBodyType { player, body_type } => {
                w.uuid(*player);
                w.utf(body_type);
            }
            S2c::OtherLeftHanded {
                player,
                left_handed,
            } => {
                w.uuid(*player);
                w.bool(*left_handed);
            }
            S2c::OtherOverlayFocused {
                player,
                overlay_focused,
            } => {
                w.uuid(*player);
                w.bool(*overlay_focused);
            }
            S2c::OtherRotationY { player, rotation_y } => {
                w.uuid(*player);
                w.float(*rotation_y);
            }
            S2c::OtherWorldScale {
                player,
                world_scale,
            } => {
                w.uuid(*player);
                w.float(*world_scale);
            }
            S2c::OtherFullHeight {
                player,
                full_height,
            } => {
                w.uuid(*player);
                w.float(*full_height);
            }
            S2c::OtherGunAngle { player, gun_angle } => {
                w.uuid(*player);
                w.float(*gun_angle);
            }
        }
        w.data
    }

    /// `VisorCorePayloadID.readToClient`.
    pub fn read(data: &[u8]) -> Result<S2c> {
        let mut r = Reader::new(data);
        let raw = r.ubyte()?;
        let Some(id) = PayloadId::from_wire(raw) else {
            return Err(Error(format!("visor payload: unknown id {raw}")));
        };
        Ok(match id {
            PayloadId::Handshake => S2c::Handshake,
            PayloadId::ServerSettings => S2c::ServerSettings(rest_string(&mut r)),
            PayloadId::RotationY => S2c::RotationY(r.float()?),
            PayloadId::OffhandSlot => S2c::OffhandSlot(r.int()?),
            PayloadId::BlockDamage => S2c::BlockDamage {
                player: r.uuid()?,
                pos: unpack_block_pos(r.long()?),
                destroy_stage: r.int()?,
            },
            PayloadId::OtherVrStartTracking => S2c::OtherStartTracking(Box::new(StartTracking {
                player: r.uuid()?,
                pose: PoseData::read(&mut r)?,
                body_type: r.utf()?,
                left_handed: r.bool()?,
                rotation_y: r.float()?,
                world_scale: r.float()?,
                full_height: r.float()?,
                gun_angle: r.float()?,
                overlay_focused: r.bool()?,
            })),
            PayloadId::OtherVrPoseData => S2c::OtherPose {
                player: r.uuid()?,
                pose: Box::new(PoseData::read(&mut r)?),
            },
            PayloadId::OtherVrBodyType => S2c::OtherBodyType {
                player: r.uuid()?,
                body_type: r.utf()?,
            },
            PayloadId::OtherVrLeftHanded => S2c::OtherLeftHanded {
                player: r.uuid()?,
                left_handed: r.bool()?,
            },
            PayloadId::OtherVrRotationY => S2c::OtherRotationY {
                player: r.uuid()?,
                rotation_y: r.float()?,
            },
            PayloadId::OtherVrWorldScale => S2c::OtherWorldScale {
                player: r.uuid()?,
                world_scale: r.float()?,
            },
            PayloadId::OtherVrFullHeight => S2c::OtherFullHeight {
                player: r.uuid()?,
                full_height: r.float()?,
            },
            PayloadId::OtherGunAngle => S2c::OtherGunAngle {
                player: r.uuid()?,
                gun_angle: r.float()?,
            },
            PayloadId::OtherVrOverlayFocused => S2c::OtherOverlayFocused {
                player: r.uuid()?,
                overlay_focused: r.bool()?,
            },
            other => S2c::Unknown(other as u8),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::PoseElement;
    use glam::{Quat, Vec3};

    fn bytes(hex: &str) -> Vec<u8> {
        let h: String = hex.split_whitespace().collect();
        (0..h.len() / 2)
            .map(|i| u8::from_str_radix(&h[i * 2..i * 2 + 2], 16).unwrap())
            .collect()
    }

    // The ordinals of VisorCorePayloadID, counted from the enum in the Java source.
    #[test]
    fn ordinals_follow_the_java_enum() {
        let expected = [
            (PayloadId::Handshake, 0),
            (PayloadId::RotationY, 1),
            (PayloadId::OffhandSlot, 2),
            (PayloadId::ServerSettings, 3),
            (PayloadId::BlockDamage, 4),
            (PayloadId::OtherVrStartTracking, 5),
            (PayloadId::OtherVrPoseData, 6),
            (PayloadId::OtherVrRotationY, 7),
            (PayloadId::OtherVrLeftHanded, 8),
            (PayloadId::OtherVrBodyType, 9),
            (PayloadId::OtherVrWorldScale, 10),
            (PayloadId::OtherGunAngle, 11),
            (PayloadId::OtherVrFullHeight, 12),
            (PayloadId::FullHeight, 13),
            (PayloadId::PoseData, 14),
            (PayloadId::VrBodyType, 15),
            (PayloadId::LeftHanded, 16),
            (PayloadId::ActiveHand, 17),
            (PayloadId::WorldScale, 18),
            (PayloadId::GunAngle, 19),
            (PayloadId::Crawling, 20),
            (PayloadId::Climbing, 21),
            (PayloadId::Teleport, 22),
            (PayloadId::SwingAttack, 23),
            (PayloadId::SwingBlock, 24),
            (PayloadId::OverlayFocused, 25),
            (PayloadId::OtherVrOverlayFocused, 26),
        ];
        for (id, n) in expected {
            assert_eq!(id as u8, n);
            assert_eq!(PayloadId::from_wire(n), Some(id));
        }
        assert_eq!(PayloadId::ALL.len(), 27);
        assert_eq!(PayloadId::from_wire(27), None);
        assert_eq!(PayloadId::from_wire(0xFF), None);
    }

    #[test]
    fn handshake_is_flag_int_and_the_version_to_the_end() {
        let p = C2s::Handshake {
            vr_active: true,
            network_version: 5,
            visor_version: "0.6.1".into(),
        };
        assert_eq!(p.write(), bytes("00 01 00000005 302e362e31"));
        assert_eq!(C2s::read(&p.write()).unwrap(), p);
        // The server answers with the id alone.
        assert_eq!(S2c::Handshake.write(), [0]);
    }

    #[test]
    fn teleport_is_three_floats() {
        let p = C2s::Teleport {
            x: 1.0,
            y: 2.0,
            z: -1.0,
        };
        assert_eq!(p.write(), bytes("16 3f800000 40000000 bf800000"));
        assert_eq!(C2s::read(&p.write()).unwrap(), p);
    }

    #[test]
    fn swing_attack_layout() {
        let p = C2s::SwingAttack {
            entity_id: 258,
            shift: true,
            main_hand: false,
        };
        assert_eq!(p.write(), bytes("17 00000102 01 00"));
        assert_eq!(C2s::read(&p.write()).unwrap(), p);
    }

    // BlockPos.asLong for (1, 2, 3): 1 << 38 | 3 << 12 | 2.
    #[test]
    fn swing_block_packs_the_position_like_block_pos_as_long() {
        let p = C2s::SwingBlock {
            pos: [1, 2, 3],
            direction: 1,
            main_hand: true,
            sequence: 7,
        };
        assert_eq!(p.write(), bytes("18 0000004000003002 01 01 00000007"));
        assert_eq!(C2s::read(&p.write()).unwrap(), p);
        // (-1, -1, -1) is the long -1; (-30000000, 319, 29999999) survives a round trip.
        assert_eq!(pack_block_pos([-1, -1, -1]), -1);
        assert_eq!(unpack_block_pos(-1), [-1, -1, -1]);
        let far = [-30_000_000, -64, 29_999_999];
        assert_eq!(unpack_block_pos(pack_block_pos(far)), far);
    }

    #[test]
    fn body_type_to_the_server_has_no_length_but_to_clients_it_does() {
        let up = C2s::VrBodyType("full_body".into());
        assert_eq!(up.write(), bytes("0f 66756c6c5f626f6479"));
        assert_eq!(C2s::read(&up.write()).unwrap(), up);
        let down = S2c::OtherBodyType {
            player: 0x0011_2233_4455_6677_8899_aabb_ccdd_eeff,
            body_type: "hands_only".into(),
        };
        assert_eq!(
            down.write(),
            bytes("09 00112233445566778899aabbccddeeff 0a 68616e64735f6f6e6c79")
        );
        assert_eq!(S2c::read(&down.write()).unwrap(), down);
    }

    #[test]
    fn small_states_to_the_server() {
        assert_eq!(C2s::ActiveHand { main: true }.write(), bytes("11 01"));
        assert_eq!(C2s::Crawling(true).write(), bytes("14 01"));
        assert_eq!(C2s::Climbing.write(), bytes("15"));
        assert_eq!(C2s::LeftHanded(false).write(), bytes("10 00"));
        assert_eq!(C2s::OffhandSlot(8).write(), bytes("02 00000008"));
        assert_eq!(C2s::RotationY(1.0).write(), bytes("01 3f800000"));
        assert_eq!(C2s::FullHeight(1.5).write(), bytes("0d 3fc00000"));
        assert_eq!(C2s::WorldScale(2.0).write(), bytes("12 40000000"));
        assert_eq!(C2s::GunAngle(60.0).write(), bytes("13 42700000"));
        assert_eq!(C2s::OverlayFocused(true).write(), bytes("19 01"));
    }

    #[test]
    fn pose_data_to_the_server() {
        let mut pose = PoseData::default();
        pose.main_hand = PoseElement::new(Vec3::new(1.0, 2.0, 3.0), Quat::IDENTITY);
        let wire = C2s::Pose(Box::new(pose)).write();
        let expected = bytes(
            "0e
             00000000 00000000 00000000  00000000 00000000 00000000 3f800000
             3f800000 40000000 40400000  00000000 00000000 00000000 3f800000
             00000000 00000000 00000000  00000000 00000000 00000000 3f800000
             00 00 00",
        );
        assert_eq!(wire, expected);
        assert_eq!(C2s::read(&wire).unwrap(), C2s::Pose(Box::new(pose)));
    }

    #[test]
    fn block_damage_to_the_client() {
        let p = S2c::BlockDamage {
            player: 1,
            pos: [1, 2, 3],
            destroy_stage: 9,
        };
        assert_eq!(
            p.write(),
            bytes("04 00000000000000000000000000000001 0000004000003002 00000009")
        );
        assert_eq!(S2c::read(&p.write()).unwrap(), p);
    }

    #[test]
    fn settings_are_the_text_to_the_end() {
        let p = S2c::ServerSettings("vrOnly: true\n".into());
        assert_eq!(p.write(), bytes("03 76724f6e6c793a20747275650a"));
        assert_eq!(S2c::read(&p.write()).unwrap(), p);
    }

    // OTHER_VR_START_TRACKING: the uuid once, then pose, body type, left handed,
    // rotation, scale, height, gun angle, overlay, each as its payload's writeSimple.
    #[test]
    fn start_tracking_is_the_other_payloads_back_to_back() {
        let t = StartTracking {
            player: 0x42,
            pose: PoseData::default(),
            body_type: "hands_only".into(),
            left_handed: true,
            rotation_y: 1.0,
            world_scale: 2.0,
            full_height: 1.5,
            gun_angle: 60.0,
            overlay_focused: false,
        };
        let wire = S2c::OtherStartTracking(Box::new(t.clone())).write();
        let expected = bytes(
            "05 00000000000000000000000000000042
             00000000 00000000 00000000  00000000 00000000 00000000 3f800000
             00000000 00000000 00000000  00000000 00000000 00000000 3f800000
             00000000 00000000 00000000  00000000 00000000 00000000 3f800000
             00 00 00
             0a 68616e64735f6f6e6c79
             01 3f800000 40000000 3fc00000 42700000 00",
        );
        assert_eq!(wire, expected);
        assert_eq!(
            S2c::read(&wire).unwrap(),
            S2c::OtherStartTracking(Box::new(t))
        );
    }

    #[test]
    fn other_state_payloads() {
        let id = 0x0011_2233_4455_6677_8899_aabb_ccdd_eeffu128;
        let uuid = "00112233445566778899aabbccddeeff";
        for (p, hex) in [
            (
                S2c::OtherLeftHanded {
                    player: id,
                    left_handed: true,
                },
                format!("08 {uuid} 01"),
            ),
            (
                S2c::OtherRotationY {
                    player: id,
                    rotation_y: 1.0,
                },
                format!("07 {uuid} 3f800000"),
            ),
            (
                S2c::OtherWorldScale {
                    player: id,
                    world_scale: 0.5,
                },
                format!("0a {uuid} 3f000000"),
            ),
            (
                S2c::OtherGunAngle {
                    player: id,
                    gun_angle: 60.0,
                },
                format!("0b {uuid} 42700000"),
            ),
            (
                S2c::OtherFullHeight {
                    player: id,
                    full_height: 2.0,
                },
                format!("0c {uuid} 40000000"),
            ),
            (
                S2c::OtherOverlayFocused {
                    player: id,
                    overlay_focused: true,
                },
                format!("1a {uuid} 01"),
            ),
        ] {
            assert_eq!(p.write(), bytes(&hex));
            assert_eq!(S2c::read(&p.write()).unwrap(), p);
        }
    }

    #[test]
    fn foreign_directions_and_payloads_are_handled() {
        // Direction.from3DDataValue wraps with `% 6`.
        let wire = bytes("18 0000004000003002 07 00 00000000");
        let C2s::SwingBlock { direction, .. } = C2s::read(&wire).unwrap() else {
            panic!("not a swing block");
        };
        assert_eq!(direction, 1);
        // An id that only goes the other way is ignored, not an error.
        assert_eq!(C2s::read(&[3, 1, 2]).unwrap(), C2s::Unknown(3));
        assert_eq!(S2c::read(&[14]).unwrap(), S2c::Unknown(14));
        // An id past the enum is an error (`values()[id]` throws).
        assert!(C2s::read(&[27]).is_err());
        assert!(S2c::read(&[0xFF]).is_err());
        // A truncated float.
        assert!(C2s::read(&[0x16, 0, 0]).is_err());
    }
}
