import Foundation
@testable import TimeTugEngineC
import TimeTugEngineProtocol

/// Runs one conformance scenario (`Conformance/Scenarios/*.json`) through the four C functions, with a scripted
/// host answering the engine's service requests. The C# runner implements the same format and semantics.
///
/// Format:
/// - `name`; `config` (the `tt_engine_create` config without `dataDirectory`, which the runner supplies: a fresh
///   temporary directory per scenario, kept across `restart`); `files` (written into the data directory before the
///   first create: a JSON value is written as JSON, a string as text).
/// - `host.http`: rules tried in order; the first whose `match` is a prefix of the request URL (and whose optional
///   `method` matches) answers with `status` (default 200), `headers` and a body from `body` (any JSON, sent as
///   JSON text), `bodyText` or `bodyFile` (under `Scenarios/data/`). `"noReply": true` never answers. A request no
///   rule matches gets a 404.
/// - `host.credentials.seed`: connection id to secrets, the starting contents of the host's credential store.
/// - `host.auth`: `redirectURI` (default `http://127.0.0.1:53682/`), `code` (default `test-code`), and
///   `authorize: "never"` to leave the browser step unanswered. The authorize answer echoes the request's `state`.
/// - `host.prompt`: the values a credential form answers with.
/// - `steps`, run in order:
///   - `{"send": {...}}` a command envelope (`v` defaults to 1); `{"sendRaw": "..."}` any text;
///   - `{"expect": {"events": [...]}}` each listed object must subset-match, in order, a subsequence of the messages
///     received since the previous `events` expectation (all messages count: events, requests, `ready`, `state`);
///   - `{"expect": {"state": {...}}}` the latest `state` payload must subset-match;
///   - `{"expect": {"noMoreMessagesFor": seconds}}` nothing at all arrives for that long;
///   - `{"restart": true}` destroy, then create again on the same data directory (and the same host);
///   - `{"destroy": true}`; `{"replyToPending": true}` answers every request the host has left unanswered.
///
/// Subset match: every member an expected object names must match (others are ignored); arrays must have the same
/// length and match element by element; other values must be equal (numbers compare as numbers). Each wait for an
/// expectation gives up after `timeout` seconds of real time; the engine runs on a manual clock, so a wait is only
/// for delivery.
final class ScenarioRunner {
    struct Result {
        var name: String
        var failures: [String]
    }

    /// `TT_BASELINE_SCENARIOS=<dir>` runs an earlier release's scenarios (and their `data/`) instead: an old front
    /// end against this backend. The `compat` CI job exports the baseline tag's `Conformance/Scenarios` there.
    static let baselineDirectory: URL? = {
        guard let path = ProcessInfo.processInfo.environment["TT_BASELINE_SCENARIOS"], !path.isEmpty else { return nil }
        return URL(fileURLWithPath: path, isDirectory: true)
    }()
    static let scenarioDirectory = baselineDirectory ?? Conformance.root.appendingPathComponent("Scenarios")

    private let scenario: JSONValue
    private let name: String
    private let timeout: TimeInterval

    init(url: URL, timeout: TimeInterval = 5) throws {
        scenario = try JSONDecoder().decode(JSONValue.self, from: Data(contentsOf: url))
        name = url.deletingPathExtension().lastPathComponent
        self.timeout = timeout
    }

    init(scenario: JSONValue, timeout: TimeInterval = 5) {
        self.scenario = scenario
        if case .string(let name)? = scenario["name"] { self.name = name } else { name = "inline" }
        self.timeout = timeout
    }

