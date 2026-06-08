use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::OnceLock;

static CONNECTED: OnceLock<Mutex<HashMap<String, bool>>> = OnceLock::new();

fn store() -> &'static Mutex<HashMap<String, bool>> {
    CONNECTED.get_or_init(|| Mutex::new(HashMap::new()))
}

fn key(channel: &str, account_id: &str) -> String {
    format!("{channel}:{account_id}")
}

pub fn set_connected(channel: &str, account_id: &str, connected: bool) {
    let k = key(channel, account_id);
    if connected {
        store().lock().insert(k, true);
    } else {
        store().lock().remove(&k);
    }
}

pub fn is_connected(channel: &str, account_id: &str) -> bool {
    store()
        .lock()
        .get(&key(channel, account_id))
        .copied()
        .unwrap_or(false)
}

/// Marks connected on creation; clears on drop (disconnect / task end).
pub struct ConnectionGuard {
    channel: String,
    account_id: String,
}

impl ConnectionGuard {
    pub fn connect(channel: &str, account_id: &str) -> Self {
        set_connected(channel, account_id, true);
        Self {
            channel: channel.into(),
            account_id: account_id.into(),
        }
    }
}

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        set_connected(&self.channel, &self.account_id, false);
    }
}
