//! Tests of `Delivery`, `Outbox` and `HandleRegistry`, which Swift pinned only through the lifecycle tests: order,
//! state coalescing, the two ways to close, and tokens that are never reused.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use crate::delivery::{Delivery, MessageSink, Outbox};
use crate::handle_registry::HandleRegistry;

type Hook = Arc<dyn Fn(&str) + Send + Sync>;

/// Records every message; can hold the callback until released, and runs a hook inside the callback.
struct Recorder {
    messages: Mutex<Vec<String>>,
    gate: (Mutex<bool>, Condvar),
    gated: AtomicBool,
    calls: AtomicUsize,
    hook: Mutex<Option<Hook>>,
}

impl Recorder {
    fn new() -> Arc<Recorder> {
        Arc::new(Recorder {
            messages: Mutex::new(Vec::new()),
            gate: (Mutex::new(false), Condvar::new()),
            gated: AtomicBool::new(false),
            calls: AtomicUsize::new(0),
            hook: Mutex::new(None),
        })
    }

    fn messages(&self) -> Vec<String> {
        self.messages.lock().unwrap().clone()
    }

    /// The next callback blocks until `release`.
    fn hold(&self) {
        *self.gate.0.lock().unwrap() = false;
        self.gated.store(true, Ordering::SeqCst);
    }

    fn release(&self) {
        self.gated.store(false, Ordering::SeqCst);
        *self.gate.0.lock().unwrap() = true;
        self.gate.1.notify_all();
    }

    fn set_hook(&self, hook: Hook) {
        *self.hook.lock().unwrap() = Some(hook);
    }
}

impl MessageSink for Recorder {
    fn deliver(&self, message: &str) {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.messages.lock().unwrap().push(message.to_string());
        let hook = self.hook.lock().unwrap().clone();
        if let Some(hook) = hook {
            hook(message);
        }
        if self.gated.load(Ordering::SeqCst) {
            let mut open = self.gate.0.lock().unwrap();
            while !*open {
                open = self.gate.1.wait(open).unwrap();
            }
        }
    }
}

