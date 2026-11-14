#!/usr/bin/env bash
# Generates the Swift and Kotlin bindings of timetug-engine-ffi into the repository (both are committed; CI fails when
# regenerating changes them).
# Usage: scripts/dev/generate-bindings.sh [--out-swift DIR] [--out-kotlin DIR] [--features FEATURES]
#   Swift:  swift/TimeTugKit/Sources/TimeTugKitGenerated/TimeTugKitGenerated.swift
#   Kotlin: kotlin/generated/
# The generator reads the built library, so the output does not depend on the profile or the target (checked: debug,
# release and iOS archives give the same bytes). The library is built for the host without optimisation.
set -euo pipefail
cd "$(dirname "$0")/../.."
swift_out="swift/TimeTugKit/Sources/TimeTugKitGenerated"
kotlin_out="kotlin/generated"
features=""
while [ $# -gt 0 ]; do
  case "$1" in
    --out-swift) swift_out="$2"; shift 2 ;;
    --out-kotlin) kotlin_out="$2"; shift 2 ;;
    --features) features="$2"; shift 2 ;;
    *) echo "unknown argument $1" >&2; exit 2 ;;
  esac
done
feature_args=()
[ -n "$features" ] && feature_args=(--features "$features")
cargo build --locked -p timetug-engine-ffi "${feature_args[@]}" --lib
cargo build --locked -p timetug-engine-ffi --features bindgen --bins
bin="${CARGO_TARGET_DIR:-$PWD/target}/debug"
case "$(uname -s)" in
  Darwin) cdylib="$bin/libtimetug_engine_ffi.dylib" ;;
  *) cdylib="$bin/libtimetug_engine_ffi.so" ;;
esac
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
mkdir -p "$swift_out"
"$bin/uniffi-bindgen-swift" "$bin/libtimetug_engine_ffi.a" "$work/swift" --swift-sources
cp "$work/swift/TimeTugKitGenerated.swift" "$swift_out/TimeTugKitGenerated.swift"
rm -rf "$kotlin_out"
mkdir -p "$kotlin_out"
"$bin/uniffi-bindgen" generate --library "$cdylib" --language kotlin --no-format --out-dir "$kotlin_out"
echo "generated: $swift_out and $kotlin_out"
