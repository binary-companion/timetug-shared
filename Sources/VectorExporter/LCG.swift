/// The fixed-seed generator every generated vector area uses (conventions section 9). Never use
/// `SystemRandomNumberGenerator` in an exporter. Each area seeds it with a constant written in that area's file.
struct LCG {
    var state: UInt64

    init(seed: UInt64) { state = seed }

    mutating func next() -> UInt64 {
        state = state &* 6364136223846793005 &+ 1442695040888963407
        return state >> 33
    }
}

extension LCG {
    /// A value in `0..<n`.
    mutating func below(_ n: Int) -> Int { Int(next() % UInt64(n)) }

    /// A value in `range`, both ends included.
    mutating func int(_ range: ClosedRange<Int>) -> Int { range.lowerBound + below(range.count) }

    /// True with probability `percent` in 100.
    mutating func chance(_ percent: Int) -> Bool { below(100) < percent }

    mutating func pick<T>(_ items: [T]) -> T { items[below(items.count)] }
}
