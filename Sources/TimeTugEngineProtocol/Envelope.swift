import Foundation

/// A command id chosen by the host (number or string). The engine echoes it on any diagnostic the command causes.
public enum MessageID: Codable, Equatable, Sendable {
    case number(UInt64)
    case string(String)

    public init(from decoder: Decoder) throws {
        let c = try decoder.singleValueContainer()
        if let n = try? c.decode(UInt64.self) { self = .number(n) }
        else { self = .string(try c.decode(String.self)) }
    }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.singleValueContainer()
        switch self {
        case .number(let n): try c.encode(n)
        case .string(let s): try c.encode(s)
        }
    }
}

/// `{"v":1,"type":...,"id":...,"payload":...}`. Unknown fields are ignored on decode (adding a field is not breaking).
public struct Envelope: Codable, Equatable, Sendable {
    public var v: Int
    public var type: String
    public var id: MessageID?
    public var payload: JSONValue?

    public init(v: Int = 1, type: String, id: MessageID? = nil, payload: JSONValue? = nil) {
        self.v = v
        self.type = type
        self.id = id
        self.payload = payload
    }

    private enum CodingKeys: String, CodingKey { case v, type, id, payload }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        v = try c.decode(Int.self, forKey: .v)
        type = try c.decode(String.self, forKey: .type)
        id = try c.decodeIfPresent(MessageID.self, forKey: .id)
        payload = try c.decodeIfPresent(JSONValue.self, forKey: .payload)
        // A JSON null payload and an absent one both mean "no payload".
        if payload == .null { payload = nil }
    }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encode(v, forKey: .v)
        try c.encode(type, forKey: .type)
        try c.encodeIfPresent(id, forKey: .id)
        try c.encodeIfPresent(payload, forKey: .payload)
    }
}

public enum ProtocolVersion {
    public static let supported: [Int] = [1]
}
