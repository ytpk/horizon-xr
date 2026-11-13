// SPDX-License-Identifier: MPL-2.0
//! The movement tasks of Visor: crawl (`TaskRoomCrawl`), teleport limits (`TaskTeleport`), the
//! movement mode the server allows (`VRClientSettings.getMoveMode`). Copyright (C) Phoenix-Ra
//! and Visor contributors; MPL-2.0 (see LICENSE-MPL-2.0).

use crate::settings::{ServerSettings, SupportedMovement};

/// `TaskRoomCrawl.EXIT_MARGIN`.
pub const CRAWL_EXIT_MARGIN: f32 = 0.03;
/// `roomCrawlThreshold`'s default.
pub const DEFAULT_CRAWL_THRESHOLD: f32 = 0.7;

/// `TaskRoomCrawl`: crawling while the head is lower than `threshold` of the full height
/// (a little lower to get out of it than to get into it).
#[derive(Clone, Copy, Debug, Default)]
pub struct CrawlTask {
    pub crawling: bool,
    pending: Option<(bool, u64)>,
}

impl CrawlTask {
    /// One tick: `actual_height` is the head's height over the floor, `full_height` the
    /// standing one. Returns the state to tell the server (`CRAWLING`) when it changed.
    pub fn tick(&mut self, actual_height: f32, full_height: f32, threshold: f32) -> Option<bool> {
        let limit = threshold
            + if self.crawling {
                CRAWL_EXIT_MARGIN
            } else {
                0.0
            };
        self.set(actual_height / full_height <= limit)
    }

    /// Ignore single-frame height spikes; require a candidate pose for 100 ms.
    pub fn tick_at(&mut self, now_ms: u64, actual: f32, full: f32, threshold: f32) -> Option<bool> {
        if !actual.is_finite() || !full.is_finite() || full <= 0.0 {
            return self.clear();
        }
        let limit = threshold
            + if self.crawling {
                CRAWL_EXIT_MARGIN
            } else {
                0.0
            };
        let wanted = actual / full <= limit;
        if wanted == self.crawling {
            self.pending = None;
            return None;
        }
        match self.pending {
            Some((candidate, since)) if candidate == wanted && now_ms >= since => {
                if now_ms - since >= 100 {
                    self.pending = None;
                    return self.set(wanted);
                }
            }
            _ => self.pending = Some((wanted, now_ms)),
        }
        None
    }

    /// `onClear`: the task stops (a screen, a vehicle, sleep).
    pub fn clear(&mut self) -> Option<bool> {
        self.pending = None;
        self.set(false)
    }

    fn set(&mut self, crawling: bool) -> Option<bool> {
        (self.crawling != crawling).then(|| {
            self.crawling = crawling;
            crawling
        })
    }
}

/// `roomSneakThreshold`'s default.
pub const DEFAULT_SNEAK_THRESHOLD: f32 = 0.85;

/// `TaskRoomSneak`: sneaking while the head's pivot is at or below `threshold` of the standing
/// height, unless the player is crawling (`TaskRoomCrawl` runs first). Both heights are the head
/// pivot's height above the floor in the room: never the camera's, which the pose moves.
#[derive(Clone, Copy, Debug, Default)]
pub struct SneakTask {
    pub sneaking: bool,
}

impl SneakTask {
    /// One tick; returns whether the player sneaks.
    pub fn tick(
        &mut self,
        actual_height: f32,
        full_height: f32,
        threshold: f32,
        crawling: bool,
    ) -> bool {
        let limit = threshold + if self.sneaking { 0.02 } else { 0.0 };
        self.sneaking = !crawling
            && actual_height.is_finite()
            && full_height.is_finite()
            && full_height > 0.0
            && actual_height / full_height <= limit;
        self.sneaking
    }

    /// `onClear`.
    pub fn clear(&mut self) {
        self.sneaking = false;
    }
}

/// Physical jumps must start from a settled standing pose, not a recenter or rising from a chair.
#[derive(Clone, Copy, Debug, Default)]
pub struct RoomJumpTask {
    stable_since: Option<u64>,
    armed: bool,
}
impl RoomJumpTask {
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn tick(
        &mut self,
        active: bool,
        now_ms: u64,
        actual: f32,
        full: f32,
        threshold: f32,
        rise: f32,
        upward_speed: f32,
    ) -> bool {
        if !active || !actual.is_finite() || !full.is_finite() || full <= 0.0 {
            self.clear();
            return false;
        }
        let near_rest = (actual - full).abs() < 0.05 && rise.abs() < 0.025;
        if near_rest {
            let since = *self.stable_since.get_or_insert(now_ms);
            if now_ms.saturating_sub(since) >= 200 {
                self.armed = true;
            }
        } else {
            self.stable_since = None;
        }
        if actual < full * 0.85 {
            self.clear();
            return false;
        }
        if self.armed && rise >= 0.1 && upward_speed >= 0.6 && actual / full >= threshold {
            self.clear();
            return true;
        }
        false
    }
}

