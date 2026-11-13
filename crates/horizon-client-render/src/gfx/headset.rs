//! A VR headset the renderer draws for instead of a window (Meta Quest through
//! OpenXR, `horizon-xr`). The headset's runtime picks the GPU and the Vulkan
//! extensions, so it opens the device; each frame it hands the renderer one
//! image per eye with the eye's pose and lens, or the image of the menu panel
//! while a screen is open, and takes them back once the frame is submitted.
//!
//! Installed once, before the first renderer opens its device; nothing changes
//! for a flat screen when none is.
use super::View;
use glam::{Quat, Vec3};
use std::sync::Mutex;

/// One eye's place and lens for a frame, in the play space (metres, +Y up,
/// -Z ahead), from the runtime's view location.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EyePose {
    pub orientation: Quat,
    /// The eye relative to the point the game's camera stands for (the head's
    /// rest position), in the play space.
    pub offset: Vec3,
    /// The eye in the room's own space (the runtime's stage, floor level), for scenes that stand
    /// on the floor, like the title's menu room.
    pub room: Vec3,
    /// Tangents of the lens's left, right, up and down edges (left and down negative).
    pub tangents: [f32; 4],
}

/// What a frame shows on the headset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layers {
    /// The world in the eyes and the HUD on a panel of its own.
    World,
    /// A screen over the world: the world keeps being drawn in the eyes (a
    /// frozen world behind a menu would follow the head) and the screen goes
    /// onto a panel that stays where it opened.
    Screen,
    /// A screen with no world behind it (the title): the panel alone.
    Flat,
}

/// What a frame draws into.
pub enum Targets {
    /// The runtime wants no frame drawn now (session not visible, not running).
    Skip,
    /// The world, normally one image per eye (left, right) sized like `Headset::eye_size`.
    /// After an accepted `request_stereo_array`, both entries may instead be clones
    /// of one two-layer texture: layer 0 is left, layer 1 right. That image must
    /// only be drawn with multiview, never through the mono fallback.
    /// Also includes the panel's image (`Headset::menu_size`, drawn over a transparent
    /// background) with the HUD or the open screen, which the runtime shows in
    /// front of the world: a HUD drawn into the eyes' images would sit at the
    /// same place for both eyes, as if infinitely far, over blocks nearer than
    /// that.
    /// The last field is an image of the headset's own (the comfort
    /// vignette's) to fill once with these RGBA8 pixels.
    Eyes(
        [(wgpu::Texture, EyePose); 2],
        Option<wgpu::Texture>,
        Option<(wgpu::Texture, Vec<u8>)>,
    ),
    /// A screen with nothing behind it: the frame as a flat screen draws it,
    /// onto the panel's image (`Headset::menu_size`), and the eyes' images to
    /// clear under it.
    /// The eyes come with their poses: the title's panorama surrounds them.
    Menu(wgpu::Texture, Option<[(wgpu::Texture, EyePose); 2]>),
}

/// The part of an eye's image the lens never shows (`XR_KHR_visibility_mask`'s hidden-area
/// mesh): triangles in the lens's tangent space (or normalized device coordinates; see
/// `vis_mask::space_of`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HiddenArea {
    pub vertices: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
}

/// The headset side of the renderer.
pub trait Headset: Send {
    /// The instance and adapter the runtime asks for.
    fn adapter(&mut self) -> Result<(wgpu::Instance, wgpu::Adapter), String>;
    /// Opens the device on `adapter` with `desc` and the extensions the runtime
    /// needs, and starts the runtime's session on it.
    fn open_device(
        &mut self,
        adapter: &wgpu::Adapter,
        desc: &wgpu::DeviceDescriptor<'_>,
    ) -> Result<(wgpu::Device, wgpu::Queue), String>;
    /// The format the renderer draws in (the images' views; the runtime may
    /// store them as sRGB).
    fn format(&self) -> wgpu::TextureFormat;
    /// Pixels of one eye's image.
    fn eye_size(&self) -> [u32; 2];
    /// Pixels of the menu panel's image.
    fn menu_size(&self) -> [u32; 2];
    /// Ask for a native two-layer image only after stereo compatibility was checked.
    fn request_stereo_array(&mut self, _enabled: bool) {}
    /// Release acquired images normally, but submit no projection for a failed frame.
    fn discard_frame(&mut self) {}
    /// Waits for the runtime's next frame and acquires the images `layers` need.
    fn begin(&mut self, device: &wgpu::Device, layers: Layers) -> Targets;
    /// The frame drawn into `begin`'s images is submitted: hand them to the runtime.
    fn end(&mut self);
    /// The image (the menu panel's size, drawn over a transparent background) of overlay panel
    /// `slot` of the frame `begin` just made, when the game asked for one
    /// (`headset::overlay_at`) and the runtime gave it; taken once.
    fn overlay_image(&mut self, _slot: usize) -> Option<wgpu::Texture> {
        None
    }
    /// Each eye's hidden area (left, right) when the runtime offers it and the player turned the
    /// mask on; `None` otherwise (the default). Asked every frame: it may change.
    fn hidden_areas(&mut self) -> Option<std::sync::Arc<[HiddenArea; 2]>> {
        None
    }
}

