import Foundation
import TimeTugEngineProtocol

// wire/dates.json
//   Entry points (Sources/TimeTugEngineProtocol/WireTypes.swift): WireDates.string(_ date: Date) -> String and
//   WireDates.date(_ text: String) -> Date?. The wire writes `2025-10-09T08:53:20.123Z` (UTC, exactly three fraction digits,
//   rounded to the nearest millisecond) and reads a missing fraction or up to nine digits.
//   fn "string":
//     input    { "fn": "string", "seconds": Number }   // the Date as a Double, seconds since 1970-01-01T00:00:00Z (negative, fractional,
//                                                      // sub-millisecond and exact .5 ms cases included; the number is the Double's shortest text)
//     expected { "text": String }
//   fn "date":
//     input    { "fn": "date", "text": String }
//     expected { "seconds": Number | null, "millis": Int | null }   // Date.timeIntervalSince1970 of the result as a Double, and
//                                                                   // that value times 1000 rounded; both null when WireDates returns nil.
//                                                                   // `millis` is exact. `seconds` is Foundation's Date, which keeps seconds
//                                                                   // since 2001, so far from 2001 it can differ from the exact value by
//                                                                   // about 1e-7 s (1969-12-31T23:59:59.999Z reads as -0.0010000467...);
//                                                                   // a replay compares `seconds` within 1e-6 and `millis` exactly.

enum WireAreas {
    static func stringCase(_ name: String, _ seconds: Double) -> VectorCase {
        VectorCase(
            name: name, input: ["fn": "string", "seconds": seconds],
            expected: ["text": WireDates.string(Date(timeIntervalSince1970: seconds))])
    }

    static func dateCase(_ name: String, _ text: String) -> VectorCase {
        let parsed = WireDates.date(text)
        let expected: JSON = [
            "seconds": parsed.map { $0.timeIntervalSince1970 as Any } ?? NSNull(),
            "millis": parsed.map { Int64(($0.timeIntervalSince1970 * 1000).rounded()) as Any } ?? NSNull(),
        ]
        return VectorCase(name: name, input: ["fn": "date", "text": text], expected: expected)
    }

