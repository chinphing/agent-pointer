use parking_lot::Mutex;
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

const MAX_ENTRIES: usize = 2000;
const TTL: Duration = Duration::from_secs(3600);

pub struct DedupStore {
    inner: Mutex<HashMap<String, VecDeque<(String, Instant)>>>,
}

impl DedupStore {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }

    fn prune(queue: &mut VecDeque<(String, Instant)>) {
        let now = Instant::now();
        while let Some((_, t)) = queue.front() {
            if now.duration_since(*t) > TTL {
                queue.pop_front();
            } else {
                break;
            }
        }
    }

    pub fn is_seen(&self, namespace: &str, message_id: &str) -> bool {
        let mut guard = self.inner.lock();
        let queue = guard.entry(namespace.to_string()).or_default();
        Self::prune(queue);
        queue.iter().any(|(id, _)| id == message_id)
    }

    pub fn mark_seen(&self, namespace: &str, message_id: &str) {
        if message_id.is_empty() || message_id == "unknown" {
            return;
        }
        let mut guard = self.inner.lock();
        let queue = guard.entry(namespace.to_string()).or_default();
        Self::prune(queue);
        if queue.iter().any(|(id, _)| id == message_id) {
            return;
        }
        queue.push_back((message_id.to_string(), Instant::now()));
        while queue.len() > MAX_ENTRIES {
            queue.pop_front();
        }
    }

    pub fn clear_namespace(&self, namespace: &str) {
        self.inner.lock().remove(namespace);
    }

    /// Legacy helper: check and mark in one step.
    pub fn seen(&self, namespace: &str, message_id: &str) -> bool {
        if self.is_seen(namespace, message_id) {
            return true;
        }
        self.mark_seen(namespace, message_id);
        false
    }
}

impl Default for DedupStore {
    fn default() -> Self {
        Self::new()
    }
}
