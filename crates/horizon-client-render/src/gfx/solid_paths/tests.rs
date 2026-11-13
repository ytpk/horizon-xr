//! These pipelines against the usual ones, on the test machine's GPU: the same terrain.wgsl,
//! scenes of alpha around the cutout threshold, fog, fade-in, light and emission, and the pixels.
use super::*;
use crate::gfx::{lod, reuse::Reuse, section_instance, terrain_source};
use wgpu::util::DeviceExt;

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const SIZE: [u32; 2] = [256, 64];
/// A whole section's instance `w` (terrain.wgsl's `terrain_uv`).
const WHOLE: f32 = 1.0 / 32768.0;
/// Half through its fade-in (`section_instance`).
const FADING_IN: f32 = 1.5 / 32768.0;

#[derive(Clone, Copy, Debug)]
struct Scene {
    /// Far enough for fog and the fade-in, with fog set.
    far: bool,
    colored_light: bool,
    emissive_overlay: bool,
    /// `Options.textureFiltering`.
    filtering: f32,
    /// A 16x16 sprite with holes; else one texel.
    textured: bool,
    alpha: u8,
    w: f32,
}

const PLAIN: Scene = Scene {
    far: false,
    colored_light: false,
    emissive_overlay: false,
    filtering: 0.0,
    textured: false,
    alpha: 255,
    w: WHOLE,
};

/// Every setting that reaches these shaders, at each alpha around the cutout threshold.
fn scenes(w: f32) -> Vec<Scene> {
    let mut scenes = Vec::new();
    for alpha in [0, 64, 127, 128, 255] {
        let scene = Scene { alpha, w, ..PLAIN };
        scenes.extend([
            scene,
            Scene { far: true, ..scene },
            Scene {
                far: true,
                colored_light: true,
                ..scene
            },
            Scene {
                emissive_overlay: true,
                ..scene
            },
            Scene {
                filtering: 2.0,
                ..scene
            },
            Scene {
                textured: true,
                ..scene
            },
            Scene {
                textured: true,
                far: true,
                ..scene
            },
            Scene {
                textured: true,
                filtering: 2.0,
                ..scene
            },
        ]);
    }
    scenes
}

/// Sixteen columns of all the emission tiers, half of them with opaque leaves' layer bit.
fn quads(scene: &Scene) -> Vec<u8> {
    let mut vertices = Vec::new();
    for emission in 0..16u8 {
        let x = -1.0 + f32::from(emission) / 8.0;
        for [px, py] in [
            [x, -1.0],
            [x + 0.125, -1.0],
            [x + 0.125, 1.0],
            [x, -1.0],
            [x + 0.125, 1.0],
            [x, 1.0],
        ] {
            let z = if scene.far { 32.5f32 } else { 0.5 };
            vertices.extend_from_slice(bytemuck::cast_slice(&[px, py, z]));
            let (u, v) = if scene.textured {
                (
                    ((px - x) * 8.0 * 32768.0) as u16,
                    ((py + 1.0) * 0.5 * 32768.0) as u16,
                )
            } else {
                (16384, 16384)
            };
            let layer = if emission % 2 == 0 { 0x8000u16 } else { 0 };
            vertices.extend_from_slice(bytemuck::cast_slice(&[u, v, layer]));
            vertices.extend_from_slice(&[240, 240, 64, 128, 192, 255 - emission]);
        }
    }
    vertices
}

struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    bindings: wgpu::BindGroupLayout,
    layout: wgpu::PipelineLayout,
    flat_bindings: Option<wgpu::BindGroupLayout>,
    shader: wgpu::ShaderModule,
    globals: naga::Module,
    reuse: Reuse,
}

