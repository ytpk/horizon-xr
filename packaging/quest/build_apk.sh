#!/usr/bin/env bash
# Builds target/quest/horizon-quest.apk: libhorizon_quest.so for aarch64 Android (API 32)
# and the Khronos OpenXR loader in a NativeActivity APK with no Java code, aligned and
# signed with a debug key. No Mojang files go in: the game's resources are pushed
# separately (push_resources.sh).
#
#   packaging/quest/build_apk.sh            # build and package
#   NO_AUDIO=1 packaging/quest/build_apk.sh # without sound (cpal/AAudio)
#   PROFILE=release packaging/quest/build_apk.sh
#   SKIP_CARGO=1 packaging/quest/build_apk.sh  # repackage the last build
#   DEBUGGABLE=1 packaging/quest/build_apk.sh  # android:debuggable, for `adb shell run-as`
#   PACKAGE=dev.horizon.questtest packaging/quest/build_apk.sh  # another app id: its own data folder,
#                                              # beside the real install (for tests on a headset)
#
#   HORIZON_MSA_CLIENT_ID=<id> packaging/quest/build_apk.sh  # your own approved Microsoft application
#                                              # client ID for Azure OAuth; unset uses the
#                                              # Prismarine-compatible Live default (see README)
#   QUEST_DEV_NO_GATE=1 packaging/quest/build_apk.sh  # a DEVELOPER build that can skip the ownership
#                                              # gate (launch.txt line HORIZON_QUEST_NO_GATE=1) to try
#                                              # the download and prepare with no account. Never ship it.
#
# The APK also carries horizon-jvm as libhorizon_jvm.so (a program the app runs from its native
# library folder, extractNativeLibs) and, from build_jre.sh and build_bundle.sh, the assets
# horizon-jre.zip and horizon-bundle.zip. NO_JRE=1 builds without them (no on-headset prepare).
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
# shellcheck source=env.sh
source "$here/env.sh"

