//! Fixed foveation (`XR_FB_foveation`, `XR_FB_foveation_vulkan`): the eyes' swapchains are made
//! with a fragment density map, the runtime fills it so the periphery is shaded at a coarser rate,
//! and the render passes into those images take the map (`wgpu_hal::vulkan::Device::set_density_map`,
//! in `third_party/wgpu-hal`). The GPU shades fewer fragments for nearly the same picture.
//!
//! Wanted with `HORIZON_XR_FOVEATION=low|medium|high` or the setting `horizonFoveation`, read once
//! at start (the instance, the device and the swapchains are made for it). Everything it needs is
//! checked: the four OpenXR extensions, `VK_EXT_fragment_density_map` and its feature on the
//! GPU; short of any, the game runs as before and says which was missing.
use ash::vk::{self, Handle};
use horizon_client_input::headset as input;
use openxr as xr;
use openxr::sys::Handle as _;
use std::ffi::{CStr, c_void};
use std::sync::OnceLock;

/// The Vulkan device extension the density map needs.
pub const DEVICE_EXTENSION: &CStr = c"VK_EXT_fragment_density_map";

/// How strong the foveation is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Off,
    Low,
    Medium,
    High,
}

impl Level {
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "" | "0" | "off" | "none" | "false" => Some(Self::Off),
            "low" | "1" => Some(Self::Low),
            "medium" | "2" | "on" | "true" => Some(Self::Medium),
            "high" | "3" => Some(Self::High),
            _ => None,
        }
    }

    fn xr(self) -> xr::sys::FoveationLevelFB {
        match self {
            Self::Off => xr::sys::FoveationLevelFB::NONE,
            Self::Low => xr::sys::FoveationLevelFB::LOW,
            Self::Medium => xr::sys::FoveationLevelFB::MEDIUM,
            Self::High => xr::sys::FoveationLevelFB::HIGH,
        }
    }
}

/// The level asked for: `HORIZON_XR_FOVEATION`, else the setting `horizonFoveation`. Read once.
pub fn wanted() -> Level {
    static LEVEL: OnceLock<Level> = OnceLock::new();
    *LEVEL.get_or_init(|| {
        std::env::var("HORIZON_XR_FOVEATION")
            .ok()
            .and_then(|v| Level::parse(&v))
            .or_else(|| Level::parse(input::settings().enum_name("horizonFoveation")))
            .unwrap_or(Level::Off)
    })
}

/// The OpenXR extensions foveation needs, which of them are missing (names).
pub fn missing_extensions(available: &xr::ExtensionSet) -> Vec<&'static str> {
    let mut missing = Vec::new();
    for (name, present) in [
        ("XR_FB_foveation", available.fb_foveation),
        (
            "XR_FB_foveation_configuration",
            available.fb_foveation_configuration,
        ),
        ("XR_FB_foveation_vulkan", available.fb_foveation_vulkan),
        (
            "XR_FB_swapchain_update_state",
            available.fb_swapchain_update_state,
        ),
    ] {
        if !present {
            missing.push(name);
        }
    }
    missing
}

/// Turns the four extensions on in `extensions`.
pub fn enable(extensions: &mut xr::ExtensionSet) {
    extensions.fb_foveation = true;
    extensions.fb_foveation_configuration = true;
    extensions.fb_foveation_vulkan = true;
    extensions.fb_swapchain_update_state = true;
}

/// Whether the GPU can take a fragment density map: the extension and its feature.
pub fn gpu_supports(adapter: &wgpu::Adapter) -> Result<(), String> {
    use wgpu::hal::api::Vulkan;
    // Safety: only reads what wgpu keeps alive with the adapter.
    let hal = unsafe { adapter.as_hal::<Vulkan>() }.ok_or("not a Vulkan adapter")?;
    if !hal
        .physical_device_capabilities()
        .supports_extension(DEVICE_EXTENSION)
    {
        return Err("the GPU lacks VK_EXT_fragment_density_map".into());
    }
    let mut density = vk::PhysicalDeviceFragmentDensityMapFeaturesEXT::default();
    let mut features = vk::PhysicalDeviceFeatures2::default().push_next(&mut density);
    // Safety: the physical device belongs to the instance, which the adapter keeps alive.
    unsafe {
        hal.shared_instance()
            .raw_instance()
            .get_physical_device_features2(hal.raw_physical_device(), &mut features);
    }
    if density.fragment_density_map == vk::TRUE {
        Ok(())
    } else {
        Err("the GPU's fragmentDensityMap feature is off".into())
    }
}

