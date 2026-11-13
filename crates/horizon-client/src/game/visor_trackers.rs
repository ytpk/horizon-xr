// SPDX-License-Identifier: MPL-2.0
//! Hand joints and body trackers of the headset in Visor's `POSE_DATA`.
//!
//! Ports `PoseHandsBuffer.create` and `PoseTrackersBuffer.create` (Visor 0.6.1,
//! https://github.com/VisorModStudio/Visor, commit c01354fad8ac03a016c249239a04b07adeb36476):
//! joints go out only when the player turned them on, the headset has them and the server takes
//! them, in world orientation and relative to the player's position. Copyright (C) Phoenix-Ra
//! and Visor contributors; MPL-2.0, see crates/horizon-visor/LICENSE-MPL-2.0.
use super::Game;
use glam::{Quat, Vec3};
use horizon_client::headset;
use horizon_visor::buffer::{HandJoints, PoseData, PoseElement, Trackers};

impl Game<'_> {
    /// What the XR layer should run this tick: the hand joints and the body trackers are wanted
    /// only while the player asked for them and a Visor server takes them (so a tracker the
    /// server ignores costs nothing).
    pub(super) fn visor_tracking_wanted(&self) -> [bool; 2] {
        if !self.visor_active() || !headset::active() {
            return [false; 2];
        }
        let settings = headset::settings();
        let server = self.vr_server_settings();
        [
            settings.bool("visorHandTracking") && server.hand_trackers_supported,
            settings.bool("visorBodyTracking") && server.body_trackers_supported,
        ]
    }

    /// Fills the joints and trackers of `data` from the headset (`PoseTrackersBuffer.create`,
    /// `PoseHandsBuffer.create`), and tells the XR layer what is wanted.
    pub(super) fn visor_tracked_body(&self, data: &mut PoseData) {
        let [hands, body] = self.visor_tracking_wanted();
        headset::set_tracking_wanted(hands, body);
        if !hands && !body {
            return;
        }
        let Some(feet) = self.player.as_ref().map(|p| p.position) else {
            return;
        };
        let world = self.pad.headset.vr.player.world_pre;
        let rotation = Quat::from_rotation_y(world.rotation);
        let element = |(pos, rot): (Vec3, Quat)| {
            let at = world.room_to_world(pos);
            PoseElement::new(
                Vec3::new(
                    (at.x - feet.x) as f32,
                    (at.y - feet.y) as f32,
                    (at.z - feet.z) as f32,
                ),
                (rotation * rot).normalize(),
            )
        };
        if hands {
            let left_handed = headset::settings().left_handed();
            // Physical left and right: the main hand is the right unless left handed.
            let convert = |joints: Option<[headset::Joint; headset::HAND_JOINTS]>| {
                joints.map_or_else(HandJoints::default, |j| {
                    HandJoints(j.map(|joint| Some(element(joint))))
                })
            };
            data.hands.left = convert(headset::hand_joints(left_handed));
            data.hands.right = convert(headset::hand_joints(!left_handed));
        }
        if body && let Some(trackers) = headset::body_trackers() {
            data.trackers = Trackers(trackers.map(|t| t.map(element)));
        }
    }
}
