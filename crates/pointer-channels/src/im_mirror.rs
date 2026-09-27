//! Mirror successful cron/IM deliveries into the target IM desktop transcript
//! so follow-up replies stay coherent (Hermes continuable / append-to-session).

use pointer_core::conversation_store::ConversationStore;
use pointer_core::models::{ChatMessage, Role, StreamEvent};
use pointer_core::stream_broadcast;

use crate::traits::OutboundContext;

const MIRROR_PREFIX: &str = "【定时投递】\n";

/// After a successful outbound deliver, append the visible text into the peer's
/// active IM session (DM only). Best-effort: never fails the delivery path.
pub fn mirror_delivery_to_im_session(
    store: &ConversationStore,
    out: &OutboundContext,
    visible: &str,
    run_id: &str,
    job_id: Option<&str>,
) {
    let visible = visible.trim();
    if visible.is_empty() {
        return;
    }
    if out.conversation_key.contains(":group:") {
        log::info!(
            "im_deliver mirror: skip group target channel={} recipient={}",
            out.channel,
            out.recipient_id
        );
        return;
    }

    let peer = out.recipient_id.trim();
    if peer.is_empty() {
        return;
    }

    let found = match store.find_im_desktop_for_channel_peer(
        &out.channel,
        &out.account_id,
        peer,
    ) {
        Ok(v) => v,
        Err(e) => {
            log::warn!(
                "im_deliver mirror: lookup failed channel={} peer={peer}: {e:#}",
                out.channel
            );
            return;
        }
    };
    let Some((base_id, desktop_id)) = found else {
        log::info!(
            "im_deliver mirror: no IM session for channel={} account={} peer={peer}; skip",
            out.channel,
            out.account_id
        );
        return;
    };

    let job_part = job_id
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("run");
    let message_id = format!("cron-deliver:{job_part}:{run_id}");
    let content = format!("{MIRROR_PREFIX}{visible}");
    let mut msg = ChatMessage::user_text(content.clone());
    msg.id = message_id.clone();
    msg.role = Role::Assistant;

    if let Err(e) =
        pointer_core::conversation_session::upsert_message_in_store(store, &desktop_id, &msg)
    {
        log::warn!(
            "im_deliver mirror: upsert failed desktop={desktop_id}: {e:#}"
        );
        return;
    }
    if let Err(e) = store.touch_im_interaction(&base_id) {
        log::warn!("im_deliver mirror: touch_im failed base={base_id}: {e:#}");
    }

    stream_broadcast::broadcast_stream(&StreamEvent::InjectedAssistantMessage {
        conversation_id: desktop_id.clone(),
        message_id,
        content,
    });
    log::info!(
        "im_deliver mirror: mirrored run_id={run_id} desktop={desktop_id} peer={peer}"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use pointer_core::conversation_store::ConversationStore;
    use tempfile::tempdir;

    fn open_store() -> (tempfile::TempDir, ConversationStore) {
        let dir = tempdir().unwrap();
        let store = ConversationStore::open(dir.path().join("conversations.db")).unwrap();
        (dir, store)
    }

    #[test]
    fn mirror_appends_assistant_into_peer_session() {
        let (_dir, store) = open_store();
        // Feishu inbound uses chat_id in conversation_key; home binding uses open_id.
        let base = "feishu:default:feishu:dm:oc_chat123:ou_user1";
        store
            .set_session_user_id(base, "ou_user1")
            .unwrap();
        let out = OutboundContext {
            channel: "feishu".into(),
            account_id: "default".into(),
            conversation_key: "feishu:dm:ou_user1".into(),
            recipient_id: "ou_user1".into(),
            reply_context: None,
        };
        mirror_delivery_to_im_session(
            &store,
            &out,
            "该喝水了",
            "run-1",
            Some("cron-abc"),
        );
        let msgs = store.load_messages(base).unwrap();
        assert_eq!(msgs.len(), 1);
        assert!(matches!(msgs[0].role, Role::Assistant));
        assert!(msgs[0].content.contains("该喝水了"));
        assert!(msgs[0].content.starts_with(MIRROR_PREFIX));
        assert_eq!(msgs[0].id, "cron-deliver:cron-abc:run-1");
    }

    #[test]
    fn mirror_skips_when_no_prior_im_session() {
        let (_dir, store) = open_store();
        let out = OutboundContext {
            channel: "feishu".into(),
            account_id: "default".into(),
            conversation_key: "feishu:dm:ou_nobody".into(),
            recipient_id: "ou_nobody".into(),
            reply_context: None,
        };
        mirror_delivery_to_im_session(&store, &out, "hello", "run-2", None);
        assert!(store
            .find_im_desktop_for_channel_peer("feishu", "default", "ou_nobody")
            .unwrap()
            .is_none());
    }

    #[test]
    fn mirror_skips_group_targets() {
        let (_dir, store) = open_store();
        let base = "feishu:default:feishu:group:oc_g1:ou_user1";
        store.set_session_user_id(base, "ou_user1").unwrap();
        let out = OutboundContext {
            channel: "feishu".into(),
            account_id: "default".into(),
            conversation_key: "feishu:group:oc_g1".into(),
            recipient_id: "ou_user1".into(),
            reply_context: None,
        };
        mirror_delivery_to_im_session(&store, &out, "群提醒", "run-3", None);
        assert!(store.load_messages(base).unwrap().is_empty());
    }
}
