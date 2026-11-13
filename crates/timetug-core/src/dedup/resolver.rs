use std::collections::{BTreeMap, BTreeSet};

use calendar_core::model::{ConferenceInfo, ConferenceOrigin, ConferenceProvider, ResponseStatus};
use calendar_core::time::seconds_between;
use jiff::Timestamp;

use crate::dedup::adjudication::{
    AdjudicationEvent, AdjudicationRequest, VerdictAnswer, VerdictCache,
};
use crate::dedup::lesson_book::{LessonBook, LessonDecision};
use crate::dedup::rules::{DuplicateRules, MergeReason, PairDecision, SeparateReason};
use crate::model::calendar_info::CalendarInfo;
use crate::model::event::TimeTugCalendarEvent;
use crate::model::merge_types::{MergeProvenance, MergedMember};

/// The outcome of resolving duplicates across a set of events.
#[derive(Clone, Debug)]
pub struct DuplicateResolution {
    /// Merged events, sorted by start, then title, then id.
    pub events: Vec<TimeTugCalendarEvent>,
    /// Ambiguous group pairs still waiting for a model verdict (one request per pair of groups).
    pub pending: Vec<AdjudicationRequest>,
    /// Event id -> look-alike events kept separate (for a manual "Merge").
    pub candidates: BTreeMap<String, Vec<TimeTugCalendarEvent>>,
    /// Pair keys of the lessons that decided a pair, so the store can refresh their `lastUsed`.
    pub used_lesson_keys: BTreeSet<String>,
}

/// Turns raw events from every source into merged events. Pure and synchronous; a model is only
/// consulted through verdicts already in `verdicts` (`None` means inference is off).
pub struct DuplicateResolver;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Pair {
    low: usize,
    high: usize,
}

impl Pair {
    fn new(a: usize, b: usize) -> Pair {
        Pair {
            low: a.min(b),
            high: a.max(b),
        }
    }
}

struct MergeLink {
    i: usize,
    j: usize,
    why: MergeProvenance,
}

/// Finds only pairs that can be exact copies or manual/automatic candidates. An interval
/// tree skips non-overlapping timed events even when one long event spans many short ones.
struct RelevantPairs<'a> {
    events: &'a [TimeTugCalendarEvent],
    content_keys: Vec<String>,
    timed_by_start: Vec<usize>,
    max_end: Vec<Timestamp>,
    exact_by_key: BTreeMap<String, Vec<usize>>,
}

