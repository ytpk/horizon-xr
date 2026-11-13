//! A headset that exists only on the computer: two eye images of a plain wgpu device, fixed
//! eye poses a pupil distance apart, and optionally PNG dumps of what the eyes drew. It runs the
//! game's whole headset path (`Renderer::render_headset`, the HUD panel, the per-eye passes)
//! with no OpenXR runtime, so a change to that path can be run and looked at on a desktop GPU
//! before it goes near a Quest. It tracks nothing: the head stays at the play space's origin.
//!
//! `HORIZON_SIM_HEADSET=1` installs it (`install_from_env`); `HORIZON_SIM_EYE=WxH` sizes the eye
//! images (default 1024x1024), `HORIZON_SIM_HZ` paces the frames (default 90, 0 unpaced),
//! `HORIZON_SIM_DUMP=DIR` with `HORIZON_SIM_DUMP_FRAME=N` (default 240) writes `left.png`,
//! `right.png` and `panel.png` of that frame and `HORIZON_SIM_DUMP_EXIT=1` ends the game then.
//! `HORIZON_SIM_HANDS=REACH` holds both controllers REACH metres ahead of the head.
//! `HORIZON_SIM_HEAD_Y=-0.4` holds the head that many metres from its rest height (a real crouch).
//! `HORIZON_SIM_POINTER=U,V` holds the pointer there on the menu panel (0..1 from its top left)
//! and `HORIZON_SIM_CLICK=A:B` the trigger down from frame A to frame B, to press the room's TV.
//! `HORIZON_SIM_POINTER=aim` takes the pointer from where the controller's ray (`HORIZON_SIM_HANDS`)
//! meets the TV, with its distance, so the ray the game draws can be looked at.
//! In the headset launcher's room (`room_layout`) the dump also writes `left_tv.png` and
//! `right_tv.png`: the eyes with the TV's panel laid over them where the quad layer would be.
use super::headset::{EyePose, Headset, HiddenArea, Layers, Targets};
use glam::{Quat, Vec3};
use std::path::PathBuf;
use std::time::{Duration, Instant};

const PANEL: [u32; 2] = [1280, 720];
/// The eyes' distance apart (metres).
const IPD: f32 = 0.064;

pub struct SimHeadset {
    eye_size: [u32; 2],
    format: wgpu::TextureFormat,
    eyes: Vec<[wgpu::Texture; 2]>,
    panel: Option<wgpu::Texture>,
    pace: Option<Duration>,
    next: Instant,
    frame: u64,
    started: Instant,
    dump: Option<Dump>,
    pending: Option<[wgpu::Texture; 3]>,
    overlays: [Option<wgpu::Texture>; horizon_client_input::headset::OVERLAYS],
    overlay_pending: [Option<wgpu::Texture>; horizon_client_input::headset::OVERLAYS],
    device: Option<(wgpu::Device, wgpu::Queue)>,
    /// `HORIZON_SIM_HEAD_Y`: the head's height from its rest.
    head_y: f32,
}

struct Dump {
    dir: PathBuf,
    frame: u64,
    exit: bool,
}

/// Where a ray meets the room's TV panel: (u, v) from its top left and the distance, as
/// `Panel::hit_along` finds them (the screen is 16:9, facing +Z at identity).
fn tv_hit(
    tv: &horizon_client_input::headset::TvPanel,
    origin: Vec3,
    direction: Vec3,
) -> Option<([f32; 2], f32)> {
    let normal = tv.rot * Vec3::Z;
    let facing = direction.dot(normal);
    if facing >= -1e-4 {
        return None;
    }
    let t = (tv.centre - origin).dot(normal) / facing;
    if t <= 0.0 {
        return None;
    }
    let local = tv.rot.conjugate() * (origin + direction * t - tv.centre);
    let u = local.x / tv.width + 0.5;
    let v = 0.5 - local.y / (tv.width * 9.0 / 16.0);
    ((0.0..=1.0).contains(&u) && (0.0..=1.0).contains(&v)).then_some(([u, v], t))
}

