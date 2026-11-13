# Quest Application SpaceWarp

The existing VR render-rate slider now includes opt-in Application SpaceWarp modes. The
saved `horizonRefreshRate` and `HORIZON_XR_HZ` override name **application render FPS** for
these low values. A launch override supplies the initial value shown in the UI; explicit
slider or room +/- edits take precedence for the rest of that process. Native 72/80/90/96/100/120 Hz retain their usual meaning; the default
remains native 90 Hz. This uses `XR_FB_space_warp`, independently of desktop FSR frame
interpolation.

| Render FPS | Display Hz | Render frame budget |
| ---: | ---: | ---: |
| 30 | 60 | 33.33 ms |
| 36 | 72 | 27.78 ms |
| 40 | 80 | 25.00 ms |
| 45 | 90 | 22.22 ms |
| 48 | 96 | 20.83 ms |
| 50 | 100 | 20.00 ms |
| 60 | 120 | 16.67 ms |

The UI filters these choices by the runtime's advertised display rates and SpaceWarp
capability. In particular, 30 FPS is offered only when a runtime actually offers 60 Hz;
this is not a promise that a Quest permits a 60 Hz interactive display mode. 96 and 100 Hz
are likewise conditional. Unsupported half rates cannot be selected with the slider or
the startup room's +/- buttons. Before capabilities arrive, the UI offers native rates.
The room distinguishes a saved SpaceWarp target from the actual display refresh and
whether the extension was submitted. Its own menu scene runs at native refresh.

Start with 45→90 if CPU **and GPU**, including the extra work described below, stay below
22.22 ms. If that budget cannot be met, try 36→72 (27.78 ms), or 40→80 where available.
SpaceWarp does not make arbitrary sub-36 FPS rendering meet the 36→72 schedule. Leave
margin for world streaming, thermal throttling and OS work rather than selecting a mode
whose budget is only met on average.

## What the implementation submits

- Lazily allocated two-layer `RGBA16F` motion and `D32F` depth swapchains at the runtime's
  recommended auxiliary resolution. They are sampled, unfoveated attachment images,
  separate from the ordinary eye colour swapchains. With world multiview enabled, one
  multiview auxiliary render pass writes **both eyes' motion and depth**; camera
  reconstruction and animated-object overrides share that pass. The separate-eye path
  is used only alongside the main renderer's separate-eye fallback. Constant uniform
  indices avoid dynamic indexing of the eye matrices on Adreno.
- One `XrCompositionLayerSpaceWarpInfoFB` in each projection view's `next` chain, with
  matching eye array indices. The projection's existing super-resolution settings and
  the HUD/overlay quad layers are preserved.
- Forward **current minus previous XYZ NDC** vectors, including camera motion. These
  differ from desktop FSR's backwards XY UV convention. Static surfaces are reconstructed
  from retained world depth, with camera matrices precombined on the CPU to save a
  matrix multiply per auxiliary pixel; independently moving entity vertices and world hands
  override those vectors, using stable identity/topology matching and occlusion tests.
- Camera-relative vertex history retains double precision when changing world origins.
  GPU entity posing falls back to CPU posing while SpaceWarp is selected and supported,
  so limbs have actual previous geometry. The shared CPU tracker reuses vertex allocations.
  Selection policy is cached once per headset frame; per-model skinning checks do not
  acquire headset/settings locks or read environment variables.
- Reversed-Z metadata maps depth zero to the world far plane and depth one to the near
  plane, both converted from blocks to room metres using world scale. Artificial
  locomotion's incremental app-space pose excludes ordinary room tracking.
- First frames, recentering, resolution changes, world-scale changes, teleports and large
  artificial turns suppress extrapolation with `FRAME_SKIP`. The runtime controls frame
  pacing; no application sleep or simulated FPS cap is used. Dynamic resolution uses the
  half-rate frame budget while SpaceWarp is active.

Acquired auxiliary images are released even when rendering is discarded. Both eye layers
must have been rendered before the extension is attached. Swapchain allocation/acquisition
or frame-submission failure disables SpaceWarp for the session and retains native display
refresh. A requested half rate whose doubled display rate is unsupported or has not yet
become active does not submit SpaceWarp data.

Motion uses the renderer's NDC (+X right, +Y up, current reversed Z). Device motion
conventions are not fully specified by the FB extension; the Quest check must explicitly
cover vertical head/object motion as well as depth motion. This buffer convention must
not be reused blindly for `XR_EXT_frame_synthesis` or another runtime's NDC convention.
The compositor-side interpretation has not been verified locally.

