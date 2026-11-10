#!/usr/bin/env bash
# Builds the VectorExporter and writes every golden vector file under vectors/ (or the directory given as the first
# argument). The Swift code at the baseline is the source of the vectors; see vectors/README.md. Run it again and
# `git diff --exit-code vectors/` must show nothing: the exporter is deterministic.
# The build uses -enable-testing because the exporter imports internals with @testable.
set -euo pipefail
cd "$(dirname "$0")/../.."
out="${1:-vectors}"
swift build -c release --product VectorExporter -Xswiftc -enable-testing
"$(swift build -c release --product VectorExporter -Xswiftc -enable-testing --show-bin-path)/VectorExporter" "$out"
