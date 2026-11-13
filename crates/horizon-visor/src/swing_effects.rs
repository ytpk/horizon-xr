// SPDX-License-Identifier: MPL-2.0
//! What Visor's `TaskSwing` does on the client when a swing hits, besides telling the server
//! (`swingAttack`, `swingMining`, `mineBetter`, `mineVanilla`, `confirmPendingBreaks`,
//! `blockDust`). The decisions are here, pure, so what plays when is tested; the game plays it.
//! Copyright (C) Phoenix-Ra and Visor contributors; MPL-2.0 (see LICENSE-MPL-2.0).
//!
//! Where a hit's feedback comes from:
//!
//! - **Entity, `betterSwinging` on the server**: `SWING_ATTACK` is sent and `Player.attack` runs
//!   locally (the attacker's own side: the strength ticker starts again, a player target slows
//!   the attacker). The attack and hit sounds are the server's.
//! - **Entity, no `betterSwinging`** (a vanilla server, or single player's server thread):
//!   `MultiPlayerGameMode.attack(player, entity)` on exactly that entity, not a click along the
//!   aim; the same sounds come from the server.
//! - **Block, `betterSwinging`**: the server sends no sound and no particles to the swinger
//!   (`visor$handleVrBlockDamage` only sends the crack), so the client plays the block's hit
//!   sound itself, throws the dust, and watches the block: when it is gone (the server broke
//!   it) the break sound and particles play, `confirmPendingBreaks`.
//! - **Block, no `betterSwinging`**: `startDestroyBlock` / `continueDestroyBlock` run as for a
//!   click and play the hit sound, the crack and the break themselves; only the dust is added.
//! - Both blocks and entities pulse the controller.

use crate::tasks::swing::{HAPTIC_ATTACK_MICROS, HAPTIC_BLOCK_PER_HIT_MICROS};
use std::collections::HashMap;

/// `TaskSwing.DUST_PER_HIT`: terrain particles per hit of a block swing.
pub const DUST_PER_HIT: u32 = 3;
/// `TaskSwing.DUST_SPEED`: the dust crumbs' size, as `blockDust(.., sizeScale, ..)` takes it.
pub const DUST_SIZE: f32 = 0.6;
/// `TaskSwing.BREAK_CONFIRM_TICKS`: how long a swung-at block is watched for breaking.
pub const BREAK_CONFIRM_TICKS: u64 = 40;

/// Whether the swing task speaks `SWING_ATTACK` / `SWING_BLOCK` (`VRServerSettings
/// .isBetterSwinging()` with a server that speaks Visor); otherwise it clicks like vanilla.
pub fn better(server_better_swinging: bool, server_speaks_visor: bool) -> bool {
    server_better_swinging && server_speaks_visor
}

/// What the client does for an entity hit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EntityFeedback {
    /// `SWING_ATTACK` goes to the server.
    pub send_swing_attack: bool,
    /// The attack is made on that very entity as `MultiPlayerGameMode.attack` makes it.
    pub vanilla_attack: bool,
    /// `Player.attack` runs locally and `resetAttackStrengthTicker`.
    pub local_attack: bool,
    /// The sounds are the server's, the client plays no hit sound of its own.
    pub client_plays_sound: bool,
    /// `triggerHapticPulseMicroSec(hand, SWING_ATTACK)`, microseconds.
    pub haptic_micros: u32,
}

pub fn entity_feedback(better: bool) -> EntityFeedback {
    EntityFeedback {
        send_swing_attack: better,
        vanilla_attack: !better,
        local_attack: better,
        client_plays_sound: false,
        haptic_micros: HAPTIC_ATTACK_MICROS,
    }
}

/// What the client does for a block swing of `hits` hits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockFeedback {
    /// One `SWING_BLOCK` per hit goes to the server.
    pub send_swing_block: bool,
    /// The vanilla dig runs (`startDestroyBlock`, `continueDestroyBlock`) and plays the hit sound,
    /// the crack and the break by itself.
    pub vanilla_dig: bool,
    /// The client plays the block's hit sound (`mineBetter`).
    pub hit_sound: bool,
    /// `DUST_PER_HIT * hits` crumbs at the hit.
    pub dust: u32,
    /// The block is watched, and its break plays when it is gone (`pendingBreaks`).
    pub watch_break: bool,
    /// `triggerHapticPulseMicroSec(hand, SWING_BLOCK_PER_HIT * hits)`, microseconds.
    pub haptic_micros: u32,
}

pub fn block_feedback(better: bool, hits: u32) -> BlockFeedback {
    BlockFeedback {
        send_swing_block: better,
        vanilla_dig: !better,
        hit_sound: better,
        dust: DUST_PER_HIT * hits,
        watch_break: better,
        haptic_micros: HAPTIC_BLOCK_PER_HIT_MICROS * hits,
    }
}

/// The block hit sound of `mineBetter`: `SimpleSoundInstance(soundType.getHitSound(), BLOCKS,
/// (volume + 1) / 8, pitch * 0.5, ..)`. Returns (volume, pitch).
pub fn hit_sound(block_volume: f32, block_pitch: f32) -> (f32, f32) {
    ((block_volume + 1.0) / 8.0, block_pitch * 0.5)
}

/// A controller pulse for `micros` microseconds of Visor's haptic call: (strength, seconds) of
/// the headset's pulse, which lasts 20 to 100 ms to be felt.
pub fn haptic_pulse(micros: u32) -> (f32, f32) {
    (0.6, (micros as f32 / 1.0e6 * 40.0).clamp(0.02, 0.1))
}

