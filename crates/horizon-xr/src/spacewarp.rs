//! XR_FB_space_warp auxiliary swapchains and per-projection-view metadata.
//! The runtime, rather than an application sleep/FPS cap, schedules half-rate rendering.
use super::*;
use horizon_client_render::gfx::headset::SpaceWarpTargets;

pub(super) struct SpaceWarp {
    size: Option<[u32; 2]>,
    chains: Option<[Chain; 2]>,
    pub requested: bool,
    held: Option<SpaceWarpTargets>,
    rendered: bool,
    active: bool,
    history: Option<(glam::DVec3, glam::DQuat, f32, u64)>,
    delta: xr::Posef,
    skip: bool,
    far: f32,
}

impl SpaceWarp {
    pub fn new(instance: &xr::Instance, system: xr::SystemId, formats: &[u32]) -> Self {
        let size = recommended_size(instance, system).filter(|_| {
            instance.exts().fb_display_refresh_rate.is_some()
                && formats.contains(&(vk::Format::R16G16B16A16_SFLOAT.as_raw() as u32))
                && formats.contains(&(vk::Format::D32_SFLOAT.as_raw() as u32))
        });
        horizon_client_input::headset_display::update(|i| {
            i.spacewarp_supported = size.is_some();
            i.spacewarp_active = false;
        });
        log::info!("Horizon XR: Application SpaceWarp auxiliary size {size:?}");
        Self {
            size,
            chains: None,
            requested: false,
            held: None,
            rendered: false,
            active: false,
            history: None,
            delta: xr::Posef::IDENTITY,
            skip: true,
            far: 1.,
        }
    }

    pub fn begin(&mut self, session: &xr::Session<xr::Vulkan>, device: &wgpu::Device) {
        self.rendered = false;
        if !self.requested || !cadence().spacewarp {
            self.history = None;
            return;
        }
        let Some(size) = self.size else {
            return;
        };
        let cadence = cadence();
        let info = horizon_client_input::headset_display::snapshot().unwrap_or_default();
        let rates = &info.rates;
        let current = info.current_hz;
        if !rates.iter().any(|r| (*r - cadence.display).abs() < 0.1)
            || current.is_none_or(|hz| (hz - cadence.display).abs() > 0.1)
        {
            self.history = None;
            return;
        }
        if self.chains.is_none() {
            let made = (|| {
                Ok::<_, String>([
                    Chain::new_layers(
                        session,
                        device,
                        vk::Format::R16G16B16A16_SFLOAT.as_raw() as u32,
                        wgpu::TextureFormat::Rgba16Float,
                        size,
                        "SpaceWarp motion",
                        None,
                        2,
                    )?,
                    Chain::new_layers(
                        session,
                        device,
                        vk::Format::D32_SFLOAT.as_raw() as u32,
                        wgpu::TextureFormat::Depth32Float,
                        size,
                        "SpaceWarp depth",
                        None,
                        2,
                    )?,
                ])
            })();
            match made {
                Ok(chains) => self.chains = Some(chains),
                Err(e) => {
                    self.disable(&e);
                    return;
                }
            }
        }
        let chains = self.chains.as_mut().expect("created above");
        match chains[0].acquire() {
            Ok(motion) => match chains[1].acquire() {
                Ok(depth) => self.held = Some(SpaceWarpTargets { motion, depth }),
                Err(e) => {
                    chains[0].release();
                    self.disable(&e);
                }
            },
            Err(e) => self.disable(&e),
        }
    }

    pub fn targets(&self) -> Option<SpaceWarpTargets> {
        self.held.clone()
    }

    pub fn rendered(
        &mut self,
        camera: glam::DVec3,
        yaw: f32,
        far: f32,
        rest: Vec3,
        scale: f32,
        epoch: u64,
        history_valid: bool,
    ) {
        if self.held.is_none() {
            return;
        }
        let (origin, rotation) = app_pose(camera, yaw, rest, scale);
        self.skip = true;
        self.delta = xr::Posef::IDENTITY;
        if let Some((prev_origin, prev_rotation, prev_scale, prev_epoch)) = self.history {
            let translation = (prev_rotation.conjugate() * (origin - prev_origin)).as_vec3();
            let rotation = (prev_rotation.conjugate() * rotation).as_quat();
            self.skip = !history_valid
                || prev_scale != scale
                || prev_epoch != epoch
                || !translation.is_finite()
                || translation.length() > 1.0
                || rotation.angle_between(Quat::IDENTITY) > 0.35;
            if !self.skip {
                self.delta = xr::Posef {
                    orientation: xr::Quaternionf {
                        x: rotation.x,
                        y: rotation.y,
                        z: rotation.z,
                        w: rotation.w,
                    },
                    position: xr::Vector3f {
                        x: translation.x,
                        y: translation.y,
                        z: translation.z,
                    },
                };
            }
        }
        self.history = Some((origin, rotation, scale, epoch));
        self.far = far / scale;
        self.rendered = true;
    }

