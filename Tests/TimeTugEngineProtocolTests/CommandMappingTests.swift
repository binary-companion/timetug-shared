import Foundation
import Testing
import TimeTugCore
import TimeTugEngine
@testable import TimeTugEngineProtocol

private func fail(_ message: String) { Issue.record(Comment(rawValue: message)) }

@Suite struct CommandMappingTests {
    private func decode(
        _ type: String, _ payload: JSONValue? = nil, id: MessageID? = nil, v: Int = 1,
        testing: Bool = false, state: EngineState = .empty
    ) -> CommandOutcome {
        CommandDecoder(testing: testing).decode(Envelope(v: v, type: type, id: id, payload: payload), state: state)
    }

    private func command(_ outcome: CommandOutcome) -> EngineCommand? {
        if case .engine(let c) = outcome { return c }
        return nil
    }

    private func reason(_ outcome: CommandOutcome) -> String? {
        if case .rejected(let d) = outcome {
            #expect(d.category == "command")
            return d.message
        }
        return nil
    }

    @Test func mergeWithUnknownIDIsRejectedWithDiagnostic() {
        let out = decode(
            "dedup.merge", .object(["eventIDs": .array([.string("nope"), .string("nada")])]), id: .string("c1"))
        #expect(reason(out) == "unknown event id")
    }

    @Test func mergeResolvesIDsInEventsAndCandidates() {
        let a = Fixture.timed("a"), b = Fixture.timed("b", title: "Other")
        var state = EngineState.empty
        state.events = [a]
        state.candidates = [a.id: [b]]
        let out = decode("dedup.merge", .object(["eventIDs": .array([.string(a.id), .string(b.id)])]), state: state)
        guard case .merge(let x, let y)? = command(out) else { return fail("not a merge") }
        #expect(x.id == a.id && y.id == b.id)
    }

    @Test func mergeNeedsExactlyTwoIDs() {
        #expect(reason(decode("dedup.merge", .object(["eventIDs": .array([.string("a")])]))) == "invalid payload")
    }

    @Test func separateResolvesMembersByContentKey() {
        var merged = Fixture.timed("m")
        let member = MergedMember(
            title: "t", calendarKey: "c", contentKey: "k1", details: "bare", start: merged.start, end: merged.end)
        merged.mergedMembers = [member]
        var state = EngineState.empty
        state.events = [merged]
        let ok = decode(
            "dedup.separate", .object(["eventID": .string(merged.id), "memberIDs": .array([.string("k1")])]), state: state)
        guard case .separate(let e, let members)? = command(ok) else { return fail("not a separate") }
        #expect(e.id == merged.id && members == [member])
        let bad = decode(
            "dedup.separate", .object(["eventID": .string(merged.id), "memberIDs": .array([.string("zz")])]), state: state)
        #expect(reason(bad) == "unknown member id")
    }

    @Test func settingsPatchLeavesAbsentFieldsAlone() {
        let out = decode("settings.apply", .object(["inferenceEnabled": .bool(true)]))
        guard case .applySettings(let f)? = command(out) else { return fail("not applySettings") }
        var s = EngineSettings()
        s.systemCalendarEnabled = false
        f(&s)
        #expect(s.inferenceEnabled)
        #expect(!s.systemCalendarEnabled)
    }

    @Test func settingsApplyWithNoPayloadIsANoOp() {
        guard case .applySettings(let f)? = command(decode("settings.apply")) else { return fail("rejected") }
        var s = EngineSettings()
        f(&s)
        #expect(s == EngineSettings())
    }

    @Test func advanceClockRequiresTestingFlag() {
        let payload = JSONValue.object(["seconds": .number(60)])
        #expect(reason(decode("test.advanceClock", payload)) == "test commands are disabled")
        guard case .testAdvanceClock(let s) = decode("test.advanceClock", payload, testing: true) else {
            return fail("not accepted")
        }
        #expect(s == 60)
        #expect(reason(decode("test.advanceClock", .object(["seconds": .number(-1)]), testing: true)) == "invalid payload")
    }

