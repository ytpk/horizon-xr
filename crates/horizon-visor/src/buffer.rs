// SPDX-License-Identifier: MPL-2.0
//! `org.vmstudio.visor.api.common.network.buffer`: `PoseElementBuffer`, `PoseTrackersBuffer`,
//! `PoseHandsBuffer`, `PoseDataBuffer`. Copyright (C) Phoenix-Ra and Visor contributors;
//! MPL-2.0 (see LICENSE-MPL-2.0).
//!
//! Positions are relative to the player's position (feet) and orientations are world
//! rotations. A quaternion goes out as x, y, z, w (Vivecraft's is w, x, y, z).

use crate::Result;
use glam::{Quat, Vec3};
use horizon_vivecraft::wire::{Reader, Writer};

/// `VRHandJointType.COUNT`: the joints of `XR_EXT_hand_tracking`.
pub const HAND_JOINT_COUNT: usize = 26;
/// The body trackers of `PoseTrackersBuffer`.
pub const TRACKER_COUNT: usize = 14;

/// `PoseElementBuffer`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PoseElement {
    pub position: Vec3,
    pub orientation: Quat,
}

impl PoseElement {
    pub const IDENTITY: PoseElement = PoseElement {
        position: Vec3::ZERO,
        orientation: Quat::IDENTITY,
    };

    pub fn new(position: Vec3, orientation: Quat) -> Self {
        Self {
            position,
            orientation,
        }
    }

    pub fn write(&self, w: &mut Writer) {
        w.float(self.position.x);
        w.float(self.position.y);
        w.float(self.position.z);
        w.float(self.orientation.x);
        w.float(self.orientation.y);
        w.float(self.orientation.z);
        w.float(self.orientation.w);
    }

    pub fn read(r: &mut Reader) -> Result<Self> {
        let position = Vec3::new(r.float()?, r.float()?, r.float()?);
        let orientation = Quat::from_xyzw(r.float()?, r.float()?, r.float()?, r.float()?);
        Ok(Self {
            position,
            orientation,
        })
    }
}

/// `VRBodyPartType` order of the tracker bit mask (`PoseTrackersBuffer.TYPES`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum TrackerKind {
    Waist,
    Chest,
    LeftFoot,
    RightFoot,
    LeftAnkle,
    RightAnkle,
    LeftKnee,
    RightKnee,
    LeftWrist,
    RightWrist,
    LeftElbow,
    RightElbow,
    LeftShoulder,
    RightShoulder,
}

/// `PoseTrackersBuffer`: any of the 14 may be absent.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Trackers(pub [Option<PoseElement>; TRACKER_COUNT]);

impl Trackers {
    pub fn get(&self, kind: TrackerKind) -> Option<PoseElement> {
        self.0[kind as usize]
    }

    pub fn is_empty(&self) -> bool {
        self.0.iter().all(Option::is_none)
    }

    pub fn write(&self, w: &mut Writer) {
        write_masked(w, &self.0);
    }

    pub fn read(r: &mut Reader) -> Result<Self> {
        let mut out = [None; TRACKER_COUNT];
        read_masked(r, &mut out)?;
        Ok(Self(out))
    }
}

/// One hand's joints (`PoseHandBuffer`), indexed by `VRHandJointType` ordinal.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HandJoints(pub [Option<PoseElement>; HAND_JOINT_COUNT]);

impl HandJoints {
    pub fn is_empty(&self) -> bool {
        self.0.iter().all(Option::is_none)
    }

    pub fn write(&self, w: &mut Writer) {
        write_masked(w, &self.0);
    }

    pub fn read(r: &mut Reader) -> Result<Self> {
        let mut out = [None; HAND_JOINT_COUNT];
        read_masked(r, &mut out)?;
        Ok(Self(out))
    }
}

/// `PoseHandsBuffer`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Hands {
    pub left: HandJoints,
    pub right: HandJoints,
}

/// A VarInt bit mask of the present elements, then those elements in index order.
fn write_masked(w: &mut Writer, items: &[Option<PoseElement>]) {
    let mut mask = 0u32;
    for (i, item) in items.iter().enumerate() {
        if item.is_some() {
            mask |= 1 << i;
        }
    }
    w.var_int(mask);
    for item in items.iter().flatten() {
        item.write(w);
    }
}

fn read_masked(r: &mut Reader, out: &mut [Option<PoseElement>]) -> Result<()> {
    let mask = r.var_int()?;
    for (i, slot) in out.iter_mut().enumerate() {
        if mask & (1 << i) != 0 {
            *slot = Some(PoseElement::read(r)?);
        }
    }
    Ok(())
}

/// `PoseDataBuffer`: the whole body for one tick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PoseData {
    pub hmd: PoseElement,
    pub main_hand: PoseElement,
    pub off_hand: PoseElement,
    pub trackers: Trackers,
    pub hands: Hands,
}

