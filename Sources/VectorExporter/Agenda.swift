import CalendarCore
import Foundation
@testable import TimeTugCore

// agenda/day-agenda.json
//   Entry point (Sources/TimeTugCore/Agenda/DayAgenda.swift): DayAgenda.make(events:settings:now:calendar:) with a Gregorian
//   `Calendar` whose time zone is the viewer's zone (the app sets `Calendar.current.timeZone` the same way).
//   input:    { "events": [EventSpec], "settings": { as in takeover/scheduler.json }, "now": Int (ms), "zone": IANA id (the viewer's) }
//   expected: { "items": [{ "index": Int, "id": String, "state": "past" | "current" | "upcoming" }],   // index into events; the order is the agenda's
//               "next": null | { "index": Int, "id": String } }
//
// widget/snapshot.json
//   Entry points (Sources/TimeTugCore/Widget/): WidgetSnapshot.make(events:calendars:settings:now:calendar:), isStale(now:),
//   WidgetTimeline.changeDates(snapshot:now:calendar:limit:), nextUp(snapshot:now:upcomingLimit:), today(snapshot:now:calendar:),
//   visibleToday(rows:capacity:). Every case names its function in input.fn. WidgetEvent = { "id", "title", "start": ms, "end": ms,
//   "isAllDay": Bool, "colorHex": String?, "joinURL": String? } (optional members absent when nil). Snapshot = { "generatedAt": ms,
//   "events": [WidgetEvent] }. Row = { "event": WidgetEvent, "state": "past" | "current" | "upcoming" }.
//   fn "make":
//     input    { "fn", "events": [EventSpec], "calendars": [{ "sourceID", "calendarID", "title", "colorHex": String? }], "settings", "now", "zone" }
//     expected { "snapshot": Snapshot }
//   fn "isStale":
//     input    { "fn", "snapshot", "now" }                      expected { "stale": Bool }
//   fn "changeDates":
//     input    { "fn", "snapshot", "now", "zone", "limit": Int }  expected { "dates": [ms] }
//   fn "nextUp":
//     input    { "fn", "snapshot", "now", "upcomingLimit": Int }  expected { "current": WidgetEvent | null, "upcoming": [WidgetEvent] }
//   fn "today":
//     input    { "fn", "snapshot", "now", "zone" }                expected { "rows": [Row] }
//   fn "visibleToday":
//     input    { "fn", "rows": [Row], "capacity": Int }           expected { "rows": [Row] }

enum AgendaAreas {
    /// A viewer zone and an instant on or near a day with a clock change in that zone (or an ordinary day).
    static let dstDays: [(zone: String, instant: Int64)] = [
        ("America/New_York", iso("2026-03-08T05:00:00Z")), ("America/New_York", iso("2026-11-01T04:00:00Z")),
        ("Europe/Berlin", iso("2026-03-28T23:00:00Z")), ("Europe/London", iso("2026-10-25T00:00:00Z")),
        ("Australia/Sydney", iso("2026-10-03T12:00:00Z")), ("Pacific/Auckland", iso("2026-09-26T10:00:00Z")),
        ("America/St_Johns", iso("2026-03-08T04:00:00Z")), ("America/Los_Angeles", iso("2026-11-01T06:00:00Z")),
        ("America/Sao_Paulo", iso("2026-03-15T00:00:00Z")), ("Asia/Tokyo", iso("2026-09-18T12:00:00Z")),
    ]

    struct Scenario {
        var zone: String
        var now: Int64
        var events: [JSON]
    }

    static func localMidnight(_ instant: Int64, zone: String) -> Int64 {
        ms(EventSpec.calendar(zone: zone).startOfDay(for: date(ms: instant)))
    }