/// Makes the headset when the renderer first asks for it.
pub type Factory = Box<dyn FnOnce() -> Result<Box<dyn Headset>, String> + Send>;

enum Slot {
    None,
    Pending(Factory),
    Ready(Box<dyn Headset>),
    Failed,
}

static HEADSET: Mutex<Slot> = Mutex::new(Slot::None);

/// Draw for `headset` from now on (once, before the first renderer starts).
pub fn install(headset: Box<dyn Headset>) {
    *lock() = Slot::Ready(headset);
}

/// Draw for the headset `make` returns, made when the renderer first needs it.
/// On Meta Quest the OpenXR instance must come after the activity's window:
/// its runtime ties the app to that window as the instance is made, and with
/// no window yet the system's launch transition never ends.
pub fn install_lazy(make: Factory) {
    *lock() = Slot::Pending(make);
}

fn lock() -> std::sync::MutexGuard<'static, Slot> {
    HEADSET
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Whether a headset is installed (or waits to be made).
pub fn installed() -> bool {
    matches!(*lock(), Slot::Pending(_) | Slot::Ready(_))
}

/// Runs `f` on the installed headset, making it first if it waits to be made.
pub(super) fn with<R>(f: impl FnOnce(&mut dyn Headset) -> R) -> Option<R> {
    let mut slot = lock();
    if matches!(*slot, Slot::Pending(_)) {
        let Slot::Pending(make) = std::mem::replace(&mut *slot, Slot::Failed) else {
            unreachable!()
        };
        match make() {
            Ok(headset) => *slot = Slot::Ready(headset),
            Err(e) => log::error!("Horizon: the headset failed to start: {e}"),
        }
    }
    match &mut *slot {
        Slot::Ready(h) => Some(f(h.as_mut())),
        _ => None,
    }
}

/// The world's rotation for a body facing `body_yaw` (Minecraft degrees): the
/// play space's -Z ahead turned onto the direction the game calls that yaw.
fn body_rotation(body_yaw: f32) -> Quat {
    Quat::from_rotation_y(std::f32::consts::PI - body_yaw.to_radians())
}

impl View {
    /// An eye of a headset, for a body facing `body_yaw`: the eye's own
    /// rotation and offset, and its asymmetric lens with reversed Z (near at
    /// depth 1, `far` at 0).
    pub fn eye(body_yaw: f32, eye: &EyePose, far: f32) -> Self {
        let body = body_rotation(body_yaw);
        let rotation = body * eye.orientation;
        let offset = body * eye.offset;
        let view =
            glam::Mat4::from_quat(rotation.conjugate()) * glam::Mat4::from_translation(-offset);
        let [left, right, up, down] = eye.tangents;
        Self {
            view,
            projection: eye_projection(eye.tangents, super::NEAR_PLANE, far),
            aspect: (right - left) / (up - down),
            tan_half_fov: (up - down) / 2.0,
        }
    }
}

/// An off-centre perspective for the lens's edge tangents, right-handed (-Z
/// ahead), depth 1 at `near` and 0 at `far` as `world_projection` maps it.
pub fn eye_projection(tangents: [f32; 4], near: f32, far: f32) -> glam::Mat4 {
    let [left, right, up, down] = tangents;
    let width = right - left;
    let height = up - down;
    let depth = near / (far - near);
    glam::Mat4::from_cols(
        glam::Vec4::new(2.0 / width, 0.0, 0.0, 0.0),
        glam::Vec4::new(0.0, 2.0 / height, 0.0, 0.0),
        glam::Vec4::new((right + left) / width, (up + down) / height, depth, -1.0),
        glam::Vec4::new(0.0, 0.0, far * depth, 0.0),
    )
}

