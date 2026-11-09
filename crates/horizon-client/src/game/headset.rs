// SPDX-License-Identifier: LGPL-3.0-or-later
// Derived from Vivecraft: render/helpers/VREffectsHelper.java (crosshair), gameplay/trackers/*, gameplay/KeybindHandler.java. Copyright (c) Vivecraft contributors; LGPL-3.0-or-later.
//! A VR headset (`horizon_client::headset`, set by horizon-xr on Quest): the
//! main hand's controller aims (the head, seated), as a mouse would, the head
//! alone moves the camera, and on the menu panel the controller's pointer is
//! the mouse. The controllers' sticks and buttons come in as a pad
//! (`poll_pads`). Nothing here runs on a flat screen.
use super::event_loop::EventLoopCtl;
use super::headset_hands::HandFrame;
use super::input_trace::InputOrigin;
use super::{Game, Hand};
use horizon_client::headset;
use horizon_vivecraft::crosshair::{self, CrosshairContext, Face};
use winit::dpi::PhysicalPosition;
use winit::event::{DeviceId, ElementState, MouseButton, WindowEvent};

/// Where a controller's ray runs (world blocks) and what it meets.
pub(super) struct RayTarget {
    pub(super) start: [f64; 3],
    pub(super) end: [f64; 3],
    /// It meets a block's face (`Some(Some(normal))`), an entity
    /// (`Some(None)`), or nothing within reach.
    pub(super) hit: Option<Option<[i32; 3]>>,
}

/// The eye height (above the feet) the camera stands for in a headset, whose head height the
/// room supplies: `LocalPlayerMixin.visor$tickRoomYOffset` and `visor$isPoseModifyCamera`. The
/// pose's lower eye (sneak 1.27, crawl 0.4) must not be applied to the room origin as well as to
/// the real head, which already dropped (the head dropped twice, below the floor). Standing and
/// walking keep the pose's eye (the head's lean and bend are the offset from it), a sneak or a
/// crawl that has headroom stands for the standing eye, and a forced pose (swimming in water,
/// gliding, the riptide spin, a crawl without headroom) lifts the room so the eye is 0.4 m above
/// the feet whatever the head does: `0.4 - head_y`.
pub(super) fn headset_eye_height(
    pose: horizon_client::pose::Pose,
    room_crawling: bool,
    headroom: bool,
    pose_eye: f32,
    standing_eye: f32,
    head_y: f32,
) -> f32 {
    use horizon_client::pose::Pose;
    match pose {
        Pose::Crouching => standing_eye,
        Pose::Swimming if room_crawling && headroom => standing_eye,
        Pose::Swimming | Pose::FallFlying | Pose::SpinAttack => 0.4 - head_y,
        _ => pose_eye,
    }
}

/// Where a headset's ray starts: the camera's eye (the frame's interpolated one for what is
/// drawn, the tick's for what the game acts on) and the hand's place from it. The hands are
/// drawn at the camera plus the same offset.
pub(super) fn headset_ray_origin(eye: [f64; 3], hand: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|i| eye[i] + hand[i])
}

/// What the game last took from the headset.
#[derive(Default)]
pub(super) struct Headset {
    /// The eye height the camera stands for (`headset_eye_target`), set each tick.
    pub(super) eye_target: Option<f32>,
    /// The camera's eye this frame (interpolated position and eye height), while a frame's
    /// pointer is worked out: the ray starts there rather than at the tick's eye.
    pub(super) frame_eye: Option<[f64; 3]>,
    /// The aim's yaw (play space) the player last followed: the game's yaw
    /// minus it is where the body faces, which the turn stick and the server
    /// turn.
    head_yaw: Option<f32>,
    /// The pointer's last place on the menu panel (window pixels).
    pointer: Option<(f64, f64)>,
    /// The pointer's buttons as last sent.
    buttons: [bool; 2],
    /// "Use" is held for the hand at the mouth (main or off).
    eating: Option<Hand>,
    /// An off-hand click (a swing) waits for the next tick, which aims it
    /// from the off hand.
    off_pending: bool,
    /// The ray targeting follows for a moment (`headset_target`): the off
    /// hand's (`true`) or the main hand's.
    aim_override: Option<bool>,
    /// The hotbar slot (0..8, 9 the off hand's) the main hand points at or
    /// touches on the HUD on the other arm.
    hud_slot: Option<usize>,
    /// The main trigger was down over the HUD.
    hud_trigger: bool,
    /// Swings into blocks and touches on them.
    pub(super) trackers: super::headset_trackers::Trackers,
    /// A bow drawn with both hands.
    pub(super) bow: super::headset_bow::Bow,
    /// Visor's keyboard (`visor_keyboard.rs`).
    pub(super) keyboard: super::visor_keyboard::KeyboardState,
    /// Visor's game menu (`visor_menu.rs`).
    pub(super) game_menu: super::visor_menu::GameMenu,
    /// Visor's hotbar wheel, while a grip holds it open (`visor_hotbar.rs`).
    pub(super) wheels: super::visor_hotbar::Wheels,
    /// The triggers-or-face-buttons of each hand (main, off) as of the last frame, for the click
    /// that makes a hand the active one.
    pub(super) clicks: [bool; 2],
    /// The pointer's left button was down off the panel last frame (a click there closes the screen).
    off_panel_click: bool,
    /// Vivecraft's player data and trackers.
    pub(super) vr: super::vr::VrRuntime,
    /// `ClientDataHolderVR.swingType`: how the hand moves while it swings.
    pub(super) swing_type: super::headset_hands::SwingType,
}

impl Headset {
    /// The next tick aims an off-hand click from the off hand.
    pub(super) fn set_off_pending(&mut self, off: bool) {
        self.off_pending = off;
    }
}

