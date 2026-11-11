// SPDX-License-Identifier: LGPL-3.0-or-later
// Derived from Vivecraft: client_vr/provider/MCVR.java (processBindings, turning), assets/vivecraft/input/oculus_defaults.json (Touch bindings), gameplay/screenhandlers/GuiHandler.java (via horizon-vivecraft). Copyright (c) Vivecraft contributors; LGPL-3.0-or-later.
//! OpenXR behind the renderer's `gfx::headset::Headset` (Meta Quest; any
//! OpenXR runtime with `XR_KHR_vulkan_enable` should do).
//!
//! The runtime names the Vulkan instance and device extensions it needs and
//! the GPU to use; wgpu-hal opens both with those added (as `horizon-hdr`
//! adds its own), so wgpu owns the device and the session runs on its queue.
//! Each eye has a swapchain whose images wgpu draws into; while a screen is
//! open the frame goes onto a quad layer floating in front of the player
//! instead. The controllers come in as a pad and a pointer
//! (`horizon_client_input::headset`).
//!
//! The play space is the runtime's LOCAL space (recentred with the system's
//! own gesture). The game's camera stands for the head's rest position: the
//! eyes draw from their offsets to it, so leaning and the eyes' separation
//! show while walking in the room moves the rest position along.
use ash::vk::{self, Handle};
use glam::{Quat, Vec3};
use horizon_client_input::gamepad::{Button, PadState};
use horizon_client_input::headset::{self as input, FreeMove};
use horizon_client_render::gfx::headset::{EyePose, Headset, HiddenArea, Layers, Targets};
use horizon_vivecraft::gui::{
    HudInput, Placement, ScreenKind, place_hud, place_screen, static_screen, touched_hotbar_slot,
};
use horizon_vivecraft::player::{TurnInput, Turning};
use horizon_vivecraft::vr_data::RoomPose;
use openxr as xr;
use std::ffi::{CStr, CString};
use std::time::Duration;
use wgpu::hal::api::Vulkan;

mod actions;
mod dynres;
mod foveation;
mod layer_filter;
mod panel;
mod permission;
mod platform;
mod screen_grab;
mod tracking;

use actions::Controls;
use panel::Panel;

const VIEW_TYPE: xr::ViewConfigurationType = xr::ViewConfigurationType::PRIMARY_STEREO;
/// The menu panel's image: 16:9 at about the headset's sharpness 1.6 m away.
const MENU_SIZE: [u32; 2] = [1280, 720];
/// Seconds the game has to save and quit after the runtime ends the session.
const QUIT_GRACE_SECONDS: u64 = 15;

/// How far the head may wander from its rest position before the rest
/// position follows (metres): leaning shows, walking around the room doesn't
/// carry the camera through walls.
const LEAN: f32 = 0.35;

/// The comfort vignette's image (pixels a side) and its quad's width at full
/// strength, one metre ahead of the eyes (metres): its clear middle then
/// spans some 20 degrees each way and the edge of sight is black.
const VIGNETTE_SIZE: u32 = 256;
const VIGNETTE_WIDTH: f32 = 3.0;

/// The OpenXR side of the game, from instance to frame loop.
pub struct Quest {
    /// Kept loaded for as long as the instance lives.
    _entry: xr::Entry,
    instance: xr::Instance,
    system: xr::SystemId,
    blend: xr::EnvironmentBlendMode,
    eye_size: [u32; 2],
    /// The eyes' size before any dynamic scaling.
    base_eye_size: [u32; 2],
    recommended_eye_size: [u32; 2],
    maximum_eye_size: [u32; 2],
    configured_scale: f32,
    scale_change: horizon_client_input::headset_display::ScaleChange,
    scale_override: Option<f32>,
    resize_retry: Option<(f32, std::time::Instant)>,
    format: Option<(u32, wgpu::TextureFormat)>,
    /// Foveation asked for and possible so far (the runtime's extensions, then the GPU's); `Off`
    /// when not wanted or when something is missing (logged).
    fov_level: foveation::Level,
    foveated_stereo_images: bool,
    session: Option<Session>,
}

struct Session {
    /// The eyes' foveation profile, when it runs (`foveation.rs`).
    foveation: Option<foveation::Foveation>,
    /// The frame interval controller of the eyes' size, and when the last frame began.
    dynres: dynres::DynamicScale,
    last_begin: Option<std::time::Instant>,
    /// Hand and body tracking (`tracking.rs`).
    tracking: tracking::Tracking,
    session: xr::Session<xr::Vulkan>,
    waiter: xr::FrameWaiter,
    stream: xr::FrameStream<xr::Vulkan>,
    space: xr::Space,
    head_space: xr::Space,
    eyes: [Chain; 2],
    retired_eyes: Vec<[Chain; 2]>,
    stereo_array: Option<Chain>,
    retired_stereo: Vec<Chain>,
    stereo_array_failed: bool,
    request_stereo_array: bool,
    discard_world: bool,
    menu: Chain,
    /// The overlay panels' images (the menu panel's size), when the game wants one this frame
    /// (`input::overlay_at`): Visor's hotbar wheel hung in front of the hand, the chat, the
    /// handles of a screen.
    overlays: [Option<Chain>; input::OVERLAYS],
    overlay_retry: [Option<std::time::Instant>; input::OVERLAYS],
    overlay_format: (u32, wgpu::TextureFormat),
    overlay_images: [Option<wgpu::Texture>; input::OVERLAYS],
    /// The comfort vignette's image (filled once) and how far it closes in now,
    /// 0..1; a snap turn flashes it for a moment (`turn_flash`, seconds left).
    vignette: Chain,
    vignette_filled: bool,
    vignette_level: f32,
    turn_flash: f32,
    controls: Controls,
    state: xr::SessionState,
    running: bool,
    /// The head's rest position in the play space: the game's camera.
    rest: Option<Vec3>,
    pose_epoch: u64,
    recenter_at: Option<xr::Time>,
    panel: Option<Panel>,
    /// The HUD's panel over the world, and where it is this frame.
    hud_at: Option<Panel>,
    /// The last frame's predicted display time, for the HUD's glide.
    last_time: Option<xr::Time>,
    /// The refresh rate and performance levels were asked for (once running:
    /// asked before, Quest keeps the system's 120 Hz), and the rate asked.
    tuned: bool,
    rate: Option<f32>,
    /// Snap and smooth turning, after `MCVR.processBindings`.
    turning: Turning,
    /// The eyes' hidden areas (`XR_KHR_visibility_mask`) and whether the runtime said they changed
    /// (or none was read yet).
    hidden: Option<std::sync::Arc<[HiddenArea; 2]>>,
    hidden_stale: bool,
    /// The controllers' aim and grip difference is logged once.
    logged_hands: bool,
    started: std::time::Instant,
    quit_asked: bool,
    /// The off-hand controller points the menu's cursor (Visor's `VRCursorHandler`: the hand
    /// that last clicked), the main one otherwise.
    cursor_off: bool,
    /// Dragging and resizing a screen by its handles (`screen_grab`).
    grab: screen_grab::ScreenGrab,
    /// The strip under the screen's panel the handles are drawn on, in the room: its middle, turn,
    /// width and height; none when the screen has no handles.
    handles_at: Option<(Vec3, Quat, f32, f32)>,
    /// The two triggers as of the last menu frame, for the click that hands the cursor over.
    cursor_triggers: [bool; 2],
    /// Y (or Menu) was down last frame: it opens Visor's game menu once per press.
    menu_button: bool,
    keyboard_button: bool,
    pending: Option<Pending>,
}

/// A swapchain and its images as wgpu textures.
struct Chain {
    swapchain: xr::Swapchain<xr::Vulkan>,
    images: Vec<wgpu::Texture>,
    size: [u32; 2],
    /// A foveated swapchain's colour images, whose density maps the device was told about
    /// (taken back when the chain goes).
    density: Option<(wgpu::Device, Vec<vk::Image>)>,
}

impl Drop for Chain {
    fn drop(&mut self) {
        let Some((device, images)) = self.density.take() else {
            return;
        };
        // Safety: only reads what wgpu keeps alive with `device`. The views stay: the hal's
        // framebuffers made with them live as long as the device.
        if let Some(hal) = unsafe { device.as_hal::<Vulkan>() } {
            for image in images {
                // Safety: stops using the map for a colour image about to be destroyed.
                unsafe { hal.set_density_map(image, None) };
            }
        }
    }
}

/// The frame `begin` acquired images for.
enum Pending {
    Eyes {
        time: xr::Time,
        views: [xr::View; 2],
        /// The menu panel's image holds the HUD (or the screen) this frame.
        hud: bool,
        /// A screen over the world: the image goes on the menu's panel, which
        /// stays where it opened, not the HUD's.
        screen: bool,
        /// The vignette's image was acquired to be filled.
        filling: bool,
        /// One native two-layer swapchain instead of two mono swapchains.
        array: bool,
        /// Each overlay panel's image holds the game's overlay this frame.
        overlay: [bool; input::OVERLAYS],
    },
    Menu {
        time: xr::Time,
        /// The eyes' images, held for a projection layer under the panel: Meta's
        /// compositor keeps its loading screen up until a frame carries one.
        views: Option<[xr::View; 2]>,
        /// The keyboard overlay's image holds the game's keyboard this frame.
        keyboard: bool,
    },
    /// Begun with nothing to draw: ended with no layers.
    Empty { time: xr::Time },
}

fn xr_error(what: &str) -> impl Fn(xr::sys::Result) -> String + '_ {
    move |e| format!("{what}: {e}")
}

impl Quest {
    /// Loads the OpenXR loader, makes the instance and finds the headset.
    /// On Android the loader is told about the activity first.
    pub fn new() -> Result<Self, String> {
        // Safety: the loader's library stays loaded in `_entry` as long as `Quest` lives.
        let entry = unsafe { xr::Entry::load() }
            .map_err(|e| format!("no OpenXR loader (libopenxr_loader.so): {e}"))?;
        #[cfg(target_os = "android")]
        entry
            .initialize_android_loader()
            .map_err(xr_error("initialising the OpenXR loader"))?;
        let available = entry
            .enumerate_extensions()
            .map_err(xr_error("OpenXR extensions"))?;
        if !available.khr_vulkan_enable {
            return Err("the OpenXR runtime has no XR_KHR_vulkan_enable".into());
        }
        // What the device could do beyond what Horizon uses by default, reported for the tester
        // (docs/visor-port.md, "Performance switches").
        log::info!(
            "Horizon XR: optional extensions: foveation {}, space warp {}, visibility mask {}, \
             hand tracking {}, body tracking {} (full body {})",
            available.fb_foveation && available.fb_foveation_vulkan,
            available.fb_space_warp,
            available.khr_visibility_mask,
            available.ext_hand_tracking,
            available.fb_body_tracking,
            available.meta_body_tracking_full_body
        );
        let mut extensions = xr::ExtensionSet::default();
        extensions.khr_vulkan_enable = true;
        extensions.fb_display_refresh_rate = available.fb_display_refresh_rate;
        extensions.fb_composition_layer_settings = available.fb_composition_layer_settings;
        horizon_client_input::headset_display::update(|info| {
            info.super_resolution_supported = available.fb_composition_layer_settings
        });
        // Visor's hand joints and body trackers: only asked for here, used when the game wants them.
        extensions.ext_hand_tracking = available.ext_hand_tracking;
        extensions.fb_body_tracking = available.fb_body_tracking;
        extensions.meta_body_tracking_full_body =
            available.fb_body_tracking && available.meta_body_tracking_full_body;
        extensions.ext_performance_settings = available.ext_performance_settings;
        // The hidden-area mesh of each eye (`vis_mask`): only used when the player turns it on.
        extensions.khr_visibility_mask = available.khr_visibility_mask;
        // The HUD's opacity.
        extensions.khr_composition_layer_color_scale_bias =
            available.khr_composition_layer_color_scale_bias;
        // Foveation, when asked for and the runtime has all of it.
        let mut fov_level = foveation::wanted();
        if fov_level != foveation::Level::Off {
            let missing = foveation::missing_extensions(&available);
            if missing.is_empty() {
                foveation::enable(&mut extensions);
            } else {
                log::warn!(
                    "Horizon XR: foveation {fov_level:?} asked for, not used: the runtime lacks {missing:?}"
                );
                fov_level = foveation::Level::Off;
            }
        }
        #[cfg(target_os = "android")]
        {
            extensions.khr_android_create_instance = true;
        }
        let instance = entry
            .create_instance(
                &xr::ApplicationInfo {
                    application_name: "Horizon",
                    application_version: 1,
                    engine_name: "Horizon",
                    engine_version: 1,
                    api_version: xr::Version::new(1, 0, 0),
                },
                &extensions,
                &[],
            )
            .map_err(xr_error("creating the OpenXR instance"))?;
        if let Ok(props) = instance.properties() {
            log::info!(
                "Horizon XR: runtime {} {}",
                props.runtime_name,
                props.runtime_version
            );
        }
        let system = instance
            .system(xr::FormFactor::HEAD_MOUNTED_DISPLAY)
            .map_err(xr_error("finding the headset"))?;
        let blend = instance
            .enumerate_environment_blend_modes(system, VIEW_TYPE)
            .map_err(xr_error("blend modes"))?
            .first()
            .copied()
            .unwrap_or(xr::EnvironmentBlendMode::OPAQUE);
        let views = instance
            .enumerate_view_configuration_views(system, VIEW_TYPE)
            .map_err(xr_error("the eyes' views"))?;
        let view = views.first().ok_or("the headset has no views")?;
        let scale_override = std::env::var("HORIZON_XR_SCALE")
            .ok()
            .and_then(|s| s.parse::<f32>().ok())
            .filter(|s| (0.25..=2.0).contains(s));
        let scale = horizon_vivecraft::settings::render_scale(
            scale_override.unwrap_or_else(|| input::settings().float("renderScaleFactor")),
        );
        let recommended_eye_size = [
            view.recommended_image_rect_width,
            view.recommended_image_rect_height,
        ];
        let maximum_eye_size = [view.max_image_rect_width, view.max_image_rect_height];
        let eye_size = horizon_client_input::headset_display::eye_size(
            recommended_eye_size,
            maximum_eye_size,
            scale,
        );
        horizon_client_input::headset_display::update(|info| {
            info.recommended = recommended_eye_size;
            info.maximum = maximum_eye_size;
            info.active = eye_size;
            info.scale_override = scale_override;
        });
        log::info!(
            "Horizon XR: eyes {}x{} (recommended {}x{}, scale {scale})",
            eye_size[0],
            eye_size[1],
            view.recommended_image_rect_width,
            view.recommended_image_rect_height
        );
        Ok(Self {
            _entry: entry,
            instance,
            system,
            blend,
            eye_size,
            base_eye_size: eye_size,
            recommended_eye_size,
            maximum_eye_size,
            configured_scale: scale,
            scale_change: Default::default(),
            scale_override,
            resize_retry: None,
            format: None,
            fov_level,
            foveated_stereo_images: false,
            session: None,
        })
    }

