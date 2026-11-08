// SPDX-License-Identifier: MPL-2.0
// Ports Visor: api/client/gui/overlays/framework/VROverlayFrameBuffer.java (startDragging, stopDragging, startResizing, applyResizePose), builtin/VROverlayGameScreen.java.
//! Dragging and resizing a screen's panel by the handles under it. Copyright (C) Phoenix-Ra and
//! Visor contributors; MPL-2.0 (see LICENSE-MPL-2.0).
//!
//! A trigger that goes down with the hand's ray on a handle grabs it. A drag keeps the panel in
//! the hand's frame (`Drag`), a resize sets the panel's width by the distance between the hands'
//! grips (`Resize`); letting go of the trigger ends it, and a resize reports the width for the game
//! to keep (`OverlayOptionsResizing.save`).
use crate::panel::Panel;
use glam::{Quat, Vec3};
use horizon_visor::overlay::{Drag, Handle, Resize, handle_at};

/// What a hand holds of the screen.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Grab {
    #[default]
    None,
    Drag {
        hand: usize,
        drag: Drag,
    },
    Resize {
        hand: usize,
        resize: Resize,
    },
}

/// A hand's pose in the room: its place and turn.
pub type Pose = (Vec3, Quat);

#[derive(Debug, Default)]
pub struct ScreenGrab {
    grab: Grab,
    triggers: [bool; 2],
}

/// What a frame of the grab did.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Update {
    /// A hand holds a handle: its trigger is not a click on the screen.
    pub dragging: bool,
    pub resizing: bool,
    /// A resize ended with the panel this wide (metres).
    pub resized: Option<f32>,
}

impl ScreenGrab {
    /// One frame with the panel, the hands' aim and grip poses and which triggers are down.
    /// `world_scale` is the room's.
    pub fn update(
        &mut self,
        panel: &mut Panel,
        aim: [Option<Pose>; 2],
        grip: [Option<Pose>; 2],
        triggers: [bool; 2],
        world_scale: f32,
    ) -> Update {
        let mut out = Update::default();
        if self.grab == Grab::None {
            for hand in 0..2 {
                if !(triggers[hand] && !self.triggers[hand]) {
                    continue;
                }
                let Some((origin, q)) = aim[hand] else {
                    continue;
                };
                let Some([u, v]) = panel.plane_hit(origin, q * Vec3::NEG_Z) else {
                    continue;
                };
                match handle_at(u, v, true) {
                    Some(Handle::Drag) => {
                        let drag = Drag::start(
                            origin,
                            q,
                            panel.centre(),
                            panel.orientation(),
                            world_scale,
                        );
                        log::info!("Horizon XR: dragging the screen by its handle");
                        self.grab = Grab::Drag { hand, drag };
                        break;
                    }
                    Some(Handle::Resize) => {
                        if let (Some(a), Some(b)) = (grip[hand], grip[1 - hand]) {
                            let resize = Resize::start(a.0, b.0, panel.width());
                            log::info!("Horizon XR: resizing the screen by its handle");
                            self.grab = Grab::Resize { hand, resize };
                            break;
                        }
                    }
                    None => {}
                }
            }
        }
        match self.grab {
            Grab::None => {}
            Grab::Drag { hand, drag } => {
                if triggers[hand]
                    && let Some((origin, q)) = aim[hand]
                {
                    let (pos, rot) = drag.pose(origin, q, world_scale);
                    panel.set_pose(pos, rot);
                    out.dragging = true;
                } else {
                    self.grab = Grab::None;
                }
            }
            Grab::Resize { hand, resize } => {
                if triggers[hand]
                    && let (Some(a), Some(b)) = (grip[hand], grip[1 - hand])
                {
                    let width = resize.scale_now(a.0, b.0);
                    panel.set_width(width);
                    out.resizing = true;
                } else {
                    out.resized = Some(panel.width());
                    self.grab = Grab::None;
                }
            }
        }
        self.triggers = triggers;
        out
    }

