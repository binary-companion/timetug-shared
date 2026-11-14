import Foundation
import Testing
import TimeTugKit

@Suite(.serialized) struct PerformanceTests {
    /// Review focus 1: a 500-event agenda behind every state change. The observer gets a summary, and the events cross the
    /// boundary only when they changed; one delivery of a state must stay under 5 ms in a release build.
    @Test func aStateDeliveryWithFiveHundredEventsStaysUnderFiveMilliseconds() async throws {
        let host = Host()
        host.http.feed.set(feed(events: 500, spacing: 60, notes: "Agenda item with some notes"))
        let engine = try makeEngine(host, directory: temporaryDirectory("perf"))
        await engine.start()
        await engine.send(command: .addAccount(kindId: "icalsub"))
        #expect(await waitUntil("500 events", timeout: 30) { engine.currentSummary().eventsRevision > 0 && engine.currentEvents().count == 500 })

        // The cost of fetching the events once (informational) and of one full state.
        let fetched = ContinuousClock().measure { _ = engine.currentEvents() }
        print("PERF fetch 500 events once:", fetched)

        // 200 state changes that do not touch the events: each is one observer call with a summary.
        let before = host.observer.summaries.get().count
        for index in 0..<200 {
            await engine.send(command: .applySettings(SettingsPatch(leadTime: 60 + Double(index % 2 + 1))))
        }
        #expect(await waitUntil("the deliveries") { host.observer.summaries.get().count >= before + 100 })
        let summaries = host.observer.summaries.get().suffix(from: before)
        let revisions = Set(summaries.map(\.eventsRevision))
        #expect(revisions.count == 1, "the events did not change, so their revision did not")
        // One delivery to the observer is the building of the summary in Rust, its lowering and its lifting in Swift; the
        // same three steps are timed here with `currentSummary()`, which returns the summary the engine holds.
        var durations: [Double] = []
        for _ in 0..<200 {
            let started = ContinuousClock.now
            _ = engine.currentSummary()
            let elapsed = ContinuousClock.now - started
            durations.append(Double(elapsed.components.seconds) * 1000 + Double(elapsed.components.attoseconds) / 1e15)
        }
        durations.sort()
        let median = durations[durations.count / 2]
        print("PERF summary of a 500-event state, median ms:", median)
        #expect(median < 5, "a state summary costs \(median) ms")
        engine.shutdown()
    }
}