impl Game<'_> {
    /// Once per loop iteration, before the pad: tell the headset whether a
    /// screen shows, and move and click the pointer on it.
    pub(super) fn headset_frame(&mut self, event_loop: &dyn EventLoopCtl) {
        // The headset's system menu quit the app: save, then leave.
        if headset::take_quit()
            && let Err(e) = self.begin_closing_save(true)
        {
            log::error!("Headset: saving before quitting: {e}");
        }
        self.visor_late_login();
        self.visor_update_two_handed();
        if !headset::active() {
            self.vr_clear_room_tracking();
            return;
        }
        // Visor's keyboard: the rays and triggers on its keys (before the screen's pointer, so a
        // press on a key is not a click beside the screen).
        self.visor_keyboard_frame();
        let effective = self.vivecraft_effective_settings();
        headset::set_settings(&effective);
        self.vr_update();
        self.vr_teleport_frame();
        let menu = self.pad_in_menu();
        if menu && !headset::menu() {
            headset::set_panel_anchor(self.headset_menu_anchor());
        }
        headset::set_menu(menu);
        headset::set_menu_kind(match self.screen {
            super::Screen::Chat => headset::MenuKind::Chat,
            _ => headset::MenuKind::Normal,
        });
        let Some(id) = self.window.as_ref().map(|w| w.id()) else {
            return;
        };
        let size = self
            .renderer
            .as_ref()
            .map(|r| (f64::from(r.config.width), f64::from(r.config.height)));
        let at = headset::pointer()
            .filter(|_| menu && !self.visor_keyboard_hand_over(headset::pointer_off()))
            .zip(size)
            .map(|([u, v], (w, h))| (f64::from(u) * w, f64::from(v) * h));
        let device_id = DeviceId::dummy();
        if at.is_none() {
            self.pad.headset.pointer = None;
        }
        if let Some(at) = at
            && self.pad.headset.pointer != Some(at)
        {
            self.pad.headset.pointer = Some(at);
            self.handle_window_event(
                event_loop,
                id,
                WindowEvent::CursorMoved {
                    device_id,
                    position: PhysicalPosition::new(at.0, at.1),
                },
                InputOrigin::User,
            );
        }
        // `MouseClickHandler.processScreen`: a click beside the screen, in a world, closes it.
        let raw_left = menu && headset::pointer_buttons()[0];
        let off_panel = raw_left && at.is_none() && !self.visor_keyboard_hovered();
        if off_panel
            && !self.pad.headset.off_panel_click
            && self.title.is_none()
            && self.player.is_some()
            && !self.remote_input_pending()
            && !self.screen_over_playing()
            && self.screen != super::Screen::Chat
        {
            let _ = self.key_pressed(
                winit::keyboard::KeyCode::Escape,
                winit::keyboard::KeyCode::Escape,
            );
        }
        self.pad.headset.off_panel_click = off_panel;
        // Off the panel nothing is held: a press there ends as the pointer leaves.
        let buttons = if at.is_some() {
            headset::pointer_buttons()
        } else {
            [false; 2]
        };
        // Over the on-screen keyboard the pointer types (Vivecraft's keyboard).
        if menu {
            self.headset_auto_keyboard();
        } else {
            self.visor_keyboard_no_screen();
        }
        let over_keys = at.is_some() && self.headset_keyboard(false);
        for (i, button) in [MouseButton::Left, MouseButton::Right]
            .into_iter()
            .enumerate()
        {
            if buttons[i] == self.pad.headset.buttons[i] {
                continue;
            }
            self.pad.headset.buttons[i] = buttons[i];
            if over_keys {
                if i == 0 && buttons[i] {
                    self.headset_keyboard(true);
                }
                continue;
            }
            // The game menu runs what its pointer clicks.
            if self.screen == super::Screen::GameMenu {
                if let (0, true, Some((x, y))) = (i, buttons[i], at) {
                    self.game_menu_click(x, y);
                }
                continue;
            }
            self.handle_window_event(
                event_loop,
                id,
                WindowEvent::MouseInput {
                    device_id,
                    state: if buttons[i] {
                        ElementState::Pressed
                    } else {
                        ElementState::Released
                    },
                    button,
                },
                InputOrigin::User,
            );
            if i == 0 && buttons[i] {
                self.headset_clicked();
            }
        }
        self.headset_game_menu_button();
        if !menu {
            self.visor_active_hand_click();
            self.headset_hotbar_wheel();
            self.headset_hud_slots();
            self.headset_gestures(event_loop, id);
        }
        self.headset_trackers(event_loop, id);
    }

    /// Where a chest's or a station's screen opens, as Vivecraft's
    /// `guiAppearOverBlock`: a little above the block the player used (the
    /// one still targeted), in play space from the rest point.
    fn headset_menu_anchor(&self) -> Option<glam::Vec3> {
        if self.screen != super::Screen::Inventory
            || (self.container.is_none() && self.station.is_none())
        {
            return None;
        }
        let hit = self.pick()?;
        let eye = self.eye_position()?;
        // `GuiHandler`: the middle of the block's base; the screen rises from there.
        let over = [
            f64::from(hit.pos[0]) + 0.5,
            f64::from(hit.pos[1]),
            f64::from(hit.pos[2]) + 0.5,
        ];
        let world = glam::Vec3::from_array(std::array::from_fn(|i| (over[i] - eye[i]) as f32));
        Some(headset::body_rotation(headset::body_yaw()).inverse() * world)
    }

    /// The HUD on the other arm as a tablet (Vivecraft's `InteractiveHotbarModule`): the
    /// main hand touches a hotbar slot and its trigger selects it; the off hand's slot swaps
    /// the hands' items.
    fn headset_hud_slots(&mut self) {
        let slot = headset::hud_slot();
        if slot != self.pad.headset.hud_slot && slot.is_some() {
            headset::pulse(true, 0.3, 0.02);
        }
        self.pad.headset.hud_slot = slot;
        let pressed = headset::hand_buttons()[0].trigger > 0.5;
        let click = pressed && !self.pad.headset.hud_trigger;
        self.pad.headset.hud_trigger = pressed;
        if !click || self.screen != super::Screen::Playing || self.spectator() {
            return;
        }
        match slot {
            Some(9) => {
                let _ = self.key_pressed(
                    winit::keyboard::KeyCode::KeyF,
                    winit::keyboard::KeyCode::KeyF,
                );
            }
            Some(i) if i != self.selected => self.hotbar_presses.push(i),
            _ => {}
        }
    }

    /// The hotbar slot the main hand points at on the HUD, for its mark.
    pub(super) fn headset_hud_slot(&self) -> Option<usize> {
        self.pad.headset.hud_slot.filter(|_| headset::active())
    }

    /// Standing, what the hands do by moving: Vivecraft's `SwingTracker` (a fast swing hits
    /// what the weapon reaches and mines what its tip crosses) and `EatingTracker` (food or a
    /// potion held to the mouth is eaten or drunk).
    fn headset_gestures(&mut self, event_loop: &dyn EventLoopCtl, id: winit::window::WindowId) {
        self.vr_swing(event_loop, id);
        self.vr_eating(event_loop, id);
    }

    /// Holds "use" while a hand is at the mouth with something to eat.
    pub(super) fn headset_eating(
        &mut self,
        event_loop: &dyn EventLoopCtl,
        id: winit::window::WindowId,
        hand: Option<Hand>,
    ) {
        if hand == self.pad.headset.eating {
            return;
        }
        let was = self.pad.headset.eating.is_some();
        self.pad.headset.eating = hand;
        if was == hand.is_some() {
            return;
        }
        self.handle_window_event(
            event_loop,
            id,
            WindowEvent::MouseInput {
                device_id: DeviceId::dummy(),
                state: if hand.is_some() {
                    ElementState::Pressed
                } else {
                    ElementState::Released
                },
                button: MouseButton::Right,
            },
            InputOrigin::User,
        );
    }

    /// The tick's clicks are done: an off-hand swing's click is spent.
    pub(super) fn headset_tick_done(&mut self) {
        self.pad.headset.off_pending = false;
        self.visor_keyboard_tick();
    }

    /// Standing, the off hand acts rather than the main one: its swing waits
    /// for the tick, it holds food at the mouth, or its item is in use.
    pub(super) fn headset_off_acts(&self) -> bool {
        if let Some(off) = self.pad.headset.aim_override {
            return off;
        }
        if !headset::active() || headset::settings().seated() {
            return false;
        }
        let [main, off] = headset::hand_buttons();
        if main.trigger > 0.5 || main.primary {
            return false;
        }
        // Visor's X is the off hand's right click (`ActionRightMouse`): it uses what the off hand
        // holds, and the off hand also acts for a swing, a meal or a shield.
        off.primary
            || (self.offhand_redirect.is_some() && off.trigger > 0.5)
            || self.pad.headset.off_pending
            || self.pad.headset.eating == Some(Hand::Off)
            || self.used_hand() == Some(Hand::Off)
    }

    /// The off hand uses first: it acts and holds something (an empty off
    /// hand leaves the use to the main hand's item, aimed from the off hand).
    pub(super) fn headset_off_uses_first(&self) -> bool {
        self.headset_off_acts() && self.stack_in(super::shield::OFFHAND_SLOT).is_some()
    }

    /// The item in `hand` is food or a drink (`consumable`).
    pub(super) fn consumable_in(&self, hand: Hand) -> bool {
        let slot = match hand {
            Hand::Main => self.selected,
            Hand::Off => super::shield::OFFHAND_SLOT,
        };
        self.stack_in(slot).is_some_and(|s| {
            let view = horizon_client::item_render::stack_view(s);
            self.pack
                .item_renderer
                .models
                .components(&view)
                .contains_key("minecraft:consumable")
        })
    }

    /// The headset's pointer is on the menu panel: the pad cursor marks it.
    pub(super) fn headset_pointing(&self) -> bool {
        headset::active() && self.pad.headset.pointer.is_some() && headset::pointer().is_some()
    }

    /// Once per frame: the player looks where the aim (the main hand's
    /// controller, or the head seated) points, from the body's facing. In a
    /// menu the view stays and the body with it.
    pub(super) fn head_look(&mut self) {
        let acting = if self.headset_off_acts() {
            headset::off_hand().map(|h| headset::angles(h.aim.1))
        } else {
            None
        };
        let Some(aim) = self.headset_bow_aim().or(acting).or_else(headset::aim) else {
            self.pad.headset.head_yaw = None;
            return;
        };
        let snap = headset::take_snap_turn();
        if snap != 0.0 && self.player.is_some() && !self.pad_in_menu() {
            self.turn_player(f64::from(snap / 0.15), 0.0);
        }
        let body = self.yaw - self.pad.headset.head_yaw.unwrap_or(aim[0]);
        self.headset_room_walk(body);
        if self.player.is_some() && !self.pad_in_menu() {
            let (yaw, pitch) = headset::turn_toward_head(body, aim, self.yaw, self.pitch);
            if yaw != 0.0 || pitch != 0.0 {
                // `Entity.turn` takes the mouse's units: 0.15 degrees each.
                self.turn_player(f64::from(yaw / 0.15), f64::from(pitch / 0.15));
            }
            self.pad.headset.head_yaw = Some(aim[0]);
        }
        headset::set_body_yaw(body);
    }

    /// Where the camera faces in a headset, first person: the head's yaw and pitch in the
    /// world (Minecraft degrees), from the room's body yaw and the headset's own pose. The
    /// player's `yaw` and `pitch` are the aim hand's, so nothing that is drawn or heard from
    /// the camera may read those.
    pub(super) fn headset_facing(&self) -> Option<(f32, f32)> {
        if !headset::active() || self.camera_type != super::perspective::CameraType::FirstPerson {
            return None;
        }
        let head = headset::head()?;
        Some((headset::body_yaw() + head[0], head[1]))
    }

    /// The rotation the server gets in the movement packets (`LocalPlayer.sendPosition`), which
    /// is what a client without Visor draws the head and body from. As Visor's
    /// `VRLocalPlayerImpl.updatePlayerLook` sets the player's rotation: the head's, and the
    /// acting hand's while an item is being used (a bow drawn, a shield raised, a meal), so
    /// a release or a throw leaves along the hand. The player's own `yaw` and `pitch` stay the
    /// aim's for the game's own use, attack and picking.
    pub(super) fn headset_sent_rotation(&self) -> (f32, f32) {
        let aim = (self.yaw, self.pitch);
        if !headset::active() || self.riding.is_some() || self.used_hand().is_some() {
            return aim;
        }
        match headset::head() {
            Some(head) => (headset::body_yaw() + head[0], head[1].clamp(-90.0, 90.0)),
            None => aim,
        }
    }

    /// Standing, a bow being drawn aims from the main hand (the string)
    /// through the off hand (the bow), as Vivecraft's `BowTracker`.
    fn headset_bow_aim(&self) -> Option<[f32; 2]> {
        if headset::settings().seated()
            || self.used_hand().is_none()
            || self.held().is_none_or(|s| &*s.item != "minecraft:bow")
        {
            return None;
        }
        let string = headset::main_hand()?.grip.0;
        let bow = headset::off_hand()?.grip.0;
        let along = bow - string;
        (along.length() > 0.1).then(|| headset::direction_angles(along))
    }

    /// Room scale: the player steps after the head's walk in the room, as far
    /// as nothing stops it, and the headset's rest position follows by as much.
    fn headset_room_walk(&mut self, body_yaw: f32) {
        let settings = headset::settings();
        if settings.seated()
            || self.pad_in_menu()
            || self.riding.is_some()
            || self.sleeping.is_some()
            || self.serving.is_some()
        {
            return;
        }
        let Some(off) = headset::head_offset() else {
            return;
        };
        // `walkMultiplier`: the player walks that many times what the head walks.
        let walk = settings.float("walkMultiplier");
        let flat = glam::Vec3::new(off.x * walk, 0.0, off.z * walk);
        // Under a millimetre: tracking noise.
        if flat.length_squared() < 1e-6 {
            return;
        }
        let step = headset::to_world(body_yaw, flat);
        // `horizonVivecraftRoomWalk`: Vivecraft's rules for the same step.
        if settings.bool("horizonVivecraftRoomWalk") {
            let moved = self.vr_room_walk(step);
            self.headset_room_moved(body_yaw, moved);
            return;
        }
        let entities = self.player_entity_collisions();
        let Some(player) = self.player.as_mut() else {
            return;
        };
        let before = player.position;
        let _ = horizon_client::motion::MotionWorld {
            entities: &entities,
            player,
            scene: &mut self.world,
            assets: &self.pack.assets,
            collision_binding: self.collision,
            resources: self.motion,
        }
        .room_step(horizon_core::geometry::Vec3::new(
            f64::from(step.x),
            0.0,
            f64::from(step.z),
        ));
        let after = player.position;
        let moved = [after.x - before.x, after.y - before.y, after.z - before.z];
        self.headset_room_moved(body_yaw, moved);
    }

    /// The player walked `moved` in the room: the camera follows and the headset's rest
    /// position goes back by as much.
    fn headset_room_moved(&mut self, body_yaw: f32, moved: [f64; 3]) {
        // The camera interpolates from the last tick's position: both move.
        self.previous_position.x += moved[0];
        self.previous_position.y += moved[1];
        self.previous_position.z += moved[2];
        let back = headset::body_rotation(body_yaw).inverse()
            * glam::Vec3::new(moved[0] as f32, 0.0, moved[2] as f32);
        headset::walked(back);
    }

    /// The tick's eye height for the camera in a headset (`headset_eye_height`), `None` off a
    /// headset, seated or without a tracked head.
    pub(super) fn headset_eye_target(&mut self) -> Option<f32> {
        let target = (|| {
            if !headset::active() || headset::settings().seated() || self.sleeping.is_some() {
                return None;
            }
            let head = headset::head_offset()?;
            let p = self.player.as_ref()?;
            let pose = p.pose;
            let crawling = p.room_crawling;
            let entity_standing = horizon_client::pose::Pose::Standing
                .dimensions(p.scale)
                .eye_height;
            let model_standing = if self.visor_mechanics()
                && self.visor_height_mode()
                    == horizon_visor::settings::SupportedHeightMode::RealSize
            {
                (horizon_vivecraft::player::player_height(&headset::settings()) + 0.1) * p.scale
            } else {
                entity_standing
            };
            let scale = headset::world_scale();
            let lift = headset::room().map_or(0.0, |r| (scale - 1.0) * r.rest.y);
            // XR adds the scaled head displacement later. Remove its standing-height lift
            // from the base once, so calibration/world scale cannot add the height twice.
            let standing = model_standing * scale - lift;
            let pose_eye = p.dimensions.eye_height * scale - lift;
            // `visor$hasHeadroom`: the body's column up to the eye and 0.1 m above is free.
            let headroom = if pose == horizon_client::pose::Pose::Swimming && crawling {
                let half = f64::from(p.dimensions.width / 2.0);
                let at = p.position;
                let top = f64::from(standing + head.y + 0.1).max(0.1);
                let bounds = horizon_core::geometry::Aabb::new(
                    horizon_core::geometry::Vec3::new(at.x - half, at.y, at.z - half),
                    horizon_core::geometry::Vec3::new(at.x + half, at.y + top, at.z + half),
                );
                let entities = self.player_entity_collisions();
                let player = self.player.as_mut()?;
                horizon_client::motion::MotionWorld {
                    entities: &entities,
                    player,
                    scene: &mut self.world,
                    assets: &self.pack.assets,
                    collision_binding: self.collision,
                    resources: self.motion,
                }
                .no_player_collision(bounds)
                .unwrap_or(true)
            } else {
                true
            };
            Some(headset_eye_height(
                pose, crawling, headroom, pose_eye, standing, head.y,
            ))
        })();
        self.pad.headset.eye_target = target;
        target
    }

    /// The ray targeting runs along, standing: where the acting hand's
    /// controller starts it from the eyes, and its direction, in the world.
    pub(super) fn headset_ray(&self) -> Option<([f64; 3], [f64; 3])> {
        if self.serving.is_some() || headset::settings().seated() || self.camera_mob().is_some() {
            return None;
        }
        let hand = if self.headset_off_acts() {
            headset::off_hand()
        } else {
            headset::main_hand()
        }?;
        let body = headset::body_yaw();
        let at = headset::to_world(body, hand.aim.0);
        let along = headset::to_world(body, hand.aim.1 * glam::Vec3::NEG_Z).normalize();
        Some((
            at.to_array().map(f64::from),
            along.to_array().map(f64::from),
        ))
    }

    /// What the main (or `off`) hand's ray meets now.
    pub(super) fn headset_target(&mut self, off: bool) -> Option<RayTarget> {
        let saved = self.pad.headset.aim_override.replace(off);
        let target = (|| {
            let (origin, dir) = self.aim()?;
            let at = |d: f64| std::array::from_fn(|i| origin[i] + dir[i] * d);
            let block = self.pick();
            let block_distance = block.map(|h| {
                (0..3)
                    .map(|i| (h.point[i] - origin[i]).powi(2))
                    .sum::<f64>()
                    .sqrt()
            });
            let entity = self.targeted_entity_distance();
            let (end, hit) = match (entity, block, block_distance) {
                (Some(e), _, d) if d.is_none_or(|d| e < d) => (at(e), Some(None)),
                (_, Some(hit), _) => (hit.point, Some(Some(hit.face))),
                _ => (at(self.block_reach.max(self.entity_reach)), None),
            };
            Some(RayTarget {
                start: origin,
                end,
                hit,
            })
        })();
        self.pad.headset.aim_override = saved;
        target
    }

    /// Standing in a headset, each hand's controller (main, off) in
    /// camera-relative world space (its aim pose, as Vivecraft renders at);
    /// `None` for a hand whose controller isn't tracked. `None` altogether
    /// off a headset or seated.
    pub(super) fn headset_hand_frames(&self) -> Option<[Option<HandFrame>; 2]> {
        let settings = headset::settings();
        if !headset::active() || settings.seated() {
            return None;
        }
        let body = headset::body_rotation(headset::body_yaw());
        let frame = |hand: Option<headset::HandPose>, left: bool| {
            let hand = hand?;
            // `setupRenderingAtController` scales what is drawn at a controller by the world scale.
            Some(HandFrame {
                at: glam::Mat4::from_scale_rotation_translation(
                    glam::Vec3::splat(headset::world_scale()),
                    body * hand.aim.1,
                    body * hand.aim.0,
                ),
                left,
                // The runtime's own angle between aim and grip, and the
                // player's tilt of the handle across the fist.
                gun: super::headset_hands::gun_angle(hand.grip.1, hand.aim.1),
                aim_to_grip: horizon_visor::item_pose::aim_to_grip(hand.aim.1, hand.grip.1),
            })
        };
        Some([
            frame(headset::main_hand(), settings.left_handed()),
            frame(headset::off_hand(), !settings.left_handed()),
        ])
    }

    /// `VRPlayer.menuHandOff` and `menuHandMain`: with a screen, the ring, the
    /// keyboard or as a spectator both controllers are pointers; the main one
    /// also while it touches the hotbar (`vrTouchHotbar`).
    pub(super) fn headset_menu_hands(&self) -> [bool; 2] {
        let off = self.pad_in_menu() || self.spectator() || self.pad.headset.vr.keyboard.showing();
        let main =
            off || (headset::settings().bool("vrTouchHotbar") && headset::hud_slot().is_some());
        [main, off]
    }

    /// `VRArmHelper.renderMenuHand`: a gray box 18 cm long, 4 by 2.5 cm, back
    /// from the controller, dark in the dark (`light` 0..15 at the head).
    pub(super) fn headset_menu_hand(
        &self,
        at: glam::Mat4,
        light: u8,
        out: &mut Vec<horizon_client::mesher::Vertex>,
    ) {
        let Some(white) = self.gui.atlas.sprites.get("horizon/white") else {
            return;
        };
        let uv = [(white.u0 + white.u1) / 2.0, (white.v0 + white.v1) / 2.0];
        menu_hand_vertices(at, uv, light, out);
    }

    /// Whether the player's arm shows at the controllers: `showPlayerHands`,
    /// and not when the full model's own arms stand in (`shouldRenderSelf`
    /// with `modelArmsMode` COMPLETE), nor when invisible.
    fn headset_arm_shown(&self) -> bool {
        let settings = headset::settings();
        settings.bool("showPlayerHands")
            && !(settings.bool("shouldRenderSelf")
                && settings.enum_name("modelArmsMode") == "COMPLETE")
            && !self.own_invisible()
    }

    /// The player's bare arm at a controller at `at`, swinging by `attack`
    /// (0..1): Vivecraft's arm, back from the controller along it.
    pub(super) fn headset_arm(
        &self,
        at: glam::Mat4,
        left: bool,
        attack: f32,
        light: u32,
        out: &mut Vec<horizon_client::mesher::Vertex>,
    ) {
        if !self.headset_arm_shown() {
            return;
        }
        let slim = self.player_model.as_ref().is_some_and(|m| m.slim);
        let m = at
            * super::headset_hands::swing(attack, self.pad.headset.swing_type, !left)
            * super::headset_hands::arm_transform(!left, slim);
        self.player_arm(left, m, light, 0.0, out);
    }

    /// A stack held in a controller's `frame`: its arm (not a bow's: the
    /// hand grips it), and where the item's right-hand first-person display
    /// transform starts, as Vivecraft holds that kind of item. `model_3d`:
    /// its model is lit as a block's (`gui_light: side`), not a flat item.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn headset_held(
        &self,
        frame: HandFrame,
        stack: &horizon_client::items::ItemStack,
        model_3d: bool,
        attack: f32,
        state: super::headset_hands::HeldUse,
        light: u32,
        out: &mut Vec<horizon_client::mesher::Vertex>,
        runs: &mut Vec<(u32, i32)>,
    ) -> glam::Mat4 {
        use super::headset_hands::{Held, item_transform, swing};
        let map = super::maps::map_id(stack).is_some();
        let block = self
            .items()
            .get(&stack.item)
            .and_then(|i| i.block)
            .map(|state| {
                let f = self.res.facts().get(state);
                (&f.classes[..], model_3d)
            });
        let food = {
            let view = horizon_client::item_render::stack_view(stack);
            self.pack
                .item_renderer
                .models
                .components(&view)
                .contains_key("minecraft:consumable")
        };
        let held = Held::of(&stack.item, block, food, map);
        if held == Held::Bow
            && let Some(drawn) = self.headset_drawn_bow_frame()
        {
            return drawn;
        }
        // Visor draws the arm under every item (`renderWorldArm` before the item), a bow too: with
        // no arm the bow hangs in the air. Vivecraft's own bow model carries the hand.
        if (held != Held::Bow || self.visor_mechanics()) && self.headset_arm_shown() {
            runs.push((out.len() as u32, 0));
            self.headset_arm(frame.at, frame.left, attack, light, out);
        }
        if self.visor_mechanics() {
            return self.visor_item_matrix(frame, stack, model_3d, attack, state);
        }
        frame.at
            * swing(attack, self.pad.headset.swing_type, !frame.left)
            * item_transform(held, !frame.left, state, frame.gun)
    }

    /// Visor's hold (`VRHandRenderer.renderWorldArmWithItem` and `VRItemPoseDefault`): the item
    /// class picks one of its poses (`horizon_visor::item_pose`), set on the chain of the entity
    /// scale, the controller-to-model turn, the arms' scale about the grip and the shift into
    /// the third person hand's frame. A spear that stabs slides instead of swinging the hand.
    fn visor_item_matrix(
        &self,
        frame: HandFrame,
        stack: &horizon_client::items::ItemStack,
        model_3d: bool,
        attack: f32,
        state: super::headset_hands::HeldUse,
    ) -> glam::Mat4 {
        use horizon_visor::item_pose::{self, BlockFacts, ItemFacts, PoseState, TransformType};
        let id = &*stack.item;
        let view = horizon_client::item_render::stack_view(stack);
        let components = self.pack.item_renderer.models.components(&view);
        let animation = components
            .get("minecraft:consumable")
            .map(horizon_client::first_person::consumable_animation);
        use horizon_world::consumable::UseAnimation as A;
        let block = self
            .items()
            .get(&stack.item)
            .and_then(|i| i.block)
            .map(|b| {
                let f = self.res.facts().get(b);
                BlockFacts {
                    torch_block: f.classes.iter().any(|c| c.ends_with("TorchBlock")),
                    gui_3d: model_3d,
                }
            });
        let spear = id.ends_with("_spear") || components.contains_key("minecraft:kinetic_weapon");
        let facts = ItemFacts {
            id,
            eats_or_drinks: matches!(animation, Some(A::Eat | A::Drink)),
            toots: matches!(animation, Some(A::TootHorn)) || id.ends_with("goat_horn"),
            shield: self.blocks_attacks(stack).is_some(),
            spear,
            tool: components.contains_key("minecraft:tool"),
            block,
            map: super::maps::map_id(stack).is_some(),
        };
        let kind = item_pose::transform_type(&facts);
        let hand_dir = if frame.left == headset::settings().left_handed() {
            1.0
        } else {
            -1.0
        };
        let stabbing = kind == TransformType::Spear && state.stab;
        let pose_state = PoseState {
            using: state.using,
            remaining_ticks: state.remaining,
            charge: state.ticks_used,
            riptide: if state.spinning { 1.0 } else { 0.0 },
            wet: false,
            spinning: state.spinning,
            ticks: self.ticks,
            millis: state.millis,
            stab: attack,
            spear: if stabbing {
                item_pose::stab_motion(attack, 0.38)
            } else {
                Default::default()
            },
            bed: id.ends_with("_bed"),
            banner: id.ends_with("_banner"),
        };
        // The poses are authored on a Quest 3S's aim frame and re-anchored by this controller's own
        // aim to grip turn (`VRHandItemPose.getAimToGripRotation`): never the identity.
        let params = if kind == TransformType::Tool {
            item_pose::compute_tool(id, hand_dir, frame.aim_to_grip, &pose_state)
        } else {
            item_pose::compute(kind, hand_dir, frame.aim_to_grip, &pose_state)
        };
        let arms = headset::settings().float("playerModelArmsScale");
        let swing_progress = if stabbing { 0.0 } else { attack };
        frame.at
            * super::headset_hands::swing(swing_progress, self.pad.headset.swing_type, !frame.left)
            * item_pose::hand_base(arms)
            * params.matrix()
    }

    /// The headset's pointers as lines in the world: over a menu, the main hand's ray to the
    /// panel; in play, Vivecraft's crosshair where the aim ray meets a block face or an
    /// entity (`VREffectsHelper.extractCrosshairState`). Vivecraft draws no in-world ray.
    pub(super) fn headset_pointer_lines(
        &mut self,
        camera: [f64; 3],
        lines: &mut Vec<horizon_client::gizmos::Line>,
        on_top: &mut Vec<horizon_client::gizmos::Line>,
    ) {
        let settings = headset::settings();
        self.hand_crosshair.clear();
        if !headset::active() || self.player.is_none() {
            return;
        }
        let body = headset::body_yaw();
        self.vr_axes_lines(lines);
        if !self.pad_in_menu() {
            self.vr_teleport_lines(lines);
        }
        if self.pad_in_menu() {
            // Visor's cursor ray (`VRHandRenderer.renderCursorLine`): from the hand that points the
            // cursor, light grey (228), thin.
            // The hands that point at Visor's keyboard draw their ray to it.
            let keys = self.visor_keyboard_ray_lengths();
            for (i, hand) in [headset::main_hand(), headset::off_hand()]
                .into_iter()
                .enumerate()
            {
                let (Some(hand), Some(length)) = (hand, keys[i]) else {
                    continue;
                };
                on_top.push(menu_pointer_line(camera, body, hand.aim, length));
            }
            let off = headset::pointer_off();
            let hand = if off {
                headset::off_hand()
            } else {
                headset::main_hand()
            };
            if let Some(hand) = hand
                && keys[usize::from(off)].is_none()
            {
                let length = headset::pointer_distance().unwrap_or(MENU_LASER_MISS);
                // Drawn over everything, as Visor draws the cursor (`depthFunc(GL_ALWAYS)`).
                on_top.push(menu_pointer_line(camera, body, hand.aim, length));
            }
            return;
        }
        let context = CrosshairContext {
            in_level: true,
            hud_hidden: self.ui.hide_gui,
            bow_notched: self.headset_drawn_bow_frame().is_some(),
            world_scale: 1.0,
            ..Default::default()
        };
        if !crosshair::should_render(&settings, &context) {
            return;
        }
        let Some(target) = self.headset_target(false) else {
            return;
        };
        let distance = (0..3)
            .map(|i| (target.end[i] - target.start[i]).powi(2))
            .sum::<f64>()
            .sqrt() as f32;
        let aim = headset::aim().unwrap_or([self.yaw, self.pitch]);
        let face = target.hit.flatten().map(|n| match n {
            [0, -1, 0] => Face::Down,
            [0, 1, 0] => Face::Up,
            [0, 0, -1] => Face::North,
            [0, 0, 1] => Face::South,
            [-1, 0, 0] => Face::West,
            _ => Face::East,
        });
        // Vivecraft's yaw is the world's, ours is the play space's.
        let yaw = aim[0] + body;
        let quad = crosshair::quad(
            &settings,
            face,
            target.hit.is_some(),
            yaw,
            aim[1],
            distance,
            1.0,
        );
        // Visor's crosshair (`HandEffectCrosshair`): the vanilla crosshair sprite on a quad of
        // `crosshairScale` * 0.125 m half width, laid on the face it meets (or across the aim in
        // the air), half bright on a miss, drawn over everything (`depthFunc(GL_ALWAYS)`) with
        // the GUI hands (`hand_crosshair`).
        let Some(sprite) = self.gui.atlas.sprites.get("hud/crosshair").copied() else {
            return;
        };
        let centre = glam::DVec3::from_array(target.end)
            + (quad.right.cross(quad.up) * 0.01 * if face.is_some() { 1.0 } else { -1.0 })
                .as_dvec3();
        let relative = (centre - glam::DVec3::from_array(camera)).as_vec3();
        let level = (quad.brightness * 255.0) as u32;
        let color = 0xFF00_0000 | (level << 16) | (level << 8) | level;
        let corner = |x: f32, y: f32, u: f32, v: f32| horizon_client::mesher::Vertex {
            pos: (relative + (quad.right * x + quad.up * y) * quad.scale).to_array(),
            uv: [u, v],
            layer: 0,
            color,
            light: 0xF0F0,
            normal: [0; 4],
            glint_uv: [0.0, 0.0],
        };
        let quad_corners = [
            corner(-1.0, 1.0, sprite.u0, sprite.v0),
            corner(1.0, 1.0, sprite.u1, sprite.v0),
            corner(1.0, -1.0, sprite.u1, sprite.v1),
            corner(-1.0, -1.0, sprite.u0, sprite.v1),
        ];
        // Both sides: the sprite shows from either, whatever way the quad faces.
        self.hand_crosshair.extend(quad_corners);
        self.hand_crosshair.extend(quad_corners.into_iter().rev());
    }
}

