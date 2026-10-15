import Foundation

/// One Codable value persisted as a JSON file. Corruption and write failures are reported through
/// the diagnostic sink (with the error's type name only, never its description) and never thrown.
struct JSONFile<Value: Codable> {
    let url: URL
    let diagnostics: EngineDiagnosticSink
    /// Used as the diagnostic category and in messages, for example "ledger".
    let label: String

    init(url: URL, diagnostics: @escaping EngineDiagnosticSink, label: String) {
        self.url = url
        self.diagnostics = diagnostics
        self.label = label
    }

    /// Nil for a missing, unreadable or corrupt file.
    func load() -> Value? {
        guard FileManager.default.fileExists(atPath: url.path) else { return nil }
        do {
            return try JSONDecoder().decode(Value.self, from: Data(contentsOf: url))
        } catch {
            diagnostics(EngineDiagnostic(
                level: .warning, category: label,
                message: "\(label) could not be read (\(String(describing: type(of: error))))"))
            return nil
        }
    }

    /// Creates the directory and writes atomically. False (and an `.error` diagnostic) on failure.
    @discardableResult
    func save(_ value: Value) -> Bool {
        do {
            try FileManager.default.createDirectory(
                at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
            try JSONEncoder().encode(value).write(to: url, options: .atomic)
            return true
        } catch {
            diagnostics(EngineDiagnostic(
                level: .error, category: label,
                message: "\(label) could not be written (\(String(describing: type(of: error))))"))
            return false
        }
    }
}