    static func scenario(_ rng: inout LCG, minEvents: Int = 2, maxEvents: Int = 9) -> Scenario {
        let zone: String
        var now: Int64
        if rng.chance(40) {
            let pick = rng.pick(dstDays)
            zone = pick.zone
            now = pick.instant + Int64(rng.int(-6...30)) * oneHour + Int64(rng.below(4)) * 15 * oneMinute
        } else {
            zone = rng.pick(Pools.zones)
            now = iso("2026-09-18T00:00:00Z") + Int64(rng.below(14 * 24 * 4)) * 15 * oneMinute
        }
        if rng.chance(10) { now = localMidnight(now, zone: zone) + Int64(rng.pick([-1, 0, 1])) * 1000 }   // right at a midnight
        let viewer = EventSpec.calendar(zone: zone)
        let todayStart = ms(viewer.startOfDay(for: date(ms: now)))
        var events: [JSON] = []
        for i in 0..<rng.int(minEvents...maxEvents) {
            let cal = rng.pick(["c1", "c2", "c3"])
            var e: JSON
            if rng.chance(25) {
                // All-day in the event's own zone, which often is not the viewer's.
                let eventZone = rng.chance(50) ? zone : rng.pick(Pools.zones)
                let first = AllDay.date(of: date(ms: now), in: viewer.timeZone).adding(days: rng.int(-3...3))
                let end = first.adding(days: rng.pick([0, 1, 1, 1, 2, 3]))
                let canonical = AllDay.canonical(first: first, endExclusive: end, in: TimeZone(identifier: eventZone)!)!
                e = DedupAreas.spec("e\(i)", cal: cal, title: rng.pick(Pools.titles), start: ms(canonical.start), end: ms(canonical.end),
                                    ["isAllDay": true, "timeZone": eventZone])
            } else {
                let start: Int64
                switch rng.below(10) {
                case 0, 1: start = todayStart - Int64(rng.int(1...10)) * oneHour                  // started yesterday, may run into today
                case 2: start = todayStart + 24 * oneHour - Int64(rng.int(0...90)) * oneMinute     // just before midnight
                case 3: start = todayStart + 24 * oneHour + Int64(rng.int(0...180)) * oneMinute    // just after midnight
                case 4: start = todayStart + 48 * oneHour                                            // the day after tomorrow
                default: start = todayStart + Int64(rng.int(0...23 * 4)) * 15 * oneMinute
                }
                let length = Int64(rng.pick([15, 30, 60, 90, 180, 600, 1500])) * oneMinute
                e = DedupAreas.spec("e\(i)", cal: cal, title: rng.pick(Pools.titles), start: start, end: start + length, ["timeZone": rng.pick(Pools.zones)])
            }
            if rng.chance(25) { e["conferences"] = [DedupAreas.conferenceJSON(Pools.conferences[rng.below(4)].url, "zoom", origin: "structured")] }
            if rng.chance(15), e["isAllDay"] == nil, let start = e["start"] {
                // A merged meeting shows the longer copy's range: displayStart earlier than start.
                e["displayStart"] = i64(start) - Int64(rng.pick([15, 30, 60])) * oneMinute
            }
            if rng.chance(12) { e["additionalCalendarKeys"] = ["s1/\(rng.pick(["c1", "c2", "c3"]))"] }
            events.append(e)
        }
        return Scenario(zone: zone, now: now, events: events)
    }

    static func agendaSettings(_ rng: inout LCG) -> JSON {
        var s: JSON = ["leadTime": rng.pick([0.0, 60.0, 300.0, 3600.0, 14400.0])]
        s["hiddenCalendarKeys"] = TakeoverAreas.subset(["s1/c1", "s1/c2", "s1/c3"], 20, &rng)
        s["skipAllDayEvents"] = rng.chance(50)
        return s
    }

    static func stateName(_ state: DayAgenda.State) -> String {
        switch state {
        case .past: "past"
        case .current: "current"
        case .upcoming: "upcoming"
        }
    }

    // MARK: agenda/day-agenda.json

    static func agendaExpected(_ input: JSON) -> JSON {
        let events = (input["events"] as! [JSON]).map(EventSpec.event)
        let agenda = DayAgenda.make(
            events: events, settings: EventSpec.settings(input["settings"] as! JSON), now: date(ms: i64(input["now"]!)),
            calendar: EventSpec.calendar(zone: input["zone"] as! String))
        return [
            "items": agenda.items.map { item -> JSON in
                ["index": events.firstIndex(of: item.event)!, "id": item.id, "state": stateName(item.state)]
            },
            "next": agenda.next.map { ["index": events.firstIndex(of: $0)!, "id": $0.id] as JSON } ?? NSNull(),
        ]
    }

