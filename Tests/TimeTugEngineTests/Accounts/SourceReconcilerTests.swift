import CalendarCore
import Foundation
import Testing
import TimeTugCore
@testable import TimeTugEngine

final class ListeningSource: TimeTugCore.CalendarSource, @unchecked Sendable {
    let id: String
    var displayName: String { id }
    let continuation: AsyncStream<SourceChange>.Continuation
    private let stream: AsyncStream<SourceChange>
    private let started = LockedBox(0)
    var listenerStarted: Int { started.get() }
    init(id: String) {
        self.id = id
        (stream, continuation) = AsyncStream.makeStream(of: SourceChange.self)
    }
    func calendars() async throws -> [CalendarInfo] { [] }
    func events(in interval: DateInterval) async throws -> [TimeTugCalendarEvent] { [] }
    func changes() -> AsyncStream<SourceChange> { started.mutate { $0 += 1 }; return stream }
}

private final class ReconcilerRig: @unchecked Sendable {
    let built = LockedBox<[String: ListeningSource]>([:])
    let changeCount = LockedBox(0)
    let failNext = LockedBox<Set<ConnectionID>>([])
    let reconciler: SourceReconciler

    init(failing: Set<ConnectionID> = []) {
        let built = built, changeCount = changeCount, failNext = failNext
        reconciler = SourceReconciler(
            buildAccount: { c in
                if failing.contains(c.connectionID) || failNext.get().contains(c.connectionID) {
                    throw TimeTugCore.SourceError.authExpired
                }
                let s = ListeningSource(id: c.sourceID)
                built.mutate { $0[c.connectionID] = s }
                return s
            },
            buildSystemSource: { ListeningSource(id: "eventkit") },
            onChange: { _ in changeCount.mutate { $0 += 1 } })
    }
}

private let a = Connection(kindID: "google", connectionID: "1", displayName: "a@x.test")
private let b = Connection(kindID: "google", connectionID: "2", displayName: "b@x.test")

private func settle() async { try? await Task.sleep(for: .milliseconds(80)) }

@Suite struct SourceReconcilerTests {
    @Test func buildsSystemSourceFirstThenAccountsAndKeysBySourceID() async {
        let rig = ReconcilerRig()
        let update = await rig.reconciler.reconcile(connections: [a, b], systemCalendarEnabled: true)
        #expect(update.sources.map(\.id) == ["eventkit", "google-1", "google-2"])
        #expect(await rig.reconciler.sourceID(forConnection: "1") == a.sourceID)
        await rig.reconciler.stop()
    }

    @Test func systemSourceDisabledIsLeftOut() async {
        let rig = ReconcilerRig()
        let update = await rig.reconciler.reconcile(connections: [a], systemCalendarEnabled: false)
        #expect(update.sources.map(\.id) == ["google-1"])
        await rig.reconciler.stop()
    }

    @Test func unchangedSourcesKeepTheirInstanceAndListener() async {
        let rig = ReconcilerRig()
        _ = await rig.reconciler.reconcile(connections: [a], systemCalendarEnabled: false)
        let first = rig.built.get()["1"]!
        let update = await rig.reconciler.reconcile(connections: [a, b], systemCalendarEnabled: false)
        let kept = update.sources.first { $0.id == "google-1" }
        #expect(kept as? ListeningSource === first)
        await settle()
        #expect(first.listenerStarted == 1)
        await rig.reconciler.stop()
    }

    @Test func removedSourceStopsItsListener() async {
        let rig = ReconcilerRig()
        _ = await rig.reconciler.reconcile(connections: [a], systemCalendarEnabled: false)
        let source = rig.built.get()["1"]!
        await settle()
        _ = await rig.reconciler.reconcile(connections: [], systemCalendarEnabled: false)
        source.continuation.yield(.eventsChanged(sourceID: source.id, calendarIDs: nil))
        await settle()
        #expect(rig.changeCount.get() == 0)
    }

