# Quest / Visor port: current change set

Latest follow-up: [Quest input, mining and controls](quest-controls-followup.md).

Base: `ytpk/horizon-xr` at `6c82f6d56ec17a1a9530a6402202fa304d6d05e5`.
Reference: Visor 0.6.1, `c01354fad8ac03a016c249239a04b07adeb36476`.

This is a standalone Quest fork. Changes in shared client files are limited to VR integration,
server-screen input needed on the headset, and the existing edit-field paths used by its keyboard.
No general terrain, meshing, lighting, world-generation or engine optimization is included.

**Implementation and validation are tracked separately.** The owner first deferred checks, then
requested all checks available in this environment. See [validation results and device scenarios](quest-port-validation.md).
No Quest runtime test or performance measurement has been performed; software checks cannot establish
physical comfort or exact behavioral parity with Visor.

## Functional changes in this package

| Area | Implementation | Validation status |
|---|---|---|
| Connection / authorization before world entry | Connecting, level loading, reconfiguration and screens over `Playing` retain the VR menu pointer. Loading cannot grab the native cursor early. Server dialogs receive keyboard text, editing keys and clipboard actions. Off-panel clicks cannot dismiss a loading/auth screen as gameplay. | Five capture/menu cases and network tests pass; Quest scenario queued |
| Desktop observer | Registers the Visor channel and sends a non-VR handshake; a headset starting later can upgrade a Horizon server session. | Final integration queued |
| Delayed protocol negotiation | Auto's fallback remains distinct from explicit Vivecraft; a delayed Visor handshake wins, and subsequent Vivecraft replies are ignored. | Protocol unit tests pass; real-server integration queued |
| Remote players | Pose silence no longer destroys the metadata required by pose-only updates. Entity removal and level changes clear it. Model height is not clamped to Vivecraft's range. | Pose and model-scale unit tests pass; integration queued |
| Creative and room swings | Visor activation rules replace Vivecraft's creative AUTO exclusion. Block queries use the swinging hand; hoe swings query the existing transformer, and weapon breaking also checks the correct-tool condition. | Queued |
| Hotbar | Both wheels have independent overlay images and XR quad layers. | Queued on device |
| Keyboard placement | Drag bar and two-hand resize handle inside its existing quad; resized scale is persisted. Placement is converted consistently between stage metres and scaled world offsets. | Queued on device |
| Keyboard editing | CUT/COPY/PASTE use existing selection/clipboard implementations in chat, dialogs, signs, books, creative search, recipe search, anvil and options/title fields. Shift navigation is forwarded where the field supports it. | Queued; not a claim about every field in Horizon |
| Keyboard layouts | GUI page for all nine layouts, preserving the serialized comma-separated list; cannot remove the last selected language. Auto-layout and scale are exposed there. | Queued |
| Overlay setup | GUI page for HUD placement/opacity and chat visibility, XYZ offset and size. This is configuration of built-in overlays, not Visor's complete addon/template editor. | Queued |
| Height and camera anchor | Vertical rest follows calibrated standing pivot + 0.1 m eye offset, not the first startup pose. World-scale lift is compensated once in the camera base; REAL_SIZE uses physical height. Manual calibration requires a recent, quiet sample window. | Seated-start camera regression passes; queued on device |
| Automatic height | Invalid values are rejected. Automatic estimates below 1.2 m are not treated as standing stature: a seated/crawling launch must not silently calibrate a tiny standing body. Shorter players can use explicit Measure Now. | Intentional comfort adaptation; queued |
| Recentering | Reset Origin is reachable on current Controls/Height pages, recenters horizontally without redefining standing height, and invalidates old gesture samples. Runtime reference-space changes are applied at their announced XR time. | Queued on device |
| Tracking loss / discontinuity | Positional validity is required. Missing/unfocused tracking, clock rollback, large jumps or a pose epoch change clear room gesture histories and held posture state. | Queued |
| Live jump and crawl | Physical jump requires a settled stance and an upward motion before rearming. Crawl transitions require 100 ms of a sustained candidate and use a short height average; sneaking has hysteresis. Menus, loading, flying and other incompatible states gate gestures. | Comfort/timing changes; queued on device |

## Optimizations restricted to XR / port code

- Overlay swapchains are allocated on first use, then reused until session end. Allocation/acquisition
  failures back off for six seconds. Hidden overlays are not eagerly allocated at session startup.
- Keyboard descriptions and labels are cached between layout/Shift changes. Key labels do not allocate
  a replacement string for every draw, and hover messages use trace logging.
- Hotbar hit testing uses a stack buffer instead of a per-hand temporary vector.
- Swing-speed averaging streams its history instead of collecting a temporary vector.
- Repeated history timestamps are coalesced. Speed uses integer timestamp differences, so long uptime
  does not turn a small interval into zero through float precision loss.

No FPS, latency, battery or memory improvement has been measured. First-open allocation latency and
peak memory with every overlay opened must be measured on the Quest.

## Remaining work, not represented as completed

- Arbitrary per-profile OpenXR action-set editing for other controller families. Quest Touch gameplay
  bindings, including triggers and grips, are now editable; see the follow-up above.
- Keyboard-only gameplay / synthetic real-key fallback with no screen; the existing legacy keyboard
  toggle remains. Not every nonstandard field has a dedicated clipboard route.
- Full Visor overlay/addon/template editor, imported layouts and arbitrary overlay dragging.
- Exact menu cloud shapes/jitter and inverse-color crosshair blending.
- Native Visor addon/events API and deliberate compatibility work for native mods.
- Full-body rendering/IK beyond the existing model and pose handling. Network joint data alone does
  not prove visible tracked limbs.
- Physical comparison of jumping, crawling, swimming, climbing, bows, item poses and room movement
  with Visor. These cannot be declared exact from source review.
- SpaceWarp/motion-vector/depth work and general render-pipeline changes are outside this package.
  The existing multiview/foveation implementation is not claimed to be newly completed here.

`visor-port.md` and `vivecraft-port.md` contain historical round-by-round statements, including TODOs
later superseded by code. They are useful evidence of past work, not a current completion percentage.
