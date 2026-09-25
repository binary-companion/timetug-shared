import CalendarCore
import Foundation

public struct DuplicateResolution: Sendable {
    /// Merged events, sorted by start, then title, then id.
    public var events: [TimeTugCalendarEvent]
    /// Ambiguous group pairs still waiting for a model verdict (one request per pair of groups).
    public var pending: [AdjudicationRequest]
    /// Event id -> look-alike events kept separate (for a manual "Merge").
    public var candidates: [String: [TimeTugCalendarEvent]]
    /// Pair keys of the lessons that decided a pair, so the store can refresh their `lastUsed`.
    public var usedLessonKeys: Set<String>
}

/// Turns raw events from every source into merged events. Pure and synchronous; a model is only
/// consulted through verdicts already in `verdicts` (nil means inference is off).
public enum DuplicateResolver {
    /// How many clusters of certain duplicates model verdicts may join into one group. Rule and user
    /// merges are never capped: any number of exact duplicates collapse into one card.
    public static let maxGroupSize = 4

    private struct Pair: Hashable {
        let low: Int, high: Int
        init(_ a: Int, _ b: Int) { low = min(a, b); high = max(a, b) }
    }

    private struct MergeLink { let i: Int, j: Int, why: MergeProvenance }

    /// Groups of event indexes. A group is one or more "clusters": phase 1 and 2 groups are counted once each
    /// when phase 3 checks the cap.
    private struct Grouping {
        var groupOf: [Int]
        var members: [Int: [Int]]
        var clusters: [Int: Int]
        var provenance: [Int: [MergeProvenance]] = [:]

        init(count: Int) {
            groupOf = Array(0..<count)
            members = Dictionary(uniqueKeysWithValues: (0..<count).map { ($0, [$0]) })
            clusters = Dictionary(uniqueKeysWithValues: (0..<count).map { ($0, 1) })
        }

        /// Group ids ordered by their smallest member index, so processing order is deterministic.
        var orderedGroups: [Int] { members.keys.sorted { members[$0]![0] < members[$1]![0] } }

        func hasBlockedPair(_ g: Int, _ h: Int, in blocked: Set<Pair>) -> Bool {
            (members[g] ?? []).contains { x in (members[h] ?? []).contains { y in blocked.contains(Pair(x, y)) } }
        }

        /// Folds `h` into `g`. Members stay sorted so the smallest index identifies the group.
        mutating func union(_ g: Int, _ h: Int, adding why: [MergeProvenance]) {
            members[g] = ((members[g] ?? []) + (members[h] ?? [])).sorted()
            for k in members[h] ?? [] { groupOf[k] = g }
            clusters[g, default: 1] += clusters[h] ?? 1
            provenance[g, default: []] += (provenance[h] ?? []) + why
            members[h] = nil
            clusters[h] = nil
            provenance[h] = nil
        }
    }