impl<'a> RelevantPairs<'a> {
    fn new(events: &'a [TimeTugCalendarEvent]) -> RelevantPairs<'a> {
        let keys: Vec<String> = events
            .iter()
            .map(TimeTugCalendarEvent::content_key)
            .collect();
        let mut exact: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (i, key) in keys.iter().enumerate() {
            exact.entry(key.clone()).or_default().push(i);
        }
        let mut sorted_timed: Vec<usize> = (0..events.len())
            .filter(|&i| !events[i].is_all_day())
            .collect();
        sorted_timed.sort_by(|&a, &b| {
            if events[a].start() == events[b].start() {
                a.cmp(&b)
            } else {
                events[a].start().cmp(&events[b].start())
            }
        });
        let mut ends = vec![Timestamp::MIN; (sorted_timed.len() * 4).max(1)];
        if !sorted_timed.is_empty() {
            fn build(
                node: usize,
                low: usize,
                high: usize,
                timed: &[usize],
                events: &[TimeTugCalendarEvent],
                ends: &mut [Timestamp],
            ) -> Timestamp {
                if low == high {
                    ends[node] = events[timed[low]].end();
                } else {
                    let mid = (low + high) / 2;
                    let left = build(node * 2, low, mid, timed, events, ends);
                    let right = build(node * 2 + 1, mid + 1, high, timed, events, ends);
                    ends[node] = left.max(right);
                }
                ends[node]
            }
            build(
                1,
                0,
                sorted_timed.len() - 1,
                &sorted_timed,
                events,
                &mut ends,
            );
        }
        RelevantPairs {
            events,
            content_keys: keys,
            timed_by_start: sorted_timed,
            max_end: ends,
            exact_by_key: exact,
        }
    }

    fn following(&self, i: usize) -> Vec<usize> {
        let mut found: BTreeSet<usize> = self
            .exact_by_key
            .get(&self.content_keys[i])
            .map(|list| list.iter().copied().filter(|&j| j > i).collect())
            .unwrap_or_default();
        if self.events[i].is_all_day() || self.timed_by_start.is_empty() {
            return found.into_iter().collect();
        }
        let event = &self.events[i];
        fn visit(
            pairs: &RelevantPairs<'_>,
            i: usize,
            node: usize,
            low: usize,
            high: usize,
            found: &mut BTreeSet<usize>,
        ) {
            let event = &pairs.events[i];
            if !(pairs.max_end[node] > event.start()
                && pairs.events[pairs.timed_by_start[low]].start() < event.end())
            {
                return;
            }
            if low == high {
                let j = pairs.timed_by_start[low];
                if j > i && pairs.events[j].end() > event.start() {
                    found.insert(j);
                }
            } else {
                let mid = (low + high) / 2;
                visit(pairs, i, node * 2, low, mid, found);
                visit(pairs, i, node * 2 + 1, mid + 1, high, found);
            }
        }
        let _ = event;
        visit(self, i, 1, 0, self.timed_by_start.len() - 1, &mut found);
        found.into_iter().collect()
    }
}

/// Groups of event indexes. A group is one or more "clusters": phase 1 and 2 groups are counted once each
/// when phase 3 checks the cap.
struct Grouping {
    group_of: Vec<usize>,
    members: BTreeMap<usize, Vec<usize>>,
    clusters: BTreeMap<usize, i64>,
    provenance: BTreeMap<usize, Vec<MergeProvenance>>,
}

impl Grouping {
    fn new(count: usize) -> Grouping {
        Grouping {
            group_of: (0..count).collect(),
            members: (0..count).map(|i| (i, vec![i])).collect(),
            clusters: (0..count).map(|i| (i, 1)).collect(),
            provenance: BTreeMap::new(),
        }
    }

    /// Group ids ordered by their smallest member index, so processing order is deterministic.
    fn ordered_groups(&self) -> Vec<usize> {
        let mut keys: Vec<usize> = self.members.keys().copied().collect();
        keys.sort_by_key(|key| self.members[key][0]);
        keys
    }

    fn has_blocked_pair(
        &self,
        g: usize,
        h: usize,
        is_blocked: &dyn Fn(usize, usize) -> bool,
    ) -> bool {
        let empty = Vec::new();
        let gm = self.members.get(&g).unwrap_or(&empty);
        let hm = self.members.get(&h).unwrap_or(&empty);
        gm.iter().any(|&x| hm.iter().any(|&y| is_blocked(x, y)))
    }

    /// Folds `h` into `g`. Members stay sorted so the smallest index identifies the group.
    fn union(&mut self, g: usize, h: usize, why: Vec<MergeProvenance>) {
        let mut combined = self.members.get(&g).cloned().unwrap_or_default();
        let absorbed = self.members.get(&h).cloned().unwrap_or_default();
        combined.extend(absorbed.iter().copied());
        combined.sort();
        self.members.insert(g, combined);
        for k in absorbed {
            self.group_of[k] = g;
        }
        let extra = self.clusters.get(&h).copied().unwrap_or(1);
        *self.clusters.entry(g).or_insert(1) += extra;
        let mut moved = self.provenance.remove(&h).unwrap_or_default();
        moved.extend(why);
        self.provenance.entry(g).or_default().extend(moved);
        self.members.remove(&h);
        self.clusters.remove(&h);
    }
}

impl DuplicateResolver {
    /// How many clusters of certain duplicates model verdicts may join into one group. Rule and user
    /// merges are never capped: any number of exact duplicates collapse into one card.
    pub const MAX_GROUP_SIZE: i64 = 4;

