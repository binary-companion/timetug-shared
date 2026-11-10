import CalendarCore
import Foundation
@testable import TimeTugCore

// takeover/scheduler.json
//   Entry points (Sources/TimeTugCore/Takeover/): Scheduler.next(events:settings:ledger:now:) -> ScheduledTakeover? and
//   TakeoverPolicy.qualifies(_:settings:) -> Bool. Scheduler takes no calendar or zone: each event carries its own zone, and
//   nothing in the scheduler reads it.
//   input:    { "events": [EventSpec],
//               "settings": { "leadTime": Number (seconds), "takeoverCalendarKeys": [String], "hiddenCalendarKeys": [String],
//                             "requireConferenceLink": Bool, "requireOtherAttendees": Bool, "skipAllDayEvents": Bool, "enabled": Bool },
//               "ledger": [Op],     // applied in order to an empty TakeoverLedger before the call; only markFired and snooze (see ledger.json)
//               "now": Int (ms) }
//   expected: { "qualifies": [Bool],                                    // TakeoverPolicy.qualifies for each event, in order
//               "next": null | { "index": Int, "id": String, "fireAt": Int (ms) } }   // index into events
//
// takeover/ledger.json
//   Entry points (Sources/TimeTugCore/Takeover/): TakeoverLedger.markFired(_:now:), snooze(_:for:now:),
//   acknowledgeInProgress(events:now:grace:), prune(now:), hasFired(_:), isSnoozed(_:), keyCount, its Codable form, and
//   TakeoverGuard.evaluate(event:currentEvents:settings:ledger:now:overlayVisible:). One case is a sequence of operations
//   on one ledger that starts empty.
//   input:    { "events": [EventSpec],            // the pool the operations refer to by index
//               "reportEvery": Bool,              // false: the per-step ledger and answer lists are given only for the last step
//               "ops": [Op] }
//   Op:       { "op": "markFired", "event": Int, "now": ms }
//           | { "op": "snooze", "event": Int, "duration": Number (seconds), "now": ms }
//           | { "op": "acknowledgeInProgress", "events": [Int], "now": ms, "grace": Number (seconds) }
//           | { "op": "prune", "now": ms }
//           | { "op": "guard", "event": Int, "current": [Int], "settings": <as above>, "now": ms, "overlayVisible": Bool }
//           | { "op": "reload" }                  // the ledger is encoded and decoded again
//           | { "op": "load", "json": Object }    // the ledger is replaced by decoding this JSON (lenient: a malformed record is dropped)
//   expected: { "steps": [{ "returned": Int | Bool | String | null,   // acknowledgeInProgress count, prune result, guard decision ("present" or
//                                                                       // "suppress:<reason>"), else null
//                           "ledger": Object,     // the encoded ledger after the step: { "records": { <key>: { "entry", "end", "recordedAt" } } },
//                                                 // dates as Swift's default Date coding (seconds since 2001-01-01T00:00:00Z, a number)
//                           "hasFired": [Bool], "isSnoozed": [Bool],   // for each event of the pool
//                           "keyCount": Int }] }  // fields other than "returned" and "keyCount" are absent on steps hidden by reportEvery

enum TakeoverAreas {
    static let anchor = iso("2026-09-18T15:00:00Z")

    static func subset<T>(_ items: [T], _ percent: Int, _ rng: inout LCG) -> [T] {
        var out: [T] = []
        for item in items where rng.chance(percent) { out.append(item) }
        return out
    }

    static func settingsJSON(_ rng: inout LCG, calendars: [String]) -> JSON {
        var s: JSON = ["leadTime": rng.pick([0, 60, 60, 300, 900, 3600, 0.5, 90.25] as [Double])]
        s["takeoverCalendarKeys"] = subset(calendars, 65, &rng)
        s["hiddenCalendarKeys"] = subset(calendars, 15, &rng)
        s["requireConferenceLink"] = rng.chance(25)
        s["requireOtherAttendees"] = rng.chance(25)
        s["skipAllDayEvents"] = rng.chance(70)
        s["enabled"] = rng.chance(90)
        return s
    }

