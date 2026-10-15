import Foundation
import TimeTugCore
import TimeTugEngine

/// What a host command decodes to.
public enum CommandOutcome: Sendable {
    case engine(EngineCommand)
    /// `system.event` with `kind: timeZoneChanged` and a zone: the session adopts the zone, then sends the command.
    case timeZone(TimeZone, then: EngineCommand)
    case testAdvanceClock(seconds: Double)
    /// The command was not understood or not applicable. The diagnostic has no id; the session adds the command's.
    case rejected(EngineDiagnostic)
}

/// Decodes host command envelopes into engine commands. Never throws and never traps: anything it cannot use is
/// `.rejected` with a diagnostic whose message holds no calendar content.
public struct CommandDecoder: Sendable {
    private let testing: Bool

    public init(testing: Bool) { self.testing = testing }

    /// `state` is the latest published state; event ids resolve against `state.events` and the values of
    /// `state.candidates`.
    public func decode(_ envelope: Envelope, state: EngineState) -> CommandOutcome {
        guard ProtocolVersion.supported.contains(envelope.v) else { return reject("unsupported protocol version") }
        do {
            return try decodeKnown(envelope, state: state)
        } catch let rejection as Rejection {
            return reject(rejection.message)
        } catch {
            return reject("invalid payload")
        }
    }

    private struct Rejection: Error { var message: String }

    private func reject(_ message: String) -> CommandOutcome {
        .rejected(EngineDiagnostic(level: .warning, category: "command", message: message))
    }

    private func payload<T: Decodable>(_ envelope: Envelope, as _: T.Type = T.self) throws -> T {
        let value = envelope.payload ?? .object([:])
        let data = try JSONEncoder().encode(value)
        return try JSONDecoder().decode(T.self, from: data)
    }

    private struct ID: Decodable { var eventID: String }
    private struct Kind: Decodable { var kindID: String }
    private struct Connection: Decodable { var connectionID: String }
    private struct Visible: Decodable { var visible: Bool }
    private struct Presented: Decodable { var eventID: String; var onScreen: Bool }
    private struct Snooze: Decodable { var eventID: String; var seconds: Double }
    private struct TestTug: Decodable { var title: String; var calendarTitle: String }
    private struct Merge: Decodable { var eventIDs: [String] }
    private struct Separate: Decodable { var eventID: String; var memberIDs: [String] }
    private struct System: Decodable { var kind: String; var timeZone: String? }
    private struct Advance: Decodable { var seconds: Double }

    private func event(_ id: String, in state: EngineState) throws -> TimeTugCalendarEvent {
        if let found = state.events.first(where: { $0.id == id }) { return found }
        for list in state.candidates.values {
            if let found = list.first(where: { $0.id == id }) { return found }
        }
        throw Rejection(message: "unknown event id")
    }

    private func decodeKnown(_ envelope: Envelope, state: EngineState) throws -> CommandOutcome {
        switch envelope.type {
        case "start": return .engine(.start)
        case "refresh": return .engine(.refresh)
        case "account.add": return .engine(.addAccount(kindID: try payload(envelope, as: Kind.self).kindID))
        case "account.reconnect":
            return .engine(.reauthorizeAccount(connectionID: try payload(envelope, as: Connection.self).connectionID))
        case "account.remove":
            return .engine(.removeAccount(connectionID: try payload(envelope, as: Connection.self).connectionID))
        case "account.cancel": return .engine(.cancelAuthorization)
        case "settings.apply":
            let patch: WireSettingsPatch = try payload(envelope)
            return .engine(.applySettings { patch.apply(to: &$0) })
        case "countdown.visible":
            return .engine(.setCountdownVisible(try payload(envelope, as: Visible.self).visible))
        case "takeover.presented":
            let p: Presented = try payload(envelope)
            return .engine(.takeoverPresented(eventID: p.eventID, onScreen: p.onScreen))
        case "takeover.closed": return .engine(.takeoverClosed(eventID: try payload(envelope, as: ID.self).eventID))
        case "takeover.join": return .engine(.join(eventID: try payload(envelope, as: ID.self).eventID))
        case "takeover.dismiss": return .engine(.dismiss(eventID: try payload(envelope, as: ID.self).eventID))
        case "takeover.snooze":
            let p: Snooze = try payload(envelope)
            guard p.seconds.isFinite, p.seconds >= 0 else { throw Rejection(message: "invalid payload") }
            return .engine(.snooze(eventID: p.eventID, seconds: p.seconds))
        case "takeover.test":
            let p: TestTug = try payload(envelope)
            return .engine(.testTug(title: p.title, calendarTitle: p.calendarTitle))
        case "dedup.merge":
            let p: Merge = try payload(envelope)
            guard p.eventIDs.count == 2 else { throw Rejection(message: "invalid payload") }
            return .engine(.merge(try event(p.eventIDs[0], in: state), try event(p.eventIDs[1], in: state)))
        case "dedup.unmerge":
            return .engine(.unmerge(try event(try payload(envelope, as: ID.self).eventID, in: state)))
        case "dedup.separate":
            let p: Separate = try payload(envelope)
            let merged = try event(p.eventID, in: state)
            let members = try p.memberIDs.map { id in
                guard let member = merged.mergedMembers.first(where: { $0.contentKey == id }) else {
                    throw Rejection(message: "unknown member id")
                }
                return member
            }
            return .engine(.separate(merged, members: members))
        case "dedup.forget": return .engine(.forgetCorrections)
        case "system.event": return try systemEvent(payload(envelope))
        case "test.advanceClock":
            guard testing else { throw Rejection(message: "test commands are disabled") }
            let p: Advance = try payload(envelope)
            guard p.seconds.isFinite, p.seconds >= 0 else { throw Rejection(message: "invalid payload") }
            return .testAdvanceClock(seconds: p.seconds)
        default:
            throw Rejection(message: "unknown command type")
        }
    }

    private func systemEvent(_ p: System) throws -> CommandOutcome {
        let kind: SystemEvent
        switch p.kind {
        case "wake": kind = .wake
        case "clockChanged": kind = .clockChanged
        case "timeZoneChanged": kind = .timeZoneChanged
        case "dayChanged": kind = .dayChanged
        case "networkReturned": kind = .networkReturned
        case "displayChanged": kind = .displayChanged
        default: throw Rejection(message: "unknown system event kind")
        }
        if kind == .timeZoneChanged, let id = p.timeZone {
            guard let zone = TimeZone(identifier: id) else { throw Rejection(message: "unknown time zone") }
            return .timeZone(zone, then: .system(kind))
        }
        return .engine(.system(kind))
    }
}
