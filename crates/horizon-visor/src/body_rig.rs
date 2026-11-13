// SPDX-License-Identifier: MPL-2.0
// Uses Visor's joint numbering: api/.../VRHandJointType.java, api/.../VRBodyPartType.java (not a renderer port, see below).
//! Fingers and legs from the joints other players send (`PoseHandsBuffer`, `PoseTrackersBuffer`).
//! The joint numbering is `XR_EXT_hand_tracking`'s and `VRHandJointType`'s. Copyright (C)
//! Phoenix-Ra and Visor contributors; MPL-2.0 (see LICENSE-MPL-2.0).
//!
//! Not a port of a Visor renderer: Visor's `VRBodyRendererFull` / `VRPlayerModelFull` draw the
//! vanilla player model with the arms aimed at the controllers and the head at the headset, and
//! nothing else; the received joints and trackers only feed `GameEffectTrackerDebug` (off, local
//! player only). So Horizon draws no fingers or legs either (faithful); these angles are the
//! tested maths for a model that has the bones, and nothing in the renderer calls them.

use crate::buffer::{HandJoints, TrackerKind, Trackers};
use glam::Vec3;

/// `XR_EXT_hand_tracking`'s joint numbers of each finger's bones, base to tip: the thumb has
/// four (metacarpal, proximal, distal, tip), the others five (metacarpal, proximal, intermediate,
/// distal, tip).
pub const FINGER_JOINTS: [&[usize]; 5] = [
    &[2, 3, 4, 5],
    &[6, 7, 8, 9, 10],
    &[11, 12, 13, 14, 15],
    &[16, 17, 18, 19, 20],
    &[21, 22, 23, 24, 25],
];
const WRIST: usize = 1;

/// A finger's flexion at its three joints, radians, positive towards the palm: for the thumb
/// the metacarpal-proximal, proximal-distal and distal-tip bends; for the others the knuckle, the
/// middle joint and the last joint.
pub type Curl = [f32; 3];

/// Each finger's curl (thumb, index, middle, ring, little), or `None` for a finger that has a
/// joint missing. `palm_normal` is the direction the palm faces in the same space as the joints;
/// a bend towards it is positive.
pub fn finger_curls(joints: &HandJoints, palm_normal: Vec3) -> [Option<Curl>; 5] {
    let mut out = [None; 5];
    for (finger, ids) in FINGER_JOINTS.iter().enumerate() {
        let positions: Option<Vec<Vec3>> = ids
            .iter()
            .map(|&j| joints.0[j].map(|p| p.position))
            .collect();
        let Some(positions) = positions else {
            continue;
        };
        let bones: Vec<Vec3> = positions
            .windows(2)
            .map(|w| (w[1] - w[0]).normalize_or_zero())
            .collect();
        // The first bone is the metacarpal: the knuckle bends between it and the next. With four
        // bones (the other fingers) the last three bends are the joints; the thumb has three
        // bones and bends at the two between them plus the base, taken from the wrist.
        let mut curl = [0.0; 3];
        let bend = |a: Vec3, b: Vec3| {
            let angle = a.dot(b).clamp(-1.0, 1.0).acos();
            // The side the bend goes to: towards the palm if the turn from `a` to `b` points the
            // way the palm faces.
            let side = a.cross(b);
            if side.dot(palm_normal.cross(a)) >= 0.0 {
                -angle
            } else {
                angle
            }
        };
        if let Some(wrist) = joints.0[WRIST] {
            let base = (positions[0] - wrist.position).normalize_or_zero();
            if ids.len() == 4 {
                curl = [
                    bend(base, bones[0]),
                    bend(bones[0], bones[1]),
                    bend(bones[1], bones[2]),
                ];
            } else {
                curl = [
                    bend(bones[0], bones[1]),
                    bend(bones[1], bones[2]),
                    bend(bones[2], bones[3]),
                ];
            }
        }
        out[finger] = Some(curl);
    }
    out
}

/// One leg's angles, radians.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LegAngles {
    /// The thigh's swing from hanging straight down: positive forward (towards `forward`), and
    /// sideways (positive outward).
    pub thigh_pitch: f32,
    pub thigh_roll: f32,
    /// The knee's flexion: 0 straight, positive bends the shin back.
    pub knee_bend: f32,
}