    /// The runtime's space-separated list of Vulkan extension names, leaked:
    /// wgpu-hal takes `&'static CStr`s, and this runs once per process.
    fn extension_names(list: &str) -> Vec<&'static CStr> {
        list.split_ascii_whitespace()
            .filter_map(|name| CString::new(name).ok())
            .map(|name| &*Box::leak(name.into_boxed_c_str()))
            .collect()
    }

    fn start_session(&mut self, device: &wgpu::Device) -> Result<(), String> {
        let info = {
            // Safety: only reads handles wgpu keeps alive with `device`.
            let hal = unsafe { device.as_hal::<Vulkan>() }.ok_or("not a Vulkan device")?;
            xr::vulkan::SessionCreateInfo {
                instance: hal.shared_instance().raw_instance().handle().as_raw() as _,
                physical_device: hal.raw_physical_device().as_raw() as _,
                device: hal.raw_device().handle().as_raw() as _,
                queue_family_index: hal.queue_family_index(),
                queue_index: hal.queue_index(),
            }
        };
        // Safety: the handles are wgpu's, made with the extensions the runtime
        // asked for on the GPU it named; wgpu outlives the session (the
        // renderer's device lives until the process ends).
        let (session, waiter, stream) = unsafe {
            self.instance
                .create_session::<xr::Vulkan>(self.system, &info)
        }
        .map_err(xr_error("creating the OpenXR session"))?;
        // Vivecraft's trackers measure heights from the floor: the floor-level stage space, or
        // the local one (eye level at the last recentre) when the runtime has no stage.
        let space = session
            .create_reference_space(xr::ReferenceSpaceType::STAGE, xr::Posef::IDENTITY)
            .or_else(|e| {
                log::warn!("Horizon XR: no stage space ({e}); the local space instead");
                session.create_reference_space(xr::ReferenceSpaceType::LOCAL, xr::Posef::IDENTITY)
            })
            .map_err(xr_error("the play space"))?;
        let head_space = session
            .create_reference_space(xr::ReferenceSpaceType::VIEW, xr::Posef::IDENTITY)
            .map_err(xr_error("the head's space"))?;
        let formats = session
            .enumerate_swapchain_formats()
            .map_err(xr_error("swapchain formats"))?;
        let (format, wgpu_format) = pick_format(&formats)?;
        self.format = Some((format, wgpu_format));
        log::info!(
            "Horizon XR: swapchain format {format} drawn as {wgpu_format:?} (of {formats:?})"
        );
        // Foveation: the runtime's profile, then the eyes' swapchains made for it. Short of
        // either the eyes are made as always.
        let mut foveation = (self.fov_level != foveation::Level::Off)
            .then(|| foveation::Foveation::new(&session, self.fov_level))
            .and_then(|made| {
                made.inspect_err(|e| log::warn!("Horizon XR: foveation not used: {e}"))
                    .ok()
            });
        let chain = |size: [u32; 2], label: &str, fov: Option<&foveation::Foveation>| {
            Chain::new(&session, device, format, wgpu_format, size, label, fov)
        };
        let eyes = match (
            chain(self.eye_size, "left eye", foveation.as_ref()),
            chain(self.eye_size, "right eye", foveation.as_ref()),
        ) {
            (Ok(left), Ok(right)) => [left, right],
            (left, right) => {
                // A foveated swapchain the runtime refused: the eyes as always.
                if foveation.is_some() {
                    for e in [left.err(), right.err()].into_iter().flatten() {
                        log::warn!("Horizon XR: foveation not used: {e}");
                    }
                    foveation = None;
                    [
                        chain(self.eye_size, "left eye", None)?,
                        chain(self.eye_size, "right eye", None)?,
                    ]
                } else {
                    return Err(left.err().or(right.err()).unwrap_or_default());
                }
            }
        };
        let foveation = foveation.filter(|_| eyes.iter().all(|e| e.density.is_some()));
        if let Some(f) = &foveation {
            log::info!("Horizon XR: foveation {:?} on", f.level);
        }
        horizon_client_input::headset_display::update(|info| {
            info.foveated = foveation.is_some();
            info.foveated_stereo_capable = foveation.is_some() && self.foveated_stereo_images;
        });
        let menu = chain(MENU_SIZE, "menu panel", None)?;
        let overlays = std::array::from_fn(|_| None);
        let vignette = chain([VIGNETTE_SIZE; 2], "comfort vignette", None)?;
        let controls = Controls::new(&self.instance, &session)?;
        let tracking = tracking::Tracking::new(tracking::Support::of(&self.instance, self.system));
        self.session = Some(Session {
            foveation,
            retired_eyes: Vec::new(),
            stereo_array: None,
            retired_stereo: Vec::new(),
            stereo_array_failed: false,
            request_stereo_array: false,
            discard_world: false,
            dynres: dynres::DynamicScale::default(),
            last_begin: None,
            tracking,
            session,
            waiter,
            stream,
            space,
            head_space,
            eyes,
            menu,
            overlays,
            overlay_retry: [None; input::OVERLAYS],
            overlay_format: (format, wgpu_format),
            overlay_images: std::array::from_fn(|_| None),
            vignette,
            vignette_filled: false,
            vignette_level: 0.0,
            turn_flash: 0.0,
            controls,
            state: xr::SessionState::UNKNOWN,
            running: false,
            rest: None,
            pose_epoch: 0,
            recenter_at: None,
            panel: None,
            hud_at: None,
            last_time: None,
            tuned: false,
            rate: None,
            turning: Turning::default(),
            hidden: None,
            hidden_stale: true,
            logged_hands: false,
            started: std::time::Instant::now(),
            quit_asked: false,
            cursor_off: false,
            grab: Default::default(),
            handles_at: None,
            cursor_triggers: [false; 2],
            menu_button: false,
            keyboard_button: false,
            pending: None,
        });
        Ok(())
    }
}

impl Session {
    /// Allocate only overlays the player actually opens, then reuse them until session end.
    /// A runtime allocation failure backs off instead of retrying/logging at headset frame rate.
    fn acquire_overlay(&mut self, slot: usize, device: &wgpu::Device) -> Option<wgpu::Texture> {
        let now = std::time::Instant::now();
        if self.overlay_retry[slot].is_some_and(|retry| now < retry) {
            return None;
        }
        if self.overlays[slot].is_none() {
            match Chain::new(
                &self.session,
                device,
                self.overlay_format.0,
                self.overlay_format.1,
                MENU_SIZE,
                "Visor overlay",
                None,
            ) {
                Ok(chain) => self.overlays[slot] = Some(chain),
                Err(e) => {
                    log::warn!("Horizon XR: overlay {slot} could not be allocated: {e}");
                    self.overlay_retry[slot] = Some(now + Duration::from_secs(6));
                    return None;
                }
            }
        }
        match self.overlays[slot].as_mut()?.acquire() {
            Ok(image) => {
                self.overlay_retry[slot] = None;
                Some(image)
            }
            Err(e) => {
                log::warn!("Horizon XR: overlay {slot} could not be acquired: {e}");
                self.overlay_retry[slot] = Some(now + Duration::from_secs(6));
                None
            }
        }
    }

    /// Asks for the display rate Horizon is made to keep up with, and for
    /// sustained high CPU and GPU clocks (the system otherwise picks levels
    /// that leave a two-pass renderer short). Both are optional extensions.
    fn tune(&mut self, instance: &xr::Instance) {
        self.tuned = true;
        self.ask_refresh_rate(instance);
        let session = &self.session;
        let Some(perf) = instance.exts().ext_performance_settings else {
            log::info!(
                "Horizon XR: XR_EXT_performance_settings is not offered; the system picks the CPU and GPU levels"
            );
            return;
        };
        let level = platform::perf_level(std::env::var("HORIZON_XR_PERF").ok().as_deref());
        let mut failed = false;
        for domain in [
            xr::sys::PerfSettingsDomainEXT::CPU,
            xr::sys::PerfSettingsDomainEXT::GPU,
        ] {
            // Safety: the extension's own function, loaded with the instance,
            // on this live session.
            let result = unsafe {
                (perf.perf_settings_set_performance_level)(session.as_raw(), domain, level)
            };
            if result.into_raw() < 0 {
                failed = true;
                log::warn!("Horizon XR: performance level {level:?} for {domain:?}: {result}");
            }
        }
        if !failed {
            log::info!("Horizon XR: CPU and GPU performance level {level:?} set");
        }
    }
}

impl Session {
    /// Asks for the refresh rate the settings name (`HORIZON_XR_HZ`
    /// overrides it), the nearest the headset offers, when it changed.
    fn ask_refresh_rate(&mut self, instance: &xr::Instance) {
        if instance.exts().fb_display_refresh_rate.is_none() {
            return;
        }
        let wanted = std::env::var("HORIZON_XR_HZ")
            .ok()
            .and_then(|s| s.parse::<f32>().ok())
            .unwrap_or_else(|| input::settings().float("horizonRefreshRate"));
        if self.rate == Some(wanted) {
            return;
        }
        self.rate = Some(wanted);
        let session = &self.session;
        let rates = session
            .enumerate_display_refresh_rates()
            .unwrap_or_default();
        let current = session.get_display_refresh_rate().ok();
        horizon_client_input::headset_display::update(|info| {
            info.rates = rates.clone();
            info.current_hz = current;
        });
        log::info!(
            "Horizon XR: {}",
            platform::rate_report(&rates, current, wanted)
        );
        if let Some(rate) = platform::nearest_rate(&rates, wanted) {
            match session.request_display_refresh_rate(rate) {
                Ok(()) => log::info!("Horizon XR: refresh rate {rate} Hz asked for"),
                Err(e) => log::warn!("Horizon XR: refresh rate {rate} Hz: {e}"),
            }
        }
    }
}

