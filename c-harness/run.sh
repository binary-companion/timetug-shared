#!/usr/bin/env bash
# Builds the TimeTugEngineC library, compiles c-harness/harness.c against include/timetug_engine.h with the system C
# compiler and runs the concurrency harness (11 cases, 200 iterations each by default).
# Usage: c-harness/run.sh (--swift | --rust) [harness arguments, for example --iterations 20 --case stale_handles]
#   --swift  the Swift library (swift build; exists on v2 until the Swift sources leave)
#   --rust   the Rust library (cargo build --release -p timetug-engine-c; target/release)
# macOS and Linux; Windows uses scripts/ci/run-c-harness.ps1. TT_HARNESS_SANITIZE=thread|address adds the matching
# -fsanitize flag to the harness (the library is not instrumented by it).
set -euo pipefail
cd "$(dirname "$0")/.."
mode=""
case "${1:-}" in
  --swift) mode=swift; shift ;;
  --rust) mode=rust; shift ;;
  *) echo "usage: c-harness/run.sh (--swift | --rust) [harness arguments]" >&2; exit 2 ;;
esac
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

if [ "$mode" = swift ]; then
  configuration="${CONFIGURATION:-release}"
  swift build -c "$configuration" --product TimeTugEngineC
  bin="$(swift build -c "$configuration" --show-bin-path)"
else
  cargo build --release --locked -p timetug-engine-c
  bin="${CARGO_TARGET_DIR:-$PWD/target}/release"
fi
case "$(uname -s)" in
  Darwin) library="$bin/libTimeTugEngineC.dylib" ;;
  *) library="$bin/libTimeTugEngineC.so" ;;
esac
[ -f "$library" ] || { echo "error: $library not found" >&2; exit 1; }

cc="${CC:-$(command -v clang || command -v cc)}"
flags=(-std=c11 -Wall -Wextra -Werror -O1 -g -I include c-harness/harness.c -L "$bin" -lTimeTugEngineC -Wl,-rpath,"$bin" -pthread)
if [ -n "${TT_HARNESS_SANITIZE:-}" ]; then flags+=("-fsanitize=$TT_HARNESS_SANITIZE"); fi
if [ "$mode" = swift ] && [ "$(uname -s)" != Darwin ]; then
  # The library needs the Swift runtime; point the linker and the loader at the toolchain's copy.
  runtime="$(cd "$(dirname "$(readlink -f "$(command -v swift)")")/../lib/swift/linux" && pwd)"
  flags+=(-Wl,-rpath-link,"$runtime" -Wl,-rpath,"$runtime")
fi
"$cc" "${flags[@]}" -o "$work/harness"
"$work/harness" "$work/data" "$@"
