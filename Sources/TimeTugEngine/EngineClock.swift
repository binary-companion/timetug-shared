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
        // Sleep in bounded chunks: converting a huge interval to nanoseconds would trap.
        while true {
            let seconds = deadline.timeIntervalSinceNow
            guard seconds > 0 else { return }
            let chunk = min(seconds, 86_400)
            try await Task.sleep(nanoseconds: UInt64(chunk * 1_000_000_000))
        }
    }
}
