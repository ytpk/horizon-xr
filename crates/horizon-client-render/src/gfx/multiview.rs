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
/// `HorizonEye`'s fields the rewritten shaders read, and their types.
const EYE_FIELDS: [(&str, &str); 5] = [
    ("view_proj", "mat4x4<f32>"),
    ("inverse_view", "mat4x4<f32>"),
    ("inverse_projection", "mat4x4<f32>"),
    ("misc", "vec4<f32>"),
    ("from_primary", "mat4x4<f32>"),
];

/// A developer A/B switch for the headset tester: on unless `HORIZON_<NAME>=0`. Read once.
fn switch(cell: &std::sync::OnceLock<bool>, name: &str) -> bool {
    *cell.get_or_init(|| std::env::var(name).as_deref() != Ok("0"))
}

/// The screen effects (vignette, overlays) draw at the end of the stereo world pass
/// (`HORIZON_QUEST_SCREEN_IN_WORLD=0`: in a pass of their own, which loads and stores both
/// layers again on a tiled GPU).
pub fn screen_in_world() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    switch(&ON, "HORIZON_QUEST_SCREEN_IN_WORLD")
}

/// The hidden-area mask is drawn in the stereo world pass too (`HORIZON_QUEST_STEREO_VISMASK=0`:
/// only in the separate eye passes).
pub fn stereo_vis_mask() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    switch(&ON, "HORIZON_QUEST_STEREO_VISMASK")
}

/// The stereo world pass leaves out the sections its last occlusion results did not see
/// (`cull::Cull::recall`), with `HORIZON_QUEST_OCCLUSION=1` alone: on a Quest 3S the boxes'
/// fragment shader, which writes its results to a storage buffer, draws nothing, so every
/// section tested counts as hidden and most of the world goes missing.
pub fn occlusion_recall() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("HORIZON_QUEST_OCCLUSION").as_deref() == Ok("1"))
}

/// `HORIZON_QUEST_EXPERIMENT=a,b`: what the stereo world pass leaves out, for measuring which
/// part of the frame owns the GPU time on a headset (only back to back runs tell there):
/// `noopaque` and `nocutout` (the two solid terrain passes), `notranslucent` (water, glass),
/// `nosky`, `noscreen` (the screen effects at the pass's end), `scissor` (only a sixteenth of each eye is drawn: what is left is the vertex
/// work). They break the picture: for measuring only.
#[derive(Clone, Copy, Default)]
pub struct Experiment {
    pub no_opaque: bool,
    pub no_cutout: bool,
    pub no_translucent: bool,
    pub no_sky: bool,
    pub scissor: bool,
    pub no_screen: bool,
}
pub fn experiment() -> Experiment {
    static ON: std::sync::OnceLock<Experiment> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        let text = std::env::var("HORIZON_QUEST_EXPERIMENT").unwrap_or_default();
        let has = |name| text.split(',').any(|part| part.trim() == name);
        Experiment {
            no_opaque: has("noopaque"),
            no_cutout: has("nocutout"),
            no_translucent: has("notranslucent"),
            no_sky: has("nosky"),
            scissor: has("scissor"),
            no_screen: has("noscreen"),
        }
    })
}

