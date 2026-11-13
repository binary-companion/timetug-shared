use std::collections::BTreeSet;

use calendar_core::time::seconds_between;
use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::dedup::rules::DuplicateRules;
use crate::model::event::TimeTugCalendarEvent;
use crate::model::merge_types::MergedMember;

/// Swift's `Lesson.Decision`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LessonDecision {
    /// The user said the two events are the same meeting.
    Same,
    /// The user said the two events are different meetings.
    Different,
}

/// One user correction. Titles are normalized; no notes, names or emails are kept.
///
/// Persisted (family A): `lastUsed` is a reference-date number; the two content keys are omitted when `None`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Lesson {
    /// Normalized title of the first event of the pair (the pair is stored in a fixed order).
    pub title_a: String,
    /// Normalized title of the second event.
    pub title_b: String,
    /// Calendar key of the first event.
    pub calendar_key_a: String,
    /// Calendar key of the second event.
    pub calendar_key_b: String,
    /// Which details each side had ("bare", "location+notes"), for the model's context.
    pub signals_a: String,
    /// Details the second event had; see `signals_a`.
    pub signals_b: String,
    /// What the user decided for the pair.
    pub decision: LessonDecision,
    /// When the lesson was recorded or last used; lessons unused for [`LessonBook::EXPIRY`] are dropped.
    #[serde(with = "calendar_core::json::swift_reference_date")]
    pub last_used: Timestamp,
    /// Set only for two different events on the same calendar: the lesson then covers just those two occurrences
    /// (title, start, end), not every later pair with the same titles. Absent (`None`) in lessons saved before this
    /// existed and in cross-calendar lessons, which match by title and calendar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_key_a: Option<String>,
    /// Content key of the second event; see `content_key_a`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_key_b: Option<String>,
}

impl Lesson {
    /// The pair's order-independent identity, as [`LessonBook::pair_key`] builds it.
    pub fn pair_key(&self) -> String {
        LessonBook::pair_key(
            &self.title_a,
            &self.calendar_key_a,
            &self.title_b,
            &self.calendar_key_b,
            self.content_key_a.as_deref(),
            self.content_key_b.as_deref(),
        )
    }
}

/// Small, bounded memory of the user's merge and unmerge decisions.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LessonBook {
    lessons: Vec<Lesson>,
}

impl LessonBook {
    /// The most lessons kept; the oldest beyond this are dropped.
    pub const MAX_LESSONS: usize = 300;
    /// Seconds after which an unused lesson is dropped (182 days).
    pub const EXPIRY: f64 = 182.0 * 24.0 * 60.0 * 60.0;
    /// The most lessons returned by [`LessonBook::relevant`].
    pub const PROMPT_LIMIT: usize = 5;

    /// Swift's `private(set) var lessons`.
    pub fn lessons(&self) -> &[Lesson] {
        &self.lessons
    }

    /// Order-independent identity of a (title, calendar) pair.
    pub fn pair_key(
        title_a: &str,
        calendar_key_a: &str,
        title_b: &str,
        calendar_key_b: &str,
        content_key_a: Option<&str>,
        content_key_b: Option<&str>,
    ) -> String {
        fn side(title: &str, calendar_key: &str, content_key: Option<&str>) -> String {
            format!(
                "{}|{}{}",
                DuplicateRules::normalize(title),
                calendar_key,
                content_key.map_or_else(String::new, |key| format!("|{key}"))
            )
        }
        let mut sides = [
            side(title_a, calendar_key_a, content_key_a),
            side(title_b, calendar_key_b, content_key_b),
        ];
        sides.sort();
        format!("{}#{}", sides[0], sides[1])
    }

    /// Two different events on one calendar: a lesson about them is scoped to these occurrences.
    fn is_scoped(a: (&str, &str), b: (&str, &str)) -> bool {
        a.0 == b.0 && a.1 != b.1
    }

