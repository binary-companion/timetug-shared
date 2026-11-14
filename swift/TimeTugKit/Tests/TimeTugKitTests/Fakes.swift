import Foundation
import TimeTugKit

/// What a fake does on its next calls.
enum Mode: Sendable {
    case works
    case throwing(HostError)
    case neverReturns
}

/// A value several threads read and write.
final class Locked<Value>: @unchecked Sendable {
    private let lock = NSLock()
    private var value: Value
    init(_ value: Value) { self.value = value }
    func get() -> Value { lock.lock(); defer { lock.unlock() }; return value }
    func set(_ new: Value) { lock.lock(); value = new; lock.unlock() }
    @discardableResult func update<R>(_ change: (inout Value) -> R) -> R { lock.lock(); defer { lock.unlock() }; return change(&value) }
}

/// Runs a fake's reply according to its mode. A call that never returns suspends until its task is cancelled (which the
/// engine does when it drops the call) and then throws `CancellationError`.
func respond<T: Sendable>(_ mode: Mode, started: Locked<Int>? = nil, cancelled: Locked<Int>? = nil, _ value: @Sendable () -> T) async throws -> T {
    switch mode {
    case .works: return value()
    case .throwing(let error): throw error
    case .neverReturns:
        started?.update { $0 += 1 }
        do {
            while true { try await Task.sleep(nanoseconds: 20_000_000) }
        } catch {
            cancelled?.update { $0 += 1 }
            throw error
        }
    }
}

final class FakeCredentials: CredentialStore {
    let mode = Locked(Mode.works)
    private let store = Locked<[String: [String: String]]>([:])
    func secrets(connectionId: String) async throws -> [String: String]? {
        try await respond(mode.get()) { store.get()[connectionId] }
    }
    func setSecrets(secrets: [String: String], connectionId: String) async throws {
        try await respond(mode.get()) { store.update { $0[connectionId] = secrets } }
    }
    func removeSecrets(connectionId: String) async throws {
        try await respond(mode.get()) { store.update { $0[connectionId] = nil } }
    }
    func credentialSnapshot(connectionId: String) async throws -> CredentialSnapshot? {
        try await respond(mode.get()) { store.get()[connectionId].map { CredentialSnapshot(secrets: $0, revision: UUID(uuid: (0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1))) } }
    }
    func updateRefreshToken(token: String, connectionId: String, expectedRevision: UUID) async throws -> Bool { false }
}

final class FakeAuthorization: AuthorizationInteraction {
    let promptMode = Locked(Mode.works)
    let promptStarted = Locked(0)
    let promptCancelled = Locked(0)
    let link = Locked("https://feeds.example.com/team.ics")
    func beginOauthRedirect() async throws -> OAuthRedirectHandle { throw HostError.Denied }
    func promptCredentials(fields: [CredentialField]) async throws -> [String: String] {
        let link = link.get()
        return try await respond(promptMode.get(), started: promptStarted, cancelled: promptCancelled) { ["link": link] }
    }
}

final class FakeSignIn: SignInPrompting {
    func prepare(title: String, help: CredentialHelp?, values: [String: String], error: String?) async {}
    func lastNonSecretValues() async -> [String: String]? { nil }
}

final class FakeSettings: SettingsStorage {
    let stored = Locked<EngineSettings?>(nil)
    func load() -> EngineSettings { stored.get() ?? EngineSettings(takeover: TakeoverSettings(leadTime: 60, takeoverCalendarKeys: [], hiddenCalendarKeys: [], requireConferenceLink: false, requireOtherAttendees: false, skipAllDayEvents: true, enabled: true), inferenceEnabled: false, systemCalendarEnabled: true) }
    func save(settings: EngineSettings) { stored.set(settings) }
}

/// Serves an iCalendar feed for every address.
final class FakeHTTP: HttpTransport {
    let mode = Locked(Mode.works)
    let feed = Locked("")
    func send(request: HttpRequest) async throws -> HttpResponse {
        let body = Data(feed.get().utf8)
        return try await respond(mode.get()) { HttpResponse(status: 200, headers: ["content-type": "text/calendar"], body: body) }
    }
}