    public static func resolve(
        events: [TimeTugCalendarEvent], calendars: [CalendarInfo], lessons: LessonBook, verdicts: VerdictCache?
    ) -> DuplicateResolution {
        let infoByKey = Dictionary(calendars.map { ($0.key, $0) }, uniquingKeysWith: { first, _ in first })
        var exact: [MergeLink] = []        // phase 1: identical content
        var certain: [MergeLink] = []      // phase 2: other rules and user-confirmed merges
        var blocked = Set<Pair>()          // hard: rules say separate, or a learned "different"
        var learnedBlocked = Set<Pair>()   // the learned "different" subset: the only block a user's "same" respects
        var ambiguous = Set<Pair>()        // pairs only a model could decide (inference on)
        var usedLessonKeys = Set<String>()

        for i in events.indices {
            for j in events.indices where j > i {
                let a = events[i], b = events[j]
                // Exact duplicates (even all-day or on one calendar) merge by rule, so a lesson must be able to undo that.
                let lessonApplies = DuplicateRules.isCandidate(a, b) || a.contentKey == b.contentKey
                if lessonApplies, let lesson = lessons.decision(a, b) {
                    usedLessonKeys.insert(lesson.pairKey)
                    if lesson.decision == .same { certain.append(MergeLink(i: i, j: j, why: .userConfirmed)) }
                    else { blocked.insert(Pair(i, j)); learnedBlocked.insert(Pair(i, j)) }
                    continue
                }
                switch DuplicateRules.decide(a, b) {
                case .merge(.exactMatch): exact.append(MergeLink(i: i, j: j, why: .rule))
                case .merge: certain.append(MergeLink(i: i, j: j, why: .rule))
                case .separate: blocked.insert(Pair(i, j))
                case .ambiguous: if verdicts != nil { ambiguous.insert(Pair(i, j)) }
                }
            }
        }

        var grouping = Grouping(count: events.count)

        // Three phases, each finished before the next starts, links in index order:
        // 1. identical items (exact content match), uncapped;
        // 2. those groups by the other rules and by the user's "same" lessons, uncapped;
        // 3. the resulting groups by model verdicts (below).
        // A hard block (a rules "separate" or a learned "different") stops any merge in any phase, checked
        // across every member of both groups. A user's "same" is stronger than a rules "separate" between
        // other members (say, a copy on the same calendar as the other event); only a learned "different"
        // stops it.
        func union(_ links: [MergeLink]) {
            for link in links {
                let gi = grouping.groupOf[link.i], gj = grouping.groupOf[link.j]
                if gi == gj { grouping.provenance[gi, default: []].append(link.why); continue }
                let stoppers = link.why == .userConfirmed ? learnedBlocked : blocked
                guard !grouping.hasBlockedPair(gi, gj, in: stoppers) else { continue }
                grouping.union(gi, gj, adding: [link.why])
            }
        }
        union(exact)
        union(certain)

        // Each certain group is now one cluster, however many copies it holds.
        for g in grouping.members.keys { grouping.clusters[g] = 1 }

        // Phase 3: the model judges each pair of groups once, through its richest copies. "same" merges the
        // two groups when the cap on clusters holds; "different" and "unsure" leave them apart. Restart after
        // each merge because the merged group has a new representative and new neighbours.
        var pending: [AdjudicationRequest] = []
        if let verdicts {
            while true {
                let step = nextModelStep(grouping, events: events, ambiguous: ambiguous, blocked: blocked,
                                         verdicts: verdicts, infoByKey: infoByKey, lessons: lessons)
                if let merge = step.merge {
                    grouping.union(merge.g, merge.h, adding: [merge.why])
                } else {
                    pending = step.pending
                    break
                }
            }
        }

        let groups = grouping.orderedGroups.map { grouping.members[$0]! }
        var output: [TimeTugCalendarEvent] = []
        var outputIndex = Array(repeating: 0, count: events.count)
        for group in groups {
            for k in group { outputIndex[k] = output.count }
            output.append(merged(group.map { events[$0] }, provenance: grouping.provenance[grouping.groupOf[group[0]]] ?? []))
        }

        var candidates: [String: [TimeTugCalendarEvent]] = [:]
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

        return DuplicateResolution(
            events: output.sorted { ($0.start, $0.title, $0.id) < ($1.start, $1.title, $1.id) },
            pending: pending, candidates: candidates, usedLessonKeys: usedLessonKeys)
    }

    /// The richest copy of a group; ties keep the earliest index (members are sorted).
    private static func representative(_ members: [Int], in events: [TimeTugCalendarEvent]) -> Int {
        var best = members[0]
        for k in members.dropFirst() where DuplicateRules.detailScore(events[k]) > DuplicateRules.detailScore(events[best]) { best = k }
        return best
    }

    /// Walks the pairs of groups that hold an ambiguous cross pair, in order of the groups' smallest member
    /// indexes. Returns the first cached "same" that may merge, else every request still without a verdict.
    private static func nextModelStep(
        _ grouping: Grouping, events: [TimeTugCalendarEvent], ambiguous: Set<Pair>, blocked: Set<Pair>,
        verdicts: VerdictCache, infoByKey: [String: CalendarInfo], lessons: LessonBook
    ) -> (merge: (g: Int, h: Int, why: MergeProvenance)?, pending: [AdjudicationRequest]) {
        let rank = Dictionary(uniqueKeysWithValues: grouping.orderedGroups.enumerated().map { ($1, $0) })
        var groupPairs = Set<Pair>()
        for pair in ambiguous {
            let g = grouping.groupOf[pair.low], h = grouping.groupOf[pair.high]
            if g != h { groupPairs.insert(Pair(g, h)) }
        }
        let ordered = groupPairs.map { pair -> (g: Int, h: Int) in
            rank[pair.low]! < rank[pair.high]! ? (pair.low, pair.high) : (pair.high, pair.low)
        }.sorted { (rank[$0.g]!, rank[$0.h]!) < (rank[$1.g]!, rank[$1.h]!) }

        var pending: [AdjudicationRequest] = []
        var seen = Set<String>()
        for (g, h) in ordered {
            guard (grouping.clusters[g] ?? 1) + (grouping.clusters[h] ?? 1) <= maxGroupSize,
                  !grouping.hasBlockedPair(g, h, in: blocked) else { continue }
            let a = representative(grouping.members[g]!, in: events), b = representative(grouping.members[h]!, in: events)
            let request = makeRequest(events[a], events[b], infoByKey: infoByKey, lessons: lessons)
            guard let entry = verdicts.entry(for: request.id) else {
                if seen.insert(request.id).inserted { pending.append(request) }
                continue
            }
            if entry.answer == .same {
                return ((g, h, .inference(engineID: entry.engine.id, engineName: entry.engine.displayName)), [])
            }
        }
        return (nil, pending)
    }