/// An sRGB format, so the compositor reads the game's sRGB-encoded colours as
/// such; wgpu draws through a UNORM view of it (the images are created with
/// mutable formats). A UNORM-only runtime gets UNORM, a little bright.
/// Dynamic resolution is on: the `horizonDynamicResolution` setting or `HORIZON_XR_DYNRES=1`.
fn dynres_wanted() -> bool {
    input::settings().bool("horizonDynamicResolution")
        || std::env::var("HORIZON_XR_DYNRES").is_ok_and(|v| v == "1")
}

/// The visibility mask is on: `HORIZON_XR_VISMASK=1`/`0`, else the setting `horizonVisibilityMask`.
fn visibility_mask_wanted() -> bool {
    match std::env::var("HORIZON_XR_VISMASK").as_deref() {
        Ok("1") => true,
        Ok(_) => false,
        Err(_) => input::settings().bool("horizonVisibilityMask"),
    }
}

/// Both eyes' hidden-area meshes, or `None` (logged) when the runtime has none.
fn read_hidden_areas(session: &xr::Session<xr::Vulkan>) -> Option<std::sync::Arc<[HiddenArea; 2]>> {
    let area = |eye: u32| -> Option<HiddenArea> {
        let mask = session
            .get_visibility_mask_khr(
                VIEW_TYPE,
                eye,
                xr::VisibilityMaskTypeKHR::HIDDEN_TRIANGLE_MESH,
            )
            .map_err(|e| log::warn!("Horizon XR: visibility mask of eye {eye}: {e}"))
            .ok()?;
        Some(HiddenArea {
            vertices: mask.vertices.iter().map(|v| [v.x, v.y]).collect(),
            indices: mask.indices,
        })
    };
    let areas = [area(0)?, area(1)?];
    log::info!(
        "Horizon XR: visibility mask: {} and {} vertices, {} and {} indices (left, right)",
        areas[0].vertices.len(),
        areas[1].vertices.len(),
        areas[0].indices.len(),
        areas[1].indices.len()
    );
    Some(std::sync::Arc::new(areas))
}

fn pick_format(formats: &[u32]) -> Result<(u32, wgpu::TextureFormat), String> {
    let wanted = [
        (vk::Format::R8G8B8A8_SRGB, wgpu::TextureFormat::Rgba8Unorm),
        (vk::Format::B8G8R8A8_SRGB, wgpu::TextureFormat::Bgra8Unorm),
        (vk::Format::R8G8B8A8_UNORM, wgpu::TextureFormat::Rgba8Unorm),
        (vk::Format::B8G8R8A8_UNORM, wgpu::TextureFormat::Bgra8Unorm),
    ];
    wanted
        .into_iter()
        .find(|(f, _)| formats.contains(&(f.as_raw() as u32)))
        .map(|(f, w)| (f.as_raw() as u32, w))
        .ok_or_else(|| format!("no 8-bit RGBA swapchain format among {formats:?}"))
}

fn is_srgb(format: u32) -> bool {
    [vk::Format::R8G8B8A8_SRGB, vk::Format::B8G8R8A8_SRGB]
        .iter()
        .any(|f| f.as_raw() as u32 == format)
}

impl Chain {
    /// `foveation`: the eyes' swapchains are made with a fragment density map and given the
    /// profile; the menu's and the vignette's are not.
    fn new(
        session: &xr::Session<xr::Vulkan>,
        device: &wgpu::Device,
        format: u32,
        wgpu_format: wgpu::TextureFormat,
        size: [u32; 2],
        label: &str,
        foveation: Option<&foveation::Foveation>,
    ) -> Result<Self, String> {
        Self::new_layers(
            session,
            device,
            format,
            wgpu_format,
            size,
            label,
            foveation,
            1,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn new_layers(
        session: &xr::Session<xr::Vulkan>,
        device: &wgpu::Device,
        format: u32,
        wgpu_format: wgpu::TextureFormat,
        size: [u32; 2],
        label: &str,
        foveation: Option<&foveation::Foveation>,
        layers: u32,
    ) -> Result<Self, String> {
        let mut usage = xr::SwapchainUsageFlags::COLOR_ATTACHMENT
            | xr::SwapchainUsageFlags::SAMPLED
            | xr::SwapchainUsageFlags::TRANSFER_SRC
            | xr::SwapchainUsageFlags::TRANSFER_DST;
        if is_srgb(format) {
            usage |= xr::SwapchainUsageFlags::MUTABLE_FORMAT;
        }
        let (swapchain, raw, density) = match foveation {
            None => {
                let swapchain = session
                    .create_swapchain(&xr::SwapchainCreateInfo {
                        create_flags: xr::SwapchainCreateFlags::EMPTY,
                        usage_flags: usage,
                        format,
                        sample_count: 1,
                        width: size[0],
                        height: size[1],
                        face_count: 1,
                        array_size: layers,
                        mip_count: 1,
                    })
                    .map_err(xr_error("creating a swapchain"))?;
                let raw = swapchain
                    .enumerate_images()
                    .map_err(xr_error("the swapchain's images"))?;
                (swapchain, raw, Vec::new())
            }
            Some(fov) => {
                let info = xr::sys::SwapchainCreateInfo {
                    ty: xr::sys::SwapchainCreateInfo::TYPE,
                    next: std::ptr::null(),
                    create_flags: xr::SwapchainCreateFlags::EMPTY,
                    usage_flags: usage,
                    format: i64::from(format),
                    sample_count: 1,
                    width: size[0],
                    height: size[1],
                    face_count: 1,
                    array_size: layers,
                    mip_count: 1,
                };
                // Safety: a swapchain of this session, as the same call without foveation makes.
                let (swapchain, images, density) = unsafe { fov.create_swapchain(session, info) }?;
                let raw = images.iter().map(|i| i.image).collect();
                (swapchain, raw, density)
            }
        };
        let extent = wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: layers,
        };
        let hal = unsafe { device.as_hal::<Vulkan>() }.ok_or("not a Vulkan device")?;
        let mut foveated = Vec::new();
        let images = raw
            .into_iter()
            .enumerate()
            .map(|(i, image)| {
                let desc = wgpu::hal::TextureDescriptor {
                    label: Some(label),
                    size: extent,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu_format,
                    usage: wgpu::TextureUses::COLOR_TARGET
                        | wgpu::TextureUses::COPY_SRC
                        | wgpu::TextureUses::COPY_DST
                        | wgpu::TextureUses::RESOURCE,
                    memory_flags: wgpu::hal::MemoryFlags::empty(),
                    view_formats: Vec::new(),
                };
                // The runtime's density map for this image: the render passes into it take it.
                if let Some(map) = density.get(i) {
                    // Safety: the runtime's map of this swapchain's image, on this device.
                    match unsafe { foveation::density_view(hal.raw_device(), map.image, layers) } {
                        Ok(view) => {
                            // Safety: the extension and feature are on (checked at device
                            // creation), and the view lives as long as the device.
                            unsafe {
                                hal.set_density_map(vk::Image::from_raw(image), Some(view));
                            }
                            foveated.push(vk::Image::from_raw(image));
                        }
                        Err(e) => log::warn!("Horizon XR: {e}"),
                    }
                }
                // Safety: the runtime's image, of this size and a format whose
                // views may be `wgpu_format` (the same, or its sRGB twin made
                // mutable); the runtime owns it and its memory, which outlive
                // the texture (the swapchain lives as long as the session).
                let texture = unsafe {
                    hal.texture_from_raw(
                        vk::Image::from_raw(image),
                        &desc,
                        Some(Box::new(|| {})),
                        wgpu::hal::vulkan::TextureMemory::External,
                    )
                };
                (texture, desc)
            })
            .collect::<Vec<_>>();
        drop(hal);
        if let Some(map) = density.first() {
            log::info!(
                "Horizon XR: foveated {label}: {} density maps of {}x{} for images of {}x{}",
                foveated.len(),
                map.size[0],
                map.size[1],
                size[0],
                size[1]
            );
        }
        let images = images
            .into_iter()
            .map(|(texture, _)| {
                // Safety: made above from this device; first used as uninitialised.
                unsafe {
                    device.create_texture_from_hal::<Vulkan>(
                        texture,
                        &wgpu::TextureDescriptor {
                            label: Some(label),
                            size: extent,
                            mip_level_count: 1,
                            sample_count: 1,
                            dimension: wgpu::TextureDimension::D2,
                            format: wgpu_format,
                            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                                | wgpu::TextureUsages::COPY_SRC
                                | wgpu::TextureUsages::COPY_DST
                                | wgpu::TextureUsages::TEXTURE_BINDING,
                            view_formats: &[],
                        },
                        wgpu::TextureUses::UNINITIALIZED,
                    )
                }
            })
            .collect();
        Ok(Self {
            swapchain,
            images,
            size,
            density: (!foveated.is_empty()).then(|| (device.clone(), foveated)),
        })
    }

    fn acquire(&mut self) -> Result<wgpu::Texture, String> {
        let index = self
            .swapchain
            .acquire_image()
            .map_err(xr_error("acquiring a swapchain image"))?;
        self.swapchain
            .wait_image(xr::Duration::INFINITE)
            .map_err(xr_error("waiting for a swapchain image"))?;
        self.images
            .get(index as usize)
            .cloned()
            .ok_or_else(|| format!("swapchain image {index} out of range"))
    }

    fn release(&mut self) {
        if let Err(e) = self.swapchain.release_image() {
            log::warn!("Horizon XR: releasing a swapchain image: {e}");
        }
    }

    fn sub_image(&self) -> xr::SwapchainSubImage<'_, xr::Vulkan> {
        xr::SwapchainSubImage::new()
            .swapchain(&self.swapchain)
            .image_array_index(0)
            .image_rect(xr::Rect2Di {
                offset: xr::Offset2Di { x: 0, y: 0 },
                extent: xr::Extent2Di {
                    width: self.size[0] as i32,
                    height: self.size[1] as i32,
                },
            })
    }
}

/// A place given relative to the head's rest, in world metres: Vivecraft puts a device at
/// `origin + room * worldScale`, so its offset from the rest scales, and the standing head
/// height (the rest's) is lifted by the same factor.
fn world_offset(relative: Vec3, rest: Vec3) -> Vec3 {
    scaled_world_offset(relative, rest, input::world_scale())
}

fn scaled_world_offset(relative: Vec3, rest: Vec3, scale: f32) -> Vec3 {
    relative * scale + Vec3::new(0.0, (scale - 1.0) * rest.y, 0.0)
}

fn neutral_rest_height(
    settings: &horizon_vivecraft::settings::VrSettings,
    tracked_eye: f32,
) -> f32 {
    if settings.seated() {
        tracked_eye
    } else {
        horizon_vivecraft::player::player_height(settings) + 0.1
    }
}

pub(crate) fn quat(q: xr::Quaternionf) -> Quat {
    let q = Quat::from_xyzw(q.x, q.y, q.z, q.w);
    if q.length_squared() > 0.0 {
        q.normalize()
    } else {
        Quat::IDENTITY
    }
}
pub(crate) fn vec3(v: xr::Vector3f) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