impl Gpu {
    fn new() -> Option<Self> {
        Self::open(false)
    }
    /// A device with MULTIVIEW, as a headset's, where the adapter has it.
    fn open(multiview: bool) -> Option<Self> {
        let instance = crate::gfx::test_gpu::instance();
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).ok()?;
        eprintln!("Solid paths test adapter: {:?}", adapter.get_info());
        let flat = adapter
            .get_downlevel_capabilities()
            .flags
            .contains(wgpu::DownlevelFlags::VERTEX_STORAGE);
        if multiview && !adapter.features().contains(wgpu::Features::MULTIVIEW) {
            return None;
        }
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_features: if multiview {
                wgpu::Features::MULTIVIEW
            } else {
                wgpu::Features::empty()
            },
            required_limits: adapter.limits(),
            ..Default::default()
        }))
        .ok()?;
        let entries: Vec<_> = (0..13)
            .map(|binding| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                count: None,
                ty: match binding {
                    0 => wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    2 | 4 => wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    _ => wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: if binding == 3 || binding == 12 {
                            wgpu::TextureViewDimension::D2
                        } else {
                            wgpu::TextureViewDimension::D2Array
                        },
                        multisampled: false,
                    },
                },
            })
            .collect();
        let bindings = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &entries,
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&bindings)],
            ..Default::default()
        });
        let storage = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let flat_bindings = flat.then(|| {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: None,
                entries: &[storage(0), storage(1)],
            })
        });
        let source = terrain_source::terrain_shader(false);
        let globals = naga::front::wgsl::parse_str(&source).unwrap();
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Solid paths"),
            source: wgpu::ShaderSource::Wgsl(source),
        });
        Some(Self {
            device,
            queue,
            bindings,
            layout,
            flat_bindings,
            shader,
            globals,
            reuse: Reuse::default(),
        })
    }

    fn globals(&self, scene: &Scene) -> Vec<u8> {
        let (_, globals) = self
            .globals
            .types
            .iter()
            .find(|(_, t)| t.name.as_deref() == Some("Globals"))
            .unwrap();
        let naga::TypeInner::Struct { members, span } = &globals.inner else {
            panic!("Globals")
        };
        let offset = |name: &str| {
            members
                .iter()
                .find(|m| m.name.as_deref() == Some(name))
                .unwrap()
                .offset as usize
        };
        let mut bytes = vec![0u8; *span as usize];
        let mut write = |name: &str, values: &[f32]| {
            let at = offset(name);
            bytes[at..at + values.len() * 4].copy_from_slice(bytemuck::cast_slice(values));
        };
        let mut matrix = glam::Mat4::IDENTITY.to_cols_array();
        // The far scene's positions come back to the clip volume, 32 blocks off the camera.
        matrix[14] = if scene.far { -32.0 } else { 0.0 };
        write("view_proj", &matrix);
        write("fog_color", &[0.3, 0.5, 0.7, 0.8]);
        write(
            "fog",
            if scene.far {
                &[20.0, 50.0, 25.0, 60.0]
            } else {
                &[1e6, 2e6, 1e6, 2e6]
            },
        );
        write("filtering", &[scene.filtering, 16.0, 0.0, 0.0]);
        write("hdr_windows", &[0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8]);
        write("hdr_light", &[0.5, 0.5, 0.0, 0.0]);
        write("sun", &[0.0, 0.0, -1.0, 0.0]);
        if scene.colored_light {
            write(
                "colored_lights",
                &[0.0, 0.0, 32.0, 8.0, 1.0, 0.25, 0.5, 1.0],
            );
        }
        let mut count = |name: &str, value: u32| {
            let at = offset(name);
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
        };
        if scene.colored_light {
            count("colored_count", 1);
        }
        if scene.emissive_overlay {
            // The first pair: layer 0 over itself.
            count("emissive_layers", 0);
            count("emissive_count", 1);
        }
        bytes
    }

    fn render(&self, passes: &[&wgpu::RenderPipeline], scene: &Scene, flat: bool) -> Vec<u8> {
        let (device, queue) = (&self.device, &self.queue);
        let texture = |edge: u32, texels: &[u8]| {
            device.create_texture_with_data(
                queue,
                &wgpu::TextureDescriptor {
                    label: None,
                    size: wgpu::Extent3d {
                        width: edge,
                        height: edge,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: FORMAT,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                },
                wgpu::util::TextureDataOrder::LayerMajor,
                texels,
            )
        };
        let sprite = if scene.textured {
            let texels: Vec<u8> = (0..256u32)
                .flat_map(|i| {
                    [
                        i.wrapping_mul(37) as u8,
                        i.wrapping_mul(91) as u8,
                        i.wrapping_mul(17) as u8,
                        if i % 4 == 0 { 0 } else { scene.alpha },
                    ]
                })
                .collect();
            texture(16, &texels)
        } else {
            texture(1, &[255, 255, 255, scene.alpha])
        };
        let array = sprite.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let white = texture(1, &[255; 4]).create_view(&Default::default());
        let sampler = device.create_sampler(&Default::default());
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: &self.globals(scene),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let entries: Vec<_> = (0..13)
            .map(|binding| wgpu::BindGroupEntry {
                binding,
                resource: match binding {
                    0 => uniform.as_entire_binding(),
                    2 | 4 => wgpu::BindingResource::Sampler(&sampler),
                    3 | 12 => wgpu::BindingResource::TextureView(&white),
                    _ => wgpu::BindingResource::TextureView(&array),
                },
            })
            .collect();
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.bindings,
            entries: &entries,
        });
        let origin = [0.0f32, 0.0, 0.0, scene.w];
        let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: &quads(scene),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let instance = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&origin),
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::STORAGE,
        });
        // Every vertex lies in block 0, whose section is slot 0 (`vs_terrain_flat`).
        let slots = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&[0u32; 4]),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let flat_group = flat.then(|| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: self.flat_bindings.as_ref().unwrap(),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: slots.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: instance.as_entire_binding(),
                    },
                ],
            })
        });
        let size = wgpu::Extent3d {
            width: SIZE[0],
            height: SIZE[1],
            depth_or_array_layers: 1,
        };
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let depth = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let read = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: u64::from(SIZE[0] * SIZE[1] * 4),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let view = target.create_view(&Default::default());
            let depth = depth.create_view(&Default::default());
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(0.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_bind_group(0, &group, &[]);
            if let Some(flat_group) = &flat_group {
                pass.set_bind_group(1, flat_group, &[]);
            }
            pass.set_vertex_buffer(0, vertices.slice(..));
            pass.set_vertex_buffer(1, instance.slice(..));
            for pipeline in passes {
                pass.set_pipeline(pipeline);
                pass.draw(0..96, 0..1);
            }
        }
        encoder.copy_texture_to_buffer(
            target.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &read,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(SIZE[0] * 4),
                    rows_per_image: Some(SIZE[1]),
                },
            },
            size,
        );
        queue.submit([encoder.finish()]);
        read.slice(..)
            .map_async(wgpu::MapMode::Read, |r| r.unwrap());
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        read.slice(..).get_mapped_range().unwrap().to_vec()
    }
}

