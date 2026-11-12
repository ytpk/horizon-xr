# Quest Adreno follow-up, 12 November 2026

Reviewed fork `157b4d711f` against upstream main `a21f8a3947`. This is a selective
integration, not a merge of all upstream gameplay, HDR and renderer refactors.
The primary target remains standalone Quest 3S (XR2 Gen 2, Adreno 740). Quest 2
(XR2 Gen 1, Adreno 650) needs its own device checks; sharing a vendor ID does not
establish identical driver behaviour or performance.

## Changes taken from upstream

All source below is from `horizon-rust/horizon`, under its MIT license (the
repository's root `LICENSE`, copyright Ludovic Marie and the Horizon contributors).
Existing Quest terrain entry points, multiview, split streams and emissive-table
lookup are retained.

| Upstream commit | Included change | Expected relevance |
| --- | --- | --- |
| `a09d036f50842675f922bec0852c693e1c0bf13a` | Pack box, panorama and Bayer lookups instead of dynamically indexing private constant arrays; use shader depth comparisons for Adreno's indirect occlusion path. | Avoid driver pipeline-link failures on older Adreno Vulkan builds. This does **not** fix Quest's fragment-storage-write failure. |
| `eb56315c765345c6cfedb269e7842fcaabb7915e` | Bound queued cave-visibility edits and resend the current columns when searches resume. | Prevent memory growth and an increasingly expensive backlog while cave culling is off. |
| `c2dd2d935d7d317af3310ad16021a334fd9a8d39` | Reuse bounded per-thread relight queues, borrow input seeds and cache consecutive sky-source lookups. | Reduce allocation churn during block edits and incremental lighting; preserve lazy section loading. |
| `fd70c6aa11df8841522404a16a72472e0fe038ec` | Share immutable native animation frames when they already match the atlas size. | Lower texture-loading memory and copying; not a claim of higher steady-state FPS. |
| `89733192ca9e64d27fc4e6ba596d6e7988f8f1f5` | Compute the sky octagon's edge parameter with half-planes. | Remove per-pixel atan2, floor and cosine while preserving the geometric boundary. |

The occlusion queue cap is extended locally: losing partial updates invalidates
the old visibility graph and discards an already-running old search. Otherwise an
opened wall can remain hidden until a replacement search finishes, because the
edits that would open it were dropped. The fallback draws by the frustum until a
fresh graph lands; it never waits for the worker. A regression test covers both
idle and in-flight searches, opening the wall after the cap is exceeded.

## Review of the latest Fable changes

`0ad1124c7b` introduced the lean opaque/cutout terrain stages, optional face merging,
plant distance and plant shrinking. `157b4d711f` exposes their settings. The recorded
headset measurements and their caveats are in [quest-gpu-handoff.md](quest-gpu-handoff.md).
Those measurements were made by the previous device session, not this cloud audit.

- Lean varyings remain limited to the headset's Qualcomm Vulkan path without HDR
  output. Fading cutout lists retain the full terrain shader, and warped/native
  material paths retain their own shaders. The imported terrain changes preserve
  the new entry points and their wire formats.
- Face merging remains opt-in. Equal baked corner light does not prove equivalent
  runtime dynamic lighting or fog over a large merged face: those are evaluated at
  vertices. Check nearby moving lights, smooth lighting, emissive packs and moving
  T-junctions before treating it as picture-preserving or enabling it by default.
- Plant shrinking remains opt-in. Its bounds include every base animation frame,
  but not the alpha coverage of smaller mip levels. Resource-pack emissive overlays
  and filtering at a shrinking edge also need device comparison.
- Split streams remain opt-in. Native displacement is still unavailable with that
  arena layout; this integration does not silently enable the combination.

The English and Russian water/foveation/multiview translation entries also had
missing indentation, which made `lang_coverage.py` reject the English reference
and fail its repository-layout test. Their whitespace is repaired without changing
the strings or marking translations reviewed.

## Adreno constraints and next experiments

The private-array workaround addresses a compiler issue, not a missing Vulkan
feature. It is useful independently of multiview or indirect draws. Keep the
Quest-specific occlusion recall experiment off: a shader-side depth comparison
still writes fragment storage, which the existing Quest tests found broken.

Do not enable GPU culling merely because upstream supports another Adreno device.
Quest's two-part solid meshes and stereo frame gate still need a compatible GPU
record/layout. First verify compute writes and indirect draws on both chipsets;
then add a conservative union of both eye frusta, matching opaque/cutout ranges,
and a fallback when required features/limits are absent. Removing indirect
validation globally is not part of this update.

Upstream flat-compute bind-group caching is not selected: the default Quest path
uses attributed section draws, and the existing large storage-buffer limits still
gate flat drawing. Desktop Intel workarounds and broad HDR/refactoring changes do
not establish a Quest performance gain. The heightmap clone fix `d9ed797708`
depends on a heightmap representation absent from this fork and is deferred with
that prerequisite rather than importing an incomplete fix.

## Validation and device procedure

Checked with Rust 1.99.1 and Mesa 25.0.7 llvmpipe (LLVM 19.1.7), Vulkan. Test
debug information and incremental compilation were disabled to fit this cloud
environment. No physical Quest or prepared Minecraft resources is attached.
Desktop software Vulkan can validate pipelines and images; it cannot reproduce
Adreno compiler behaviour, foveation or tile-renderer cost.

| Check | Result |
| --- | --- |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | Passed. The vendored wgpu-core still logs its existing dependency lint-expectation warning. |
| `cargo clippy -p horizon-client --all-targets --locked --no-default-features -- -D warnings` | Passed. |
| Renderer library tests, final full run | 2182 passed, 3 failed, 75 ignored. Failures detailed below; no tests disabled. |
| Six explicitly enabled ignored GPU tests | Passed: three multiview projection/external-array/sky tests, split-stream pixel equality in both eyes, stereo occlusion and per-eye visibility masks. |
| Python suite after repairing language-file whitespace | 831 tests, OK, 4 skipped. The 15 language-coverage tests also pass separately. |
| Renderer doc tests | 8 passed. |
| Formatting, public tree, layers, file size | Passed. |
| Built-in address check | Existing failures in unchanged `options_ui.rs`, `game/quest_room.rs`, and Quest launcher's `news.rs`. |
| APK prerequisites | `sh build-apk.sh --check` stops at missing `javac`; no APK built or headset performance measured. |

The final renderer failures are the two crash-report source-location tests
(`crash::tests::a_panic_becomes_a_report`,
`crash::tests::the_debug_crash_is_reported_as_vanillas`) under stripped test debug
information, and
`gfx::hdr::tests::sdr_screenshots_roll_off_the_world_and_keep_the_hud_as_drawn`:
red `0.9038086` instead of `0.89990234` on llvmpipe. These limitations were already
recorded in [quest-upstream-integration.md](quest-upstream-integration.md).
The room-scene strict pixel comparison failed in the initial run (red 198 versus
196) and passed in the final run. An initial UI child-process launch also failed
when a parallel rebuild replaced its executable; its isolated stable-copy rerun
and the final full run both pass.

`pr_ready.py --base origin/main --quick` was run. Its initial clippy step stopped
at an environment header path (`libudev.h`), and its Python step caught the
language-format defect subsequently fixed here. The two clippy commands and the
complete Python suite above were rerun successfully after those corrections.
The overall self-check remains red on the address check and the documented GPU/
debug-information failures. Vanilla-resource parity harnesses were not run.

For device A/B, build the previous fork head and this update with the same APK
profile. Use the same saved viewpoint, time/weather, render distance, LOD distance,
render scale, refresh rate, foveation and dynamic-resolution setting. Compare
warm, thermally settled runs in both orders. Record CPU/GPU frame times, p95 frame
time and both-eye images. Include tall grass, open sky, cave openings, repeated
block edits and a resource pack with animated/emissive textures. On Quest 2,
also test cold pipeline creation and chunk/LOD fades. No new FPS number is claimed
until those headset runs exist.
