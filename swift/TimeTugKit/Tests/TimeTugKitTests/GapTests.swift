import Foundation
import Testing
import TimeTugKit

/// Collects what a host's `DiagnosticSink` receives.
final class CollectingSink: DiagnosticSink {
    let lines = Locked<[EngineDiagnostic]>([])
    let events = Locked<[DiagnosticEvent]>([])
    func log(diagnostic: EngineDiagnostic) { lines.update { $0.append(diagnostic) } }
    func record(event: DiagnosticEvent) { events.update { $0.append(event) } }
}

/// What phase 6 needs from the library beyond the types: the engine's own ids, the pending start, connector diagnostics and
/// the write checks.
@Suite(.serialized) struct GapTests {
    // M1: the shim formulas equal the engine's.
    @Test func theShimIdEqualsWhatTheEngineReports() async throws {
        let host = Host()
        host.http.feed.set(feed(events: 1, title: { _ in "Standup" }))
        let engine = try makeEngine(host, directory: temporaryDirectory("shim-id"))
        await engine.start()
        await engine.send(command: .addAccount(kindId: "icalsub"))
        #expect(await waitUntil("the event") { !engine.currentEvents().isEmpty })
        let event = engine.currentEvents()[0]
        let calendar = engine.currentSummary().calendars[0]
        // The settings key is the engine's `calendar_key`.
        #expect(event.calendarKey == "\(calendar.sourceID)/\(calendar.calendarID)" && event.calendarKey == calendar.key)
        await engine.send(command: .applySettings(SettingsPatch(takeoverCalendarKeys: [event.calendarKey])))
        #expect(await waitUntil("an armed takeover") { engine.currentSummary().nextTakeover != nil })
        // M9: the pending start is the start of the event the engine armed, and it survives the takeover being out.
        #expect(engine.currentSummary().nextPendingStart == event.start)

        // The engine names an event by its `id()` when it withdraws a takeover; switching takeovers off withdraws this one.
        await engine.send(command: .applySettings(SettingsPatch(enabled: false)))
        #expect(await waitUntil("the withdrawal") {
            host.observer.events.get().contains { if case .takeoverWithdrawn = $0 { true } else { false } }
        })
        let withdrawn = host.observer.events.get().compactMap { event -> String? in
            if case .takeoverWithdrawn(let id) = event { id } else { nil }
        }
        #expect(withdrawn == [event.id])
        #expect(event.id == "\(event.sourceID)/\(event.event.eventID)/1794654000")
        engine.shutdown()
    }

    // M9: the pending start stays while a takeover is on screen, the armed takeover does not.
    @Test func thePendingStartIsStillThereWhileATakeoverIsOnScreen() async throws {
        let host = Host()
        host.http.feed.set(feed(events: 2, spacing: 600, title: { "Meeting \($0)" }))
        let engine = try makeEngine(host, directory: temporaryDirectory("pending"))
        await engine.start()
        await engine.send(command: .addAccount(kindId: "icalsub"))
        #expect(await waitUntil("the events") { engine.currentEvents().count == 2 })
        let key = engine.currentEvents()[0].calendarKey
        await engine.send(command: .applySettings(SettingsPatch(takeoverCalendarKeys: [key])))
        #expect(await waitUntil("an armed takeover") { engine.currentSummary().nextTakeover != nil })
        let first = Date(timeIntervalSince1970: 1_794_654_000)
        #expect(engine.currentSummary().nextPendingStart == first)

        engine.advanceClock(seconds: 3600 - 30)
        #expect(await waitUntil("takeoverDue") { host.observer.events.get().contains { if case .takeoverDue = $0 { true } else { false } } })
        let request = host.observer.events.get().compactMap { event -> TakeoverRequest? in if case .takeoverDue(let request, _) = event { request } else { nil } }[0]
        await engine.send(command: .takeoverPresented(eventId: request.event.id, onScreen: true))
        #expect(await waitUntil("the takeover on screen") { engine.currentSummary().takeoverVisible })
        let summary = engine.currentSummary()
        #expect(summary.nextTakeover == nil)
        // The first event has fired, so the next one that qualifies is the second, ten minutes later.
        #expect(summary.nextPendingStart == first.addingTimeInterval(600))
        engine.shutdown()
    }

    // M10: connector events reach the sink with their private flags.
    @Test func connectorDiagnosticsReachTheHostsSink() async throws {
        let host = Host()
        host.http.feed.set(feed(events: 1))
        let sink = CollectingSink()
        var interfaces = host.interfaces
        interfaces.diagnostics = sink
        let engine = try Engine.newForTesting(
            config: host.config(directory: temporaryDirectory("diagnostics")), host: interfaces, observer: host.observer, clockStart: clockStart)
        await engine.start()
        await engine.send(command: .addAccount(kindId: "icalsub"))
        #expect(await waitUntil("a connector event") { !sink.events.get().isEmpty })
        let events = sink.events.get()
        #expect(events.allSatisfy { $0.category == "icalsub" })
        #expect(events.allSatisfy { $0.date == clockStart })   // the engine clock, not the wall clock
        #expect(events.flatMap(\.fields).allSatisfy { field in
            if case .string(let text) = field.value { !text.contains("Event ") } else { true }
        })
        engine.shutdown()
    }

    // M5: the write checks.
    private func timing(_ start: TimeInterval, _ end: TimeInterval, allDay: Bool = false) -> EventTiming {
        EventTiming(start: Date(timeIntervalSince1970: start), end: Date(timeIntervalSince1970: end), timeZone: TimeZone(identifier: "UTC"), isAllDay: allDay)
    }

    private func draft(_ timing: EventTiming) -> EventDraft {
        EventDraft(title: "Lunch", notes: nil, location: nil, timing: timing, availability: .busy, visibility: .default, reminders: nil, attendees: [], conference: .none, recurrence: nil, uid: nil)
    }

    private func rule(_ frequency: Frequency = .weekly) -> RecurrenceRule {
        RecurrenceRule(frequency: frequency, interval: 1, weekStart: .monday, weekdays: [], monthDays: [], months: [], yearDays: [], weekNumbers: [], setPositions: [], hours: [], minutes: [], seconds: [], end: .never, unrecognizedParts: [])
    }

    @Test func aDraftUsesTheFieldsItSets() {
        var value = draft(timing(1_700_000_000, 1_700_003_600))
        #expect(eventDraftUsedFields(draft: value) == [.title, .timing])
        value.notes = "n"
        value.recurrence = rule()
        value.attendees = [AttendeeDraft(name: nil, email: "a@example.com", role: .required)]
        #expect(eventDraftUsedFields(draft: value) == [.title, .timing, .notes, .recurrence, .attendees])
    }

    @Test func aPatchTouchesTheFieldsItChanges() {
        let before = CalendarEvent(eventID: "e", calendarID: "c", title: "A", start: Date(timeIntervalSince1970: 100), end: Date(timeIntervalSince1970: 200))
        var after = before
        #expect(eventPatchTouchedFields(patch: eventPatchDiff(original: before, edited: after)).isEmpty)
        after.title = "B"
        after.location = "Room"
        #expect(eventPatchTouchedFields(patch: eventPatchDiff(original: before, edited: after)) == [.title, .location])
    }

    @Test func validationThrowsTheWriteErrorOfTheLibrary() {
        #expect(throws: Never.self) { try validateEventDraft(draft: draft(timing(0, 60))) }
        #expect(throws: Never.self) { try validateEventTiming(timing: timing(0, 60)) }
        #expect(throws: Never.self) { try validateRecurrenceRule(rule: rule()) }
        #expect(throws: FfiError.Write(.invalid("end must be after start"))) { try validateEventDraft(draft: draft(timing(60, 0))) }
        #expect(throws: FfiError.Write(.invalid("end must be after start"))) { try validateEventTiming(timing: timing(60, 60)) }
        // An all-day timing must start and end at midnight in its zone.
        #expect(throws: FfiError.self) { try validateEventTiming(timing: timing(1_700_000_000, 1_700_086_400, allDay: true)) }
        #expect(throws: Never.self) { try validateEventTiming(timing: timing(1_699_920_000, 1_700_006_400, allDay: true)) }
        var bad = rule()
        bad.interval = 0
        #expect(throws: FfiError.Write(.invalid("recurrence interval must be at least 1"))) { try validateRecurrenceRule(rule: bad) }
        var daily = rule(.daily)
        daily.weekdays = [WeekdayOccurrence(.monday)]
        #expect(throws: FfiError.self) { try validateRecurrenceRule(rule: daily) }
        // The error is `Error`-conformant through `FfiError.Write`, so the adapter can catch `WriteError` from it.
        do { try validateEventTiming(timing: timing(60, 0)) } catch FfiError.Write(let error) { #expect(error == .invalid("end must be after start")) } catch { Issue.record("\(error)") }
    }
}
