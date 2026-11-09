//! Meta Quest Super Resolution, requested from the compositor. This does not
//! change the app's render size. The runtime can decline the quality hint.
use openxr as xr;

pub fn settings(supported: bool, enabled: bool) -> Option<xr::sys::CompositionLayerSettingsFB> {
    (supported && enabled).then_some(xr::sys::CompositionLayerSettingsFB {
        ty: xr::sys::CompositionLayerSettingsFB::TYPE,
        next: std::ptr::null(),
        layer_flags: xr::sys::CompositionLayerSettingsFlagsFB::QUALITY_SHARPENING,
    })
}

pub fn apply<'a>(
    layer: xr::CompositionLayerProjection<'a, xr::Vulkan>,
    settings: Option<&'a xr::sys::CompositionLayerSettingsFB>,
) -> xr::CompositionLayerProjection<'a, xr::Vulkan> {
    let Some(settings) = settings else {
        return layer;
    };
    let mut raw = layer.into_raw();
    // This helper only takes a newly built projection, with no other chain.
    debug_assert!(raw.next.is_null());
    raw.next = std::ptr::from_ref(settings).cast();
    // Safety: a valid nonzero filter on an enabled extension; the borrow ties
    // the filter's lifetime to this layer, through xrEndFrame.
    unsafe { xr::CompositionLayerProjection::from_raw(raw) }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unsupported_or_disabled_filter_is_absent_and_mqsr_uses_only_quality_sharpening() {
        assert!(settings(false, true).is_none());
        assert!(settings(true, false).is_none());
        let filter = settings(true, true).unwrap();
        assert_eq!(
            filter.layer_flags,
            xr::sys::CompositionLayerSettingsFlagsFB::QUALITY_SHARPENING
        );
        assert!(filter.next.is_null());
        let layer = apply(xr::CompositionLayerProjection::new(), Some(&filter));
        assert_eq!(layer.into_raw().next, std::ptr::from_ref(&filter).cast());
    }
}
