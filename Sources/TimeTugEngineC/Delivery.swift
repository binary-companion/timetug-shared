import Foundation

/// The host's `tt_message_fn` and its context pointer. The engine never reads the context; it hands it back.
struct HostCallback: @unchecked Sendable {
    typealias Function = @convention(c) (UnsafeMutableRawPointer?, UnsafePointer<CChar>?) -> Void
    let function: Function
    let context: UnsafeMutableRawPointer?

    func call(_ message: String) {
        message.withCString { function(context, $0) }
    }
}

/// Delivers messages to the host on one dedicated thread, one at a time, in order.
///
/// - Events and service requests are queued without limit and never dropped or reordered.
/// - States coalesce: a new state replaces the one still waiting (if any) and moves to the back of the queue, so the
///   host always gets the newest state, never a state older than an event queued before it.
/// - `close` stops delivery. Called from outside a callback with `waitForCallback`, it returns only when no callback
///   is running, and none starts after it. Called from inside a callback, no further callback starts after the
///   current one returns.
///
/// A thread (not a task) carries the callbacks, so a slow host blocks only its own engine's delivery and never a
/// thread of Swift's cooperative pool.
final class Delivery: @unchecked Sendable {
    private enum Item {
        case message(String)
        case state
    }

    private let callback: HostCallback
    private let condition = NSCondition()
    private var items: [Item] = []
    private var pendingState: String?
    private var closed = false
    private var inCallback = false
    private var thread: Thread?

    init(callback: HostCallback) {
        self.callback = callback
    }

    /// Starts the delivery thread. Called once, after the session is fully built.
    func start() {
        let thread = Thread { [self] in run() }
        thread.name = "TimeTug engine delivery"
        thread.start()
    }

    func enqueue(_ message: String) {
        condition.lock()
        defer { condition.unlock() }
        guard !closed else { return }
        items.append(.message(message))
        condition.signal()
    }

    func enqueueState(_ message: String) {
        condition.lock()
        defer { condition.unlock() }
        guard !closed else { return }
        if pendingState != nil { items.removeAll { if case .state = $0 { true } else { false } } }
        pendingState = message
        items.append(.state)
        condition.signal()
    }

    /// True on the delivery thread, that is inside one of this engine's callbacks.
    var isDeliveryThread: Bool {
        condition.lock()
        defer { condition.unlock() }
        return thread != nil && thread === Thread.current
    }

    /// Stops delivery; see the type's documentation. Idempotent.
    func close(waitForCallback: Bool) {
        condition.lock()
        defer { condition.unlock() }
        closed = true
        items = []
        pendingState = nil
        condition.broadcast()
        if waitForCallback {
            while inCallback { condition.wait() }
        }
    }

    private func run() {
        condition.lock()
        thread = Thread.current
        while true {
            while items.isEmpty, !closed { condition.wait() }
            if closed { break }
            let message: String
            switch items.removeFirst() {
            case .message(let text): message = text
            case .state:
                guard let state = pendingState else { continue }
                pendingState = nil
                message = state
            }
            inCallback = true
            condition.unlock()
            callback.call(message)
            condition.lock()
            inCallback = false
            condition.broadcast()
        }
        thread = nil
        condition.unlock()
    }
}

/// Holds every outgoing message until `ready` has been delivered (the spec makes `ready` the first callback), then
/// passes them to `Delivery` in order. Diagnostics from building the engine (an unreadable settings file) therefore
/// arrive right after `ready`.
final class Outbox: @unchecked Sendable {
    private let delivery: Delivery
    private let lock = NSLock()
    private var open = false
    private var held: [String] = []

    init(delivery: Delivery) { self.delivery = delivery }

    func post(_ message: String) {
        lock.withLock {
            if open { delivery.enqueue(message) } else { held.append(message) }
        }
    }

    /// States are only produced after `ready` (the state pump starts with it), so they never wait here.
    func postState(_ message: String) {
        lock.withLock {
            if open { delivery.enqueueState(message) }
        }
    }

    /// Delivers `ready`, then everything held. Returns false (and does nothing) if it already ran.
    @discardableResult
    func openWith(ready: String) -> Bool {
        lock.withLock {
            guard !open else { return false }
            open = true
            delivery.enqueue(ready)
            for message in held { delivery.enqueue(message) }
            held = []
            return true
        }
    }
}