    /// Events spread around `now`: running, upcoming, ended, all-day, declined, merged.
    static func eventPool(_ rng: inout LCG, now: Int64, count: Int) -> [JSON] {
        var events: [JSON] = []
        for i in 0..<count {
            let cal = rng.pick(["c1", "c2", "c3"])
            let start = now + Int64(rng.int(-120...240)) * oneMinute + (rng.chance(10) ? Int64(rng.below(60_000)) : 0)
            let end = start + Int64(rng.pick([15, 30, 45, 60, 90])) * oneMinute
            var e = DedupAreas.spec("e\(i)", cal: cal, title: rng.pick(Pools.titles), start: start, end: end)
            if rng.chance(40) {
                let link = rng.pick(Pools.conferences)
                e["conferences"] = [DedupAreas.conferenceJSON(link.url, link.provider, origin: rng.pick(["structured", "location", "eventURL"]))]
            }
            if rng.chance(50) { e["attendees"] = [["email": rng.pick(Pools.emails)] as JSON] }
            if rng.chance(15) { e["myResponse"] = "declined" } else if rng.chance(40) { e["myResponse"] = rng.pick(["accepted", "tentative", "needsAction"]) }
            if rng.chance(8) {
                let midnight = start - start % oneDay
                e["isAllDay"] = true; e["start"] = midnight; e["end"] = midnight + oneDay
            }
            if rng.chance(15) {
                // A merged meeting: its own calendar plus up to two others, with the copies' content keys.
                let copy = DedupAreas.variant(of: e, id: "m\(i)", &rng, sameCalendarChance: 0, exactChance: 100)
                let members = [e, copy].map { j -> JSON in
                    let ev = EventSpec.event(j)
                    return ["title": ev.title, "calendarKey": ev.calendarKey, "contentKey": ev.contentKey, "details": "bare", "start": j["start"]!, "end": j["end"]!]
                }
                e["mergedMembers"] = members
                e["additionalCalendarKeys"] = ["\(copy["sourceID"]!)/\(copy["calendarID"]!)"]
                e["mergeProvenance"] = "rule"
            }
            events.append(e)
        }
        return events
    }

    static func op(_ kind: String, _ fields: JSON) -> JSON {
        var j = fields
        j["op"] = kind
        return j
    }

    // MARK: Applying operations

    struct Runner {
        var events: [TimeTugCalendarEvent]
        var ledger = TakeoverLedger()

        init(events specs: [JSON]) { events = specs.map(EventSpec.event) }

        func ledgerJSON() -> JSON {
            let encoder = JSONEncoder()
            encoder.outputFormatting = [.sortedKeys]
            let data = try! encoder.encode(ledger)
            return try! JSONSerialization.jsonObject(with: data) as! JSON
        }

        mutating func apply(_ op: JSON) -> Any {
            let kind = op["op"] as! String
            func event(_ key: String = "event") -> TimeTugCalendarEvent { events[op[key] as! Int] }
            switch kind {
            case "markFired":
                ledger.markFired(event(), now: date(ms: i64(op["now"]!)))
            case "snooze":
                ledger.snooze(event(), for: (op["duration"] as! NSNumber).doubleValue, now: date(ms: i64(op["now"]!)))
            case "acknowledgeInProgress":
                return ledger.acknowledgeInProgress(
                    events: (op["events"] as! [Int]).map { events[$0] }, now: date(ms: i64(op["now"]!)),
                    grace: (op["grace"] as! NSNumber).doubleValue)
            case "prune":
                return ledger.prune(now: date(ms: i64(op["now"]!)))
            case "guard":
                let current = (op["current"] as! [Int]).map { events[$0] }
                let decision = TakeoverGuard.evaluate(
                    event: event(), currentEvents: current, settings: EventSpec.settings(op["settings"] as! JSON), ledger: ledger,
                    now: date(ms: i64(op["now"]!)), overlayVisible: op["overlayVisible"] as! Bool)
                switch decision {
                case .present: return "present"
                case .suppress(let reason): return "suppress:\(reason.rawValue)"
                }
            case "reload":
                let encoder = JSONEncoder()
                ledger = try! JSONDecoder().decode(TakeoverLedger.self, from: try! encoder.encode(ledger))
            case "load":
                let data = try! JSONSerialization.data(withJSONObject: op["json"]!)
                ledger = try! JSONDecoder().decode(TakeoverLedger.self, from: data)
            default: fatalError("unknown op \(kind)")
            }
            return NSNull()
        }
    }

