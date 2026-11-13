# Vivecraft port: plan, status and device checklist

Target: Vivecraft 1.3.15 (commit `8b4d6b95a1891d9839f19b45c13e93415c02a31e`, Minecraft 26.2) in Horizon on Meta Quest
(OpenXR). Branch `vivecraft-port`, from `quest-openxr` (left untouched).

Licensing (decided by the maintainer; it overrides the LGPL ban in CONTRIBUTING.md for this fork): Vivecraft is
LGPL-3.0-or-later. Derived code lives in `crates/horizon-vivecraft` and `crates/horizon-xr` (both
`license = "LGPL-3.0-or-later"` with `LICENSE-LGPL-3.0`), and in files of MIT crates that carry an SPDX
`LGPL-3.0-or-later` header naming the Vivecraft sources they port (`horizon-client/src/game/{vr,vivecraft_net,
vivecraft_server,headset,headset_bow,headset_radial,headset_hands,headset_trackers}.rs`,
`horizon-client-render/src/vr_options.rs`). `horizon-quest` holds no derived code and stays MIT; hooks in
`horizon-client-input` (`headset.rs`) stay MIT. See `THIRD_PARTY_NOTICES.md`.

Status words: **ported** (compiles, unit tested against the Java byte layouts or math), **ported-untested**
(compiles, needs a device or a Vivecraft server), **todo**, **skipped** (reason).


## Round 4: keyboards, the radial menu, vehicles, server gaps

No device again. Checked: `cargo check`/clippy `-D warnings` for horizon-vivecraft, horizon-xr, horizon-client-render and
horizon-client; unit tests of horizon-vivecraft (134), the options UI (68), and the touched `game::` tests of horizon-client.

