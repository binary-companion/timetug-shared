import CalendarCore
import Foundation
import Testing
import TimeTugCore
import TimeTugEngine
@testable import TimeTugEngineProtocol

@Suite struct MappingTests {
    @Test func everyEngineStatePropertyHasAWireField() {
        let engine = Set(Mirror(reflecting: EngineState.empty).children.compactMap(\.label))
        let wire = Set(WireState.fieldNames)
        #expect(engine.subtracting(wire).isEmpty, "wire state is missing engine fields")
        #expect(wire.subtracting(engine).isEmpty, "wire state has fields the engine does not")
    }

    @Test func emptyStateEncodesEveryFieldName() throws {
        let envelope = wireMessage(for: .empty)
        #expect(envelope.type == "state")
        guard case .object(let fields)? = envelope.payload else { Issue.record("no payload"); return }
        // nextTakeover is absent when nil; every other field is present.
        #expect(Set(fields.keys) == Set(WireState.fieldNames).subtracting(["nextTakeover"]))
    }

    @Test func allDayEventUsesDatesNotInstants() {
        let wire = WireEvent(Fixture.allDay())
        #expect(wire.startDate == "2026-10-16")
        #expect(wire.endDate == "2026-10-17")
        #expect(wire.start == nil && wire.end == nil)
        #expect(wire.timeZone == "Europe/Berlin")
        #expect(wire.isAllDay)
    }

    @Test func allDayDatesFollowTheEventZoneNotUTC() {
        let zone = TimeZone(identifier: "Pacific/Auckland")!
        let wire = WireEvent(Fixture.allDay(zone: zone))
        #expect(wire.startDate == "2026-10-16")
        #expect(wire.endDate == "2026-10-17")
        #expect(wire.timeZone == "Pacific/Auckland")
    }

    @Test func timedEventCarriesInstantsAndNoDates() {
        let wire = WireEvent(Fixture.timed(conference: "https://zoom.us/j/123"))
        #expect(wire.start == "2025-10-09T08:53:20.000Z")
        #expect(wire.end == "2025-10-09T09:23:20.000Z")
        #expect(wire.startDate == nil)
        #expect(wire.timeZone == "Europe/Berlin")
        #expect(wire.conferences.map(\.url) == ["https://zoom.us/j/123"])
    }

    @Test func eventWithNoConferenceLinkHasEmptyList() {
        #expect(WireEvent(Fixture.timed()).conferences.isEmpty)
    }

    @Test func mergedMembersAreListedByContentKey() {
        var event = Fixture.timed()
        let member = MergedMember(
            title: "Design review", calendarKey: "fake/other", contentKey: "design review|1760000000|1760001800",
            details: "bare", start: event.start, end: event.end.addingTimeInterval(600))
        event.mergedMembers = [member]
        event.mergeProvenance = .inference(engineID: "m", engineName: "Model")
        let wire = WireEvent(event)
        #expect(wire.members.count == 1)
        #expect(wire.members[0].memberID == member.contentKey)
        #expect(wire.members[0].end == "2025-10-09T09:33:20.000Z")
        #expect(wire.mergeProvenance == WireProvenance(kind: "inference", engineID: "m", engineName: "Model"))
    }

    @Test func datesAreUTCMilliseconds() {
        #expect(WireDates.string(Date(timeIntervalSince1970: 1_760_000_000.123)) == "2025-10-09T08:53:20.123Z")
        #expect(WireDates.string(Date(timeIntervalSince1970: 0)) == "1970-01-01T00:00:00.000Z")
        #expect(WireDates.string(Date(timeIntervalSince1970: -1.5)) == "1969-12-31T23:59:58.500Z")
        #expect(WireDates.string(Date(timeIntervalSince1970: 951_782_400)) == "2000-02-29T00:00:00.000Z")
    }

    @Test func datesParseWhatTheyWrite() {
        for t in [0.0, 1_760_000_000.123, -86_400.25, 4_102_444_800] {
            let d = Date(timeIntervalSince1970: t)
            let parsed = WireDates.date(WireDates.string(d))
            #expect(parsed != nil && abs(parsed!.timeIntervalSince(d)) < 0.001)
        }
        #expect(WireDates.date("2025-10-09T08:53:20Z") == Date(timeIntervalSince1970: 1_760_000_000))
        #expect(WireDates.date("garbage") == nil)
        #expect(WireDates.date("2025-13-09T08:53:20Z") == nil)
    }

    @Test func diagnosticCarriesNoPayloadID() {
        let env = wireMessage(for: .diagnostic(EngineDiagnostic(level: .info, category: "x", message: "m")))
        #expect(env.type == "event.diagnostic")
        #expect(env.id == nil)
        #expect(env.payload == .object(["level": .string("info"), "category": .string("x"), "message": .string("m")]))
    }

