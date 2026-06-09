use std::collections::HashMap;
use std::sync::OnceLock;

use parking_lot::Mutex;

use crate::traits::OutboundContext;

static SESSIONS: OnceLock<Mutex<HashMap<String, OutboundContext>>> = OnceLock::new();

fn sessions() -> &'static Mutex<HashMap<String, OutboundContext>> {
    SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn register(conversation_id: &str, ctx: OutboundContext) {
    sessions()
        .lock()
        .insert(conversation_id.to_string(), ctx);
}

pub fn unregister(conversation_id: &str) {
    sessions().lock().remove(conversation_id);
}

pub fn get(conversation_id: &str) -> Option<OutboundContext> {
    sessions().lock().get(conversation_id).cloned()
}