    /// `resolve(events:calendars:lessons:verdicts:)` with the default `engineID` ("unknown").
    pub fn resolve(
        events: &[TimeTugCalendarEvent],
        calendars: &[CalendarInfo],
        lessons: &LessonBook,
        verdicts: Option<&VerdictCache>,
    ) -> DuplicateResolution {
        Self::resolve_with_engine(events, calendars, lessons, verdicts, "unknown")
    }

    /// As [`DuplicateResolver::resolve`], with `engine_id` naming the inference engine; it is part of the cache key
    /// of each verdict request, so verdicts from another engine are not reused.
    pub fn resolve_with_engine(
        events: &[TimeTugCalendarEvent],
        calendars: &[CalendarInfo],
        lessons: &LessonBook,
        verdicts: Option<&VerdictCache>,
        engine_id: &str,
    ) -> DuplicateResolution {
        let mut info_by_key: BTreeMap<String, &CalendarInfo> = BTreeMap::new();
        for calendar in calendars {
            info_by_key.entry(calendar.key()).or_insert(calendar);
        }
        let mut exact: Vec<MergeLink> = Vec::new(); // phase 1: identical content
        let mut certain: Vec<MergeLink> = Vec::new(); // phase 2: other rules and user-confirmed merges
        let relevant = RelevantPairs::new(events);
        let mut ambiguous: BTreeSet<Pair> = BTreeSet::new(); // pairs only a model could decide (inference on)
        let mut used_lesson_keys: BTreeSet<String> = BTreeSet::new();

        // A skipped distant pair is still a hard separator between two groups reached through
        // other links. Evaluate that rule only when a proposed group merge needs it.
        let is_blocked = |i: usize, j: usize, learned_only: bool| -> bool {
            let (a, b) = (&events[i], &events[j]);
            let lesson_applies = DuplicateRules::is_manual_candidate(a, b)
                || relevant.content_keys[i] == relevant.content_keys[j];
            if lesson_applies && let Some(lesson) = lessons.decision(a, b) {
                return lesson.decision == LessonDecision::Different;
            }
            if learned_only {
                return false;
            }
            matches!(DuplicateRules::decide(a, b), PairDecision::Separate(_))
        };

        for i in 0..events.len() {
            for j in relevant.following(i) {
                let (a, b) = (&events[i], &events[j]);
                // Exact duplicates (even all-day or on one calendar) merge by rule, so a lesson must be able to undo that.
                let lesson_applies = DuplicateRules::is_manual_candidate(a, b)
                    || relevant.content_keys[i] == relevant.content_keys[j];
                if lesson_applies && let Some(lesson) = lessons.decision(a, b) {
                    used_lesson_keys.insert(lesson.pair_key());
                    if lesson.decision == LessonDecision::Same {
                        certain.push(MergeLink {
                            i,
                            j,
                            why: MergeProvenance::UserConfirmed,
                        });
                    }
                    continue;
                }
                match DuplicateRules::decide(a, b) {
                    PairDecision::Merge(MergeReason::ExactMatch) => exact.push(MergeLink {
                        i,
                        j,
                        why: MergeProvenance::Rule,
                    }),
                    PairDecision::Merge(_) => certain.push(MergeLink {
                        i,
                        j,
                        why: MergeProvenance::Rule,
                    }),
                    PairDecision::Separate(_) => {}
                    PairDecision::Ambiguous => {
                        if verdicts.is_some() {
                            ambiguous.insert(Pair::new(i, j));
                        }
                    }
                }
            }
        }

        let mut grouping = Grouping::new(events.len());

        // Three phases, each finished before the next starts, links in index order:
        // 1. identical items (exact content match), uncapped;
        // 2. those groups by the other rules and by the user's "same" lessons, uncapped;
        // 3. the resulting groups by model verdicts (below).
        // A hard block (a rules "separate" or a learned "different") stops any merge in any phase, checked
        // across every member of both groups. A user's "same" is stronger than a rules "separate" between
        // other members (say, a copy on the same calendar as the other event); only a learned "different"
        // stops it.
        let union = |grouping: &mut Grouping, links: &[MergeLink]| {
            for link in links {
                let gi = grouping.group_of[link.i];
                let gj = grouping.group_of[link.j];
                if gi == gj {
                    grouping
                        .provenance
                        .entry(gi)
                        .or_default()
                        .push(link.why.clone());
                    continue;
                }
                let learned_only = link.why == MergeProvenance::UserConfirmed;
                if grouping.has_blocked_pair(gi, gj, &|x, y| is_blocked(x, y, learned_only)) {
                    continue;
                }
                grouping.union(gi, gj, vec![link.why.clone()]);
            }
        };
        union(&mut grouping, &exact);
        union(&mut grouping, &certain);

        // Each certain group is now one cluster, however many copies it holds.
        let group_ids: Vec<usize> = grouping.members.keys().copied().collect();
        for g in group_ids {
            grouping.clusters.insert(g, 1);
        }

        // Phase 3: the model judges each pair of groups once, through its richest copies. "same" merges the
        // two groups when the cap on clusters holds; "different" and "unsure" leave them apart. Restart after
        // each merge because the merged group has a new representative and new neighbours.
        let mut pending: Vec<AdjudicationRequest> = Vec::new();
        if let Some(verdicts) = verdicts {
            loop {
                let (merge, step_pending) = Self::next_model_step(
                    &grouping,
                    events,
                    &ambiguous,
                    &|x, y| is_blocked(x, y, false),
                    verdicts,
                    &info_by_key,
                    lessons,
                    engine_id,
                );
                if let Some((g, h, why)) = merge {
                    grouping.union(g, h, vec![why]);
                } else {
                    pending = step_pending;
                    break;
                }
            }
        }

        let groups: Vec<Vec<usize>> = grouping
            .ordered_groups()
            .iter()
            .map(|g| grouping.members[g].clone())
            .collect();
        let mut output: Vec<TimeTugCalendarEvent> = Vec::new();
        let mut output_index = vec![0usize; events.len()];
        for group in &groups {
            for &k in group {
                output_index[k] = output.len();
            }
            let provenance = grouping
                .provenance
                .get(&grouping.group_of[group[0]])
                .cloned()
                .unwrap_or_default();
            let members: Vec<&TimeTugCalendarEvent> = group.iter().map(|&k| &events[k]).collect();
            output.push(Self::merged(&members, &provenance));
        }

        let mut candidates: BTreeMap<String, Vec<TimeTugCalendarEvent>> = BTreeMap::new();
        let mut seen_candidate_ids: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for i in 0..events.len() {
            for j in relevant.following(i) {
                let (oi, oj) = (output_index[i], output_index[j]);
                if oi == oj
                    || !(DuplicateRules::is_manual_candidate(&events[i], &events[j])
                        || relevant.content_keys[i] == relevant.content_keys[j])
                {
                    continue;
                }
                let (left_id, right_id) = (output[oi].id(), output[oj].id());
                if seen_candidate_ids
                    .entry(left_id.clone())
                    .or_default()
                    .insert(right_id.clone())
                {
                    candidates
                        .entry(left_id.clone())
                        .or_default()
                        .push(output[oj].clone());
                }
                if seen_candidate_ids
                    .entry(right_id.clone())
                    .or_default()
                    .insert(left_id)
                {
                    candidates
                        .entry(right_id)
                        .or_default()
                        .push(output[oi].clone());
                }
            }
        }

        output.sort_by(|a, b| (a.start(), a.title(), a.id()).cmp(&(b.start(), b.title(), b.id())));
        DuplicateResolution {
            events: output,
            pending,
            candidates,
            used_lesson_keys,
        }
    }

