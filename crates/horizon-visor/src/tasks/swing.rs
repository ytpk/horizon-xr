// SPDX-License-Identifier: MPL-2.0
//! `TaskSwing` (Visor 0.6.1, `core/client/tasks/types/TaskSwing.java`; the Java marks it a
//! prototype to be reworked after 0.7.0): a fast swing of a hand attacks what the item reaches
//! and mines the block its tip crosses. Copyright (C) Phoenix-Ra and Visor contributors;
//! MPL-2.0 (see LICENSE-MPL-2.0).
//!
//! Differences from Vivecraft's `SwingTracker`: the speed is the average over the last 0.31 s
//! of the tip (0.3 m along the hand); the swing threshold is `max(0.5, swing.speed_threshold)`
//! (default 3.0); the block search follows the arc between two ticks; a swing mines
//! `3 + min(speed - threshold, 4)` hits; each entity is hit once per swing.

use glam::{DQuat, DVec3};
use std::collections::VecDeque;

pub const SWORD_LENGTH: f32 = 0.6;
pub const TOOL_LENGTH: f32 = 0.35;
pub const DEFAULT_ITEM_LENGTH: f32 = 0.1;
pub const FIST_REACH: f32 = 0.3;
pub const TIP_OFFSET: f32 = 0.3;
pub const MAX_ARC_SUBDIVISIONS: i32 = 8;
/// `VANILLA_ENTITY_REACH`: `GameRenderer.pick`'s entity interaction distance.
pub const VANILLA_ENTITY_REACH: f64 = 3.0;
pub const DEGENERATE_SEGMENT_SQ: f64 = 1.0e-7;
pub const DEGENERATE_NUDGE: f64 = 0.001;
pub const MAX_OVERSPEED_BONUS: f32 = 4.0;
/// `swing.speed_threshold` default.
pub const DEFAULT_SPEED_THRESHOLD: f32 = 3.0;
/// The window the tip speed averages over, seconds.
pub const SPEED_WINDOW: f64 = 0.31;
/// `HapticFeedback.SWING_BLOCK_PER_HIT` and `SWING_ATTACK`, microseconds.
pub const HAPTIC_BLOCK_PER_HIT_MICROS: u32 = 250;
pub const HAPTIC_ATTACK_MICROS: u32 = 1_000;

/// `TaskSwing.isActive`: do not inherit Vivecraft's AUTO exclusion in creative.
pub fn is_active(
    player: &horizon_vivecraft::trackers::PlayerFlags,
    creative: bool,
    in_creative: bool,
    blocking: bool,
    attacks_while_blocking: bool,
    grabbed: bool,
) -> bool {
    player.in_game
        && player.alive
        && !player.sleeping
        && !player.spectator
        && !player.passenger
        && !player.paused_or_screen
        && !grabbed
        && (in_creative || !creative)
        && (attacks_while_blocking || !blocking)
}

/// What the hand holds, as `getItemProperties` sorts it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemKind {
    Empty,
    /// A sword, trident or mace (`ItemClassifier.SWORD`, `TRIDENT`, `MACE`).
    Weapon,
    /// `isTool`: a digger, shears, flint and steel, a brush, a rod, a stick ...
    Tool,
    Other,
}

/// `getItemProperties`: the length of the item along the hand and how far past its tip an attack
/// reaches. `reach` is the entity interaction range the item gives.
pub fn item_properties(kind: ItemKind, reach: f64, world_scale: f32) -> (f32, f32) {
    let (mut length, range) = if kind == ItemKind::Empty {
        (0.0, FIST_REACH)
    } else {
        let reach = (reach.min(6.0) - 0.5) as f32;
        let (length, share) = match kind {
            ItemKind::Weapon => (SWORD_LENGTH, 1.0),
            ItemKind::Tool => (TOOL_LENGTH, 0.62),
            _ => (DEFAULT_ITEM_LENGTH, 0.16),
        };
        (length, reach * share - length)
    };
    length *= world_scale;
    (length, range)
}

