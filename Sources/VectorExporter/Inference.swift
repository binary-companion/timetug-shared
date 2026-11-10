import Foundation
@testable import TimeTugCore
@testable import TimeTugEngine

// inference/prompts.json
//   Entry points: PromptBuilder.prompt(for:timeZone:) and PromptBuilder.instructions (Sources/TimeTugEngine/Inference/PromptBuilder.swift),
//   AdjudicationRequest.input.cacheKey(engineID:) (Sources/TimeTugCore/Dedup/Adjudication.swift: JudgmentInput bounds and escapes the
//   text, then fingerprints it with FNV-1a), and the answer parsing in TextGeneratorAdjudicator.judge(_:) (Sources/TimeTugEngine/
//   Inference/TextGeneratorAdjudicator.swift: trim whitespace and newlines, lowercase, accept exactly same | different | unsure).
//   The parser is not a function of its own, so the answer cases drive `judge` with a stub TextGenerator.
//   fn "prompt":
//     input    { "fn": "prompt", "engineID": String, "zone": IANA id,
//                "request": { "id": String, "first": AdjEvent, "second": AdjEvent, "lessons": [Lesson], "startOffsetMinutes": Int,
//                             "endOffsetMinutes": Int, "overlapMinutes": Int, "firstDetails": String, "secondDetails": String,
//                             "hasConflictingDetails": Bool } }
//                AdjEvent = { "title", "start": ms, "end": ms, "location": String?, "notes": String?, "attendeeNames": [String], "calendarTitle": String? }
//                           (optional members absent when nil; the strings are given as the model would see them before bounding)
//                Lesson   = { "titleA", "titleB", "calendarKeyA", "calendarKeyB", "signalsA", "signalsB", "decision": "same" | "different",
//                             "lastUsed": ms, "contentKeyA": String?, "contentKeyB": String? }
//     expected { "prompt": String, "cacheKey": String, "instructions": String }   // instructions only on the first case
//   fn "answer":
//     input    { "fn": "answer", "mode": "answer" | "throws" | "unavailable" | "temporarilyUnavailable", "raw": String }
//                // "answer": the stub generator is available and returns `raw`; "throws": it is available and throws; the others report unavailable
//     expected { "verdicts": [{ "requestID": "r1", "answer": "same" | "different" | "unsure" }] }   // empty when no verdict results

enum InferenceAreas {
    /// Seconds since 2001-01-01T00:00:00Z, the default `Date` coding of the Codable types decoded below.
    static func referenceSeconds(_ ms: Int64) -> Double { Double(ms) / 1000 - 978_307_200 }

    static func adjudicationEvent(_ j: JSON) -> AdjudicationEvent {
        var o: JSON = [
            "title": j["title"]!, "start": referenceSeconds(i64(j["start"]!)), "end": referenceSeconds(i64(j["end"]!)),
            "attendeeNames": j["attendeeNames"] ?? [] as [String],
        ]
        for key in ["location", "notes", "calendarTitle"] { if let v = j[key] { o[key] = v } }
        return try! JSONDecoder().decode(AdjudicationEvent.self, from: JSONSerialization.data(withJSONObject: o))
    }

    static func lesson(_ j: JSON) -> Lesson {
        var o = j
        o["lastUsed"] = referenceSeconds(i64(j["lastUsed"]!))
        return try! JSONDecoder().decode(Lesson.self, from: JSONSerialization.data(withJSONObject: o))
    }

    static func request(_ j: JSON) -> AdjudicationRequest {
        AdjudicationRequest(
            id: j["id"] as! String, first: adjudicationEvent(j["first"] as! JSON), second: adjudicationEvent(j["second"] as! JSON),
            lessons: (j["lessons"] as! [JSON]).map(lesson), startOffsetMinutes: j["startOffsetMinutes"] as! Int,
            endOffsetMinutes: j["endOffsetMinutes"] as! Int, overlapMinutes: j["overlapMinutes"] as! Int,
            firstDetails: j["firstDetails"] as! String, secondDetails: j["secondDetails"] as! String,
            hasConflictingDetails: j["hasConflictingDetails"] as! Bool)
    }

