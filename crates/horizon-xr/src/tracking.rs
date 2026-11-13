// SPDX-License-Identifier: MPL-2.0
//! Hand and body tracking for Visor's `POSE_DATA` (`PoseHandsBuffer`, `PoseTrackersBuffer`):
//! `XR_EXT_hand_tracking` for the 26 joints of each hand and `XR_FB_body_tracking` (with
//! `XR_META_body_tracking_full_body` for the legs) for the 14 body trackers.
//! Copyright (C) Phoenix-Ra and Visor contributors for the joint orders; MPL-2.0.
//!
//! A tracker is made only while the game wants it (the player turned it on and the server takes
//! it) and the extension is there; it is dropped when the game stops asking. A failed creation
//! (no permission, an unsupported device) is logged once and not retried until the game asks
//! afresh. Nothing here is checked on a device.

use super::{quat, vec3};
use horizon_client_input::headset as input;
use openxr as xr;
use xr::sys::{self, Handle as _};

/// `XR_EXT_hand_tracking` is available.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Support {
    pub hands: bool,
    pub body: bool,
    pub full_body: bool,
}

impl Support {
    /// What the instance was made with and the system supports.
    pub fn of(instance: &xr::Instance, system: xr::SystemId) -> Self {
        let exts = instance.exts();
        Self {
            hands: exts.ext_hand_tracking.is_some()
                && instance.supports_hand_tracking(system).unwrap_or(false),
            body: exts.fb_body_tracking.is_some(),
            full_body: exts.fb_body_tracking.is_some()
                && exts.meta_body_tracking_full_body.is_some(),
        }
    }
}

/// `xrCreateBodyTrackerFB`'s handle and what destroys it.
struct BodyTracker {
    handle: sys::BodyTrackerFB,
    destroy: sys::pfn::DestroyBodyTrackerFB,
    full: bool,
}

impl Drop for BodyTracker {
    fn drop(&mut self) {
        // Safety: the handle was made by the same extension and is destroyed once.
        unsafe {
            (self.destroy)(self.handle);
        }
    }
}

/// How long after asking for a permission a refused tracker is tried again.
const RETRY: std::time::Duration = std::time::Duration::from_secs(6);

/// The trackers of a session.
pub(crate) struct Tracking {
    pub support: Support,
    hands: [Option<xr::HandTracker>; 2],
    body: Option<BodyTracker>,
    /// A creation failed since the game last asked: not retried until it stops asking.
    failed: [bool; 2],
    /// When a refused tracker is tried again, after the system's permission prompt had time to answer.
    retry_at: Option<std::time::Instant>,
    logged: bool,
}

impl Tracking {
    pub fn new(support: Support) -> Self {
        input::set_tracking_supported(support.hands, support.body);
        log::info!(
            "Horizon XR: hand tracking {}, body tracking {} (legs: {})",
            support.hands,
            support.body,
            support.full_body
        );
        Self {
            support,
            hands: [None, None],
            body: None,
            failed: [false; 2],
            retry_at: None,
            logged: false,
        }
    }

    /// Once per frame: runs or stops the trackers as the game wants and reads them in `space` at
    /// `time` into the input state.
    pub fn sync(&mut self, session: &xr::Session<xr::Vulkan>, space: &xr::Space, time: xr::Time) {
        let [want_hands, want_body] = input::tracking_wanted();
        if self
            .retry_at
            .is_some_and(|at| std::time::Instant::now() >= at)
        {
            self.retry_at = None;
            self.logged = false;
            self.failed = [false; 2];
        }
        let want_hands = want_hands && self.support.hands && !self.failed[0];
        let want_body = want_body && self.support.body && !self.failed[1];
        if !input::tracking_wanted()[0] {
            self.failed[0] = false;
        }
        if !input::tracking_wanted()[1] {
            self.failed[1] = false;
        }
        self.sync_hands(session, space, time, want_hands);
        self.sync_body(session, space, time, want_body);
    }