    /// The screen went away: nothing is held.
    pub fn clear(&mut self) {
        self.grab = Grab::None;
        self.triggers = [false; 2];
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_vivecraft::gui::Placement;

    fn panel() -> Panel {
        // 1 m wide, 1 m ahead of the origin, at head height, facing back at it.
        Panel::placed(&Placement {
            pos: Vec3::new(0.0, 1.5, -1.0),
            rot: Quat::IDENTITY,
            scale: 1.0,
            local: Vec3::ZERO,
        })
    }

    /// A hand at `origin` pointing at the plane point `target`.
    fn aim_at(origin: Vec3, target: Vec3) -> Pose {
        let dir = (target - origin).normalize();
        (origin, Quat::from_rotation_arc(Vec3::NEG_Z, dir))
    }

    #[test]
    fn a_click_on_the_bar_drags_the_panel_with_the_hand_and_letting_go_leaves_it() {
        let mut grab = ScreenGrab::default();
        let mut p = panel();
        // The strip is below the panel's bottom edge (y = 1.5 - 0.28125) at the left part.
        let hand = Vec3::new(-0.2, 1.0, 0.0);
        let aim = aim_at(hand, Vec3::new(-0.2, 1.5 - 0.28125 - 0.03, -1.0));
        let none = [None, None];
        let up = grab.update(&mut p, [Some(aim), None], none, [false, false], 1.0);
        assert_eq!(up, Update::default());
        let up = grab.update(&mut p, [Some(aim), None], none, [true, false], 1.0);
        assert!(up.dragging);
        // The hand goes 0.3 m to the right: the panel goes with it.
        let moved = (aim.0 + Vec3::new(0.3, 0.0, 0.0), aim.1);
        grab.update(&mut p, [Some(moved), None], none, [true, false], 1.0);
        assert!(
            (p.centre() - Vec3::new(0.3, 1.5, -1.0)).length() < 1e-4,
            "{:?}",
            p.centre()
        );
        // Letting go keeps it there and the next frame is no drag.
        grab.update(&mut p, [Some(moved), None], none, [false, false], 1.0);
        let later = (moved.0 + Vec3::new(1.0, 0.0, 0.0), moved.1);
        let up = grab.update(&mut p, [Some(later), None], none, [false, false], 1.0);
        assert!(!up.dragging);
        assert!((p.centre() - Vec3::new(0.3, 1.5, -1.0)).length() < 1e-4);
    }

    #[test]
    fn a_click_inside_the_picture_grabs_nothing() {
        let mut grab = ScreenGrab::default();
        let mut p = panel();
        let hand = Vec3::new(0.0, 1.5, 0.0);
        let aim = aim_at(hand, Vec3::new(0.0, 1.5, -1.0));
        let up = grab.update(&mut p, [Some(aim), None], [None, None], [true, false], 1.0);
        assert_eq!(up, Update::default());
        assert!((p.centre() - Vec3::new(0.0, 1.5, -1.0)).length() < 1e-6);
    }

    #[test]
    fn the_square_resizes_by_the_distance_between_the_grips_and_reports_the_width() {
        let mut grab = ScreenGrab::default();
        let mut p = panel();
        // The resize square is at 0.72 to 0.81 of the width: x = -0.5 + 0.77 = 0.27.
        let hand = Vec3::new(0.27, 1.0, 0.0);
        let aim = aim_at(hand, Vec3::new(0.27, 1.5 - 0.28125 - 0.03, -1.0));
        let grips = |d: f32| {
            [
                Some((Vec3::new(0.27, 1.0, 0.0), Quat::IDENTITY)),
                Some((Vec3::new(0.27 - d, 1.0, 0.0), Quat::IDENTITY)),
            ]
        };
        let aims = [Some(aim), None];
        let up = grab.update(&mut p, aims, grips(0.4), [true, false], 1.0);
        assert!(up.resizing || up == Update::default());
        // Hands twice as far apart: twice as wide.
        let up = grab.update(&mut p, aims, grips(0.8), [true, false], 1.0);
        assert!(up.resizing);
        assert!((p.width() - 2.0).abs() < 1e-5, "{}", p.width());
        let up = grab.update(&mut p, aims, grips(0.8), [false, false], 1.0);
        assert_eq!(up.resized, Some(p.width()));
        assert!((p.width() - 2.0).abs() < 1e-5);
    }
}
