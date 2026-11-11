# Experimental native foveated multiview

`horizonFoveatedMultiview` is a saved VR boolean, off by default. It attempts real combined
rendering; it does not merely remove the old compatibility guard or copy a full-rate
stereo image into a foveated destination.

## Controls

In the startup room, enable Multiview on the graphics page. In additional graphics options,
select a foveation level, restart the app to activate it, and enable Experimental multiview +
foveation. The normal and experimental multiview switches are also on VR Rendering pages.

The foveation tooltip and room description warn that active foveation disables standard
multiview. The experimental option is an opt-in exception, subject to compatibility checks.
With it off, behavior remains the existing per-eye foveated renderer.

## Render path

- Before acquiring images, the renderer checks normal stereo restrictions and probes the
  exact stereo pipeline set. Shader/native render hooks, unsupported effects, screenshots,
  incompatible GPU timing and other existing restrictions still select per-eye rendering.
- On a compatible Vulkan device, OpenXR creates a **two-layer foveated swapchain**. Its own
  density images are imported as two-layer R8G8 views. The device's
  `fragmentDensityMapNonSubsampledImages` capability is queried and enabled when supported;
  the experimental path requires it for ordinary wgpu depth attachments.
- The color image acquired for this frame becomes the multiview color target directly.
  A private two-layer depth image is retained across frames. There is no intermediate
  stereo color target or layer-to-mono image copy on this path.
- Both projection views reference the same swapchain, with image-array indices 0 and 1.
  That swapchain is acquired/waited/released once per frame. HUD and overlay swapchains
  remain separate.
- Native swapchain/density-map refusal disables further attempts for the session and keeps
  the original mono eye pair. A target/pipeline failure after acquiring an array discards
  that projection, releases all acquired images and selects per-eye rendering next frame.
  Restart the app to retry a failed native path. Normal compatibility fallback is automatic.
- Resizes retain old array chains until queued GPU work has completed, with a bounded
  retired list. If replacement is temporarily unavailable, the mono path can draw at the
  current configured size.

The fallback mono swapchains are retained, so this experiment may use more memory than
ordinary multiview. Runtime refusal and device/driver failure are not the same thing:
application-level fallback cannot promise recovery from every GPU driver fault.

## Confirming the active path

The log must say:

`Quest world rendering: single-pass multiview (native foveated array)`

An enabled checkbox alone does not establish that both features are active. The GPU profile
continues to count actual multiview frames. Initial device logs report non-subsampled image
support; refusal logs explain why the native path was rejected.

## Validation limits

Software Vulkan tests can verify eye separation and direct rendering into a supplied color
array, alongside the existing intermediate/copy path. They cannot validate Meta's density
images, OpenXR array swapchain acceptance, foveated render-pass compatibility or Quest FPS.
Those require a real Quest and preferably Vulkan validation enabled.

Device checks: both eyes and peripheral image quality, sleep/resume, switching the experiment
off/on, menus, unsupported effects/shader fallback, screenshots, resolution changes and
dynamic resolution, prolonged memory/thermal behavior, and comparison with ordinary FFR
and ordinary multiview at identical resolution/refresh rate. Foveation level changes still
require an app restart. No APK or headset benchmark is produced by the local checks.

Completed locally: 359 input/settings/XR library tests, 3 multiview checks (shader
validation, intermediate copy and direct external color-array rendering on Vulkan),
and the room-step test passed. Workspace/all-target Clippy with warnings denied,
formatting, layer and public-tree checks passed. No Meta/OpenXR hardware validation
is implied by these results.
