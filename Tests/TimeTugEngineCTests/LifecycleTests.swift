import Foundation
import Testing
@testable import TimeTugEngineC
import TimeTugEngineProtocol

/// The ABI rules of the spec, through the four C functions: creation, `ready`, rejected commands, destroy from
/// outside and inside a callback, stale handles, state coalescing under a slow host and engine isolation.
@Suite(.timeLimit(.minutes(1))) struct LifecycleTests {
    private func startedEngine(_ collector: MessageCollector, config: String = testConfig()) async throws -> Handle {
        let handle = try #require(createTestEngine(collector, config: config))
        send(handle, #"{"v":1,"type":"start"}"#)
        try #require(await poll("ready and a state") { collector.types.contains("state") })
        return handle
    }

    @Test func createReturnsNilForBadConfig() {
        let collector = MessageCollector()
        #expect(tt_engine_create("{not json", MessageCollector.callback, collector.context) == nil)
        #expect(tt_engine_create(nil, MessageCollector.callback, collector.context) == nil)
        #expect(tt_engine_create(#"{"v":2,"dataDirectory":"/tmp/x","platform":"linux"}"#, MessageCollector.callback, nil) == nil)
        #expect(tt_engine_create(#"{"v":1,"dataDirectory":"","platform":"linux"}"#, MessageCollector.callback, nil) == nil)
        #expect(tt_engine_create(testConfig(), nil, nil) == nil, "a missing callback is a bad config")
        #expect(collector.count == 0, "a failed create makes no callback")
    }

    @Test func readyThenStateAfterStart() async throws {
        let collector = MessageCollector()
        let handle = try #require(createTestEngine(collector))
        #expect(collector.count == 0, "nothing is delivered before start")
        send(handle, #"{"v":1,"type":"start"}"#)
        try #require(await poll("a state") { collector.types.contains("state") })
        let envelopes = collector.envelopes
        #expect(envelopes.first?.type == "ready")
        #expect(envelopes.filter { $0.type == "ready" }.count == 1)
        let ready = try #require(envelopes.first?.payload)
        guard case .string(let version)? = ready["engineVersion"] else { Issue.record("no engineVersion"); return }
        #expect(!version.isEmpty)
        #expect(ready["protocolVersions"] == .array([.number(1)]))
        #expect(ready["platform"] == .string("linux"))
        #expect(ready["capabilities"] == .object(["systemCalendar": .bool(false), "textGeneration": .bool(false)]))
        // A second start is the engine's to refuse: no second ready.
        send(handle, #"{"v":1,"type":"start","id":2}"#)
        try #require(await poll("the refusal") { collector.types.contains("event.diagnostic") })
        #expect(collector.types.filter { $0 == "ready" }.count == 1)
        await destroy(handle)
    }

    @Test func rejectedCommandDiagnosticEchoesTheCommandID() async throws {
        let collector = MessageCollector()
        let handle = try await startedEngine(collector)
        send(handle, #"{"v":1,"type":"nope","id":"cmd-9"}"#)
        send(handle, #"{"v":1,"type":"dedup.unmerge","id":42,"payload":{"eventID":"gone"}}"#)
        send(handle, "{not json")
        send(handle, #"{"v":2,"type":"refresh","id":"v2"}"#)
        try #require(await poll("four diagnostics") {
            collector.envelopes.filter { $0.type == "event.diagnostic" && $0.payload?["category"] == .string("command") }.count == 4
        })
        let diagnostics = collector.envelopes.filter { $0.type == "event.diagnostic" && $0.payload?["category"] == .string("command") }
        #expect(diagnostics.map(\.id) == [.string("cmd-9"), .number(42), nil, .string("v2")])
        #expect(diagnostics[0].payload?["message"] == .string("unknown command type"))
        #expect(diagnostics[1].payload?["message"] == .string("unknown event id"))
        await destroy(handle)
    }

    @Test func testCommandsNeedTheTestingFlag() async throws {
        let collector = MessageCollector()
        let config = testConfig(extra: ["testing": .bool(false)])
        let handle = try await startedEngine(collector, config: config)
        send(handle, #"{"v":1,"type":"test.advanceClock","id":"t","payload":{"seconds":60}}"#)
        try #require(await poll("the refusal") { collector.envelopes.contains { $0.id == .string("t") } })
        let refusal = try #require(collector.envelopes.first { $0.id == .string("t") })
        #expect(refusal.payload?["message"] == .string("test commands are disabled"))
        await destroy(handle)
    }

    @Test func sendFromInsideACallbackIsProcessed() async throws {
        let collector = MessageCollector()
        let handle = try #require(createTestEngine(collector))
        let sent = LockedBox(false)
        collector.setHook { text in
            guard text.contains(#""type":"ready""#), !sent.mutate({ let was = $0; $0 = true; return was }) else { return }
            tt_engine_send(handle.raw, #"{"v":1,"type":"nope","id":"inside"}"#)
        }
        send(handle, #"{"v":1,"type":"start"}"#)
        try #require(await poll("the diagnostic") { collector.envelopes.contains { $0.id == .string("inside") } })
        await destroy(handle)
    }

    @Test func noCallbackAfterDestroyReturns() async throws {
        let collector = MessageCollector()
        let handle = try await startedEngine(collector)
        collector.setHook { _ in Thread.sleep(forTimeInterval: 0.05) }
        for index in 0..<20 { send(handle, #"{"v":1,"type":"takeover.test","payload":{"title":"t\#(index)","calendarTitle":"c"}}"#) }
        try #require(await poll("the slow host is busy") { collector.types.contains("event.takeoverDue") })
        await destroy(handle)
        let count = collector.count
        try await Task.sleep(nanoseconds: 200_000_000)
        #expect(collector.count == count)
    }

    @Test func destroyFromInsideCallbackDoesNotDeadlockOrCallAgain() async throws {
        let collector = MessageCollector()
        let handle = try #require(createTestEngine(collector))
        let destroyedAt = LockedBox<Int?>(nil)
        collector.setHook { text in
            guard text.contains(#""type":"state""#), destroyedAt.value == nil else { return }
            tt_engine_destroy(handle.raw)
            destroyedAt.mutate { $0 = collector.count }
        }
        send(handle, #"{"v":1,"type":"start"}"#)
        for _ in 0..<5 { send(handle, #"{"v":1,"type":"refresh"}"#) }
        try #require(await poll("destroy ran in the callback") { destroyedAt.value != nil })
        try await Task.sleep(nanoseconds: 200_000_000)
        #expect(collector.count == destroyedAt.value, "no callback after the one that destroyed")
        // The handle is dead: a second destroy and a send are ignored.
        tt_engine_destroy(handle.raw)
        send(handle, #"{"v":1,"type":"refresh"}"#)
    }

    @Test func staleNullAndRepeatedHandlesAreIgnored() async throws {
        tt_engine_send(nil, "{}")
        tt_engine_reply(nil, 1, "{}")
        tt_engine_destroy(nil)
        let collector = MessageCollector()
        let handle = try #require(createTestEngine(collector))
        await destroy(handle)
        await destroy(handle)
        send(handle, #"{"v":1,"type":"start"}"#)
        tt_engine_reply(handle.raw, 1, #"{"v":1,"result":{}}"#)
        tt_engine_send(OpaquePointer(bitPattern: 0xdead_beef), #"{"v":1,"type":"start"}"#)
        tt_engine_destroy(OpaquePointer(bitPattern: 0xdead_beef))
        try await Task.sleep(nanoseconds: 50_000_000)
        #expect(collector.count == 0)
    }

    @Test func replyForUnknownRequestIDIsIgnored() async throws {
        let collector = MessageCollector()
        let handle = try await startedEngine(collector)
        await settle(collector)
        let before = collector.count
        tt_engine_reply(handle.raw, 424_242, #"{"v":1,"result":{}}"#)
        tt_engine_reply(handle.raw, 424_242, nil)
        tt_engine_reply(handle.raw, 0, "not json")
        try await Task.sleep(nanoseconds: 50_000_000)
        #expect(collector.count == before)
        await destroy(handle)
    }

    @Test func destroyWithAPendingSignInReturnsAndIgnoresTheLateReply() async throws {
        let collector = MessageCollector()
        let config = testConfig(extra: ["connectors": .object(["microsoft": .object(["clientID": .string("test-client")])])])
        let handle = try await startedEngine(collector, config: config)
        send(handle, #"{"v":1,"type":"account.add","payload":{"kindID":"microsoft"}}"#)
        try #require(await poll("the sign-in request") { collector.types.contains("service.auth.beginRedirect") })
        let request = try #require(collector.envelopes.first { $0.type == "service.auth.beginRedirect" })
        guard case .number(let id)? = request.id else { Issue.record("no request id"); return }
        await destroy(handle)
        let count = collector.count
        tt_engine_reply(handle.raw, id, #"{"v":1,"result":{"sessionID":"s","redirectURI":"http://127.0.0.1:1"}}"#)
        try await Task.sleep(nanoseconds: 100_000_000)
        #expect(collector.count == count)
    }

    @Test func diagnosticsFromBuildingTheEngineFollowReady() async throws {
        let directory = temporaryDirectory()
        try Data("{not json".utf8).write(to: directory.appendingPathComponent("settings.json"))
        let collector = MessageCollector()
        let handle = try #require(createTestEngine(collector, config: testConfig(directory: directory)))
        try await Task.sleep(nanoseconds: 50_000_000)
        #expect(collector.count == 0, "held until ready")
        send(handle, #"{"v":1,"type":"start"}"#)
        try #require(await poll("a state") { collector.types.contains("state") })
        let envelopes = collector.envelopes
        #expect(envelopes.first?.type == "ready")
        #expect(envelopes.dropFirst().first?.type == "event.diagnostic")
        #expect(envelopes.dropFirst().first?.payload?["category"] == .string("settings"))
        #expect(envelopes.last { $0.type == "state" }?.payload?["settings"]?["takeover"]?["leadTime"] == .number(60))
        await destroy(handle)
    }

    @Test func accountCancelEndsAPendingSignIn() async throws {
        let collector = MessageCollector()
        let config = testConfig(extra: ["connectors": .object(["microsoft": .object(["clientID": .string("test-client")])])])
        let handle = try await startedEngine(collector, config: config)
        send(handle, #"{"v":1,"type":"account.add","payload":{"kindID":"microsoft"}}"#)
        func isWorking() -> JSONValue? {
            collector.envelopes.last { $0.type == "state" }?.payload?["accountStatus"]?["isWorking"]
        }
        try #require(await poll("the sign-in runs") {
            collector.types.contains("service.auth.beginRedirect") && isWorking() == .bool(true)
        })
        // The sign-in lane is busy waiting for the host; the cancel must not queue behind it.
        send(handle, #"{"v":1,"type":"account.cancel"}"#)
        try #require(await poll("the sign-in ends") { isWorking() == .bool(false) })
        // A later sign-in starts afresh.
        send(handle, #"{"v":1,"type":"account.add","payload":{"kindID":"microsoft"}}"#)
        try #require(await poll("a second sign-in") {
            collector.types.filter { $0 == "service.auth.beginRedirect" }.count == 2
        })
        await destroy(handle)
    }

    @Test func stateCoalescesButEventsKeepOrder() async throws {
        let collector = MessageCollector()
        let handle = try await startedEngine(collector)
        collector.setHook { _ in Thread.sleep(forTimeInterval: 0.03) }
        let total = 30
        for index in 0..<total {
            send(handle, #"{"v":1,"type":"takeover.test","payload":{"title":"t\#(index)","calendarTitle":"c"}}"#)
        }
        // The last change: the newest state must show it, however many states were skipped.
        send(handle, #"{"v":1,"type":"settings.apply","payload":{"leadTime":123}}"#)
        try #require(await poll("every takeover and the last change", timeout: 20) {
            collector.types.filter { $0 == "event.takeoverDue" }.count == total
                && collector.envelopes.last { $0.type == "state" }?.payload?["settings"]?["takeover"]?["leadTime"] == .number(123)
        })
        let envelopes = collector.envelopes
        let titles = envelopes.filter { $0.type == "event.takeoverDue" }.compactMap { $0.payload?["event"]?["title"] }
        #expect(titles == (0..<total).map { .string("t\($0)") }, "events are never dropped or reordered")
        let revisions = envelopes.filter { $0.type == "state" }.compactMap { envelope -> Double? in
            if case .number(let revision)? = envelope.payload?["revision"] { return revision }
            return nil
        }
        #expect(revisions == revisions.sorted() && Set(revisions).count == revisions.count, "revisions only increase")
        let statesAfterStart = envelopes.drop(while: { $0.type != "event.takeoverDue" }).filter { $0.type == "state" }.count
        #expect(statesAfterStart < total, "a slow host sees fewer states than changes (\(statesAfterStart))")
        await destroy(handle)
    }

    @Test func twoEnginesInOneProcessDoNotShareState() async throws {
        let first = MessageCollector(), second = MessageCollector()
        let firstDirectory = temporaryDirectory(), secondDirectory = temporaryDirectory()
        let a = try await startedEngine(first, config: testConfig(directory: firstDirectory))
        let b = try await startedEngine(second, config: testConfig(directory: secondDirectory))
        send(a, #"{"v":1,"type":"settings.apply","payload":{"leadTime":300}}"#)
        send(b, #"{"v":1,"type":"settings.apply","payload":{"leadTime":600}}"#)
        func leadTime(_ collector: MessageCollector) -> JSONValue? {
            collector.envelopes.last { $0.type == "state" }?.payload?["settings"]?["takeover"]?["leadTime"]
        }
        try #require(await poll("both settings applied") { leadTime(first) == .number(300) && leadTime(second) == .number(600) })
        await destroy(a)
        await destroy(b)
        let firstFile = try String(contentsOf: firstDirectory.appendingPathComponent("settings.json"), encoding: .utf8)
        let secondFile = try String(contentsOf: secondDirectory.appendingPathComponent("settings.json"), encoding: .utf8)
        #expect(firstFile.contains("300") && !firstFile.contains("600"))
        #expect(secondFile.contains("600") && !secondFile.contains("300"))
    }
}
