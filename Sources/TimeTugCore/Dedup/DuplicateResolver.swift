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
    /// How many clusters of certain duplicates model verdicts may join into one group. Rule and user
    /// merges are never capped: any number of exact duplicates collapse into one card.
    public static let maxGroupSize = 4

    private struct Pair: Hashable {
        let low: Int, high: Int
        init(_ a: Int, _ b: Int) { low = min(a, b); high = max(a, b) }
    }

    private struct MergeLink { let i: Int, j: Int, why: MergeProvenance }

    /// Groups of event indexes. A group is one or more "clusters": phase 1 clusters are counted once each
    /// when phase 2 checks the cap.
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
        events: [CalendarEvent], calendars: [CalendarInfo], lessons: LessonBook, verdicts: VerdictCache?
    ) -> DuplicateResolution {
        let infoByKey = Dictionary(calendars.map { ($0.key, $0) }, uniquingKeysWith: { first, _ in first })
        var certain: [MergeLink] = []      // rule and user-confirmed merges
        var inference: [MergeLink] = []    // model "same" verdicts
        var blocked = Set<Pair>()          // hard: rules say separate, or a learned "different"
        var softDifferent = Set<Pair>()    // model "different" verdicts: votes only
        var pendingPairs: [(i: Int, j: Int, request: AdjudicationRequest)] = []
        var usedLessonKeys = Set<String>()

        for i in events.indices {
            for j in events.indices where j > i {
                let a = events[i], b = events[j]
                // Exact duplicates (even all-day or on one calendar) merge by rule, so a lesson must be able to undo that.
                let lessonApplies = DuplicateRules.isCandidate(a, b) || a.contentKey == b.contentKey
                if lessonApplies, let lesson = lessons.decision(a, b) {
                    usedLessonKeys.insert(lesson.pairKey)
                    if lesson.decision == .same { certain.append(MergeLink(i: i, j: j, why: .userConfirmed)) }
                    else { blocked.insert(Pair(i, j)) }
                    continue
                }
                switch DuplicateRules.decide(a, b) {
                case .merge: certain.append(MergeLink(i: i, j: j, why: .rule))
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
                        inference.append(MergeLink(i: i, j: j, why: .inference(engineID: entry.engine.id, engineName: entry.engine.displayName)))
                    case .different: softDifferent.insert(Pair(i, j))
                    case .unsure: break
                    }
                }
            }
        }

        var grouping = Grouping(count: events.count)

        // Phase 1: certain links (rules, user lessons), uncapped, never across a hard-blocked pair.
        for link in certain {
            let gi = grouping.groupOf[link.i], gj = grouping.groupOf[link.j]
            if gi == gj { grouping.provenance[gi, default: []].append(link.why); continue }
            guard !grouping.hasBlockedPair(gi, gj, in: blocked) else { continue }
            grouping.union(gi, gj, adding: [link.why])
        }

        // Each phase 1 group is now one cluster, however many exact duplicates it holds.
        for g in grouping.members.keys { grouping.clusters[g] = 1 }

        // Phase 2: model verdicts are votes between groups. Merge when "same" outnumbers "different",
        // nothing hard-blocks the pair and the cap on clusters holds. Restart after each merge because a
        // merged group carries all its members' votes.
        while let (g, h, links) = nextModelMerge(grouping, inference: inference, softDifferent: softDifferent, blocked: blocked) {
            grouping.union(g, h, adding: links.map(\.why))
        }

        let groups = grouping.orderedGroups.map { grouping.members[$0]! }
        var output: [CalendarEvent] = []
        var outputIndex = Array(repeating: 0, count: events.count)
        for group in groups {
            for k in group { outputIndex[k] = output.count }
            output.append(merged(group.map { events[$0] }, provenance: grouping.provenance[grouping.groupOf[group[0]]] ?? []))
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

    /// The next pair of groups the model votes say to merge, with the inference links between them.
    /// Pairs are tried in order of the groups' smallest member indexes.
    private static func nextModelMerge(
        _ grouping: Grouping, inference: [MergeLink], softDifferent: Set<Pair>, blocked: Set<Pair>
    ) -> (Int, Int, [MergeLink])? {
        var sameLinks: [Pair: [MergeLink]] = [:]   // keyed by (low group, high group) ids
        for link in inference {
            let gi = grouping.groupOf[link.i], gj = grouping.groupOf[link.j]
            if gi != gj { sameLinks[Pair(gi, gj), default: []].append(link) }
        }
        let rank = Dictionary(uniqueKeysWithValues: grouping.orderedGroups.enumerated().map { ($1, $0) })
        let ordered = sameLinks.keys.sorted {
            let l = (min(rank[$0.low]!, rank[$0.high]!), max(rank[$0.low]!, rank[$0.high]!))
            let r = (min(rank[$1.low]!, rank[$1.high]!), max(rank[$1.low]!, rank[$1.high]!))
            return l < r
        }
        for pair in ordered {
            let (g, h) = rank[pair.low]! < rank[pair.high]! ? (pair.low, pair.high) : (pair.high, pair.low)
            let differentVotes = softDifferent.filter {
                let a = grouping.groupOf[$0.low], b = grouping.groupOf[$0.high]
                return (a == g && b == h) || (a == h && b == g)
            }.count
            guard let links = sameLinks[pair], links.count > differentVotes,
                  (grouping.clusters[g] ?? 1) + (grouping.clusters[h] ?? 1) <= maxGroupSize,
                  !grouping.hasBlockedPair(g, h, in: blocked) else { continue }
            return (g, h, links)
        }
        return nil
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
