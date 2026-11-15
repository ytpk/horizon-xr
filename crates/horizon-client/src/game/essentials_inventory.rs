// SPDX-License-Identifier: LGPL-3.0-only
// Copyright (c) 2025-2026 Phoenix-Ra and VisorEssentials contributors.
//! VROverlayInventory, VROverlayContainer, VRInvScreen and VROverlayDraggedItem.
use super::{Game, Screen};
use glam::{DVec3, EulerRot, Quat, Vec3};
use horizon_client::{headset, hud::HudBatch};
use horizon_visor::overlay::{Drag, Resize};
use starling_essentials::inventory as layout;

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub(super) enum Scope {
    #[default]
    None,
    Wrist,
    Container,
}
#[derive(Clone, Copy)]
enum Grab {
    Drag {
        panel: usize,
        hand: usize,
        drag: Drag,
    },
    Resize {
        panel: usize,
        hand: usize,
        resize: Resize,
    },
}
impl Grab {
    fn hand(self) -> usize {
        match self {
            Self::Drag { hand, .. } | Self::Resize { hand, .. } => hand,
        }
    }
}
#[derive(Default)]
pub(super) struct State {
    pub(super) scope: Scope,
    pub(super) canvas: [f32; 2],
    pub(super) container: bool,
    pub(super) focused: Scope,
    cursor: (f32, f32),
    down: [bool; 2],
    owner: usize,
    pub(super) wrist_visible: bool,
    pub(super) captured: bool,
    pub(super) book_visible: [bool; 2],
    keyboard_scope: Scope,
    use_down: bool,
    scroll_at: u64,
    source: Option<DVec3>,
    source_entity: Option<u32>,
    panels: [Option<headset::OverlaySpec>; 2],
    dismissed_at: Option<std::time::Instant>,
    grab: Option<Grab>,
    container_pose: Option<(Vec3, Quat)>,
}
impl Game<'_> {
    pub(super) fn essentials_inventory_wanted(&self) -> bool {
        headset::active()
            && !headset::settings().seated()
            && self.settings.vr.bool("essentialsBetterInventory")
            && self.title.is_none()
            && self
                .player
                .as_ref()
                .is_some_and(|p| !p.health.dead_or_dying())
            && !self.spectator()
    }
    pub(super) fn essentials_inventory_full(&self) -> bool {
        !self.essentials_ui.container
    }
    fn essentials_offhand_slot(&self) -> bool {
        !self.vr_server_settings().two_handed_vr
    }
    pub(super) fn essentials_inventory_origin(&self) -> Option<(f32, f32)> {
        (self.essentials_ui.scope == Scope::Wrist).then(|| {
            (
                (self.essentials_ui.canvas[0] - layout::width(self.essentials_offhand_slot()))
                    * 0.5,
                (self.essentials_ui.canvas[1] - layout::HEIGHT) * 0.5,
            )
        })
    }
    pub(super) fn essentials_inventory_slots(&self, slots: Vec<usize>) -> Vec<usize> {
        match self.essentials_ui.scope {
            Scope::None => slots,
            Scope::Wrist => slots
                .into_iter()
                .filter(|&i| {
                    layout::slot(
                        i,
                        self.essentials_inventory_full(),
                        self.essentials_offhand_slot(),
                    )
                    .is_some()
                })
                .collect(),
            Scope::Container => slots
                .into_iter()
                .filter(|i| (100..500).contains(i))
                .collect(),
        }
    }
    fn essentials_container_dimensions(&self) -> (f32, f32) {
        let (_, name) = self.remote_menu();
        let width = if name.contains("merchant") {
            276.0
        } else if name.contains("beacon") {
            230.0
        } else {
            176.0
        };
        (width, layout::container_height(&name, self.chest_rows()))
    }
    pub(super) fn essentials_ui_close(&mut self) {
        self.essentials_ui.container = false;
        self.essentials_ui.source = None;
        self.essentials_ui.source_entity = None;
        self.essentials_ui.container_pose = None;
        self.essentials_ui.focused = Scope::None;
        self.essentials_ui.down = [false; 2];
        self.essentials_ui.dismissed_at = Some(std::time::Instant::now());
        self.close_container();
    }
    /// Setup and route both panels before the ordinary menu/game pointer. Returns true if a
    /// panel owns the triggers this frame; locomotion remains in Screen::Playing.
    pub(super) fn essentials_inventory_frame(&mut self) -> bool {
        self.essentials_ui.captured = false;
        if !self.essentials_inventory_wanted() {
            if self.essentials_ui.container {
                // Disabling the feature restores the ordinary still-open menu.
                self.essentials_ui.container = false;
                if self.screen == Screen::Playing {
                    self.screen = Screen::Inventory;
                    self.capture(false);
                }
            }
            self.essentials_ui.panels = [None; 2];
            self.essentials_ui.wrist_visible = false;
            return false;
        }
        let buttons = headset::hand_buttons();
        let use_down = buttons.iter().any(|b| b.primary);
        let used = use_down && !self.essentials_ui.use_down;
        self.essentials_ui.use_down = use_down;
        if self.essentials_ui.container && used && self.essentials_ui.focused == Scope::None {
            let source = self.essentials_ui.source;
            let same_block = self.pick().is_some_and(|hit| {
                source.is_some_and(|p| {
                    p.x.floor() == f64::from(hit.pos[0])
                        && p.y.floor() == f64::from(hit.pos[1])
                        && p.z.floor() == f64::from(hit.pos[2])
                })
            });
            let same_entity = self
                .pick_entity_hit()
                .flatten()
                .is_some_and(|id| Some(id as u32) == self.essentials_ui.source_entity);
            if same_block || same_entity {
                self.essentials_ui_close();
                self.essentials_ui.captured = true;
                return true;
            }
        }
        if self
            .essentials_ui
            .dismissed_at
            .is_some_and(|t| t.elapsed().as_millis() < 200)
        {
            self.essentials_ui.captured = true;
            headset::set_triggers_captured([true; 2]);
            return true;
        }
        let hands = [headset::main_hand(), headset::off_hand()];
        if self.screen == Screen::Inventory {
            let container = self
                .remote
                .as_ref()
                .is_some_and(|_| self.remote_menu_id() != 0)
                || self.container.is_some()
                || self.station.is_some()
                || self.mount_active()
                || self.craft_size == 3;
            if container
                && !self.native_screen_shown()
                && (self.pick().is_some() || self.pick_entity_hit().is_some())
            {
                if !self.essentials_ui.container {
                    self.reset_inventory_clicks();
                    self.essentials_ui.container_pose = None;
                    self.essentials_ui.source = self.pick().map(|hit| {
                        DVec3::new(
                            hit.pos[0] as f64 + 0.5,
                            hit.pos[1] as f64,
                            hit.pos[2] as f64 + 0.5,
                        )
                    });
                    self.essentials_ui.source_entity = self
                        .pick_entity_hit()
                        .flatten()
                        .map(|id| id as u32)
                        .filter(|&id| self.essentials_source_position(id).is_some());
                }
                self.essentials_ui.container = true;
                self.screen = Screen::Playing;
                self.capture(true);
            }
        }
        if self.essentials_ui.container && self.screen != Screen::Playing {
            let screen = self.screen;
            self.essentials_ui_close();
            self.screen = screen;
        }
        let Some(room) = headset::room() else {
            return false;
        };
        let scale = headset::world_scale();
        let head = (room.head.0 - room.rest) * scale + Vec3::Y * ((scale - 1.0) * room.rest.y);
        let mut canvas = self
            .gui_scale()
            .map_or([427.0, 492.0], |b| [b.width, b.height]);
        canvas[1] = canvas[1].max(layout::CANVAS_HEIGHT + 44.0);
        self.essentials_ui.canvas = canvas;
        let mut wrist = self.essentials_wrist_anchors().map(|(position, rotation)| {
            let at = position.0
                + position.1
                    * Vec3::from_array(self.settings.vr.vec3("essentialsInventoryPosition"))
                    * scale;
            let [pitch, yaw, roll] = self
                .settings
                .vr
                .vec3("essentialsInventoryRotation")
                .map(f32::to_radians);
            let rot = if self.settings.vr.bool("essentialsInventoryAimedRotation")
                && self
                    .settings
                    .vr
                    .enum_name("essentialsInventoryRotationAnchor")
                    != "NONE"
            {
                let direction = (at - rotation.0).normalize_or_zero();
                Quat::from_euler(
                    EulerRot::ZYX,
                    roll,
                    std::f32::consts::PI + direction.x.atan2(direction.z) + yaw,
                    direction.y.clamp(-1.0, 1.0).asin() + pitch,
                )
            } else {
                rotation.1 * Quat::from_euler(EulerRot::ZYX, roll, yaw, pitch)
            };
            headset::OverlaySpec {
                pos: at,
                rot,
                width: canvas[0] / 427.0
                    * self.settings.vr.float("essentialsInventoryScale")
                    * scale,
                height: canvas[1] / 427.0
                    * self.settings.vr.float("essentialsInventoryScale")
                    * scale,
            }
        });
        if self.screen != Screen::Playing
            || self.essentials.bow.notched
            || self.essentials.bow.drawing
            || self.pad.headset.bow.notched()
        {
            wrist = None;
        }
        // Inventory is visible when the cursor or head faces its useful rectangle. The
        // wider head bounds are upstream's 0.6/1.0 expansion, not an invisible click target.
        wrist = wrist.filter(|p| {
            if self.essentials_ui.grab.is_some() {
                return true;
            }
            let sees = |at, dir, extra| {
                self.essentials_panel_hit(*p, Scope::Wrist, at, dir, extra)
                    .is_some()
            };
            hands[0].is_some_and(|h| sees(h.aim.0, h.aim.1 * Vec3::NEG_Z, 0.0))
                || sees(head, room.head.1 * Vec3::NEG_Z, 0.5)
        });
        self.essentials_ui.wrist_visible = wrist.is_some();
        let mut container = None;
        if self.essentials_ui.container {
            if let Some(id) = self.essentials_ui.source_entity {
                if let Some((at, _)) = self.essentials_source_position(id) {
                    self.essentials_ui.source = Some(at);
                } else {
                    self.essentials_ui_close();
                }
            }
            let eye = self
                .eye_position()
                .map(DVec3::from_array)
                .unwrap_or(DVec3::ZERO);
            let source = self.essentials_ui.source;
            if source.is_some_and(|p| p.distance(eye) > 4.0 * f64::from(scale)) {
                self.essentials_ui_close();
            } else if self.essentials_ui.container {
                let body = headset::body_rotation(headset::body_yaw()).inverse();
                let pos = if let Some(source) = source {
                    let mut at = head + body * (source - eye).as_vec3();
                    let top = self
                        .essentials_ui
                        .source_entity
                        .and_then(|id| self.essentials_source_position(id))
                        .map_or(1.1, |(_, height)| height);
                    at.y = head.y.clamp(at.y, at.y + top * scale) + 0.25 * scale;
                    at
                } else {
                    head + room.head.1 * Vec3::NEG_Z * scale
                };
                let forward = (pos - head).try_normalize().unwrap_or(Vec3::NEG_Z);
                let (_, rot) = super::visor_overlay::upright_panel(head, forward, 0.0);
                container = Some(headset::OverlaySpec {
                    pos,
                    rot,
                    width: canvas[0] / 427.0
                        * self.settings.vr.float("essentialsContainerScale")
                        * scale,
                    height: canvas[1] / 427.0
                        * self.settings.vr.float("essentialsContainerScale")
                        * scale,
                });
            }
        }
        if let Some((at, rot)) = self.essentials_ui.container_pose
            && let Some(panel) = container.as_mut()
        {
            panel.pos = at - room.rest * scale;
            panel.rot = rot;
        }
        self.essentials_ui.panels = [wrist, container];
        let buttons = headset::hand_buttons();
        // One cursor owns a press through release, including when dragged to another panel.
        let mut target = None;
        let order = if self.essentials_ui.down.iter().any(|b| *b) {
            [self.essentials_ui.owner, self.essentials_ui.owner]
        } else if buttons[1].trigger > 0.5 || buttons[1].primary {
            [1, 0]
        } else {
            [0, 1]
        };
        for hand in order {
            let Some(h) = hands[hand] else {
                continue;
            };
            for (idx, p) in self.essentials_ui.panels.into_iter().enumerate() {
                if idx == 0
                    && self.settings.vr.enum_name("essentialsInventoryAnchor")
                        == if hand == 0 { "MAIN_HAND" } else { "OFFHAND" }
                {
                    continue;
                }
                let Some(p) = p else {
                    continue;
                };
                let scope = if idx == 0 {
                    Scope::Wrist
                } else {
                    Scope::Container
                };
                if let Some((cursor, distance)) =
                    self.essentials_panel_hit(p, scope, h.aim.0, h.aim.1 * Vec3::NEG_Z, 0.0)
                    && target.is_none_or(|(old, _, _, d)| hand == old && distance < d)
                {
                    target = Some((hand, scope, cursor, distance));
                }
            }
        }
        if self.visor_keyboard_hovered() {
            self.essentials_ui.captured = true;
            headset::set_triggers_captured([true; 2]);
            return true;
        }
        let before = self.essentials_ui.down;
        if let Some(grab) = self.essentials_ui.grab {
            let hand = grab.hand();
            if buttons[hand].trigger <= 0.5 || hands[hand].is_none() {
                self.essentials_ui.grab = None;
                self.settings.save(&Self::options_path());
            } else {
                let h = hands[hand].expect("tracked drag hand");
                match grab {
                    Grab::Drag { panel, drag, .. } => {
                        let (pos, rot) = drag.pose(h.aim.0, h.aim.1, scale);
                        if panel == 0 {
                            if let Some((position, rotation)) = self.essentials_wrist_anchors() {
                                let offset = position.1.inverse() * (pos - position.0) / scale;
                                let (roll, yaw, pitch) =
                                    (rotation.1.inverse() * rot).to_euler(EulerRot::ZYX);
                                let vr = std::sync::Arc::make_mut(&mut self.settings.vr);
                                vr.set("essentialsInventoryAimedRotation", "false");
                                vr.set(
                                    "essentialsInventoryPosition",
                                    &format!("{},{},{}", offset.x, offset.y, offset.z),
                                );
                                vr.set(
                                    "essentialsInventoryRotation",
                                    &format!(
                                        "{},{},{}",
                                        pitch.to_degrees(),
                                        yaw.to_degrees(),
                                        roll.to_degrees()
                                    ),
                                );
                            }
                        } else {
                            self.essentials_ui.container_pose =
                                Some((pos + room.rest * scale, rot));
                        }
                        if let Some(p) = self.essentials_ui.panels[panel].as_mut() {
                            p.pos = pos;
                            p.rot = rot;
                        }
                    }
                    Grab::Resize { panel, resize, .. } => {
                        if let (Some(a), Some(b)) = (hands[0], hands[1]) {
                            let key = if panel == 0 {
                                "essentialsInventoryScale"
                            } else {
                                "essentialsContainerScale"
                            };
                            std::sync::Arc::make_mut(&mut self.settings.vr).set_float(
                                key,
                                resize.scale_now(a.grip.0, b.grip.0).clamp(0.1, 3.0),
                            );
                        }
                    }
                }
            }
            self.essentials_ui.down = [buttons[hand].trigger > 0.5, buttons[hand].primary];
            self.essentials_ui.captured = true;
            headset::set_triggers_captured([true; 2]);
            return true;
        }
        if let Some((hand, scope, (x, y), _)) = target {
            let panel = usize::from(scope == Scope::Container);
            let handles = self.essentials_handles(scope);
            if buttons[hand].trigger > 0.5
                && !before[0]
                && let Some(which) = handles
                    .iter()
                    .position(|[l, t, w, h]| x >= *l && x < l + w && y >= *t && y < t + h)
            {
                let pose = self.essentials_ui.panels[panel].expect("hit panel");
                let h = hands[hand].expect("hit hand");
                self.essentials_ui.grab = if which == 0 {
                    Some(Grab::Drag {
                        panel,
                        hand,
                        drag: Drag::start(h.aim.0, h.aim.1, pose.pos, pose.rot, scale),
                    })
                } else {
                    hands[1 - hand].map(|other| Grab::Resize {
                        panel,
                        hand,
                        resize: Resize::start(
                            h.grip.0,
                            other.grip.0,
                            self.settings.vr.float(if panel == 0 {
                                "essentialsInventoryScale"
                            } else {
                                "essentialsContainerScale"
                            }),
                        ),
                    })
                };
                self.reset_inventory_clicks();
                self.essentials_ui.down = [true, false];
                self.essentials_ui.captured = true;
                headset::set_triggers_captured([true; 2]);
                return true;
            }
        }
        let owner = target.map_or(self.essentials_ui.owner, |(h, _, _, _)| h);
        let down = [buttons[owner].trigger > 0.5, buttons[owner].primary];
        let prev_focus = self.essentials_ui.focused;
        if let Some((hand, scope, cursor, _)) = target {
            self.essentials_ui.owner = hand;
            self.essentials_ui.focused = scope;
            self.essentials_ui.cursor = cursor;
            if scope != prev_focus {
                headset::pulse(hand == 0, 0.2, 0.02);
            }
            let oldscreen = self.screen;
            let oldcursor = self.cursor;
            self.screen = Screen::Inventory;
            self.cursor = cursor;
            self.essentials_ui.scope = scope;
            if self.inv_drag.is_some() {
                self.inventory_drag_move();
            }
            if scope == Scope::Container && before[0] {
                self.station_drag();
                self.recipe_book_drag();
            }
            let now = self.pad.headset.vr.clock_ms();
            let scroll = headset::pad().map_or(0.0, |p| p.right[1]);
            if scope == Scope::Container
                && scroll.abs() > 0.5
                && now.saturating_sub(self.essentials_ui.scroll_at) > 100
            {
                self.station_scroll(-scroll);
                self.essentials_ui.scroll_at = now;
            }
            for i in 0..2 {
                if down[i] && !before[i] {
                    let (ox, oy) = self.inventory_origin(&HudBatch::new([1280, 720]));
                    let width = layout::width(self.essentials_offhand_slot());
                    if i == 0
                        && scope == Scope::Wrist
                        && self.essentials_inventory_full()
                        && self.creative
                        && cursor.0 >= ox + (width - 120.0) * 0.5
                        && cursor.0 < ox + (width + 120.0) * 0.5
                        && cursor.1 >= oy - 24.0
                        && cursor.1 < oy - 4.0
                    {
                        self.screen = Screen::Creative;
                        self.refresh_creative_tabs();
                        self.creative_screen.reopen();
                        self.capture(false);
                    } else {
                        self.inventory_press(i == 1);
                    }
                } else if !down[i] && before[i] {
                    self.inventory_mouse_release(i == 1);
                    if scope == Scope::Container {
                        self.station_release();
                    }
                    self.recipe_book_release();
                }
            }
            if self.recipe_search_focused() || (scope == Scope::Container && self.station_typing())
            {
                self.essentials_ui.keyboard_scope = scope;
            }
            self.essentials_ui.scope = Scope::None;
            self.cursor = oldcursor;
            if self.screen == Screen::Inventory {
                self.screen = oldscreen;
            }
        } else {
            self.essentials_ui.focused = Scope::None;
            // A carried item follows the cursor hand. Clicking it away from either panel
            // is vanilla slot -999: left drops the stack, right drops a single item.
            if self.carried.is_some() && self.screen == Screen::Playing {
                let oldscreen = self.screen;
                self.screen = Screen::Inventory;
                for i in 0..2 {
                    if down[i] && !before[i] {
                        self.throw_click(i == 1, horizon_client::menu::Input::Pickup);
                    }
                }
                self.screen = oldscreen;
            }
            if before.iter().any(|b| *b) && !down.iter().any(|b| *b) {
                self.reset_inventory_clicks();
            }
        }
        self.essentials_ui.down = down;
        let captured =
            target.is_some() || (self.carried.is_some() && self.screen == Screen::Playing);
        self.essentials_ui.captured = captured;
        if captured {
            self.left = false;
            self.right = false;
            self.visor.swing.clear();
            headset::set_triggers_captured([true; 2]);
        }
        captured
    }
    fn essentials_panel_hit(
        &self,
        p: headset::OverlaySpec,
        scope: Scope,
        at: Vec3,
        dir: Vec3,
        extra: f32,
    ) -> Option<((f32, f32), f32)> {
        if dir.dot(p.rot * Vec3::Z) >= -1e-5 {
            return None;
        }
        let ([u, v], distance) = horizon_visor::keyboard::ray_uv(
            at,
            dir,
            p.pos,
            p.rot,
            p.width * (1.0 + extra),
            p.height * (1.0 + extra),
        )?;
        let canvas = self.essentials_ui.canvas;
        let x = (u - 0.5) * (1.0 + extra) * canvas[0] + canvas[0] * 0.5;
        let y = (v - 0.5) * (1.0 + extra) * canvas[1] + canvas[1] * 0.5;
        let (width, height) = if scope == Scope::Wrist {
            (
                layout::width(self.essentials_offhand_slot()),
                layout::HEIGHT,
            )
        } else {
            self.essentials_container_dimensions()
        };
        let left = (canvas[0] - width) * 0.5;
        let top = (canvas[1] - height) * 0.5;
        let book = self.essentials_ui.book_visible[0]
            && scope == Scope::Wrist
            && self.essentials_inventory_full()
            && y >= top + layout::HEIGHT
            && y < top + layout::HEIGHT + 168.0
            && x >= canvas[0] * 0.5 - 90.0
            && x < canvas[0] * 0.5 + 90.0;
        let handle = self
            .essentials_handles(scope)
            .iter()
            .any(|[l, t, w, h]| x >= *l && x < l + w && y >= *t && y < t + h);
        let creative = scope == Scope::Wrist
            && self.essentials_inventory_full()
            && self.creative
            && x >= canvas[0] * 0.5 - 60.0
            && x < canvas[0] * 0.5 + 60.0
            && y >= top - 24.0
            && y < top - 4.0;
        let container_book = scope == Scope::Container
            && self.essentials_ui.book_visible[1]
            && x >= canvas[0] * 0.5 - 190.0
            && x < canvas[0] * 0.5
            && y >= canvas[1] * 0.5 - 83.0
            && y < canvas[1] * 0.5 + 83.0;
        ((x >= left - extra * width
            && x < left + width * (1.0 + extra)
            && y >= top - extra * height
            && y < top + height * (1.0 + extra))
            || book
            || handle
            || creative
            || container_book)
            .then_some(((x, y), distance))
    }
    /// Draw into dedicated XR layers; the two panels share the current authoritative menu.
    pub(super) fn essentials_inventory_overlays(
        &mut self,
        size: [u32; 2],
    ) -> [Option<HudBatch>; 3] {
        let mut batches: [Option<HudBatch>; 3] = std::array::from_fn(|_| None);
        for (index, batch_out) in batches[..2].iter_mut().enumerate() {
            let slot = if index == 0 {
                headset::OVERLAY_INVENTORY
            } else {
                headset::OVERLAY_CONTAINER
            };
            let Some(panel) =
                self.essentials_ui.panels[index].filter(|_| self.essentials_inventory_wanted())
            else {
                headset::set_overlay_at(slot, None);
                continue;
            };
            let scope = if index == 0 {
                Scope::Wrist
            } else {
                Scope::Container
            };
            let mut batch = HudBatch::new(size);
            batch.width = self.essentials_ui.canvas[0];
            batch.height = self.essentials_ui.canvas[1];
            let oldscreen = self.screen;
            let oldcursor = self.cursor;
            self.screen = Screen::Inventory;
            self.essentials_ui.scope = scope;
            self.cursor = if self.essentials_ui.focused == scope {
                self.essentials_ui.cursor
            } else {
                (-10000.0, -10000.0)
            };
            if scope == Scope::Wrist {
                self.draw_essentials_wrist(&mut batch);
            } else {
                self.draw_essentials_container(&mut batch);
            }
            self.essentials_ui.book_visible[index] =
                self.active_book().is_some_and(|b| self.book_open(b));
            for rect in self.essentials_handles(scope) {
                batch.fill(
                    &self.gui.atlas,
                    rect[0],
                    rect[1],
                    rect[2],
                    rect[3],
                    0x6EBE_BEBE,
                );
            }
            if self.essentials_ui.focused == scope {
                batch.fill(
                    &self.gui.atlas,
                    self.cursor.0 - 1.0,
                    self.cursor.1 - 1.0,
                    2.0,
                    2.0,
                    u32::MAX,
                );
                self.essentials_ui.scope = Scope::None;
                self.draw_carried(&mut batch);
                self.essentials_ui.scope = scope;
            }
            self.screen = oldscreen;
            self.cursor = oldcursor;
            self.essentials_ui.scope = Scope::None;
            fit_canvas(&mut batch, size);
            headset::set_overlay_at(slot, Some(panel));
            *batch_out = Some(batch);
        }
        let drag_slot = headset::OVERLAY_DRAGGED;
        if self.essentials_inventory_wanted()
            && self.essentials_ui.focused == Scope::None
            && self.screen == Screen::Playing
            && let Some(stack) = self.carried.clone()
            && let Some(hand) = if self.essentials_ui.owner == 0 {
                headset::main_hand()
            } else {
                headset::off_hand()
            }
        {
            let mut batch = HudBatch::new(size);
            let (x, y) = (batch.width * 0.5 - 8.0, batch.height * 0.5 - 8.0);
            self.draw_item(&mut batch, &stack, x, y);
            let (at, rot) = super::visor_overlay::upright_panel(
                hand.aim.0,
                hand.aim.1 * Vec3::NEG_Z,
                0.6 * headset::world_scale(),
            );
            headset::set_overlay_at(
                drag_slot,
                Some(headset::OverlaySpec::new(
                    at,
                    rot,
                    batch.width / 427.0 * headset::world_scale(),
                )),
            );
            batches[2] = Some(batch);
        } else {
            headset::set_overlay_at(drag_slot, None);
        }
        batches
    }
    fn draw_essentials_wrist(&mut self, batch: &mut HudBatch) {
        let (ox, oy) = self.inventory_origin(batch);
        let offhand = self.essentials_offhand_slot();
        let full = self.essentials_inventory_full();
        let texture = if !full {
            "inventory_simplified"
        } else if offhand {
            "inventory_with_offhand"
        } else {
            "inventory"
        };
        let width = if full { layout::width(offhand) } else { 258.0 };
        let x = if !full {
            ox + layout::width(offhand) - 258.0
        } else {
            ox
        };
        batch.sprite_sub(
            &self.gui.atlas,
            &format!("horizon/essentials/{texture}"),
            [0.0, 0.0, width, 156.0],
            [width, 156.0],
            [x, oy, width, 156.0],
            u32::MAX,
        );
        if full {
            if self.creative {
                let text = self.translate("starling.essentials.creative", "Creative Inventory");
                horizon_client::ui::button(
                    batch,
                    &self.gui.atlas,
                    ox + (width - 120.0) * 0.5,
                    oy - 24.0,
                    120.0,
                    20.0,
                    &horizon_client::text::Text::literal(text),
                    true,
                    false,
                );
            }
            self.draw_inventory_player(batch, ox, oy);
        }
        self.draw_hovered_slot_highlight(batch, false, ox, oy);
        self.draw_inventory_slots(batch, ox, oy);
        self.draw_hovered_slot_highlight(batch, true, ox, oy);
        if full {
            self.draw_recipe_book(batch);
            if let Some(lines) =
                self.draw_inventory_effects(batch, (ox, oy), layout::width(offhand) as i32)
            {
                self.draw_tooltip_lines(batch, &lines, self.cursor.0, self.cursor.1);
            }
        }
        self.draw_slot_tooltip(batch);
    }
    fn draw_essentials_container(&mut self, batch: &mut HudBatch) {
        // The container-specific widgets, progress, offers and menu actions are the existing
        // game implementations. Replace their backgrounds with Essentials' original art.
        let names = [
            "furnace",
            "blast_furnace",
            "smoker",
            "brewing_stand",
            "crafting_table",
            "anvil",
            "grindstone",
            "stonecutter",
            "loom",
            "smithing",
            "enchanting_table",
            "beacon",
            "cartography_table",
            "hopper",
            "dispenser",
            "horse",
            "villager",
            "generic_54",
            "shulker_box",
        ];
        let mut previous = Vec::new();
        for name in names {
            let key = format!("gui/container/{name}");
            if let Some(sprite) = self
                .gui
                .atlas
                .sprites
                .get(&format!("horizon/essentials/container/{name}"))
                .copied()
            {
                previous.push((key.clone(), self.gui.atlas.sprites.insert(key, sprite)));
            }
        }
        self.draw_inventory(batch);
        for (key, value) in previous {
            if let Some(value) = value {
                self.gui.atlas.sprites.insert(key, value);
            } else {
                self.gui.atlas.sprites.remove(&key);
            }
        }
        // No full-screen dimming around a world-space panel.
        if batch.vertices.len() >= 6
            && batch.vertices[..6]
                .iter()
                .all(|v| v.pos[0] == 0.0 || v.pos[0] == batch.width)
        {
            for v in &mut batch.vertices[..6] {
                v.color = 0;
            }
        }
        let (_ox, oy) = self.inventory_origin(batch);
        let (w, h) = self.essentials_container_dimensions();
        let left = (self.essentials_ui.canvas[0] - w) * 0.5;
        batch.clip_since(0, left, oy, left + w, oy + h);
        if self.active_book().is_some_and(|b| self.book_open(b)) {
            self.draw_recipe_book(batch);
        }
    }
}