    @Test func aChangeFromASourceTriggersOnChange() async {
        let rig = ReconcilerRig()
        _ = await rig.reconciler.reconcile(connections: [a], systemCalendarEnabled: false)
        await settle()
        rig.built.get()["1"]!.continuation.yield(.eventsChanged(sourceID: a.sourceID, calendarIDs: nil))
        await settle()
        #expect(rig.changeCount.get() == 1)
        await rig.reconciler.stop()
    }

    @Test func failingBuildIsReportedAndDoesNotStopOthers() async {
        let rig = ReconcilerRig(failing: ["1"])
        let update = await rig.reconciler.reconcile(connections: [a, b], systemCalendarEnabled: false)
        #expect(update.sources.map(\.id) == ["google-2"])
        #expect(update.failures["1"] != nil)
        await rig.reconciler.stop()
    }

    @Test func rebuildReplacesTheInstanceAndRestartsTheListener() async {
        let rig = ReconcilerRig()
        _ = await rig.reconciler.reconcile(connections: [a], systemCalendarEnabled: false)
        let old = rig.built.get()["1"]!
        old.continuation.finish()   // like a source whose stream ended after .sourceFailed
        let update = await rig.reconciler.rebuild(a, connections: [a], systemCalendarEnabled: false)
        let new = rig.built.get()["1"]!
        #expect(old !== new)
        let replaced = update.sources.first
        #expect(replaced as? ListeningSource === new)
        await settle()
        new.continuation.yield(.eventsChanged(sourceID: new.id, calendarIDs: nil))
        await settle()
        #expect(rig.changeCount.get() == 1)
        await rig.reconciler.stop()
    }

    @Test func aFailedRebuildKeepsTheWorkingSourceAndItsListener() async {
        let rig = ReconcilerRig()
        _ = await rig.reconciler.reconcile(connections: [a], systemCalendarEnabled: false)
        let old = rig.built.get()["1"]!
        await settle()
        rig.failNext.set(["1"])
        let update = await rig.reconciler.rebuild(a, connections: [a], systemCalendarEnabled: false)
        #expect(update.sources.count == 1)
        let kept = update.sources.first
        #expect(kept as? ListeningSource === old)
        #expect(update.failures["1"] != nil)
        old.continuation.yield(.eventsChanged(sourceID: old.id, calendarIDs: nil))
        await settle()
        #expect(rig.changeCount.get() == 1)
        await rig.reconciler.stop()
    }

    @Test func accountManagerWorksEndToEndWithTheRealReconciler() async throws {
        let dir = try TempDir.make()
        let credentials = InMemoryCredentialStore()
        let signIn = FakeSignIn()
        let settings = InMemorySettingsStorage(EngineSettings())
        var registry = ConnectorRegistry()
        registry.register(FakeConnectorKind())
        let host = EngineHost(
            dataDirectory: dir, settings: settings, credentials: credentials, interaction: signIn, signIn: signIn,
            registry: registry, requestSystemAccess: { })
        let applied = LockedBox<[[String]]>([])
        let reconciler = SourceReconciler(
            buildAccount: { ListeningSource(id: $0.sourceID) },
            buildSystemSource: { ListeningSource(id: "eventkit") },
            onChange: { _ in })
        let manager = AccountManager(
            host: host, reconciler: reconciler,
            settings: { settings.load() },
            applySources: { sources in applied.mutate { $0.append(sources.map(\.id)) } },
            updateSettings: { change in
                var s = settings.load()
                change(&s)
                settings.save(s)
            })
        await manager.addAccount(kindID: "google")
        #expect(applied.get().last == ["eventkit", "google-1"])
        #expect(await reconciler.sourceID(forConnection: "1") == "google-1")
        await manager.removeAccount(connectionID: "1")
        #expect(applied.get().last == ["eventkit"])
        #expect(await reconciler.sourceID(forConnection: "1") == nil)
        await reconciler.stop()
    }
}
