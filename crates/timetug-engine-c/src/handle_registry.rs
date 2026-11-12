//! Port of `HandleRegistry.swift`.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

/// Maps the opaque handles the C interface returns to live values (the sessions). A handle is a token (an increasing
/// integer carried in a pointer), never a real pointer and never reused, so `NULL`, a destroyed handle and a made-up
/// value are all simply not found. The table is the only process-wide state; each engine's state lives in its session.
///
/// Lookups clone an `Arc` out and release the table's lock before the caller uses it, so a slow or blocking session
/// call never holds up another engine's lookup.
pub struct HandleRegistry<T> {
    inner: Mutex<Inner<T>>,
}

struct Inner<T> {
    last_token: usize,
    sessions: BTreeMap<usize, Arc<T>>,
}

impl<T> HandleRegistry<T> {
    pub const fn new() -> HandleRegistry<T> {
        HandleRegistry {
            inner: Mutex::new(Inner {
                last_token: 0,
                sessions: BTreeMap::new(),
            }),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner<T>> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The token for `session`: it starts well above zero and steps by 16, so a handle never looks like a small integer
    /// or NULL.
    pub fn insert(&self, session: Arc<T>) -> usize {
        let mut inner = self.lock();
        inner.last_token = inner.last_token.wrapping_add(16);
        let token = inner.last_token;
        inner.sessions.insert(token, session);
        token
    }

    pub fn session(&self, token: usize) -> Option<Arc<T>> {
        if token == 0 {
            return None;
        }
        self.lock().sessions.get(&token).cloned()
    }

    /// Removes and returns the session; `None` when the token is unknown or already removed.
    pub fn remove(&self, token: usize) -> Option<Arc<T>> {
        if token == 0 {
            return None;
        }
        self.lock().sessions.remove(&token)
    }

    /// How many sessions are alive.
    pub fn len(&self) -> usize {
        self.lock().sessions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl<T> Default for HandleRegistry<T> {
    fn default() -> Self {
        Self::new()
    }
}
