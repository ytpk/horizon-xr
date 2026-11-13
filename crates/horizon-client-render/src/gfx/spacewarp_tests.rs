use super::*;

fn device(multiview: bool) -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance = test_gpu::instance();
    let adapter = pollster::block_on(instance.request_adapter(&Default::default())).ok()?;
    if multiview
        && (!adapter.features().contains(wgpu::Features::MULTIVIEW)
            || adapter.limits().max_multiview_view_count < 2)
    {
        return None;
    }
    let info = adapter.get_info();
    let device = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_features: if multiview {
            wgpu::Features::MULTIVIEW
        } else {
            wgpu::Features::empty()
        },
        required_limits: wgpu::Limits {
            max_multiview_view_count: if multiview { 2 } else { 0 },
            ..Default::default()
        },
        ..Default::default()
    }))
    .ok();
    if device.is_some() {
        eprintln!("SpaceWarp GPU: {}, multiview={multiview}", info.name);
    }
    device
}

/// Portable coverage for the actual OpenXR/Vulkan variants, independent of GPU presence.
#[test]
fn vulkan_variants_translate() {
    for stereo in [false, true] {
        let module = naga::front::wgsl::parse_str(shader_source(stereo)).unwrap();
        let info = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap();
        let words = naga::back::spv::write_vec(&module, &info, &Default::default(), None).unwrap();
        assert!(!words.is_empty());
    }
}

fn texture(device: &wgpu::Device, format: wgpu::TextureFormat, layers: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("SpaceWarp test"),
        size: wgpu::Extent3d {
            width: 64,
            height: 64,
            depth_or_array_layers: layers,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}
fn clear(device: &wgpu::Device, queue: &wgpu::Queue, depth: &wgpu::Texture, values: &[f32]) {
    let mut encoder = device.create_command_encoder(&Default::default());
    for (layer, z) in values.iter().enumerate() {
        let view = depth.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2),
            base_array_layer: layer as u32,
            array_layer_count: Some(1),
            ..Default::default()
        });
        drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(*z),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        }));
    }
    queue.submit([encoder.finish()]);
}
fn pixel(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    layer: u32,
) -> Vec<f32> {
    let texel = if texture.format().is_depth_stencil_format() {
        4
    } else {
        8
    };
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 64 * 64 * texel,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d {
                x: 0,
                y: 0,
                z: layer,
            },
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(64 * texel as u32),
                rows_per_image: None,
            },
        },
        wgpu::Extent3d {
            width: 64,
            height: 64,
            depth_or_array_layers: 1,
        },
    );
    queue.submit([encoder.finish()]);
    buffer.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let bytes = buffer.slice(..).get_mapped_range().unwrap();
    let start = (32 * 64 + 32) * texel as usize;
    if texel == 4 {
        vec![f32::from_le_bytes(
            bytes[start..start + 4].try_into().unwrap(),
        )]
    } else {
        bytes[start..start + 8]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| half::f16::from_bits(u16::from_le_bytes(*b)).to_f32())
            .collect()
    }
}

/// Read actual GPU output for both eye layers: forward XYZ NDC motion, reversed depth,
/// independent eye uniforms, and the separate-eye fallback. No OpenXR runtime required.
#[test]
fn camera_motion_and_depth_are_correct_for_stereo_and_separate_eyes() {
    let Some((device, queue)) = device(true) else {
        return;
    };
    let atlas =
        texture(&device, wgpu::TextureFormat::Rgba8Unorm, 1).create_view(&Default::default());
    let mut sw = SpaceWarp::new(&device, &atlas);
    let p = headset::eye_projection([-1., 1., 1., -1.], 0.05, 100.);
    let right = headset::eye_projection([-0.7, 0.9, 1., -1.], 0.05, 100.);
    sw.current = [p, right];
    sw.reproject = [
        p * Mat4::from_translation(Vec3::new(0.01, 0., 0.02)),
        right * Mat4::from_translation(Vec3::new(-0.02, 0., -0.01)),
    ];
    for stereo in [true, false] {
        let targets = SpaceWarpTargets {
            motion: texture(&device, wgpu::TextureFormat::Rgba16Float, 2),
            depth: texture(&device, wgpu::TextureFormat::Depth32Float, 2),
        };
        sw.targets = Some(targets.clone());
        sw.drawn = 0;
        let source = texture(
            &device,
            wgpu::TextureFormat::Depth32Float,
            if stereo { 2 } else { 1 },
        );
        if stereo {
            clear(&device, &queue, &source, &[0.5, 0.25]);
            let mut encoder = device.create_command_encoder(&Default::default());
            sw.draw(
                &device,
                &queue,
                &mut encoder,
                &source.create_view(&Default::default()),
                true,
                None,
            );
            queue.submit([encoder.finish()]);
        } else {
            for eye in 0..2 {
                clear(
                    &device,
                    &queue,
                    &source,
                    &[if eye == 0 { 0.5 } else { 0.25 }],
                );
                sw.eye = eye;
                let mut encoder = device.create_command_encoder(&Default::default());
                sw.draw(
                    &device,
                    &queue,
                    &mut encoder,
                    &source.create_view(&Default::default()),
                    false,
                    None,
                );
                queue.submit([encoder.finish()]);
            }
        }
        assert_eq!(sw.drawn, 3);
        for (eye, z, dx, dz, focal, offset) in [
            (0, 0.5, 0.01, 0.02, 1., 0.),
            (1, 0.25, -0.02, -0.01, 1.25, 0.125),
        ] {
            let actual = pixel(&device, &queue, &targets.motion, eye);
            let a = 0.05 / (100. - 0.05);
            let b = 100. * a;
            let distance = b / (z + a);
            let nx = 1. / 64.;
            let ny = -1. / 64.;
            let expected = [
                nx - (((nx + offset) * distance + focal * dx) / (distance - dz) - offset),
                ny - ny * distance / (distance - dz),
                z - (b / (distance - dz) - a),
            ];
            for i in 0..3 {
                assert!(
                    (actual[i] - expected[i]).abs() < 0.0003,
                    "eye {eye} channel {i}: {actual:?}, expected {expected:?}"
                );
            }
            assert_eq!(actual[3], 1.);
            assert!((pixel(&device, &queue, &targets.depth, eye)[0] - z).abs() < 1e-6);
        }
    }
}