    struct StubGenerator: TextGenerator {
        enum Failure: Error { case failed }
        var mode: String
        var raw: String
        var availability: TextGeneratorAvailability {
            get async {
                switch mode {
                case "unavailable": .unavailable(reason: "stub")
                case "temporarilyUnavailable": .temporarilyUnavailable(reason: "stub")
                default: .available(EngineInfo(id: "stub", displayName: "Stub", isOnDevice: true))
                }
            }
        }
        func generate(instructions: String, prompt: String) async throws -> String {
            if mode == "throws" { throw Failure.failed }
            return raw
        }
    }

    struct FixedClock: EngineClock {
        func now() -> Date { Date(timeIntervalSince1970: 1_800_000_000) }
        func sleep(until deadline: Date) async throws {}
    }

    /// Runs an async body to completion from the synchronous exporter.
    static func blocking<T: Sendable>(_ body: @escaping @Sendable () async -> T) -> T {
        let box = Box<T>()
        let done = DispatchSemaphore(value: 0)
        Task.detached {
            box.value = await body()
            done.signal()
        }
        done.wait()
        return box.value!
    }

    final class Box<T>: @unchecked Sendable { var value: T? }

    static func expected(_ input: JSON, includeInstructions: Bool = false) -> JSON {
        switch input["fn"] as! String {
        case "prompt":
            let r = request(input["request"] as! JSON)
            var out: JSON = [
                "prompt": PromptBuilder.prompt(for: r, timeZone: TimeZone(identifier: input["zone"] as! String)!),
                "cacheKey": r.input.cacheKey(engineID: input["engineID"] as! String),
            ]
            if includeInstructions { out["instructions"] = PromptBuilder.instructions }
            return out
        case "answer":
            let mode = input["mode"] as! String, raw = input["raw"] as! String
            let verdicts = blocking {
                let adjudicator = TextGeneratorAdjudicator(
                    generator: StubGenerator(mode: mode, raw: raw), clock: FixedClock(), timeZone: { TimeZone(identifier: "UTC")! })
                let event = adjudicationEvent(["title": "A", "start": Int64(0), "end": Int64(1000)])
                return await adjudicator.judge([AdjudicationRequest(id: "r1", first: event, second: event, lessons: [])])
            }
            return ["verdicts": verdicts.map { ["requestID": $0.requestID, "answer": $0.answer.rawValue] as JSON }]
        default: fatalError("unknown fn")
        }
    }