/// Installs the simulated headset when `HORIZON_SIM_HEADSET=1` and no real one is there.
pub fn install_from_env() -> bool {
    if super::headset::installed() || std::env::var("HORIZON_SIM_HEADSET").as_deref() != Ok("1") {
        return false;
    }
    super::headset::install(Box::new(SimHeadset::from_env()));
    log::info!("Horizon: the simulated headset is installed (HORIZON_SIM_HEADSET=1)");
    true
}

/// `1024x768` as `[1024, 768]`.
fn parse_size(text: &str) -> Option<[u32; 2]> {
    let (w, h) = text.split_once(['x', 'X'])?;
    let size = [w.trim().parse().ok()?, h.trim().parse().ok()?];
    (size.iter().all(|&s| (16..=8192).contains(&s))).then_some(size)
}

impl SimHeadset {
    pub fn from_env() -> Self {
        let hz = std::env::var("HORIZON_SIM_HZ")
            .ok()
            .and_then(|v| v.parse::<f32>().ok())
            .unwrap_or(90.0);
        Self {
            eye_size: std::env::var("HORIZON_SIM_EYE")
                .ok()
                .and_then(|v| parse_size(&v))
                .unwrap_or([1024, 1024]),
            format: wgpu::TextureFormat::Rgba8Unorm,
            eyes: Vec::new(),
            panel: None,
            pace: (hz > 0.0).then(|| Duration::from_secs_f32(1.0 / hz)),
            next: Instant::now(),
            frame: 0,
            started: Instant::now(),
            dump: std::env::var_os("HORIZON_SIM_DUMP").map(|dir| Dump {
                dir: dir.into(),
                frame: std::env::var("HORIZON_SIM_DUMP_FRAME")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(240),
                exit: std::env::var("HORIZON_SIM_DUMP_EXIT").as_deref() == Ok("1"),
            }),
            pending: None,
            overlays: std::array::from_fn(|_| None),
            overlay_pending: std::array::from_fn(|_| None),
            device: None,
            head_y: std::env::var("HORIZON_SIM_HEAD_Y")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.0),
        }
    }

    /// The eyes of a Quest-like lens, the left looking a little right and the right a little
    /// left through the nose (tangents left, right, up, down).
    fn poses(&self) -> [EyePose; 2] {
        let pose = |side: f32| EyePose {
            orientation: Quat::IDENTITY,
            offset: Vec3::new(side * IPD / 2.0, self.head_y, 0.0),
            room: Vec3::new(side * IPD / 2.0, 1.6 + self.head_y, 0.0),
            tangents: if side < 0.0 {
                [-1.15, 0.95, 1.05, -1.1]
            } else {
                [-0.95, 1.15, 1.05, -1.1]
            },
        };
        [pose(-1.0), pose(1.0)]
    }

    fn image(&self, device: &wgpu::Device, size: [u32; 2], label: &str) -> wgpu::Texture {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
    }

    /// The images this and the next frames draw into: two sets, as a runtime's swapchain rotates.
    fn ensure_images(&mut self, device: &wgpu::Device) {
        if self.eyes.is_empty() {
            self.eyes = (0..3)
                .map(|_| {
                    [
                        self.image(device, self.eye_size, "sim left eye"),
                        self.image(device, self.eye_size, "sim right eye"),
                    ]
                })
                .collect();
            self.panel = Some(self.image(device, PANEL, "sim panel"));
            self.overlays = std::array::from_fn(|_| Some(self.image(device, PANEL, "sim overlay")));
        }
    }
}

/// A stand-in for a runtime's hidden-area mesh: a triangle cut off each corner of the lens, in
/// tangent space, `share` of the lens's width and height along each side.
fn corner_cuts(tangents: [f32; 4], share: f32) -> HiddenArea {
    let [left, right, up, down] = tangents;
    let (dx, dy) = ((right - left) * share, (up - down) * share);
    let mut area = HiddenArea::default();
    for (x, sx, y, sy) in [
        (left, 1.0, down, 1.0),
        (right, -1.0, down, 1.0),
        (right, -1.0, up, -1.0),
        (left, 1.0, up, -1.0),
    ] {
        let first = area.vertices.len() as u32;
        area.vertices
            .extend([[x, y], [x + sx * dx, y], [x, y + sy * dy]]);
        area.indices.extend([first, first + 1, first + 2]);
    }
    area
}

