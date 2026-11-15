// SPDX-License-Identifier: LGPL-3.0-only
// Copyright (c) 2025-2026 Phoenix-Ra and VisorEssentials contributors.
//! VisorEssentials RemoteOverlayRenderHelper, ported to Starling's world renderer.
//! Upstream 88ffec507449eb1a97ccbf495cd331a46c6d635b; see docs/modded.md.
use super::{Game, names::TextAxes};
use glam::DVec3;
use horizon_client::{block_entity_render::vertex_light, mesher::Vertex, text::Text};
use starling_essentials::{FocusPanel, PANEL_COLOR, TEXT_COLOR};

impl Game<'_> {
    pub(super) fn essentials_focus_geometry(
        &mut self,
        camera: DVec3,
        alpha: f64,
        out: &mut Vec<Vertex>,
    ) {
        if !self.settings.vr.bool("essentialsOverlayFocusVisualizer") {
            return;
        }
        let Some(white) = self.gui.atlas.sprites.get("horizon/white") else {
            return;
        };
        let uv = [(white.u0 + white.u1) * 0.5, (white.v0 + white.v1) * 0.5];
        let mut panels = Vec::new();
        for mob in &self.mobs {
            if !mob.alive()
                || mob.invisible
                || mob.extra.pose == horizon_client::mobs::sync::SLEEPING
            {
                continue;
            }
            let Some(uuid) = self.remote_player_uuid(mob.id) else {
                continue;
            };
            let Some(player) = self
                .visor
                .net
                .players
                .get(&uuid)
                .filter(|p| p.overlay_focused)
            else {
                continue;
            };
            let before = player.prev_pose.hmd;
            let now = player.pose.hmd;
            let fraction = alpha.clamp(0.0, 1.0) as f32;
            let offset = before.position.lerp(now.position, fraction);
            let position = mob.previous.lerp(mob.position, alpha);
            let head = (DVec3::new(position.x, position.y, position.z) - camera).as_vec3() + offset;
            // Interpolate yaw over the short arc, including across +/-180 degrees.
            let yaw_delta =
                (mob.body_yaw - mob.previous_body_yaw + 180.0).rem_euclid(360.0) - 180.0;
            let yaw = (mob.previous_body_yaw + yaw_delta * fraction).to_radians();
            if let Some(panel) = FocusPanel::new(
                head,
                before.orientation.slerp(now.orientation, fraction),
                yaw,
            ) {
                panels.push(panel);
            }
        }
        // Alpha blending: draw the farthest panels first. Both eyes use camera-relative geometry.
        panels.sort_by(|a, b| {
            b.center
                .length_squared()
                .total_cmp(&a.center.length_squared())
        });
        let layer = horizon_client::gfx::TRANSLUCENT_ITEM_OVERLAY;
        let light = vertex_light(0xFF);
        for panel in panels {
            self.mark_translucent_entity(out.len(), "visor_essentials:overlay_focus");
            for pos in panel.corners() {
                out.push(Vertex {
                    pos: pos.to_array(),
                    uv,
                    layer,
                    color: PANEL_COLOR,
                    light,
                    normal: [0; 4],
                    glint_uv: [0.0, 0.0],
                });
            }
            let (at, right, up) = panel.label_axes();
            let patches = self.text_quads_along(
                (at + up * 0.045).as_dvec3(),
                &[(
                    Text::tr("starling.essentials.overlay", &[]).with_color(TEXT_COLOR),
                    0.0,
                )],
                TextAxes {
                    right,
                    up,
                    away: glam::Vec3::ZERO,
                },
                0.01,
                140,
                None,
                layer,
                light,
                out,
            );
            self.map_patches.extend(patches);
        }
    }
}
