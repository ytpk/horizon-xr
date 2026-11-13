#!/usr/bin/env bash
# Checks the tree, builds the Quest APK and writes it to dist/ with its SHA-256:
#   dist/horizon-quest-<version>-<commit>.apk and .apk.sha256
# Nothing lands in dist/ unless every check passes. An older APK with another name is kept.
#
#   packaging/quest/release.sh                # tests, build, verify, checksum
#   SKIP_TESTS=1 packaging/quest/release.sh   # build only
#   NO_MSA=1 packaging/quest/release.sh       # build with no Microsoft sign-in (not for players)
#
# Sign-in: by default the APK uses the Prismarine-compatible Live flow with prismarine-auth's
# public title ID (Minecraft for Nintendo Switch), built into horizon-account. A non-empty
# HORIZON_MSA_CLIENT_ID selects your own approved Azure application instead.
# dist/msa_client_id is no longer read: Horizon's registration must not be embedded, and the
# build fails if the ID in that file ends up in the library.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
cd "$root"
# shellcheck source=env.sh
source "$here/env.sh"

dist=$root/dist
out=$dist/out
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$dist/target}
test_target=$root/target

step() { printf '\n== %s\n' "$*"; }
die() {
    printf '\nFAILED: %s\n' "$*" >&2
    exit 1
}

mkdir -p "$dist"

prismarine_id=$(sed -n 's/^pub const DEFAULT_CLIENT_ID: &str = "\([^"]*\)";/\1/p' \
    crates/horizon-account/src/prismarine.rs)
[ -n "$prismarine_id" ] || die "no DEFAULT_CLIENT_ID in crates/horizon-account/src/prismarine.rs"
forbidden_id=
if [ -f "$dist/msa_client_id" ]; then
    forbidden_id=$(head -n 1 "$dist/msa_client_id" | tr -d '[:space:]')
fi

if [ "${NO_MSA:-0}" = 1 ]; then
    client_id=
    sign_in="disabled (NO_MSA=1)"
elif [ -n "${HORIZON_MSA_CLIENT_ID:-}" ]; then
    client_id=$(printf '%s' "$HORIZON_MSA_CLIENT_ID" | tr -d '[:space:]')
    [ "$client_id" != "$forbidden_id" ] || die "HORIZON_MSA_CLIENT_ID is the ID from dist/msa_client_id (Horizon's), which must not be used"
    sign_in="custom Azure application"
else
    client_id=$prismarine_id
    sign_in="Prismarine-compatible Live ($prismarine_id)"
fi
echo "Sign-in: $sign_in"

version=$(sed -n 's/.*android:versionName="\([^"]*\)".*/\1/p' "$here/AndroidManifest.xml" | head -n 1)
[ -n "$version" ] || die "no versionName in AndroidManifest.xml"
commit=$(git rev-parse --short=7 HEAD)
name=horizon-quest-$version-$commit
if ! git diff --quiet HEAD; then
    name=$name-dirty
    echo "WARNING: tracked files have uncommitted changes, the APK is named $name"
fi

# Cargo compares file dates: sources unpacked with a 1980 date look older than the last build,
# so it would silently reuse stale code.
step "Refreshing stale source dates"
find . \( -path ./target -o -path ./dist -o -path ./.git -o -path ./.local \) -prune \
    -o -type f ! -newermt 2000-01-01 -exec touch -c {} +

if [ "${SKIP_TESTS:-0}" != 1 ]; then
    step "Formatting and tree checks"
    cargo fmt --all -- --check
    git diff --check HEAD
    python3 tools/check_public_tree.py
    python3 - <<'EOF'
import json
for lang in ("en_us", "ru_ru"):
    with open(f"crates/horizon-client/assets/horizon/lang/{lang}.json", encoding="utf-8") as f:
        json.load(f)
print("language files parse")
EOF

    step "Input capture fixtures"
    python3 -m unittest discover -s tests -p test_world_input_capture.py

    step "VR library tests"
    CARGO_TARGET_DIR=$test_target cargo test --locked --no-default-features --lib \
        -p horizon-visor -p horizon-vivecraft -p horizon-xr

    step "Visor gameplay tests"
    CARGO_TARGET_DIR=$test_target cargo test --locked --no-default-features --lib \
        -p horizon-client visor

    step "Network tests"
    CARGO_TARGET_DIR=$test_target cargo test --locked --no-default-features --lib \
        -p horizon-client-lib net::
else
    echo "SKIP_TESTS=1: tests were not run"
fi

step "Building the exporter bundle"
# Always rebuilt: an old $out/assets/horizon-bundle.zip lacks exporters added since, and the
# headset's prepare then fails. compile-exporters is cached by its sources' key, so it is quick.
OUT=$out "$here/build_bundle.sh" || die "exporter bundle (needs the pinned jars imported: horizon-prepare --prepare)"
[ -f "$out/assets/horizon-jre.zip" ] || OUT=$out "$here/build_jre.sh" || die "Java runtime"

step "Building the APK"
log=$dist/$name.build.log
# Prismarine: leave the variable unset so horizon-account builds in its own default. NO_MSA:
# an empty value disables sign-in. Custom: the ID is baked in.
build() {
    if [ "$client_id" = "$prismarine_id" ]; then
        env -u HORIZON_MSA_CLIENT_ID OUT=$out "$here/build_apk.sh"
    else
        HORIZON_MSA_CLIENT_ID=$client_id OUT=$out "$here/build_apk.sh"
    fi
}
if ! build 2>&1 | tee "$log"; then
    die "APK build, see $log"
fi

step "Verifying the APK"
apk=$out/horizon-quest.apk
build_tools=$(ls -d "$ANDROID_HOME"/build-tools/* | sort -V | tail -n 1)
"$build_tools/apksigner" verify "$apk" 2>/dev/null || die "bad APK signature"
"$build_tools/zipalign" -c -P 16 4 "$apk" || die "APK is not aligned"
lib=$dist/$name.so.tmp
unzip -p "$apk" lib/arm64-v8a/libhorizon_quest.so >"$lib"
trap 'rm -f "$lib"' EXIT
if [ -n "$client_id" ]; then
    grep -a -c -F "$client_id" "$lib" >/dev/null || die "the Microsoft client ID is not in the library"
fi
if [ -n "$forbidden_id" ] && grep -a -c -F "$forbidden_id" "$lib" >/dev/null; then
    die "the library contains the ID from dist/msa_client_id (Horizon's registration)"
fi
echo "signature, alignment and sign-in ID are fine ($sign_in)"

step "Writing dist/"
cp "$apk" "$dist/$name.apk"
(cd "$dist" && sha256sum "$name.apk" >"$name.apk.sha256" && sha256sum -c "$name.apk.sha256")

printf '\nDONE (sign-in: %s)\n  %s\n  %s\n' "$sign_in" "$dist/$name.apk" "$(cat "$dist/$name.apk.sha256")"