impl Headset for SimHeadset {
    fn adapter(&mut self) -> Result<(wgpu::Instance, wgpu::Adapter), String> {
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = wgpu::Backends::VULKAN;
        let instance = wgpu::Instance::new(descriptor);
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        }))
        .map_err(|e| format!("simulated headset: no Vulkan adapter: {e}"))?;
        Ok((instance, adapter))
    }

    fn open_device(
        &mut self,
        adapter: &wgpu::Adapter,
        desc: &wgpu::DeviceDescriptor<'_>,
    ) -> Result<(wgpu::Device, wgpu::Queue), String> {
        let (device, queue) = pollster::block_on(adapter.request_device(desc))
            .map_err(|e| format!("simulated headset: {e}"))?;
        self.device = Some((device.clone(), queue.clone()));
        Ok((device, queue))
    }

    fn format(&self) -> wgpu::TextureFormat {
        self.format
    }

    fn eye_size(&self) -> [u32; 2] {
        self.eye_size
    }

    fn menu_size(&self) -> [u32; 2] {
        PANEL
    }

    fn begin(&mut self, device: &wgpu::Device, layers: Layers) -> Targets {
        horizon_client_input::headset::set_active(true);
        horizon_client_input::headset::set_head(Some([0.0, 0.0]));
        horizon_client_input::headset::set_head_offset(Some(Vec3::new(0.0, self.head_y, 0.0)));
        // The head stands 1.7 m above the floor, facing -Z (what head-locked overlays hang from).
        // The controllers (`HORIZON_SIM_HANDS`) in the room's space too, as the runtime gives them.
        let rest = Vec3::new(0.0, 1.7, 0.0);
        let reach = std::env::var("HORIZON_SIM_HANDS")
            .ok()
            .and_then(|v| v.parse::<f32>().ok());
        let pitch = std::env::var("HORIZON_SIM_HAND_PITCH")
            .ok()
            .and_then(|v| v.parse::<f32>().ok())
            .unwrap_or(0.0);
        // `HORIZON_SIM_HAND_YAW=20` turns both controllers that many degrees to the left.
        let yaw = std::env::var("HORIZON_SIM_HAND_YAW")
            .ok()
            .and_then(|v| v.parse::<f32>().ok())
            .unwrap_or(0.0);
        let aim_rotation =
            Quat::from_rotation_y(yaw.to_radians()) * Quat::from_rotation_x(pitch.to_radians());
        let held = [-0.2f32, 0.2]
            .map(|x| reach.map(|reach| (rest + Vec3::new(x, -0.3, -reach), aim_rotation)));
        horizon_client_input::headset::set_room(Some(
            horizon_client_input::headset::RoomSnapshot {
                head: (rest + Vec3::Y * self.head_y, Quat::IDENTITY),
                aim: held,
                grip: held,
                rest,
                time_ms: self.started.elapsed().as_millis() as u64,
                pose_epoch: 0,
            },
        ));
        // `HORIZON_SIM_HANDS=0.4` holds both controllers that many metres ahead of the head and
        // 0.3 m below it (a hand pushed into a wall, to see what hides it).
        if let Some(reach) = std::env::var("HORIZON_SIM_HANDS")
            .ok()
            .and_then(|v| v.parse::<f32>().ok())
        {
            use horizon_client_input::headset::{HandPose, set_hands};
            // `HORIZON_SIM_HAND_PITCH=-45` tips both controllers' aim down by that many degrees.
            let hand = |x: f32| {
                let pose = (Vec3::new(x, -0.3, -reach), aim_rotation);
                Some(HandPose {
                    grip: pose,
                    aim: pose,
                })
            };
            set_hands([hand(-0.2), hand(0.2)]);
            // `HORIZON_SIM_TRIGGER=1` pulls the main trigger for 20 frames in every 60.
            if std::env::var("HORIZON_SIM_TRIGGER").as_deref() == Ok("1") {
                let pulled = horizon_client_input::headset::HandButtons {
                    trigger: if self.frame % 60 < 20 { 1.0 } else { 0.0 },
                    ..Default::default()
                };
                horizon_client_input::headset::set_buttons([pulled, Default::default()]);
            }
            // `HORIZON_SIM_GRIP=1` squeezes both grips: the hotbar wheel opens in front of the hand.
            if std::env::var("HORIZON_SIM_GRIP").as_deref() == Ok("1") {
                let squeezed = horizon_client_input::headset::HandButtons {
                    grip: 1.0,
                    ..Default::default()
                };
                horizon_client_input::headset::set_buttons([squeezed; 2]);
            }
        }
        // A scripted pointer on the menu panel (`HORIZON_SIM_POINTER`, `HORIZON_SIM_CLICK`).
        // `HORIZON_SIM_POINTER=aim` takes it from where the main controller's ray meets the
        // room's TV, as the runtime does (with the distance the ray is drawn to).
        let aimed = (std::env::var("HORIZON_SIM_POINTER").as_deref() == Ok("aim"))
            .then(|| {
                use horizon_client_input::headset::{main_hand, room_tv};
                let (tv, hand) = (room_tv()?, main_hand()?);
                let rest = Vec3::new(0.0, 1.7, 0.0);
                tv_hit(&tv, rest + hand.aim.0, hand.aim.1 * Vec3::NEG_Z)
            })
            .flatten();
        if layers != Layers::World
            && let Some(([u, v], distance)) = aimed.map(|(uv, d)| (uv, Some(d))).or_else(|| {
                std::env::var("HORIZON_SIM_POINTER").ok().and_then(|p| {
                    let (u, v) = p.split_once(',')?;
                    Some(([u.trim().parse().ok()?, v.trim().parse().ok()?], None))
                })
            })
        {
            use horizon_client_input::headset::{
                set_pointer, set_pointer_buttons, set_pointer_distance,
            };
            set_pointer(Some([u, v]));
            set_pointer_distance(distance);
            let down = std::env::var("HORIZON_SIM_CLICK").ok().is_some_and(|c| {
                c.split_once(':')
                    .and_then(|(a, b)| Some((a.parse::<u64>().ok()?, b.parse::<u64>().ok()?)))
                    .is_some_and(|(a, b)| (a..b).contains(&self.frame))
            });
            set_pointer_buttons([down, false]);
        }
        // The runtime holds a frame back to the display's rate (xrWaitFrame).
        if let Some(period) = self.pace {
            let waiting = Instant::now();
            let now = Instant::now();
            if self.next > now {
                std::thread::sleep(self.next - now);
            }
            self.next = self.next.max(now) + period;
            crate::vr_profile::record(crate::vr_profile::Stage::XrWait, waiting.elapsed());
        }
        self.ensure_images(device);
        let set = &self.eyes[(self.frame % 3) as usize];
        let (left, right) = (set[0].clone(), set[1].clone());
        let panel = self.panel.clone().expect("made with the eyes");
        let poses = self.poses();
        // The overlay panel, when the game wants one (`HORIZON_SIM_OVERLAY=1` asks for the
        // hotbar wheel to be held open, see `hold_wheel` in the game).
        self.overlay_pending = std::array::from_fn(|i| {
            horizon_client_input::headset::overlay_at(i)
                .and_then(|_| self.overlays[i].clone())
                .filter(|_| {
                    layers != Layers::Flat || i == horizon_client_input::headset::OVERLAY_KEYBOARD
                })
        });
        match layers {
            Layers::Flat => {
                self.pending = Some([left.clone(), right.clone(), panel.clone()]);
                Targets::Menu(panel, Some([(left, poses[0]), (right, poses[1])]))
            }
            _ => {
                self.pending = Some([left.clone(), right.clone(), panel.clone()]);
                Targets::Eyes([(left, poses[0]), (right, poses[1])], Some(panel), None)
            }
        }
    }

    fn overlay_image(&mut self, slot: usize) -> Option<wgpu::Texture> {
        self.overlay_pending[slot].clone()
    }

    fn hidden_areas(&mut self) -> Option<std::sync::Arc<[HiddenArea; 2]>> {
        // `HORIZON_XR_VISMASK=1` here too: the same switch as on a headset, with corner cuts
        // 30 % of a side long standing in for the runtime's mesh.
        if std::env::var("HORIZON_XR_VISMASK").as_deref() != Ok("1") {
            return None;
        }
        let poses = self.poses();
        Some(std::sync::Arc::new([
            corner_cuts(poses[0].tangents, 0.3),
            corner_cuts(poses[1].tangents, 0.3),
        ]))
    }

    fn end(&mut self) {
        self.frame += 1;
        let (Some(dump), Some(images), Some((device, queue))) =
            (&self.dump, self.pending.take(), self.device.as_ref())
        else {
            return;
        };
        if self.frame < dump.frame {
            return;
        }
        let overlays = std::mem::take(&mut self.overlay_pending);
        let overlay_names = [
            "overlay",
            "overlay_chat",
            "overlay_handles",
            "overlay_keyboard",
            "overlay_offhand",
        ];
        let named = images.iter().zip(["left", "right", "panel"]).chain(
            overlays
                .iter()
                .zip(overlay_names)
                .filter_map(|(image, name)| Some((image.as_ref()?, name))),
        );
        for (image, name) in named {
            let path = dump.dir.join(format!("{name}.png"));
            match write_png(device, queue, image, &path) {
                Ok(()) => log::info!("Horizon: simulated headset wrote {}", path.display()),
                Err(e) => log::warn!("Horizon: simulated headset {}: {e}", path.display()),
            }
        }
        // The room's TV: the panel where the quad layer would put it, over each eye.
        if let Some(tv) = horizon_client_input::headset::room_tv() {
            let poses = self.poses();
            for (i, name) in ["left_tv", "right_tv"].into_iter().enumerate() {
                let result = read_pixels(device, queue, &images[i])
                    .and_then(|(size, mut eye)| {
                        let (panel_size, panel) = read_pixels(device, queue, &images[2])?;
                        let (position, rotation) = super::room_scene::dev_view(&poses[i]);
                        lay_panel(
                            &mut eye, size, &poses[i], position, rotation, &tv, &panel, panel_size,
                        );
                        Ok((size, eye))
                    })
                    .and_then(|(size, eye)| {
                        save_png(&dump.dir.join(format!("{name}.png")), size, &eye)
                    });
                if let Err(e) = result {
                    log::warn!("Horizon: simulated headset {name}: {e}");
                }
            }
        }
        if dump.exit {
            crate::logging::flush();
            std::process::exit(0);
        }
    }
}

