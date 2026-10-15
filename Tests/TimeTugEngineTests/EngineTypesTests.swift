import CalendarCore
import Foundation
import Testing
import TimeTugCore
@testable import TimeTugEngine

@Suite struct EngineTypesTests {
    private func event(_ id: String = "e1") -> TimeTugCalendarEvent {
        let start = Date(timeIntervalSince1970: 1_800_000_000)
        return TimeTugCalendarEvent(
            event: CalendarCore.CalendarEvent(
                eventID: id, calendarID: "c", title: "Standup", start: start, end: start.addingTimeInterval(1800)),
            sourceID: "s")
    }

    @Test func emptyStateIsTheZeroState() {
        let s = EngineState.empty
        #expect(s.revision == 0)
        #expect(s.agenda == .empty)
        #expect(s.events.isEmpty && s.calendars.isEmpty && s.statuses.isEmpty)
        #expect(s.notices.isEmpty && s.sourceNames.isEmpty && s.candidates.isEmpty && s.accounts.isEmpty)
        #expect(s.accountStatus == AccountManager.Status.empty)
        #expect(s.settings == EngineSettings())
        #expect(s.inferenceStatus == .disabled)
        #expect(s.ledger == TakeoverLedger())
        #expect(s.nextTakeover == nil)
        #expect(s.takeoverVisible == false)
    }

    @Test func everyCommandConstructs() {
        let e = event()
        let commands: [EngineCommand] = [
            .start, .refresh, .addAccount(kindID: "google"), .removeAccount(connectionID: "c"),
            .reauthorizeAccount(connectionID: "c"), .cancelAuthorization,
            .applySettings { $0.inferenceEnabled = true }, .settingsChangedExternally,
            .setCountdownVisible(true), .takeoverPresented(eventID: "e1", onScreen: false),
            .takeoverClosed(eventID: "e1"), .join(eventID: "e1"), .snooze(eventID: "e1", seconds: 60),
            .dismiss(eventID: "e1"), .testTug(title: "Sample", calendarTitle: "Calendar"), .merge(e, e), .unmerge(e), .separate(e, members: []),
            .forgetCorrections, .system(.wake), .system(.networkReturned), .stop,
        ]
        #expect(commands.count == 22)
        if case .applySettings(let change) = commands[6] {
            var settings = EngineSettings()
            change(&settings)
            #expect(settings.inferenceEnabled)
        } else {
            Issue.record("expected applySettings")
        }
    }

    @Test func everyEventConstructs() {
        let e = event()
        let events: [EngineEvent] = [
            .takeoverDue(TakeoverRequest.make(for: e, now: e.start), calendarTitle: nil),
            .takeoverWithdrawn(eventID: "e1"),
            .widgetSnapshot(WidgetSnapshot(generatedAt: e.start, events: [])),
            .joinRequested(URL(string: "https://example.com/j")!),
            .diagnostic(EngineDiagnostic(level: .info, category: "t", message: "m")),
        ]
        #expect(events.count == 5)
    }

    @Test func stateIsMutableValue() {
        var s = EngineState.empty
        s.revision = 3
        #expect(EngineState.empty.revision == 0)
        #expect(s.revision == 3)
    }
}