    // MARK: takeover/scheduler.json

    static func schedulerExpected(_ input: JSON) -> JSON {
        var runner = Runner(events: input["events"] as! [JSON])
        for op in input["ledger"] as! [JSON] { _ = runner.apply(op) }
        let settings = EventSpec.settings(input["settings"] as! JSON)
        let now = date(ms: i64(input["now"]!))
        let next = Scheduler.next(events: runner.events, settings: settings, ledger: runner.ledger, now: now)
        var out: JSON = ["qualifies": runner.events.map { TakeoverPolicy.qualifies($0, settings: settings) }]
        if let next, let index = runner.events.firstIndex(of: next.event) {
            out["next"] = ["index": index, "id": next.event.id, "fireAt": ms(next.fireAt)] as JSON
        } else {
            out["next"] = NSNull()
        }
        return out
    }

    static func schedulerCases() -> [VectorCase] {
        var cases: [VectorCase] = []
        func add(_ name: String, _ input: JSON) {
            cases.append(VectorCase(name: name, input: input, expected: schedulerExpected(input)))
        }
        let now = anchor
        func settings(_ extra: JSON = [:]) -> JSON {
            var s: JSON = ["leadTime": 60.0, "takeoverCalendarKeys": ["s1/c1"], "hiddenCalendarKeys": [] as [String]]
            for (k, v) in extra { s[k] = v }
            return s
        }
        func ev(_ id: String, start: Int64, minutes: Int64 = 30, _ extra: JSON = [:]) -> JSON {
            DedupAreas.spec(id, title: "Meeting \(id)", start: start, end: start + minutes * oneMinute, extra)
        }
        // Hand-written, after SchedulerTests and TakeoverPolicyTests.
        add("hand-simple", ["events": [ev("a", start: now + 10 * oneMinute)], "settings": settings(), "ledger": [] as [JSON], "now": now])
        add("hand-lead-time-already-passed", ["events": [ev("a", start: now + 30_000)], "settings": settings(), "ledger": [] as [JSON], "now": now])
        add("hand-in-progress-fires-now", ["events": [ev("a", start: now - 5 * oneMinute)], "settings": settings(), "ledger": [] as [JSON], "now": now])
        add("hand-ended", ["events": [ev("a", start: now - oneHour)], "settings": settings(), "ledger": [] as [JSON], "now": now])
        add("hand-ends-exactly-now", ["events": [ev("a", start: now - 30 * oneMinute)], "settings": settings(), "ledger": [] as [JSON], "now": now])
        add("hand-earliest-wins", ["events": [ev("a", start: now + 20 * oneMinute), ev("b", start: now + 5 * oneMinute), ev("c", start: now + 8 * oneMinute)], "settings": settings(), "ledger": [] as [JSON], "now": now])
        add("hand-fired-skipped", ["events": [ev("a", start: now + 5 * oneMinute), ev("b", start: now + 20 * oneMinute)], "settings": settings(),
                                   "ledger": [op("markFired", ["event": 0, "now": now - oneMinute])], "now": now])
        add("hand-snoozed-until-later", ["events": [ev("a", start: now - 5 * oneMinute, minutes: 60), ev("b", start: now + 20 * oneMinute)], "settings": settings(),
                                         "ledger": [op("snooze", ["event": 0, "duration": 300.0, "now": now])], "now": now])
        add("hand-snooze-capped-at-end", ["events": [ev("a", start: now - 5 * oneMinute, minutes: 10)], "settings": settings(),
                                          "ledger": [op("snooze", ["event": 0, "duration": 3600.0, "now": now])], "now": now + oneMinute])
        add("hand-snooze-past-end-dropped", ["events": [ev("a", start: now - 5 * oneMinute, minutes: 10)], "settings": settings(),
                                             "ledger": [op("snooze", ["event": 0, "duration": 3600.0, "now": now])], "now": now + 5 * oneMinute])
        add("hand-disabled", ["events": [ev("a", start: now + oneMinute)], "settings": settings(["enabled": false]), "ledger": [] as [JSON], "now": now])
        add("hand-calendar-not-opted-in", ["events": [ev("a", start: now + oneMinute, ["calendarID": "c2"])], "settings": settings(), "ledger": [] as [JSON], "now": now])
        add("hand-calendar-hidden", ["events": [ev("a", start: now + oneMinute)], "settings": settings(["hiddenCalendarKeys": ["s1/c1"]]), "ledger": [] as [JSON], "now": now])
        add("hand-all-day-never", ["events": [ev("a", start: now - now % oneDay, minutes: 1440, ["isAllDay": true])], "settings": settings(["skipAllDayEvents": false]), "ledger": [] as [JSON], "now": now])
        add("hand-declined", ["events": [ev("a", start: now + oneMinute, ["myResponse": "declined"])], "settings": settings(), "ledger": [] as [JSON], "now": now])
        add("hand-require-attendees", ["events": [ev("a", start: now + oneMinute), ev("b", start: now + 2 * oneMinute, ["attendees": [["email": "x@x.com"] as JSON]])],
                                       "settings": settings(["requireOtherAttendees": true]), "ledger": [] as [JSON], "now": now])
        add("hand-require-conference", ["events": [ev("a", start: now + oneMinute), ev("b", start: now + 2 * oneMinute, ["conferences": [DedupAreas.conferenceJSON("https://acme.zoom.us/j/1", "zoom", origin: "structured")]])],
                                        "settings": settings(["requireConferenceLink": true]), "ledger": [] as [JSON], "now": now])
        add("hand-merged-other-calendar-opted-in", ["events": [ev("a", start: now + oneMinute, ["calendarID": "c2", "additionalCalendarKeys": ["s1/c1"]])], "settings": settings(), "ledger": [] as [JSON], "now": now])
        add("hand-merged-other-calendar-hidden", ["events": [ev("a", start: now + oneMinute, ["additionalCalendarKeys": ["s1/c1"]])], "settings": settings(["takeoverCalendarKeys": ["s1/c1", "s1/c2"], "hiddenCalendarKeys": ["s1/c1"]]), "ledger": [] as [JSON], "now": now])
        add("hand-zero-lead", ["events": [ev("a", start: now + 10 * oneMinute)], "settings": settings(["leadTime": 0.0]), "ledger": [] as [JSON], "now": now])
        add("hand-fractional-lead", ["events": [ev("a", start: now + 10 * oneMinute)], "settings": settings(["leadTime": 0.5]), "ledger": [] as [JSON], "now": now])

        var rng = LCG(seed: 0x5343_4845_0001)
        var index = 0
        while cases.count < 410 {
            index += 1
            let events = eventPool(&rng, now: now, count: rng.int(1...8))
            var ops: [JSON] = []
            for _ in 0..<rng.pick([0, 0, 1, 2, 3]) {
                let i = rng.below(events.count)
                let at = now - Int64(rng.int(0...90)) * oneMinute
                ops.append(rng.chance(50) ? op("markFired", ["event": i, "now": at]) : op("snooze", ["event": i, "duration": rng.pick([60.0, 300.0, 600.0, 7200.0]), "now": at]))
            }
            add("generated-\(String(format: "%04d", index))", [
                "events": events, "settings": settingsJSON(&rng, calendars: ["s1/c1", "s1/c2", "s1/c3", "s2/c1"]), "ledger": ops,
                "now": now + Int64(rng.int(-30...60)) * oneMinute + Int64(rng.below(60_000)),
            ])
        }
        return cases
    }

