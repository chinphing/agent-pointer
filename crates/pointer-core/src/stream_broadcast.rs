//! Global subscribers for chat stream events (desktop UI + IM channel mirror).

use parking_lot::Mutex;
use std::sync::{Arc, OnceLock};

use crate::models::{ChatStreamSender, StreamEvent};

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

/// Deliver a non-chat event (such as a persistent console PTY update) to every
/// host subscriber. Unlike [`publish_stream`], this does not require an active
/// chat run or its per-run mpsc sender.
pub fn publish_global_stream(ev: StreamEvent) {
    broadcast_stream(&ev);
}

/// Deliver a stream event to UI subscribers and the per-run mpsc sink (web SSE forward).
pub fn publish_stream(tx: &ChatStreamSender, ev: StreamEvent) {
    broadcast_stream(&ev);
    if tx.send(ev).is_err() {
        log::warn!("stream event not delivered (stream receiver dropped)");
    }
}
