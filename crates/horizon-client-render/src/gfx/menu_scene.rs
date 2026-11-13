// SPDX-License-Identifier: MPL-2.0
// Ports Visor: render/decoration/decorators/DecoratorMainMenu.java, mainmenu/VRMenuSky.java, mainmenu/VRMenuFloor.java. Copyright (C) Phoenix-Ra and Visor contributors; MPL-2.0 (see LICENSE-MPL-2.0).
//! The room around the title screen in a headset, as Visor's main menu draws it instead of the
//! game's panorama: a small moss platform of the play area's size with a border, standing above a
//! sea of blocky clouds, in a sky that follows the clock (sun, moon in its real phase, stars at night, a
//! dotted VISOR sign). It is one full-screen pass per eye (`menu_scene.wgsl`); the panel sits in it.
use crate::gfx::headset::EyePose;
use glam::{Mat3, Vec3};
use std::time::Instant;

/// The play area's size when the runtime does not give one, metres (Visor asks the runtime for it).
const PLAY_AREA: [f32; 2] = [2.5, 2.5];
/// `VRMenuFloor`: a second, larger floor 2 m wider than the play area around it.
const BORDER: f32 = 2.0;
/// `VRMenuSky.ARC_TILT_RAD`: the sun's path leans 20 degrees toward the south.
const ARC_TILT_DEG: f32 = 20.0;
/// `SYNODIC_MONTH_DAYS` and `NEW_MOON_EPOCH_MS` (2000-01-06 18:14 UTC).
const SYNODIC_MONTH_DAYS: f64 = 29.530_588_853;
const NEW_MOON_EPOCH_MS: f64 = 947_182_440_000.0;

/// What the sky looks like at a moment: the sun and the moon's directions, how much day there is,
/// the twilight and the lit part of the moon.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sky {
    pub sun: Vec3,
    pub moon: Vec3,
    /// 0 night, 1 day.
    pub day: f32,
    /// 1 as the sun crosses the horizon, 0 well away from it.
    pub twilight: f32,
    /// 0 new moon to 1 full.
    pub moon_lit: f32,
    /// The local hour (0..24), for what only happens at midnight.
    pub hour: f32,
}

/// The sky at local `hour` (0..24, fractions of it) with the moon `moon_age` of the way through
/// its month (0..1, 0 new): the sun rises in the east at 6 and sets at 18 (`SUNRISE_TIME`,
/// `SUNSET_TIME`), on a path tilted [`ARC_TILT_DEG`] degrees.
pub fn sky_at(hour: f32, moon_age: f32) -> Sky {
    let angle = (hour - 6.0) / 12.0 * std::f32::consts::PI;
    let tilt = ARC_TILT_DEG.to_radians();
    let sun = Vec3::new(
        angle.cos(),
        angle.sin() * tilt.cos(),
        angle.sin() * tilt.sin(),
    )
    .normalize();
    let day = ((sun.y + 0.08) / 0.30).clamp(0.0, 1.0);
    let day = day * day * (3.0 - 2.0 * day);
    let twilight = (1.0 - sun.y.abs() / 0.25).clamp(0.0, 1.0);
    let moon_lit = 0.5 * (1.0 - (moon_age * std::f32::consts::TAU).cos());
    Sky {
        sun,
        moon: -sun,
        day,
        twilight,
        moon_lit,
        hour,
    }
}