    /// Release before xrEndFrame, retaining the chain handles and stack-owned next structs.
    pub fn finish(
        &mut self,
        discard: bool,
    ) -> Option<[xr::sys::CompositionLayerSpaceWarpInfoFB; 2]> {
        let held = self.held.take().is_some();
        let active = held && self.rendered && !discard;
        if self.active != active {
            self.active = active;
            log::info!(
                "Horizon XR: Application SpaceWarp {}",
                if active {
                    "active"
                } else {
                    "inactive; native refresh"
                }
            );
            horizon_client_input::headset_display::update(|i| i.spacewarp_active = active);
        }
        if !active {
            self.history = None;
        }
        if !held {
            return None;
        }
        let chains = self.chains.as_mut().expect("held chains");
        chains[0].release();
        chains[1].release();
        active.then(|| {
            std::array::from_fn(|eye| xr::sys::CompositionLayerSpaceWarpInfoFB {
                ty: xr::sys::CompositionLayerSpaceWarpInfoFB::TYPE,
                next: std::ptr::null(),
                layer_flags: if self.skip {
                    xr::sys::CompositionLayerSpaceWarpInfoFlagsFB::FRAME_SKIP
                } else {
                    xr::sys::CompositionLayerSpaceWarpInfoFlagsFB::EMPTY
                },
                motion_vector_sub_image: chains[0]
                    .sub_image()
                    .image_array_index(eye as u32)
                    .into_raw(),
                app_space_delta_pose: self.delta,
                depth_sub_image: chains[1]
                    .sub_image()
                    .image_array_index(eye as u32)
                    .into_raw(),
                min_depth: 0.,
                max_depth: 1.,
                // Reversed Z: depth zero is far; depth one is near, in room metres.
                near_z: self.far,
                far_z: horizon_client_render::gfx::NEAR_PLANE / input::world_scale(),
            })
        })
    }

    pub fn disable(&mut self, reason: &str) {
        log::warn!("Horizon XR: Application SpaceWarp disabled; native refresh retained: {reason}");
        self.size = None;
        self.active = false;
        self.history = None;
        horizon_client_input::headset_display::update(|i| {
            i.spacewarp_supported = false;
            i.spacewarp_active = false;
        });
    }
}

/// World-from-room pose in metres. Removing room tracking prevents ordinary head/room
/// movement from being reported as artificial locomotion a second time.
fn app_pose(camera: glam::DVec3, yaw: f32, rest: Vec3, scale: f32) -> (glam::DVec3, glam::DQuat) {
    let rotation = glam::DQuat::from_rotation_y(std::f64::consts::PI - f64::from(yaw).to_radians());
    let anchor = glam::DVec3::new(
        f64::from(rest.x),
        f64::from(rest.y / scale),
        f64::from(rest.z),
    );
    (camera / f64::from(scale) - rotation * anchor, rotation)
}

fn recommended_size(instance: &xr::Instance, system: xr::SystemId) -> Option<[u32; 2]> {
    instance.exts().fb_space_warp.as_ref()?;
    let mut warp = xr::sys::SystemSpaceWarpPropertiesFB {
        ty: xr::sys::SystemSpaceWarpPropertiesFB::TYPE,
        next: std::ptr::null_mut(),
        recommended_motion_vector_image_rect_width: 0,
        recommended_motion_vector_image_rect_height: 0,
    };
    // Safety: output structures have the required types and live through this call.
    let mut properties = xr::sys::SystemProperties {
        ty: xr::sys::SystemProperties::TYPE,
        next: std::ptr::from_mut(&mut warp).cast(),
        ..unsafe { std::mem::zeroed() }
    };
    let result = unsafe {
        (instance.fp().get_system_properties)(instance.as_raw(), system, &mut properties)
    };
    let size = [
        warp.recommended_motion_vector_image_rect_width,
        warp.recommended_motion_vector_image_rect_height,
    ];
    (result.into_raw() >= 0 && size.iter().all(|&s| s > 0)).then_some(size)
}

pub(super) fn cadence() -> horizon_vivecraft::refresh::Mode {
    horizon_vivecraft::refresh::requested(input::settings().float("horizonRefreshRate"))
}

impl Session {
    pub(super) fn ask_refresh_rate(&mut self, instance: &xr::Instance) {
        if instance.exts().fb_display_refresh_rate.is_none() {
            return;
        }
        let wanted = cadence().display;
        if self.rate == Some(wanted) {
            return;
        }
        self.rate = Some(wanted);
        let rates = self
            .session
            .enumerate_display_refresh_rates()
            .unwrap_or_default();
        let current = self.session.get_display_refresh_rate().ok();
        horizon_client_input::headset_display::update(|info| {
            info.rates = rates.clone();
            info.current_hz = current;
        });
        log::info!(
            "Horizon XR: {}",
            platform::rate_report(&rates, current, wanted)
        );
        if let Some(rate) = platform::nearest_rate(&rates, wanted)
            && let Err(e) = self.session.request_display_refresh_rate(rate)
        {
            log::warn!("Horizon XR: refresh rate {rate} Hz: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn app_pose_removes_room_movement_and_preserves_locomotion_at_world_scale() {
        for scale in [0.5, 1., 2.] {
            let rest = Vec3::new(1., 1.7, 2.);
            let yaw = 37.0f32;
            let rotation = Quat::from_rotation_y(std::f32::consts::PI - yaw.to_radians());
            let camera = glam::DVec3::new(100., 80., -300.);
            let (a, _) = app_pose(camera, yaw, rest, scale);
            let room_move = Vec3::new(0.1, 0., 0.2);
            let (b, _) = app_pose(
                camera + (rotation * room_move * scale).as_dvec3(),
                yaw,
                rest + room_move,
                scale,
            );
            assert!((a - b).length() < 1e-5);
            let (c, _) = app_pose(camera + glam::DVec3::X * f64::from(scale), yaw, rest, scale);
            assert!((c - a - glam::DVec3::X).length() < 1e-6);
        }
    }
}
