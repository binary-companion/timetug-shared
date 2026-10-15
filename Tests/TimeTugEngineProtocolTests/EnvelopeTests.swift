import Foundation
import Testing
@testable import TimeTugEngineProtocol

@Suite struct EnvelopeTests {
    @Test func envelopeIgnoresUnknownFields() throws {
        let json = #"{"v":1,"type":"refresh","extra":{"a":1},"payload":null}"#
        let e = try JSONDecoder().decode(Envelope.self, from: Data(json.utf8))
        #expect(e.type == "refresh")
        #expect(e.v == 1)
        #expect(e.payload == nil)
    }

    @Test func idAcceptsNumberAndString() throws {
        #expect(try JSONDecoder().decode(MessageID.self, from: Data("7".utf8)) == .number(7))
        #expect(try JSONDecoder().decode(MessageID.self, from: Data(#""cmd-7""#.utf8)) == .string("cmd-7"))
    }

    @Test func idRoundTrips() throws {
        for id in [MessageID.number(18_446_744_073_709_551_615), .string("x")] {
            let data = try JSONEncoder().encode(id)
            #expect(try JSONDecoder().decode(MessageID.self, from: data) == id)
        }
    }

    @Test func jsonValueRoundTripsEveryShape() throws {
        let value: JSONValue = .object([
            "n": .null, "b": .bool(true), "d": .number(1.5), "s": .string("a/b"),
            "a": .array([.number(1), .bool(false), .object([:])]),
        ])
        let data = try JSONEncoder().encode(value)
        #expect(try JSONDecoder().decode(JSONValue.self, from: data) == value)
    }

    @Test func booleanAndNumberStayDistinct() throws {
        let value = try JSONDecoder().decode(JSONValue.self, from: Data(#"[true,1,0,false]"#.utf8))
        #expect(value == .array([.bool(true), .number(1), .number(0), .bool(false)]))
    }

    @Test func envelopeOmitsAbsentFields() throws {
        let data = try JSONEncoder().encode(Envelope(type: "start"))
        let value = try JSONDecoder().decode(JSONValue.self, from: data)
        #expect(value == .object(["v": .number(1), "type": .string("start")]))
    }

    @Test func configRejectsUnsupportedVersionAndEmptyDirectory() {
        #expect(throws: (any Error).self) { try WireConfig.parse(#"{"v":2,"dataDirectory":"/x","platform":"linux"}"#) }
        #expect(throws: (any Error).self) { try WireConfig.parse(#"{"v":1,"dataDirectory":"","platform":"linux"}"#) }
        #expect(throws: (any Error).self) { try WireConfig.parse(#"{"v":1,"dataDirectory":"/x","platform":"beos"}"#) }
        #expect(throws: (any Error).self) {
            try WireConfig.parse(#"{"v":1,"dataDirectory":"/x","platform":"linux","timeZone":"Mars/Olympus"}"#)
        }
        #expect(throws: (any Error).self) { try WireConfig.parse("{not json") }
    }

    @Test func configDefaults() throws {
        let c = try WireConfig.parse(#"{"v":1,"dataDirectory":"/x","platform":"windows"}"#)
        #expect(!c.testing)
        #expect(!c.textGeneration)
        #expect(c.timeZone == nil)
        #expect(c.connectors.isEmpty)
    }

    @Test func configReadsEveryField() throws {
        let c = try WireConfig.parse(#"""
        {"v":1,"dataDirectory":"/x","platform":"macos","testing":true,"textGeneration":true,
         "timeZone":"America/Chicago","connectors":{"google":{"clientID":"abc"}},"unknown":1}
        """#)
        #expect(c.testing && c.textGeneration)
        #expect(c.timeZone == "America/Chicago")
        #expect(c.connectors["google"]?.clientID == "abc")
    }
}
