// SPDX-License-Identifier: LGPL-3.0-or-later
//! Ports `SwingTracker` for the hands: a fast enough swing of a controller hits the entities
//! its weapon reaches and mines the block its tip crosses. Skipped (see
//! `docs/vivecraft-port.md`): feet trackers (full body tracking), piercing weapons and lunging,
//! door hitting, hoes, scythes and brushes, the debug rendering.
//! Copyright (c) Vivecraft contributors; LGPL-3.0-or-later (see LICENSE-LGPL-3.0).

use super::{Haptic, PlayerFlags};
use crate::crosshair::Face;
use crate::modes::{FreeMove, Tri};
use crate::player::Aabb;
use crate::settings::VrSettings;
use crate::vr_data::{Vec3History, VrData};
use glam::{DVec3, Quat, Vec3};

const SPEED_THRESHOLD: f32 = 2.5;

/// What kind of item is in the swinging hand (`isTool`, swords, spears, lances).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HeldItem {
    pub empty: bool,
    /// A sword, a trident or a Vivecraft spear.
    pub sword: bool,
    pub lance: bool,
    /// Tools and the items Vivecraft treats as tools (`isTool`).
    pub tool: bool,
}

/// An entity the weapon box overlaps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EntityHit {
    pub id: u32,
    pub pickable: bool,
    pub is_player: bool,
    /// It is the vehicle the player rides.
    pub is_vehicle: bool,
}

/// A block the weapon tip crosses.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlockHit {
    pub pos: [i32; 3],
    pub face: Face,
    pub location: DVec3,
    /// The segment started inside the block (`isInside`).
    pub inside: bool,
}

/// How the block is struck.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockKind {
    Normal,
    /// A note block or a Vivecraft music block: `continueDestroyBlock` (a tap).
    Music,
    /// A ladder, a vine or a climbable block: protected while climbing is on.
    Climbable,
    /// A sword only mines what it is the right tool for, or what breaks instantly.
    NotMineableBySword,
}

/// What the swing tracker asks of the world.
pub trait SwingWorld {
    /// `level.getEntities(player, bb)`.
    fn entities(&mut self, bb: Aabb) -> Vec<EntityHit>;
    /// `level.clip(from, to, OUTLINE, no fluid)`.
    fn clip(&mut self, from: DVec3, to: DVec3) -> Option<BlockHit>;
    fn block_kind(&mut self, hit: &BlockHit) -> BlockKind;
}

/// The inputs of a tick.
#[derive(Clone, Copy, Debug)]
pub struct SwingInput<'a> {
    pub now_ms: u64,
    /// The world data of the tick and the room data of the poll.
    pub world: &'a VrData,
    pub room: &'a VrData,
    pub held: [HeldItem; 2],
    pub creative: bool,
    /// The player's entity interaction range (blocks), or the held item's attack range.
    pub entity_reach: f64,
    pub shift_down: bool,
    pub grabbing_ladder: [bool; 2],
    /// `cannotAttackWithItem(stack, 0)` per hand.
    pub cannot_attack: [bool; 2],
    pub can_break_climbable_when_sneaking: bool,
}

/// What the game should do.
#[derive(Clone, Debug, PartialEq)]
pub enum SwingAction {
    /// `gameMode.attack(player, entity)` with the hand as the active body part.
    Attack {
        hand: usize,
        entity: u32,
    },
    /// `startDestroyBlock` then `hits` times `continueDestroyBlock`, with block dust.
    Mine {
        hand: usize,
        hit: BlockHit,
        hits: u32,
    },
    /// `continueDestroyBlock` once.
    Tap {
        hand: usize,
        hit: BlockHit,
    },
    Haptic(Haptic),
}

