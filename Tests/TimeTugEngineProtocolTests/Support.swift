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
    static let goldenDirectory = root.appendingPathComponent("Fixtures").appendingPathComponent("golden")

    static func goldenFiles() throws -> [String] {
        try FileManager.default.contentsOfDirectory(atPath: goldenDirectory.path).filter { $0.hasSuffix(".json") }.sorted()
    }

    static func golden(_ name: String) throws -> Data {
        try Data(contentsOf: goldenDirectory.appendingPathComponent(name))
    }
}