impl Session {
    /// The runtime's events: the session's state, the instance going away.
    fn poll_events(&mut self, instance: &xr::Instance) {
        // `HORIZON_XR_QUIT_AFTER=SECONDS` (a developer's switch): the app asks
        // the runtime to end the session, as the system menu's Quit does.
        if self.running
            && !self.quit_asked
            && std::env::var("HORIZON_XR_QUIT_AFTER")
                .ok()
                .and_then(|s| s.parse::<f32>().ok())
                .is_some_and(|after| self.started.elapsed().as_secs_f32() > after)
        {
            self.quit_asked = true;
            log::info!("Horizon XR: HORIZON_XR_QUIT_AFTER: asking the runtime to end the session");
            if let Err(e) = self.session.request_exit() {
                log::warn!("Horizon XR: requesting the exit: {e}");
            }
        }
        let mut events = xr::EventDataBuffer::new();
        loop {
            let event = match instance.poll_event(&mut events) {
                Ok(Some(event)) => event,
                Ok(None) => break,
                Err(e) => {
                    log::warn!("Horizon XR: polling events: {e}");
                    break;
                }
            };
            match event {
                xr::Event::SessionStateChanged(change) => {
                    self.state = change.state();
                    log::info!("Horizon XR: session {:?}", self.state);
                    match self.state {
                        xr::SessionState::READY => match self.session.begin(VIEW_TYPE) {
                            Ok(_) => self.running = true,
                            Err(e) => log::error!("Horizon XR: beginning the session: {e}"),
                        },
                        xr::SessionState::STOPPING => {
                            self.running = false;
                            if let Err(e) = self.session.end() {
                                log::warn!("Horizon XR: ending the session: {e}");
                            }
                        }
                        xr::SessionState::EXITING | xr::SessionState::LOSS_PENDING => {
                            self.running = false;
                            input::set_active(false);
                            // The system closed the app (the headset's menu, Quit): the
                            // game saves the world and leaves, as the window's close
                            // would; if that takes too long, the process goes anyway.
                            log::info!(
                                "Horizon XR: the runtime ended the session; saving and quitting"
                            );
                            input::request_quit();
                            std::thread::spawn(|| {
                                std::thread::sleep(std::time::Duration::from_secs(
                                    QUIT_GRACE_SECONDS,
                                ));
                                log::warn!("Horizon XR: the game did not quit in time; exiting");
                                horizon_client_render::logging::flush();
                                std::process::exit(0);
                            });
                        }
                        _ => {}
                    }
                }
                // The runtime moved the display to another rate (after the request above, or
                // by itself, e.g. the system menu): the frame profile's fps is read against it.
                xr::Event::DisplayRefreshRateChangedFB(change) => {
                    horizon_client_input::headset_display::update(|info| {
                        info.current_hz = Some(change.to_display_refresh_rate())
                    });
                    log::info!(
                        "Horizon XR: display refresh rate {} -> {} Hz",
                        change.from_display_refresh_rate(),
                        change.to_display_refresh_rate()
                    );
                }
                // Thermal or power notifications of XR_EXT_performance_settings: a warning or
                // an impaired level means the chip is about to run slower than asked.
                xr::Event::PerfSettingsEXT(change) => log::warn!(
                    "Horizon XR: performance notification: {:?} {:?}: {:?} -> {:?}",
                    change.domain(),
                    change.sub_domain(),
                    change.from_level(),
                    change.to_level()
                ),
                // The lens's hidden area changed (IPD adjusted, the headset refit).
                xr::Event::VisibilityMaskChangedKHR(_) => self.hidden_stale = true,
                // Recentred (the system's gesture): the play space moved under the head.
                xr::Event::ReferenceSpaceChangePending(event) => {
                    self.recenter_at = Some(event.change_time());
                }
                xr::Event::InstanceLossPending(_) => {
                    log::warn!("Horizon XR: the OpenXR instance is going away; exiting");
                    horizon_client_render::logging::flush();
                    std::process::exit(0);
                }
                _ => {}
            }
        }
    }

    /// The head, the controllers and the pointer at `time`, for the game.
    fn sync_input(&mut self, time: xr::Time, menu: bool, title: bool) {
        let focused = self.state == xr::SessionState::FOCUSED;
        if self
            .recenter_at
            .is_some_and(|at| time.as_nanos() >= at.as_nanos())
        {
            self.recenter_at = None;
            self.rest = None;
            self.panel = None;
            self.grab.clear();
            self.pose_epoch = self.pose_epoch.wrapping_add(1);
        }
        let settings = input::settings();
        input::set_active(self.running);
        let head = self.head_space.locate(&self.space, time).ok().filter(|l| {
            l.location_flags.contains(
                xr::SpaceLocationFlags::ORIENTATION_VALID | xr::SpaceLocationFlags::POSITION_VALID,
            ) && vec3(l.pose.position).is_finite()
        });
        diag::head(head.is_some());
        // Visor's hand joints and body trackers, while the game wants them.
        if focused {
            self.tracking.sync(&self.session, &self.space, time);
        }
        let mut head_pose = head.map(|l| (quat(l.pose.orientation), vec3(l.pose.position)));
        // A developer's fake head height (`hy=` in the fake hands' file): the head is held at it.
        if let (Some((_, at)), Some(y)) = (head_pose.as_mut(), fake_state().and_then(|f| f.head_y))
        {
            at.y = y;
        }
        if let Some((orientation, position)) = head_pose {
            input::set_head(Some(input::angles(orientation)));
            // The game walked the player after the head (room scale): the
            // rest position follows, and the eyes stay where the head is.
            // The VR options' Reset Origin (`MCVR.resetPosition`): the room is centred on the head.
            // A recenter moves the horizontal origin, never teaches standing height from a crouch.
            let reset = input::take_reset_origin();
            if reset {
                self.rest = None;
                self.panel = None;
                self.grab.clear();
                input::take_walked();
                self.pose_epoch = self.pose_epoch.wrapping_add(1);
            }
            let neutral_y = neutral_rest_height(&settings, position.y);
            let rest = self
                .rest
                .get_or_insert(Vec3::new(position.x, neutral_y, position.z));
            // Auto/manual calibration may change after a seated launch. Keep the camera anchor in sync.
            rest.y = neutral_y;
            // The game walked in world metres: a room metre is `world_scale` of them.
            *rest += input::take_walked() / input::world_scale();
            // It follows the head once it wanders past leaning in any case.
            let off = position - *rest;
            let flat = Vec3::new(off.x, 0.0, off.z);
            if flat.length() > LEAN {
                *rest += flat - flat.normalize() * LEAN;
            }
            input::set_head_offset(Some(world_offset(position - *rest, *rest)));
        } else {
            input::set_head(None);
            input::set_head_offset(None);
        }
        // The room's TV: the title's panel hangs on it, in the stage's space, whatever the head does.
        if title && let Some(tv) = input::room_tv() {
            let tv = Panel::fixed(tv.centre, tv.rot, tv.width);
            if self.panel != Some(tv) {
                self.panel = Some(tv);
            }
        }
        // The panel opens ahead of the head, focused or not (the loading screen
        // shows before the system hands over the input).
        if menu
            && self.panel.is_none()
            && let Some((orientation, position)) = head_pose
        {
            // Vivecraft's `GuiHandler.onScreenChanged`: a metre ahead of the eyes, or over the
            // block whose screen it is, when that is near enough.
            let rest = self.rest.unwrap_or(position);
            // The block's offset is in world metres; the panel lives in room metres.
            let anchor = input::panel_anchor()
                .map(|at| at / input::world_scale() + rest)
                .filter(|at| {
                    settings.bool("guiAppearOverBlock") && (*at - position).length() < 3.0
                });
            let kind = match (anchor, input::menu_kind()) {
                (Some(source_room), _) => ScreenKind::Container { source_room },
                (None, input::MenuKind::Chat) => ScreenKind::Chat,
                (None, _) => ScreenKind::Normal,
            };
            let hmd = RoomPose {
                pos: position,
                rot: orientation,
            };
            // The title stands in its room (Visor's main menu): 2 m wide, 1.3 m up, 1.5 m ahead
            // of the middle of the play area, whichever way the head faces.
            let placed = if title {
                static_screen(None)
            } else {
                place_screen(&settings, kind, &hmd, &hmd, false)
            };
            let mut panel = Panel::placed(&placed);
            // A screen the player resized keeps its size (`OverlayOptionsResizing`), except a
            // container over its block (`overlayScale = optionsResizing.getResizingScale()` is
            // taken for any screen that is not the title's or the win screen's).
            let saved = settings.float("visorScreenScale");
            if !title && saved >= horizon_visor::overlay::MIN_SCALE {
                panel.set_width(saved.clamp(
                    horizon_visor::overlay::MIN_SCALE,
                    horizon_visor::overlay::MAX_SCALE,
                ));
            }
            self.panel = Some(panel);
        }
        let dt = self.last_time.map_or(0.0, |last| {
            (time.as_nanos() - last.as_nanos()) as f32 * 1e-9
        });
        self.last_time = Some(time);
        if !focused {
            input::set_pad(PadState::default());
            input::set_pointer(None);
            input::set_pointer_distance(None);
            input::set_pointer_buttons([false; 2]);
            input::set_hands([None; 2]);
            input::set_room(None);
            input::set_buttons(Default::default());
            input::set_off_use(false);
            self.keyboard_button = false;
            input::set_hud_slot(None);
            input::set_aim(head_pose.map(|(q, _)| input::angles(q)));
            self.hud_at = None;
            return;
        }
        let mut controls = self.controls.read(&self.session, &self.space, time);
        if let Some((orientation, position)) = head_pose {
            fake_hands(&mut controls, orientation, position);
        }
        let rest = self.rest.unwrap_or(Vec3::ZERO);
        let hands: [Option<input::HandPose>; 2] = std::array::from_fn(|i| {
            let (grip, aim) = controls.grip[i].zip(controls.aim[i])?;
            Some(input::HandPose {
                grip: (world_offset(grip.0 - rest, rest), grip.1),
                aim: (world_offset(aim.0 - rest, rest), aim.1),
            })
        });
        input::set_hands(hands);
        // The trackers (Vivecraft's) work on the runtime's room-space poses.
        input::set_room(head_pose.map(|head| input::RoomSnapshot {
            head: (head.1, head.0),
            aim: controls.aim,
            grip: controls.grip,
            rest,
            time_ms: self.started.elapsed().as_millis() as u64,
            pose_epoch: self.pose_epoch,
        }));
        if !self.logged_hands
            && let Some(hand) = hands[1]
        {
            self.logged_hands = true;
            let (grip, aim) = (hand.grip.1 * Vec3::NEG_Z, hand.aim.1 * Vec3::NEG_Z);
            log::info!(
                "Horizon XR: right controller's aim {:.1}° from its grip, {:.3} m apart",
                grip.dot(aim).clamp(-1.0, 1.0).acos().to_degrees(),
                (hand.grip.0 - hand.aim.0).length(),
            );
        }
        // The main hand is the right unless the hands are reversed.
        let (main, off) = if settings.left_handed() {
            (0, 1)
        } else {
            (1, 0)
        };
        // Standing, the main hand aims; seated (or untracked), the head.
        let aim = controls.aim[main]
            .filter(|_| !settings.seated())
            .map(|(_, q)| q)
            .or(head_pose.map(|(q, _)| q));
        input::set_aim(aim.map(input::angles));
        // The HUD (`GuiHandler.extractGui`) and, touched with the main hand, its hotbar
        // (`InteractiveHotbarModule`): the game picks the slot.
        let (hud, popup) =
            hud_placement(&settings, head_pose, controls.aim[off], controls.aim[main]);
        self.hud_at = hud.as_ref().map(Panel::placed);
        let hud_slot = match (hud, controls.aim[main]) {
            (Some(p), Some((hand, _)))
                if !menu && !settings.seated() && popup && settings.bool("vrTouchHotbar") =>
            {
                touched_hotbar_slot(&settings, p.pos, p.rot, p.local, p.scale, hand, false)
            }
            _ => None,
        };
        input::set_hud_slot(hud_slot);
        let sticks = [controls.move_stick, controls.turn_stick];
        let move_hand = if settings.enum_name("questMoveStick") == "MAIN" {
            main
        } else {
            off
        };
        let turn_hand = if settings.enum_name("questTurnStick") == "OFF" {
            off
        } else {
            main
        };
        // Menus keep the fixed layout, so an accidental unbind can always be repaired.
        let (move_stick, turn_stick) = if menu {
            (sticks[off], sticks[main])
        } else {
            (sticks[move_hand], sticks[turn_hand])
        };
        // The game walks along the player's yaw, which follows the aim: the stick's
        // forward is turned onto the off hand's or the head's direction instead.
        let forward = match settings.free_move_mode(false) {
            FreeMove::Hmd => None,
            _ => controls.aim[off].map(|(_, q)| q),
        }
        .or(head_pose.map(|(q, _)| q));
        let move_stick = match aim.zip(forward) {
            Some((aim, forward)) if !menu => {
                let turn = (input::angles(forward)[0] - input::angles(aim)[0]).to_radians();
                let [x, y] = move_stick;
                let (sin, cos) = turn.sin_cos();
                [x * cos + y * sin, -x * sin + y * cos]
            }
            _ => move_stick,
        };
        input::set_move_stick(move_stick);
        // In Visor's teleport mode the stick aims the arc (the game reads it from the pad it
        // gave back) and does not walk.
        let mut pad = PadState {
            left: if input::stick_teleports() && !menu {
                [0.0; 2]
            } else {
                move_stick
            },
            // The view follows the head and the aim the hand: the stick only turns
            // the body (`snap_turn`), except in menus where it scrolls.
            right: if menu { turn_stick } else { [0.0; 2] },
            ..Default::default()
        };
        // Vivecraft's Touch bindings: the main trigger attacks, A or the off
        // trigger uses (the game aims the off trigger's use from that hand),
        // B opens the radial menu (the game), X the inventory, Y and Menu pause; the move
        // stick's click jumps and pushing it all the way sprints, the turn
        // stick's click sneaks; the grips page the hotbar.
        // Each controller's buttons, physical left and right.
        let faces = [(controls.x, controls.y), (controls.a, controls.b)];
        let buttons: [input::HandButtons; 2] = std::array::from_fn(|i| input::HandButtons {
            trigger: controls.triggers[i],
            grip: controls.grips[i],
            primary: faces[i].0,
            secondary: faces[i].1,
            stick_click: controls.stick_clicks[i],
        });
        input::set_buttons(buttons);
        let logical = [buttons[main], buttons[off]];
        let value =
            |key| horizon_client_input::vr_bindings::value(&settings, key, logical, controls.menu);
        let held = |key| value(key) > 0.5;
        let off_primary = held("questBindOffUse");
        input::set_off_use(!menu && off_primary);
        let keyboard_button = held("questBindKeyboard");
        if keyboard_button && !self.keyboard_button {
            input::request_keyboard_toggle();
        }
        self.keyboard_button = keyboard_button;
        // Visor's Touch layout (`ActionLeftMouse`, `ActionRightMouse`, ...): the main trigger
        // attacks (left click), A uses with the main hand (right click) and X with the off hand,
        // B jumps, the main stick's click sneaks (`ActionShift`), Y opens the game menu
        // (`ActionMenu`), the grips open the hotbar wheels.
        if !menu {
            // The game may take a trigger for a gesture (touching a button,
            // drawing a bow); on the HUD the main one picks a slot instead.
            let [main_taken, off_taken] = input::triggers_captured();
            let attack_main = horizon_client_input::vr_bindings::uncaptured_value(
                &settings,
                "questBindAttack",
                logical,
                [main_taken, off_taken],
            );
            let attack_off = if input::two_handed() {
                horizon_client_input::vr_bindings::uncaptured_value(
                    &settings,
                    "questBindOffAttack",
                    logical,
                    [main_taken, off_taken],
                )
            } else {
                0.0
            };
            pad.triggers = [
                horizon_client_input::vr_bindings::uncaptured_value(
                    &settings,
                    "questBindUse",
                    logical,
                    [main_taken, off_taken],
                )
                .max(horizon_client_input::vr_bindings::uncaptured_value(
                    &settings,
                    "questBindOffUse",
                    logical,
                    [main_taken, off_taken],
                )),
                if hud_slot.is_some() {
                    0.0
                } else {
                    attack_main.max(attack_off)
                },
            ];
        }
        let tilt = (move_stick[0].powi(2) + move_stick[1].powi(2)).sqrt();
        // A fresh press of Y or Menu: it opens the game menu in play and closes a screen, and
        // the press that opened a menu does not close it again.
        let menu_button = if menu {
            faces[off].1 || controls.menu
        } else {
            held("questBindMenu") || controls.menu
        };
        let menu_pressed = menu_button && !self.menu_button;
        for (held, button) in [
            // B jumps in play (`GameActionJump`); the room's own jump joins through the trackers.
            (!menu && held("questBindJump"), Button::South),
            (!menu && held("questBindInventory"), Button::North),
            (!menu && held("questBindDrop"), Button::East),
            (!menu && held("questBindSwap"), Button::West),
            (!menu && held("questBindChat"), Button::DPadRight),
            // Y or Menu closes a screen; in play they open the game menu (below).
            (menu && menu_pressed, Button::Start),
            // The grips page the hotbar in menus; in play they are the hotbar wheels.
            (menu && controls.grips[off] > 0.6, Button::LeftBumper),
            (menu && controls.grips[main] > 0.6, Button::RightBumper),
            (
                !menu
                    && (held("questBindSprint")
                        || (settings.bool("autoSprint")
                            && tilt > settings.float("autoSprintThreshold"))),
                Button::LeftStick,
            ),
            (!menu && held("questBindSneak"), Button::RightStick),
        ] {
            pad.set(button, held);
        }
        self.menu_button = menu_button;
        if !menu && menu_pressed {
            input::request_game_menu();
        }
        input::set_pad(pad);
        let x = turn_stick[0];
        // Vivecraft's turning: a flick turns the world by the increment, or it turns smoothly.
        let increment = settings.float("vrWorldRotationIncrement");
        let turned = if menu {
            self.turning = Turning::default();
            0.0
        } else {
            self.turning.update(
                &settings,
                TurnInput {
                    axis: x,
                    dt: dt.min(0.1),
                },
            )
        };
        // The world rotates against the stick: the player turns the other way.
        if turned != 0.0 {
            input::snap_turn(-turned);
            if increment != 0.0 {
                self.turn_flash = 0.25;
            }
        }
        // The vignette closes in while the sticks move or turn the view.
        let turning = if menu || increment != 0.0 {
            0.0
        } else {
            x.abs()
        };
        self.turn_flash = (self.turn_flash - dt).max(0.0);
        let moving = if menu {
            0.0
        } else {
            let tilt = (move_stick[0].powi(2) + move_stick[1].powi(2)).sqrt();
            ((tilt - 0.15) / 0.85).clamp(0.0, 1.0)
        };
        let wanted = f32::from(u8::from(settings.bool("fovReduction")))
            * (1.0 - settings.float("fovReductionMin")).clamp(0.0, 1.0)
            * moving
                .max(turning)
                .max(if self.turn_flash > 0.0 { 1.0 } else { 0.0 });
        // Visor's in-block vignette: the view darkens as the eyes near a solid block.
        let wanted = if settings.bool("visorInBlockVignette") {
            wanted.max(input::in_block())
        } else {
            wanted
        };
        let rate = if wanted > self.vignette_level {
            8.0
        } else {
            3.0
        };
        self.vignette_level += (wanted - self.vignette_level) * (1.0 - (-rate * dt).exp());
        for (hand, pulse) in input::take_pulses().into_iter().enumerate() {
            if let Some((strength, seconds)) = pulse {
                self.controls.buzz(&self.session, hand, strength, seconds);
            }
        }
        if menu {
            // The handles under a screen: a trigger down on one holds it (`VROverlayGameScreen`).
            let mut holding = false;
            self.handles_at = None;
            if !title && let Some(panel) = self.panel.as_mut() {
                let up = self.grab.update(
                    panel,
                    controls.aim,
                    controls.grip,
                    [controls.triggers[0] > 0.5, controls.triggers[1] > 0.5],
                    input::world_scale(),
                );
                holding = up.dragging || up.resizing;
                if let Some(width) = up.resized {
                    input::set_screen_scale(width);
                }
                input::set_screen_handles(input::ScreenHandles {
                    visible: true,
                    dragging: up.dragging,
                    resizing: up.resizing,
                });
                self.handles_at = Some(panel.handle_strip());
            } else {
                self.grab.clear();
                input::set_screen_handles(input::ScreenHandles::default());
            }
            // The hand that clicks takes the cursor (`MouseClickHandler.processCursorUpdate`).
            let triggers = [controls.triggers[main] > 0.5, controls.triggers[off] > 0.5];
            let hits = |hand: usize| {
                self.panel
                    .as_ref()
                    .zip(controls.aim[hand])
                    .and_then(|(panel, (origin, q))| panel.hit_along(origin, q * Vec3::NEG_Z))
            };
            if triggers[1] && !self.cursor_triggers[1] && !triggers[0] && hits(off).is_some() {
                self.cursor_off = true;
            } else if triggers[0]
                && !self.cursor_triggers[0]
                && !triggers[1]
                && hits(main).is_some()
            {
                self.cursor_off = false;
            }
            self.cursor_triggers = triggers;
            let hand = if self.cursor_off { off } else { main };
            input::set_pointer_off(self.cursor_off);
            let at = hits(hand).filter(|_| !holding);
            input::set_pointer(at.map(|(at, _)| at));
            input::set_pointer_distance(at.map(|(_, d)| d));
            // The trigger is the left click; A (main hand) and X (off hand) the right.
            let right = faces[hand].0;
            input::set_pointer_buttons([
                triggers[usize::from(self.cursor_off)] && !holding && at.is_some(),
                right,
            ]);
        } else {
            // The next menu opens where the player then looks.
            self.panel = None;
            self.handles_at = None;
            self.grab.clear();
            input::set_screen_handles(input::ScreenHandles::default());
            input::set_pointer(None);
            input::set_pointer_distance(None);
            input::set_pointer_buttons([false; 2]);
        }
    }