    /// The richest copy of a group; ties keep the earliest index (members are sorted).
    pub(crate) fn representative(members: &[usize], events: &[TimeTugCalendarEvent]) -> usize {
        let mut best = members[0];
        for &k in &members[1..] {
            if DuplicateRules::detail_score(&events[k])
                > DuplicateRules::detail_score(&events[best])
            {
                best = k;
            }
        }
        best
    }

    /// Walks the pairs of groups that hold an ambiguous cross pair, in order of the groups' smallest member
    /// indexes. Returns the first cached "same" that may merge, else every request still without a verdict.
    #[allow(clippy::too_many_arguments, clippy::type_complexity)]
    fn next_model_step(
        grouping: &Grouping,
        events: &[TimeTugCalendarEvent],
        ambiguous: &BTreeSet<Pair>,
        is_blocked: &dyn Fn(usize, usize) -> bool,
        verdicts: &VerdictCache,
        info_by_key: &BTreeMap<String, &CalendarInfo>,
        lessons: &LessonBook,
        engine_id: &str,
    ) -> (
        Option<(usize, usize, MergeProvenance)>,
        Vec<AdjudicationRequest>,
    ) {
        let rank: BTreeMap<usize, usize> = grouping
            .ordered_groups()
            .into_iter()
            .enumerate()
            .map(|(index, group)| (group, index))
            .collect();
        let mut group_pairs: BTreeSet<Pair> = BTreeSet::new();
        for pair in ambiguous {
            let g = grouping.group_of[pair.low];
            let h = grouping.group_of[pair.high];
            if g != h {
                group_pairs.insert(Pair::new(g, h));
            }
        }
        let mut ordered: Vec<(usize, usize)> = group_pairs
            .iter()
            .map(|pair| {
                if rank[&pair.low] < rank[&pair.high] {
                    (pair.low, pair.high)
                } else {
                    (pair.high, pair.low)
                }
            })
            .collect();
        ordered.sort_by_key(|(g, h)| (rank[g], rank[h]));

        let mut pending: Vec<AdjudicationRequest> = Vec::new();
        let mut seen: BTreeSet<String> = BTreeSet::new();
        for (g, h) in ordered {
            let clusters = grouping.clusters.get(&g).copied().unwrap_or(1)
                + grouping.clusters.get(&h).copied().unwrap_or(1);
            if clusters > Self::MAX_GROUP_SIZE || grouping.has_blocked_pair(g, h, is_blocked) {
                continue;
            }
            let a = Self::representative(&grouping.members[&g], events);
            let b = Self::representative(&grouping.members[&h], events);
            let request =
                Self::make_request(&events[a], &events[b], info_by_key, lessons, engine_id);
            let Some(entry) = verdicts.entry_for(&request.id) else {
                if seen.insert(request.id.clone()) {
                    pending.push(request);
                }
                continue;
            };
            if entry.answer == VerdictAnswer::Same {
                return (
                    Some((
                        g,
                        h,
                        MergeProvenance::Inference {
                            engine_id: entry.engine.id.clone(),
                            engine_name: entry.engine.display_name.clone(),
                        },
                    )),
                    Vec::new(),
                );
            }
        }
        (None, pending)
    }