| Vivecraft | Horizon | Status |
|---|---|---|
| `RowTracker` + `AbstractBoatMixin` | `trackers/scope.rs::steer_boat`, `vr.rs::vr_row`/`vr_boat_steer`, `boats.rs` (`tick_boat`'s control block) | ported-untested. Standing mode only: forward held steers the boat toward the off controller (or head/waist per free move), with tank controls when `vehicleRotation` (default on); otherwise the oars (controller speed over 0.5 m/s, max 2) turn and drive it. The oar force path (`forces`, `doProcessFinaltransmithastofixthis`) is commented out in Vivecraft too and not ported |
| `VehicleTracker` | `trackers/vehicle.rs`, `vr.rs::vr_vehicle`/`vr_mount_look`, `remote_vehicles.rs::drive_mount` | ported-untested. The body turns (`headset::snap_turn`) to follow a mob or minecart you ride in steps of 10 degrees (more in a fast cart); boats are left to Horizon's own `positionRider` turn (it already carries the rider). A horse or happy ghast the client moves is steered by the main controller (`getSteeringDirection`) while forward is held. Roomscale dismount: the tracker has the gate and cooldown but nothing walks away from a vehicle (Horizon has no vehicle room walk), so Vivecraft's dismount-by-walking is not wired |
| `HorseTracker` | not ported | Vivecraft's `isActive` starts with `if (true) return false`: it is dead code upstream |
| `CrawlTracker`, `CameraTracker` | not wired | crawl needs a swimming pose on the player (not forcible yet); the handheld camera has no rendering |
| `PhysicalKeyboard`, `KeyboardKeys`, `KeyboardTheme`, `KeyboardHandler.orientOverlay` | `horizon-vivecraft/src/keyboard.rs`, `game/vr_keyboard.rs` | ported-untested. 13 x 4 key grid plus the special keys (shift x2, space, tab, esc, backspace, enter, arrows, paste, switch layout), all of Vivecraft's layouts (en_us, en_gb, de_de, fr_fr, fr_be, be_by, uk_ua, ru_ru; `custom` and `system` are en_us), the themes DEFAULT/RED/GREEN/BLUE/BLACK/GRASS/BEES/AESTHETIC/DOSE, 40 mm keys, key delay 150 ms, repeat after 500 ms every 100 ms, a pressed key sinks 12 mm and the hand stays on it, shift is one shot and sticks on a second tap within 400 ms, the grips hold a sticky shift. Keys are translucent quads (alpha 0.5, half colour until pressed) with labels from the game font, drawn in the entity pass beside name tags; hands become grey boxes while it shows |
| `physicalKeyboard` default | now `true` (Vivecraft's) | the keyboard opens when a text field takes focus (`autoOpenKeyboard`); **Horizon's default is ON** (Vivecraft's OFF opens it only when a field is clicked or by the toggle binding; Horizon cannot tell a click from other focus). `autoCloseKeyboard` closes it with the focus. Sign and book editors always open it. Seated mode uses Horizon's pad keyboard |
| Pointer keyboard (`GuiKeyboard`) | not ported; Horizon's own on-screen keyboard (pad) stays for `physicalKeyboard` off and seated | Vivecraft's differs (a 13-column grid on a second panel); a second panel needs a second XR swapchain in the platform layer, which this round did not touch |
| Keyboard cut/copy, layout selection screen, layout and theme editors, `keyboardPressBinds`, the CUSTOM theme file, the toggle binding (long Y) | not ported | |
| Menu room | not ported | `renderJrbuddasAwesomeMainMenuRoomNew` only shows when `menuWorldFallbackPanorama` is off; the default (on) is the panorama, which works. The menu world (`.mmw` snapshots/generation) is not ported. Both need geometry in the title pass, which the XR title path does not take |
| `GuiRadialConfiguration`, `GuiRadialItemsList`, `GuiRadial` (item sets) | `horizon-vivecraft/src/radial.rs`, `options_ui.rs` (`Page::VrRadial`), `game/headset_radial.rs` | ported-untested. The ring is built from `vrRadialItems` (main set) or `vrRadialItemsAlt` (while a grip is held, Vivecraft's shift) with `vrRadialButtons` slots; any slot holding a binding Horizon knows shows (width grows with the label). A slot runs: rotate left/right, quick torch, `vivecraft.key.quickcommandN`, or the key/mouse button the game's binding is on (`key.*`). Handheld camera and player list do nothing. Config page: Options > Horizon Settings > VR Settings > Radial Menu...: a button per slot that cycles through the actions (no selection screen), the main/alternate set, the button count, reset, a link to the radial settings |
| `GuiQuickCommandEditor` | `Page::VrQuickCommands` | ported-untested. Click a command, type it in the box (the settings-search box is reused), leave the page to save; `/` commands run, others are said in chat. The commands are run from a radial slot `Quick Command N` |
| Controller binding remap screen (`GuiVRControls`) | not ported | Vivecraft's lists SteamVR actions; on OpenXR Horizon's own controller binds page is the screen |
| `AimFixHandler` scope | `lan.rs` (`vr_aim_packet`), `guest_vr_aim` | only UseItem, UseItemOn and PlayerAction packets use the hand's aim now |
| Roomscale shield | `vr_roomscale_shield`, `vr_shield_cooldown`, `shield.rs`, `creatures.rs` | projectiles use the hand-distance path (hand to a point 1.8 m back on the projectile's line to the player's centre, since the hit does not carry the arrow's velocity), others the flat path; shields whose `bypassed_by` holds the damage type are skipped; `roomscaleShieldCooldown` and `...DamageTrigger` are in `ServerConfig` (defaults 0 and 3.0) and applied on a block |
| Headshot indicator | `vr_flush_headshots` (`vivecraft_server.rs`) | crit particles at the arrow, flying back, and the item-break sound at 0.7/0.5, to the shooting guest only (`bowHeadshotIndicator`, default BOTH). Needs the arrow to be ticked in the shooter's turn like the multiplier |
| Item fade (`getItemFade`) | `SwingTracker::item_fade` is ported and tested | not drawn: the hand pass has no translucency |
| Third-person item transforms, `SPLIT_ARMS` models | not ported | |
| XR menu pointer at world scale != 1 | `horizon-xr`: the block-anchored screen position divides the block's offset by the scale | ported-untested |
| `doPlayerMoveInRoom` | `player.rs::room_walk_to`, `vr.rs::vr_room_walk`, setting `horizonVivecraftRoomWalk` | ported-untested, **off by default**; VR Settings > Roomscale > "Vivecraft Room Walk". It replaces only the stepping rule of Horizon's room walk (the position source, the head's offset from the rest point, is Horizon's): no sliding along walls (the whole step must be free), walk up blocks (`walkUpBlocks`, up to 1.1 m), nothing while sneaking/asleep/jumping/on a ladder/dead/riding. Horizon's `room_step` (slide) is the default |
| `doPermanentLookOverride` | ported, not driven | Vivecraft keeps the entity's yaw apart from the camera; Horizon's `yaw` is the camera, so overriding it would turn the view. The parts that matter are done separately (mount steering, shield aim, hand-aimed use) |

Settings added: `vrRadialItems`, `vrRadialItemsAlt`, `vrQuickCommands`, `keyboardLayoutOrder`, `currentKeyboardLayout` (arrays are
newline separated lines of the string), `horizonVivecraftRoomWalk` (Horizon only). A config file saved by an earlier build
keeps `physicalKeyboard: false` and `autoOpenKeyboard: OFF`: delete `vivecraft-client-config.json` or flip them in Keyboard Settings.

### What the tester checks (round 4, in this order)

1. A text field (chat, a sign, a world name) opens the keyboard below and in front of the head, tilted up; with a screen open both
   hands are grey boxes. Poking a key with the controller tip types it after a short delay, holding repeats, Shift (or a grip
   held) shifts one letter, two taps stick it, Backspace, Enter, arrows, Esc and Paste work; Switch changes layout only with
   `keyboardLayoutOrder` listing more than one (edit the JSON: `"custom\nen_us\nru_ru"`). Keyboard Settings: scale and theme
   change it. If keys are not hit, the tip is the controller's aim pose origin: report how far off it is.
2. Radial menu (B): the default eight actions appear as before; Options > ... > Radial Menu...: change a slot, close, open the ring
   (holding a grip shows the alternate set); a slot set to `Drop Selected Item`, `Inventory`, `Quick Command 1` works.
3. Quick Commands...: select one, type, back out; the radial slot sends it.
4. Boat: forward on the stick points the boat where the off controller points (tank controls if Vehicle Rotation is on);
   without forward, paddling both controllers down in turns drives it, one controller turns it.
5. Mounted mob/minecart: the view turns with the vehicle slowly; a saddled horse follows the controller while forward is held.
6. Server (Paper plugin or a LAN host): arrows follow the controller aim; a shot at a head gives crit particles and a sound to the
   shooter; a shield held up against an arrow blocks it; with `roomscaleShieldCooldown` set the shield cools down.
7. Roomscale > Vivecraft Room Walk on: walking into a wall stops you instead of sliding; walking toward a block up to 1.1 m high
   steps up (Walk Up Blocks on). Off: as before.
8. A screen opened over a chest at world scale 2.0 and 0.5 appears over the chest.

### Consolidated device checklist (all rounds, by priority)

1. Boot, OpenXR session in the STAGE space (`no stage space` in the log means the fallback ran), 90 Hz, render distance 8,
   multi-draw off on Adreno, no crash joining a world; head height right.
2. Options > Horizon Settings > VR Settings: every page opens, values change, the JSON is written, Reset Origin and Calibrate
   Height work, no raw lang keys; Radial Menu... and Quick Commands... open.
3. Turning (snap 45 by default), screens 1 m ahead, chat higher, HUD on the wrist, hotbar touch, radial ring with B.
4. Hands and items (round 3 list): arms from the controllers, tools along the ray, bow in the off hand, map held up, shield on the
   forearm, grey boxes with a screen/ring/keyboard open.
5. Typing with the keyboard (round 4 list) in chat, on a sign and in the world-name box.
6. Trackers: crouch sneaks, a quick rise jumps, backpack, swing at mobs and blocks, bow nock/draw/release, eating, spyglass, swim,
   climb ladders and vines, run in place, teleport (arc colours, onto water; on a vanilla server the off trigger walks forward).
7. Vehicles: boat rowing and steering, riding a horse/pig/minecart.
8. A Vivecraft server (Paper plugin/Fabric): handshake in the log, second VR player's model, server aim fix, headshots, shields,
   Kick, haptics on hit.
9. World scale 2.0 / 0.5: room, IPD, hands, walking, screens.
10. `horizonVivecraftRoomWalk` on versus off.

## Round 3: what the user sees, and the rest of the gameplay

No device again: every "ported-untested" row needs the headset. Crates checked after this round: horizon-vivecraft,
horizon-xr, horizon-client-input, horizon-client (clippy `-D warnings` clean; unit tests of the first three and
of the touched parts of horizon-client pass).

### 1. Hands and held items (`headset_hands.rs`, `headset.rs`, `game.rs::hand_geometry`)

What differed from Vivecraft, found by reading `ItemInHandRendererVRMixin`, `VRArmHelper`, `ArmsRenderState` and
`VivecraftItemRendering` against the Rust:

| Difference found | Fix |
|---|---|
| Maps used an invented transform (`HELD_SCALE`, `HELD_TILT`) and the vanilla two-armed hold | `Held::Map` (`MAP`: rotate -45 about X, translate (0, 0.16, -0.075), scale 0.75) then vanilla's `renderMap`; one VR arm under it. The old constants and `headset_item_frame` are deleted |
| Swing was always the "attack" tip; Vivecraft has three (`VRFirstPersonArmSwing`) | `SwingType::{Attack, Use, Interact}` (`headset.swing_type`), set by an attack, a use, and a touch of a block |
| No eating jitter, shield cooldown, trident charge/jitter/riptide spin, bamboo as a tool | added to `item_transform` (`HeldUse`) with tests |
| `showPlayerHands` and `shouldRenderSelf` + `modelArmsMode COMPLETE` were ignored | `headset_arm_shown` |
| With a screen, the ring, the keyboard or a touched hotbar Vivecraft draws a grey box instead of the hand (`renderMenuHand`, `menuHandMain/Off`) | `headset_menu_hand` (a 4 x 2.5 x 18 cm box, 64/255 grey scaled by the head light) |
| Hands did not scale with the world | the controller frame is scaled by the world scale (`setupRenderingAtController`) |

The constants of `applyFirstPersonItemTransforms` for tool, rod, block, item, food, compass, stick, horn, shield,
crossbow, mace, lance, spyglass, bow (roomscale) already matched. **Not ported:** the arm's and held item's
translucency (`SwingTracker.getItemFade`: the hand fades with the attack cooldown, sneaking and a solid hit), the
drawing-bow transform's jitter, the spear (lance) animation (`SpearAnimations`), the telescope lens view, the
physical Climbey claws model (`ViveItems`), third-person item transforms (`thirdPersonItems`), mirror/camera passes.
Gun angle: Vivecraft takes it from the right controller's tip/grip components and uses it for both hands; here each
hand uses its own aim/grip angle. Hand placement uses the runtime's aim pose (Vivecraft's "tip" from its
`ControllerTransform` tables), the log line "right controller's aim ... from its grip" shows the angle (about 40 on
Touch controllers; if it is near 0 the items will hold too flat).

### 2. VR player model of other VR players (`player_model.rs`, `mobs/anim.rs::vr_player_model`)

`VRPlayerModel.animateVRModel` for the default `playerModelType` VANILLA with connected arms (default
`playerLimbsConnected`), plus `ModelUtils` (world/model space, `pointAtModel`, swing animations, bend progress,
swim offsets). The head follows the sent head pose, the body bends with the head height, the arms are rotated and
stretched to the hands (`playerLimbsLimit`, `playerArmAnim` honoured), the legs sit at the hips and move back with
the bend, passengers sit, swimmers lie. The entity is turned by the estimated body yaw (`RotInfo.getBodyYawRad`).
Poses come from `ClientNet::players`, the last two ticks interpolated (`vivecraft_bodies`). `renderVrPlayerAxes`
still draws the axes. **Not ported:** `SPLIT_ARMS`/`SPLIT_ARMS_LEGS` meshes, FBT (a player sending FBT is shown as
arms-only), the HMD model layer, the local player's own body ("shadow"/`shouldRenderSelf`), `positionGUI`.

### 3. World scale and movement

World scale now reaches: the eyes' offsets (IPD) and the head's standing height (`horizon-xr::world_offset`), the
hands (render frames, ray origins, the pose sent to the server), the room walk (the XR layer divides the game's
walked distance by the scale), the `VrData` the trackers use and `WorldScale` to the server. The room walk gets
`walkMultiplier`. The stick's movement direction is `VRPlayer.freemoveDirection` (off controller by default; head,
room, waist, run in place) through `vr_free_move_stick`; this replaces "the stick walks where the head looks".
**Kept deliberately:** Horizon's own room walk (`headset_room_walk`) and the head-following yaw/pitch of the game:
`do_player_move_in_room` and `look_override` are ported and tested but the XR camera is built on the rest-position
model and on the head-following yaw, so swapping them blind would break the working platform layer. The server still
learns the hand ray through the aim packets and `vr_aim` (below).

### 4. Climbing and the trackers

`ClimbTracker` (ladders, vines, climbable tag, open trapdoors over ladders, auto grab, getting off on top, the jump
off, Climbey claws with the grips as the grab binding) and the Climbey boots jump (A on the main hand as the jump
binding): `trackers/climb.rs`, `vr.rs::vr_climb`; the player gets a `no_gravity` flag (`PlayerMotion`), packets
`Climbing`/`Jumping` go out. Claws and boots are shears and leather boots named "Climb Claws"/"Jump Boots" when the
server allows Climbey. Run in place and walk multiplier are wired. **Not wired:** `CrawlTracker` (the swimming pose
cannot be forced on the player yet), `CameraTracker`, `HorseTracker` (dead code upstream). `RowTracker` and `VehicleTracker` since round 4.

### 5. Server side (`vivecraft_server.rs`, `lan_actions.rs`)

| Vivecraft | Horizon |
|---|---|
| `AimFixHandler`, `ProjectileMixin` aim | for a VR guest `enter_guest` sets `vr_aim` (aim pos and dir from its sent hand) and turns the guest's look to it for that turn; `aim()` and `eye_position` read it. Applies to every action of the guest's turn, not only UseItem/UseItemOn/PlayerAction packets |
| `ProjectileMixin` draw | arrows of a drawing VR guest fly at `power * 3 * draw` |
| `AbstractArrowMixin` | bow multipliers (`ServerConfig.bow_*`) and headshots (`head_kind`, `head_hitbox`, `is_headshot`; mobs only); the headshot particle/sound indicator is **not** sent |
| `ServerPlayerMixin` roomscale shield | `vr_roomscale_shield` inside `player_blocking`: a shield held up within 120 degrees blocks. Projectile hits use the horizontal path, not Vivecraft's hand-distance path; the shield cooldown config is not read |
| Dual wielding attribute swap | `apply_equipment` swaps main and off hand modifiers while the active body part is the off hand |
| Server Kick | the hub kicks with the reason |
| Haptics, damage direction | no bHaptics vest: a lost health buzzes both controllers, harder on the hit's side when the server sent `DamageDirection` |

### 6, 7. Keyboard, menu room

**Not ported:** the physical 3D keyboard (`PhysicalKeyboard`, `KeyboardKeys`, themes: textured keys and text in the
world, poked by the hands) and the menu room/world. `physicalKeyboard` stays off by default (Vivecraft: on) and the
pointer keyboard remains. The title's panorama around the eyes is Vivecraft's own fallback
(`menuWorldFallbackPanorama`, default true); its dirt room (`renderJrbuddasAwesomeMainMenuRoomNew`) and menu worlds
(`.mmw` files downloaded by Vivecraft) are not drawn.

### 8. Polish

Teleport onto water and lava: `GameTeleport::fluid_surface` adds the fluid's top to the arc's ray (`Fluid.ANY`).
Radial menu configuration and quick commands: no editors (defaults only).

### What the tester checks (round 3, in this order)

1. Hands: empty hand = an arm in the skin from the controller back toward the shoulder (both hands), a sword/
   pickaxe along the ray, bread/potion at the mouth (shakes while eating), a block in the palm, a torch upright, a
   shield on the forearm (raised when blocking), a bow in the off controller with the arrow hand at the string, a
   map held up in front of the hand with one arm under it, a trident along the ray (pulls back while charging).
   Swing: attack tips the hand, a use pushes it ahead, touching a lever/button turns it. Options > Player Model >
   Show Player Hands off hides the arms but not the items. With a screen/ring/keyboard open both hands are grey boxes.
2. Second VR player (needs the Vivecraft server mod/plugin): the model's head turns with theirs, the body faces
   where head and hands say, the arms reach their hands, crouching bends the body, riding sits. Compare with
   `renderVrPlayerAxes`.
3. World scale 2.0 and 0.5 (VR Settings > Main > World Scale): the room looks half/double size, IPD changes with it,
   hands and walking scale; check the server learns it (Paper plugin debug).
4. Free move: the stick walks toward the off controller (default), the head in HMD mode; Run in place needs the
   stick pushed while the arms swing.
5. Climb: put a hand in a ladder or vine and pull down; let go and fall; get off on top; Climbey claws only on a
   server that allows Climbey.
6. Server: a bow shot at a zombie's head does 3x (standing), arrows follow the controller aim; a shield held up in
   front blocks; Kick action disconnects.
7. Teleport arc lands on water.

## Old text, round 2 and 1 (kept for reference)

## Round 1 (kept for reference)

## Done in this session, by module

| Vivecraft | Horizon | Status |
|---|---|---|
| `common/network/CommonNetworkHelper`, `NetworkVersion`, `Pose`, `TrackerPose`, `VrPlayerState`, `PayloadIdentifier` | `horizon-vivecraft/src/network/` | ported (unit tests: byte layouts, ordinals, FBT part, version strings) |
| `common/network/packet/c2s/*` (all 18 payloads + `readPacket`) | `network/c2s.rs` (`C2s`) | ported |
| `common/network/packet/s2c/*` (all payloads + `readPacket`) | `network/s2c.rs` (`S2c`) | ported |
| `api/data/VRBodyPart`, `FBTMode` | `network/body.rs` | ported |
| FriendlyByteBuf calls (VarInt, UTF, UUID, floats) | `wire.rs` | ported |
| `client/network/ClientNetworking` (state, handlePacket, sendVRPlayerPositions, active hand, aim overrides, version info) | `horizon-vivecraft/src/client.rs` (`ClientNet`) | ported (unit tests) |
| `ClientPacketListenerVRMixin` (reset + VERSION on login/respawn), `minecraft:register` of `vivecraft:data` | `horizon-client/src/game/vivecraft_net.rs`, `net.rs` (`configure`), `remote.rs` hooks | ported-untested |
| Local pose to `VrPlayerState` (head, main/off hand, relative to feet) | `vivecraft_net.rs::vivecraft_tick` | ported-untested |
| `server/ServerNetworking.handlePacket`, `ServerVivePlayer`, `ServerVRPlayers`, `ServerConfig` defaults, `ServerUtil.kickIfNotAllowed` (VR/Vive only) | `horizon-vivecraft/src/server.rs` (`VrServer`) | ported (unit tests) |
| Server glue: handshake replies, pose collection, `sendVrPlayerStateToClients`, leave cleanup | `game/vivecraft_server.rs`, `lan_actions.rs` (guest payload), `native_net.rs::flush_native_net` | ported-untested. Single player, LAN and dedicated all go through the hub, so all three behave as a Vivecraft server |
| `common/utils/MathUtils` (angles, deadzone, toDigital, body yaw estimate) | `horizon-vivecraft/src/math.rs` | ported (unit tests) |
| `client_vr/settings/VRSettings` (`@SettingField` options: config keys, defaults, slider ranges, enums, JSON config) | `horizon-vivecraft/src/settings.rs` + generated `settings_table.rs` | ported, **not yet used by the game** (see below) |
| `vivecraft.messages.*` lang strings (serverplugin, calibrateheight, novrhotswitching) | `horizon-client/assets/horizon/lang/{en_us,ru_ru}.json` | ported |
| About / Credits screen (Horizon, Vivecraft, LGPL note, OpenXR loader); URLs shown as text (Horizon opens no links) | `horizon-client-render/src/options_ui.rs` (`Page::VrCredits`, button on the VR page) | ported-untested |
| Vivecraft defaults in the old settings: snap turn 45 (`vrWorldRotationIncrement`), no vignette (FOV reduction off) | `horizon-client-input/src/headset/settings.rs` | ported |


## Round 2: the VR client

Everything below compiles (`cargo check` and clippy clean for horizon-vivecraft, horizon-xr, horizon-client-input,
horizon-client-render, horizon-client, horizon-quest) and the pure logic is unit tested. **Nothing was run on a
device** (no adb this round): every row marked "ported-untested" needs the headset. The platform layer (OpenXR
session, `android_main` and its ndk_context swap, the lazy headset install, multi-draw off on Adreno, 90 Hz, eye
cap, render distance 8) was left as it was, except where noted under "Decisions".

### Ported, by module

| Vivecraft | Horizon | Status | The tester checks |
|---|---|---|---|
| `VRSettings` table, `vivecraft-client-config.json` | `horizon-vivecraft/{settings,options,pages,modes}.rs`; `Settings::vr` is an `Arc<VrSettings>`; `horizon-vr.txt` is migrated once if the JSON is absent | ported (unit tested) | VR Settings opens from Options > Horizon Settings; the file appears in the game home after a change; an old `horizon-vr.txt` carries over |
| Options screens (`GuiMainVRSettings`, `GuiHUDSettings`, `GuiOtherHUDSettings`, `GuiRenderOpticsSettings`, `GuiPostEffectsSettings`, `GuiStandingSettings`, `GuiRoomscaleSettings`, `GuiWeaponCollisionSettings`, `GuiBackpackSwitchingSettings`, `GuiClimbingSettings`, `GuiFreeMoveSettings`, `GuiTeleportSettings`, `GuiSeatedOptions`, `GuiPlayerModelSettings`, `GuiKeyboardSettings`, `GuiRadialSettings`, `GuiMenuWorldSettings`) | `horizon-vivecraft/src/pages.rs` + `horizon-client-render/src/vr_options.rs` + `options_ui.rs` (`Page::Vr(PageId)`) | ported-untested | page names, button order and option names read as Vivecraft's; sliders (world scale, rotation increment are index sliders) show the right values; Reset Origin and Calibrate Height buttons work |
| Vivecraft lang (`vivecraft.options.*`, `.gui.*`, `.key.*`, `.keyboard.*`, `.actionset.*`, `.toasts.*`, `.messages.*`) | `en_us.json`, `ru_ru.json` (Russian where Vivecraft has it, English fallback) | ported | no raw keys on the pages |
| `VRData`, `Vector3fHistory` | `vr_data.rs` | ported (tests) | `renderDeviceAxes` shows the head and controllers where they are |
| `VRPlayer` (room origin, scale, rotation, room walk, free move, look override, teleport gates), turning of `MCVR.processBindings` | `player.rs`; turning is used by `horizon-xr`; `freemove_direction` and the world scale drive the game since round 3; `do_player_move_in_room`, `look_override` and `pre_render` are ported and tested but not driven (the game keeps its own room walk and head-following yaw, see round 3) | turning, free move, scale: ported-untested; rest: ported, unused | snap turn (45 by default) turns once per flick; smooth turn (increment 0) is 10 degrees a tick at full tilt |
| `SneakTracker`, `JumpTracker` (not the Climbey boots), `BackpackTracker` | `trackers/{body,hands}.rs`, `game/vr.rs`; the old `Body` and `Backpack` in horizon-xr are deleted | ported-untested | crouch 40 cm below the calibrated height (1.52 m, or Calibrate Height) sneaks; a quick rise jumps; a hand over the shoulder presses the binding |
| `CrawlTracker` | `trackers/body.rs` | ported (tests), **not wired** (needs the server's crawl permission and the pose) | |
| `RunTracker` | `trackers/moves.rs`, `vr.rs::vr_tick`, `vr_free_move_stick` | ported-untested (round 3: wired to the free move direction) | free move mode Run in place: swing both arms while pushing the stick; the speed follows the arms |
| `RowTracker` | `trackers/scope.rs` | ported (tests), **not wired** (needs a boat tick that reads oar forces) | |
| `SwimTracker` | `trackers/moves.rs` + `game/vr.rs::vr_swim` (tick-gated) | ported-untested | strokes with both hands in water push you |
| `EatingTracker` | `trackers/hands.rs` + `vr_eating` | ported-untested | food or a potion at the mouth is used; a drink tipped upright is not |
| `TelescopeTracker` | `trackers/scope.rs` + `vr_eating` | ported-untested (the lens view is not rendered; vanilla's spyglass zoom is) | a spyglass to the eye is used |
| `BowTracker` | `trackers/hands.rs`, `game/headset_bow.rs` | ported-untested | nock at the bow hand's string, draw by hand distance, release shoots; haptics on nock and draw |
| `SwingTracker` (hands) | `trackers/swing.rs`, `vr_swing` | ported-untested | a swing at 2.5 m/s attacks what the weapon reaches and mines the block the tip crosses (faster = more hits); sword/axe/pick reach as in Vivecraft |
| `TeleportTracker` (Arc style), `VRMovementStyle` Arc | `trackers/teleport.rs`, `vr_teleport_*`, `vr_tick` | ported-untested | off trigger held draws an arc (green = valid), release teleports; limits from the server/settings; with a Vivecraft server the teleport packet is sent, on a vanilla server teleporting is off (Vivecraft's `noTeleportClient`) and the off trigger walks forward |
| `InteractiveHotbarModule` | `gui.rs::touched_hotbar_slot`, `horizon-xr` (sets `hud_slot`), `headset_hud_slots` | ported-untested | with "Touch hotbar" on, the main hand at the HUD's hotbar picks a slot |
| `GuiHandler` placement (screens, chat, containers over blocks, HUD head/hand/wrist, cursor mapping) | `gui.rs`, `horizon-xr/src/{panel,lib}.rs` | ported-untested | screens open 1 m ahead at eye height (1.5 m wide), chat higher, containers above their block, HUD on the wrist (default), hand or head; the pointer hits what it points at |
| `RadialHandler`, `GuiRadial` | `gui.rs::place_radial`, `headset_radial.rs` | ported-untested (handheld camera and player list items do nothing) | B held opens the ring at the hand, letting go picks; a quick tap repeats the last pick |
| Crosshair (`VREffectsHelper.shouldRenderCrosshair`, `extractCrosshairState`) | `crosshair.rs`, `headset_pointer_lines` | ported-untested (drawn as two lines, not the sprite) | crosshair lies on the hit face, scales with distance if set |
| Touch default bindings (`oculus_defaults.json`) | `horizon-xr/src/lib.rs` | ported-untested | right trigger attack, A use, B radial, X inventory, Y pause, left stick move + click jump, right stick turn + click sneak, left trigger teleport, grips previous/next hotbar slot |
| `ServerOverrides` and `SETTING_OVERRIDE` | `overrides.rs`, `vivecraft_net.rs` | ported (tests), applied to the headset's settings | a server's teleport limits show on the Teleport page |
| Server actions (`Teleport`, `FoodExhaustion`, `ResetFall`, `CrawlPose`, `Message`) | `vivecraft_server.rs` | ported-untested | a guest's teleport/climb exhaustion work on the hub |
| Debug axes (`renderDeviceAxes`, `renderVrPlayerAxes`) | `vr_axes_lines` | ported | other VR players' head and hands as axes where their poses say |
| Item and arm transforms (`VivecraftItemRendering`, `VRArmRenderer`) | `headset_hands.rs` (earlier round, checked against the Java: the constants match) | ported-untested | tools/food/blocks/shield/bow/trident sit in the hand as in Vivecraft |

### Not ported (and why)

- (Round 2 list; round 3 ported climbing, the Climbey boots jump, world scale, run in place and the walk
  multiplier, see above.) Still not ported: horse and vehicle trackers (`HorseTracker`, `VehicleTracker`),
  `CameraTracker`, `InteractTracker`'s entity module (the block touch of the earlier round stays), swing: feet,
  piercing weapons, door hitting, hoes/scythes/brushes. **Roomscale crawl, rowing, sneak-to-dismount, vehicle
  rotation** options exist but nothing reads them yet.
- **Virtual keyboard**: Vivecraft's physical keyboard (`PhysicalKeyboard`, `KeyboardKeys`, themes) needs textured 3D
  quads and text in the world, which the gizmo lines cannot draw. Horizon's pointer keyboard (Vivecraft's
  `physicalKeyboard = false` mode) stays; `physicalKeyboard` defaults to false here.
- **Menu room / menu world**: not started (no world rendering behind the title).
- **Remote VR bodies**: drawn since round 3 (see above); `renderVrPlayerAxes` still draws the axes.
- **Radial menu configuration, quick commands, controller binding editor, keyboard layout editor, mixed reality,
  mirror, external camera, FBT, server settings screen**: no editors; the data stays at the defaults.
- OpenVR/SteamVR, mixed reality, desktop mirror, the non-VR mode: out of scope.

### Decisions

- The XR layer now asks for the **STAGE** reference space (floor-level; falls back to LOCAL). Vivecraft's
  height-based trackers (sneak 0.4 m below 1.52 m, jump) need absolute heights; the earlier "standing is the
  highest the head has been" estimate is gone. **Calibrate Height** (Player Model page) sets `manualCalibration`.
- `renderScaleFactor` defaults to 0.4 here (Vivecraft: 1.0) and `horizonRefreshRate` (default 90) is a
  Horizon-only key in the same JSON, to keep the perf defaults; Vivecraft ignores unknown keys.
- The world data (`VrData`) is built every frame from the headset's room poses with the room rotated as the body
  (`pi - body_yaw`) and the origin put so the head rest position is under the player's eyes (what
  `snapRoomOriginToPlayerEntity` does), so the ported trackers work in true world coordinates while the game's
  camera/room-walk mechanism is unchanged.
- Trackers that Vivecraft runs per tick (teleport, swim) are tick-gated; those it runs per frame (bow) run per
  frame. Sneak/jump/backpack drive the pad through `headset::set_tracker_pad`.
- The off trigger is Vivecraft's teleport button; the old "off trigger uses the item" is gone (A uses).
- A server `Kick` action kicks since round 3. Teleport onto water works since round 3 (fluid-aware arc).

### Device test checklist (priority order)

1. Boot, OpenXR session in the STAGE space (log: `no stage space` means the fallback ran), 90 Hz, render
   distance 8, no crash on joining a world. Head height reads right in the world (STAGE changed the origin).
2. Options > Horizon Settings > VR Settings: every page opens, values change, the JSON is written, Reset Origin
   and Calibrate Height work, labels are Vivecraft's.
3. Turning: snap 45 per flick (HUD and screens unaffected), smooth when the increment is set to Smooth.
4. HUD on the wrist (default), hand and head; screens 1 m ahead; chat higher; chest screens over the block.
5. Radial menu: hold B, point, release.
6. Crouch to sneak, quick rise to jump (check thresholds with Calibrate Height), backpack swap.
7. Swing a sword at a mob and a pickaxe at stone; bow (nock, draw, release); eat; spyglass at the eye; swim.
8. Teleport on a Vivecraft server (Paper plugin or Fabric mod): arc colours, limits, the position the server
   keeps; on a vanilla server the off trigger must walk forward.
9. Second VR player: `renderVrPlayerAxes` on, the axes follow their head and hands.


Decisions: Horizon announces itself as a Fabric build (`Vivecraft-26.2-fabric-1.3.15`); servers read only the string
(`UpdateChecker.Version.fromClient`). The client sends VERSION only when a headset is active (Vivecraft sends `NONVR`
on a flat screen; Horizon stays silent there). `minecraft:register` is sent in the configuration phase, as Fabric API
does, because Paper only sends plugin messages on channels the client registered.

## What to check on a device / against a real server (in this order)

1. Build still starts, OpenXR session, ndk_context swap, 90 Hz, render distance 8 (nothing in the platform layer
   was touched).
2. Join a **Paper/Spigot server with the Vivecraft plugin**: in the server log, "player joined with ..." (enable
   `general.debug` in the plugin config). In chat: "Vivecraft server mod detected: ..." and the calibrate hint. If
   nothing appears, capture `minecraft:register` and `vivecraft:data` with a proxy: the channel must be registered
   in the configuration phase (`net.rs::configure`) and the first play packet after Login is VERSION
   (`remote.rs` `C::Login` -> `vivecraft_reset`).
3. With a second VR player: do they appear with VR hands/head? (Not rendered yet: check
   `ClientNet::players` via logs only.) Pose accuracy: positions are relative to the **feet**, orientation in world
   space; head orientation is built from `game.yaw/pitch` (no roll).
4. Single player: the window is a guest of the integrated server; confirm "Vivecraft server mod detected" does NOT
   show twice and that no `Vivecraft: not applied yet` spam appears in the log when teleporting/climbing.
5. A **Vivecraft Fabric server** (not the Spigot plugin): it needs `vivecraft:data` registered both ways; same flow.
6. Credits: Options > VR Settings > About / Credits shows five lines and three URLs without clipping (long LGPL
   lines may overflow at narrow GUI scales: shorten the lang strings if so).
7. Snap turn default is now 45 degrees; vignette default is off.