/// `SwingTracker`.
#[derive(Clone, Debug)]
pub struct SwingTracker {
    /// Ticks to wait before swinging again (after a teleport, a screen).
    pub disable_swing: i32,
    last_hand_rot: [Option<Quat>; 2],
    last_hand_pos: [Option<DVec3>; 2],
    last_weapon_solid: [bool; 2],
    last_hit_entities: [Vec<u32>; 2],
    mining_point: [Option<DVec3>; 2],
    attacking_point: [Option<DVec3>; 2],
    weapon_tip: [Option<DVec3>; 2],
    tip_history: [Vec3History; 2],
    tip_dir_history: [Vec3History; 2],
    can_act: [bool; 2],
    use_hold_ticks: [i32; 2],
}

impl Default for SwingTracker {
    fn default() -> Self {
        Self {
            disable_swing: 3,
            last_hand_rot: [None; 2],
            last_hand_pos: [None; 2],
            last_weapon_solid: [false; 2],
            last_hit_entities: [Vec::new(), Vec::new()],
            mining_point: [None; 2],
            attacking_point: [None; 2],
            weapon_tip: [None; 2],
            tip_history: Default::default(),
            tip_dir_history: Default::default(),
            can_act: [false; 2],
            use_hold_ticks: [0; 2],
        }
    }
}

impl SwingTracker {
    /// `isActive`: counts down `disable_swing` while it holds the tracker back.
    #[allow(clippy::too_many_arguments)]
    pub fn is_active(
        &mut self,
        s: &VrSettings,
        p: &PlayerFlags,
        creative: bool,
        blocking: bool,
        server_allows_attacking_while_blocking: bool,
        move_forward: bool,
        jumping: bool,
    ) -> bool {
        if self.disable_swing > 0 {
            self.disable_swing -= 1;
            return false;
        }
        if !p.in_game || !p.alive || p.sleeping || p.paused_or_screen {
            return false;
        }
        match s.weapon_collision() {
            Tri::Off => return false,
            Tri::Auto if creative => return false,
            _ => {}
        }
        if s.seated()
            || (s.free_move_mode(false) == FreeMove::RunInPlace && move_forward)
            || (blocking && !server_allows_attacking_while_blocking)
        {
            return false;
        }
        !jumping
    }

    pub fn inactive(&mut self) {
        self.last_hand_pos = [None; 2];
        self.attacking_point = [None; 2];
        self.weapon_tip = [None; 2];
        self.use_hold_ticks = [0; 2];
    }

    /// `itemInUse`: a held-use gesture (brushing) is going.
    pub fn item_in_use(&self) -> bool {
        self.use_hold_ticks.iter().any(|&t| t > 0)
    }

    /// Whether the hand's last swing hit something solid (`lastWeaponSolid`, for the item fade).
    pub fn weapon_solid(&self, hand: usize) -> bool {
        self.last_weapon_solid[hand.min(1)]
    }

    /// `getItemFade`: how opaque a held item is while the attack charges.
    pub fn item_fade(
        attack_strength: f32,
        shift_down: bool,
        weapon_solid: bool,
        blocking_other: bool,
        shield_idle: bool,
    ) -> f32 {
        let mut fade = attack_strength * 0.75 + 0.25;
        if shift_down {
            fade = 0.75;
        }
        if weapon_solid {
            fade -= 0.25;
        }
        if blocking_other {
            fade -= 0.25;
        }
        if shield_idle {
            fade -= 0.25;
        }
        fade.clamp(0.1, 1.0)
    }

