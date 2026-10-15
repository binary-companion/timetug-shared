import CalendarCore
import Foundation
import TimeTugCore
import TimeTugEngine
@testable import TimeTugEngineProtocol

/// The engine-to-host messages the golden fixtures pin, built from fixed engine values. A fixture file is the
/// reviewed JSON of exactly one of these; the test fails if the mapping drifts from it.
enum GoldenMessages {
    static let base = Date(timeIntervalSince1970: 1_760_000_000)   // 2025-10-09T08:53:20Z

    static var calendar: Calendar {
        var c = Calendar(identifier: .gregorian)
        c.timeZone = TimeZone(identifier: "UTC")!
        return c
    }

    static var settings: EngineSettings {
        var s = EngineSettings()
        s.takeover.leadTime = 120
        s.takeover.takeoverCalendarKeys = ["google-1/primary"]
        s.takeover.hiddenCalendarKeys = ["google-1/holidays"]
        s.inferenceEnabled = true
        return s
    }

    static var standup: TimeTugCalendarEvent {
        var e = Fixture.timed("evt-1", title: "Standup", start: base.addingTimeInterval(1800), minutes: 15,
                              calendarID: "primary", source: "google-1", conference: "https://meet.google.com/abc-defg-hij")
        e.otherAttendeeCount = 4
        e.responseStatus = .accepted
        return e
    }

    static var merged: TimeTugCalendarEvent {
        var e = Fixture.timed("evt-2", title: "Design review", start: base.addingTimeInterval(7200), minutes: 60,
                              calendarID: "primary", source: "google-1")
        e.additionalCalendarKeys = ["caldav-1/work"]
        e.displayStart = e.start.addingTimeInterval(-300)
        e.mergedMembers = [
            MergedMember(title: "Design review", calendarKey: "google-1/primary",
                         contentKey: "design review|1760007200|1760010800", details: "location",
                         start: e.start, end: e.end),
            MergedMember(title: "Design Review", calendarKey: "caldav-1/work",
                         contentKey: "design review|1760006900|1760010500", details: "bare",
                         start: e.start.addingTimeInterval(-300), end: e.end.addingTimeInterval(-300)),
        ]
        e.mergeProvenance = .inference(engineID: "foundation-models", engineName: "Apple Intelligence")
        return e
    }

    static var holiday: TimeTugCalendarEvent { Fixture.allDay("evt-3") }

    static var state: EngineState {
        let events = [standup, merged, holiday]
        let ledgerEvent = events[0]
        var ledger = TakeoverLedger()
        ledger.markFired(ledgerEvent, now: base)
        var state = EngineState.empty
        state.revision = 12
        state.events = events
        state.agenda = DayAgenda.make(events: events, settings: settings.takeover, now: base, calendar: calendar)
        state.calendars = [
            CalendarInfo(sourceID: "google-1", calendarID: "primary", title: "Work", accountName: "me@example.com",
                         colorHex: "#3366cc", service: .google, provider: .google),
            CalendarInfo(sourceID: "google-1", calendarID: "holidays", title: "Holidays", kind: .subscribed,
                         service: .google, provider: .google),
        ]
        state.statuses = [
            "google-1": .ok, "caldav-1": .failing("Server unreachable"),
            "ms-1": .authExpired, "eventkit": .needsPermission,
        ]
        state.notices = ["google-1": [CalendarCore.SourceNotice(kind: .unreadableRecurrence, count: 2)]]
        state.sourceNames = ["google-1": "me@example.com", "caldav-1": "Work CalDAV"]
        state.candidates = [events[0].id: [events[1]]]
        state.accounts = [
            Connection(kindID: "google", connectionID: "1", displayName: "me@example.com"),
            Connection(kindID: "caldav", connectionID: "1", displayName: "Work CalDAV", config: ["server": "https://cal.example.com"]),
        ]
        var status = AccountManager.Status.empty
        status.accounts = state.accounts
        status.isWorking = true
        status.buildFailures = ["2": "Could not connect"]
        status.waitingStyle = .inApp
        state.accountStatus = status
        state.settings = settings
        state.inferenceStatus = .active(EngineInfo(id: "foundation-models", displayName: "Apple Intelligence", isOnDevice: true))
        state.ledger = ledger
        state.nextTakeover = Scheduler.next(
            events: events, settings: {
                var t = settings.takeover
                t.takeoverCalendarKeys = ["google-1/primary"]
                t.hiddenCalendarKeys = []
                return t
            }(), ledger: TakeoverLedger(), now: base)
        state.takeoverVisible = true
        state.availableAccountKinds = [
            AccountKindInfo(id: "google", displayName: "Google", authorization: .oauth, isConfigured: true),
            AccountKindInfo(id: "microsoft", displayName: "Microsoft", authorization: .oauth, isConfigured: false),
            AccountKindInfo(id: "caldav", displayName: "Other CalDAV", authorization: .password, isConfigured: true),
        ]
        return state
    }

    static var widget: WidgetSnapshot {
        WidgetSnapshot.make(
            events: [standup, holiday], calendars: state.calendars,
            settings: { var t = TakeoverSettings(); t.skipAllDayEvents = false; return t }(),
            now: base, calendar: calendar)
    }

    static var ready: Envelope {
        Envelope(type: "ready", payload: JSONValue(encoding: WireReady(
            engineVersion: "1.4.0", platform: "windows",
            capabilities: .init(systemCalendar: false, textGeneration: false))))
    }

    /// File name (without `.json`) to message.
    static var all: [String: Envelope] {
        let request = TakeoverRequest.make(for: standup, now: base.addingTimeInterval(1750))
        return [
            "ready": ready,
            "state": wireMessage(for: state),
            "event.takeoverDue": wireMessage(for: .takeoverDue(request, calendarTitle: "Work")),
            "event.takeoverWithdrawn": wireMessage(for: .takeoverWithdrawn(eventID: standup.id)),
            "event.widgetSnapshot": wireMessage(for: .widgetSnapshot(widget)),
            "event.joinRequested": wireMessage(for: .joinRequested(URL(string: "https://meet.google.com/abc-defg-hij")!)),
            "event.diagnostic": wireMessage(for: .diagnostic(
                EngineDiagnostic(level: .warning, category: "settings", message: "settings.json unreadable; using defaults"))),
            "edge-state-empty": wireMessage(for: .empty),
            "edge-state-all-day-only": wireMessage(for: {
                var s = EngineState.empty
                s.events = [holiday]
                s.agenda = DayAgenda.make(events: [holiday], settings: TakeoverSettings(), now: base, calendar: calendar)
                s.revision = 1
                return s
            }()),
            "edge-event-diagnostic-echoes-id": wireDiagnostic(
                EngineDiagnostic(level: .warning, category: "command", message: "unknown event id"), id: .string("cmd-9")),
            "edge-state-sources-each-status": wireMessage(for: {
                var s = EngineState.empty
                s.statuses = ["a": .ok, "b": .needsPermission, "c": .authExpired, "d": .failing("HTTP 503")]
                s.inferenceStatus = .unavailable(reason: "model not ready")
                return s
            }()),
        ]
    }
}