/// Ordinary wgpu depth images need this feature in a foveated multiview pass.
pub fn non_subsampled_images(adapter: &wgpu::Adapter) -> bool {
    use wgpu::hal::api::Vulkan;
    let Some(hal) = (unsafe { adapter.as_hal::<Vulkan>() }) else {
        return false;
    };
    let mut density = vk::PhysicalDeviceFragmentDensityMapFeaturesEXT::default();
    let mut features = vk::PhysicalDeviceFeatures2::default().push_next(&mut density);
    // Safety: physical device belongs to this live adapter/instance.
    unsafe {
        hal.shared_instance()
            .raw_instance()
            .get_physical_device_features2(hal.raw_physical_device(), &mut features);
    }
    density.fragment_density_map_non_subsampled_images == vk::TRUE
}

/// The feature struct to chain into device creation, made once and kept for the process
/// (wgpu-hal borrows it for the callback's lifetime).
pub fn device_feature(
    non_subsampled: bool,
) -> &'static mut vk::PhysicalDeviceFragmentDensityMapFeaturesEXT<'static> {
    Box::leak(Box::new(
        vk::PhysicalDeviceFragmentDensityMapFeaturesEXT::default()
            .fragment_density_map(true)
            .fragment_density_map_non_subsampled_images(non_subsampled),
    ))
}

/// The session's foveation: the runtime's profile for the level.
pub struct Foveation {
    pub level: Level,
    profile: xr::FoveationProfileFB,
}

impl Foveation {
    pub fn new(session: &xr::Session<xr::Vulkan>, level: Level) -> Result<Self, String> {
        let profile = session
            .create_foveation_profile(Some(xr::FoveationLevelProfile {
                level: level.xr(),
                vertical_offset: 0.0,
                dynamic: xr::sys::FoveationDynamicFB::DISABLED,
            }))
            .map_err(|e| format!("creating the foveation profile: {e}"))?;
        Ok(Self { level, profile })
    }

    /// Makes an eye's swapchain with a fragment density map, gives it the profile, and returns its
    /// images' density maps (the runtime's own images, one per colour image).
    ///
    /// # Safety
    ///
    /// `info` describes a valid swapchain for `session`.
    pub unsafe fn create_swapchain(
        &self,
        session: &xr::Session<xr::Vulkan>,
        mut info: xr::sys::SwapchainCreateInfo,
    ) -> Result<
        (
            xr::Swapchain<xr::Vulkan>,
            Vec<xr::sys::SwapchainImageVulkanKHR>,
            Vec<DensityImage>,
        ),
        String,
    > {
        let mut flags = xr::sys::SwapchainCreateInfoFoveationFB {
            ty: xr::sys::SwapchainCreateInfoFoveationFB::TYPE,
            next: std::ptr::null_mut(),
            flags: xr::sys::SwapchainCreateFoveationFlagsFB::FRAGMENT_DENSITY_MAP,
        };
        info.next = std::ptr::from_mut(&mut flags).cast::<c_void>();
        let fp = session.instance().fp();
        let mut handle = xr::sys::Swapchain::NULL;
        // Safety: the create info chain lives through the call.
        let result = unsafe { (fp.create_swapchain)(session.as_raw(), &info, &mut handle) };
        if result.into_raw() < 0 {
            return Err(format!("creating a foveated swapchain: {result}"));
        }
        // Safety: a fresh handle of this session, owned from here.
        let swapchain = unsafe { xr::Swapchain::<xr::Vulkan>::from_raw(session.clone(), handle) };
        // The profile goes on first: the runtime only writes a density map into the images'
        // `XrSwapchainImageFoveationVulkanFB` once the swapchain has one (enumerating before it
        // returned 0x0 maps with a null image, whose view crashed the driver in `vkCreateFramebuffer`).
        let update = session
            .instance()
            .exts()
            .fb_swapchain_update_state
            .as_ref()
            .ok_or("XR_FB_swapchain_update_state was not loaded")?;
        let state = xr::sys::SwapchainStateFoveationFB {
            ty: xr::sys::SwapchainStateFoveationFB::TYPE,
            next: std::ptr::null_mut(),
            flags: xr::sys::SwapchainStateFoveationFlagsFB::EMPTY,
            profile: self.profile.as_raw(),
        };
        // Safety: the state struct is one of the extension's, valid for this swapchain.
        let result = unsafe {
            (update.update_swapchain)(
                handle,
                std::ptr::from_ref(&state).cast::<xr::sys::SwapchainStateBaseHeaderFB>(),
            )
        };
        if result.into_raw() < 0 {
            return Err(format!(
                "giving the swapchain its foveation profile: {result}"
            ));
        }
        // The images, each with its density map chained on.
        let mut count = 0;
        // Safety: the size query of the enumeration call.
        let result =
            unsafe { (fp.enumerate_swapchain_images)(handle, 0, &mut count, std::ptr::null_mut()) };
        if result.into_raw() < 0 {
            return Err(format!(
                "counting the foveated swapchain's images: {result}"
            ));
        }
        let mut maps: Vec<xr::sys::SwapchainImageFoveationVulkanFB> = (0..count)
            .map(|_| xr::sys::SwapchainImageFoveationVulkanFB {
                ty: xr::sys::SwapchainImageFoveationVulkanFB::TYPE,
                next: std::ptr::null_mut(),
                image: 0,
                width: 0,
                height: 0,
            })
            .collect();
        let mut images: Vec<xr::sys::SwapchainImageVulkanKHR> = maps
            .iter_mut()
            .map(|map| xr::sys::SwapchainImageVulkanKHR {
                ty: xr::sys::SwapchainImageVulkanKHR::TYPE,
                next: std::ptr::from_mut(map).cast::<c_void>(),
                image: 0,
            })
            .collect();
        let mut written = 0;
        // Safety: `images` holds `count` structs of the Vulkan image type, as the call wants.
        let result = unsafe {
            (fp.enumerate_swapchain_images)(
                handle,
                count,
                &mut written,
                images
                    .as_mut_ptr()
                    .cast::<xr::sys::SwapchainImageBaseHeader>(),
            )
        };
        if result.into_raw() < 0 {
            return Err(format!("the foveated swapchain's images: {result}"));
        }
        images.truncate(written as usize);
        for (i, m) in maps.iter().enumerate().take(written as usize) {
            log::info!(
                "Horizon XR: foveation: image {i}: swapchain image {:#x}, density map {:#x} {}x{}, create flags {:#x}",
                images[i].image,
                m.image,
                m.width,
                m.height,
                flags.flags.into_raw()
            );
        }
        let density: Vec<DensityImage> = maps
            .iter()
            .take(written as usize)
            .map(|m| DensityImage {
                image: vk::Image::from_raw(m.image),
                size: [m.width, m.height],
            })
            .collect();
        if let Some(bad) = density.iter().position(|d| !d.is_valid()) {
            return Err(format!(
                "the runtime gave no density map for image {bad} of {written} ({}x{}, handle {:#x})",
                density[bad].size[0],
                density[bad].size[1],
                density[bad].image.as_raw()
            ));
        }
        Ok((swapchain, images, density))
    }
}

