import Foundation
@testable import TimeTugCore

// text/fold.json
//   Entry point: DuplicateRules.normalize(_:) (Sources/TimeTugCore/Dedup/DuplicateRules.swift): lowercased,
//   accent-folded (`folding(options: [.caseInsensitive, .diacriticInsensitive], locale: nil)`), every character that
//   is not a letter or number turned into a space, runs of spaces collapsed to one, ends trimmed.
//   input:    { "text": String }
//   expected: { "result": String }
//
// text/case-compare.json
//   Entry point: `a.caseInsensitiveCompare(b) == .orderedSame` (AccountManager.sameAccount compares display names this way).
//   input:    { "a": String, "b": String }
//   expected: { "same": Bool }
//
// text/dedup-key.json
//   Entry point: TimeTugCalendarEvent.contentKey, "\(title.lowercased())|\(Int(start.timeIntervalSince1970))|\(Int(end.timeIntervalSince1970))"
//   (Sources/TimeTugCore/Model/TimeTugCalendarEvent.swift, the `contentKey` property). `Int(Double)` truncates toward
//   zero, so sub-second and negative instants matter.
//   input:    { "title": String, "start": Int (Unix ms), "end": Int (Unix ms) }
//   expected: { "key": String }

enum TextAreas {
    static func fold(_ text: String) -> VectorCase {
        VectorCase(name: "", input: ["text": text], expected: ["result": DuplicateRules.normalize(text)])
    }

    static func foldCases() -> [VectorCase] {
        var cases: [VectorCase] = []
        func add(_ name: String, _ text: String) {
            var c = fold(text)
            c.name = name
            cases.append(c)
        }
        // Hand-written.
        let hand = [
            "Caf\u{E9}", "CAF\u{C9}", "  Hello,   World!  ", "Stand-up (daily)", "O'Brien's 1:1", "\u{1F600} party \u{1F389}", "", "   ", "!!!",
            "Stra\u{DF}e", "\u{130}stanbul", "\u{131}", "\u{FB01}ne", "\u{3A3}\u{3A4}\u{391}\u{3A3}", "\u{3C3}\u{3C2}", "e\u{301}", "e\u{301}\u{301}",
            "\u{1E9B}\u{323}", "\u{FF21}\u{FF22}\u{FF23}", "\u{FF11}\u{FF12}\u{FF13}", "tab\there", "line\nbreak", "non\u{A0}breaking",
            "\u{AA}\u{BA}", "x\u{200B}y", "\u{2160}\u{2161}", "\u{BD} price", "caf\u{E9}\u{2014}bar", "A.B.C.", "r\u{E9}sum\u{E9} \u{2013} 2026",
        ]
        for (i, text) in hand.enumerated() { add("hand-\(i + 1)", text) }
        // Every scalar of the named blocks, alone.
        let blocks: [(String, ClosedRange<UInt32>)] = [
            ("latin1", 0xA0...0xFF), ("latin-ext-a", 0x100...0x17F), ("latin-ext-b", 0x180...0x24F),
            ("greek", 0x370...0x3FF), ("cyrillic", 0x400...0x4FF), ("fullwidth", 0xFF00...0xFFEF),
        ]
        for (block, range) in blocks {
            for value in range {
                guard let scalar = Unicode.Scalar(value) else { continue }
                add("\(block)-\(String(value, radix: 16, uppercase: true))", String(Character(scalar)))
            }
        }
        // Combining marks U+0300 to U+036F applied to a e i o u n c.
        for value in UInt32(0x300)...UInt32(0x36F) {
            for base in ["a", "e", "i", "o", "u", "n", "c"] {
                add("combining-\(base)-\(String(value, radix: 16, uppercase: true))", base + String(Character(Unicode.Scalar(value)!)))
            }
        }
        // 200 mixed strings from a fixed seed.
        var rng = LCG(seed: 0x464F_4C44_0001)
        let fragments = [
            "Caf\u{E9}", "r\u{E9}union", "\u{C9}COLE", "na\u{EF}ve", "Z\u{FC}rich", "se\u{F1}or", "\u{141}\u{F3}d\u{17A}", "Stra\u{DF}e", "\u{39A}\u{3B1}\u{3BB}\u{3B7}\u{3BC}\u{3AD}\u{3C1}\u{3B1}",
            "\u{41F}\u{440}\u{438}\u{432}\u{435}\u{442}", "meeting", "Q4", "2026", "1:1", "-", "--", "(", ")", "/", "&", "'", "\"", ".", ",", "!", "?",
            "\u{1F600}", "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}", "\u{1F1FA}\u{1F1F8}", "  ", "\t", "\u{A0}", "\u{2014}", "\u{2026}", "\u{FF21}\u{FF42}", "\u{FB01}",
            "e\u{301}", "n\u{303}", "\u{130}", "\u{3A3}", "\u{3C2}",
        ]
        for i in 1...200 {
            let count = rng.int(1...6)
            let text = (0..<count).map { _ in rng.pick(fragments) }.joined(separator: rng.chance(60) ? " " : "")
            add("mixed-\(i)", text)
        }
        return cases
    }

