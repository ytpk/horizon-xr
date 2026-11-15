# Starling modded

`starling-main-modded` starts at `starling-main` commit
`934b71707943f8294e4422359cfc1bd957e973a8`. This is the fork owner's native VR
mod branch, separate from upstream Horizon. Source and assets may be used under
their respective licenses; retain copyright and license notices when porting.

## Selection

Prefer mods whose blocks, items, textures and effects fit vanilla Minecraft and
whose interactions are useful and comfortable with tracked hands. Keep text
readable in stereo, preserve world scale and depth, avoid camera shake and forced
head movement. Port one observable feature at a time, with a settings toggle.
Future user-supplied mods enter this ledger before implementation; additions
should not claim Java JAR compatibility.

## VisorEssentials

Source: https://github.com/VisorModStudio/VisorEssentials
Pinned revision: `88ffec507449eb1a97ccbf495cd331a46c6d635b`.
License: LGPL-3.0-only; Phoenix-Ra and VisorEssentials contributors.

| Feature | Status | Implementation / next step |
| --- | --- | --- |
| Remote overlay focus visualizer | Implemented; headset verification pending | `starling-essentials::FocusPanel`, `game/essentials.rs`; consumes existing Visor `overlay_focused` state and HMD poses |
| Better inventory | Pending | Wrist inventory, separate container and dragged-item overlays, recipe book, cursor focus and slot routing |
| Better bow | Pending | Replace Vivecraft bow behavior with Essentials hand selection, nocking, draw cancellation and tension; implement addon channel v2 on client and server |
| Addon settings | Partial | Indicator toggle in VR Settings > Visor; other toggles arrive with their working features |
| Upstream textures | Pending consumers | Import inventory/container textures with their UI implementation, preserving upstream attribution |
| General Visor addon API / Java loader | Not implemented | Native feature integration does not execute upstream Java addons |

### Indicator

Enabled by default on this modded branch. `essentialsOverlayFocusVisualizer` is
persisted in the VR settings and can be switched off under VR Settings > Visor.
A 0.8 x 0.45 m translucent panel sits 0.45 m in front of the other player's head,
without roll, using body yaw when the head looks straight up/down. It is sorted
back-to-front with other indicators and depth tested by the normal translucent
entity material, without depth writes. The localized MENU label is readable
from either side. Invisible, sleeping, dead and local players are skipped.

This uses Visor's existing core networking, requiring no Essentials install on
the server. A server must send remote Visor tracking/focus updates. There is no
remote screen capture: the panel is a placeholder as in the original mod.

Differences from upstream: localized MENU instead of the hardcoded OVERLAY label;
the existing wgpu translucent material with full light replaces Minecraft's
OpenGL state calls. Actual screen contents are never transmitted.

### Manual verification

1. Join a Visor-enabled server with a second Visor player. Open/close a menu on
   that client: its panel appears/disappears on this client.
2. Walk around the panel: the label reads from both sides. Walk behind a wall:
   the panel is occluded. Check near/far panels and stereo on a headset.
3. Rotate the remote head, look vertically, move across tick boundaries: no roll,
   unstable heading, or tick snapping. Invisible/sleeping/dead players show none.
4. Switch the indicator off, restart, and verify it remains off. Ordinary
   non-Visor players show no indicator.

### Validation

Automated geometry tests cover the upstream dimensions and placement, roll
removal, vertical-look fallback, invalid poses, and label orientation on both
sides. No APK or headset run is claimed by this initial port.

Initial checks: `cargo check -p horizon-client --no-default-features --lib`
passed (existing audio unreachable-code warning); `cargo test -p starling-essentials
-p horizon-vivecraft` passed 147 tests; formatting and public tree checks passed.
