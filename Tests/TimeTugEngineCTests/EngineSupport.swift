import Foundation
import Testing
@testable import TimeTugEngineC
import TimeTugEngineProtocol

/// A fresh, empty data directory.
func temporaryDirectory() -> URL {
    let url = FileManager.default.temporaryDirectory.appendingPathComponent("tt-c-\(UUID().uuidString)")
    try? FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
    return url
}

/// A `testing: true` config in `directory` (a new temporary directory when nil), as JSON text.
func testConfig(directory: URL? = nil, extra: [String: JSONValue] = [:]) -> String {
    var object: [String: JSONValue] = [
        "v": .number(1), "dataDirectory": .string((directory ?? temporaryDirectory()).path),
        "platform": .string("linux"), "testing": .bool(true),
        "testClockStart": .string("2026-10-16T13:00:00.000Z"), "timeZone": .string("Europe/Berlin"),
    ]
    for (key, value) in extra { object[key] = value }
    return String(decoding: try! JSONEncoder().encode(JSONValue.object(object)), as: UTF8.self)
}

/// Collects every message an engine delivers. Passed to `tt_engine_create` as the context; `callback` is the C
/// function that forwards to it.
final class MessageCollector: @unchecked Sendable {
    private let lock = NSLock()
    private var stored: [String] = []
    /// Runs inside the callback, on the delivery thread, after the message is recorded.
    private var hook: (@Sendable (String) -> Void)?

    var messages: [String] { lock.withLock { stored } }
    var count: Int { lock.withLock { stored.count } }

    func setHook(_ hook: (@Sendable (String) -> Void)?) { lock.withLock { self.hook = hook } }

    func receive(_ text: String) {
        let hook = lock.withLock { () -> (@Sendable (String) -> Void)? in
            stored.append(text)
            return self.hook
        }
        hook?(text)
    }

    /// Parsed envelopes (type, id and payload) in delivery order.
    var envelopes: [Envelope] {
        messages.compactMap { try? JSONDecoder().decode(Envelope.self, from: Data($0.utf8)) }
    }

    var types: [String] { envelopes.map(\.type) }

    var context: UnsafeMutableRawPointer { Unmanaged.passUnretained(self).toOpaque() }

    static let callback: @convention(c) (UnsafeMutableRawPointer?, UnsafePointer<CChar>?) -> Void = { context, json in
        guard let context, let json else { return }
        Unmanaged<MessageCollector>.fromOpaque(context).takeUnretainedValue().receive(String(cString: json))
    }
}

/// A handle as a sendable value, so tests can pass it to `offPool`.
struct Handle: @unchecked Sendable {
    let raw: OpaquePointer
}

/// Creates a test engine delivering into `collector`; the collector must outlive the engine.
func createTestEngine(_ collector: MessageCollector, config: String = testConfig()) -> Handle? {
    tt_engine_create(config, MessageCollector.callback, collector.context).map(Handle.init)
}

func send(_ handle: Handle, _ json: String) { tt_engine_send(handle.raw, json) }

func destroy(_ handle: Handle) async { await offPool { tt_engine_destroy(handle.raw) } }

/// Waits until `collector` has received nothing new for `quiet` seconds (a test engine on a manual clock goes quiet
/// once its commands are handled).
func settle(_ collector: MessageCollector, quiet: TimeInterval = 0.15, timeout: TimeInterval = 10) async {
    let deadline = Date().addingTimeInterval(timeout)
    var count = collector.count
    var since = Date()
    while Date() < deadline {
        try? await Task.sleep(nanoseconds: 10_000_000)
        let now = collector.count
        if now != count {
            count = now
            since = Date()
        } else if Date().timeIntervalSince(since) >= quiet {
            return
        }
    }
    Issue.record("the engine did not go quiet within \(Int(timeout)) s")
}