    /// One logic tick (`activeProcess`) for the hands.
    pub fn tick(
        &mut self,
        s: &VrSettings,
        input: &SwingInput<'_>,
        world: &mut impl SwingWorld,
    ) -> Vec<SwingAction> {
        let mut actions = Vec::new();
        let threshold = SPEED_THRESHOLD * if input.creative { 1.5 } else { 1.0 };
        for i in 0..2 {
            if self.use_hold_ticks[i] > 0 {
                self.use_hold_ticks[i] -= 1;
            }
            if input.grabbing_ladder[i] {
                continue;
            }
            let held = input.held[i];
            if s.bool("onlySwordCollision") && !held.sword {
                continue;
            }
            let hand_pos = input.world.controller(i).position();
            let hand_dir = input.world.hand(i).custom_vector(Vec3::NEG_Z);
            let is_tool = held.sword || held.lance || held.tool;
            let reach = input.entity_reach.min(6.0) - 0.5;
            let (mut length, mut reach_add) = (0.0_f32, 0.3_f32);
            if held.sword || held.lance {
                length = 0.6;
                reach_add = reach as f32 - length;
            } else if is_tool {
                length = 0.35;
                reach_add = reach as f32 * 0.62 - length;
            } else if !held.empty {
                length = 0.1;
                reach_add = reach as f32 * 0.16 - length;
            }
            length *= input.world.world_scale;
            let prev_mining = self.mining_point[i];
            let mining = hand_pos + (hand_dir * length).as_dvec3();
            self.mining_point[i] = Some(mining);
            let base = input.room.controller(i).position_f();
            let tip = input.room.hand(i).custom_vector(Vec3::NEG_Z) * 0.3 + base;
            self.tip_history[i].add(input.now_ms, tip);
            self.tip_dir_history[i].add(
                input.now_ms,
                if is_tool {
                    input.room.hand(i).direction()
                } else {
                    input.room.controller(i).direction()
                },
            );
            let speed = self.tip_history[i].average_speed(input.now_ms, 0.33);
            self.can_act[i] =
                speed > threshold && !self.last_weapon_solid[i] && !input.cannot_attack[i];
            // Entities: not through a block between the head and the hand.
            let mut entity_act = self.can_act[i] && s.bool("entityCollision");
            if entity_act && world.clip(input.world.hmd().position(), hand_pos).is_some() {
                entity_act = false;
            }
            let last_attack = self.attacking_point[i];
            let last_tip = self.weapon_tip[i];
            let attacking = constrain(world, hand_pos, mining);
            self.attacking_point[i] = Some(attacking);
            let tip_end = hand_pos + (hand_dir * (length + reach_add)).as_dvec3();
            let weapon_tip = constrain(world, hand_pos, tip_end);
            self.weapon_tip[i] = Some(weapon_tip);
            let mut tip_bb = bb_of(hand_pos, weapon_tip);
            let mut weapon_bb = bb_of(hand_pos, attacking);
            if let Some(p) = last_attack {
                weapon_bb = include(weapon_bb, p);
            }
            if let Some(p) = last_tip {
                tip_bb = include(tip_bb, p);
            }
            let mut mobs = world.entities(tip_bb);
            if s.bool("reducedPlayerReach") {
                mobs.retain(|e| !e.is_player);
                let players: Vec<_> = world
                    .entities(weapon_bb)
                    .into_iter()
                    .filter(|e| e.is_player)
                    .collect();
                mobs.extend(players);
            }
            let mut in_an_entity = false;
            for e in &mobs {
                if e.pickable
                    && !e.is_vehicle
                    && !self.last_hit_entities[i].contains(&e.id)
                    && entity_act
                {
                    actions.push(SwingAction::Attack {
                        hand: i,
                        entity: e.id,
                    });
                    actions.push(SwingAction::Haptic(Haptic {
                        controller: i,
                        micros: 1000,
                    }));
                    self.last_weapon_solid[i] = true;
                    in_an_entity = true;
                }
            }
            self.last_hit_entities[i] = if speed > threshold {
                mobs.iter().map(|e| e.id).collect()
            } else {
                Vec::new()
            };
            self.can_act[i] &= !in_an_entity;

            // Blocks: along the tip's path since the last tick.
            let rotation = input.world.hand(i).orientation();
            let mut block: Option<(BlockHit, BlockKind)> = None;
            if self.can_act[i] && s.bool("blockCollision") {
                let mut points = Vec::new();
                match (self.last_hand_pos[i], prev_mining, self.last_hand_rot[i]) {
                    (Some(last_pos), Some(prev), Some(last_rot)) => {
                        let angle = 2.0 * last_rot.dot(rotation).abs().min(1.0).acos();
                        let subdivisions = (angle / std::f32::consts::PI * 8.0).floor() as i32;
                        points.push(prev);
                        for step in 1..subdivisions {
                            let t = step as f32 / subdivisions as f32;
                            let q = last_rot.slerp(rotation, t);
                            let at = last_pos + (hand_pos - last_pos) * f64::from(t);
                            points.push(at + (q * Vec3::new(0.0, 0.0, -length)).as_dvec3());
                        }
                    }
                    _ => points.push(mining),
                }
                points.push(mining);
                for pair in points.windows(2) {
                    let (start, mut end) = (pair[0], pair[1]);
                    if (start - end).length_squared() < 1.0e-7 {
                        end += DVec3::splat(0.001);
                    }
                    let Some(hit) = world.clip(start, end) else {
                        continue;
                    };
                    let kind = world.block_kind(&hit);
                    let protected = s.bool("realisticClimbEnabled")
                        && (!input.shift_down || !s.bool("allowBreakingClimbable"))
                        && kind == BlockKind::Climbable;
                    let sword_refuses = held.sword
                        && s.bool("swordBlockCollision")
                        && kind == BlockKind::NotMineableBySword;
                    if sword_refuses || protected {
                        continue;
                    }
                    block = Some((hit, kind));
                    break;
                }
            }
            self.last_hand_pos[i] = Some(hand_pos);
            self.last_hand_rot[i] = Some(rotation);
            match block {
                Some((hit, kind)) if !hit.inside && self.can_act[i] => {
                    self.last_weapon_solid[i] = true;
                    let mut total = 3u32;
                    if kind == BlockKind::Music {
                        actions.push(SwingAction::Tap { hand: i, hit });
                    } else {
                        total += (speed - threshold).clamp(0.0, 4.0) as u32;
                        actions.push(SwingAction::Mine {
                            hand: i,
                            hit,
                            hits: total,
                        });
                    }
                    actions.push(SwingAction::Haptic(Haptic {
                        controller: i,
                        micros: 250 * total,
                    }));
                }
                _ => self.last_weapon_solid[i] = in_an_entity && self.last_weapon_solid[i],
            }
        }
        actions
    }
}