    static func agendaCases() -> [VectorCase] {
        var cases: [VectorCase] = []
        func add(_ name: String, _ input: JSON) {
            cases.append(VectorCase(name: name, input: input, expected: agendaExpected(input)))
        }
        let now = iso("2026-09-18T15:00:00Z")
        func settings(_ extra: JSON = [:]) -> JSON {
            var s: JSON = ["leadTime": 60.0]
            for (k, v) in extra { s[k] = v }
            return s
        }
        func ev(_ id: String, start: Int64, minutes: Int64, _ extra: JSON = [:]) -> JSON {
            DedupAreas.spec(id, title: "Event \(id)", start: start, end: start + minutes * oneMinute, extra)
        }
        // Hand-written, after DayAgendaTests.
        add("hand-states", ["events": [ev("past", start: now - 3 * oneHour, minutes: 60), ev("current", start: now - 10 * oneMinute, minutes: 60), ev("upcoming", start: now + oneHour, minutes: 30)],
                            "settings": settings(), "now": now, "zone": "UTC"])
        add("hand-ends-exactly-now", ["events": [ev("a", start: now - oneHour, minutes: 60)], "settings": settings(), "now": now, "zone": "UTC"])
        add("hand-starts-exactly-now", ["events": [ev("a", start: now, minutes: 60), ev("b", start: now + 1, minutes: 60)], "settings": settings(), "now": now, "zone": "UTC"])
        add("hand-midnight-span-in", ["events": [ev("a", start: iso("2026-09-17T23:00:00Z"), minutes: 120)], "settings": settings(), "now": now, "zone": "UTC"])
        add("hand-ends-at-midnight-out", ["events": [ev("a", start: iso("2026-09-17T23:00:00Z"), minutes: 60)], "settings": settings(), "now": now, "zone": "UTC"])
        add("hand-after-midnight-lead-time", ["events": [ev("a", start: iso("2026-09-19T00:30:00Z"), minutes: 30)], "settings": settings(["leadTime": 3600.0]), "now": iso("2026-09-18T23:30:00Z"), "zone": "UTC"])
        add("hand-after-midnight-not-yet", ["events": [ev("a", start: iso("2026-09-19T00:30:00Z"), minutes: 30)], "settings": settings(["leadTime": 60.0]), "now": iso("2026-09-18T23:30:00Z"), "zone": "UTC"])
        add("hand-hidden-calendar", ["events": [ev("a", start: now + oneHour, minutes: 30), ev("b", start: now + 2 * oneHour, minutes: 30, ["calendarID": "c2"])], "settings": settings(["hiddenCalendarKeys": ["s1/c1"]]), "now": now, "zone": "UTC"])
        add("hand-merged-hidden-only-if-all", ["events": [ev("a", start: now + oneHour, minutes: 30, ["additionalCalendarKeys": ["s1/c2"]])], "settings": settings(["hiddenCalendarKeys": ["s1/c1"]]), "now": now, "zone": "UTC"])
        add("hand-merged-all-hidden", ["events": [ev("a", start: now + oneHour, minutes: 30, ["additionalCalendarKeys": ["s1/c2"]])], "settings": settings(["hiddenCalendarKeys": ["s1/c1", "s1/c2"]]), "now": now, "zone": "UTC"])
        add("hand-sort-ties-by-title", ["events": [ev("b", start: now + oneHour, minutes: 30, ["title": "Zed"]), ev("a", start: now + oneHour, minutes: 30, ["title": "Alpha"])], "settings": settings(), "now": now, "zone": "UTC"])
        // All-day events read in their own zone.
        let sydneyStart = AllDay.startOfDay(CalendarDate(year: 2026, month: 9, day: 18), in: TimeZone(identifier: "Australia/Sydney")!)!
        let sydneyEnd = AllDay.startOfDay(CalendarDate(year: 2026, month: 9, day: 19), in: TimeZone(identifier: "Australia/Sydney")!)!
        for (name, zone, instant) in [("sydney-viewed-in-utc", "UTC", now), ("sydney-viewed-in-la", "America/Los_Angeles", now), ("sydney-viewed-in-sydney", "Australia/Sydney", now), ("sydney-day-after", "UTC", now + oneDay), ("sydney-day-before", "UTC", now - oneDay)] {
            add("hand-all-day-\(name)", ["events": [ev("allday", start: ms(sydneyStart), minutes: 0, ["end": ms(sydneyEnd), "isAllDay": true, "timeZone": "Australia/Sydney"])],
                                          "settings": settings(["skipAllDayEvents": false]), "now": instant, "zone": zone])
        }
        add("hand-all-day-skipped", ["events": [ev("allday", start: ms(sydneyStart), minutes: 0, ["end": ms(sydneyEnd), "isAllDay": true, "timeZone": "Australia/Sydney"])], "settings": settings(["skipAllDayEvents": true]), "now": now, "zone": "Australia/Sydney"])
        add("hand-all-day-same-date-is-one-day", ["events": [ev("allday", start: iso("2026-09-18T00:00:00Z"), minutes: 0, ["isAllDay": true])], "settings": settings(["skipAllDayEvents": false]), "now": now, "zone": "UTC"])
        add("hand-all-day-multi-day-past", ["events": [ev("allday", start: iso("2026-09-15T00:00:00Z"), minutes: 0, ["end": iso("2026-09-18T00:00:00Z"), "isAllDay": true])], "settings": settings(["skipAllDayEvents": false]), "now": now, "zone": "UTC"])
        add("hand-all-day-multi-day-current", ["events": [ev("allday", start: iso("2026-09-17T00:00:00Z"), minutes: 0, ["end": iso("2026-09-20T00:00:00Z"), "isAllDay": true])], "settings": settings(["skipAllDayEvents": false]), "now": now, "zone": "UTC"])
        add("hand-all-day-never-next", ["events": [ev("allday", start: iso("2026-09-18T00:00:00Z"), minutes: 0, ["end": iso("2026-09-19T00:00:00Z"), "isAllDay": true]), ev("b", start: now + oneHour, minutes: 30)], "settings": settings(["skipAllDayEvents": false]), "now": now, "zone": "UTC"])
        // A day with 23 and with 25 hours.
        add("hand-dst-spring-forward-day", ["events": [ev("a", start: iso("2026-03-08T04:30:00Z"), minutes: 60), ev("b", start: iso("2026-03-09T03:30:00Z"), minutes: 60), ev("c", start: iso("2026-03-09T04:30:00Z"), minutes: 60)],
                                            "settings": settings(), "now": iso("2026-03-08T15:00:00Z"), "zone": "America/New_York"])
        add("hand-dst-fall-back-day", ["events": [ev("a", start: iso("2026-11-01T03:30:00Z"), minutes: 60), ev("b", start: iso("2026-11-02T04:30:00Z"), minutes: 60), ev("c", start: iso("2026-11-02T05:30:00Z"), minutes: 60)],
                                       "settings": settings(), "now": iso("2026-11-01T15:00:00Z"), "zone": "America/New_York"])
        add("hand-half-hour-zone", ["events": [ev("a", start: iso("2026-09-17T18:30:00Z"), minutes: 30), ev("b", start: iso("2026-09-18T18:29:00Z"), minutes: 30)], "settings": settings(), "now": now, "zone": "Asia/Kolkata"])

        var rng = LCG(seed: 0x4147_454E_0001)
        var index = 0
        while cases.count < 320 {
            index += 1
            let s = scenario(&rng)
            add("generated-\(String(format: "%04d", index))", ["events": s.events, "settings": agendaSettings(&rng), "now": s.now, "zone": s.zone])
        }
        return cases
    }

