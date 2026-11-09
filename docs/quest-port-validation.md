# Validation: Quest / Visor fixes

The owner requested validation of the final source after initially deferring it. Checks below run
on Linux x86_64 with Rust 1.99.1. ALSA/udev development files were extracted into a local build
sysroot; no system or repository dependencies were changed. No APK has been produced here.

## Final environment results

- `git diff --check` and `tools/check_public_tree.py`: passed.
- English/Russian language JSON: parsed successfully, with no duplicate keys.
- No-default-features client/XR `cargo check`: passed; later headless Clippy also checks this variant.
- Strict Clippy for all seven touched libraries: passed with `-D warnings`.
- VR libraries: 119 Visor + 139 Vivecraft + 20 XR tests passed (278 total).
- Capture/menu routing: all five Python/Rust fixture cases passed.
- Networking: all 18 client-library network tests passed after updating three fixtures to explicitly
  verify the new observer channel registration and then the original configuration replies.
- Full Python suite: 643 tests, four failures, two errors and one skipped (636 passed).
- Workspace `cargo clippy --workspace --all-targets --locked -- -D warnings`: passed.
- Headless client `cargo clippy -p horizon-client --all-targets --locked --no-default-features
  -- -D warnings`: passed.
- Broad selected Rust test run: 4087 passed, 51 failed before Cargo stopped at the renderer crate.
  The renderer was rerun directly on both the modified source and untouched base: the exact same
  51 failing test names were observed. All 1440 test outcomes match exactly (1346 passed,
  51 failed, 43 ignored). The software GPU rejects some required storage-texture formats and
  pixel-readback assertions fail. Two crash-report cases expect file names in stack traces,
  unavailable with this build's debug information disabled. No engine change or silent skip
  was made to conceal these failures.
- Final doctest command for all seven touched libraries: passed (no doctest examples in these crates).

The first broad checks identified compile/lint issues and outdated packet expectations; those were
corrected and rerun. The first `pr_ready` log can retain a failed Clippy step from before the
teleport-test initializer fix; the later standalone workspace Clippy log is the final result.
The first doctest attempt lost a cached rlib while the comparison build was starting; it was
rerun after the baseline compilation ended. Initial failed logs are retained for traceability;
use the later dedicated logs for the final status.

The untouched base reproduced four failures and three errors in 642 Python tests (one skipped).
The final source fixes the stale capture fixture and adds a fifth capture regression case. The six
remaining failures/errors are reproduced on the base: README/parity accounting, script/audio
policy checks, the remote-mining extraction fixture and the remote-rocket fixture. They are
recorded rather than expanding this port change into unrelated engine/policy repairs.

The overall readiness result is **not green**: the reproducible base failures above and the
unperformed Quest scenarios must remain visible in review. These checks show no new failing
cases compared with the base; they do not establish hardware usability or complete Visor parity.

## Commands for reproduction

```sh
cargo fmt --all -- --check
cargo test --locked -p horizon-visor -p horizon-vivecraft --lib
cargo check --locked -p horizon-client -p horizon-xr --no-default-features
cargo clippy --locked -p horizon-visor -p horizon-vivecraft -p horizon-client-input \
  -p horizon-client-render -p horizon-client-lib -p horizon-client -p horizon-xr \
  --no-default-features --lib -- -D warnings
cargo test --locked -p horizon-client --bin horizon-client --no-default-features visor_
python3 -m unittest discover -s tests -p test_world_input_capture.py
python3 -m unittest discover -s tests
python3 tools/pr_ready.py --quick --base origin/main
```

For full default-feature Linux checks, install the repository's ALSA/udev prerequisites. Run the
existing Quest APK build procedure in `packaging/quest/README.md` with its Android SDK/NDK and assets.
Real gameplay/parity harnesses need the owner's prepared Minecraft 26.3 resources.

Use fresh scratch homes for all game/launcher runs as required by AGENTS.md. No player saves,
accounts, prepared game files or build directories belong in the PR.

## Highest priority: pre-world authorization