    pub(crate) fn merged(
        group: &[&TimeTugCalendarEvent],
        provenance: &[MergeProvenance],
    ) -> TimeTugCalendarEvent {
        if group.len() <= 1 {
            return group[0].clone();
        }
        // The richest copy is the primary so the official title and details win; ties keep the earliest.
        let Some(primary_index) = swift_max_by(0..group.len(), |&l, &r| {
            let sl = DuplicateRules::detail_score(group[l]);
            let sr = DuplicateRules::detail_score(group[r]);
            if sl != sr { sl < sr } else { l > r }
        }) else {
            return group[0].clone(); // unreachable: the group has more than one copy
        };
        let mut result = group[primary_index].clone();
        for (index, other) in group.iter().enumerate() {
            if index == primary_index {
                continue;
            }
            if other.calendar_key() != result.calendar_key() {
                result.additional_calendar_keys.insert(other.calendar_key());
            }
            if result.location().is_none() {
                result.set_location(other.location().map(str::to_string));
            }
            if result.notes().is_none() {
                result.set_notes(other.notes().map(str::to_string));
            }
            if result.url().is_none() {
                result.set_url(other.url().cloned());
            }
        }
        result.conferences = Self::merged_conferences(group[primary_index], group);
        // The card shows the longer copy's range; the takeover (`start`) fires when the copy with a join link
        // starts (the actual appointment), else when the longer copy starts. Everything that schedules or
        // counts down reads `start`/`end`, so it follows the tug time without further change.
        let primary = group[primary_index];
        let Some((_, longer)) = swift_min_by(group.iter().copied().enumerate(), |l, r| {
            let dl = seconds_between(l.1.start(), l.1.end());
            let dr = seconds_between(r.1.start(), r.1.end());
            if dl != dr {
                return dl > dr;
            }
            if (l.0 == primary_index) != (r.0 == primary_index) {
                return l.0 == primary_index;
            }
            l.1.start() < r.1.start()
        }) else {
            return group[0].clone(); // unreachable: the group has more than one copy
        };
        // The carrier is the copy with a join link: the primary when it has one, else the earliest other copy. A copy
        // whose only link is the event's own generic url counts only when no copy has a real link, so the tug time
        // agrees with the real link the card opens.
        let has_real_link = |member: &TimeTugCalendarEvent| {
            member
                .conferences
                .iter()
                .any(|c| c.origin != ConferenceOrigin::EventUrl)
        };
        let has_any_link = |member: &TimeTugCalendarEvent| !member.conferences.is_empty();
        let carrier_of =
            |has: &dyn Fn(&TimeTugCalendarEvent) -> bool| -> Option<&TimeTugCalendarEvent> {
                if has(primary) {
                    Some(primary)
                } else {
                    swift_min_by(group.iter().copied().filter(|m| has(m)), |l, r| {
                        l.start() < r.start()
                    })
                }
            };
        let carrier = carrier_of(&has_real_link).or_else(|| carrier_of(&has_any_link));
        result.set_end(longer.end());
        result.set_start(carrier.map_or_else(|| longer.start(), TimeTugCalendarEvent::start));
        result.display_start = if longer.start() == result.start() {
            None
        } else {
            Some(longer.start())
        };
        // Takeover qualification reads these two, so the merged event must not be weaker than any copy.
        result.other_attendee_count = group
            .iter()
            .map(|m| m.other_attendee_count)
            .max()
            .unwrap_or(result.other_attendee_count);
        if let Some(best) = swift_max_by(group.iter().map(|m| m.response_status), |l, r| {
            Self::attendance(*l) < Self::attendance(*r)
        }) {
            result.response_status = best;
        }
        result.merged_members = group.iter().map(|m| MergedMember::from_event(m)).collect();
        result.merge_provenance = Some(Self::strongest(provenance));
        result
    }

