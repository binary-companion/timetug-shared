//! Port of `TimeTugCoreTests/BaselineDuplicateResolver.swift`: the frozen pre-optimization resolver that the
//! equivalence tests compare the interval-tree resolver with. It scans every pair `i < j` and precomputes the blocked
//! sets, as the Swift baseline does. Its `merged`, `representative` and `make_request` are the resolver's own
//! (`pub(crate)`): the Swift baseline holds frozen copies of them, but the optimization under test is only the pair
//! scan and the lazy blocking, and the vectors pin `merged` and `make_request` independently.

use std::collections::{BTreeMap, BTreeSet};

use crate::dedup::adjudication::{AdjudicationRequest, VerdictAnswer, VerdictCache};
use crate::dedup::lesson_book::{LessonBook, LessonDecision};
use crate::dedup::resolver::DuplicateResolution;
use crate::dedup::resolver::DuplicateResolver;
use crate::dedup::rules::{DuplicateRules, PairDecision};
use crate::model::calendar_info::CalendarInfo;
use crate::model::event::TimeTugCalendarEvent;
use crate::model::merge_types::MergeProvenance;

pub(super) struct BaselineDuplicateResolver;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
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

    fn ordered_groups(&self) -> Vec<usize> {
        let mut keys: Vec<usize> = self.members.keys().copied().collect();
        keys.sort_by_key(|k| self.members[k][0]);
        keys
    }

    fn has_blocked_pair(&self, g: usize, h: usize, blocked: &BTreeSet<Pair>) -> bool {
        let empty = Vec::new();
        let gm = self.members.get(&g).unwrap_or(&empty);
        let hm = self.members.get(&h).unwrap_or(&empty);
        gm.iter()
            .any(|&x| hm.iter().any(|&y| blocked.contains(&Pair::new(x, y))))
    }

    fn union(&mut self, g: usize, h: usize, why: Vec<MergeProvenance>) {
        let mut merged = self.members.get(&g).cloned().unwrap_or_default();
        merged.extend(self.members.get(&h).cloned().unwrap_or_default());
        merged.sort();
        self.members.insert(g, merged);
        for &k in self.members.get(&h).cloned().unwrap_or_default().iter() {
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

type ModelMerge = (usize, usize, MergeProvenance);

impl BaselineDuplicateResolver {
    pub(super) fn resolve(
        events: &[TimeTugCalendarEvent],
        calendars: &[CalendarInfo],
        lessons: &LessonBook,
        verdicts: Option<&VerdictCache>,
    ) -> DuplicateResolution {
        let engine_id = "unknown";
        let mut info_by_key: BTreeMap<String, &CalendarInfo> = BTreeMap::new();
        for calendar in calendars {
            info_by_key.entry(calendar.key()).or_insert(calendar);
        }
        let mut exact: Vec<MergeLink> = Vec::new();
        let mut certain: Vec<MergeLink> = Vec::new();
        let mut blocked: BTreeSet<Pair> = BTreeSet::new();
        let mut learned_blocked: BTreeSet<Pair> = BTreeSet::new();
        let mut ambiguous: BTreeSet<Pair> = BTreeSet::new();
        let mut used_lesson_keys: BTreeSet<String> = BTreeSet::new();

        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                let (a, b) = (&events[i], &events[j]);
                let lesson_applies =
                    DuplicateRules::is_manual_candidate(a, b) || a.content_key() == b.content_key();
                if lesson_applies && let Some(lesson) = lessons.decision(a, b) {
                    used_lesson_keys.insert(lesson.pair_key());
                    if lesson.decision == LessonDecision::Same {
                        certain.push(MergeLink {
                            i,
                            j,
                            why: MergeProvenance::UserConfirmed,
                        });
                    } else {
                        blocked.insert(Pair::new(i, j));
                        learned_blocked.insert(Pair::new(i, j));
                    }
                    continue;
                }
                match DuplicateRules::decide(a, b) {
                    PairDecision::Merge(crate::dedup::rules::MergeReason::ExactMatch) => exact
                        .push(MergeLink {
                            i,
                            j,
                            why: MergeProvenance::Rule,
                        }),
                    PairDecision::Merge(_) => certain.push(MergeLink {
                        i,
                        j,
                        why: MergeProvenance::Rule,
                    }),
                    PairDecision::Separate(_) => {
                        blocked.insert(Pair::new(i, j));
                    }
                    PairDecision::Ambiguous => {
                        if verdicts.is_some() {
                            ambiguous.insert(Pair::new(i, j));
                        }
                    }
                }
            }
        }

        let mut grouping = Grouping::new(events.len());
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
                let stoppers = if link.why == MergeProvenance::UserConfirmed {
                    &learned_blocked
                } else {
                    &blocked
                };
                if grouping.has_blocked_pair(gi, gj, stoppers) {
                    continue;
                }
                grouping.union(gi, gj, vec![link.why.clone()]);
            }
        };
        union(&mut grouping, &exact);
        union(&mut grouping, &certain);

        let group_ids: Vec<usize> = grouping.members.keys().copied().collect();
        for g in group_ids {
            grouping.clusters.insert(g, 1);
        }

        let mut pending: Vec<AdjudicationRequest> = Vec::new();
        if let Some(verdicts) = verdicts {
            loop {
                let (merge, step_pending) = Self::next_model_step(
                    &grouping,
                    events,
                    &ambiguous,
                    &blocked,
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
            output.push(DuplicateResolver::merged(&members, &provenance));
        }

        let mut candidates: BTreeMap<String, Vec<TimeTugCalendarEvent>> = BTreeMap::new();
        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                let (oi, oj) = (output_index[i], output_index[j]);
                if oi == oj
                    || !(DuplicateRules::is_manual_candidate(&events[i], &events[j])
                        || events[i].content_key() == events[j].content_key())
                {
                    continue;
                }
                let (left, right) = (output[oi].id(), output[oj].id());
                let listed = candidates
                    .get(&left)
                    .is_some_and(|l| l.iter().any(|e| e.id() == right));
                if !listed {
                    candidates
                        .entry(left.clone())
                        .or_default()
                        .push(output[oj].clone());
                }
                let listed = candidates
                    .get(&right)
                    .is_some_and(|l| l.iter().any(|e| e.id() == left));
                if !listed {
                    candidates
                        .entry(right)
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

    #[allow(clippy::too_many_arguments)]
    fn next_model_step(
        grouping: &Grouping,
        events: &[TimeTugCalendarEvent],
        ambiguous: &BTreeSet<Pair>,
        blocked: &BTreeSet<Pair>,
        verdicts: &VerdictCache,
        info_by_key: &BTreeMap<String, &CalendarInfo>,
        lessons: &LessonBook,
        engine_id: &str,
    ) -> (Option<ModelMerge>, Vec<AdjudicationRequest>) {
        let rank: BTreeMap<usize, usize> = grouping
            .ordered_groups()
            .into_iter()
            .enumerate()
            .map(|(i, g)| (g, i))
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
            let total = grouping.clusters.get(&g).copied().unwrap_or(1)
                + grouping.clusters.get(&h).copied().unwrap_or(1);
            if total > DuplicateResolver::MAX_GROUP_SIZE || grouping.has_blocked_pair(g, h, blocked)
            {
                continue;
            }
            let a = DuplicateResolver::representative(&grouping.members[&g], events);
            let b = DuplicateResolver::representative(&grouping.members[&h], events);
            let request = DuplicateResolver::make_request(
                &events[a],
                &events[b],
                info_by_key,
                lessons,
                engine_id,
            );
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
                    vec![],
                );
            }
        }
        (None, pending)
    }
}
