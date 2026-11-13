//! Lossless 16+8-byte terrain streams. Layer stays beside position for opaque rejection.
pub const PRIMARY: [wgpu::VertexAttribute; 3] =
    wgpu::vertex_attr_array![0=>Float32x3,2=>Uint16,4=>Uint8x2];
pub const SECONDARY: [wgpu::VertexAttribute; 2] = wgpu::vertex_attr_array![1=>Uint16x2,3=>Unorm8x4];
pub(super) fn buffers<'a>(
    split: bool,
    instance: &'a [wgpu::VertexAttribute],
) -> Vec<Option<wgpu::VertexBufferLayout<'a>>> {
    let mut buffers = vec![
        Some(wgpu::VertexBufferLayout {
            array_stride: if split { 16 } else { 24 },
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: if split {
                &PRIMARY
            } else {
                &super::TERRAIN_ATTRIBUTES
            },
        }),
        Some(wgpu::VertexBufferLayout {
            array_stride: 16,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: instance,
        }),
    ];
    if split {
        buffers.push(Some(wgpu::VertexBufferLayout {
            array_stride: 8,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &SECONDARY,
        }));
    }
    buffers
}

pub fn split(bytes: &[u8], primary: &mut Vec<u8>, secondary: &mut Vec<u8>) {
    assert!(bytes.len().is_multiple_of(24));
    primary.clear();
    secondary.clear();
    primary.reserve(bytes.len() / 24 * 16);
    secondary.reserve(bytes.len() / 24 * 8);
    for v in bytes.as_chunks::<24>().0 {
        primary.extend_from_slice(&v[..12]);
        primary.extend_from_slice(&v[16..20]);
        secondary.extend_from_slice(&v[12..16]);
        secondary.extend_from_slice(&v[20..24]);
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn all_vertex_bits_survive_split_including_flags_and_float_payloads() {
        let bytes: Vec<u8> = (0..24 * 513)
            .map(|i| ((i * 37 + i / 11) & 255) as u8)
            .collect();
        let (mut a, mut b) = (Vec::new(), Vec::new());
        super::split(&bytes, &mut a, &mut b);
        assert_eq!(a.len() + b.len(), bytes.len());
        let rebuilt: Vec<u8> = a
            .as_chunks::<16>()
            .0
            .iter()
            .zip(b.as_chunks::<8>().0)
            .flat_map(|(a, b)| {
                a[..12]
                    .iter()
                    .chain(&b[..4])
                    .chain(&a[12..])
                    .chain(&b[4..])
                    .copied()
            })
            .collect();
        assert_eq!(rebuilt, bytes);
        super::split(&[], &mut a, &mut b);
        assert!(a.is_empty() && b.is_empty());
    }

    #[test]
    #[ignore = "requires Vulkan MULTIVIEW; runs on Mesa lavapipe"]
    fn split_and_interleaved_streams_render_identical_pixels_for_both_eyes() {
        use super::super::{arena::Arena, test_gpu, upload::Uploads};
        use wgpu::util::DeviceExt;
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
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(
                r#"
            struct O { @builtin(position) p:vec4<f32>, @location(0) color:vec4<f32> }
            @vertex fn vs(@location(0) p:vec3<f32>, @location(1) uv:vec2<u32>,
                @location(2) layer:u32,@location(3) color:vec4<f32>,@location(4) light:vec2<u32>,
                @location(5) origin:vec4<f32>,@builtin(view_index) eye:u32)->O {
                var o:O; o.p=vec4(p+origin.xyz+vec3(f32(eye)*0.15,0.,0.),1.);
                o.color=color+vec4(f32(uv.x+uv.y+layer+light.x+light.y)/65535.,0.,0.,0.); return o;
            }
            @fragment fn fs(o:O)->@location(0) vec4<f32> {return o.color;}
        "#
                .into(),
            ),
        });
        let origins = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&[0f32; 4]),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let vertices = [
            crate::mesher::TerrainVertex::new(
                [-0.8, -0.8, 0.5],
                [0., 0.],
                0x8001,
                0xff0000ff,
                0x1234,
            ),
            crate::mesher::TerrainVertex::new([0.7, -0.8, 0.5], [1., 0.], 2, 0xff00ff00, 0x5678),
            crate::mesher::TerrainVertex::new([0., 0.8, 0.5], [0., 1.], 3, 0xffff0000, 0xabcd),
        ];
        let read = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 32768,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        let mut uploads = Uploads::new(&device);
        let indices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&[0u32, 1, 2]),
            usage: wgpu::BufferUsages::INDEX,
        });
        let mut arenas = Vec::new();
        let mut images = Vec::new();
        for (i, split) in [false, true].into_iter().enumerate() {
            let mut arena = Arena::new(&device, 24, 256, false);
            arena.set_split(split);
            arena.alloc(&device, &mut encoder, 256).unwrap();
            let (page, start) = arena.alloc(&device, &mut encoder, 3).unwrap();
            assert!(start > 0, "exercise a nonzero base vertex in both streams");
            arena.upload(
                &mut encoder,
                &mut uploads,
                page,
                start,
                bytemuck::cast_slice(&vertices),
            );
            let instance = wgpu::vertex_attr_array![5=>Float32x4];
            let buffers = super::buffers(split, &instance);
            let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: None,
                layout: None,
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs"),
                    buffers: &buffers,
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs"),
                    targets: &[Some(wgpu::TextureFormat::Rgba8Unorm.into())],
                    compilation_options: Default::default(),
                }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview_mask: std::num::NonZeroU32::new(3),
                cache: None,
            });
            let image = device.create_texture(&wgpu::TextureDescriptor {
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
            let view = image.create_view(&Default::default());
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    multiview_mask: std::num::NonZeroU32::new(3),
                    ..Default::default()
                });
                pass.set_pipeline(&pipeline);
                pass.set_vertex_buffer(0, arena.buffer(page).slice(..));
                pass.set_vertex_buffer(1, origins.slice(..));
                if let Some(attributes) = arena.attribute_binding(page) {
                    pass.set_vertex_buffer(2, attributes);
                }
                pass.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..3, start as i32, 0..1);
            }
            encoder.copy_texture_to_buffer(
                image.as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer: &read,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: i as u64 * 16384,
                        bytes_per_row: Some(256),
                        rows_per_image: Some(32),
                    },
                },
                image.size(),
            );
            arenas.push(arena);
            images.push(image);
        }
        uploads.finish(&encoder);
        queue.submit([encoder.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        read.slice(..)
            .map_async(wgpu::MapMode::Read, move |r| tx.send(r).unwrap());
        device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(std::time::Duration::from_secs(30)),
            })
            .unwrap();
        rx.recv().unwrap().unwrap();
        let bytes = read.slice(..).get_mapped_range().unwrap();
        for row in 0..64 {
            let at = row * 256;
            assert_eq!(&bytes[at..at + 128], &bytes[16384 + at..16384 + at + 128]);
        }
        assert!(
            bytes[..16384]
                .as_chunks::<4>()
                .0
                .iter()
                .any(|p| p[0] > 0 || p[1] > 0 || p[2] > 0)
        );
    }
}