    fn sync_hands(
        &mut self,
        session: &xr::Session<xr::Vulkan>,
        space: &xr::Space,
        time: xr::Time,
        want: bool,
    ) {
        if !want {
            if self.hands.iter().any(Option::is_some) {
                self.hands = [None, None];
                input::set_hand_joints(0, None);
                input::set_hand_joints(1, None);
            }
            return;
        }
        for (i, hand) in [xr::Hand::LEFT, xr::Hand::RIGHT].into_iter().enumerate() {
            if self.hands[i].is_none() {
                match session.create_hand_tracker(hand) {
                    Ok(t) => self.hands[i] = Some(t),
                    Err(e) => {
                        log::warn!("Horizon XR: no hand tracker ({e}); hand tracking stays off");
                        self.failed[0] = true;
                        // The permission may be what is missing: ask, and try again in a few seconds.
                        super::permission::request(super::permission::HAND_TRACKING);
                        self.retry_at = Some(std::time::Instant::now() + RETRY);
                        return;
                    }
                }
            }
            let Some(tracker) = &self.hands[i] else {
                continue;
            };
            let joints = match space.locate_hand_joints(tracker, time) {
                Ok(Some(j)) => Some(j),
                Ok(None) | Err(_) => None,
            };
            input::set_hand_joints(
                i,
                joints.and_then(|j| {
                    let mut out = [(glam::Vec3::ZERO, glam::Quat::IDENTITY); input::HAND_JOINTS];
                    for (o, joint) in out.iter_mut().zip(j.iter()) {
                        // A joint that is not valid keeps the wrist's place: the whole hand
                        // is sent or none of it.
                        if !joint.location_flags.contains(
                            xr::SpaceLocationFlags::POSITION_VALID
                                | xr::SpaceLocationFlags::ORIENTATION_VALID,
                        ) {
                            return None;
                        }
                        *o = (vec3(joint.pose.position), quat(joint.pose.orientation));
                    }
                    Some(out)
                }),
            );
        }
    }

    fn sync_body(
        &mut self,
        session: &xr::Session<xr::Vulkan>,
        space: &xr::Space,
        time: xr::Time,
        want: bool,
    ) {
        if !want {
            if self.body.take().is_some() {
                input::set_body_trackers(None);
            }
            return;
        }
        let instance = session.instance();
        let Some(fp) = instance.exts().fb_body_tracking.as_ref() else {
            return;
        };
        if self.body.is_none() {
            // The full body set (with legs) first; a device without it takes the default set.
            let mut created = None;
            let mut last = sys::Result::SUCCESS;
            for full in [self.support.full_body, false] {
                if !full && created.is_some() {
                    break;
                }
                let info = sys::BodyTrackerCreateInfoFB {
                    ty: sys::BodyTrackerCreateInfoFB::TYPE,
                    next: std::ptr::null(),
                    body_joint_set: if full {
                        sys::BodyJointSetFB::FULL_BODY_M
                    } else {
                        sys::BodyJointSetFB::DEFAULT
                    },
                };
                let mut handle = sys::BodyTrackerFB::NULL;
                // Safety: the session is live and the info is a valid struct of the extension.
                let result =
                    unsafe { (fp.create_body_tracker)(session.as_raw(), &info, &mut handle) };
                if result.into_raw() >= 0 {
                    created = Some(BodyTracker {
                        handle,
                        destroy: fp.destroy_body_tracker,
                        full,
                    });
                    break;
                }
                last = result;
                if !full {
                    break;
                }
            }
            let Some(tracker) = created else {
                if !self.logged {
                    log::warn!(
                        "Horizon XR: no body tracker ({last:?}; the body tracking permission \
                         may be missing); body tracking stays off"
                    );
                    self.logged = true;
                }
                self.failed[1] = true;
                super::permission::request(super::permission::BODY_TRACKING);
                self.retry_at = Some(std::time::Instant::now() + RETRY);
                return;
            };
            log::info!(
                "Horizon XR: body tracker made ({} joints set)",
                if tracker.full { "full body" } else { "default" }
            );
            self.body = Some(tracker);
        }
        let Some(tracker) = &self.body else {
            return;
        };
        let count = if tracker.full {
            sys::FullBodyJointMETA::COUNT.into_raw() as usize
        } else {
            sys::BodyJointFB::COUNT.into_raw() as usize
        };
        let mut joints = vec![sys::BodyJointLocationFB::default(); count];
        let info = sys::BodyJointsLocateInfoFB {
            ty: sys::BodyJointsLocateInfoFB::TYPE,
            next: std::ptr::null(),
            base_space: space.as_raw(),
            time,
        };
        let mut out = sys::BodyJointLocationsFB {
            ty: sys::BodyJointLocationsFB::TYPE,
            next: std::ptr::null_mut(),
            is_active: false.into(),
            confidence: 0.0,
            joint_count: count as u32,
            joint_locations: joints.as_mut_ptr(),
            skeleton_changed_count: 0,
            time: xr::Time::from_nanos(0),
        };
        // Safety: `joints` holds `joint_count` entries and outlives the call.
        let result = unsafe { (fp.locate_body_joints)(tracker.handle, &info, &mut out) };
        if result.into_raw() < 0 || !bool::from(out.is_active) {
            input::set_body_trackers(None);
            return;
        }
        input::set_body_trackers(Some(body_trackers(&joints, tracker.full)));
    }
}

