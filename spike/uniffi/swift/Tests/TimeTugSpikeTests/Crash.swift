import Foundation
import Testing
import TimeTugSpike
@Suite struct Crash { @Test(.enabled(if: ProcessInfo.processInfo.environment["CRASH"] != nil)) func badZoneNonThrowing() { _ = echoZone(z: TimeZone(identifier: "Asia/Tokyo")!) } }
