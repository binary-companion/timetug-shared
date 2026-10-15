import Foundation
import TimeTugCore

public enum TextGeneratorAvailability: Equatable, Sendable {
    case available(EngineInfo)
    /// Permanent for now: not supported, not enabled, model not ready.
    case unavailable(reason: String)
    /// For example energy saver; pairs stay pending.
    case temporarilyUnavailable(reason: String)
}

/// A platform's text model. Hosts implement it; the engine owns the prompt and the answer parsing.
public protocol TextGenerator: Sendable {
    var availability: TextGeneratorAvailability { get async }
    /// Returns the model's one-word answer for `prompt` given `instructions`. Throws on failure or cancellation.
    func generate(instructions: String, prompt: String) async throws -> String
}
