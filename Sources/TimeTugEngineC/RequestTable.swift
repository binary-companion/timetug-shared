import Foundation
import TimeTugEngine
import TimeTugEngineProtocol

/// A host-service request that failed: the reply's error kind and message, or a timeout (`network`).
/// A `cancelled` reply is thrown as `CancellationError` instead, so the engine treats it as a cancelled operation.
struct ServiceError: Error, Equatable, CustomStringConvertible {
    var kind: ReplyErrorKind
    var message: String

    var description: String { message.isEmpty ? kind.rawValue : "\(kind.rawValue): \(message)" }
}

/// The engine's outstanding host-service requests. `request` sends one `service.*` envelope with a fresh id and
/// suspends until the host answers through `tt_engine_reply` (`fulfill`), the optional timeout passes on the engine
/// clock, the calling task is cancelled, or `failAll` closes the table (destroy). Every path resolves the request
/// exactly once; a reply for an unknown or already resolved id is ignored.
final class RequestTable: @unchecked Sendable {
    private struct Pending {
        var continuation: CheckedContinuation<JSONValue, Error>
        var timer: Task<Void, Never>?
    }

    private let send: @Sendable (Envelope) -> Void
    private let clock: any EngineClock
    private let lock = NSLock()
    private var lastID: UInt64 = 0
    private var pending: [UInt64: Pending] = [:]
    /// Ids whose caller was cancelled before the request was registered.
    private var cancelledEarly: Set<UInt64> = []
    /// Set by `failAll`: every later request fails at once with this kind and is never sent.
    private var closedKind: ReplyErrorKind?

    init(send: @escaping @Sendable (Envelope) -> Void, clock: any EngineClock) {
        self.send = send
        self.clock = clock
    }

    /// The number of requests waiting for an answer.
    var pendingCount: Int { lock.withLock { pending.count } }

    func request(type: String, payload: JSONValue?, timeout: TimeInterval?) async throws -> JSONValue {
        let id = lock.withLock { () -> UInt64 in
            lastID &+= 1
            return lastID
        }
        // The deadline is fixed now, so a clock advanced before the timer task first runs still counts.
        let deadline = timeout.map { clock.now().addingTimeInterval($0) }
        let envelope = Envelope(type: type, id: .number(id), payload: payload)
        return try await withTaskCancellationHandler {
            try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<JSONValue, Error>) in
                register(id: id, envelope: envelope, deadline: deadline, timeout: timeout, continuation: continuation)
            }
        } onCancel: {
            cancel(id: id)
        }
    }

    private func register(
        id: UInt64, envelope: Envelope, deadline: Date?, timeout: TimeInterval?,
        continuation: CheckedContinuation<JSONValue, Error>
    ) {
        enum Outcome { case send, fail(Error) }
        let outcome: Outcome = lock.withLock {
            if let kind = closedKind { return .fail(Self.error(kind: kind, message: "the engine is shutting down")) }
            if cancelledEarly.remove(id) != nil { return .fail(CancellationError()) }
            var timer: Task<Void, Never>?
            if let deadline, let timeout {
                let clock = clock
                timer = Task { [weak self] in
                    do { try await clock.sleep(until: deadline) } catch { return }
                    self?.resolve(id: id, with: .failure(ServiceError(
                        kind: .network, message: "no answer within \(Int(timeout)) seconds")))
                }
            }
            pending[id] = Pending(continuation: continuation, timer: timer)
            return .send
        }
        switch outcome {
        case .send: send(envelope)
        case .fail(let error): continuation.resume(throwing: error)
        }
    }

    private func cancel(id: UInt64) {
        let found: Pending? = lock.withLock {
            if let entry = pending.removeValue(forKey: id) { return entry }
            if id <= lastID { cancelledEarly.insert(id) }
            return nil
        }
        found?.timer?.cancel()
        found?.continuation.resume(throwing: CancellationError())
    }

    private func resolve(id: UInt64, with result: Result<JSONValue, Error>) {
        guard let entry = lock.withLock({ pending.removeValue(forKey: id) }) else { return }
        entry.timer?.cancel()
        entry.continuation.resume(with: result)
    }

    /// The host's answer to request `id`. Unknown and already resolved ids are ignored; a reply that is not a
    /// well-formed v1 reply fails the request with `failed`.
    func fulfill(id: UInt64, reply: String) {
        switch WireReply.parse(reply) {
        case .result(let value): resolve(id: id, with: .success(value))
        case .error(let kind, let message): resolve(id: id, with: .failure(Self.error(kind: kind, message: message)))
        }
    }

    /// Fails every pending request with `kind` and every later one before it is sent.
    func failAll(kind: ReplyErrorKind) {
        let entries: [Pending] = lock.withLock {
            closedKind = kind
            let entries = Array(pending.values)
            pending = [:]
            return entries
        }
        for entry in entries {
            entry.timer?.cancel()
            entry.continuation.resume(throwing: Self.error(kind: kind, message: "the engine is shutting down"))
        }
    }

    private static func error(kind: ReplyErrorKind, message: String) -> Error {
        kind == .cancelled ? CancellationError() : ServiceError(kind: kind, message: message)
    }

    /// Sends `payload` (none when nil) and decodes the result as `Result`. A result that does not decode is a
    /// `failed` error.
    func call<Result: Decodable>(
        _ type: String, payload: JSONValue? = nil, timeout: TimeInterval?, as _: Result.Type = Result.self
    ) async throws -> Result {
        let value = try await request(type: type, payload: payload, timeout: timeout)
        do {
            return try JSONDecoder().decode(Result.self, from: JSONEncoder().encode(value))
        } catch {
            throw ServiceError(kind: .failed, message: "the \(type) result has an unexpected shape")
        }
    }
}

/// A result the engine does not read (`set`, `remove`, `closeRedirect`, `prepare`).
struct IgnoredResult: Decodable {
    init(from decoder: Decoder) throws {}
}
