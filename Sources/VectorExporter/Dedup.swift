import CalendarCore
import Foundation
@testable import TimeTugCore

// dedup/pair-decisions.json
//   Entry points (Sources/TimeTugCore/Dedup/DuplicateRules.swift): DuplicateRules.isCandidate(a, b),
//   isManualCandidate(a, b), withinTimeGate(a, b), decide(a, b) -> PairDecision, detailScore(event) -> Int,
//   detailSummary(event) -> String. `a` and `b` are EventSpec objects (see EventSpec.swift): the library event plus the
//   Core state the rules read (calendarService/calendarProvider for provider-scoped uids, conferences, attendees,
//   organizer, location, notes).
//   input:    { "a": EventSpec, "b": EventSpec }
//   expected: { "isCandidate": Bool, "isManualCandidate": Bool, "withinTimeGate": Bool,
//               "decide": { "kind": "merge" | "separate" | "ambiguous", "reason": String? },   // MergeReason / SeparateReason raw value; absent for ambiguous
//               "detailScoreA": Int, "detailScoreB": Int, "detailSummaryA": String, "detailSummaryB": String }
//
// dedup/resolutions.json
//   Entry point (Sources/TimeTugCore/Dedup/DuplicateResolver.swift): DuplicateResolver.resolve(events:calendars:lessons:verdicts:engineID:).
//   `lessons` are built by LessonBook.record(MergedMember(events[a]), MergedMember(events[b]), decision:, now:) in the order
//   listed (what the user's manual merge "same" and separation "different" record); `verdicts` is null when inference is
//   off (the resolver's `verdicts: nil`), else a VerdictCache filled by VerdictCache.store(verdict, engine:, end:, now:)
//   in the order listed (request ids are the fingerprints the resolver itself computes; one listed id may match nothing).
//   input:    { "engineID": String,
//               "events": [EventSpec],
//               "calendars": [{ "sourceID", "calendarID", "title" }],
//               "lessons": [{ "a": Int, "b": Int, "decision": "same" | "different", "now": Int (ms) }],   // indexes into events
//               "verdicts": null | [{ "requestID", "answer": "same" | "different" | "unsure", "engine": { "id", "displayName", "isOnDevice" }, "end": ms, "now": ms }] }
//   expected: { "events":  [EventSpec.summary],          // the resolver's merged events, in its output order
//               "pending": [{ "id", "firstTitle", "secondTitle", "startOffsetMinutes", "endOffsetMinutes", "overlapMinutes",
//                             "firstDetails", "secondDetails", "hasConflictingDetails" }],
//               "candidates": { <merged event id>: [<merged event id>] },   // lists keep the resolver's order
//               "usedLessonKeys": [String] }                                  // sorted

enum DedupAreas {
    // MARK: Event generation shared by both areas

    static func spec(
        _ id: String, source: String = "s1", cal: String = "c1", title: String, start: Int64, end: Int64, _ extra: JSON = [:]
    ) -> JSON {
        var j: JSON = ["eventID": id, "sourceID": source, "calendarID": cal, "title": title, "start": start, "end": end]
        for (k, v) in extra { j[k] = v }
        return j
    }

    static let startTimes: [Int64] = [
        iso("2026-09-18T10:00:00Z"), iso("2026-09-18T23:30:00Z"), iso("2026-03-08T06:30:00Z"), iso("2026-11-01T05:30:00Z"),
        iso("2026-03-29T00:30:00Z"), iso("2026-10-25T00:30:00Z"), iso("2026-12-31T23:45:00Z"), iso("2026-02-28T12:00:00Z"),
    ]

    static func conferenceJSON(_ url: String, _ provider: String, origin: String) -> JSON {
        ["url": url, "provider": provider, "origin": origin]
    }

