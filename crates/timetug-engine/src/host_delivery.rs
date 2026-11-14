//! Delivery of engine output to a host on one dedicated thread: the queue both front doors share (the C interface
//! delivers JSON text, the bindings deliver typed values). Moved from `timetug-engine-c` (port of `Delivery.swift`), made
//! generic over the item and the sink; the C callback type stays in `timetug-engine-c`.

use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::ThreadId;

/// What items are delivered to. Called on the delivery thread, one item at a time, in order.
pub trait DeliverySink<M>: Send + Sync {
    /// Delivers one item.
    fn deliver(&self, item: M);
}

enum Entry<M> {
    Item(M),
    State,
}

struct Queue<M> {
    items: VecDeque<Entry<M>>,
    pending_state: Option<M>,
    closed: bool,
    in_callback: bool,
    thread: Option<ThreadId>,
}

/// Delivers items to the host on one dedicated thread, one at a time, in order.
///
/// - Events and requests are queued without limit and never dropped or reordered.
/// - States coalesce: a new state replaces the one still waiting (if any) and moves to the back of the queue, so the
///   host always gets the newest state, never a state older than an event queued before it.
/// - `close` stops delivery. Called from outside a callback with `wait_for_callback`, it returns only when no callback
///   is running, and none starts after it. Called from inside a callback, no further callback starts after the current
///   one returns.
///
/// A thread (not a task) carries the callbacks, so a slow host blocks only its own engine's delivery and never the
/// engine thread.
pub struct Delivery<M: Send + 'static> {
    sink: Arc<dyn DeliverySink<M>>,
    queue: Mutex<Queue<M>>,
    changed: Condvar,
}

impl<M: Send + 'static> Delivery<M> {
    /// Creates a delivery that passes items to `sink`. No thread runs until [`Delivery::start`].
    pub fn new(sink: Arc<dyn DeliverySink<M>>) -> Arc<Delivery<M>> {
        Arc::new(Delivery {
            sink,
            queue: Mutex::new(Queue {
                items: VecDeque::new(),
                pending_state: None,
                closed: false,
                in_callback: false,
                thread: None,
            }),
            changed: Condvar::new(),
        })
    }

    fn lock(&self) -> MutexGuard<'_, Queue<M>> {
        self.queue.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Starts the delivery thread. Called once, after the session is fully built. Fails when the system cannot start a
    /// thread.
    pub fn start(self: &Arc<Delivery<M>>) -> std::io::Result<()> {
        let delivery = self.clone();
        std::thread::Builder::new()
            .name("TimeTug engine delivery".to_string())
            .spawn(move || delivery.run())
            .map(drop)
    }

    /// Queues an event or request. It is never dropped or reordered; ignored once closed.
    pub fn enqueue(&self, item: M) {
        let mut queue = self.lock();
        if queue.closed {
            return;
        }
        queue.items.push_back(Entry::Item(item));
        self.changed.notify_all();
    }

    /// Queues a state, replacing a state still waiting (coalescing); ignored once closed.
    pub fn enqueue_state(&self, state: M) {
        let mut queue = self.lock();
        if queue.closed {
            return;
        }
        if queue.pending_state.is_some() {
            queue.items.retain(|entry| !matches!(entry, Entry::State));
        }
        queue.pending_state = Some(state);
        queue.items.push_back(Entry::State);
        self.changed.notify_all();
    }

    /// True on the delivery thread, that is inside one of this engine's callbacks.
    pub fn is_delivery_thread(&self) -> bool {
        self.lock().thread == Some(std::thread::current().id())
    }

    /// Stops delivery; see the type's documentation. Idempotent.
    pub fn close(&self, wait_for_callback: bool) {
        let mut queue = self.lock();
        queue.closed = true;
        queue.items.clear();
        queue.pending_state = None;
        self.changed.notify_all();
        if wait_for_callback {
            while queue.in_callback {
                queue = self
                    .changed
                    .wait(queue)
                    .unwrap_or_else(PoisonError::into_inner);
            }
        }
    }

    fn run(&self) {
        let mut queue = self.lock();
        queue.thread = Some(std::thread::current().id());
        loop {
            while queue.items.is_empty() && !queue.closed {
                queue = self
                    .changed
                    .wait(queue)
                    .unwrap_or_else(PoisonError::into_inner);
            }
            if queue.closed {
                break;
            }
            let item = match queue.items.pop_front() {
                Some(Entry::Item(item)) => item,
                Some(Entry::State) => match queue.pending_state.take() {
                    Some(state) => state,
                    None => continue,
                },
                None => continue,
            };
            queue.in_callback = true;
            drop(queue);
            self.sink.deliver(item);
            queue = self.lock();
            queue.in_callback = false;
            self.changed.notify_all();
        }
        queue.thread = None;
    }
}
