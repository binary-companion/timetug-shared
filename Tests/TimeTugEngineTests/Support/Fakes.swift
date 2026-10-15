import Foundation
import Testing

/// A lock-protected mutable value for sharing state between a test and the tasks it starts.
final class LockedBox<T: Sendable>: @unchecked Sendable {
    private let lock = NSLock()
    private var value: T
    init(_ value: T) { self.value = value }
    func get() -> T { lock.lock(); defer { lock.unlock() }; return value }
    func set(_ newValue: T) { lock.lock(); defer { lock.unlock() }; value = newValue }
    func mutate<R>(_ body: (inout T) -> R) -> R { lock.lock(); defer { lock.unlock() }; return body(&value) }
}

/// A throwaway directory for store tests.
enum TempDir {
    /// A fresh directory, optionally holding a copy of a fixture under its own name.
    static func make(copying fixture: String? = nil) throws -> URL {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("TimeTugEngineTests-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        if let fixture {
            try FileManager.default.copyItem(at: Fixtures.url(fixture), to: dir.appendingPathComponent(fixture))
        }
        return dir
    }
}

/// Synthetic JSON files written by the Mac encoders, bundled with the test target.
enum Fixtures {
    static func url(_ name: String) -> URL {
        Bundle.module.resourceURL!.appendingPathComponent("Fixtures").appendingPathComponent(name)
    }
}

/// Polls `condition` until it holds: yields, and after the first thousand checks also sleeps a millisecond between
/// checks, so a slow machine gets real time. Gives up after `timeout` seconds of wall-clock time and records an issue
/// naming `what` (with `dump()` when given). The bound only limits a hang; it never decides an ordering.
@discardableResult
func poll(
    _ what: @autoclosure () -> String, timeout: TimeInterval = 30, dump: (() async -> String)? = nil,
    sourceLocation: SourceLocation = #_sourceLocation, _ condition: () async -> Bool
) async -> Bool {
    let deadline = Date().addingTimeInterval(timeout)
    var checks = 0
    while true {
        if await condition() { return true }
        checks += 1
        if Date() >= deadline { break }
        await Task.yield()
        if checks > 1_000 { try? await Task.sleep(nanoseconds: 1_000_000) }
    }
    let detail = await dump?() ?? ""
    Issue.record("timed out after \(Int(timeout)) s waiting for \(what()) \(detail)", sourceLocation: sourceLocation)
    return false
}
