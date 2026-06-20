//! Sub-agent scoped messages: linkage fields, DB load, per-row persist.

use crate::conversation_store;
use crate::models::{AgentTrace, ChatMessage, MessageContextState};

use super::conversation_persist;

#[derive(Debug, Clone)]
pub struct SubMessageLinkage {
    pub anchor_message_id: String,
    pub trace_id: String,
    pub task_id: String,
    pub spawn_depth: u32,
}

impl SubMessageLinkage {
    pub fn stamp(&self, msg: &mut ChatMessage) {
        msg.anchor_message_id = Some(self.anchor_message_id.clone());
        msg.trace_id = Some(self.trace_id.clone());
        msg.task_id = Some(self.task_id.clone());
        msg.spawn_depth = Some(self.spawn_depth);
        msg.context_state = Some(MessageContextState {
            included: false,
            excluded_reason: None,
        });
    }
}

pub fn is_scoped_sub_message(msg: &ChatMessage) -> bool {
    crate::models::is_scoped_sub_message(msg)
}

pub fn filter_scoped_messages(
    messages: &[ChatMessage],
    linkage: &SubMessageLinkage,
) -> Vec<ChatMessage> {
    messages
        .iter()
        .filter(|m| {
            m.anchor_message_id.as_deref() == Some(linkage.anchor_message_id.as_str())
                && m.trace_id.as_deref() == Some(linkage.trace_id.as_str())
        })
        .cloned()
        .collect()
}

pub fn load_scoped_transcript(
    conversation_id: &str,
    linkage: &SubMessageLinkage,
) -> anyhow::Result<Vec<ChatMessage>> {
    let store = conversation_store::global_store()?;
    let all = store.load_messages(conversation_id)?;
    Ok(filter_scoped_messages(&all, linkage))
}

pub fn persist_sub_message(conversation_id: &str, linkage: &SubMessageLinkage, msg: &ChatMessage) {
    let mut stamped = msg.clone();
    linkage.stamp(&mut stamped);
    conversation_persist::upsert_message(conversation_id, &stamped);
    log::debug!(
        "sub_message: persisted conversation_id={conversation_id} message_id={} trace_id={}",
        stamped.id,
        linkage.trace_id
    );
}

/// Re-upsert a scoped assistant row after tool outcome patches (reload-safe tool stats).
pub fn persist_scoped_assistant_snapshot(
    conversation_id: &str,
    linkage: &SubMessageLinkage,
    history: &[ChatMessage],
    scoped_message_id: &str,
) {
    let Some(msg) = history.iter().find(|m| m.id == scoped_message_id) else {
        log::warn!(
            "sub_message: scoped assistant missing for persist conversation_id={conversation_id} id={scoped_message_id}"
        );
        return;
    };
    persist_sub_message(conversation_id, linkage, msg);
}

/// Write the in-memory `agent_trace` index onto the lead anchor row (UI reload after restart).
pub fn sync_anchor_agent_trace_index(
    conversation_id: &str,
    history: &mut Vec<ChatMessage>,
    anchor_message_id: &str,
    agent_trace: &[AgentTrace],
) {
    if agent_trace.is_empty() {
        return;
    }
    let snapshot = history
        .iter_mut()
        .find(|m| m.id == anchor_message_id)
        .map(|msg| {
            msg.agent_trace = Some(agent_trace.to_vec());
            msg.clone()
        });
    let Some(msg) = snapshot else {
        log::warn!(
            "sub_message: sync agent_trace anchor missing conversation_id={conversation_id} anchor={anchor_message_id}"
        );
        return;
    };
    conversation_persist::upsert_message(conversation_id, &msg);
    log::debug!(
        "sub_message: synced agent_trace index conversation_id={conversation_id} anchor={anchor_message_id} traces={}",
        agent_trace.len()
    );
}

pub fn push_sub_tool_result(
    history: &mut Vec<ChatMessage>,
    conversation_id: &str,
    hint_message_id: &str,
    tool_call_id: &str,
    content: &str,
    linkage: &SubMessageLinkage,
) {
    crate::conversation_transcript::insert_tool_result_in_history(
        history,
        hint_message_id,
        tool_call_id,
        content,
    );
    let Some(tool_row) = history
        .iter()
        .find(|m| m.tool_call_id.as_deref() == Some(tool_call_id))
    else {
        log::warn!(
            "sub_message: tool result row missing after insert tool_call_id={tool_call_id}"
        );
        return;
    };
    persist_sub_message(conversation_id, linkage, tool_row);
}

