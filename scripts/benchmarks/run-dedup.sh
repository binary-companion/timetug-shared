#!/bin/sh
# Dedup benchmark. Default: the Swift executable. `--rust` (first argument): the Rust port.
# Both print `shape,count,median_ms,output_events` and accept the same optional `shape [count]`.
set -eu
repo_dir=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
if [ "${1:-}" = "--rust" ]; then
  shift
  exec cargo run --release --quiet --manifest-path "$repo_dir/Cargo.toml" -p dedup-benchmark -- "$@"
fi
swift run -c release --package-path "$repo_dir" DedupBenchmark "$@"