/// The runtime's density map of one swapchain image.
pub struct DensityImage {
    pub image: vk::Image,
    pub size: [u32; 2],
}

impl DensityImage {
    /// A real image of a real size: the runtime fills both in, or neither.
    pub fn is_valid(&self) -> bool {
        self.image != vk::Image::null() && self.size[0] > 0 && self.size[1] > 0
    }
}

/// An `R8G8_UNORM` view of a density map, for the render pass.
///
/// # Safety
///
/// `image` is the runtime's density map for a swapchain of this device.
pub unsafe fn density_view(
    device: &ash::Device,
    image: vk::Image,
    layers: u32,
) -> Result<vk::ImageView, String> {
    let info = vk::ImageViewCreateInfo::default()
        .image(image)
        .view_type(if layers == 1 {
            vk::ImageViewType::TYPE_2D
        } else {
            vk::ImageViewType::TYPE_2D_ARRAY
        })
        .format(vk::Format::R8G8_UNORM)
        .subresource_range(vk::ImageSubresourceRange {
            aspect_mask: vk::ImageAspectFlags::COLOR,
            base_mip_level: 0,
            level_count: 1,
            base_array_layer: 0,
            layer_count: layers,
        });
    // Safety: a valid image of this device.
    unsafe { device.create_image_view(&info, None) }
        .map_err(|e| format!("the density map's view: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_parse_from_the_setting_and_the_environment() {
        assert_eq!(Level::parse("LOW"), Some(Level::Low));
        assert_eq!(Level::parse(" high "), Some(Level::High));
        assert_eq!(Level::parse("1"), Some(Level::Low));
        assert_eq!(Level::parse("off"), Some(Level::Off));
        assert_eq!(Level::parse("strong"), None);
    }

    #[test]
    fn a_density_map_the_runtime_left_empty_is_not_valid() {
        let empty = DensityImage {
            image: vk::Image::null(),
            size: [0, 0],
        };
        assert!(!empty.is_valid());
        let no_size = DensityImage {
            image: vk::Image::from_raw(7),
            size: [0, 64],
        };
        assert!(!no_size.is_valid());
        let real = DensityImage {
            image: vk::Image::from_raw(7),
            size: [358, 358],
        };
        assert!(real.is_valid());
    }

    #[test]
    fn a_runtime_missing_one_extension_is_told_which() {
        let mut available = xr::ExtensionSet::default();
        available.fb_foveation = true;
        available.fb_foveation_vulkan = true;
        available.fb_swapchain_update_state = true;
        assert_eq!(
            missing_extensions(&available),
            vec!["XR_FB_foveation_configuration"]
        );
        available.fb_foveation_configuration = true;
        assert!(missing_extensions(&available).is_empty());
    }
}
