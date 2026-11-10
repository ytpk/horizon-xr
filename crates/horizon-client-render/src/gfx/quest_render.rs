//! Quest room, stereo world passes and OpenXR image handoff.
//! Kept separate from the upstream frame renderer to make updates easier to merge.
use super::*;

impl Renderer {
    /// The launcher's room's pointer: triangles (corner, 0xRRGGBB) in the room's space, drawn over
    /// everything: the ray of the hand that points and the controllers.
    pub fn set_room_pointer(&mut self, triangles: &[([f32; 3], u32)]) {
        self.room_pointer = triangles
            .iter()
            .take(room_scene::POINTER_CAPACITY)
            .map(|&(pos, rgb)| room_layout::Vertex {
                pos,
                uv: [0.0, 0.0],
                data: rgb & 0xFF_FFFF,
            })
            .collect();
    }

    /// The headset launcher's room behind the panel, or the title's menu room again.
    pub fn set_room(&mut self, on: bool) {
        self.room_on = on;
    }

    /// The room's textures from the game's real block textures (`room_layout::load_real`); the
    /// room's own stay in the layers that are not named.
    pub fn set_room_textures(&mut self, layers: Vec<(usize, Vec<u8>)>) {
        self.room_textures = Some(layers);
    }

    /// `set_room_textures` unless the room already has (or is about to take) them.
    pub fn ensure_room_textures(&mut self, layers: &[(usize, Vec<u8>)]) {
        if self.room_textures.is_none() && !self.room_is_real() {
            self.room_textures = Some(layers.to_vec());
        }
    }

    /// The room shows the game's textures.
    pub fn room_is_real(&self) -> bool {
        self.room.as_ref().is_some_and(room_scene::RoomScene::real)
    }

    /// Swap the entire compatible pipeline set atomically; a failed variant
    /// leaves the original renderer intact. Calling again restores mono handles.
    pub(super) fn swap_stereo_pipelines(&mut self) -> bool {
        let mut slots = vec![
            &mut self.solid.plain,
            &mut self.solid_opaque.plain,
            &mut self.solid_cutout.plain,
            &mut self.fluid_pipeline,
            &mut self.translucent,
            &mut self.translucent_mark,
            &mut self.sky,
            &mut self.sky_base,
            &mut self.sunrise_pipeline,
            &mut self.dark_disc,
            &mut self.depth_reset,
            &mut self.lines,
            &mut self.lines_secondary,
            &mut self.debug_lines_opaque,
            &mut self.debug_lines,
            &mut self.debug_lines_on_top,
            &mut self.entity_pipeline,
            &mut self.static_entity_pipeline,
            &mut self.crumbling_pipeline,
            &mut self.shadow_blob_pipeline,
            &mut self.translucent_item_pipeline,
            &mut self.water_mask_pipeline,
            &mut self.cloud_pipeline,
            &mut self.additive_pipeline,
            &mut self.dragon_rays_pipeline,
            &mut self.energy_swirl_pipeline,
            &mut self.lightning_pipeline,
            &mut self.see_through_pipeline,
            &mut self.text_polygon_offset_pipeline,
            &mut self.celestial_pipeline,
            &mut self.stars.pipeline,
            &mut self.weather.pipeline,
        ];
        slots.extend(self.screen_pipelines.iter_mut());
        slots.extend(self.terrain_model_pipelines.iter_mut());
        if let Some(flat) = &mut self.flat {
            slots.extend([
                &mut flat.solid.plain,
                &mut flat.solid_opaque.plain,
                &mut flat.solid_cutout.plain,
                &mut flat.solid_mirror,
                &mut flat.translucent,
                &mut flat.translucent_mark,
            ]);
        }
        let variants = self.reuse.alternates(slots.iter().map(|p| &**p));
        let Some(variants) = variants else {
            return false;
        };
        for (slot, variant) in slots.into_iter().zip(variants) {
            *slot = variant;
        }
        true
    }

