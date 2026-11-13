# Quest split terrain vertex streams

Baseline: `946c885498` (the GPU handoff). This implements the split-stream work item;
it does not enable compute culling, change render/LOD distances or alter shader math.

## Format and scope

The CPU/mesher `TerrainVertex` remains its existing 24-byte ABI. Uploads deinterleave it
without converting any values:

| GPU binding | Stride | Attributes and offsets |
| --- | --- | --- |
| 0, primary | 16 bytes | position: Float32x3 at 0; layer: Uint16 at 12; light: Uint8x2 at 14 |
| 1, instances | 16 bytes | unchanged section origin/step |
| 2, secondary | 8 bytes | UV: Uint16x2 at 0; color: Unorm8x4 at 4 |

The opaque vertex shader uses the layer to reject cutout/fading quads. Keeping it with
position lets a position/layer-only binning phase fetch the 16-byte primary stream,
instead of needing both buffers. Light fills the two remaining primary bytes; total
vertex payload stays **24 bytes**, not 28 or 36. No quantization, precision reduction,
geometry removal, UV/color/light adjustment or change to index/base-vertex numbering.
Whether Adreno's compiler/fetch hardware realizes a performance gain still needs an A/B test.

The implementation uses ordinary Vulkan vertex buffer bindings, with at least three
bindings required. It is opt-in on Quest/Qualcomm Vulkan and excludes flat/storage-pulled
terrain and initial shader-pack layouts. Other backends and the default path stay interleaved.

## Enable and compare

Add this line to the headset's `launch.txt`, then restart the app:

```text
HORIZON_QUEST_SPLIT_STREAMS=1
```

`0` or an absent variable uses the original path. The setting is read at renderer creation;
it cannot change the layout of live arena pages. Startup logs report the enabled state,
requested state and flat/shader-pack gates.

With the handoff's existing A/B script, use identical saved camera/settings and run back to back:

```bash
bash ab.sh "interleaved" "HORIZON_QUEST_SPLIT_STREAMS=0"
bash ab.sh "split16+8" "HORIZON_QUEST_SPLIT_STREAMS=1"
```

Compare aggregate world GPU time and vertex-fetch stall / vertex traffic from
`ovrgpuprofiler`. Do not use the tiled GPU's zero-valued per-subpass timers. Geometry
counts and the image must remain unchanged. Reverse A/B order to reduce time/weather bias.
Take screenshots of both eyes, including sky, water, foliage and the near/LOD seam;
check rotation, chunk streaming, teleportation, resource reload and arena shrink/compaction.
The extra CPU deinterleave happens on upload, not every frame; compare streaming CPU time too.

## Arena and pipeline handling

Each split page owns a paired primary/secondary allocation. Allocation failure leaves
existing pages valid. Both streams grow, move, shrink, retire and clear together, sharing
logical page/range indices. Logical 24-byte accounting is retained; driver allocation
alignment/metadata may add overhead. Upload scratch vectors are reused per arena and
retain capacity for the largest upload. Indirect draws bind the secondary stream as well.

Section and translucent arenas use matching layouts, including arena recreation. Built-in
and attribute-based shader-pack pipeline layouts follow the same binding arrangement;
asynchronous shader compilation captures the layout flag without retaining the renderer.
Flat/pulled layouts remain excluded rather than interpreting split buffers as packed structs.

## Validation

- A bitwise reconstruction test covers all byte fields, flags and arbitrary float payloads.
- A Vulkan multiview pixel comparison renders the same vertices through interleaved and
  split layouts for both eyes, including indexed draws with a nonzero base vertex.
- Arena upload → grow → cross-page copy → readback → paired retirement is covered.
- Full renderer suite: **2165 passed, 4 failed, 74 ignored** in this cloud environment.
  All four failures were rerun against unchanged `946c885498` and reproduce there:
  `crash::tests::a_panic_becomes_a_report`,
  `crash::tests::the_debug_crash_is_reported_as_vanillas`,
  `gfx::hdr::tests::sdr_screenshots_roll_off_the_world_and_keep_the_hud_as_drawn`,
  `gfx::room_scene::tests::the_room_is_drawn_from_every_side_before_and_after_the_real_textures`.
  This environment's failures differ from the three recorded in the headset handoff;
  neither list should be silently substituted for the other.
- Renderer/all-target check and Clippy completed. The existing vendored wgpu-core
  `unfulfilled_lint_expectations` warning remains.

There is no headset/ADB access in this cloud session. APK building, real-pack emissive
appearance and Quest GPU counters/FPS are not verified. The option remains off by default
until device A/B validation; no numerical FPS gain is claimed.
