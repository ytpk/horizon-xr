# Quest upstream integration — 2026-11-10

This integration updates the Quest port without replacing its VR implementation.
It is kept on `quest/upstream-integration-2026-11-10` for headset testing before
promotion to `main`. Desktop validation cannot establish Quest frame times.

## Sources

| Source | Pinned commit |
| --- | --- |
| Quest port before integration | `a92f513ac6e22d33c0e7a1cc288b116aec26c493` |
| Original imported Horizon baseline | `6ddd14e727bf54cfe9b2809706ff4fb622a54d69` |
| Official Horizon `main` | `8933e8c2d81bef1549a63dc1c8341ca46fe3cffa` |
| `perf/vulkan-pipeline-cache` | `1538d954400562aa3235d01b34b66f2c2a4a51ea` |
| `perf/visible-bucket-sort` | `71f43e5e8118601ded64b37fb35e7df087f4d26d` |

The port began as an unrelated source import. An unchanged-tree merge records
its original Horizon baseline as an ancestor; the following merge has the
pinned official `main` as its second parent. Existing port commits are retained.
Future upstream updates can use ordinary merges against this shared ancestry.
The performance commits retain their upstream authors.

## Integration details

- Preserve the Quest room and its renderer/window handoff, embedded Android event
  loop and arguments, shared account/download code, and Android clipboard fallback.
- Carry the Visor/Vivecraft, offhand, controller/physical movement, keyboard and
  pre-world authentication input changes into the upstream's new game modules.
- Preserve single-pass multiview and tight translucent bounds/planar-water index
  pruning, alongside the upstream's terrain shader, culling and mesh updates.
- Adapt multiview to the new solid-pipeline wrappers and native terrain/fluid
  pipelines. Custom sky layers and native world primitives use separate-eye
  rendering until their own uniforms support stereo; their content is retained.
  GPU-driven culling remains off during multiview until its visibility tests
  conservatively cover both eyes; CPU visibility traversal and bucket sorting
  remain active.
- Keep Quest room/stereo/image-handoff methods in `gfx/quest_render.rs`, separate
  from the upstream frame encoder. Existing `multiview.rs` and `planar_water.rs`
  remain separate modules.
- Request Vulkan pipeline-cache support for both the headset-owned device and
  desktop adapters. The retained-pipeline wrapper supplies the disk cache to
  multiview variants as well. The moved shader-pack job builder and the Quest
  room pipelines also receive it. `HORIZON_PIPELINE_CACHE=0` disables it.
- Move the bucket sort to the renderer's new resident-section module and adapt
  its mutable scratch state to `walk`. Each bucket is sorted exactly: distance
  ties still use the section key, preserving the old order for transparency.
- Preserve the Quest file-key sign-in format. Desktop secret stores adopt the
  upstream install-secret format and migrate old desktop files on read.
- Register port crates in the upstream layer checker; remove obsolete unused
  dependencies rather than adding layer exceptions.

## Validation

- Python source/tool suite: **831 tests, OK, 4 skipped** (Rust available).
- Rust input, Visor, Vivecraft, OpenXR and shared account suites: **484 passed**.
- Full renderer suite with Vulkan/lavapipe and two test threads: **2153 passed,
  68 ignored, 6 failed**. Two integration fixtures were then corrected and
  individually passed: the stars HDR fixture now allocates both eye matrices;
  the Vulkan cache test no longer assumes a no-op shader grows the driver's
  opaque cache. No production assertion was weakened.
- Explicit GPU checks passed: stereo projection/layer copies (2 tests including
  shader validation), planar-water image equality (1), Vulkan cache round-trip,
  corruption/fallback and file handling (5), stars HDR image comparison (1).
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo clippy -p horizon-client -p horizon-xr --no-default-features --lib -- -D warnings`: passed.
- Layer checker against `upstream/main`: **60 crates, no new exceptions**.
- Formatting, public-tree and integration-history checks: passed. Both lockfiles include
  the shared Quest dependencies; the desktop GPUI launcher was not built.

Four renderer checks remain failing in this environment and were kept enabled:

| Test | Observed limitation |
| --- | --- |
| `crash::tests::a_panic_becomes_a_report` | Build uses `CARGO_PROFILE_TEST_DEBUG=0`; source locations are unavailable. |
| `crash::tests::the_debug_crash_is_reported_as_vanillas` | Same stripped debug-location limitation. |
| `gfx::hdr::tests::sdr_screenshots_roll_off_the_world_and_keep_the_hud_as_drawn` | Strict GPU pixel comparison: red channel `0.9038086` versus `0.89990234` on Mesa llvmpipe. HDR implementation/shader is unchanged from upstream apart from supplying a pipeline cache. |
| `gfx::room_scene::tests::the_room_is_drawn_from_every_side_before_and_after_the_real_textures` | Existing room simulator's strict pixel-colour comparison. |

GPU checks select the lavapipe ICD explicitly through `VK_DRIVER_FILES`,
with `WGPU_BACKEND=vulkan` and a writable `MESA_SHADER_CACHE_DIR`; a desktop software Vulkan driver does not model Adreno performance.
No Android SDK/NDK, prepared game assets or physical Quest is available here:
APK compilation, real world/server gameplay and device performance remain open.
No APK is produced in this environment.

## Required headset checks

1. Cold/warm launch and game entry from the Quest room; Microsoft sign-in and
   session persistence across an update; room-to-game renderer handoff.
2. Keyboard/cursor on a server's pre-world authentication or button prompt.
3. Button jumps while turning/leaning, physical movement and crouching,
   recentering, seated/standing height, both-hand mining and offhand items.
4. Separate-eye versus multiview image quality, peripheral geometry and effects;
   shader/resource reloads, custom skies and native-mod fallback.
5. Compare land, visible water and water behind an opaque wall from one fixed
   position, at identical render scale, distance, refresh rate and power settings.
   Also check shores, flowing water, diving, eye-height crossings and distant LODs.
6. Compare cold/warm Vulkan cache startup. Delete/corrupt only its cache file to
   verify recovery; repeat after changing the build or GPU driver.
7. On Quest 3S and Quest 2, record CPU/GPU times and sustained frame rate after
   thermal warm-up. Performance gains are unmeasured until these device checks.