/// What a test compares: the usual pipelines and this module's, per-section and flat.
struct Pipelines<'a> {
    gpu: &'a Gpu,
    paths: SolidPaths,
    /// The same with the lean varyings (`Lean`).
    lean: SolidPaths,
    solid: wgpu::RenderPipeline,
    opaque: [Option<wgpu::RenderPipeline>; 2],
}

impl<'a> Pipelines<'a> {
    fn new(gpu: &'a Gpu) -> Self {
        gpu.reuse.begin();
        let kept = gpu.reuse.on(&gpu.device);
        let attributes = crate::gfx::TERRAIN_ATTRIBUTES;
        let instance_attributes = wgpu::vertex_attr_array![5=>Float32x4];
        let buffers = [
            Some(wgpu::VertexBufferLayout {
                array_stride: 24,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &attributes,
            }),
            Some(wgpu::VertexBufferLayout {
                array_stride: 16,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &instance_attributes,
            }),
        ];
        let terrain = TerrainModule {
            kept: &kept,
            shader: &gpu.shader,
            format: FORMAT,
            // As `Renderer::new`'s.
            constants: &[("SPRITE_ARRAYS", 1.0), ("SPRITE_SPLIT", 2048.0)],
        };
        let paths = SolidPaths::new(
            &terrain,
            &gpu.bindings,
            &gpu.layout,
            gpu.flat_bindings.as_ref(),
            &buffers,
            Lean::default(),
        )
        .expect("the test GPU takes these pipelines");
        let lean = SolidPaths::new(
            &terrain,
            &gpu.bindings,
            &gpu.layout,
            gpu.flat_bindings.as_ref(),
            &buffers,
            Lean {
                opaque: true,
                cutout: true,
            },
        )
        .expect("the test GPU takes these pipelines");
        let back = Some(wgpu::Face::Back);
        let usual = |vertex, fragment| {
            terrain
                .pipeline(
                    &gpu.layout,
                    "Usual",
                    vertex,
                    fragment,
                    &buffers,
                    back,
                    true,
                    None,
                )
                .unwrap()
        };
        let flat_layout = gpu.flat_bindings.as_ref().map(|flat| {
            gpu.device
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: None,
                    bind_group_layouts: &[Some(&gpu.bindings), Some(flat)],
                    ..Default::default()
                })
        });
        let flat_opaque = flat_layout.as_ref().map(|layout| {
            terrain
                .pipeline(
                    layout,
                    "Usual flat",
                    "vs_terrain_flat_opaque",
                    "fs_opaque",
                    &buffers[..1],
                    back,
                    true,
                    None,
                )
                .unwrap()
        });
        Self {
            gpu,
            paths,
            lean,
            solid: usual("vs_terrain", "fs_solid"),
            opaque: [Some(usual("vs_terrain_opaque", "fs_opaque")), flat_opaque],
        }
    }
    /// The usual opaque pass, then `cutout`.
    fn split(&self, scene: &Scene, cutout: &Pass, flat: bool) -> Vec<u8> {
        let opaque = self.opaque[usize::from(flat)].as_ref().unwrap();
        self.gpu
            .render(&[opaque, cutout.get(flat).unwrap()], scene, flat)
    }
}

