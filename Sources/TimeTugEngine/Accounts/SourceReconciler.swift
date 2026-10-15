import CalendarCore
import Foundation
import TimeTugCore

/// Decides which sources exist (the system calendar if enabled, then one per stored account), keeps each running
/// source's change listener alive, and replaces sources when asked. Sources are identified by their own `id`.
actor SourceReconciler: SourceReconciling {
    private struct Entry {
        let source: any TimeTugCore.CalendarSource
        let listener: Task<Void, Never>
    }

    private static let systemKey = "\u{0}system"
    private let buildAccount: @Sendable (Connection) throws -> any TimeTugCore.CalendarSource
    private let buildSystemSource: @Sendable () -> any TimeTugCore.CalendarSource
    private let onChange: @Sendable (SourceChange) async -> Void
    private var entries: [String: Entry] = [:]   // keyed by connection id, or `systemKey`

    init(
        buildAccount: @escaping @Sendable (Connection) throws -> any TimeTugCore.CalendarSource,
        buildSystemSource: @escaping @Sendable () -> any TimeTugCore.CalendarSource,
        onChange: @escaping @Sendable (SourceChange) async -> Void
    ) {
        self.buildAccount = buildAccount
        self.buildSystemSource = buildSystemSource
        self.onChange = onChange
    }

    func sourceID(forConnection id: ConnectionID) -> String? { entries[id]?.source.id }

    func reconcile(connections: [Connection], systemCalendarEnabled: Bool) -> SourceReconcilerUpdate {
        apply(connections: connections, systemCalendarEnabled: systemCalendarEnabled, rebuilding: nil)
    }

    /// After a successful re-sign-in: builds a fresh source for `connection` and restarts its listener (a source's
    /// change stream ends after `.sourceFailed`, so an unchanged id would otherwise never poll again).
    func rebuild(_ connection: Connection, connections: [Connection], systemCalendarEnabled: Bool) -> SourceReconcilerUpdate {
        apply(connections: connections, systemCalendarEnabled: systemCalendarEnabled, rebuilding: connection.connectionID)
    }

    func stop() {
        entries.values.forEach { $0.listener.cancel() }
        entries = [:]
    }

    private func apply(connections: [Connection], systemCalendarEnabled: Bool, rebuilding: ConnectionID?) -> SourceReconcilerUpdate {
        var wanted: [String] = []
        var failures: [ConnectionID: String] = [:]
        if systemCalendarEnabled {
            wanted.append(Self.systemKey)
            if entries[Self.systemKey] == nil { entries[Self.systemKey] = start(buildSystemSource()) }
        }
        for connection in connections {
            let key = connection.connectionID
            if key == rebuilding {
                // Build the replacement first: a failed build must not take down the source that still works.
                do {
                    let replacement = start(try buildAccount(connection))
                    entries[key]?.listener.cancel()
                    entries[key] = replacement
                } catch {
                    failures[key] = String(describing: error)
                }
            } else if entries[key] == nil {
                do { entries[key] = start(try buildAccount(connection)) }
                catch { failures[key] = String(describing: error); continue }
            }
            if entries[key] == nil { continue }
            wanted.append(key)
        }
        for key in entries.keys where !wanted.contains(key) {
            entries[key]?.listener.cancel()
            entries[key] = nil
        }
        return SourceReconcilerUpdate(sources: wanted.compactMap { entries[$0]?.source }, failures: failures)
    }

    private func start(_ source: any TimeTugCore.CalendarSource) -> Entry {
        let onChange = onChange
        let listener = Task {
            for await change in source.changes() {
                // A removed source's listener may still see a buffered change before cancellation lands.
                if Task.isCancelled { break }
                await onChange(change)
            }
        }
        return Entry(source: source, listener: listener)
    }
}
