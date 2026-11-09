# Visor port: status, switches and device checklist

Current fork changes and remaining work: [quest-port-status.md](quest-port-status.md).
Validation results and remaining device checks: [quest-port-validation.md](quest-port-validation.md). The rounds below are historical;
older TODO and done labels are not a current completion percentage.

Target: Visor 0.6.1 (commit `c01354fad8ac03a016c249239a04b07adeb36476`, `visor:channel` network version 5) and the
server half of VisorPlugin 0.2.0alpha (commit `b4f3fc8be0e67f517c3e98c07ac1b5510eaf3d24`) in Horizon on Meta Quest
(OpenXR). Branch `visor-port`, from `vivecraft-port` (left untouched). The analysis and the phases are in
[visor-port-plan.md](visor-port-plan.md); the Vivecraft port it sits beside is in [vivecraft-port.md](vivecraft-port.md).

Licensing (decided by the user; the fork is personal and will be published, and both sides gave permission): Visor is
MPL-2.0. Derived code lives in `crates/horizon-visor` (`license = "MPL-2.0"`, `LICENSE-MPL-2.0`) and in files that carry an
SPDX `MPL-2.0` header naming the Visor sources they port (`horizon-client/src/game/{visor_net,visor_server,visor_play,
visor_trackers}.rs`, `horizon-xr/src/tracking.rs`). Other files that call into it stay MIT (or LGPL where they already
were). See `THIRD_PARTY_NOTICES.md` and the credits page (VR Settings > About / Credits).

Status words: **ported** (compiles, unit tested against byte layouts worked out by hand from the Java, or pure logic
tested), **ported-untested** (compiles, needs the headset or a Visor server), **todo**, **skipped** (reason).

Rounds 1 to 3 used no device: what they touch (OpenXR, rendering, a real Visor server) is **ported-untested** unless a
later section says it was looked at. Round 4 (the look and feel, below) ran on a Quest 3S that lay on a table, over adb:
screens, the game menu, the hotbar wheel and the title room were seen in screenshots and the log; nothing could be felt (no
head, no hands), so every gesture and every comfort question is in the checklist at the end of this file.

## Post-audit fixes (standalone Quest fork)

These fixes address seven functional gaps found against the pinned Visor 0.6.1 source. They do not
claim to complete the unported settings editors, keyboard features, menu clouds or crosshair blend.

- Login announces Visor even without an active XR session (`vr_active = false`), so flat clients can
  see VR players. The Horizon server can upgrade that session when the headset starts after login.
- Auto keeps a distinct fallback state: a delayed Visor handshake wins over the Vivecraft probe;
  an explicit Vivecraft selection still ignores Visor. Late Vivecraft replies cannot override Visor.
- Remote VR snapshots survive network silence. Entity removal and level changes clear them, rather
  than a 100-tick pose timeout that made subsequent pose-only updates unusable.
- Swing activation follows Visor's creative setting and excludes passengers and climbing players.
  Horizon's explicit collision-off and seated settings remain respected.
- Hoe swings query the existing block transformer without mutating the world. Dirt/grass that can
  be tilled use the item, while an invalid face or blocked transform retains the mining fallback.
  The query runs with the swinging hand's stack, including the redirected off hand.
- Remote model height is no longer clamped to Vivecraft's 0.5–1.5 range.
- Each hotbar wheel has its own quad layer; holding both grips keeps both visible. The simulator
  writes the second wheel to `overlay_offhand.png`. The extra swapchain is 1280 by 720, like the other
  overlay panels; its performance and memory cost still need measurement on Quest.

Regression tests cover creative activation, a flat observer, a headset starting after login, delayed
protocol selection, pose-only recovery after 200 silent ticks, entity removal and small/large model
scales. No Quest device was attached for this audit. Before merging, check both wheels together on
Quest, creative/survival swings, hoe swings on grass/dirt (including blocked-above and underside
hits), a delayed server response and a network pause longer than five seconds. Test existing controls,
chat, keyboard and screen handles alongside the new wheel layer.

## Visor look and feel: audit (round 4)

The user's complaint was that the port "does not look like Visor, it still feels like Vivecraft". The hypothesis in the
task was checked against the Java and the assets (not the plan) and held: rounds 1 to 3 added Visor's protocol and a few
mechanics, but every thing the player sees and touches came from the Vivecraft port (`horizon-vivecraft`,
`game/vr*.rs`, `game/headset*.rs`): the wrist HUD, the radial ring on B, the inventory on X, jump on the stick click,
30 degree turns, the hotbar paged by the grips, the world keyboard, Vivecraft's settings tree, the panorama behind the title.
The plan's decision to drop Visor's overlay system and screens is reversed for what the player sees; the widget framework
is still not ported 1:1 (a Horizon screen or a HUD draw stands in for each Visor screen).

