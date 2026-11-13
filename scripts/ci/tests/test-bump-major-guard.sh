#!/usr/bin/env bash
# Tests scripts/ci/bump-major-guard.sh in a throwaway tree whose Package.resolved pins calendar-connectors at
# 1.2.1. The guard prints proceed or skip (exit 0), or exits 2 for a bad argument and 1 when it cannot read the pin.
set -euo pipefail
cd "$(dirname "$0")/../../.."
G="$PWD/scripts/ci/bump-major-guard.sh"
RV="$PWD/scripts/ci/resolved-version.py"
T="$(mktemp -d)"; trap 'rm -rf "$T"' EXIT
fail() { echo "FAIL: $*" >&2; exit 1; }
mkdir -p "$T/scripts/ci"
cp "$G" "$RV" "$T/scripts/ci/" 2>/dev/null || true
printf '{"pins":[{"identity":"calendar-connectors","state":{"revision":"1111111111111111111111111111111111111111","version":"1.2.1"}}],"version":3}\n' > "$T/Package.resolved"

expect_out() { # version expected-output
  local out
  out="$(cd "$T" && bash scripts/ci/bump-major-guard.sh "$1")" || fail "$1: exited non-zero"
  [ "$out" = "$2" ] || fail "$1: expected '$2', got '$out'"
  echo "ok: $1 -> $2"
}
expect_status() { # expected-status version
  local status=0
  (cd "$T" && bash scripts/ci/bump-major-guard.sh "$2") >/dev/null 2>"$T/err" || status=$?
  [ "$status" = "$1" ] || fail "$2: expected exit $1, got $status"
  [ -s "$T/err" ] || fail "$2: expected a message on stderr"
  echo "ok: $2 -> exit $1"
}

expect_out 1.2.2 proceed
expect_out 1.3.0 proceed
expect_out 1.2.1 proceed
expect_out 2.0.0 skip
expect_out 3.1.4 skip
expect_status 2 2.0.0-alpha.1
expect_status 2 v2.0.0
expect_status 2 ''
expect_status 2 '1.0'
expect_status 2 99999999999999999999.0.0
# No argument at all.
status=0; (cd "$T" && bash scripts/ci/bump-major-guard.sh) >/dev/null 2>&1 || status=$?
[ "$status" = 2 ] || fail "no argument: expected exit 2, got $status"
# An unreadable pin (no Package.resolved) is an error, never a silent proceed.
mv "$T/Package.resolved" "$T/Package.resolved.off"
expect_status 1 2.0.0
mv "$T/Package.resolved.off" "$T/Package.resolved"
# A branch pin has no version: error.
printf '{"pins":[{"identity":"calendar-connectors","state":{"branch":"main","revision":"1"}}],"version":3}\n' > "$T/Package.resolved"
expect_status 1 2.0.0
echo "PASS: test-bump-major-guard"
