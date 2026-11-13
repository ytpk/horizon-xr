# Quest GPU optimisation: handoff

State of the Quest 3S (Adreno 740, Vulkan, OpenXR) rendering work after commit `5b16366`, and what to do
next. Read [CLAUDE.md](../CLAUDE.md) first. Goal: a steady 120 fps (8.33 ms GPU per frame, both eyes in one
multiview pass) at a high render distance, with headroom left for simple shaders. Vanilla's picture stays
the spec: every optimisation is gated and has an env switch to turn it off.

## How to measure on the headset

Profiling on this GPU only works through A/B runs and the GPU's own counters. The in-game per-pass GPU
timer shows `0.00` for every sub-pass on a tiled GPU; don't trust it.

- Build and install (~4–6 min incremental; the full `build-apk.sh` needs JDK 25, the APK stage does not):
  `HORIZON_MSA_CLIENT_ID=$(cat dist/msa_client_id) OUT=$PWD/dist/out bash packaging/quest/build_apk.sh && adb install -r dist/out/horizon-quest.apk`
- The headset is reachable over `adb` (wireless). `adb shell am broadcast -a com.oculus.vrpowermanager.prox_close`
  makes it render while lying on a table.
- Device files: `/storage/emulated/0/Android/data/dev.horizon.quest/files/`. `launch.txt` holds extra
  arguments and `NAME=value` env lines, one per line. For measuring it has `--world VisorTest` and
  `HORIZON_QUEST_ROOM=0` (skips the launcher room and sign-in). `home/options.txt` has `renderDistance:24`
  (the reference load); the player's earlier files are kept as `options.txt.agent-backup` and
  `launch.txt.user-backup`.
- GPU counters: `adb shell 'timeout 4 ovrgpuprofiler -r="7,25,26,29,30,32,33"'` (vertex fetch stall, % time
  vertices, vertices/s, bytes and ALU per vertex, % time fragments, fragments/s). `ovrgpuprofiler -m` lists all.
- Frame log: `adb logcat -d -s horizon:I` — the `Horizon XR: frame profile` line every 5 s gives fps,
  `gpu per world render`, chunks, LOD tiles, draw calls and indices. Also logged: `Quest world rendering: …`
  (which stereo path), `Quest multiview not usable: <reason>`, `Horizon renderer: terrain path: …`.
- Screenshots: `adb exec-out screencap -p > shot.png` (3664×1920, left eye is the left half).
- World time and weather change while it runs, and they move the numbers by ~1–2 ms: only compare runs
  taken back to back.

