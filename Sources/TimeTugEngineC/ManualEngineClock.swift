import Foundation
import TimeTugEngine

/// The clock of a `testing: true` engine: time moves only when the host sends `test.advanceClock`. Sleepers whose
/// deadline the clock reaches wake in deadline order. Shipped in the library (not a test target) because the tested
/// library is the one that ships; without `testing` the engine uses `SystemClock` and the command is rejected.
final class ManualEngineClock: EngineClock, @unchecked Sendable {
    private struct Waiter {
        var id: UInt64
        var deadline: Date
        var continuation: CheckedContinuation<Void, Error>
    }

    private let lock = NSLock()
    private var current: Date
    private var waiters: [Waiter] = []
    private var lastID: UInt64 = 0
    /// Ids issued by `sleep` whose continuation is not registered yet.
    private var unregistered: Set<UInt64> = []
    /// Ids cancelled before their continuation was registered (always a subset of `unregistered`).
    private var cancelledEarly: Set<UInt64> = []

    init(start: Date) { current = start }

    func now() -> Date { lock.withLock { current } }

    /// Ids held for cancellation bookkeeping; zero once every caller has finished.
    var trackedIDCount: Int { lock.withLock { unregistered.count + cancelledEarly.count } }

    /// Sleeps that have not woken yet.
    var pendingSleeps: Int { lock.withLock { waiters.count } }

    func sleep(until deadline: Date) async throws {
        let id = lock.withLock { () -> UInt64 in
            lastID &+= 1
            unregistered.insert(lastID)
            return lastID
        }
        try await withTaskCancellationHandler {
            try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
                register(id: id, deadline: deadline, continuation: continuation)
            }
        } onCancel: {
            cancel(id: id)
        }
    }

    private func register(id: UInt64, deadline: Date, continuation: CheckedContinuation<Void, Error>) {
        enum Action { case wait, resume, cancel }
        let action: Action = lock.withLock {
            unregistered.remove(id)
            if cancelledEarly.remove(id) != nil { return .cancel }
            if deadline <= current { return .resume }
            waiters.append(Waiter(id: id, deadline: deadline, continuation: continuation))
            return .wait
        }
        switch action {
        case .wait: break
        case .resume: continuation.resume()
        case .cancel: continuation.resume(throwing: CancellationError())
        }
    }

    /// The cancellation handler's work; internal so tests can run it late, after the answer.
    func cancel(id: UInt64) {
        let waiter: Waiter? = lock.withLock {
            if let index = waiters.firstIndex(where: { $0.id == id }) { return waiters.remove(at: index) }
            // Only a sleep not registered yet is marked; one already woken needs nothing.
            if unregistered.contains(id) { cancelledEarly.insert(id) }
            return nil
        }
        waiter?.continuation.resume(throwing: CancellationError())
    }

    /// Moves time forward by `seconds` (never backward) and wakes every sleeper whose deadline has passed.
    func advance(by seconds: TimeInterval) {
        let due: [Waiter] = lock.withLock {
            current = current.addingTimeInterval(max(0, seconds))
            let due = waiters.filter { $0.deadline <= current }.sorted { $0.deadline < $1.deadline }
            waiters.removeAll { $0.deadline <= current }
            return due
        }
        for waiter in due { waiter.continuation.resume() }
    }
}
