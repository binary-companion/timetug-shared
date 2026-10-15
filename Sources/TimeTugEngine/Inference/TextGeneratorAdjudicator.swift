import Foundation
import TimeTugCore

/// Judges ambiguous duplicate pairs with a host-supplied `TextGenerator`.
public struct TextGeneratorAdjudicator: DuplicateAdjudicator {
    public static let deadline: TimeInterval = 30
    public static let maxRequests = 20
    public static let temporaryPrefix = "temporary: "

    private let generator: any TextGenerator
    private let clock: any EngineClock
    private let timeZone: @Sendable () -> TimeZone
    private let diagnostics: EngineDiagnosticSink
    private let cached = LockedAvailability()

    public init(generator: any TextGenerator, clock: any EngineClock,
                timeZone: @escaping @Sendable () -> TimeZone,
                diagnostics: @escaping EngineDiagnosticSink = { _ in }) {
        self.generator = generator
        self.clock = clock
        self.timeZone = timeZone
        self.diagnostics = diagnostics
    }

    /// The last value `refreshAvailability()` saw; `.unavailable` until the first refresh.
    public var availability: AdjudicatorAvailability { cached.get() }

    /// Asks the generator and caches the answer for the synchronous `availability`.
    @discardableResult
    public func refreshAvailability() async -> AdjudicatorAvailability {
        let mapped: AdjudicatorAvailability
        switch await generator.availability {
        case .available(let info): mapped = .available(info)
        case .unavailable(let reason): mapped = .unavailable(reason: reason)
        case .temporarilyUnavailable(let reason): mapped = .unavailable(reason: Self.temporaryPrefix + reason)
        }
        cached.set(mapped)
        return mapped
    }

    public func judge(_ requests: [AdjudicationRequest]) async -> [AdjudicationVerdict] {
        guard case .available = await refreshAvailability() else { return [] }
        var verdicts: [AdjudicationVerdict] = []
        let deadline = clock.now().addingTimeInterval(Self.deadline)
        for request in requests.prefix(Self.maxRequests) {
            if Task.isCancelled || clock.now() >= deadline { break }
            do {
                let raw = try await generator.generate(
                    instructions: PromptBuilder.instructions,
                    prompt: PromptBuilder.prompt(for: request, timeZone: timeZone()))
                let word = raw.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
                guard let answer = AdjudicationVerdict.Answer(rawValue: word) else {
                    diagnostics(EngineDiagnostic(level: .error, category: "inference",
                                                 message: "Unexpected answer from the text model"))
                    continue
                }
                verdicts.append(AdjudicationVerdict(requestID: request.id, answer: answer))
            } catch {
                if error is CancellationError { break }
                // No verdict is cached, so the pair is retried later. Only the error type is logged:
                // model errors can embed event content.
                diagnostics(EngineDiagnostic(
                    level: .error, category: "inference",
                    message: "Judgment failed: \(String(describing: type(of: error)))"))
            }
        }
        return verdicts
    }
}

private final class LockedAvailability: @unchecked Sendable {
    private let lock = NSLock()
    private var value: AdjudicatorAvailability = .unavailable(reason: "not checked yet")
    func get() -> AdjudicatorAvailability { lock.lock(); defer { lock.unlock() }; return value }
    func set(_ new: AdjudicatorAvailability) { lock.lock(); defer { lock.unlock() }; value = new }
}