    @Test func simpleCommandsMap() {
        guard case .start? = command(decode("start")) else { return fail("start") }
        guard case .refresh? = command(decode("refresh")) else { return fail("refresh") }
        guard case .cancelAuthorization? = command(decode("account.cancel")) else { return fail("cancel") }
        guard case .forgetCorrections? = command(decode("dedup.forget")) else { return fail("forget") }
        guard case .addAccount(let kind)? = command(decode("account.add", .object(["kindID": .string("google")])))
        else { return fail("add") }
        #expect(kind == "google")
        guard case .reauthorizeAccount(let c)? = command(decode("account.reconnect", .object(["connectionID": .string("x")])))
        else { return fail("reconnect") }
        #expect(c == "x")
        guard case .removeAccount(let r)? = command(decode("account.remove", .object(["connectionID": .string("y")])))
        else { return fail("remove") }
        #expect(r == "y")
        guard case .setCountdownVisible(true)? = command(decode("countdown.visible", .object(["visible": .bool(true)])))
        else { return fail("visible") }
    }

    @Test func takeoverCommandsMap() {
        let id = JSONValue.string("e1")
        guard case .takeoverPresented("e1", false)? =
            command(decode("takeover.presented", .object(["eventID": id, "onScreen": .bool(false)])))
        else { return fail("presented") }
        guard case .takeoverClosed("e1")? = command(decode("takeover.closed", .object(["eventID": id])))
        else { return fail("closed") }
        guard case .join("e1")? = command(decode("takeover.join", .object(["eventID": id]))) else { return fail("join") }
        guard case .dismiss("e1")? = command(decode("takeover.dismiss", .object(["eventID": id])))
        else { return fail("dismiss") }
        guard case .snooze("e1", 300)? =
            command(decode("takeover.snooze", .object(["eventID": id, "seconds": .number(300)])))
        else { return fail("snooze") }
        guard case .testTug("T", "C")? =
            command(decode("takeover.test", .object(["title": .string("T"), "calendarTitle": .string("C")])))
        else { return fail("test") }
    }

    @Test func systemEventKindsMapAndUnknownIsRejected() {
        for (kind, expected) in [
            ("wake", SystemEvent.wake), ("clockChanged", .clockChanged), ("dayChanged", .dayChanged),
            ("networkReturned", .networkReturned), ("displayChanged", .displayChanged),
        ] {
            guard case .system(let e)? = command(decode("system.event", .object(["kind": .string(kind)])))
            else { return fail("\(kind)") }
            #expect(e == expected)
        }
        #expect(reason(decode("system.event", .object(["kind": .string("explode")]))) == "unknown system event kind")
    }

    @Test func timeZoneChangeCarriesTheZone() {
        let out = decode("system.event", .object(["kind": .string("timeZoneChanged"), "timeZone": .string("Asia/Tokyo")]))
        guard case .timeZone(let zone, .system(.timeZoneChanged)) = out else { return fail("not a zone change") }
        #expect(zone.identifier == "Asia/Tokyo")
        guard case .system(.timeZoneChanged)? = command(decode("system.event", .object(["kind": .string("timeZoneChanged")])))
        else { return fail("zone-less change") }
        let bad = decode("system.event", .object(["kind": .string("timeZoneChanged"), "timeZone": .string("Mars/X")]))
        #expect(reason(bad) == "unknown time zone")
    }

    @Test func rejectsUnknownTypeWrongVersionAndBadPayloadWithoutThrowing() {
        #expect(reason(decode("nope")) == "unknown command type")
        #expect(reason(decode("stop")) == "unknown command type")
        #expect(reason(decode("refresh", v: 2)) == "unsupported protocol version")
        #expect(reason(decode("account.add", .object(["kindID": .number(3)]))) == "invalid payload")
        #expect(reason(decode("takeover.join", .string("not an object"))) == "invalid payload")
        #expect(reason(decode("takeover.snooze", .object(["eventID": .string("e"), "seconds": .number(-5)]))) == "invalid payload")
    }
}
