//! Low-resolution OpenXR motion/depth output; opt-in half-rate modes only.
use super::*;
use frame_gen::motion::{MotionTracker, MotionVertex};
use headset::SpaceWarpTargets;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Params {
    camera_reproject: [[f32; 4]; 4],
    current: [[f32; 4]; 4],
    previous: [[f32; 4]; 4],
    target_size: [f32; 4],
}

pub(super) fn requested() -> bool {
    headset::installed()
        && horizon_client_input::headset_display::snapshot().is_some_and(|i| i.spacewarp_supported)
        && horizon_vivecraft::refresh::requested(
            horizon_client_input::headset::settings().float("horizonRefreshRate"),
        )
        .spacewarp
}

pub(super) struct SpaceWarp {
    mono: Pipelines,
    stereo: Option<Pipelines>,
    stereo_uniform: wgpu::Buffer,
    uniforms: [wgpu::Buffer; 2],
    vertices: wgpu::Buffer,
    atlas: wgpu::TextureView,
    sampler: wgpu::Sampler,
    entities: MotionTracker,
    hands: MotionTracker,
    gui: MotionTracker,
    previous: Option<([Mat4; 2], DVec3, [u32; 2])>,
    current: [Mat4; 2],
    reproject: [Mat4; 2],
    targets: Option<SpaceWarpTargets>,
    pub eye: usize,
    pub drawn: u8,
    pub history_valid: bool,
    count: u32,
    staged: Vec<MotionVertex>,
}