impl Game<'_> {
    fn essentials_handles(&self, scope: Scope) -> [[f32; 4]; 2] {
        let (w, h) = if scope == Scope::Wrist {
            (layout::width(self.essentials_offhand_slot()), 156.0)
        } else {
            self.essentials_container_dimensions()
        };
        let index = usize::from(scope == Scope::Container);
        let y = (self.essentials_ui.canvas[1] + h) * 0.5
            + 4.0
            + if index == 0 && self.essentials_ui.book_visible[0] {
                168.0
            } else {
                0.0
            };
        let x = self.essentials_ui.canvas[0] * 0.5;
        let _ = w;
        [[x - 42.0, y, 70.0, 7.0], [x + 31.0, y, 7.0, 7.0]]
    }
    pub(super) fn essentials_pointer_lines(
        &self,
        camera: [f64; 3],
        out: &mut Vec<horizon_client::gizmos::Line>,
    ) -> bool {
        if self.essentials_ui.focused == Scope::None {
            return false;
        }
        let index = usize::from(self.essentials_ui.focused == Scope::Container);
        let Some(p) = self.essentials_ui.panels[index] else {
            return false;
        };
        let hand = if self.essentials_ui.owner == 0 {
            headset::main_hand()
        } else {
            headset::off_hand()
        };
        let Some(hand) = hand else {
            return false;
        };
        let (x, y) = self.essentials_ui.cursor;
        let canvas = self.essentials_ui.canvas;
        let at = p.pos
            + p.rot
                * Vec3::new(
                    (x / canvas[0] - 0.5) * p.width,
                    (0.5 - y / canvas[1]) * p.height,
                    0.0,
                );
        out.push(super::headset::menu_pointer_line(
            camera,
            headset::body_yaw(),
            hand.aim,
            at.distance(hand.aim.0),
        ));
        true
    }
}

