#!/usr/bin/env bash
# Decides whether a calendar-connectors release may be bumped here.
#
#   scripts/ci/bump-major-guard.sh <X.Y.Z>     (run from the repository root)
#
# Prints `skip` when the new major version is greater than the major version of the calendar-connectors pin in
# Package.resolved (a new major is adopted by hand, in a reviewed pull request: bump-resolved.py refuses it), otherwise
# `proceed`. Exits 2 with a message on stderr for a missing or malformed version, and 1 when the pin cannot be read.
# The pin is read through scripts/ci/resolved-version.py, the same reader the tests of the bump workflow use.
set -euo pipefail

usage() {
  echo "usage: bump-major-guard.sh X.Y.Z" >&2
  exit 2
}

[ "$#" -eq 1 ] || usage
version="$1"

# At most 9 digits per number, like bump-resolved.py, so the arithmetic below cannot overflow.
SEMVER='^(0|[1-9][0-9]{0,8})\.(0|[1-9][0-9]{0,8})\.(0|[1-9][0-9]{0,8})$'
if ! [[ "$version" =~ $SEMVER ]]; then
  echo "not X.Y.Z: $version" >&2
  exit 2
fi

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
current="$("$here/resolved-version.py" Package.resolved calendar-connectors)" || exit 1
if ! [[ "$current" =~ $SEMVER ]]; then
  echo "current pin is not X.Y.Z: $current" >&2
  exit 1
fi

cur_major="${current%%.*}"
new_major="${version%%.*}"
if (( new_major > cur_major )); then echo skip; else echo proceed; fi