    fn eye_poses(&self, views: &[xr::View; 2]) -> [EyePose; 2] {
        let rest = self.rest.unwrap_or(Vec3::ZERO);
        views.map(|v| EyePose {
            orientation: quat(v.pose.orientation),
            offset: world_offset(vec3(v.pose.position) - rest, rest),
            room: vec3(v.pose.position),
            tangents: [
                v.fov.angle_left.tan(),
                v.fov.angle_right.tan(),
                v.fov.angle_up.tan(),
                v.fov.angle_down.tan(),
            ],
        })
    }
}

impl Headset for Quest {
    fn adapter(&mut self) -> Result<(wgpu::Instance, wgpu::Adapter), String> {
        // Asked before the session: the runtime requires the query.
        let requirements = self
            .instance
            .graphics_requirements::<xr::Vulkan>(self.system)
            .map_err(xr_error("the runtime's Vulkan requirements"))?;
        log::info!(
            "Horizon XR: Vulkan {}..{}",
            requirements.min_api_version_supported,
            requirements.max_api_version_supported
        );
        let wanted = self
            .instance
            .vulkan_legacy_instance_extensions(self.system)
            .map_err(xr_error("the runtime's Vulkan instance extensions"))?;
        log::info!("Horizon XR: Vulkan instance extensions: {wanted}");
        let names = Self::extension_names(&wanted);
        let desc = wgpu::hal::InstanceDescriptor {
            name: "Horizon",
            flags: wgpu::InstanceFlags::from_build_config(),
            memory_budget_thresholds: Default::default(),
            backend_options: Default::default(),
            telemetry: None,
            display: None,
        };
        // Safety: wgpu-hal's own instance creation, with the runtime's extensions added.
        let hal = unsafe {
            wgpu::hal::vulkan::Instance::init_with_callback(
                &desc,
                Some(Box::new(
                    move |args: wgpu::hal::vulkan::CreateInstanceCallbackArgs| {
                        for name in &names {
                            if !args.extensions.contains(name) {
                                args.extensions.push(name);
                            }
                        }
                    },
                )),
            )
        }
        .map_err(|e| format!("creating the Vulkan instance: {e}"))?;
        let raw_instance = hal.shared_instance().raw_instance().handle();
        // Safety: the instance made just now, which `hal` keeps alive.
        let physical = unsafe {
            self.instance
                .vulkan_graphics_device(self.system, raw_instance.as_raw() as _)
        }
        .map_err(xr_error("the runtime's GPU"))?;
        let physical = vk::PhysicalDevice::from_raw(physical as u64);
        let exposed = hal
            .expose_adapter(physical)
            .ok_or("wgpu cannot use the runtime's GPU")?;
        // Safety: the adapter is of this very instance.
        let instance = unsafe { wgpu::Instance::from_hal::<Vulkan>(hal) };
        let adapter = unsafe { instance.create_adapter_from_hal(exposed) };
        Ok((instance, adapter))
    }