#[test]
fn moving_objects_override_camera_vectors_and_respect_occlusion() {
    let Some((device, queue)) = device(true) else {
        return;
    };
    let atlas = device.create_texture_with_data(
        &queue,
        &wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        &[255; 4],
    );
    let mut sw = SpaceWarp::new(&device, &atlas.create_view(&Default::default()));
    let p = headset::eye_projection([-1., 1., 1., -1.], 0.05, 100.);
    let right = headset::eye_projection([-0.7, 0.9, 1., -1.], 0.05, 100.);
    sw.current = [p, right];
    sw.reproject = [p, right];
    let targets = SpaceWarpTargets {
        motion: texture(&device, wgpu::TextureFormat::Rgba16Float, 2),
        depth: texture(&device, wgpu::TextureFormat::Depth32Float, 2),
    };
    sw.targets = Some(targets.clone());
    let source = texture(&device, wgpu::TextureFormat::Depth32Float, 2);
    clear(&device, &queue, &source, &[0.5, 0.75]);
    let distance = 5.0 / 50.025; // depth 0.5 for n=0.05,f=100, independently inverted.
    let vertices = [
        (-0.04, -0.04),
        (0.04, -0.04),
        (0.04, 0.04),
        (-0.04, -0.04),
        (0.04, 0.04),
        (-0.04, 0.04),
    ]
    .map(|(x, y)| MotionVertex {
        position: [x, y, -distance],
        previous: [x - 0.01, y, -distance - 0.02],
        uv: [0.5, 0.5],
        translucent: 0,
    });
    queue.write_buffer(&sw.vertices, 0, bytemuck::cast_slice(&vertices));
    sw.count = 6;
    let mut encoder = device.create_command_encoder(&Default::default());
    sw.draw(
        &device,
        &queue,
        &mut encoder,
        &source.create_view(&Default::default()),
        true,
        None,
    );
    queue.submit([encoder.finish()]);
    let moving = pixel(&device, &queue, &targets.motion, 0);
    assert!(moving[0] > 0.08 && moving[0] < 0.09, "{moving:?}");
    assert!(moving[2] > 0.08 && moving[2] < 0.09, "{moving:?}");
    let hidden = pixel(&device, &queue, &targets.motion, 1);
    assert!(hidden[..3].iter().all(|v| v.abs() < 0.0001), "{hidden:?}");
    let overlay = vertices.map(|v| MotionVertex {
        translucent: 2,
        ..v
    });
    queue.write_buffer(&sw.vertices, 0, bytemuck::cast_slice(&overlay));
    let mut encoder = device.create_command_encoder(&Default::default());
    sw.draw(
        &device,
        &queue,
        &mut encoder,
        &source.create_view(&Default::default()),
        true,
        None,
    );
    queue.submit([encoder.finish()]);
    let pointer = pixel(&device, &queue, &targets.motion, 1);
    // Right-eye asymmetric lens: its focal scale is 1.25 and offset 0.125.
    let expected_x = (0.0125 + (1. / 64. + 0.125) * 0.02) / (distance + 0.02);
    assert!(
        (pointer[0] - expected_x).abs() < 0.0004 && pointer[2] > 0.08,
        "{pointer:?}"
    );
    assert!((pixel(&device, &queue, &targets.depth, 1)[0] - 0.5).abs() < 0.0001);
}

#[test]
fn separate_eye_fallback_works_without_the_multiview_device_feature() {
    let Some((device, queue)) = device(false) else {
        return;
    };
    let atlas =
        texture(&device, wgpu::TextureFormat::Rgba8Unorm, 1).create_view(&Default::default());
    let mut sw = SpaceWarp::new(&device, &atlas);
    assert!(sw.stereo.is_none());
    let p = headset::eye_projection([-1., 1., 1., -1.], 0.05, 100.);
    sw.current = [p; 2];
    sw.reproject = [p; 2];
    let targets = SpaceWarpTargets {
        motion: texture(&device, wgpu::TextureFormat::Rgba16Float, 2),
        depth: texture(&device, wgpu::TextureFormat::Depth32Float, 2),
    };
    sw.targets = Some(targets.clone());
    let source = texture(&device, wgpu::TextureFormat::Depth32Float, 1);
    clear(&device, &queue, &source, &[0.5]);
    for eye in 0..2 {
        sw.eye = eye;
        let mut encoder = device.create_command_encoder(&Default::default());
        sw.draw(
            &device,
            &queue,
            &mut encoder,
            &source.create_view(&Default::default()),
            false,
            None,
        );
        queue.submit([encoder.finish()]);
        let actual = pixel(&device, &queue, &targets.motion, eye as u32);
        assert!(actual[..3].iter().all(|v| v.abs() < 0.0001), "{actual:?}");
        assert_eq!(actual[3], 1.);
        assert_eq!(pixel(&device, &queue, &targets.depth, eye as u32)[0], 0.5);
    }
}
