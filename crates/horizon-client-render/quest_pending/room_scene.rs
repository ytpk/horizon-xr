//! The headset launcher's room, drawn into each eye (`room_layout.rs` has what stands in it):
//! one render pass per eye with a sky and hills behind the window, a few hundred quads lit per
//! pixel by two lanterns, a flickering fire, the window and the TV, and the window's glass. A
//! static mesh in one vertex buffer, a 16 by 16 texture array (the room's own textures until the
//! game's are prepared, then the real block textures), four samples a pixel on a tiling GPU, where
//! that costs nothing but the resolve. No uniform but the eye's.
use super::headset::{EyePose, eye_projection};
use super::menu_scene::{Sky, mip_chain};
use super::room_layout::{self as layout, LAYERS, Vertex};
use glam::{Mat3, Mat4, Vec3};
use std::time::Instant;

const SAMPLES: u32 = 4;
const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// The far plane: the room is 8 m across.
const FAR: f32 = 40.0;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniform {
    view_proj: [[f32; 4]; 4],
    rot: [[f32; 4]; 3],
    eye: [f32; 4],
    tangents: [f32; 4],
    sun: [f32; 4],
    moon: [f32; 4],
    lights: [[f32; 4]; 5],
    colors: [[f32; 4]; 5],
    params: [f32; 4],
}

/// The fire's strength at `t` seconds: the sum of a few sines of unrelated rates, so that it
/// never repeats in a way an eye can see (0.6 to 1.2).
pub fn flicker(t: f32) -> f32 {
    let steps = (t * 14.0).floor();
    let jitter = ((steps * 12.9898).sin() * 43758.547).fract().abs();
    (0.86
        + 0.14 * (t * 13.0).sin()
        + 0.08 * (t * 29.0 + 1.3).sin()
        + 0.06 * (t * 7.3 + 4.0).sin()
        + 0.1 * (jitter - 0.5))
        .clamp(0.6, 1.2)
}

/// The lights of the room at `t` seconds under `sky`: position and reach, colour and strength.
pub fn lights(t: f32, sky: &Sky) -> ([[f32; 4]; 5], [[f32; 4]; 5]) {
    let lantern = [1.0 * 1.7, 0.72 * 1.7, 0.38 * 1.7, 0.0];
    let fire = flicker(t);
    let daylight = sky.day * 0.9 + (1.0 - sky.day) * 0.12 * (0.4 + sky.moon_lit);
    let at = |p: Vec3, reach: f32| [p.x, p.y, p.z, reach];
    (
        [
            at(layout::LANTERNS[0], 7.0),
            at(layout::LANTERNS[1], 7.0),
            at(layout::FIRE + Vec3::new(-0.2, 0.2, 0.0), 5.5),
            at(layout::WINDOW + Vec3::new(0.3, 0.0, 0.0), 5.0),
            at(layout::TV_LIGHT, 4.5),
        ],
        [
            lantern,
            lantern,
            [1.0 * 1.9 * fire, 0.45 * 1.9 * fire, 0.14 * 1.9 * fire, 0.0],
            [0.72 * daylight, 0.84 * daylight, 1.0 * daylight, 0.0],
            [0.5 * 0.35, 0.6 * 0.35, 1.0 * 0.35, 0.0],
        ],
    )
}

struct Targets {
    size: [u32; 2],
    format: wgpu::TextureFormat,
    colour: wgpu::TextureView,
    depth: wgpu::TextureView,
}

struct Pipelines {
    format: wgpu::TextureFormat,
    sky: wgpu::RenderPipeline,
    room: wgpu::RenderPipeline,
    glass: wgpu::RenderPipeline,
    /// The pointer's ray and the controllers: flat colours, over everything.
    pointer: wgpu::RenderPipeline,
}

/// How many vertices of pointer geometry a frame may carry: a ray and two controllers, a few
/// dozen triangles.
pub const POINTER_CAPACITY: usize = 256;

pub(super) struct RoomScene {
    shader: wgpu::ShaderModule,
    layout: wgpu::PipelineLayout,
    pipelines: Option<Pipelines>,
    targets: Option<Targets>,
    uniform: wgpu::Buffer,
    texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    opaque: (wgpu::Buffer, wgpu::Buffer, u32),
    glass: (wgpu::Buffer, wgpu::Buffer, u32),
    /// The pointer's triangles of the frame (`POINTER_CAPACITY` vertices).
    pointer: wgpu::Buffer,
    started: Instant,
    /// The real block textures are in the array.
    real: bool,
    /// `HORIZON_ROOM_STRESS=8` draws the room that many times an eye, to see how much frame
    /// the room leaves free on a headset (the frame rate holds while its cost is small).
    stress: u32,
}