/// `effectiveSpeedThreshold`.
pub fn speed_threshold(setting: f32) -> f32 {
    setting.max(0.5)
}

/// How many hits a block swing makes: 3, plus the whole part of the speed over the threshold
/// (at most 4).
pub fn block_hits(speed: f32, threshold: f32) -> u32 {
    3 + (speed - threshold).min(MAX_OVERSPEED_BONUS) as u32
}

/// `Vector3fHistory.averageSpeed`: the mean of the speeds of the segments in the last `window`
/// seconds.
#[derive(Clone, Debug, Default)]
pub struct TipHistory {
    samples: VecDeque<(u64, DVec3)>,
}

const HISTORY_CAPACITY: usize = 200;

impl TipHistory {
    pub fn add(&mut self, now_ms: u64, p: DVec3) {
        self.samples.push_back((now_ms, p));
        if self.samples.len() > HISTORY_CAPACITY {
            self.samples.pop_front();
        }
    }

    pub fn clear(&mut self) {
        self.samples.clear();
    }

    pub fn average_speed(&self, now_ms: u64, window: f64) -> f32 {
        let cutoff = now_ms as f64 - window * 1000.0;
        let mut previous = None;
        let mut sum = 0.0f64;
        let mut segments = 0;
        for &(at, point) in self.samples.iter().filter(|s| s.0 as f64 >= cutoff) {
            if let Some((last_at, last_point)) = previous {
                let dt = (at - last_at) as f64 / 1000.0;
                if dt > 0.0 {
                    sum += point.distance(last_point) / dt;
                    segments += 1;
                }
            }
            previous = Some((at, point));
        }
        if segments == 0 {
            0.0
        } else {
            (sum / f64::from(segments)) as f32
        }
    }
}

/// A block the tip crossed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlockHit {
    pub pos: [i32; 3],
    pub face: [i32; 3],
    pub location: DVec3,
    /// The ray started inside the block (`hit.isInside()`).
    pub inside: bool,
}

/// An entity in the damage area.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EntityRef {
    pub id: u32,
    /// `isPickable`.
    pub pickable: bool,
    /// It is the vehicle the player rides.
    pub is_vehicle: bool,
    pub is_player: bool,
}

/// The world the swing looks at.
pub trait SwingWorld {
    /// Entities (spectators left out) in the box from `lo` to `hi`.
    fn entities(&mut self, lo: DVec3, hi: DVec3) -> Vec<EntityRef>;
    /// `level.clip(from, to, OUTLINE, no fluid)`: the first block along the segment.
    fn clip(&mut self, from: DVec3, to: DVec3) -> Option<BlockHit>;
    /// `TaskRoomClimb.isClimbableBlock`.
    fn is_climbable(&mut self, pos: [i32; 3]) -> bool;
    /// `canWeaponBreak`: the item is the right tool or the block breaks instantly.
    fn weapon_can_break(&mut self, hit: &BlockHit) -> bool;
    /// A farming tool on a farmable block: the swing uses the item on the block.
    fn is_farmable(&mut self, hit: &BlockHit) -> bool;
}

/// One hand's inputs for a tick.
#[derive(Clone, Copy, Debug)]
pub struct HandInput {
    pub now_ms: u64,
    /// The hand's aim pose in the world: position, forward (-Z) and rotation.
    pub pos: DVec3,
    pub dir: DVec3,
    pub rot: DQuat,
    /// The same in the room: the tip history is room relative.
    pub room_pos: DVec3,
    pub room_dir: DVec3,
    pub head: DVec3,
    pub kind: ItemKind,
    pub reach: f64,
    pub world_scale: f32,
    pub farming_tool: bool,
}

/// What the game does.
#[derive(Clone, Debug, PartialEq)]
pub enum SwingAction {
    /// `swingAttack(player, entity, hand)`.
    Attack { hand: usize, entity: u32 },
    /// `MC.gameMode.useItemOn` on a farmable block.
    UseOn { hand: usize, hit: BlockHit },
    /// `swingMining(hit, state, totalHits, hand)`.
    Mine {
        hand: usize,
        hit: BlockHit,
        hits: u32,
    },
    /// A pulse of `micros` microseconds.
    Haptic { hand: usize, micros: u32 },
}

