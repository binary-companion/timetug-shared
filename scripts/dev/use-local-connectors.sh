#!/usr/bin/env bash
# Build against a local checkout of calendar-connectors instead of the pinned git revision.
#   scripts/dev/use-local-connectors.sh <path-to-calendar-connectors>   write .cargo/config.local.toml
#   scripts/dev/use-local-connectors.sh --restore                       delete it
# The tracked .cargo/config.toml includes .cargo/config.local.toml when it exists; the local file is git-ignored.
# Never commit a [patch] section: the rust-lint job of ci.yml fails if a tracked cargo config has one.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
config="$root/.cargo/config.local.toml"
crates=(calendar-core calendar-oauth calendar-ical calendar-google calendar-microsoft calendar-caldav
        calendar-ical-subscription calendar-test-support)

if [ "${1:-}" = "--restore" ]; then
  rm -f "$config"
  echo "Removed $config; Cargo uses the pinned calendar-connectors revision again."
  exit 0
fi

if [ $# -ne 1 ]; then
  echo "usage: $0 <path-to-calendar-connectors> | --restore" >&2
  exit 2
fi

if [ ! -d "$1" ]; then
  echo "error: $1 is not a directory" >&2
  exit 1
fi
connectors="$(cd "$1" && pwd -P)"
for name in "${crates[@]}"; do
  if [ ! -d "$connectors/crates/$name" ]; then
    echo "error: $connectors/crates/$name does not exist; is $connectors a calendar-connectors v2 checkout?" >&2
    exit 1
  fi
done

{
  echo "# Written by scripts/dev/use-local-connectors.sh; git-ignored. Remove it with --restore."
  echo '[patch."https://github.com/binary-companion/calendar-connectors"]'
  for name in "${crates[@]}"; do
    echo "$name = { path = \"$connectors/crates/$name\" }"
  done
} > "$config"
echo "Wrote $config; Cargo now builds the connector crates from $connectors."