/// Reads `image` back: its size and 8-bit RGBA rows.
fn read_pixels(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    image: &wgpu::Texture,
) -> Result<([u32; 2], Vec<u8>), String> {
    let size = image.size();
    let row = (size.width * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("sim headset readback"),
        size: u64::from(row) * u64::from(size.height),
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
        size,
    );
    let (tx, rx) = std::sync::mpsc::channel();
    queue.submit([encoder.finish()]);
    buffer.slice(..).map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|e| e.to_string())?;
    rx.recv()
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    let data = buffer
        .slice(..)
        .get_mapped_range()
        .map_err(|e| e.to_string())?;
    let mut pixels = Vec::with_capacity((size.width * size.height * 4) as usize);
    for y in 0..size.height as usize {
        let start = y * row as usize;
        pixels.extend_from_slice(&data[start..start + size.width as usize * 4]);
    }
    drop(data);
    Ok(([size.width, size.height], pixels))
}

fn save_png(path: &std::path::Path, size: [u32; 2], pixels: &[u8]) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), size[0], size[1]);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .and_then(|mut w| w.write_image_data(pixels))
        .map_err(|e| e.to_string())
}

/// Reads `image` back and saves it as an 8-bit RGBA PNG.
fn write_png(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    image: &wgpu::Texture,
    path: &std::path::Path,
) -> Result<(), String> {
    let (size, pixels) = read_pixels(device, queue, image)?;
    save_png(path, size, &pixels)
}

