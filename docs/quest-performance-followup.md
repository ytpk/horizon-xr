# Quest performance follow-up

This change targets standalone Quest rendering. It does not promise 120 FPS on
Quest 2 or Quest 3S: the compositor, GPU bandwidth, simulation, scene and thermal
state still matter. No render-distance or resolution reduction is enabled by default.

## Startup room

Open **Settings → Graphics** before starting the game:

- Resolution: explicit linear scale presets, with pixels per eye computed from
  the runtime's recommendation and maximum. Existing environment overrides win.
  The selected resolution applies when the game starts drawing its world.
- Refresh: select only rates advertised by the runtime. The saved default is
  still 90 Hz; choose 120 Hz explicitly if the runtime offers it. The page also
  reports the current display rate, which can differ from the requested rate.
- Multiview: experimental, off by default. Pipeline variants are built when the
  game renderer is constructed after Play. Compare it separately from other knobs.
- VR performance: optional GPU-aware adaptive resolution, Meta Quest Super
  Resolution, and fixed foveation. All are off by default.

Fixed foveation is read when the XR instance/session is created: changing its
saved level requires restarting the app, not just pressing Play. Existing
`HORIZON_XR_FOVEATION` overrides still apply. A requested level can fall back if
extensions, density maps or GPU support are absent. The historical runtime
limitation in `visor-port.md` has not been verified as resolved on a headset.

## Actual single-pass multiview

The built-in world shaders carry `view_index` in vertex and fragment stages.
Two projections, inverse views and inverse projections share a trailing stereo
uniform block; the ordinary shaders retain their previous Globals ABI. The
main world pass uses a mask of `0b11` and a two-layer color/depth target. Terrain,
ordinary water/glass, static and dynamic entities, hands, clouds, sky, sunrise,
stars, weather and the End sky use this path. Visibility and geometry preparation
run once for the stereo world render, using the existing union cull frustum.

The current OpenXR interface retains separate per-eye swapchains. Each internal
layer is copied to its runtime image in the same command buffer as the world.
This is real multiview, but it adds color-copy bandwidth and intermediate target
memory. Direct rendering into a native two-layer OpenXR swapchain remains future
work and is likely more valuable when bandwidth is the bottleneck.

Compatibility fallback preserves the selected picture. Shader packs, native
render hooks, HDR, improved transparency/OIT, world post scaling/FXAA, post
effects, glowing silhouettes (including GPU-skinned outlines), world borders,
custom line shaders, the debug crosshair, visibility-buffer mode, screenshots,
and active runtime foveation currently use the separate-eye path. Devices must
offer at least two multiview views; the timestamp marker fallback is also kept
on the separate-eye path. Failed target allocation disables attempts for that
renderer instead of retrying a large allocation every frame.

Depth-based GPU occlusion/GPU-driven section culling is not reused from one eye
for the stereo pass: that could hide geometry seen by the other eye. Multiview
uses the existing conservative CPU lists and cave/union-frustum culling instead.
The mono paths keep their existing GPU culling. The optional hidden-area mask is
currently not applied in multiview: extra invisible corner pixels are rendered.
These tradeoffs can outweigh reduced CPU submissions in some scenes.

HUD, keyboard and world overlay panels now share one command buffer with the
runtime-layout transitions. Their uploads remain ordered before their draws.
The explicit `HORIZON_XR_LAYOUT=barrier` experiment is unchanged; the proven
load/store transition path remains the default.

## Dynamic resolution and MQSR

Adaptive resolution used to react to frame intervals alone, including CPU stalls.
It now requires recent complete stereo GPU-render estimates. Untimed/stale
samples and CPU-limited frames do not lower resolution. Render-path and size
changes invalidate feedback while delayed timestamp slots drain. Hysteresis,
allocation failure backoff and a floor of 85% of the selected dimensions limit
quality changes. A disabled adaptive option restores the configured eye size.
Retired XR swapchains stay alive until submitted GPU work is complete; pending
submit-thread work is landed before beginning the next XR frame.

