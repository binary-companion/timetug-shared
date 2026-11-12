#!/usr/bin/env bash
# Tests scripts/release/next-version.sh.
set -euo pipefail
cd "$(dirname "$0")/../../.."
S=scripts/release/next-version.sh
fail() { echo "FAIL: $*" >&2; exit 1; }
# expect <exit code> <stdout> <args...>
expect() {
  local want_code="$1" want_out="$2" out code=0; shift 2
  out="$("$S" "$@" 2>/dev/null)" || code=$?
  [ "$code" = "$want_code" ] || fail "$*: exit $code, wanted $want_code"
  [ "$out" = "$want_out" ] || fail "$*: printed '$out', wanted '$want_out'"
}
expect 0 1.0.1 --latest 1.0.0 --labels ""
expect 0 1.0.1 --latest 1.0.0 --labels "bug,docs"
expect 0 1.1.0 --latest 1.0.7 --labels "semver:minor"
expect 0 2.0.0 --latest 1.4.9 --labels "semver:major"
expect 0 2.0.0 --latest 1.4.9 --labels "semver:patch,semver:minor,semver:major"
expect 0 1.5.0 --latest 1.4.9 --labels "semver:patch,semver:minor"
expect 0 1.0.1 --latest 1.0.0 --labels "semver:Major,semver-minor,semver:"
expect 0 1.4.10 --latest 1.4.9 --labels ""
# Precedence: major beats minor whatever the order (GitHub lists labels alphabetically, so major comes first).
expect 0 2.0.0 --latest 1.4.9 --labels "semver:major,semver:minor"
expect 0 2.0.0 --latest 1.4.9 --labels "semver:minor,semver:major"
# A mistyped label is ignored and a valid one beside it still wins.
expect 0 1.5.0 --latest 1.4.9 --labels "semver:Major,semver:minor"
# Labels are never used as glob patterns, in the script's own directory or anywhere else.
expect 0 1.4.10 --latest 1.4.9 --labels "*"
expect 0 1.4.10 --latest 1.4.9 --labels "semver:*"
expect 0 3.0.0 --latest 1.0.0 --labels "semver:major" --override 3.0.0
expect 0 1.0.0 --latest "" --labels "" --override 1.0.0
expect 3 "" --latest "" --labels "semver:major"
expect 2 "" --latest 01.2.3 --labels ""
expect 2 "" --latest 1.2 --labels ""
expect 2 "" --latest v1.2.3 --labels ""
expect 2 "" --latest 1.2.3-rc1 --labels ""
expect 3 "" --override "" --latest ""
expect 2 "" --latest 1.0.0 --labels "" --override v1.0.1
expect 2 "" --latest 1.0.0 --labels "" --bogus x
# Labels are split on commas only; whitespace around a label is trimmed, whitespace inside one is not.
expect 0 1.0.1 --latest 1.0.0 --labels "needs semver:major review"
expect 0 1.0.1 --latest 1.0.0 --labels "needs semver:minor review,docs"
expect 0 1.0.1 --latest 1.0.0 --labels "semver:major extra"
expect 0 1.1.0 --latest 1.0.0 --labels " semver:minor "
expect 0 2.0.0 --latest 1.0.0 --labels "bug, semver:major ,docs"
expect 0 1.1.0 --latest 1.0.0 --labels "  semver:minor  ,  semver:patch"
# An override is never below the latest tag (a typo would make an immutable wrong tag and release, which the tag
# ruleset forbids deleting). Equal stays allowed (recovering a tag whose release is missing), higher and a first
# release (no latest) stay allowed. Components are compared as numbers, not strings.
expect 2 "" --latest 1.0.6 --labels "" --override 1.0.5
expect 2 "" --latest 1.0.6 --labels "" --override 0.9.0
expect 2 "" --latest 1.2.3 --labels "" --override 1.1.9
expect 2 "" --latest 2.0.0 --labels "" --override 1.9.9
expect 2 "" --latest 1.10.0 --labels "" --override 1.9.0
expect 2 "" --latest 1.0.10 --labels "semver:major" --override 1.0.9
expect 2 "" --latest 10.0.0 --labels "" --override 9.9.9
expect 2 "" --latest 1.2.3 --labels "" --override 0.0.0
expect 0 1.2.10 --latest 1.2.3 --labels "" --override 1.2.10
expect 0 10.0.0 --latest 9.0.0 --labels "" --override 10.0.0
expect 0 1.10.0 --latest 1.9.0 --labels "" --override 1.10.0
expect 0 1.0.6 --latest 1.0.6 --labels "" --override 1.0.6
expect 0 1.0.7 --latest 1.0.6 --labels "" --override 1.0.7
expect 0 1.1.0 --latest 1.0.6 --labels "" --override 1.1.0
expect 0 2.0.0 --latest 1.0.6 --labels "" --override 2.0.0
expect 0 0.9.0 --latest "" --labels "" --override 0.9.0
expect 0 99999999999999999999999.0.0 --latest 99999999999999999999998.0.0 --labels "" --override 99999999999999999999999.0.0
expect 2 "" --latest 99999999999999999999999.0.0 --labels "" --override 99999999999999999999998.0.0
# The comparison needs a well-formed latest tag, as the bump does.
expect 2 "" --latest v1.0.0 --labels "" --override 1.0.1
expect 2 "" --latest 1.0 --labels "" --override 1.0.1
# A flag given without a value is a usage error (exit 2) with a message, not a silent exit 1.
expect_usage_error() {
  local want_msg="$1" err code=0; shift
  err="$("$S" "$@" 2>&1 >/dev/null)" || code=$?
  [ "$code" = 2 ] || fail "$*: exit $code, wanted 2"
  case "$err" in *"$want_msg"*) ;; *) fail "$*: stderr '$err' does not mention '$want_msg'" ;; esac
}
expect_usage_error "--labels" --latest 1.0.0 --labels
expect_usage_error "--latest" --latest
expect_usage_error "--override" --latest 1.0.0 --labels "" --override
expect_usage_error "override 0.9.0 is lower than the latest tag 1.0.6" --latest 1.0.6 --labels "" --override 0.9.0
# The prerelease form (v2 only): --prerelease <id> prints 2.0.0-<id>. The id is dot-separated identifiers of
# [0-9A-Za-z-], numeric ones without a leading zero; labels are ignored; an override cannot be combined with it; an
# empty id means "not a prerelease"; and it is refused once a stable 2.x or higher release exists.
expect 0 2.0.0-alpha.1 --prerelease alpha.1
expect 0 2.0.0-alpha.1 --latest 1.8.1 --labels "semver:major" --prerelease alpha.1
expect 0 2.0.0-alpha.1 --latest "" --labels "" --prerelease alpha.1
expect 0 2.0.0-rc.12 --latest 1.9.9 --prerelease rc.12
expect 0 2.0.0-beta --prerelease beta
expect 0 2.0.0-alpha.0 --prerelease alpha.0
expect 0 2.0.0-x-y.7a --prerelease x-y.7a
expect 3 "" --prerelease "" --latest ""
expect 0 1.0.1 --prerelease "" --latest 1.0.0 --labels ""
expect 2 "" --prerelease alpha..1
expect 2 "" --prerelease .alpha
expect 2 "" --prerelease alpha.
expect 2 "" --prerelease alpha.01
expect 2 "" --prerelease 01
expect 2 "" --prerelease -
expect 2 "" --prerelease "alpha 1"
expect 2 "" --prerelease "alpha.1+build"
expect 2 "" --prerelease "alpha.1;rm"
expect 2 "" --prerelease "../x"
expect 2 "" --prerelease "*"
expect 2 "" --prerelease alpha.1 --override 2.0.0-alpha.1
expect 2 "" --prerelease alpha.1 --override 2.0.0
expect 2 "" --prerelease alpha.1 --latest 2.0.0
expect 2 "" --prerelease alpha.1 --latest 2.3.4
expect 2 "" --prerelease alpha.1 --latest 10.0.0
expect 2 "" --prerelease alpha.1 --latest v1.0.0
expect_usage_error "--prerelease" --prerelease
expect_usage_error "stable release 2.0.0 exists" --prerelease alpha.1 --latest 2.0.0
expect_usage_error "not a prerelease identifier" --prerelease alpha.01
expect_usage_error "cannot be combined" --prerelease alpha.1 --override 3.0.0
echo PASS
