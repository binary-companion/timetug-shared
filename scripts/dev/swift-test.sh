#!/usr/bin/env bash
# Runs the Swift tests of TimeTugKit against a copy of the package whose generated Swift and library carry the cargo feature
# `test-clock` (Engine.newForTesting and advanceClock). The committed package has neither: they are off in every release
# build, so the tests cannot run against it.
# Usage: scripts/dev/swift-test.sh [--release] [--ios-sim-link] [swift test arguments]
#   --release        the release-apple profile and `swift test -c release` (the performance test needs it)
#   --ios-sim-link   instead of running tests, build the tests for the iOS simulator (arm64), which links the library
set -euo pipefail
cd "$(dirname "$0")/../.."
release=0; ios=0; extra=()
while [ $# -gt 0 ]; do
  case "$1" in
    --release) release=1; shift ;;
    --ios-sim-link) ios=1; shift ;;
    *) extra+=("$1"); shift ;;
  esac
done
work="$PWD/build/swift-test"
rm -rf "$work"
mkdir -p "$work"
rsync -a --exclude .build --exclude .swiftpm swift/TimeTugKit "$work/"
build_args=(--features test-clock)
[ "$release" = 1 ] && build_args+=(--release)
[ "$ios" = 1 ] && build_args+=(--with-ios-sim)
scripts/dev/generate-bindings.sh --features test-clock --out-swift "$work/TimeTugKit/Sources/TimeTugKitGenerated" --out-kotlin "$work/kotlin"
scripts/dev/build-xcframework.sh --macos-only "${build_args[@]}" --output "$work"
sed -i.bak 's|../../build/TimeTugEngineFFI.xcframework|../TimeTugEngineFFI.xcframework|' "$work/TimeTugKit/Package.swift"
rm -f "$work/TimeTugKit/Package.swift.bak"
cd "$work/TimeTugKit"
if [ "$ios" = 1 ]; then
  xcodebuild build-for-testing -scheme TimeTugKit -destination 'generic/platform=iOS Simulator' ARCHS=arm64 | tail -3
elif [ "$release" = 1 ]; then
  swift test -c release ${extra[@]+"${extra[@]}"}
else
  swift test ${extra[@]+"${extra[@]}"}
fi
