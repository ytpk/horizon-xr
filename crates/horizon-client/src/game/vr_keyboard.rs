// SPDX-License-Identifier: LGPL-3.0-or-later
// Derived from Vivecraft: client_vr/gui/PhysicalKeyboard.java, gameplay/screenhandlers/KeyboardHandler.java, render/helpers (keyboard drawing). Copyright (c) Vivecraft contributors; LGPL-3.0-or-later.
//! Vivecraft's physical keyboard in the game: it opens below the head when a text field takes
//! focus (`autoOpenKeyboard`), the controllers' tips press its keys (`horizon-vivecraft`'s
//! `PhysicalKeyboard` decides which), and the keys type into the focused field. The keys are
//! drawn as translucent quads with their labels in the world, like a name tag's text.
use super::{Game, Screen};
use glam::{Mat4, Quat, Vec3};
use horizon_client::{
    block_entity_render::vertex_light,
    headset,
    hud::{HudBatch, HudVertex},
    mesher::Vertex,
    text::Text,
};
use horizon_vivecraft::keyboard::{
    KeyAction, KeyEvent, KeyLabel, PhysicalKeyboard, current_layout_id, layout_chars,
    next_layout_index, orient,
};
use std::sync::Arc;
use winit::keyboard::KeyCode;

/// A font pixel's size on a key (`textScale = 0.002 * scale`).
const TEXT_UNIT: f32 = 0.002;

pub(super) struct VrKeyboard {
    kb: PhysicalKeyboard,
    showing: bool,
    /// Where the keyboard is, in the room.
    pos: Vec3,
    rot: Quat,
    /// A text field had focus at the last look.
    focused: bool,
    screen: Option<Screen>,
    events: Vec<KeyEvent>,
    /// The labels' quads in keyboard space, per key `(start, end)`, for layout `built`.
    labels: Vec<HudVertex>,
    ranges: Vec<(u32, u32)>,
    built: u32,
}

impl Default for VrKeyboard {
    fn default() -> Self {
        Self {
            kb: PhysicalKeyboard::default(),
            showing: false,
            pos: Vec3::ZERO,
            rot: Quat::IDENTITY,
            focused: false,
            screen: None,
            events: Vec::new(),
            labels: Vec::new(),
            ranges: Vec::new(),
            built: 0,
        }
    }
}

impl VrKeyboard {
    pub(super) fn showing(&self) -> bool {
        self.showing
    }
}

/// An ARGB colour from 0..1 channels.
fn argb([r, g, b, a]: [f32; 4]) -> u32 {
    let c = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
    c(a) << 24 | c(r) << 16 | c(g) << 8 | c(b)
}