    /// A random base event: a title, a time and a random selection of details.
    static func base(_ id: String, _ rng: inout LCG, start overrideStart: Int64? = nil, title overrideTitle: String? = nil) -> JSON {
        let start = overrideStart ?? (rng.pick(startTimes) + Int64(rng.below(8)) * 30 * oneMinute)
        let duration = Int64(rng.pick([15, 30, 45, 60, 90, 120])) * oneMinute
        var j = spec(id, source: "s1", cal: "c1", title: overrideTitle ?? rng.pick(Pools.titles), start: start, end: start + duration)
        if rng.chance(25) {
            j["uid"] = "uid-\(rng.below(5))"
            switch rng.below(10) {
            case 0..<6: j["uidScope"] = "global"
            case 6..<9: j["uidScope"] = "provider"
            default: break
            }
            j["calendarService"] = rng.pick(["google", "microsoft", "caldav"])
            j["calendarProvider"] = rng.pick(["google", "microsoft", "icloud"])
        }
        if rng.chance(35) {
            let link = rng.pick(Pools.conferences)
            j["conferences"] = [conferenceJSON(link.url, link.provider, origin: rng.pick(["structured", "structured", "location", "notes", "eventURL"]))]
            if rng.chance(15) {
                let second = rng.pick(Pools.conferences)
                j["conferences"] = (j["conferences"] as! [JSON]) + [conferenceJSON(second.url, second.provider, origin: "notes")]
            }
        }
        if rng.chance(40) {
            var attendees: [JSON] = (0..<rng.int(1...3)).map { _ in ["email": rng.pick(Pools.emails), "name": "Person \(rng.below(9))"] }
            if rng.chance(30) { attendees.append(["email": "me@x.com", "isSelf": true]) }
            j["attendees"] = attendees
            if rng.chance(30) { j["organizer"] = ["email": rng.pick(Pools.emails), "isSelf": rng.chance(20)] as JSON }
        } else if rng.chance(15) {
            j["organizer"] = ["email": rng.pick(Pools.emails), "isSelf": false] as JSON
        }
        if rng.chance(40) { j["location"] = rng.pick(Pools.locations) }
        if rng.chance(25) { j["notes"] = rng.pick(["Bring laptop", "  ", "Agenda attached", "\n"]) }
        if rng.chance(10) { j["url"] = "https://example.com/event/\(rng.below(5))" }
        if rng.chance(50) { j["myResponse"] = rng.pick(["accepted", "tentative", "declined", "needsAction"]) }
        return j
    }

    /// Another copy of `a`, perturbed by the knobs the rules look at.
    static func variant(of a: JSON, id: String, _ rng: inout LCG, sameCalendarChance: Int = 10, exactChance: Int = 5) -> JSON {
        var b = a
        b["eventID"] = id
        let aKey = "\(a["sourceID"]!)/\(a["calendarID"]!)"
        if !rng.chance(sameCalendarChance) {
            b["sourceID"] = rng.pick(["s1", "s2"])
            b["calendarID"] = rng.pick(["c1", "c2", "c3"])
            if "\(b["sourceID"]!)/\(b["calendarID"]!)" == aKey { b["calendarID"] = "c9" }
        }
        if rng.chance(exactChance) {
            if rng.chance(40) { b["title"] = (a["title"] as! String).uppercased() }
            return b
        }
        let aStart = i64(a["start"]!), aEnd = i64(a["end"]!)
        let startShift = Int64(rng.pick([0, 0, 0, 0, 0, 0, 0, 0, 5, 25, 29, 30, 31, -5, -30, 60, -120])) * oneMinute
            + (rng.chance(10) ? Int64(rng.below(60)) * 1000 : 0)
        let endShift = Int64(rng.pick([0, 0, 0, 0, 0, 10, 30, 59, 60, 61, -30, -61, 120])) * oneMinute
        b["start"] = aStart + startShift
        b["end"] = aEnd + endShift
        let title = a["title"] as! String
        switch rng.below(10) {
        case 0..<3: break
        case 3: b["title"] = title.uppercased()
        case 4: b["title"] = " \(title)!! "
        case 5: b["title"] = title.replacingOccurrences(of: "e", with: "\u{E9}")
        case 6: b["title"] = title + " (copy)"
        default: b["title"] = rng.pick(Pools.titles)
        }
        // uid
        if a["uid"] != nil {
            switch rng.below(20) {
            case 0..<11: break
            case 11..<16: b["uid"] = "uid-other-\(rng.below(3))"
            case 16..<18: b["uid"] = nil
            default: b["calendarService"] = "other"
            }
        } else if rng.chance(15) {
            b["uid"] = "uid-\(rng.below(5))"
            b["uidScope"] = "global"
        }
        // conference
        if let confs = a["conferences"] as? [JSON], let first = confs.first {
            switch rng.below(10) {
            case 0..<3:
                let link = Pools.conferences.first { $0.url == first["url"] as! String }!
                let spelled = link.variants.isEmpty ? link.url : rng.pick(link.variants)
                b["conferences"] = [conferenceJSON(spelled, link.provider, origin: "structured")]
            case 3, 4:
                let link = rng.pick(Pools.conferences)
                b["conferences"] = [conferenceJSON(link.url, link.provider, origin: "location")]
            case 5, 6: b["conferences"] = nil
            case 7: b["conferences"] = confs.map { conferenceJSON($0["url"] as! String, $0["provider"] as! String, origin: "eventURL") }
            default: break
            }
        } else if rng.chance(20) {
            let link = rng.pick(Pools.conferences)
            b["conferences"] = [conferenceJSON(link.url, link.provider, origin: rng.pick(["structured", "eventURL"]))]
        }
        // attendees and organizer
        if a["attendees"] != nil {
            switch rng.below(10) {
            case 0..<3: break
            case 3, 4: b["attendees"] = [["email": (a["attendees"] as! [JSON])[0]["email"] ?? "none@x.com", "name": "Same"] as JSON]
            case 5, 6: b["attendees"] = [["email": "stranger\(rng.below(5))@elsewhere.org"] as JSON]
            case 7: b["attendees"] = nil
            default: b["attendees"] = [["email": "Kristin@X.com"] as JSON]
            }
        } else if rng.chance(20) {
            b["attendees"] = [["email": rng.pick(Pools.emails)] as JSON]
        }
        if rng.chance(15) { b["organizer"] = ["email": rng.pick(Pools.emails), "isSelf": rng.chance(25)] as JSON }
        // location
        if let location = a["location"] as? String {
            switch rng.below(10) {
            case 0..<3: break
            case 3: b["location"] = location.uppercased()
            case 4, 5: b["location"] = "Building 2, \(location), floor 3"
            case 6: b["location"] = rng.pick(Pools.locations)
            case 7: b["location"] = "https://acme.zoom.us/j/\(rng.below(9))"
            default: b["location"] = nil
            }
        } else if rng.chance(20) {
            b["location"] = rng.pick(Pools.locations + ["HQ", "a", "Room 4", "Room 4, Main Street"])
        }
        if rng.chance(15) { b["notes"] = "Different notes" }
        if rng.chance(8) { b["isAllDay"] = true }
        return b
    }

