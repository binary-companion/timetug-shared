import Foundation
import TimeTugCore

/// Settings kept in `settings.json` inside the data directory, for hosts with no preference store of their own
/// (the C interface). Writes are atomic (through `JSONFile`); a missing file loads defaults and an unreadable one
/// loads defaults and reports a diagnostic. Calls are serialized, so one instance is safe from any thread.
public final class FileSettingsStorage: SettingsStorage, @unchecked Sendable {
    public static let fileName = "settings.json"

    /// The persisted shape. `EngineSettings` itself is not Codable (it is not a wire or file format), and every
    /// field decodes with a default so a file written by an older build survives a newly added field.
    private struct Stored: Codable {
        var takeover: TakeoverSettings
        var inferenceEnabled: Bool
        var systemCalendarEnabled: Bool

        init(_ settings: EngineSettings) {
            takeover = settings.takeover
            inferenceEnabled = settings.inferenceEnabled
            systemCalendarEnabled = settings.systemCalendarEnabled
        }

        init(from decoder: Decoder) throws {
            let c = try decoder.container(keyedBy: CodingKeys.self)
            let d = EngineSettings()
            takeover = try c.decodeIfPresent(TakeoverSettings.self, forKey: .takeover) ?? d.takeover
            inferenceEnabled = try c.decodeIfPresent(Bool.self, forKey: .inferenceEnabled) ?? d.inferenceEnabled
            systemCalendarEnabled =
                try c.decodeIfPresent(Bool.self, forKey: .systemCalendarEnabled) ?? d.systemCalendarEnabled
        }
    }

    private let file: JSONFile<Stored>
    private let lock = NSLock()

    public init(directory: URL, diagnostics: @escaping EngineDiagnosticSink = { _ in }) {
        file = JSONFile(
            url: directory.appendingPathComponent(Self.fileName), diagnostics: diagnostics, label: "settings")
    }

    public func load() -> EngineSettings {
        lock.withLock {
            guard let stored = file.load() else { return EngineSettings() }
            return EngineSettings(
                takeover: stored.takeover, inferenceEnabled: stored.inferenceEnabled,
                systemCalendarEnabled: stored.systemCalendarEnabled)
        }
    }

    public func save(_ settings: EngineSettings) {
        lock.withLock { _ = file.save(Stored(settings)) }
    }
}