impl SpaceWarp {
    pub fn new(device: &wgpu::Device, atlas: &wgpu::TextureView) -> Self {
        let mono = Pipelines::new(device, false);
        let stereo = (device.features().contains(wgpu::Features::MULTIVIEW)
            && device.limits().max_multiview_view_count >= 2)
            .then(|| Pipelines::new(device, true));
        Self {
            mono,
            stereo,
            stereo_uniform: uniform_buffer(device),
            uniforms: std::array::from_fn(|_| uniform_buffer(device)),
            vertices: vertex_buffer(device, 4096),
            atlas: atlas.clone(),
            sampler: device.create_sampler(&Default::default()),
            entities: MotionTracker::default(),
            hands: MotionTracker::default(),
            gui: MotionTracker::default(),
            previous: None,
            current: [Mat4::IDENTITY; 2],
            reproject: [Mat4::IDENTITY; 2],
            targets: None,
            eye: 0,
            drawn: 0,
            history_valid: false,
            count: 0,
            staged: Vec::new(),
        }
    }
    pub fn active(&self) -> bool {
        self.targets.is_some()
    }
    pub fn stop(&mut self) {
        self.targets = None;
        self.previous = None;
        self.entities.reset();
        self.hands.reset();
        self.gui.reset();
    }
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        targets: SpaceWarpTargets,
        views: [View; 2],
        frame: &Frame,
        size: [u32; 2],
    ) {
        self.current = views.map(|v| v.view_proj(Mat4::IDENTITY));
        self.history_valid = self.previous.is_some_and(|(_, camera, old_size)| {
            old_size == size && camera.distance(frame.camera) < 1.0
        });
        self.reproject =
            if let Some((previous, camera, _)) = self.previous.filter(|_| self.history_valid) {
                previous.map(|p| {
                    (p.as_dmat4() * glam::DMat4::from_translation(frame.camera - camera)).as_mat4()
                })
            } else {
                self.current
            };
        if !self.history_valid {
            self.entities.reset();
            self.hands.reset();
            self.gui.reset();
        }
        self.entities
            .track(frame.entities, frame.entity_motion, frame.camera);
        let gui_start = frame
            .hand
            .len()
            .saturating_sub(frame.hand_gui_quads as usize * 4);
        self.hands
            .track(&frame.hand[..gui_start], &[(0, 1)], frame.camera);
        self.gui
            .track(&frame.hand[gui_start..], &[(0, 1)], frame.camera);
        self.staged.clear();
        self.staged.extend_from_slice(self.entities.staged());
        self.staged.extend_from_slice(self.hands.staged());
        if self.gui.staged().is_empty() {
            // New/topology-changing overlay geometry still needs its own depth now.
            for quad in frame.hand[gui_start..].as_chunks::<4>().0 {
                self.staged.extend([0, 1, 2, 0, 2, 3].map(|i| MotionVertex {
                    position: quad[i].pos,
                    previous: quad[i].pos,
                    uv: quad[i].uv,
                    translucent: 2,
                }));
            }
        } else {
            self.staged
                .extend(self.gui.staged().iter().map(|v| MotionVertex {
                    translucent: v.translucent | 2,
                    ..*v
                }));
        }
        let bytes = bytemuck::cast_slice(&self.staged);
        if bytes.len() as u64 > self.vertices.size() {
            self.vertices = vertex_buffer(device, (bytes.len() as u64).next_power_of_two());
        }
        if !bytes.is_empty() {
            queue.write_buffer(&self.vertices, 0, bytes);
        }
        self.count = self.staged.len() as u32;
        self.previous = Some((self.current, frame.camera, size));
        self.targets = Some(targets);
        self.drawn = 0;
        self.eye = 0;
    }
    fn params(&self, eye: usize, targets: &SpaceWarpTargets) -> Params {
        Params {
            camera_reproject: (self.reproject[eye] * self.current[eye].inverse())
                .to_cols_array_2d(),
            current: self.current[eye].to_cols_array_2d(),
            previous: self.reproject[eye].to_cols_array_2d(),
            target_size: [
                targets.motion.width() as f32,
                targets.motion.height() as f32,
                0.,
                0.,
            ],
        }
    }
    pub fn draw(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        source: &wgpu::TextureView,
        stereo: bool,
        timing: Option<&mut pack_timing::PassTiming>,
    ) {
        let Some(targets) = &self.targets else {
            return;
        };
        let (pipelines, uniform, params) = if stereo {
            let pipelines = self
                .stereo
                .as_ref()
                .expect("world multiview was checked before acquisition");
            (
                pipelines,
                &self.stereo_uniform,
                [self.params(0, targets), self.params(1, targets)],
            )
        } else {
            (
                &self.mono,
                &self.uniforms[self.eye],
                [self.params(self.eye, targets); 2],
            )
        };
        queue.write_buffer(uniform, 0, bytemuck::cast_slice(&params));
        // One array attachment and one draw per geometry set for BOTH eyes.
        // The mono fallback selects one output layer and samples its single-layer source.
        let view = |texture: &wgpu::Texture, layer| {
            texture.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2Array),
                base_array_layer: layer,
                array_layer_count: Some(if stereo { 2 } else { 1 }),
                ..Default::default()
            })
        };
        let depth = view(source.texture(), 0);
        let layer = if stereo { 0 } else { self.eye as u32 };
        let motion = view(&targets.motion, layer);
        let output_depth = view(&targets.depth, layer);
        let entries = [
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&depth),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(&self.atlas),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::Sampler(&self.sampler),
            },
        ];
        let camera = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("SpaceWarp camera"),
            layout: &pipelines.background.get_bind_group_layout(0),
            entries: &entries[..2],
        });
        let objects = (self.count > 0).then(|| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("SpaceWarp objects"),
                layout: &pipelines.objects.get_bind_group_layout(0),
                entries: &entries,
            })
        });
        // Background reconstruction and animated overrides share the same tiled pass;
        // no intermediate motion/depth store and reload on Adreno.
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            timestamp_writes: timing.and_then(|t| t.writes("SpaceWarp")),
            label: Some(if stereo {
                "SpaceWarp single-pass multiview"
            } else {
                "SpaceWarp eye"
            }),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &motion,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &output_depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(0.),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            multiview_mask: if stereo { multiview::MASK } else { None },
            ..Default::default()
        });
        pass.set_pipeline(&pipelines.background);
        pass.set_bind_group(0, &camera, &[]);
        pass.draw(0..3, 0..1);
        if let Some(objects) = objects {
            pass.set_pipeline(&pipelines.objects);
            pass.set_bind_group(0, &objects, &[]);
            pass.set_vertex_buffer(0, self.vertices.slice(..));
            pass.draw(0..self.count, 0..1);
        }
        drop(pass);
        self.drawn |= if stereo { 3 } else { 1 << self.eye };
    }
}

