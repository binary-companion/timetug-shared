import CalendarCore
import Foundation
@testable import TimeTugCore

/// Builders shared by the protocol tests. Times are fixed so expected strings are literals.
enum Fixture {
    static let utc = TimeZone(identifier: "UTC")!
    /// The default event zone: a named IANA zone, because Foundation spells the UTC zone "GMT" on some platforms.
    static let berlin = TimeZone(identifier: "Europe/Berlin")!

    static func timed(
        _ id: String = "e1", title: String = "Design review", start: Date = Date(timeIntervalSince1970: 1_760_000_000),
        minutes: Int = 30, calendarID: String = "cal", source: String = "fake", conference: String? = nil
    ) -> TimeTugCalendarEvent {
        var event = TimeTugCalendarEvent(
            event: CalendarCore.CalendarEvent(
                eventID: id, calendarID: calendarID, title: title, start: start,
                end: start.addingTimeInterval(TimeInterval(minutes * 60)), timeZone: berlin),
            sourceID: source)
        if let conference, let url = URL(string: conference) {
            event.conferences = [ConferenceInfo(url: url, provider: ConferenceDetector.provider(of: url) ?? .other)]
        }
        return event
    }

    /// 2026-10-16 as an all-day event in `zone` (canonical form: local midnight, end exclusive).
    static func allDay(_ id: String = "ad1", zone: TimeZone = berlin) -> TimeTugCalendarEvent {
        let first = CalendarDate(year: 2026, month: 10, day: 16)
        let (start, end) = AllDay.canonical(first: first, endExclusive: first.adding(days: 1), in: zone)!
        return TimeTugCalendarEvent(
            event: CalendarCore.CalendarEvent(
                eventID: id, calendarID: "cal", title: "Holiday", start: start, end: end, timeZone: zone, isAllDay: true),
            sourceID: "fake")
    }
}

/// Where the shared conformance files live: the package's `Conformance/` directory, found from this file's path
/// (SwiftPM resources cannot reach outside the target, and the C# tests read the same files).
enum Conformance {
    static let root: URL = {
        var url = URL(fileURLWithPath: #filePath)
        for _ in 0..<12 where url.lastPathComponent != "Tests" { url.deleteLastPathComponent() }
        return url.deletingLastPathComponent().appendingPathComponent("Conformance")
    }()
    static let currentGoldenDirectory = root.appendingPathComponent("Fixtures").appendingPathComponent("golden")

    /// `TT_BASELINE_FIXTURES=<dir>` points the fixture tests at an earlier release's golden fixtures (the `compat` CI
    /// job exports the baseline tag's `Conformance/Fixtures/golden` there): what an older front end sends and expects
    /// must still decode and round-trip with this code.
    static let baselineDirectory: URL? = {
        guard let path = ProcessInfo.processInfo.environment["TT_BASELINE_FIXTURES"], !path.isEmpty else { return nil }
        return URL(fileURLWithPath: path, isDirectory: true)
    }()
    static var isBaseline: Bool { baselineDirectory != nil }
    static var goldenDirectory: URL { baselineDirectory ?? currentGoldenDirectory }

    /// The fixtures being tested: the baseline's in baseline mode, else the current ones.
    static func goldenFiles() throws -> [String] {
        try FileManager.default.contentsOfDirectory(atPath: goldenDirectory.path).filter { $0.hasSuffix(".json") }.sorted()
    }

    /// One fixture. In baseline mode a name the baseline does not have (a fixture added since) is read from the
    /// current set, so tests that name a newer fixture still run; new fixtures are covered by the normal run.
    static func golden(_ name: String) throws -> Data {
        let file = goldenDirectory.appendingPathComponent(name)
        if isBaseline, !FileManager.default.fileExists(atPath: file.path) {
            return try Data(contentsOf: currentGoldenDirectory.appendingPathComponent(name))
        }
        return try Data(contentsOf: file)
    }
}