MQSR requests `QUALITY_SHARPENING` through an advertised and enabled
`XR_FB_composition_layer_settings`, only on the world's projection layer. HUD
panels keep their own native resolution. Enabling MQSR alone does not lower the
render size. The compositor can decline the hint and filtering itself costs GPU
time. This is not an assertion of lossless reconstruction or a fixed FPS gain.

Primary API references:

- [Meta composition-layer filtering / MQSR](https://developers.meta.com/vr/documentation/native/android/mobile-openxr-composition-layer-filtering/)
- [OpenXR composition-layer settings](https://registry.khronos.org/OpenXR/specs/1.1/man/html/XrCompositionLayerSettingsFB.html)

## Reading profiles and comparing on a headset

An approximately 11.11 ms XR wait can be 90 Hz pacing even with little GPU work.
The profile now labels paced spans separately from app spans excluding XR calls
and nested eye waits; neither is hardware CPU utilization. GPU samples are per
world render: one eye on the legacy path, both eyes on multiview. Mode counts
and logs reveal compatibility fallback, and timestamp samples arrive late.

1. Keep the world, position, distance, resolution and effects fixed. Compare
   dry land and the same view of water separately at 90 Hz, then 120 Hz.
2. Start with all optional quality controls off. Compare multiview alone,
   checking binocular projection, skies, clouds, hands, inside-block outlines,
   transparent faces, chunk edges and loading transitions.
3. Toggle one quality control at a time. Check actual display rate and compositor
   timing as well as app timing. Foveation requires an app restart.
4. Exercise each fallback and return to the same scene. Check that nothing
   disappears or changes resolution simply to keep multiview active.
5. Test adaptive recovery and disabling it, resolution changes, suspend/resume,
   reconnecting and long sessions/thermals on both Quest 2 and Quest 3S.

Replacing glass/water blending with dither, synthesizing frames with SpaceWarp,
or replacing texture reads with procedural math is not enabled here. These are
not generally equivalent pictures or guaranteed speedups. SpaceWarp requires
correct motion vectors/depth; procedural math versus texture access needs actual
shader measurements. They remain separate potential work.

## Verification

Checked on Debian 13, Rust 1.99.1 and Mesa lavapipe 25.0.7 (software Vulkan).
No Quest hardware, compositor performance or Android APK validation is implied.

- Workspace Clippy, all targets, `-D warnings`: passed.
- Client Clippy with `--no-default-features --all-targets -D warnings`: passed.
- Formatting, staged diff whitespace, translation JSON uniqueness and public-tree
  checks: passed.
- Input / Visor / Vivecraft / XR unit suites: **375 passed**, including GPU-bound
  versus CPU-bound adaptive resolution, allocation backoff, quality presets,
  offered display rates, filter availability and MQSR's exact flag/chain.
- Full renderer suite: **1392 passed, 7 failed, 44 ignored**. All seven failing
  case names belong to the previously established baseline failure list: two
  debug-stack checks (debug information is disabled in this environment), four
  HDR attachment-budget checks, and the room's strict pixel-color check (one
  channel differs by one). No new failing cases remain.
- The normally ignored Vulkan multiview pixel test was explicitly run and
  **passed**. It exercises the production shader/pipeline variant cache, two
  independently projected layers, and copies each layer to a separate runtime-
  shaped texture. All four built-in stereo shader sources validate with Naga.
- The seven profiler tests pass, including separating 9 ms of runtime pacing
  from 1.5 ms of estimated application spans.
- Python suite: **644 cases, 637 passed, 4 failures, 2 errors, 1 skip**. The six
  failing/error case names exactly match the prior baseline (README parity,
  game-home/audio script policy, remote-mining fixture and remote rocket fixture).

These results establish desktop source/build consistency and selected GPU
behavior, not sustained Quest frame rate. Headset checks listed above remain
required before treating the experimental paths as verified on device.