    // MARK: widget/snapshot.json

    static func widgetEventJSON(_ e: WidgetEvent) -> JSON {
        var j: JSON = ["id": e.id, "title": e.title, "start": ms(e.start), "end": ms(e.end), "isAllDay": e.isAllDay]
        if let hex = e.colorHex { j["colorHex"] = hex }
        if let url = e.joinURL { j["joinURL"] = url.absoluteString }
        return j
    }

    static func widgetEvent(_ j: JSON) -> WidgetEvent {
        WidgetEvent(
            id: j["id"] as! String, title: j["title"] as! String, start: date(ms: i64(j["start"]!)), end: date(ms: i64(j["end"]!)),
            isAllDay: j["isAllDay"] as! Bool, colorHex: j["colorHex"] as? String, joinURL: (j["joinURL"] as? String).flatMap { URL(string: $0) })
    }

    static func snapshotJSON(_ s: WidgetSnapshot) -> JSON {
        ["generatedAt": ms(s.generatedAt), "events": s.events.map(widgetEventJSON)]
    }

    static func snapshot(_ j: JSON) -> WidgetSnapshot {
        WidgetSnapshot(generatedAt: date(ms: i64(j["generatedAt"]!)), events: (j["events"] as! [JSON]).map(widgetEvent))
    }

    static func rowJSON(_ r: WidgetTimeline.TodayRow) -> JSON { ["event": widgetEventJSON(r.event), "state": stateName(r.state)] }

    static func row(_ j: JSON) -> WidgetTimeline.TodayRow {
        let state: DayAgenda.State
        switch j["state"] as! String {
        case "past": state = .past
        case "current": state = .current
        default: state = .upcoming
        }
        return WidgetTimeline.TodayRow(event: widgetEvent(j["event"] as! JSON), state: state)
    }