fn upload_layer(queue: &wgpu::Queue, texture: &wgpu::Texture, layer: u32, texels: &[u8]) {
    for (level, (w, h, data)) in mip_chain(16, 16, texels).iter().enumerate() {
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: level as u32,
                origin: wgpu::Origin3d {
                    x: 0,
                    y: 0,
                    z: layer,
                },
                aspect: wgpu::TextureAspect::All,
            },
            data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * w),
                rows_per_image: None,
            },
            wgpu::Extent3d {
                width: *w,
                height: *h,
                depth_or_array_layers: 1,
            },
        );
    }
}

fn buffer(
    device: &wgpu::Device,
    label: &str,
    usage: wgpu::BufferUsages,
    data: &[u8],
) -> wgpu::Buffer {
    use wgpu::util::DeviceExt;
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(label),
        contents: data,
        usage,
    })
}

/// `HORIZON_ROOM_LOOK=yaw,pitch` (degrees) turns the head and `HORIZON_ROOM_POS=x,y,z` moves it,
/// to look at the room from a headset that lies on a table or from the desktop simulator.
pub(super) fn dev_view(eye: &EyePose) -> (Vec3, glam::Quat) {
    let numbers = |name: &str| -> Option<Vec<f32>> {
        std::env::var(name)
            .ok()?
            .split(',')
            .map(|v| v.trim().parse::<f32>().ok())
            .collect()
    };
    let rotation = match numbers("HORIZON_ROOM_LOOK").as_deref() {
        Some([yaw, pitch]) => {
            glam::Quat::from_rotation_y(yaw.to_radians())
                * glam::Quat::from_rotation_x(pitch.to_radians())
        }
        _ => eye.orientation,
    };
    let position = match numbers("HORIZON_ROOM_POS").as_deref() {
        Some([x, y, z]) => Vec3::new(*x + eye.room.x, *y, *z),
        _ => eye.room,
    };
    (position, rotation)
}

