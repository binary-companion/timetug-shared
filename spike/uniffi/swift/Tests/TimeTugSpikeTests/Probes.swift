import Foundation
import Testing
import TimeTugSpike

// ---- fakes ----
final class Box<T>: @unchecked Sendable {
    private let lock = NSLock(); private var v: T
    init(_ v: T) { self.v = v }
    func with<R>(_ f: (inout T) -> R) -> R { lock.lock(); defer { lock.unlock() }; return f(&v) }
}

final class FakeSession: RedirectSession {
    func redirectUri() async throws -> URL { URL(string: "http://127.0.0.1:1234/cb")! }
    func close() async {}
}
final class FakeAuth: AuthInteraction {
    func beginRedirect() async throws -> RedirectBegun { RedirectBegun(session: FakeSession()) }
    func beginRedirectSync() throws -> RedirectSession { FakeSession() }
}
enum Boom: Error { case x }
final class FakeStore: CredentialStore {
    let mode = Box("ok")
    let sawCancel = Box(false)
    func secrets(id: UUID) async throws -> Data? {
        switch mode.with({ $0 }) {
        case "deny": throw HostError.Denied
        case "net": throw HostError.Network("offline")
        case "swift": throw Boom.x
        default: return Data([1, 2, 3])
        }
    }
    func setSecrets(id: UUID, value: Data) async throws {}
    func neverReturns() async throws {
        // Never returns on its own; notices cancellation if the engine drops the future.
        for _ in 0..<200 {
            if Task.isCancelled { sawCancel.with { $0 = true }; throw CancellationError() }
            try? await Task.sleep(nanoseconds: 10_000_000)
        }
    }
    func onMainActor() async throws -> String { await MainActor.run { MainActor.preconditionIsolated(); return "main" } }
}
final class MainActorStore: CredentialStore {
    func secrets(id: UUID) async throws -> Data? { nil }
    func setSecrets(id: UUID, value: Data) async throws {}
    func neverReturns() async throws {}
    @MainActor func onMainActor() async throws -> String { MainActor.preconditionIsolated(); return "main-actor-impl" }
}
final class FakeSystem: SystemSource {
    let handler = Box<ChangeHandler?>(nil)
    func requestAccess() async throws -> Bool { true }
    func calendars() async throws -> [SysCalendar] { [SysCalendar(id: "c", title: "t", writable: true)] }
    func eventsIn(start: Date, end: Date) async throws -> [BigEvent] { makeAgenda(count: 2) }
    func series(id: String, calendarId: String) async throws -> BigEvent? { makeAgenda(count: 1)[0] }
    func capabilities() async throws -> SysCalendar? { nil }
    func createEvent(draft: BigEvent) async throws -> BigEvent { draft }
    func updateEvent(id: String, edit: EventEdit) async throws -> BigEvent? { nil }
    func deleteEvent(id: String) async throws {}
    func moveEvent(id: String, to: String) async throws -> BigEvent? { makeAgenda(count: 1)[0] }
    func rsvp(id: String, p: Participation) async throws {}
    func setChangeHandler(handler: ChangeHandler) async throws { self.handler.with { $0 = handler } }
    func refresh() async throws {}
}
final class FakeObserver: Observer {
    let states = Box<[UInt64]>([]); let events = Box<[String]>([]); let last = Box<Int>(0)
    let done = Box(false)
    let firstAt = Box<Date?>(nil); let lastAt = Box<Date?>(nil)
    func stateChanged(state: SpikeState) {
        let now = Date()
        firstAt.with { if $0 == nil { $0 = now } }; lastAt.with { $0 = now }
        states.with { $0.append(state.revision) }; last.with { $0 = state.events.count }
    }
    func event(name: String) { events.with { $0.append(name) }; if name == "blast-done" { done.with { $0 = true } } }
}
func makeEngine(store: CredentialStore = FakeStore(), system: SystemSource? = FakeSystem(), observer: Observer = FakeObserver()) throws -> SpikeEngine {
    try SpikeEngine(host: HostBundle(store: store, system: system, auth: FakeAuth()), observer: observer)
}

