use anyhow::Result;
use pointer_core::channel_outbound::{im_desktop_conversation_id, im_session_fork_title};
use pointer_core::conversation_store::im_session::ImSessionState;
use pointer_core::conversation_store::ConversationStore;
use pointer_core::models::{ConversationMeta, StreamEvent};

pub fn resolve_active_desktop_id(base_conv_id: &str, state: &ImSessionState) -> String {
    pointer_core::conversation_store::im_session::resolve_active_desktop_id(base_conv_id, state)
}

pub fn fork_im_desktop_session(
    store: &ConversationStore,
    base_conv_id: &str,
    session_state: &mut ImSessionState,
    sender_name: Option<&str>,
) -> Result<String> {
    session_state.session_epoch = session_state.session_epoch.saturating_add(1);
    let new_id = im_desktop_conversation_id(base_conv_id, session_state.session_epoch);
    session_state.active_conversation_id = Some(new_id.clone());
    store.save_im_session(base_conv_id, session_state)?;

    let previous_id = if session_state.session_epoch > 1 {
        im_desktop_conversation_id(base_conv_id, session_state.session_epoch - 1)
    } else {
        base_conv_id.to_string()
    };
    let session_user_id = store.session_user_id(&previous_id).unwrap_or_default();

    let title = im_session_fork_title(base_conv_id, sender_name, session_state.session_epoch)
        .unwrap_or_else(|| "新会话".to_string());
    let now = chrono::Utc::now().timestamp_millis();
    let lead_agent_id = session_state.lead_agent_id.clone();
    let agent_mode = session_state.agent_mode.clone();
    store.upsert_meta(&ConversationMeta {
        id: new_id.clone(),
        title: title.clone(),
        created_at: now,
        updated_at: now,
        is_pinned: false,
        skill_ids: vec![],
        tool_rounds_used: 0,
        tool_rounds_used_supervisor: 0,
        computer_monitor_id: None,
        project_id: None,
        workspace_root: String::new(),
        workspace_user_set: false,
        workspace_inherit_disabled: false,
        lead_agent_id: lead_agent_id.clone(),
        agent_mode: agent_mode.clone(),
        message_count: 0,
        preview: String::new(),
        session_user_id,
    })?;

    pointer_core::stream_broadcast::broadcast_stream(&StreamEvent::ImSessionForked {
        conversation_id: new_id.clone(),
        base_conversation_id: base_conv_id.to_string(),
        title,
        session_epoch: session_state.session_epoch,
        lead_agent_id,
        agent_mode,
    });

    Ok(new_id)
}
