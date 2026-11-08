// SPDX-License-Identifier: LGPL-3.0-or-later
// Derived from Vivecraft: gameplay/trackers/BowTracker.java, interact_modules/BowModule.java. Copyright (c) Vivecraft contributors; LGPL-3.0-or-later.
//! A bow drawn with both hands, by Vivecraft's `BowTracker` (`horizon-vivecraft`): standing,
//! the bow (in the main hand's slot) is held in the off hand's controller;
//! the main hand nocks an arrow by coming to the string and pulling its
//! trigger, draws by moving away from the bow, and shoots by letting go.
//! The arrow flies from the string through the bow. How far the string is
//! drawn shows on the bow and in the controllers; the arrow's power still
//! comes from how long it was drawn, as vanilla's server reckons it.
use super::Game;
use super::event_loop::EventLoopCtl;
use super::input_trace::InputOrigin;
use glam::{Mat4, Quat, Vec3};
use horizon_client::headset;
use horizon_vivecraft::trackers::hands::{BowInput, BowTracker};
use winit::event::{DeviceId, ElementState, MouseButton, WindowEvent};

/// The bow's state between frames.
#[derive(Default)]
pub(super) struct Bow {
    /// The arrow hand is at the string.
    notched: bool,
    /// The string is being drawn (the main trigger held since nocking).
    drawing: bool,
    /// How far it is drawn, 0..1.
    draw: f32,
    /// Vivecraft's tracker: the notch, the draw and the haptics.
    tracker: BowTracker,
}

impl Bow {
    /// An arrow is at the string, not yet drawn (the drawn bow's model
    /// shows its own).
    pub(super) fn notched(&self) -> bool {
        self.notched && !self.drawing
    }
}

impl Game<'_> {
    /// Standing with a bow in the main hand's slot: whether the bow is held
    /// in the off hand's controller (as it is drawn).
    pub(super) fn headset_holds_bow(&self) -> bool {
        headset::active()
            && !headset::settings().seated()
            && self.held().is_some_and(|s| &*s.item == "minecraft:bow")
    }

    /// The bow drawn this far (0..1), while it is.
    pub(super) fn headset_bow_draw(&self) -> Option<f32> {
        let bow = &self.pad.headset.bow;
        (bow.drawing && self.headset_holds_bow()).then_some(bow.draw)
    }

    /// Once a frame: nocks, draws and shoots with the hands, by Vivecraft's `BowTracker`.
    /// Returns whether the main trigger is the bow's.
    pub(super) fn headset_bow(
        &mut self,
        event_loop: &dyn EventLoopCtl,
        id: winit::window::WindowId,
        playing: bool,
    ) -> bool {
        let hands = headset::main_hand().zip(headset::off_hand());
        if hands.is_none() || !playing || !self.headset_holds_bow() {
            if self.pad.headset.bow.drawing {
                self.headset_bow_release(event_loop, id);
            }
            self.pad.headset.bow.tracker.inactive();
            self.pad.headset.bow = Bow::default();
            return false;
        }
        let settings = headset::settings();
        let world = self.pad.headset.vr.player.world_pre;
        let height = self.player.as_ref().map_or(1.8, |p| p.dimensions.height);
        let trigger = headset::hand_buttons()[0].trigger > 0.5;
        let input = BowInput {
            now_ms: self.pad.headset.vr.clock_ms(),
            bow_in_main: true,
            has_ammo: true,
            drawing: self.pad.headset.bow.drawing,
            bb_height: height,
            bb_scale: 1.0,
            reversed: settings.bool("reverseShootingEye"),
            use_duration: 72000,
        };
        let out = self.pad.headset.bow.tracker.frame(&world, input);
        for (c, strength) in out.haptics.into_iter().enumerate() {
            if let Some(micros) = strength {
                headset::pulse(c == 0, 0.2 + micros as f32 / 2000.0, 0.03);
            }
        }
        let notched = self
            .pad
            .headset
            .bow
            .tracker
            .is_notched(self.pad.headset.bow.drawing);
        let state = &mut self.pad.headset.bow;
        state.notched = notched;
        if state.drawing && !trigger {
            self.headset_bow_release(event_loop, id);
            return true;
        }
        if !state.drawing && notched && trigger {
            state.drawing = true;
            self.headset_bow_click(event_loop, id, ElementState::Pressed);
        }
        let state = &mut self.pad.headset.bow;
        state.draw = if state.drawing {
            state.tracker.draw_percent().clamp(0.0, 1.0)
        } else {
            0.0
        };
        notched || state.drawing
    }

    fn headset_bow_release(&mut self, event_loop: &dyn EventLoopCtl, id: winit::window::WindowId) {
        self.pad.headset.bow.drawing = false;
        self.headset_bow_click(event_loop, id, ElementState::Released);
    }

    fn headset_bow_click(
        &mut self,
        event_loop: &dyn EventLoopCtl,
        id: winit::window::WindowId,
        state: ElementState,
    ) {
        self.handle_window_event(
            event_loop,
            id,
            WindowEvent::MouseInput {
                device_id: DeviceId::dummy(),
                state,
                button: MouseButton::Right,
            },
            InputOrigin::User,
        );
    }

    /// While the bow is drawn, the use ticks its model shows: just drawn,
    /// half or fully, by how far the string is pulled (Vivecraft's stages).
    pub(super) fn headset_bow_remaining(&self, duration: i32) -> Option<i32> {
        let draw = self.headset_bow_draw()?;
        Some(if draw >= 1.0 {
            0
        } else if draw > 0.4 {
            duration - 15
        } else {
            duration
        })
    }

    /// While the bow is drawn, where it is held (camera-relative world):
    /// at the off hand, turned along the arrow's flight from the string
    /// hand through the bow hand, its top toward the bow hand's up
    /// (Vivecraft's `BOW_ROOMSCALE_DRAWING`); `None` while it isn't.
    pub(super) fn headset_drawn_bow_frame(&self) -> Option<Mat4> {
        self.headset_bow_draw()?;
        let (arrow, bow) = headset::main_hand().zip(headset::off_hand())?;
        let body = headset::body_rotation(headset::body_yaw());
        let aim = body * (bow.aim.0 - arrow.aim.0);
        let up = body * bow.aim.1 * Vec3::Y;
        let aim = aim.try_normalize()?;
        let up = up.reject_from_normalized(aim).try_normalize()?;
        // -Z along the aim, +Y up.
        let turn = Quat::from_mat3(&glam::Mat3::from_cols(aim.cross(up), up, -aim));
        Some(
            Mat4::from_rotation_translation(turn, body * bow.aim.0)
                * Mat4::from_translation(Vec3::new(0.075, 0.1275 + 0.1, 0.16))
                * Mat4::from_quat(Quat::from_rotation_y(std::f32::consts::PI))
                * Mat4::from_quat(Quat::from_rotation_x(160f32.to_radians())),
        )
    }
}
