use parking_lot::Mutex;
use std::collections::HashMap;
use std::time::{Duration, Instant};

const MAX_BODY_BYTES: usize = 2 * 1024 * 1024;
const RATE_LIMIT_WINDOW: Duration = Duration::from_secs(60);
const RATE_LIMIT_MAX: u32 = 120;

pub struct WebhookGuards {
    rate: Mutex<HashMap<String, (u32, Instant)>>,
}

impl WebhookGuards {
    pub fn new() -> Self {
        Self {
            rate: Mutex::new(HashMap::new()),
        }
    }

    pub fn check_body_size(len: usize) -> bool {
        len <= MAX_BODY_BYTES
    }

    pub fn check_rate(&self, key: &str) -> bool {
        let mut guard = self.rate.lock();
        let now = Instant::now();
        let entry = guard.entry(key.to_string()).or_insert((0, now));
        if now.duration_since(entry.1) > RATE_LIMIT_WINDOW {
            *entry = (0, now);
        }
        if entry.0 >= RATE_LIMIT_MAX {
            return false;
        }
        entry.0 += 1;
        true
    }
}

impl Default for WebhookGuards {
    fn default() -> Self {
        Self::new()
    }
}