    static func decisionJSON(_ decision: PairDecision) -> JSON {
        switch decision {
        case .merge(let reason): ["kind": "merge", "reason": reason.rawValue]
        case .separate(let reason): ["kind": "separate", "reason": reason.rawValue]
        case .ambiguous: ["kind": "ambiguous"]
        }
    }

    static func pairExpected(_ a: JSON, _ b: JSON) -> JSON {
        let ea = EventSpec.event(a), eb = EventSpec.event(b)
        return [
            "isCandidate": DuplicateRules.isCandidate(ea, eb),
            "isManualCandidate": DuplicateRules.isManualCandidate(ea, eb),
            "withinTimeGate": DuplicateRules.withinTimeGate(ea, eb),
            "decide": decisionJSON(DuplicateRules.decide(ea, eb)),
            "detailScoreA": DuplicateRules.detailScore(ea), "detailScoreB": DuplicateRules.detailScore(eb),
            "detailSummaryA": DuplicateRules.detailSummary(ea), "detailSummaryB": DuplicateRules.detailSummary(eb),
        ]
    }

    // MARK: dedup/pair-decisions.json

    static func pairCases() -> [VectorCase] {
        var cases: [VectorCase] = []
        func add(_ name: String, _ a: JSON, _ b: JSON) {
            cases.append(VectorCase(name: name, input: ["a": a, "b": b], expected: pairExpected(a, b)))
        }
        let t = iso("2026-09-18T10:00:00Z")
        let zoom1 = conferenceJSON("https://acme.zoom.us/j/1", "zoom", origin: "structured")
        let zoom2 = conferenceJSON("https://acme.zoom.us/j/2", "zoom", origin: "structured")
        func two(_ title: String = "A") -> JSON { spec("1", title: title, start: t, end: t + oneHour) }
        func other(_ title: String = "B", _ extra: JSON = [:]) -> JSON {
            spec("2", cal: "o", title: title, start: t, end: t + oneHour, extra)
        }
        // Hand-written: one for each reason and the boundaries of the time gate (the inputs follow DuplicateRulesTests).
        add("hand-exact-match-other-calendar", spec("1", title: "Sync", start: t, end: t + 30 * oneMinute), spec("2", cal: "other", title: "sync", start: t, end: t + 30 * oneMinute))
        add("hand-exact-match-same-calendar", spec("1", title: "Sync", start: t, end: t + 30 * oneMinute), spec("2", title: "Sync", start: t, end: t + 30 * oneMinute))
        add("hand-same-calendar", two("Doctor"), spec("2", title: "Dance", start: t, end: t + oneHour))
        add("hand-all-day", spec("1", title: "Birthday", start: t, end: t + oneDay, ["isAllDay": true]), spec("2", cal: "other", title: "Kristin birthday", start: t, end: t + oneDay, ["isAllDay": true]))
        add("hand-gate-start-30", two(), spec("2", cal: "o", title: "B", start: t + 30 * oneMinute, end: t + 90 * oneMinute))
        add("hand-gate-start-31", two(), spec("2", cal: "o", title: "B", start: t + 31 * oneMinute, end: t + 91 * oneMinute))
        add("hand-gate-end-plus-60", spec("1", title: "A", start: t, end: t + 30 * oneMinute), spec("2", cal: "o", title: "B", start: t, end: t + 90 * oneMinute))
        add("hand-gate-end-plus-61", spec("1", title: "A", start: t, end: t + 30 * oneMinute), spec("2", cal: "o", title: "B", start: t, end: t + 91 * oneMinute))
        add("hand-back-to-back", two(), spec("2", cal: "o", title: "B", start: t + oneHour, end: t + 2 * oneHour))
        add("hand-shared-conference", spec("1", title: "Weekly", start: t, end: t + oneHour, ["location": "https://acme.zoom.us/j/555?pwd=abc", "conferences": [conferenceJSON("https://acme.zoom.us/j/555?pwd=abc", "zoom", origin: "location")]]),
            spec("2", cal: "o", title: "Team sync", start: t, end: t + oneHour, ["conferences": [conferenceJSON("https://acme.zoom.us/j/555/", "zoom", origin: "notes")]]))
        add("hand-shared-uid-global", spec("1", title: "A", start: t, end: t + oneHour, ["uid": "uid-1", "uidScope": "global"]), other("B", ["uid": "uid-1", "uidScope": "global"]))
        add("hand-shared-uid-provider-same-service", spec("1", title: "A", start: t, end: t + oneHour, ["uid": "u", "uidScope": "provider", "calendarService": "microsoft", "calendarProvider": "microsoft"]),
            other("B", ["uid": "u", "uidScope": "provider", "calendarService": "microsoft", "calendarProvider": "microsoft"]))
        add("hand-shared-uid-provider-other-service", spec("1", title: "A", start: t, end: t + oneHour, ["uid": "u", "uidScope": "provider", "calendarService": "microsoft", "calendarProvider": "microsoft"]),
            other("B", ["uid": "u", "uidScope": "provider", "calendarService": "google", "calendarProvider": "google"]))
        add("hand-shared-uid-unknown-scope-no-service", spec("1", title: "A", start: t, end: t + oneHour, ["uid": "u"]), other("B", ["uid": "u"]))
        add("hand-empty-uid", spec("1", title: "A", start: t, end: t + oneHour, ["uid": ""]), other("B", ["uid": ""]))
        add("hand-shared-attendee-case", spec("1", title: "A", start: t, end: t + oneHour, ["attendees": [["email": "Kristin@x.com"] as JSON]]),
            other("B", ["attendees": [["email": "kristin@X.com"] as JSON, ["email": "z@x.com"] as JSON]]))
        add("hand-shared-organizer-email", spec("1", title: "A", start: t, end: t + oneHour, ["organizer": ["email": "boss@x.com"] as JSON]), other("B", ["attendees": [["email": "boss@x.com"] as JSON]]))
        add("hand-self-attendee-ignored", spec("1", title: "A", start: t, end: t + oneHour, ["attendees": [["email": "me@x.com", "isSelf": true] as JSON]]), other("B", ["attendees": [["email": "me@x.com", "isSelf": true] as JSON]]))
        add("hand-attendee-vs-location-veto", spec("1", title: "A", start: t, end: t + oneHour, ["location": "Room 101 East", "attendees": [["email": "k@x.com"] as JSON]]),
            other("B", ["location": "Dentist Office", "attendees": [["email": "k@x.com"] as JSON]]))
        add("hand-attendee-vs-conference-veto", spec("1", title: "A", start: t, end: t + oneHour, ["conferences": [zoom1], "attendees": [["email": "k@x.com"] as JSON]]),
            other("B", ["conferences": [zoom2], "attendees": [["email": "k@x.com"] as JSON]]))
        add("hand-same-location", spec("1", title: "A", start: t, end: t + oneHour, ["location": "Room 101 East"]), other("B", ["location": "room 101 east!"]))
        add("hand-contained-location", spec("1", title: "A", start: t, end: t + oneHour, ["location": "Room 101"]), other("B", ["location": "Building 2, Room 101, floor 3"]))
        add("hand-short-location-not-contained", spec("1", title: "A", start: t, end: t + oneHour, ["location": "HQ"]), other("B", ["location": "HQ Annex, level 2"]))
        add("hand-url-location-ignored", spec("1", title: "A", start: t, end: t + oneHour, ["location": "https://acme.zoom.us/j/9"]), other("B", ["location": "Room 4"]))
        add("hand-conflicting-attendees", spec("1", title: "A", start: t, end: t + oneHour, ["attendees": [["email": "a@x.com"] as JSON]]), other("B", ["attendees": [["email": "b@x.com"] as JSON]]))
        add("hand-same-title-after-vetoes", two("Piano lesson"), other("piano  LESSON"))
        add("hand-same-title-accents", two("Caf\u{E9} meeting"), other("CAFE MEETING"))
        add("hand-same-title-vetoed-by-location", spec("1", title: "Sync", start: t, end: t + oneHour, ["location": "Room 101 East"]), other("Sync", ["location": "Dentist Office"]))
        add("hand-empty-titles", two(""), other(""))
        add("hand-ambiguous-bare", two("Scott: Doctor"), other("Intermountain Health"))
        add("hand-event-url-not-a-conference", spec("1", title: "A", start: t, end: t + oneHour, ["conferences": [conferenceJSON("https://acme.zoom.us/j/1", "zoom", origin: "eventURL")]]),
            other("B", ["conferences": [conferenceJSON("https://acme.zoom.us/j/1", "zoom", origin: "eventURL")]]))
        add("hand-webex-mtid", spec("1", title: "A", start: t, end: t + oneHour, ["conferences": [conferenceJSON("https://acme.webex.com/acme/j.php?MTID=m11", "webex", origin: "structured")]]),
            other("B", ["conferences": [conferenceJSON("https://acme.webex.com/acme/j.php?MTID=m22", "webex", origin: "structured")]]))
        add("hand-details-all", spec("1", title: "A", start: t, end: t + oneHour, ["location": "Room 101", "notes": "x", "attendees": [["email": "a@x.com"] as JSON], "conferences": [zoom1]]), other("B"))
        add("hand-details-other-attendee-count-only", spec("1", title: "A", start: t, end: t + oneHour, ["otherAttendeeCount": 3]), other("B", ["notes": "  \n"]))

        var rng = LCG(seed: 0x5041_4952_0001)
        var index = 0
        while cases.count < 640 {
            index += 1
            var a = base("1", &rng)
            var b = variant(of: a, id: "2", &rng)
            // A few all-day pairs on canonical midnights.
            if rng.chance(4) {
                let midnight = iso("2026-09-18T00:00:00Z") + Int64(rng.below(3)) * oneDay
                a["isAllDay"] = true; a["start"] = midnight; a["end"] = midnight + oneDay
                b["isAllDay"] = true; b["start"] = midnight; b["end"] = midnight + oneDay
                if rng.chance(50) { b["title"] = rng.pick(Pools.titles) }
            }
            add("generated-\(String(format: "%04d", index))", a, b)
        }
        return cases
    }