    static func widgetExpected(_ input: JSON) -> JSON {
        switch input["fn"] as! String {
        case "make":
            let calendars = (input["calendars"] as! [JSON]).map {
                CalendarInfo(sourceID: $0["sourceID"] as! String, calendarID: $0["calendarID"] as! String, title: $0["title"] as! String,
                             colorHex: $0["colorHex"] as? String)
            }
            let made = WidgetSnapshot.make(
                events: (input["events"] as! [JSON]).map(EventSpec.event), calendars: calendars, settings: EventSpec.settings(input["settings"] as! JSON),
                now: date(ms: i64(input["now"]!)), calendar: EventSpec.calendar(zone: input["zone"] as! String))
            return ["snapshot": snapshotJSON(made)]
        case "isStale":
            return ["stale": snapshot(input["snapshot"] as! JSON).isStale(now: date(ms: i64(input["now"]!)))]
        case "changeDates":
            let dates = WidgetTimeline.changeDates(
                snapshot: snapshot(input["snapshot"] as! JSON), now: date(ms: i64(input["now"]!)),
                calendar: EventSpec.calendar(zone: input["zone"] as! String), limit: input["limit"] as! Int)
            return ["dates": dates.map { ms($0) }]
        case "nextUp":
            let next = WidgetTimeline.nextUp(snapshot: snapshot(input["snapshot"] as! JSON), now: date(ms: i64(input["now"]!)), upcomingLimit: input["upcomingLimit"] as! Int)
            return ["current": next.current.map(widgetEventJSON) ?? NSNull(), "upcoming": next.upcoming.map(widgetEventJSON)]
        case "today":
            let rows = WidgetTimeline.today(
                snapshot: snapshot(input["snapshot"] as! JSON), now: date(ms: i64(input["now"]!)), calendar: EventSpec.calendar(zone: input["zone"] as! String))
            return ["rows": rows.map(rowJSON)]
        case "visibleToday":
            let rows = WidgetTimeline.visibleToday(rows: (input["rows"] as! [JSON]).map(row), capacity: input["capacity"] as! Int)
            return ["rows": rows.map(rowJSON)]
        default: fatalError("unknown fn")
        }
    }