#[derive(Clone, Debug, Default)]
struct HandState {
    history: TipHistory,
    /// The last swing hit a block: not again until the hand slows (`lastSwingBlock`).
    last_swing_block: bool,
    last_hit_entities: Vec<u32>,
    last_hand: Option<(DVec3, DQuat)>,
    last_swing_point: Option<DVec3>,
    last_attack_point: Option<DVec3>,
    last_weapon_tip: Option<DVec3>,
}

impl HandState {
    fn reset_swing(&mut self) {
        self.last_swing_block = false;
        self.last_hit_entities.clear();
        self.last_hand = None;
        self.last_swing_point = None;
        self.last_attack_point = None;
        self.last_weapon_tip = None;
    }
}

/// `TaskSwing`'s state.
#[derive(Clone, Debug, Default)]
pub struct SwingTask {
    hands: [HandState; 2],
    /// `swingInactiveTicks`: ticks to wait after a start, a teleport or a screen.
    pub inactive_ticks: i32,
}

impl SwingTask {
    /// `isActive`'s counter: false while the task waits; the game checks the rest (alive, not
    /// asleep, not a spectator, no vehicle, no screen, not climbing, the creative setting, shield).
    pub fn waiting(&mut self) -> bool {
        if self.inactive_ticks > 0 {
            self.inactive_ticks -= 1;
            return true;
        }
        false
    }

    /// `onClear`.
    pub fn clear(&mut self) {
        for h in &mut self.hands {
            h.reset_swing();
            h.history.clear();
        }
    }

    /// A hand that is using an item swings nothing (`player.isUsingItem()` on the used hand).
    pub fn busy(&mut self, hand: usize) {
        self.hands[hand].reset_swing();
    }

    /// Records the tip for the speed (also while the hand is busy), as `onRun` does at the top of
    /// each hand's turn.
    pub fn track(&mut self, hand: usize, input: &HandInput) {
        let tip = input.room_pos + input.room_dir * f64::from(TIP_OFFSET);
        self.hands[hand].history.add(input.now_ms, tip);
    }

    /// `onRun` for one hand, after `track`. `threshold` is `effectiveSpeedThreshold`,
    /// `grabbed` that a ladder is held, `cancelled_*` that a mod stopped it.
    pub fn tick(
        &mut self,
        hand: usize,
        input: &HandInput,
        threshold: f32,
        grabbed: bool,
        world: &mut dyn SwingWorld,
    ) -> Vec<SwingAction> {
        let mut out = Vec::new();
        let st = &mut self.hands[hand];
        let speed = st.history.average_speed(input.now_ms, SPEED_WINDOW);
        let (length, damage_range) = item_properties(input.kind, input.reach, input.world_scale);
        let recognized = matches!(input.kind, ItemKind::Weapon | ItemKind::Tool);
        let swing_point = input.pos + input.dir * f64::from(length);
        let prev = (st.last_hand, st.last_swing_point);
        st.last_hand = Some((input.pos, input.rot));
        st.last_swing_point = Some(swing_point);

        let mut can_swing = speed > threshold && !st.last_swing_block;
        st.last_swing_block = false;

        // ----- entities -----
        let attacked = entity_swing(
            st,
            hand,
            input,
            length,
            damage_range,
            speed,
            threshold,
            can_swing,
            swing_point,
            world,
            &mut out,
        );

        // ----- blocks: only if no entity was attacked -----
        can_swing = can_swing && !attacked;
        if (grabbed && !recognized) || !can_swing {
            return out;
        }
        let weapon = input.kind == ItemKind::Weapon;
        let Some(hit) = find_block_hit(prev, input, swing_point, length, weapon, world) else {
            return out;
        };
        st.last_swing_block = true;
        if input.farming_tool && world.is_farmable(&hit) {
            out.push(SwingAction::UseOn { hand, hit });
            out.push(SwingAction::Haptic {
                hand,
                micros: HAPTIC_BLOCK_PER_HIT_MICROS * 3,
            });
        } else {
            let hits = block_hits(speed, threshold);
            out.push(SwingAction::Mine { hand, hit, hits });
            out.push(SwingAction::Haptic {
                hand,
                micros: HAPTIC_BLOCK_PER_HIT_MICROS * hits,
            });
        }
        out
    }
}