    fn open_device(
        &mut self,
        adapter: &wgpu::Adapter,
        desc: &wgpu::DeviceDescriptor<'_>,
    ) -> Result<(wgpu::Device, wgpu::Queue), String> {
        let wanted = self
            .instance
            .vulkan_legacy_device_extensions(self.system)
            .map_err(xr_error("the runtime's Vulkan device extensions"))?;
        log::info!("Horizon XR: Vulkan device extensions: {wanted}");
        log::info!(
            "Horizon XR: wgpu multiview {}",
            adapter.features().contains(wgpu::Features::MULTIVIEW)
        );
        let names = Self::extension_names(&wanted);
        // The GPU's fragment density map, for foveation.
        let density = if self.fov_level == foveation::Level::Off {
            false
        } else {
            match foveation::gpu_supports(adapter) {
                Ok(()) => true,
                Err(e) => {
                    log::warn!("Horizon XR: foveation not used: {e}");
                    self.fov_level = foveation::Level::Off;
                    false
                }
            }
        };
        let non_subsampled = density && foveation::non_subsampled_images(adapter);
        self.foveated_stereo_images = non_subsampled;
        log::info!("Horizon XR: foveated multiview non-subsampled image support {non_subsampled}");
        let open = {
            // Safety: only opened from here; dropped before wgpu takes the adapter back.
            let hal = unsafe { adapter.as_hal::<Vulkan>() }.ok_or("not a Vulkan adapter")?;
            for name in &names {
                if !hal.physical_device_capabilities().supports_extension(name) {
                    return Err(format!("the GPU lacks {name:?}, which the runtime needs"));
                }
            }
            // Safety: what wgpu's own request_device does, with the runtime's extensions.
            unsafe {
                hal.open_with_callback(
                    desc.required_features,
                    &desc.required_limits,
                    &desc.memory_hints,
                    Some(Box::new(
                        move |args: wgpu::hal::vulkan::CreateDeviceCallbackArgs| {
                            for name in &names {
                                if !args.extensions.contains(name) {
                                    args.extensions.push(name);
                                }
                            }
                            if density {
                                if !args.extensions.contains(&foveation::DEVICE_EXTENSION) {
                                    args.extensions.push(foveation::DEVICE_EXTENSION);
                                }
                                // The feature joins the head of the create info's chain.
                                let info = *args.create_info;
                                *args.create_info =
                                    info.push_next(foveation::device_feature(non_subsampled));
                            }
                        },
                    )),
                )
            }
            .map_err(|e| format!("opening the Vulkan device: {e}"))?
        };
        // Safety: opened from this adapter with exactly `desc`'s features.
        let (device, queue) = unsafe { adapter.create_device_from_hal(open, desc) }
            .map_err(|e| format!("wgpu device: {e}"))?;
        self.start_session(&device)?;
        Ok((device, queue))
    }

    fn format(&self) -> wgpu::TextureFormat {
        // Until the session lists its formats: what nearly every runtime offers.
        self.format
            .map_or(wgpu::TextureFormat::Rgba8Unorm, |(_, format)| format)
    }

    fn eye_size(&self) -> [u32; 2] {
        self.eye_size
    }

    fn menu_size(&self) -> [u32; 2] {
        MENU_SIZE
    }

    fn begin(&mut self, device: &wgpu::Device, layers: Layers) -> Targets {
        let menu = layers != Layers::World;
        let Some(s) = self.session.as_mut() else {
            return Targets::Skip;
        };
        s.poll_events(&self.instance);
        if s.running && !s.tuned {
            s.tune(&self.instance);
        } else if s.running {
            s.ask_refresh_rate(&self.instance);
        }
        if !s.running {
            input::set_active(false);
            // Not shown yet (or any more): don't spin the CPU waiting.
            std::thread::sleep(Duration::from_millis(10));
            return Targets::Skip;
        }
        let waiting = std::time::Instant::now();
        let state = s.waiter.wait();
        diag::waited(0, waiting.elapsed());
        let state = match state {
            Ok(state) => state,
            Err(e) => {
                log::warn!("Horizon XR: waiting for a frame: {e}");
                return Targets::Skip;
            }
        };
        let waiting = std::time::Instant::now();
        let begun = s.stream.begin();
        diag::waited(1, waiting.elapsed());
        if let Err(e) = begun {
            log::warn!("Horizon XR: beginning a frame: {e}");
            return Targets::Skip;
        }
        if (!s.retired_eyes.is_empty() || !s.retired_stereo.is_empty())
            && device
                .poll(wgpu::PollType::Poll)
                .is_ok_and(|status| status.is_queue_empty())
        {
            s.retired_eyes.clear();
            s.retired_stereo.clear();
        }
        let desired_scale = horizon_vivecraft::settings::render_scale(
            self.scale_override
                .unwrap_or_else(|| input::settings().float("renderScaleFactor")),
        );
        let scale_ready = self.scale_change.ready(
            desired_scale,
            self.configured_scale,
            std::time::Instant::now(),
        );
        if layers != Layers::Flat
            && scale_ready
            && desired_scale.is_finite()
            && desired_scale > 0.0
            && (desired_scale != self.configured_scale
                || (!dynres_wanted() && self.eye_size != self.base_eye_size))
            && self
                .resize_retry
                .is_none_or(|(value, at)| value != desired_scale || at.elapsed().as_secs() >= 6)
            && let Some((format, wgpu_format)) = self.format
        {
            let size = horizon_client_input::headset_display::eye_size(
                self.recommended_eye_size,
                self.maximum_eye_size,
                desired_scale,
            );
            let make = || {
                let left = Chain::new(
                    &s.session,
                    device,
                    format,
                    wgpu_format,
                    size,
                    "left eye",
                    s.foveation.as_ref(),
                )?;
                let right = Chain::new(
                    &s.session,
                    device,
                    format,
                    wgpu_format,
                    size,
                    "right eye",
                    s.foveation.as_ref(),
                )?;
                Ok::<_, String>([left, right])
            };
            match make() {
                Ok(eyes) => {
                    // Old images may still be referenced by submitted GPU work. This wait is
                    // only for an explicit quality change, never in the steady frame loop.
                    match device.poll(wgpu::PollType::Wait {
                        submission_index: None,
                        timeout: Some(Duration::from_secs(5)),
                    }) {
                        Ok(_) => {
                            s.eyes = eyes;
                            self.eye_size = size;
                            self.base_eye_size = size;
                            self.configured_scale = desired_scale;
                            self.resize_retry = None;
                            s.dynres = Default::default();
                            horizon_client_input::headset_display::update(|info| {
                                info.active = size
                            });
                            log::info!(
                                "Horizon XR: selected eye size {}x{}, scale {}",
                                size[0],
                                size[1],
                                desired_scale
                            );
                        }
                        Err(e) => {
                            self.resize_retry = Some((desired_scale, std::time::Instant::now()));
                            log::warn!("Horizon XR: eye resize postponed: {e}");
                        }
                    }
                }
                Err(e) => {
                    self.resize_retry = Some((desired_scale, std::time::Instant::now()));
                    log::warn!("Horizon XR: eye resize failed, retaining old size: {e}");
                }
            }
        }
        // Dynamic resolution (off by default): late frames shrink the eyes' images a step, a long
        // quiet stretch grows them back. Only in play: a screen's frames say nothing.
        let now = std::time::Instant::now();
        let interval = s.last_begin.replace(now).map(|t| (now - t).as_secs_f32());
        if let (true, Some(interval), false, Some((format, wgpu_format))) =
            (dynres_wanted(), interval, menu, self.format)
        {
            let period = state.predicted_display_period.as_nanos() as f32 * 1e-9;
            let previous = s.dynres.clone();
            if s.retired_eyes.len() < 2
                && let Some(scale) = s.dynres.frame(
                    interval,
                    period,
                    horizon_client_input::headset_display::recent_gpu_seconds(),
                )
            {
                let size = self
                    .base_eye_size
                    .map(|side| dynres::scaled_side(side, scale));
                let fov = s.foveation.as_ref();
                let made = Chain::new(
                    &s.session,
                    device,
                    format,
                    wgpu_format,
                    size,
                    "left eye",
                    fov,
                )
                .and_then(|left| {
                    Chain::new(
                        &s.session,
                        device,
                        format,
                        wgpu_format,
                        size,
                        "right eye",
                        fov,
                    )
                    .map(|right| [left, right])
                });
                match made {
                    Ok(eyes) => {
                        log::info!(
                            "Horizon XR: dynamic resolution {scale:.2}: eyes {}x{}",
                            size[0],
                            size[1]
                        );
                        s.retired_eyes.push(std::mem::replace(&mut s.eyes, eyes));
                        self.eye_size = size;
                        horizon_client_input::headset_display::update(|info| info.active = size);
                    }
                    Err(e) => {
                        s.dynres.reject(previous);
                        log::warn!("Horizon XR: dynamic resolution failed: {e}");
                    }
                }
            }
        }
        s.discard_world = false;
        let wants_array = self.foveated_stereo_images
            && s.request_stereo_array
            && layers != Layers::Flat
            && s.foveation.is_some()
            && !s.stereo_array_failed;
        if wants_array
            && s.stereo_array
                .as_ref()
                .is_none_or(|chain| chain.size != self.eye_size)
        {
            // Keep old images until submitted work has finished, as for mono resize.
            if s.retired_stereo.len() < 2 {
                let made = self
                    .format
                    .ok_or_else(|| "no stereo format".to_string())
                    .and_then(|(format, gpu_format)| {
                        Chain::new_layers(
                            &s.session,
                            device,
                            format,
                            gpu_format,
                            self.eye_size,
                            "foveated stereo eyes",
                            s.foveation.as_ref(),
                            2,
                        )
                    })
                    .and_then(|chain| {
                        if chain
                            .density
                            .as_ref()
                            .is_some_and(|(_, maps)| maps.len() == chain.images.len())
                        {
                            Ok(chain)
                        } else {
                            Err("incomplete stereo density maps".to_string())
                        }
                    });
                match made {
                    Ok(chain) => {
                        if let Some(old) = s.stereo_array.replace(chain) {
                            s.retired_stereo.push(old);
                        }
                        log::info!("Horizon XR: experimental native foveated stereo array ready");
                    }
                    Err(error) => {
                        s.stereo_array_failed = true;
                        horizon_client_input::headset_display::update(|info| {
                            info.foveated_stereo_capable = false
                        });
                        log::warn!(
                            "Horizon XR: foveated multiview unavailable, keeping per-eye rendering: {error}"
                        );
                    }
                }
            }
        }
        let use_array = wants_array
            && !s.stereo_array_failed
            && s.stereo_array
                .as_ref()
                .is_some_and(|chain| chain.size == self.eye_size);
        let time = state.predicted_display_time;
        s.sync_input(time, menu, layers == Layers::Flat);
        let views = match s.session.locate_views(VIEW_TYPE, time, &s.space) {
            Ok((_, views)) if views.len() >= 2 && state.should_render => [views[0], views[1]],
            Ok(_) => {
                s.pending = Some(Pending::Empty { time });
                self.end();
                return Targets::Skip;
            }
            Err(e) => {
                log::warn!("Horizon XR: locating the eyes: {e}");
                s.pending = Some(Pending::Empty { time });
                self.end();
                return Targets::Skip;
            }
        };
        let waiting = std::time::Instant::now();
        let acquired = if layers == Layers::Flat {
            s.menu.acquire().map(|image| {
                // Cleared under the panel: the compositor wants a projection layer.
                let eyes = s.eyes[0].acquire().and_then(|left| {
                    let right = s.eyes[1].acquire().inspect_err(|_| s.eyes[0].release())?;
                    Ok([left, right])
                });
                let poses = s.eye_poses(&views);
                let views = eyes.as_ref().ok().map(|_| views);
                // Visor's keyboard over the title's screens too, on its own swapchain.
                let keyboard = input::overlay_at(input::OVERLAY_KEYBOARD)
                    .is_some()
                    .then(|| s.acquire_overlay(input::OVERLAY_KEYBOARD, device))
                    .flatten();
                s.overlay_images = std::array::from_fn(|i| {
                    keyboard.clone().filter(|_| i == input::OVERLAY_KEYBOARD)
                });
                s.pending = Some(Pending::Menu {
                    time,
                    views,
                    keyboard: keyboard.is_some(),
                });
                Targets::Menu(
                    image,
                    eyes.ok()
                        .map(|[left, right]| [(left, poses[0]), (right, poses[1])]),
                )
            })
        } else {
            let screen = layers == Layers::Screen;
            let poses = s.eye_poses(&views);
            let eye_images = if use_array {
                let acquired = s.stereo_array.as_mut().unwrap().acquire();
                if acquired.is_err() {
                    s.stereo_array_failed = true;
                    horizon_client_input::headset_display::update(|info| {
                        info.foveated_stereo_capable = false
                    });
                }
                acquired.map(|image| [image.clone(), image])
            } else {
                s.eyes[0].acquire().and_then(|left| {
                    let right = s.eyes[1].acquire().inspect_err(|_| s.eyes[0].release())?;
                    Ok([left, right])
                })
            };
            eye_images.map(|[left, right]| {
                // The HUD goes on the menu panel's swapchain, shown on its own panel.
                let shown = if screen {
                    s.panel.is_some()
                } else {
                    s.hud_at.is_some()
                };
                let hud = shown
                    .then(|| {
                        s.menu
                            .acquire()
                            .inspect_err(|e| log::warn!("Horizon XR: {e}"))
                            .ok()
                    })
                    .flatten();
                // The overlay (the hotbar wheel) goes on a swapchain of its own.
                // The chat and the handles of a screen go on their own, so do they: the screen's
                // handles also in a screen.
                let overlay: [Option<wgpu::Texture>; input::OVERLAYS] = std::array::from_fn(|i| {
                    let wanted = if i == input::OVERLAY_HANDLES {
                        s.handles_at.is_some() && screen
                    } else if i == input::OVERLAY_KEYBOARD {
                        // Visor's keyboard hangs under a screen as well as in the world.
                        input::overlay_at(i).is_some()
                    } else {
                        !screen && input::overlay_at(i).is_some()
                    };
                    wanted.then(|| s.acquire_overlay(i, device)).flatten()
                });
                for (i, image) in overlay.iter().enumerate() {
                    if image.is_some() {
                        static LOGGED: [std::sync::Once; input::OVERLAYS] =
                            [const { std::sync::Once::new() }; input::OVERLAYS];
                        LOGGED[i].call_once(|| {
                            log::info!("Horizon XR: the overlay panel {i} is in use");
                        });
                    }
                }
                s.overlay_images = overlay.clone();
                let upload = (!s.vignette_filled)
                    .then(|| s.vignette.acquire().ok())
                    .flatten()
                    .map(|image| (image, vignette_pixels()));
                s.pending = Some(Pending::Eyes {
                    time,
                    views,
                    hud: hud.is_some(),
                    screen,
                    filling: upload.is_some(),
                    array: use_array,
                    overlay: std::array::from_fn(|i| overlay[i].is_some()),
                });
                Targets::Eyes([(left, poses[0]), (right, poses[1])], hud, upload)
            })
        };
        diag::waited(2, waiting.elapsed());
        diag::app_start();
        match acquired {
            Ok(targets) => targets,
            Err(e) => {
                log::warn!("Horizon XR: {e}");
                s.pending = Some(Pending::Empty { time });
                self.end();
                Targets::Skip
            }
        }
    }