A/B script used for every measurement below (run it with bash; zsh doesn't split unquoted words):

```bash
#!/usr/bin/env bash
# ab.sh "label" "ENV=1 ENV2=x" ["vivecraft key=value ..."]
# Restarts the game straight into VisorTest with extra launch.txt lines, waits for the
# world to settle, prints fps, GPU ms, counts and GPU counters, then removes the lines.
D=/storage/emulated/0/Android/data/dev.horizon.quest/files
label=$1; envs=$2; vr=${3:-}
adb shell "sed -i '/^#AB$/,\$d' $D/launch.txt; echo '#AB' >> $D/launch.txt"
for kv in $envs; do adb shell "echo $kv >> $D/launch.txt"; done
V=$D/home/vivecraft-client-config.json
if [ -n "$vr" ]; then
  adb shell "cp $V $V.ab"
  for kv in $vr; do k=${kv%%=*}; v=${kv#*=}; adb shell "sed -i 's/\"$k\": \"[^\"]*\"/\"$k\": \"$v\"/' $V"; done
fi
adb shell am broadcast -a com.oculus.vrpowermanager.prox_close >/dev/null
adb shell am force-stop dev.horizon.quest; adb logcat -c
adb shell am start -n dev.horizon.quest/android.app.NativeActivity >/dev/null
sleep ${AB_WAIT:-80}
echo "== $label"
adb logcat -d -s horizon:I horizon:W | grep -i "experiment\|multiview not\|Quest world rendering" | sort -u -k6 | cut -c50-200 | head -4
adb logcat -d -s horizon:I | grep "frame profile" | tail -2 | grep -o "[0-9.]* fps\|gpu per world render[^,]*\|indices [0-9]*" | paste -sd' '
adb shell 'timeout 4 ovrgpuprofiler -r="7,25,26,29,30,32"' 2>&1 | tail -6 | sed 's/  */ /g' | paste -sd'|'
adb shell "sed -i '/^#AB$/,\$d' $D/launch.txt"
[ -n "$vr" ] && adb shell "cp $V.ab $V"
true
```

## What was done since `5b16366`

Measured at render distance 24 in VisorTest, same view, run back to back.

| Step | GPU ms / frame | fps |
|---|---|---|
| Start (foveation and multiview both silently off) | 17.9 | 53 |
| Two-pass solid terrain on Adreno | 13.5 | 69 |
| Emissive lookup out of the vertex shader | **10.4** | **87** |

### Foveation and visibility mask (`crates/horizon-xr/src/lib.rs`)
- **Cause:** Meta's runtime keys compatibility quirks on the patch number of `XrApplicationInfo.apiVersion`.
  We passed 1.0.0. The runtime then dropped `XrSwapchainCreateInfo::next`, so foveated swapchains came back
  with 0×0 density maps, and it returned empty visibility masks. logcat says
  `OpenXR_SessionImpl: Disable SwapchainCreateInfo next pointer on dev.horizon.quest`. The threshold table is
  in `libvrapiimpl.so` of `com.oculus.systemdriver`.
- **Fix:** request 1.1.54, falling back to 1.0.34 if the runtime refuses 1.1.
- **Also:** the instance and device are now created through `XR_KHR_vulkan_enable2`, via creator hooks added
  to vendored `third_party/wgpu-hal` (`CreateInstanceCallbackArgs::creator`, `CreateDeviceCallbackArgs::creator`).
  `HORIZON_XR_VULKAN_ENABLE2=0` keeps the legacy binding.

### Multiview actually on
- `stereo_compatible()` (`gfx/quest_render.rs`) now delegates to `stereo_blocker()`, which logs the first
  blocking condition.
- **Cause:** every in-world frame requests `minecraft:end_of_frame`, which has no file, and it counted as a
  post effect.
- **Fix:** `post_chain::Request::has_chain_files` counts only chains that have a file. It is used in the
  multiview gate, and for keeping the depth buffer, which was also stored every frame for nothing.

### Two-pass solid terrain on Adreno (`gfx.rs` `split_solid_gpu`)
- **Cause:** on Vulkan everything solid went through `fs_solid`, which contains `discard`, so the tiled GPU
  lost early hidden-surface rejection.
- **Fix:** Metal's existing opaque/cutout split now also applies to Qualcomm (`vendor == 0x5143`), and
  `mesher::group_parts` is set to match.
- **Constraint:** GPU culling is not offered while the split is on, because `Sides::thresholds` needs
  ungrouped meshes.
- **Switch:** `HORIZON_SPLIT_SOLID=0|1`.

### Emissive overlay table (`gfx/emissive_table.rs`, `terrain.wgsl` `terrain_vertex`)
- **Cause:** the search loop over `Globals.emissive_layers` cost a third of the vertex shader's ALU just by
  being in the shader, even with 0 pairs; a uniform `if` around it did not help.
- **Fix:** an `R32Uint` texture, one texel per sprite layer (binding 13 of the terrain group), read with a
  single `textureLoad`.

### Multiview shader rewrite (`gfx/multiview.rs`)
- Per-eye data is read through `horizon_eye_<field>()` accessors with constant indices instead of
  `horizon_eyes[horizon_eye]`. About 6% fewer ALU per vertex.

### Right-eye sky artifact, foveated native array path (Fable agent)
- **Cause:** binding both layers of the runtime's density map broke blocks in the right eye's upper
  corners; either layer alone is clean.
- **Fix:** both views use layer 0 (`horizon-xr/src/foveation.rs`). The right eye now gets the left eye's
  fixed-foveation pattern.
- **Switch:** `HORIZON_XR_DENSITY_LAYERS=all|second`. `HORIZON_QUEST_SKY_DEBUG=magenta,nobase,nosunrise,…` is
  kept as a diagnostic.

### Screen effects at the end of the stereo world pass (Fable)
- Saves one load/store of both layers: 13.9 → 12.4 ms in that scene.
- **Switch:** `HORIZON_QUEST_SCREEN_IN_WORLD=0`.

### Hidden-area mask in the multiview pass (`gfx/vis_mask.rs`, Fable)
- No measurable gain; kept, since it is correct.
- **Switch:** `HORIZON_QUEST_STEREO_VISMASK=0`. The mask itself needs `HORIZON_XR_VISMASK=1` or the setting.

### Indirect draws work without wgpu's indirect validation (Fable)
- **Patch:** `third_party/wgpu-core` is 30.0.1 with one line changed (`src/instance.rs`, `from_hal_instance`
  reads the `WGPU_*` variables), wired in through `[patch.crates-io]`.
- **Result:** with `HORIZON_MULTI_DRAW=1`, terrain is drawn only with `WGPU_VALIDATION_INDIRECT_CALL=0`.
  This contradicts the conclusion of [gfx-rs/wgpu#8801](https://github.com/gfx-rs/wgpu/issues/8801).
- **Not a win on its own:** one indirect call per section is slower than plain draws, 15.0 vs 12.1 ms.
- Default behaviour is unchanged.

### Quest occlusion culling from read-back results (`gfx/cull.rs` `recall_*`, Fable)
- Built, but **broken on this GPU**: a fragment shader that writes a storage buffer draws nothing on the
  Quest, with or without foveation, early depth test or indirect validation. Every section therefore reads
  as hidden.
- Off by default; `HORIZON_QUEST_OCCLUSION=1` breaks the picture on the device. It passes its GPU test on radv.

### Developer switches for A/B (in `gfx.rs`)
- `HORIZON_TERRAIN_EXPERIMENT=noemissive,nolightmap,nodynamic,clip,fsnohdr,fsnooverlay,fsplain,fsnotex`
  rewrites the terrain WGSL for measuring. These change the picture.
- The terrain-path log line.

## What the measurements showed (do not re-derive)
- At RD 24 after these fixes, about 9 of the 10.4 ms is **geometry**: ~2.9 M indices, ~1060 sections, about
  2.5 vertex-shader runs per unique vertex (once per view, plus binning).
- Fragment work is minor: removing all texturing saves 0.2–0.3 ms, half resolution 1.5 ms, the HDR and
  overlay branches nothing. Further shader tweaks won't help; less geometry will.
- Quest draws terrain per section through `vs_terrain` with vertex attributes. Flat (pulled) draws are not
  available: `flat.rs::supported` probably fails on `max_storage_buffer_binding_size` (256 MiB pages), not
  confirmed. Even if available they likely cost more (dependent storage loads per vertex).
- Leaf culling modes do nothing with `cutoutLeaves:false`, because fast leaves already drop leaf-to-leaf faces.
- Existing far-terrain LODs (`lodDistance`) draw beyond the render distance and work on the Quest:

| Render distance + LOD distance | GPU ms | fps |
|---|---|---|
| 24 + 0 | 10.4 | 87 |
| 12 + 24 | 8.6–9.5 | 95–103 |
| 8 + 24 | 7.2–8.2 | 108–114 |

- Indirect multi-draw without the validation fix draws no terrain (screenshot-verified).

## Known issues and checks
- The full `cargo test -p horizon-client-render --lib` has 3 failures that also fail on `5b16366`:
  `frame_gen::interpolation::tests::moves_*` (2) and `hdr::tests::the_menu_blur_keeps_the_light_it_blurs`.
- After Fable's last round only clippy and targeted tests were run, not the full suite. Run it.
- Clippy prints one `unfulfilled lint expectation` warning inside the vendored `third_party/wgpu-core` (a
  path dependency).
- The emissive table is covered by unit tests, but has not been seen on the device with a pack that has
  emissive overlays (OptiFine `_e` textures, native mods' `emissive/`). Check one.

## Next tasks, in order

### 1. GPU culling on the Quest via compute (the most promising lever)
`gfx/gpu_culling.rs` decides visibility in a **compute** shader and draws through indirect draws. Indirect
draws now work with `WGPU_VALIDATION_INDIRECT_CALL=0`. Fragment-stage storage writes are broken on this GPU,
but compute writes are a separate unit (wgpu's own validation pass is compute), so test them first.

1. A minimal on-device probe: a compute pass writes a storage buffer, then read it back or draw from it.
2. If compute writes work, find every gate that keeps GPU culling off on the Quest:
   - `gpu_culling::offer` / `supported` (`multi_draw` is false for Adreno; `HORIZON_MULTI_DRAW`);
   - `gpu_frame` requires `stereo_views.is_none()` (`gfx.rs`, search `let mut gpu_frame`);
   - `split_solid_gpu` forces `GpuCullingOffer::Unsupported`, because two-pass meshes (`group_parts`) have no
     `Sides::thresholds`. Either sort meshes ungrouped and draw each part with the matching pipeline, or
     extend the GPU records for two stretches per bucket.
3. Set `WGPU_VALIDATION_INDIRECT_CALL=0` for the Quest by default (not only through the env), with an
   override. Engine-side argument ranges must then be correct, since nothing validates them.
4. Measure against the two-pass baseline. Indirect-per-section was slower (15 vs 12 ms), so GPU culling has
   to remove enough geometry (the audit expects about half the solid vertices at RD 16+) to pay for it.

### 2. Split vertex streams
`TerrainVertex` is 24 interleaved bytes (`mesher.rs`). The binning pass only needs the position, but fetches
all 24 bytes. Put `pos` (and `layer`, which `opaque_quad` reads) in one buffer and the rest in another. The
picture stays identical. It needs arena/upload plumbing on the non-flat path. Target: the 65% vertex fetch
stall.

### 3. Lean varyings for the opaque pass
`fs_opaque` with HDR off doesn't need `local`, `fade` or `height`. A dedicated `TerrainWire` for
`vs_terrain_opaque` drops 4–5 components. The `clip` experiment (vertex outputs only position) gave 17.9 →
12.7 ms before the other fixes, which is the upper bound. Fog distances moved to the fragment shader would
deviate from vanilla's per-vertex interpolation near the camera; keep them per vertex unless you prove there
is no visible difference.

### 4. Occlusion queries instead of fragment storage writes
A fallback for `cull.rs::recall` if GPU culling stays blocked: about 600 query draws a frame. wgpu resets
only the query indices it knows about, while Vulkan multiview uses `views` consecutive queries per query.

### 5. Quest defaults
Consider recommending render distance 8–12 plus LOD 24–48 for the Quest. That's a settings and UI decision
for the maintainer, not an engine change. Check the chunk-to-LOD seam on a wide landscape first; the
measured scene was close range.

## Rules for the next agent
- Test on the headset after every change, with A/B runs and screenshots of both eyes, including sky at
  several times of day: the right-eye artifact showed only in some views.
- Keep every Quest change gated and switchable. The flat-screen path must not change.
- Don't commit Mojang files (`tools/check_public_tree.py`). Commits end with the agent's `Co-Authored-By` line.


## Split-stream follow-up (cloud implementation)

The opt-in `HORIZON_QUEST_SPLIT_STREAMS=1` path implements 16+8-byte terrain streams,
including arena growth/compaction and matching vertex bindings. See
[implementation, validation and A/B steps](quest-split-vertex-streams.md).
This was software-tested without Quest access; it is not a new headset measurement.
The compute probe/culling work was deferred when the maintainer prioritized split streams.

## Lean opaque varyings and split streams, measured on the headset

Quest 3S, VisorTest at render distance 24 (`--render-distance 24` after `#AB`), the player's
position in tall grass: a fragment-heavy view (about 71% of GPU time in fragments), 3.33 M indices,
not the 10.4 ms reference view. Each pair was run back to back in both orders.

| Run | GPU ms | fps | ALU / vertex | Bytes / vertex |
|---|---|---|---|---|
| Interleaved, `TerrainWire` | 20.9–21.1 | 45.2–45.4 | 114 | 19.7 |
| `HORIZON_QUEST_SPLIT_STREAMS=1` | 20.5–20.6 | 46.0–46.2 | 114 | 15.0 |
| Lean opaque varyings | 20.1–20.3 | 47.0–47.2 | 103 | 19.7 |
| Both | 19.6–20.0 | 47.4–47.7 | 103 | 15.0 |

- **Lean opaque varyings** (`terrain.wgsl` `OpaqueWire`, `vs_terrain_opaque_lean`, `fs_opaque_lean`;
  `gfx.rs` `lean_opaque`): the opaque pass's pipeline without HDR lighting exports 3 locations
  instead of 5. On by default on a headset's Qualcomm Vulkan only; `HORIZON_QUEST_LEAN_OPAQUE=0`
  turns it off. The HDR pipeline and every other backend keep `TerrainWire`.
- **Split streams** stay opt-in. Improved Transparency, world warps and native shader packs'
  material and shadow pipelines now take the arena's layout; `[displacement]` displaces nothing
  while the arena is split. None of those four was run on the headset.
- `adb exec-out screencap` is black on the native foveated array path. For a screenshot, set
  `horizonFoveatedMultiview` to `false` in `vivecraft-client-config.json` and
  `HORIZON_XR_FOVEATION=off` for that run (`single-pass multiview (2 layers)`), then restore it.
- `--render-distance` in `launch.txt` is written back to `options.txt`: restore the file after.
- What is left in the vertex stage in that view: `HORIZON_TERRAIN_EXPERIMENT=nolightmap` on top of
  the lean varyings gave 20.1 → 19.1 ms (ALU 103 → 99), `clip` 20.0 → 18.8 ms; `fsplain` nothing.

## Which pass owns the grass view, and the cutout pass (headset, render distance 24, no LODs)

Same view as the section above (VisorTest, standing in tall grass), `--render-distance 24`,
`lodDistance:0` in `horizon-graphics.txt` for the run, each pair back to back in both orders.

**Dynamic resolution was on in every earlier number on this page.** The player's
`horizonDynamicResolution` is `true`, and at 20 ms a frame it sat at its floor (0.85: eyes of
1432x1496 instead of 1680x1760). `HORIZON_XR_DYNRES=0` now turns it off whatever the setting says
(`horizon-xr/src/lib.rs` `dynres_wanted`); every number below has it off, so the baseline is
21.9 ms, not 20. No MSAA anywhere (sample count 1). Foveation (Low) is applied on the native array
path: `HORIZON_XR_FOVEATION=off` costs 0.9 ms (21.7 -> 22.6).

### Pass by pass

`HORIZON_QUEST_EXPERIMENT=a,b` (`gfx/multiview.rs` `experiment`) leaves parts of the stereo world
pass out: `noopaque`, `nocutout`, `notranslucent`, `nosky`, `noscreen`, `scissor` (a sixteenth of
each eye). New `HORIZON_TERRAIN_EXPERIMENT` names: `cutoutnodiscard`, `cutoutcheap`, `opaquecheap`.

| Run | GPU ms | fps |
|---|---|---|
| Baseline | 21.7–21.9 | 43.4–44.0 |
| `nocutout` | 13.6 | 68 |
| `noopaque` | 12.8 | 72 |
| `notranslucent` | 21.8 | 44 |
| `nosky` | 20.9 | 45.5 |
| `noscreen` | no change | |
| `noopaque,nocutout,notranslucent` (sky, entities, hand, screen) | 2.9 | 120 |
| `scissor` | 11.2–11.4 | 81 |
| `scissor,nocutout` | 6.4 | 120 |
| `cutoutnodiscard` (cutout pass with the depth test first) | 16.6 | 56.5 |
| `nolightmap` | 20.6 | 46 |
| `fsnotex` | 20.5 | 46 |
| `opaquecheap` / `cutoutcheap` / both (lean cutout on, baseline 21.1) | 19.8 / 20.7 / 19.1 | |

- The frame is about 3 ms of everything else (0.8 of it the sky), 10–11 ms of opaque terrain and
  8 ms of cutout terrain. Translucent terrain and the screen effects cost nothing here.
- **The fragment shaders' own work is 2 ms of the 21**, both passes together (`opaquecheap`,
  `cutoutcheap`); texturing 1.4. The profiler's "71–77% time shading fragments" is not shader
  work: do not optimise the shaders' arithmetic further.
- The cutout pass: crossed plants farther than 64 blocks are 0.46 M of the 3.34 M indices and
  1.5 ms (geometry). What is left, about 6 ms, is the grass within 32 blocks (a few tens of
  thousands of indices): fragments that reach the late depth test. With the depth test before the
  shader (`cutoutnodiscard`) 5 ms of it go. Drawing the cutout lists farthest first costs 0.45 ms
  more, so finer front to back sorting has at most that to give.
- The `Fragments Shaded` counter is not the game's alone (15 M a frame with no terrain at all).

### Tried and removed: a depth pre-pass for the cutout quads

Cutout depth first through a minimal alpha-test shader, then colour with compare Equal, no discard,
`@invariant` positions and a coverage blend (for a grass block's side overlay, which lies in its
opaque face's plane). The picture was right in both eyes, and it was **5.4 ms slower**: 27.2–27.5
against 21.8–22.1 ms, both orders. The pre-pass's fragments cost what the cutout pass's did (their
number counts, not their shader), and the second pass adds the cutout geometry again. Not kept.
An explicit mip level in place of `textureSampleGrad` changed nothing either.

### Kept

| Run | GPU ms | fps |
|---|---|---|
| `HORIZON_QUEST_LEAN_CUTOUT=0` | 21.9–22.1 | 42.9–43.4 |
| Default (lean cutout) | 21.1–21.2 | 44.7–45.0 |
| Default + `HORIZON_QUEST_PLANT_DISTANCE=64` | 19.8 | 47.7–48.2 |
| Default + `HORIZON_QUEST_PLANT_DISTANCE=32` | 19.6–19.8 | 48.0–48.6 |

- **Lean cutout** (`gfx.rs` `lean_cutout`, `terrain.wgsl` `vs_terrain_cutout_lean`,
  `fs_cutout_lean`): the cutout lists that are not fading go through `OpaqueWire` (3 locations
  instead of 5) with the alpha test first; fading lists keep `fs_solid` in a third draw group
  after them. Same picture. On by default on a headset's Qualcomm Vulkan without HDR output and
  without flat draws; `HORIZON_QUEST_LEAN_CUTOUT=0` turns it off.
- **Plant distance** (`gfx.rs` `plant_distance`, `mesher::Part::CutoutAligned`):
  `HORIZON_QUEST_PLANT_DISTANCE=<blocks>` leaves the crossed cutout quads (grass, ferns, flowers,
  saplings) out of sections whose centre is farther. It changes the picture, a section at a time,
  so it is off unless set. Sharpness is untouched.

### What is left at render distance 24 without LODs, toward 11.1 ms

Both terrain passes are bound by how many triangles and fragments reach the GPU, not by what the
shaders compute.

1. **Fewer triangles from the same chunks** (opaque pass, 10 ms for about 2.2 M indices): merge
   coplanar full-block faces with the same sprite, tint, light and occlusion into one quad with a
   repeated UV (`tile_uv` already repeats a sprite per block for the far tiles). Same picture, real
   chunks. This is the only lever of that size.
2. **Crossed plants as two quads drawn without back-face culling instead of four** (halves their
   vertices; the back shows the sprite mirrored), and **quads shrunk to their sprite's opaque
   bounds** (fewer late-test fragments near the camera; a dandelion fills a fifth of its quad).
3. The lightmap fetch in the vertex shader: 1.1 ms (`nolightmap`).
4. The sky after the opaque terrain with a depth test: at most 0.5 ms.
5. `HORIZON_QUEST_PLANT_DISTANCE` as a setting, if the maintainer accepts the look.

## Merged block faces and shrunk plant quads (headset, render distance 24, no LODs)

Same view and rules as the section above (`--render-distance 24`, `lodDistance:0`,
`HORIZON_XR_DYNRES=0`), back to back in both orders. The player's `ao` option is `false`
(flat lighting): every number here is without smooth lighting.

| Run (in this order) | GPU ms | fps | Indices |
|---|---|---|---|
| Default | 21.0 | 45.2 | 3,335,063 |
| `HORIZON_QUEST_MERGE_FACES=1` | 20.2 | 46.9 | 2,731,864 |
| `HORIZON_QUEST_MERGE_FACES=1` | 20.3–20.4 | 46.4–46.6 | 2,728,002 |
| Default | 21.0–21.2 | 44.7–45.2 | 3,339,498 |

### Merged block faces (`mesher/merge.rs`, off unless `HORIZON_QUEST_MERGE_FACES=1`)

- Whole opaque block faces on the grid, side by side in one plane and facing one way, become one
  quad when each has one colour and one light at its four corners and both have the same
  colour, light, sprite layer, tint and UV orientation, and no other quad lies on either (a
  grass block's side under its overlay keeps its own four corners: the overlay draws at its
  depth). Greedy rectangles up to 16 by 16, in the meshing worker before `sort_solid`
  (`streaming.rs`); the merged quad takes the place of its first face, the other quads keep
  their order. Far tiles, shader-pack meshes (per-quad sources) and every other GPU are untouched.
- UV: a merged quad counts blocks down from 1. Packed values above 32768 mean
  `-(v - 32768) / 2048` (`MERGED_STEP`), so 15 blocks below 0 fit the same 16 bits; faces that
  are not merged are packed as before. `gfx.rs` `terrain_merged` rewrites `terrain_uv` and
  `tile_uv` in the terrain shader's text while merging is on (every shader built from
  `terrain_shader()` gets it); the shader file itself and the default build are unchanged.
  Below 0 `tile_uv`'s result keeps `tiled_face` false, so a merged face samples through
  `sprite_grad` as the separate faces did. Both wraps give 2^-13 of a sprite to the block the
  quad ends on, so rounding at a merged quad's outer edge cannot read the texel across the sprite.
- What merged (summed over all sections meshed in a run, flat lighting): of 7.40 M opaque quads,
  2.88 M went into merged quads (8.54 M solid quads became 6.80 M, -20%); 3.68 M could merge
  and had no equal neighbour (another light level, sprite, or random rotation beside them);
  0.70 M are no whole block face; 0.14 M lie under an overlay; none was shaded across its
  corners, since smooth lighting is off on this headset. **With smooth lighting on the rate is
  not measured**: the stats line (`Horizon XR: merged block faces: Stats {..}`, logged with the
  frame profile) reports it as `shaded`.
- The frame: -18% indices for -0.7 ms. The opaque pass does not scale with its triangle count
  alone here; the merged triangles are the large flat ones.
- Screenshots (copy path, both eyes), same place, a few seconds apart: no seam, wrong repeat
  or missing face seen in a still. Not checked: the picture in motion (merged quads meet their
  neighbours at T-junctions, which can show single flickering pixels), smooth lighting, far
  tiles with LODs on, emissive packs, the split-stream path. Off by default until that is done.
- Meshing CPU time was not measured.

### Shrunk plant quads (off unless `HORIZON_QUEST_SHRINK_PLANTS=1`; never run on the headset)

- `Sprites::drawn_bounds` (the box of a sprite's texels that are not fully clear, over all
  frames), `models/catalog.rs` `shrink_quad` and `BakedFace::shrunk` (baked for cutout quads in
  no axis plane), used by `Mesher::face` when the switch is on and the quad is lit flat with no
  connected texture on it. Unit-tested; not built into an APK, not measured, no screenshot.
- The box is the full-size sprite's. `MipmapGenerator` raises alpha on the smaller levels to
  keep a cutout's coverage (a box over every level is the whole sprite), so far away a shrunk
  quad may lose up to a pixel of fringe. Near the camera, where the cost is, it is exact.
- Expected gain is small in the grass view: `short_grass` draws in 88% of its quad,
  `tall_grass_top` 88%, `tall_grass_bottom` and `large_fern_bottom` 100%; flowers 21–38%.

### Next

1. Merging: check motion and smooth lighting on the headset, then turn it on by default there.
   If smooth lighting leaves little to merge, a second switch could merge faces whose corner
   values differ (changes the picture).
2. The 3.68 M faces without an equal neighbour: see which key differs most (light level in
   caves, random rotation of dirt/stone/sand) before widening the rule.
3. Not started: the lightmap fetch in the vertex shader (1.1 ms) and the sky after the opaque
   terrain (at most 0.5 ms).
