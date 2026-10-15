import Foundation
import Testing
@testable import TimeTugEngine

/// A clock tests drive by hand: sleepers wake only when `advance(by:)` passes their deadline.
final class ManualClock: EngineClock, @unchecked Sendable {
    private struct Waiter {
        var id: UUID
        var deadline: Date
        var continuation: CheckedContinuation<Void, Error>
    }

    private let lock = NSLock()
    private var current: Date
    private var waiters: [Waiter] = []
    /// Ids cancelled before their continuation was registered.
    private var cancelledEarly: Set<UUID> = []

    init(start: Date) { current = start }

    var pendingSleeps: Int { lock.withLock { waiters.count } }

    func now() -> Date { lock.withLock { current } }

    // Lock use is confined to synchronous helpers: NSLock.lock() is unavailable from async contexts.
    private func register(id: UUID, deadline: Date, continuation: CheckedContinuation<Void, Error>) {
        let action: (() -> Void)? = lock.withLock {
            if cancelledEarly.remove(id) != nil { return { continuation.resume(throwing: CancellationError()) } }
            if deadline <= current { return { continuation.resume() } }
            waiters.append(Waiter(id: id, deadline: deadline, continuation: continuation))
            return nil
        }
        action?()
    }

    private func cancel(id: UUID) {
        let waiter: Waiter? = lock.withLock {
            if let index = waiters.firstIndex(where: { $0.id == id }) { return waiters.remove(at: index) }
            cancelledEarly.insert(id)
            return nil
        }
        waiter?.continuation.resume(throwing: CancellationError())
    }

    private func moveForward(by interval: TimeInterval) {
        let due: [Waiter] = lock.withLock {
            current = current.addingTimeInterval(interval)
            let due = waiters.filter { $0.deadline <= current }
            waiters.removeAll { $0.deadline <= current }
            return due
        }
        for waiter in due { waiter.continuation.resume() }
    }

    func sleep(until deadline: Date) async throws {
        let id = UUID()
        try await withTaskCancellationHandler {
            try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
                register(id: id, deadline: deadline, continuation: continuation)
            }
        } onCancel: {
            cancel(id: id)
        }
    }

    func advance(by interval: TimeInterval) async {
        moveForward(by: interval)
        for _ in 0..<3 { await Task.yield() }
    }

    func waitForSleepers(_ count: Int) async {
        for _ in 0..<10_000 {
            if pendingSleeps >= count { return }
            await Task.yield()
        }
        Issue.record("timed out waiting for \(count) sleeper(s); \(pendingSleeps) pending")
    }
}