    /// The join links of the whole group: the primary's, then the other copies' in group order, duplicates removed by
    /// identity, then (stable) recognised providers before generic links.
    fn merged_conferences(
        primary: &TimeTugCalendarEvent,
        group: &[&TimeTugCalendarEvent],
    ) -> Vec<ConferenceInfo> {
        let primary_id = primary.id();
        let mut ordered: Vec<&TimeTugCalendarEvent> = vec![primary];
        ordered.extend(group.iter().copied().filter(|m| m.id() != primary_id));
        let mut seen: BTreeSet<String> = BTreeSet::new();
        let all: Vec<&ConferenceInfo> = ordered
            .iter()
            .flat_map(|m| m.conferences.iter())
            .filter(|c| seen.insert(c.identity()))
            .collect();
        all.iter()
            .filter(|c| c.provider != ConferenceProvider::Other)
            .chain(
                all.iter()
                    .filter(|c| c.provider == ConferenceProvider::Other),
            )
            .map(|c| (*c).clone())
            .collect()
    }

    /// accepted > tentative > needsAction > unknown (`None`) > declined.
    fn attendance(status: Option<ResponseStatus>) -> i64 {
        match status {
            Some(ResponseStatus::Accepted) => 4,
            Some(ResponseStatus::Tentative) => 3,
            Some(ResponseStatus::NeedsAction) => 2,
            None => 1,
            Some(ResponseStatus::Declined) => 0,
        }
    }