/// Lays `panel` (RGBA) over the eye image `eye` where a quad layer at `tv` would show it, seen from
/// `position` turned by `rotation` through the lens `pose.tangents`: each pixel's ray meets the
/// panel's plane, and the panel's pixel there is blended in by its alpha.
#[allow(clippy::too_many_arguments)]
fn lay_panel(
    eye: &mut [u8],
    size: [u32; 2],
    pose: &EyePose,
    position: Vec3,
    rotation: Quat,
    tv: &horizon_client_input::headset::TvPanel,
    panel: &[u8],
    panel_size: [u32; 2],
) {
    let [left, right, up, down] = pose.tangents;
    let normal = tv.rot * Vec3::Z;
    let height = tv.width * panel_size[1] as f32 / panel_size[0] as f32;
    for y in 0..size[1] {
        for x in 0..size[0] {
            let tx = left + (right - left) * (x as f32 + 0.5) / size[0] as f32;
            let ty = up + (down - up) * (y as f32 + 0.5) / size[1] as f32;
            let direction = rotation * Vec3::new(tx, ty, -1.0);
            let facing = direction.dot(normal);
            if facing >= -1e-4 {
                continue;
            }
            let t = (tv.centre - position).dot(normal) / facing;
            if t <= 0.0 {
                continue;
            }
            let local = tv.rot.conjugate() * (position + direction * t - tv.centre);
            let (u, v) = (local.x / tv.width + 0.5, 0.5 - local.y / height);
            if !(0.0..1.0).contains(&u) || !(0.0..1.0).contains(&v) {
                continue;
            }
            let (px, py) = (
                ((u * panel_size[0] as f32) as u32).min(panel_size[0] - 1),
                ((v * panel_size[1] as f32) as u32).min(panel_size[1] - 1),
            );
            let p = &panel[((py * panel_size[0] + px) * 4) as usize..][..4];
            let at = ((y * size[0] + x) * 4) as usize;
            let alpha = f32::from(p[3]) / 255.0;
            for c in 0..3 {
                eye[at + c] = (f32::from(eye[at + c]) * (1.0 - alpha) + f32::from(p[c]) * alpha)
                    .round() as u8;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_corner_cuts_reach_the_lens_edges_so_the_mask_is_taken_as_tangent_space() {
        use super::super::vis_mask::{Space, space_of};
        let tangents = [-1.15, 0.95, 1.05, -1.1];
        let area = corner_cuts(tangents, 0.3);
        assert_eq!(area.indices.len(), 12);
        assert_eq!(space_of(&area.vertices, tangents), Some(Space::Tangent));
    }

    #[test]
    fn the_eye_size_is_width_x_height_within_limits() {
        assert_eq!(parse_size("1280x720"), Some([1280, 720]));
        assert_eq!(parse_size(" 64X64 "), Some([64, 64]));
        assert_eq!(parse_size("8x8"), None);
        assert_eq!(parse_size("1024"), None);
    }

    #[test]
    fn the_eyes_stand_a_pupil_distance_apart_looking_in_through_the_nose() {
        let poses = SimHeadset::from_env().poses();
        assert!((poses[1].offset.x - poses[0].offset.x - 0.064).abs() < 1e-6);
        // The left eye sees more to its left, the right eye more to its right.
        assert!(poses[0].tangents[0].abs() > poses[0].tangents[1].abs());
        assert!(poses[1].tangents[1].abs() > poses[1].tangents[0].abs());
    }
}