    private static func merged(_ group: [TimeTugCalendarEvent], provenance: [MergeProvenance]) -> TimeTugCalendarEvent {
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
        // The card shows the longer copy's range; the takeover (`start`) fires when the copy with a join link
        // starts (the actual appointment), else when the longer copy starts. Everything that schedules or
        // counts down reads `start`/`end`, so it follows the tug time without further change.
        let primary = group[primaryIndex]
        let longer = group.enumerated().min { l, r in
            let dl = l.element.end.timeIntervalSince(l.element.start), dr = r.element.end.timeIntervalSince(r.element.start)
            if dl != dr { return dl > dr }
            if (l.offset == primaryIndex) != (r.offset == primaryIndex) { return l.offset == primaryIndex }
            return l.element.start < r.element.start
        }!.element
        let carrier = joinLink(of: primary) != nil
            ? primary
            : group.filter { joinLink(of: $0) != nil }.min { $0.start < $1.start }
        result.end = longer.end
        result.start = carrier?.start ?? longer.start
        result.displayStart = longer.start == result.start ? nil : longer.start
        // Takeover qualification reads these two, so the merged event must not be weaker than any copy.
        result.otherAttendeeCount = group.map(\.otherAttendeeCount).max() ?? result.otherAttendeeCount
        if let best = group.map(\.responseStatus).max(by: { attendance($0) < attendance($1) }) { result.responseStatus = best }
        result.mergedMembers = group.map { MergedMember($0) }
        result.mergeProvenance = strongest(provenance)
        return result
    }

    /// The best join link across the whole group: a recognised provider beats a generic link; ties keep the
    /// primary's, then group order. A member's own `conferenceURL` (the provider's link, when it has one) comes first;
    /// otherwise a link is detected in its location, url or notes.
    private static func bestConferenceLink(primary: TimeTugCalendarEvent, group: [TimeTugCalendarEvent]) -> URL? {
        let ordered = [primary] + group.filter { $0.id != primary.id }
        let links = ordered.compactMap(joinLink(of:))
        return links.first { ConferenceLinkDetector.isProvider($0) } ?? links.first
    }

    /// One member's join link: the structured `conferenceURL`, else one detected in its location, url or notes.
    private static func joinLink(of member: TimeTugCalendarEvent) -> URL? {
        member.conferenceURL ?? ConferenceLinkDetector.detect(location: member.location, url: member.url, notes: member.notes)
    }

    /// accepted > tentative > needsAction > unknown (nil) > declined.
    private static func attendance(_ status: CalendarCore.ResponseStatus?) -> Int {
        switch status {
        case .accepted: 4
        case .tentative: 3
        case .needsAction: 2
        case nil: 1
        case .declined: 0
        }
    }

    /// User decisions outrank model verdicts, which outrank rules.
    private static func strongest(_ list: [MergeProvenance]) -> MergeProvenance {
        if list.contains(.userConfirmed) { return .userConfirmed }
        return list.first { if case .inference = $0 { return true } else { return false } } ?? .rule
    }

    private static func makeRequest(
        _ a: TimeTugCalendarEvent, _ b: TimeTugCalendarEvent, infoByKey: [String: CalendarInfo], lessons: LessonBook
    ) -> AdjudicationRequest {
        let (first, second) = DuplicateRules.detailScore(a) >= DuplicateRules.detailScore(b) ? (a, b) : (b, a)
        func minutes(_ seconds: TimeInterval) -> Int { Int((seconds / 60).rounded()) }
        let overlap = min(first.end, second.end).timeIntervalSince(max(first.start, second.start))
        var conflicting = false
        if case .separate(let reason) = DuplicateRules.decide(a, b) {
            conflicting = [.conflictingLocation, .conflictingConference, .conflictingAttendees].contains(reason)
        }
        return AdjudicationRequest(
            id: fingerprint(a, b),
            first: AdjudicationEvent(first, calendar: infoByKey[first.calendarKey]),
            second: AdjudicationEvent(second, calendar: infoByKey[second.calendarKey]),
            lessons: lessons.relevant(to: a, b),
            startOffsetMinutes: minutes(second.start.timeIntervalSince(first.start)),
            endOffsetMinutes: minutes(second.end.timeIntervalSince(first.end)),
            overlapMinutes: max(0, minutes(overlap)),
            firstDetails: DuplicateRules.detailSummary(first),
            secondDetails: DuplicateRules.detailSummary(second),
            hasConflictingDetails: conflicting)
    }

    /// Order-independent digest of everything the judgment depends on; an edited event gets a new one.
    /// Calendar keys are left out on purpose: identical copies must share one verdict whichever is the representative.
    private static func fingerprint(_ a: TimeTugCalendarEvent, _ b: TimeTugCalendarEvent) -> String {
        func part(_ e: TimeTugCalendarEvent) -> String {
            [DuplicateRules.normalize(e.title), String(Int(e.start.timeIntervalSince1970)), String(Int(e.end.timeIntervalSince1970)),
             DuplicateRules.normalizedLocation(e.location) ?? "",
             DuplicateRules.normalize(String((e.notes ?? "").prefix(AdjudicationEvent.maxNotesLength))),
             DuplicateRules.emails(e).sorted().joined(separator: ","), String(e.otherAttendeeCount)].joined(separator: "|")
        }
        return Fingerprint.fnv1a([part(a), part(b)].sorted().joined(separator: "##"))
    }
}
