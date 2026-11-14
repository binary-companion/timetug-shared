//! Rust-only: the generic delivery queue both front doors share (`host_delivery`): order, state coalescing, the two ways to
//! close and what happens to messages queued after close. The C interface's own tests exercise it with JSON text.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use crate::host_delivery::{Delivery, DeliverySink};

type InsideHook = Arc<dyn Fn(u32) + Send + Sync>;

struct Recorder {
    items: Mutex<Vec<u32>>,
    gate: (Mutex<bool>, Condvar),
    gated: AtomicBool,
    inside: Mutex<Option<InsideHook>>,
}

impl Recorder {
    fn new() -> Arc<Recorder> {
        Arc::new(Recorder {
            items: Mutex::default(),
            gate: (Mutex::new(false), Condvar::new()),
            gated: AtomicBool::new(false),
            inside: Mutex::default(),
        })
    }
    fn items(&self) -> Vec<u32> {
        self.items.lock().unwrap().clone()
    }
    fn hold(&self) {
        *self.gate.0.lock().unwrap() = false;
        self.gated.store(true, Ordering::SeqCst);
    }
    fn release(&self) {
        self.gated.store(false, Ordering::SeqCst);
        *self.gate.0.lock().unwrap() = true;
        self.gate.1.notify_all();
    }
}

impl DeliverySink<u32> for Recorder {
    fn deliver(&self, item: u32) {
        self.items.lock().unwrap().push(item);
        let hook = self.inside.lock().unwrap().clone();
        if let Some(hook) = hook {
            hook(item);
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

fn started(recorder: &Arc<Recorder>) -> Arc<Delivery<u32>> {
    let delivery = Delivery::new(recorder.clone());
    delivery.start().unwrap();
    delivery
}

#[test]
fn items_arrive_in_order() {
    let recorder = Recorder::new();
    let delivery = started(&recorder);
    for item in 1..=5 {
        delivery.enqueue(item);
    }
    wait_until("five items", || recorder.items().len() == 5);
    assert_eq!(recorder.items(), vec![1, 2, 3, 4, 5]);
    delivery.close(true);
}

#[test]
fn a_waiting_state_is_replaced_and_moves_behind_the_events_before_it() {
    let recorder = Recorder::new();
    let delivery = started(&recorder);
    recorder.hold();
    delivery.enqueue(1);
    wait_until("the held first item", || recorder.items() == vec![1]);
    delivery.enqueue_state(100);
    delivery.enqueue(2);
    delivery.enqueue_state(200);
    delivery.enqueue(3);
    delivery.enqueue_state(300);
    recorder.release();
    wait_until("the rest", || recorder.items().len() == 4);
    std::thread::sleep(Duration::from_millis(30));
    assert_eq!(
        recorder.items(),
        vec![1, 2, 3, 300],
        "only the newest state is delivered, behind the events queued before it"
    );
    delivery.close(true);
}

#[test]
fn close_from_outside_waits_for_the_running_callback_and_stops_the_rest() {
    let recorder = Recorder::new();
    let delivery = started(&recorder);
    recorder.hold();
    delivery.enqueue(1);
    wait_until("the callback to start", || recorder.items() == vec![1]);
    delivery.enqueue(2);
    let closer = {
        let delivery = delivery.clone();
        std::thread::spawn(move || delivery.close(true))
    };
    std::thread::sleep(Duration::from_millis(50));
    assert!(
        !closer.is_finished(),
        "close must wait for the running callback"
    );
    recorder.release();
    closer.join().unwrap();
    delivery.enqueue(3);
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(
        recorder.items(),
        vec![1],
        "nothing is delivered after close returned"
    );
}

#[test]
fn close_from_inside_a_callback_returns_at_once_and_no_further_callback_starts() {
    let recorder = Recorder::new();
    let delivery = started(&recorder);
    let inner = delivery.clone();
    let on_thread = Arc::new(AtomicBool::new(false));
    let seen = on_thread.clone();
    *recorder.inside.lock().unwrap() = Some(Arc::new(move |_| {
        seen.store(inner.is_delivery_thread(), Ordering::SeqCst);
        inner.close(false);
    }));
    delivery.enqueue(1);
    delivery.enqueue(2);
    wait_until("the first callback", || !recorder.items().is_empty());
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(recorder.items(), vec![1]);
    assert!(on_thread.load(Ordering::SeqCst));
    assert!(!delivery.is_delivery_thread());
}

#[test]
fn close_is_idempotent() {
    let recorder = Recorder::new();
    let delivery = started(&recorder);
    delivery.close(true);
    delivery.close(true);
    delivery.enqueue(1);
    delivery.enqueue_state(2);
    std::thread::sleep(Duration::from_millis(30));
    assert!(recorder.items().is_empty());
}