    /// Records the user's `decision` for the pair `a`, `b` at `now`, replacing an earlier lesson for the same pair.
    /// Two different events on one calendar are remembered as those occurrences only. Prunes afterwards.
    pub fn record(
        &mut self,
        a: &MergedMember,
        b: &MergedMember,
        decision: LessonDecision,
        now: Timestamp,
    ) {
        // Two different events on one calendar are remembered too, but only as those occurrences: a title-level
        // lesson would apply to every later pair with these titles on that calendar.
        let scoped = Self::is_scoped(
            (&a.calendar_key, &a.content_key),
            (&b.calendar_key, &b.content_key),
        );
        let mut sides = [a, b];
        sides.sort_by_key(|member| Self::side(member, scoped));
        let lesson = Lesson {
            title_a: DuplicateRules::normalize(&sides[0].title),
            title_b: DuplicateRules::normalize(&sides[1].title),
            calendar_key_a: sides[0].calendar_key.clone(),
            calendar_key_b: sides[1].calendar_key.clone(),
            signals_a: sides[0].details.clone(),
            signals_b: sides[1].details.clone(),
            decision,
            last_used: now,
            content_key_a: scoped.then(|| sides[0].content_key.clone()),
            content_key_b: scoped.then(|| sides[1].content_key.clone()),
        };
        let key = lesson.pair_key();
        self.lessons.retain(|existing| existing.pair_key() != key);
        self.lessons.push(lesson);
        self.prune(now);
    }

    /// The recorded decision for this pair of events, if any.
    pub fn decision(&self, a: &TimeTugCalendarEvent, b: &TimeTugCalendarEvent) -> Option<&Lesson> {
        let (a_calendar, b_calendar) = (a.calendar_key(), b.calendar_key());
        let (a_content, b_content) = (a.content_key(), b.content_key());
        let scoped = Self::is_scoped((&a_calendar, &a_content), (&b_calendar, &b_content));
        let key = Self::pair_key(
            a.title(),
            &a_calendar,
            b.title(),
            &b_calendar,
            scoped.then_some(a_content.as_str()),
            scoped.then_some(b_content.as_str()),
        );
        self.lessons.iter().find(|lesson| lesson.pair_key() == key)
    }

    /// Up to `PROMPT_LIMIT` lessons most similar to this pair (shared title words, same calendar pair).
    pub fn relevant(&self, a: &TimeTugCalendarEvent, b: &TimeTugCalendarEvent) -> Vec<Lesson> {
        let words = |titles: [&str; 2]| -> BTreeSet<String> {
            titles
                .iter()
                .flat_map(|title| {
                    DuplicateRules::normalize(title)
                        .split(' ')
                        .map(str::to_string)
                        .collect::<Vec<_>>()
                })
                .filter(|word| !word.is_empty())
                .collect()
        };
        let wanted = words([a.title(), b.title()]);
        let calendars: BTreeSet<String> = BTreeSet::from([a.calendar_key(), b.calendar_key()]);
        let mut scored: Vec<(&Lesson, usize, String)> = self
            .lessons
            .iter()
            .map(|lesson| {
                let lesson_words = words([&lesson.title_a, &lesson.title_b]);
                let lesson_calendars =
                    BTreeSet::from([lesson.calendar_key_a.clone(), lesson.calendar_key_b.clone()]);
                let bonus = if calendars == lesson_calendars { 2 } else { 0 };
                (
                    lesson,
                    wanted.intersection(&lesson_words).count() + bonus,
                    lesson.pair_key(),
                )
            })
            .filter(|(_, score, _)| *score > 0)
            .collect();
        scored.sort_by(|x, y| y.1.cmp(&x.1).then_with(|| x.2.cmp(&y.2)));
        scored
            .into_iter()
            .take(Self::PROMPT_LIMIT)
            .map(|(lesson, _, _)| lesson.clone())
            .collect()
    }

    /// Marks the lessons with the given pair keys as used at `now`, so they are not expired.
    pub fn touch(&mut self, pair_keys: &BTreeSet<String>, now: Timestamp) {
        for lesson in &mut self.lessons {
            if pair_keys.contains(&lesson.pair_key()) {
                lesson.last_used = now;
            }
        }
    }

    /// Drops lessons unused for `EXPIRY`, then the oldest beyond `MAX_LESSONS`.
    pub fn prune(&mut self, now: Timestamp) {
        self.lessons
            .retain(|lesson| seconds_between(lesson.last_used, now) <= Self::EXPIRY);
        if self.lessons.len() > Self::MAX_LESSONS {
            self.lessons.sort_by_key(|lesson| lesson.last_used);
            let excess = self.lessons.len() - Self::MAX_LESSONS;
            self.lessons.drain(..excess);
        }
    }

    fn side(member: &MergedMember, scoped: bool) -> String {
        format!(
            "{}|{}{}",
            DuplicateRules::normalize(&member.title),
            member.calendar_key,
            if scoped {
                format!("|{}", member.content_key)
            } else {
                String::new()
            }
        )
    }
}
