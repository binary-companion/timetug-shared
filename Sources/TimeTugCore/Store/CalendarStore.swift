import Foundation

public struct CalendarSnapshot: Sendable {
    public let events: [CalendarEvent]
    public let calendars: [CalendarInfo]
    /// Keyed by `CalendarSource.id`.
    public let statuses: [String: SourceStatus]
    /// `CalendarSource.id` -> `displayName`, for front ends that show source problems.
    public let sourceNames: [String: String]
    public let fetchedAt: Date

    public static let empty = CalendarSnapshot(
        events: [], calendars: [], statuses: [:], sourceNames: [:], fetchedAt: .distantPast)
}

/// Merges events from all sources over the fetch window. A failing source keeps its last good
/// events so a network blip never causes a missed meeting.
public actor CalendarStore {
    /// Extra time past the lead time so events just after midnight are already loaded.
    public static let fetchBuffer: TimeInterval = 300

    private let sources: [any CalendarSource]
    private let calendar: Calendar
    private var lastEvents: [String: [CalendarEvent]] = [:]
    private var lastCalendars: [String: [CalendarInfo]] = [:]
    private var statuses: [String: SourceStatus] = [:]

    public init(sources: [any CalendarSource], calendar: Calendar = .current) {
        self.sources = sources
        self.calendar = calendar
    }

    /// Local midnight today through next local midnight + lead time + buffer.
    public func fetchWindow(now: Date, leadTime: TimeInterval) -> DateInterval {
        let dayStart = calendar.startOfDay(for: now)
        let nextDayStart = calendar.date(byAdding: .day, value: 1, to: dayStart)!
        return DateInterval(start: dayStart, end: nextDayStart.addingTimeInterval(leadTime + Self.fetchBuffer))
    }

    public func refresh(now: Date, leadTime: TimeInterval) async -> CalendarSnapshot {
        let window = fetchWindow(now: now, leadTime: leadTime)

        let results = await withTaskGroup(
            of: (String, Result<([CalendarInfo], [CalendarEvent]), Error>).self
        ) { group in
            for source in sources {
                group.addTask {
                    do {
                        let calendars = try await source.calendars()
                        let events = try await source.events(in: window)
                        return (source.id, .success((calendars, events)))
                    } catch {
                        return (source.id, .failure(error))
                    }
                }
            }
            var collected: [(String, Result<([CalendarInfo], [CalendarEvent]), Error>)] = []
            for await result in group { collected.append(result) }
            return collected
        }

        for (sourceID, result) in results {
            switch result {
            case .success(let (calendars, events)):
                lastCalendars[sourceID] = calendars
                lastEvents[sourceID] = events
                statuses[sourceID] = .ok
            case .failure(let error):
                switch error {
                case SourceError.needsPermission: statuses[sourceID] = .needsPermission
                case SourceError.authExpired: statuses[sourceID] = .authExpired
                default: statuses[sourceID] = .failing(String(describing: error))
                }
            }
        }

        return CalendarSnapshot(
            events: merged(within: window),
            calendars: sources.flatMap { lastCalendars[$0.id] ?? [] },
            statuses: statuses,
            sourceNames: Dictionary(uniqueKeysWithValues: sources.map { ($0.id, $0.displayName) }),
            fetchedAt: now
        )
    }

    private func merged(within window: DateInterval) -> [CalendarEvent] {
        var indexByKey: [String: Int] = [:]
        var result: [CalendarEvent] = []
        for source in sources {
            for event in lastEvents[source.id] ?? [] {
                guard event.end > window.start, event.start < window.end else { continue }
                let key = "\(event.title.lowercased())|\(event.start.timeIntervalSince1970)|\(event.end.timeIntervalSince1970)"
                if let index = indexByKey[key] {
                    // Same meeting on another calendar: keep the first copy but remember the
                    // other calendar (for takeover opt-in) and borrow any details it lacks.
                    if event.calendarKey != result[index].calendarKey {
                        result[index].additionalCalendarKeys.insert(event.calendarKey)
                    }
                    result[index].location = result[index].location ?? event.location
                    result[index].notes = result[index].notes ?? event.notes
                    result[index].url = result[index].url ?? event.url
                    result[index].conferenceURL = result[index].conferenceURL ?? event.conferenceURL
                } else {
                    indexByKey[key] = result.count
                    result.append(event)
                }
            }
        }
        for index in result.indices where result[index].conferenceURL == nil {
            result[index].conferenceURL = ConferenceLinkDetector.detect(
                location: result[index].location, url: result[index].url, notes: result[index].notes)
        }
        return result.sorted { ($0.start, $0.title) < ($1.start, $1.title) }
    }
}
