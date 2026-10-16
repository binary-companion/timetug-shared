import Foundation

/// Maps the opaque handles the C interface returns to live sessions. A handle is a token (an increasing integer
/// carried in a pointer), never a real pointer and never reused, so `NULL`, a destroyed handle and a made-up value
/// are all simply not found. The table is the only process-wide state; each engine's state lives in its session.
final class HandleRegistry: @unchecked Sendable {
    static let shared = HandleRegistry()

    private let lock = NSLock()
    private var lastToken: UInt = 0
    private var sessions: [UInt: EngineSession] = [:]

    func insert(_ session: EngineSession) -> OpaquePointer {
        lock.withLock {
            // Tokens start well above zero and step by 16 so a handle never looks like a small integer or NULL.
            lastToken &+= 16
            sessions[lastToken] = session
            return OpaquePointer(bitPattern: lastToken)!
        }
    }

    func session(for handle: OpaquePointer?) -> EngineSession? {
        guard let handle else { return nil }
        let token = UInt(bitPattern: handle)
        return lock.withLock { sessions[token] }
    }

    /// Removes and returns the session; nil when the handle is unknown or already removed.
    func remove(_ handle: OpaquePointer?) -> EngineSession? {
        guard let handle else { return nil }
        let token = UInt(bitPattern: handle)
        return lock.withLock { sessions.removeValue(forKey: token) }
    }
}