/// `pendingBreaks`: blocks the swings hit, until the server breaks them.
#[derive(Clone, Debug)]
pub struct PendingBreaks<S> {
    pending: HashMap<[i32; 3], (S, u64)>,
}

impl<S> Default for PendingBreaks<S> {
    fn default() -> Self {
        Self {
            pending: HashMap::new(),
        }
    }
}

impl<S: PartialEq + Clone> PendingBreaks<S> {
    pub fn new() -> Self {
        Self {
            pending: HashMap::new(),
        }
    }

    /// `pendingBreaks.put(pos, new PendingBreak(state, tick))`.
    pub fn watch(&mut self, pos: [i32; 3], state: S, tick: u64) {
        self.pending.insert(pos, (state, tick));
    }

    /// `confirmPendingBreaks`: forgets what is older than [`BREAK_CONFIRM_TICKS`], and returns
    /// the blocks that are not what they were any more (with what they were), once each.
    /// `now_at` is the block at a position now.
    pub fn confirm(
        &mut self,
        tick: u64,
        mut now_at: impl FnMut([i32; 3]) -> S,
    ) -> Vec<([i32; 3], S)> {
        let mut broken = Vec::new();
        self.pending.retain(|pos, (state, since)| {
            if tick.saturating_sub(*since) > BREAK_CONFIRM_TICKS {
                return false;
            }
            if now_at(*pos) == *state {
                return true;
            }
            broken.push((*pos, state.clone()));
            false
        });
        broken.sort_by_key(|(pos, _)| *pos);
        broken
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    pub fn clear(&mut self) {
        self.pending.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn better_swinging_needs_the_server_to_allow_and_to_speak_visor() {
        // Single player's server thread does not speak Visor; a vanilla server neither.
        assert!(!better(true, false));
        assert!(!better(false, true));
        assert!(better(true, true));
    }

    #[test]
    fn an_entity_hit_with_better_swinging_is_sent_and_attacked_locally() {
        let f = entity_feedback(true);
        assert!(f.send_swing_attack && f.local_attack && !f.vanilla_attack);
        // The sounds are the server's: nothing is played twice.
        assert!(!f.client_plays_sound);
        assert_eq!(f.haptic_micros, 1_000);
    }

    #[test]
    fn an_entity_hit_without_it_attacks_that_entity_like_vanilla() {
        let f = entity_feedback(false);
        assert!(!f.send_swing_attack && !f.local_attack && f.vanilla_attack);
        assert_eq!(f.haptic_micros, 1_000);
    }

    #[test]
    fn a_block_swing_with_better_swinging_plays_the_hit_and_watches_the_break() {
        // Three hits: nine crumbs, 750 microseconds.
        let f = block_feedback(true, 3);
        assert!(f.send_swing_block && f.hit_sound && f.watch_break && !f.vanilla_dig);
        assert_eq!((f.dust, f.haptic_micros), (9, 750));
        // A fast swing of seven hits.
        let f = block_feedback(true, 7);
        assert_eq!((f.dust, f.haptic_micros), (21, 1_750));
    }

    #[test]
    fn a_block_swing_without_it_digs_like_vanilla_and_only_adds_the_dust() {
        let f = block_feedback(false, 4);
        // The vanilla dig plays its own hit sound, crack and break: no second sound, nothing watched.
        assert!(f.vanilla_dig && !f.hit_sound && !f.watch_break && !f.send_swing_block);
        assert_eq!((f.dust, f.haptic_micros), (12, 1_000));
    }

    #[test]
    fn the_hit_sound_is_the_block_sound_an_eighth_as_loud_and_half_the_pitch() {
        // Stone: volume 1.0, pitch 1.0.
        assert_eq!(hit_sound(1.0, 1.0), (0.25, 0.5));
        // Glass: pitch 1.0; wool: volume 1.0, pitch 1.0; a loud block of volume 3.
        assert_eq!(hit_sound(3.0, 1.2), (0.5, 0.6));
    }

    #[test]
    fn a_pulse_is_felt_for_at_least_twenty_milliseconds() {
        assert_eq!(haptic_pulse(250), (0.6, 0.02));
        let (_, long) = haptic_pulse(1_750);
        assert!((long - 0.07).abs() < 1e-6);
        assert_eq!(haptic_pulse(10_000_000).1, 0.1);
    }

    #[test]
    fn a_watched_block_that_is_gone_breaks_once_and_an_old_one_is_forgotten() {
        let mut p: PendingBreaks<u32> = PendingBreaks::new();
        p.watch([1, 2, 3], 7, 100);
        p.watch([5, 5, 5], 9, 100);
        // Both blocks are still what they were: nothing yet.
        let same = |pos: [i32; 3]| if pos == [1, 2, 3] { 7 } else { 9 };
        assert!(p.confirm(105, same).is_empty());
        // The server broke the first (0 now): what it was is handed back, once.
        let one_gone = |pos: [i32; 3]| if pos == [1, 2, 3] { 0 } else { 9 };
        assert_eq!(p.confirm(110, one_gone), vec![([1, 2, 3], 7)]);
        assert!(p.confirm(111, one_gone).is_empty());
        assert!(!p.is_empty(), "the other block is still watched");
        // 41 ticks after the hit nothing is waited for: not even a block that is gone.
        let mut p: PendingBreaks<u32> = PendingBreaks::new();
        p.watch([0, 0, 0], 3, 0);
        assert!(p.confirm(41, |_| 0).is_empty());
        assert!(p.is_empty());
        // Exactly 40 ticks later it still counts.
        p.watch([0, 0, 0], 3, 0);
        assert_eq!(p.confirm(40, |_| 0).len(), 1);
    }
}
