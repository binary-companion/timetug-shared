#!/bin/sh
# Resource baseline of the Swift engine on macOS or Linux (docs/rust-port/baselines.md): builds the engine library,
# builds dotnet/TimeTug.Engine.Bench with the library copied next to it, runs it and writes the JSON.
#   scripts/benchmarks/run-engine-bench.sh [--idle-seconds <n>] [--out <file>]
# Defaults: 600 seconds, build/bench/engine-bench.json. Needs the .NET 8 SDK or newer and a Swift toolchain.
set -eu
repo_dir=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
idle_seconds=600
out="$repo_dir/build/bench/engine-bench.json"
while [ $# -gt 0 ]; do
  case "$1" in
    --idle-seconds) idle_seconds="$2"; shift 2 ;;
    --out) out="$2"; shift 2 ;;
    *) echo "usage: $0 [--idle-seconds <n>] [--out <file>]" >&2; exit 2 ;;
  esac
done
case "$(uname -s)" in
  Darwin) library=libTimeTugEngineC.dylib ;;
  *) library=libTimeTugEngineC.so ;;
esac

swift build -c release --product TimeTugEngineC --package-path "$repo_dir"
bin_dir=$(swift build -c release --package-path "$repo_dir" --show-bin-path)
native_dir="$repo_dir/build/bench/native"
rm -rf "$native_dir"
mkdir -p "$native_dir" "$(dirname -- "$out")"
cp "$bin_dir/$library" "$native_dir/"

dotnet build "$repo_dir/dotnet/TimeTug.Engine.Bench" -c Release "-p:TimeTugNativeDir=$native_dir"
env -u TIMETUG_ENGINE_LIBRARY dotnet "$repo_dir/dotnet/TimeTug.Engine.Bench/bin/Release/net8.0/TimeTug.Engine.Bench.dll" \
  --idle-seconds "$idle_seconds" --out "$out"
echo "wrote $out"
