#!/usr/bin/env bash
# Builds build/TimeTugEngineFFI.xcframework: timetug-engine-ffi as a static library for macOS (arm64 and x86_64 joined with
# lipo), iOS and the iOS simulator (arm64 only), with the generated header and module map.
# Usage: scripts/dev/build-xcframework.sh [--release] [--features FEATURES] [--macos-only] [--output DIR]
#   --release     the release-apple profile (what ships); default is the dev profile
#   --features    cargo features of the library (the tests use test-clock)
#   --macos-only  only the host macOS slice (fast; what the Swift tests use)
#   --with-ios-sim  with --macos-only: also the iOS simulator slice (the iOS link check of the tests)
set -euo pipefail
cd "$(dirname "$0")/../.."
profile_args=(); profile_dir=debug
features=""; macos_only=0; with_ios_sim=0; output="build"
while [ $# -gt 0 ]; do
  case "$1" in
    --release) profile_args=(--profile release-apple); profile_dir=release-apple; shift ;;
    --features) features="$2"; shift 2 ;;
    --macos-only) macos_only=1; shift ;;
    --with-ios-sim) with_ios_sim=1; shift ;;
    --output) output="$2"; shift 2 ;;
    *) echo "unknown argument $1" >&2; exit 2 ;;
  esac
done
feature_args=()
[ -n "$features" ] && feature_args=(--features "$features")
target_dir="${CARGO_TARGET_DIR:-$PWD/target}"
lib=libtimetug_engine_ffi.a
build() { cargo build --locked -p timetug-engine-ffi --lib ${profile_args[@]+"${profile_args[@]}"} ${feature_args[@]+"${feature_args[@]}"} --target "$1"; }
host="aarch64-apple-darwin"
build "$host"
slices=("$host")
if [ "$macos_only" = 0 ]; then
  for target in x86_64-apple-darwin aarch64-apple-ios aarch64-apple-ios-sim; do build "$target"; slices+=("$target"); done
elif [ "$with_ios_sim" = 1 ]; then
  build aarch64-apple-ios-sim
fi
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/headers" "$work/macos"
generated="$target_dir/$host/$profile_dir/$lib"
cargo build --locked -p timetug-engine-ffi --features bindgen --bin uniffi-bindgen-swift
bindgen="$target_dir/debug/uniffi-bindgen-swift"
"$bindgen" "$generated" "$work/headers" --headers
"$bindgen" "$generated" "$work/headers" --modulemap --module-name TimeTugEngineFFI --modulemap-filename module.modulemap
macos_lib="$target_dir/$host/$profile_dir/$lib"
if [ "$macos_only" = 0 ]; then
  lipo -create "$target_dir/$host/$profile_dir/$lib" "$target_dir/x86_64-apple-darwin/$profile_dir/$lib" -output "$work/macos/$lib"
  macos_lib="$work/macos/$lib"
fi
rm -rf "$output/TimeTugEngineFFI.xcframework"
mkdir -p "$output"
args=(-library "$macos_lib" -headers "$work/headers")
if [ "$macos_only" = 0 ]; then
  args+=(-library "$target_dir/aarch64-apple-ios/$profile_dir/$lib" -headers "$work/headers")
  args+=(-library "$target_dir/aarch64-apple-ios-sim/$profile_dir/$lib" -headers "$work/headers")
elif [ "$with_ios_sim" = 1 ]; then
  args+=(-library "$target_dir/aarch64-apple-ios-sim/$profile_dir/$lib" -headers "$work/headers")
fi
xcodebuild -create-xcframework "${args[@]}" -output "$output/TimeTugEngineFFI.xcframework"