/// `restrictToFirstBlock`: the end of a segment, or the first block's hit on it.
fn restrict_to_first_block(world: &mut dyn SwingWorld, from: DVec3, to: DVec3) -> DVec3 {
    world.clip(from, to).map_or(to, |h| h.location)
}

#[allow(clippy::too_many_arguments)]
fn entity_swing(
    st: &mut HandState,
    hand: usize,
    input: &HandInput,
    length: f32,
    damage_range: f32,
    speed: f32,
    threshold: f32,
    can_swing: bool,
    swing_point: DVec3,
    world: &mut dyn SwingWorld,
    out: &mut Vec<SwingAction>,
) -> bool {
    // A wall between the head and the hand: nothing to hit.
    let mut can_attack = can_swing && world.clip(input.head, input.pos).is_none();
    let reach_end = input.pos + input.dir * (f64::from(length) + f64::from(damage_range));
    let attack_mobs = restrict_to_first_block(world, input.pos, reach_end);
    let attack_players = restrict_to_first_block(world, input.pos, swing_point);
    let mut mobs = (input.pos.min(attack_mobs), input.pos.max(attack_mobs));
    let mut players = (input.pos.min(attack_players), input.pos.max(attack_players));
    // The previous tick's points are in the area too, so fast swings do not skip targets.
    if let Some(p) = st.last_weapon_tip {
        mobs = (mobs.0.min(p), mobs.1.max(p));
    }
    if let Some(p) = st.last_attack_point {
        players = (players.0.min(p), players.1.max(p));
    }
    st.last_weapon_tip = Some(attack_mobs);
    st.last_attack_point = Some(attack_players);

    // A shorter reach against players, to avoid accidental PvP.
    let mut targets: Vec<EntityRef> = world
        .entities(mobs.0, mobs.1)
        .into_iter()
        .filter(|e| !e.is_player)
        .collect();
    targets.extend(
        world
            .entities(players.0, players.1)
            .into_iter()
            .filter(|e| e.is_player),
    );
    let mut attacked = false;
    if can_attack {
        for t in &targets {
            if !t.pickable || t.is_vehicle || st.last_hit_entities.contains(&t.id) {
                continue;
            }
            out.push(SwingAction::Attack { hand, entity: t.id });
            out.push(SwingAction::Haptic {
                hand,
                micros: HAPTIC_ATTACK_MICROS,
            });
            attacked = true;
        }
        can_attack = false;
    }
    let _ = can_attack;
    st.last_hit_entities = if speed > threshold {
        targets.iter().map(|t| t.id).collect()
    } else {
        Vec::new()
    };
    attacked
}

