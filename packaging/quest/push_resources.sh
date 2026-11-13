#!/usr/bin/env bash
# Copies the prepared game files (horizon-prepare's output from your own Minecraft 26.3)
# to the headset, where the app reads them: /sdcard/Android/data/dev.horizon.quest/files/resources.
#
#   packaging/quest/push_resources.sh                 # .local/horizon-client/26.3
#   packaging/quest/push_resources.sh /path/to/prepared/26.3
#   packaging/quest/push_resources.sh --launch "--name Steve"   # also write launch.txt
#
# Install the APK first (adb install -r target/quest/horizon-quest.apk): Android creates the
# app's data folder at install, and an uninstall deletes it with the pushed files.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
PKG=${PKG:-dev.horizon.quest}
dest=/sdcard/Android/data/$PKG/files
src=$root/.local/horizon-client/26.3
launch=

while [ $# -gt 0 ]; do
    case "$1" in
        --launch) launch=$2; shift 2 ;;
        -h|--help) sed -n '2,11p' "$0"; exit 0 ;;
        *) src=$1; shift ;;
    esac
done

if [ ! -f "$src/horizon-resources.json" ]; then
    echo "no horizon-resources.json in $src" >&2
    echo "prepare them on this computer first: cargo run --release -p horizon-prepare -- --prepare --server-jar ~/Downloads/server.jar" >&2
    exit 1
fi
adb get-state >/dev/null || { echo "no device: connect the headset (developer mode, USB debugging)" >&2; exit 1; }

echo "== $src -> $dest/resources"
adb shell mkdir -p "$dest/resources" "$dest/home"
# One tar through /data/local/tmp, unpacked by the device's shell: `adb push` of a folder into
# Android/data fails on Quest ("remote secure_mkdirs failed: Operation not permitted"), and one
# file is much faster than thousands over Wi-Fi adb anyway.
tmp=/data/local/tmp/horizon-resources.tar
tar -C "$src" -cf - . | adb shell "cat > $tmp"
adb shell "rm -rf $dest/resources && mkdir -p $dest/resources && tar -xf $tmp -C $dest/resources && rm $tmp"
if [ -n "$launch" ]; then
    # One argument per line.
    printf '%s\n' $launch | adb shell "cat > $dest/launch.txt"
    echo "launch.txt: $(adb shell cat "$dest/launch.txt" | tr '\n' ' ')"
fi
echo "== done; start Horizon from the headset's library (Unknown sources) or:"
echo "   adb shell am start -n $PKG/android.app.NativeActivity"