    fn request_stereo_array(&mut self, enabled: bool) {
        if let Some(s) = self.session.as_mut() {
            s.request_stereo_array = enabled;
        }
    }
    fn discard_frame(&mut self) {
        if let Some(s) = self.session.as_mut() {
            s.discard_world = true;
        }
    }

    fn overlay_image(&mut self, slot: usize) -> Option<wgpu::Texture> {
        self.session.as_mut()?.overlay_images[slot].take()
    }

    fn hidden_areas(&mut self) -> Option<std::sync::Arc<[HiddenArea; 2]>> {
        if !visibility_mask_wanted() || self.instance.exts().khr_visibility_mask.is_none() {
            return None;
        }
        let s = self.session.as_mut()?;
        if s.hidden_stale {
            s.hidden_stale = false;
            s.hidden = read_hidden_areas(&s.session);
        }
        s.hidden.clone()
    }

    fn end(&mut self) {
        diag::app_end();
        let Some(s) = self.session.as_mut() else {
            return;
        };
        let Some(pending) = s.pending.take() else {
            return;
        };
        diag::count(match &pending {
            Pending::Empty { .. } => 0,
            Pending::Eyes { screen: false, .. } => 1,
            Pending::Eyes { .. } => 2,
            Pending::Menu { .. } => 3,
        });
        let result = match pending {
            Pending::Empty { time } => s.stream.end(time, self.blend, &[]),
            Pending::Eyes {
                time,
                views,
                hud,
                screen,
                filling,
                array,
                overlay,
            } => {
                if array {
                    s.stereo_array.as_mut().unwrap().release();
                } else {
                    s.eyes[0].release();
                    s.eyes[1].release();
                }
                if hud {
                    s.menu.release();
                }
                for (i, held) in overlay.iter().enumerate() {
                    if *held && let Some(chain) = s.overlays[i].as_mut() {
                        chain.release();
                    }
                }
                if filling {
                    s.vignette.release();
                    s.vignette_filled = true;
                }
                if s.discard_world {
                    s.stream.end(time, self.blend, &[])
                } else {
                    let projection_views = [
                        xr::CompositionLayerProjectionView::new()
                            .pose(views[0].pose)
                            .fov(views[0].fov)
                            .sub_image(if array {
                                s.stereo_array
                                    .as_ref()
                                    .unwrap()
                                    .sub_image()
                                    .image_array_index(0)
                            } else {
                                s.eyes[0].sub_image()
                            }),
                        xr::CompositionLayerProjectionView::new()
                            .pose(views[1].pose)
                            .fov(views[1].fov)
                            .sub_image(if array {
                                s.stereo_array
                                    .as_ref()
                                    .unwrap()
                                    .sub_image()
                                    .image_array_index(1)
                            } else {
                                s.eyes[1].sub_image()
                            }),
                    ];
                    let layer = xr::CompositionLayerProjection::new()
                        .space(&s.space)
                        .views(&projection_views);
                    let filter = layer_filter::settings(
                        self.instance.exts().fb_composition_layer_settings.is_some(),
                        input::settings().bool("horizonSuperResolution"),
                    );
                    let layer = layer_filter::apply(layer, filter.as_ref());
                    // The HUD over the world, its transparent parts showing it
                    // (premultiplied: the HUD's alpha blending onto a clear image).
                    let panel = if screen { s.panel } else { s.hud_at };
                    // The HUD's opacity scales its premultiplied colour and alpha alike.
                    let opacity = if screen {
                        1.0
                    } else {
                        input::settings().float("hudOpacity")
                    };
                    let fade = xr::sys::CompositionLayerColorScaleBiasKHR {
                        ty: xr::sys::CompositionLayerColorScaleBiasKHR::TYPE,
                        next: std::ptr::null(),
                        color_scale: xr::Color4f {
                            r: opacity,
                            g: opacity,
                            b: opacity,
                            a: opacity,
                        },
                        color_bias: xr::Color4f {
                            r: 0.0,
                            g: 0.0,
                            b: 0.0,
                            a: 0.0,
                        },
                    };
                    let fades = opacity < 1.0
                        && self
                            .instance
                            .exts()
                            .khr_composition_layer_color_scale_bias
                            .is_some();
                    let quad = panel.filter(|_| hud).map(|panel| {
                        let quad = xr::CompositionLayerQuad::new()
                            .space(&s.space)
                            .layer_flags(xr::CompositionLayerFlags::BLEND_TEXTURE_SOURCE_ALPHA)
                            .eye_visibility(xr::EyeVisibility::BOTH)
                            .sub_image(s.menu.sub_image())
                            .pose(panel.pose())
                            .size(panel.size());
                        if !fades {
                            return quad;
                        }
                        let mut raw = quad.into_raw();
                        raw.next = std::ptr::from_ref(&fade).cast();
                        // Safety: the same layer with the extension's struct chained
                        // on; `fade` outlives the frame's end below.
                        unsafe { xr::CompositionLayerQuad::from_raw(raw) }
                    });
                    // The comfort vignette, fixed to the head: the further it closes
                    // in, the smaller its quad (its clear middle shrinks with it).
                    let vignette = (s.vignette_filled && s.vignette_level > 0.02).then(|| {
                        let width = VIGNETTE_WIDTH / s.vignette_level;
                        xr::CompositionLayerQuad::new()
                            .space(&s.head_space)
                            .layer_flags(xr::CompositionLayerFlags::BLEND_TEXTURE_SOURCE_ALPHA)
                            .eye_visibility(xr::EyeVisibility::BOTH)
                            .sub_image(s.vignette.sub_image())
                            .pose(xr::Posef {
                                orientation: xr::Quaternionf::IDENTITY,
                                position: xr::Vector3f {
                                    x: 0.0,
                                    y: 0.0,
                                    z: -1.0,
                                },
                            })
                            .size(xr::Extent2Df {
                                width,
                                height: width,
                            })
                    });
                    // The overlay panel: where the game put it (the play space's hand frame, turned
                    // back into the room's), shown with the HUD's blending over the world.
                    let rest = s.rest.unwrap_or(Vec3::ZERO);
                    let scale = input::world_scale();
                    let overlay_quads: Vec<xr::CompositionLayerQuad<'_, xr::Vulkan>> = (0
                        ..input::OVERLAYS)
                        .filter_map(|i| {
                            if !overlay[i] {
                                return None;
                            }
                            // The handles' strip is placed in the room by the panel; the others by
                            // the game, in the play space's hand frame.
                            let (at, q, width, height) = if i == input::OVERLAY_HANDLES {
                                let (at, q, w, h) = s.handles_at?;
                                (at, q, w, h)
                            } else {
                                let spec = input::overlay_at(i)?;
                                let relative = (spec.pos
                                    - Vec3::new(0.0, (scale - 1.0) * rest.y, 0.0))
                                    / scale;
                                (
                                    rest + relative,
                                    spec.rot,
                                    spec.width / scale,
                                    spec.height / scale,
                                )
                            };
                            Some(
                                xr::CompositionLayerQuad::new()
                                    .space(&s.space)
                                    .layer_flags(
                                        xr::CompositionLayerFlags::BLEND_TEXTURE_SOURCE_ALPHA,
                                    )
                                    .eye_visibility(xr::EyeVisibility::BOTH)
                                    .sub_image(s.overlays[i].as_ref()?.sub_image())
                                    .pose(xr::Posef {
                                        orientation: xr::Quaternionf {
                                            x: q.x,
                                            y: q.y,
                                            z: q.z,
                                            w: q.w,
                                        },
                                        position: xr::Vector3f {
                                            x: at.x,
                                            y: at.y,
                                            z: at.z,
                                        },
                                    })
                                    .size(xr::Extent2Df { width, height }),
                            )
                        })
                        .collect();
                    let mut layers: Vec<&xr::CompositionLayerBase<'_, xr::Vulkan>> = vec![&layer];
                    if let Some(quad) = &quad {
                        layers.push(quad);
                    }
                    for quad in &overlay_quads {
                        layers.push(quad);
                    }
                    if let Some(vignette) = &vignette {
                        layers.push(vignette);
                    }
                    s.stream.end(time, self.blend, &layers)
                }
            }
            Pending::Menu {
                time,
                views,
                keyboard,
            } => {
                s.menu.release();
                if keyboard && let Some(chain) = s.overlays[input::OVERLAY_KEYBOARD].as_mut() {
                    chain.release();
                }
                if views.is_some() {
                    s.eyes[0].release();
                    s.eyes[1].release();
                }
                let projection_views = views.map(|views| {
                    [
                        xr::CompositionLayerProjectionView::new()
                            .pose(views[0].pose)
                            .fov(views[0].fov)
                            .sub_image(s.eyes[0].sub_image()),
                        xr::CompositionLayerProjectionView::new()
                            .pose(views[1].pose)
                            .fov(views[1].fov)
                            .sub_image(s.eyes[1].sub_image()),
                    ]
                });
                let projection = projection_views.as_ref().map(|v| {
                    xr::CompositionLayerProjection::new()
                        .space(&s.space)
                        .views(v)
                });
                // The title's panel is its buttons and logo on nothing: the room shows through.
                let quad = s.panel.as_ref().map(|panel| {
                    xr::CompositionLayerQuad::new()
                        .layer_flags(xr::CompositionLayerFlags::BLEND_TEXTURE_SOURCE_ALPHA)
                        .space(&s.space)
                        .eye_visibility(xr::EyeVisibility::BOTH)
                        .sub_image(s.menu.sub_image())
                        .pose(panel.pose())
                        .size(panel.size())
                });
                // Visor's keyboard, in the room where the game hung it.
                let rest = s.rest.unwrap_or(Vec3::ZERO);
                let scale = input::world_scale();
                let keyboard_quad = input::overlay_at(input::OVERLAY_KEYBOARD)
                    .filter(|_| keyboard)
                    .and_then(|spec| {
                        let relative =
                            (spec.pos - Vec3::new(0.0, (scale - 1.0) * rest.y, 0.0)) / scale;
                        let (at, q) = (rest + relative, spec.rot);
                        Some(
                            xr::CompositionLayerQuad::new()
                                .space(&s.space)
                                .layer_flags(xr::CompositionLayerFlags::BLEND_TEXTURE_SOURCE_ALPHA)
                                .eye_visibility(xr::EyeVisibility::BOTH)
                                .sub_image(
                                    s.overlays[input::OVERLAY_KEYBOARD].as_ref()?.sub_image(),
                                )
                                .pose(xr::Posef {
                                    orientation: xr::Quaternionf {
                                        x: q.x,
                                        y: q.y,
                                        z: q.z,
                                        w: q.w,
                                    },
                                    position: xr::Vector3f {
                                        x: at.x,
                                        y: at.y,
                                        z: at.z,
                                    },
                                })
                                .size(xr::Extent2Df {
                                    width: spec.width / scale,
                                    height: spec.height / scale,
                                }),
                        )
                    });
                let mut layers: Vec<&xr::CompositionLayerBase<'_, xr::Vulkan>> = Vec::new();
                if let Some(p) = &projection {
                    layers.push(p);
                }
                if let Some(q) = &quad {
                    layers.push(q);
                }
                if let Some(q) = &keyboard_quad {
                    layers.push(q);
                }
                s.stream.end(time, self.blend, &layers)
            }
        };
        if let Err(e) = result {
            log::warn!("Horizon XR: ending a frame: {e}");
        }
    }
}

