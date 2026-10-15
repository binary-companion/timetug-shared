import Foundation
import TimeTugCore

/// Keeps each source pending until its first successful read after launch or addition.
struct LaunchAcknowledgement {
    private let launchedAt: Date
    private var registeredInitialSources = false
    private var pending: [String: Date] = [:]
    private var known = Set<String>()

    init(launchedAt: Date) { self.launchedAt = launchedAt }

    mutating func registerSources(_ ids: Set<String>, now: Date) {
        let cutoff = registeredInitialSources ? now : launchedAt
        registeredInitialSources = true
        for id in ids.subtracting(known) { pending[id] = cutoff }
        pending = pending.filter { ids.contains($0.key) }
        known = ids
    }

    /// Returns already-underway meetings on newly healthy sources, then marks those sources initialized.
    mutating func eventsToAcknowledge(
        in snapshot: CalendarSnapshot, now: Date, grace: TimeInterval
    ) -> [TimeTugCalendarEvent] {
        let ready = pending.filter { snapshot.statuses[$0.key] == .ok }
        for id in ready.keys { pending[id] = nil }
        guard !ready.isEmpty else { return [] }
        return snapshot.events.filter { event in
            guard event.end > now else { return false }
            return event.participants.contains { member in
                ready.contains { sourceID, cutoff in
                    member.calendarKey.hasPrefix(sourceID + "/") && member.start < cutoff.addingTimeInterval(-grace)
                }
            }
        }
    }
}
