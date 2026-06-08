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

    pub fn seen(&self, namespace: &str, message_id: &str) -> bool {
        let mut guard = self.inner.lock();
        let queue = guard.entry(namespace.to_string()).or_default();
        let now = Instant::now();
        while let Some((_, t)) = queue.front() {
            if now.duration_since(*t) > TTL {
                queue.pop_front();
            } else {
                break;
            }
        }
        if queue.iter().any(|(id, _)| id == message_id) {
            return true;
        }
        queue.push_back((message_id.to_string(), now));
        while queue.len() > MAX_ENTRIES {
            queue.pop_front();
        }
        false
    }
}

impl Default for DedupStore {
    fn default() -> Self {
        Self::new()
    }
}