/// The 14 trackers of Visor's mask from the joints of the body set: the hips, chest, the
/// shoulders, elbows (the start of the lower arm) and wrists, and with the full body set the
/// knees (the start of the lower leg), ankles and feet (the ball).
fn body_trackers(
    joints: &[sys::BodyJointLocationFB],
    full: bool,
) -> [Option<input::Joint>; input::BODY_TRACKERS] {
    use sys::BodyJointFB as J;
    use sys::FullBodyJointMETA as F;
    let valid = xr::SpaceLocationFlags::POSITION_VALID | xr::SpaceLocationFlags::ORIENTATION_VALID;
    let at = |i: i32| -> Option<input::Joint> {
        let j = joints.get(usize::try_from(i).ok()?)?;
        j.location_flags
            .contains(valid)
            .then(|| (vec3(j.pose.position), quat(j.pose.orientation)))
    };
    let leg = |i: i32| if full { at(i) } else { None };
    [
        at(J::HIPS.into_raw()),
        at(J::CHEST.into_raw()),
        leg(F::LEFT_FOOT_BALL.into_raw()),
        leg(F::RIGHT_FOOT_BALL.into_raw()),
        leg(F::LEFT_FOOT_ANKLE.into_raw()),
        leg(F::RIGHT_FOOT_ANKLE.into_raw()),
        leg(F::LEFT_LOWER_LEG.into_raw()),
        leg(F::RIGHT_LOWER_LEG.into_raw()),
        at(J::LEFT_HAND_WRIST_TWIST.into_raw()),
        at(J::RIGHT_HAND_WRIST_TWIST.into_raw()),
        at(J::LEFT_ARM_LOWER.into_raw()),
        at(J::RIGHT_ARM_LOWER.into_raw()),
        at(J::LEFT_SHOULDER.into_raw()),
        at(J::RIGHT_SHOULDER.into_raw()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn joint(i: usize, x: f32, joints: &mut [sys::BodyJointLocationFB]) {
        joints[i].location_flags =
            xr::SpaceLocationFlags::POSITION_VALID | xr::SpaceLocationFlags::ORIENTATION_VALID;
        joints[i].pose.position = xr::Vector3f { x, y: 0.0, z: 0.0 };
        joints[i].pose.orientation = xr::Quaternionf {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            w: 1.0,
        };
    }

    // The joint numbers of XrBodyJointFB / XrFullBodyJointMETA in the OpenXR registry:
    // hips 1, chest 5, left shoulder 8, left lower arm 11, left wrist twist 12, right
    // shoulder 13, right lower arm 16, right wrist twist 17; legs 71 left lower leg, 73 left
    // ankle, 76 left ball, 78 right lower leg, 80 right ankle, 83 right ball.
    #[test]
    fn trackers_come_from_the_registry_numbers() {
        let mut j = vec![sys::BodyJointLocationFB::default(); 84];
        for (i, x) in [
            (1, 1.0),
            (5, 5.0),
            (8, 8.0),
            (11, 11.0),
            (12, 12.0),
            (13, 13.0),
            (16, 16.0),
            (17, 17.0),
            (71, 71.0),
            (73, 73.0),
            (76, 76.0),
            (78, 78.0),
            (80, 80.0),
            (83, 83.0),
        ] {
            joint(i, x, &mut j);
        }
        let t = body_trackers(&j, true);
        let xs: Vec<f32> = t.iter().map(|t| t.map_or(-1.0, |t| t.0.x)).collect();
        // waist, chest, L/R foot, L/R ankle, L/R knee, L/R wrist, L/R elbow, L/R shoulder
        assert_eq!(
            xs,
            [
                1.0, 5.0, 76.0, 83.0, 73.0, 80.0, 71.0, 78.0, 12.0, 17.0, 11.0, 16.0, 8.0, 13.0
            ]
        );
        // The default set has no legs.
        let t = body_trackers(&j[..70], false);
        assert!(t[2..8].iter().all(Option::is_none));
        assert!(t[0].is_some() && t[13].is_some());
    }

    #[test]
    fn a_joint_that_is_not_valid_is_left_out() {
        let mut j = vec![sys::BodyJointLocationFB::default(); 70];
        joint(1, 1.0, &mut j);
        j[5].location_flags = xr::SpaceLocationFlags::POSITION_VALID;
        let t = body_trackers(&j, false);
        assert!(t[0].is_some());
        assert!(t[1].is_none());
    }
}