    @Test func takeoverDueFlattensTheRequestAndCalendarTitle() {
        let event = Fixture.timed(conference: "https://zoom.us/j/9")
        let request = TakeoverRequest.make(for: event, now: event.start.addingTimeInterval(-30))
        let env = wireMessage(for: .takeoverDue(request, calendarTitle: "Work"))
        #expect(env.type == "event.takeoverDue")
        #expect(env.payload?["calendarTitle"] == .string("Work"))
        #expect(env.payload?["event"]?["title"] == .string("Design review"))
        #expect(env.payload?["hasStarted"] == .bool(false))
        #expect(env.payload?["joinURL"] == .string("https://zoom.us/j/9"))
    }

    @Test func simpleEventsMapTheirPayloads() {
        let withdrawn = wireMessage(for: .takeoverWithdrawn(eventID: "e1"))
        #expect(withdrawn.type == "event.takeoverWithdrawn")
        #expect(withdrawn.payload == .object(["eventID": .string("e1")]))
        let join = wireMessage(for: .joinRequested(URL(string: "https://meet.example/x")!))
        #expect(join.type == "event.joinRequested")
        #expect(join.payload == .object(["url": .string("https://meet.example/x")]))
    }

    @Test func sourceStatusesMapEachKind() {
        #expect(WireSourceStatus(.ok) == WireSourceStatus(kind: "ok", message: nil))
        #expect(WireSourceStatus(.needsPermission).kind == "needsPermission")
        #expect(WireSourceStatus(.authExpired).kind == "authExpired")
        #expect(WireSourceStatus(.failing("boom")) == WireSourceStatus(kind: "failing", message: "boom"))
    }

    @Test func inferenceStatusMapsEachKind() {
        #expect(WireInferenceStatus(.disabled).kind == "disabled")
        #expect(WireInferenceStatus(.noEngine).kind == "noEngine")
        #expect(WireInferenceStatus(.notOnDevice).kind == "notOnDevice")
        #expect(WireInferenceStatus(.unavailable(reason: "r")).reason == "r")
        let active = WireInferenceStatus(.active(EngineInfo(id: "e", displayName: "N", isOnDevice: true)))
        #expect(active.kind == "active" && active.engineID == "e" && active.engineName == "N" && active.isOnDevice == true)
    }

    @Test func fullStateMapsCollectionsByID() {
        let a = Fixture.timed("a"), b = Fixture.timed("b", title: "Design review ")
        var state = EngineState.empty
        state.revision = 7
        state.events = [a, b]
        state.candidates = [a.id: [b]]
        state.statuses = ["s1": .failing("x"), "s2": .ok]
        state.agenda = DayAgenda.make(
            events: [a], settings: TakeoverSettings(), now: a.start.addingTimeInterval(-60),
            calendar: { var c = Calendar(identifier: .gregorian); c.timeZone = Fixture.utc; return c }())
        let wire = WireState(state)
        #expect(wire.revision == 7)
        #expect(wire.events.map(\.id) == [a.id, b.id])
        #expect(wire.candidates == [a.id: [b.id]])
        #expect(wire.statuses["s1"]?.kind == "failing")
        #expect(wire.agenda.items.map(\.eventID) == [a.id])
        #expect(wire.agenda.nextEventID == a.id)
    }

    @Test func settingsMapAndPatchLeavesAbsentFieldsAlone() {
        var settings = EngineSettings()
        settings.takeover.takeoverCalendarKeys = ["b/1", "a/1"]
        let wire = WireSettings(settings)
        #expect(wire.takeover.takeoverCalendarKeys == ["a/1", "b/1"])

        var patch = WireSettingsPatch()
        patch.inferenceEnabled = true
        patch.leadTime = 300
        var changed = settings
        patch.apply(to: &changed)
        #expect(changed.inferenceEnabled)
        #expect(changed.takeover.leadTime == 300)
        #expect(changed.takeover.takeoverCalendarKeys == settings.takeover.takeoverCalendarKeys)
        #expect(changed.systemCalendarEnabled == settings.systemCalendarEnabled)

        var empty = settings
        WireSettingsPatch().apply(to: &empty)
        #expect(empty == settings)
    }

    @Test func hidingACalendarRemovesItsTakeoverOptIn() {
        var settings = EngineSettings()
        settings.takeover.takeoverCalendarKeys = ["a/1"]
        var patch = WireSettingsPatch()
        patch.hiddenCalendarKeys = ["a/1"]
        patch.apply(to: &settings)
        #expect(settings.takeover.takeoverCalendarKeys.isEmpty)
    }

    @Test func replyParsingNeverThrows() {
        #expect(WireReply.parse(#"{"v":1,"result":{"a":1}}"#) == .result(.object(["a": .number(1)])))
        #expect(WireReply.parse(#"{"v":1,"error":{"kind":"denied","message":"m"}}"#) == .error(kind: .denied, message: "m"))
        #expect(WireReply.parse(#"{"v":1,"error":{"kind":"weird"}}"#) == .error(kind: .failed, message: ""))
        #expect(WireReply.parse(#"{"v":2,"result":1}"#) == .error(kind: .failed, message: "unsupported reply version"))
        #expect(WireReply.parse("{nope") == .error(kind: .failed, message: "reply is not a JSON object"))
        #expect(WireReply.parse(#"{"v":1}"#) == .result(.null))
    }
}
