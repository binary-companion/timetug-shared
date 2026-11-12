#!/usr/bin/env bash
# Builds the TimeTugEngineC library, compiles c-harness/harness.c against include/timetug_engine.h with the system C
# compiler and runs the concurrency harness (11 cases, 200 iterations each by default).
# Usage: c-harness/run.sh [--rust] [harness arguments, for example --iterations 20 --case stale_handles]
# The library is the Rust one (cargo build --release -p timetug-engine-c), taken from target/release. --rust is
# accepted and changes nothing: it was the switch while the Swift library still existed.
# macOS and Linux; Windows uses scripts/ci/run-c-harness.ps1. TT_HARNESS_SANITIZE=thread|address adds the matching
# -fsanitize flag to the harness (the library is not instrumented by it).
set -euo pipefail
cd "$(dirname "$0")/.."
case "${1:-}" in
  --rust) shift ;;
esac
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

cargo build --release --locked -p timetug-engine-c
bin="${CARGO_TARGET_DIR:-$PWD/target}/release"
case "$(uname -s)" in
  Darwin) library="$bin/libTimeTugEngineC.dylib" ;;
  *) library="$bin/libTimeTugEngineC.so" ;;
esac
[ -f "$library" ] || { echo "error: $library not found" >&2; exit 1; }

cc="${CC:-$(command -v clang || command -v cc)}"
flags=(-std=c11 -Wall -Wextra -Werror -O1 -g -I include c-harness/harness.c -L "$bin" -lTimeTugEngineC -Wl,-rpath,"$bin" -pthread)
if [ -n "${TT_HARNESS_SANITIZE:-}" ]; then flags+=("-fsanitize=$TT_HARNESS_SANITIZE"); fi
"$cc" "${flags[@]}" -o "$work/harness"
"$work/harness" "$work/data" "$@"