## Compatibility and costs

SpaceWarp currently runs in ordinary world rendering, with either separate eye passes or
multiview, including Quest colour foveation. GUI hands/crosshairs draw on top without
clearing the retained world depth; their auxiliary vectors and depth override the world
in the same motion pass. Menus, shader packs/native world hooks, HDR, visibility-buffer rendering, improved transparency,
custom core lines and world/screen post-processing use native refresh. Compatibility
changes and actual activation/deactivation are logged. Choosing a native slider value
turns SpaceWarp off.

On Adreno, storing full world depth costs memory bandwidth; the auxiliary passes and CPU
entity posing also cost time. Auxiliary images use the runtime's smaller recommended
resolution and are handed back using explicit attachment-layout transitions, avoiding
empty load/store passes over them. The runtime's final extrapolation has its own GPU
budget. Local software rendering cannot establish the performance tradeoff on a Quest.

Particles, animated terrain textures, translucent effects and unmatched/new geometry have
camera motion rather than complete independent animation history. Thin edges and
transparency can therefore show extrapolation artifacts. This is an experimental opt-in
mode, not a claim of artifact-free output or of verified Quest frame rates.

## Local validation and headset check

The GPU tests render/read back both eye layers and exercise the separate-eye fallback.
They check signed XYZ motion against independently calculated values, reversed depth,
independent eye uniforms, moving-object overrides and occlusion. Other tests cover slider
round trips and legacy import, conditional rates, and room-versus-artificial locomotion
at multiple world scales. No Quest/OpenXR runtime was available for this implementation.

On a headset, compare native 72/90/120 with 36→72/45→90/60→120 in the same warmed RD24
scene. Check runtime frame statistics and logs for actual SpaceWarp activation, head and
controller motion, moving mobs, room walking, locomotion, snap turns, teleport/recenter,
world scale, visibility masks, colour foveation, multiview and resolution changes. Open
and close menus and incompatible effects; confirm native fallback and subsequent recovery.
The existing pass timer labels the auxiliary pass `SpaceWarp`, so its GPU cost can be
measured separately when pass timing is enabled; the eye frame's total includes it too.
Record sustained CPU/GPU times, dropped frames and thermal behavior rather than relying
on the display-Hz value alone.

## References

Written against the public contracts; no external application source was copied:

- [Khronos XR_FB_space_warp specification](https://registry.khronos.org/OpenXR/specs/1.1/man/html/XR_FB_space_warp.html)
- [Khronos extension source and motion convention](https://github.com/KhronosGroup/OpenXR-Docs/blob/main/specification/sources/chapters/extensions/fb/fb_space_warp.adoc)
- [Meta native Application SpaceWarp guide](https://developers.meta.com/vr/documentation/native/android/mobile-asw/)

### Validation results (local)

Rust 1.99.1, Mesa 25.0.7 lavapipe / LLVM 19, Vulkan, debug information disabled:

- 397 tests passed across input (196), VR settings (144), OpenXR (25) and threads (32).
- Four SpaceWarp checks passed: actual multiview camera/depth readback, moving/occluded
  objects and GUI overlay depth, a device without MULTIVIEW, and portable SPIR-V
  translation of both shader variants. Eye matrices/lenses and source depths differ.
- The existing DX12/OpenGL shader translation test passed; Vulkan-only SpaceWarp is
  covered by its own SPIR-V test instead of requiring FXC shader model 5.1 to support
  `ViewIndex`.
- All-target Clippy with `-D warnings` passed for the changed support/render crates and
  `horizon-client --no-default-features`. The vendored wgpu-core still emits its existing
  unfulfilled-lint-expectation warning.
- Full renderer run: **2191 passed, 4 failed, 77 ignored**. The failures have the same
  names as those observed in the earlier audit: two crash-report source-location tests
  with debug information disabled; strict HDR readback red `0.9038086` versus `0.89990234`;
  room-scene colour `[195,28,17]` versus `[194,28,17]` on software Vulkan. This is not a
  clean full-suite result. A separate unchanged-base build was not performed here.
- Python suite: 831 tests OK, four skips. Formatting, public-tree, crate-layer and file
  size checks passed.

An APK/headset run and runtime extrapolation quality/performance remain unverified.