    pub(super) fn stereo_compatible(&self, frame: &Frame) -> bool {
        !self.stereo_failed
            && horizon_client_input::headset::settings().bool("horizonMultiview")
            && self.device.features().contains(wgpu::Features::MULTIVIEW)
            && self.device.limits().max_multiview_view_count >= 2
            && (self.gpu_timer.is_none()
                || self
                    .device
                    .features()
                    .contains(wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS))
            && horizon_client_input::headset_display::snapshot().is_none_or(|info| !info.foveated)
            && self.hdr.is_none()
            && self.custom_sky_draws.is_empty()
            && frame.world_primitives.is_empty()
            && self.native.is_none()
            && self.pack_post.is_none()
            && self.core_lines.is_none()
            && !self.improved_transparency
            && self.visbuf.is_none()
            && self.world_post.scale == 1.0
            && !self.world_post.antialias
            && frame.menu_blur.is_none()
            && frame.post_effects.effects.is_empty()
            && frame.glowing.is_empty()
            && frame.skin.is_none_or(|skin| skin.outline_vertices == 0)
            && frame.border.is_none()
            && frame.debug_crosshair.is_empty()
            && self.screenshot.is_none()
            && !native_render::world_drawn_hooked()
            && self.output.frame.format == self.config.format
    }