fn wait_until(what: &str, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !condition() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn started(recorder: &Arc<Recorder>) -> Arc<Delivery> {
    let delivery = Delivery::new(recorder.clone());
    delivery.start().unwrap();
    delivery
}

#[test]
fn messages_arrive_in_order() {
    let recorder = Recorder::new();
    let delivery = started(&recorder);
    for index in 0..50 {
        delivery.enqueue(format!("m{index}"));
    }
    wait_until("50 messages", || recorder.messages().len() == 50);
    let expected: Vec<String> = (0..50).map(|index| format!("m{index}")).collect();
    assert_eq!(recorder.messages(), expected);
    delivery.close(true);
}

/// A new state replaces the one still waiting and moves to the back, so the host never gets a state older than an
/// event queued before it, and never an outdated state at all.
#[test]
fn a_waiting_state_is_replaced_and_moves_behind_the_events_before_it() {
    let recorder = Recorder::new();
    let delivery = started(&recorder);
    recorder.hold();
    delivery.enqueue("first".to_string());
    wait_until("the first callback", || {
        recorder.calls.load(Ordering::SeqCst) == 1
    });
    delivery.enqueue_state("s1".to_string());
    delivery.enqueue("event".to_string());
    delivery.enqueue_state("s2".to_string());
    delivery.enqueue_state("s3".to_string());
    recorder.release();
    wait_until("three more messages", || recorder.messages().len() == 3);
    delivery.close(true);
    assert_eq!(recorder.messages(), vec!["first", "event", "s3"]);
}

#[test]
fn close_from_outside_waits_for_the_running_callback_and_stops_the_rest() {
    let recorder = Recorder::new();
    let delivery = started(&recorder);
    recorder.hold();
    delivery.enqueue("running".to_string());
    delivery.enqueue("never".to_string());
    wait_until("the callback", || {
        recorder.calls.load(Ordering::SeqCst) == 1
    });
    let closer = {
        let delivery = delivery.clone();
        let done = Arc::new(AtomicBool::new(false));
        let flag = done.clone();
        let handle = std::thread::spawn(move || {
            delivery.close(true);
            flag.store(true, Ordering::SeqCst);
        });
        (handle, done)
    };
    std::thread::sleep(Duration::from_millis(100));
    assert!(
        !closer.1.load(Ordering::SeqCst),
        "close returned while a callback was running"
    );
    recorder.release();
    closer.0.join().unwrap();
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(recorder.messages(), vec!["running"]);
    delivery.enqueue("after".to_string());
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(
        recorder.messages(),
        vec!["running"],
        "nothing is delivered after close"
    );
}

#[test]
fn close_from_inside_a_callback_returns_at_once_and_no_further_callback_starts() {
    let recorder = Recorder::new();
    let delivery = started(&recorder);
    let observed_delivery_thread = Arc::new(AtomicBool::new(false));
    let hook_delivery = delivery.clone();
    let observed = observed_delivery_thread.clone();
    recorder.set_hook(Arc::new(move |message: &str| {
        if message == "a" {
            observed.store(hook_delivery.is_delivery_thread(), Ordering::SeqCst);
            hook_delivery.close(!hook_delivery.is_delivery_thread());
        }
    }));
    delivery.enqueue("a".to_string());
    delivery.enqueue("b".to_string());
    wait_until("the first callback", || {
        recorder.calls.load(Ordering::SeqCst) >= 1
    });
    std::thread::sleep(Duration::from_millis(100));
    assert!(observed_delivery_thread.load(Ordering::SeqCst));
    assert_eq!(recorder.messages(), vec!["a"]);
    assert!(
        !delivery.is_delivery_thread(),
        "the test thread is not the delivery thread"
    );
}

#[test]
fn close_is_idempotent() {
    let recorder = Recorder::new();
    let delivery = started(&recorder);
    delivery.close(true);
    delivery.close(true);
    delivery.close(false);
}

#[test]
fn the_outbox_holds_messages_until_ready_and_sends_ready_first() {
    let recorder = Recorder::new();
    let delivery = started(&recorder);
    let outbox = Outbox::new(delivery.clone());
    outbox.post("held-1".to_string());
    outbox.post_state("dropped-state".to_string());
    outbox.post("held-2".to_string());
    std::thread::sleep(Duration::from_millis(50));
    assert!(recorder.messages().is_empty(), "nothing before ready");
    assert!(outbox.open_with("ready".to_string()));
    assert!(!outbox.open_with("ready-again".to_string()), "only once");
    outbox.post("live".to_string());
    outbox.post_state("state".to_string());
    wait_until("five messages", || recorder.messages().len() == 5);
    delivery.close(true);
    assert_eq!(
        recorder.messages(),
        vec!["ready", "held-1", "held-2", "live", "state"]
    );
}

#[test]
fn handle_tokens_are_never_zero_never_reused_and_unknown_values_are_not_found() {
    let registry: HandleRegistry<u32> = HandleRegistry::new();
    assert!(registry.session(0).is_none());
    assert!(registry.remove(0).is_none());
    let first = registry.insert(Arc::new(1));
    let second = registry.insert(Arc::new(2));
    assert!(first >= 16 && first.is_multiple_of(16) && second > first);
    assert_eq!(registry.session(first).as_deref(), Some(&1));
    assert_eq!(registry.remove(first).as_deref(), Some(&1));
    assert!(
        registry.remove(first).is_none(),
        "a second remove finds nothing"
    );
    assert!(registry.session(first).is_none());
    let third = registry.insert(Arc::new(3));
    assert!(third > second, "a removed token is not handed out again");
    assert!(registry.session(0xdead_beef).is_none());
    assert_eq!(registry.len(), 2);
}