    static func widgetCases() -> [VectorCase] {
        var cases: [VectorCase] = []
        func add(_ name: String, _ input: JSON) {
            cases.append(VectorCase(name: name, input: input, expected: widgetExpected(input)))
        }
        let now = iso("2026-09-18T15:00:00Z")
        func we(_ id: String, start: Int64, minutes: Int64, allDay: Bool = false, _ extra: JSON = [:]) -> JSON {
            var j: JSON = ["id": id, "title": "Title \(id)", "start": start, "end": start + minutes * oneMinute, "isAllDay": allDay]
            for (k, v) in extra { j[k] = v }
            return j
        }
        func snap(_ events: [JSON], generatedAt: Int64 = iso("2026-09-18T12:00:00Z")) -> JSON { ["generatedAt": generatedAt, "events": events] }
        let calendars: [JSON] = [
            ["sourceID": "s1", "calendarID": "c1", "title": "One", "colorHex": "#abc"], ["sourceID": "s1", "calendarID": "c2", "title": "Two", "colorHex": "ff8800"],
            ["sourceID": "s1", "calendarID": "c3", "title": "Three"],
        ]
        // Hand-written.
        add("hand-stale-boundary-exact", ["fn": "isStale", "snapshot": snap([], generatedAt: now - 12 * oneHour), "now": now])
        add("hand-stale-boundary-over", ["fn": "isStale", "snapshot": snap([], generatedAt: now - 12 * oneHour - 1), "now": now])
        add("hand-stale-future", ["fn": "isStale", "snapshot": snap([], generatedAt: now + oneHour), "now": now])
        let tl = snap([we("past", start: now - 3 * oneHour, minutes: 60), we("cur", start: now - 10 * oneMinute, minutes: 60), we("cur2", start: now - 5 * oneMinute, minutes: 90),
                       we("next", start: now + 30 * oneMinute, minutes: 30), we("next2", start: now + 30 * oneMinute, minutes: 30, ["title": "Aardvark"]), we("later", start: now + 5 * oneHour, minutes: 30),
                       we("allday", start: iso("2026-09-18T00:00:00Z"), minutes: 1440, allDay: true)])
        add("hand-nextup", ["fn": "nextUp", "snapshot": tl, "now": now, "upcomingLimit": 2])
        add("hand-nextup-limit-zero", ["fn": "nextUp", "snapshot": tl, "now": now, "upcomingLimit": 0])
        add("hand-nextup-boundary-start", ["fn": "nextUp", "snapshot": snap([we("a", start: now, minutes: 30)]), "now": now, "upcomingLimit": 3])
        add("hand-nextup-boundary-end", ["fn": "nextUp", "snapshot": snap([we("a", start: now - 30 * oneMinute, minutes: 30)]), "now": now, "upcomingLimit": 3])
        add("hand-change-dates", ["fn": "changeDates", "snapshot": tl, "now": now, "zone": "UTC", "limit": 20])
        add("hand-change-dates-limit", ["fn": "changeDates", "snapshot": tl, "now": now, "zone": "UTC", "limit": 3])
        add("hand-change-dates-midnights-only", ["fn": "changeDates", "snapshot": snap([]), "now": now, "zone": "America/New_York", "limit": 5])
        add("hand-change-dates-dst", ["fn": "changeDates", "snapshot": snap([]), "now": iso("2026-03-07T15:00:00Z"), "zone": "America/New_York", "limit": 5])
        add("hand-today", ["fn": "today", "snapshot": tl, "now": now, "zone": "UTC"])
        let rows = [we("a", start: now - 3 * oneHour, minutes: 60), we("b", start: now - 2 * oneHour, minutes: 60), we("c", start: now - 10 * oneMinute, minutes: 60), we("d", start: now + oneHour, minutes: 30)].enumerated().map { i, e -> JSON in
            ["event": e, "state": ["past", "past", "current", "upcoming"][i]]
        }
        for capacity in [0, 1, 2, 3, 4, 5] { add("hand-visible-capacity-\(capacity)", ["fn": "visibleToday", "rows": rows, "capacity": capacity]) }
        add("hand-visible-all-past", ["fn": "visibleToday", "rows": Array(rows.prefix(2)), "capacity": 1])
        add("hand-visible-empty", ["fn": "visibleToday", "rows": [] as [JSON], "capacity": 2])

        var rng = LCG(seed: 0x5749_4447_0001)
        var index = 0
        var made: [(JSON, Int64, String)] = []   // snapshots produced by make, reused for the other functions
        func name(_ kind: String) -> String { index += 1; return "generated-\(kind)-\(String(format: "%04d", index))" }
        for _ in 0..<130 {
            let s = scenario(&rng, minEvents: 1, maxEvents: 10)
            let input: JSON = ["fn": "make", "events": s.events, "calendars": TakeoverAreas.subset(calendars, 80, &rng), "settings": agendaSettings(&rng), "now": s.now, "zone": s.zone]
            let n = name("make")
            add(n, input)
            made.append((widgetExpected(input)["snapshot"] as! JSON, s.now, s.zone))
        }
        for _ in 0..<35 {
            let (snapshot, _, _) = rng.pick(made)
            let age = Int64(rng.pick([0, 3600, 43_199, 43_200, 43_201, 86_400, -3600])) * 1000
            add(name("stale"), ["fn": "isStale", "snapshot": snapshot, "now": (snapshot["generatedAt"] as! Int64) + age + (rng.chance(30) ? 1 : 0)])
        }
        for _ in 0..<50 {
            let (snapshot, snapNow, zone) = rng.pick(made)
            add(name("change"), ["fn": "changeDates", "snapshot": snapshot, "now": snapNow + Int64(rng.int(-3...3)) * oneHour, "zone": zone, "limit": rng.pick([1, 2, 5, 10, 50])])
        }
        for _ in 0..<50 {
            let (snapshot, snapNow, _) = rng.pick(made)
            add(name("nextup"), ["fn": "nextUp", "snapshot": snapshot, "now": snapNow + Int64(rng.int(-3...3)) * oneHour, "upcomingLimit": rng.pick([0, 1, 2, 3, 5])])
        }
        var todayRows: [[JSON]] = []
        for _ in 0..<50 {
            let (snapshot, snapNow, zone) = rng.pick(made)
            let input: JSON = ["fn": "today", "snapshot": snapshot, "now": snapNow + Int64(rng.int(-3...3)) * oneHour, "zone": zone]
            add(name("today"), input)
            todayRows.append(widgetExpected(input)["rows"] as! [JSON])
        }
        for _ in 0..<35 {
            add(name("visible"), ["fn": "visibleToday", "rows": rng.pick(todayRows), "capacity": rng.pick([0, 1, 2, 3, 4, 6])])
        }
        return cases
    }
}
