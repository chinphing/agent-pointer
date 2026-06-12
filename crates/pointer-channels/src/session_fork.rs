use anyhow::Result;
use pointer_core::channel_outbound::{
    im_base_conversation_id, im_desktop_conversation_id, im_session_fork_title,
};
use pointer_core::conversation_store::ConversationStore;
use pointer_core::models::{ConversationMeta, StreamEvent};

use crate::inbound::{ChannelHistoryStore, ChannelSessionMeta};

pub fn resolve_active_desktop_id(base_conv_id: &str, meta: &ChannelSessionMeta) -> String {
    if let Some(active) = &meta.active_conversation_id {
        if im_base_conversation_id(active) == base_conv_id {
            return active.clone();
        }
    }
    im_desktop_conversation_id(base_conv_id, meta.session_epoch)
}

pub fn fork_im_desktop_session(
    history: &ChannelHistoryStore,
    store: &ConversationStore,
    base_conv_id: &str,
    session_meta: &mut ChannelSessionMeta,
    sender_name: Option<&str>,
) -> Result<String> {
    session_meta.session_epoch = session_meta.session_epoch.saturating_add(1);
    let new_id = im_desktop_conversation_id(base_conv_id, session_meta.session_epoch);
    session_meta.active_conversation_id = Some(new_id.clone());
    history.save_meta(base_conv_id, session_meta)?;

    let title = im_session_fork_title(base_conv_id, sender_name, session_meta.session_epoch)
        .unwrap_or_else(|| "新会话".to_string());
    let now = chrono::Utc::now().timestamp_millis();
    let lead_agent_id = session_meta.effective_lead_agent_id();
    let agent_mode = session_meta.effective_agent_mode();
    store.upsert_meta(&ConversationMeta {
        id: new_id.clone(),
        title: title.clone(),
        created_at: now,
        updated_at: now,
        skill_ids: vec![],
        tool_rounds_used: 0,
        tool_rounds_used_supervisor: 0,
        computer_monitor_id: None,
        workspace_root: String::new(),
        lead_agent_id: lead_agent_id.clone(),
        agent_mode: agent_mode.clone(),
    })?;

    pointer_core::stream_broadcast::broadcast_stream(&StreamEvent::ImSessionForked {
        conversation_id: new_id.clone(),
        base_conversation_id: base_conv_id.to_string(),
        title,
        session_epoch: session_meta.session_epoch,
        lead_agent_id,
        agent_mode,
    });

    Ok(new_id)
}