1. A 26.3 server sends a configuration-phase dialog before Login, with a password field and buttons.
   Both controller rays must work; a field click must open the keyboard; typing, backspace, arrows,
   clipboard and submit must reach the dialog. No gameplay click, jump or rotation may leak through.
2. Repeat after Login creates a player but before PlayerLoaded. Finish terrain loading while the
   dialog remains open: its pointer and keyboard must remain available.
3. Repeat during proxy reconfiguration, with multiple consecutive dialogs, a waiting response,
   a resource-pack confirmation and a code-of-conduct screen. Test closing/reopening the keyboard.
4. Complete authorization, then loading: capture gameplay only after the last screen is gone.
   Restore focus during loading; a focus-gain event must not capture early. A click beside the
   panel must not dismiss a mandatory server prompt.
5. The capture fixture now includes attempted capture/focus gain while connection input is pending;
   all five capture cases pass, including the actual menu-routing method extracted into a Rust fixture.

## Height, camera, recenter and immersion

1. Fresh config, standing launch; fresh config, seated launch then stand. Repeat with existing
   stored calibration, Auto Height off/on and explicit seated mode off/on. No persistent extra
   startup-height offset may remain after standing. Include 1.2 m and taller users and explicit
   manual calibration for shorter users (automatic low-pose rejection is deliberate).
2. Measure Now after its countdown, while standing still; reject a moving or insufficient sample
   window. Recenter standing, crouching and seated; it must not redefine stature from that pose.
3. Change world scale and MATCH_MODEL / REAL_SIZE. Check camera, hands, rays, keyboard and chat;
   scale and standing-height lift must each be applied once. Check low ceilings, swimming,
   gliding and crawl headroom separately.
4. Lose tracking, cover cameras, open the system menu, remove/refit the headset, resume and trigger
   a reference-space change. Invalid/missing position must not start a jump/crawl or recalibrate.
   Old history must not cross clock rollback, pause or recenter.
5. Stand up from a chair, crouch then stand, lean/look down, do small and large real jumps, then
   repeated jumps. A real jump needs a stable stance and upward speed; tune only with device
   evidence. Automated posture tests supplement, but cannot replace, this device scenario.
6. A brief low sample must not crawl. A sustained low pose must enter, retain within hysteresis
   and leave after standing. Confirm no stuck room-sneak key. Repeat in menus, during loading,
   flying, in water/lava, dead, sleeping, riding and climbing.
7. Duplicate-millisecond samples and multi-hour timestamps must give finite speed and continue
   calibration; simulators now publish real time and configured head height, not timestamp zero.

## UI, mechanics, networking and XR resources

- Both hotbar grips together: both wheels visible, independent selection and release. Simulator
  captures include `overlay_offhand.png`; verify on Quest too.
- Creative and survival swings, each hand, with collision on/off. Hoe grass/dirt, invalid faces,
  blocked-above transforms and crops; weapon correct-tool blocks; verify no duplicated actions.
- Flat observer, Quest peer and Visor peer; delayed handshake beyond 40 ticks; reconnect, respawn,
  dimension change; pause poses for over five seconds, then resume pose-only updates.
- Enable/disable all nine keyboard layouts, prevent removing the last one, auto language, Shift,
  two simultaneous hands, held-repeat ownership and cancellation when leaving a key.
- Drag and resize keyboard from either hand; lose one controller mid-resize; release and reopen;
  verify saved size, recenter, world scale and pointer suppression behind the keyboard.
- CUT/COPY/PASTE and selection in chat, server dialog, signs, books, creative/recipe searches,
  anvil, options and title fields. Verify that ordinary physical editing still behaves as before.
- HUD/chat setup persistence; hidden chat must not leak back into the HUD. Defaults should retain
  the prior placement. Built-in setup is not an arbitrary overlay editor.
- Count swapchain allocations before first opening and on repeated opens. Inject allocation and
  acquisition failures; confirm six-second backoff and recovery. Open all panels and measure
  peak GPU memory, first-open latency, frame time, thermal behavior and FPS.
- Keyboard descriptions should rebuild on open/layout/Shift changes, not each steady frame.
  Compare port-only allocation/performance changes with the base; do not credit unrelated engine
  changes as XR gains.