/// `HORIZON_QUEST_SKY_DEBUG=a,b`: what the stereo world pass leaves out of the sky, for finding
/// which draw a fault is in. `magenta` clears the pass to magenta: a sky pixel that stays
/// magenta was not covered by the sky's triangle.
#[derive(Clone, Copy, Default)]
pub struct SkyDebug {
    pub no_base: bool,
    pub no_sunrise: bool,
    pub no_celestial: bool,
    pub no_stars: bool,
    pub no_dark: bool,
    pub magenta: bool,
}
impl SkyDebug {
    pub fn parse(text: &str) -> Self {
        let has = |name| text.split(',').any(|part| part.trim() == name);
        Self {
            no_base: has("nobase"),
            no_sunrise: has("nosunrise"),
            no_celestial: has("nocelestial"),
            no_stars: has("nostars"),
            no_dark: has("nodark"),
            magenta: has("magenta"),
        }
    }
}
pub fn sky_debug() -> SkyDebug {
    static DEBUG: std::sync::OnceLock<SkyDebug> = std::sync::OnceLock::new();
    *DEBUG.get_or_init(|| {
        std::env::var("HORIZON_QUEST_SKY_DEBUG")
            .map(|text| SkyDebug::parse(&text))
            .unwrap_or_default()
    })
}

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
        // Each eye's values through constant indices only: indexing the uniform block by the
        // view index made Adreno's compiler read all of `Globals` from memory instead of its
        // constant registers, in every vertex.
        for (field, ty) in EYE_FIELDS {
            source.push_str(&format!(
                "\nfn horizon_eye_{field}() -> {ty} {{ if horizon_eye == 0u {{ return globals.horizon_eyes[0].{field}; }} return globals.horizon_eyes[1].{field}; }}\n"
            ));
        }
        for field in ["view_proj", "inverse_view", "misc"] {
            source = source.replace(
                &format!("globals.{field}"),
                &format!("horizon_eye_{field}()"),
            );
        }
        // The ordinary sky's symmetric reconstruction cannot represent two
        // asymmetric headset lenses. Reconstruct through each actual projection.
        if let Some(start) = source.find("fn view_ray(") {
            let body = start + source[start..].find('{').unwrap();
            let end = body + source[body..].find('}').unwrap();
            source.replace_range(body + 1..end, "\nlet p = horizon_eye_inverse_projection() * vec4(ndc, 1.0, 1.0);\nreturn normalize((horizon_eye_inverse_view() * vec4(p.xyz / p.w, 0.0)).xyz);\n");
        }
        source = source.replace(
            "SunriseOut(clip, weight)",
            "SunriseOut(horizon_eye_from_primary() * clip, weight)",
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
    pub native: bool,
    pub depth: wgpu::TextureView,
}
impl Targets {
    pub fn new(device: &wgpu::Device, size: [u32; 2], format: wgpu::TextureFormat) -> Self {
        Self::with_color(device, size, format, None)
    }
    pub fn with_color(
        device: &wgpu::Device,
        size: [u32; 2],
        format: wgpu::TextureFormat,
        color: Option<&wgpu::Texture>,
    ) -> Self {
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
            native: color.is_some(),
            color: color.cloned().unwrap_or_else(|| {
                make(
                    "Quest stereo color",
                    format,
                    wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                )
            }),
            depth: make(
                "Quest stereo depth",
                wgpu::TextureFormat::Depth32Float,
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
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

    #[test]
    fn sky_debug_names_what_to_leave_out() {
        let debug = SkyDebug::parse("nobase, magenta");
        assert!(debug.no_base && debug.magenta);
        assert!(!debug.no_stars && !debug.no_dark && !debug.no_sunrise && !debug.no_celestial);
    }

    /// The sky through two different lenses, one of them off-centre and turned: each pixel is
    /// sky above the horizon and fog below it, by the pixel's own direction worked out here.
    #[test]
    #[ignore = "requires a Vulkan adapter with MULTIVIEW; also runs on Mesa lavapipe"]
    fn stereo_sky_follows_each_eyes_lens() {
        use wgpu::util::DeviceExt;
        const SIZE: usize = 32;
        let instance = super::super::test_gpu::instance();
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_features: wgpu::Features::MULTIVIEW,
            required_limits: wgpu::Limits {
                max_multiview_view_count: 2,
                ..Default::default()
            },
            ..Default::default()
        }))
        .unwrap();
        // [left, right, up, down] tangents and the eye's turn.
        let lenses = [
            ([-1.0f32, 1.0, 1.0, -1.0], glam::Quat::IDENTITY),
            (
                [-1.3, 0.8, 0.9, -1.1],
                glam::Quat::from_rotation_y(0.4)
                    * glam::Quat::from_rotation_x(0.5)
                    * glam::Quat::from_rotation_z(0.3),
            ),
        ];
        let views = lenses.map(|(tangents, turn)| View {
            view: Mat4::from_quat(turn.conjugate()),
            projection: super::super::headset::eye_projection(tangents, 0.05, 1000.0),
            aspect: (tangents[1] - tangents[0]) / (tangents[2] - tangents[3]),
            tan_half_fov: (tangents[2] - tangents[3]) / 2.0,
        });
        let mut globals: super::super::Globals = bytemuck::Zeroable::zeroed();
        globals.sky_color = [0.0, 0.0, 1.0, 1.0];
        globals.fog_color = [1.0, 0.0, 0.0, 1.0];
        // No fog on the disc: sky colour wherever the ray meets it.
        globals.fog_ends = [1e9, 1e9, 1e9, 1e9];
        let eyes = views.map(|view| Eye::new(view, [0.0; 4], views[0]));
        let mut bytes = bytemuck::bytes_of(&globals).to_vec();
        bytes.extend_from_slice(bytemuck::cast_slice(&eyes));
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: &bytes,
            usage: wgpu::BufferUsages::UNIFORM,
        });
        {
            let source = shader(&super::super::terrain_shader());
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: None,
                source: wgpu::ShaderSource::Wgsl(source.into()),
            });
            let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: None,
                layout: None,
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs_sky"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some("fs_sky_base"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::TextureFormat::Rgba8Unorm.into())],
                }),
                multiview_mask: MASK,
                cache: None,
            });
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &pipeline.get_bind_group_layout(0),
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                }],
            });
            let target = Targets::new(&device, [SIZE as u32; 2], wgpu::TextureFormat::Rgba8Unorm);
            let row = SIZE * 4;
            let read = device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: (256 * SIZE * 2) as u64,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            assert!(row <= 256);
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
                            load: wgpu::LoadOp::Clear(wgpu::Color::GREEN),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    multiview_mask: MASK,
                    ..Default::default()
                });
                pass.set_pipeline(&pipeline);
                pass.set_bind_group(0, &group, &[]);
                pass.draw(0..3, 0..1);
            }
            encoder.copy_texture_to_buffer(
                target.color.as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer: &read,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(256),
                        rows_per_image: Some(SIZE as u32),
                    },
                },
                target.color.size(),
            );
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
            let mut seen = [[0; 2]; 2];
            for (eye, (tangents, turn)) in lenses.iter().enumerate() {
                let [left, right, up, down] = *tangents;
                for y in 0..SIZE {
                    for x in 0..SIZE {
                        let across = (x as f32 + 0.5) / SIZE as f32;
                        let below = (y as f32 + 0.5) / SIZE as f32;
                        let ray = *turn
                            * glam::Vec3::new(
                                left + across * (right - left),
                                up + below * (down - up),
                                -1.0,
                            )
                            .normalize();
                        let elevation = ray.y.asin().to_degrees();
                        // The disc's rim stands 1.8 to 1.9 degrees over the horizon.
                        let wanted = if elevation > 2.5 {
                            [0, 0, 255]
                        } else if elevation < 1.2 {
                            [255, 0, 0]
                        } else {
                            continue;
                        };
                        seen[eye][usize::from(wanted[0] == 0)] += 1;
                        let at = eye * 256 * SIZE + y * 256 + x * 4;
                        assert_eq!(pixels[at..at + 3], wanted, "eye {eye} pixel {x},{y}");
                    }
                }
            }
            // Both eyes show both sky and fog, or the picture says nothing.
            assert!(seen.iter().flatten().all(|&count| count > 20), "{seen:?}");
        }
    }

    /// A real GPU pass, through the production shader adapter and layer-copy
    /// path. The second eye must land on a different pixel, not duplicate L.
    #[test]
    #[ignore = "requires a Vulkan adapter with MULTIVIEW; also runs on Mesa lavapipe"]
    fn stereo_pass_projects_and_copies_each_eye_independently() {
        check_stereo_target(false);
    }
    #[test]
    #[ignore = "requires a Vulkan adapter with MULTIVIEW; also runs on Mesa lavapipe"]
    fn stereo_pass_draws_directly_into_an_external_color_array() {
        check_stereo_target(true);
    }
    fn check_stereo_target(native: bool) {
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
        let external = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("mock OpenXR stereo array"),
            size: wgpu::Extent3d {
                width: 32,
                height: 32,
                depth_or_array_layers: 2,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let target = if native {
            let target = Targets::with_color(
                &device,
                [32, 32],
                wgpu::TextureFormat::Rgba8Unorm,
                Some(&external),
            );
            assert_eq!(
                target.color, external,
                "render directly into the supplied image"
            );
            target
        } else {
            Targets::new(&device, [32, 32], wgpu::TextureFormat::Rgba8Unorm)
        };
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
        if !native {
            target.copy(&mut encoder, [&eyes[0], &eyes[1]]);
        }
        for (i, eye) in eyes.iter().enumerate() {
            let source = if native {
                wgpu::TexelCopyTextureInfo {
                    texture: &external,
                    origin: wgpu::Origin3d {
                        z: i as u32,
                        ..Default::default()
                    },
                    ..external.as_image_copy()
                }
            } else {
                eye.as_image_copy()
            };
            encoder.copy_texture_to_buffer(
                source,
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