/// `VRArmHelper.renderMenuHand`'s box at `at`, gray in the light `light` (0..15), as quads whose
/// texture coordinates `uv` point at a white texel.
pub(super) fn menu_hand_vertices(
    at: glam::Mat4,
    uv: [f32; 2],
    light: u8,
    out: &mut Vec<horizon_client::mesher::Vertex>,
) {
    use horizon_client::mesher::Vertex;
    let gray = (64.0 * f32::from(light) / 15.0) as u32;
    let color = 0xFF00_0000 | (gray << 16) | (gray << 8) | gray;
    for corners in menu_box_quads() {
        for c in corners {
            out.push(Vertex {
                pos: at.transform_point3(c).to_array(),
                uv,
                layer: 0,
                color,
                light: 0xF0F0,
                normal: [0; 4],
                glint_uv: [0.0, 0.0],
            });
        }
    }
}

/// Visor's cursor ray (`VRHandRenderer.renderCursorLine`): from the hand that points, a little
/// ahead of the controller, `length` metres along its aim, light grey and thin. `camera` is the
/// world position the hand's offset is from, `body` the room's yaw.
pub(super) fn menu_pointer_line(
    camera: [f64; 3],
    body: f32,
    aim: (glam::Vec3, glam::Quat),
    length: f32,
) -> horizon_client::gizmos::Line {
    let along = headset::to_world(body, aim.1 * glam::Vec3::NEG_Z).normalize();
    let at = headset::to_world(body, aim.0) + along * LASER_GAP;
    let start: [f64; 3] = std::array::from_fn(|i| camera[i] + f64::from(at[i]));
    let end: [f64; 3] =
        std::array::from_fn(|i| start[i] + f64::from(along[i] * (length - LASER_GAP).max(0.0)));
    horizon_client::gizmos::Line {
        start,
        end,
        color: LASER_MAIN,
        width: LASER_WIDTH,
    }
}