/// Where the moon is in its month at `unix_ms`: 0..1, 0 new (`VRMenuSky`'s "real lunar phase").
pub fn moon_age(unix_ms: f64) -> f32 {
    let days = (unix_ms - NEW_MOON_EPOCH_MS) / 86_400_000.0;
    (days / SYNODIC_MONTH_DAYS).rem_euclid(1.0) as f32
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniform {
    rot: [[f32; 4]; 3],
    eye: [f32; 4],
    tangents: [f32; 4],
    sun: [f32; 4],
    moon: [f32; 4],
    floor: [f32; 4],
    /// The shooting star: its spawn direction and how far along its streak it is (0..1), then its
    /// travel direction and its alpha (0 for none now).
    meteor0: [f32; 4],
    meteor1: [f32; 4],
    /// The UFO: its direction and how faded in it is (0 for none now), then which belly light
    /// chases and how many groups there are.
    ufo0: [f32; 4],
    ufo1: [f32; 4],
    /// The headset icon's dots over the sign: 10 rows of 21 bits, row 0 first, a lit dot per bit
    /// (the left column of the picture is bit 0).
    icon: [[u32; 4]; 3],
}

/// The headset icon (`VISOR_ICON_TEXTURE`) sampled to `VISOR_ICON_COLS` by `VISOR_ICON_ROWS` dots.
const ICON_PNG: &[u8] = include_bytes!("../../assets/visor/icon_headset.png");
const ICON_COLS: usize = 21;
const ICON_ROWS: usize = 10;
/// `VISOR_ICON_MIN_ALPHA`.
const ICON_MIN_ALPHA: u8 = 128;

/// `sampleIconGrid` and `withoutCornerElbows`: the dots of the icon picture (`rgba`, `w` by `h`),
/// a dot lit where the sampled pixel's alpha is at least 128, except those that are the corner of an
/// elbow (two lit neighbours at the diagonal-free corner of a bend).
pub fn icon_grid(rgba: &[u8], w: usize, h: usize) -> [[bool; ICON_COLS]; ICON_ROWS] {
    let mut lit = [[false; ICON_COLS]; ICON_ROWS];
    for (row, line) in lit.iter_mut().enumerate() {
        let y = ((row as f32 + 0.5) * h as f32 / ICON_ROWS as f32) as usize;
        for (col, dot) in line.iter_mut().enumerate() {
            let x = ((col as f32 + 0.5) * w as f32 / ICON_COLS as f32) as usize;
            *dot = rgba
                .get((y.min(h - 1) * w + x.min(w - 1)) * 4 + 3)
                .is_some_and(|a| *a >= ICON_MIN_ALPHA);
        }
    }
    let get = |r: i32, c: i32| {
        r >= 0
            && c >= 0
            && (r as usize) < ICON_ROWS
            && (c as usize) < ICON_COLS
            && lit[r as usize][c as usize]
    };
    let mut out = lit;
    for r in 0..ICON_ROWS as i32 {
        for c in 0..ICON_COLS as i32 {
            if !get(r, c) {
                continue;
            }
            let mut neighbours = 0;
            for dr in -1..=1 {
                for dc in -1..=1 {
                    if (dr != 0 || dc != 0) && get(r + dr, c + dc) {
                        neighbours += 1;
                    }
                }
            }
            let elbow = neighbours == 2
                && (get(r, c - 1) || get(r, c + 1))
                && (get(r - 1, c) || get(r + 1, c));
            if elbow {
                out[r as usize][c as usize] = false;
            }
        }
    }
    out
}

/// The icon's dots as the shader reads them: a row per `u32`, bit `c` the column `c`.
fn icon_rows() -> [u32; 12] {
    let mut rows = [0u32; 12];
    let Ok(mut reader) = png::Decoder::new(std::io::Cursor::new(ICON_PNG)).read_info() else {
        return rows;
    };
    let mut buf = vec![0u8; reader.output_buffer_size().unwrap_or(0)];
    let Ok(info) = reader.next_frame(&mut buf) else {
        return rows;
    };
    let channels = match info.color_type {
        png::ColorType::Rgba => 4,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Rgb => 3,
        _ => 1,
    };
    // The alpha channel as an RGBA image's, whatever the file stores.
    let rgba: Vec<u8> = buf[..info.buffer_size()]
        .chunks(channels)
        .flat_map(|p| match channels {
            4 => [p[0], p[1], p[2], p[3]],
            2 => [p[0], p[0], p[0], p[1]],
            _ => [p[0], p[0], p[0], 255],
        })
        .collect();
    let grid = icon_grid(&rgba, info.width as usize, info.height as usize);
    for (r, line) in grid.iter().enumerate() {
        for (c, on) in line.iter().enumerate() {
            if *on {
                rows[r] |= 1 << c;
            }
        }
    }
    rows
}

/// `renderUfo`'s state at `time` seconds of the scene when the hour is `hour`: the UFO parks 90
/// blocks away for 15 of every 35 seconds between 00:00 and 01:00, blinking in and out for 0.3
/// s at each end; returns its direction, its fade (0..1) and which belly light group chases
/// (`UFO_LIGHT_STEP_HZ` 2, three groups).
pub fn ufo_at(time: f64, hour: f32) -> Option<([f32; 3], f32, u32)> {
    if hour >= 1.0 {
        return None;
    }
    let cycle = (time / 35.0).floor();
    let age = (time - cycle * 35.0) as f32;
    if age >= 15.0 {
        return None;
    }
    let cycle = cycle as i32;
    // `ufoParkingDir`.
    let mut azimuth = ((f64::from(cycle) * 2.39996) % std::f64::consts::TAU) as f32;
    let elevation = 0.175 + 0.075 * (cycle as f32 * 1.7).sin();
    let horiz = elevation.cos();
    let mut dir = Vec3::new(
        horiz * azimuth.cos(),
        elevation.sin(),
        horiz * azimuth.sin(),
    );
    if dir.dot(VISOR_DIR) > 0.93 {
        azimuth += 1.1;
        dir.x = horiz * azimuth.cos();
        dir.z = horiz * azimuth.sin();
    }
    let fade = (age.min(15.0 - age) / 0.3).clamp(0.0, 1.0);
    let step = (age * 2.0) as u32 % 3;
    Some((dir.to_array(), fade, step))
}

/// `VRMenuSky.hash01`: a deterministic 0..1 from three integers.
fn hash01(i: i32, j: i32, salt: i32) -> f32 {
    let mut h = (i64::from(i).wrapping_mul(0x9E37_79B9_7F4A_7C15_u64 as i64))
        .wrapping_add(i64::from(j).wrapping_mul(0xC2B2_AE3D_27D4_EB4F_u64 as i64))
        .wrapping_add(i64::from(salt).wrapping_mul(0x1656_67B1_9E37_79F9_u64 as i64))
        as u64;
    h ^= h >> 30;
    h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^= h >> 27;
    h = h.wrapping_mul(0x94D0_49BB_1331_11EB);
    h ^= h >> 31;
    (h >> 40) as f32 * (1.0 / (1 << 24) as f32)
}

/// One shooting star every `SHOOTINGSTAR_FREQUENCY` seconds with `SHOOTINGSTAR_CHANCE`, lasting
/// `SHOOTINGSTAR_DURATION_SEC` at a random moment of its window (`VRMenuSky.emitShootingStar`).
const METEOR_FREQUENCY: f32 = 20.0;
const METEOR_CHANCE: f32 = 0.85;
const METEOR_DURATION: f32 = 0.65;
/// The room's `VISOR_DIR` (the sign's direction): a meteor never starts near it.
const VISOR_DIR: Vec3 = Vec3::new(0.0, 0.447_213_6, -0.894_427_2);

/// The shooting star at `time` seconds, with `night` 0..1: `([spawn, progress], [travel, alpha])`
/// for the shader, or none now.
fn meteor_at(time: f64, night: f32) -> ([f32; 4], [f32; 4]) {
    let none = ([0.0; 4], [0.0; 4]);
    let window = (time / f64::from(METEOR_FREQUENCY)).floor() as i32;
    if hash01(window, 0, 30) > METEOR_CHANCE {
        return none;
    }
    let start = 1.0 + hash01(window, 0, 31) * (METEOR_FREQUENCY - METEOR_DURATION - 2.0);
    let age = (time - f64::from(window) * f64::from(METEOR_FREQUENCY)) as f32 - start;
    if !(0.0..=METEOR_DURATION).contains(&age) {
        return none;
    }
    let progress = age / METEOR_DURATION;
    let alpha = (235.0 * night * (std::f32::consts::PI * progress).sin()).floor();
    if alpha <= 0.0 {
        return none;
    }
    let azimuth = hash01(window, 1, 32) * std::f32::consts::TAU;
    let y = 0.35 + 0.45 * hash01(window, 1, 33);
    let ring = (1.0 - y * y).sqrt();
    let spawn = Vec3::new(ring * azimuth.cos(), y, ring * azimuth.sin());
    if spawn.dot(VISOR_DIR) > 0.9 {
        return none;
    }
    // The travel tangent: the horizontal one rolled by a random angle about the spawn direction.
    let inverse = 1.0 / ring.max(1e-4);
    let horizontal = Vec3::new(-spawn.z * inverse, 0.0, spawn.x * inverse);
    let rolled = Vec3::new(
        spawn.y * horizontal.z,
        spawn.z * horizontal.x - spawn.x * horizontal.z,
        -spawn.y * horizontal.x,
    );
    let roll = hash01(window, 2, 34) * std::f32::consts::TAU;
    let travel = Vec3::new(
        horizontal.x * roll.cos() + rolled.x * roll.sin(),
        rolled.y * roll.sin(),
        horizontal.z * roll.cos() + rolled.z * roll.sin(),
    );
    (
        [spawn.x, spawn.y, spawn.z, progress],
        [travel.x, travel.y, travel.z, alpha / 255.0],
    )
}

/// The sky now: the local clock (`HORIZON_MENU_HOUR=14.5` shows it at that hour), the real moon.
pub(super) fn current_sky() -> Sky {
    let now = chrono::Local::now();
    let clock = chrono::Timelike::hour(&now) as f32
        + chrono::Timelike::minute(&now) as f32 / 60.0
        + chrono::Timelike::second(&now) as f32 / 3600.0;
    let hour = std::env::var("HORIZON_MENU_HOUR")
        .ok()
        .and_then(|h| h.parse::<f32>().ok())
        .unwrap_or(clock);
    sky_at(hour, moon_age(now.timestamp_millis() as f64))
}

/// The floor's texture and the pass that draws the room.
pub(super) struct MenuScene {
    shader: wgpu::ShaderModule,
    pipeline_layout: wgpu::PipelineLayout,
    pipeline: Option<(wgpu::TextureFormat, wgpu::RenderPipeline)>,
    uniform: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    started: Instant,
    icon: [u32; 12],
}

/// The floor texture with its mip levels (a box filter), because a 16 pixel texture repeating four times
/// a metre shimmers without them.
pub(super) fn mip_chain(width: u32, height: u32, texels: &[u8]) -> Vec<(u32, u32, Vec<u8>)> {
    let mut out = vec![(width, height, texels.to_vec())];
    while let Some((w, h, last)) = out.last() {
        if *w <= 1 && *h <= 1 {
            break;
        }
        let (nw, nh) = ((*w / 2).max(1), (*h / 2).max(1));
        let mut next = vec![0u8; (nw * nh * 4) as usize];
        for y in 0..nh {
            for x in 0..nw {
                for c in 0..4usize {
                    let mut sum = 0u32;
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        let sx = (x * 2 + dx).min(w - 1);
                        let sy = (y * 2 + dy).min(h - 1);
                        sum += u32::from(last[((sy * w + sx) * 4) as usize + c]);
                    }
                    next[((y * nw + x) * 4) as usize + c] = (sum / 4) as u8;
                }
            }
        }
        out.push((nw, nh, next));
    }
    out
}

