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
- Lean varyings: ported in the fourth pass (see below).
- Upstream-origin changes inside our delta were skipped (light.rs relight scratch, occlusion edit cap and
  resident sort, cull.wgsl box table, frame_gen tests). Occlusion keeps Starling's code; only the worker pin was added.
- Mesher: no per-quad sources or tints (Starling has none); `shader_pack` guard replaced by plain settings.
- `held.rs` (Source::Room): Starling's `keys` is a plain set; the room's sneak is a `room_sneaking` flag.
- GPU-culling `Unsupported` offer for the split path (Starling has no GpuCulling).
- Tests still missing: occlusion/relight benches (skipped on purpose). The cull stereo tests are ported.
- Provisioning: see the fourth pass; the sign-in, runtime and room stay ours.
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

## Fourth pass (2026-11-13): lean varyings and provisioning

### A. Lean terrain varyings
- Starling's terrain.wgsl had no `HDR_LIGHTING` override (HDR lighting was always in). Added `override HDR_LIGHTING: bool = true`
  (default on: nothing changes for the phone app), guarding the face normal and the emission alpha in `solid_finish`.
- `OpaqueWire` (3 locations instead of 11), `vs_terrain_{opaque,cutout}_lean`, flat variants `vs_terrain_flat_{opaque,cutout}_lean`,
  `fs_opaque_lean`, `fs_cutout_lean` (alpha test first, with the fade-in's fog alpha so it discards exactly as `fs_solid`).
  They honour `SETTLED_TERRAIN`; written with the `in.` naming so the GLES lowering still rewrites them.
- Overlap kept: Starling's `gfx/solid_paths.rs` (static cutout, settled terrain, flat passes) and `adreno.rs` stay; their
  passes are built lean where asked (falling back to the usual entry points if the device refuses), plus one lean opaque
  pass for frames with no settled terrain (the opaque pass has no fade to know). Our source's per-list "fading" group split
  does not exist in Starling: its lean cutout applies in the frames `solid_paths` proves unfaded, not per list.
- Switches (headset, Qualcomm Vulkan only, **default on**): `HORIZON_QUEST_LEAN_OPAQUE=0`, `HORIZON_QUEST_LEAN_CUTOUT=0`
  (as in the source). The `HORIZON_TERRAIN_EXPERIMENT` names `cutoutcheap`, `opaquecheap`, `fsnohdr` match the new shaders.
- Tests: `solid_paths::tests::lean_passes_draw_the_usual_colours` (static, settled, flat; colours equal to the usual
  pipelines on the test GPU, alpha excluded) and `lean_opaque_serves_frames_with_fades`; `cull::tests` stereo shader and
  the real two-eye pass (`a_stereo_pass_sees_the_boxes_either_eye_shows`, `#[ignore]`d, passes with `--include-ignored`).
  Benches skipped.

### B. Provisioning
- Unified: the Mojang download. `starling_account::files::download` keeps its API, pins, messages and phone retry policy
  but plans with `horizon_fetch::mojang::{game_files, objects}` and transfers through `horizon_fetch::fetch_all`: one
  engine for the phone and the headset (Range resume, size check, free-space check, cancel, progress). horizon-fetch gained
  `Options::{wait, growing, transient}` (defaults = the headset's old behaviour) and a shared keep-alive agent in `Ureq`
  (Starling's reason for its own agent). Starling's own `files` tests pass unchanged. Behaviour new on the phone: Range
  resume, a space check, the logging config file is fetched too.
- Kept separate, and why:
  - Sign-in: `horizon-account` already contains the Azure device-code flow (`Endpoints::azure`, `HORIZON_MSA_CLIENT_ID`)
    beside the Prismarine-compatible Live flow, so a `starling-account` backend would add no capability, needs adapters for
    five types whose fields are private there, and would activate Starling's Azure client id from the room. Starling's
    id is still linked into the Quest binary through `horizon-client` (phone accounts code), but the room never uses it.
  - Java runtime: the Quest bundles the JRE and `horizon-jvm` (offline, tested); `starling_account::java` fetches a Termux
    runtime at run time and stays the phone's.
  - Preparer glue: both call `horizon_prepare::prepare` with different JVM supply (`java::supply` and `libstarling_java.so`
    vs `HORIZON_JAVA_EXE`/`horizon-jvm`) and different post steps (controls textures vs languages/sounds stages); equal in
    effect, not worth a shared layer. The room's state machine, stages, language/sounds steps and reserve check stay ours.

### Fourth pass: tests, APK
- fmt clean; clippy `-D warnings --all-targets`: horizon-client-render, horizon-fetch, starling-account, horizon-quest-launcher,
  horizon-account clean. Tests: horizon-client-render --lib 1409 pass + the 2 known frame_gen failures; horizon-fetch 11,
  starling-account 26 (+1 ignored), quest-launcher 36, account 7, jvm 3, horizon-client --lib 792: pass.
  `cargo check --target aarch64-linux-android -p horizon-quest -p horizon-android`: passes.
- APK: bundle and JRE reused (horizon-prepare and the exporters unchanged); `OUT=target/quest build_apk.sh`:
  `target/quest/horizon-quest.apk`, 131498824 bytes, sha256 `5a033dfa998bdfce72aa909b0e2d3e0105ecc94825d5cb487010d192b90ae1a3`;
  exporter guard passed, bundle `research-lock.json` equals the tree's (cmp). Not installed on any headset.

## Fifth pass (2026-11-13): fixes from the first headset test

1. On-device prepare: "Java 25 is needed ... No java was found." Starling's `horizon-prepare::java` does not read
   `HORIZON_JAVA_EXE`; `horizon-quest-launcher::prepare` now hands `horizon-jvm` to Starling's own `java::supply` hook
   (`java` = libhorizon_jvm.so, `HORIZON_JVM_HOME` in the environment of every JVM command). horizon-prepare is
   unchanged, so the exporter bundle and its key stay as they were. Test:
   `the_quests_runtime_is_found_without_java_home_or_path`. The rest of the chain was read, not run on a host: the
   download (`Selection::all`, strict assets) fetches `indexes/<id>.json` and the objects; `horizon-prepare`'s steps import
   `asset-index.json`, packs, panorama, fonts, languages and sounds from them; `horizon-data.bin` is written by the game
   at its start (`prepared_files::write`), not by prepare. The earlier host run lacked those only because it had no asset
   store (no `indexes/34.json`).
2. Multiview never engaged. Cause: `Reuse::try_create_render_pipeline` (Starling's Adreno paths: `solid_paths` static
   cutout, settled, lean and flat passes, and `late_sky`) made no multiview variant, so `swap_stereo_pipelines` found none
   and returned false at every frame. All routes now end in `add_stereo`; a failed swap logs `Quest multiview: no stereo
   variant for pipelines: ...` by slot name. Test: `solid_paths::tests::every_adreno_path_pipeline_has_a_multiview_variant`
   (fails without the fix, passes on a MULTIVIEW device).
3. Phone HUD in the world: Android's touch mode was the start mode and the vanilla tutorial's "Move with W,A,S,D" ran.
   `touch_shown()` is false while a headset session runs, the touch platform is "Android and no headset", the tutorial is
   off in a headset. The first-start card "Keep your files where you can reach them" stays and now answers the headset
   pointer (`files_offer_click`, a `MouseInput` arm). Its "Set up" button calls StarlingActivity's Java (`openFileAccessSettings`,
   `fileAccessAllowed`, `restart`) which the Quest's NativeActivity lacks, and `SHARED_FOLDER` is never set there: Set up
   does nothing on a Quest until a Quest-side equivalent exists (an Intent to Settings' all-files page through JNI, and
   `set_shared_folder` at start). Not done: reported.
4. Gray foliage and the pink-brown sky: probably the biome at that spot (the same seed looks the same in vanilla);
   not investigated further.
5. Bedrock touch buttons and Starling's control textures are no longer fetched in a headset. The `horizon-data.bin` and
   other Permission denied lines came from files pushed by adb (owner shell); a normal install writes them as the app.

## Sixth pass (2026-11-13): the second headset test

1. On-device prepare still wanted javac ("Java 25 is needed (javac ...)"). Cause: `exporters::key` hashed every `.java`
   under the parity folder, but the APK bundle ships the sources of its 26 bundled classes only (of 326), so on the
   headset the key never matched `exporters/manifest.json`, `valid(shipped)` failed and `classes()` took the javac branch.
   Fix (ported from the source repo): a root without `crates/horizon-prepare/src/exporters.rs` (the Quest's app data)
   returns the key of its own `exporters/manifest.json`; `valid` still hashes every class against it. The phone's kit
   ships `crates/` and is unchanged. Tests: `a_root_without_a_checkout_uses_the_bundles_own_key` (fails without the
   branch); host proof with the rebuilt zip unzipped alone: `HORIZON_QUEST_BUNDLE_ROOT=<dir> cargo test -p
   horizon-prepare --lib the_shipped -- --ignored` passes, and fails with the branch disabled ("classes() would compile
   with javac"). The only javac user reachable from prepare is `exporters::classes` (compile branch); everything else uses
   `java::find_runtime`.
   `Languages not imported: Missing asset object .../2ab603...`: prepare's language import took every `minecraft/lang/*`
   of the asset index, but the headset downloads only en_us and the chosen languages (44 objects). The shared downloader
   did not drop anything (`Selection.languages` selects them); the importer was the strict side. Ported the source's rule:
   absent `lang` objects are left out and counted in the log, everything else stays all or nothing. Test:
   `languages_missing_from_the_store_are_left_out_not_an_error`.
2. Multiview still never engaged: `no stereo variant for pipelines: solid_paths[0..7], late_sky[0..4]`. Cause: not a missing
   shader module. `Reuse::finish` kept the stereo variants only of pipelines in the `render` map; the pipelines made by
   `try_create_render_pipeline` (the Adreno solid paths, the late sky) live in the `tried` map, so their variants were
   dropped when the renderer finished building. RADV never builds those paths, and the old test did not call `finish`.
   Fix: `finish` also retains the variants of the `tried` pipelines; `add_stereo` now warns which pipeline lacked a stereo
   copy of its vertex/fragment module instead of skipping silently. Test (rewritten):
   `solid_paths::tests::every_adreno_path_pipeline_has_a_multiview_variant` builds the solid paths (plain and lean) and the
   late sky with the stereo flag, calls `finish`, and asserts a variant for all of them; it failed before the fix and
   passes now. A full-renderer swap test is not possible without a surface; every other pipeline of the swap list is made
   through `kept.create_render_pipeline` (retained already) and the headset log named only these two sets.
3. The files card's "Set up" now works on the Quest by JNI (needs a headset check): `android.rs` has a Quest branch
   (`embed::set_android_app` marks it): `Environment.isExternalStorageManager`, an Intent to
   `MANAGE_APP_ALL_FILES_ACCESS_PERMISSION` with `package:` Uri (then `MANAGE_ALL_FILES_ACCESS_PERMISSION`), restart by the
   launch intent (NEW_TASK|CLEAR_TASK, then the process ends). `AndroidManifest.xml` declares `MANAGE_EXTERNAL_STORAGE`.
   At start `android_main` calls `embed::quest_game_folder`: when allowed, the game folder is `<external storage>/Starling`
   (the phone's name rule, `shared_folder::name`), files are moved once by `game_folder`, `HORIZON_PRIVATE` keeps caches
   in the app's files, the headset-defaults mark stays in the private home. When no settings page opens the card turns
   into "Your files are here" with the folder's path (no dead button). Texts: `horizon.quest.files.*` in en_us and ru_ru
   (test `the_headset_files_card_is_translated`). To check on the headset: that Horizon OS opens the all-files page, that
   the card returns to "All set" after coming back, that Restart relaunches, and that worlds appear in /sdcard/Starling.

### Sixth pass: tests, APK
- fmt clean; clippy `-D warnings --all-targets` clean for horizon-prepare, horizon-client-render, horizon-quest-launcher,
  horizon-fetch, starling-account, and `-p horizon-client --lib --tests`. (`clippy --target aarch64-linux-android` stops on
  two `unnecessary_cast` in `horizon-threads/src/cores.rs`, untouched here.) Tests: horizon-prepare 62 (+2 integration,
  +1 ignored bundle check), horizon-client-render --lib 1411 pass + the 2 known frame_gen failures, quest-launcher 37,
  horizon-fetch 11, starling-account 26, horizon-client --lib 794: pass. `cargo check --target aarch64-linux-android -p
  horizon-quest -p horizon-android`: passes.
- Bundle rebuilt (`build_bundle.sh`, 282605 bytes); JRE reused. APK `target/quest/horizon-quest.apk`, 131711816 bytes,
  sha256 `006e3231bd23cd73e859f2f1c38133df9c036a5a156e5533404829ca38785013`. The exporter guard in `build_apk.sh` is silent
  when it passes (it prints only the stale exporters): the build ran to the end. Bundle `research-lock.json` equals the
  tree's (cmp). Not installed on any headset.

### Seventh pass: the pointer on the title (user's headset test of the sixth APK)
Report: on the title nothing could be clicked with the controllers, the files card's buttons neither, and the cursor was
drawn under the card.
- Found by reading (no controller input can be injected here, so the click path itself is not reproduced): the files card is
  modal and takes every `MouseInput` while it shows (game.rs arm `files_offer_shown()`), so the title below it is dead until the
  card goes; the card's own click went the long way (headset frame -> `CursorMoved` -> `self.cursor` -> `MouseInput` ->
  hit test), where `self.cursor` can be somewhere else than the laser (the pad's menu focus moves it to a widget, and the
  headset sends `CursorMoved` only when the laser moves). Sure bug: `draw_pad_cursor` ran before `draw_phone_offers`, so the
  arrow was under the card.
- Fixes: (1) the pointer is drawn last (`draw_pad_cursor(batch, true)` after offers and imports; the keyboard keeps its
  place); (2) `headset_frame` sends `CursorMoved` to the laser on every press edge, even when the pointer did not move;
  (3) while a card shows, `files_offer_pointer` takes the headset pointer's buttons and hit-tests the press directly at the
  laser's panel pixel in UI units of the renderer's size (`game/pointer_ui.rs`, tests), not through the cursor; (4) every pointer
  button edge is logged (`Headset: pointer button ...`, and `the files card is clicked at ...`) so the next logcat shows where
  a click fails. The Starling touch-only overlays (update offer, native builds, catalog) still answer touches only; the update
  offer does not appear on the Quest (`app_version` has no Java behind it).
- Checks: fmt; clippy `-D warnings --all-targets` horizon-client clean (android-target clippy still stops on horizon-threads);
  horizon-client --lib 798, horizon-client-input 90, render `headset|quest` 14 pass; `cargo check --target aarch64-linux-android`
  passes. APK rebuilt on the same bundle (research-lock equal), installed on the headset with launch.txt untouched.

### Eighth pass: sound, the cursor, a pointer for each hand (user's headset test of the seventh APK)
Report: clicks work now; no sound at all; the cursor looks fixed at the panel's centre.
- Sound. Cause: the launch.txt run (`HORIZON_QUEST_PROVISION_TEST`) skips the room and downloaded only `Stage::Core`, so the
  prepared resources had no `sounds.json` and no sound objects; the audio output itself (cpal 0.16 on AAudio, feature `audio`
  default in horizon-quest, no `HORIZON_NO_AUDIO` in that path) is not the cause. The room flow already has the second stage
  (`QuestProvisioner::download(Stage::Sounds)` then `prepare(Stage::Sounds)` = `import_assets("sounds")`, `.sounds-complete`
  stamp, a retry and an off switch; machine_tests cover it). Fix: `provision.rs` `add_sounds` after the core prepare: skipped
  when `assets/minecraft/sounds.json` is there, else `download(Stage::Sounds)` (the downloader keeps what is on disk) and
  `import_assets("sounds")`. Device log lines are in the report of this pass.
- Cursor. The drawn arrow used `Game::cursor`, which the headset sets only when the laser moves or a button goes down; anything
  that writes the cursor between the headset's frame and the draw (Starling's Android code has several writers: the virtual
  mouse's viewport sync, the pad's focus following) showed as a cursor that stays where it was put, and a press worked
  because the seventh pass re-sent the position on the press edge. Fix: (1) `draw_pad_cursor` draws the arrow at the laser's
  point of this frame (`headset_laser_points`, `pointer_ui::laser_gui_point`, window pixels over the GUI scale), not at `cursor`;
  (2) `hud()` first puts `cursor` on the owning laser (`headset_cursor_to_laser`), so hover matches the arrow; (3) `headset_frame`
  re-sends the position whenever `cursor` is off the laser by more than half a pixel (`cursor_off_laser`) and logs the first
  five times (`Headset: the cursor ... was taken off the laser`), which names the writer if it still happens.
- Option `horizonBothHandPointers` (off by default = one pointer, the hand that clicked last, as Visor and Vivecraft): on the
  VR settings' Controls pages after "Reverse Hands" (`pages.rs`), keys `horizon.vr.both_hand_pointers` (+ `.tooltip`) in en_us
  and ru_ru. On: `horizon-xr` publishes the off hand's laser as `headset::second_pointer` (point, distance, trigger and face
  button) beside the main hand's `pointer`; `horizon_client_input::pointer_route::route` (tests) gives the mouse to the hand
  that presses first (the main hand on a tie) until it lets go; the trigger is a left click and A/X a right click at that
  hand's point; both lasers are drawn (`headset_pointer_lines`) and both arrows (the off hand's light blue). The sign-in room
  still uses the main pointer only.
- Checks: fmt; clippy `-D warnings --all-targets` clean for horizon-client-input, -vivecraft, -xr, -quest-launcher and
  `-p horizon-client --lib --tests`; tests horizon-client --lib 800, client-input 96, vivecraft 144, visor 123 (+2), quest-launcher
  37, client-render `headset|quest` 14: pass; `cargo check --target aarch64-linux-android -p horizon-quest -p horizon-android`
  passes. APK on the same JRE and bundle (research-lock equal), `launch.txt` untouched.

### Ninth pass: SpaceWarp hands, no centre crosshair (user's headset test of the eighth APK)
Report: with Application SpaceWarp on the hands smear; a screen-space crosshair stays at the centre of the view.
- Hands. The port of `spacewarp.rs`, `gfx/frame_gen/motion.rs` and the shader is byte-equal to `public` (checked by diff), and
  the hands do reach `Frame::hand` and the world depth. What the port inherited is that the hands' motion came from
  `MotionTracker::track` keyed on ONE id for all of them and needing the same count, UVs and layers as last frame: any change of
  the set (a held item swapped, a face culled, a pointer or the other hand shown, the activation geometry) gave the hands no
  motion vectors for that frame, and they were extrapolated with the world's camera-only motion. Starling's headset builds
  the hands every frame from the controllers (`headset_hand_frames`, `headset_arm`, `submit_item_world`), so the set is not
  steady. Fix: `MotionTracker::track_quads` (`frame_gen/motion.rs`) follows each hand quad by itself: the nearest quad of last
  frame with the same texture and layer within 1 block (`QUAD_REACH`), its previous place moved with the camera; a quad
  with no match is still staged standing (camera-only). `spacewarp.rs` `prepare` uses it for the world hands and for the
  GUI hands/pointers (which are all staged now, with the overlay flag, so the old fallback loop is gone). Depth is
  unchanged (hands are depth-tested into the world depth, the GUI hands write their own).
  This is a fix of a weakness found by reading, not seen on the device: if the hands still smear, the next thing to
  look at is `Quest world rendering`/`Application SpaceWarp compatibility` in the log.
- Tests (`motion.rs`): `hand_quads_are_followed_when_the_set_changes` (a moved quad has its own non-zero motion, a new one
  none, order and set changed), `a_hand_still_in_the_world_has_no_motion_of_its_own` (camera-only motion for what stands
  still, own motion for what moves with the camera). The existing GPU tests (`moving_objects_override_camera_vectors_and_respect_occlusion`,
  camera motion per eye) cover the shader and pass.
- Crosshair. Cause: `Game::hud` drew `hud_layout::crosshair` whenever the camera was first person; `public` also requires
  `!headset::active()` and the port dropped it. Fix: `game.rs` (the crosshair condition) has `&& !horizon_client::headset::active()`;
  the per-controller crosshair (`hand_crosshair`, Visor's `HandEffectCrosshair`) is kept. In menus `draw_pad_cursor` drew
  the arrow at `cursor` when no laser touched the panel: on a headset it now draws nothing then (the lasers' arrows stay).
- Checks: fmt; clippy `-D warnings --all-targets` horizon-client-render and horizon-client clean; tests client-render --lib
  1413 pass, 2 fail (the known `moves_the_view_halfway_with_the_camera`, `moves_what_has_its_own_motion_vectors_halfway`),
  horizon-client --lib 800, horizon-xr 25; `cargo check --target aarch64-linux-android -p horizon-quest -p horizon-android` passes.

## Remaining work
1. Test on the headset: stereo/multiview, SpaceWarp, room, controls, then A/B the switches (including
   `HORIZON_QUEST_LEAN_OPAQUE`, `HORIZON_QUEST_LEAN_CUTOUT`); check the on-device prepare and the download (Range resume).
2. Optional: a `starling-account` sign-in backend behind the room's `Accounts` trait (not cheap, see B).
3. Translations: not needed (decision); only en_us and ru_ru carry the keys.
