// SPDX-License-Identifier: LGPL-3.0-or-later
// Derived from Vivecraft: gameplay/trackers/InteractTracker.java, interact_modules/BlockInteractionModule.java. Copyright (c) Vivecraft contributors; LGPL-3.0-or-later.
//! What a headset's hands do to blocks by moving, after Vivecraft's trackers:
//! a tool or a fist swung fast through a block hits it (`SwingTracker`'s
//! roomscale mining), and a hand in a button, lever, door or container uses
//! it with its trigger (`InteractTracker`'s block module). Standing only.
use super::event_loop::EventLoopCtl;
use super::input_trace::InputOrigin;
use super::{Game, Hit};
use horizon_client::headset;
use std::time::{Duration, Instant};
use winit::event::{DeviceId, ElementState, MouseButton, WindowEvent};

/// A game tick.
const TICK: Duration = Duration::from_millis(50);

/// The hands' state between frames.
#[derive(Default)]
pub(super) struct Trackers {
    /// A button held for a gesture, the block it acts on, and when it lets go.
    holding: Option<(MouseButton, Instant)>,
    /// The block each hand (main, off) is in, while it can be used.
    touching: [Option<Hit>; 2],
    /// Each hand's trigger was down.
    triggers: [bool; 2],
}

/// The block classes a hand uses by touching (they have
/// `useWithoutItem`, as Vivecraft's list is made of).
const TOUCHABLE: &[&str] = &[
    "ButtonBlock",
    "LeverBlock",
    "DoorBlock",
    "TrapDoorBlock",
    "FenceGateBlock",
    "NoteBlock",
    "RepeaterBlock",
    "ComparatorBlock",
    "DaylightDetectorBlock",
    "BellBlock",
    "ChestBlock",
    "BarrelBlock",
    "EnderChestBlock",
    "ShulkerBoxBlock",
    "CraftingTableBlock",
    "AbstractFurnaceBlock",
    "AnvilBlock",
    "GrindstoneBlock",
    "StonecutterBlock",
    "LoomBlock",
    "CartographyTableBlock",
    "SmithingTableBlock",
    "EnchantingTableBlock",
    "BrewingStandBlock",
    "HopperBlock",
    "DispenserBlock",
    "LecternBlock",
    "JukeboxBlock",
    "BeaconBlock",
    "CakeBlock",
    "ComposterBlock",
    "CrafterBlock",
];

impl Trackers {
    /// No gesture holds a button.
    pub(super) fn idle(&self) -> bool {
        self.holding.is_none()
    }

    /// Holds `button` down for `time`; it is let go in `headset_trackers`.
    pub(super) fn hold(&mut self, button: MouseButton, time: Duration) {
        self.holding = Some((button, Instant::now() + time));
    }
}

impl Game<'_> {
    /// Once a frame, standing in a headset and playing: swings into blocks
    /// and touches on them.
    pub(super) fn headset_trackers(
        &mut self,
        event_loop: &dyn EventLoopCtl,
        id: winit::window::WindowId,
    ) {
        // A gesture's button goes up when its time is over.
        if let Some((button, until)) = self.pad.headset.trackers.holding
            && Instant::now() >= until
        {
            self.pad.headset.trackers.holding = None;
            self.forced_hit = None;
            self.headset_click(event_loop, id, button, ElementState::Released);
        }
        let settings = headset::settings();
        let playing = !settings.seated()
            && self.player.is_some()
            && !self.spectator()
            && self.screen == super::Screen::Playing;
        // The bow takes the main trigger while an arrow is at its string.
        let bow = self.headset_bow(event_loop, id, playing);
        if !playing {
            self.pad.headset.trackers.touching = [None, None];
            headset::set_triggers_captured([false; 2]);
            return;
        }
        let touching = if (settings.bool("realisticBlockInteractEnabled")
            || settings.bool("realisticEntityInteractEnabled"))
            && !bow
        {
            self.headset_touch(event_loop, id)
        } else {
            self.pad.headset.trackers.touching = [None, None];
            [false; 2]
        };
        headset::set_triggers_captured([touching[0] || bow, touching[1]]);
        self.vr_swim();
    }

    /// Presses or lets go of a mouse button for a gesture.
    fn headset_click(
        &mut self,
        event_loop: &dyn EventLoopCtl,
        id: winit::window::WindowId,
        button: MouseButton,
        state: ElementState,
    ) {
        self.handle_window_event(
            event_loop,
            id,
            WindowEvent::MouseInput {
                device_id: DeviceId::dummy(),
                state,
                button,
            },
            InputOrigin::User,
        );
    }

    /// A hand's controller (main or `off`) in the world (blocks): its aim
    /// pose's place, and where it points.
    fn headset_hand_world(&self, off: bool) -> Option<([f64; 3], glam::Quat)> {
        let hand = if off {
            headset::off_hand()
        } else {
            headset::main_hand()
        }?;
        let eye = self.eye_position()?;
        let body = headset::body_yaw();
        let at = headset::to_world(body, hand.aim.0);
        let turn = headset::body_rotation(body) * hand.aim.1;
        Some((std::array::from_fn(|i| eye[i] + f64::from(at[i])), turn))
    }

    /// `InteractTracker`'s block module: a hand inside a block it can use
    /// takes its trigger, which uses the block.
    fn headset_touch(
        &mut self,
        event_loop: &dyn EventLoopCtl,
        id: winit::window::WindowId,
    ) -> [bool; 2] {
        let eye = self.eye_position();
        let touching: [Option<Hit>; 2] = std::array::from_fn(|i| {
            let (at, _) = self.headset_hand_world(i == 1)?;
            let pos = at.map(|v| v.floor() as i32);
            let state = self.world.block(pos[0], pos[1], pos[2]);
            let facts = self.res.facts().get(state);
            if !facts
                .classes
                .iter()
                .any(|c| TOUCHABLE.iter().any(|t| c.ends_with(t)))
            {
                return None;
            }
            // From the eyes to the hand, as Vivecraft clips the block's shape.
            let eye = eye?;
            let along: [f64; 3] = std::array::from_fn(|k| at[k] - eye[k]);
            let length = along.iter().map(|v| v * v).sum::<f64>().sqrt();
            if length < 1e-3 {
                return None;
            }
            let dir = along.map(|v| v / length);
            self.clip_ray(eye, dir, length).filter(|hit| hit.pos == pos)
        });
        self.pad.headset.trackers.touching = touching;
        let buttons = headset::hand_buttons();
        for (i, touched) in touching.into_iter().enumerate() {
            let pressed = buttons[i].trigger > 0.5;
            let click = pressed && !self.pad.headset.trackers.triggers[i];
            self.pad.headset.trackers.triggers[i] = pressed;
            let Some(hit) = touched else {
                continue;
            };
            if !click || self.pad.headset.trackers.holding.is_some() {
                continue;
            }
            // The use goes to the touched block for the next tick or two.
            self.forced_hit = Some(hit);
            self.pad.headset.trackers.holding =
                Some((MouseButton::Right, Instant::now() + TICK * 2));
            self.headset_click(event_loop, id, MouseButton::Right, ElementState::Pressed);
            self.pad.headset.swing_type = super::headset_hands::SwingType::Interact;
            headset::pulse(i == 0, 0.5, 0.04);
        }
        [touching[0].is_some(), touching[1].is_some()]
    }
}
