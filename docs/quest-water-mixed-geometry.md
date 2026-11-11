# Quest water: mixed-section geometry follow-up

The report after upstream integration is 120 FPS away from water and around 80–90 FPS
looking toward water, using retained settings. This is a device observation, not a local benchmark.

## Findings

The upstream merge preserved the original planar-water filter and its render-loop calls.
There is no confirmed deletion or bypass of that filter in the default headset path.
However, its sorting shortcut requires the **entire translucent mesh** to consist of
non-overlapping rectangles at exactly one height. A bank, a bottom face, another water
level, glass, or flowing water disables the shortcut for the whole section. The old
unit/GPU fixtures covered an isolated horizontal surface, not this mixed case.
This limitation predates the merge; without a matched device capture it does not establish
the sole cause of the reported regression.

## Change

Keep the original flat-surface shortcut. For other headset translucent meshes, classify
individual axis-aligned rectangular quads once at upload and group them by oriented plane.
Remove a face only when it is back-facing for **both actual eye positions**. Unclassified
slopes, irregular quads and ambiguous near-plane faces stay in the draw list.
The remaining faces keep vanilla's stable distance order. Plane visibility is checked
without traversing all quads every frame; index sorting/upload occurs only when the cached
viewpoint or visibility changes. Custom shader/native render paths restore the full index
list. World-warp paths also bypass this positional optimization.

This reduces submitted geometry for shores and mixed-height water without reducing
resolution or changing water transparency. It does not implement hidden-surface occlusion
behind arbitrary walls, nor prove a particular FPS gain on Quest.

## Validation

Regression tests cover a double-sided shore mesh rejected by the old shortcut, exact
retained sort order, cached updates, stereo eyes straddling a vertical plane inside one
camera block, and full restoration when leaving the optimized path.
Vulkan multiview pixel comparisons cover the original flat case and mixed banks/multiple
surface heights from above, below and with eyes straddling the surface.

Completed local checks: 12 geometry/sorting unit tests and both Vulkan multiview
pixel-comparison tests passed. Renderer Clippy (`--all-targets -- -D warnings`),
workspace formatting, layer and public-tree checks passed.

Remaining device checks: repeat the same scene and settings on Quest before/after this
revision, above/below water, near banks, glass and waterfalls, both multiview and per-eye
rendering, then compare frame time and thermal behavior over a longer session.
