#!/bin/sh
# Dedup benchmark (the Rust port). Prints `shape,count,median_ms,output_events` and accepts an optional `shape [count]`.
# `--rust` (first argument) is accepted and changes nothing: it was the switch while the Swift executable existed.
set -eu
repo_dir=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
if [ "${1:-}" = "--rust" ]; then
  shift
fi
exec cargo run --release --quiet --manifest-path "$repo_dir/Cargo.toml" -p dedup-benchmark -- "$@"
