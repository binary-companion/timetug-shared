set -euo pipefail
cd "$(dirname "$0")"
PROFILE=${1:-debug}
FLAGS=""; DIR=debug
if [ "$PROFILE" = release ]; then FLAGS="--release"; DIR=release; fi
cargo build $FLAGS --lib --target aarch64-apple-darwin
cargo build --features bindgen --bin uniffi-bindgen-swift
LIB=target/aarch64-apple-darwin/$DIR/libtimetug_spike.a
rm -rf out build; mkdir -p out/Headers out/Sources
target/debug/uniffi-bindgen-swift $LIB out/Sources --swift-sources
target/debug/uniffi-bindgen-swift $LIB out/Headers --headers
target/debug/uniffi-bindgen-swift $LIB out/Headers --modulemap --module-name TimeTugSpikeFFI --modulemap-filename module.modulemap
xcodebuild -create-xcframework -library $LIB -headers out/Headers -output build/TimeTugSpikeFFI.xcframework
mkdir -p swift/Sources/TimeTugSpike/Generated
cp out/Sources/TimeTugSpike.swift swift/Sources/TimeTugSpike/Generated/
ls -la $LIB