impl Default for PoseData {
    fn default() -> Self {
        Self {
            hmd: PoseElement::IDENTITY,
            main_hand: PoseElement::IDENTITY,
            off_hand: PoseElement::IDENTITY,
            trackers: Trackers::default(),
            hands: Hands::default(),
        }
    }
}

impl PoseData {
    pub fn write(&self, w: &mut Writer) {
        self.hmd.write(w);
        self.main_hand.write(w);
        self.off_hand.write(w);
        self.trackers.write(w);
        self.hands.left.write(w);
        self.hands.right.write(w);
    }

    pub fn read(r: &mut Reader) -> Result<Self> {
        Ok(Self {
            hmd: PoseElement::read(r)?,
            main_hand: PoseElement::read(r)?,
            off_hand: PoseElement::read(r)?,
            trackers: Trackers::read(r)?,
            hands: Hands {
                left: HandJoints::read(r)?,
                right: HandJoints::read(r)?,
            },
        })
    }

    /// `ServerNetworking.getPoseDataForOthers`: the trackers and the joints stay at home
    /// unless the server says they go to others.
    pub fn for_others(&self, trackers: bool, hands: bool) -> Self {
        Self {
            trackers: if trackers {
                self.trackers
            } else {
                Trackers::default()
            },
            hands: if hands { self.hands } else { Hands::default() },
            ..*self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(hex: &str) -> Vec<u8> {
        let h: String = hex.split_whitespace().collect();
        (0..h.len() / 2)
            .map(|i| u8::from_str_radix(&h[i * 2..i * 2 + 2], 16).unwrap())
            .collect()
    }

    // Written by hand from PoseElementBuffer.serialize: x y z, then the quaternion x y z w.
    #[test]
    fn element_is_position_then_xyzw() {
        let e = PoseElement::new(
            Vec3::new(1.0, 2.0, 3.0),
            Quat::from_xyzw(0.5, -0.5, 0.25, 1.0),
        );
        let mut w = Writer::new();
        e.write(&mut w);
        assert_eq!(
            w.data,
            bytes("3f800000 40000000 40400000  3f000000 bf000000 3e800000 3f800000")
        );
    }

    // PoseTrackersBuffer: mask bit 0 is the waist, bit 13 the right shoulder; the mask
    // 0x2001 is the VarInt 81 40, and the elements follow in index order.
    #[test]
    fn trackers_mask_and_order() {
        let mut t = Trackers::default();
        t.0[TrackerKind::RightShoulder as usize] = Some(PoseElement::new(Vec3::X, Quat::IDENTITY));
        t.0[TrackerKind::Waist as usize] = Some(PoseElement::new(Vec3::Y, Quat::IDENTITY));
        let mut w = Writer::new();
        t.write(&mut w);
        assert_eq!(
            w.data,
            bytes(
                "81 40
                 00000000 3f800000 00000000  00000000 00000000 00000000 3f800000
                 3f800000 00000000 00000000  00000000 00000000 00000000 3f800000"
            )
        );
        assert_eq!(Trackers::read(&mut Reader::new(&w.data)).unwrap(), t);
    }

    // PoseHandBuffer: palm (bit 0) and little tip (bit 25): the mask 0x2000001 is the
    // VarInt 81 80 80 10.
    #[test]
    fn hand_joint_mask_is_a_26_bit_var_int() {
        let mut h = HandJoints::default();
        h.0[25] = Some(PoseElement::IDENTITY);
        h.0[0] = Some(PoseElement::IDENTITY);
        let mut w = Writer::new();
        h.write(&mut w);
        assert_eq!(&w.data[..4], &bytes("81 80 80 10")[..]);
        assert_eq!(w.data.len(), 4 + 2 * 28);
        assert_eq!(HandJoints::read(&mut Reader::new(&w.data)).unwrap(), h);
    }

    #[test]
    fn pose_data_without_trackers_or_joints_has_three_zero_masks() {
        let mut p = PoseData::default();
        p.hmd.position = Vec3::new(0.0, 1.5, 0.0);
        let mut w = Writer::new();
        p.write(&mut w);
        assert_eq!(w.data.len(), 3 * 28 + 3);
        assert_eq!(&w.data[..12], &bytes("00000000 3fc00000 00000000")[..]);
        assert_eq!(&w.data[3 * 28..], &[0, 0, 0]);
        assert_eq!(PoseData::read(&mut Reader::new(&w.data)).unwrap(), p);
    }

    #[test]
    fn for_others_strips_what_the_server_does_not_share() {
        let mut p = PoseData::default();
        p.trackers.0[0] = Some(PoseElement::IDENTITY);
        p.hands.left.0[3] = Some(PoseElement::IDENTITY);
        let stripped = p.for_others(false, false);
        assert!(stripped.trackers.is_empty() && stripped.hands.left.is_empty());
        let kept = p.for_others(true, true);
        assert_eq!(kept, p);
        assert!(p.for_others(true, false).hands.left.is_empty());
    }

    #[test]
    fn a_truncated_element_is_an_error() {
        assert!(PoseElement::read(&mut Reader::new(&[0; 27])).is_err());
    }
}
