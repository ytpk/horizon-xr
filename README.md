# Horizon Quest (Visor port)

An unofficial fork of [Horizon](https://github.com/horizon-rust/horizon) (Minecraft: Java Edition 26.3 rewritten in Rust)
with a port of the [Visor](https://github.com/VisorModStudio/Visor) VR mod. It runs standalone on **Meta Quest 3 / 3S**
(Quest 2 / Pro untested), with a launcher room inside the headset. The upstream README is kept as
[README.horizon.md](README.horizon.md).

> Not an official Minecraft product. Not approved by or associated with Mojang, Microsoft or Meta.
> The APK contains **no Minecraft code, assets or data**.

## Install

Published APK releases are currently hidden. Source builds include the experimental sign-in modes described below.
Build an APK using the instructions below, enable developer mode on the headset, then install it with
[SideQuest](https://sidequestvr.com/) or:

```sh
adb install -r target/quest/horizon-quest.apk
```

Start **Horizon** from Library > Unknown Sources. These builds use a local **debug signing key**;
keep `target/quest/debug.keystore` for future updates. An APK signed with a different key cannot update
an existing installation. Back up your worlds before uninstalling anything.

## Microsoft sign-in

Microsoft sign-in has two modes, both followed by the Minecraft ownership and profile checks:

- **No `HORIZON_MSA_CLIENT_ID` configured:** a native Rust adaptation of
  [prismarine-auth](https://github.com/PrismarineJS/prismarine-auth)'s default Live flow.
  The browser identifies its application as **Minecraft for Nintendo Switch**. This public client ID
  belongs to Microsoft's Minecraft title; it is **not** PrismarineJS's or this fork's own registration.
  No Node.js runtime is included. This compatibility mode is experimental and has not been verified
  with a real account on Quest; availability depends on Microsoft's services.
- **Your own approved `HORIZON_MSA_CLIENT_ID`:** Azure OAuth device-code sign-in, as before.
  Register your application and obtain the required [Minecraft App ID approval](https://aka.ms/mce-reviewappid).
  Horizon's application client ID must not be used. An explicitly **empty** variable disables sign-in.

`HORIZON_MSA_CLIENT_ID` takes an application ID, never an account password, personal access token
or client secret. Changing applications may require signing in again: credentials issued for one
application are not silently refreshed through another. Signing into another installed launcher does
not automatically sign this APK in; external session handoff is not implemented.

**Self-building does not disable the ownership gate.** Sign in from the room using the displayed
Microsoft code or QR code; the account must own **Minecraft: Java Edition**. Game files are downloaded
from Mojang and prepared on the headset (at least 3 GB free); they are absent from the APK and repository.
See [the implementation and validation notes](docs/quest-live-auth-render-stability.md).

## Optional opaque water

Use **Room → Settings → Graphics → additional graphics options → Opaque Water**, or
**VR Settings → Rendering → Opaque Water**. It is off by default.

When enabled, nearby and distant water draw in the depth-writing solid pass, without
transparency sorting or blending. Water textures, biome tint and lighting remain; the
surface hides the bottom and scenery behind it. Changing the setting rebuilds loaded
terrain and far-detail tiles, so the world may briefly reload. The setting is saved.
Shader packs use normal transparent water even when the preference is enabled.
Actual Quest FPS improvement still needs testing.

## Experimental foveated multiview

**Active foveation disables standard multiview.** To try both, enable Multiview and the
separate **Experimental multiview + foveation** option in the room's graphics settings.
Restart after selecting a foveation level. The experiment attempts direct rendering into
a native two-layer OpenXR foveated swapchain, with per-eye fallback when unavailable.
It is off by default and may use more memory. Actual Quest compatibility and FPS are
unverified; see [details and device checks](docs/quest-foveated-multiview.md).

## Build the APK

The complete build is currently supported on **Linux x86-64**, including **Ubuntu in WSL2 on Windows**.
The existing Android-JRE builder uses Linux host tools; Git Bash or a native Windows Android SDK alone
is not sufficient. The scripts run from any working directory and stop at the first failed step.
A first build downloads dependencies and compiles a large Rust project: allow several hours and tens
of GB of free space. Subsequent builds reuse Cargo's build cache and downloaded archives.

### One-time setup (Linux / Ubuntu in WSL2)

1. Install Linux build tools:

   ```sh
   sudo apt update
   sudo apt install build-essential pkg-config libssl-dev libasound2-dev libudev-dev \
     curl unzip zip xz-utils binutils patchelf python3 ca-certificates
   ```

2. Install [Rust stable](https://rustup.rs/) (at least the version specified in `Cargo.toml`) and
   [JDK 25 for Linux x64](https://adoptium.net/temurin/releases/?version=25).
   Ensure `cargo`, `rustup`, `java`, `javac` and `keytool` are on the **Linux** PATH, then run:

   ```sh
   rustup target add aarch64-linux-android
   ```

3. Download the **Linux Command line tools** from [Android Studio downloads](https://developer.android.com/studio#command-tools).
   Extract them so `sdkmanager` is at `~/Android/Sdk/cmdline-tools/latest/bin/sdkmanager`.
   Install the Linux SDK and NDK (accept the SDK licences when prompted):

   ```sh
   export ANDROID_HOME="$HOME/Android/Sdk"
   "$ANDROID_HOME/cmdline-tools/latest/bin/sdkmanager" --sdk_root="$ANDROID_HOME" \
     "platform-tools" "platforms;android-34" "build-tools;35.0.0" "ndk;29.0.14206865"
   "$ANDROID_HOME/cmdline-tools/latest/bin/sdkmanager" --sdk_root="$ANDROID_HOME" --licenses
   ```

4. Install and launch your own **Minecraft: Java Edition 26.3** once with the official launcher.
   Obtain the matching server jar for the preparer as described in [the upstream build instructions](README.horizon.md).
   The build imports these local files to compile Horizon's original exporters; it does not package
   Minecraft jars, resources or data into the APK.

5. In the repository root, copy the local configuration template:

   ```sh
   cp quest-build.env.example .quest-build.env
   ```

   Edit `.quest-build.env`: set `HORIZON_MINECRAFT_DIR` and `HORIZON_SERVER_JAR` to your local files,
   and set SDK/JDK paths if necessary. Set `HORIZON_MSA_CLIENT_ID` only if you have your own suitable
   application registration; leave it unset for the experimental Live default. The local configuration is ignored by Git. All paths in it must use
   Linux syntax; quote paths containing spaces.

### Build with one command

```sh
sh build-apk.sh --check   # Check tools only; no downloads or compilation.
sh build-apk.sh           # Import game files, build Android JRE + exporters, build and sign APK.
```

You can also run `bash build-apk.bash`. The result is **`target/quest/horizon-quest.apk`**, with a
`.sha256` checksum alongside it. Optional settings such as `OUT` and `PROFILE` go in `.quest-build.env`.
The default `quest` profile is optimized. Do not commit the local configuration, signing key,
Minecraft files or build output.

### Windows: double-click build

1. In an Administrator PowerShell, run `wsl --install -d Ubuntu`. Restart if requested, launch Ubuntu
   and finish creating its Linux user. Use an x86-64 PC with WSL2.
2. Complete the one-time Linux setup above **inside Ubuntu**, including Linux Rust, JDK, SDK and NDK.
   Windows installations of these tools do not replace the Linux ones.
3. For a checkout on a Windows drive, double-click **`build-apk.cmd`** in the repository root.
   It invokes the default WSL distribution, builds the complete APK and leaves the window open so
   errors remain visible. Select Ubuntu as the default with `wsl --set-default Ubuntu` if needed.
   You can run `build-apk.cmd --check` from a terminal to check prerequisites first.

Windows Minecraft files can be read from WSL, for example
`/mnt/c/Users/YOUR_NAME/AppData/Roaming/.minecraft`; configure that path in `.quest-build.env`.
The resulting APK appears in the checkout's `target/quest` folder. Compiling on `/mnt/c` can be slow:
for better performance, clone into Ubuntu's `~/horizon-xr` and run `sh build-apk.sh` there instead.
Install the APK using Windows SideQuest or Windows `adb`; USB passthrough into WSL is not required.

The scripts do not install system packages or register an Azure application for you. Full APK
compilation and the Windows launcher still require testing on a machine with those dependencies.
Low-level packaging options are documented in [packaging/quest/README.md](packaging/quest/README.md).

## Status and known issues

The port is described in [docs/visor-port.md](docs/visor-port.md). Most of it is ported but only tested in simulation
or on mocks, not on a headset with a real account. Known gaps: the Microsoft sign-in is not tested end to end with a real
account on a headset; the bow sits in the off hand; multiview and native performance controls are experimental; SpaceWarp is not implemented; the prepare step
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

Неофициальный форк Horizon с портом VR-мода Visor для **Meta Quest 3 / 3S**, автономно, без ПК во время игры.
Публичные APK пока скрыты. Сборка после установки зависимостей: `sh build-apk.sh` в Linux;
на Windows — двойной щелчок по `build-apk.cmd` через заранее настроенный Ubuntu/WSL2.
Инструкция выше описывает Rust, Linux JDK 25, Android SDK/NDK и локальный `.quest-build.env`.
Результат: `target/quest/horizon-quest.apk`, установка через SideQuest или `adb install -r`.
Без указанного ID используется экспериментальный Live-вход по схеме prismarine-auth;
в браузере приложение называется Minecraft для Nintendo Switch. Это публичный ID приложения Microsoft,
а не регистрация PrismarineJS или форка. Собственный одобренный Azure ID можно указать отдельно;
пустое значение отключает вход. ID Horizon использовать нельзя. Проверка покупки Java Edition сохраняется.
Лицензии: Horizon MIT, Visor MPL-2.0, Vivecraft LGPL-3.0, OpenJDK GPLv2 + Classpath Exception; подробности в `THIRD_PARTY_NOTICES.md`.
Не одобрено Mojang, Microsoft или Meta.

Upstream update branch: [integration notes and headset checks](docs/quest-upstream-integration.md).
