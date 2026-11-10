//! Experimental two-layer world rendering. The runtime's separate swapchains
//! are kept: after one stereo world pass each layer is copied to its eye image.
use super::view::View;
use glam::Mat4;

pub const MASK: Option<std::num::NonZeroU32> = std::num::NonZeroU32::new(3);

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Eye {
    pub view_proj: [[f32; 4]; 4],
    pub inverse_view: [[f32; 4]; 4],
    pub inverse_projection: [[f32; 4]; 4],
    pub misc: [f32; 4],
    pub from_primary: [[f32; 4]; 4],
}
impl Eye {
    pub fn new(view: View, misc: [f32; 4], primary: View) -> Self {
        Self {
            view_proj: view.view_proj(Mat4::IDENTITY).to_cols_array_2d(),
            inverse_view: view.view.inverse().to_cols_array_2d(),
            inverse_projection: view.projection.inverse().to_cols_array_2d(),
            misc: [misc[0], misc[1], view.aspect, view.tan_half_fov],
            from_primary: (view.view_proj(Mat4::IDENTITY)
                * primary.view_proj(Mat4::IDENTITY).inverse())
            .to_cols_array_2d(),
        }
    }
}

/// Adapt only the built-in Globals ABI. Private state is set at each stage's
/// entry, including fragment entries (sky rays and screen-space effects).
/// Helpers therefore use the same eye without changing every function ABI.
pub fn shader(source: &str) -> String {
    let globals = source.contains("struct Globals {");
    let stars = source.contains("struct Stars {");
    if !globals && !stars {
        return source.to_owned();
    }
    let mut source = source.to_owned();
    if globals {
        let start = source.find("struct Globals {").unwrap();
        let end = start + source[start..].find('}').unwrap();
        source.insert_str(end, "horizon_eyes: array<HorizonEye, 2>,\n");
        source.insert_str(0, "struct HorizonEye { view_proj: mat4x4<f32>, inverse_view: mat4x4<f32>, inverse_projection: mat4x4<f32>, misc: vec4<f32>, from_primary: mat4x4<f32>, }\n");
        for field in ["view_proj", "inverse_view", "misc"] {
            source = source.replace(
                &format!("globals.{field}"),
                &format!("globals.horizon_eyes[horizon_eye].{field}"),
            );
        }
        // The ordinary sky's symmetric reconstruction cannot represent two
        // asymmetric headset lenses. Reconstruct through each actual projection.
        if let Some(start) = source.find("fn view_ray(") {
            let body = start + source[start..].find('{').unwrap();
            let end = body + source[body..].find('}').unwrap();
            source.replace_range(body + 1..end, "\nlet e = globals.horizon_eyes[horizon_eye];\nlet p = e.inverse_projection * vec4(ndc, 1.0, 1.0);\nreturn normalize((e.inverse_view * vec4(p.xyz / p.w, 0.0)).xyz);\n");
        }
        source = source.replace(
            "SunriseOut(clip, weight)",
            "SunriseOut(globals.horizon_eyes[horizon_eye].from_primary * clip, weight)",
        );
    } else {
        source = source.replace("stars.clip_from_sky[0]", "stars.clip_from_sky[horizon_eye]");
    }
    source.insert_str(0, "var<private> horizon_eye: u32;\n");
    let mut edits = Vec::new();
    for stage in ["@vertex", "@fragment"] {
        for (at, _) in source.match_indices(stage) {
            let function = at + source[at..].find("fn ").unwrap();
            let args = function + source[function..].find('(').unwrap() + 1;
            let mut level = 1;
            let mut end = args;
            for (i, c) in source[args..].char_indices() {
                match c {
                    '(' => level += 1,
                    ')' => level -= 1,
                    _ => {}
                }
                if level == 0 {
                    end = args + i;
                    break;
                }
            }
            let comma = if source[args..end].trim().is_empty() {
                ""
            } else {
                ", "
            };
            edits.push((
                args,
                format!("@builtin(view_index) horizon_view: u32{comma}"),
            ));
            let body = end + source[end..].find('{').unwrap() + 1;
            edits.push((body, "horizon_eye = horizon_view;\n".to_owned()));
        }
    }
    edits.sort_by_key(|(at, _)| *at);
    for (at, text) in edits.into_iter().rev() {
        source.insert_str(at, &text);
    }
    source
}