impl Pipelines<'_> {
    fn settled(&self, scene: &Scene, flat: bool) -> Vec<u8> {
        let [opaque, cutout] = self.paths.settled.as_ref().unwrap();
        let passes = [opaque.get(flat).unwrap(), cutout.get(flat).unwrap()];
        self.gpu.render(&passes, scene, flat)
    }
}

/// A frame's colour without its alpha: what the lean passes keep. Alpha only carries HDR
/// lighting, which they leave out (`HDR_LIGHTING` off), so it is 1 there.
fn colour(pixels: &[u8]) -> Vec<u8> {
    let mut pixels = pixels.to_vec();
    pixels.iter_mut().skip(3).step_by(4).for_each(|a| *a = 0);
    pixels
}

/// Each image holds what the scene should: the comparisons would pass over empty ones too.
fn assert_drawn(pixels: &[u8], scene: &Scene) {
    if scene.alpha > 127 {
        assert!(pixels.iter().any(|&p| p != 0), "{scene:?} drew nothing");
    }
}

#[test]
fn static_cutout_draws_the_usual_pixels() {
    let Some(gpu) = Gpu::new() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let pipelines = Pipelines::new(&gpu);
    for w in [WHOLE, FADING_IN] {
        for scene in scenes(w) {
            let usual = gpu.render(&[&pipelines.solid], &scene, false);
            assert_drawn(&usual, &scene);
            let split = pipelines.split(&scene, &pipelines.paths.static_cutout, false);
            assert!(usual == split, "{scene:?}");
            if gpu.flat_bindings.is_some() {
                let flat = pipelines.split(&scene, &pipelines.paths.static_cutout, true);
                assert!(usual == flat, "{scene:?} (flat)");
            }
        }
    }
}

#[test]
fn settled_terrain_draws_the_usual_pixels() {
    let Some(gpu) = Gpu::new() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let pipelines = Pipelines::new(&gpu);
    for scene in scenes(WHOLE) {
        let usual = gpu.render(&[&pipelines.solid], &scene, false);
        assert_drawn(&usual, &scene);
        assert!(usual == pipelines.settled(&scene, false), "{scene:?}");
        if gpu.flat_bindings.is_some() {
            assert!(usual == pipelines.settled(&scene, true), "{scene:?} (flat)");
        }
    }
}

