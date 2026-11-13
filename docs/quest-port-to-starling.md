# Quest VR port onto Starling: ledger

Experiment (branch `quest-starling`, from `starling/starling` c4e44c1e0d, 2026-11-12). Source: the `public`
branch of the Horizon fork: upstream Horizon up to 2026-11-10 (`44607399d2^2`) plus our Meta Quest (Visor VR) port.
Starling's base is upstream main of 2026-11-02 (SYNC.md) with its own Android work on top, so the quest diff
(`git diff 44607399d2^2 public`) is applied as a 3-way merge with upstream-11-10 as the base: only our work
arrives, never the upstream changes of 11-02..11-10 (Starling syncs upstream itself).

Rule from the owner: port everything except optimizations Starling already has; where both cover the same
thing, prefer Starling's. VR parts (OpenXR, headset render path, foveation, Visor controls, room, Quest APK) have
no Starling counterpart and are ported. The Quest APK stays its own target (NativeActivity + OpenXR, app id
`dev.horizon.quest`, `packaging/quest/`) beside Starling's phone app (`packaging/android/`).

Status column: `planned` while this file is first committed; updated at the end (see "What happened" below).

## Starling facts that decide the overlaps

| Area | Starling has | Ours |
|---|---|---|
| Android glue | `crates/horizon-android` (cdylib around `horizon-client/src/android_lib.rs`), `horizon_client::android`, patched winit (input hook), Java side in `packaging/android/java`, `build_apk.py` | `crates/horizon-quest` (NativeActivity, OpenXR), `packaging/quest/*.sh` |
| On-device Java + prepare | `starling-account` (`java.rs` fetches an OpenJDK, `files.rs` downloads the owner's files), `packaging/android/native/java_launcher.c`, `horizon-client/src/preparer*.rs` | `horizon-jvm` (JNI launcher), `horizon-fetch` (Mojang download), `horizon-quest-launcher` (state machine, bundled JRE) |
| Sign-in | `starling-account` device-code flow, needs an Azure client id Starling owns | `horizon-account` Prismarine-compatible Live sign-in (works without our own Azure app) |
| Adreno terrain | `gfx/adreno.rs`: flat draws from 512 sections, vanilla sky after solid terrain, cutouts without fade, RGSS-free sections, rsqrt; "solid terrain without discard, two solid passes only on Adreno" (000e4ebfb5); Mali/PowerVR variants; `terrain_gles.wgsl.inc` | split opaque/cutout two passes (946c885498), lean varyings, split vertex streams, face merging, aggressive merge, plant density/shrink |
| wgpu | patched `wgpu-hal` (relaxed precision, swapchain tweaks) | patched `wgpu-hal` (XR instance/device creator hooks) + vendored `wgpu-core` (WGPU_* env for HAL-made instances) |
| Pipeline cache on disk | none for Vulkan | `gfx/pipeline_cache.rs` (upstream-origin commits) |
| Thread affinity | none | `horizon-threads` hybrid P/E affinity |
| Game structure | monolithic `game.rs` (23k lines, upstream of 11-02) | upstream of 11-10 split `game.rs` into `play_keys.rs`, `use_hand.rs`, `frame_geometry.rs`, `world_entry.rs`... which Starling does not have yet |

## Ledger: our 31 commits

Decision: PORT / SKIP (overlap or upstream) / MERGE.

| # | Commit | What | Decision | Reason / Starling counterpart |
|---|---|---|---|---|
| 1 | 4fd9a93dd6 Horizon Quest 0.1.0 | The whole Quest port: horizon-xr, horizon-visor, horizon-vivecraft, horizon-quest, horizon-quest-launcher, horizon-account, horizon-fetch, horizon-jvm, headset render path, game/vr*, room, input, lang, packaging/quest, docs | PORT in parts (below) | its diff against upstream is the Quest work; its other 4000 files are the upstream snapshot |
| 2 | 6c82f6d56e room: failed sound download | PORT | with horizon-quest-launcher / fetch / room |
| 3 | 3e914e5c97 VR input and Visor integration | PORT | VR only |
| 4 | d9906f3276 head-motion jumps, swing timing, bindings | PORT | VR only |
| 5 | 473bbf2192 experimental multiview, native perf controls | PORT | headset render path |
| 6 | a92f513ac6 visible water geometry, translucent bounds | PORT (render side, headset) | Starling has no equivalent of translucent bounds for stereo; not Adreno-mobile-generic |
| 7 | 3acc2813c9, b55ddc4374, ce9939c698 Vulkan pipeline cache | SKIP | upstream-origin (Ludovic's PRs, cherry-picked): not our work, Starling takes it with its next sync. Noted as worth having on the headset |
| 8 | 0006e7ebd1 section walk bucket sort | SKIP | upstream-origin |
| 9 | da01859092 adapt to the upstream performance integration | SKIP mostly | upstream 11-10 adaptation; only Quest-specific validation docs kept |
| 10 | 7f82e8721a APK self-build docs, WSL entry points | PORT (quest part only) | `build-apk.*`, `quest-build.env.example`, packaging notes |
| 11 | 1db07e2917 Prismarine-compatible Live sign-in | PORT | Starling's account needs an Azure id of its own; ours works on the headset as is (MERGE later: starling-account as a second backend) |
| 12 | c20da6da42 render scale across graphics controls | PORT | VR |
| 13 | 97a3824c34 release builds with Prismarine sign-in | PORT | packaging/quest |
| 14 | cb9760ef25 back-facing water cull | PORT | headset water |
| 15 | 08cbe355a1 render CPU audit, water churn | PORT | headset water |
| 16 | ab26b2513a optional opaque water | PORT | VR setting |
| 17 | 2cd0287122 native foveated stereo array swapchains | PORT | VR only |
| 18 | 5b1636624f foveated multiview exposed | PORT | VR only |
| 19 | 946c885498 foveation, multiview, lighter terrain | PORT VR parts; SKIP the two-pass Adreno solid terrain | Starling 000e4ebfb5/f4f46caca0 already draws solid terrain without discard, two passes on Adreno; emissive table and lean varyings are ported behind the headset profile |
| 20 | 195e793fad split vertex streams | PORT (opt-in) | Starling has nothing alike (its "smaller vertex" is a listed untried idea) |
| 21 | 0ad1124c7b lean varyings, face merging, plant switches | PORT (opt-in / headset) | not in Starling (greedy meshing is an untried idea in docs/starling-performance.md) |
| 22 | 157b4d711f Experiments page | PORT | VR settings |
| 23 | 81af30d775 targeted upstream Adreno improvements | SKIP | upstream cherry-picks (c2dd2d935d, fd70c6aa11, 89733192ca, a09d036f50, eb56315c76); Starling has its own Adreno set |
| 24 | e5731a7cda stale visibility after capped edits, VR lang | PORT | headset culling; lang keys |
| 25 | 0195321bcb rebuild exporter bundle on release | PORT | packaging/quest |
| 26 | 6490a7e156 P/E worker affinity | PORT | Starling has no affinity code; `horizon-threads` |
| 27 | 7498364279 aggressive merging, plant density | PORT (opt-in) | not in Starling |
| 28 | 63e1ca906c RD24 audit docs, profiles | PORT | docs/quest-*, packaging/quest/profiles |
| 29 | a3a4f6373e launcher dependency limitation note | PORT | docs |

(31 non-merge commits: rows 7 and 29 count several; row 1 is the base of the port.)

## Plan of commits on this branch

1. This ledger.
2. Vendored `wgpu-core` and the XR hooks in `wgpu-hal`, merged with Starling's own hal patch.
3. `horizon-threads` hybrid affinity.
4. Input layer (headset, room, VR bindings, headset display).
5. Render layer: headset path, multiview, quest render and quality, room scene, mesher merging.
6. `horizon-vivecraft`, `horizon-visor`, `horizon-xr`.
7. Game integration (`game/vr*`, headset, visor, quest room).
8. `horizon-quest`, `horizon-quest-launcher`, `horizon-account`, `horizon-fetch`, `horizon-jvm`.
9. `packaging/quest`, docs, language keys, notices.

## What happened (2026-11-13)

Branch `quest-starling`, local commits only, nothing pushed. Status per area:

| Area | Result | Notes |
|---|---|---|
| Vendored `wgpu-core`, XR creator hooks in `wgpu-hal` | PORTED | hal hooks apply cleanly over Starling's own hal patch (relaxed precision etc. kept) |
| `horizon-threads` P/E affinity | PORTED | hooked into Starling's `worker_count()` (it has no `default_workers`); `pin_background` call sites in assets.rs / occlusion.rs not added yet |
| `horizon-vivecraft`, `horizon-visor` | PORTED | whole; 264 tests pass |
| `horizon-client-input`: headset, headset_display, room, vr_bindings | PORTED | `embed.rs` and the arboard/Android clipboard stub SKIPPED (Starling's `horizon_client::android` has both; a thin `android::embed` shim was added for the Quest app); `held.rs` Room source SKIPPED (Starling has no `held.rs`, the room crouch needs another hook) |
| Render: `gfx::headset` trait, `vr_options`, `vr_profile`, `visor_art` + Visor textures, `Settings::vr` | PORTED | compiles, clippy clean |
| Render: headset frame path (`quest_render`, `multiview`, `room_*`, `menu_scene`, `vis_mask`, `headset_sim`, stereo changes in `gfx.rs`, `reuse.rs`, `cull.rs`, `translucency.rs`, `stars`, `panorama`) | NOT WIRED | files copied to `crates/horizon-client-render/quest_pending/`, the gfx.rs diff to `docs/quest-port-pending/gfx.rs.quest.diff`. They are written against upstream's 11-10 renderer (`fluid.rs`, `terrain_model.rs`, `surface_setup.rs`, `SolidPipeline`, `Frame::world_primitives`, `Renderer::output`, `visbuf`...) that Starling's 11-02-based gfx.rs does not have. Needs a real adaptation (about 90 hunks of gfx.rs) or Starling's next upstream sync |
| Adreno terrain work (lean varyings, emissive table, split vertex streams, face merging, aggressive merge, plant density/shrink, flat forced) | NOT PORTED | same blocker (they sit in the same gfx.rs/mesher hunks); partly overlaps Starling (solid terrain without discard, flat draws, late sky); the files are in `quest_pending/` |
| Game integration (`game/vr*.rs`, `headset*.rs`, `visor_*`, `vivecraft_*`, `quest_room*`, hooks in game.rs and 40 split-out game files) | NOT PORTED | Starling's game.rs is the monolith from before upstream's split (no `play_keys.rs`, `use_hand.rs`, `frame_geometry.rs`, `held_items.rs`...); the VR logic would have to be re-hooked by hand |
| `horizon-quest` (NativeActivity app) | PORTED | `cargo check -p horizon-quest --target aarch64-linux-android` passes; reaches the game via `horizon_client::android::embed` |
| `horizon-quest-launcher`, `horizon-account` (Prismarine sign-in), `horizon-fetch`, `horizon-jvm` | PORTED | 56 tests pass. Overlap with Starling (`starling-account` fetch + Java runtime, `java_launcher.c`, `preparer*.rs`) is NOT merged yet: both stacks exist side by side, the Quest app uses ours. Replacing `QuestProvisioner` by starling-account is the planned merge |
| Language keys (visor.*, vivecraft.*, room) | PORTED | en_us +1426, ru_ru +1295 keys, by key, Starling's own untouched; no other language got them |
| `packaging/quest`, `build-apk.*`, notices, docs | PORTED | see APK below |
| Vulkan pipeline cache, section-walk sort, targeted upstream Adreno picks | SKIPPED | upstream-origin |

Known breakages / caveats
- The Quest APK builds but has no VR rendering or VR controls: `gfx::headset` is never drawn to and the game logic knows nothing of the headset. It would start Starling's phone game windowless. Do not install it expecting VR.
- Starling's workspace MSRV is 1.85: `is_multiple_of` was replaced in three places.
- `horizon-prepare::Inputs` of Starling's base has no `prism_dir`; the launcher no longer passes it.
- Cargo.lock grew with the Quest crates' dependencies.

## Second pass (2026-11-13): headset render path, game integration, SpaceWarp

Commits: `806ca66eaf` (render path, SpaceWarp, mesher switches), `20d4631f09` and `54c476cdc5` (game).
The 3-way merge of gfx.rs against upstream-11-10 was too noisy, so every hunk of the Quest delta was
applied to Starling's renderer with exact context, then by hand; `quest_pending/` and
`docs/quest-port-pending/` are gone (the files live in `src/gfx/`). Starling was adapted, no
upstream merge was done.

### Ported
- Render: headset device (`open_headset`), stereo views and multiview targets, room and menu scenes, vis mask,
  headset hands over the world, screen effects in the stereo pass, per-eye translucent sorting, headset
  frustum, `quest_render` (the multiview pipeline swap now lists Starling's pipelines: solid, solid_paths,
  late_sky, flat, ...), SpaceWarp (`gfx/spacewarp*`, horizon-xr, vivecraft refresh; GPU tests pass),
  planar water, pipeline cache (+ horizon-hdr fn), split vertex streams, emissive table (Vulkan source only:
  Starling's GLES lowering unrolls the old search), merged-face shader decoding, A/B shader experiments.
- Mesher: face merging, aggressive merge, plant density, plant shrink, optional opaque water.
- VR pages in options_ui, headset resolution scale in graphics_ui, horizon-threads affinity recovery (bcfb1a7f82).
- Game: game/vr, headset*, visor_*, vivecraft_*, quest_room (App::Room, `build_loading`, `run_app`), aim ray,
  two-handed off hand and VR guest swap in `as_guest`, hand geometry with controllers, HUD overlay panels,
  bow, mounts, remote ticks, headset pointer, upload budget, Visor game menu.

### Starling optimizations kept (overlaps) and how to A/B them on the headset (`launch.txt` lines)
| Starling | Reachable on Quest | Switch |
|---|---|---|
| two solid passes (`splits_solid`) | yes (Adreno + Android + Vulkan) | `HORIZON_SPLIT_SOLID=0/1` |
| flat draws (512 sections on Adreno) | yes | `HORIZON_FLAT_DRAWS=0/1`, VR setting `horizonFlatDraws` |
| late sky | yes, also in the stereo pass (pipelines swapped) | `HORIZON_LATE_SKY=0/1` |
| solid_paths: static cutout, settled terrain | yes | `HORIZON_STATIC_CUTOUT=0/1`, `HORIZON_SETTLED_TERRAIN=0/1` |
| cull.wgsl and panorama array fixes for Adreno | Starling's own kept | |
| multi-draw indirect | off on a headset's Adreno (our measurement: draws nothing) | `HORIZON_MULTI_DRAW=0/1` |
Ours, opt-in: `HORIZON_QUEST_SPLIT_STREAMS`, `_MERGE_FACES`, `_AGGRESSIVE_MESH`, `_SHRINK_PLANTS`,
`_PLANT_DENSITY`, `_PLANT_DISTANCE`, `_NEAREST_LIGHTMAP`, `HORIZON_TERRAIN_EXPERIMENT`, `HORIZON_QUEST_EXPERIMENT`.

### Not ported / differences
- Lean varyings (`lean_opaque`, `lean_cutout`): need upstream's `HDR_LIGHTING` pipeline override; Starling's
  terrain shader has HDR lighting always in. Removed from the port.
- Upstream-origin changes inside our delta were skipped (light.rs relight scratch, occlusion edit cap and
  resident sort, cull.wgsl box table, frame_gen tests). Occlusion keeps Starling's code; only the worker pin was added.
- Mesher: no per-quad sources or tints (Starling has none); `shader_pack` guard replaced by plain settings.
- `held.rs` (Source::Room): Starling's `keys` is a plain set; the room's sneak is a `room_sneaking` flag.
- GPU-culling `Unsupported` offer for the split path (Starling has no GpuCulling).
- Tests for several ported files are partly missing (cull stereo GPU test, occlusion/relight benches).
- Provisioning stays our stack (horizon-quest-launcher, Prismarine sign-in, horizon-fetch, horizon-jvm); not merged with starling-account.
- Language keys: en_us and ru_ru only; SpaceWarp keys came from 54ba05033f.
- `tools/quest_room_sim.py` and harness tests of the fork not ported.

### APK
Third pass (2026-11-13). Both assets are now made from this tree:
- `horizon-bundle.zip`: `packaging/quest/build_bundle.sh` with Starling's `horizon-prepare` and this worktree's own
  `.local/minecraft/26.3` (the jars and libraries copied from the source worktree's import, the version JSON
  `manifest.json` fetched from Mojang's `piston-meta` by its sha1 `11f1aefe...` that Starling's lock accepts; the source
  tree's `manifest.json` has the republished sha1 `80608ff6...`, which Starling's lock does not, hence the earlier
  "Manifest checksum changed"). The bundle's `research-lock.json` is byte-equal to this tree's. `build_apk.sh`'s guard
  (every SERVER/CLIENT exporter of `horizon-prepare` as a class and as a source) passed. The `.local` symlink to the
  source worktree is gone.
- `horizon-jre.zip`: `build_jre.sh` rebuilt, with the Termux URL moved to `openjdk-25_25.0.5-2_aarch64.deb`
  (sha256 `a3253f29...`); jlinked OpenJDK 25, 30 MB zipped.
- APK: `OUT=target/quest packaging/quest/build_apk.sh` (profile `quest`): `target/quest/horizon-quest.apk`, 126 MB,
  sha256 `59d48a67da9c5049e0a2d2333402ef4f3e7e25a498a47a58a92cb50bc9102b15`. Not installed on any headset.
- A stale-mtime trap: files copied from the source worktree keep old mtimes, and `cargo test -p horizon-client-input`
  failed with "cannot find `refresh` in `horizon_vivecraft`" until `cargo clean -p horizon-vivecraft`.

### Tests and checks (third pass)
- `cargo fmt --check`, `cargo clippy -D warnings --all-targets` on horizon-xr, -vivecraft, -visor, -client-input, -threads,
  -quest-launcher, -account, -fetch, -jvm, -client-render, -client: clean.
- Tests: xr 25, vivecraft 144, visor 123+2, client-input 90, threads 30, quest-launcher 36, account 7, fetch 10, jvm 3,
  horizon-client lib 792 (28 of them quest/visor/vivecraft/headset/vr): all pass.
- `horizon-client-render --lib`: 1407 pass, 2 fail: `frame_gen::interpolation::tests::moves_the_view_halfway_with_the_camera` and
  `moves_what_has_its_own_motion_vectors_halfway` (software GPU numerics, 0.28010 vs bounds). The same two fail on
  Starling's base c4e44c1e0d: not ours. The SpaceWarp GPU tests pass.
- `cargo check --target aarch64-linux-android -p horizon-quest -p horizon-android`: passes.

## Remaining work
1. Test on the headset: stereo/multiview, SpaceWarp, room, controls, then A/B the switches above.
2. (done 2026-11-13: JRE and bundle rebuilt from this tree, see APK.) Check on the headset that the on-device prepare passes.
3. Lean varyings after porting the `HDR_LIGHTING` override; missing tests above.
4. Merge provisioning with starling-account if wanted.
5. Translations of new keys in the other languages.
