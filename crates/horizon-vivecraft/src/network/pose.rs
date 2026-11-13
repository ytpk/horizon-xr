// SPDX-License-Identifier: LGPL-3.0-or-later
//! Ports `Pose`, `TrackerPose` and `VrPlayerState` of `org.vivecraft.common.network`.
//! Copyright (c) Vivecraft contributors; LGPL-3.0-or-later (see LICENSE-LGPL-3.0).

use super::{FbtMode, NetworkVersion, VrBodyPart};
use crate::wire::{Reader, Writer};
use crate::{Error, Result};
use glam::{Quat, Vec3};

/// `CommonNetworkHelper.serializeF`.
pub fn write_vec3(w: &mut Writer, v: Vec3) {
    w.float(v.x);
    w.float(v.y);
    w.float(v.z);
}

/// `CommonNetworkHelper.deserializeFVec3`.
pub fn read_vec3(r: &mut Reader) -> Result<Vec3> {
    Ok(Vec3::new(r.float()?, r.float()?, r.float()?))
}

/// `CommonNetworkHelper.serialize(Quaternionfc)`: w first, then x, y, z.
pub fn write_quat(w: &mut Writer, q: Quat) {
    w.float(q.w);
    w.float(q.x);
    w.float(q.y);
    w.float(q.z);
}

/// `CommonNetworkHelper.deserializeVivecraftQuaternion`.
pub fn read_quat(r: &mut Reader) -> Result<Quat> {
    let w = r.float()?;
    let x = r.float()?;
    let y = r.float()?;
    let z = r.float()?;
    Ok(Quat::from_xyzw(x, y, z, w))
}

/// A device pose: `position` is in player local space (relative to the player's feet
/// position), `orientation` in world space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub position: Vec3,
    pub orientation: Quat,
}

impl Pose {
    pub fn new(position: Vec3, orientation: Quat) -> Self {
        Self {
            position,
            orientation,
        }
    }

    pub fn serialize(&self, w: &mut Writer) {
        write_vec3(w, self.position);
        write_quat(w, self.orientation);
    }

    pub fn deserialize(r: &mut Reader) -> Result<Self> {
        Ok(Self {
            position: read_vec3(r)?,
            orientation: read_quat(r)?,
        })
    }

    /// The device's pointing direction: its orientation applied to `BACK` (0, 0, -1).
    pub fn direction(&self) -> Vec3 {
        self.orientation * crate::math::BACK
    }
}

/// `TrackerPose`: a tracker that has a position only.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrackerPose {
    pub position: Vec3,
}

impl TrackerPose {
    pub fn serialize(&self, w: &mut Writer) {
        write_vec3(w, self.position);
    }
    pub fn deserialize(r: &mut Reader) -> Result<Self> {
        Ok(Self {
            position: read_vec3(r)?,
        })
    }
}

/// Everything a player sends about their body (`VrPlayerState`).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct VrPlayerState {
    pub seated: bool,
    pub hmd: Pose,
    /// Left handed: `main_hand` is the left hand.
    pub left_handed: bool,
    pub main_hand: Pose,
    /// Same as `left_handed`, for legacy compatibility (it travels twice).
    pub reverse_hands_legacy: bool,
    pub off_hand: Pose,
    pub fbt_mode: FbtMode,
    pub waist: Option<Pose>,
    pub right_foot: Option<Pose>,
    pub left_foot: Option<Pose>,
    pub right_knee: Option<Pose>,
    pub left_knee: Option<Pose>,
    pub right_elbow: Option<Pose>,
    pub left_elbow: Option<Pose>,
}

impl Default for Pose {
    fn default() -> Self {
        Self {
            position: Vec3::ZERO,
            orientation: Quat::IDENTITY,
        }
    }
}

impl VrPlayerState {
    /// The `VrPlayerState(other, version)` constructor: strips what `version` cannot hold.
    pub fn stripped_for(&self, version: NetworkVersion) -> Self {
        if NetworkVersion::Fbt.accepts(version) {
            self.clone()
        } else {
            Self {
                fbt_mode: FbtMode::ArmsOnly,
                waist: None,
                right_foot: None,
                left_foot: None,
                right_knee: None,
                left_knee: None,
                right_elbow: None,
                left_elbow: None,
                ..self.clone()
            }
        }
    }

    /// `getBodyPartPose`: `None` for a part the FBT mode lacks.
    pub fn body_part_pose(&self, part: VrBodyPart) -> Option<Pose> {
        match part {
            VrBodyPart::MainHand => Some(self.main_hand),
            VrBodyPart::OffHand => Some(self.off_hand),
            VrBodyPart::LeftFoot => self.left_foot,
            VrBodyPart::RightFoot => self.right_foot,
            VrBodyPart::LeftElbow => self.left_elbow,
            VrBodyPart::RightElbow => self.right_elbow,
            VrBodyPart::LeftKnee => self.left_knee,
            VrBodyPart::RightKnee => self.right_knee,
            VrBodyPart::Waist => self.waist,
            VrBodyPart::Head => Some(self.hmd),
        }
    }

    /// `serialize`. The FBT part is only sent when the mode is not arms only.
    pub fn serialize(&self, w: &mut Writer) {
        w.bool(self.seated);
        self.hmd.serialize(w);
        w.bool(self.left_handed);
        self.main_hand.serialize(w);
        w.bool(self.left_handed);
        self.off_hand.serialize(w);
        if self.fbt_mode != FbtMode::ArmsOnly {
            w.byte(self.fbt_mode as u8);
            let zero = Pose::default();
            self.waist.unwrap_or(zero).serialize(w);
            self.right_foot.unwrap_or(zero).serialize(w);
            self.left_foot.unwrap_or(zero).serialize(w);
            if self.fbt_mode == FbtMode::WithJoints {
                self.right_knee.unwrap_or(zero).serialize(w);
                self.left_knee.unwrap_or(zero).serialize(w);
                self.right_elbow.unwrap_or(zero).serialize(w);
                self.left_elbow.unwrap_or(zero).serialize(w);
            }
        }
    }

    /// `deserialize`: `bytes_after` bytes of the buffer are left unread (the FBT part is
    /// there only when more than that remain).
    pub fn deserialize(r: &mut Reader, bytes_after: usize) -> Result<Self> {
        let seated = r.bool()?;
        let hmd = Pose::deserialize(r)?;
        let left_handed = r.bool()?;
        let main_hand = Pose::deserialize(r)?;
        let reverse_hands_legacy = r.bool()?;
        let off_hand = Pose::deserialize(r)?;
        let mut s = Self {
            seated,
            hmd,
            left_handed,
            main_hand,
            reverse_hands_legacy,
            off_hand,
            ..Self::default()
        };
        if r.remaining() > bytes_after {
            s.fbt_mode = FbtMode::from_wire(r.byte()?)
                .ok_or_else(|| Error::new("vivecraft payload: bad FBT mode"))?;
        }
        if s.fbt_mode != FbtMode::ArmsOnly {
            s.waist = Some(Pose::deserialize(r)?);
            s.right_foot = Some(Pose::deserialize(r)?);
            s.left_foot = Some(Pose::deserialize(r)?);
        }
        if s.fbt_mode == FbtMode::WithJoints {
            s.right_knee = Some(Pose::deserialize(r)?);
            s.left_knee = Some(Pose::deserialize(r)?);
            s.right_elbow = Some(Pose::deserialize(r)?);
            s.left_elbow = Some(Pose::deserialize(r)?);
        }
        Ok(s)
    }
}
