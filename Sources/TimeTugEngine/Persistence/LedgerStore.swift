import Foundation
import TimeTugCore

/// Persists the takeover ledger as JSON so a relaunch never repeats a takeover. The file format is
/// the one the Mac app writes.
struct LedgerStore {
    static let fileName = "takeover-ledger.json"
    private let file: JSONFile<TakeoverLedger>

    init(directory: URL, diagnostics: @escaping EngineDiagnosticSink) {
        file = JSONFile(
            url: directory.appendingPathComponent(Self.fileName), diagnostics: diagnostics, label: "ledger")
    }

    /// An empty ledger for a missing or unreadable file (corruption is reported, never fatal).
    func load() -> TakeoverLedger { file.load() ?? TakeoverLedger() }

    /// Prunes expired entries before encoding so the file never holds them. False when the file
    /// could not be written.
    @discardableResult
    func save(_ ledger: TakeoverLedger, now: Date) -> Bool {
        var pruned = ledger
        pruned.prune(now: now)
        return file.save(pruned)
    }
}