    static func cases() -> [VectorCase] {
        var cases: [VectorCase] = []
        func add(_ name: String, _ input: JSON) {
            cases.append(VectorCase(name: name, input: input, expected: expected(input, includeInstructions: cases.isEmpty)))
        }
        let t = iso("2026-09-18T15:00:00Z")
        func ev(_ title: String, start: Int64, minutes: Int64, _ extra: JSON = [:]) -> JSON {
            var j: JSON = ["title": title, "start": start, "end": start + minutes * oneMinute, "attendeeNames": [] as [String]]
            for (k, v) in extra { j[k] = v }
            return j
        }
        func req(_ first: JSON, _ second: JSON, lessons: [JSON] = [], _ extra: JSON = [:]) -> JSON {
            var j: JSON = [
                "id": "req-1", "first": first, "second": second, "lessons": lessons, "startOffsetMinutes": 0, "endOffsetMinutes": 0, "overlapMinutes": 60,
                "firstDetails": "bare", "secondDetails": "bare", "hasConflictingDetails": false,
            ]
            for (k, v) in extra { j[k] = v }
            return j
        }
        func prompt(_ name: String, _ request: JSON, zone: String = "UTC", engine: String = "fm") {
            add(name, ["fn": "prompt", "engineID": engine, "zone": zone, "request": request])
        }
        func lessonJSON(_ a: String, _ b: String, _ decision: String, signals: (String, String) = ("bare", "location")) -> JSON {
            ["titleA": a, "titleB": b, "calendarKeyA": "s1/c1", "calendarKeyB": "s1/c2", "signalsA": signals.0, "signalsB": signals.1, "decision": decision, "lastUsed": t]
        }
        // Hand-written.
        prompt("prompt-hand-bare", req(ev("Scott: Doctor", start: t, minutes: 60), ev("Intermountain Health", start: t + 15 * oneMinute, minutes: 30)))
        prompt("prompt-hand-detailed", req(
            ev("Intermountain Health", start: t + 15 * oneMinute, minutes: 30, ["location": "1234 Main St", "notes": "Bring card", "attendeeNames": ["Dr. Lee", "Kim"], "calendarTitle": "Family"]),
            ev("Scott: Doctor", start: t, minutes: 60, ["calendarTitle": "Busy Times"]),
            ["startOffsetMinutes": -15, "endOffsetMinutes": 15, "overlapMinutes": 30, "firstDetails": "location+attendees+notes", "secondDetails": "bare"]))
        prompt("prompt-hand-lessons", req(ev("Piano lesson", start: t, minutes: 45), ev("Piano lesson", start: t, minutes: 60),
            lessons: [lessonJSON("piano lesson", "piano", "same"), lessonJSON("doctor", "dance", "different", signals: ("notes", "bare"))],
            ["startOffsetMinutes": 0, "endOffsetMinutes": 15, "overlapMinutes": 45, "firstDetails": "bare", "secondDetails": "conference", "hasConflictingDetails": true]))
        prompt("prompt-hand-escapes", req(ev("He said \"hi\" <b>bold</b> & a/b \\ c", start: t, minutes: 30, ["location": "Room <5>/Annex", "notes": "line1\nline2\ttab", "attendeeNames": ["O'Brien", "Zo\u{EB} <z>"]]),
            ev("\u{1F600} caf\u{E9} \u{4E2D}\u{6587} \u{7} \u{7F}", start: t, minutes: 30)))
        prompt("prompt-hand-email-names-dropped", req(ev("A", start: t, minutes: 30, ["attendeeNames": ["a@x.com", "Kim", "", "kim at x"]]), ev("B", start: t, minutes: 30)))
        prompt("prompt-hand-long-title", req(ev(String(repeating: "x", count: 600), start: t, minutes: 30), ev(String(repeating: "\u{E9}", count: 300), start: t, minutes: 30)))
        prompt("prompt-hand-long-quoted", req(ev(String(repeating: "\"", count: 200), start: t, minutes: 30, ["location": String(repeating: "<", count: 300), "notes": String(repeating: "\u{1F600}", count: 200)]), ev("B", start: t, minutes: 30)))
        prompt("prompt-hand-many-attendees", req(ev("A", start: t, minutes: 30, ["attendeeNames": (1...14).map { "Person number \($0) " + String(repeating: "n", count: 90) }]), ev("B", start: t, minutes: 30)))
        prompt("prompt-hand-equal-offsets", req(ev("A", start: t, minutes: 30), ev("B", start: t, minutes: 30), ["firstDetails": "bare", "secondDetails": "location"]))
        prompt("prompt-hand-details-neither-bare", req(ev("A", start: t, minutes: 30), ev("B", start: t, minutes: 30), ["firstDetails": "location+notes", "secondDetails": "attendees"]))
        prompt("prompt-hand-dst-ambiguous-hour", req(ev("A", start: iso("2026-11-01T05:30:00Z"), minutes: 60), ev("B", start: iso("2026-11-01T06:30:00Z"), minutes: 60)), zone: "America/New_York")
        prompt("prompt-hand-half-hour-zone", req(ev("A", start: t, minutes: 90), ev("B", start: t + 30 * oneMinute, minutes: 30)), zone: "Asia/Kolkata")
        prompt("prompt-hand-empty-strings", req(ev("", start: t, minutes: 30, ["location": "", "notes": "", "calendarTitle": ""]), ev("B", start: t, minutes: 30)))
        prompt("prompt-hand-engine-id-in-key", req(ev("A", start: t, minutes: 30), ev("B", start: t, minutes: 30)), engine: "other engine")

        var rng = LCG(seed: 0x494E_4652_0001)
        let odd = [
            "Weekly <sync>", "Q&A \"live\"", "back\\slash", "forward/slash", "caf\u{E9}", "\u{65E5}\u{672C}\u{8A9E}", "emoji \u{1F389}", "tab\there", "new\nline", "\u{2028}sep", "quote's",
            "\u{1}ctrl", "e\u{301} combining", "\u{FF21}\u{FF22}",
        ]
        let long300 = String(repeating: "word ", count: 80)
        for i in 1...86 {
            func title() -> String { rng.chance(25) ? rng.pick(odd) : (rng.chance(5) ? rng.pick([long300, String(repeating: "<", count: 120)]) : rng.pick(Pools.titles)) }
            func event(_ minutesShift: Int) -> JSON {
                let start = t + Int64(rng.below(48)) * 30 * oneMinute + Int64(minutesShift) * oneMinute
                var e = ev(title(), start: start, minutes: Int64(rng.pick([15, 30, 60, 90])))
                if rng.chance(50) { e["location"] = rng.chance(20) ? rng.pick(odd) : rng.pick(Pools.locations) }
                if rng.chance(40) { e["notes"] = rng.chance(15) ? String(repeating: "note ", count: 150) : rng.pick(["Bring laptop", "see <link>", "a/b"]) }
                if rng.chance(40) { e["attendeeNames"] = (0..<rng.int(1...12)).map { _ in rng.pick(["Kim", "Lee", "a@b.c", "O'Neil", "", "\u{C9}va", String(repeating: "L", count: 100)]) } }
                if rng.chance(50) { e["calendarTitle"] = rng.pick(["Family", "Busy Times", "Work <main>", "", String(repeating: "C", count: 200)]) }
                return e
            }
            var lessons: [JSON] = []
            for _ in 0..<rng.pick([0, 0, 1, 2, 7]) {
                lessons.append(lessonJSON(rng.pick(odd + ["doctor", "piano lesson"]), rng.pick(odd + ["dance", "yoga"]), rng.pick(["same", "different"]),
                                          signals: (rng.pick(["bare", "location+notes", "attendees"]), rng.pick(["bare", "conference", String(repeating: "s", count: 60)]))))
            }
            let details = ["bare", "location", "attendees", "location+attendees", "notes", "conference+notes"]
            prompt("prompt-generated-\(String(format: "%03d", i))", req(
                event(0), event(rng.int(-30...30)), lessons: lessons,
                ["startOffsetMinutes": rng.int(-120...120), "endOffsetMinutes": rng.int(-120...120), "overlapMinutes": rng.int(0...180),
                 "firstDetails": rng.pick(details), "secondDetails": rng.pick(details), "hasConflictingDetails": rng.chance(20)]),
                zone: rng.pick(Pools.zones), engine: rng.pick(["fm", "test-engine", "e\u{301}"]))
        }
        // Answers.
        let answers = [
            "same", "different", "unsure", "Same", "DIFFERENT", "Unsure", " same", "same ", "\nsame\n", "\t different \r\n", "same.", "same!", "Same.", "the same", "sameness",
            "same different", "yes", "no", "", " ", "\n", "unknown", "SAME\u{A0}", "\u{A0}same", "\u{2003}same\u{2003}", "\u{200B}same", "s\u{0301}ame", "\u{FF53}ame", "sam\u{E9}",
            "unsure\u{2028}", "different\u{85}", "\u{3000}different", "SAME\n\nbecause they match", "same, because", "'same'", "\"same\"", "Answer: same", "diff", "uns\u{DF}re", "SAME\u{0}",
            "\u{130}", "different\u{FEFF}", "Different Unsure", "same\r", "\rsame", "\u{1F600}", "unsure \u{200D}",
        ]
        for (i, raw) in answers.enumerated() { add("answer-\(String(format: "%02d", i + 1))", ["fn": "answer", "mode": "answer", "raw": raw]) }
        add("answer-generator-throws", ["fn": "answer", "mode": "throws", "raw": "same"])
        add("answer-unavailable", ["fn": "answer", "mode": "unavailable", "raw": "same"])
        add("answer-temporarily-unavailable", ["fn": "answer", "mode": "temporarilyUnavailable", "raw": "same"])
        return cases
    }
}