@Suite struct Probes {
    @Test func bigRecordRoundTrip() {
        let e = makeAgenda(count: 1)[0]
        #expect(echoEvent(e: e) == e)
        #expect(Set([e, e]).count == 1)
        #expect(e.f12X == .occurrence(seriesId: "s", originalStart: Date(timeIntervalSince1970: 0.005)))
        #expect(e.f10X == ["k": "v"]); #expect(e.f11X == Set(["z"]))
    }
    @Test func unicodeAndLongStrings() {
        let s = "e\u{0301} 😀 👩‍👩‍👧‍👦 \u{0000} \u{FFFF} 日本語"
        #expect(echoString(s: s) == s)
        let long = String(repeating: "x😀", count: 2_000_000)
        #expect(echoString(s: long) == long)
        var e = makeAgenda(count: 1)[0]; e.f00X = s; e.f06X = long
        #expect(echoEvent(e: e) == e)
    }
    @Test func enums() {
        let edit = EventEdit.many([.title("a"), .time(start: Date(timeIntervalSince1970: 1), end: Date(timeIntervalSince1970: 2)), .notes(.set("n")), .many([])])
        #expect(echoEdit(e: edit) == edit)
        let a = Attendee(name: nil, email: "e@x", response: .tentative(0.5, "x"))   // isSelf default omitted
        #expect(a.isSelf == false)
    }
    @Test func customTypes() throws {
        let d = Date(timeIntervalSince1970: 1_700_000_000.0004)
        #expect(echoTimestamp(t: d).timeIntervalSince1970 == 1_700_000_000.000)
        let d2 = Date(timeIntervalSince1970: 1_700_000_000.0006)
        #expect(echoTimestamp(t: d2).timeIntervalSince1970 == 1_700_000_000.001)
        #expect(echoZone(z: TimeZone(identifier: "Europe/Berlin")!).identifier == "Europe/Berlin")
        #expect(echoZone(z: TimeZone(secondsFromGMT: 3600)!).identifier == "GMT+0100")  // what the identifier of a fixed zone is in Swift
        #expect(echoUrl(u: URL(string: "https://example.com/é?a=b c")!).absoluteString.isEmpty == false)
        let u = UUID(); #expect(echoUuid(u: u) == u)
        #expect(echoData(d: Data([0, 255])) == Data([0, 255]))
        #expect(echoDuration(d: 1.5) == 1.5)
        do { _ = try echoZoneThrowing(z: TimeZone(identifier: "Asia/Tokyo")!); Issue.record("expected throw") }
        catch let e as SpikeError { #expect(e == .InvalidConfig(reason: "Asia/Tokyo")) }
        catch { Issue.record("wrong error \(error)") }
        #expect(echoCalendar(c: CalendarRecord(id: "a", title: "b")).id == "a")
        #expect(echoConnection(c: Connection(id: UUID(), label: "x")).label == "x")
    }
    @Test func errors() {
        let cases: [(Int, SpikeError)] = [(0, .Source(.authExpired)), (1, .Source(.http(status: 503))), (2, .Cancelled), (3, .Host(message: "boom"))]
        for (k, want) in cases {
            do { try failWith(kind: UInt8(k)); Issue.record("no throw") } catch { #expect(error as? SpikeError == want) }
        }
    }
    @Test func engineSendAndObserver() async throws {
        let obs = FakeObserver(); let engine = try makeEngine(observer: obs)
        let r = try await engine.send(command: .refresh)
        #expect(r == "Some([1, 2, 3]) http://127.0.0.1:1234/cb")
        #expect(obs.states.with { $0 } == [1]); #expect(obs.events.with { $0 } == ["calendars=1"])
        do { print("EXERCISE", try await engine.exerciseSystem()) } catch { print("EXERCISE-ERR", error) }
        engine.shutdown()
    }
    @Test func hostErrorsMapToRustErrors() async throws {
        let store = FakeStore(); let engine = try makeEngine(store: store)
        for (mode, expect) in [("deny", "denied"), ("net", "network offline"), ("swift", "failed")] {
            store.mode.with { $0 = mode }
            do { _ = try await engine.send(command: .refresh); Issue.record("no throw") }
            catch let e as SpikeError { print("MODE", mode, e); if case .Host(let m) = e { #expect(m.hasPrefix(expect), "\(m)") } else { Issue.record("\(e)") } }
        }
        engine.shutdown()
    }
    @Test func neverReturningHostDroppedByTimeout() async throws {
        let store = FakeStore(); let engine = try makeEngine(store: store)
        let r = await engine.callNeverWithTimeout(ms: 100)
        #expect(r == "timed out, future dropped")
        try? await Task.sleep(nanoseconds: 300_000_000)
        print("CANCEL-OBSERVED-BY-SWIFT", store.sawCancel.with { $0 })
        engine.shutdown()
    }
    @Test @MainActor func mainActorFakeDoesNotDeadlock() async throws {
        let engine = try makeEngine(store: MainActorStore())
        #expect(try await engine.callMainActor() == "main-actor-impl")
        let engine2 = try makeEngine(store: FakeStore())
        #expect(try await engine2.callMainActor() == "main")
    }
    @Test func lifecycleLoop() async throws {
        for _ in 0..<500 {
            let obs = FakeObserver(); let engine = try makeEngine(observer: obs)
            async let a = engine.send(command: .refresh)
            engine.shutdown()
            _ = try? await a
            engine.shutdown()
        }
    }
    @Test func perCallOverheadAndStateDelivery() async throws {
        let agenda = makeAgenda(count: 500)
        let t0 = ContinuousClock.now
        for _ in 0..<(ProcessInfo.processInfo.environment["ECHO_N"].flatMap(Int.init) ?? 1000) { _ = echoEvents(e: agenda) }
        let dt = ContinuousClock.now - t0
        print("ECHO500 x1000 total", dt, "per call ms", Double(dt.components.attoseconds) / 1e15 / 1000 + Double(dt.components.seconds) * 1000 / 1000)
        func ms(_ n: Int, _ f: () -> Void) -> Double { let t = ContinuousClock.now; for _ in 0..<n { f() }; let d = ContinuousClock.now - t; return (Double(d.components.seconds) * 1e3 + Double(d.components.attoseconds) / 1e15) / Double(n) }
        print("RUST->SWIFT lift 500 (makeAgenda) ms/call", ms(100) { _ = makeAgenda(count: 500) })
        print("SWIFT->RUST lower 500 (countEvents) ms/call", ms(100) { _ = countEvents(e: agenda) })
        print("SUMMARY make ms/call", ms(10000) { _ = makeSummary(r: 1) })
        print("ONE EVENT lift ms/call", ms(10000) { _ = makeAgenda(count: 1) })
        let obs = FakeObserver(); let engine = try makeEngine(observer: obs)
        engine.blastStates(count: 200, events: 500)
        while !obs.done.with({ $0 }) { try await Task.sleep(nanoseconds: 5_000_000) }
        let span = obs.lastAt.with { $0! }.timeIntervalSince(obs.firstAt.with { $0! })
        print("BLAST 200 states of 500 events: per state ms", span / 199 * 1000)
        engine.shutdown()
    }

    @Test func highRateObserverAndHandler() async throws {
        let obs = FakeObserver(); let sys = FakeSystem(); let engine = try makeEngine(system: sys, observer: obs)
        let t = ContinuousClock.now
        engine.blastStates(count: 100_000, events: 0)
        while !obs.done.with({ $0 }) { try await Task.sleep(nanoseconds: 1_000_000) }
        let d = ContinuousClock.now - t
        print("HIGHRATE 100k empty states", d, "count", obs.states.with { $0.count }, "ordered", obs.states.with { $0 == Array(0..<100_000) })
        _ = try await engine.exerciseSystem()
        let h = sys.handler.with { $0! }
        h.changed(); h.changed(); h.changed()
        #expect(h.count() == 3)
        engine.shutdown()
    }
}
