import Foundation
import Testing
@testable import TimeTugEngineC
import TimeTugEngineProtocol

/// A value shared between a test and the threads it observes.
final class LockedBox<T>: @unchecked Sendable {
    private let lock = NSLock()
    private var stored: T
    init(_ value: T) { stored = value }
    var value: T { lock.withLock { stored } }
    func mutate<R>(_ change: (inout T) -> R) -> R { lock.withLock { change(&stored) } }
}

/// Waits (in real time) until `condition` holds; records an issue and returns false after `timeout`.
@discardableResult
func poll(
    _ what: @autoclosure () -> String, timeout: TimeInterval = 10, sourceLocation: SourceLocation = #_sourceLocation,
    _ condition: () -> Bool
) async -> Bool {
    let deadline = Date().addingTimeInterval(timeout)
    while !condition() {
        if Date() >= deadline {
            Issue.record("timed out after \(Int(timeout)) s waiting for \(what())", sourceLocation: sourceLocation)
            return false
        }
        try? await Task.sleep(nanoseconds: 2_000_000)
    }
    return true
}

/// Runs `body` on a fresh thread, off the cooperative pool: `tt_engine_destroy` blocks its caller while the engine
/// stops, which a test must not do on a pool thread the engine itself needs.
func offPool<T: Sendable>(_ body: @escaping @Sendable () -> T) async -> T {
    await withCheckedContinuation { continuation in
        Thread { continuation.resume(returning: body()) }.start()
    }
}
