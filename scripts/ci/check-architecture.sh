#!/usr/bin/env bash
# Layering rules the compiler does not enforce. Usage: scripts/ci/check-architecture.sh [repo-root]
# Prints "path:line: message" per violation and exits 1 if there are any.
set -euo pipefail
cd "${1:-$(dirname "$0")/../..}"
bad=0
report() { echo "$1: $2"; bad=$((bad + 1)); }
has() { case " $1 " in *" $2 "*) return 0 ;; esac; return 1; }

PREFIX='(@[A-Za-z_]+(\([^)]*\))?[[:space:]]+|(public|package|internal|fileprivate|private)[[:space:]]+)*'
imports() {
  local p existing=()
  for p in "$@"; do [ -e "$p" ] && existing+=("$p"); done
  [ ${#existing[@]} -gt 0 ] || return 0
  grep -rnE --include='*.swift' "^[[:space:]]*${PREFIX}import[[:space:]]" "${existing[@]}" |
    sed -E "s/^([^:]+:[0-9]+):[[:space:]]*${PREFIX}import[[:space:]]+((typealias|struct|class|enum|protocol|let|var|func)[[:space:]]+)?([A-Za-z_][A-Za-z0-9_]*).*/\1 \7/" || true
}
only() {
  local label="$1" allowed="$2" loc mod; shift 2
  while read -r loc mod; do
    has "$allowed" "$mod" || report "$loc" "$label may not import $mod (allowed: $allowed)"
  done < <(imports "$@")
}

# Core is platform-neutral and builds on Linux and Windows: Foundation and the connector model only.
only TimeTugCore "Foundation CalendarCore TimeTugCore Testing" Sources/TimeTugCore Tests/TimeTugCoreTests
# The bridge is minimal glue between Core and the library.
only CalendarBridge "Foundation TimeTugCore CalendarCore CalendarBridge Testing" Sources/CalendarBridge Tests/CalendarBridgeTests
# The engine is portable orchestration over Core, the bridge and the connector model.
only TimeTugEngine "Foundation CalendarCore TimeTugCore CalendarBridge TimeTugEngine Testing" Sources/TimeTugEngine Tests/TimeTugEngineTests
only DedupBenchmark "Foundation CalendarCore TimeTugCore" Sources/DedupBenchmark

# Time is passed in: Core never reads the clock. Mark a justified exception with "architecture-check: allow".
if [ -d Sources/TimeTugCore ]; then
  while IFS= read -r hit; do
    report "$(echo "$hit" | cut -d: -f1,2)" "TimeTugCore reads the clock (Date()/Date.now); pass now: Date in instead"
  done < <(grep -rn --include='*.swift' 'Date' Sources/TimeTugCore | grep -v 'architecture-check: allow' |
    sed -E -e 's#^([^:]+:[0-9]+:)[[:space:]]*//.*#\1#' -e 's#[[:space:]]//.*##' |
    grep -E '^[^:]+:[0-9]+:(.*[^A-Za-z0-9_])?Date(\(\)|\.now([^A-Za-z0-9_]|$)|\(timeIntervalSinceNow)' || true)
fi

# One package dependency, the connector library by URL; no platform conditions.
if [ -f Package.swift ]; then
  LIBURL=https://github.com/binary-companion/calendar-connectors
  while IFS= read -r dep; do
    line="$(grep -nF "\"$dep\"" Package.swift | head -1 | cut -d: -f1)"
    [ "$dep" = "$LIBURL" ] || report "Package.swift:${line:-1}" "timetug-shared may not depend on $dep (allowed: $LIBURL)"
  done < <(tr '\n' ' ' < Package.swift | grep -oE '\.package\([^)]*\)' |
    sed -E 's/.*(path|url):[[:space:]]*"([^"]*)".*/\2/' || true)
  grep -n '\.when' Package.swift | grep -v '^[0-9]*:[[:space:]]*//' | while IFS=: read -r n _; do
    echo "Package.swift:$n: platform conditions (.when) are not allowed"
  done > "${TMPDIR:-/tmp}/arch-when.$$" || true
  if [ -s "${TMPDIR:-/tmp}/arch-when.$$" ]; then
    cat "${TMPDIR:-/tmp}/arch-when.$$"; bad=$((bad + $(wc -l < "${TMPDIR:-/tmp}/arch-when.$$")))
  fi
  rm -f "${TMPDIR:-/tmp}/arch-when.$$"
fi

[ "$bad" = 0 ] || { echo "$bad architecture violation(s)" >&2; exit 1; }
