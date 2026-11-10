#!/usr/bin/env bash
# Build the complete Quest APK using the existing packaging scripts.
set -euo pipefail
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
cd "$root"
case "${1:-}" in
    --help|-h)
        echo "Usage: sh build-apk.sh [--check]"
        echo "Configure .quest-build.env; see README.md for the one-time prerequisites."
        echo "--check validates the build tools without downloading or building anything."
        exit 0 ;;
    --check|'') ;;
    *) echo "Unknown argument: $1 (use --help)" >&2; exit 2 ;;
esac
if (( $# > 1 )); then echo "Only one argument is supported." >&2; exit 2; fi
if [[ -f .quest-build.env ]]; then
    # This is the user's local shell configuration, never part of the APK.
    set -a
    source ./.quest-build.env
    set +a
fi
# WSL's non-interactive launch may not read the profile that adds rustup to PATH.
if ! command -v cargo >/dev/null && [[ -f $HOME/.cargo/env ]]; then
    source "$HOME/.cargo/env"
fi
fail() { echo "Build prerequisite: $*" >&2; echo "See README.md > Build the APK." >&2; exit 1; }
[[ $(uname -s) == Linux && $(uname -m) == x86_64 ]] ||
    fail "use Linux x86-64, or Ubuntu under WSL2 on Windows (not Git Bash)."
for tool in bash cargo rustup java javac keytool curl unzip zip ar tar xz patchelf sha256sum; do
    command -v "$tool" >/dev/null || fail "missing $tool"
done
[[ $(javac -version 2>&1) == 'javac 25'* ]] || fail "JDK 25 must be on PATH."
java_version=$(java -version 2>&1)
[[ $java_version == *'version "25'* ]] || fail "java and javac must both use JDK 25."
rustup target list --installed | grep -qx aarch64-linux-android ||
    fail "run: rustup target add aarch64-linux-android"
: "${ANDROID_HOME:=$HOME/Android/Sdk}"
[[ -d $ANDROID_HOME ]] || fail "set ANDROID_HOME to a Linux Android SDK."
if [[ -z ${ANDROID_NDK_HOME:-} ]]; then
    compgen -G "$ANDROID_HOME/ndk/*" >/dev/null || fail "install the NDK or set ANDROID_NDK_HOME."
fi
source packaging/quest/env.sh
[[ -x $CC_aarch64_linux_android ]] || fail "Linux NDK compiler not found: $CC_aarch64_linux_android"
[[ -f $ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/sysroot/usr/lib/aarch64-linux-android/libc++_shared.so ]] ||
    fail "NDK libc++_shared.so not found."
if [[ -z ${BUILD_TOOLS:-} ]]; then
    compgen -G "$ANDROID_HOME/build-tools/*" >/dev/null || fail "install Android SDK build-tools 35.0.0."
    BUILD_TOOLS=$(find "$ANDROID_HOME/build-tools" -mindepth 1 -maxdepth 1 -type d | sort -V | tail -n 1)
fi
for tool in aapt2 zipalign apksigner; do
    [[ -x $BUILD_TOOLS/$tool ]] || fail "missing $BUILD_TOOLS/$tool"
done
if [[ -z ${PLATFORM_JAR:-} ]]; then
    PLATFORM_JAR=$ANDROID_HOME/platforms/android-34/android.jar
fi
[[ -f $PLATFORM_JAR ]] || fail "install platforms;android-34 or set PLATFORM_JAR."
export BUILD_TOOLS PLATFORM_JAR
if [[ -z ${HORIZON_MSA_CLIENT_ID:-} ]]; then
    echo "No Microsoft application client ID configured: the APK will build, but sign-in will be unavailable."
fi
if [[ ${1:-} == --check ]]; then
    echo "Build tool checks passed. Minecraft files, network downloads and APK compilation were not tested."
    exit 0
fi
export OUT=${OUT:-$root/target/quest}
# Import the user's own game files before compiling the original exporter bundle.
# The preparer reads HORIZON_MINECRAFT_DIR / HORIZON_SERVER_JAR when configured.
cargo run --locked --release -p horizon-prepare -- --prepare --root "$root"
bash packaging/quest/build_jre.sh
bash packaging/quest/build_bundle.sh
bash packaging/quest/build_apk.sh
sha256sum "$OUT/horizon-quest.apk" > "$OUT/horizon-quest.apk.sha256"
echo "Complete: $OUT/horizon-quest.apk"