/// Drop scoped rows from the lead transcript buffer (they live as linked siblings in DB).
pub fn strip_scoped_from_lead_history(history: &mut Vec<ChatMessage>) {
    let before = history.len();
    history.retain(|m| !is_scoped_sub_message(m));
    let removed = before.saturating_sub(history.len());
    if removed > 0 {
        log::info!("sub_message: stripped {removed} scoped rows from lead history");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Role;

    fn sample_msg(id: &str, anchor: Option<&str>, trace: Option<&str>) -> ChatMessage {
        ChatMessage {
            id: id.into(),
            role: Role::User,
            content: "hi".into(),
            status: "done".into(),
            created_at: 1,
            tool_calls: None,
            tool_call_id: None,
            error_message: None,
            reasoning: None,
            thoughts: None,
            headline: None,
            raw_content: None,
            tool_raw_output: None,
            agent_id: None,
            agent_instance_id: None,
            agent_name: None,
            agent_trace: None,
            image_slot_labels: None,
            images_base64: None,
            computer_round_screen_rel_path: None,
            ui_bindings: None,
            context_state: None,
            attachments: None,
            anchor_message_id: anchor.map(str::to_string),
            trace_id: trace.map(str::to_string),
            task_id: trace.map(|_| "task_a".to_string()),
            spawn_depth: Some(1),
        }
    }

    #[test]
    fn is_scoped_detects_anchor() {
        assert!(!is_scoped_sub_message(&sample_msg("m1", None, None)));
        assert!(is_scoped_sub_message(&sample_msg("m2", Some("anchor"), Some("t:a"))));
    }

    #[test]
    fn linkage_stamps_context_excluded() {
        let mut msg = sample_msg("m1", None, None);
        let link = SubMessageLinkage {
            anchor_message_id: "anchor".into(),
            trace_id: "task:explore".into(),
            task_id: "task".into(),
            spawn_depth: 1,
        };
        link.stamp(&mut msg);
        assert_eq!(msg.anchor_message_id.as_deref(), Some("anchor"));
        assert_eq!(msg.context_state.as_ref().map(|s| s.included), Some(false));
    }

    #[test]
    fn filter_scoped_messages_by_anchor_and_trace() {
        let link = SubMessageLinkage {
            anchor_message_id: "anchor".into(),
            trace_id: "task:explore".into(),
            task_id: "task".into(),
            spawn_depth: 1,
        };
        let loaded = filter_scoped_messages(
            &[
                sample_msg("lead", None, None),
                sample_msg("sub1", Some("anchor"), Some("task:explore")),
                sample_msg("other", Some("anchor"), Some("task:coder")),
            ],
            &link,
        );
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, "sub1");
    }

    #[test]
    fn sync_anchor_agent_trace_updates_lead_row() {
        use crate::models::Role;
        let mut history = vec![ChatMessage {
            id: "lead".into(),
            role: Role::Assistant,
            content: "ok".into(),
            status: "done".into(),
            created_at: 1,
            tool_calls: None,
            tool_call_id: None,
            error_message: None,
            reasoning: None,
            thoughts: None,
            headline: None,
            raw_content: None,
            tool_raw_output: None,
            agent_id: None,
            agent_instance_id: None,
            agent_name: None,
            agent_trace: None,
            image_slot_labels: None,
            images_base64: None,
            computer_round_screen_rel_path: None,
            ui_bindings: None,
            context_state: None,
            attachments: None,
            anchor_message_id: None,
            trace_id: None,
            task_id: None,
            spawn_depth: None,
        }];
        let traces = vec![AgentTrace {
            id: "task:explore".into(),
            name: "Explore".into(),
            role: "worker".into(),
            status: "completed".into(),
            detail: None,
            content: None,
            depth: Some(1),
            session: None,
            collapsed: true,
            user_expanded: false,
            computer_target: None,
        }];
        // DB may be unavailable in unit tests; history mutation is still required.
        sync_anchor_agent_trace_index("conv", &mut history, "lead", &traces);
        assert_eq!(history[0].agent_trace.as_ref().map(|t| t.len()), Some(1));
        assert_eq!(history[0].agent_trace.as_ref().unwrap()[0].id, "task:explore");
    }
}