pub(super) fn shader_source(stereo: bool) -> &'static str {
    let source = include_str!("spacewarp.wgsl");
    if stereo {
        source
    } else {
        source.split("// Multiview entry points").next().unwrap()
    }
}

struct Pipelines {
    background: wgpu::RenderPipeline,
    objects: wgpu::RenderPipeline,
}
impl Pipelines {
    fn new(device: &wgpu::Device, stereo: bool) -> Self {
        let source = shader_source(stereo);
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Application SpaceWarp"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let attributes = wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x3,2=>Float32x2,3=>Uint32];
        let buffers = [Some(wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<MotionVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &attributes,
        })];
        let pipeline = |objects| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Application SpaceWarp"),
                layout: None,
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(match (objects, stereo) {
                        (false, false) => "vs_full",
                        (false, true) => "vs_full_stereo",
                        (true, false) => "vs_object",
                        (true, true) => "vs_object_stereo",
                    }),
                    compilation_options: Default::default(),
                    buffers: if objects { &buffers } else { &[] },
                },
                primitive: Default::default(),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::Always),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(match (objects, stereo) {
                        (false, false) => "fs_full",
                        (false, true) => "fs_full_stereo",
                        (true, false) => "fs_object",
                        (true, true) => "fs_object_stereo",
                    }),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::TextureFormat::Rgba16Float.into())],
                }),
                multiview_mask: if stereo { multiview::MASK } else { None },
                cache: pipeline_cache::get(device),
            })
        };
        Self {
            background: pipeline(false),
            objects: pipeline(true),
        }
    }
}
fn uniform_buffer(device: &wgpu::Device) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("SpaceWarp eye parameters"),
        size: (std::mem::size_of::<Params>() * 2) as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn vertex_buffer(device: &wgpu::Device, size: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("SpaceWarp object motion"),
        size,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

impl Renderer {
    /// GUI hands and pointers stay on top without clearing the world's retained depth.
    /// The auxiliary pass supplies their own motion and depth over the world afterwards.
    pub(super) fn draw_spacewarp_gui_hand(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        quads: u32,
        first: u32,
    ) {
        pass.set_pipeline(&self.see_through_pipeline);
        pass.set_bind_group(0, &self.hand_group, &[]);
        pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
        pass.set_vertex_buffer(0, self.entity_buffer.slice(..));
        pass.draw_indexed(0..quads * 6, (first * 4) as i32, 0..1);
    }
    pub(super) fn spacewarp_compatible(&self, frame: &Frame) -> bool {
        if !self.spacewarp_requested {
            return false;
        }
        let reason = if frame.title {
            Some("title")
        } else if frame.skin.is_some() {
            Some("GPU-posed entities; waiting for CPU pose fallback")
        } else if self.native.is_some() || self.pack_post.is_some() {
            Some("shader pack")
        } else if self.hdr.is_some() {
            Some("HDR")
        } else if self.visbuf.is_some() || self.improved_transparency || self.core_lines.is_some() {
            Some("custom world rendering")
        } else if self.world_post.scale != 1. || self.world_post.taa || self.world_post.antialias {
            Some("world post processing")
        } else if frame.post_effects.has_chain_files() || frame.menu_blur.is_some() {
            Some("screen post effects")
        } else if !frame.world_primitives.is_empty() || native_render::world_drawn_hooked() {
            Some("native world hook or primitives")
        } else {
            None
        };
        static LAST: std::sync::Mutex<Option<Option<&'static str>>> = std::sync::Mutex::new(None);
        if let Ok(mut last) = LAST.lock()
            && *last != Some(reason)
        {
            *last = Some(reason);
            log::info!(
                "Application SpaceWarp compatibility: {}",
                reason.unwrap_or("compatible world rendering")
            );
        }
        reason.is_none()
    }
}

#[cfg(test)]
#[path = "spacewarp_tests.rs"]
mod tests;
