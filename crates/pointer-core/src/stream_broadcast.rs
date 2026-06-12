//! Global subscribers for chat stream events (desktop UI + IM channel mirror).

use parking_lot::Mutex;
use std::sync::{Arc, OnceLock};

use crate::models::StreamEvent;

type StreamSubscriber = Arc<dyn Fn(StreamEvent) + Send + Sync>;

static SUBSCRIBERS: OnceLock<Mutex<Vec<StreamSubscriber>>> = OnceLock::new();

fn subscribers() -> &'static Mutex<Vec<StreamSubscriber>> {
    SUBSCRIBERS.get_or_init(|| Mutex::new(Vec::new()))
}

pub fn subscribe_stream(handler: StreamSubscriber) {
    subscribers().lock().push(handler);
}

pub fn broadcast_stream(ev: &StreamEvent) {
    for sub in subscribers().lock().iter() {
        sub(ev.clone());
    }
}