    // MARK: takeover/ledger.json

    static func ledgerExpected(_ input: JSON) -> JSON {
        var runner = Runner(events: input["events"] as! [JSON])
        let ops = input["ops"] as! [JSON]
        let reportEvery = input["reportEvery"] as! Bool
        var steps: [JSON] = []
        for (i, op) in ops.enumerated() {
            let returned = runner.apply(op)
            var step: JSON = ["returned": returned, "keyCount": runner.ledger.keyCount]
            if reportEvery || i == ops.count - 1 {
                step["ledger"] = runner.ledgerJSON()
                step["hasFired"] = runner.events.map { runner.ledger.hasFired($0) }
                step["isSnoozed"] = runner.events.map { runner.ledger.isSnoozed($0) }
            }
            steps.append(step)
        }
        return ["steps": steps]
    }

    static func ledgerCases() -> [VectorCase] {
        var cases: [VectorCase] = []
        func add(_ name: String, _ input: JSON) {
            cases.append(VectorCase(name: name, input: input, expected: ledgerExpected(input)))
        }
        let now = anchor
        let guardSettings: JSON = ["leadTime": 60.0, "takeoverCalendarKeys": ["s1/c1", "s1/c2"], "hiddenCalendarKeys": [] as [String]]
        func ev(_ id: String, cal: String = "c1", title: String = "Standup", start: Int64, minutes: Int64 = 30, _ extra: JSON = [:]) -> JSON {
            DedupAreas.spec(id, cal: cal, title: title, start: start, end: start + minutes * oneMinute, extra)
        }
        func member(_ j: JSON) -> JSON {
            let e = EventSpec.event(j)
            return ["title": e.title, "calendarKey": e.calendarKey, "contentKey": e.contentKey, "details": "bare", "start": j["start"]!, "end": j["end"]!]
        }
        let a = ev("a", start: now + 10 * oneMinute)
        let aCopy = ev("a2", cal: "c2", title: "STANDUP", start: now + 10 * oneMinute)
        let merged = ev("a", start: now + 10 * oneMinute, ["mergedMembers": [member(a), member(aCopy)], "additionalCalendarKeys": ["s1/c2"], "mergeProvenance": "rule"])
        let split = ev("zzz", start: now + 10 * oneMinute)    // same content, new source id
        let moved = ev("a", start: now + 70 * oneMinute)      // same source id, new start: a new occurrence
        let other = ev("b", title: "Lunch", start: now + 40 * oneMinute)
        let pool = [a, aCopy, merged, split, moved, other]
        // Hand-written.
        add("hand-fired-survives-merge", ["events": pool, "reportEvery": true, "ops": [op("markFired", ["event": 0, "now": now]), op("prune", ["now": now + oneMinute])]])
        add("hand-fired-survives-split", ["events": pool, "reportEvery": true, "ops": [op("markFired", ["event": 2, "now": now])]])
        add("hand-fired-by-copy-content", ["events": pool, "reportEvery": true, "ops": [op("markFired", ["event": 1, "now": now])]])
        add("hand-rescheduled-is-new", ["events": pool, "reportEvery": true, "ops": [op("markFired", ["event": 0, "now": now]), op("markFired", ["event": 5, "now": now + oneMinute])]])
        add("hand-snooze-then-fire", ["events": pool, "reportEvery": true, "ops": [op("snooze", ["event": 0, "duration": 300.0, "now": now]), op("markFired", ["event": 0, "now": now + oneMinute])]])
        add("hand-fired-wins-over-snooze-via-copy", ["events": pool, "reportEvery": true, "ops": [op("snooze", ["event": 0, "duration": 300.0, "now": now]), op("markFired", ["event": 1, "now": now + oneMinute])]])
        add("hand-snooze-capped-at-end", ["events": pool, "reportEvery": true, "ops": [op("snooze", ["event": 5, "duration": 86_400.0, "now": now])]])
        add("hand-prune-ended", ["events": pool, "reportEvery": true, "ops": [op("markFired", ["event": 5, "now": now]), op("prune", ["now": now + 41 * oneMinute + 29 * oneMinute]), op("prune", ["now": now + 5 * oneHour])]])
        add("hand-prune-retention", ["events": pool, "reportEvery": true,
                                      "ops": [op("markFired", ["event": 0, "now": now - 8 * oneDay]), op("prune", ["now": now - 8 * oneDay + oneMinute]), op("markFired", ["event": 5, "now": now]),
                                              op("prune", ["now": now])]])
        add("hand-acknowledge-in-progress", ["events": pool, "reportEvery": true,
                                              "ops": [op("acknowledgeInProgress", ["events": [0, 5], "now": now + 20 * oneMinute, "grace": 60.0]),
                                                      op("acknowledgeInProgress", ["events": [0, 5], "now": now + 20 * oneMinute, "grace": 60.0]),
                                                      op("acknowledgeInProgress", ["events": [4], "now": now + 80 * oneMinute, "grace": 7200.0])]])
        add("hand-acknowledge-skips-ledgered", ["events": pool, "reportEvery": true,
                                                 "ops": [op("snooze", ["event": 0, "duration": 60.0, "now": now]), op("acknowledgeInProgress", ["events": [0, 1, 5], "now": now + 15 * oneMinute, "grace": 0.0])]])
        add("hand-reload", ["events": pool, "reportEvery": true, "ops": [op("markFired", ["event": 2, "now": now]), op("snooze", ["event": 5, "duration": 300.0, "now": now]), op("reload", [:])]])
        func rec(_ entry: String, _ end: Double, _ recordedAt: Double) -> JSON {
            let encoded: JSON = entry == "fired" ? ["fired": [:] as JSON] : ["snoozed": ["until": end] as JSON]
            return ["entry": encoded, "end": end, "recordedAt": recordedAt]
        }
        add("hand-load-lenient", ["events": pool, "reportEvery": true, "ops": [
            op("load", ["json": ["records": [
                "fake/cal/x/1": rec("fired", 800_000_000, 799_000_000),
                "no-recorded-at": ["entry": ["fired": [:] as JSON], "end": 800_000_000.0] as JSON,
                "bad-entry": ["entry": ["unknown": [:] as JSON], "end": 800_000_000.0, "recordedAt": 799_000_000.0] as JSON,
                "snoozed": rec("snoozed", 800_000_300, 799_000_000),
            ] as JSON] as JSON]),
        ]])
        add("hand-load-empty", ["events": pool, "reportEvery": true, "ops": [op("load", ["json": ["records": [:] as JSON] as JSON])]])
        // The guard, one case per decision and the order of its checks.
        let guardOps: [(String, [JSON])] = [
            ("present", [op("guard", ["event": 0, "current": [0], "settings": guardSettings, "now": now, "overlayVisible": false])]),
            ("overlay-visible", [op("guard", ["event": 0, "current": [0], "settings": guardSettings, "now": now, "overlayVisible": true])]),
            ("not-in-snapshot", [op("guard", ["event": 0, "current": [5], "settings": guardSettings, "now": now, "overlayVisible": false])]),
            ("in-snapshot-merged", [op("guard", ["event": 0, "current": [2], "settings": guardSettings, "now": now, "overlayVisible": false])]),
            ("in-snapshot-split", [op("guard", ["event": 2, "current": [3], "settings": guardSettings, "now": now, "overlayVisible": false])]),
            ("moved-is-not-same", [op("guard", ["event": 0, "current": [4], "settings": guardSettings, "now": now, "overlayVisible": false])]),
            ("ended", [op("guard", ["event": 0, "current": [0], "settings": guardSettings, "now": now + 40 * oneMinute, "overlayVisible": false])]),
            ("no-longer-qualifies", [op("guard", ["event": 0, "current": [0], "settings": ["leadTime": 60.0, "takeoverCalendarKeys": [] as [String]] as JSON, "now": now, "overlayVisible": false])]),
            ("already-fired", [op("markFired", ["event": 0, "now": now]), op("guard", ["event": 0, "current": [0], "settings": guardSettings, "now": now, "overlayVisible": false])]),
            ("already-fired-by-copy", [op("markFired", ["event": 1, "now": now]), op("guard", ["event": 0, "current": [0], "settings": guardSettings, "now": now, "overlayVisible": false])]),
            ("snoozed-still-present", [op("snooze", ["event": 0, "duration": 300.0, "now": now]), op("guard", ["event": 0, "current": [0], "settings": guardSettings, "now": now, "overlayVisible": false])]),
            ("overlay-before-ended", [op("guard", ["event": 0, "current": [], "settings": guardSettings, "now": now + 5 * oneHour, "overlayVisible": true])]),
            ("not-in-snapshot-before-ended", [op("guard", ["event": 0, "current": [], "settings": guardSettings, "now": now + 5 * oneHour, "overlayVisible": false])]),
            ("ended-before-qualifies", [op("guard", ["event": 0, "current": [0], "settings": ["enabled": false] as JSON, "now": now + 5 * oneHour, "overlayVisible": false])]),
            ("qualifies-before-fired", [op("markFired", ["event": 0, "now": now]), op("guard", ["event": 0, "current": [0], "settings": ["enabled": false] as JSON, "now": now, "overlayVisible": false])]),
        ]
        for (name, ops) in guardOps { add("hand-guard-\(name)", ["events": pool, "reportEvery": true, "ops": ops]) }

        // Generated sequences over a pool of related events.
        var rng = LCG(seed: 0x4C45_4447_0001)
        var index = 0
        while cases.count < 150 {
            index += 1
            var events: [JSON] = []
            for i in 0..<rng.int(2...4) {
                let start = now + Int64(rng.int(-60...180)) * oneMinute
                let root = ev("r\(i)", cal: rng.pick(["c1", "c2"]), title: rng.pick(Pools.titles), start: start, minutes: Int64(rng.pick([15, 30, 60])))
                events.append(root)
                if rng.chance(50) {
                    let copy = DedupAreas.variant(of: root, id: "c\(i)", &rng, sameCalendarChance: 0, exactChance: 100)
                    events.append(copy)
                    if rng.chance(60) {
                        var mergedEvent = root
                        mergedEvent["mergedMembers"] = [member(root), member(copy)]
                        mergedEvent["additionalCalendarKeys"] = ["\(copy["sourceID"]!)/\(copy["calendarID"]!)"]
                        events.append(mergedEvent)
                    }
                }
                if rng.chance(30) { var splitEvent = root; splitEvent["eventID"] = "split\(i)"; events.append(splitEvent) }
                if rng.chance(25) { var movedEvent = root; movedEvent["start"] = start + oneHour; movedEvent["end"] = i64(root["end"]!) + oneHour; events.append(movedEvent) }
            }
            var ops: [JSON] = []
            var clock = now - Int64(rng.int(0...30)) * oneMinute
            for _ in 0..<rng.int(3...10) {
                clock += Int64(rng.int(0...20)) * oneMinute + Int64(rng.below(1000))
                let i = rng.below(events.count)
                switch rng.below(10) {
                case 0, 1, 2: ops.append(op("markFired", ["event": i, "now": clock]))
                case 3, 4: ops.append(op("snooze", ["event": i, "duration": rng.pick([60.0, 300.0, 600.0, 90.5]), "now": clock]))
                case 5: ops.append(op("acknowledgeInProgress", ["events": subset(Array(0..<events.count), 60, &rng), "now": clock, "grace": rng.pick([0.0, 60.0, 600.0])]))
                case 6: ops.append(op("prune", ["now": clock + Int64(rng.pick([0, 1, 2, 10])) * oneHour]))
                case 7: ops.append(op("reload", [:]))
                default:
                    ops.append(op("guard", [
                        "event": i, "current": subset(Array(0..<events.count), 55, &rng),
                        "settings": settingsJSON(&rng, calendars: ["s1/c1", "s1/c2", "s1/c3", "s2/c1"]), "now": clock, "overlayVisible": rng.chance(15),
                    ]))
                }
            }
            add("generated-\(String(format: "%04d", index))", ["events": events, "reportEvery": true, "ops": ops])
        }

        // The cap on stored keys: 2100 events fired in turn hold 4200 keys, over the 4000 `prune` keeps; the oldest go first.
        var big: [JSON] = []
        var bigOps: [JSON] = []
        for i in 0..<2100 {
            let start = now + oneDay + Int64(i) * oneMinute
            big.append(DedupAreas.spec("b\(i)", title: "Event \(i)", start: start, end: start + oneHour))
            bigOps.append(op("markFired", ["event": i, "now": now + Int64(i) * 1000]))
        }
        bigOps.append(op("prune", ["now": now + 3000 * 1000]))
        add("cap-2100-events-fired-then-pruned", ["events": big, "reportEvery": false, "ops": bigOps])
        return cases
    }
}