PROFILE=${PROFILE:-quest}
OUT=${OUT:-$root/target/quest}
BUILD_TOOLS=${BUILD_TOOLS:-$(ls -d "$ANDROID_HOME"/build-tools/* | sort -V | tail -n 1)}
PLATFORM_JAR=${PLATFORM_JAR:-$(ls -d "$ANDROID_HOME"/platforms/android-* | sort -V | tail -n 1)/android.jar}
LOADER_VERSION=1.1.54
LOADER_SHA256=66702fc24475d7a5362688e77444670124c2fa119745d14c2524bcf3eb584834
LOADER_URL=https://repo1.maven.org/maven2/org/khronos/openxr/openxr_loader_for_android/$LOADER_VERSION/openxr_loader_for_android-$LOADER_VERSION.aar
# What a Quest's system offers to an app's libraries (the NDK's stable ones).
ALLOWED_NEEDED="libc.so libm.so libdl.so liblog.so libandroid.so libvulkan.so libaaudio.so libc++_shared.so libOpenSLES.so libmediandk.so libnativewindow.so"

for tool in aapt2 zipalign apksigner; do
    [ -x "$BUILD_TOOLS/$tool" ] || { echo "missing $BUILD_TOOLS/$tool" >&2; exit 1; }
done
[ -f "$PLATFORM_JAR" ] || { echo "missing $PLATFORM_JAR (sdkmanager 'platforms;android-34')" >&2; exit 1; }
[ -x "$CC_aarch64_linux_android" ] || { echo "missing NDK clang $CC_aarch64_linux_android" >&2; exit 1; }
mkdir -p "$OUT/cache"

features=()
if [ "${NO_AUDIO:-0}" = 1 ]; then
    features=(--no-default-features)
fi
if [ "${QUEST_DEV_NO_GATE:-0}" = 1 ]; then
    echo "== WARNING: developer build with the ownership gate switch (feature dev-no-gate)" >&2
    features+=(--features dev-no-gate)
fi
if [ "${SKIP_CARGO:-0}" != 1 ]; then
    echo "== cargo build (aarch64-linux-android, profile $PROFILE)"
    (cd "$root" && cargo build --locked --target aarch64-linux-android -p horizon-quest --profile "$PROFILE" "${features[@]}")
    (cd "$root" && cargo build --locked --target aarch64-linux-android -p horizon-jvm --profile "$PROFILE")
fi
jvm=${CARGO_TARGET_DIR:-$root/target}/aarch64-linux-android/$PROFILE/horizon-jvm
[ -f "$jvm" ] || { echo "missing $jvm" >&2; exit 1; }
so=${CARGO_TARGET_DIR:-$root/target}/aarch64-linux-android/$PROFILE/libhorizon_quest.so
[ -f "$so" ] || { echo "missing $so" >&2; exit 1; }

echo "== OpenXR loader $LOADER_VERSION"
aar=$OUT/cache/openxr_loader_for_android-$LOADER_VERSION.aar
if [ ! -f "$aar" ]; then
    curl -fsSL -o "$aar.part" "$LOADER_URL"
    mv "$aar.part" "$aar"
fi
echo "$LOADER_SHA256  $aar" | sha256sum -c --quiet - || { echo "loader checksum mismatch: $aar" >&2; rm -f "$aar"; exit 1; }

stage=$OUT/stage
rm -rf "$stage"
mkdir -p "$stage/lib/arm64-v8a"
unzip -p "$aar" jni/arm64-v8a/libopenxr_loader.so > "$stage/lib/arm64-v8a/libopenxr_loader.so"
# Debug info off the copy; the symbol table stays for backtraces in logcat.
"$NDK_BIN/llvm-strip" --strip-debug -o "$stage/lib/arm64-v8a/libhorizon_quest.so" "$so"

echo "== checking the library"
lib=$stage/lib/arm64-v8a/libhorizon_quest.so
needed=$("$NDK_BIN/llvm-readelf" -d "$lib" | sed -n 's/.*(NEEDED).*\[\(.*\)\]/\1/p')
echo "needs: $(echo $needed)"
for n in $needed; do
    case " $ALLOWED_NEEDED " in
        *" $n "*) ;;
        *) echo "libhorizon_quest.so needs $n, which a Quest does not offer" >&2; exit 1 ;;
    esac
done
if echo "$needed" | grep -qx 'libc++_shared.so'; then
    cp "$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/sysroot/usr/lib/aarch64-linux-android/libc++_shared.so" "$stage/lib/arm64-v8a/"
fi
# NativeActivity calls the exported ANativeActivity_onCreate (android-activity's glue),
# which runs our android_main. (No `grep -q` on a pipe: with pipefail nm's SIGPIPE fails it.)
exported=$("$NDK_BIN/llvm-nm" -D --defined-only "$lib")
for sym in ANativeActivity_onCreate android_main; do
    grep -w "$sym" <<<"$exported" >/dev/null || { echo "libhorizon_quest.so exports no $sym" >&2; exit 1; }
done

# The Java launcher: an executable the app runs from its native library folder.
"$NDK_BIN/llvm-strip" --strip-debug -o "$stage/lib/arm64-v8a/libhorizon_jvm.so" "$jvm"
for n in $("$NDK_BIN/llvm-readelf" -d "$stage/lib/arm64-v8a/libhorizon_jvm.so" | sed -n 's/.*(NEEDED).*\[\(.*\)\]/\1/p'); do
    case " $ALLOWED_NEEDED " in
        *" $n "*) ;;
        *) echo "libhorizon_jvm.so needs $n, which a Quest does not offer" >&2; exit 1 ;;
    esac
done
assets=()
if [ "${NO_JRE:-0}" != 1 ]; then
    for asset in horizon-jre.zip horizon-bundle.zip; do
        [ -f "$OUT/assets/$asset" ] || { echo "missing $OUT/assets/$asset: run packaging/quest/build_jre.sh and build_bundle.sh (or NO_JRE=1)" >&2; exit 1; }
        mkdir -p "$stage/assets"; cp "$OUT/assets/$asset" "$stage/assets/"
        assets=(assets)
    done
    # A bundle built before an exporter was added prepares nothing on the headset (it fails with
    # "<exporter>.java: No such file"): every exporter this checkout's preparer runs must be in it,
    # as a class and as the source its stamps hash.
    parity=dev/horizonlab/parity
    listing=$(unzip -Z1 "$OUT/assets/horizon-bundle.zip")
    stale=
    for side in SERVER CLIENT; do
        for main in $(sed -n "/^const $side: &\[&str\] = &\[/,/^\];/s/^ *\"\([A-Za-z]*\)\",$/\1/p" \
            "$root/crates/horizon-prepare/src/exporters.rs"); do
            lower=$(tr '[:upper:]' '[:lower:]' <<<"$side")
            grep -qxF "exporters/$lower/$parity/$main.class" <<<"$listing" || stale+=" $lower/$main.class"
            grep -qxF "mods/horizon/src/test/java/$parity/$main.java" <<<"$listing" || stale+=" $main.java"
        done
    done
    if [ -n "$stale" ]; then
        echo "$OUT/assets/horizon-bundle.zip is older than the preparer, missing:$stale" >&2
        echo "run packaging/quest/build_bundle.sh (OUT=$OUT)" >&2
        exit 1
    fi
fi

echo "== packaging"
rm -f "$OUT/unsigned.apk" "$OUT/aligned.apk"
debug_mode=()
if [ "${DEBUGGABLE:-0}" = 1 ]; then
    debug_mode=(--debug-mode)
fi
manifest=$here/AndroidManifest.xml
if [ -n "${PACKAGE:-}" ]; then
    manifest=$OUT/AndroidManifest.xml
    sed "s/package=\"dev.horizon.quest\"/package=\"$PACKAGE\"/" "$here/AndroidManifest.xml" > "$manifest"
fi
"$BUILD_TOOLS/aapt2" link "${debug_mode[@]}" \
    --manifest "$manifest" \
    -I "$PLATFORM_JAR" \
    -o "$OUT/unsigned.apk"
# Stored, not deflated, and 16 KiB aligned below. The libraries are extracted at install
# (extractNativeLibs="true"): horizon-jvm is a program, and only a file can be run. The assets
# are zip files already: stored too, so the app reads them straight from the APK.
(cd "$stage" && zip -q -0 -r "$OUT/unsigned.apk" lib "${assets[@]}")
# 16 KiB page alignment for the stored libraries (-P 16), 4 bytes for the rest.
"$BUILD_TOOLS/zipalign" -f -P 16 4 "$OUT/unsigned.apk" "$OUT/aligned.apk"
"$BUILD_TOOLS/zipalign" -c -P 16 4 "$OUT/aligned.apk"

keystore=${KEYSTORE:-$OUT/debug.keystore}
if [ ! -f "$keystore" ]; then
    echo "== making a debug key ($keystore)"
    keytool -genkeypair -keystore "$keystore" -storepass android -keypass android \
        -alias androiddebugkey -keyalg RSA -keysize 2048 -validity 10000 \
        -dname "CN=Android Debug,O=Android,C=US" >/dev/null
fi
apk=$OUT/horizon-quest.apk
"$BUILD_TOOLS/apksigner" sign --ks "$keystore" --ks-pass pass:android --key-pass pass:android \
    --ks-key-alias androiddebugkey --v2-signing-enabled true --v3-signing-enabled true \
    --out "$apk" "$OUT/aligned.apk"
"$BUILD_TOOLS/apksigner" verify "$apk" 2>/dev/null
rm -f "$OUT/unsigned.apk" "$OUT/aligned.apk" "$apk.idsig"

echo "== $apk ($(du -h "$apk" | cut -f1))"
"$BUILD_TOOLS/aapt2" dump badging "$apk" | grep -E "^(package|minSdkVersion|targetSdkVersion|native-code|launchable-activity)|vr.headtracking"