/// One view whose frustum holds both eyes': the head's orientation (the
/// eyes' average) and the widest of their lenses, a little wider for the
/// eyes standing apart.
pub fn cull_view(body_yaw: f32, eyes: &[EyePose; 2], far: f32) -> View {
    let [a, b] = eyes;
    let orientation = a.orientation.slerp(b.orientation, 0.5);
    // The lens edges seen from the head: each eye's tangents turned into the
    // head's frame are close enough for the few degrees the eyes differ by.
    let margin = 0.05;
    let tangents = [
        a.tangents[0].min(b.tangents[0]) - margin,
        a.tangents[1].max(b.tangents[1]) + margin,
        a.tangents[2].max(b.tangents[2]) + margin,
        a.tangents[3].min(b.tangents[3]) - margin,
    ];
    let pose = EyePose {
        orientation,
        offset: (a.offset + b.offset) / 2.0,
        room: (a.room + b.room) / 2.0,
        tangents,
    };
    View::eye(body_yaw, &pose, far)
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec4;

    fn ndc(m: glam::Mat4, p: Vec3) -> Vec3 {
        let c = m * Vec4::new(p.x, p.y, p.z, 1.0);
        c.truncate() / c.w
    }

    #[test]
    fn a_symmetric_lens_matches_the_flat_screen_projection() {
        let fov = 90f32;
        let t = (fov.to_radians() / 2.0).tan();
        let ours = eye_projection([-t, t, t, -t], 0.05, 512.0);
        let flat = glam::Mat4::perspective_rh(fov.to_radians(), 1.0, 512.0, 0.05);
        for (a, b) in ours.to_cols_array().iter().zip(flat.to_cols_array()) {
            assert!((a - b).abs() < 1e-4, "{ours:?} {flat:?}");
        }
    }

    #[test]
    fn the_lens_edges_land_on_the_clip_edges_with_near_at_depth_one() {
        let m = eye_projection([-1.0, 0.5, 0.8, -1.2], 0.1, 100.0);
        // A point on the left edge, one metre ahead.
        let left = ndc(m, Vec3::new(-1.0, 0.0, -1.0));
        assert!((left.x + 1.0).abs() < 1e-5, "{left}");
        let top = ndc(m, Vec3::new(0.0, 0.8 * 3.0, -3.0));
        assert!((top.y - 1.0).abs() < 1e-5, "{top}");
        assert!((ndc(m, Vec3::new(0.0, 0.0, -0.1)).z - 1.0).abs() < 1e-5);
        assert!(ndc(m, Vec3::new(0.0, 0.0, -100.0)).z.abs() < 1e-5);
    }

    #[test]
    fn an_eye_looking_ahead_sees_what_the_screen_view_sees_at_the_same_yaw() {
        let t = 1.0;
        let eye = EyePose {
            orientation: Quat::IDENTITY,
            offset: Vec3::ZERO,
            room: Vec3::ZERO,
            tangents: [-t, t, t, -t],
        };
        for yaw in [0.0f32, 37.0, 90.0, -135.0] {
            let view = View::eye(yaw, &eye, 256.0);
            let screen = View::screen(yaw, 0.0, 90.0, 1.0, 256.0);
            for (a, b) in view
                .view
                .to_cols_array()
                .iter()
                .zip(screen.view.to_cols_array())
            {
                assert!(
                    (a - b).abs() < 1e-5,
                    "yaw {yaw}: {:?} {:?}",
                    view.view,
                    screen.view
                );
            }
        }
    }

    #[test]
    fn looking_up_in_the_headset_is_a_negative_minecraft_pitch() {
        let t = 1.0;
        let up = Quat::from_rotation_x(30f32.to_radians());
        let eye = EyePose {
            orientation: up,
            offset: Vec3::ZERO,
            room: Vec3::ZERO,
            tangents: [-t, t, t, -t],
        };
        let view = View::eye(0.0, &eye, 256.0);
        let screen = View::screen(0.0, -30.0, 90.0, 1.0, 256.0);
        for (a, b) in view
            .view
            .to_cols_array()
            .iter()
            .zip(screen.view.to_cols_array())
        {
            assert!((a - b).abs() < 1e-5);
        }
    }
}
