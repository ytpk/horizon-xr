//! Geometry the frame builds on the CPU each time it draws: the breaking cracks (`cracks`), the sun,
//! moon and stars (`celestial_geometry`), the first-person hand and arm (`hand_geometry`,
//! `first_person_arm`) and the rain and snow around the camera (`weather_geometry`).
use super::*;

impl Game<'_> {
    /// The cracks to draw and their stage sprite's atlas origin: our own mining and the
    /// other players' (`renderBlockDestroyAnimation`: within 32 blocks of the camera, the
    /// furthest progress on each block).
    pub(super) fn cracks(&self, camera: glam::DVec3) -> Vec<([i32; 3], [f32; 2])> {
        // `destroy_stage_0` .. `destroy_stage_9`: the sprite name by stage, so the
        // lookup does not format a string per crack.
        const DESTROY_STAGES: [&str; 10] = [
            "block/destroy_stage_0",
            "block/destroy_stage_1",
            "block/destroy_stage_2",
            "block/destroy_stage_3",
            "block/destroy_stage_4",
            "block/destroy_stage_5",
            "block/destroy_stage_6",
            "block/destroy_stage_7",
            "block/destroy_stage_8",
            "block/destroy_stage_9",
        ];
        // `ModelBakery.DESTROY_STAGES_OIT`: with improved transparency the cracks are
        // ordinary translucent quads (`OIT_CRUMBLING`) and use their own sprites.
        const DESTROY_STAGES_OIT: [&str; 10] = [
            "block/destroy_oit_stage_0",
            "block/destroy_oit_stage_1",
            "block/destroy_oit_stage_2",
            "block/destroy_oit_stage_3",
            "block/destroy_oit_stage_4",
            "block/destroy_oit_stage_5",
            "block/destroy_oit_stage_6",
            "block/destroy_oit_stage_7",
            "block/destroy_oit_stage_8",
            "block/destroy_oit_stage_9",
        ];
        let names = if self.settings.improved_transparency {
            &DESTROY_STAGES_OIT
        } else {
            &DESTROY_STAGES
        };
        let mut stages: Vec<([i32; 3], u32)> = Vec::new();
        let mut add = |pos: [i32; 3], stage: u32| match stages.iter_mut().find(|c| c.0 == pos) {
            Some(c) => c.1 = c.1.max(stage),
            None => stages.push((pos, stage)),
        };
        if let Some(m) = &self.mining
            && m.progress > 0.0
        {
            add(m.pos, ((m.progress * 10.0) as u32).min(9));
        }
        let now = self.world.game_time;
        for (pos, stage, at) in self.destroying.values() {
            if now.saturating_sub(*at) <= 400 {
                add(*pos, u32::from(*stage));
            }
        }
        stages
            .into_iter()
            .filter(|(pos, _)| {
                let d = glam::DVec3::new(f64::from(pos[0]), f64::from(pos[1]), f64::from(pos[2]))
                    + 0.5
                    - camera;
                d.length_squared() <= 1024.0
            })
            .filter_map(|(pos, stage)| {
                let name = names.get(stage as usize)?;
                let r = self.gui.atlas.sprites.get(*name)?;
                Some((pos, [r.u0, r.v0]))
            })
            .collect()
    }
    /// SkyRenderer.renderSunMoonAndStars: bodies on a frame rotated Y -90 then X by angle.
    /// Returns the sun and moon quads, and the stars' frame and brightness
    /// (the renderer keeps their geometry, built once). The bodies fade with
    /// the rain (`rainBrightness = 1 - rainLevel`, their colour's alpha).
    pub(super) fn celestial_geometry(
        &self,
        sky: &horizon_client::environment::Sky,
        rain: f32,
    ) -> (Vec<Vertex>, Option<(glam::Mat4, f32)>) {
        let body_color =
            0x00FF_FFFF | (((1.0 - rain).clamp(0.0, 1.0) * 255.0).round() as u32) << 24;
        let mut out = Vec::new();
        let atlas = &self.gui.atlas;
        let frame = |angle: f32| {
            glam::Mat4::from_rotation_y((-90.0f32).to_radians())
                * glam::Mat4::from_rotation_x(angle.to_radians())
        };
        let mut body = |name: &str, angle: f32, size: f32, moon: bool| {
            let Some(r) = atlas.sprites.get(name).copied() else {
                return;
            };
            // How brightly HDR output lets the body shine (mesher::hdr_tier):
            // the moon at the low tier, not the sun's.
            let tier = if moon {
                horizon_client::mesher::LOW
            } else {
                horizon_client::mesher::VERY_BRIGHT
            };
            let m = frame(angle);
            let corners = [(-size, -size), (size, -size), (size, size), (-size, size)];
            // buildCelestialQuad (sun) vs buildMoonPhases: the moon's UVs run
            // the other way round.
            let uv = if moon {
                [[r.u1, r.v1], [r.u0, r.v1], [r.u0, r.v0], [r.u1, r.v0]]
            } else {
                [[r.u0, r.v0], [r.u1, r.v0], [r.u1, r.v1], [r.u0, r.v1]]
            };
            for ((x, z), uv) in corners.iter().zip(uv) {
                let p = m.transform_point3(glam::Vec3::new(*x, 100.0, *z));
                out.push(Vertex {
                    normal: [0; 4],
                    pos: p.to_array(),
                    uv,
                    layer: 2 | u32::from(tier) << 16,
                    color: body_color,
                    light: 0,

                    glint_uv: [0.0, 0.0],
                });
            }
        };
        body("environment/celestial/sun", sky.sun_angle, 30.0, false);
        let phases = [
            "full_moon",
            "waning_gibbous",
            "third_quarter",
            "waning_crescent",
            "new_moon",
            "waxing_crescent",
            "first_quarter",
            "waxing_gibbous",
        ];
        let phase = phases[(self.view_day_time() / 24000).rem_euclid(8) as usize];
        body(
            &format!("environment/celestial/moon/{phase}"),
            sky.moon_angle,
            20.0,
            true,
        );
        // SkyRenderer.renderStars: the static star buffer on the same frame.
        let stars =
            (sky.star_brightness > 0.0).then(|| (frame(sky.star_angle), sky.star_brightness));
        (out, stars)
    }
    /// `renderPlayerHand` (`AvatarRenderer.renderHand`): the player's own
    /// `left` or right arm with its skin at pose `m`, in its rest pose
    /// turned 0.1 radians outwards.
    pub(super) fn first_person_arm(
        &self,
        left: bool,
        m: glam::Mat4,
        light: u32,
        out: &mut Vec<Vertex>,
    ) {
        self.player_arm(left, m, light, if left { -0.1 } else { 0.1 }, out);
    }
    /// FirstPersonHandsAndItemsRenderer: the right arm or the held items of
    /// both hands in view space, converted to camera-relative world space,
    /// and where the arm and each item start, with the `currentRenderedItemId`
    /// a shader pack reads for them.
    pub(super) fn hand_geometry(&mut self, alpha: f64) -> (Vec<Vertex>, Vec<(u32, i32)>) {
        let mut out = Vec::new();
        let mut runs = Vec::new();
        // A headset draws menu hands over the scene.
        let mut gui_hands = Vec::new();
        // F1 hides both; a native HUD mode may keep the hands visible. The
        // replay player's hands remain visible in first person; freecam has
        // no player-view hand.
        if !fov::studio_draws_hand(
            self.studio.flying(),
            self.ui.hands_drawn(
                self.spectator(),
                self.sleeping.is_some(),
                self.camera_type.first_person(),
                self.player
                    .as_ref()
                    .is_some_and(|p| !p.health.dead_or_dying()),
                self.screen == Screen::Dead,
            ),
        ) {
            return (out, runs);
        }
        let a = alpha as f32;
        // FirstPersonHandsAndItemsRenderer: the swing goes to the hand that
        // swings; each hand has its inverse arm height (the swap scale is 1
        // for every item but spears).
        let swing = self.swing.animation(a);
        let attacks = Hand::BOTH.map(|h| {
            if self.swing.attack_hand() == h {
                swing
            } else {
                0.0
            }
        });
        let heights = self.heights.interpolated(a);
        // The hand sway: both hands turn by a tenth of the bobs' lag.
        let to_world = self.sway.to_world(a, &self.view);
        // Standing in a headset each hand is drawn at its own controller.
        let vr_hands = self.headset_hand_frames();
        let menu_hands = self.headset_menu_hands();
        let eye = self.eye_position().unwrap_or_default();
        let light = self.world.light(
            eye[0].floor() as i32,
            eye[1].floor() as i32,
            eye[2].floor() as i32,
        );
        let packed = (u32::from(light >> 4) * 16) | ((u32::from(light & 15) * 16) << 8);
        let shown = self.hands_shown();
        let used_hand = self.used_hand();
        let spinning = self
            .player
            .as_ref()
            .is_some_and(|p| p.auto_spin_attack_ticks > 0);
        let mut items = self.visible_hand_items();
        // Standing, a bow is held in the off hand's controller and the main
        // one draws the string (Vivecraft's `BowTracker`).
        let bow_in_off = vr_hands.is_some()
            && items[0]
                .as_ref()
                .is_some_and(|s| &*s.item == "minecraft:bow");
        // Each controller shows its hand, whatever is in use (vanilla hides
        // the other hand while a bow is drawn).
        let shown = if vr_hands.is_some() { [true; 2] } else { shown };
        if bow_in_off {
            items[1] = items[0].take();
            // The main hand holds the arrow at the string.
            if self.pad.headset.bow.notched() {
                items[0] = Some(ItemStack::new("minecraft:arrow", 1));
            }
        }
        let off_hand_empty = items[1].is_none();
        // Combat feel Legacy: a sword is held up while use is held, as 1.8.9's block looked.
        // Only the picture changes: the clicks and what they send stay 26.3's.
        let legacy_block = self.settings.extras.pvp.combat.legacy
            && self.right
            && self.screen == Screen::Playing
            && items[0]
                .as_ref()
                .is_some_and(|s| s.item.ends_with("_sword"))
            && match used_hand {
                None => true,
                Some(Hand::Off) => items[1]
                    .as_ref()
                    .is_some_and(|s| &*s.item == "minecraft:shield"),
                Some(Hand::Main) => false,
            };
        for (i, (hand, item)) in Hand::BOTH.into_iter().zip(items).enumerate() {
            if !shown[i] {
                continue;
            }
            let scale = item.as_ref().map_or(1.0, |stack| {
                self.pack
                    .item_renderer
                    .models
                    .swap_animation_scale(&horizon_client::item_render::stack_view(stack))
            });
            let attack = attacks[i];
            let inverse_height =
                horizon_client::first_person::equip_displacement(heights[i], scale);
            let mut canceled = false;
            self.native_render_hand(
                hand,
                item.as_ref(),
                a,
                attack,
                inverse_height,
                &mut canceled,
            );
            if canceled {
                continue;
            }
            let (mut x, mut y, mut z, mut pitch, mut yaw, mut roll, mut pose_scale) =
                (0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0);
            self.native_render_hand_pose(
                hand,
                item.as_ref(),
                a,
                attack,
                inverse_height,
                &mut x,
                &mut y,
                &mut z,
                &mut pitch,
                &mut yaw,
                &mut roll,
                &mut pose_scale,
            );
            // The main hand on its arm, the off hand on the other one.
            let mut left = self.settings.main_hand_left != (hand == Hand::Off);
            let mut controller = None;
            if let Some(frames) = &vr_hands {
                let Some(frame) = frames[i] else {
                    continue;
                };
                left = frame.left;
                controller = Some(frame);
                // `menuHandMain` and `menuHandOff`: a pointer box, not a hand.
                if menu_hands[i] {
                    self.headset_menu_hand(frame.at, (light >> 4).max(light & 15), &mut gui_hands);
                    continue;
                }
            }
            // No mod moved the hand (always so without one): no matrices to build.
            let pose = [x, y, z, pitch, yaw, roll, pose_scale];
            let to_world = if pose == [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0] {
                to_world
            } else {
                to_world * native_hooks::hand_pose_matrix(pose)
            };
            // A Legacy sword held up hides the off hand's shield.
            if controller.is_none() && hand == Hand::Off && legacy_block {
                continue;
            }
            let Some(stack) = item else {
                // renderArmWithItem: only the main hand's empty arm shows,
                // on the main arm (the left one with the main hand set to
                // left), and not while the player is invisible. A headset
                // shows both: each is a controller.
                if (hand == Hand::Off && vr_hands.is_none()) || self.own_invisible() {
                    continue;
                }
                // renderPlayerArm; a headset's arm reaches back from its controller.
                runs.push((out.len() as u32, 0));
                match controller {
                    Some(frame) => self.headset_arm(frame.at, left, attack, packed, &mut out),
                    None => {
                        let m = to_world
                            * horizon_client::first_person::player_arm_pose(
                                left,
                                inverse_height,
                                attack,
                            );
                        self.first_person_arm(left, m, packed, &mut out);
                    }
                }
                continue;
            };
            let item_id = self.rendered_item_id(&stack.item);
            if let Some(id) = maps::map_id(&stack) {
                use horizon_client::first_person::{MapHold, map_arm_poses, one_handed_map_pose};
                // A headset's map is held up in the hand, its arm under it.
                if let Some(frame) = controller {
                    let m = self.headset_held(
                        frame,
                        &stack,
                        false,
                        attack,
                        headset_hands::HeldUse::default(),
                        packed,
                        &mut out,
                        &mut runs,
                    );
                    runs.push((out.len() as u32, item_id));
                    self.held_map(m, id, packed, &mut out);
                    continue;
                }
                let hold = MapHold::of(hand, off_hand_empty);
                // renderOneHandedMap and renderTwoHandedMap: the arms under
                // the map, unless the player is invisible.
                if !self.own_invisible() {
                    runs.push((out.len() as u32, 0));
                    for (arm, pose) in map_arm_poses(hold, left, self.pitch, inverse_height, attack)
                    {
                        self.first_person_arm(arm, to_world * pose, packed, &mut out);
                    }
                }
                runs.push((out.len() as u32, item_id));
                match hold {
                    MapHold::TwoHanded => self.two_handed_map_geometry(
                        to_world,
                        attack,
                        inverse_height,
                        id,
                        packed,
                        &mut out,
                    ),
                    MapHold::OneHanded => self.held_map(
                        to_world * one_handed_map_pose(left, inverse_height, attack),
                        id,
                        packed,
                        &mut out,
                    ),
                }
                continue;
            }
            let mut owner = self.player_owner(Some(&stack));
            // A headset's bow (the main hand's, in the off controller) shows
            // how far its string is drawn.
            let owner_hand = if bow_in_off && i == 1 {
                Hand::Main
            } else {
                hand
            };
            if controller.is_some()
                && owner_hand == Hand::Main
                && let Some(remaining) = self.headset_bow_remaining(owner.use_duration)
            {
                owner.use_item_remaining_ticks = remaining;
            }
            // The other hand may hold an equal stack: only the used
            // hand's item is in use, only the main hand's selected.
            owner.using_this_item &= used_hand == Some(owner_hand);
            if owner_hand == Hand::Off {
                owner.is_selected = false;
                owner.is_carried = false;
            }
            // The hand renderer's copy of the main hand stack.
            owner.fishing_cast_with_this_item =
                self.main_hand_rod() && self.cast_rod() == Some(&stack);
            let using = owner.using_this_item && owner.use_item_remaining_ticks > 0;
            let ticks_used =
                owner.use_duration as f32 - (owner.use_item_remaining_ticks as f32 - a + 1.0);
            use horizon_client::first_person::ItemPose;
            // ItemUseAnimation.SPEAR.
            let spear = using
                .then(|| horizon_client::spear::Kinetic::of(&stack.item))
                .flatten()
                .map(|kinetic| ItemPose::Spear {
                    kinetic,
                    time_held: ticks_used,
                    ticks_since_hit: self.kinetic_hit_ticks(a),
                });
            let pose = match &*stack.item {
                _ if let Some(pose) = spear => pose,
                "minecraft:bow" if using => ItemPose::Bow { ticks_used },
                "minecraft:crossbow" => {
                    let charged = stack.data().is_some_and(|d| !d.charged.is_empty());
                    ItemPose::Crossbow {
                        ticks_used: (using && !charged).then_some(ticks_used),
                        charge_duration: owner.crossbow_charge_duration as f32,
                        // Only the main hand turns a charged crossbow aside.
                        charged: charged && hand == Hand::Main,
                    }
                }
                "minecraft:trident" if using => ItemPose::Trident { ticks_used },
                // ItemUseAnimation.BRUSH: applyBrushTransform.
                "minecraft:brush" if using => ItemPose::Brush {
                    remaining: owner.use_item_remaining_ticks as f32,
                    partial: a,
                },
                _ if using => {
                    let view = horizon_client::item_render::stack_view(&stack);
                    let mut animation = self
                        .pack
                        .item_renderer
                        .models
                        .components_view(&view)
                        .get("minecraft:consumable")
                        .map_or(horizon_world::consumable::UseAnimation::None, |c| {
                            horizon_client::first_person::consumable_animation(c)
                        });
                    self.native_use_animation(&stack, &mut animation);
                    item_use::use_pose(
                        animation,
                        owner.use_item_remaining_ticks as f32,
                        owner.use_duration as f32,
                        a,
                    )
                }
                _ if spinning => ItemPose::SpinAttack,
                // ItemInHandRenderer: a STAB swing of this hand jabs.
                _ if self
                    .swing
                    .current
                    .is_some_and(|c| c.hand == hand && c.animation.kind == 2) =>
                {
                    ItemPose::Stab
                }
                _ => ItemPose::Swing,
            };
            let mut m = to_world
                * horizon_client::first_person::item_pose(left, inverse_height, attack, pose);
            // PvP & HUD's shield size (100% is vanilla's), toward the hand holding it.
            let shield_size = self.settings.extras.pvp.tweaks.shield_size;
            if shield_size != 100 && &*stack.item == "minecraft:shield" {
                m *= glam::Mat4::from_scale(glam::Vec3::splat(f32::from(shield_size) / 100.0));
            }
            // A headset's hand holds the item as the right hand's
            // first-person view, as Vivecraft's does; Visor draws it with the third person
            // hand transform of its own side (`THIRD_PERSON_*_HAND`).
            let visor_hand = controller.is_some() && self.visor_mechanics();
            let physical_left = left;
            let left = left && controller.is_none();
            // Seeded by the player id plus the context ordinal.
            let (context, seed) = if visor_hand {
                if physical_left {
                    (
                        horizon_assets::item_model::DisplayContext::ThirdPersonLeftHand,
                        1,
                    )
                } else {
                    (
                        horizon_assets::item_model::DisplayContext::ThirdPersonRightHand,
                        2,
                    )
                }
            } else if left {
                (
                    horizon_assets::item_model::DisplayContext::FirstPersonLeftHand,
                    3,
                )
            } else {
                (
                    horizon_assets::item_model::DisplayContext::FirstPersonRightHand,
                    4,
                )
            };
            let cit_hand = if hand == Hand::Off {
                horizon_client::cit::Hand::Off
            } else {
                horizon_client::cit::Hand::Main
            };
            let state = self.resolve_item_in_hand(&stack, context, Some(&owner), seed, cit_hand);
            // Combat feel Legacy: 1.8.9's pose for a flat item drawn with vanilla's display.
            if controller.is_none()
                && self.settings.extras.pvp.combat.legacy
                && !state.layers.is_empty()
                && state
                    .layers
                    .iter()
                    .all(|l| horizon_client::first_person::is_modern_flat_display(&l.matrix, left))
                && let Some(legacy) = horizon_client::first_person::legacy_item_pose(
                    left,
                    inverse_height,
                    attack,
                    pose,
                    hand == Hand::Main && legacy_block,
                )
            {
                m = to_world * legacy;
            }
            // Its arm shows too.
            let m = match controller {
                Some(frame) => self.headset_held(
                    frame,
                    &stack,
                    state.uses_block_light(),
                    attack,
                    headset_hands::HeldUse {
                        using,
                        cooldown: self.items().get(&stack.item).is_some_and(|info| {
                            let group = horizon_client::cooldowns::group(
                                &stack,
                                info.use_cooldown.as_ref(),
                            );
                            self.cooldowns.is_on_cooldown(&group)
                        }),
                        ticks_used,
                        spinning,
                        stab: matches!(pose, ItemPose::Stab),
                        remaining: owner.use_item_remaining_ticks,
                        millis: self.pad.headset.vr.clock_ms(),
                    },
                    packed,
                    &mut out,
                    &mut runs,
                ),
                None => m,
            };
            runs.push((out.len() as u32, item_id));
            let light = self.world.light(
                eye[0].floor() as i32,
                eye[1].floor() as i32,
                eye[2].floor() as i32,
            );
            self.submit_item_world(
                &state,
                m,
                block_entity_render::vertex_light(light),
                &mut out,
            );
        }
        // The hand has its own submit storage, which never uses improved
        // transparency (`LevelRenderer` alone calls `setUseImprovedTransparency`):
        // a held player head stays classic.
        for vertex in &mut out {
            vertex.layer &= !horizon_client::gfx::OIT_ENTITY;
        }
        if vr_hands.is_some() && !self.ui.hide_gui {
            gui_hands.extend(std::mem::take(&mut self.hand_crosshair));
        }
        self.hand_gui_quads = (gui_hands.len() / 4) as u32;
        out.extend(gui_hands);
        (out, runs)
    }
    /// WeatherEffectRenderer: rain and snow columns within the weather radius.
    pub(super) fn weather_geometry(
        &mut self,
        camera: glam::DVec3,
        alpha: f64,
    ) -> Vec<horizon_client::weather::Vertex> {
        use horizon_client::weather;
        let (intensity, _) = self.view_weather(alpha as f32);
        if intensity <= 0.0 {
            return Vec::new();
        }
        let atlas = &self.gui.atlas;
        let (Some(rain_tex), Some(snow_tex)) = (
            atlas.sprites.get("environment/rain").copied(),
            atlas.sprites.get("environment/snow").copied(),
        ) else {
            return Vec::new();
        };
        let (cx, cy, cz) = (
            camera.x.floor() as i32,
            camera.y.floor() as i32,
            camera.z.floor() as i32,
        );
        // The 32x32 direction table supports at most a 15-block radius.
        let radius = self.settings.weather_radius.clamp(0, 15);
        let eye = camera.to_array();
        let mut rain = Vec::new();
        let mut snow_vertices = Vec::new();
        // Saturation keeps extreme camera coordinates from overflowing the loop.
        for z in cz.saturating_sub(radius)..=cz.saturating_add(radius) {
            for x in cx.saturating_sub(radius)..=cx.saturating_add(radius) {
                let top = self.rain_height(x, z);
                let y0 = cy.saturating_sub(radius).max(top);
                let y1 = cy.saturating_add(radius).max(top);
                if y1 == y0 {
                    continue;
                }
                let Some(snow) = self.precipitation(x, cy, z) else {
                    continue;
                };
                let parameters = self.weather_parameters.get(x, z);
                let light = self.world.light(x, top.max(cy), z);
                let coords = i32::from(light & 15) << 4 | i32::from(light >> 4) << 20;
                // Only light brightening is needed here; UV phases stay zero.
                let lc = if snow {
                    let (block, sky) = (coords >> 4 & 15, coords >> 20 & 15);
                    ((block * 3 + 15) / 4) << 4 | ((sky * 3 + 15) / 4) << 20
                } else {
                    coords
                };
                let column = weather::Column {
                    x,
                    z,
                    bottom_y: y0,
                    top_y: y1,
                    u_offset: 0.0,
                    v_offset: 0.0,
                    light_coords: lc,
                };
                let r = if snow { snow_tex } else { rain_tex };
                let vertices = weather::vertices(
                    &column,
                    parameters,
                    snow,
                    eye,
                    radius,
                    intensity,
                    [r.u0, r.v0, r.u1 - r.u0, r.v1 - r.v0],
                );
                if snow {
                    snow_vertices.extend(vertices);
                } else {
                    rain.extend(vertices);
                }
            }
        }
        // Vanilla draws every rain column before the snow ones.
        rain.extend(snow_vertices);
        rain
    }
}

impl Game<'_> {
    pub(super) fn player_arm(
        &self,
        left: bool,
        m: glam::Mat4,
        light: u32,
        z_rot: f32,
        out: &mut Vec<Vertex>,
    ) {
        let slim = self.player_model.as_ref().is_some_and(|m| m.slim);
        let Some(arm) = self.res.player_arm(slim, left).as_ref() else {
            return;
        };
        // The player's own skin, as the inventory's model shows it.
        let sprites = &self.gui.atlas.sprites;
        let Some(r) = self
            .player_model
            .as_ref()
            .and_then(|m| m.skin.as_deref())
            .and_then(|skin| sprites.get(skin))
            .or_else(|| sprites.get("entity/player/wide/steve"))
            .copied()
        else {
            return;
        };
        let pose = |p: &horizon_client::mobs::Part| {
            let mut pose = p.pose;
            pose[3] = 0.0;
            pose[4] = 0.0;
            pose[5] = if p.name == arm.name { z_rot } else { 0.0 };
            pose
        };
        horizon_client::mobs::render_model(arm, m, &pose, [r.u0, r.v0, r.u1, r.v1], light, 0, out);
    }
}