    // MARK: dedup/resolutions.json

    static func engine(_ id: String) -> JSON {
        ["id": id, "displayName": id == "fm" ? "Foundation Models" : "Test Engine", "isOnDevice": id == "fm"]
    }

    static func resolveExpected(_ input: JSON) -> JSON {
        let events = (input["events"] as! [JSON]).map(EventSpec.event)
        let calendars = (input["calendars"] as! [JSON]).map {
            CalendarInfo(sourceID: $0["sourceID"] as! String, calendarID: $0["calendarID"] as! String, title: $0["title"] as! String)
        }
        var book = LessonBook()
        for lesson in input["lessons"] as! [JSON] {
            book.record(
                MergedMember(events[lesson["a"] as! Int]), MergedMember(events[lesson["b"] as! Int]),
                decision: Lesson.Decision(rawValue: lesson["decision"] as! String)!, now: date(ms: i64(lesson["now"]!)))
        }
        var cache: VerdictCache?
        if let verdicts = input["verdicts"] as? [JSON] {
            var filled = VerdictCache()
            for v in verdicts {
                let e = v["engine"] as! JSON
                filled.store(
                    AdjudicationVerdict(requestID: v["requestID"] as! String, answer: AdjudicationVerdict.Answer(rawValue: v["answer"] as! String)!),
                    engine: EngineInfo(id: e["id"] as! String, displayName: e["displayName"] as! String, isOnDevice: e["isOnDevice"] as! Bool),
                    end: date(ms: i64(v["end"]!)), now: date(ms: i64(v["now"]!)))
            }
            cache = filled
        }
        let result = DuplicateResolver.resolve(
            events: events, calendars: calendars, lessons: book, verdicts: cache, engineID: input["engineID"] as! String)
        return [
            "events": result.events.map(EventSpec.summary),
            "pending": result.pending.map { r -> JSON in
                ["id": r.id, "firstTitle": r.first.title, "secondTitle": r.second.title,
                 "startOffsetMinutes": r.startOffsetMinutes, "endOffsetMinutes": r.endOffsetMinutes,
                 "overlapMinutes": r.overlapMinutes, "firstDetails": r.firstDetails, "secondDetails": r.secondDetails,
                 "hasConflictingDetails": r.hasConflictingDetails]
            },
            "candidates": result.candidates.mapValues { $0.map(\.id) },
            "usedLessonKeys": result.usedLessonKeys.sorted(),
        ]
    }