    static func caseCompareCases() -> [VectorCase] {
        var cases: [VectorCase] = []
        func add(_ name: String, _ a: String, _ b: String) {
            cases.append(VectorCase(
                name: name, input: ["a": a, "b": b],
                expected: ["same": a.caseInsensitiveCompare(b) == .orderedSame]))
        }
        let hand: [(String, String)] = [
            ("Stra\u{DF}e", "STRASSE"), ("Stra\u{DF}e", "strasse"), ("\u{DF}", "SS"), ("\u{DF}", "ss"), ("\u{1E9E}", "\u{DF}"), ("\u{1E9E}", "ss"),
            ("i", "I"), ("i", "\u{130}"), ("\u{131}", "I"), ("\u{131}", "i"), ("\u{130}", "I"), ("\u{130}stanbul", "istanbul"),
            ("\u{3C3}", "\u{3A3}"), ("\u{3C2}", "\u{3A3}"), ("\u{3C2}", "\u{3C3}"), ("\u{39F}\u{394}\u{3A5}\u{3A3}\u{3A3}\u{395}\u{3A5}\u{3A3}", "\u{3BF}\u{3B4}\u{3C5}\u{3C3}\u{3C3}\u{3B5}\u{3C5}\u{3C2}"),
            ("\u{39F}\u{394}\u{3A5}\u{3A3}\u{3A3}\u{395}\u{3A5}\u{3A3}", "\u{3BF}\u{3B4}\u{3C5}\u{3C3}\u{3C3}\u{3B5}\u{3C5}\u{3C3}"),
            ("K", "\u{212A}"), ("k", "\u{212A}"), ("\u{17F}", "s"), ("\u{17F}", "S"), ("\u{E5}", "\u{212B}"), ("\u{C5}", "\u{212B}"),
            ("\u{1C5}", "\u{1C4}"), ("\u{1C5}", "\u{1C6}"), ("\u{1C4}", "\u{1C6}"), ("\u{FB01}", "FI"), ("\u{FB01}", "fi"), ("\u{FB00}", "ff"),
            ("\u{C6}", "\u{E6}"), ("\u{C6}", "AE"), ("\u{152}", "OE"), ("\u{152}", "\u{153}"),
            ("caf\u{E9}", "CAF\u{C9}"), ("caf\u{E9}", "cafe"), ("caf\u{E9}", "cafe\u{301}"), ("CAFE\u{301}", "caf\u{E9}"), ("e\u{301}", "\u{E9}"), ("\u{C9}", "e\u{301}"),
            ("", ""), (" ", ""), ("a", "a "), ("A", "a"), ("Z", "z"), ("abc", "ABD"), ("abc", "ab"), ("ab", "abc"),
            ("Scott@Example.com", "scott@example.com"), ("O'Brien", "o\u{2019}brien"), ("\u{FF21}", "\u{FF41}"), ("\u{FF21}", "a"),
            ("\u{41F}\u{420}\u{418}\u{412}\u{415}\u{422}", "\u{43F}\u{440}\u{438}\u{432}\u{435}\u{442}"), ("\u{401}", "\u{435}\u{308}"), ("\u{401}", "\u{451}"),
            ("\u{1F600}", "\u{1F600}"), ("\u{10400}", "\u{10428}"), ("\u{2160}", "\u{2170}"), ("\u{24B6}", "\u{24D0}"), ("\u{3A9}", "\u{2126}"), ("\u{3C9}", "\u{2126}"),
            ("\u{3F4}", "\u{3B8}"), ("\u{3D1}", "\u{398}"), ("\u{1E9E}", "SS"), ("MASSE", "Ma\u{DF}e"), ("\u{130}", "i\u{307}"), ("i\u{307}", "\u{130}"),
        ]
        for (i, pair) in hand.enumerated() { add("hand-\(i + 1)", pair.0, pair.1) }
        // Each scalar of the four alphabetic blocks against its own upper- and lower-case form.
        let blocks: [(String, ClosedRange<UInt32>)] = [
            ("latin1", 0xC0...0xFF), ("latin-ext-a", 0x100...0x17F), ("greek", 0x370...0x3FF), ("cyrillic", 0x400...0x4FF),
        ]
        for (block, range) in blocks {
            for value in range {
                guard let scalar = Unicode.Scalar(value) else { continue }
                let s = String(Character(scalar))
                add("\(block)-\(String(value, radix: 16, uppercase: true))-upper", s, s.uppercased())
                add("\(block)-\(String(value, radix: 16, uppercase: true))-lower", s, s.lowercased())
            }
        }
        // Generated: a word in a random case against a random case of itself or of a near miss.
        var rng = LCG(seed: 0x4341_5345_0001)
        let words = [
            "Meeting", "R\u{E9}union", "Stra\u{DF}e", "\u{39F}\u{394}\u{3A5}\u{3A3}\u{3A3}\u{395}\u{3A5}\u{3A3}", "\u{130}zmir", "kitchen", "Scott", "Kristin", "Z\u{FC}rich", "\u{41C}\u{43E}\u{441}\u{43A}\u{432}\u{430}",
            "\u{141}\u{F3}d\u{17A}", "ma\u{DF}", "\u{FB01}nal", "Ma\u{17F}", "KELVIN", "\u{3A3}\u{39F}\u{3A6}\u{39F}\u{3A3}", "caf\u{E9}", "Na\u{EF}ve",
        ]
        func recase(_ s: String, _ mode: Int) -> String {
            switch mode {
            case 0: s.uppercased()
            case 1: s.lowercased()
            case 2: s
            default: String(s.enumerated().map { $0.offset % 2 == 0 ? Character($0.element.uppercased()) : Character($0.element.lowercased()) })
            }
        }
        for i in 1...160 {
            let word = rng.pick(words)
            var other = word
            switch rng.below(5) {
            case 0: other = rng.pick(words)
            case 1: other = word + "s"
            case 2: other = String(word.dropFirst())
            default: break
            }
            add("generated-\(i)", recase(word, rng.below(4)), recase(other, rng.below(4)))
        }
        return cases
    }

