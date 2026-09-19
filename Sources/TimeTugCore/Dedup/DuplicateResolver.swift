import Foundation

public struct DuplicateResolution: Sendable {
    /// Merged events, sorted by start, then title, then id.
    public var events: [CalendarEvent]
    /// Ambiguous pairs still waiting for a model verdict.
    public var pending: [AdjudicationRequest]
    /// Event id -> look-alike events kept separate (for a manual "Merge").
    public var candidates: [String: [CalendarEvent]]
    /// Pair keys of the lessons that decided a pair, so the store can refresh their `lastUsed`.
    public var usedLessonKeys: Set<String>
}

/// Turns raw events from every source into merged events. Pure and synchronous; a model is only
/// consulted through verdicts already in `verdicts` (nil means inference is off).
public enum DuplicateResolver {
    public static let maxGroupSize = 4

    private struct Pair: Hashable {
        let low: Int, high: Int
        init(_ a: Int, _ b: Int) { low = min(a, b); high = max(a, b) }
    }

    private struct MergeLink { let i: Int, j: Int, why: MergeProvenance }

    public static func resolve(
        events: [CalendarEvent], calendars: [CalendarInfo], lessons: LessonBook, verdicts: VerdictCache?
    ) -> DuplicateResolution {
        let infoByKey = Dictionary(calendars.map { ($0.key, $0) }, uniquingKeysWith: { first, _ in first })
        var merges: [MergeLink] = []
        var blocked = Set<Pair>()
        var pendingPairs: [(i: Int, j: Int, request: AdjudicationRequest)] = []
        var usedLessonKeys = Set<String>()

        for i in events.indices {
            for j in events.indices where j > i {
                let a = events[i], b = events[j]
                // Exact duplicates (even all-day or on one calendar) merge by rule, so a lesson must be able to undo that.
                let lessonApplies = DuplicateRules.isCandidate(a, b) || a.contentKey == b.contentKey
                if lessonApplies, let lesson = lessons.decision(a, b) {
                    usedLessonKeys.insert(lesson.pairKey)
                    if lesson.decision == .same { merges.append(MergeLink(i: i, j: j, why: .userConfirmed)) }
                    else { blocked.insert(Pair(i, j)) }
                    continue
                }
                switch DuplicateRules.decide(a, b) {
                case .merge: merges.append(MergeLink(i: i, j: j, why: .rule))
                case .separate: blocked.insert(Pair(i, j))
                case .ambiguous:
                    guard let verdicts else { continue }
                    let request = makeRequest(a, b, infoByKey: infoByKey, lessons: lessons)
                    guard let entry = verdicts.entry(for: request.id) else {
                        pendingPairs.append((i, j, request))
                        continue
                    }
                    switch entry.answer {
                    case .same:
                        merges.append(MergeLink(i: i, j: j, why: .inference(engineID: entry.engine.id, engineName: entry.engine.displayName)))
                    case .different: blocked.insert(Pair(i, j))
                    case .unsure: break
                    }
                }
            }
        }

        // Union merge links conservatively: never past the size cap, never across a blocked pair.
        var groupOf = Array(events.indices)
        var members = Dictionary(uniqueKeysWithValues: events.indices.map { ($0, [$0]) })
        var provenance: [Int: [MergeProvenance]] = [:]
        for link in merges {
            let gi = groupOf[link.i], gj = groupOf[link.j]
            if gi == gj { provenance[gi, default: []].append(link.why); continue }
            let left = members[gi] ?? [], right = members[gj] ?? []
            guard left.count + right.count <= maxGroupSize,
                  !left.contains(where: { x in right.contains { y in blocked.contains(Pair(x, y)) } }) else { continue }
            members[gi] = left + right
            members[gj] = nil
            for k in right { groupOf[k] = gi }
            provenance[gi, default: []] += (provenance[gj] ?? []) + [link.why]
            provenance[gj] = nil
        }

        let groups = members.values.map { $0.sorted() }.sorted { $0[0] < $1[0] }
        var output: [CalendarEvent] = []
        var outputIndex = Array(repeating: 0, count: events.count)
        for group in groups {
            for k in group { outputIndex[k] = output.count }
            output.append(merged(group.map { events[$0] }, provenance: provenance[groupOf[group[0]]] ?? []))
        }

        var candidates: [String: [CalendarEvent]] = [:]
        for i in events.indices {
            for j in events.indices where j > i {
                let oi = outputIndex[i], oj = outputIndex[j]
                guard oi != oj,
                      DuplicateRules.isCandidate(events[i], events[j]) || events[i].contentKey == events[j].contentKey else { continue }
                if !(candidates[output[oi].id] ?? []).contains(where: { $0.id == output[oj].id }) {
                    candidates[output[oi].id, default: []].append(output[oj])
                }
                if !(candidates[output[oj].id] ?? []).contains(where: { $0.id == output[oi].id }) {
                    candidates[output[oj].id, default: []].append(output[oi])
                }
            }
        }

        var seen = Set<String>()
        let pending = pendingPairs
            .filter { outputIndex[$0.i] != outputIndex[$0.j] && seen.insert($0.request.id).inserted }
            .map(\.request)

        return DuplicateResolution(
            events: output.sorted { ($0.start, $0.title, $0.id) < ($1.start, $1.title, $1.id) },
            pending: pending, candidates: candidates, usedLessonKeys: usedLessonKeys)
    }