    func run() async -> Result {
        let directory = temporaryDirectory()
        var failures: [String] = []
        do {
            try writeFiles(into: directory)
        } catch {
            return Result(name: name, failures: ["cannot write the scenario files: \(error)"])
        }
        let host = ScriptedHost(script: scenario["host"] ?? .object([:]))
        var config: [String: JSONValue] = [:]
        if case .object(let given)? = scenario["config"] { config = given }
        config["dataDirectory"] = .string(directory.path)
        let configJSON = Self.text(.object(config))

        func create() -> Bool {
            guard let handle = tt_engine_create(configJSON, ScriptedHost.callback, host.context) else {
                failures.append("tt_engine_create returned NULL")
                return false
            }
            host.attach(Handle(raw: handle))
            return true
        }
        guard create() else { return Result(name: name, failures: failures) }

        var cursor = 0
        guard case .array(let steps)? = scenario["steps"] else {
            return Result(name: name, failures: ["the scenario has no steps"])
        }
        for (index, step) in steps.enumerated() {
            let label = "step \(index + 1)"
            if case .object? = step["send"], let command = step["send"] {
                var envelope: [String: JSONValue] = [:]
                if case .object(let given) = command { envelope = given }
                if envelope["v"] == nil { envelope["v"] = .number(1) }
                host.send(Self.text(.object(envelope)))
            } else if case .string(let raw)? = step["sendRaw"] {
                host.send(raw)
            } else if step["restart"] == .bool(true) {
                await host.destroy()
                guard create() else { break }
            } else if step["destroy"] == .bool(true) {
                await host.destroy()
            } else if step["replyToPending"] == .bool(true) {
                host.replyToPending()
            } else if let expectation = step["expect"] {
                if case .array(let events)? = expectation["events"] {
                    if let next = await waitForEvents(events, host: host, from: cursor) {
                        cursor = next
                    } else {
                        failures.append("\(label): expected events \(Self.text(.array(events))) in order; received "
                                        + host.summary(from: cursor))
                        break
                    }
                }
                if let state = expectation["state"] {
                    let matched = await waitUntil { Self.matches(state, host.latestState) }
                    if !matched {
                        failures.append("\(label): expected state \(Self.text(state)); latest state "
                                        + (host.latestState.map(Self.text) ?? "none")
                                        + "; received since the last events expectation: " + host.summary(from: cursor))
                        break
                    }
                }
                if case .number(let seconds)? = expectation["noMoreMessagesFor"] {
                    let before = host.messageCount
                    try? await Task.sleep(nanoseconds: UInt64(seconds * 1_000_000_000))
                    if host.messageCount != before {
                        failures.append("\(label): \(host.messageCount - before) message(s) arrived: "
                                        + host.summary(from: before))
                        break
                    }
                }
            } else {
                failures.append("\(label): unknown step \(Self.text(step))")
                break
            }
        }
        await host.destroy()
        try? FileManager.default.removeItem(at: directory)
        return Result(name: name, failures: failures)
    }

    private func writeFiles(into directory: URL) throws {
        guard case .object(let files)? = scenario["files"] else { return }
        for (file, content) in files {
            let data: Data
            if case .string(let text) = content { data = Data(text.utf8) } else { data = Data(Self.text(content).utf8) }
            try data.write(to: directory.appendingPathComponent(file))
        }
    }

    /// Index after the last matched message, or nil when the events do not all arrive in time.
    private func waitForEvents(_ expected: [JSONValue], host: ScriptedHost, from cursor: Int) async -> Int? {
        var result: Int?
        _ = await waitUntil {
            result = Self.subsequence(expected, in: host.messages, from: cursor)
            return result != nil
        }
        return result
    }

    private func waitUntil(_ condition: () -> Bool) async -> Bool {
        let deadline = Date().addingTimeInterval(timeout)
        while !condition() {
            if Date() >= deadline { return false }
            try? await Task.sleep(nanoseconds: 2_000_000)
        }
        return true
    }

    static func subsequence(_ expected: [JSONValue], in messages: [JSONValue], from start: Int) -> Int? {
        var index = start
        for pattern in expected {
            guard let found = messages[index...].firstIndex(where: { matches(pattern, $0) }) else { return nil }
            index = found + 1
        }
        return index
    }

    static func matches(_ expected: JSONValue, _ actual: JSONValue?) -> Bool {
        switch expected {
        case .object(let members):
            guard case .object? = actual else { return false }
            return members.allSatisfy { key, value in
                value == .null ? (actual?[key] ?? .null) == .null : matches(value, actual?[key])
            }
        case .array(let elements):
            guard case .array(let values)? = actual, values.count == elements.count else { return false }
            return zip(elements, values).allSatisfy { matches($0, $1) }
        default:
            return expected == actual
        }
    }

    static func text(_ value: JSONValue) -> String {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys, .withoutEscapingSlashes]
        return (try? encoder.encode(value)).map { String(decoding: $0, as: UTF8.self) } ?? "null"
    }

    /// Scenario files, sorted by name.
    static func scenarioURLs() throws -> [URL] {
        try FileManager.default.contentsOfDirectory(at: scenarioDirectory, includingPropertiesForKeys: nil)
            .filter { $0.pathExtension == "json" }
            .sorted { $0.lastPathComponent < $1.lastPathComponent }
    }
}