pub struct Targets {
    pub color: wgpu::Texture,
    pub depth: wgpu::TextureView,
}
impl Targets {
    pub fn new(device: &wgpu::Device, size: [u32; 2], format: wgpu::TextureFormat) -> Self {
        let make = |label, format, usage| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: size[0],
                    height: size[1],
                    depth_or_array_layers: 2,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        Self {
            color: make(
                "Quest stereo color",
                format,
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            ),
            depth: make(
                "Quest stereo depth",
                wgpu::TextureFormat::Depth32Float,
                wgpu::TextureUsages::RENDER_ATTACHMENT,
            )
            .create_view(&Default::default()),
        }
    }
    pub fn fits(&self, size: [u32; 2], format: wgpu::TextureFormat) -> bool {
        [self.color.width(), self.color.height()] == size && self.color.format() == format
    }
    pub fn copy(&self, encoder: &mut wgpu::CommandEncoder, eyes: [&wgpu::Texture; 2]) {
        for (layer, eye) in eyes.iter().enumerate() {
            encoder.copy_texture_to_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.color,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        z: layer as u32,
                        ..Default::default()
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                eye.as_image_copy(),
                wgpu::Extent3d {
                    width: self.color.width(),
                    height: self.color.height(),
                    depth_or_array_layers: 1,
                },
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn built_in_stereo_shaders_validate() {
        for source in [
            super::super::terrain_shader(),
            super::super::entity_shader(),
            super::super::static_entity_shader(),
            include_str!("../hdr_light.wgsl").to_owned() + include_str!("stars.wgsl"),
        ] {
            let source = shader(&source);
            let module = naga::front::wgsl::parse_str(&source)
                .unwrap_or_else(|e| panic!("{}", e.emit_to_string(&source)));
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .unwrap();
            assert!(module.entry_points.iter().all(|e| {
                e.function
                    .arguments
                    .iter()
                    .any(|a| a.binding == Some(naga::Binding::BuiltIn(naga::BuiltIn::ViewIndex)))
            }));
        }
    }

    /// A real GPU pass, through the production shader adapter and layer-copy
    /// path. The second eye must land on a different pixel, not duplicate L.
    #[test]
    #[ignore = "requires a Vulkan adapter with MULTIVIEW; also runs on Mesa lavapipe"]
    fn stereo_pass_projects_and_copies_each_eye_independently() {
        use wgpu::util::DeviceExt;
        let instance = super::super::test_gpu::instance();
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
        assert!(adapter.features().contains(wgpu::Features::MULTIVIEW));
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_features: wgpu::Features::MULTIVIEW,
            required_limits: wgpu::Limits {
                max_multiview_view_count: 2,
                ..Default::default()
            },
            ..Default::default()
        }))
        .unwrap();
        let mut globals: super::super::Globals = bytemuck::Zeroable::zeroed();
        globals.sunrise = [1.0, 0.0, 0.0, 1.0];
        let primary = View {
            view: Mat4::IDENTITY,
            projection: Mat4::IDENTITY,
            aspect: 1.0,
            tan_half_fov: 1.0,
        };
        let eyes = [
            Eye::new(primary, [0.0; 4], primary),
            Eye::new(
                View {
                    view: Mat4::from_translation(glam::Vec3::X),
                    ..primary
                },
                [0.0; 4],
                primary,
            ),
        ];
        let mut bytes = bytemuck::bytes_of(&globals).to_vec();
        bytes.extend_from_slice(bytemuck::cast_slice(&eyes));
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: &bytes,
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let reuse = super::super::reuse::Reuse::default();
        let generation = reuse.begin();
        let kept = reuse.stereo_for_test(&device);
        let module = kept.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(super::super::terrain_shader().into()),
        });
        let attributes = wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32];
        let mono = kept.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: None,
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_sunrise"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: 20,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &attributes,
                })],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_sunrise"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::TextureFormat::Rgba8Unorm.into())],
            }),
            multiview_mask: None,
            cache: None,
        });
        reuse.finish(generation);
        let pipeline = reuse
            .alternates(std::iter::once(&mono))
            .unwrap()
            .pop()
            .unwrap();
        assert_eq!(
            reuse.alternates(std::iter::once(&pipeline)).unwrap()[0],
            mono
        );
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        let vertices: [[f32; 5]; 3] = [
            [-0.8, -0.6, 0.5, 1.0, 1.0],
            [-0.2, -0.6, 0.5, 1.0, 1.0],
            [-0.5, 0.6, 0.5, 1.0, 1.0],
        ];
        let vertex = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let target = Targets::new(&device, [32, 32], wgpu::TextureFormat::Rgba8Unorm);
        let eyes: [_; 2] = std::array::from_fn(|_| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d {
                    width: 32,
                    height: 32,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            })
        });
        let read = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 256 * 32 * 2,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        let view = target.color.create_view(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                multiview_mask: MASK,
                ..Default::default()
            });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.set_vertex_buffer(0, vertex.slice(..));
            pass.draw(0..3, 0..1);
        }
        target.copy(&mut encoder, [&eyes[0], &eyes[1]]);
        for (i, eye) in eyes.iter().enumerate() {
            encoder.copy_texture_to_buffer(
                eye.as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer: &read,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: i as u64 * 256 * 32,
                        bytes_per_row: Some(256),
                        rows_per_image: Some(32),
                    },
                },
                eye.size(),
            );
        }
        queue.submit([encoder.finish()]);
        let (sent, received) = std::sync::mpsc::channel();
        read.slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                sent.send(result).unwrap()
            });
        device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(std::time::Duration::from_secs(30)),
            })
            .unwrap();
        received.recv().unwrap().unwrap();
        let pixels = read.slice(..).get_mapped_range().unwrap();
        let red = |eye: usize, x: usize| pixels[eye * 256 * 32 + 16 * 256 + x * 4];
        assert_eq!((red(0, 8), red(0, 24)), (255, 0));
        assert_eq!((red(1, 8), red(1, 24)), (0, 255));
    }
}
