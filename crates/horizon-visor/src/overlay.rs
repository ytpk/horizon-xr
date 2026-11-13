// SPDX-License-Identifier: MPL-2.0
// Ports Visor: core/client/gui/overlays/templates/VROverlayChat.java, api/client/gui/overlays/VROverlay.java (handles),
// api/client/gui/overlays/framework/VROverlayFrameBuffer.java (dragging and resizing), builtin/VROverlayGameScreen.java.
//! Visor's overlays as numbers and small state machines: where the chat hangs, where the handles
//! of a screen are and what a hand does with them. Copyright (C) Phoenix-Ra and Visor
//! contributors; MPL-2.0 (see LICENSE-MPL-2.0).

/// `VROverlayChat`: the offset of its pose from the head, metres (right, up, forward is -Z).
pub const CHAT_OFFSET: [f32; 3] = [0.392, 0.214, -1.706];
/// `VROverlayChat`: its scale.
pub const CHAT_SCALE: f32 = 1.2;
/// Visor's overlays are GUIs 427 pixels to the metre of their scale (`PIXELS_PER_METRE` of the
/// hotbar wheel): a GUI this many pixels across is `scale` metres wide.
pub const GUI_PIXELS_ACROSS: f32 = 427.0;

use glam::{Quat, Vec3};

// ---- The handles of a screen (`VROverlay.isCursorOnDragHandle`, `isCursorOnResizeHandle`) ----
//
// Below the bottom edge of a screen, outside its picture, is a strip 0.15 of the screen's height
// tall. A click in it with the main button grabs: on the left part of the strip (up to where the
// bar's centre half width plus a gap ends) the screen is dragged along with the hand, on the small
// square after it the screen is resized by the distance between the hands.

/// Where the strip ends below the picture, as a share of the screen's height (`rawBottom`).
pub const STRIP_HEIGHT: f32 = 0.15;
/// The drag bar's centre and half width, as shares of the screen's width (`barHalfW = 0.18`).
pub const BAR_CENTRE: f32 = 0.5;
pub const BAR_HALF_WIDTH: f32 = 0.18;
/// Where the resize square starts and ends (`left`, `left + barHalfW * 0.55`), shares of the width.
pub const RESIZE_START: f32 = BAR_CENTRE + BAR_HALF_WIDTH + BAR_HALF_WIDTH * 0.20;
pub const RESIZE_END: f32 = RESIZE_START + BAR_HALF_WIDTH * 0.55;
/// `getMinScale` and `getMaxScale`: a screen's size while resized, in metres across.
pub const MIN_SCALE: f32 = 0.25;
pub const MAX_SCALE: f32 = 4.0;

/// What a click in the strip grabs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle {
    Drag,
    Resize,
}

/// The handle under a point of the screen's plane: `u` across (0 left to 1 right) and `v` down
/// (0 top, 1 the bottom edge) as the ray meets it, not limited to the picture. `resizing` is
/// whether the screen supports being resized (`supportsResizing`).
pub fn handle_at(u: f32, v: f32, resizing: bool) -> Option<Handle> {
    if v <= 1.0 || v > 1.0 + STRIP_HEIGHT {
        return None;
    }
    let zone_end = if resizing { RESIZE_START } else { 1.0 };
    if (0.0..=zone_end).contains(&u) {
        return Some(Handle::Drag);
    }
    (resizing && (RESIZE_START..=RESIZE_END).contains(&u)).then_some(Handle::Resize)
}

/// What the handles look like in the strip, as shares of the strip's width and height
/// (`RenderGuiHelper.drawHandles`): the drag bar, then the resize square, each `[left, top,
/// right, bottom]`.
pub fn handle_rects(resizing: bool) -> (Vec<[f32; 4]>, f32) {
    // In units of the screen's height H (the width is H * 16 / 9): the bar is 0.025 H tall, 0.02 H
    // below the screen; the square 0.0325 H each way, centred on the bar's row.
    let aspect = 9.0 / 16.0;
    let top = |y: f32| (y - 0.0) / STRIP_HEIGHT;
    let bar_centre_y = 0.02 + 0.0125;
    let mut rects = vec![[
        BAR_CENTRE - BAR_HALF_WIDTH,
        top(bar_centre_y - 0.0125),
        BAR_CENTRE + BAR_HALF_WIDTH,
        top(bar_centre_y + 0.0125),
    ]];
    if resizing {
        let side = 0.0125 * 1.3;
        rects.push([
            RESIZE_START,
            top(bar_centre_y - side),
            RESIZE_START + 2.0 * side * aspect,
            top(bar_centre_y + side),
        ]);
    }
    (rects, STRIP_HEIGHT)
}

/// `startDragging`: the screen's place in the dragging hand's frame, kept while the hand holds it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Drag {
    offset: Vec3,
    rotation: Quat,
}

