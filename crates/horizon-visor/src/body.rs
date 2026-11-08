// SPDX-License-Identifier: MPL-2.0
//! `VRBodyType` ids and the height numbers of `VRClientSettings` / `VRLocalPlayerImpl`.
//! Copyright (C) Phoenix-Ra and Visor contributors; MPL-2.0 (see LICENSE-MPL-2.0).

/// `VRPlayer.DEFAULT_FULL_HEIGHT`: the head pivot height of a Minecraft player, metres.
pub const DEFAULT_FULL_HEIGHT: f32 = 1.52;
/// `VRPlayer.DEFAULT_GUN_ANGLE`, degrees.
pub const DEFAULT_GUN_ANGLE: f32 = 60.0;
/// `VRClientSettings.MIN_HEIGHT`: a measured height below this is not a measurement.
pub const MIN_HEIGHT: f32 = DEFAULT_FULL_HEIGHT / 4.0;

/// `VRBodyType` (`getId`): what the client announces with `VR_BODY_TYPE` and what the
/// others render. Addons can register more; an id we do not know renders as the fallback.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BodyType {
    /// `VRBodyTypeHandsOnly.ID`; also `VRBodyType.FALLBACK_BODY_TYPE`.
    HandsOnly,
    /// `VRBodyTypeFull.ID`: hands, head and the trackers' body.
    FullBody,
    /// An id of an addon; rendered as [`BodyType::HandsOnly`].
    Other(String),
}

impl BodyType {
    pub const HANDS_ONLY_ID: &'static str = "hands_only";
    pub const FULL_BODY_ID: &'static str = "full_body";

    pub fn id(&self) -> &str {
        match self {
            Self::HandsOnly => Self::HANDS_ONLY_ID,
            Self::FullBody => Self::FULL_BODY_ID,
            Self::Other(s) => s,
        }
    }

    pub fn from_id(id: &str) -> Self {
        match id {
            Self::HANDS_ONLY_ID => Self::HandsOnly,
            Self::FULL_BODY_ID => Self::FullBody,
            other => Self::Other(other.to_string()),
        }
    }

    /// `VRRemotePlayerImpl.receivedBodyTypePacket`: an unknown id falls back to hands only.
    pub fn rendered(&self) -> &Self {
        match self {
            Self::Other(_) => &Self::HandsOnly,
            known => known,
        }
    }
}

/// `XrRawPoseHandler.extractGunAngle`: the pitch, degrees, of the grip's forward in the aim's
/// frame (`aimToGrip * (0, 0, -1)` as `atan2(y, -z)`). Touch controllers sit near 40 to 60.
pub fn gun_angle(grip: glam::Quat, aim: glam::Quat) -> f32 {
    gun_angle_of(aim.inverse() * grip)
}

/// `extractGunAngle` of an aim to grip turn.
pub fn gun_angle_of(aim_to_grip: glam::Quat) -> f32 {
    let forward = aim_to_grip * glam::Vec3::NEG_Z;
    forward.y.atan2(-forward.z).to_degrees()
}

/// `ClientNetworking.sendVRPlayerState`: the model height goes out rounded to a centimetre.
pub fn round_height(model_height: f32) -> f32 {
    (model_height * 100.0).round() / 100.0
}

/// `VRClientSettings.getModelHeightRatio` for the height mode `real_size` (or without the
/// camera offset adjustment): the measured height over the default.
pub fn model_height_ratio(measured: f32) -> f32 {
    measured / DEFAULT_FULL_HEIGHT
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_the_java_constants() {
        assert_eq!(BodyType::HandsOnly.id(), "hands_only");
        assert_eq!(BodyType::FullBody.id(), "full_body");
        assert_eq!(BodyType::from_id("full_body"), BodyType::FullBody);
        assert_eq!(BodyType::from_id("x:y").rendered(), &BodyType::HandsOnly);
    }

    #[test]
    fn the_gun_angle_is_the_pitch_of_the_grip_against_the_aim() {
        use glam::Quat;
        let aim = Quat::from_rotation_y(0.7);
        // A grip tipped up by 40 degrees about the aim's own right axis.
        let grip = aim * Quat::from_rotation_x(40f32.to_radians());
        assert!((gun_angle(grip, aim) - 40.0).abs() < 1e-3);
        assert!((gun_angle(aim, aim)).abs() < 1e-4);
        let down = aim * Quat::from_rotation_x((-25f32).to_radians());
        assert!((gun_angle(down, aim) + 25.0).abs() < 1e-3);
        // The turn itself: the default aim to grip turn (60 degrees about X) is a gun angle of 60,
        // which is what a runtime that gives the aim as the grip sends.
        let default = crate::item_pose::aim_to_grip(aim, aim);
        assert!((gun_angle_of(default) - DEFAULT_GUN_ANGLE).abs() < 1e-3);
    }

    #[test]
    fn the_height_goes_out_in_centimetres() {
        assert_eq!(round_height(1.7249), 1.72);
        assert_eq!(round_height(1.725), 1.73);
        assert_eq!(round_height(DEFAULT_FULL_HEIGHT), 1.52);
    }
}
