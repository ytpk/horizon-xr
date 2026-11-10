//! Shields and other `blocks_attacks` items: raising them (`ShieldItem.use`,
//! from either hand), `LivingEntity.getItemBlockingWith` and
//! `applyItemBlocking` inputs, and what follows a block: item damage,
//! the block sound, attacker knockback (`blockedByItem`) and axes disabling
//! the shield (`Player.blockUsingItem`, `BlocksAttacks.disable`).
use super::Game;
use horizon_client::{
    health::{BlockOutcome, Blocking, HurtSource},
    items::ItemStack,
};
use horizon_core::random::RandomSource;
use horizon_world::blocking_rules::{BlockingRules, DamageReduction, ItemDamageFunction};

/// `Inventory.SLOT_OFFHAND`: the `using` slot of an off-hand item.
pub(super) const OFFHAND_SLOT: usize = 40;

impl Game<'_> {
    /// The stack in an inventory slot, the off hand at `OFFHAND_SLOT`. In two-handed play
    /// (`offhand_redirect`) the off hand is the hotbar slot the player chose
    /// (`VROffhandEquipment.get`), and an unchosen slot is an empty hand.
    pub(super) fn stack_in(&self, slot: usize) -> Option<&ItemStack> {
        if slot == OFFHAND_SLOT {
            self.offhand_cell().as_ref()
        } else {
            self.inventory.get(slot)?.as_ref()
        }
    }
    pub(super) fn stack_in_mut(&mut self, slot: usize) -> &mut Option<ItemStack> {
        if slot == OFFHAND_SLOT {
            self.offhand_cell_mut()
        } else {
            &mut self.inventory[slot]
        }
    }
    /// The cell the off hand is: the vanilla off-hand slot, or in two-handed play the hotbar slot
    /// the player chose (`VROffhandEquipment.get`); no slot chosen is an empty hand.
    pub(super) fn offhand_cell(&self) -> &Option<ItemStack> {
        static EMPTY: Option<ItemStack> = None;
        match self.offhand_redirect {
            None => &self.offhand,
            Some(hotbar) => usize::try_from(hotbar)
                .ok()
                .and_then(|i| self.inventory.get(i))
                .unwrap_or(&EMPTY),
        }
    }
    pub(super) fn offhand_cell_mut(&mut self) -> &mut Option<ItemStack> {
        match self.offhand_redirect {
            None => &mut self.offhand,
            Some(hotbar) => match usize::try_from(hotbar) {
                Ok(i) if i < self.inventory.len() => &mut self.inventory[i],
                // `VROffhandEquipment.set` with no slot: the write goes nowhere.
                _ => {
                    self.offhand_void = None;
                    &mut self.offhand_void
                }
            },
        }
    }
    /// The stack in the off hand, as `stack_in(OFFHAND_SLOT)` has it.
    pub(super) fn offhand_item(&self) -> Option<&ItemStack> {
        self.stack_in(OFFHAND_SLOT)
    }
    /// Runs `f` with the hand that mines and attacks acting: the off hand when `ACTIVE_HAND` is
    /// the off hand in two-handed play, else the hand already acting.
    pub(super) fn with_active_hand<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        if self.active_off && self.offhand_redirect.is_some() && self.hand == super::Hand::Main {
            self.with_hand(super::Hand::Off, f)
        } else {
            f(self)
        }
    }
    /// Two-handed play is on for the player in the game's seat.
    pub(super) fn two_handed_play(&self) -> bool {
        self.offhand_redirect.is_some()
    }
    /// The stack's `blocks_attacks`: its own component, else its item's.
    pub(super) fn blocks_attacks(
        &self,
        stack: &ItemStack,
    ) -> Option<std::borrow::Cow<'_, horizon_client::items::BlocksAttacks>> {
        horizon_client::items::BlocksAttacks::of_stack(stack, self.items().get(&stack.item))
    }
    /// `ShieldItem.use` (ItemUtils.startUsingInstantly) for the hand at `slot`;
    /// false when that hand holds no blocking item or it is on cooldown.
    pub(super) fn raise_shield(&mut self, slot: usize) -> bool {
        let Some(stack) = self.stack_in(slot) else {
            return false;
        };
        if self.blocks_attacks(stack).is_none() {
            return false;
        }
        if self.shield_cooldown == 0 {
            self.start_using_item();
        }
        true
    }
    /// `LivingEntity.getItemBlockingWith` and the inputs of `applyItemBlocking`
    /// for an attack from `from` (`DamageSource.getSourcePosition`).
    pub(super) fn player_blocking(
        &self,
        from: Option<[f64; 3]>,
        damage_type: &str,
        pierce: bool,
        projectile: bool,
    ) -> Option<Blocking> {
        let tags = &self.stations().enchantments.damage_tags;
        let matches = |spec: &str| match spec.strip_prefix('#') {
            Some(tag) => tags.get(tag).is_some_and(|t| t.contains(damage_type)),
            None => spec == damage_type,
        };
        // Vivecraft's roomscale shield: a VR player blocks with a shield held up against the hit
        // even when not using it.
        let roomscale = self
            .using
            .is_none()
            .then(|| {
                let bypassed = |c: &horizon_client::items::BlocksAttacks| {
                    c.bypassed_by.as_deref().is_some_and(matches)
                };
                self.vr_roomscale_shield(from, projectile, &bypassed)
                    .or_else(|| self.visor_roomscale_shield(from, projectile, &bypassed))
            })
            .flatten();
        let (slot, ticks) = match (self.using, roomscale) {
            (Some(using), _) => using,
            (None, Some((slot, _))) => (slot, 1_000),
            (None, None) => return None,
        };
        self.vr_shield_slot.set(roomscale.map(|r| r.0));
        let component = self.blocks_attacks(self.stack_in(slot)?)?;
        // BlocksAttacks.blockDelayTicks: Math.round(seconds * 20).
        if (ticks as i32) < (component.delay_seconds * 20.0).round() as i32 {
            return None;
        }
        let p = self.player.as_ref()?;
        let angle = match from {
            // The roomscale shield passes the dot product as the angle, as Vivecraft does.
            _ if let Some((_, dot)) = roomscale => dot,
            Some(src) => {
                // calculateViewVector(0, yHeadRot) against the flattened direction.
                let yaw = (-self.yaw).to_radians();
                let view = [yaw.sin() as f64, yaw.cos() as f64];
                let (dx, dz) = (src[0] - p.position.x, src[2] - p.position.z);
                let len = (dx * dx + dz * dz).sqrt();
                let to = if len < 1.0e-4 {
                    [0.0, 0.0]
                } else {
                    [dx / len, dz / len]
                };
                (to[0] * view[0] + to[1] * view[1]).acos()
            }
            None => f64::from(std::f32::consts::PI),
        };
        let (threshold, base, factor) = component.item_damage;
        Some(Blocking {
            rules: BlockingRules {
                delay_seconds: component.delay_seconds,
                disable_scale: component.disable_scale,
                reductions: component
                    .reductions
                    .iter()
                    .map(|r| DamageReduction {
                        horizontal_angle: r.horizontal_angle,
                        types: r.types.as_ref().map(|t| t.iter().any(|s| matches(s))),
                        base: r.base,
                        factor: r.factor,
                    })
                    .collect(),
                item_damage: ItemDamageFunction {
                    threshold,
                    base,
                    factor,
                },
            },
            bypassed: component.bypassed_by.as_deref().is_some_and(matches),
            pierce,
            angle,
        })
    }
    /// After a hurt with the shield raised: `hurtBlockingItem`, `onBlocked`
    /// and, for melee from a mob, `blockUsingItem`. `source` is the hit's.
    pub(super) fn after_block(
        &mut self,
        outcome: BlockOutcome,
        (attacker, projectile): (Option<u32>, bool),
        source: HurtSource<'_>,
    ) {
        let slot = match self.using {
            Some((slot, _)) => slot,
            None => match self.vr_shield_slot.take() {
                Some(slot) => slot,
                None => return,
            },
        };
        let Some(stack) = self.stack_in(slot).cloned() else {
            return;
        };
        let Some(component) = self
            .blocks_attacks(&stack)
            .map(std::borrow::Cow::into_owned)
        else {
            return;
        };
        let at = self
            .player
            .as_ref()
            .map(|p| [p.position.x, p.position.y, p.position.z]);
        if outcome.sound
            && let Some(sound) = &component.block_sound
        {
            let pitch = 0.8 + self.world.random.next_float() * 0.4;
            self.audio.play(sound, at, 1.0, pitch);
        }
        if outcome.blocked <= 0.0 {
            return;
        }
        // ItemDamageFunction.apply, then hurtAndBreak.
        let (threshold, base, factor) = component.item_damage;
        let item_damage = if outcome.blocked < threshold {
            0
        } else {
            (base + factor * outcome.blocked).floor() as i32
        };
        if item_damage > 0
            && !self.creative
            && let Some(max) = blocking_item_max_damage(
                &stack,
                self.items()
                    .get(&stack.item)
                    .map_or(0, |item| item.max_damage),
            )
        {
            let loss = self.wear(&stack, item_damage as u32);
            let cell = self.stack_in_mut(slot);
            if loss > 0
                && let Some(s) = cell.as_mut()
            {
                s.damage = s.damage.saturating_add(loss).min(max);
                if s.damage >= max {
                    let broken = s.clone();
                    s.count -= 1;
                    if s.count == 0 {
                        *cell = None;
                    }
                    self.using = None;
                    // ItemStack.hurtAndBreak: onEquippedItemBroken for the hand it was used from.
                    let hand = if slot == OFFHAND_SLOT {
                        horizon_world::player_inventory::EquipmentSlot::OffHand
                    } else {
                        horizon_world::player_inventory::EquipmentSlot::MainHand
                    };
                    self.player_equipment_broke(hand, &broken);
                    return;
                }
            }
        }
        if projectile {
            return;
        }
        let Some(id) = attacker else {
            // Another player's melee hit has no mob behind it.
            if let Some((from, disable)) = self.combat.player_attacker {
                let fully = outcome.fully_blocked;
                let attacker = ([from[0], from[2]], disable);
                self.blocked_by_attacker(&component, attacker, fully, at, source);
            }
            return;
        };
        let fully = outcome.fully_blocked;
        let Some(m) = self.mobs.iter().find(|m| m.id == id) else {
            return;
        };
        let (mx, mz) = (m.position.x, m.position.z);
        // LivingEntity.getSecondsToDisableBlocking: the held weapon's value;
        // Warden overrides it with a flat 5 seconds.
        let disable = if m.species() == horizon_client::mobs::Species::Warden {
            5.0
        } else {
            m.extra
                .main_hand
                .as_deref()
                .and_then(|item| self.items().get(item))
                .map_or(0.0, |i| i.disable_blocking_seconds)
        };
        // Ravager.blockedByItem: stunned half the time (the defender pushed
        // apart), else the defender flung away; no roar in progress.
        if m.species() == horizon_client::mobs::Species::Ravager {
            let mut events = Vec::new();
            let roaring = m.extra.anim_ticks != 0;
            let stunned = self
                .mobs
                .iter_mut()
                .find(|m| m.id == id)
                .is_some_and(|m| horizon_client::mobs::ravager::blocked(m, &mut events));
            if !roaring && let Some(p) = self.player.as_mut() {
                let (xd, zd) = (p.position.x - mx, p.position.z - mz);
                let push = if stunned {
                    // Entity.push(entity): apart by 0.05 per block of overlap.
                    let d = (xd * xd + zd * zd).sqrt().max(0.01);
                    let k = (1.0 / d).min(1.0) * 0.05;
                    horizon_core::geometry::Vec3::new(xd / d * k, 0.0, zd / d * k)
                } else {
                    let dd = (xd * xd + zd * zd).max(0.001);
                    horizon_core::geometry::Vec3::new(xd / dd * 4.0, 0.2, zd / dd * 4.0)
                };
                p.velocity = p.velocity.add_vector(push);
            }
            let _ = self.handle_mob_events(events);
            return;
        }
        self.blocked_by_attacker(&component, ([mx, mz], disable), fully, at, source);
    }

    /// What a blocked melee hit does to the defender once the shield took
    /// it: `LivingEntity.blockedByItem` knocks them back unless fully
    /// blocked, then `Player.blockUsingItem` disables the shield. `attacker`
    /// is where the attacker stands (x, z) and its seconds of disabling.
    fn blocked_by_attacker(
        &mut self,
        component: &horizon_client::items::BlocksAttacks,
        ([mx, mz], disable): ([f64; 2], f32),
        fully: bool,
        at: Option<[f64; 3]>,
        source: HurtSource<'_>,
    ) {
        // LivingEntity.blockedByItem: defender.knockback(0.5, defender - attacker,
        // source), which moves the defender along -(xd, zd).
        if !fully && let Some(p) = self.player.as_ref() {
            let (xd, zd) = (p.position.x - mx, p.position.z - mz);
            self.knock_back_player(0.5, xd, zd, source);
        }
        // BlocksAttacks.disable: item cooldown and stopUsingItem.
        let ticks = disable * component.disable_scale;
        let ticks = if ticks > 0.0 {
            (ticks * 20.0).round() as u32
        } else {
            0
        };
        if ticks > 0 {
            if let (Some(player), Some(name)) = (self.lan_acting_uuid, self.lan_acting_name.clone())
            {
                self.native_player_shield_disable(player, &name, ticks);
            }
            let used = self
                .using
                .and_then(|(slot, _)| self.stack_in(slot))
                .cloned();
            let item = used.as_ref().map(|stack| stack.item.clone());
            let mut cancel = false;
            if let (Some(player), Some(name), Some(item)) =
                (self.lan_acting_uuid, self.lan_acting_name.clone(), item)
            {
                self.native_player_item_cooldown(player, &name, &item, ticks as i32, &mut cancel);
            }
            if !cancel {
                self.shield_cooldown = ticks;
                // ItemCooldowns.addCooldown(stack, ticks) on the shield's group: a guest is
                // sent it (ServerItemCooldowns), which keeps the shield down on their screen,
                // and the 0 that ends it.
                if let Some(stack) = &used {
                    let default = self
                        .items()
                        .get(&stack.item)
                        .and_then(|i| i.use_cooldown.clone());
                    let group = horizon_client::cooldowns::group(stack, default.as_ref());
                    self.add_cooldown(group, ticks as i32);
                }
            }
            self.using = None;
            if let Some(sound) = &component.disabled_sound {
                let pitch = 0.8 + self.world.random.next_float() * 0.4;
                self.audio.play(sound, at, 0.8, pitch);
            }
        }
    }
}