/// The legs from the body trackers, left then right: the hip is the waist tracker pushed
/// `hip_width / 2` to the side, the knee and ankle (or foot) trackers give the thigh and shin.
/// `forward` is the way the body faces (horizontal, unit); `None` where a tracker is missing.
pub fn legs(trackers: &Trackers, forward: Vec3, hip_width: f32) -> [Option<LegAngles>; 2] {
    let right = forward.cross(Vec3::Y).normalize_or_zero();
    let waist = trackers.get(TrackerKind::Waist).map(|w| w.position);
    let leg = |knee: TrackerKind, ankle: TrackerKind, foot: TrackerKind, side: f32| {
        let waist = waist?;
        let knee = trackers.get(knee)?.position;
        let ankle = trackers.get(ankle).or_else(|| trackers.get(foot))?.position;
        let hip = waist + right * (side * hip_width * 0.5);
        let thigh = (knee - hip).normalize_or_zero();
        let shin = (ankle - knee).normalize_or_zero();
        if thigh == Vec3::ZERO || shin == Vec3::ZERO {
            return None;
        }
        let pitch = thigh.dot(forward).atan2(-thigh.y);
        let roll = (thigh.dot(right) * side).atan2(-thigh.y);
        let bend = thigh.dot(shin).clamp(-1.0, 1.0).acos();
        Some(LegAngles {
            thigh_pitch: pitch,
            thigh_roll: roll,
            knee_bend: bend,
        })
    };
    [
        leg(
            TrackerKind::LeftKnee,
            TrackerKind::LeftAnkle,
            TrackerKind::LeftFoot,
            -1.0,
        ),
        leg(
            TrackerKind::RightKnee,
            TrackerKind::RightAnkle,
            TrackerKind::RightFoot,
            1.0,
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::{HAND_JOINT_COUNT, PoseElement};
    use glam::Quat;

    fn at(p: Vec3) -> Option<PoseElement> {
        Some(PoseElement::new(p, Quat::IDENTITY))
    }

    /// A hand with the wrist at the origin, the fingers along +Z and the palm facing -Y; the index
    /// bends down (towards the palm) by `bend` at each of its joints.
    fn hand(bend: f32) -> HandJoints {
        let mut j = [None; HAND_JOINT_COUNT];
        j[WRIST] = at(Vec3::ZERO);
        let mut dir = Vec3::Z;
        let mut p = Vec3::new(0.0, 0.0, 0.05);
        for (k, &id) in FINGER_JOINTS[1].iter().enumerate() {
            j[id] = at(p);
            // The metacarpal runs straight; the bones after it each turn by `bend`.
            if k >= 1 {
                dir = Quat::from_rotation_x(bend) * dir;
            }
            p += dir * 0.03;
        }
        HandJoints(j)
    }

    #[test]
    fn a_straight_finger_does_not_bend_and_a_curled_one_bends_at_each_joint() {
        let palm = Vec3::NEG_Y;
        let flat = finger_curls(&hand(0.0), palm)[1].unwrap();
        assert!(flat.iter().all(|c| c.abs() < 1e-4), "{flat:?}");
        let curled = finger_curls(&hand(0.5), palm)[1].unwrap();
        // Turned about +X the fingertip dips to -Y, towards the palm: three joints of 0.5 each.
        assert!(
            curled.iter().all(|c| (c.abs() - 0.5).abs() < 1e-3),
            "{curled:?}"
        );
        // Every curl has the same sign.
        assert!(curled.iter().all(|c| c.signum() == curled[0].signum()));
        // The other way flips it, and a finger with a joint missing has no curl.
        let back = finger_curls(&hand(-0.5), palm)[1].unwrap();
        assert!(back.iter().all(|c| c.signum() == -curled[0].signum()));
        let mut holey = hand(0.0);
        holey.0[FINGER_JOINTS[2][1]] = None;
        assert!(finger_curls(&holey, palm)[2].is_none());
    }

    fn tracked(parts: &[(TrackerKind, Vec3)]) -> Trackers {
        let mut t = Trackers::default();
        for (kind, p) in parts {
            t.0[*kind as usize] = at(*p);
        }
        t
    }

    #[test]
    fn a_leg_straight_down_is_straight_and_a_raised_knee_swings_the_thigh_forward() {
        let forward = Vec3::NEG_Z;
        let waist = Vec3::new(0.0, 1.0, 0.0);
        let down = tracked(&[
            (TrackerKind::Waist, waist),
            (TrackerKind::RightKnee, Vec3::new(0.1, 0.5, 0.0)),
            (TrackerKind::RightAnkle, Vec3::new(0.1, 0.0, 0.0)),
            (TrackerKind::LeftKnee, Vec3::new(-0.1, 0.5, 0.0)),
            (TrackerKind::LeftFoot, Vec3::new(-0.1, 0.0, 0.0)),
        ]);
        let [left, right] = legs(&down, forward, 0.2);
        let right = right.unwrap();
        assert!(right.thigh_pitch.abs() < 1e-4 && right.knee_bend.abs() < 1e-3);
        assert!(
            left.unwrap().knee_bend.abs() < 1e-3,
            "a foot tracker stands in for the ankle"
        );
        // A knee raised in front, the shin hanging: 90 degrees of thigh pitch and of knee bend.
        let step = tracked(&[
            (TrackerKind::Waist, waist),
            (TrackerKind::RightKnee, Vec3::new(0.1, 1.0, -0.5)),
            (TrackerKind::RightAnkle, Vec3::new(0.1, 0.5, -0.5)),
        ]);
        let right = legs(&step, forward, 0.2)[1].unwrap();
        assert!(
            (right.thigh_pitch - std::f32::consts::FRAC_PI_2).abs() < 1e-3,
            "{right:?}"
        );
        assert!((right.knee_bend - std::f32::consts::FRAC_PI_2).abs() < 1e-3);
        // Without a waist or a knee tracker there is no leg.
        assert_eq!(legs(&Trackers::default(), forward, 0.2), [None, None]);
    }
}
