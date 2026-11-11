//! Running a set of futures concurrently on the caller's task, without a runtime and without the `futures` crate.
//!
//! Swift's `CalendarStore.refresh` runs one child task per source in a task group and consumes the results with
//! `for await`, in completion order. [`Completions`] is that consumption: it polls every unfinished future on each
//! turn (so none starves another) and hands results out as they finish, ties in input order. [`join_all`] is the
//! all-results form: the results in input order.

use std::collections::VecDeque;
use std::future::{Future, poll_fn};
use std::pin::Pin;
use std::task::Poll;

pub(crate) type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

/// A task group: futures polled together, results taken one at a time as they finish.
pub(crate) struct Completions<T> {
    unfinished: Vec<Option<BoxFuture<T>>>,
    finished: VecDeque<(usize, T)>,
}

impl<T> Completions<T> {
    pub(crate) fn new(futures: Vec<BoxFuture<T>>) -> Completions<T> {
        Completions {
            unfinished: futures.into_iter().map(Some).collect(),
            finished: VecDeque::new(),
        }
    }

    /// The next result with its input index, or `None` when every future has been taken. Every future is polled on
    /// every turn that has no finished result waiting.
    pub(crate) async fn next_indexed(&mut self) -> Option<(usize, T)> {
        poll_fn(|cx| {
            if self.finished.is_empty() {
                for (index, slot) in self.unfinished.iter_mut().enumerate() {
                    let Some(future) = slot else { continue };
                    if let Poll::Ready(value) = future.as_mut().poll(cx) {
                        *slot = None;
                        self.finished.push_back((index, value));
                    }
                }
            }
            match self.finished.pop_front() {
                Some(result) => Poll::Ready(Some(result)),
                None if self.unfinished.iter().all(Option::is_none) => Poll::Ready(None),
                None => Poll::Pending,
            }
        })
        .await
    }

    /// The next result in completion order, or `None` when every future has been taken.
    pub(crate) async fn next(&mut self) -> Option<T> {
        self.next_indexed().await.map(|(_, value)| value)
    }
}

/// Every result, in input order, once all futures have finished. All futures are polled before any is polled twice.
// `CalendarStore::refresh` consumes `Completions` in completion order, as Swift's `for await` does; this all-results
// form is the helper of the phase plan and is exercised by its tests.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) async fn join_all<T>(futures: Vec<BoxFuture<T>>) -> Vec<T> {
    let mut slots: Vec<Option<T>> = futures.iter().map(|_| None).collect();
    let mut group = Completions::new(futures);
    while let Some((index, value)) = group.next_indexed().await {
        slots[index] = Some(value);
    }
    // Every future finished, so every slot is filled.
    slots.into_iter().flatten().collect()
}
