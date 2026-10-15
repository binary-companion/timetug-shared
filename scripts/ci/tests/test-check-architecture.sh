#!/usr/bin/env bash
# Tests check-architecture.sh: the repository passes, a fixture passes, each rule catches its violation.
set -euo pipefail
cd "$(dirname "$0")/../../.."
S="$PWD/scripts/ci/check-architecture.sh"
T="$(mktemp -d)"; trap 'rm -rf "$T"' EXIT
fail() { echo "FAIL: $*" >&2; exit 1; }
"$S" >/dev/null || { "$S" >&2; fail "the repository itself violates the rules"; }

mkdir -p "$T/Sources/TimeTugCore" "$T/Sources/CalendarBridge" "$T/Sources/DedupBenchmark" "$T/Tests/TimeTugCoreTests" "$T/Tests/CalendarBridgeTests" "$T/Sources/TimeTugEngine" "$T/Tests/TimeTugEngineTests"
printf '// swift-tools-version: 6.0\nlet package = Package(name: "x", dependencies: [.package(url: "https://github.com/binary-companion/calendar-connectors", from: "1.0.0")], targets: [])\n' > "$T/Package.swift"
printf '@preconcurrency import CalendarCore\nimport Foundation\nimport struct Foundation.Date\nfunc f(now: Date) -> Date { now }\n' > "$T/Sources/TimeTugCore/A.swift"
printf 'import CalendarCore\nimport Foundation\nimport TimeTugCore\n' > "$T/Sources/CalendarBridge/A.swift"
printf 'import Foundation\nimport TimeTugCore\n' > "$T/Sources/DedupBenchmark/main.swift"
printf 'import Testing\n@testable import TimeTugCore\n' > "$T/Tests/TimeTugCoreTests/T.swift"
printf 'import Testing\n@testable import CalendarBridge\n' > "$T/Tests/CalendarBridgeTests/T.swift"
printf 'import Foundation\nimport CalendarCore\nimport TimeTugCore\nimport CalendarBridge\n' > "$T/Sources/TimeTugEngine/A.swift"
printf 'import Testing\n@testable import TimeTugEngine\n' > "$T/Tests/TimeTugEngineTests/T.swift"
out="$("$S" "$T")" || fail "clean fixture rejected: $out"

# expect <file> <content> <message substring>
expect() {
  local f="$T/$1" had=0 out
  [ -e "$f" ] && { had=1; cp "$f" "$f.orig"; }
  mkdir -p "$(dirname "$f")"; printf '%b' "$2" > "$f"
  if out="$("$S" "$T" 2>/dev/null)"; then fail "accepted $1 containing: $2"; fi
  case "$out" in *"$3"*) ;; *) fail "for $1 expected '$3' in: $out" ;; esac
  if [ "$had" = 1 ]; then mv "$f.orig" "$f"; else rm "$f"; fi
}
C=Sources/TimeTugCore/X.swift
expect "$C" 'import AppKit\n' "TimeTugCore may not import AppKit"
expect "$C" 'public import AppKit\n' "TimeTugCore may not import AppKit"
expect "$C" '@testable import CalendarBridge\n' "TimeTugCore may not import CalendarBridge"
expect "$C" 'import GoogleCalendar\n' "TimeTugCore may not import GoogleCalendar"
expect "$C" 'let t = Date()\n' "reads the clock"
expect "$C" 'if Date.now > x {}\n' "reads the clock"
expect "$C" 'let t = Date(timeIntervalSinceNow: 5)\n' "reads the clock"
expect Tests/TimeTugCoreTests/X.swift 'import EventKit\n' "TimeTugCore may not import EventKit"
expect Sources/CalendarBridge/X.swift 'import SwiftUI\n' "CalendarBridge may not import SwiftUI"
expect Sources/CalendarBridge/X.swift 'import CalendarOAuth\n' "CalendarBridge may not import CalendarOAuth"
expect "$C" '@preconcurrency import SwiftUI\n' "TimeTugCore may not import SwiftUI"
expect "$C" 'import struct AppKit.NSColor\n' "TimeTugCore may not import AppKit"
expect "$C" 'internal import Security\n' "TimeTugCore may not import Security"
expect "$C" '@_exported import Combine\n' "TimeTugCore may not import Combine"
expect "$C" 'let a = 1\nlet t = Date()\n' "X.swift:2: TimeTugCore reads the clock"
expect Tests/CalendarBridgeTests/X.swift '@testable import SwiftUI\n' "CalendarBridge may not import SwiftUI"
expect Sources/TimeTugEngine/X.swift 'import AppKit\n' "TimeTugEngine may not import AppKit"
expect Tests/TimeTugEngineTests/X.swift 'import Combine\n' "TimeTugEngine may not import Combine"
expect Sources/TimeTugEngineProtocol/X.swift 'import Network\n' "TimeTugEngineProtocol may not import Network"
expect Tests/TimeTugEngineProtocolTests/X.swift 'import AppKit\n' "TimeTugEngineProtocol may not import AppKit"
expect Sources/DedupBenchmark/X.swift 'import AppKit\n' "DedupBenchmark may not import AppKit"
expect Sources/DedupBenchmark/X.swift 'import CalendarBridge\n' "DedupBenchmark may not import CalendarBridge"
expect Package.swift 'let package = Package(dependencies: [.package(url: "https://github.com/x/y", from: "1.0.0")])\n' \
  "may not depend on https://github.com/x/y"
expect Package.swift 'let package = Package(dependencies: [.package(path: "../CalendarConnectors")])\n' \
  "may not depend on ../CalendarConnectors"
expect Package.swift 'let package = Package(platforms: [], targets: [.target(name: "x", condition: .when(platforms: [.macOS]))])\n' \
  "platform conditions (.when)"
expect Package.swift 'let package = Package(dependencies: [.package(url: "https://github.com/binary-companion/calendar-connectors", from: "1.0.0"), .package(url: "https://github.com/x/y", from: "1.0.0")])\n' \
  "may not depend on https://github.com/x/y"
# Not clock reads: comments, a Date-typed value, a fixed instant.
printf '// let t = Date()\n/// Date.now is banned\nlet o: Date? = nil\nlet d = Date(timeIntervalSince1970: 0)\nlet x = now // Date()\nlet m = makeDate()\nlet n = Date.nowhere\n' > "$T/$C"
"$S" "$T" >/dev/null || fail "comments or fixed instants flagged as clock reads"
cp "$T/Package.swift" "$T/Package.swift.orig"
printf '// .when(platforms: [.macOS]) is only a comment\nlet package = Package(dependencies: [.package(url: "https://github.com/binary-companion/calendar-connectors", from: "1.0.0")])\n' > "$T/Package.swift"
"$S" "$T" >/dev/null || fail "a commented-out .when was flagged"
mv "$T/Package.swift.orig" "$T/Package.swift"
printf 'let t = Date() // architecture-check: allow (wall clock for logging only)\n' > "$T/$C"
"$S" "$T" >/dev/null || fail "allow marker ignored"
echo PASS
