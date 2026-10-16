#!/usr/bin/env bash
# Exports the baseline release's conformance files for the compat CI job, so the current tests can run against them.
# Usage: scripts/ci/compat/export-baseline.sh TAG OUT
#   OUT/golden     Conformance/Fixtures/golden at TAG (TT_BASELINE_FIXTURES)
#   OUT/scenarios  Conformance/Scenarios at TAG, data/ included (TT_BASELINE_SCENARIOS)
set -euo pipefail
[ $# -eq 2 ] || { echo "usage: $0 TAG OUT" >&2; exit 2; }
tag="$1" out="$2"
cd "$(dirname "$0")/../../.."
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
git archive --format=tar "$tag" -- Conformance/Fixtures/golden Conformance/Scenarios | tar -x -C "$work"
mkdir -p "$out"
rm -rf "$out/golden" "$out/scenarios"
mv "$work/Conformance/Fixtures/golden" "$out/golden"
mv "$work/Conformance/Scenarios" "$out/scenarios"
echo "exported $tag: $(find "$out/golden" -name '*.json' | wc -l | tr -d ' ') fixtures, $(find "$out/scenarios" -maxdepth 1 -name '*.json' | wc -l | tr -d ' ') scenarios"