impl Game<'_> {
    /// Every loop iteration in a headset: opens and closes the keyboard with the text field's
    /// focus (`autoOpenKeyboard`, `autoCloseKeyboard`), and types what the hands press.
    pub(super) fn vr_keyboard_tick(&mut self, now_ms: u64) {
        let settings = headset::settings();
        let usable = headset::active() && !settings.seated() && settings.bool("physicalKeyboard");
        let focused = usable && self.text_field_focused();
        let screen = self.screen;
        let forced = self.sign_edit.is_some() || self.book.is_some();
        let mode = settings.enum_name("autoOpenKeyboard");
        let open_mode = mode == "ON" || (mode == "CHAT" && screen == Screen::Chat);
        let vr = &mut self.pad.headset.vr;
        let k = &mut vr.keyboard;
        let rose = focused && !k.focused;
        let fell = !focused && k.focused;
        k.focused = focused;
        if !usable {
            if k.showing {
                self.vr_keyboard_hide();
            }
            return;
        }
        if rose && (open_mode || forced) {
            self.vr_keyboard_show();
        } else if fell && settings.bool("autoCloseKeyboard") && k.showing {
            self.vr_keyboard_hide();
        }
        let vr = &mut self.pad.headset.vr;
        let k = &mut vr.keyboard;
        if !k.showing {
            return;
        }
        // `GuiHandler.onScreenChanged` re-orients the keyboard for the new screen.
        if k.screen != Some(screen) {
            k.screen = Some(screen);
            (k.pos, k.rot) = orient(&vr.player.room_pre.hmd);
        }
        let (regular, shifted) = layout_chars(current_layout_id(&settings));
        k.kb.set_layout(regular, shifted, settings.float("physicalKeyboardScale"));
        // The shift binding (the grips) holds a sticky shift while held.
        let grips = headset::hand_buttons().map(|b| b.grip > 0.6);
        k.kb.shift_binding(grips[0] || grips[1]);
        let room = &vr.player.room_pre;
        let tips =
            [0, 1].map(|c| room.tracking[c].then(|| k.kb.to_local(k.pos, k.rot, room.c[c].pos)));
        let mut events = std::mem::take(&mut k.events);
        k.kb.process(now_ms, tips, &mut events);
        if k.kb.generation != k.built {
            k.built = k.kb.generation;
            self.vr_keyboard_labels();
        }
        for event in events.drain(..) {
            self.vr_key_event(event);
        }
        self.pad.headset.vr.keyboard.events = events;
    }

    fn vr_keyboard_show(&mut self) {
        let vr = &mut self.pad.headset.vr;
        let k = &mut vr.keyboard;
        k.showing = true;
        k.screen = Some(self.screen);
        (k.pos, k.rot) = orient(&vr.player.room_pre.hmd);
        k.kb.show();
    }

    /// Visor's game menu "Keyboard": the keyboard opens below the head, or closes when it shows.
    pub(super) fn vr_keyboard_toggle(&mut self) {
        if self.visor_keyboard_wanted() {
            self.visor_keyboard_toggle();
            return;
        }
        if self.pad.headset.vr.keyboard.showing() {
            self.vr_keyboard_hide();
        } else {
            self.vr_keyboard_show();
        }
    }

    pub(super) fn vr_keyboard_hide(&mut self) {
        let k = &mut self.pad.headset.vr.keyboard;
        k.showing = false;
        k.screen = None;
        let mut events = std::mem::take(&mut k.events);
        k.kb.unpress_all(&mut events);
        events.clear();
        k.events = events;
    }

    /// The keys' labels as quads in keyboard space; the glyphs they need are uploaded with
    /// the next frame.
    fn vr_keyboard_labels(&mut self) {
        let mut batch = HudBatch::new([1, 1]);
        let k = &mut self.pad.headset.vr.keyboard;
        k.labels.clear();
        k.ranges.clear();
        let unit = TEXT_UNIT * k.kb.scale;
        for key in k.kb.keys() {
            let text = match key.key.label {
                KeyLabel::Char('\0') | KeyLabel::Char(' ') => Text::empty(),
                KeyLabel::Char(c) => Text::literal(c.to_string()),
                KeyLabel::Symbol(s) => Text::literal(s),
                KeyLabel::Lang(name) => {
                    Text::translate(&format!("vivecraft.keyboard.key.{name}"), &[])
                }
            };
            batch.vertices.clear();
            // Centred on the key: the text is 9 pixels tall.
            batch.name_tag(&self.gui.atlas, &text, -4.5, u32::MAX, 0);
            let centre = (key.min + key.max) / 2.0;
            let start = k.labels.len() as u32;
            k.labels.extend(batch.vertices.iter().map(|v| HudVertex {
                pos: [centre.x + v.pos[0] * unit, centre.y + v.pos[1] * unit],
                ..*v
            }));
            k.ranges.push((start, k.labels.len() as u32));
        }
        self.map_patches.extend(std::mem::take(&mut batch.patches));
    }

    /// What a key press does in the game.
    fn vr_key_event(&mut self, event: KeyEvent) {
        let KeyEvent::Press {
            action,
            controller,
            repeat,
        } = event
        else {
            return;
        };
        if !repeat {
            self.audio
                .play("minecraft:ui.button.click", None, 0.25, 1.0);
        }
        headset::pulse(controller == 0, 0.8, if repeat { 0.015 } else { 0.03 });
        let code = match action {
            KeyAction::Char(c) => {
                let _ = self.type_chars_into_screen(&c.to_string());
                return;
            }
            KeyAction::Space => {
                let _ = self.type_chars_into_screen(" ");
                return;
            }
            KeyAction::Paste => {
                use horizon_client::clipboard::Clipboard;
                let text = self.clipboard.get();
                let text: String = text.chars().filter(|c| !c.is_control()).collect();
                if !text.is_empty() {
                    let _ = self.type_chars_into_screen(&text);
                }
                return;
            }
            KeyAction::NextLayout => {
                let vr = Arc::make_mut(&mut self.settings.vr);
                let next = next_layout_index(vr);
                vr.set_int("currentKeyboardLayout", next);
                return;
            }
            KeyAction::Shift => return,
            KeyAction::Tab => KeyCode::Tab,
            KeyAction::Escape => KeyCode::Escape,
            KeyAction::Backspace => KeyCode::Backspace,
            KeyAction::Enter => KeyCode::Enter,
            KeyAction::Up => KeyCode::ArrowUp,
            KeyAction::Down => KeyCode::ArrowDown,
            KeyAction::Left => KeyCode::ArrowLeft,
            KeyAction::Right => KeyCode::ArrowRight,
        };
        let _ = self.key_pressed(code, code);
    }

    /// The keyboard's keys and labels, camera relative, for the entity pass (drawn like a name
    /// tag, on top of nothing: a wall in front hides them).
    pub(super) fn vr_keyboard_geometry(&mut self, camera: glam::DVec3, out: &mut Vec<Vertex>) {
        if !self.pad.headset.vr.keyboard.showing || !headset::active() {
            return;
        }
        let theme = headset::settings()
            .enum_name("physicalKeyboardTheme")
            .to_string();
        let vr = &mut self.pad.headset.vr;
        let world = &vr.player.world_pre;
        let k = &mut vr.keyboard;
        if !k.kb.has_keys() {
            return;
        }
        // Keyboard space to camera relative world space.
        let to_world = Mat4::from_translation((world.origin - camera).as_vec3())
            * Mat4::from_rotation_y(world.rotation)
            * Mat4::from_scale(Vec3::splat(world.world_scale))
            * Mat4::from_rotation_translation(k.rot, k.pos)
            * Mat4::from_translation(-k.kb.center());
        let scale = k.kb.scale;
        let atlas = &self.gui.atlas;
        let Some(white) = atlas.sprites.get("horizon/white") else {
            return;
        };
        let uv = [(white.u0 + white.u1) / 2.0, (white.v0 + white.v1) / 2.0];
        let light = vertex_light(0xFF);
        let vertex = |m: &Mat4, p: Vec3, uv: [f32; 2], color: u32| Vertex {
            pos: m.transform_point3(p).to_array(),
            uv,
            layer: 0,
            color,
            light,
            normal: [0; 4],
            glint_uv: [0.0, 0.0],
        };
        for key in k.kb.keys() {
            let (min, max) = key.render_box(scale);
            let color = argb(k.kb.key_color(key, &theme));
            // The corners from the bottom left, as the world draws quads.
            for (x, y) in [
                (min.x, max.y),
                (max.x, max.y),
                (max.x, min.y),
                (min.x, min.y),
            ] {
                out.push(vertex(&to_world, Vec3::new(x, y, min.z), uv, color));
            }
        }
        // The labels sit a millimetre in front of the key faces.
        let lift = 0.001 * scale;
        for (key, &(start, end)) in k.kb.keys().iter().zip(&k.ranges) {
            let (min, _) = key.render_box(scale);
            let z = min.z - lift;
            for quad in k.labels[start as usize..end as usize].as_chunks::<6>().0 {
                for v in [quad[5], quad[2], quad[1], quad[0]] {
                    out.push(vertex(
                        &to_world,
                        Vec3::new(v.pos[0], v.pos[1], z),
                        v.uv,
                        v.color,
                    ));
                }
            }
        }
    }
}
