# Starling modded

`starling-main-modded` starts at `starling-main` commit
`934b71707943f8294e4422359cfc1bd957e973a8`. This is the fork owner's native VR
mod branch, separate from upstream Horizon. Source and assets may be used under
their respective licenses; preserve copyright and license notices.

Prefer mods whose blocks, items, textures and effects fit vanilla Minecraft and
whose interactions work with tracked hands. Keep text readable in stereo,
preserve world scale and depth, and avoid forced camera movement. Record each
user-supplied mod here with its upstream revision and validation evidence.

## VisorEssentials

Source: https://github.com/VisorModStudio/VisorEssentials
Pinned revision: `88ffec507449eb1a97ccbf495cd331a46c6d635b`.
License: LGPL-3.0-only; Phoenix-Ra and VisorEssentials contributors.

The addon is implemented natively. It does not load Java JARs, and does not
provide a general Java Visor addon API. Its original 22 inventory/container PNGs
are included unchanged and loaded into the GUI atlas.

| Upstream feature | Native implementation |
| --- | --- |
| `VRInvScreen`, `VROverlayInventory` | Offhand inventory, original nine-slot hotbar layout, armor/offhand slots, 2x2 crafting/result, player preview, effects, recipe book below the inventory, creative-inventory button |
| `VROverlayContainer` and container mixins | World-space container at its block/entity, following boats/carts/mobs, close beyond four room metres, original shortened backgrounds and slot positions; player inventory stays on the wrist |
| `VROverlayDraggedItem` | Carried item follows the cursor hand outside either panel; left drops the stack and right drops one through the authoritative menu |
| Container interaction | Shared menu and server slot IDs, pickup, split, double-click, quick craft, recipe placement, station controls/scrollers and keyboard input; UI consumes attack/use while focused |
| Overlay placement | Position/rotation anchors (room, head, main hand, offhand), source ZYX offsets, aimed rotation, drag bar, two-hand resizing and persisted wrist pose/scale |
| `BowItemTask`, `BowItemPose` | Either hand holds the bow; the other must hold arrows; fresh attack press near the string nocks/draws; distance determines power, with 250 ms notch grace and 500 ms shot cooldown; short draw cancels; active hand is restored; source haptic steps and item pose |
| `BowAimCrosshairRenderer` | 64-block collider/pickable-entity ray, 1 cm surface offset, distance/world-scale sizing, draw-ready/miss/local-light brightness, rendered with the existing hand crosshair pass |
| `EssentialsChannel` v2 and server bow mixins | `visor_essentials:channel`, float32 big-endian tension and draw-cancel packets, `betterBow` settings; authoritative full charge with tension-scaled velocity and critical hits only at tension >= 0.99 |
| `RemoteOverlayRenderHelper` | Interpolated remote HMD menu indicator, 0.8 x 0.45 m at 0.45 m from the head, no roll, readable label on both sides, hidden for local/invisible/sleeping/dead players |
| `EssentialsSettingsScreen` | Dedicated VisorEssentials page after Visor's eight VR categories; independent feature toggles and overlay settings, English/Russian labels |

### How to use

Features are enabled by default. Open **VR Settings > VisorEssentials** to toggle
them or change anchors and sizes. Turn the offhand so its inventory faces you,
then point at it with the other controller. A container stays at its source while
the wrist panel displays the player slots of the same menu. Drag its bar to move
it; use the square and both hands to resize it. The ordinary inventory/creative
screens remain available through their normal bindings.

For the physical bow, hold a bow in one hand and an arrow stack in the other.
Bring the arrow hand near the bow, press either attack binding, draw, and release.
The bow feature requires the server to permit Essentials' `betterBow`. Without
permission, Visor's ordinary controller bow behavior remains available.

The native integrated/LAN/dedicated server writes
`visor_essentials-server.yml` in the world's folder:

```yaml
betterBow: true
```

The server sends settings only to clients that announced the addon channel.
Channel announcements survive configuration into the joined player, including
announcements from Java clients. Native clients also announce the channel in
play before the Visor handshake. Bow tension precedes vanilla release on the
same ordered connection and is consumed once. Invalid/non-finite tension is
ignored, and leaving players lose their addon state.

### Native integration details

- VR inventory code: `game/essentials_inventory.rs`; state machine/layout/wire
  logic: `starling-essentials`; bow game/server integration: `game/essentials_bow.rs`.
- Existing menu prediction and server transactions handle all item changes.
  There is no second inventory, local-only item storage, or alternate recipe engine.
- Three additional XR overlay slots hold inventory, container and dragged item.
  A tall virtual canvas fits the book; vertices and depth/scissor rectangles are
  mapped back to the renderer's viewport so controller hits match drawn slots.
- The existing world renderer replaces Minecraft's OpenGL render state calls.
  The indicator uses a localized OVERLAY label. Settings use Starling's existing
  persisted VR configuration instead of AtumConfig YAML on the client.
- The regular mouse/controller screens and native mod screens retain their own
  paths. Custom mod containers need their own VR layout if upstream Essentials
  does not support them either.

### Validation and device checks

Automated validation covers bow nocking/distance/cancel/cooldown, independent
Java wire fixtures, malformed packets, slot geometry, a controller ray through
the real panel-placement helper at several scales, the source ZYX rotation,
virtual-canvas projection, texture decoding and configuration channel retention.
The existing client/gameplay, networking, Visor, VR-input and settings tests are
also run. Reproduce the host checks with:

```sh
cargo test -p horizon-client-input -p horizon-visor -p horizon-vivecraft -p starling-essentials -p horizon-xr --no-default-features --lib --tests
cargo test -p horizon-client-lib -p horizon-client --no-default-features --lib
cargo test -p horizon-client-render --no-default-features --lib essentials_art
cargo clippy -p horizon-client -p horizon-xr -p starling-essentials --no-default-features --lib -- -D warnings -A unreachable-code
python3 tools/check_public_tree.py
```

Host results: 402 VR/input/settings/Essentials tests, 1562 reusable-client tests,
801 gameplay-client tests and the texture-decoding test passed (2766 total).
The existing library suites skip 55 tests. Formatting and the public-tree check
also passed.

The Clippy exception is for the existing disabled-audio branch in
`horizon-client-render/src/audio.rs`; it is unrelated to Essentials. Content-root
checks that need prepared Minecraft resources remain ignored by the host test
suite.

An APK build, a real Quest session and a live Java VisorEssentials server are
**not validated in this environment**. Source parity and host tests do not prove
identical headset feel or GPU output. Verify on the target device:

1. Turn the wrist panel toward your head. Check armor, offhand, every hotbar slot,
   crafting/result, effects, creative button and recipe search/placement.
2. Open a chest, double chest, shulker, furnace, brewing stand and each workstation;
   move stacks both ways between the wrist and container. Test splitting, double
   clicks, drag distribution, scrollers and anvil text input.
3. Carry a stack away from both panels. Left drops all, right drops one. Move and
   resize panels, release the handles, restart, and verify the saved wrist pose.
4. Move away from a container, close it by using its source again, open another
   screen, leave/rejoin and disable Better Inventory: no stuck drag or lost menu.
5. Draw with the bow in each hand at partial/full tension; release under 10%, switch
   items and interrupt tracking. Check no accidental arrow, correct speed/critical
   status, 500 ms cooldown, aim, haptics and return to the prior active hand.
6. Test the native single-player server and a Java server running the pinned addon;
   disable `betterBow` on each. An ordinary server must not activate the feature.
7. Check overlay depth, stereo, long-distance bow reticle, night lighting, remote
   player indicators, SpaceWarp and frame time on the actual Quest.
