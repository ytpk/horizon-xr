#!/usr/bin/env bash
# Packs what horizon-prepare needs on the headset besides the JRE, as
# target/quest/assets/horizon-bundle.zip: research-lock.json, the precompiled exporter classes
# (exporters/: Horizon's own classes, built from mods/horizon/src/test/java/dev/horizonlab/parity,
# no Mojang code) and the exporter sources the preparer names (it hashes them for its stamps).
# No Mojang file goes in: the jars, libraries and assets are downloaded on the headset, after
# the player signs in.
#
#   packaging/quest/build_bundle.sh
#
# Needs the pinned jars imported once on this computer (the compile step links against them) and
# a JDK 25 (docs/launcher-preparation.md).
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
OUT=${OUT:-$root/target/quest}
parity=mods/horizon/src/test/java/dev/horizonlab/parity
stage=$OUT/bundle
mkdir -p "$OUT/assets"

(cd "$root" && cargo build --locked --release -p horizon-prepare)
prepare=${CARGO_TARGET_DIR:-$root/target}/release/horizon-prepare
echo "== compile-exporters"
folder=$("$prepare" compile-exporters --root "$root" | tail -n 1)
[ -f "$folder/manifest.json" ] || folder=$root/exporters
[ -f "$folder/manifest.json" ] || { echo "compile-exporters made no bundle ($folder)" >&2; exit 1; }

rm -rf "$stage"; mkdir -p "$stage/$parity"
cp -r "$folder" "$stage/exporters"
cp "$root/research-lock.json" "$stage/"
# The sources of the bundle's top-level classes, which the preparer's stamps hash.
(cd "$stage/exporters" && find . -name '*.class' ! -name '*$*' -print | sed 's#.*/##; s#\.class$#.java#' | sort -u) |
while read -r name; do cp "$root/$parity/$name" "$stage/$parity/"; done
# Horizon's own classes and sources only.
if find "$stage/exporters" -name '*.class' | grep -v '/dev/horizonlab/parity/' | grep -q .; then
    echo "a class outside dev/horizonlab/parity is in the bundle" >&2; exit 1
fi
rm -f "$OUT/assets/horizon-bundle.zip"
(cd "$stage" && zip -q -r -9 "$OUT/assets/horizon-bundle.zip" .)
du -sh "$OUT/assets/horizon-bundle.zip"