    static func dedupKeyCases() -> [VectorCase] {
        var cases: [VectorCase] = []
        func add(_ name: String, _ title: String, _ start: Int64, _ end: Int64) {
            let event = EventSpec.event([
                "eventID": "e", "sourceID": "s", "calendarID": "c", "title": title, "start": start, "end": end,
            ])
            cases.append(VectorCase(
                name: name, input: ["title": title, "start": start, "end": end], expected: ["key": event.contentKey]))
        }
        let t0 = iso("2026-09-18T10:00:00Z")
        let hand: [(String, Int64, Int64)] = [
            ("Standup", t0, t0 + 30 * oneMinute), ("STANDUP", t0, t0 + 30 * oneMinute), ("Caf\u{C9}", t0, t0 + oneHour),
            ("Stra\u{DF}e", t0, t0 + oneHour), ("STRASSE", t0, t0 + oneHour), ("\u{130}stanbul", t0, t0 + oneHour),
            ("\u{39F}\u{394}\u{3A5}\u{3A3}\u{3A3}\u{395}\u{3A5}\u{3A3}", t0, t0 + oneHour), ("\u{3A3}", t0, t0 + oneHour), ("A\u{3A3} B", t0, t0 + oneHour),
            ("", t0, t0), ("a|b", t0, t0 + oneHour), ("\u{1F600}", t0, t0 + oneHour),
            ("Sub-second", t0 + 999, t0 + 1999), ("Half second", t0 + 500, t0 + 1500), ("Negative", -500, 500), ("Negative2", -1, -1500),
            ("Epoch", 0, 0), ("Before epoch", -86_400_000, -3_600_000), ("Far future", iso("2100-01-01T00:00:00Z"), iso("2100-01-01T01:00:00Z")),
            ("Truncation", 1_700_000_000_999, 1_700_000_001_001), ("Year 1900", iso("1900-01-01T00:00:00Z"), iso("1900-01-01T01:00:00Z")),
        ]
        for (i, c) in hand.enumerated() { add("hand-\(i + 1)", c.0, c.1, c.2) }
        var rng = LCG(seed: 0x4B45_5900_0001)
        for i in 1...80 {
            let title = rng.pick(Pools.titles + ["\u{3A3}\u{3A3}", "\u{130}", "\u{DF}", "ALL CAPS TITLE", "MiXeD cAsE"])
            let start = iso("2026-01-01T00:00:00Z") + Int64(rng.below(400)) * oneDay + Int64(rng.below(86_400_000))
            let end = start + Int64(rng.below(8 * 3_600_000)) - (rng.chance(10) ? 3_600_000 : 0)
            add("generated-\(i)", title, start, end)
        }
        return cases
    }
}
