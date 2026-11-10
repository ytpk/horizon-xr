# Horizon Quest (Visor port)

An unofficial fork of [Horizon](https://github.com/horizon-rust/horizon) (Minecraft: Java Edition 26.3 rewritten in Rust)
with a port of the [Visor](https://github.com/VisorModStudio/Visor) VR mod. It runs standalone on **Meta Quest 3 / 3S**
(Quest 2 / Pro untested), with a launcher room inside the headset. The upstream README is kept as
[README.horizon.md](README.horizon.md).

> Not an official Minecraft product. Not approved by or associated with Mojang, Microsoft or Meta.
> The APK contains **no Minecraft code, assets or data**.

## Install

1. Put the headset in developer mode.
2. Sideload `horizon-quest-0.1.0.apk` from the Releases page with [SideQuest](https://sidequestvr.com/), or `adb install horizon-quest-0.1.0.apk`.
3. Start it from Library > Unknown Sources.

## First start

You land in the launcher room. Point at **Sign in** and sign in with a Microsoft account, by the code shown (microsoft.com/link)
or by scanning the QR code with your phone. The account must **own Minecraft: Java Edition**. After that the game files are
downloaded from Mojang's servers to the headset (about 3 GB free needed) and prepared; none of them ship in the APK or in
this repository. Then press Play.

## Build

Rust with the `aarch64-linux-android` target, Android SDK (build-tools 35) and NDK, JDK 25.
See [packaging/quest/README.md](packaging/quest/README.md); in short `packaging/quest/build_apk.sh` (the Java runtime and
resource bundle come from `build_jre.sh` and `build_bundle.sh`).

## Status and known issues

The port is described in [docs/visor-port.md](docs/visor-port.md). Most of it is ported but only tested in simulation
or on mocks, not on a headset with a real account. Known gaps: the Microsoft sign-in is not tested end to end with a real
account on a headset; the bow sits in the off hand; foveation, multiview and SpaceWarp are not done yet; the prepare step
cannot be cancelled; hand and body tracking are untested on a device; there is no auto-update.

## Credits and licenses

- [Horizon](https://github.com/horizon-rust/horizon) by Ludovic Marie and contributors, MIT.
- [Visor](https://github.com/VisorModStudio/Visor) by Phoenix-Ra and contributors, MPL-2.0 (ported with permission).
- [Vivecraft](https://github.com/Vivecraft/VivecraftMod) by Vivecraft contributors, LGPL-3.0-or-later (ported with permission).
- [OpenJDK](https://openjdk.org/) 25 (GPLv2 + Classpath Exception), built by [Termux](https://github.com/termux/termux-packages);
  Khronos OpenXR loader (Apache-2.0); qrcodegen (MIT); Geist, Bricolage Grotesque and Pixelify Sans fonts (OFL 1.1).

License summary: the Horizon code is MIT (`LICENSE`); `crates/horizon-visor` and files marked MPL-2.0 are MPL-2.0
(`crates/horizon-visor/LICENSE-MPL-2.0`); the Vivecraft-derived crates are LGPL-3.0 (`crates/horizon-vivecraft/LICENSE-LGPL-3.0`,
`crates/horizon-xr/LICENSE-LGPL-3.0`). Full texts, versions and source offers: [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

## По-русски

Неофициальный форк Horizon (Minecraft Java Edition 26.3 на Rust) с портом VR-мода Visor для **Meta Quest 3 / 3S**,
автономно, без компьютера. Установка: APK из Releases через SideQuest или `adb install`. При первом запуске в комнате-лаунчере
войдите в аккаунт Microsoft по коду или QR-коду; нужна купленная Minecraft: Java Edition. Файлы игры скачиваются с серверов
Mojang прямо на шлем, в APK их нет. Сборка: `packaging/quest/`. Состояние и известные проблемы: `docs/visor-port.md`.
Лицензии: Horizon MIT, Visor MPL-2.0, Vivecraft LGPL-3.0, OpenJDK GPLv2 + Classpath Exception; подробности в `THIRD_PARTY_NOTICES.md`.
Не одобрено Mojang, Microsoft или Meta.

Upstream update branch: [integration notes and headset checks](docs/quest-upstream-integration.md).
