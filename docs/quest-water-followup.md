# Quest water geometry follow-up

The reported symptom is a similar CPU/GPU drop when looking at visible water
and at water hidden behind blocks, provided its chunks are in the view direction.
This points towards geometry/preparation costs, but is not a substitute for a
headset timing capture. No percentage FPS gain is asserted here.

## Changes

The headset's built-in terrain path now retains tight bounds for translucent
geometry separately from the section's solid geometry. Water outside the eye's
frustum (or the union frustum in multiview) no longer enters the translucent sort
and draw list just because the same section has visible solid blocks.

For horizontal, rectangular quads in exactly one plane, a mesh-upload-time
sweep proves that rectangles of each winding do not overlap. Those surfaces
cannot blend over one another, so moving the camera does not require re-sorting
or re-uploading their index order. Texture UVs, colors, lighting and animation
remain unchanged; water is not merged, replaced with dither, or rendered at a
lower resolution.

This fast path also omits windings facing away from **every** current eye before
indices reach the GPU. A double-sided planar mesh above or below the viewer uses
6 indices per surface instead of 12. That reduces visible-water geometry and,
with Quest's flat draws, the compute copying that assembles its index list.
Hardware back-face culling previously discarded those triangles only after
vertex processing. When eyes straddle the plane, both windings are retained.
Crossing the surface updates the indices immediately, including one-sided
surfaces whose draw count can become zero.

Mixed heights, slopes, overlapping rectangles, malformed geometry and shader
packs keep regular sorting. Switching render paths restores the complete index
list, including sections used only by a shadow view. Native render hooks also
keep the original path. The optimization applies to near sections and distant
LOD meshes, to separate-eye rendering and to multiview.

## Limits and validation

Tight bounds are frustum culling, **not** depth occlusion: water behind a wall
can still be submitted. The planar fast path nevertheless reduces its geometry
and sorting cost just as it does for visible water. Visible fragment shading is
unchanged; a remaining pixel/fill bottleneck would need a separate measured
optimization. This change does not implement stereo depth-based GPU occlusion.

- Vulkan pixel comparison passed for views above, below and straddling the
  surface, using alpha blending, back-face culling and both multiview layers.
  The original and optimized images match byte for byte; the test checks that
  each reference eye actually sees the surface.
- Targeted tests cover movement without uploads, winding changes, zero-count
  recovery, shader-path restoration, tight bounds and classifier rejection.
  A deterministic randomized overlap test compares the sweep with an independent
  pairwise rectangle oracle.
- Full renderer run: 1397 passed, 7 baseline-listed failures, 45 ignored. The
  last two additional CPU regressions were subsequently verified in the targeted
  suites (8 transparency tests and 3 planar-classifier tests).
- Workspace/all-target Clippy and client/no-default-features Clippy pass with
  `-D warnings`; formatting and public-tree checks are recorded with the change.

On Quest, compare the same position and settings while moving along a large
flat water surface, then while stationary looking at it, behind an opaque wall,
and away from it. Also check shores, flowing water, diving through the surface,
LOD transitions and resource/shader-pack reloads. Keep render scale and distance
fixed. Actual frame-time improvement remains a headset check.
