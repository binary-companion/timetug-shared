import Foundation
import TimeTugCore

/// Persists lessons and model verdicts as JSON so corrections survive a relaunch.
struct DedupStateStore {
    static let fileName = "dedup-state.json"
    private let file: JSONFile<DedupState>

    init(directory: URL, diagnostics: @escaping EngineDiagnosticSink) {
        file = JSONFile(
            url: directory.appendingPathComponent(Self.fileName), diagnostics: diagnostics, label: "dedup")
    }

    /// Empty state for a missing or unreadable file (corruption is reported, never fatal).
    func load() -> DedupState { file.load() ?? DedupState() }

    /// False when the file could not be written.
    @discardableResult
    func save(_ state: DedupState) -> Bool { file.save(state) }
}
