import CalendarCore
import Foundation
import Testing
import TimeTugCore
@testable import CalendarBridge

final class FakeLibrarySource: CalendarCore.CalendarSource, @unchecked Sendable {
    let id: String
    let displayName = "Fake"
    let capabilities = SourceCapabilities()
    var calendarsResult: Result<[CalendarDescriptor], Error> = .success([])
    var eventsResult: Result<[CalendarCore.CalendarEvent], Error> = .success([])
    let continuation: AsyncStream<CalendarChange>.Continuation
    private let stream: AsyncStream<CalendarChange>
    private let terminated = Flag()

    init(id: String = "src") {
        self.id = id
        (stream, continuation) = AsyncStream.makeStream(of: CalendarChange.self)
        continuation.onTermination = { [terminated] _ in terminated.set() }
    }
    var wasTerminated: Bool { terminated.value }

    func calendars() async throws -> [CalendarDescriptor] { try calendarsResult.get() }
    func events(in interval: DateInterval) async throws -> [CalendarCore.CalendarEvent] { try eventsResult.get() }
    func changes() -> AsyncStream<CalendarChange> { stream }
}

final class Flag: @unchecked Sendable {
    private let lock = NSLock()
    private var flag = false
    func set() { lock.lock(); flag = true; lock.unlock() }
    var value: Bool { lock.lock(); defer { lock.unlock() }; return flag }
}

private func thrown(_ body: () async throws -> Void) async -> Error? {
    do { try await body(); return nil } catch { return error }
}

@Test func forwardsIdentityAndMapsCalendarsAndEvents() async throws {
    let fake = FakeLibrarySource(id: "google-1")
    fake.calendarsResult = .success([CalendarDescriptor(id: "c", title: "Work", service: .google, accountName: "me@x.test")])
    fake.eventsResult = .success([CalendarCore.CalendarEvent(eventID: "e", calendarID: "c", title: "T",
        start: Date(timeIntervalSince1970: 1_000), end: Date(timeIntervalSince1970: 2_000))])
    let source = ConnectedSource(fake)
    #expect(source.id == "google-1" && source.displayName == "Fake")
    #expect(try await source.calendars().map(\.key) == ["google-1/c"])
    let events = try await source.events(in: DateInterval(start: .distantPast, end: .distantFuture))
    #expect(events.map(\.sourceID) == ["google-1"] && events.map(\.title) == ["T"])
}

@Test func translatesPermissionAndAuthErrors() async {
    let fake = FakeLibrarySource()
    let source = ConnectedSource(fake)
    fake.calendarsResult = .failure(CalendarCore.SourceError.needsPermission)
    let permission = await thrown { _ = try await source.calendars() }
    guard case TimeTugCore.SourceError.needsPermission? = permission else { Issue.record("got \(String(describing: permission))"); return }
    fake.eventsResult = .failure(CalendarCore.SourceError.authExpired)
    let auth = await thrown { _ = try await source.events(in: DateInterval(start: .distantPast, end: .distantFuture)) }
    guard case TimeTugCore.SourceError.authExpired? = auth else { Issue.record("got \(String(describing: auth))"); return }
}

@Test func otherErrorsPropagateUntranslated() async {
    let fake = FakeLibrarySource()
    fake.eventsResult = .failure(CalendarCore.SourceError.server(status: 500))
    let error = await thrown { _ = try await ConnectedSource(fake).events(in: DateInterval(start: .distantPast, end: .distantFuture)) }
    #expect(error as? CalendarCore.SourceError == .server(status: 500))
}

@Test func everyLibraryChangeYieldsOnceIncludingSourceFailed() async {
    let fake = FakeLibrarySource()
    var iterator = ConnectedSource(fake).changes().makeAsyncIterator()
    fake.continuation.yield(.calendarsChanged)
    fake.continuation.yield(.eventsChanged(calendarIDs: ["c"]))
    fake.continuation.yield(.sourceFailed(.authExpired))
    for _ in 0..<3 { #expect(await iterator.next() != nil) }
}

@Test func cancellingTheConsumerEndsTheLibraryStream() async throws {
    let fake = FakeLibrarySource()
    let consumer = Task { for await _ in ConnectedSource(fake).changes() {} }
    try await Task.sleep(for: .milliseconds(50))
    consumer.cancel()
    for _ in 0..<200 where !fake.wasTerminated { try await Task.sleep(for: .milliseconds(10)) }
    #expect(fake.wasTerminated)
}
