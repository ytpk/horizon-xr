#!/bin/sh
# POSIX entry point: also works with `sh build-apk.sh`.
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
exec bash "$root/build-apk.bash" "$@"