final class RecordingObserver: EngineObserver {
    let summaries = Locked<[EngineStateSummary]>([])
    let events = Locked<[EngineEvent]>([])
    let arrivals = Locked<[ContinuousClock.Instant]>([])
    let calls = Locked(0)
    func stateChanged(summary: EngineStateSummary) {
        arrivals.update { $0.append(.now) }
        summaries.update { $0.append(summary) }
        calls.update { $0 += 1 }
    }
    func event(event: EngineEvent) {
        events.update { $0.append(event) }
        calls.update { $0 += 1 }
    }
}

struct Host {
    let credentials = FakeCredentials()
    let authorization = FakeAuthorization()
    let settings = FakeSettings()
    let http = FakeHTTP()
    let observer = RecordingObserver()

    var interfaces: EngineHostInterfaces {
        EngineHostInterfaces(credentials: credentials, authorization: authorization, signIn: FakeSignIn(), settings: settings, http: http, diagnostics: nil, textGenerator: nil, externalTokens: nil, systemCalendar: nil)
    }

    func config(directory: URL) -> EngineConfig {
        EngineConfig(dataDirectory: directory.path, platform: .macos, timeZone: "Europe/Berlin")
    }
}

/// 2026-11-14T10:00:00Z, where the tests' manual clock starts.
let clockStart = Date(timeIntervalSince1970: 1_794_650_400)

func temporaryDirectory(_ name: String) -> URL {
    let url = FileManager.default.temporaryDirectory.appendingPathComponent("timetug-kit-\(name)-\(UUID().uuidString)")
    try? FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
    return url
}

func waitUntil(_ what: String, timeout: Double = 20, _ condition: @Sendable () -> Bool) async -> Bool {
    let deadline = Date().addingTimeInterval(timeout)
    while !condition() {
        if Date() > deadline { return false }
        try? await Task.sleep(nanoseconds: 5_000_000)
    }
    return true
}

/// An iCalendar feed with `count` events starting at 11:00 UTC on 14 November 2026, an hour apart.
func feed(events count: Int = 1, spacing: Double = 600, title: @Sendable (Int) -> String = { "Event \($0)" }, notes: String? = nil) -> String {
    var lines = ["BEGIN:VCALENDAR", "VERSION:2.0", "PRODID:-//test//EN"]
    for index in 0..<count {
        let start = Date(timeIntervalSince1970: 1_794_654_000 + Double(index) * spacing)
        let end = start.addingTimeInterval(1800)
        lines += ["BEGIN:VEVENT", "UID:uid-\(index)@example.com", "DTSTAMP:20261114T100000Z", "DTSTART:\(stamp(start))", "DTEND:\(stamp(end))", "SUMMARY:\(escape(title(index)))"]
        if let notes { lines.append("DESCRIPTION:\(escape(notes))") }
        lines.append("END:VEVENT")
    }
    lines.append("END:VCALENDAR")
    return lines.flatMap(fold).joined(separator: "\r\n") + "\r\n"
}

private func stamp(_ date: Date) -> String {
    let formatter = DateFormatter()
    formatter.locale = Locale(identifier: "en_US_POSIX")
    formatter.timeZone = TimeZone(identifier: "UTC")
    formatter.dateFormat = "yyyyMMdd'T'HHmmss'Z'"
    return formatter.string(from: date)
}

private func escape(_ text: String) -> String {
    text.replacingOccurrences(of: "\\", with: "\\\\").replacingOccurrences(of: ";", with: "\\;")
        .replacingOccurrences(of: ",", with: "\\,").replacingOccurrences(of: "\n", with: "\\n")
}

/// RFC 5545 line folding at 75 octets, never inside a UTF-8 sequence.
private func fold(_ line: String) -> [String] {
    var result: [String] = []
    var current = ""
    var size = 0
    for character in line {
        let width = String(character).utf8.count
        if size + width > (result.isEmpty ? 75 : 74) {
            result.append(result.isEmpty ? current : " " + current)
            current = ""
            size = 0
        }
        current.append(character)
        size += width
    }
    result.append(result.isEmpty ? current : " " + current)
    return result
}
