import Foundation

/// A JSON object as `JSONSerialization` takes it.
typealias JSON = [String: Any]

/// One vector case. `input` and `expected` are JSON objects; the shape of each is documented at the top of the
/// exporter file of its area.
struct VectorCase {
    var name: String
    var input: JSON
    var expected: JSON
}

enum VectorFile {
    /// The release whose Swift code produced the vectors (the baseline tag of the Rust program).
    static let baseline = "1.8.1"

    struct Failure: Error, CustomStringConvertible {
        var description: String
    }

    /// Writes `<directory>/<area>.json` in the shared envelope of conventions section 9:
    /// `{"schema":1,"area":...,"baseline":...,"cases":[{"name","input","expected"}]}`, pretty printed with sorted keys.
    static func write(area: String, cases: [VectorCase], minimum: Int, to directory: URL) throws {
        guard cases.count >= minimum else { throw Failure(description: "\(area): \(cases.count) cases, need \(minimum)") }
        var seen = Set<String>()
        for item in cases where !seen.insert(item.name).inserted {
            throw Failure(description: "\(area): duplicate case name \(item.name)")
        }
        let document: JSON = [
            "schema": 1, "area": area, "baseline": baseline,
            "cases": cases.map { ["name": $0.name, "input": $0.input, "expected": $0.expected] as JSON },
        ]
        let data = try JSONSerialization.data(withJSONObject: document, options: [.prettyPrinted, .sortedKeys])
        // Guard against a value `JSONSerialization` cannot represent faithfully. Foundation writes a Double with 17
        // significant digits, which any correct parser (serde_json, Python) reads back to the same Double, but Foundation's own
        // reader is a few ulps off, so numbers are compared with a relative tolerance and everything else exactly.
        let reread = try JSONSerialization.jsonObject(with: data)
        guard sameJSON(reread, document) else {
            throw Failure(description: "\(area): the written JSON does not read back to the same document")
        }
        let url = directory.appendingPathComponent(area + ".json")
        try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        try (data + Data("\n".utf8)).write(to: url, options: .atomic)
        FileHandle.standardError.write(Data("\(area): \(cases.count) cases\n".utf8))
    }
}

/// Instants in vectors are integer Unix milliseconds.
func ms(_ date: Date) -> Int64 { Int64((date.timeIntervalSince1970 * 1000).rounded()) }
func date(ms: Int64) -> Date { Date(timeIntervalSince1970: Double(ms) / 1000) }

/// `2026-09-18T10:00:00Z` as Unix milliseconds. Only used for constants written in the exporter.
func iso(_ text: String) -> Int64 {
    let formatter = ISO8601DateFormatter()
    guard let value = formatter.date(from: text) else { fatalError("bad date \(text)") }
    return ms(value)
}

let oneMinute: Int64 = 60_000
let oneHour: Int64 = 3_600_000
let oneDay: Int64 = 86_400_000

/// Structural equality of two JSON values; numbers within a relative 1e-12.
func sameJSON(_ a: Any, _ b: Any) -> Bool {
    switch (a, b) {
    case (let x as [String: Any], let y as [String: Any]):
        return x.count == y.count && x.allSatisfy { key, value in y[key].map { sameJSON(value, $0) } ?? false }
    case (let x as [Any], let y as [Any]):
        return x.count == y.count && zip(x, y).allSatisfy { sameJSON($0, $1) }
    case (let x as String, let y as String):
        return x == y
    case (is NSNull, is NSNull):
        return true
    case (let x as NSNumber, let y as NSNumber):
        let (dx, dy) = (x.doubleValue, y.doubleValue)
        if dx == dy { return true }
        // Whole numbers (instants, counts) must match exactly; only fractions get the tolerance.
        if dx.rounded() == dx, dy.rounded() == dy, abs(dx) < 9e15, abs(dy) < 9e15 { return false }
        return abs(dx - dy) <= 1e-12 * max(abs(dx), abs(dy))
    default:
        return false
    }
}
