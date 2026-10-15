import Foundation

/// The only place the engine reads time. Hosts normally do not supply one; tests supply a manual clock.
public protocol EngineClock: Sendable {
    func now() -> Date
    /// Returns when `now()` reaches `deadline`; returns at once for a past deadline; throws `CancellationError` if cancelled.
    func sleep(until deadline: Date) async throws
}

public struct SystemClock: EngineClock {
    public init() {}
    public func now() -> Date { Date() }
    public func sleep(until deadline: Date) async throws {
        let seconds = deadline.timeIntervalSinceNow
        guard seconds > 0 else { return }
        try await Task.sleep(nanoseconds: UInt64(seconds * 1_000_000_000))
    }
}