impl Game<'_> {
    /// Keyboard input uses the focused overlay's screen/menu, without globally pausing play.
    pub(super) fn essentials_keyboard_frame(&mut self) -> bool {
        if !self.essentials_inventory_wanted() || self.essentials_ui.keyboard_scope == Scope::None {
            return false;
        }
        let scope = self.essentials_ui.keyboard_scope;
        if scope == Scope::Container && !self.essentials_ui.container {
            self.essentials_ui.keyboard_scope = Scope::None;
            return false;
        }
        let oldscreen = self.screen;
        if oldscreen != Screen::Playing {
            return false;
        }
        self.screen = Screen::Inventory;
        self.essentials_ui.scope = scope;
        let typing =
            self.recipe_search_focused() || (scope == Scope::Container && self.station_typing());
        if typing {
            self.visor_keyboard_auto(true);
            self.visor_keyboard_frame();
        } else {
            self.visor_keyboard_close(false);
            self.essentials_ui.keyboard_scope = Scope::None;
        }
        self.essentials_ui.scope = Scope::None;
        if self.screen == Screen::Inventory {
            self.screen = oldscreen;
        }
        typing
    }
}

/// HudRenderer projects using target size / GUI scale. Map the taller virtual canvas
/// into that viewport; the XR quad's aspect restores square pixels in world space.
fn fit_canvas(batch: &mut HudBatch, size: [u32; 2]) {
    let viewport = HudBatch::new(size);
    let scale = [viewport.width / batch.width, viewport.height / batch.height];
    for vertex in &mut batch.vertices {
        vertex.pos[0] *= scale[0];
        vertex.pos[1] *= scale[1];
    }
    for (_, blend) in &mut batch.blends {
        if let horizon_client::hud::Blend::Depth(rect) = blend {
            rect[0] = (rect[0] as f32 * scale[0]).round() as u32;
            rect[1] = (rect[1] as f32 * scale[1]).round() as u32;
            rect[2] = (rect[2] as f32 * scale[0]).round() as u32;
            rect[3] = (rect[3] as f32 * scale[1]).round() as u32;
        }
    }
    batch.width = viewport.width;
    batch.height = viewport.height;
}