impl RoomScene {
    pub(super) fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let bindings = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Room bindings"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Room shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("room_scene.wgsl").into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Room layout"),
            bind_group_layouts: &[Some(&bindings)],
            ..Default::default()
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Room uniform"),
            size: std::mem::size_of::<Uniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Room textures"),
            size: wgpu::Extent3d {
                width: 16,
                height: 16,
                depth_or_array_layers: LAYERS as u32,
            },
            mip_level_count: 5,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        for (layer, texels) in layout::procedural_layers().iter().enumerate() {
            upload_layer(queue, &texture, layer as u32, texels);
        }
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Room sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Room resources"),
            layout: &bindings,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        let mesh = layout::mesh();
        let upload = |name: &str, (v, i): &(Vec<Vertex>, Vec<u32>)| {
            (
                buffer(
                    device,
                    name,
                    wgpu::BufferUsages::VERTEX,
                    bytemuck::cast_slice(v),
                ),
                buffer(
                    device,
                    name,
                    wgpu::BufferUsages::INDEX,
                    bytemuck::cast_slice(i),
                ),
                i.len() as u32,
            )
        };
        Self {
            opaque: upload("Room mesh", &mesh.opaque),
            glass: upload("Room glass", &mesh.glass),
            pointer: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Room pointer"),
                size: (POINTER_CAPACITY * std::mem::size_of::<Vertex>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            shader,
            layout,
            pipelines: None,
            targets: None,
            uniform,
            texture,
            bind_group,
            started: Instant::now(),
            real: false,
            stress: std::env::var("HORIZON_ROOM_STRESS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(1)
                .clamp(1, 64),
        }
    }

    /// Puts the real block textures in the layers (`room_layout::load_real`).
    pub(super) fn set_textures(&mut self, queue: &wgpu::Queue, layers: &[(usize, Vec<u8>)]) {
        for (layer, texels) in layers {
            if *layer < LAYERS && texels.len() == 16 * 16 * 4 {
                upload_layer(queue, &self.texture, *layer as u32, texels);
            }
        }
        self.real = !layers.is_empty();
    }

    pub(super) fn real(&self) -> bool {
        self.real
    }

    fn make_pipelines(&self, device: &wgpu::Device, format: wgpu::TextureFormat) -> Pipelines {
        let multisample = wgpu::MultisampleState {
            count: SAMPLES,
            ..Default::default()
        };
        let attributes = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Uint32];
        let buffers = [Some(wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &attributes,
        })];
        // Reversed depth, as the eye's projection makes it: near is 1, far is 0.
        let depth = |write: bool, compare: wgpu::CompareFunction| {
            Some(wgpu::DepthStencilState {
                format: DEPTH,
                depth_write_enabled: Some(write),
                depth_compare: Some(compare),
                stencil: Default::default(),
                bias: Default::default(),
            })
        };
        let make = |label: &str,
                    vs: &str,
                    fs: &str,
                    buffers: &[Option<wgpu::VertexBufferLayout>],
                    depth: Option<wgpu::DepthStencilState>,
                    blend: Option<wgpu::BlendState>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&self.layout),
                vertex: wgpu::VertexState {
                    module: &self.shader,
                    entry_point: Some(vs),
                    compilation_options: Default::default(),
                    buffers,
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: depth,
                multisample,
                fragment: Some(wgpu::FragmentState {
                    module: &self.shader,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        Pipelines {
            format,
            sky: make(
                "Room sky",
                "vs_sky",
                "fs_sky",
                &[],
                depth(false, wgpu::CompareFunction::Always),
                None,
            ),
            room: make(
                "Room",
                "vs_room",
                "fs_room",
                &buffers,
                depth(true, wgpu::CompareFunction::Greater),
                None,
            ),
            glass: make(
                "Room glass",
                "vs_room",
                "fs_glass",
                &buffers,
                depth(false, wgpu::CompareFunction::Greater),
                Some(wgpu::BlendState::ALPHA_BLENDING),
            ),
            pointer: make(
                "Room pointer",
                "vs_room",
                "fs_flat",
                &buffers,
                depth(false, wgpu::CompareFunction::Always),
                None,
            ),
        }
    }

    fn make_targets(device: &wgpu::Device, size: [u32; 2], format: wgpu::TextureFormat) -> Targets {
        let extent = wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        };
        let make = |label: &str, format: wgpu::TextureFormat| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: extent,
                    mip_level_count: 1,
                    sample_count: SAMPLES,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        Targets {
            size,
            format,
            colour: make("Room eye colour", format),
            depth: make("Room eye depth", DEPTH),
        }
    }

    /// The room into a headset's eye image `target`, seen from `eye`.
    pub(super) fn draw_eye(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &wgpu::Texture,
        eye: &EyePose,
        sky: &Sky,
        pointer: &[Vertex],
    ) {
        let format = target.format();
        let size = [target.width(), target.height()];
        if self.pipelines.as_ref().is_none_or(|p| p.format != format) {
            self.pipelines = Some(self.make_pipelines(device, format));
        }
        if self
            .targets
            .as_ref()
            .is_none_or(|t| t.size != size || t.format != format)
        {
            self.targets = Some(Self::make_targets(device, size, format));
        }
        let (Some(pipelines), Some(targets)) = (&self.pipelines, &self.targets) else {
            return;
        };
        let (position, rotation) = dev_view(eye);
        let view = Mat4::from_quat(rotation.conjugate()) * Mat4::from_translation(-position);
        let view_proj = eye_projection(eye.tangents, 0.05, FAR) * view;
        let m = Mat3::from_quat(rotation);
        let col = |v: Vec3| [v.x, v.y, v.z, 0.0];
        let t = self.started.elapsed().as_secs_f32();
        let (lights, colors) = lights(t, sky);
        let uniform = Uniform {
            view_proj: view_proj.to_cols_array_2d(),
            rot: [col(m.x_axis), col(m.y_axis), col(m.z_axis)],
            eye: [position.x, position.y, position.z, t],
            tangents: eye.tangents,
            sun: [sky.sun.x, sky.sun.y, sky.sun.z, sky.day],
            moon: [sky.moon.x, sky.moon.y, sky.moon.z, sky.moon_lit],
            lights,
            colors,
            params: [flicker(t), sky.twilight, 0.0, 0.0],
        };
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&uniform));
        let pointer = &pointer[..pointer.len().min(POINTER_CAPACITY)];
        if !pointer.is_empty() {
            queue.write_buffer(&self.pointer, 0, bytemuck::cast_slice(pointer));
        }
        let resolve = target.create_view(&Default::default());
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Room for an eye"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Room for an eye"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &targets.colour,
                    resolve_target: Some(&resolve),
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        // Only the resolved image is wanted: the samples stay in the tile.
                        store: wgpu::StoreOp::Discard,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &targets.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(0.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_bind_group(0, &self.bind_group, &[]);
            for _ in 0..self.stress {
                pass.set_pipeline(&pipelines.sky);
                pass.draw(0..3, 0..1);
                pass.set_pipeline(&pipelines.room);
                pass.set_vertex_buffer(0, self.opaque.0.slice(..));
                pass.set_index_buffer(self.opaque.1.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..self.opaque.2, 0, 0..1);
                pass.set_pipeline(&pipelines.glass);
                pass.set_vertex_buffer(0, self.glass.0.slice(..));
                pass.set_index_buffer(self.glass.1.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..self.glass.2, 0, 0..1);
            }
            // Over everything, as Visor draws the cursor and the menu hands.
            if !pointer.is_empty() {
                pass.set_pipeline(&pipelines.pointer);
                pass.set_vertex_buffer(0, self.pointer.slice(..));
                pass.draw(0..pointer.len() as u32, 0..1);
            }
        }
        queue.submit([encoder.finish()]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gfx::menu_scene::sky_at;
    use glam::Quat;

    /// A device on whatever adapter the machine has; none in a CI without a GPU.
    fn gpu() -> Option<(wgpu::Device, wgpu::Queue)> {
        let instance = crate::gfx::test_gpu::instance();
        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .ok()?;
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).ok()
    }

    const SIZE: u32 = 320;

    /// The room seen from `position` looking `yaw`, `pitch` degrees (yaw 0 faces the TV, 90 the
    /// window, 180 the sofa, -90 the fireplace) at `hour`, as RGBA rows of a square eye image.
    fn picture(
        gpu: &(wgpu::Device, wgpu::Queue),
        scene: &mut RoomScene,
        position: Vec3,
        look: (f32, f32),
        hour: f32,
    ) -> Vec<u8> {
        let (device, queue) = gpu;
        let image = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("room test eye"),
            size: wgpu::Extent3d {
                width: SIZE,
                height: SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let eye = EyePose {
            orientation: Quat::from_rotation_y(look.0.to_radians())
                * Quat::from_rotation_x(look.1.to_radians()),
            offset: Vec3::ZERO,
            room: position,
            tangents: [-1.0, 1.0, 1.0, -1.0],
        };
        scene.draw_eye(device, queue, &image, &eye, &sky_at(hour, 0.5), &[]);
        let row = (SIZE * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: u64::from(row * SIZE),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            image.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: None,
                },
            },
            image.size(),
        );
        queue.submit([encoder.finish()]);
        buffer.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let data = buffer.slice(..).get_mapped_range().unwrap();
        let mut out = Vec::new();
        for y in 0..SIZE as usize {
            out.extend_from_slice(&data[y * row as usize..][..SIZE as usize * 4]);
        }
        out
    }

    fn dump(name: &str, pixels: &[u8]) {
        let Some(dir) = std::env::var_os("HORIZON_ROOM_DUMP") else {
            return;
        };
        let path = std::path::Path::new(&dir).join(format!("{name}.png"));
        let mut enc = png::Encoder::new(std::fs::File::create(path).unwrap(), SIZE, SIZE);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header()
            .unwrap()
            .write_image_data(pixels)
            .unwrap();
    }

    fn px(pixels: &[u8], x: u32, y: u32) -> [u8; 3] {
        let at = ((y * SIZE + x) * 4) as usize;
        [pixels[at], pixels[at + 1], pixels[at + 2]]
    }

    fn count(pixels: &[u8], test: impl Fn([u8; 3]) -> bool) -> usize {
        pixels
            .chunks(4)
            .filter(|p| test([p[0], p[1], p[2]]))
            .count()
    }

    #[test]
    fn the_room_is_drawn_from_every_side_before_and_after_the_real_textures() {
        let Some(gpu) = gpu() else { return };
        let mut scene = RoomScene::new(&gpu.0, &gpu.1);
        let at = Vec3::new(0.0, 1.6, 0.0);
        let tv = picture(&gpu, &mut scene, at, (0.0, 0.0), 12.0);
        let sofa = picture(&gpu, &mut scene, at, (180.0, -5.0), 12.0);
        let window = picture(&gpu, &mut scene, at, (90.0, 5.0), 12.0);
        let fire = picture(&gpu, &mut scene, at, (-90.0, -10.0), 12.0);
        let night = picture(&gpu, &mut scene, at, (90.0, 5.0), 0.0);
        for (name, p) in [
            ("tv", &tv),
            ("sofa", &sofa),
            ("window", &window),
            ("fire", &fire),
            ("window_night", &night),
        ] {
            dump(&format!("room_{name}"), p);
        }
        // Every pixel is drawn (the sky and the room cover the eye), none transparent.
        assert!(tv.chunks(4).all(|p| p[3] == 255));
        // The TV wall: the black frame in the middle, the spruce walls beside it.
        let centre = px(&tv, SIZE / 2, SIZE / 2);
        assert!(
            centre.iter().all(|&c| c < 40),
            "the TV's screen is dark: {centre:?}"
        );
        let wall = px(&tv, SIZE / 16, SIZE / 3);
        assert!(wall[0] > wall[2] + 15, "a warm brown wall: {wall:?}");
        // The sofa's red cushions and the red rug show from behind the player's seat.
        let reds = count(&sofa, |[r, g, b]| {
            r > 90 && r as u32 > 2 * g as u32 && r as u32 > 2 * b as u32
        });
        assert!(reds > 1500, "red cushions: {reds}");
        // The fire is bright orange where it burns.
        let flames = count(&fire, |[r, g, b]| r > 200 && g > 90 && g < 200 && b < 90);
        assert!(flames > 20, "fire pixels: {flames}");
        // The window shows a blue sky by day and a dark one at night.
        let sky = |p: &[u8]| count(p, |[r, g, b]| b > 180 && b > r + 20 && g > 120);
        assert!(sky(&window) > 600, "blue sky: {}", sky(&window));
        assert!(sky(&night) < sky(&window) / 8, "{} at night", sky(&night));
        // The bricks, the bookshelf and the beams are all there to see.
        assert!(
            count(&fire, |[r, g, b]| r > 90
                && r as u32 > g as u32 * 2
                && b < 70)
                > 500,
            "bricks"
        );

        // The real textures turn the room: the oak floor becomes the pixels given for it.
        let before = picture(&gpu, &mut scene, at, (0.0, -60.0), 12.0);
        let floor = px(&before, 200, 90);
        assert!(!scene.real());
        scene.set_textures(
            &gpu.1,
            &[(
                layout::Mat::OakPlanks as usize,
                [255, 0, 255, 255].repeat(256),
            )],
        );
        assert!(scene.real());
        let after = picture(&gpu, &mut scene, at, (0.0, -60.0), 12.0);
        dump("room_floor_before", &before);
        dump("room_floor_after", &after);
        let turned = px(&after, 200, 90);
        assert!(
            turned[0] > 100 && turned[1] < 60 && turned[2] > 100,
            "magenta oak: {turned:?}"
        );
        assert_ne!(floor, turned);
        // The rug (red wool) in the middle was not touched.
        assert_eq!(
            px(&before, SIZE / 2, SIZE / 2),
            px(&after, SIZE / 2, SIZE / 2)
        );
    }

    #[test]
    fn the_scene_is_a_small_static_mesh_and_two_passes_of_it_run_well_inside_a_frame() {
        let Some(gpu) = gpu() else { return };
        let mut scene = RoomScene::new(&gpu.0, &gpu.1);
        let at = Vec3::new(0.0, 1.6, 0.0);
        picture(&gpu, &mut scene, at, (0.0, 0.0), 12.0);
        let started = Instant::now();
        for _ in 0..20 {
            picture(&gpu, &mut scene, at, (0.0, 0.0), 12.0);
        }
        // Includes the readback and a software rasteriser on a CI box: a generous bound that a
        // pass with a runaway cost would still break.
        assert!(
            started.elapsed().as_millis() / 20 < 100,
            "{:?}",
            started.elapsed() / 20
        );
    }

    #[test]
    fn the_fire_flickers_within_its_range_and_never_stands_still() {
        let samples: Vec<f32> = (0..400).map(|i| flicker(i as f32 * 0.037)).collect();
        assert!(samples.iter().all(|f| (0.6..=1.2).contains(f)));
        let (lo, hi) = samples
            .iter()
            .fold((f32::MAX, f32::MIN), |(lo, hi), f| (lo.min(*f), hi.max(*f)));
        assert!(hi - lo > 0.3, "{lo} {hi}");
    }

    #[test]
    fn the_lanterns_and_the_fire_are_warm_the_window_is_cool_and_it_dims_at_night() {
        let noon = sky_at(12.0, 0.5);
        let (_, colours) = lights(1.0, &noon);
        for c in &colours[..3] {
            assert!(c[0] > c[2] * 2.0, "{c:?} is warm");
        }
        assert!(colours[3][2] > colours[3][0], "daylight is bluish");
        let night = sky_at(0.0, 0.0);
        let (_, dark) = lights(1.0, &night);
        assert!(dark[3][2] < colours[3][2] * 0.2);
        // The lanterns do not care what hour it is.
        assert_eq!(dark[0], colours[0]);
    }
}
