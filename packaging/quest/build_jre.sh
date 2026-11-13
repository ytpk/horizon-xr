#!/usr/bin/env bash
# Builds the Java runtime the Quest runs Horizon's exporters on: a jlinked, minimal OpenJDK 25
# for aarch64 Android, zipped as target/quest/assets/horizon-jre.zip (build_apk.sh puts it in the
# APK). It holds no Mojang file. OpenJDK is GPLv2 with the Classpath Exception; its licence texts
# stay in the image (legal/).
#
# Where it comes from: Termux's openjdk-25 package (the OpenJDK source built for Android with the
# NDK, bionic libc) supplies the aarch64 jmods and the libraries they need; jlink of a matching
# Temurin 25 for this computer links them into an image (jlink can link another platform's
# jmods when the build matches). Termux's libandroid-spawn and zlib supply what bionic lacks
# (libz.so.1, posix_spawn helpers); libc++_shared comes from the NDK.
#
#   packaging/quest/build_jre.sh
#
# Needs curl, ar, tar, xz, zip, patchelf and the NDK (env.sh). The result is
# lib/server/libjvm.so, lib/*.so, lib/modules, conf/ ...: horizon-jvm (the launcher, built
# with the app) opens it. Measured size, 2026-11-08: 48 MB on disk, 28 MB zipped.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
# shellcheck source=env.sh
source "$here/env.sh"
OUT=${OUT:-$root/target/quest}
cache=$OUT/cache/jre
jre=$OUT/jre
mkdir -p "$cache" "$OUT/assets"

TERMUX=https://packages.termux.dev/apt/termux-main/pool/main
TEMURIN_URL=https://github.com/adoptium/temurin25-binaries/releases/download/jdk-25.0.5%2B8/OpenJDK25U-jdk_x64_linux_hotspot_25.0.5_8.tar.gz
TEMURIN_SHA256=68982529869257d0ee6b824ec5aa270ff8b118bb438670da0b97ebdab178d7fb
# name|path under $TERMUX|sha256
DEBS=(
  "openjdk-25.deb|o/openjdk-25/openjdk-25_25.0.5-2_aarch64.deb|a3253f29515c50a67ad5296c97b8dea91440d74f90c0cf2171d27b9d367992ff"
  "libandroid-spawn.deb|liba/libandroid-spawn/libandroid-spawn_0.3_aarch64.deb|7988fa788ef48ab5da9660443905a2e4099ac36221739d72f5c39acc644b4d1c"
  "zlib.deb|z/zlib/zlib_1.4_aarch64.deb|e3027565ae50bcd8ba5113c988b589754ae192c9cc3e6afbe3e4ed7c06b2598e"
)
# Modules the exporters, the data generator and the server code need (checked with the real
# 26.3 jars: jdeps, then a full preparation on a Quest 3S).
MODULES=java.base,java.logging,java.management,java.naming,java.net.http,java.sql,java.xml,jdk.unsupported,jdk.zipfs,jdk.charsets,jdk.crypto.ec,jdk.net,jdk.management,jdk.jfr

for tool in curl ar tar xz zip patchelf; do
    command -v "$tool" >/dev/null || { echo "missing $tool" >&2; exit 1; }
done

fetch() { # url sha256 file
    if [ ! -f "$3" ]; then
        curl -fsSL -o "$3.part" "$1"
        mv "$3.part" "$3"
    fi
    echo "$2  $3" | sha256sum -c --quiet - || { echo "checksum mismatch: $3" >&2; rm -f "$3"; exit 1; }
}

echo "== Temurin 25 (runs jlink here)"
fetch "$TEMURIN_URL" "$TEMURIN_SHA256" "$cache/temurin25.tar.gz"
[ -d "$cache/temurin" ] || { mkdir -p "$cache/temurin.part" && tar -xzf "$cache/temurin25.tar.gz" -C "$cache/temurin.part" && mv "$cache/temurin.part" "$cache/temurin"; }
jdk=$(ls -d "$cache"/temurin/jdk-25*)

echo "== Termux packages (aarch64)"
rm -rf "$cache/unpacked"; mkdir -p "$cache/unpacked"
for entry in "${DEBS[@]}"; do
    IFS='|' read -r name path sha <<<"$entry"
    fetch "$TERMUX/$path" "$sha" "$cache/$name"
    d=$cache/unpacked/${name%.deb}; mkdir -p "$d"
    (cd "$d" && ar x "$cache/$name" && tar -xf data.tar.*)
done
termux_usr=data/data/com.termux/files/usr
jdk_target=$(ls -d "$cache"/unpacked/openjdk-25/$termux_usr/lib/jvm/java-25-openjdk)

echo "== jlink"
rm -rf "$jre"
"$jdk/bin/jlink" --module-path "$jdk_target/jmods" --add-modules "$MODULES" \
    --no-header-files --no-man-pages --compress zip-6 --output "$jre"
# The host's objcopy cannot strip aarch64 files: the NDK's does.
while IFS= read -r -d '' so; do "$NDK_BIN/llvm-strip" --strip-unneeded "$so"; done < <(find "$jre/lib" -name '*.so' -print0)
cp "$cache/unpacked/libandroid-spawn/$termux_usr/lib/libandroid-spawn.so" "$jre/lib/"
cp "$cache/unpacked/zlib/$termux_usr/lib/libz.so.1.4" "$jre/lib/libz.so.1"
cp "$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/sysroot/usr/lib/aarch64-linux-android/libc++_shared.so" "$jre/lib/"
"$NDK_BIN/llvm-strip" --strip-unneeded "$jre/lib/libc++_shared.so"
# Termux's libraries look for each other in its own prefix: point them at their own folder.
while IFS= read -r -d '' so; do patchelf --set-rpath '$ORIGIN:$ORIGIN/..' "$so"; done < <(find "$jre/lib" -name '*.so*' -print0)
# The JDK's own launcher is not used (horizon-jvm is): drop it and what only it needs.
rm -rf "$jre/bin"
rm -f "$jre/lib/jspawnhelper"

echo "== check"
file=$(find "$jre/lib/server" -name libjvm.so)
"$NDK_BIN/llvm-readelf" -h "$file" | grep -q AArch64 || { echo "libjvm is not aarch64" >&2; exit 1; }
for so in $(find "$jre/lib" -name '*.so*'); do
    for n in $("$NDK_BIN/llvm-readelf" -d "$so" | sed -n 's/.*(NEEDED).*\[\(.*\)\]/\1/p'); do
        case " libc.so libm.so libdl.so liblog.so " in *" $n "*) continue ;; esac
        # libjvm.so is opened first, by its path, so its name is already loaded for the rest.
        [ "$n" = libjvm.so ] && continue
        [ -e "$jre/lib/$n" ] || { echo "$so needs $n, which is not in the image" >&2; exit 1; }
    done
done

echo "== zip"
rm -f "$OUT/assets/horizon-jre.zip"
(cd "$jre" && zip -q -r -9 -y "$OUT/assets/horizon-jre.zip" .)
du -sh "$jre" "$OUT/assets/horizon-jre.zip"