    static func resolutionCases() -> [VectorCase] {
        var cases: [VectorCase] = []
        func add(_ name: String, _ input: JSON) {
            cases.append(VectorCase(name: name, input: input, expected: resolveExpected(input)))
        }
        func calendars(for events: [JSON]) -> [JSON] {
            var seen = Set<String>()
            var out: [JSON] = []
            for e in events {
                let key = "\(e["sourceID"]!)/\(e["calendarID"]!)"
                if seen.insert(key).inserted, key != "s2/c3" {   // one calendar stays unknown to the resolver
                    out.append(["sourceID": e["sourceID"]!, "calendarID": e["calendarID"]!, "title": "Calendar \(e["calendarID"]!)"])
                }
            }
            return out
        }
        let t = iso("2026-09-18T10:00:00Z")
        // Hand-written.
        let exactThree = [
            spec("1", title: "Sync", start: t, end: t + oneHour), spec("2", cal: "c2", title: "sync", start: t, end: t + oneHour),
            spec("3", source: "s2", cal: "c1", title: "SYNC", start: t, end: t + oneHour),
        ]
        add("hand-exact-three", ["engineID": "fm", "events": exactThree, "calendars": calendars(for: exactThree), "lessons": [] as [JSON], "verdicts": NSNull()])
        let uidChain = [
            spec("1", title: "A", start: t, end: t + oneHour, ["uid": "u1", "uidScope": "global", "location": "Room 101"]),
            spec("2", cal: "c2", title: "B", start: t + 10 * oneMinute, end: t + oneHour, ["uid": "u1", "uidScope": "global", "notes": "n"]),
            spec("3", cal: "c3", title: "C", start: t + 5 * oneMinute, end: t + 70 * oneMinute, ["conferences": [conferenceJSON("https://acme.zoom.us/j/1", "zoom", origin: "structured")]]),
        ]
        add("hand-uid-pair-plus-bystander", ["engineID": "fm", "events": uidChain, "calendars": calendars(for: uidChain), "lessons": [] as [JSON], "verdicts": NSNull()])
        let ambiguous = [
            spec("1", title: "Scott: Doctor", start: t, end: t + oneHour), spec("2", cal: "c2", title: "Intermountain Health", start: t + 15 * oneMinute, end: t + 45 * oneMinute, ["location": "1234 Main St"]),
        ]
        add("hand-ambiguous-inference-off", ["engineID": "fm", "events": ambiguous, "calendars": calendars(for: ambiguous), "lessons": [] as [JSON], "verdicts": NSNull()])
        add("hand-ambiguous-no-verdicts", ["engineID": "fm", "events": ambiguous, "calendars": calendars(for: ambiguous), "lessons": [] as [JSON], "verdicts": [] as [JSON]])
        let pendingID = ((resolveExpected(["engineID": "fm", "events": ambiguous, "calendars": calendars(for: ambiguous), "lessons": [] as [JSON], "verdicts": [] as [JSON]])["pending"] as! [JSON]).first?["id"] as? String) ?? "missing"
        for answer in ["same", "different", "unsure"] {
            add("hand-ambiguous-verdict-\(answer)", ["engineID": "fm", "events": ambiguous, "calendars": calendars(for: ambiguous), "lessons": [] as [JSON],
                "verdicts": [["requestID": pendingID, "answer": answer, "engine": engine("fm"), "end": t + oneHour, "now": t - oneDay] as JSON]])
        }
        add("hand-ambiguous-lesson-same", ["engineID": "fm", "events": ambiguous, "calendars": calendars(for: ambiguous),
            "lessons": [["a": 0, "b": 1, "decision": "same", "now": t - oneDay] as JSON], "verdicts": NSNull()])
        add("hand-exact-lesson-different", ["engineID": "fm", "events": Array(exactThree.prefix(2)), "calendars": calendars(for: exactThree),
            "lessons": [["a": 0, "b": 1, "decision": "different", "now": t - oneDay] as JSON], "verdicts": NSNull()])

        // Generated.
        var rng = LCG(seed: 0x5245_534F_0001)
        var index = 0
        while cases.count < 310 {
            index += 1
            let target = rng.chance(72) ? rng.int(2...12) : rng.int(13...32)
            var events: [JSON] = []
            // The meetings of one case cluster in a few hours so their copies, and the strangers among them, meet.
            let anchor = rng.pick(startTimes)
            while events.count < target {
                let start = anchor + Int64(rng.below(10)) * 30 * oneMinute
                var root = base("e\(events.count)", &rng, start: start)
                if rng.chance(8) { root["isAllDay"] = true; root["start"] = anchor - anchor % oneDay; root["end"] = (anchor - anchor % oneDay) + oneDay }
                events.append(root)
                for _ in 0..<rng.pick([0, 0, 1, 1, 2, 3]) where events.count < target {
                    events.append(variant(of: root, id: "e\(events.count)", &rng, sameCalendarChance: 10, exactChance: 15))
                }
            }
            // Distinct calendar keys per copy keep ids readable; duplicated (source, calendar, id, start) cannot occur because ids are unique.
            var lessons: [JSON] = []
            if events.count >= 2 && rng.chance(35) {
                for _ in 0..<rng.int(1...3) {
                    let a = rng.below(events.count)
                    var b = rng.below(events.count)
                    if a == b { b = (b + 1) % events.count }
                    lessons.append(["a": a, "b": b, "decision": rng.chance(55) ? "same" : "different", "now": anchor - Int64(rng.below(5)) * oneDay])
                }
            }
            var input: JSON = ["engineID": rng.pick(["fm", "test-engine"]), "events": events, "calendars": calendars(for: events), "lessons": lessons, "verdicts": NSNull()]
            if rng.chance(65) {
                var verdicts: [JSON] = []
                for _ in 0..<rng.int(1...5) {
                    input["verdicts"] = verdicts
                    let pending = resolveExpected(input)["pending"] as! [JSON]
                    if pending.isEmpty { break }
                    for p in pending.prefix(rng.int(1...4)) {
                        let answer = rng.pick(["same", "same", "same", "different", "unsure"])
                        verdicts.append([
                            "requestID": p["id"]!, "answer": answer, "engine": engine(rng.chance(70) ? "fm" : "test-engine"),
                            "end": anchor + 6 * oneHour, "now": anchor - oneDay,
                        ])
                    }
                }
                if rng.chance(15) {
                    verdicts.append(["requestID": "j2-0000000000000000", "answer": "same", "engine": engine("fm"), "end": anchor, "now": anchor])
                }
                input["verdicts"] = verdicts
            }
            add("generated-\(String(format: "%04d", index))", input)
        }
        return cases
    }
}