impl Drag {
    /// The screen is at `pos` turned `rot`; the hand at `hand_pos` turned `hand_rot` grabs it in a
    /// room scaled by `world_scale`.
    pub fn start(hand_pos: Vec3, hand_rot: Quat, pos: Vec3, rot: Quat, world_scale: f32) -> Self {
        let inverse = hand_rot.inverse();
        Self {
            offset: inverse * (pos - hand_pos) / world_scale,
            rotation: inverse * rot,
        }
    }

    /// `applyForcedPose`: where the screen is for the hand now.
    pub fn pose(&self, hand_pos: Vec3, hand_rot: Quat, world_scale: f32) -> (Vec3, Quat) {
        (
            hand_pos + hand_rot * (self.offset * world_scale),
            hand_rot * self.rotation,
        )
    }
}

/// `startResizing`: the distance between the hands' grips and the screen's scale then.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Resize {
    distance: f32,
    scale: f32,
}

impl Resize {
    pub fn start(grip: Vec3, other_grip: Vec3, scale: f32) -> Self {
        Self {
            distance: grip.distance(other_grip),
            scale,
        }
    }

    /// `applyResizePose`: the scale now, from the hands' distance, kept between [`MIN_SCALE`] and
    /// [`MAX_SCALE`]; unchanged while either distance is nil.
    pub fn scale_now(&self, grip: Vec3, other_grip: Vec3) -> f32 {
        let now = grip.distance(other_grip);
        if self.distance > 1.0e-4 && now > 1.0e-4 {
            (self.scale * now / self.distance).clamp(MIN_SCALE, MAX_SCALE)
        } else {
            self.scale
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_chat_hangs_where_visors_template_puts_it() {
        assert_eq!(CHAT_OFFSET, [0.392, 0.214, -1.706]);
        assert_eq!(CHAT_SCALE, 1.2);
    }

    #[test]
    fn the_strip_below_a_screen_grabs_by_its_zones() {
        // Inside the picture or far below: nothing.
        assert_eq!(handle_at(0.5, 0.9, true), None);
        assert_eq!(handle_at(0.5, 1.0, true), None);
        assert_eq!(handle_at(0.5, 1.2, true), None);
        // The left of the strip up to the resize zone drags, the square after it resizes.
        assert_eq!(handle_at(0.0, 1.05, true), Some(Handle::Drag));
        assert_eq!(handle_at(0.7, 1.15, true), Some(Handle::Drag));
        assert_eq!(handle_at(0.72, 1.1, true), Some(Handle::Resize));
        assert_eq!(handle_at(0.81, 1.1, true), Some(Handle::Resize));
        assert_eq!(handle_at(0.9, 1.1, true), None);
        // A screen that cannot be resized drags along the whole strip.
        assert_eq!(handle_at(0.9, 1.1, false), Some(Handle::Drag));
        assert_eq!(handle_at(1.01, 1.1, false), None);
    }

    #[test]
    fn dragging_keeps_the_screen_in_the_hands_frame() {
        let (pos, rot) = (Vec3::new(0.0, 1.5, -1.0), Quat::IDENTITY);
        let (hand, turn) = (Vec3::new(0.3, 1.2, -0.4), Quat::from_rotation_y(0.3));
        let drag = Drag::start(hand, turn, pos, rot, 1.0);
        // The hand where it was: the screen has not moved.
        let (p, r) = drag.pose(hand, turn, 1.0);
        assert!((p - pos).length() < 1e-5 && r.dot(rot).abs() > 1.0 - 1e-5);
        // The hand moves 0.5 m up and a quarter turn about Y: the screen swings with it.
        let moved = Vec3::new(0.3, 1.7, -0.4);
        let quarter = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2) * turn;
        let (p, r) = drag.pose(moved, quarter, 1.0);
        let expected_offset = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2) * (pos - hand);
        assert!((p - (moved + expected_offset)).length() < 1e-5, "{p}");
        assert!(
            r.dot(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2))
                .abs()
                > 1.0 - 1e-5
        );
    }

    #[test]
    fn resizing_follows_the_distance_between_the_hands_within_limits() {
        let resize = Resize::start(Vec3::ZERO, Vec3::new(0.4, 0.0, 0.0), 1.0);
        assert!((resize.scale_now(Vec3::ZERO, Vec3::new(0.8, 0.0, 0.0)) - 2.0).abs() < 1e-6);
        assert!((resize.scale_now(Vec3::ZERO, Vec3::new(0.2, 0.0, 0.0)) - 0.5).abs() < 1e-6);
        assert_eq!(
            resize.scale_now(Vec3::ZERO, Vec3::new(10.0, 0.0, 0.0)),
            MAX_SCALE
        );
        assert_eq!(
            resize.scale_now(Vec3::ZERO, Vec3::new(0.01, 0.0, 0.0)),
            MIN_SCALE
        );
        // Hands together at the start or now: the scale stays.
        let flat = Resize::start(Vec3::ZERO, Vec3::ZERO, 1.5);
        assert_eq!(flat.scale_now(Vec3::ZERO, Vec3::X), 1.5);
        assert_eq!(resize.scale_now(Vec3::X, Vec3::X), 1.0);
    }
}