    /// User decisions outrank model verdicts, which outrank rules.
    fn strongest(list: &[MergeProvenance]) -> MergeProvenance {
        if list.contains(&MergeProvenance::UserConfirmed) {
            return MergeProvenance::UserConfirmed;
        }
        list.iter()
            .find(|p| matches!(p, MergeProvenance::Inference { .. }))
            .cloned()
            .unwrap_or(MergeProvenance::Rule)
    }

    pub(crate) fn make_request(
        a: &TimeTugCalendarEvent,
        b: &TimeTugCalendarEvent,
        info_by_key: &BTreeMap<String, &CalendarInfo>,
        lessons: &LessonBook,
        engine_id: &str,
    ) -> AdjudicationRequest {
        let info = |event: &TimeTugCalendarEvent| info_by_key.get(&event.calendar_key()).copied();
        let projection_a = AdjudicationEvent::new(a, info(a));
        let projection_b = AdjudicationEvent::new(b, info(b));
        let score_a = DuplicateRules::detail_score(a);
        let score_b = DuplicateRules::detail_score(b);
        // Swift interpolates `Date.timeIntervalSince1970` (a Double) into the sort key.
        let sort_key = |p: &AdjudicationEvent| {
            format!(
                "{}|{:?}|{}",
                p.title,
                seconds_between(Timestamp::UNIX_EPOCH, p.start),
                p.calendar_title.as_deref().unwrap_or("")
            )
        };
        let a_first = if score_a != score_b {
            score_a > score_b
        } else {
            sort_key(&projection_a) <= sort_key(&projection_b)
        };
        let (first, second) = if a_first { (a, b) } else { (b, a) };
        let minutes = |seconds: f64| (seconds / 60.0).round() as i64;
        let overlap = seconds_between(
            first.start().max(second.start()),
            first.end().min(second.end()),
        );
        let conflicting = matches!(
            DuplicateRules::decide(a, b),
            PairDecision::Separate(
                SeparateReason::ConflictingLocation
                    | SeparateReason::ConflictingConference
                    | SeparateReason::ConflictingAttendees
            )
        );
        let mut request = AdjudicationRequest::new(
            "",
            AdjudicationEvent::new(first, info(first)),
            AdjudicationEvent::new(second, info(second)),
            lessons.relevant(a, b),
        );
        request.start_offset_minutes = minutes(seconds_between(first.start(), second.start()));
        request.end_offset_minutes = minutes(seconds_between(first.end(), second.end()));
        request.overlap_minutes = minutes(overlap).max(0);
        request.first_details = DuplicateRules::detail_summary(first);
        request.second_details = DuplicateRules::detail_summary(second);
        request.has_conflicting_details = conflicting;
        request.id = request.input().cache_key(engine_id);
        request
    }
}

/// Swift's `max(by:)`: the first element no later element is greater than (`less(result, e)` replaces it).
fn swift_max_by<T>(items: impl IntoIterator<Item = T>, less: impl Fn(&T, &T) -> bool) -> Option<T> {
    let mut iter = items.into_iter();
    let mut result = iter.next()?;
    for item in iter {
        if less(&result, &item) {
            result = item;
        }
    }
    Some(result)
}

/// Swift's `min(by:)`: the first element no later element is less than (`less(e, result)` replaces it).
fn swift_min_by<T>(items: impl IntoIterator<Item = T>, less: impl Fn(&T, &T) -> bool) -> Option<T> {
    let mut iter = items.into_iter();
    let mut result = iter.next()?;
    for item in iter {
        if less(&item, &result) {
            result = item;
        }
    }
    Some(result)
}