    /// A headset's frame: each eye drawn in a pass of its own into the runtime's
    /// image, from the eye's pose and lens, or the frame as a screen shows it onto
    /// the menu panel's image while a screen is open.
    pub(super) fn render_headset(&mut self, frame: &Frame) -> Result<bool> {
        if self.device_lost() {
            return Err("the graphics device was lost".into());
        }
        let timed = self.gpu_times.len();
        self.land_frame();
        vr_profile::refresh_often();
        let layers = match horizon_client_input::headset::menu() {
            false => headset::Layers::World,
            true if frame.title => headset::Layers::Flat,
            true => headset::Layers::Screen,
        };
        let body_yaw = horizon_client_input::headset::body_yaw();
        let targets =
            headset::with(|h| h.begin(&self.device, layers)).unwrap_or(headset::Targets::Skip);
        let drawn = match targets {
            headset::Targets::Skip => {
                vr_profile::frame_skipped();
                // Nothing shows: the frame's uploads go to the GPU all the same.
                let suspended = std::mem::replace(&mut self.suspended, true);
                let drawn = self.render_flat(frame);
                self.suspended = suspended;
                return drawn.map(|_| false);
            }
            headset::Targets::Menu(image, eyes) => {
                vr_profile::frame_skipped();
                self.headset_eye = None;
                self.headset_cull = None;
                let size = headset::with(|h| h.menu_size()).unwrap_or([1, 1]);
                self.set_headset_size(size);
                self.offscreen = Some(image.clone());
                let drawn = self.render_flat(frame);
                // Visor's keyboard under the title's screens, on its own image.
                let keyboard = headset::with(|h| {
                    h.overlay_image(horizon_client_input::headset::OVERLAY_KEYBOARD)
                })
                .flatten();
                if let Some(image) = &keyboard {
                    match frame.overlays[horizon_client_input::headset::OVERLAY_KEYBOARD] {
                        Some(batch) => self.draw_headset_hud(batch, Some(image)),
                        None => self.draw_headset_hud(&HudBatch::new([1, 1]), Some(image)),
                    }
                }
                // Behind the panel, the title's panorama around the eyes (a
                // first menu world); dark eyes without one.
                let mut images = vec![&image];
                images.extend(keyboard.as_ref());
                let mut painted = images.len();
                if let Some(eyes) = &eyes {
                    // Visor's main menu: a moss platform over a sea of clouds in a sky that follows
                    // the clock, in place of the game's panorama.
                    let around = if self.room_on {
                        let (device, queue) = (&self.device, &self.queue);
                        let room = self
                            .room
                            .get_or_insert_with(|| room_scene::RoomScene::new(device, queue));
                        if let Some(layers) = self.room_textures.take() {
                            room.set_textures(queue, &layers);
                        }
                        let sky = menu_scene::current_sky();
                        for (image, pose) in eyes.iter() {
                            room.draw_eye(device, queue, image, pose, &sky, &self.room_pointer);
                        }
                        true
                    } else {
                        false
                    } || frame.title && !self.room_on && {
                        let scene = self.menu_scene.get_or_insert_with(|| {
                            menu_scene::MenuScene::new(
                                &self.device,
                                &self.queue,
                                self.panorama.floor(),
                            )
                        });
                        let now = chrono::Local::now();
                        let clock = chrono::Timelike::hour(&now) as f32
                            + chrono::Timelike::minute(&now) as f32 / 60.0
                            + chrono::Timelike::second(&now) as f32 / 3600.0;
                        // `HORIZON_MENU_HOUR=14.5` shows the room at that hour, for looking at it.
                        let hour = std::env::var("HORIZON_MENU_HOUR")
                            .ok()
                            .and_then(|h| h.parse::<f32>().ok())
                            .unwrap_or(clock);
                        let sky = menu_scene::sky_at(
                            hour,
                            menu_scene::moon_age(now.timestamp_millis() as f64),
                        );
                        // `HORIZON_MENU_LOOK=yaw,pitch` (degrees) fixes where the room is seen from (the
                        // head's own turn is ignored), to look at the sky and the sign from a headset
                        // that lies on a table.
                        let look = std::env::var("HORIZON_MENU_LOOK").ok().and_then(|v| {
                            let (y, p) = v.split_once(',')?;
                            Some(
                                glam::Quat::from_rotation_y(
                                    y.trim().parse::<f32>().ok()?.to_radians(),
                                ) * glam::Quat::from_rotation_x(
                                    p.trim().parse::<f32>().ok()?.to_radians(),
                                ),
                            )
                        });
                        for (image, pose) in eyes.iter() {
                            let mut pose = *pose;
                            if let Some(look) = look {
                                pose.orientation = look;
                            }
                            scene.draw_eye(&self.device, &self.queue, image, &pose, &sky);
                        }
                        true
                    };
                    if around {
                        painted += 2;
                    }
                    images.extend(eyes.iter().map(|(image, _)| image));
                }
                self.finish_headset_images(&images, painted);
                drawn
            }
            headset::Targets::Eyes(eyes, hud, upload) => {
                if let Some((image, pixels)) = &upload {
                    let size = image.size();
                    self.queue.write_texture(
                        image.as_image_copy(),
                        pixels,
                        wgpu::TexelCopyBufferLayout {
                            offset: 0,
                            bytes_per_row: Some(4 * size.width),
                            rows_per_image: None,
                        },
                        size,
                    );
                }
                let size = headset::with(|h| h.eye_size()).unwrap_or([1, 1]);
                self.set_headset_size(size);
                let poses = [eyes[0].1, eyes[1].1];
                let cull = headset::cull_view(body_yaw, &poses, frame.depth_far);
                self.headset_cull = Some(cull);
                let mut drawn = Ok(false);
                let masks = headset::with(|h| h.hidden_areas()).flatten();
                let stereo = self.stereo_compatible(frame) && self.swap_stereo_pipelines();
                if stereo {
                    self.stereo_views =
                        Some(poses.map(|p| View::eye(body_yaw, &p, frame.depth_far)));
                    self.headset_eye = Some((self.stereo_views.unwrap()[0], cull));
                    if self
                        .stereo_targets
                        .as_ref()
                        .is_none_or(|t| !t.fits(size, self.config.format))
                    {
                        let validation =
                            self.device.push_error_scope(wgpu::ErrorFilter::Validation);
                        let memory = self.device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
                        let target =
                            multiview::Targets::new(&self.device, size, self.config.format);
                        let error = pollster::block_on(memory.pop())
                            .or(pollster::block_on(validation.pop()));
                        if let Some(error) = error {
                            log::warn!(
                                "Quest multiview targets unavailable, using per-eye passes: {error}"
                            );
                            self.stereo_targets = None;
                            self.stereo_failed = true;
                        } else {
                            self.stereo_targets = Some(target);
                        }
                    }
                    if let Some(target) = &self.stereo_targets {
                        self.offscreen = Some(target.color.clone());
                        self.stereo_destinations = Some([eyes[0].0.clone(), eyes[1].0.clone()]);
                        let drawing = Instant::now();
                        drawn = self.render_flat(frame);
                        self.stereo_destinations = None;
                        vr_profile::record(vr_profile::Stage::EyeLeft, drawing.elapsed());
                        vr_profile::record(vr_profile::Stage::SubmitLeft, self.last_submit);
                        vr_profile::record(vr_profile::Stage::StallLeft, self.last_wait);
                    }
                    self.stereo_views = None;
                    self.headset_eye = None;
                    assert!(self.swap_stereo_pipelines());
                }
                let stereo_drawn = stereo && self.stereo_targets.is_some();
                if self.stereo_status != Some(stereo_drawn) || self.stereo_sample_size != size {
                    log::info!(
                        "Quest world rendering: {}",
                        if stereo_drawn {
                            "single-pass multiview (2 layers)"
                        } else {
                            "separate eye passes (disabled or compatibility fallback)"
                        }
                    );
                    self.stereo_status = Some(stereo_drawn);
                    self.stereo_stable_frames = 0;
                    self.stereo_sample_size = size;
                    horizon_client_input::headset_display::clear_gpu_frame();
                }
                for (eye, (image, pose)) in eyes.iter().enumerate().filter(|_| !stereo_drawn) {
                    self.eye_mask = masks
                        .as_ref()
                        .and_then(|m| vis_mask::EyeMask::new(&m[eye], pose.tangents));
                    let view = View::eye(body_yaw, pose, frame.depth_far);
                    self.headset_eye = Some((view, cull));
                    self.offscreen = Some(image.clone());
                    let drawing = Instant::now();
                    drawn = self.render_flat(frame);
                    if vr_profile::enabled() {
                        let [total, submit, stall] = if eye == 0 {
                            [
                                vr_profile::Stage::EyeLeft,
                                vr_profile::Stage::SubmitLeft,
                                vr_profile::Stage::StallLeft,
                            ]
                        } else {
                            [
                                vr_profile::Stage::EyeRight,
                                vr_profile::Stage::SubmitRight,
                                vr_profile::Stage::StallRight,
                            ]
                        };
                        vr_profile::record(total, drawing.elapsed());
                        vr_profile::record(submit, self.last_submit);
                        vr_profile::record(stall, self.last_wait);
                    }
                    if drawn.is_err() {
                        break;
                    }
                }
                self.stereo_stable_frames = self.stereo_stable_frames.saturating_add(1);
                let samples = self.gpu_times.get(timed..).unwrap_or_default();
                // Timer readbacks are delayed. Ignore transition frames and batches
                // whose size cannot describe a complete stereo world render.
                if self.stereo_stable_frames > 16
                    && samples.len() == if stereo_drawn { 1 } else { 2 }
                {
                    horizon_client_input::headset_display::record_gpu_frame(samples.iter().sum());
                }
                vr_profile::render_mode(stereo_drawn);
                if vr_profile::enabled() {
                    // Read back a few frames late: the eye passes timed since this frame began.
                    for &ms in self.gpu_times.get(timed..).unwrap_or_default() {
                        vr_profile::gpu(ms);
                    }
                    vr_profile::passes(self.own_pass_times());
                    vr_profile::scene(self.drawn_scene());
                }
                self.headset_eye = None;
                self.eye_mask = None;
                // Back at the panel's size, which the game lays the HUD out for.
                let size = headset::with(|h| h.menu_size()).unwrap_or([1, 1]);
                self.set_headset_size(size);
                let hud_started = Instant::now();
                let mut panels =
                    self.device
                        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: Some("Quest panels and runtime layouts"),
                        });
                self.encode_headset_hud(frame.hud, hud.as_ref(), &mut panels);
                // The overlay panel (Visor's hotbar wheel in front of the hand), when the game
                // wants one: its own batch onto its own image.
                let overlays: Vec<wgpu::Texture> = (0..horizon_client_input::headset::OVERLAYS)
                    .filter_map(|slot| {
                        let image = headset::with(|h| h.overlay_image(slot)).flatten()?;
                        match frame.overlays[slot] {
                            Some(batch) => {
                                self.encode_headset_hud(batch, Some(&image), &mut panels)
                            }
                            None => self.encode_headset_hud(
                                &HudBatch::new([1, 1]),
                                Some(&image),
                                &mut panels,
                            ),
                        }
                        Some(image)
                    })
                    .collect();
                vr_profile::record(vr_profile::Stage::Hud, hud_started.elapsed());
                let mut images = vec![&eyes[0].0, &eyes[1].0];
                images.extend(hud.as_ref());
                images.extend(overlays.iter());
                images.extend(upload.as_ref().map(|u| &u.0));
                let finish_started = Instant::now();
                self.finish_headset_images_encoded(panels, &images, images.len());
                vr_profile::record(vr_profile::Stage::Finish, finish_started.elapsed());
                drawn
            }
        };
        let ending = Instant::now();
        headset::with(|h| h.end());
        if matches!(layers, headset::Layers::World | headset::Layers::Screen) {
            vr_profile::record(vr_profile::Stage::XrEnd, ending.elapsed());
            vr_profile::frame_done();
        }
        drawn
    }

    /// What the last pass drew, for the frame profile: chunk sections, far terrain tiles,
    /// terrain draw calls, and the indices of the section draws and far tiles (six a quad; the
    /// sections' count is what the draw list asks for after back faces are left out, so
    /// it does not depend on whether the calls are flat).
    pub(super) fn drawn_scene(&self) -> [u32; 4] {
        let sections: u64 = self.draws[..self.section_draws.min(self.draws.len())]
            .iter()
            .map(|d| u64::from(d[0]))
            .sum();
        let tiles: u64 = self
            .visible_lods
            .iter()
            .filter_map(|v| self.lods.get(&v.key))
            .map(|t| (u64::from(t.mesh.solid) + u64::from(t.mesh.translucent)) * 6)
            .sum();
        [
            self.visible.len() as u32,
            self.visible_lods.len() as u32,
            self.section_draw_calls as u32,
            (sections + tiles).min(u64::from(u32::MAX)) as u32,
        ]
    }

    /// Draws at `size` from now on: the headset's eye images and its menu panel
    /// differ.
    pub(super) fn set_headset_size(&mut self, size: [u32; 2]) {
        if [self.config.width, self.config.height] == size {
            return;
        }
        let old = [self.config.width, self.config.height];
        [self.config.width, self.config.height] = size;
        let depth = match self.headset_depth.take() {
            Some((at, depth)) if at == size => depth,
            _ => Self::depth(&self.device, &self.config),
        };
        self.headset_depth = Some((old, std::mem::replace(&mut self.depth, depth)));
    }

    /// The HUD of a world frame on a headset, or the screen open over it, onto
    /// the panel's image (`image`, at `config`'s size) over a transparent background; the
    /// compositor shows it in front of the eyes' images. With no image its
    /// atlas patches still go up.
    pub(super) fn draw_headset_hud(&mut self, hud: &HudBatch, image: Option<&wgpu::Texture>) {
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Horizon headset HUD"),
            });
        self.encode_headset_hud(hud, image, &mut encoder);
        self.uploads.finish(&encoder);
        self.queue.submit([encoder.finish()]);
    }

    pub(super) fn encode_headset_hud(
        &mut self,
        hud: &HudBatch,
        image: Option<&wgpu::Texture>,
        encoder: &mut wgpu::CommandEncoder,
    ) {
        let size = [self.config.width, self.config.height];
        self.hud
            .prepare(&self.device, &mut self.uploads, encoder, hud, size);
        if let Some(image) = image {
            let view = image.create_view(&Default::default());
            drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Horizon headset HUD clear"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            }));
            self.hud.draw_range(
                encoder,
                &view,
                None,
                &self.depth,
                hud,
                size,
                0..hud.vertices.len() as u32,
                None,
            );
        }
    }

    /// Leaves the headset's images as its runtime takes them back (in the
    /// colour attachment layout) and submits the frame before they go; those
    /// from `drawn` on are cleared to black.
    pub(super) fn finish_headset_images(&mut self, images: &[&wgpu::Texture], drawn: usize) {
        let encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Horizon headset images"),
            });
        self.finish_headset_images_encoded(encoder, images, drawn);
    }

    pub(super) fn finish_headset_images_encoded(
        &mut self,
        mut encoder: wgpu::CommandEncoder,
        images: &[&wgpu::Texture],
        drawn: usize,
    ) {
        // A tiling GPU (Quest's Adreno) reads and writes the whole image again for a pass that
        // loads and stores it and draws nothing, some 30 MB a frame an eye. With
        // `HORIZON_XR_LAYOUT=barrier` the images drawn already hold their colour work and only
        // change to the layout the runtime takes, through a barrier.
        let barrier = layout_by_barrier();
        let mut moved = Vec::new();
        for (i, image) in images.iter().enumerate() {
            if barrier && i < drawn {
                moved.push(wgpu::TextureTransition {
                    texture: *image,
                    selector: None,
                    state: wgpu::TextureUses::COLOR_TARGET,
                });
                continue;
            }
            let view = image.create_view(&Default::default());
            drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Horizon headset image layout"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: if i < drawn {
                            wgpu::LoadOp::Load
                        } else {
                            wgpu::LoadOp::Clear(wgpu::Color::BLACK)
                        },
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            }));
        }
        if !moved.is_empty() {
            encoder.transition_resources(std::iter::empty(), moved.into_iter());
        }
        self.uploads.finish(&encoder);
        self.queue.submit([encoder.finish()]);
    }
}