#[test]
fn a_frame_takes_what_its_instances_prove() {
    let Some(gpu) = Gpu::new() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let pipelines = Pipelines::new(&gpu);
    let paths = &pipelines.paths;
    let origin = [1.0, 2.0, 3.0];
    let (whole, fading_in, fading) = (
        section_instance(origin, 0, 1.0),
        section_instance(origin, 0, 0.5),
        section_instance(origin, 56, 1.0),
    );
    let (tile, fading_tile) = (lod::instance(origin, 0), lod::instance(origin, 56));
    // What each pass takes in place of the usual pipeline.
    let taken = |origins: &[[f32; 4]], filtering| {
        let passes = paths.for_frame(origins, filtering);
        (
            passes.opaque(false).is_some(),
            passes.cutout(false).is_some(),
        )
    };
    assert_eq!(taken(&[whole, whole], 0), (true, true));
    assert_eq!(taken(&[whole, whole], 2), (true, true));
    // RGSS, a fade-in or a distant tile leave the cutout without its fade.
    assert_eq!(taken(&[whole, whole], 1), (false, true));
    assert_eq!(taken(&[whole, fading_in], 0), (false, true));
    assert_eq!(taken(&[whole, tile], 0), (false, true));
    // A fade keeps the usual passes, even in one instance of many.
    assert_eq!(taken(&[whole, fading], 0), (false, false));
    assert_eq!(taken(&[whole, fading_tile], 0), (false, false));
}

#[test]
fn lean_passes_draw_the_usual_colours() {
    let Some(gpu) = Gpu::new() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let pipelines = Pipelines::new(&gpu);
    let lean = &pipelines.lean;
    let opaque = lean.lean_opaque.as_ref().expect("a lean opaque pass");
    // A frame with fades or tiles: the lean opaque pass, then the cutout pass without fades
    // (the frames the instances prove unfaded).
    for w in [WHOLE, FADING_IN] {
        for scene in scenes(w) {
            let usual = colour(&gpu.render(&[&pipelines.solid], &scene, false));
            assert_drawn(&usual, &scene);
            let cutout = lean.static_cutout.get(false).unwrap();
            let image = colour(&gpu.render(&[opaque.get(false).unwrap(), cutout], &scene, false));
            assert!(usual == image, "{scene:?}");
            if gpu.flat_bindings.is_some() {
                let flat = colour(&gpu.render(
                    &[
                        opaque.get(true).unwrap(),
                        lean.static_cutout.get(true).unwrap(),
                    ],
                    &scene,
                    true,
                ));
                assert!(usual == flat, "{scene:?} (flat)");
            }
        }
    }
    // Settled terrain through the lean passes.
    for scene in scenes(WHOLE) {
        let usual = colour(&gpu.render(&[&pipelines.solid], &scene, false));
        let [opaque, cutout] = lean.settled.as_ref().unwrap();
        let image = colour(&gpu.render(
            &[opaque.get(false).unwrap(), cutout.get(false).unwrap()],
            &scene,
            false,
        ));
        assert!(usual == image, "{scene:?} (settled)");
    }
}

#[test]
fn lean_opaque_serves_frames_with_fades() {
    let Some(gpu) = Gpu::new() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let pipelines = Pipelines::new(&gpu);
    let fading = section_instance([1.0, 2.0, 3.0], 56, 1.0);
    let frame = |paths: &SolidPaths| {
        let passes = paths.for_frame(&[fading], 0);
        (
            passes.opaque(false).is_some(),
            passes.cutout(false).is_some(),
        )
    };
    // The opaque pass has no fade to know; the cutout keeps the usual pipeline.
    assert_eq!(frame(&pipelines.lean), (true, false));
    assert_eq!(frame(&pipelines.paths), (false, false));
}