Status words: **done** (changed, compiled, unit tested where it is logic), **device** (also looked at on the Quest, by
screenshot or log), **partial** (the visible part is Visor's, a part is not), **not done** (reason given).

| What the player sees or feels in Visor (source: Java / assets) | Horizon before | Visor file | Now |
|---|---|---|---|
| Touch layout (right-handed): main trigger = left click, A = right click, X = off-hand right click, left trigger = off-hand left click, B = jump, Y = Menu, right stick click = shift, left stick = move, right stick = rotate; grips = hotbar wheels; middle click, sprint, screenshot unbound | trigger attack, A use, **B radial ring, X inventory, Y pause, left stick click jump, grips page the slots, off trigger teleports/walks** | `input/actions/*`, `actionset/GameActionSet` | done, device (game menu and wheel by simulated buttons; nothing else can be felt on a table). `horizon-xr/src/lib.rs::sync_input` |
| Rotation: snap 45 degrees (threshold 0.5), smooth 0.06 per tick on step 0 | 30 degrees in the player's file | `GameActionRotate`, `VRClientSettings.worldRotationStep` | done: the profile sets 45 |
| Movement: stick relative to the head (`rotationMode = HMD`), flying relative to the off hand, auto sprint at 0.9 | stick relative to the off hand (`CONTROLLER`) | `GameActionMovement`, `rotationMode`, `rotationFlyMode`, `autoSprint`, `sprintThreshold` | done: `vrFreeMoveMode HMD`, `vrFreeMoveFlyMode CONTROLLER` (the off hand) |
| Teleport mode: the move stick aims the arc, letting go teleports; no trigger teleport, no "trigger walks" | off trigger | `GameActionMovement`, `TaskTeleport` | done: the stick aims (`game/vr.rs`), the trigger does nothing for movement |
| Teleport arc: cyan 0.9 (may fly) / 0.8 (survival limited) / 0.3 at half alpha (no landing) | green / red lines | `HandEffectTeleport` | done, device-booted (round 5: the dashes scroll along the arc at 2.4 x 0.6 segments a second, the landing mark is two spreading square ripples as `vr_teleport_point.fsh` computes them, both lit by the light where they land; drawn as lines, not Visor's shader quad) |
| HUD: the vanilla HUD as an overlay 1 m wide, 1.2 m ahead, 0.1 m below the eyes, on the head; **no row of item slots** in it (`hud_disabled`); chat is a second overlay at (0.392, 0.214, -1.706) scale 1.2; "Defaults: Hand HUD" moves it to the off hand | wrist HUD 0.6 m, hotbar touched with the main hand | `VROverlayHUD`, `VROverlayChat`, `GuiMixin.visor$noVanillaHotbar`, `presets/defaults_hand_hud.yml` | partial: head HUD at Visor's place and size, hotbar row left out (`visorHudHotbar` brings it back), the Hand HUD preset puts it on the wrist. Round 6: chat is Visor's own overlay, a head-locked panel at (0.392, 0.214, -1.706) scale 1.2 (`game/visor_overlay.rs::chat_overlay`), **done** |
| Screens: an overlay as wide as its scale in metres, 1 m ahead of the face at the moment it opens, chat 0.5 m higher, books and signs 0.25 m higher, no "over the block" placement; a click beside the screen closes it | 1.5 m wide quad, container screens over their block | `VROverlayGameScreen`, `VROverlayPose` | done as before (placement); round 6 added dragging and resizing by the handle strip under the panel (`horizon-xr/src/screen_grab.rs`, `horizon-visor/src/overlay.rs`), checked on the Quest by log only (the handles need two real hands): **done, device-booted** |
| Main menu room: you stand on a moss floor of the play area's size (+2 m border) in a sky that follows the clock (palette, sun and moon discs, real moon phase, 500 stars, shooting stars, a UFO at midnight), over blocky clouds at -14 m, the dotted VISOR sign ahead; panel 2 m wide at (0.02, 1.3, -1.5) | the title's panorama cube behind a 1 m panel in front of the head | `DecoratorMainMenu`, `VRMenuSky`, `VRMenuFloor`, `OverlayGameScreen.orientMainMenu` | round 6: the midnight UFO (`menu_scene.rs::ufo_at`, `menu_scene.wgsl::ufo`; exact `ufoParkingDir`, 11 by 5 dots, chasing belly lights), the headset icon over the sign (`icon_headset.png` sampled to 21 by 10 dots with `withoutCornerElbows`) and box clouds (5 m tiles, Visor's colours, face shades 1 / 0.85 / 0.72, drift) are **done, device** (UFO seen at `HORIZON_MENU_HOUR=0`). **Not exact**: the clouds are hashed clumps per 5 m tile, not Visor's 25 m cells with its nine `CLOUD_SHAPES`, per-cloud jitter and 80 % fill (that needs the 64-bit `hash01` and per-cloud tile offsets that a fullscreen shader cannot share; it would need real geometry) |
| Game menu on Y: tabs Main / Commands / Tools, buttons as listed, accent line, "Return to Menu" at the foot | radial ring of eight bindings | `VRPauseMenuScreen`, `ActionMenu` | done, device: `game/visor_menu.rs` (layout tested against hand-computed pixels) |
| Hotbar: hold the grip, a 3 x 3 wheel (slot 0 in the middle, 1 to 8 clockwise), the hand's aim picks, centre radius 50, hysteresis 7 degrees, names under it, `slot_numbers` | grips page the slots; HUD hotbar touched at the wrist | `VROverlayHotBar`, `TaskHotBar`, `HotBarSlice` | done, device (round 5): drawn from Visor's own textures (`visor_art.rs`) on a panel hung 0.6 m in front of the hand that opened it (`game/visor_overlay.rs`, a second XR quad layer), seen in the simulated headset's `overlay.png`; on the Quest the layer is acquired and shown (log `the overlay panel is in use`, 97 fps while it is open, 113 closed). The off-hand wheel and marks need the two-handed mechanic (below) |
| Settings: "VR Settings" with tabs Presets, Rendering, VR Body, Controls, Movement, Immersion, GUI, World; Visor's names and tooltips | "VR Settings" with Vivecraft's tree (HUD, Other HUD, Stereo, Standing, Roomscale, Weapon collision, Backpack, Climbing, Free move, Teleport, Seated, Player model, Keyboard, Radial, Quick commands, Menu world) | `VRSettingsCategory`, `VROptionWidgetType`, `lang/en_us.json` | done: `horizon-vivecraft/src/pages.rs` (Vivecraft's pages stay in the code, none is reachable from the menu), 515 Visor strings imported to en_us and 434 to ru_ru. Not done: Action Bindings (Horizon has no per-profile VR binding editor), Keyboard Layouts, Overlay setup, Addons, Mixed Reality, Third Person, Mirror (no desktop mirror on a Quest) |
| Keyboard: an overlay panel under the screen (chat opens it), layouts per language | Vivecraft's world keyboard on any focused text field | `VROverlayKeyboard`, `KeyboardLayouts` | **done (round 6)**: Visor's keyboard itself: the nine `KeyboardLayout`s (EN RU UA DE FR ES IT PT HU), `VRKeyboardScreen`'s buttons at their pixels, its own quad layer (`OVERLAY_KEYBOARD`, also under the title's screens), either hand's ray and trigger, shift toggle, language button, backspace and arrows repeating after a second, hover highlight and haptics; opens with chat, signs, books and on a click on a text field; `visorKeyboard` off brings the old keyboard back. Device: typed and sent a chat message with the rays. Not done: CUT and COPY (Horizon's fields keep no selection; PASTE works), dragging and resizing the keyboard, the fallback real-key presses when there is no screen |
| Hands: the player's arm at 0.4, side shift -0.375 (slim -0.34375), depth 0.75; items at 0.9375 with arms scale 0.5 and grip 0.65; GUI hands are 18 cm grey boxes (64) | same numbers (both mods share the arm), same GUI hands | `VRHandRenderer`, `VRItemPoseDefault` | round 5: world hands depth tested, GUI hands on top. Round 6: per-item-type poses are `VRItemPoseDefault` (`horizon-visor/src/item_pose.rs::compute`, `compute_tool`: sword, tool, bow, crossbow, trident, spear, shield, block and the rest, with the aim to grip turn), applied to the held items; **done, ported-untested on a hand** (the unit tests pin the tables) |
| Menu pointer: a light grey (228) ray of 3.2 mm from the hand that clicked last; either trigger takes the cursor | white translucent ray from the main hand | `VRHandRenderer.renderCursorLine`, `MouseClickHandler.processCursorUpdate` | done, device (ray colour, cursor hand by trigger) |
| Crosshair and block outline: the vanilla crosshair sprite on the face at 0.125 scale, half bright on a miss | Vivecraft's crosshair lines | `HandEffectCrosshair` | done (round 5): the vanilla `hud/crosshair` sprite on the quad, over everything, half bright on a miss (`game/headset.rs`, pushed with the GUI hands); Visor's inverse-colour blend is not reproduced (plain alpha) |
| Sneak when the head is under 0.85 of the standing height, crawl under 0.7 (exit +0.03), jump when the head rises 0.1 m in 0.25 s and is over 1.05 of the height | 0.4 m below, 0.05 m above | `TaskRoomSneak`, `TaskRoomCrawl`, `TaskRoomJump` | done: the settings are the shares; the trackers read the metres derived from them (`vr.rs::visor_height_tick`); crawl uses `visorCrawlThreshold` |
| Local crawl pose | server only | `TaskRoomCrawl` | done: `Player.room_crawling` makes the swimming pose (`pose.rs`) |
| Auto height (90th percentile of the resting head, dead zone 2 %, a lower value only at a screen or the title, the hint and the "set to" chat lines), Measure Now averages 1 s after a 3 s countdown | calibrate at once on the average of 0.5 s | `PlayerHeightTracker`, `HeightEstimator`, `HeightFormat` | done: `horizon-visor/src/height.rs`, `vr.rs` |
| Swing speed threshold 3.0, in creative too | 3.0 | `TaskSwing`, `swing.in_creative` | done: `visorSwingInCreative` gates the swing task (`visor_play.rs`); Consume (`visorRoomConsume`) gates the eating tracker (`vr.rs`) |
| In-block vignette only; no locomotion vignette | comfort vignette on moving, on in the file | `VRCameraOverlaps`, `VRShaderInBlockVignette` | done: `fovReduction` off in the profile |
| Body: hands only by default, full body option; arms scale 0.5 | same | `VRBodyType*` | unchanged |
| Two-handed play: the off hand holds the hotbar item of a slot, `ACTIVE_HAND` switches on click | the vanilla off-hand item | `VROffhandEquipment`, `TaskHotBar`, `GameActionHotBar` | **done (round 6)**: the off hand holds the hotbar slot chosen on the off wheel (left grip), `ACTIVE_HAND` on a click, the left trigger acts for it (`game/visor_hotbar.rs`, `game/shield.rs`, `visor_server.rs`); device: the left grip opened the wheel and the off hand showed the shield of its slot |
| Hand tracking and body tracking: permission asked | manifest only | AtumVR | done (not device-tested: the headset has it granted): `horizon-xr/src/permission.rs` asks the activity when a tracker is refused, retries after 6 s |
| Movement-state overlay on the off hand, fullscreen warning, inactive-VR screen | not there | `VROverlayMovementState`, `VROverlayFullscreenWarning`, `VRInactiveScreen` | partial (round 5): the movement-state icon (Visor's `movement_state.png`) shows on the off hand when the HUD is on the wrist (the Hand HUD preset), on the new second panel; the fullscreen warning is a desktop-window notice (**skipped**: a Quest has no desktop window); the inactive screen is covered by the headset's own pause |
| Fingers and legs of other players | not drawn | `VRBodyRendererFull`, `RemoteHandsPose` | **not drawn, as in Visor** (round 6, checked against the Java): `VRBodyRendererFull` and `VRPlayerModelFull` draw the vanilla player model with arms aimed by `ArmPoseClamp` and the head by the headset, nothing else; the received hand joints and trackers only feed `GameEffectTrackerDebug` (off, local player). The joint maths is kept in `horizon-visor/src/body_rig.rs` (tested, not called) |
| Foveation | crash on start | n/a | the crash is fixed (below); foveation stays off |


## What a Horizon client does on the network

1. Configuration phase: `minecraft:register` names the channels of the protocols the player allows
   (`visor:channel` and `vivecraft:data` in Auto).
2. Right after the `Login` packet (the tail of Visor's `handleLogin`), in a headset: `HANDSHAKE` `{vrActive: true,
   networkVersion: 5, "0.6.1"}` on `visor:channel`, to any server. A flat-screen Horizon stays silent (Visor would send
   `vrActive: false`; Horizon's Vivecraft port is silent on a flat screen too).
3. A Visor server (the mod or the plugin) answers with an empty `HANDSHAKE`, then `SERVER_SETTINGS`, then for a VR
   player `ROTATION_Y` and (with `twoHandedVR`) `OFFHAND_SLOT`. Only then does the client send its state (changes only,
   then `POSE_DATA` every tick). Chat says "Server supports Visor!" on a remote server.
4. Auto holds Vivecraft's `VERSION` back; 40 client ticks with no Visor answer and it is sent (log: `VR protocol: no
   Visor answer ...`). So a server with both mods gets Visor. `horizonVrProtocol` VISOR or VIVECRAFT skip the wait.
5. Before the server answers (and on a vanilla server) the settings are `ServerSettings::vanilla_server()`: Visor's
   `joinedDedicatedServer()` profile (no crawling, climbing, two-handed, better swinging or roomscale shield; movement
   by controller; height mode MATCH_MODEL; no trackers).
6. A different network version: Horizon always sends 5. A Visor mod server kicks other numbers with the translatable
   `visor.messages.network_mismatch` ("... Your: %s Server: %s"), VisorPlugin with a plain English line; Horizon has the
   key in `en_us.json` and `ru_ru.json` so the disconnect screen reads, not a raw key. `visor.messages.server_vr_only`
   likewise.

Horizon as the server (LAN host, the single player's own server thread, `--dedicated`) answers the same way, with the
mod's defaults, or `server_settings.yml` beside `server.properties` on a dedicated server (written with the defaults if
missing; the same flat YAML as the mod's `server_settings.yml`).

## Module map: Visor -> Horizon -> status

| Visor module | Horizon file | Status |
|---|---|---|
| `VisorCorePayloadID` (27 ids; the plan said 28), the `toserver`/`toclient` payloads, `FriendlyByteBuf` use | `horizon-visor/src/payload.rs`, `horizon-vivecraft/src/wire.rs` (shared, `int`/`long`/`ubyte` added) | ported (literal-byte tests) |
| `PoseDataBuffer`, `PoseElementBuffer`, `PoseTrackersBuffer`, `PoseHandsBuffer` (VarInt bit masks, quaternion x y z w) | `horizon-visor/src/buffer.rs` | ported (literal-byte tests) |
| `VRServerSettings`, `ServerConfig` (YAML of the `@SendSettingToClient` fields), `joinedDedicatedServer` | `horizon-visor/src/settings.rs` | ported; the text the mod writes is parsed as flat YAML (its library was not read), the plugin's text is exact |
| `ClientNetworking` (state, change-only sends), `ClientPacketHandler` | `horizon-visor/src/client.rs`, `game/visor_net.rs` | ported (client state), ported-untested (glue) |
| `ClientPacketListenerMixin` (HANDSHAKE at login) | `game/visor_net.rs::vr_login`, `net.rs::vr_register_payload` | ported-untested |
| Protocol choice (not in Visor) | `game/visor_net.rs` (`Protocol`, `Preference`, `AUTO_WAIT_TICKS`), setting `horizonVrProtocol` | ported-untested |
| `VRClientPlayers`, `VRRemotePlayerImpl`, `RemotePlayerPose` (other VR players) | `client.rs::RemotePlayer`, `visor_net.rs::visor_rot_info`, `vivecraft_net.rs::vivecraft_bodies` (shared arms model) | ported-untested. Unknown players' packets are dropped; a player with no pose for 100 ticks is forgotten; no deferral is needed because players are keyed by UUID, not by entity |
| `VRBodyType` ids `hands_only`, `full_body` | `horizon-visor/src/body.rs`, setting `visorBodyType` | ported (ids). Full body is announced and stored; the legs/torso of a full body are **skipped** (the model draws arms only) |
| Height (`getModelHeight`, `HeightMode`, rounding to 1 cm), gun angle (`extractGunAngle`) | `body.rs`, `visor_net.rs::visor_local_state` | ported (maths tested), ported-untested (wiring). Auto height measuring (`PlayerHeightTracker`) is **skipped**: the standing height is Horizon's Calibrate Height (`manualCalibration`) |
| `ROTATION_Y` (room rotation) | `visor_net.rs` (sent as `pi - bodyYaw`, restored through `headset::set_body_yaw`) | ported-untested |
| `OFFHAND_SLOT`, `ACTIVE_HAND`, `TaskHotBar` | payloads and storage in `visor_net.rs`; the active hand switches on a swing (`two_handed_vr`) | the protocol is ported-untested; the mechanic "the off hand holds the hotbar item in slot N" is **todo** (the off hand keeps Horizon's vanilla off-hand item) |
| `ServerPacketHandler`, `ServerNetworking.sendVRStatePacketOf` (snapshots to new trackers, change-only updates, pose every tick, strip trackers/joints by the `...SendToOthers` settings) | `horizon-visor/src/server.rs`, `game/visor_server.rs` | ported (state machine, round trip in memory with the client), ported-untested (hub glue) |
| `visor$handleVrBlockDamage` (SWING_BLOCK), `BLOCK_DAMAGE` | `game/visor_server.rs::visor_swing_block`, `visor_play.rs::visor_block_damage` | ported-untested. Per-hit damage capped by `(elapsed + 2) * step * swingingMiningSpeed`, decay after `swingingRepairDelay`, cracks to everybody; the break sound, hit sound, dust and particles the Java client adds locally are done in round 7 (`swing_effects.rs`) |
| `SWING_ATTACK` | `visor_server.rs::visor_swing_attack` | ported-untested (the existing `guest_attack` does the attack, in the swinging hand; `attacksWhileBlocking` honoured). Round 5: a swing at an item, orb or arrow in reach disconnects the player (`multiplayer.disconnect.invalid_entity_attacked`) |
| `TELEPORT`, `CLIMBING`, `CRAWLING` (server side) | `visor_server.rs` | ported-untested. Teleport is, as in Java, not validated against the limits (the client's job) |
| Roomscale shield (`ServerPlayerMixin.visor$poseBlocks`), PvP rules (`pvpVRvsVR`, `pvpVRvsVanilla`, `notifyPvpBlocked`) | `visor_server.rs::visor_roomscale_shield`, `visor_pvp_blocked`; hooks in `shield.rs`, `lan_actions.rs` | ported-untested. Round 5: PvP by arrows and tridents (`lan_pvp.rs`) and the one block offset behind a projectile for the shield test, piercing arrows skipped (`creatures.rs`, `combat.rs`) are **done**; `creeperSwellDistance` is **not done on purpose** (the mod never reads it, only VisorPlugin does); the sweep particles at the hand, item drops from the hand: **todo/skipped** |
| `vrOnly` kick (`kickDelayedIfNoVR`: 5 s, ops exempt) | `visor_server.rs::flush_visor` | ported-untested (VisorPlugin waits 10 s; the mod's 5 s is used) |
| Persisted `rotationY` and offhand slot (`visor$rotation_y`, `visor$offhand_slot` in the player's data) | `server.rs` `prefs` | done (round 5): written to the world's `vr_prefs.properties` in VisorPlugin's format every five seconds when changed, read at start (`game/visor_server.rs::visor_prefs`); the mod's NBT keys are not used: Horizon's player file has no free-form compound |
| `TaskSwing` | `horizon-visor/src/tasks/swing.rs`, `game/visor_play.rs::visor_swing` | ported (logic tested: lengths, speed window, arc search, hits per swing, hit-once, climbables). The "correct tool for drops" half of `canWeaponBreak` is not known to Horizon (only "breaks instantly"). `SwingEntityVREvent`/`SwingBlockVREvent` (mod events) skipped |
| `TaskRoomCrawl` | `tasks/movement.rs::CrawlTask`, `visor_play.rs::visor_crawl_tick` | ported (hysteresis tested), ported-untested. The local player's pose is **not** forced to swimming (Horizon cannot yet); the server's is |
| `TaskTeleport` (limits, energy), `getMoveMode` | `tasks/movement.rs::move_mode/teleport_limits`, `vr.rs` (Vivecraft's `TeleportTracker` reused: same arc, energy and limits shape), setting `visorMovementMode` | ported-untested |
| `TaskRoomClimb` | Vivecraft's `ClimbTracker` reused, gated by `roomClimbingSupported`; `CLIMBING` sent | round 6: Visor's own grab (`horizon-visor/src/tasks/climb.rs`, `game/visor_play.rs::visor_climb`) replaces Vivecraft's `ClimbTracker`; `CLIMBING` sent. **done, ported-untested on a ladder** (needs hands on a ladder) |
| `TaskRoomJump`, `TaskRoomSneak`, `TaskRoomSwim`, `TaskRoomConsume`, vehicles, boat | Vivecraft's trackers, unchanged | not ported as Visor's: their thresholds (sneak 0.85, jump 1.05 of the height) differ from Vivecraft's: **todo** |
| `VRAimPicker`, `sendLookPacket` before use, `updatePlayerLook` | round 5: the player's own look stays the acting hand's aim (`head_look`; use, attack and picking read it), but what the camera faces and the rotation in the movement packets are the head's, and the hand's while an item is in use (`UseItem` carries the aim rotation) | done (bugs 1 and 2) |
| `XR_EXT_hand_tracking` joints, `XR_FB_body_tracking` trackers (`XrInputHandler` in AtumVR) | `horizon-xr/src/tracking.rs`, `horizon-client-input/src/headset.rs` (storage), `game/visor_trackers.rs`, settings `visorHandTracking`, `visorBodyTracking` | ported-untested (joint-number mapping tested). Runtime check of the extensions, one creation attempt, full-body set (legs) then default set, off unless wanted. The Android body-tracking permission has to be granted in the headset's settings: **todo** (no runtime prompt) |
| Remote hand joints and full body | received and stored | received and stored; not drawn, as in Visor (see the fingers and legs row) |
| Presets `defaults`, `defaults_hand_hud` | `horizon-visor/src/presets.rs`, VR Settings > Presets | done (round 4): Defaults applies the whole Visor profile, Hand HUD puts the HUD on the wrist; Visor's overlay layouts: **skipped** |
| Settings screens, game menu, hotbar wheel, menu room | `horizon-vivecraft/src/pages.rs` (Visor's tabs), `game/visor_menu.rs`, `game/visor_hotbar.rs`, `gfx/menu_scene.rs` | done (round 4, see the audit); the widget framework and the overlay editor are **skipped** |
| FPS tooling (F1: profile, pinning, flat draws, mask, foveation, simulated headset) | `horizon-client-render/src/{vr_profile.rs,gfx/vis_mask.rs,gfx/headset_sim.rs}`, `horizon-xr/src/{foveation.rs,platform.rs}`, `horizon-threads/src/cores.rs`, `third_party/wgpu-hal` | ported (pure logic tested), ported-untested on a Quest; see Round 3 below |
| In-block vignette (`VRCameraOverlaps`, `vr_in_block_vignette`) | `visor_play.rs::block_proximity`, `horizon-xr` vignette, setting `visorInBlockVignette` (on) | ported (proximity tested), ported-untested (drawn through the comfort vignette's image) |
| Pumpkin, freeze, low health, hit indicator, hand effects, teleport point sprite | Horizon's own overlays; the teleport arc in Visor's cyan; crosshair as in Vivecraft (same code in both) | partial (round 4): the eye effects are Horizon's flat-screen ones, switched from VR Settings > Rendering > Effects; the teleport landing mark is lines, not Visor's sprite |
| Events/API for mods (`api/**`, `VisorAddon`, `VisorChannel` addons) | - | **skipped**: a native mod hook is a versioned public ABI (fragment, stamp, mod API generator); a stub hook is not a feature. Separate task |
| Mixins, Sodium/Iris/Create compat, loaders, treadmills, VisorPlugin itself, the Minecraft render pipeline | - | **skipped** (plan section 7) |

## Where Java and the plan disagreed

- `VisorCorePayloadID` has **27** values, not 28.
- Quaternions go out **x, y, z, w** (Vivecraft: w, x, y, z).
- `SERVER_SETTINGS` keys are the Java **field names** (`twoHandedVR`, ...); the plan's `two_handed` are client option keys.
- The client sends `HANDSHAKE` to every server at login; there is no announcement to wait for (the plan's open question).
- `OTHER_VR_BODY_TYPE` uses `writeUtf` (VarInt length), `VR_BODY_TYPE` to the server uses the rest of the buffer.
- VisorPlugin does not send `supportedHeightMode`; its default config has every mechanic that needs server code off.
- The mod's server sends `OTHER_GUN_ANGLE` to new trackers twice (the snapshot has it, and the change is sent to all);
  Horizon's server copies that.

## Choosing a test server (the plugin's adapters end at 26.2)

VisorPlugin 0.2.0alpha's NMS adapters are v1_20_R1 to v26_2 (`VersionAdapterRegistry`); the tree has `nms-v26_2` and
nothing for 26.3. Horizon is Minecraft 26.3 (protocol 777), so it cannot join a 26.2 server. On a **26.3 Paper** the plugin
loads and falls back to `PaperApiAdapter` ("paper-api"): handshake, settings, pose, tracking broadcast, teleport,
climbing, crawl (`setSwimming`) work, but `betterSwinging` attack-by-id and block swings are **degraded to no-ops with
a one-time warning** ("melee VR swings fall back to vanilla"). A Horizon client that sees `betterSwinging: true` sends
only `SWING_ATTACK`/`SWING_BLOCK` and no vanilla click, so on a 26.3 plugin server **hits and mining by swing do nothing**
while it is on. The plugin's default is `false`; keep it `false` there.

Test on, in this order of usefulness:
1. **Fabric 26.3 with the Visor 0.6.1 mod** (supports 26.3; every feature is the mod's own).
2. **Horizon's own server** (LAN host and `--dedicated`), with a PC Visor 0.6.1 client (Fabric 26.3) as the second player.
3. **Paper 26.3 + VisorPlugin** with `betterSwinging: false`, then once with `true` to see the degradation.
4. A **vanilla 26.3** server and Horizon's single player.

## Performance switches

Frame budget notes: 90 Hz is held, 120 Hz is the target, the frame is CPU bound.

| Switch | Default | What it does | How to turn it on/off |
|---|---|---|---|
| Shared cull view for both eyes (`gfx/headset.rs::cull_view`) | on (existed before this task) | one frustum holds both eyes for culling | n/a |
| One packet per payload for the players who hear it (`state_updates_grouped`) | on | the server builds and encodes each `OTHER_VR_*` payload once | n/a |
| Change-only sends (`ClientNet::state_updates`, `VisorServer::state_updates`) | on | height, scale, hands, body type, overlay flag go out when they change | n/a |
| Trackers run only when wanted | on | hand and body trackers are made when the player turned them on and a Visor server takes them, dropped otherwise | settings `visorHandTracking`, `visorBodyTracking` |
| Optional extension report | on | log line `Horizon XR: optional extensions: foveation .., space warp .., visibility mask .., ...` and `wgpu multiview ..` | n/a |
| **Dynamic resolution** | **off until tried on the headset** | four late frames in 30 shrink both eye swapchains 12 % (floor 0.6), 900 quiet frames grow them 6 %, 120 frames cooldown; log `dynamic resolution` | VR Settings > Rendering > Dynamic Resolution (`horizonDynamicResolution`), or `HORIZON_XR_DYNRES=1` |
| Fixed render scale, refresh rate | existed | `renderScaleFactor`, `HORIZON_XR_SCALE`, `horizonRefreshRate` | settings / env |
| In-block vignette | on | costs a few block lookups per tick | `visorInBlockVignette` |
| Single-pass multiview | **todo, not attempted** | would halve the draw submissions. Blocked: needs the device to offer multiview (the log line says), every pipeline and WGSL shader to carry `view_index`, array eye images and `imageArrayIndex` on the projection layer; the renderer is 13K lines of two-pass code and none of it can be checked without a headset | - |
| Application SpaceWarp (`XR_FB_space_warp`) | **todo, not attempted** | 60 Hz rendering for 120 Hz display. Blocked: needs motion-vector and depth swapchains and `XrCompositionLayerSpaceWarpInfoFB` each frame; the renderer produces neither | - |
| Foveation (`XR_FB_foveation`, `..._vulkan`, fragment density map) | **off; blocked by the runtime** (round 4: the crash is fixed, the runtime gives no density map, see "Round 4: foveation") | the eyes' swapchains are made with a density map, the runtime's profile LOW/MEDIUM/HIGH is set, passes into them take the map; falls back to plain eyes with a log line when the runtime lacks one of four extensions, the GPU lacks `VK_EXT_fragment_density_map`, or the runtime gives no density map | VR Settings > Rendering > Foveation (read at start), or `HORIZON_XR_FOVEATION=low|medium|high` |
| Visibility mask (`XR_KHR_visibility_mask`) | **off, ported-untested** (F1.4) | hidden-area mesh drawn first in each eye's world pass, depth only at the nearest depth; GPU only | VR Settings > Rendering > Visibility Mask, or `HORIZON_XR_VISMASK=1` |
| Frame profile | **off** (F1.1) | the frame split into update, tick, prepare, xr wait/begin/acquire, per-eye submit and stall, hud, finish, xr end, GPU per eye pass and per section of the world pass, chunks/LOD tiles/draw calls/indices per eye pass; log every 5 s, F3 group in OVERLAY | VR Settings > Rendering > VR Frame Profile (OFF, LOG, OVERLAY), or `HORIZON_VR_PROFILE=log|overlay` |
| Big-core pinning | **off** (F1.2) | render and server threads restricted to the cores outside the chip's slowest tier (found at run time: `sched_getaffinity`, `cpuinfo_max_freq`, else `cpu_capacity`); a failure leaves the thread as is; the layout and where each thread went are in the log | VR Settings > Rendering > Pin Threads To Big Cores, or `HORIZON_PIN_BIG_CORES=1` or a list such as `3-7` |
| Flat terrain draws | **off** (F1.3) | where multi-draw is off (Adreno): one draw call per arena page instead of one per section, plus the flatten compute pass; same picture | VR Settings > Rendering > Flat Terrain Draws, or `HORIZON_FLAT_DRAWS=1` (`0` forces off) |
| Chunk upload time | **0 = unchanged** (F1.3) | ms a frame may spend uploading chunk meshes in a headset's world view (3x while terrain arrives); desktop values are 2 ms and 6 ms | VR Settings > Rendering > Chunk Upload Time (ms) |
| Image layout by barrier | **off** (F1.3) | the pass that leaves the eyes' images in the runtime's layout (a load+store of the whole image on a tiler) becomes a barrier | `HORIZON_XR_LAYOUT=barrier` |
| Performance level | sustained high (existed) | `HORIZON_XR_PERF=low|sustained_low|sustained_high|boost` | env |
| Simulated headset | **dev tool** (F1.3) | `HORIZON_SIM_HEADSET=1`: the whole headset path on a desktop Vulkan GPU, no OpenXR; `HORIZON_SIM_EYE=WxH`, `HORIZON_SIM_HZ`, `HORIZON_SIM_DUMP=DIR` (+ `_FRAME`, `_EXIT`) writes left/right/panel PNGs | env |

To measure each switch: run the same world and spot with it off, then on, and compare the `Horizon XR: waited ...`
frame statistics in `logcat` and the frame time with the OVR metrics overlay. Dynamic resolution only helps when the
GPU, not the CPU, is the limit.

## Round 2 (partial)

Done: the XR log line now ends with `app(cpu sim+draw+submit)`, the ms between the end of `xrBeginFrame` and `xrEndFrame`,
beside `wait`, `begin` and `acquire` (always on, one line per 5 s). `XR_EXT_performance_settings` at sustained-high CPU/GPU,
the `XR_FB_display_refresh_rate` request and `horizon-threads` priorities already existed. `isTool` was checked against
`TaskSwing` (the two other arrow items added, test). Per-eye submit and GPU timings, the overlay, multiview, SpaceWarp,
foveation, the visibility mask, the off-hand hotbar slot, fingers/legs, Visor's sneak/jump thresholds and height
measuring, the server todo items, the body-tracking permission prompt and the native-mod hook are **not done** in this
round (still todo as in the table above). The AtumConfig `toPlaintext()` format could not be read in this round (round 5 read it, see there). The
27-payload and `VRServerSettings` line check was done while writing the tests in round 1 (every payload has a literal
byte test or a round trip; every field of `VRServerSettings` is in `ServerSettings` with Java's default), not as a
separate pass.

## Round 3, track F1 (FPS: measure, platform, CPU, foveation, mask)

All five items are done as code; everything that needs a Quest is **ported-untested**. What ran: `cargo fmt`, `cargo clippy -D warnings`
and the tests of the touched crates (`horizon-client-render --lib` except 6 tests that already fail on the base commit 3b27e8223:
`frame_gen::interpolation::tests::moves_*` and `pack_post::...::clear_budget_tests::*`, GPU fixtures on RADV), `horizon-threads`,
`horizon-xr`, `horizon-vivecraft`, `horizon-client --lib visor`; `cargo check -p horizon-quest --target aarch64-linux-android --profile quest`
(wgpu-hal patch included); and the game itself on a desktop RX 9060 XT through the **simulated headset** (below).

**Simulated headset** (`gfx/headset_sim.rs`): `HORIZON_SIM_HEADSET=1 target/fast/horizon-client --offscreen --world T ...` (scratch homes as in
CLAUDE.md) runs `Renderer::render_headset`, both eye passes, the HUD panel and the profile on a computer, and can dump the eyes. It is how the
mask, flat draws, the barrier and the profile were looked at. F2 and F3 should use it: a multiview or SpaceWarp change can be compared eye by
eye against the two-pass picture there. It does not test OpenXR, Adreno or the Quest's CPU.

Findings (desktop GPU, so only ratios mean anything):
- **No multi-draw on Adreno** means one draw call per section and **no** occlusion culling and **no** GPU culling (both need multi-draw).
  On the test scene (12 chunks, 324 sections in view): 645 draw calls per eye per-section, 2 with flat draws; the same picture
  (0.13 % of pixels differ, animation), World pass 0.26 ms against 0.21 ms. That is the replacement for chunk batching; it is off until tried.
- The world pass is split in the log into `sky + solid chunks`, `entities`, `translucent chunks`, `clouds, weather, hand` (in-pass timestamps,
  `TIMESTAMP_QUERY_INSIDE_PASSES`, when the device has it), with the chunk, LOD-tile, draw-call and index counts per eye pass. For the
  report "GPU-bound at minimum settings with 12-16 chunks": run with `horizonVrProfile` LOG, read `gpu per eye pass`, `gpu passes` and
  `per eye pass`. Overdraw is **not** measured (no counter without extra GPU work); what LOD gives in VR is visible as `lod tiles` and its
  indices there.
- Fixed here: Horizon's own pass timer was never told a frame had been submitted when the submit thread sent it (Linux), so its figures
  (F3 `Horizon passes`) were never produced there.
- Mesh upload: 2 ms (6 ms while terrain arrives) of every frame on the desktop budget is a fifth to over half of a 90 Hz frame on the
  headset; `Chunk Upload Time` lowers it in the world view only. Not measured on a device.
- No MSAA exists in the renderer (every pass is 1 sample), so there is no resolve to tune; the world pass already discards depth
  (`StoreOp::Discard`) unless a pack, TAA, OIT or frame generation needs it. The HUD panel is a clear pass then a load pass on a 1280x720
  image (small; not merged).
- Remote pose update rate: Visor sends one pose per player per tick (the protocol); the receiving cost is decoding ~100 bytes per packet and
  the arms model per frame, not a bottleneck by construction; left alone. Entity, block-entity and particle culling already use the shared
  cull view of both eyes.
- **Not done, idea for F2:** the second eye rebuilds the same visibility walk and draw lists (the cull view is shared), which a multiview pass
  would remove altogether; reusing them across the two passes in the two-pass path touches fades, translucent sort budgets and the flatten input
  and was left for F2's structure.
- Foveation was never blocked by wgpu: `third_party/wgpu-hal` was already a `[patch]`. It now has `Device::set_density_map` (render pass
  key flag, a `VkRenderPassFragmentDensityMapCreateInfoEXT`, the density map view as the last framebuffer attachment). Known risk to read in
  the log on first try: pipelines are created for passes without the density map; strict render-pass compatibility rules could object. A
  swapchain image sampled in a later pass (post effects on the eyes, nausea, spectator chains) would read the subsampled image without a
  subsampled sampler; keep those off while foveation is on. Density map views are kept (the hal caches framebuffers with them), so dynamic
  resolution with foveation leaks a few views per resize.
- The visibility mask's space (tangent or NDC) is told from the mesh's extent; a mesh that fits neither is not drawn and nothing is logged
  beyond the vertex counts, so an unexpected runtime shows as "no effect", not as a broken picture.

Order of measurement on the headset for F1: profile LOG alone, 90 Hz; then Flat Terrain Draws; then Visibility Mask; then Foveation LOW;
then Pin Threads; then Chunk Upload Time 1.0; `HORIZON_XR_LAYOUT=barrier` last (watch for runtime or validation complaints).

## Round 4: foveation

`HORIZON_XR_FOVEATION=low` crashed at the first frame: `SIGSEGV fault addr 0x104` in `vulkan.adreno.so`
(`vkCreateFramebuffer`, from `wgpu_hal::vulkan::CommandEncoder::begin_render_pass`, on the loading screen). The patch of
`third_party/wgpu-hal` is not the cause (the render pass lists colour, depth, density map in the order the framebuffer
lists its views, the clear values cover it, nothing is duplicated). The log line the code already wrote said what was
wrong: `foveated left eye: 3 density maps of 0x0`. The runtime had written **nothing** into the
`XrSwapchainImageFoveationVulkanFB` structs chained to the swapchain images (image handle 0, size 0x0); the code made an image
view of the null image and the driver dereferenced it when the framebuffer was made.

Fixed in `horizon-xr/src/foveation.rs`: the foveation profile is put on the swapchain (`xrUpdateSwapchainFB`) **before** the
images are enumerated, every density map is checked (`DensityImage::is_valid`: a real handle and a size), and a swapchain whose
maps are not real makes foveation fall back to plain eyes with the log line `the runtime gave no density map for image 0 of 3
(0x0, handle 0x0)`. Checked on the Quest 3S (runtime Oculus 209.165): no crash, the game runs on plain eyes.

What was tried to make the runtime fill the maps, all with the same answer (0x0, null) at start: the profile applied before and
after the enumeration; the levels LOW and HIGH, with the dynamic flag DISABLED and LEVEL_ENABLED; sRGB and UNORM formats
(sRGB 43 needs `MUTABLE_FORMAT`); the usage flags colour only, colour+sampled, and the game's full set; an array swapchain of 2
layers. The flag was passed (`XrSwapchainCreateInfoFoveationFB.flags = 0x2`, FRAGMENT_DENSITY_MAP), the Vulkan device has
`VK_EXT_fragment_density_map` and its feature (a null view crashed in the framebuffer, not in device creation). The remaining
suspect, not tested: the Vulkan binding. Horizon uses `XR_KHR_vulkan_enable` (v1: wgpu-hal creates the instance and the device,
see the Quest README); with `XR_KHR_vulkan_enable2` the runtime itself creates the device and so knows the extension is on
(I could not confirm from documentation here that this is what the runtime needs). Trying it means creating the Vulkan instance and device through `xrCreateVulkanInstanceKHR` and
`xrCreateVulkanDeviceKHR` and handing them to wgpu-hal with `from_raw`-style adapters, which this tree does not do (no hook in
`horizon-xr/src/lib.rs::adapter`/`open_device`, which use the callbacks of the v1 path). Foveation stays **off** in the
settings and in `launch.txt` until that is done.

## Settings added (Horizon only, VR Settings > Visor Settings and Rendering)

`horizonVrProtocol` (AUTO, VISOR, VIVECRAFT), `visorHeightMode`, `visorBodyType`, `visorMovementMode`,
`visorSwingSpeedThreshold` (3.0), `visorHandTracking`, `visorBodyTracking`, `visorInBlockVignette`,
`horizonDynamicResolution`, `horizonVrProfile`, `horizonPinBigCores`, `horizonFlatDraws`, `horizonVrUploadMs`, `horizonVisibilityMask`, `horizonFoveation`, and the two preset buttons. They live in `vivecraft-client-config.json` with the others.

Round 4 added (Visor's names in VR Settings): `visorProfile` (which look the file was brought to; 1 is Visor's), `visorHeightAdjustment`,
`visorHeightUnits`, `visorHeightAuto`, `visorSneakThreshold` (0.85), `visorCrawlThreshold` (0.7), `visorJumpThreshold` (1.05),
`visorSwingInCreative`, `visorRoomConsume`, `visorHotbarCenterRadius` (50), `visorHotbarHysteresis` (7), `visorHotbarSlotNumbers`,
`visorHudHotbar` (off: the HUD has no hotbar row). `settings::VISOR_PROFILE` lists the values of Vivecraft's keys that a file takes
over once (`vrFreeMoveMode HMD`, `vrWorldRotationIncrement 45`, `vrHudLockMode HEAD`, `hudDistance 1.2`, `vrTouchHotbar off`,
`guiAppearOverBlock off`, `fovReduction off`, `autoOpenKeyboard OFF`, ...). `visorHeightAdjustment` (Visor's camera offset / world scale choice) is in the table but not on any page: nothing reads it yet.

## Round 5

The user's headset test of round 4 (a PC Java client running the Visor mod could join and play) found four bugs; this
round fixed them first, then worked through the audit. Everything below was built into the APK, installed on the Quest 3S
and booted into a world (`VisorTest`; 110 to 113 fps, `app` 2.4 ms, no `Horizon XR:` warning but foveation's known one);
what needs a head and hands is in the checklist at the end of this section.

### The four bugs

**1. The camera follows the right hand.** Root cause: the player's `yaw` and `pitch` follow the acting hand's aim
(`head_look`, on purpose: use, attack, picking and projectile launches read them) and everything that faces the camera read
the same two numbers instead of the headset. The eyes themselves were right (`View::eye` from each eye's pose), the particle
quads, the thrown-item and flame billboards, name tags, text displays, the sound listener, the locator compass and the debug
line cut were not. Fix: `Game::headset_facing` (`game/headset.rs:381`, the room's body yaw plus the head's angles) replaces the
view's yaw and pitch for the frame in a first person headset (`game.rs:16001`), which is also `posed_rotation`, so every user of
`camera_orientation` follows; particles take `billboard_angles` (`game.rs:11031`), name tags, text displays and flames
`display_camera_angles` (`game/text_displays.rs:220`), thrown items `posed_rotation`. Visor does the same by construction (its camera
rotation is the HMD's; only `VRAimPicker` swaps the camera entity for a pick). Roll is not applied to billboards.

**2. Other players see the model wrong.** Root causes, found by reading `PoseDataBuffer`, `PoseElementBuffer`, `RemotePlayerPose`,
`VRClientPlayers`, `VRLocalPlayerImpl.updatePlayerLook`, `ClientNetworking.sendLookPacket`:
- The bytes were right: `hmd`, `mainHand`, `offhand` in that order, each position (x y z, floats) then the quaternion x y z w;
  position relative to the feet, orientation in the world, the hands the **aim** pose (`LocalPlayerPose.update` feeds the aim, not the grip),
  `mainHand` the main hand's controller whatever its side (`leftHanded` is a separate payload). `ROTATION_Y` and the world scale
  do not change what a Visor client draws: it undoes the sender's turn (`rotateY(-rotationY)`) and re-applies the same turn
  (`VRPoseImpl.update`), so any value cancels; the world scale is applied once more on receipt (a quirk of Visor that is the
  same at the default 1.0).
- The head slot's orientation was `from_rotation_arc(-Z, player look)`: the aim hand's direction, no roll. It is the headset's own rotation
  now, `body_rotation * room head` (`game/vivecraft_net.rs:212`, for both protocols).
- The movement packets carried the aim's yaw and pitch, which a client without Visor draws the head and body from. Visor's
  `updatePlayerLook` sets the player's rotation to the HMD, to the using hand's pose while blocking or using an item, to the vehicle's while riding.
  `headset_sent_rotation` (`game/headset.rs:395`, used at `game/remote.rs:7101`) does the same; the item use packets already carry the aim rotation
  (`UseItem.yaw/pitch`), so a throw or a release leaves along the hand (Visor sends a look packet before each use for this).
- Horizon drawing a remote Visor player used Vivecraft's body yaw estimate from the head and both hands, so a player with
  one hand out turned the body; Visor's body faces `hmd.getYaw()` (`calcBodyYaw`). `RotInfo::head_yaw_body` and `math::head_yaw_rad`
  (`horizon-vivecraft/src/{player_model.rs:77,math.rs:151}`, a literal port of `extractAngles`, tested) do that for Visor players.
- Not done: Visor's arms follow the gun angle (`OTHER_GUN_ANGLE`) of the left controller; the shared Vivecraft arm model ignores it.

**3. Hands and held items show through everything.** Root cause: the first-person hand pass draws after `depth_reset`, which clears the
world's depth (right for a flat screen's hand, wrong for hands standing in the room). In a headset the arms and held items now draw
after the world without the reset (`gfx.rs:10520`, `Frame::hand_gui_quads`); the menu hands (the grey boxes) and the crosshair are
the last quads and draw after a reset, and the menu ray is on the always-on-top lines, as Visor's `renderGuiHand`, `renderCursorLine` and
`HandEffectCrosshair` use `depthFunc(GL_ALWAYS)` and `renderWorldHand` uses LEQUAL. Checked with the simulated headset
(`HORIZON_SIM_HANDS=REACH`, a new switch): a stone/gold wall 0.5 m ahead, both controllers 0.3 m ahead of the head: arms drawn in front of
it; 1.0 m ahead (inside the wall): hidden (eye dumps `wall_hands_front_left.png`, `wall_hands_inside_left.png`, kept in the session scratch, not committed). The pointer
lines also used the aim's yaw for their near plane cut; they use the head's now.

**4. Use Visor's own resources.** See the table below: the hotbar wheel, its slot numbers and marks, the movement state icons, the
warning sign and the headset icon are now loaded from Visor's own files (`horizon-client-render/assets/visor/`, unchanged, with a `NOTICE`,
`visor_art.rs`, atlas sprites `horizon/visor/*`). Visor ships no sign, keyboard art, crosshair or teleport sprite files: the VISOR sign
and the keyboard layouts are code tables in Visor and the crosshair is vanilla's sprite (now drawn as such).

| Visor asset (`src/**/resources`) | What Horizon does |
|---|---|
| `assets/visor/textures/gui/overlays/hotbar/{default,top,top_right,right,bottom_right,bottom,bottom_left,left,top_left}.png` (98 x 98) | **loaded** as shipped: the wheel's backdrops (`visor_art::hotbar_background`); before round 5 imitated with filled boxes |
| `.../hotbar/slot_numbers.png` (112 x 112), `hotbar_main_selected{0,1}.png`, `hotbar_offhand_selected{0,1}.png` (26 x 26) | **loaded**; the off-hand marks draw in two-handed play only, which is not done |
| `.../overlays/movement_state.png` (162 x 212, 8 icons of 50 x 50) | **loaded**: the icon on the off hand with the HUD on the wrist |
| `.../overlays/warning.png`, `assets/visor/icon_headset.png` (+ `.mcmeta` blur) | **bundled** in the atlas, not drawn: the warning belongs to the desktop fullscreen warning (a Quest has no desktop window), the headset icon to the menu sky's glowing icon and the inactive screen (not done) |
| `.../overlays/camera.png`, `missing_icon.png` | not used: third-person camera overlay (Third Person / Mixed Reality are not ported, no desktop mirror) and Visor's overlay editor icons |
| `.../overlays/settings/*.png` (9), `gui/settings/*.png` (5) | not used: backgrounds of Visor's own settings widgets; the VR Settings pages are Horizon screens with Visor's names and tooltips (the strings are Visor's, imported in round 4) |
| `assets/visor/icon.png` | not used (no mod list) |
| `assets/visor/lang/en_us.json`, `ru_ru.json` | **loaded in substance**: 515 / 434 strings were imported into Horizon's `assets/horizon/lang/{en_us,ru_ru}.json` in round 4 (`visor.*`) |
| `lang/fr_fr.json`, `hu_hu.json`, `uk_ua.json` | not imported: Horizon ships its own strings for en_us and ru_ru only |
| `presets/defaults_hand_hud.yml` | imitated: `presets.rs` HAND_HUD sets the HUD on the wrist; the one overlay of the file that has meaning here, `movement_state` on the off hand (offset 0.06, -0.0178, 0.2628, a quarter turn, scale 0.05), is placed with those numbers (`game/visor_overlay.rs`); the other overlays have no Horizon counterpart |
| `data/visor/tags/items/shields.json` | not needed: Horizon finds shields by the `minecraft:blocks_attacks` component (the other tags are Fabric/Forge conventions) |
| shaders `vr_teleport_point.*` | the formula is ported, drawn with lines (animated dashes, two square ripples, `game/vr.rs:1460`); not a shader quad |
| `vr_pumpkin_overlay.*` | equivalent: the pumpkin texture times an opacity; Horizon's flat-screen pumpkin overlay does that |
| `vr_in_block_vignette.*` | formula not ported: Horizon's comfort vignette image stands in (its proximity is the same, `visorInBlockVignette`) |
| `vr_end_portal.*`, `vr_mixed_reality.*`, `vr_post_process_eye.*` | not ported: Horizon's own end portal; mixed reality is a desktop compositing mode; the per-eye post pass belongs to Visor's pipeline |
| `pack.mcmeta`, `*.mixins.json`, `visor.accesswidener`, `fabric.mod.json`, `data` loader files | n/a (mod loading) |

### The audit, worked through

| Item | Status |
|---|---|
| Hotbar wheel hung in front of the hand | **done, device** (above) |
| A second overlay layer | **done** for what needs it here: `headset::set_overlay` / `Headset::overlay_image`, `Frame::overlay`, an XR quad layer on its own swapchain (`horizon-xr/src/lib.rs`), `game/visor_overlay.rs` (wheel, movement state). Not done on it: **chat** (it is Visor's second overlay at (0.392, 0.214, -1.706); chat is in the HUD panel's batch and moving it needs the chat layer split out of `hud()`), the fullscreen warning (skipped, no desktop window), the settings/keyboard/screen overlays (separate Horizon panels already) |
| Crosshair sprite | **done** |
| Teleport sprite and animated beam | **done** as lines; not Visor's shader quad |
| Server: persisted rotationY and offhand slot | **done** (`vr_prefs.properties`) |
| Server: PvP by arrows, projectile offset for the shield, hit item/orb/arrow kick | **done** |
| Server: `creeperSwellDistance` | **not done on purpose**: only VisorPlugin implements it (`VisorServer.tickPlayer` swells creepers near the head); the Visor mod server sends the setting and never reads it |
| Server: the sweep particles at the hand, item drops from the hand | **skipped**: cosmetic, Horizon's sweep and drops are the player's |
| AtumConfig `toPlaintext` | **done**: `Config.toPlaintext` is SnakeYAML's block dump (indent 2, `LinkedHashMap` order, comments as `# ` lines, `@SendSettingToClient` fields for `SERVER_SETTINGS`). `to_client_text` was already in that order; `to_file_text` is now the whole file as `ServerConfig.applySettingsTo` + `save()` writes it, with the comments, and a test pins the text. The parser reads that text (comments skipped) |
| A line-by-line check of all 27 payloads | **done**: every id (0 `HANDSHAKE` ... 26 `OTHER_VR_OVERLAY_FOCUSED`, the byte of `byteOrdinal()`), every `onWrite`/`read` of `toclient`/`toserver` compared with `horizon-visor/src/payload.rs`: the field order and widths (`writeInt`, `writeFloat`, `writeUUID`, `writeBlockPos`, `writeByte(get3DDataValue)`, `writeUtf` for `OTHER_VR_BODY_TYPE`, the rest of the buffer as UTF-8 for `SERVER_SETTINGS`, `VR_BODY_TYPE`, the visor version of `HANDSHAKE`, nothing for `CLIMBING`, `HANDSHAKE` to the client) all match; the literal-byte tests pin the ids 1, 2, 13, 16 to 21, 25 |
| Visor's own climbing details | **not done**: `TaskRoomClimb` (about 450 lines: per-hand anchoring on the face of a ladder or vine, retreat by axis pairs, the ledge push-out at 0.82 of the block's top, the `NoGravity` toggle) needs the player's collision test at arbitrary positions and cannot be tried without hands on a ladder; Vivecraft's `ClimbTracker` (same grab idea) stays |
| Two-handed play / the off-hand hotbar slot | **not done**: see the round 4 row; the protocol, the stored slot and the wheel mark are there |
| Menu room details (shooting stars, UFO, box clouds, the headset icon) | **shooting stars done** (`gfx/menu_scene.{rs,wgsl}`: the exact `hash01` and `emitShootingStar` maths on the CPU, the streak drawn per pixel as the quad along its great circle; time-gated, so not seen on a screenshot). **UFO, box clouds and the headset icon not done**: the UFO is a dotted sprite of `UFO_ROWS` flying a parked path with blinking light groups, the clouds are boxes, the icon needs a texture in the menu shader (`icon_headset.png` is bundled); each is a visual feature that only an eye can judge, left for a round with the headset on |
| Dragging and resizing screens | **not done**: the screen is a Horizon panel placed once; Visor's handles belong to its overlay framework |
| Keyboard layouts and key art | **not done**: Visor has no key art files, only `KeyboardLayouts.java` tables; Horizon's keyboard has its own layout |
| Per-item-type item poses against `VRItemPoseDefault` | **not done**: a line-by-line comparison needs an item per type held in a hand and looked at; the arm and base transform are the shared constants |
| Fingers and legs of remote players | **not done**: Horizon's player model has no finger bones or leg targets to drive; the joints are received and stored |
| Auto-height, foveation, multiview, SpaceWarp | postponed (performance), unchanged |

### What ran

`cargo fmt`; `cargo clippy -D warnings` on `horizon-vivecraft`, `horizon-visor`, `horizon-client-input`, `horizon-client-render`,
`horizon-client`, `horizon-xr` (clean; `--all-targets` fails on a clippy lint in a `horizon-vivecraft` test that was already there:
`trackers/teleport.rs`); tests: `horizon-visor` (68), `horizon-vivecraft` (138), `horizon-xr`, `horizon-client-input`, `horizon-client --lib` (890 passed, 14 ignored),
`horizon-client-render --lib` (only the 6 GPU fixtures that already failed on the base commit fail: `frame_gen::interpolation::tests::moves_*`,
`pack_post::...::clear_budget_tests::*`; the 5 `menu_scene` and 2 `visor_art` tests pass); `cargo check -p horizon-quest --target aarch64-linux-android --profile quest`;
`packaging/quest/build_apk.sh` with `DEBUGGABLE=1`, `adb install -r`, launch into `VisorTest` with `HORIZON_XR_FAKE_HANDS=file:`
(a pose line is needed before `gr=1`: `0.22,-0.3,-0.35,-28.6,0,0 gr=1`): the grip opened the wheel and the log said `the overlay
panel is in use`. The simulated headset (`HORIZON_SIM_HEADSET=1`, new `HORIZON_SIM_HANDS`, `HORIZON_SIM_GRIP`, `overlay.png`) showed the wheel
with Visor's art and the hand/wall occlusion.

### Round 5 checklist: headset on

1. **Camera (bug 1).** Set off a firework (or break a block: particles) and tilt and roll your right hand a lot: the sparks and
   crumbs keep facing your eyes and do not turn with the hand. Name tags above players and mobs read straight at you. Sounds
   stay where they are as you turn your head (not your hand).
2. **Others see you (bug 2).** With a PC player (Visor or plain) watching: your head turns where your head turns, not where your right
   hand points; your body faces your head; both arms follow their controllers (try a T-pose: both hands out to the sides). A player
   without Visor sees your head look where you look and, while you draw a bow or hold a shield up, along the hand.
   A Visor player seeing you needs no other setting.
3. **Hands and walls (bug 3).** Push a hand with a tool into a wall: it is hidden by the wall (not drawn through it). In a menu the
   grey boxes and the ray still show over everything, and the crosshair (vanilla sprite) shows over the surface the hand points at.
4. **Visor's art (bug 4).** Hold the right grip: the wheel is Visor's picture (rounded boxes, the arrows on the side you point to), hung
   about 0.6 m in front of the hand where it was when the grip went down; it stays there while the hand moves. With the HUD on the wrist
   (Defaults: Hand HUD) the icon of your movement (run, sneak, ...) sits on the off hand.
5. **Teleport.** Dashes slide along the arc towards where you land; at the landing a square ripple spreads and fades twice a few
   seconds.
6. **Server.** Join a Horizon server with a Visor player: PvP by arrows follows `pvpVRvsVR`; leaving and rejoining keeps your rotation.

## Round 6

Round 5 left eight items not done and the user's headset test added three more; all are worked through here against the
Java. Status words as in round 4. Everything was built into the APK, installed on the Quest 3S and booted into `VisorTest`
(120 fps, cpu 7.9 ms, no `Horizon XR:` warning or error) and into the title (`HORIZON_MENU_HOUR=0`).

| Item | State | Where | How checked |
|---|---|---|---|
| 1 Two-handed play | done | `game/visor_hotbar.rs:77` (`visor_update_two_handed`), `:103` (`visor_active_hand_click`), `horizon-visor/src/tasks/hotbar.rs`, `game/shield.rs` | unit tests of the hotbar task; device with fake hands: the left grip opened the off wheel, the off hand showed the shield |
| 2 Chat as its own overlay | done | `game/visor_overlay.rs:106` (`chat_overlay`), `horizon-visor/src/overlay.rs` (`CHAT_OFFSET`, `CHAT_SCALE`) | simulated headset overlay dump; device log `the overlay panel 1 is in use` |
| 3 Dragging and resizing screens | done, device by log only | `horizon-xr/src/screen_grab.rs:33`, `horizon-visor/src/overlay.rs:47` (`handle_at`), `game/visor_overlay.rs:196` | unit tests of the handle maths; the log lines `dragging/resizing the screen by its handle` were added for the headset check. A real drag needs the hands: in the checklist |
| 4 Item poses per type | done | `horizon-visor/src/item_pose.rs:268` (`compute`), `:441` (`compute_tool`), `game/headset.rs:791` (`visor_item_matrix`) | unit tests of every table entry; not looked at on a real hand |
| 5 Visor's climbing | done (pure parts tested) | `horizon-visor/src/tasks/climb.rs:175` (`ClimbTask`), `game/visor_play.rs:231` (`visor_climb`) | unit tests; not tried on a ladder (needs hands) |
| 6 Menu room: UFO, box clouds, icon | UFO and icon done and seen; clouds approximate | `gfx/menu_scene.rs:184` (`ufo_at`), `:101` (`icon_grid`), `gfx/menu_scene.wgsl:151` (`ufo`), `:217` (`clouds`), `:102` (`visor_sign`) | unit tests (UFO timing and parking direction, icon sampling, shader validates); device: the teal UFO dome seen at midnight (`HORIZON_MENU_HOUR=0`, `HORIZON_MENU_LOOK=-90,8`) over the moss floor with stars. The clouds are **not** Visor's cell/shape model (see the audit row) |
| 7 Fingers and legs of remote players | nothing to draw: Visor draws none | `horizon-visor/src/body_rig.rs` (joint maths only) | read `VRBodyRendererFull`, `VRPlayerModelFull`, `RemoteHandsPose`, `VRPlayerRendererFull`: the model is the vanilla one with `ArmPoseClamp` arms and the headset's head; the joints go to `GameEffectTrackerDebug` only. Unit tests of the maths |
| 8 Gun angle on remote players | done, as Visor does it | `mobs/render.rs:2688` (`visor_item_pose`), `horizon-vivecraft/src/player_model.rs` (`RotInfo.gun_angle`), `game/visor_net.rs` | Visor has **no arm tilt from the gun angle**: `OTHER_GUN_ANGLE` is only the `aimToGrip` turn of the held item's pose (`VRHandItemPose.getAimToGripRotation`), with the roll about the aim undone (`ItemInHandLayerMixin.visor$applyHandPose`). Unit test of the angle turning the sword; not seen on a second player |

Bugs the user found in the headset (each its own commit):

**A. Crouching and lying down put the eyes under the floor.** Root cause: the camera stands for the head's rest position
and the eyes are the real head's offset from it, but the camera's height also followed the pose's eye (sneak 1.27, crawl
0.4), so the head dropped twice (a crawl: 1.22 m for the pose and about a metre for the head). Visor's camera is the real
head; only a forced pose moves the room (`LocalPlayerMixin.visor$tickRoomYOffset`: swimming, gliding, the riptide spin, and
a crawl without headroom put the eye 0.4 m above the feet). Fix: `game/headset.rs::headset_eye_height` (sneak or a crawl
with headroom stand for the standing eye, a forced pose is `0.4 - head height`), fed to `Camera.tick`
(`game/fov.rs`) and to the aim. Tests: `headset::tests` (a crouch of 0.4 m is 1.22 m above the feet, a crawl 0.32 m, never
below the floor; a forced pose is 0.4 m whatever the head does). `HORIZON_SIM_HEAD_Y=-0.4` holds the simulated head low;
an eye dump with the head held low showed the ground (the pose was not proven to be a sneak in that run: the unit tests are the proof). The room y offset is eased by the camera's halving per tick instead of
Visor's 0.06 m per tick.

**B. The cursor jumped while flying.** Root cause: the aim started at the tick's position plus the hand's offset of this
frame, and the block under it was picked once per tick (`tick_hit`), while the hands and eyes are drawn at the position
interpolated between ticks: at 28 m/s the ray start was up to 1.4 m from the drawn hand, worse at 120 Hz where there are
more frames per tick. Fix: for the frame's pointer, crosshair and block outline (`game.rs`, `frame_eye`) the ray starts at the
camera's own interpolated eye and is picked afresh from the hand as drawn, as Visor's `GameRenderer.pick` runs every frame;
the tick's pick still serves gameplay. Test: `headset::tests::the_pointer_stays_fixed_to_the_hand_while_the_player_flies_between_ticks`.

**C. Typing.** Visor's keyboard (`VROverlayKeyboard`, `VRKeyboardScreen`, `KeyboardLayouts`) now is the headset's keyboard:
`horizon-visor/src/keyboard.rs` (layouts, buttons, hit test, shift and repeat), `game/visor_keyboard.rs` (state, rays,
drawing, routing into the focused field through `pad_osk_output`), slot `OVERLAY_KEYBOARD` of the second quad layer
(`horizon-xr/src/lib.rs`, `gfx.rs`), hung 0.5 m below and 0.6 m ahead of the head, facing it, and fixed in the room. Settings
`visorKeyboard` (on; off brings Horizon's keyboard back), `visorKeyboardAutoLayout`, `visorKeyboardLayouts`. Tests: layouts,
button rectangles, hit testing, a ray at a key of the hung panel, shift, repeat. The simulated headset
(`HORIZON_SIM_HANDS`, `HORIZON_SIM_HAND_PITCH`, `HORIZON_SIM_TRIGGER`) typed `qqqq` into chat and dumped
`overlay_keyboard.png`. Device with fake hands (`0,-0.3,-0.35,-45,0,0 tl=1 tr=1`): the rays showed on the `h` key, both
triggers typed `h`, Enter sent `<Player> hh`, and the keyboard went away with the chat.

### Round 6 checklist: headset on

1. **Crouch and crawl (A).** Crouch for real, then lie on the floor: the camera follows your head and never sees under the
   ground; swim in water or glide with an elytra: the eyes sit about 0.4 m above the body, whatever the head does.
2. **Cursor while flying (B).** Fly fast (creative, sprint) pointing at the ground: the crosshair and the outline stay on
   where the hand points, no jumping at 90 or 120 Hz.
3. **Keyboard (C).** Open chat (it opens by itself) and click a sign, a book, the anvil's name field, the creative search
   and the server address: the keyboard hangs under the screen, each hand's ray lights a key, a trigger types it, Shift stays
   on until pressed again, the language button changes to your language's layout (Russian: ЙЦУКЕН), Backspace held repeats after a
   second, Enter sends. A click on a key does not close the screen. The red cross closes it until the next screen.
4. **Two hands.** Grip the left hand: the off wheel; the item of its slot is in the left hand and acts with the left trigger.
5. **Chat.** Chat lines hang in the room ahead and to the right, not on the HUD.
6. **Screens.** Grab the handle under a screen with the trigger and drag; grip both hands on the square to resize.
7. **Items.** Hold a sword, a pickaxe, a bow, a trident and a shield: each sits in the hand as in Visor, the gun angle of your
   controller turning it.
8. **Ladder.** Grab a ladder or a vine with the hands and pull yourself up; let go at the top.
9. **Menu room.** At midnight (or `HORIZON_MENU_HOUR=0`) a teal UFO parks low over the horizon for 15 of every 35 seconds,
   blinking in and out; the headset icon is over the VISOR sign; the clouds are boxes below you.
10. **Other players.** A Visor player sees your items turned by your controller's grip; neither sees fingers or legs (Visor draws none).

## Round 7: headset bug fixes

The user's headset test of round 6 found four more bugs. Each is its own commit; the root causes were found by reading the
Java (`TaskRoomSneak`, `TaskRoomCrawl`, `TaskSwing`, `VRItemPoseDefault`, `XrRawPoseHandler`, `VRHandRenderer`), not by
guessing from the symptom.

| # | Symptom | Root cause | Fix | Checked |
|---|---|---|---|---|
| 1 | Hitting by swinging the hands makes no sound | `TaskSwing` plays the block's hit sound itself (a `betterSwinging` server sends the swinger none), throws `3 * hits` crumbs at the hit, watches the block and plays its break when the server breaks it (`confirmPendingBreaks`), and attacks the entity it found (`gameMode.attack`) or runs `Player.attack` locally. Horizon sent the packets and stopped; without `betterSwinging` it clicked along the aim ray, which misses what the weapon reached, silently | `horizon-visor/src/swing_effects.rs` (the decisions: `better`, `entity_feedback`, `block_feedback`, `hit_sound`, `PendingBreaks`), `game/visor_play.rs` (`visor_apply_swings`, `visor_block_feedback`, `visor_confirm_breaks`), `game/remote.rs` (`break_sound_and_particles`) | unit tests for single player's server thread, a server with and one without `betterSwinging`; the sounds themselves are the server's for entities, as in Java |
| 2 | Crouching and crawling broken, a 175 cm player always sneaking | The room sneak pressed the stick's click, which is a *toggle* (controller `toggleSneak`, on by default): the dip below 0.85 of the height flipped sneaking on and standing up released nothing, so one dip (a lean, sitting down) sneaked for good. Also the height estimator ran per frame where Visor runs it per tick (first estimate after 0.6 s of rest instead of 3 s, a decay five times faster) | `horizon-visor/src/tasks/movement.rs` (`SneakTask`, Visor's `TaskRoomSneak`: pivot over full height, never while crawling), `game/vr.rs` (`visor_room_sneak`: a held key through its own source `Source::Room`, never a click), `game.rs` (`sync_toggle` lets go of the keyboard's and the pad's hold only: `heal_hold_latches` runs it every loop and wiped the room's hold, found on the headset), `horizon-visor/src/height.rs` (`sample_at`: one sample per 50 ms; none while sneaking or crawling, so the pose cannot teach the height) | unit tests: a 1.75 m player standing, crouching, crawling and standing again through the estimator, `auto_update` and both tasks; the key sources; sim and device with `hy=` in the fake hands' file |
| 3 | The launch room has no cursor and no laser | The room's eyes are drawn by `RoomScene` and its panel by the flat frame; the menu ray and the menu hands live in the world pass, which the room never runs | `gfx/room_scene.rs` (`pointer` pipeline, `fs_flat`), `game/quest_room.rs` (`pointer_mesh`: the ray of the hand that clicked last, 3.2 mm, Visor's grey 228, from just ahead of the controller to where it meets the TV; both controllers as the menus' grey boxes), `quest_room.rs` (the cursor dot on the page) | unit tests of the geometry and the dot; `tools/quest_room_sim.py` (ray, boxes, dot on the Play button); device |
| 4 | Pickaxes and swords lie along the arm (and a bow hung in the air with no arm under it) | `VRItemPoseDefault` authors poses on a Quest 3S's aim frame minus the default gun angle (`yaw -= 60`) and re-anchors them by the controller's *own* aim to grip turn (`XrRawPoseHandler.updateAimToGrip`, the default `X(60)` when the runtime gives the aim as the grip). The first person hand passed the identity: every item 60 degrees short | `horizon-visor/src/item_pose.rs` (`aim_to_grip`), `game/headset.rs` (`HandFrame.aim_to_grip`), `game/visor_net.rs` (the gun angle sent comes from the same turn) | unit tests (identity versus the turn: the sword is 60 degrees off without it); sim eye dumps of a pickaxe, sword, trident (forward and up out of the fist), apple, block, bow, shield; device |

What else was compared and found right: the table of poses (`compute`, `compute_tool`) against `computeParams` entry by entry,
the chain before the pose (`hand_base`: 0.9375, controller to model, arms scale about the grip, shift to `0, 0.125, 0.625`) against
`renderWorldArmWithItem`, the third person hand display transform, and the remote player's pose (`X(gunAngle)`, as
`getAimToGripRotation` does for another player).

Seen on the headset (a test package with its own folder, fake hands, the headset on a desk, `hy=` moving the head): the room's pointer
draws without a failed frame and the ray meets the TV (`Horizon room: the ray meets the TV at (0.58, 0.50), 2.4 m`); a head at 1.7 m
(pivot 1.60) stands, 1.3 m sneaks once and stays (`the room sneaks`), 1.0 m crawls and the sneak lets go, back up it stops crawling,
sneaks and stands up, one log line per change. Not seen on the headset: the swing sounds and particles (fake hands cannot swing at
3 m/s: sim and unit tests only) and the items (sim eye dumps only; the simulated controllers have no aim to grip turn of their own,
so they take Visor's default 60 degrees).

Developer switches added: `hy=1.2` in the fake hands' file holds the head 1.2 m above the floor (a crouch on a headset that
lies on a desk); `HORIZON_SIM_POINTER=aim` and `HORIZON_SIM_HAND_YAW=20` for the simulated headset's room.

### Round 7 checklist: headset on

1. **Swing sounds (1).** Swing a sword at a mob: the hit sound plays (and the attack/crit sound of the server). Swing a pickaxe at
   stone: a block hit sound and crumbs at the hit on every swing, the crack grows, the controller pulses; when the block breaks its
   break sound and particles play. Try single player, a server with `betterSwinging` and one without.
2. **Crouch and crawl (2).** Stand upright: no sneaking, ever (also after sitting down and standing up). Crouch to about 85 % of
   your measured height: sneaking; stand up: it lets go. Crouch below about 70 %: crawling. The camera never goes under the floor.
3. **Launch room (3).** In the room a light grey ray leaves the hand that clicked last and ends on the TV; a white dot sits where
   it meets the page (accent while the trigger is down); both controllers show as grey boxes; either trigger takes the cursor.
4. **Items (4).** A pickaxe, a sword and a trident point forward and up out of the fist, a bow stands in the off hand, a shield
   covers, food sits at the fist, a block rests on the palm.

## Round 4 checklist: headset on (what should feel like Visor)

None of this could be felt in round 4 (the headset lay on a table); each line says what to look at and what Visor does.

1. **Title.** You stand on a small moss platform (about the size of your play area) above a sea of clouds, in a sky that follows
   the clock; at night stars, the moon in its real phase; look up and ahead for the dotted VISOR sign. The menu is a 2 m panel at
   chest height 1.5 m in front of the middle of the play area, with no picture behind its buttons. (`HORIZON_MENU_HOUR=14`
   and `HORIZON_MENU_LOOK=0,26` in `launch.txt` show it by day, looking at the sign, for a screenshot.)
2. **First world.** The chat says "Stand up straight for a moment so Visor can measure your height."; after a few quiet seconds
   "Height set to N cm. You can adjust it in VR Settings -> VR Body". If your head is tall or ducked at that moment, use
   Y > Calibrate Height (3 s countdown) standing straight.
3. **HUD.** The vanilla HUD hangs 1.2 m ahead, a little below the line of sight, and follows the head. There is no row of
   hotbar items in it. Hearts, hunger and the XP bar are in it.
4. **Hotbar wheel.** Hold the right grip: a 3 x 3 wheel appears in the middle of the HUD (slot 0 is the centre, 1 at the top and
   clockwise), the first slot is chosen at once, then move the hand: past about 11 degrees from where it was when the grip went
   down a side slot is chosen (a buzz on each change); the chosen item's name shows under the wheel; let go to close it.
   Slot numbers and the radius are in VR Settings > GUI > Hotbar.
5. **Y.** The game menu opens in front of you (tabs Main, Commands, Tools). Calibrate Height, Chat, VR Settings and the
   pause menu are on Main. Y again, or a click beside the menu, closes it.
6. **Buttons.** A uses with the right hand, X with the left; the right trigger attacks; B jumps; the right stick click sneaks;
   the left stick walks where you look and sprints when pushed all the way; the right stick turns 45 degrees at a flick. The left
   trigger does nothing in play (as in Visor on a server without the two-handed mode).
7. **Menus.** A light grey thin ray comes from the hand that last clicked (either trigger takes the cursor); A or X is the right
   click. Screens open 1 m wide, 1 m in front of your face (chat a little higher). Chat and signs open a keyboard on the panel.
8. **Teleport.** VR Settings > Movement > Movement: Teleport. Push the left stick forward: a cyan arc (dimmer in survival, dark
   where you cannot land); let go to go. (Needs a server that allows it; Horizon's own single player does.)
9. **Body.** Duck so that your head is below 0.85 of your height: you sneak; below 0.7: you crawl (the server must allow it;
   the model lies down); rise quickly past 1.05 of your height: you jump. The thresholds are in VR Settings > Immersion >
   Advanced.
10. **VR Settings.** Tabs in Visor's order (Presets, Rendering, VR Body, Controls, Movement, Immersion, GUI, World). Hover an
    option: Visor's tooltip. Presets > Defaults brings back Visor's values; Defaults: Hand HUD puts the HUD on the wrist.
11. **Your old settings.** The first start with this build moved the Vivecraft-era values to Visor's (45 degree turns, HUD on
    the head, walking by the head, no locomotion vignette, ...). The old file is `home/vivecraft-client-config.json.pre-visor`.
12. **Frame time.** `Horizon XR: frame profile` in the log (profile LOG): compare with 92 fps, `wait` 6.3 ms, `app` 2.8 ms.
13. **Permissions.** Turn VR Settings > VR Body > FBT (or Hand Tracking) on: on a fresh install the system asks for the
    permission in the headset; the tracker is tried again 6 s later.

## What the tester checks (in this order)

Log lines are Horizon's `latest.log` (`logcat` on the headset) unless a server is named. Set `serverDebug: true` in the
server's `server_settings.yml` to see the server's join line.

1. **Boot.** The Vivecraft checklist's boot items still hold. New in the log: `Horizon XR: optional extensions: ...`,
   `Horizon XR: wgpu multiview ...`, `Horizon XR: hand tracking .., body tracking .. (legs: ..)`. Note which are true.
2. **Settings.** Options > Horizon Settings > VR Settings opens Visor's tabs (see the round 4 checklist), every value changes and
   survives a restart, no raw lang keys; Presets > Defaults and Defaults: Hand HUD change the HUD, the turn step, the walking
   direction as listed; About / Credits shows the Visor lines.
3. **Vanilla 26.3 server, protocol Auto.** Log: `VR protocol: Visor HANDSHAKE sent (network 5, preference Auto)`, about two
   seconds later `VR protocol: no Visor answer in 40 ticks, speaking Vivecraft`. No kick, no chat message. Teleporting is off
   (stick moves), swinging a sword at a mob and a pickaxe at stone works with ordinary clicks (Visor's swing task, so
   speed and arc differ from Vivecraft's).
4. **Fabric 26.3 + Visor 0.6.1 server (the main test).** Log: `VR protocol: the server speaks Visor (network 5)`, then
   `Visor: server settings ServerSettings { ... }`. Chat: "Server supports Visor!". Server (`serverDebug`): `player '<name>'
   joined with 0.6.1 (VR)`. If nothing answers: the channel must be in `minecraft:register`; capture the configuration
   phase and the first play packets and look for `visor:channel`.
5. **Server settings are obeyed.** Change `server_settings.yml` and restart the server: with `betterSwinging: true` a swing at a
   mob damages it with one `SWING_ATTACK` and no click; with `false` it clicks. `supportedMovement: CONTROLLER` leaves
   no teleport (off trigger walks), `TELEPORT` or `BOTH` gives the arc; the arc refuses what `teleportUp/Down/ForwardLimit`
   forbids; `roomCrawlingSupported`/`roomClimbingSupported` gate crawling (crouch low, the server shows you swimming) and
   ladders; `attacksWhileBlocking: false` with a raised shield: swings do nothing.
6. **Block swings.** Swing a pickaxe at stone: the crack builds, 3 to 7 hits per swing by speed, the block breaks after
   a few swings; another player sees the cracks (`BLOCK_DAMAGE`).
7. **Rotation and slot memory.** Turn 90 degrees, leave and rejoin the same server: the room faces the same way (`ROTATION_Y`).
   (Horizon's own server forgets on restart.)
8. **Second Visor player (PC client).** Each sees the other's head and hands; the model scales with their height/world
   scale; left-handedness and crouching show. Compare with a second Horizon headset if you have one.
9. **Hand and body tracking.** In the headset enable hand tracking, then Visor Settings > Hand Tracking: log
   `hand tracking true`; the PC player's Visor shows fingers only if the server has `handTrackersSupported`/
   `handTrackersSendToOthers`. Body Tracking: log `body tracker made (full body joints set)` or `no body tracker (...
   permission may be missing)`: grant the permission in the headset's app settings and retry.
10. **Horizon as the server.** LAN host with the PC Visor client joining: PC log shows Visor handshake, Horizon's chat shows
    the player; PvP and shield rules from the settings file on a dedicated server.
11. **Paper 26.3 + VisorPlugin.** Handshake, settings and tracking work; with `betterSwinging: true` swings do nothing and
    the plugin logs the degradation warning (documented above).
12. **Version mismatch (optional).** With a PC Visor of another network version on the same server: the disconnect screen
    reads "Your Visor network version is not supported by this server" (the mod) or the plugin's line.
13. **Performance.** Same spot, off then on: Dynamic Resolution; note frame times at 90 Hz; set `horizonRefreshRate` to 120 and
    note whether it holds with 1, 4 and 8 other VR players (the server sends one pose packet each per tick).

## Build and test commands run for this task

- `cargo test -p horizon-visor -p horizon-vivecraft -p horizon-xr`, `cargo test -p horizon-client --lib visor`,
  `cargo test -p horizon-client-render --lib options_ui`, `cargo test -p horizon-client-lib --lib a_headset_registers`.
- `cargo clippy -D warnings` and `cargo fmt` on the touched crates.
- `cargo check -p horizon-quest --target aarch64-linux-android --profile quest` (after `. packaging/quest/env.sh`).
- Round 4: the same for `horizon-xr`, `horizon-client-input`, `horizon-client-render`, `horizon-client --lib` (888 passed);
  `horizon-client-render --lib` fails only the 6 GPU fixtures that failed before (`frame_gen::interpolation::tests::moves_*`,
  `pack_post::...::clear_budget_tests::*`). `packaging/quest/build_apk.sh` and `adb install -r` for the device checks; the
  control socket (`--control-socket` in `launch.txt`, a `DEBUGGABLE=1` build) drove the settings screens, and
  `HORIZON_XR_FAKE_HANDS=file:...` (buttons `y=1`, `gr=1`) pressed Y and the grip for the game menu and the wheel.

Round 6: `cargo fmt`; `cargo clippy -p horizon-visor -p horizon-xr -p horizon-client-render -p horizon-client -p horizon-client-input -p horizon-vivecraft -- -D warnings` clean; tests `horizon-visor` (100), `horizon-vivecraft` (138), `horizon-xr`, `horizon-client-input`, `horizon-client --lib` (893 passed, 15 ignored), `horizon-client-render --lib menu_scene` (7); `cargo check -p horizon-quest --target aarch64-linux-android --profile quest`; `packaging/quest/build_apk.sh` with `DEBUGGABLE=1`, `adb install -r`, boots into `VisorTest` and into the title with no new warning. Not run: the whole `horizon-client-render --lib` after round 6 (its 6 GPU fixtures failed before round 5 too).
