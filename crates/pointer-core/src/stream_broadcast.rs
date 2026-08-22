//! Global subscribers for chat stream events (desktop UI + IM channel mirror).

use parking_lot::Mutex;
use std::sync::{Arc, OnceLock};

use crate::models::{ChatStreamSender, StreamEvent};

/// One broadcast frame: event plus routing ids (deltas may omit `conversationId`).
#[derive(Clone, Debug)]
pub struct StreamBroadcastItem {
    pub conversation_id: Option<String>,
    pub session_user_id: Option<String>,
    pub event: StreamEvent,
}

type StreamSubscriber = Arc<dyn Fn(StreamBroadcastItem) + Send + Sync>;

static SUBSCRIBERS: OnceLock<Mutex<Vec<StreamSubscriber>>> = OnceLock::new();

fn subscribers() -> &'static Mutex<Vec<StreamSubscriber>> {
    SUBSCRIBERS.get_or_init(|| Mutex::new(Vec::new()))
}

fn nonempty(id: &str) -> Option<String> {
    let trimmed = id.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn resolve_conversation_id(sender: Option<&ChatStreamSender>, ev: &StreamEvent) -> Option<String> {
    ev.conversation_id_for_sse()
        .map(str::to_string)
        .or_else(|| sender.and_then(|s| nonempty(s.conversation_id())))
}

fn resolve_session_user_id(
    sender: Option<&ChatStreamSender>,
    conversation_id: Option<&str>,
) -> Option<String> {
    if let Some(uid) = sender.and_then(|s| nonempty(s.session_user_id())) {
        return Some(uid);
    }
    let cid = conversation_id?;
    nonempty(&crate::user_storage::session_user_id_for_conversation(cid))
}

fn dispatch_item(item: StreamBroadcastItem) {
    for sub in subscribers().lock().iter() {
        sub(item.clone());
    }
}

pub fn subscribe_stream(handler: StreamSubscriber) {
    subscribers().lock().push(handler);
}

/// Deliver a stream event to UI subscribers. Prefer [`publish_stream`] during a chat run.
pub fn broadcast_stream(ev: &StreamEvent) {
    let conversation_id = resolve_conversation_id(None, ev);
    let session_user_id = resolve_session_user_id(None, conversation_id.as_deref());
    dispatch_item(StreamBroadcastItem {
        conversation_id,
        session_user_id,
        event: ev.clone(),
    });
}

/// Deliver a non-chat event (such as a persistent console PTY update) to every
/// host subscriber. Unlike [`publish_stream`], this does not require an active
/// chat run or its per-run mpsc sender.
pub fn publish_global_stream(ev: StreamEvent) {
    broadcast_stream(&ev);
}

/// Deliver a stream event to UI subscribers and the per-run mpsc sink (web SSE forward).
pub fn publish_stream(tx: &ChatStreamSender, ev: StreamEvent) {
    let conversation_id = resolve_conversation_id(Some(tx), &ev);
    let session_user_id = resolve_session_user_id(Some(tx), conversation_id.as_deref());
    dispatch_item(StreamBroadcastItem {
        conversation_id,
        session_user_id,
        event: ev.clone(),
    });
    if tx.send(ev).is_err() {
        log::warn!("stream event not delivered (stream receiver dropped)");
    }
}

/// SSE fan-out: `global` is process-global events plus the viewer's own chats;
/// a concrete conversation id only receives that conversation.
pub fn sse_subscription_matches(
    subscription: &str,
    viewer_session_user_id: &str,
    envelope_conversation_id: Option<&str>,
    envelope_session_user_id: Option<&str>,
    event: &StreamEvent,
) -> bool {
    let conv = nonempty(envelope_conversation_id.unwrap_or(""))
        .or_else(|| event.conversation_id_for_sse().map(str::to_string));
    let owner = nonempty(envelope_session_user_id.unwrap_or(""));
    let sub = subscription.trim();
    if sub == "global" {
        return match conv {
            None => true,
            Some(_) => {
                let viewer = viewer_session_user_id.trim();
                !viewer.is_empty() && owner.as_deref() == Some(viewer)
            }
        };
    }
    conv.as_deref() == Some(sub)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toast(conversation_id: &str) -> StreamEvent {
        StreamEvent::UiToast {
            conversation_id: conversation_id.into(),
            message: "x".into(),
            level: "info".into(),
        }
    }

    fn delta() -> StreamEvent {
        StreamEvent::Delta {
            message_id: "m1".into(),
            text: "hi".into(),
        }
    }

    #[test]
    fn per_conversation_subscription_denies_other_ids() {
        let ev = toast("conv-a");
        assert!(sse_subscription_matches(
            "conv-a",
            "user-a",
            Some("conv-a"),
            Some("user-a"),
            &ev
        ));
        assert!(!sse_subscription_matches(
            "conv-b",
            "user-a",
            Some("conv-a"),
            Some("user-a"),
            &ev
        ));
        assert!(!sse_subscription_matches(
            "conv-b",
            "user-b",
            Some("conv-a"),
            Some("user-a"),
            &delta()
        ));
    }

    #[test]
    fn global_keeps_process_wide_events() {
        let pairing = StreamEvent::ChannelPairingPending {
            channel: "wecom".into(),
            account_id: "acc".into(),
            code: "1".into(),
            sender_id: "s".into(),
            issued_at: 0,
        };
        assert!(sse_subscription_matches(
            "global", "user-a", None, None, &pairing
        ));
        assert!(sse_subscription_matches("global", "", None, None, &pairing));
        assert!(!sse_subscription_matches(
            "conv-a", "user-a", None, None, &pairing
        ));
    }

    #[test]
    fn global_only_delivers_viewer_owned_chat_events() {
        let ev = delta();
        assert!(sse_subscription_matches(
            "global",
            "user-a",
            Some("conv-a"),
            Some("user-a"),
            &ev
        ));
        assert!(!sse_subscription_matches(
            "global",
            "user-b",
            Some("conv-a"),
            Some("user-a"),
            &ev
        ));
        assert!(!sse_subscription_matches(
            "global",
            "",
            Some("conv-a"),
            Some("user-a"),
            &ev
        ));
    }

    #[test]
    fn empty_toast_conversation_is_process_global() {
        let ev = toast("");
        assert!(sse_subscription_matches(
            "global", "user-a", None, None, &ev
        ));
        assert!(!sse_subscription_matches(
            "conv-a", "user-a", None, None, &ev
        ));
    }
}
