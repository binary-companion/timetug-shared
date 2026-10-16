import Foundation

// The four exported functions of include/timetug_engine.h. This module is the only place with @_cdecl (the
// architecture check enforces it). Every function tolerates NULL arguments and unknown handles.

/// Builds an engine (not started) from a UTF-8 JSON config. Returns NULL, without calling `on_message`, for a config
/// that is not valid JSON or fails validation, or when `on_message` is NULL.
@_cdecl("tt_engine_create")
public func tt_engine_create(
    _ configJSON: UnsafePointer<CChar>?,
    _ onMessage: (@convention(c) (UnsafeMutableRawPointer?, UnsafePointer<CChar>?) -> Void)?,
    _ context: UnsafeMutableRawPointer?
) -> OpaquePointer? {
    guard let configJSON, let onMessage else { return nil }
    let callback = HostCallback(function: onMessage, context: context)
    guard let session = EngineSession.make(configJSON: String(cString: configJSON), callback: callback) else {
        return nil
    }
    return HandleRegistry.shared.insert(session)
}

/// Enqueues a command (copied before returning). Never blocks; safe from any thread and from inside `on_message`.
@_cdecl("tt_engine_send")
public func tt_engine_send(_ engine: OpaquePointer?, _ commandJSON: UnsafePointer<CChar>?) {
    guard let session = HandleRegistry.shared.session(for: engine) else { return }
    session.send(commandJSON.map { String(cString: $0) } ?? "")
}

/// Answers the service request `requestID` (copied before returning). Unknown or already answered ids are ignored.
@_cdecl("tt_engine_reply")
public func tt_engine_reply(_ engine: OpaquePointer?, _ requestID: UInt64, _ replyJSON: UnsafePointer<CChar>?) {
    guard let session = HandleRegistry.shared.session(for: engine) else { return }
    session.reply(requestID: requestID, json: replyJSON.map { String(cString: $0) } ?? "")
}

/// Stops and releases the engine. Outside a callback: no callback runs after it returns. Inside one: returns at once
/// and no further callback starts. A NULL, unknown or already destroyed handle is ignored.
@_cdecl("tt_engine_destroy")
public func tt_engine_destroy(_ engine: OpaquePointer?) {
    HandleRegistry.shared.remove(engine)?.shutdown()
}
