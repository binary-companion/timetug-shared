set -e
L=${1:-target/debug/libtimetug_spike.a}; B=target/debug/uniffi-bindgen-swift
rm -rf out; $B $L out/Sources --swift-sources; $B $L out/Headers --headers; $B $L out/Modules --xcframework --modulemap --modulemap-filename module.modulemap
