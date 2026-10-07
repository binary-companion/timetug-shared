#!/bin/sh
set -eu
repo_dir=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
swift run -c release --package-path "$repo_dir/Packages/TimeTugCore" DedupBenchmark