#[cfg(test)]
mod vr_offhand_tests {
    use super::*;
    use crate::game::Hand;

    /// A game on the prepared resources with a player (as `crossbow.rs` builds one).
    fn game_with_player(root: &std::path::Path) -> Game<'static> {
        use crate::game::{Audio, Options, Progress, Settings, load_resources};
        let settings = Settings::default();
        let loaded = load_resources(
            root.to_path_buf(),
            None,
            root,
            &settings,
            std::time::Instant::now(),
            false,
            true,
            &Progress::new(),
            &mut |_, _| unreachable!(),
        )
        .unwrap();
        let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
        options.resources = root.to_path_buf();
        options.execution_role = crate::game::execution_role::ExecutionRole::IntegratedServer;
        let res: &'static _ = Box::leak(Box::new(loaded.resources));
        let mut game = Game::new(
            res,
            loaded.pack,
            loaded.gui.unwrap(),
            options,
            settings,
            Audio::silent(root),
            None,
        )
        .unwrap();
        game.player = Some(
            horizon_client::motion::PlayerMotion::new(
                game.motion,
                horizon_core::geometry::Vec3::new(10.5, 64.0, -3.25),
            )
            .unwrap(),
        );
        game
    }

    fn game() -> Option<Game<'static>> {
        let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES")?);
        Some(game_with_player(&root))
    }

    /// `VROffhandEquipment.get` and `.set`: two-handed play makes the off hand the chosen hotbar
    /// slot for every reader and writer of the hand (use, blocking, totems, mending, the hand's
    /// item), while the vanilla off-hand slot stays untouched and a player with the mode off sees
    /// no difference.
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn two_handed_play_makes_the_off_hand_a_hotbar_slot_for_every_reader() {
        let Some(mut g) = game() else { return };
        g.inventory[3] = Some(ItemStack::new("minecraft:shield", 1));
        g.offhand = Some(ItemStack::new("minecraft:totem_of_undying", 1));
        // Vanilla (mode off): the off-hand slot, as ever.
        assert_eq!(g.offhand_redirect, None);
        assert_eq!(
            &*g.stack_in(OFFHAND_SLOT).unwrap().item,
            "minecraft:totem_of_undying"
        );
        g.update_totem_ready();
        assert!(g.player.as_ref().unwrap().health.totem_ready);
        // Mode on, slot 3: the shield is the off hand; the totem in the vanilla slot is out of reach.
        g.offhand_redirect = Some(3);
        assert_eq!(&*g.stack_in(OFFHAND_SLOT).unwrap().item, "minecraft:shield");
        assert_eq!(&*g.offhand_item().unwrap().item, "minecraft:shield");
        g.update_totem_ready();
        assert!(!g.player.as_ref().unwrap().health.totem_ready);
        // The hand that acts when the off hand does is that stack, and it raises like a shield.
        let held = g.with_hand(Hand::Off, |g| g.held().map(|s| s.item.to_string()));
        assert_eq!(held.as_deref(), Some("minecraft:shield"));
        assert!(g.raise_shield(OFFHAND_SLOT));
        // A write to the off hand lands in the hotbar slot, not the vanilla one.
        *g.stack_in_mut(OFFHAND_SLOT) = Some(ItemStack::new("minecraft:apple", 2));
        assert_eq!(&*g.inventory[3].as_ref().unwrap().item, "minecraft:apple");
        assert_eq!(
            &*g.offhand.as_ref().unwrap().item,
            "minecraft:totem_of_undying"
        );
        // No slot chosen (-1): an empty hand, and writes go nowhere.
        g.offhand_redirect = Some(-1);
        assert!(g.stack_in(OFFHAND_SLOT).is_none());
        *g.stack_in_mut(OFFHAND_SLOT) = Some(ItemStack::new("minecraft:stone", 1));
        assert!(g.stack_in(OFFHAND_SLOT).is_none());
        assert_eq!(&*g.inventory[3].as_ref().unwrap().item, "minecraft:apple");
        assert!(
            g.inventory
                .iter()
                .flatten()
                .all(|s| &*s.item != "minecraft:stone")
        );
        // The off hand is not the hand mining and attacking until ACTIVE_HAND says so.
        g.offhand_redirect = Some(3);
        g.inventory[g.selected] = Some(ItemStack::new("minecraft:iron_sword", 1));
        let main = g.with_active_hand(|g| g.held().map(|s| s.item.to_string()));
        assert_eq!(main.as_deref(), Some("minecraft:iron_sword"));
        g.active_off = true;
        let active = g.with_active_hand(|g| g.held().map(|s| s.item.to_string()));
        assert_eq!(active.as_deref(), Some("minecraft:apple"));
        // And the attribute modifiers follow the active hand's item.
        assert!(g.vr_dual_wield_swapped());
        g.active_off = false;
        assert!(!g.vr_dual_wield_swapped());
    }
}
/// `ItemStack.isDamageableItem` and `getMaxDamage` for the blocking stack.
fn blocking_item_max_damage(stack: &ItemStack, prototype_max: u32) -> Option<u32> {
    let data = stack.data();
    let component = |id| data.and_then(|data| data.other(id));
    if stack.count == 0
        || component("minecraft:unbreakable").is_some()
        || component("!minecraft:max_damage").is_some()
        || component("!minecraft:damage").is_some()
    {
        return None;
    }
    // A non-damageable prototype supplies neither MAX_DAMAGE nor DAMAGE.
    if prototype_max == 0
        && stack.damage == 0
        && !data.is_some_and(|data| data.explicit_zero_damage)
    {
        return None;
    }
    let max = match component("minecraft:max_damage") {
        Some(value) => horizon_world::datafix::dynamic::as_int(Some(value), 0).max(0) as u32,
        None => prototype_max,
    };
    (max > 0).then_some(max)
}
