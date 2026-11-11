//! Tests of `store/join_all.rs` (Rust only: Swift uses `withTaskGroup`).

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use crate::store::join_all::{Completions, join_all};

type Boxed<T> = Pin<Box<dyn Future<Output = T> + Send>>;

/// A future that returns `Pending` (and wakes itself) `turns` times, then `value`; `polled` counts first polls.
fn after<T: Send + 'static>(
    turns: usize,
    value: T,
    polled: Arc<AtomicUsize>,
    finished: Arc<Mutex<Vec<usize>>>,
    index: usize,
) -> Boxed<T> {
    Box::pin(async move {
        polled.fetch_add(1, Ordering::SeqCst);
        for _ in 0..turns {
            tokio::task::yield_now().await;
        }
        finished.lock().unwrap().push(index);
        value
    })
}

#[tokio::test(flavor = "current_thread")]
async fn three_futures_finishing_in_reverse_order_return_in_input_order() {
    let polled = Arc::new(AtomicUsize::new(0));
    let finished = Arc::new(Mutex::new(Vec::new()));
    let futures = vec![
        after(6, "first", polled.clone(), finished.clone(), 0),
        after(3, "second", polled.clone(), finished.clone(), 1),
        after(0, "third", polled.clone(), finished.clone(), 2),
    ];
    let results = join_all(futures).await;
    assert_eq!(results, ["first", "second", "third"]);
    // They really did finish in reverse order.
    assert_eq!(*finished.lock().unwrap(), [2, 1, 0]);
}

#[tokio::test(flavor = "current_thread")]
async fn all_futures_are_polled_before_any_completes() {
    let polled = Arc::new(AtomicUsize::new(0));
    // Each future starts, waits one turn, and reports how many futures had started when it finished.
    let futures: Vec<Boxed<usize>> = (0..3)
        .map(|_| {
            let polled = polled.clone();
            let future: Boxed<usize> = Box::pin(async move {
                polled.fetch_add(1, Ordering::SeqCst);
                tokio::task::yield_now().await;
                polled.load(Ordering::SeqCst)
            });
            future
        })
        .collect();
    assert_eq!(join_all(futures).await, [3, 3, 3]);
}

#[tokio::test(flavor = "current_thread")]
async fn completions_yield_in_completion_order_and_then_none() {
    let polled = Arc::new(AtomicUsize::new(0));
    let finished = Arc::new(Mutex::new(Vec::new()));
    let mut completions = Completions::new(vec![
        after(5, 'a', polled.clone(), finished.clone(), 0),
        after(0, 'b', polled.clone(), finished.clone(), 1),
        after(2, 'c', polled.clone(), finished.clone(), 2),
    ]);
    assert_eq!(completions.next().await, Some('b'));
    assert_eq!(completions.next().await, Some('c'));
    assert_eq!(completions.next().await, Some('a'));
    assert_eq!(completions.next().await, None);
}

#[tokio::test(flavor = "current_thread")]
async fn no_futures_gives_no_results() {
    let results: Vec<u8> = join_all(Vec::new()).await;
    assert!(results.is_empty());
    assert_eq!(Completions::<u8>::new(Vec::new()).next().await, None);
}

#[tokio::test(flavor = "current_thread")]
async fn a_future_that_is_ready_with_ties_is_taken_in_input_order() {
    let ready = |v: u8| -> Boxed<u8> { Box::pin(async move { v }) };
    let mut completions = Completions::new(vec![ready(1), ready(2), ready(3)]);
    assert_eq!(completions.next().await, Some(1));
    assert_eq!(completions.next().await, Some(2));
    assert_eq!(completions.next().await, Some(3));
}