    private static func merged(_ group: [CalendarEvent], provenance: [MergeProvenance]) -> CalendarEvent {
        guard group.count > 1 else { return group[0] }
        // The richest copy is the primary so the official title and details win; ties keep the earliest.
        let primaryIndex = group.indices.max { l, r in
            let sl = DuplicateRules.detailScore(group[l]), sr = DuplicateRules.detailScore(group[r])
            return sl != sr ? sl < sr : l > r
        }!
        var result = group[primaryIndex]
        for (index, other) in group.enumerated() where index != primaryIndex {
            if other.calendarKey != result.calendarKey { result.additionalCalendarKeys.insert(other.calendarKey) }
            result.location = result.location ?? other.location
            result.notes = result.notes ?? other.notes
            result.url = result.url ?? other.url
        }
        result.conferenceURL = bestConferenceLink(primary: group[primaryIndex], group: group)
        // Takeover qualification reads these two, so the merged event must not be weaker than any copy.
        result.otherAttendeeCount = group.map(\.otherAttendeeCount).max() ?? result.otherAttendeeCount
        result.responseStatus = group.map(\.responseStatus).max { attendance($0) < attendance($1) } ?? result.responseStatus
        result.mergedMembers = group.map { MergedMember($0) }
        result.mergeProvenance = strongest(provenance)
        return result
    }

    /// The best join link across the whole group: a recognised provider beats a generic link; ties keep the
    /// primary's, then group order. Sources leave `conferenceURL` nil, so links are detected per member.
    private static func bestConferenceLink(primary: CalendarEvent, group: [CalendarEvent]) -> URL? {
        let ordered = [primary] + group.filter { $0.id != primary.id }
        let links = ordered.compactMap { member in
            member.conferenceURL ?? ConferenceLinkDetector.detect(location: member.location, url: member.url, notes: member.notes)
        }
        return links.first { ConferenceLinkDetector.isProvider($0) } ?? links.first
    }

    /// accepted > tentative > pending > unknown > declined.
    private static func attendance(_ status: ResponseStatus) -> Int {
        switch status {
        case .accepted: 4
        case .tentative: 3
        case .pending: 2
        case .unknown: 1
        case .declined: 0
        }
    }

    /// User decisions outrank model verdicts, which outrank rules.
    private static func strongest(_ list: [MergeProvenance]) -> MergeProvenance {
        if list.contains(.userConfirmed) { return .userConfirmed }
        return list.first { if case .inference = $0 { return true } else { return false } } ?? .rule
    }

    private static func makeRequest(
        _ a: CalendarEvent, _ b: CalendarEvent, infoByKey: [String: CalendarInfo], lessons: LessonBook
    ) -> AdjudicationRequest {
        let (first, second) = DuplicateRules.detailScore(a) >= DuplicateRules.detailScore(b) ? (a, b) : (b, a)
        return AdjudicationRequest(
            id: fingerprint(a, b),
            first: AdjudicationEvent(first, calendar: infoByKey[first.calendarKey]),
            second: AdjudicationEvent(second, calendar: infoByKey[second.calendarKey]),
            lessons: lessons.relevant(to: a, b))
    }

    /// Order-independent digest of everything the judgment depends on; an edited event gets a new one.
    private static func fingerprint(_ a: CalendarEvent, _ b: CalendarEvent) -> String {
        func part(_ e: CalendarEvent) -> String {
            [DuplicateRules.normalize(e.title), String(Int(e.start.timeIntervalSince1970)), String(Int(e.end.timeIntervalSince1970)),
             e.calendarKey, DuplicateRules.normalizedLocation(e.location) ?? "",
             DuplicateRules.normalize(String((e.notes ?? "").prefix(AdjudicationEvent.maxNotesLength))),
             DuplicateRules.emails(e).sorted().joined(separator: ","), String(e.otherAttendeeCount)].joined(separator: "|")
        }
        return Fingerprint.fnv1a([part(a), part(b)].sorted().joined(separator: "##"))
    }
}