/// The faces of `renderBox(start, end, -0.02, 0.02, -0.0125, 0.0125)` with
/// `start` at the controller and `end` 18 cm behind it (+Z).
fn menu_box_quads() -> [[glam::Vec3; 4]; 6] {
    let v = |x: f32, y: f32, z: f32| glam::Vec3::new(x * 0.02, y * 0.0125, z * 0.18);
    [
        [
            v(-1., -1., 0.),
            v(1., -1., 0.),
            v(1., 1., 0.),
            v(-1., 1., 0.),
        ],
        [
            v(-1., 1., 1.),
            v(1., 1., 1.),
            v(1., -1., 1.),
            v(-1., -1., 1.),
        ],
        [v(-1., 1., 0.), v(1., 1., 0.), v(1., 1., 1.), v(-1., 1., 1.)],
        [
            v(-1., -1., 1.),
            v(1., -1., 1.),
            v(1., -1., 0.),
            v(-1., -1., 0.),
        ],
        [
            v(-1., -1., 1.),
            v(-1., -1., 0.),
            v(-1., 1., 0.),
            v(-1., 1., 1.),
        ],
        [v(1., -1., 0.), v(1., -1., 1.), v(1., 1., 1.), v(1., 1., 0.)],
    ]
}

/// How far ahead of the controller its laser starts (metres).
const LASER_GAP: f32 = 0.04;
/// A menu laser that meets no panel: this long (metres).
const MENU_LASER_MISS: f32 = 0.6;
/// Line widths in pixels.
const LASER_WIDTH: f32 = 3.0;
/// ARGB: Visor's menu ray, `CURSOR_DEFAULT_COLOR` (228, 228, 228).
const LASER_MAIN: u32 = 0xFFE4_E4E4;

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_client::pose::Pose;

    const STANDING: f32 = 1.62;

    #[test]
    fn a_real_crouch_or_crawl_is_not_lowered_a_second_time_by_the_pose() {
        // The head dropped by `drop` metres from where it rested; the eye is the camera's eye
        // height plus that. Standing eyes are 1.62 above the feet.
        let eye = |pose, crawling, headroom, pose_eye, drop: f32| {
            headset_eye_height(pose, crawling, headroom, pose_eye, STANDING, -drop) - drop
        };
        // Sneaking 0.4 m for real (the sneak tracker made the pose 1.27): 1.22 above the feet,
        // not 1.27 - 0.4 = 0.87.
        let sneak = eye(Pose::Crouching, false, true, 1.27, 0.4);
        assert!((sneak - 1.22).abs() < 1e-5, "{sneak}");
        // Lying on the floor (head 1.3 m lower, the crawl pose's eye 0.4): the head's own 0.32.
        let crawl = eye(Pose::Swimming, true, true, 0.4, 1.3);
        assert!((crawl - 0.32).abs() < 1e-5, "{crawl}");
        // The old sum went under the floor: 0.4 - 1.3 = -0.9.
        assert!(0.4 - 1.3 < 0.0 && crawl > 0.0);
        // Standing is the pose's eye, untouched.
        assert_eq!(
            headset_eye_height(Pose::Standing, false, true, STANDING, STANDING, 0.1),
            STANDING
        );
    }

    #[test]
    fn a_forced_pose_puts_the_eye_0_4_above_the_feet_whatever_the_head_does() {
        for head_y in [-0.6_f32, 0.0, 0.2] {
            for (pose, crawling, headroom) in [
                (Pose::Swimming, false, true),
                (Pose::Swimming, true, false),
                (Pose::FallFlying, false, true),
                (Pose::SpinAttack, false, true),
            ] {
                let camera = headset_eye_height(pose, crawling, headroom, 0.4, STANDING, head_y);
                assert!((camera + head_y - 0.4).abs() < 1e-5, "{pose:?} {head_y}");
            }
        }
    }

    #[test]
    fn the_pointer_stays_fixed_to_the_hand_while_the_player_flies_between_ticks() {
        let hand = [0.2, -0.3, -0.4];
        let (from, to) = ([100.0, 70.0, 5.0], [100.0, 70.0, 6.4]); // 28 m/s, 1.4 m a tick
        let eye = 1.62;
        for step in 0..=12 {
            let a = f64::from(step) / 12.0;
            let camera: [f64; 3] = std::array::from_fn(|i| {
                from[i] + (to[i] - from[i]) * a + if i == 1 { eye } else { 0.0 }
            });
            // The frame's ray starts at the camera's eye plus the hand's offset: fixed to the hand.
            let start = headset_ray_origin(camera, hand);
            for i in 0..3 {
                assert!((start[i] - camera[i] - hand[i]).abs() < 1e-9);
            }
            // The tick's eye would be 1.4 m behind (or ahead) of the drawn hand halfway between ticks.
            let tick = headset_ray_origin([to[0], to[1] + eye, to[2]], hand);
            if step == 0 {
                assert!((tick[2] - start[2] - 1.4).abs() < 1e-9);
            }
        }
    }

    #[test]
    fn a_menu_ray_starts_ahead_of_the_hand_and_ends_where_it_was_asked_to() {
        // The hand 0.2 m right, 0.3 m down and 0.3 m ahead of the camera, aiming straight ahead
        // (-Z), the ray 2 m long (the panel's distance from the hand).
        let hand = (glam::Vec3::new(0.2, -0.3, -0.3), glam::Quat::IDENTITY);
        let camera = [10.0, 100.0, -4.0];
        // A body yaw of 180 faces -Z: the play space is the world's.
        let line = menu_pointer_line(camera, 180.0, hand, 2.0);
        let near = |a: [f64; 3], b: [f64; 3]| (0..3).all(|i| (a[i] - b[i]).abs() < 1e-5);
        assert!(
            near(line.start, [10.2, 99.7, -4.0 - 0.3 - 0.04]),
            "{:?}",
            line.start
        );
        // Ends 2 m from the hand's own position, straight ahead of it.
        assert!(
            near(line.end, [10.2, 99.7, -4.0 - 0.3 - 2.0]),
            "{:?}",
            line.end
        );
        assert_eq!((line.color, line.width), (0xFFE4_E4E4, 3.0));
        // A ray tipped down by 45 degrees goes down and ahead by the same amount.
        let down = (
            hand.0,
            glam::Quat::from_rotation_x(-std::f32::consts::FRAC_PI_4),
        );
        let line = menu_pointer_line(camera, 180.0, down, 1.0);
        let d = [
            line.end[0] - line.start[0],
            line.end[1] - line.start[1],
            line.end[2] - line.start[2],
        ];
        assert!((d[1] - d[2]).abs() < 1e-5 && d[1] < 0.0, "{d:?}");
        assert!(((d[1] * d[1] + d[2] * d[2]).sqrt() - 0.96).abs() < 1e-5);
    }

    #[test]
    fn a_menu_hand_is_one_gray_box_of_six_quads() {
        let mut out = Vec::new();
        menu_hand_vertices(glam::Mat4::IDENTITY, [0.5, 0.5], 15, &mut out);
        assert_eq!(out.len(), 24);
        assert!(out.iter().all(|v| v.color == 0xFF40_4040));
        // 18 cm long.
        let zs: Vec<f32> = out.iter().map(|v| v.pos[2]).collect();
        let length = zs.iter().cloned().fold(f32::MIN, f32::max)
            - zs.iter().cloned().fold(f32::MAX, f32::min);
        assert!((length - 0.18).abs() < 1e-5, "{length}");
    }
}
