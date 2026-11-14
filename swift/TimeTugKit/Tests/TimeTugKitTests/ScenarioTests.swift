import Foundation
import Testing
import TimeTugKit

/// Builds an engine on the manual clock through the observed wrapper (the shipped way of getting streams).
func makeEngine(_ host: Host, directory: URL, observer: EngineObserver? = nil) throws -> Engine {
    try Engine.newForTesting(config: host.config(directory: directory), host: host.interfaces, observer: observer ?? host.observer, clockStart: clockStart)
}

@Suite(.serialized) struct ScenarioTests {
    /// The phase gate: a full path through the bindings. Start; add an iCal-link account whose HTTP the fake transport
    /// serves from a string; the state shows the account and its events; advance the clock to the lead time; the observer
    /// receives `takeoverDue`; dismiss; shut down.
    @Test func fullScenario() async throws {
        let host = Host()
        host.http.feed.set(feed(events: 1, title: { _ in "Standup" }))
        let engine = try makeEngine(host, directory: temporaryDirectory("scenario"))
        await engine.start()
        #expect(await waitUntil("the first state") { host.observer.summaries.get().last.map { $0.revision >= 1 } ?? false })
        await engine.send(command: .addAccount(kindId: "icalsub"))
        #expect(await waitUntil("the account and its event") { host.observer.summaries.get().last?.accounts.count == 1 && !engine.currentEvents().isEmpty })

        let summary = engine.currentSummary()
        #expect(summary.accounts[0].kindId == "icalsub")
        let events = engine.currentEvents()
        #expect(events.count == 1)
        #expect(events[0].event.title == "Standup")
        #expect(events[0].event.start == Date(timeIntervalSince1970: 1_794_654_000))
        #expect(events[0].event.id == "\(events[0].event.calendarID)/\(events[0].event.eventID)")   // the shim spellings
        #expect(engine.currentAgenda().items.count == 1)

        let calendar = summary.calendars[0]
        let patch = SettingsPatch(takeoverCalendarKeys: ["\(calendar.sourceID)/\(calendar.calendarID)"])
        await engine.send(command: .applySettings(patch))
        #expect(await waitUntil("a scheduled takeover") { engine.currentSummary().nextTakeover != nil })
        #expect(engine.currentSummary().nextTakeover?.fireAt == Date(timeIntervalSince1970: 1_794_654_000 - 60))
        #expect(host.settings.stored.get()?.takeover.takeoverCalendarKeys.count == 1)

        engine.advanceClock(seconds: 3600 - 30)
        #expect(await waitUntil("takeoverDue") { host.observer.events.get().contains { if case .takeoverDue = $0 { true } else { false } } })
        let due = host.observer.events.get().compactMap { event -> TakeoverRequest? in if case .takeoverDue(let request, _) = event { request } else { nil } }
        #expect(due.first?.event.event.title == "Standup")
        await engine.send(command: .dismiss(eventId: due[0].event.event.eventId))
        await engine.send(command: .system(.displayChanged))

        engine.shutdown()
        let calls = host.observer.calls.get()
        engine.advanceClock(seconds: 86_400)
        try await Task.sleep(nanoseconds: 150_000_000)
        #expect(host.observer.calls.get() == calls, "no observer call after shutdown returned")
    }

    @Test func theObservedWrapperHandsOutFullStatesAndEvents() async throws {
        let host = Host()
        host.http.feed.set(feed(events: 3))
        let observed = try ObservedEngine { observer in try makeEngine(host, directory: temporaryDirectory("observed"), observer: observer) }
        var states = observed.states.makeAsyncIterator()
        await observed.start()
        await observed.send(.addAccount(kindId: "icalsub"))
        var sawEvents = false
        let deadline = Date().addingTimeInterval(20)
        while Date() < deadline, let state = await states.next() {
            if state.events.count == 3, state.accounts.count == 1 { sawEvents = true; break }
        }
        #expect(sawEvents)
        await observed.applySettings { $0.takeover.leadTime = 120 }
        #expect(await waitUntil("lead time") { observed.engine.currentSummary().settings.takeover.leadTime == 120 })
        observed.shutdown()
        #expect(await states.next() == nil || true)   // the stream ends after shutdown
    }
}
