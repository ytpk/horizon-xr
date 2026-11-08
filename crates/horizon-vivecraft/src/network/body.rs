// SPDX-License-Identifier: LGPL-3.0-or-later
//! Ports `org.vivecraft.api.data.VRBodyPart` and `FBTMode`.
//! Copyright (c) Vivecraft contributors; LGPL-3.0-or-later (see LICENSE-LGPL-3.0).

/// `FBTMode`: what trackers a player state holds (the ordinal is on the wire).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum FbtMode {
    #[default]
    ArmsOnly,
    ArmsLegs,
    WithJoints,
}

impl FbtMode {
    pub fn from_wire(b: i8) -> Option<Self> {
        match b {
            0 => Some(Self::ArmsOnly),
            1 => Some(Self::ArmsLegs),
            2 => Some(Self::WithJoints),
            _ => None,
        }
    }
}

/// `VRBodyPart` (the ordinal is on the wire).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum VrBodyPart {
    #[default]
    MainHand,
    OffHand,
    RightFoot,
    LeftFoot,
    Waist,
    RightKnee,
    LeftKnee,
    RightElbow,
    LeftElbow,
    Head,
}

impl VrBodyPart {
    pub fn from_wire(b: i8) -> Option<Self> {
        use VrBodyPart::*;
        Some(match b {
            0 => MainHand,
            1 => OffHand,
            2 => RightFoot,
            3 => LeftFoot,
            4 => Waist,
            5 => RightKnee,
            6 => LeftKnee,
            7 => RightElbow,
            8 => LeftElbow,
            9 => Head,
            _ => return None,
        })
    }

    pub fn opposite(self) -> Self {
        use VrBodyPart::*;
        match self {
            MainHand => OffHand,
            OffHand => MainHand,
            RightFoot => LeftFoot,
            LeftFoot => RightFoot,
            RightKnee => LeftKnee,
            LeftKnee => RightKnee,
            RightElbow => LeftElbow,
            LeftElbow => RightElbow,
            other => other,
        }
    }

    pub fn available_in_mode(self, mode: FbtMode) -> bool {
        use VrBodyPart::*;
        match self {
            MainHand | OffHand | Head => true,
            RightFoot | LeftFoot | Waist => mode != FbtMode::ArmsOnly,
            RightKnee | LeftKnee | RightElbow | LeftElbow => mode == FbtMode::WithJoints,
        }
    }

    pub fn is_hand(self) -> bool {
        matches!(self, Self::MainHand | Self::OffHand)
    }

    pub fn is_foot(self) -> bool {
        matches!(self, Self::RightFoot | Self::LeftFoot)
    }
}
