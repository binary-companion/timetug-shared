# Size probes: archive size, strip versus debuginfo, linked executable (needs `bash build-xcf.sh release` first).
set -euo pipefail
cd "$(dirname "$0")"
cargo build --profile release-symbols --lib --target aarch64-apple-darwin
ls -l target/aarch64-apple-darwin/release-symbols/libtimetug_spike.a target/aarch64-apple-darwin/release/libtimetug_spike.a | awk '{print $5,$9}'
for p in release release-symbols; do echo "$p external symbols:"; nm target/aarch64-apple-darwin/$p/libtimetug_spike.a 2>/dev/null | grep -c " T " || true; done
mkdir -p sz && cd sz
echo 'print("hi")' > hello.swift && swiftc -O hello.swift -o hello
echo 'import Foundation
let e = makeAgenda(count: 1)[0]; print(echoEvent(e: e) == e)' > main.swift
swiftc -O -swift-version 5 -module-name TimeTugSpike -Xcc -fmodule-map-file=../out/Headers/module.modulemap -I ../out/Headers ../out/Sources/TimeTugSpike.swift main.swift ../target/aarch64-apple-darwin/release/libtimetug_spike.a -o linked
strip -x linked -o linked-stripped
ls -l hello linked linked-stripped | awk '{print $5,$9}'