/// The headset's multiview swap needs a stereo variant of every pipeline the Adreno paths build
/// (they go through `try_create_render_pipeline`) once the renderer is built (`Reuse::finish`
/// lets go of what the build did not ask for: it dropped these variants): without one the world
/// never leaves the separate eye passes. Solid paths, both lean settings, and the late sky.
#[test]
fn every_adreno_path_pipeline_has_a_multiview_variant() {
    let Some(gpu) = Gpu::open(true) else {
        eprintln!("no GPU adapter with MULTIVIEW: skipped");
        return;
    };
    let generation = gpu.reuse.begin();
    let kept = gpu.reuse.stereo_for_test(&gpu.device);
    let source = terrain_source::terrain_shader(false);
    let shader = kept.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Solid paths, stereo"),
        source: wgpu::ShaderSource::Wgsl(source),
    });
    let attributes = crate::gfx::TERRAIN_ATTRIBUTES;
    let instance_attributes = wgpu::vertex_attr_array![5=>Float32x4];
    let buffers = [
        Some(wgpu::VertexBufferLayout {
            array_stride: 24,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &attributes,
        }),
        Some(wgpu::VertexBufferLayout {
            array_stride: 16,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &instance_attributes,
        }),
    ];
    let constants = [("SPRITE_ARRAYS", 1.0), ("SPRITE_SPLIT", 2048.0)];
    let terrain = TerrainModule {
        kept: &kept,
        shader: &shader,
        format: FORMAT,
        constants: &constants,
    };
    let mut built = Vec::new();
    for lean in [
        Lean::default(),
        Lean {
            opaque: true,
            cutout: true,
        },
    ] {
        let paths = SolidPaths::new(
            &terrain,
            &gpu.bindings,
            &gpu.layout,
            gpu.flat_bindings.as_ref(),
            &buffers,
            lean,
        )
        .expect("the test GPU takes these pipelines");
        built.push((lean, paths));
    }
    // The late sky, as `Renderer::new` builds it.
    let sunrise_attributes = wgpu::vertex_attr_array![0=>Float32x4, 1=>Float32];
    let sunrise_buffers = [Some(wgpu::VertexBufferLayout {
        array_stride: 20,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &sunrise_attributes,
    })];
    let blend = wgpu::BlendState::ALPHA_BLENDING;
    let entity = kept.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Late sky entities"),
        source: wgpu::ShaderSource::Wgsl(crate::gfx::entity_shader().into()),
    });
    let entity_attributes = wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x2,2=>Uint32,3=>Unorm8x4,4=>Uint8x4,5=>Snorm8x4,6=>Float32x2];
    let entity_buffers = [Some(wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<crate::mesher::Vertex>() as u64,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &entity_attributes,
    })];
    let targets = [Some(wgpu::ColorTargetState {
        format: FORMAT,
        blend: Some(blend),
        write_mask: wgpu::ColorWrites::ALL,
    })];
    let celestial = wgpu::RenderPipelineDescriptor {
        label: Some("Celestial bodies"),
        layout: None,
        vertex: wgpu::VertexState {
            module: &entity,
            entry_point: Some("vs_entity"),
            compilation_options: Default::default(),
            buffers: &entity_buffers,
        },
        primitive: Default::default(),
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: Some(false),
            depth_compare: Some(wgpu::CompareFunction::Always),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState {
            module: &entity,
            entry_point: Some("fs_sky_celestial"),
            compilation_options: Default::default(),
            targets: &targets,
        }),
        multiview_mask: None,
        cache: None,
    };
    let stars = crate::gfx::stars::Stars::new(&gpu.device, FORMAT, &kept);
    let mut late = crate::gfx::late_sky::LateSky::new(
        &terrain,
        &gpu.layout,
        (&sunrise_buffers, blend),
        &celestial,
        &stars,
    )
    .expect("the test GPU takes these pipelines");
    gpu.reuse.finish(generation);
    for (lean, paths) in &mut built {
        let all = paths.pipelines_mut();
        assert!(!all.is_empty());
        assert!(
            gpu.reuse.alternates(all.iter().map(|p| &**p)).is_some(),
            "a pipeline of the solid paths has no multiview variant ({lean:?})"
        );
    }
    let sky = late.pipelines_mut();
    assert!(
        gpu.reuse.alternates(sky.iter().map(|p| &**p)).is_some(),
        "a pipeline of the late sky has no multiview variant"
    );
}
