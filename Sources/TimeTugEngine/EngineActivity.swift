import Foundation

/// Lock-protected bookkeeping of the engine's in-flight work, readable from any context. Tests use it (through
/// `@testable`) to wait for real quiescence instead of a fixed number of yields; the engine never reads it.
final class EngineActivity: @unchecked Sendable {
    enum Kind: String, CaseIterable, Sendable {
        /// A `send` that has not returned.
        case command
        /// A `refresh(sourceIDs:)` that has not returned.
        case refresh
        /// An inference resolution task that has not ended.
        case resolution
        /// An account manager message yielded to the engine and not yet handled.
        case accountMessage
        /// A source-signal follow-up pass in the refresh coordinator.
        case coordinator
    }

    private let lock = NSLock()
    private var counts: [Kind: Int] = [:]
    private var handledWakes = 0
    private var emitted = 0
    private var signals = 0
    private var stopped = false

    func begin(_ kind: Kind) { lock.withLock { counts[kind, default: 0] += 1 } }
    func end(_ kind: Kind) { lock.withLock { counts[kind, default: 0] -= 1 } }

    /// A clock sleep the engine armed returned normally and its work (or its decision to do nothing) finished.
    func wakeHandled() { lock.withLock { handledWakes += 1 } }
    func eventEmitted() { lock.withLock { emitted += 1 } }
    func signalReceived() { lock.withLock { signals += 1 } }
    func markStopped() { lock.withLock { stopped = true } }

    var wakesHandled: Int { lock.withLock { handledWakes } }
    var eventsEmitted: Int { lock.withLock { emitted } }
    var signalsReceived: Int { lock.withLock { signals } }

    /// No command, refresh, resolution, account message or coordinator pass in flight. After `stop` only commands
    /// and refreshes count (the cancelled background tasks may never report back).
    var isIdle: Bool {
        lock.withLock {
            let active: [Kind] = stopped ? [.command, .refresh] : Kind.allCases
            return active.allSatisfy { (counts[$0] ?? 0) == 0 }
        }
    }

    var summary: String {
        lock.withLock {
            let parts = Kind.allCases.map { "\($0.rawValue)=\(counts[$0] ?? 0)" }
            return parts.joined(separator: " ")
                + " wakesHandled=\(handledWakes) emitted=\(emitted) signals=\(signals) stopped=\(stopped)"
        }
    }
}
