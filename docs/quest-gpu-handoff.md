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
