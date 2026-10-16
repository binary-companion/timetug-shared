import CalendarCore
import Foundation
import Testing
@testable import TimeTugEngineC
import TimeTugEngineProtocol

@Suite struct RemoteHostServicesTests {
    private let start = Date(timeIntervalSince1970: 1_760_000_000)

    /// A table whose sent envelopes are recorded, on a manual clock.
    private func makeTable() -> (RequestTable, LockedBox<[Envelope]>, ManualEngineClock) {
        let sent = LockedBox<[Envelope]>([])
        let clock = ManualEngineClock(start: start)
        let table = RequestTable(send: { envelope in sent.mutate { $0.append(envelope) } }, clock: clock)
        return (table, sent, clock)
    }

    private func requestID(_ envelope: Envelope) -> UInt64? {
        if case .number(let id)? = envelope.id { return id }
        return nil
    }

    @Test func replyResolvesRequest() async throws {
        let (table, sent, _) = makeTable()
        let result = Task { try await table.request(type: "service.x", payload: nil, timeout: nil) }
        await poll("the request is sent") { !sent.value.isEmpty }
        let id = try #require(requestID(sent.value[0]))
        #expect(sent.value[0].type == "service.x")
        table.fulfill(id: id, reply: #"{"v":1,"result":{"a":1}}"#)
        #expect(try await result.value == .object(["a": .number(1)]))
    }

    @Test func requestIDsAreDistinctAndNonZero() async throws {
        let (table, sent, _) = makeTable()
        let a = Task { try await table.request(type: "service.a", payload: nil, timeout: nil) }
        let b = Task { try await table.request(type: "service.b", payload: nil, timeout: nil) }
        await poll("both requests are sent") { sent.value.count == 2 }
        let ids = sent.value.compactMap(requestID)
        #expect(Set(ids).count == 2)
        #expect(!ids.contains(0))
        for envelope in sent.value {
            table.fulfill(id: try #require(requestID(envelope)), reply: #"{"v":1,"result":"\#(envelope.type)"}"#)
        }
        #expect(try await a.value == .string("service.a"))
        #expect(try await b.value == .string("service.b"))
    }

    @Test func errorReplyThrowsTheKind() async throws {
        let (table, sent, _) = makeTable()
        let result = Task { try await table.request(type: "service.x", payload: nil, timeout: nil) }
        await poll("the request is sent") { !sent.value.isEmpty }
        table.fulfill(id: try #require(requestID(sent.value[0])), reply: #"{"v":1,"error":{"kind":"denied","message":"no"}}"#)
        await #expect(throws: ServiceError(kind: .denied, message: "no")) { try await result.value }
    }

    @Test func unsupportedReplyVersionFailsTheRequest() async throws {
        let (table, sent, _) = makeTable()
        let result = Task { try await table.request(type: "service.x", payload: nil, timeout: nil) }
        await poll("the request is sent") { !sent.value.isEmpty }
        table.fulfill(id: try #require(requestID(sent.value[0])), reply: #"{"v":2,"result":{}}"#)
        do {
            _ = try await result.value
            Issue.record("a v2 reply resolved the request")
        } catch let error as ServiceError {
            #expect(error.kind == .failed)
        }
    }

    @Test func unknownAndRepeatedRepliesAreIgnored() async throws {
        let (table, sent, _) = makeTable()
        table.fulfill(id: 999, reply: #"{"v":1,"result":1}"#)
        let result = Task { try await table.request(type: "service.x", payload: nil, timeout: nil) }
        await poll("the request is sent") { !sent.value.isEmpty }
        let id = try #require(requestID(sent.value[0]))
        table.fulfill(id: id, reply: #"{"v":1,"result":1}"#)
        table.fulfill(id: id, reply: #"{"v":1,"result":2}"#)
        table.fulfill(id: id, reply: "not json")
        #expect(try await result.value == .number(1))
    }

    @Test func failAllCancelsPendingAndLaterRequests() async throws {
        let (table, sent, _) = makeTable()
        let result = Task { try await table.request(type: "service.x", payload: nil, timeout: nil) }
        await poll("the request is sent") { !sent.value.isEmpty }
        table.failAll(kind: .cancelled)
        await #expect(throws: CancellationError.self) { try await result.value }
        await #expect(throws: CancellationError.self) {
            try await table.request(type: "service.y", payload: nil, timeout: nil)
        }
        #expect(sent.value.count == 1, "a request after failAll is never sent")
    }

    @Test func cancellingTheCallerCancelsTheRequest() async throws {
        let (table, sent, _) = makeTable()
        let task = Task { try await table.request(type: "service.x", payload: nil, timeout: nil) }
        await poll("the request is sent") { !sent.value.isEmpty }
        task.cancel()
        await #expect(throws: CancellationError.self) { try await task.value }
        // The host's late answer is ignored.
        table.fulfill(id: try #require(requestID(sent.value[0])), reply: #"{"v":1,"result":1}"#)
    }

    /// A cancellation handler that runs after the answer (a reply or failAll racing task.cancel) keeps nothing.
    @Test func lateCancellationKeepsNoBookkeeping() async throws {
        let (table, sent, clock) = makeTable()
        let result = Task { try await table.request(type: "service.x", payload: nil, timeout: nil) }
        await poll("the request is sent") { !sent.value.isEmpty }
        let id = try #require(requestID(sent.value[0]))
        table.fulfill(id: id, reply: #"{"v":1,"result":1}"#)
        #expect(try await result.value == .number(1))
        table.cancel(id: id)
        #expect(table.trackedIDCount == 0)

        let sleeper = Task { try await clock.sleep(until: start.addingTimeInterval(10)) }
        await poll("the sleep is registered") { clock.pendingSleeps == 1 }
        clock.advance(by: 10)
        try await sleeper.value
        clock.cancel(id: 1)
        #expect(clock.trackedIDCount == 0)
    }

    @Test func timeoutFollowsTheEngineClock() async throws {
        let (table, sent, clock) = makeTable()
        let result = Task { try await table.request(type: "service.x", payload: nil, timeout: 60) }
        await poll("the request is sent") { !sent.value.isEmpty }
        await poll("the timer sleeps") { clock.pendingSleeps == 1 }
        clock.advance(by: 59)
        #expect(clock.pendingSleeps == 1)
        clock.advance(by: 2)
        do {
            _ = try await result.value
            Issue.record("the request did not time out")
        } catch let error as ServiceError {
            #expect(error.kind == .network)
        }
    }

    @Test func answeredRequestStopsItsTimer() async throws {
        let (table, sent, clock) = makeTable()
        let result = Task { try await table.request(type: "service.x", payload: nil, timeout: 60) }
        await poll("the request is sent") { !sent.value.isEmpty }
        table.fulfill(id: try #require(requestID(sent.value[0])), reply: #"{"v":1,"result":true}"#)
        #expect(try await result.value == .bool(true))
        await poll("the timer is cancelled") { clock.pendingSleeps == 0 }
    }

    @Test func httpTimeoutMapsToNetworkError() async throws {
        let (table, sent, clock) = makeTable()
        let transport = RemoteHTTPTransport(table: table)
        let response = Task { try await transport.send(HTTPRequest(url: URL(string: "https://example.com/a")!)) }
        await poll("the request is sent") { !sent.value.isEmpty }
        await poll("the timer sleeps") { clock.pendingSleeps == 1 }
        clock.advance(by: 61)
        do {
            _ = try await response.value
            Issue.record("the request did not time out")
        } catch let error as SourceError {
            guard case .network = error else { Issue.record("expected network, got \(error)"); return }
        }
    }

    @Test func httpBodyIsBase64OnTheWire() async throws {
        let (table, sent, _) = makeTable()
        let transport = RemoteHTTPTransport(table: table)
        let response = Task { try await transport.send(HTTPRequest(
            url: URL(string: "https://example.com/token")!, method: "POST",
            headers: ["Content-Type": "text/plain"], body: Data("hello".utf8))) }
        await poll("the request is sent") { !sent.value.isEmpty }
        let envelope = sent.value[0]
        #expect(envelope.type == "service.http.send")
        #expect(envelope.payload?["body"] == .string("aGVsbG8="))
        #expect(envelope.payload?["method"] == .string("POST"))
        #expect(envelope.payload?["url"] == .string("https://example.com/token"))
        #expect(envelope.payload?["headers"]?["Content-Type"] == .string("text/plain"))
        table.fulfill(
            id: try #require(requestID(envelope)),
            reply: #"{"v":1,"result":{"status":201,"headers":{"X-A":"b"},"body":"eyJvayI6dHJ1ZX0="}}"#)
        let value = try await response.value
        #expect(value.status == 201)
        #expect(value.header("x-a") == "b")
        #expect(String(decoding: value.body, as: UTF8.self) == #"{"ok":true}"#)
    }

    @Test func httpTransportErrorsAreNetworkErrors() async throws {
        let (table, sent, _) = makeTable()
        let transport = RemoteHTTPTransport(table: table)
        let response = Task { try await transport.send(HTTPRequest(url: URL(string: "https://example.com/a")!)) }
        await poll("the request is sent") { !sent.value.isEmpty }
        table.fulfill(id: try #require(requestID(sent.value[0])), reply: #"{"v":1,"error":{"kind":"network","message":"offline"}}"#)
        await #expect(throws: SourceError.network("network: offline")) { try await response.value }
    }

    @Test func credentialStoreRoundTrip() async throws {
        let (table, sent, _) = makeTable()
        let store = RemoteCredentialStore(table: table)
        let revision = UUID()
        // A host that keeps one secret set.
        let answering = Task {
            var answered = 0
            while answered < 3 {
                let pending = sent.value.dropFirst(answered)
                for envelope in pending {
                    guard case .number(let id)? = envelope.id else { continue }
                    switch envelope.type {
                    case "service.credentials.get":
                        table.fulfill(id: id, reply: #"{"v":1,"result":{"secrets":{"refresh_token":"r"}}}"#)
                    case "service.credentials.snapshot":
                        table.fulfill(id: id, reply: #"{"v":1,"result":{"snapshot":{"secrets":{"refresh_token":"r"},"revision":"\#(revision.uuidString)"}}}"#)
                    case "service.credentials.updateRefreshToken":
                        table.fulfill(id: id, reply: #"{"v":1,"result":{"updated":true}}"#)
                    default: break
                    }
                    answered += 1
                }
                try? await Task.sleep(nanoseconds: 1_000_000)
            }
        }
        #expect(try await store.secrets(for: "c1") == ["refresh_token": "r"])
        let snapshot = try #require(try await store.credentialSnapshot(for: "c1"))
        #expect(snapshot.revision == revision)
        #expect(try await store.updateRefreshToken("n", for: "c1", expectedRevision: revision))
        await answering.value
        #expect(sent.value[2].payload?["expectedRevision"] == .string(revision.uuidString))
        #expect(sent.value[0].payload == .object(["connectionID": .string("c1")]))
    }
}