impl MenuScene {
    pub(super) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        floor: Option<&(u32, u32, Vec<u8>)>,
    ) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Menu room bindings"),
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
                        view_dimension: wgpu::TextureViewDimension::D2,
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
            label: Some("Menu room shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("menu_scene.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Menu room layout"),
            bind_group_layouts: &[Some(&layout)],
            ..Default::default()
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Menu room uniform"),
            size: std::mem::size_of::<Uniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        // Without the moss texture a flat green stands in.
        let fallback = (1, 1, vec![98u8, 128, 62, 255]);
        let (w, h, texels) = floor.unwrap_or(&fallback);
        let mips = mip_chain(*w, *h, texels);
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Menu room floor"),
            size: wgpu::Extent3d {
                width: *w,
                height: *h,
                depth_or_array_layers: 1,
            },
            mip_level_count: mips.len() as u32,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        for (level, (mw, mh, data)) in mips.iter().enumerate() {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: level as u32,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                data,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(4 * mw),
                    rows_per_image: None,
                },
                wgpu::Extent3d {
                    width: *mw,
                    height: *mh,
                    depth_or_array_layers: 1,
                },
            );
        }
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Menu room floor sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let view = texture.create_view(&Default::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Menu room resources"),
            layout: &layout,
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
        Self {
            shader,
            pipeline_layout,
            pipeline: None,
            uniform,
            bind_group,
            started: Instant::now(),
            icon: icon_rows(),
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
    ) {
        let format = target.format();
        if self.pipeline.as_ref().is_none_or(|(f, _)| *f != format) {
            let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Menu room"),
                layout: Some(&self.pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &self.shader,
                    entry_point: Some("vs_scene"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &self.shader,
                    entry_point: Some("fs_scene"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            });
            self.pipeline = Some((format, pipeline));
        }
        let Some((_, pipeline)) = &self.pipeline else {
            return;
        };
        let m = Mat3::from_quat(eye.orientation);
        let col = |v: Vec3| [v.x, v.y, v.z, 0.0];
        let meteor = meteor_at(
            f64::from(self.started.elapsed().as_secs_f32()),
            1.0 - sky.day,
        );
        let ufo = ufo_at(f64::from(self.started.elapsed().as_secs_f32()), sky.hour);
        let uniform = Uniform {
            rot: [col(m.x_axis), col(m.y_axis), col(m.z_axis)],
            eye: [
                eye.room.x,
                eye.room.y,
                eye.room.z,
                self.started.elapsed().as_secs_f32(),
            ],
            tangents: eye.tangents,
            sun: [sky.sun.x, sky.sun.y, sky.sun.z, sky.day],
            moon: [sky.moon.x, sky.moon.y, sky.moon.z, sky.moon_lit],
            floor: [PLAY_AREA[0] / 2.0, PLAY_AREA[1] / 2.0, BORDER, sky.twilight],
            meteor0: meteor.0,
            meteor1: meteor.1,
            ufo0: ufo.map_or([0.0; 4], |(d, fade, _)| [d[0], d[1], d[2], fade]),
            ufo1: ufo.map_or([0.0; 4], |(_, _, step)| [step as f32, 3.0, 0.0, 0.0]),
            icon: {
                let r = &self.icon;
                [
                    [r[0], r[1], r[2], r[3]],
                    [r[4], r[5], r[6], r[7]],
                    [r[8], r[9], r[10], r[11]],
                ]
            },
        };
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&uniform));
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Menu room for an eye"),
        });
        let view = target.create_view(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Menu room for an eye"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        queue.submit([encoder.finish()]);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_shooting_star_streaks_across_the_sky_for_a_moment_of_some_windows() {
        use super::*;
        // Whatever the window, `hash01` is a 0..1 number and the same one every time.
        for i in -3..40 {
            let h = hash01(i, 1, 33);
            assert!((0.0..1.0).contains(&h), "{h}");
            assert_eq!(h, hash01(i, 1, 33));
        }
        // Over ten minutes at night: stars appear in most windows (85 %) for 0.65 s each, with a
        // unit spawn direction and a travel direction square to it; by day there are none.
        let mut seen = 0;
        let mut t = 0.0;
        while t < 600.0 {
            let (a, b) = meteor_at(t, 1.0);
            if b[3] > 0.0 {
                seen += 1;
                let spawn = Vec3::new(a[0], a[1], a[2]);
                let travel = Vec3::new(b[0], b[1], b[2]);
                assert!((spawn.length() - 1.0).abs() < 1e-4, "{spawn}");
                assert!(spawn.dot(travel).abs() < 1e-4, "{spawn} {travel}");
                assert!((0.0..=1.0).contains(&a[3]));
                assert!(spawn.dot(VISOR_DIR) <= 0.9);
            }
            assert_eq!(meteor_at(t, 0.0).1[3], 0.0);
            t += 0.05;
        }
        // 30 windows of which 85 % show one for 0.65 s at a 0.05 s step, less the first and last
        // instants where it fades to nothing.
        assert!((150..=330).contains(&seen), "{seen}");
    }
    use super::*;

    #[test]
    fn the_sun_is_on_the_east_horizon_at_six_overhead_at_noon_and_gone_at_midnight() {
        let six = sky_at(6.0, 0.0);
        assert!((six.sun - Vec3::X).length() < 1e-5, "{}", six.sun);
        // Twilight is at its fullest as it crosses the horizon, and the day is just starting.
        assert!(six.twilight > 0.99, "{}", six.twilight);
        // The sun on the horizon is 0.08 of the 0.30 ramp up: x = 0.2667, x * x * (3 - 2x) = 0.1754.
        assert!((six.day - 0.1754).abs() < 1e-3, "{}", six.day);
        let noon = sky_at(12.0, 0.0);
        // At noon it is highest: cos(20 deg) up and sin(20 deg) toward the south (+Z).
        assert!((noon.sun.y - 20f32.to_radians().cos()).abs() < 1e-5);
        assert!((noon.sun.z - 20f32.to_radians().sin()).abs() < 1e-5);
        assert_eq!(noon.day, 1.0);
        assert_eq!(noon.twilight, 0.0);
        let midnight = sky_at(0.0, 0.0);
        assert!(midnight.sun.y < 0.0);
        assert_eq!(midnight.day, 0.0);
        // The moon stands where the sun is not.
        assert!((midnight.moon + midnight.sun).length() < 1e-6);
        assert!(midnight.moon.y > 0.9);
    }

    #[test]
    fn the_moon_phase_is_the_real_one() {
        // 2000-01-06 18:14 UTC is a new moon: nothing lit, and half a month later it is full.
        assert_eq!(sky_at(0.0, moon_age(NEW_MOON_EPOCH_MS)).moon_lit, 0.0);
        let full = NEW_MOON_EPOCH_MS + SYNODIC_MONTH_DAYS * 0.5 * 86_400_000.0;
        assert!((sky_at(0.0, moon_age(full)).moon_lit - 1.0).abs() < 1e-5);
        // A quarter of a month on, half of it shows.
        let quarter = NEW_MOON_EPOCH_MS + SYNODIC_MONTH_DAYS * 0.25 * 86_400_000.0;
        assert!((moon_age(quarter) - 0.25).abs() < 1e-5);
        assert!((sky_at(0.0, 0.25).moon_lit - 0.5).abs() < 1e-5);
        // Before the epoch it still wraps into 0..1.
        let before = moon_age(NEW_MOON_EPOCH_MS - 3.0 * 86_400_000.0);
        assert!((0.0..1.0).contains(&before));
    }

    #[test]
    fn the_floor_texture_has_every_mip_down_to_one_texel_averaged() {
        // A 2 x 2 texture of one red, one green, one blue and one white texel.
        let texels = [
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
        ];
        let mips = mip_chain(2, 2, &texels);
        assert_eq!(mips.len(), 2);
        assert_eq!((mips[1].0, mips[1].1), (1, 1));
        // (255 + 0 + 0 + 255) / 4 = 127 for red, and the same for green and blue.
        assert_eq!(mips[1].2, vec![127, 127, 127, 255]);
    }

    #[test]
    fn the_icon_is_sampled_to_dots_and_elbows_lose_their_corner() {
        // A 21 by 10 picture, every pixel a dot: opaque, then a corner of two lit neighbours.
        let all = vec![255u8; 21 * 10 * 4];
        assert!(icon_grid(&all, 21, 10).iter().flatten().all(|d| *d));
        let mut rgba = vec![0u8; 21 * 10 * 4];
        for (r, c) in [(0usize, 0usize), (0, 1), (1, 0)] {
            rgba[(r * 21 + c) * 4 + 3] = 255;
        }
        let grid = icon_grid(&rgba, 21, 10);
        // (0,0) has two neighbours, one beside it and one below: an elbow, it goes.
        assert!(!grid[0][0]);
        assert!(grid[0][1] && grid[1][0]);
        // A pixel less than half opaque is no dot.
        let mut faint = vec![0u8; 21 * 10 * 4];
        faint[(5 * 21 + 5) * 4 + 3] = 127;
        assert!(!icon_grid(&faint, 21, 10)[5][5]);
        // The shipped picture makes dots.
        assert!(icon_rows().iter().any(|r| *r != 0));
    }

    #[test]
    fn the_ufo_comes_for_fifteen_of_every_thirty_five_seconds_in_the_first_hour_of_the_day() {
        // Not at all after 01:00.
        assert!(ufo_at(5.0, 1.0).is_none() && ufo_at(5.0, 13.0).is_none());
        let (dir, fade, step) = ufo_at(7.0, 0.5).expect("there at 7 s");
        assert_eq!(fade, 1.0);
        assert!(step < 3);
        assert!((Vec3::from_array(dir).length() - 1.0).abs() < 1e-5);
        // Blinks in over the first 0.3 s and out over the last, and is gone for the rest of the cycle.
        assert!((ufo_at(0.15, 0.5).unwrap().1 - 0.5).abs() < 1e-4);
        assert!((ufo_at(14.85, 0.5).unwrap().1 - 0.5).abs() < 1e-3);
        assert!(ufo_at(15.0, 0.5).is_none() && ufo_at(34.0, 0.5).is_none());
        // The next cycle parks elsewhere, low over the horizon and never in front of the sign.
        let next = ufo_at(36.0, 0.5).expect("again at 36 s").0;
        assert_ne!(next, dir);
        for cycle in 0..40 {
            let d = Vec3::from_array(ufo_at(f64::from(cycle) * 35.0 + 1.0, 0.5).unwrap().0);
            assert!(d.y > 0.09 && d.y < 0.26, "{d}");
            assert!(d.dot(VISOR_DIR) <= 0.93 + 1e-3, "{d}");
        }
        // The chase moves twice a second through three groups.
        assert_eq!(ufo_at(1.0, 0.5).unwrap().2, 2);
        assert_eq!(ufo_at(1.5, 0.5).unwrap().2, 0);
    }

    #[test]
    fn the_shader_validates() {
        let module = naga::front::wgsl::parse_str(include_str!("menu_scene.wgsl")).unwrap();
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap();
    }
}
