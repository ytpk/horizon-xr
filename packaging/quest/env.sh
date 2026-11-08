# Cross-compilation environment for the Quest build (aarch64-linux-android, API 32).
# Sourced by build_apk.sh; source it by hand to run cargo for the headset yourself.
: "${ANDROID_HOME:=$HOME/Android/Sdk}"
if [ -z "${ANDROID_NDK_HOME:-}" ]; then
    ANDROID_NDK_HOME=$(ls -d "$ANDROID_HOME"/ndk/* 2>/dev/null | sort -V | tail -n 1)
fi
: "${QUEST_API:=32}"
export ANDROID_HOME ANDROID_NDK_HOME QUEST_API
NDK_BIN="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin"
export NDK_BIN
export CC_aarch64_linux_android="$NDK_BIN/aarch64-linux-android$QUEST_API-clang"
export CXX_aarch64_linux_android="$NDK_BIN/aarch64-linux-android$QUEST_API-clang++"
export AR_aarch64_linux_android="$NDK_BIN/llvm-ar"
export RANLIB_aarch64_linux_android="$NDK_BIN/llvm-ranlib"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$CC_aarch64_linux_android"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_AR="$AR_aarch64_linux_android"
