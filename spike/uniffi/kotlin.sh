# Kotlin generation probe (no kotlinc on the machine, so generation only).
set -euo pipefail
cd "$(dirname "$0")"
cargo build --features bindgen --bins
cargo build
rm -rf out-kt
target/debug/uniffi-bindgen generate --library target/debug/libtimetug_spike.dylib --language kotlin --out-dir out-kt
wc -l out-kt/uniffi/timetug_spike/timetug_spike.kt