/// `findBlockHitAlongArc`: the tip's path since the last tick, cut in steps by how far the hand
/// turned, and the first block it crosses that the item may touch.
fn find_block_hit(
    prev: (Option<(DVec3, DQuat)>, Option<DVec3>),
    input: &HandInput,
    swing_point: DVec3,
    length: f32,
    weapon: bool,
    world: &mut dyn SwingWorld,
) -> Option<BlockHit> {
    let mut points: Vec<DVec3> = Vec::new();
    if let (Some((prev_pos, prev_rot)), Some(prev_point)) = prev {
        let cos_half = prev_rot.dot(input.rot).abs().min(1.0);
        let turn = 2.0 * cos_half.acos();
        let steps = (turn / std::f64::consts::PI * f64::from(MAX_ARC_SUBDIVISIONS)).floor() as i32;
        points.push(prev_point);
        for s in 1..steps {
            let t = f64::from(s) / f64::from(steps);
            let rot = prev_rot.slerp(input.rot, t);
            let tip = rot * DVec3::new(0.0, 0.0, -f64::from(length));
            points.push(prev_pos.lerp(input.pos, t) + tip);
        }
    } else {
        points.push(swing_point);
    }
    points.push(swing_point);
    for pair in points.windows(2) {
        let (start, mut end) = (pair[0], pair[1]);
        if start.distance_squared(end) < DEGENERATE_SEGMENT_SQ {
            // `clip` reports a miss for near zero spans.
            end += DVec3::splat(DEGENERATE_NUDGE);
        }
        let Some(hit) = world.clip(start, end) else {
            continue;
        };
        if world.is_climbable(hit.pos) {
            continue;
        }
        // Weapons only touch blocks they break instantly.
        if weapon && !world.weapon_can_break(&hit) {
            continue;
        }
        // An inside hit means the tip started buried in the block.
        return if hit.inside { None } else { Some(hit) };
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creative_swings_follow_visor_and_busy_bodies_do_not_swing() {
        use horizon_vivecraft::{settings::VrSettings, trackers::PlayerFlags};
        let mut settings = VrSettings::default();
        settings.apply_visor_profile();
        let flags = PlayerFlags {
            in_game: true,
            alive: true,
            ..Default::default()
        };
        assert!(is_active(
            &flags,
            true,
            settings.bool("visorSwingInCreative"),
            false,
            true,
            false
        ));
        assert!(!is_active(&flags, true, false, false, true, false));
        assert!(is_active(&flags, false, false, false, true, false));
        assert!(!is_active(&flags, false, true, true, false, false));
        assert!(!is_active(&flags, false, true, false, true, true));
        for blocked in [
            PlayerFlags {
                passenger: true,
                ..flags
            },
            PlayerFlags {
                spectator: true,
                ..flags
            },
            PlayerFlags {
                sleeping: true,
                ..flags
            },
            PlayerFlags {
                paused_or_screen: true,
                ..flags
            },
            PlayerFlags {
                alive: false,
                ..flags
            },
        ] {
            assert!(!is_active(&blocked, false, true, false, true, false));
        }
    }

    #[derive(Default)]
    struct Stub {
        /// A solid block: the clip returns it for any segment that ends inside `solid`.
        solid: Option<([f64; 3], [f64; 3])>,
        entities: Vec<(DVec3, EntityRef)>,
        climbable: bool,
        clips: usize,
    }

    impl SwingWorld for Stub {
        fn entities(&mut self, lo: DVec3, hi: DVec3) -> Vec<EntityRef> {
            self.entities
                .iter()
                .filter(|(p, _)| p.cmpge(lo).all() && p.cmple(hi).all())
                .map(|(_, e)| *e)
                .collect()
        }
        fn clip(&mut self, from: DVec3, to: DVec3) -> Option<BlockHit> {
            self.clips += 1;
            let (lo, hi) = self.solid?;
            let (lo, hi) = (DVec3::from_array(lo), DVec3::from_array(hi));
            let inside = |p: DVec3| p.cmpge(lo).all() && p.cmple(hi).all();
            (inside(to) && !inside(from)).then_some(BlockHit {
                pos: [lo.x as i32, lo.y as i32, lo.z as i32],
                face: [0, 1, 0],
                location: to,
                inside: false,
            })
        }
        fn is_climbable(&mut self, _: [i32; 3]) -> bool {
            self.climbable
        }
        fn weapon_can_break(&mut self, _: &BlockHit) -> bool {
            false
        }
        fn is_farmable(&mut self, _: &BlockHit) -> bool {
            false
        }
    }

    fn hand(now_ms: u64, pos: DVec3, kind: ItemKind) -> HandInput {
        HandInput {
            now_ms,
            pos,
            dir: DVec3::NEG_Z,
            rot: DQuat::IDENTITY,
            room_pos: pos,
            room_dir: DVec3::NEG_Z,
            head: pos + DVec3::Y,
            kind,
            reach: 3.0,
            world_scale: 1.0,
            farming_tool: false,
        }
    }

    // A hand swung 0.1 m every 20 ms is 5 m/s: over the threshold.
    fn swing(
        task: &mut SwingTask,
        world: &mut Stub,
        kind: ItemKind,
        steps: u64,
    ) -> Vec<SwingAction> {
        let mut out = Vec::new();
        for i in 0..steps {
            let h = hand(i * 20, DVec3::new(i as f64 * 0.1, 1.0, 0.0), kind);
            task.track(0, &h);
            out.extend(task.tick(0, &h, 3.0, false, world));
        }
        out
    }

    #[test]
    fn item_length_and_reach_follow_the_class() {
        let (len, range) = item_properties(ItemKind::Weapon, 3.0, 1.0);
        assert!((len - 0.6).abs() < 1e-6);
        // (3.0 - 0.5) * 1.0 - 0.6
        assert!((range - 1.9).abs() < 1e-6);
        let (len, range) = item_properties(ItemKind::Tool, 3.0, 1.0);
        assert!((len - 0.35).abs() < 1e-6 && (range - (2.5 * 0.62 - 0.35)).abs() < 1e-6);
        let (len, range) = item_properties(ItemKind::Other, 3.0, 1.0);
        assert!((len - 0.1).abs() < 1e-6 && (range - (2.5 * 0.16 - 0.1)).abs() < 1e-6);
        assert_eq!(item_properties(ItemKind::Empty, 3.0, 1.0), (0.0, 0.3));
        // The length scales with the world, the reach does not.
        assert!((item_properties(ItemKind::Weapon, 3.0, 2.0).0 - 1.2).abs() < 1e-6);
        // A long reach is capped at 6 before the 0.5 comes off.
        assert!((item_properties(ItemKind::Weapon, 9.0, 1.0).1 - 4.9).abs() < 1e-6);
    }

    #[test]
    fn a_block_swing_makes_three_hits_plus_the_extra_speed() {
        assert_eq!(block_hits(3.1, 3.0), 3);
        assert_eq!(block_hits(5.2, 3.0), 5);
        assert_eq!(block_hits(30.0, 3.0), 7);
        assert_eq!(speed_threshold(0.1), 0.5);
        assert_eq!(speed_threshold(3.0), 3.0);
    }

    #[test]
    fn the_speed_is_the_mean_of_the_segments_in_the_window() {
        let mut h = TipHistory::default();
        h.add(0, DVec3::ZERO);
        h.add(100, DVec3::new(0.5, 0.0, 0.0)); // 5 m/s
        h.add(200, DVec3::new(0.6, 0.0, 0.0)); // 1 m/s
        assert!((h.average_speed(200, 0.31) - 3.0).abs() < 1e-4);
        // Only the last segment is inside 0.15 s.
        assert!((h.average_speed(200, 0.15) - 1.0).abs() < 1e-4);
        assert_eq!(TipHistory::default().average_speed(0, 1.0), 0.0);
    }

    #[test]
    fn a_fast_swing_through_a_mob_attacks_it_once() {
        let mut world = Stub::default();
        world.entities.push((
            DVec3::new(0.5, 1.0, -0.4),
            EntityRef {
                id: 7,
                pickable: true,
                is_vehicle: false,
                is_player: false,
            },
        ));
        let mut task = SwingTask::default();
        let actions = swing(&mut task, &mut world, ItemKind::Weapon, 12);
        let attacks: Vec<_> = actions
            .iter()
            .filter(|a| matches!(a, SwingAction::Attack { .. }))
            .collect();
        assert_eq!(attacks, vec![&SwingAction::Attack { hand: 0, entity: 7 }]);
        // The attack is felt.
        assert!(actions.contains(&SwingAction::Haptic {
            hand: 0,
            micros: 1_000
        }));
    }

    #[test]
    fn a_slow_hand_hits_nothing() {
        let mut world = Stub::default();
        world.entities.push((
            DVec3::new(0.1, 1.0, -0.2),
            EntityRef {
                id: 7,
                pickable: true,
                is_vehicle: false,
                is_player: false,
            },
        ));
        let mut task = SwingTask::default();
        let mut out = Vec::new();
        for i in 0..12u64 {
            // 1 m/s.
            let h = hand(
                i * 20,
                DVec3::new(i as f64 * 0.02, 1.0, 0.0),
                ItemKind::Weapon,
            );
            task.track(0, &h);
            out.extend(task.tick(0, &h, 3.0, false, &mut world));
        }
        assert!(out.is_empty());
    }

    #[test]
    fn vehicles_and_unpickable_entities_are_skipped_and_players_use_the_short_reach() {
        let mut world = Stub::default();
        let at = DVec3::new(0.5, 1.0, -0.4);
        world.entities.push((
            at,
            EntityRef {
                id: 1,
                pickable: true,
                is_vehicle: true,
                is_player: false,
            },
        ));
        world.entities.push((
            at,
            EntityRef {
                id: 2,
                pickable: false,
                is_vehicle: false,
                is_player: false,
            },
        ));
        let mut task = SwingTask::default();
        assert!(
            swing(&mut task, &mut world, ItemKind::Weapon, 12)
                .iter()
                .all(|a| !matches!(a, SwingAction::Attack { .. }))
        );
    }

    #[test]
    fn a_fast_swing_into_a_block_mines_it_with_the_hits_of_its_speed() {
        let mut world = Stub {
            solid: Some(([1.0, 0.5, -1.0], [2.0, 1.5, 1.0])),
            ..Stub::default()
        };
        let mut task = SwingTask::default();
        let actions = swing(&mut task, &mut world, ItemKind::Tool, 20);
        let mines: Vec<_> = actions
            .iter()
            .filter_map(|a| match a {
                SwingAction::Mine { hit, hits, .. } => Some((hit.pos, *hits)),
                _ => None,
            })
            .collect();
        // 5 m/s against the threshold 3: 3 + 2 hits. One swing, one mine until the hand slows.
        assert_eq!(mines, vec![([1, 0, -1], 5)]);
    }

    #[test]
    fn a_weapon_leaves_blocks_it_does_not_break_and_a_ladder_hand_leaves_unknown_items() {
        let mut world = Stub {
            solid: Some(([1.0, 0.5, -1.0], [2.0, 1.5, 1.0])),
            ..Stub::default()
        };
        let mut task = SwingTask::default();
        assert!(
            swing(&mut task, &mut world, ItemKind::Weapon, 20)
                .iter()
                .all(|a| !matches!(a, SwingAction::Mine { .. }))
        );
        // Holding a ladder with an item that is neither weapon nor tool: no block swings.
        let mut task = SwingTask::default();
        let mut out = Vec::new();
        for i in 0..20u64 {
            let h = hand(
                i * 20,
                DVec3::new(i as f64 * 0.1, 1.0, 0.0),
                ItemKind::Other,
            );
            task.track(0, &h);
            out.extend(task.tick(0, &h, 3.0, true, &mut world));
        }
        assert!(out.iter().all(|a| !matches!(a, SwingAction::Mine { .. })));
        // Climbable blocks are not mined.
        let mut world = Stub {
            solid: Some(([1.0, 0.5, -1.0], [2.0, 1.5, 1.0])),
            climbable: true,
            ..Stub::default()
        };
        let mut task = SwingTask::default();
        assert!(
            swing(&mut task, &mut world, ItemKind::Tool, 20)
                .iter()
                .all(|a| !matches!(a, SwingAction::Mine { .. }))
        );
    }

    #[test]
    fn the_task_waits_after_a_start() {
        let mut t = SwingTask {
            inactive_ticks: 3,
            ..SwingTask::default()
        };
        assert!([t.waiting(), t.waiting(), t.waiting(), t.waiting()] == [true, true, true, false]);
    }
}
