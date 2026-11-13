# Quest render CPU audit — 2026-11-11

Source baseline: `cb9760ef25`. Static code audit, not measured Quest profiling.
The reported 120 → 80–90 FPS water case does not by itself distinguish CPU work,
GPU work, runtime pacing or thermal/clock limits. XR waits are not CPU utilization.

## Already implemented

- Single-pass multiview with compatibility fallback (`gfx/quest_render.rs`).
- Exact nearest-first bucket sorting with reusable buffers (`gfx/resident.rs`).
- Vulkan pipeline caching (`gfx/reuse.rs`), reusable staging belt (`gfx/upload.rs`).
- Bounded streaming work (`streaming/bounded.rs`), visible animated-sprite masks.
- Flat-water sort avoidance, mixed-section back-face filtering, translucent bounds,
  cached index updates and restored indices for incompatible paths (`gfx/translucency.rs`).

These should not be counted again as new optimization proposals.

## CPU opportunities, in priority order

| Priority | Finding and source | Proposed action | Quality / limitation |
| --- | --- | --- | --- |
| 1 | Per-eye fallback calls `render_flat` twice (`gfx/quest_render.rs`), including visibility traversal, section grouping and parts of frame preparation. | Share frame-invariant visibility, instance generation, animation masks and upload preparation between eyes. Keep eye-specific uniforms, masks and passes separate. | No intended image change; must key caches by frame, mesh generation, camera, union frustum, shader and LOD state. Do not reuse one eye's frustum alone. |
| 1 | `stereo_compatible` explicitly rejects active foveation. | Expose the effective render mode and fallback reason in profiling/UI; investigate native stereo swapchain/foveation integration before attempting simultaneous support. | Current enabled checkboxes do not establish that multiview and FFR execute together. Simply removing the guard does not implement foveation on intermediate stereo targets. |
| 1 | `gpu_frame` requires `self.stereo_views.is_none()` (`gfx.rs`). | Evaluate stereo-safe GPU culling with conservative visibility for both eyes. | Not a one-line removal of the guard: depth/Hi-Z and draw preparation must support stereo. Avoid GPU→CPU synchronous visibility readback. Benchmark Adreno CPU and GPU costs. |
| 2 | Transparent quad sorts use stable `sort_by` even though each quad has an original unique index (`gfx/translucency.rs`). | Use in-place sorting with original index as the equal-distance tie-breaker. | Same ordering as vanilla; removes sort scratch allocations, not all memory allocations or sorting cost. Applied in this revision. |
| 2 | Water visibility is derived from the active view list; per-eye rendering supplies one view. | Feed both eye offsets to the water filter in both stereo modes. | Prevents face-mask/index churn when eyes straddle a bank; retains all faces visible to either eye. Applied in this revision. |
| 2 | Multiview pipeline swapping constructs a slot vector and a vector of alternate handles twice per frame (`swap_stereo_pipelines`, `Reuse::alternates`). | Cache compatible pipeline sets by renderer generation and rebuild on shader/pipeline changes. | Small expected CPU benefit; preserve atomic fallback and invalidate on resource-pack/device changes. |
| 2 | Translucent sections still contribute separate draw commands and sorted index buffers. | Measure grouping/indirect submission costs, cache unchanged command groups and index uploads. | Preserve global back-to-front order across pages and near/far water tiers. Arbitrary material batching changes blending. |
| 3 | Upload and meshing budgets compete with the 8.33 ms frame budget at 120 Hz. | Budget streaming preparation against measured active frame time with bounded queues and guaranteed progress. | Preserve draw distance and eventual detail; may delay chunk appearance. Not a free steady-scene FPS increase. |

`wait_frames` already limits GPU submissions and uses OpenXR pacing. Removing waits can
increase latency, memory usage or queue depth; 11 ms of waiting is not evidence of 11 ms
of CPU computation. Profile active render preparation separately.

## Optional opaque / dense water

This mode is reasonable **off by default**, with the normal mode retaining current visuals.
It intentionally hides scenery below/behind the water surface; it is not quality-neutral.
Textures, biome tint, lighting and underwater fog can remain.

A fragment-only `alpha = 1` toggle is insufficient for CPU optimization: water would still
be sorted, use translucent draw lists and run blending-capable passes.
For a meaningful implementation:

1. Identify water in the mesher while fluid kind is available; do not infer it from all
   translucent blocks or a hard-coded texture layer. Glass and lava retain their behavior.
2. Route opaque-mode water to a depth-writing, non-blended terrain queue. Reuse solid-fluid
   shading where valid (lava already follows the solid mesh path), retaining correct normals,
   textures, face culling and lighting. Handle near and distant water consistently.
3. Exclude that water from transparency sorting, index reuploads, OIT/translucent HDR passes.
   Front-to-back submission can then reject covered terrain/water earlier on the GPU.
4. Remesh loaded sections and invalidate LOD/sort caches on a mode change; ensure pending
   worker results cannot restore geometry built for the previous setting.
5. Define resource-pack/native-shader compatibility explicitly; shader packs that require
   water in the translucent pass need fallback, not silent material reassignment.
6. Test surface/below-surface views, shores, waterfalls, ice/glass intersections, chunk/LOD
   transitions, mode changes during meshing, and both stereo rendering paths.

Expected benefit: less CPU sorting/index traffic and potentially less GPU overdraw.
The world behind the water is not automatically removed from CPU visibility or submission;
that requires valid occlusion integration. No FPS percentage is justified before measurements.
Follow-up: the opaque-water switch is now implemented as `horizonOpaqueWater` (off by
 default), on the startup-room graphics extras page and the VR rendering pages. Both near
 and far meshers use an immutable worker-context flag; mode changes rebuild loaded sections
 and reset LOD. Shader-pack selection uses the normal transparent material path.

## Suggested implementation sequence

The two small behavior-preserving changes above come first. Next share per-eye CPU frame
preparation and make actual fallback reasons visible. Then add opaque water as a separate
optional material mode. Stereo GPU culling and combined multiview/FFR are larger follow-ups
requiring device measurements and stereo-specific rendering support.

## Validation of the applied changes

Nine translucency regression tests and two Vulkan multiview pixel comparisons passed.
These cover equal-distance mesh order, mixed shore geometry, stereo plane crossings and
index restoration. Eye view matrices are also computed once per frame and reused in both
rendering paths. Renderer Clippy (`--all-targets -- -D warnings`), formatting, layer and
public-tree checks passed. Headset CPU time/FPS and the actual per-eye integration still
require device testing; the GPU fixtures are not a full OpenXR runtime test.


Follow-up: [experimental native foveated multiview](quest-foveated-multiview.md)
adds an opt-in native stereo swapchain path; the default FFR/multiview exclusion remains.
