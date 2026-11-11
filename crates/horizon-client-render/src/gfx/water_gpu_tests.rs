//! Compare actual GPU pixels before and after the planar-water index filter.
use super::{
    multiview, test_gpu,
    translucency::{SortState, Sorter},
};
use glam::{Mat4, Vec3};
use wgpu::util::DeviceExt;

#[test]
#[ignore = "requires Vulkan MULTIVIEW; runs on Mesa lavapipe"]
fn planar_water_pixels_match_above_below_and_with_eyes_straddling_the_surface() {
    compare_water_pixels(false);
}

#[test]
#[ignore = "requires Vulkan MULTIVIEW; runs on Mesa lavapipe"]
fn mixed_water_pixels_match_with_banks_and_multiple_surface_heights() {
    compare_water_pixels(true);
}

fn compare_water_pixels(mixed: bool) {
    let instance = test_gpu::instance();
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
    let q = [
        [-1.0, 0.0, -1.0],
        [-1.0, 0.0, 1.0],
        [1.0, 0.0, 1.0],
        [1.0, 0.0, -1.0],
    ];
    let mut positions: Vec<_> = q.into_iter().chain(q.into_iter().rev()).collect();
    if mixed {
        let raised = q.map(|mut p| {
            p[1] += 0.75;
            p
        });
        let wall = q.map(|p| [p[1] + 0.25, p[2], p[0]]);
        for face in [raised, wall] {
            positions.extend(face);
            positions.extend(face.into_iter().rev());
        }
    }
    let vertices: Vec<_> = positions
        .iter()
        .map(|&p| crate::mesher::TerrainVertex::new(p, [0.0; 2], 0, 0, 0))
        .collect();
    let centers: Vec<_> = positions
        .as_chunks::<4>()
        .0
        .iter()
        .map(|q| super::translucency::quad_center(q[0], q[2]))
        .collect();
    let mut state = SortState::from_vertices(&vertices, true);
    let mut sorter = Sorter::default();
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: None, source: wgpu::ShaderSource::Wgsl(r#"
        struct U { matrices: array<mat4x4<f32>, 2> }
        @group(0) @binding(0) var<uniform> u: U;
        struct Out { @builtin(position) position: vec4<f32>, @location(0) color: vec4<f32> }
        @vertex fn vs(@location(0) p: vec3<f32>, @builtin(vertex_index) i: u32, @builtin(view_index) eye: u32) -> Out {
            var o: Out; o.position = u.matrices[eye] * vec4(p, 1.0);
            o.color = select(vec4(1.0,0.0,0.0,0.5), vec4(0.0,0.0,1.0,0.5), i >= 4u); return o;
        }
        @fragment fn fs(o: Out) -> @location(0) vec4<f32> { return o.color; }
    "#.into()) });
    let attributes = wgpu::vertex_attr_array![0 => Float32x3];
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: None,
        layout: None,
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs"),
            compilation_options: Default::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: 12,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &attributes,
            })],
        },
        primitive: wgpu::PrimitiveState {
            cull_mode: Some(wgpu::Face::Back),
            ..Default::default()
        },
        depth_stencil: None,
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba8Unorm,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: multiview::MASK,
        cache: None,
    });
    let vertex = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: bytemuck::cast_slice(&positions),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let uniform = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 128,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: uniform.as_entire_binding(),
        }],
    });
    for heights in [[2.0, 2.0], [-2.0, -2.0], [0.5, -0.5]] {
        let all = Sorter::default()
            .sort(&centers, [0.0, heights[0] as f32, 4.0])
            .to_vec();
        let selected = state
            .update_headset_eyes(
                [0.0, heights[0], 4.0],
                [0; 3],
                heights.map(|y| [0.0, y, 4.0]),
                &mut sorter,
                false,
            )
            .unwrap()
            .to_vec();
        if mixed {
            assert!(selected.len() < all.len());
        } else {
            assert_eq!(
                selected.len(),
                if heights[0] == heights[1] { 6 } else { 12 }
            );
        }
        let matrices = heights.map(|y| {
            (Mat4::perspective_rh(70.0f32.to_radians(), 1.0, 0.05, 100.0)
                * Mat4::look_at_rh(Vec3::new(0.0, y as f32, 4.0), Vec3::ZERO, Vec3::Y))
            .to_cols_array_2d()
        });
        queue.write_buffer(&uniform, 0, bytemuck::cast_slice(&matrices));
        let read = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 256 * 32 * 4,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        for (i, indices) in [&all, &selected].into_iter().enumerate() {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: None,
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
            let view = texture.create_view(&Default::default());
            let index = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytemuck::cast_slice(indices),
                usage: wgpu::BufferUsages::INDEX,
            });
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
                    multiview_mask: multiview::MASK,
                    ..Default::default()
                });
                pass.set_pipeline(&pipeline);
                pass.set_bind_group(0, &group, &[]);
                pass.set_vertex_buffer(0, vertex.slice(..));
                pass.set_index_buffer(index.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..indices.len() as u32, 0, 0..1);
            }
            encoder.copy_texture_to_buffer(
                texture.as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer: &read,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: i as u64 * 256 * 32 * 2,
                        bytes_per_row: Some(256),
                        rows_per_image: Some(32),
                    },
                },
                texture.size(),
            );
        }
        queue.submit([encoder.finish()]);
        let (send, receive) = std::sync::mpsc::channel();
        read.slice(..)
            .map_async(wgpu::MapMode::Read, move |r| send.send(r).unwrap());
        device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(std::time::Duration::from_secs(30)),
            })
            .unwrap();
        receive.recv().unwrap().unwrap();
        let pixels = read.slice(..).get_mapped_range().unwrap();
        let (before, after) = pixels.split_at(256 * 32 * 2);
        for eye in 0..2 {
            assert!(
                before[eye * 8192..(eye + 1) * 8192]
                    .as_chunks::<256>()
                    .0
                    .iter()
                    .any(|row| row[..128]
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .any(|p| p[0] != 0 || p[2] != 0)),
                "reference eye {eye} must actually see water"
            );
        }
        assert_eq!(before, after, "eye heights {heights:?}");
    }
}