/// The scripted host: the callback context of a scenario's engine. It records every message and answers service
/// requests from the script, from inside the callback (which the ABI allows).
final class ScriptedHost: @unchecked Sendable {
    private let script: JSONValue
    private let lock = NSLock()
    private var handle: Handle?
    /// The most recent engine, kept after destroy so `replyToPending` can answer a destroyed engine.
    private var lastHandle: Handle?
    private var received: [JSONValue] = []
    private var latest: JSONValue?
    private var credentials: [String: (secrets: [String: String], revision: String)] = [:]
    private var unanswered: [UInt64] = []
    private var redirects = 0

    init(script: JSONValue) {
        self.script = script
        if case .object(let seed)? = script["credentials"]?["seed"] {
            for (id, secrets) in seed {
                guard case .object(let values) = secrets else { continue }
                credentials[id] = (values.compactMapValues { if case .string(let s) = $0 { s } else { nil } },
                                   UUID().uuidString)
            }
        }
    }

    var context: UnsafeMutableRawPointer { Unmanaged.passUnretained(self).toOpaque() }

    static let callback: @convention(c) (UnsafeMutableRawPointer?, UnsafePointer<CChar>?) -> Void = { context, json in
        guard let context, let json else { return }
        Unmanaged<ScriptedHost>.fromOpaque(context).takeUnretainedValue().receive(String(cString: json))
    }

    func attach(_ handle: Handle) {
        lock.withLock {
            self.handle = handle
            lastHandle = handle
        }
    }

    var messages: [JSONValue] { lock.withLock { received } }
    var messageCount: Int { lock.withLock { received.count } }
    var latestState: JSONValue? { lock.withLock { latest } }

    func summary(from start: Int) -> String {
        let all = messages
        guard start < all.count else { return "nothing" }
        return all[start...].map { message -> String in
            guard case .string(let type)? = message["type"] else { return "?" }
            if type == "event.diagnostic", case .string(let text)? = message["payload"]?["message"] { return "\(type)(\(text))" }
            if type == "service.http.send", case .string(let url)? = message["payload"]?["url"] { return "\(type)(\(url))" }
            return type
        }.joined(separator: ", ")
    }

    func send(_ json: String) {
        guard let handle = lock.withLock({ handle }) else { return }
        tt_engine_send(handle.raw, json)
    }

    func destroy() async {
        guard let handle = lock.withLock({ () -> Handle? in
            defer { handle = nil }
            return handle
        }) else { return }
        await offPool { tt_engine_destroy(handle.raw) }
    }