    static func cases() -> [VectorCase] {
        var cases: [VectorCase] = []
        // Writing.
        let handSeconds: [Double] = [
            0, 1, -1, 0.001, -0.001, 0.0004, 0.0005, 0.0006, -0.0005, -0.0006, 1.5, 1.0005, 1.0015, 2.0005, 1760000000.123, 1760000000.1235, 1760000000.1225,
            1760000000.9995, 951782400, 951782399.9996, 1709164800, 1709251199.9999, 4107542400, -62135596800, -62135596800.5, -62167219200, -62167219200.001,
            253402300799, 253402300799.9996, 253402300800, 253402300800.4, -86400, -86399.9995, 1e9, 2147483647, 2147483648, -2147483648, -2147483649,
            946684800, 978307200, 1234567890.987654, 1700000000.0004999, 1700000000.0005001, 0.1, 0.2, 0.3, 0.7, 1e-9, -1e-9, 86399.9995, 86400,
        ]
        for (i, seconds) in handSeconds.enumerated() { cases.append(stringCase("string-hand-\(i + 1)", seconds)) }
        var rng = LCG(seed: 0x5749_5245_0001)
        for i in 1...150 {
            let whole = Double(rng.int(-3_000_000_000...12_000_000_000))
            let millis = Double(rng.below(1000)) / 1000
            let sub = rng.pick([0, 0, 0.0004, 0.0005, 0.0006, 0.00049, 0.00051, 0.0009])
            let sign = rng.chance(15) ? -1.0 : 1.0
            cases.append(stringCase("string-generated-\(String(format: "%03d", i))", whole + sign * (millis + sub)))
        }
        // Reading.
        let handTexts = [
            "2025-10-09T08:53:20.123Z", "2025-10-09T08:53:20Z", "2025-10-09T08:53:20.1Z", "2025-10-09T08:53:20.12Z", "2025-10-09T08:53:20.1234Z",
            "2025-10-09T08:53:20.123456789Z", "2025-10-09T08:53:20.1234567890Z", "2025-10-09T08:53:20.Z", "2025-10-09T08:53:20.000Z",
            "1970-01-01T00:00:00Z", "1970-01-01T00:00:00.000Z", "1969-12-31T23:59:59.999Z", "2000-02-29T12:00:00Z", "1900-02-29T00:00:00Z", "2100-02-29T00:00:00Z",
            "2024-02-29T23:59:59.999Z", "2025-02-29T00:00:00Z", "2025-02-30T00:00:00Z", "2025-04-31T00:00:00Z", "2025-13-01T00:00:00Z", "2025-00-10T00:00:00Z",
            "2025-01-00T00:00:00Z", "2025-01-32T00:00:00Z", "2025-01-31T24:00:00Z", "2025-01-31T23:60:00Z", "2025-01-31T23:59:60Z", "2025-01-31T23:59:61Z",
            "0001-01-01T00:00:00Z", "0000-01-01T00:00:00Z", "9999-12-31T23:59:59.999Z", "2025-10-09t08:53:20Z", "2025-10-09T08:53:20z", "2025-10-09 08:53:20Z",
            "2025-10-09T08:53:20+00:00", "2025-10-09T08:53:20-05:00", "2025-10-09T08:53:20", "2025-10-09T08:53Z", "2025-10-09", "", "Z", " 2025-10-09T08:53:20Z",
            "2025-10-09T08:53:20Z ", "2025-10-09T08:53:20Z\n", "2025/10/09T08:53:20Z", "2025-10-09T08.53.20Z", "25-10-09T08:53:20Z", "2025-1-9T08:53:20Z",
            "2025-10-09T08:53:20,123Z", "2025-10-09T08:53:20.12a3Z", "2025-10-09T08:53:2\u{0}Z", "\u{665}025-10-09T08:53:20Z", "2025-10-09T08:53:20.\u{661}Z",
            "2025-10-09T08:53:20.5Z", "2025-10-09T08:53:20.9999Z", "2025-10-09T08:53:20.0005Z", "2025-10-09T08:53:20.0004Z", "2025-12-31T23:59:60.5Z",
            "+2025-10-09T08:53:20Z", "-001-10-09T08:53:20Z", "2025-10-09T08:53:20.Z0", "2025-10-09T08:53:20ZZ", "20251009T085320Z",
        ]
        for (i, text) in handTexts.enumerated() { cases.append(dateCase("date-hand-\(i + 1)", text)) }
        // Generated: texts the wire writes (round trip), the same without a fraction or with a longer one, and corruptions.
        for i in 1...150 {
            let seconds = Double(rng.int(-2_000_000_000...6_000_000_000)) + Double(rng.below(1000)) / 1000
            var text = WireDates.string(Date(timeIntervalSince1970: seconds))
            switch rng.below(10) {
            case 0, 1: text = String(text.prefix(19)) + "Z"
            case 2: text = String(text.prefix(23)) + String(rng.below(1000)) + "Z"
            case 3: text = String(text.prefix(23)) + String(rng.int(100000...999999)) + "Z"
            case 4:
                var chars = Array(text)
                let at = rng.below(chars.count)
                chars[at] = rng.pick(["x", "0", "9", "-", ":", "T", "Z", " ", "."])
                text = String(chars)
            case 5: text = String(text.dropLast())
            case 6: text = text.replacingOccurrences(of: "Z", with: "+00:00")
            case 7:
                let day = rng.int(28...34)
                let month = rng.int(1...13)
                text = "2025-\(String(format: "%02d", month))-\(String(format: "%02d", day))" + text.dropFirst(10)
            default: break
            }
            cases.append(dateCase("date-generated-\(String(format: "%03d", i))", text))
        }
        return cases
    }
}
