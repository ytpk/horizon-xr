// SPDX-License-Identifier: LGPL-3.0-or-later
//! Ports `org.vivecraft.client_vr.gameplay.trackers`: the gameplay trackers as state machines
//! that take the tracked data and the player's state and say what the game should do.
//! Copyright (c) Vivecraft contributors; LGPL-3.0-or-later (see LICENSE-LGPL-3.0).
//!
//! Each tracker keeps Vivecraft's `isActive` gate (the `Active` input structs and settings) and
//! its `activeProcess`/`inactiveProcess` split; the game applies the outputs (a jump, a click,
//! a haptic pulse) and supplies the world queries as plain inputs or small traits.

pub mod body;
pub mod climb;
pub mod hands;
pub mod moves;
pub mod scope;
pub mod swing;
pub mod teleport;
pub mod vehicle;

/// A haptic pulse asked for: the controller (0 main, 1 off) and microseconds, as
/// `triggerHapticPulse(controller, durationMicros)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Haptic {
    pub controller: usize,
    pub micros: u32,
}

/// The player's state the trackers gate on (`LocalPlayer` queries).
#[derive(Clone, Copy, Debug, Default)]
pub struct PlayerFlags {
    pub alive: bool,
    pub on_ground: bool,
    pub on_climbable: bool,
    pub in_water: bool,
    pub in_lava: bool,
    pub shift_down: bool,
    pub passenger: bool,
    pub sleeping: bool,
    pub spectator: bool,
    /// The game has a game mode (a level is joined).
    pub in_game: bool,
    pub paused_or_screen: bool,
}