/// `HORIZON_XR_FAKE_HANDS=1` (a developer's switch in `launch.txt`): an
/// untracked controller is held still below and ahead of the head, pointing
/// a little down, so hands and the HUD on them show on a headset lying on a
/// desk with its controllers away. `x,y,z,pitch,yaw,roll` (metres and
/// degrees, the right hand's, the left one mirrored) places them elsewhere;
/// six more numbers place the left hand on its own (`FakeHands::parse`); `hy=1.2` holds the head
/// 1.2 m above the floor (a crouch).
fn fake_hands(controls: &mut actions::State, head: Quat, at: Vec3) {
    let Some(fake) = fake_state() else {
        return;
    };
    place_fake_hands(&fake, controls, head, at);
}

/// The fake hands' source (`HORIZON_XR_FAKE_HANDS`), read again every tenth of a second from a file.
fn fake_state() -> Option<FakeHands> {
    static FIXED: std::sync::OnceLock<Option<FakeHands>> = std::sync::OnceLock::new();
    static FILE: std::sync::Mutex<Option<(std::time::Instant, FakeHands)>> =
        std::sync::Mutex::new(None);
    let Ok(value) = std::env::var("HORIZON_XR_FAKE_HANDS") else {
        return None;
    };
    // `file:PATH`: the poses and buttons come from a file, read again every
    // tenth of a second, so a script on the computer moves the hands.
    let fake = if let Some(path) = value.strip_prefix("file:") {
        let mut file = FILE.lock().unwrap_or_else(|e| e.into_inner());
        let stale = file
            .as_ref()
            .is_none_or(|(read, _)| read.elapsed().as_secs_f32() > 0.1);
        if stale {
            let read = std::fs::read_to_string(path)
                .ok()
                .and_then(|text| FakeHands::parse(&text))
                .or_else(|| file.as_ref().map(|(_, f)| f.clone()));
            *file = read.map(|f| (std::time::Instant::now(), f));
        }
        file.as_ref().map(|(_, fake)| fake.clone())?
    } else {
        FIXED.get_or_init(|| FakeHands::parse(&value)).clone()?
    };
    Some(fake)
}

fn place_fake_hands(fake: &FakeHands, controls: &mut actions::State, head: Quat, at: Vec3) {
    let forward = head * Vec3::NEG_Z;
    let yaw = Quat::from_rotation_y((-forward.x).atan2(-forward.z));
    for (i, [x, y, z, pitch, yaw_deg, roll]) in fake.poses.iter().copied().enumerate() {
        let pose = yaw
            * Quat::from_rotation_y((-yaw_deg).to_radians())
            * Quat::from_rotation_x(pitch.to_radians())
            * Quat::from_rotation_z((-roll).to_radians());
        let hand = (at + yaw * Vec3::new(x, y, z), pose);
        controls.grip[i] = controls.grip[i].or(Some(hand));
        controls.aim[i] = controls.aim[i].or(Some(hand));
    }
    for (i, t) in fake.triggers.iter().copied().enumerate() {
        controls.triggers[i] = controls.triggers[i].max(t);
    }
    for (i, g) in fake.grips.iter().copied().enumerate() {
        controls.grips[i] = controls.grips[i].max(g);
    }
    controls.a |= fake.buttons[0];
    controls.b |= fake.buttons[1];
    controls.x |= fake.buttons[2];
    controls.y |= fake.buttons[3];
}

/// `HORIZON_XR_FAKE_HANDS`' hands: each one's place and turn (left, right:
/// metres and degrees from the head), and the buttons held.
#[derive(Clone, Debug, Default)]
struct FakeHands {
    poses: [[f32; 6]; 2],
    triggers: [f32; 2],
    grips: [f32; 2],
    /// A, B, X, Y.
    buttons: [bool; 4],
    /// `hy=`: the head's height above the floor, metres (a crouch or a crawl on a headset on a desk).
    head_y: Option<f32>,
}

impl FakeHands {
    /// `1`; or `x,y,z,pitch,yaw,roll` for the right hand, the left one
    /// mirrored, or six more for the left; then `tl=`, `tr=`, `gl=`, `gr=`
    /// (triggers and grips, 0..1), `a=1`, `b=1`, `x=1`, `y=1`.
    fn parse(text: &str) -> Option<Self> {
        let mut numbers = Vec::new();
        let mut fake = Self::default();
        for word in text.split(|c: char| c == ',' || c.is_whitespace()) {
            if let Some((key, value)) = word.split_once('=') {
                let v: f32 = value.parse().ok()?;
                match key {
                    "hy" => fake.head_y = Some(v),
                    "tl" => fake.triggers[0] = v,
                    "tr" => fake.triggers[1] = v,
                    "gl" => fake.grips[0] = v,
                    "gr" => fake.grips[1] = v,
                    "a" => fake.buttons[0] = v > 0.5,
                    "b" => fake.buttons[1] = v > 0.5,
                    "x" => fake.buttons[2] = v > 0.5,
                    "y" => fake.buttons[3] = v > 0.5,
                    _ => {}
                }
            } else if word == "1" && numbers.is_empty() && text.trim() == "1" {
                numbers = vec![0.22, -0.3, -0.35, -28.6, 0.0, 0.0];
            } else if let Ok(n) = word.parse::<f32>() {
                numbers.push(n);
            }
        }
        let right = <[f32; 6]>::try_from(numbers.get(..6)?).ok()?;
        let [x, y, z, pitch, yaw, roll] = right;
        let left = match numbers.get(6..12) {
            Some(left) => <[f32; 6]>::try_from(left).ok()?,
            None => [-x, y, z, pitch, -yaw, -roll],
        };
        fake.poses = [left, right];
        Some(fake)
    }
}

/// The comfort vignette: clear in the middle, black (premultiplied, so all
/// zero but alpha) from a ring outward, with a smooth edge between.
fn vignette_pixels() -> Vec<u8> {
    const CLEAR: f32 = 0.25;
    const BLACK: f32 = 0.6;
    let n = VIGNETTE_SIZE;
    let mut pixels = Vec::with_capacity((n * n * 4) as usize);
    for y in 0..n {
        for x in 0..n {
            let at = |i: u32| (i as f32 + 0.5) / n as f32 * 2.0 - 1.0;
            let r = (at(x).powi(2) + at(y).powi(2)).sqrt();
            let t = ((r - CLEAR) / (BLACK - CLEAR)).clamp(0.0, 1.0);
            let alpha = t * t * (3.0 - 2.0 * t);
            pixels.extend_from_slice(&[0, 0, 0, (alpha * 255.0).round() as u8]);
        }
    }
    pixels
}

/// What the frames carried, logged every few seconds while bringing the headset up.
mod diag {
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    struct Counts {
        since: Option<Instant>,
        kinds: [u32; 4],
        head: [u32; 2],
        /// Time spent in xrWaitFrame, xrBeginFrame and acquiring images.
        waits: [Duration; 4],
        app_from: Option<Instant>,
    }

    static COUNTS: Mutex<Counts> = Mutex::new(Counts {
        since: None,
        kinds: [0; 4],
        head: [0; 2],
        waits: [Duration::ZERO; 4],
        app_from: None,
    });

    pub fn waited(what: usize, time: Duration) {
        if let Ok(mut c) = COUNTS.lock() {
            c.waits[what] += time;
        }
        // The frame profile (`horizonVrProfile`) splits the same three waits per frame.
        use horizon_client_render::vr_profile::{Stage, record};
        match what {
            0 => record(Stage::XrWait, time),
            1 => record(Stage::XrBegin, time),
            2 => record(Stage::XrAcquire, time),
            _ => {}
        }
    }

    /// The app's own time starts: the game's tick, the draw calls and the submit all happen
    /// between this and `app_end`.
    pub fn app_start() {
        if let Ok(mut c) = COUNTS.lock() {
            c.app_from = Some(Instant::now());
        }
    }

    pub fn app_end() {
        if let Ok(mut c) = COUNTS.lock()
            && let Some(from) = c.app_from.take()
        {
            c.waits[3] += from.elapsed();
        }
    }

    pub fn head(found: bool) {
        if let Ok(mut c) = COUNTS.lock() {
            c.head[found as usize] += 1;
        }
    }

    pub fn count(kind: usize) {
        let Ok(mut c) = COUNTS.lock() else { return };
        c.kinds[kind] += 1;
        let since = *c.since.get_or_insert_with(Instant::now);
        if since.elapsed() >= Duration::from_secs(5) {
            let frames = c.kinds.iter().sum::<u32>().max(1) as f32;
            let ms = c.waits.map(|w| w.as_secs_f32() * 1000.0 / frames);
            log::info!(
                "Horizon XR: frames empty {} world {} screen {} flat {}; head found {} lost {}; \
                 per frame ms: wait {:.1} begin {:.1} acquire {:.1} app(cpu sim+draw+submit) {:.1}",
                c.kinds[0],
                c.kinds[1],
                c.kinds[2],
                c.kinds[3],
                c.head[1],
                c.head[0],
                ms[0],
                ms[1],
                ms[2],
                ms[3]
            );
            c.kinds = [0; 4];
            c.head = [0; 2];
            c.waits = [Duration::ZERO; 4];
            c.since = Some(Instant::now());
        }
    }
}

/// The HUD's place this frame and whether its hotbar can be touched (`HUD_POPUP`). Without an
/// off-hand controller the hand sits where a resting hand would.
fn hud_placement(
    settings: &input::VrSettings,
    head: Option<(Quat, Vec3)>,
    off_aim: Option<actions::Pose>,
    main_aim: Option<actions::Pose>,
) -> (Option<Placement>, bool) {
    let Some((rot, pos)) = head else {
        return (None, false);
    };
    let hmd = RoomPose { pos, rot };
    let pose = |p: Option<actions::Pose>, rest: Vec3| match p {
        Some((pos, rot)) => RoomPose { pos, rot },
        None => RoomPose {
            pos: pos + rot * rest,
            rot,
        },
    };
    let input = HudInput {
        hmd,
        off_aim: pose(off_aim, Vec3::new(-0.25, -0.5, -0.3)),
        main_aim: pose(main_aim, Vec3::new(0.25, -0.5, -0.3)),
        slim: false,
        world_scale: 1.0,
    };
    let (placement, popup) = place_hud(settings, &input);
    (Some(placement), popup)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_seated_launch_does_not_become_a_tall_camera_after_standing() {
        let mut settings = horizon_vivecraft::settings::VrSettings::default();
        let reference = neutral_rest_height(&settings, 0.9);
        let rest = Vec3::new(0.0, reference, 0.0);
        for (physical, expected) in [(0.9, 0.9), (1.75, 1.75)] {
            let offset = scaled_world_offset(Vec3::Y * (physical - reference), rest, 1.0);
            assert!((1.62 + offset.y - expected).abs() < 1e-5);
        }
        settings.set_float("manualCalibration", 1.65);
        let reference = neutral_rest_height(&settings, 1.75);
        let rest = Vec3::Y * reference;
        let physical = Vec3::Y * 1.75;
        let offset = scaled_world_offset(physical - rest, rest, 1.0);
        assert!((1.62 + offset.y - 1.62).abs() < 1e-5);
        // Camera-base compensation and the XR offset together apply world scale once.
        let offset = scaled_world_offset(physical - rest, rest, 2.0);
        let base = 1.62 * 2.0 - reference;
        assert!((base + offset.y - 3.24).abs() < 1e-5);
    }

    #[test]
    fn srgb_comes_first_and_is_drawn_through_its_unorm_view() {
        let formats = [
            vk::Format::R8G8B8A8_UNORM.as_raw() as u32,
            vk::Format::R8G8B8A8_SRGB.as_raw() as u32,
        ];
        let (format, view) = pick_format(&formats).unwrap();
        assert!(is_srgb(format));
        assert_eq!(view, wgpu::TextureFormat::Rgba8Unorm);
        assert!(pick_format(&[vk::Format::R16G16B16A16_SFLOAT.as_raw() as u32]).is_err());
    }
}