fn bb_of(a: DVec3, b: DVec3) -> Aabb {
    Aabb {
        min: a.min(b),
        max: a.max(b),
    }
}

fn include(bb: Aabb, p: DVec3) -> Aabb {
    Aabb {
        min: bb.min.min(p),
        max: bb.max.max(p),
    }
}

fn constrain(world: &mut impl SwingWorld, start: DVec3, end: DVec3) -> DVec3 {
    world.clip(start, end).map_or(end, |h| h.location)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vr_data::{DataSettings, RoomPose, RoomPoses};

    /// A wall of blocks at z = -5, and an entity around z = -2 and beyond.
    struct Scene {
        entity: bool,
    }
    impl SwingWorld for Scene {
        fn entities(&mut self, bb: Aabb) -> Vec<EntityHit> {
            if self.entity && bb.min.z < -2.0 {
                vec![EntityHit {
                    id: 7,
                    pickable: true,
                    is_player: false,
                    is_vehicle: false,
                }]
            } else {
                Vec::new()
            }
        }
        fn clip(&mut self, from: DVec3, to: DVec3) -> Option<BlockHit> {
            // A wall: the plane z = -5 (north), hit going toward -Z.
            if from.z > -5.0 && to.z <= -5.0 {
                let t = (from.z + 5.0) / (from.z - to.z);
                let at = from + (to - from) * t;
                return Some(BlockHit {
                    pos: [0, 64, -5],
                    face: Face::South,
                    location: at,
                    inside: false,
                });
            }
            None
        }
        fn block_kind(&mut self, _: &BlockHit) -> BlockKind {
            BlockKind::Normal
        }
    }

    fn data(hand_z: f32) -> VrData {
        let head = RoomPose {
            pos: Vec3::new(0.0, 1.7, 0.0),
            rot: Quat::IDENTITY,
        };
        // Controllers pointing along -Z (their aim and hand directions).
        let hand = RoomPose {
            pos: Vec3::new(0.3, 1.2, hand_z),
            rot: Quat::IDENTITY,
        };
        let room = RoomPoses {
            hmd: head,
            eyes: [head; 2],
            aim: [hand; 2],
            hand: [hand; 2],
            tracking: [true; 2],
        };
        VrData::new(&room, DataSettings::default(), DVec3::ZERO, 1.0, 1.0, 0.0)
    }

    fn input<'a>(now: u64, world: &'a VrData, held: HeldItem) -> SwingInput<'a> {
        SwingInput {
            now_ms: now,
            world,
            room: world,
            held: [held; 2],
            creative: false,
            entity_reach: 3.0,
            shift_down: false,
            grabbing_ladder: [false; 2],
            cannot_attack: [false; 2],
            can_break_climbable_when_sneaking: false,
        }
    }

    fn sword() -> HeldItem {
        HeldItem {
            sword: true,
            tool: true,
            ..Default::default()
        }
    }

    #[test]
    fn a_slow_hand_hits_nothing() {
        let s = VrSettings::default();
        let mut t = SwingTracker::default();
        let mut scene = Scene { entity: true };
        for i in 0..10u64 {
            let d = data(-0.1 * i as f32 * 0.05);
            assert!(
                t.tick(&s, &input(i * 50, &d, sword()), &mut scene)
                    .is_empty()
            );
        }
    }

    #[test]
    fn a_fast_swing_into_a_wall_mines_it_with_more_hits_when_faster() {
        let s = VrSettings::default();
        let mut t = SwingTracker::default();
        let mut scene = Scene { entity: false };
        let mut got = Vec::new();
        // The controller flies forward at 8 m/s, 4 m of travel to the wall at z = -5 (head at 0).
        for i in 0..12u64 {
            let d = data(-0.4 * i as f32);
            got.extend(t.tick(&s, &input(i * 50, &d, sword()), &mut scene));
        }
        let mine = got.iter().find_map(|a| match a {
            SwingAction::Mine { hit, hits, .. } => Some((*hit, *hits)),
            _ => None,
        });
        let (hit, hits) = mine.expect("mined");
        assert_eq!(hit.pos, [0, 64, -5]);
        assert!(hits >= 3 + 3, "{hits}");
        assert!(got.iter().any(|a| matches!(a, SwingAction::Haptic(_))));
    }

    #[test]
    fn a_fast_swing_hits_an_entity_once_per_swing() {
        let mut s = VrSettings::default();
        s.set_bool("blockCollision", false);
        let mut t = SwingTracker::default();
        let mut scene = Scene { entity: true };
        let mut attacks = 0;
        for i in 0..8u64 {
            let d = data(-0.35 * i as f32);
            attacks += t
                .tick(&s, &input(i * 50, &d, sword()), &mut scene)
                .iter()
                .filter(|a| matches!(a, SwingAction::Attack { entity: 7, .. }))
                .count();
        }
        // Two hands, one entity each: the entity is hit once per hand, then not again.
        assert!((1..=2).contains(&attacks), "{attacks}");
    }

    #[test]
    fn the_tracker_is_gated_by_the_settings() {
        let mut s = VrSettings::default();
        let p = PlayerFlags {
            alive: true,
            in_game: true,
            ..Default::default()
        };
        let mut t = SwingTracker::default();
        for _ in 0..3 {
            assert!(!t.is_active(&s, &p, false, false, true, false, false));
        }
        assert!(t.is_active(&s, &p, false, false, true, false, false));
        // AUTO is off in creative; OFF is off.
        s.set("weaponCollisionNew", "AUTO");
        assert!(!t.is_active(&s, &p, true, false, true, false, false));
        assert!(t.is_active(&s, &p, false, false, true, false, false));
        s.set("weaponCollisionNew", "OFF");
        assert!(!t.is_active(&s, &p, false, false, true, false, false));
    }

    #[test]
    fn the_item_fade_follows_the_attack_charge() {
        assert_eq!(
            SwingTracker::item_fade(1.0, false, false, false, false),
            1.0
        );
        assert!((SwingTracker::item_fade(0.0, false, false, false, false) - 0.25).abs() < 1e-6);
        assert_eq!(SwingTracker::item_fade(1.0, true, true, true, true), 0.1);
    }
}
