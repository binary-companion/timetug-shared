#!/usr/bin/env bash
# Builds the TimeTugEngineC library, checks that it exports the four tt_engine_* functions, compiles
# c-smoke/smoke.c against include/timetug_engine.h with the system C compiler and runs it.
# Usage: c-smoke/run.sh [--rust]   (macOS and Linux; Windows uses scripts/ci/run-c-smoke.ps1)
#   default  the Swift library (swift build --product TimeTugEngineC).
#   --rust   the Rust library (cargo build --release -p timetug-engine-c), taken from target/release.
set -euo pipefail
cd "$(dirname "$0")/.."
mode=swift
case "${1:-}" in
  "") ;;
  --rust) mode=rust ;;
  *) echo "usage: c-smoke/run.sh [--rust]" >&2; exit 2 ;;
esac
if [ "$mode" = rust ]; then
  cargo build --release --locked -p timetug-engine-c
  bin="${CARGO_TARGET_DIR:-target}/release"
  bin="$(cd "$bin" && pwd)"
else
  configuration="${CONFIGURATION:-release}"
  swift build -c "$configuration" --product TimeTugEngineC
  bin="$(swift build -c "$configuration" --show-bin-path)"
fi
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

case "$(uname -s)" in
  Darwin)
    library="$bin/libTimeTugEngineC.dylib"
    exports="$(nm -gU "$library" | awk '{print $NF}' | sed 's/^_//')"
    ;;
  *)
    library="$bin/libTimeTugEngineC.so"
    exports="$(nm -D --defined-only "$library" | awk '{print $NF}')"
    ;;
esac
missing=0
for name in tt_engine_create tt_engine_send tt_engine_reply tt_engine_destroy; do
  if ! grep -qx "$name" <<<"$exports"; then
    echo "error: $library does not export $name" >&2
    missing=1
  fi
done
[ "$missing" = 0 ] || exit 1
echo "exports: tt_engine_create tt_engine_send tt_engine_reply tt_engine_destroy"

cc="${CC:-$(command -v clang || command -v cc)}"
flags=(-std=c11 -Wall -Wextra -Werror -I include c-smoke/smoke.c -L "$bin" -lTimeTugEngineC -Wl,-rpath,"$bin")
if [ "$mode" = swift ] && [ "$(uname -s)" != Darwin ]; then
  # The library needs the Swift runtime; point the linker and the loader at the toolchain's copy.
  runtime="$(cd "$(dirname "$(readlink -f "$(command -v swift)")")/../lib/swift/linux" && pwd)"
  flags+=(-Wl,-rpath-link,"$runtime" -Wl,-rpath,"$runtime")
fi
"$cc" "${flags[@]}" -o "$work/smoke"
"$work/smoke" "$work/data"
