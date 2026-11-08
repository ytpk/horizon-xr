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

    /// `onClear`: the task stops (a screen, a vehicle, sleep).
    pub fn clear(&mut self) -> Option<bool> {
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
        self.sneaking = !crawling && actual_height / full_height <= threshold;
        self.sneaking
    }

    /// `onClear`.
    pub fn clear(&mut self) {
        self.sneaking = false;
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
