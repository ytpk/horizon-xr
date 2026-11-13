# Native Live sign-in and Quest render-scale stability

## Sign-in

The Rust account crate adapts prismarine-auth 3.1.1 at commit
`a7f4b0b8f79b7526496c99a4794c1ecc1a36dd51`; Node.js is not required.
With no configured `HORIZON_MSA_CLIENT_ID`, it uses Live device-code sign-in with
Minecraft for Nintendo Switch's public title ID `00000000441cc96b`.
This is not Prismarine's own registration or a claim of Microsoft approval for this fork.
The room identifies the provider before sign-in. Availability is experimental and service-dependent.

A custom ID preserves the Azure device-code flow; an explicitly empty ID disables sign-in.
The Live flow signs Xbox user/device/title/XSTS requests with an ephemeral P-256 proof key,
then uses the existing Minecraft token, Java ownership and profile checks.
Cached refresh tokens are bound to their client ID. Changing providers requires signing in
again rather than submitting an old provider's refresh token to the new provider.
Horizon’s registration is not a fallback.

## Render scale

Both in-game graphics controls now use the same VR render-scale setting as the room.
The scale is limited to 25–200% of the runtime-recommended eye dimensions, including legacy
configuration and slider input. Percentage labels replace the misleading unitless value.
Desktop resolution scaling no longer multiplies the headset eye scale a second time.
Swapchain resizing waits for a scale value to settle for 150 ms, avoiding repeated large
allocations while moving a slider. Runtime maximum image dimensions still apply.
Desktop rendering keeps its own resolution scale. Dynamic resolution remains bounded by
its configured base scale.

## Validation and remaining device checks

Local checks completed:

- 402 Rust tests across account, input, Quest launcher, Vivecraft settings and OpenXR.
- 37 graphics-menu tests, including slider stepping, change tracking and Undo.
- Python suite: 831 tests, initially 33 skipped without Rust on PATH; the affected
  Rust/Cargo-dependent modules were rerun with the toolchain (60 tests, all passed).
- APK wrapper mock checks: preflight, build order, paths with spaces, checksums,
  early failure, unsupported JDK and invalid arguments.
- Workspace layer and public-tree checks; formatting of both workspaces.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`.

Local HTTP fixtures exercise Live device-code cookies, polling, refresh, Xbox proof signatures,
Minecraft ownership rejection and profile retrieval. They do not contact Microsoft or use a real account.
Unit tests cover scale normalization, slider round trips and delayed resize behavior.

Before treating this as verified on hardware, test a real licensed account, cancellation,
expired codes, refresh after app restart, provider changes, and restricted Xbox accounts.
On Quest, move both scale controls through their range, restart the app, exercise Undo,
and verify eye dimensions and memory use while enabling multiview, spatial scaling and
dynamic resolution. APK packaging, actual service acceptance and device FPS are not verified here.
