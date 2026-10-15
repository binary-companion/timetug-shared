import Foundation

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