impl Game<'_> {
    pub(super) fn essentials_gui_hands(&self) -> [bool; 2] {
        let mut hands = [false; 2];
        if self.essentials_ui.focused != Scope::None || self.carried.is_some() {
            hands[self.essentials_ui.owner] = self.essentials_inventory_wanted();
        }
        hands
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tall_canvas_projects_to_the_slot_the_controller_ray_hits() {
        let mut batch = HudBatch::new([1280, 720]);
        let viewport = [batch.width, batch.height];
        batch.height = 536.0;
        let before = [batch.width * 0.5, 268.0];
        batch.vertices.push(horizon_client::hud::HudVertex {
            pos: before,
            uv: [0.0; 2],
            color: u32::MAX,
            depth: 0.0,
            glint_uv: [0.0; 2],
            text: 0,
        });
        fit_canvas(&mut batch, [1280, 720]);
        let point = batch.vertices[0].pos;
        assert!((point[0] / viewport[0] - 0.5).abs() < 1e-6);
        assert!((point[1] / viewport[1] - 0.5).abs() < 1e-6);
        assert_eq!([batch.width, batch.height], viewport);
    }
}

impl Game<'_> {
    fn essentials_wrist_anchors(&self) -> Option<((Vec3, Quat), (Vec3, Quat))> {
        let anchor = |key| match self.settings.vr.enum_name(key) {
            "MAIN_HAND" => headset::main_hand().map(|h| h.aim),
            "OFFHAND" => headset::off_hand().map(|h| h.aim),
            "HMD" => headset::room().map(|r| {
                (
                    (r.head.0 - r.rest) * headset::world_scale()
                        + Vec3::Y * ((headset::world_scale() - 1.0) * r.rest.y),
                    r.head.1,
                )
            }),
            _ => Some((Vec3::ZERO, Quat::IDENTITY)),
        };
        Some((
            anchor("essentialsInventoryAnchor")?,
            anchor("essentialsInventoryRotationAnchor")?,
        ))
    }
    fn essentials_source_position(&self, id: u32) -> Option<(DVec3, f32)> {
        let alpha = self.clock.camera_fraction();
        let (position, eye) = if let Some(m) = self.mobs.iter().find(|m| m.id == id && m.alive()) {
            (
                m.previous.lerp(m.position, alpha),
                (m.eye_y() - m.position.y) as f32,
            )
        } else if let Some(b) = self.boats.iter().find(|b| b.id == id) {
            (b.previous.lerp(b.position, alpha), 0.5625)
        } else {
            let c = self.carts.iter().find(|c| c.id == id)?;
            (c.previous.lerp(c.position, alpha), 0.7)
        };
        Some((DVec3::new(position.x, position.y, position.z), eye))
    }
}
