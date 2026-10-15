import Foundation
import TimeTugCore

/// Collects source notifications while one refresh is running. A burst causes one follow-up pass.
actor SourceRefreshCoordinator {
    private let refresh: @Sendable (Set<String>?) async -> Void
    private var pending: Set<String> = []
    private var pendingFull = false
    private var worker: Task<Void, Never>?

    init(refresh: @escaping @Sendable (Set<String>?) async -> Void) {
        self.refresh = refresh
    }

    func signal(_ change: SourceChange) {
        if case .eventsChanged(_, nil) = change {
            pendingFull = true
        } else {
            pending.insert(change.sourceID)
        }
        guard worker == nil else { return }
        worker = Task {
            while pendingFull || !pending.isEmpty {
                let batch: Set<String>? = pendingFull ? nil : pending
                pending.removeAll()
                pendingFull = false
                await refresh(batch)
            }
            worker = nil
        }
    }
}