/// How the player moves: the stick or teleporting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MovementMode {
    Controller,
    Teleport,
}

/// `VRClientSettings.getMoveMode`: what the server allows wins over the player's setting; a
/// passenger or a flying player always uses the controller.
pub fn move_mode(
    setting: MovementMode,
    server: &ServerSettings,
    passenger: bool,
    flying: bool,
) -> MovementMode {
    if passenger || flying {
        return MovementMode::Controller;
    }
    match server.supported_movement {
        SupportedMovement::Both => setting,
        SupportedMovement::Controller => MovementMode::Controller,
        SupportedMovement::Teleport => MovementMode::Teleport,
    }
}

/// The teleport distance limits of the server, in blocks: up, down and forward.
pub fn teleport_limits(server: &ServerSettings) -> (i32, i32, i32) {
    (
        server.teleport_up_limit,
        server.teleport_down_limit,
        server.teleport_forward_limit,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crawl_ignores_a_spike_and_requires_a_sustained_transition() {
        let mut task = CrawlTask::default();
        assert_eq!(task.tick_at(0, 0.8, 1.52, 0.7), None);
        assert_eq!(task.tick_at(50, 1.52, 1.52, 0.7), None);
        assert!(!task.crawling);
        assert_eq!(task.tick_at(100, 0.8, 1.52, 0.7), None);
        assert_eq!(task.tick_at(200, 0.8, 1.52, 0.7), Some(true));
        assert_eq!(task.tick_at(250, 1.52, 1.52, 0.7), None);
        assert_eq!(task.tick_at(350, 1.52, 1.52, 0.7), Some(false));
    }

    #[test]
    fn standing_up_and_a_recenter_do_not_arm_a_jump() {
        let mut task = RoomJumpTask::default();
        for (ms, height) in [(0, 0.8), (100, 1.2), (200, 1.6)] {
            assert!(!task.tick(true, ms, height, 1.52, 1.05, 0.2, 1.0));
        }
        for ms in [300, 400, 500] {
            assert!(!task.tick(true, ms, 1.52, 1.52, 1.05, 0.0, 0.0));
        }
        assert!(task.tick(true, 600, 1.67, 1.52, 1.05, 0.15, 1.2));
        assert!(!task.tick(true, 650, 1.7, 1.52, 1.05, 0.18, 1.0));
        task.clear();
        assert!(!task.tick(true, 700, 1.7, 1.52, 1.05, 0.2, 2.0));
    }

    #[test]
    fn crawling_starts_below_the_threshold_and_ends_a_margin_above() {
        let mut c = CrawlTask::default();
        // 0.69 of the height: in.
        assert_eq!(c.tick(1.05, 1.52, 0.7), Some(true));
        // 0.71: still crawling (inside the margin), nothing to say.
        assert_eq!(c.tick(1.08, 1.52, 0.7), None);
        // 0.74: out.
        assert_eq!(c.tick(1.13, 1.52, 0.7), Some(false));
        // 0.71 again does not go back in.
        assert_eq!(c.tick(1.08, 1.52, 0.7), None);
        assert_eq!(c.tick(1.0, 1.52, 0.7), Some(true));
        assert_eq!(c.clear(), Some(false));
        assert_eq!(c.clear(), None);
    }

    #[test]
    fn the_server_decides_the_movement_when_it_is_not_both() {
        let mut s = ServerSettings::default();
        assert_eq!(
            move_mode(MovementMode::Teleport, &s, false, false),
            MovementMode::Teleport
        );
        s.supported_movement = SupportedMovement::Controller;
        assert_eq!(
            move_mode(MovementMode::Teleport, &s, false, false),
            MovementMode::Controller
        );
        s.supported_movement = SupportedMovement::Teleport;
        assert_eq!(
            move_mode(MovementMode::Controller, &s, false, false),
            MovementMode::Teleport
        );
        // A rider or a flyer uses the stick whatever the server says.
        assert_eq!(
            move_mode(MovementMode::Controller, &s, true, false),
            MovementMode::Controller
        );
        assert_eq!(
            move_mode(MovementMode::Teleport, &s, false, true),
            MovementMode::Controller
        );
        assert_eq!(teleport_limits(&ServerSettings::default()), (1, 4, 16));
    }
}
