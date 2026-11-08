# Third-party notices

## wgpu-hal

`third_party/wgpu-hal` is [wgpu-hal](https://github.com/gfx-rs/wgpu) 30.0.1 by the gfx-rs
developers, with upstream fixes applied
([gfx-rs/wgpu#10124](https://github.com/gfx-rs/wgpu/pull/10124),
[gfx-rs/wgpu#10714](https://github.com/gfx-rs/wgpu/pull/10714)). It is licensed under MIT or
Apache-2.0; both licenses are in that folder (`LICENSE.MIT`, `LICENSE.APACHE`).

## naga

`third_party/naga` is [naga](https://github.com/gfx-rs/wgpu) 30.0.1 by the gfx-rs developers, with
fixes to its GLSL frontend that shader packs need, each applied in the commit that needed it. It is licensed under MIT or Apache-2.0; both licenses
are in that folder (`LICENSE.MIT`, `LICENSE.APACHE`).

The GLSL frontend also resets the common type for each comma-separated declarator so one
variable's array specifier does not change later variables.
It also reads a struct member outside a constant expression through a pointer to the member,
not out of a loaded copy of the whole struct, so a member an atomic function uses can be read.

## libmimalloc-sys and mimalloc

`third_party/libmimalloc-sys` is [libmimalloc-sys](https://github.com/purpleprotocol/mimalloc_rust)
0.1.49 by Octavian Oncescu, with the [mimalloc](https://github.com/microsoft/mimalloc) v3 sources it
bundles (Copyright (c) 2018-2025 Microsoft Corporation, Daan Leijen), both under the MIT license
(`LICENSE.txt` and `c_src/mimalloc/v3/LICENSE` in that folder). It holds only the v3 sources. One
change: `mi_popcount` in `include/mimalloc/bits.h` uses mimalloc's portable bit count on MSVC
x86/x64 instead of the POPCNT instruction, which CPUs before Nehalem lack (#7264).

## Pixelify Sans

`crates/horizon-theme/fonts/PixelifySans-*.ttf` is [Pixelify Sans](https://github.com/eifetx/Pixelify-Sans)
by The Pixelify Sans Project Authors, licensed under the SIL Open Font License 1.1
(`crates/horizon-theme/fonts/OFL-PixelifySans.txt`).

## Pixelarticons

`crates/horizon-theme/icons/pixel/*.svg` are icons from
[Pixelarticons](https://github.com/halfmage/pixelarticons) by Gerrit Halfmann, under the MIT
license (`crates/horizon-theme/icons/pixel/LICENSE-pixelarticons.txt`). `star.svg` is Horizon's own, drawn on
the same grid.

## Hugeicons

`crates/horizon-theme/icons/glass/*.svg` are icons from the free set of
[Hugeicons](https://github.com/hugeicons/hugeicons) (`@hugeicons/core-free-icons` 4.4.0), under
the MIT license (`crates/horizon-theme/icons/glass/LICENSE-hugeicons.txt`), exported with a 1.6
stroke in `currentColor`.

`crates/horizon-theme/icons/glass/discord.svg` is Discord's logo as drawn by
[Simple Icons](https://github.com/simple-icons/simple-icons) 13.21.0 (CC0 1.0). The logo is Discord's
trademark, used only on the button that opens Horizon's Discord server.

## AMD FidelityFX SDK (FSR 3.1 frame generation)

`crates/horizon-client-render/src/gfx/frame_gen/optical_flow.wgsl` and `interpolation.wgsl` are
ports to WGSL of the optical flow and frame interpolation shaders of the
[AMD FidelityFX SDK](https://github.com/GPUOpen-LibrariesAndSDKs/FidelityFX-SDK) 1.1.4
(`sdk/include/FidelityFX/gpu/{opticalflow,frameinterpolation}`, at commit
[c6efa6bf7f20](https://github.com/GPUOpen-LibrariesAndSDKs/FidelityFX-SDK/tree/c6efa6bf7f2027b3ec94f28578bb5965eabb9e55)),
and `optical_flow.rs` follows the SDK's `sdk/src/components/opticalflow/ffx_opticalflow.cpp`
(its passes, their order and their dispatch sizes). Copyright (C) 2024 Advanced Micro Devices,
Inc., under the MIT license, whose text heads each file. AMD and FidelityFX are AMD's
trademarks; Horizon is not affiliated with AMD.

## Mod loader logos

The launcher shows the logo of the mod loader an instance plays with, and offers the logos as
instance icons.

- `launcher/assets/logos/{fabric,quilt,neoforge,forge}.png` are the loaders' logos as the launcher
  mockup on [playhorizon.dev/launcher](https://playhorizon.dev/launcher) shows them (horizon-site
  `public/media/launcher/*.webp`, converted to PNG), each its project's own mark:
  [Fabric](https://fabricmc.net/), [Quilt](https://quiltmc.org/) (its art is
  [CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/)),
  [NeoForged](https://neoforged.net/) (its art, by Ridanisaurus, is
  [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/)) and
  [Minecraft Forge](https://minecraftforge.net/). They name the loader an instance runs.

Each logo belongs to its project. Showing it says which loader an instance runs; it does not mean
the project endorses Horizon.

## Sign in with Microsoft

The launcher's sign-in buttons follow Microsoft's published branding guidelines for apps that sign in
with Microsoft accounts
([Sign in with Microsoft branding](https://learn.microsoft.com/en-us/entra/identity-platform/howto-add-branding-in-apps)).
The four-square Microsoft symbol in `launcher/src/microsoft.rs` reproduces the squares, positions and
colours of Microsoft's official `ms-symbollockup_mssymbol_19.svg` from that page, unmodified. Microsoft
and its logo are trademarks of the Microsoft group of companies; Horizon is not endorsed by Microsoft.

## CIT Resewn

`crates/horizon-optifine/src/cit.rs` and `crates/horizon-client-render/src/cit.rs` follow the
condition semantics of [CIT Resewn Continuation](https://github.com/EthanVisagie/CIT-Resewn-Continuation-1.21.11)
(at `b792f9a`), a continuation of [CIT Resewn](https://github.com/SHsuperCM/CITResewn) by SHsuperCM: the
1.21 component paths, enchantment levels, percent damage, text matching and rule ordering. It is
licensed under the MIT license:

> Copyright (c) 2021 SHsuperCM
>
> Permission is hereby granted, free of charge, to any person obtaining a copy of this software
> and associated documentation files (the "Software"), to deal in the Software without
> restriction, including without limitation the rights to use, copy, modify, merge, publish,
> distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the
> Software is furnished to do so, subject to the following conditions:
>
> The above copyright notice and this permission notice shall be included in all copies or
> substantial portions of the Software.
>
> THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING
> BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
> NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM,
> DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
> OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

## FusionExamples

The tile tables in `crates/horizon-optifine/src/fusion/layout.rs` were read off the connecting
texture templates (`templates/template <layout>.png`) of
[FusionExamples](https://github.com/SuperMartijn642/FusionExamples) (at `8c9c56e`), which is
licensed under the MIT license:

> Copyright (c) 2024 Martijn van den Brand
>
> Permission is hereby granted, free of charge, to any person obtaining a copy of this software
> and associated documentation files (the "Software"), to deal in the Software without
> restriction, including without limitation the rights to use, copy, modify, merge, publish,
> distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the
> Software is furnished to do so, subject to the following conditions:
>
> The above copyright notice and this permission notice shall be included in all copies or
> substantial portions of the Software.
>
> THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING
> BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
> NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM,
> DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
> OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

## BlueMap

`crates/horizon-client-render/src/bluemap.rs` reads the lowres tiles of a server's
[BlueMap](https://github.com/BlueMap-Minecraft/BlueMap) web map. It follows three of BlueMap's files:
`core/src/main/java/de/bluecolored/bluemap/core/map/lowres/LowresLayer.java` (`saveTile`) for the
tiles' pixel layout, `common/webapp/src/js/util/Utils.js` (`pathFromCoords`) for their paths, and
`core/src/main/java/de/bluecolored/bluemap/core/map/MapSettingsSerializer.java` for a map's
`settings.json`, at commit
[96ae320d1d39](https://github.com/BlueMap-Minecraft/BlueMap/tree/96ae320d1d39581c10058d8222f2290f2fcb5333).
BlueMap is licensed under the MIT license:

> Copyright (c) Blue (Lukas Rieger) <https://bluecolored.de>
> Copyright (c) contributors
>
> Permission is hereby granted, free of charge, to any person obtaining a copy
> of this software and associated documentation files (the "Software"), to deal
> in the Software without restriction, including without limitation the rights
> to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
> copies of the Software, and to permit persons to whom the Software is
> furnished to do so, subject to the following conditions:
>
> The above copyright notice and this permission notice shall be included in
> all copies or substantial portions of the Software.
>
> THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
> IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
> FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
> AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
> LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
> OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
> THE SOFTWARE.

## Vivecraft

`crates/horizon-vivecraft`, `crates/horizon-xr` and the files marked `SPDX-License-Identifier: LGPL-3.0-or-later`
(in `crates/horizon-client/src/game/`: `vr.rs`, `vr_keyboard.rs`, `vivecraft_net.rs`, `vivecraft_server.rs`, `headset.rs`,
`headset_bow.rs`, `headset_radial.rs`, `headset_hands.rs`, `headset_trackers.rs`; the thin glue that applies the
ported logic sits in otherwise MIT files such as `mobs/anim.rs` (the VR player model), `game/shield.rs`,
`game/creatures.rs`, `game/boats.rs` (rowing and boat steering), `game/remote_vehicles.rs` (mount
steering), `game/lan.rs` and `game/lan_actions.rs`; in
`crates/horizon-client-render/src/`: `vr_options.rs`, and `options_ui.rs` for the radial menu and quick command
pages) are a Rust port of parts of
[Vivecraft](https://github.com/Vivecraft/VivecraftMod) (Copyright (c) Vivecraft contributors), commit
`8b4d6b95a1891d9839f19b45c13e93415c02a31e` (mod 1.3.15, Minecraft 26.2). Vivecraft is licensed under the
GNU Lesser General Public License v3 (or later); the text is in `crates/horizon-vivecraft/LICENSE-LGPL-3.0`
(and `crates/horizon-xr/LICENSE-LGPL-3.0`). The rest of Horizon stays MIT. A build that includes these
crates is distributed under the terms of the LGPL for their code: the complete source is in this
repository, and you may modify and relink it.

Ported: the `vivecraft:data` network protocol, the client and server networking state, `MathUtils`, the
`VRSettings` option table with its config file, the options pages and their names (`GuiMainVRSettings` and the
screens it opens), `VRData`, `VRPlayer` (world scale, turning, free move; the room walk behind the Horizon-only
`horizonVivecraftRoomWalk` setting; the look override is ported but not driven), the gameplay trackers (sneak, jump, crawl, run, swim, climb and the Climbey boots jump,
eating, backpack, bow, swing, teleport, row, vehicle), the physical keyboard (`PhysicalKeyboard`,
`KeyboardKeys`, `KeyboardTheme`), the radial menu configuration and quick commands, the first-person hand and item transforms (`VivecraftItemRendering`,
`VRArmRenderer`), the VR player model (`VRPlayerModel`, `ModelUtils`), the server's aim fix, bow draw and
multipliers, headshots, roomscale shield blocking and dual wielding, `GuiHandler`'s screen and HUD
placement, the interactive hotbar, the radial menu, the crosshair, the Touch default bindings, the server
setting overrides, and Vivecraft's language strings (`vivecraft.options.*`, `vivecraft.gui.*`, `vivecraft.key.*`,
`vivecraft.messages.*` and more). See `docs/vivecraft-port.md` for the full list and what is not ported.

## Visor

`crates/horizon-visor` and the files marked `SPDX-License-Identifier: MPL-2.0` (in
`crates/horizon-client/src/game/`: `visor_net.rs`, `visor_server.rs`, `visor_play.rs`, `visor_trackers.rs`; thin glue in
otherwise MIT files such as `game/remote.rs`, `net.rs`, `game/lan_actions.rs`) are a Rust port of parts of
[Visor](https://github.com/VisorModStudio/Visor) (Copyright (C) Phoenix-Ra and Visor contributors), commit
`c01354fad8ac03a016c249239a04b07adeb36476` (Visor 0.6.1), and of the server half of
[VisorPlugin](https://github.com/VisorModStudio/VisorPlugin), commit `b4f3fc8be0e67f517c3e98c07ac1b5510eaf3d24`
(0.2.0alpha). Visor is licensed under the Mozilla Public License 2.0; the text is in
`crates/horizon-visor/LICENSE-MPL-2.0`. The copyright holders and the Visor authors gave their permission for
this fork. The rest of Horizon stays MIT (and the Vivecraft files LGPL). The MPL covers the files that hold
Visor's code: they stay MPL, their source is in this repository, and notices must be kept.

Ported: the `visor:channel` network protocol (version 5): all payloads and pose buffers, `VRServerSettings` and the
`SERVER_SETTINGS` text, the client and server networking state, the gameplay tasks (swing, teleport, crawl,
climb, off hand), other VR players, hand and body tracking data; and, in round 4, Visor's look and feel: the game menu
(`game/visor_menu.rs`) and the hotbar wheel (`game/visor_hotbar.rs`, `horizon-visor/src/tasks/hotbar.rs`), the height
tracking (`horizon-visor/src/height.rs`), the title's menu room (`horizon-client-render/src/gfx/menu_scene.{rs,wgsl}`: the
palette, the sun, the moon's phase, the VISOR glyphs and the floor of `VRMenuSky` and `VRMenuFloor`), the settings screens'
structure and the language strings imported into `assets/horizon/lang/{en_us,ru_ru}.json` (`visor.*`). Those files carry
`SPDX-License-Identifier: MPL-2.0` headers. See `docs/visor-port.md`.

Visor's own textures are bundled unchanged in `crates/horizon-client-render/assets/visor/` (the hotbar wheel's pictures,
`movement_state.png`, `warning.png`, `icon_headset.png`; see the `NOTICE` there) and loaded by
`crates/horizon-client-render/src/visor_art.rs` (MPL-2.0, Copyright (C) Phoenix-Ra and Visor contributors). The overlay
panel (`game/visor_overlay.rs`) is also MPL-2.0.

## Khronos OpenXR loader

The Meta Quest build bundles the [OpenXR loader](https://github.com/KhronosGroup/OpenXR-SDK)
(`org.khronos.openxr:openxr_loader_for_android`) by The Khronos Group Inc., licensed under Apache-2.0.

## OpenJDK 25 (the Java runtime inside the Quest APK)

The Quest APK bundles a Java runtime (`assets/horizon-jre.zip`, built by `packaging/quest/build_jre.sh`) that runs the
Minecraft game files on the headset. It is OpenJDK 25.0.5, built for Android by the
[Termux](https://github.com/termux/termux-packages) project (package `openjdk-25_25.0.5-1_aarch64.deb`; URL and SHA-256 in
`build_jre.sh`), with the module set cut down by `jlink`. Horizon changes nothing in OpenJDK itself.
OpenJDK is licensed under the GNU General Public License version 2 with the Classpath Exception. The license texts
(`LICENSE`, `ASSEMBLY_EXCEPTION`, `ADDITIONAL_LICENSE_INFO`) are in the runtime image under `legal/`.

Source offer: the corresponding source is the OpenJDK 25 source, <https://github.com/openjdk/jdk25u> (tag
`jdk-25.0.5-ga`, please check the exact tag name), with the Android patches and build recipe of Termux:
<https://github.com/termux/termux-packages/tree/master/packages/openjdk-25>. If those links stop working, ask in
this repository's issues and the source will be provided.

## Termux zlib and libandroid-spawn

The runtime image also holds two libraries from Termux packages (versions and SHA-256 in `build_jre.sh`):
`libz.so.1` from `zlib_1.4` (the zlib license) and `libandroid-spawn.so` from `libandroid-spawn_0.3`
(Copyright (C) 2017 The Android Open Source Project; a BSD-style license, its text is in the package at
`share/doc/libandroid-spawn/LICENSE`; the exact BSD variant: to verify).

## LLVM libc++ (`libc++_shared.so`)

`libc++_shared.so` from the Android NDK (r30 beta/`30.0.16248370`) is placed in the runtime image, and in the APK's `lib/` when the
native library needs it. libc++ is licensed under Apache-2.0 with LLVM exceptions
(<https://libcxx.llvm.org/>).

## qrcodegen

The launcher room draws the sign-in QR code with the [qrcodegen](https://github.com/nayuki/QR-Code-generator) crate 1.8
by Project Nayuki, licensed under the MIT license.

## Geist and Bricolage Grotesque

`crates/horizon-theme/fonts/Geist-*.ttf`, `GeistMono-*.ttf` are [Geist](https://github.com/vercel/geist-font) by
Vercel and Basement Studio, and `crates/horizon-theme/fonts/BricolageGrotesque*.ttf` are
[Bricolage Grotesque](https://github.com/ateliertriay/bricolage) by Mathieu Triay, both under the SIL Open Font License 1.1
(`crates/horizon-theme/fonts/OFL-Geist.txt`, `OFL-BricolageGrotesque.txt`). They are built into the client.

## Source for the Quest APK

The source of a release APK is the tagged commit of this repository (for example `v0.1.0`). To rebuild it, for instance
with a changed Vivecraft-derived (LGPL) crate, run `packaging/quest/build_apk.sh` (see `packaging/quest/README.md`).

This project is not approved by or associated with Mojang, Microsoft or Meta. Meta Quest is a trademark of Meta
Platforms, Inc.
