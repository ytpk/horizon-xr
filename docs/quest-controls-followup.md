# Quest input and mining follow-up

Base: `3e914e5c9733605f28b0676563b396db5400e979` (the previous public source package).
Reference: Visor 0.6.1, `c01354fad8ac03a016c249239a04b07adeb36476`.

## Jumping while moving the head

Room-scale displacement goes through `MotionWorld::room_step` between simulation ticks.
A zero-Y `Entity.move` clears `onGround` even on a flat floor. Small displacements from
turning or leaning could therefore make the next button jump, or the physical-jump tracker,
see an airborne player. This is an input-independent ground-state problem.

After the VR room step, revalidate support with a thin collision probe directly beneath the
feet. Retain ground only if the player was grounded, is not rising or a spectator, the
requested step was horizontal, and support remains at the destination. Walking off a ledge
must still clear ground. Ordinary movement, gravity and jump physics are unchanged.

The regression fixture executes the production adapter with a finite geometric floor and
an isolated `Entity.move` seam. It checks repeated small head displacements, edges, an
already rising jump, airborne movement, vertical movement and spectator movement. This is
not a substitute for headset testing or an end-to-end collision test with prepared resources.

## Mining comparison with Visor

`TaskSwing.java::getType` returns `VR_PLAYER_TICK`. Its `handleBlockSwing` deliberately gives
one block action `3 + min(speed - threshold, 4)` damage steps: 3–7 hits at the configured
threshold. `findBlockHitAlongArc` returns the first eligible intersection, not one action
for every subdivided arc segment or block face.

The port previously called the task on every rendered frame, advancing history and the
one-task-tick block suppression at headset refresh frequency. It now runs at most once per
simulation tick. A regression compares complete mining-action sequences at 20, 72, 90 and
120 render FPS. The documented speed bonus is retained.

The VR ray adapter also uses `VoxelShape.clip`'s nudged-start test to identify an inside hit,
including an inward segment starting on a block face. Zero-face inside hits remain rejected.
Tests cover all six faces and partial-height outlines. Menus and tracking discontinuities
clear the old swing history so the next interaction cannot trace an arc from an old pose.

## Quest controls

Open **VR Settings → Controls → Quest controller bindings**. Bindings are stored in the
existing `vivecraft-client-config.json` file and apply without restarting the XR session.

- Jump, main/off-hand attack, main/off-hand use, inventory, drop, swap, chat, keyboard,
  sneak, sprint, both hotbar wheels and the game menu can be rebound or unbound.
- Sources are the main/off-hand triggers, grips, lower/upper face buttons and stick clicks.
  Main/off follow the handedness setting. Off-hand attack requires two-handed mode.
- Movement and turning sticks can be selected separately.
- Shared assignments remain possible and are marked **shared binding** in the UI.
- **Restore Quest bindings** resets only these assignments and stick choices. It preserves
  height calibration, world scale and handedness.
- Menu pointer/navigation use the fixed recovery layout. The physical Menu button remains
  available to reach settings even after unbinding gameplay actions.
- A captured physical trigger does not disable the other hand or an unrelated rebound face
  button. Hand-use and active-hand routing use the configured logical actions.

This is a Quest Touch gameplay binding editor over the existing runtime inputs. It does not
claim an arbitrary OpenXR interaction-profile/action-set editor for other controller families.

## Keyboard

The binding and the VR menu now open the selected Visor keyboard rather than always opening
the legacy physical keyboard. In ordinary gameplay, the Visor keyboard action opens chat
first, giving typed text an explicit destination. Existing server dialogs remain in place.

Delete, Home, End and Select All supplement the existing editing keys. Shift navigation
includes Home/End. A layout or Shift change invalidates both hover indices before another
hand can act on the rebuilt layout. Opening the keyboard with a held trigger does not type
an accidental first character. All nine layouts have non-overlapping, reachable editing keys.
This does not add a screenless synthetic-key gameplay keyboard.

## Height thresholds

The existing Visor thresholds compare actual head-pivot height with calibrated standing
height: 0.85 sneak, 0.70 crawl and 1.05 jump by default. Physical jumping additionally uses
rise/speed/stance checks. Button jumping does not use these height ratios. This follow-up
does not change the camera-height calibration or gesture thresholds from the previous build.

## Validation

Final Linux checks (Rust 1.99.1):

- Workspace Clippy, all targets, `-D warnings`: passed.
- Headless client Clippy, all targets, `--no-default-features`, `-D warnings`: passed.
- Final library run: 87 input + 123 Visor + 140 Vivecraft + 20 XR tests passed (370 total).
- VR room-step regression fixture: passed. The existing capture/menu regressions also pass.
- Broad selected Rust run: 4,089 passed, 51 failed, 112 ignored before Cargo stopped at the
  renderer crate. The 51 failing names exactly match the original base's renderer failures.
  As documented in the previous validation report, the software GPU lacks required features
  and debug-information-free builds fail two crash-stack assertions. No failure was hidden.
- Python suite: 644 tests; four failures, two errors, one skipped. The same six failure/error
  names reproduce on the previous base; no added failing case was observed.
- Doctest commands for all seven touched libraries: passed (no doctest examples).
- `cargo fmt --all -- --check`, `git diff --check`, public-tree check, both locale JSON files:
  passed; English/Russian JSON have no duplicate keys.

Reproduction commands:

```sh
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo clippy -p horizon-client --all-targets --no-default-features --locked -- -D warnings
cargo test -p horizon-client-input -p horizon-visor -p horizon-vivecraft -p horizon-xr --lib --locked
cargo test -p horizon-client -p horizon-client-lib -p horizon-client-input -p horizon-client-render \
  -p horizon-visor -p horizon-vivecraft -p horizon-xr --lib --bins --tests --locked
python3 -m unittest discover -s tests
python3 -m unittest discover -s tests -p test_vr_room_step.py
```

The full readiness result remains non-green because of the known base failures. A passing
source-level regression does not establish real-headset behavior.

Hardware scenarios still required: button jumps while turning/leaning and walking, ledges,
slabs/stairs and moving supports; physical jumps with head movement; identical swings at
72/90/120 Hz; entry, exit and dragging along each block face; tools and both hands; every
binding, conflicts/reset, left-handed mode, menu recovery, keyboard toggling and server dialogs.
No APK or physical Quest run was performed in this environment.