    /// Answers every request left unanswered (after a destroy the engine must ignore these).
    func replyToPending() {
        let (handle, ids) = lock.withLock { () -> (Handle?, [UInt64]) in
            defer { unanswered = [] }
            return (lastHandle, unanswered)
        }
        // Replies go to the engine the requests came from, even when it has been destroyed meanwhile.
        guard let handle else { return }
        for id in ids { tt_engine_reply(handle.raw, id, #"{"v":1,"result":{}}"#) }
    }

    private func receive(_ text: String) {
        guard let message = try? JSONDecoder().decode(JSONValue.self, from: Data(text.utf8)) else { return }
        let handle = lock.withLock { () -> Handle? in
            received.append(message)
            if message["type"] == .string("state") { latest = message["payload"] }
            return self.handle
        }
        guard case .string(let type)? = message["type"], type.hasPrefix("service."),
              case .number(let number)? = message["id"], let handle else { return }
        let id = UInt64(number)
        if let reply = answer(type: type, payload: message["payload"] ?? .null) {
            tt_engine_reply(handle.raw, id, ScenarioRunner.text(reply))
        } else {
            lock.withLock { unanswered.append(id) }
        }
    }

    private static func result(_ value: JSONValue) -> JSONValue { .object(["v": .number(1), "result": value]) }

    private static func error(_ kind: String, _ message: String) -> JSONValue {
        .object(["v": .number(1), "error": .object(["kind": .string(kind), "message": .string(message)])])
    }

    private func string(_ value: JSONValue?) -> String? {
        if case .string(let text)? = value { return text }
        return nil
    }

    /// The scripted answer, or nil to leave the request unanswered.
    private func answer(type: String, payload: JSONValue) -> JSONValue? {
        switch type {
        case "service.http.send": return http(payload)
        case "service.credentials.get":
            let id = string(payload["connectionID"]) ?? ""
            let secrets = lock.withLock { credentials[id]?.secrets }
            return Self.result(.object(secrets.map { ["secrets": .object($0.mapValues(JSONValue.string))] } ?? [:]))
        case "service.credentials.set":
            let id = string(payload["connectionID"]) ?? ""
            var secrets: [String: String] = [:]
            if case .object(let values)? = payload["secrets"] {
                secrets = values.compactMapValues { if case .string(let s) = $0 { s } else { nil } }
            }
            lock.withLock { credentials[id] = (secrets, UUID().uuidString) }
            return Self.result(.object([:]))
        case "service.credentials.remove":
            let id = string(payload["connectionID"]) ?? ""
            _ = lock.withLock { credentials.removeValue(forKey: id) }
            return Self.result(.object([:]))
        case "service.credentials.snapshot":
            let id = string(payload["connectionID"]) ?? ""
            guard let entry = lock.withLock({ credentials[id] }) else { return Self.result(.object([:])) }
            return Self.result(.object(["snapshot": .object([
                "secrets": .object(entry.secrets.mapValues(JSONValue.string)), "revision": .string(entry.revision),
            ])]))
        case "service.credentials.updateRefreshToken":
            let id = string(payload["connectionID"]) ?? ""
            let updated = lock.withLock { () -> Bool in
                guard var entry = credentials[id], entry.revision == string(payload["expectedRevision"]) else { return false }
                entry.secrets["refresh_token"] = string(payload["token"])
                credentials[id] = entry
                return true
            }
            return Self.result(.object(["updated": .bool(updated)]))
        case "service.auth.beginRedirect":
            let number = lock.withLock { () -> Int in
                redirects += 1
                return redirects
            }
            return Self.result(.object([
                "sessionID": .string("session-\(number)"),
                "redirectURI": .string(string(script["auth"]?["redirectURI"]) ?? "http://127.0.0.1:53682/"),
            ]))
        case "service.auth.authorize":
            if string(script["auth"]?["authorize"]) == "never" { return nil }
            guard let url = string(payload["url"]).flatMap(URLComponents.init(string:)),
                  let state = url.queryItems?.first(where: { $0.name == "state" })?.value,
                  let redirect = url.queryItems?.first(where: { $0.name == "redirect_uri" })?.value,
                  var components = URLComponents(string: redirect) else {
                return Self.error("failed", "no state or redirect in the authorization URL")
            }
            components.queryItems = [
                URLQueryItem(name: "code", value: string(script["auth"]?["code"]) ?? "test-code"),
                URLQueryItem(name: "state", value: state),
            ]
            return Self.result(.object(["url": .string(components.string ?? redirect)]))
        case "service.auth.closeRedirect", "service.signIn.prepare":
            return Self.result(.object([:]))
        case "service.auth.promptCredentials":
            guard case .object(let values)? = script["prompt"] else { return Self.error("cancelled", "no form answer") }
            return Self.result(.object(["values": .object(values)]))
        case "service.signIn.lastNonSecretValues":
            return Self.result(.object([:]))
        case "service.text.availability":
            return Self.result(.object(["kind": .string("unavailable"), "reason": .string("scripted host")]))
        default:
            return Self.error("notFound", "the scripted host has no \(type)")
        }
    }

    private func http(_ payload: JSONValue) -> JSONValue? {
        let url = string(payload["url"]) ?? ""
        let method = string(payload["method"]) ?? "GET"
        guard case .array(let rules)? = script["http"] else { return notFound }
        for rule in rules {
            guard let match = string(rule["match"]), url.hasPrefix(match) else { continue }
            if let wanted = string(rule["method"]), wanted != method { continue }
            if rule["noReply"] == .bool(true) { return nil }
            var body = Data()
            if let text = string(rule["bodyText"]) {
                body = Data(text.utf8)
            } else if let file = string(rule["bodyFile"]) {
                body = (try? Data(contentsOf: ScenarioRunner.scenarioDirectory.appendingPathComponent("data")
                    .appendingPathComponent(file))) ?? Data()
            } else if let value = rule["body"] {
                body = Data(ScenarioRunner.text(value).utf8)
            }
            var reply: [String: JSONValue] = [
                "status": rule["status"] ?? .number(200), "body": .string(body.base64EncodedString()),
            ]
            reply["headers"] = rule["headers"] ?? .object(["content-type": .string("application/json")])
            return Self.result(.object(reply))
        }
        return notFound
    }

    private var notFound: JSONValue {
        Self.result(.object(["status": .number(404), "headers": .object([:]), "body": .string("")]))
    }
}

/// Where the shared conformance files live: the package's `Conformance/` directory, found from this file's path.
enum Conformance {
    static let root: URL = {
        var url = URL(fileURLWithPath: #filePath)
        for _ in 0..<12 where url.lastPathComponent != "Tests" { url.deleteLastPathComponent() }
        return url.deletingLastPathComponent().appendingPathComponent("Conformance")
    }()
}
