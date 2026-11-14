//! Port of `TimeTugCoreTests/CalendarStoreNoticesTests.swift`.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

use async_trait::async_trait;
use calendar_core::failure::Outcome;
use calendar_core::source_types::{SourceNotice, SourceNoticeKind};
use calendar_core::time::DateInterval;
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};

use super::calendar_store_tests::{FakeSource, new_store};
use super::support::date;
use crate::model::calendar_info::CalendarInfo;
use crate::model::event::TimeTugCalendarEvent;
use crate::sources::{CalendarSource, SourceChange, SourceError};

fn now() -> jiff::Timestamp {
    date("2026-09-18T10:00:00Z")
}

/// `private actor NoticeSource`.
struct NoticeSource {
    id: String,
    state: Mutex<(Vec<SourceNotice>, bool)>,
}

impl NoticeSource {
    fn new(id: &str, notices: Vec<SourceNotice>) -> Arc<NoticeSource> {
        Arc::new(NoticeSource {
            id: id.to_string(),
            state: Mutex::new((notices, false)),
        })
    }
    fn set_notices(&self, notices: Vec<SourceNotice>) {
        self.state.lock().unwrap_or_else(PoisonError::into_inner).0 = notices;
    }
    fn set_failing(&self, value: bool) {
        self.state.lock().unwrap_or_else(PoisonError::into_inner).1 = value;
    }
}

#[async_trait]
impl CalendarSource for NoticeSource {
    fn id(&self) -> String {
        self.id.clone()
    }
    fn display_name(&self) -> String {
        "Notice".to_string()
    }
    async fn calendars(&self) -> Outcome<Vec<CalendarInfo>> {
        Ok(Vec::new())
    }
    async fn events_in(&self, _interval: DateInterval) -> Outcome<Vec<TimeTugCalendarEvent>> {
        let fails = self.state.lock().unwrap_or_else(PoisonError::into_inner).1;
        if fails {
            return Err(SourceError::AuthExpired.into());
        }
        Ok(Vec::new())
    }
    async fn notices(&self) -> Vec<SourceNotice> {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .0
            .clone()
    }
    fn changes(&self) -> UnboundedReceiver<SourceChange> {
        unbounded_channel().1
    }
}

fn warning() -> SourceNotice {
    SourceNotice::new(SourceNoticeKind::UnreadableRecurrence, 2)
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_sources_notices_reach_the_snapshot_and_are_cleared_when_it_is_fixed() {
    let source = NoticeSource::new("a", vec![warning()]);
    let plain = FakeSource::new("b");
    let store = new_store(sources![source, plain], "UTC");
    let mut snapshot = store.refresh(now(), 60.0, None, None).await;
    assert_eq!(
        snapshot.notices,
        BTreeMap::from([("a".to_string(), vec![warning()])])
    );
    source.set_notices(vec![]);
    snapshot = store.refresh(now(), 60.0, None, None).await;
    assert!(snapshot.notices.is_empty());
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn notices_are_cleared_when_the_source_fails_or_is_removed() {
    let source = NoticeSource::new("a", vec![warning()]);
    let store = new_store(sources![source], "UTC");
    store.refresh(now(), 60.0, None, None).await;
    source.set_failing(true);
    assert!(
        store
            .refresh(now(), 60.0, None, None)
            .await
            .notices
            .is_empty()
    );
    source.set_failing(false);
    assert_eq!(
        store.refresh(now(), 60.0, None, None).await.notices.len(),
        1
    );
    store.set_sources(vec![]);
    assert!(store.set_inference_enabled(false, now()).notices.is_empty());
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_source_without_notices_gives_none() {
    let store = new_store(sources![FakeSource::new("a")], "UTC");
    assert!(
        store
            .refresh(now(), 60.0, None, None)
            .await
            .notices
            .is_empty()
    );
}
