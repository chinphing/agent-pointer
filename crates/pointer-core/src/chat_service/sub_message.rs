//! Sub-agent scoped messages: linkage fields, filtering, and per-row persistence.

use crate::models::{AgentTrace, ChatMessage, MessageContextState};

use super::conversation_persist;

#[derive(Debug, Clone)]
pub struct SubMessageLinkage {
    pub anchor_message_id: String,
    pub trace_id: String,
    pub task_id: String,
    pub spawn_depth: u32,
    pub agent_instance_id: String,
}

impl SubMessageLinkage {
    pub fn stamp(&self, msg: &mut ChatMessage) {
        msg.anchor_message_id = Some(self.anchor_message_id.clone());
        msg.trace_id = Some(self.trace_id.clone());
        msg.task_id = Some(self.task_id.clone());
        msg.spawn_depth = Some(self.spawn_depth);
        msg.agent_instance_id = Some(self.agent_instance_id.clone());
        msg.context_state = Some(MessageContextState {
            included: false,
            excluded_reason: None,
        });
    }
}

pub fn is_scoped_sub_message(msg: &ChatMessage) -> bool {
    crate::models::is_scoped_sub_message(msg)
}

#[cfg(test)]
/// Strip DB-only scoped linkage so sub-agent loop memory matches lead loop (unstamped rows).
fn clear_scoped_linkage_for_loop(msg: &mut ChatMessage) {
    msg.anchor_message_id = None;
    msg.trace_id = None;
    msg.task_id = None;
    msg.spawn_depth = None;
    msg.agent_instance_id = None;
    if let Some(state) = msg.context_state.as_ref() {
        if !state.included && state.excluded_reason.is_none() {
            msg.context_state = None;
        }
    }
}

#[cfg(test)]
fn filter_scoped_messages(
    messages: &[ChatMessage],
    linkage: &SubMessageLinkage,
) -> Vec<ChatMessage> {
    messages
        .iter()
        .filter(|m| {
            m.anchor_message_id.as_deref() == Some(linkage.anchor_message_id.as_str())
                && m.trace_id.as_deref() == Some(linkage.trace_id.as_str())
                && m.agent_instance_id.as_deref() == Some(linkage.agent_instance_id.as_str())
        })
        .cloned()
        .collect()
}

pub fn persist_sub_message(conversation_id: &str, linkage: &SubMessageLinkage, msg: &ChatMessage) {
    let mut stamped = msg.clone();
    linkage.stamp(&mut stamped);
    conversation_persist::upsert_message(conversation_id, &stamped);
    log::debug!(
        "sub_message: persisted conversation_id={conversation_id} message_id={} trace_id={} agent_instance_id={}",
        stamped.id,
        linkage.trace_id,
        linkage.agent_instance_id
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

/// Persist the lead `agent_trace` index only for terminal UI states.
/// Running detail stays in SSE / in-memory `ctx.agent_trace` — writing the
/// whole lead row on every spawn step serializes 100 workers onto one SQLite row.
pub fn should_persist_anchor_agent_trace(status: &str) -> bool {
    matches!(
        status.trim(),
        "completed" | "failed" | "cancelled" | "error"
    )
}

/// Write `agent_trace` onto the in-memory lead row and coalesce SQLite patches.
/// Many parallel terminals share one lead message: debounce ~400ms, then
/// `json_set` only `payload.agentTrace` (no full ChatMessage clone).
pub fn sync_anchor_agent_trace_index(
    conversation_id: &str,
    history: &mut Vec<ChatMessage>,
    anchor_message_id: &str,
    agent_trace: &[AgentTrace],
) {
    if agent_trace.is_empty() {
        return;
    }
    let Some(msg) = history.iter_mut().find(|m| m.id == anchor_message_id) else {
        log::warn!(
            "sub_message: sync agent_trace anchor missing conversation_id={conversation_id} anchor={anchor_message_id}"
        );
        return;
    };
    msg.agent_trace = Some(agent_trace.to_vec());
    let json = match serde_json::to_string(agent_trace) {
        Ok(s) => s,
        Err(e) => {
            log::warn!(
                "sub_message: serialize agent_trace failed conversation_id={conversation_id}: {e}"
            );
            return;
        }
    };
    schedule_anchor_agent_trace_persist(conversation_id, anchor_message_id, json);
}

const ANCHOR_TRACE_PERSIST_COALESCE_MS: u64 = 400;

fn pending_anchor_traces(
) -> &'static std::sync::Mutex<std::collections::HashMap<String, (String, String, String)>> {
    static MAP: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<String, (String, String, String)>>,
    > = std::sync::OnceLock::new();
    MAP.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

fn persist_worker_scheduled() -> &'static std::sync::Mutex<bool> {
    static FLAG: std::sync::OnceLock<std::sync::Mutex<bool>> = std::sync::OnceLock::new();
    FLAG.get_or_init(|| std::sync::Mutex::new(false))
}

fn schedule_anchor_agent_trace_persist(
    conversation_id: &str,
    anchor_message_id: &str,
    traces_json: String,
) {
    let key = format!("{conversation_id}\0{anchor_message_id}");
    match pending_anchor_traces().lock() {
        Ok(mut map) => {
            map.insert(
                key,
                (
                    conversation_id.to_string(),
                    anchor_message_id.to_string(),
                    traces_json,
                ),
            );
        }
        Err(e) => {
            log::warn!("sub_message: pending agent_trace map poisoned: {e}");
            return;
        }
    }
    let mut scheduled = match persist_worker_scheduled().lock() {
        Ok(g) => g,
        Err(e) => {
            log::warn!("sub_message: persist worker flag poisoned: {e}");
            return;
        }
    };
    if *scheduled {
        return;
    }
    *scheduled = true;
    drop(scheduled);
    let spawn = std::thread::Builder::new()
        .name("pointer-agent-trace-persist".into())
        .spawn(|| {
            std::thread::sleep(std::time::Duration::from_millis(
                ANCHOR_TRACE_PERSIST_COALESCE_MS,
            ));
            flush_pending_anchor_agent_traces();
        });
    if let Err(e) = spawn {
        log::warn!("sub_message: failed to spawn agent_trace persist worker: {e}");
        if let Ok(mut flag) = persist_worker_scheduled().lock() {
            *flag = false;
        }
        flush_pending_anchor_agent_traces();
    }
}

fn flush_pending_anchor_agent_traces() {
    if let Ok(mut flag) = persist_worker_scheduled().lock() {
        *flag = false;
    }
    let batch = match pending_anchor_traces().lock() {
        Ok(mut map) => std::mem::take(&mut *map),
        Err(e) => {
            log::warn!("sub_message: pending agent_trace map poisoned on flush: {e}");
            return;
        }
    };
    for (conversation_id, anchor_message_id, traces_json) in batch.into_values() {
        conversation_persist::patch_anchor_agent_trace(
            &conversation_id,
            &anchor_message_id,
            &traces_json,
        );
        log::debug!(
            "sub_message: patched agent_trace index conversation_id={conversation_id} anchor={anchor_message_id}"
        );
    }
    let again = pending_anchor_traces()
        .lock()
        .ok()
        .is_some_and(|m| !m.is_empty());
    if again {
        if let Ok(mut flag) = persist_worker_scheduled().lock() {
            if !*flag {
                *flag = true;
                drop(flag);
                let _ = std::thread::Builder::new()
                    .name("pointer-agent-trace-persist".into())
                    .spawn(|| {
                        std::thread::sleep(std::time::Duration::from_millis(
                            ANCHOR_TRACE_PERSIST_COALESCE_MS,
                        ));
                        flush_pending_anchor_agent_traces();
                    });
            }
        }
    }
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
        log::warn!("sub_message: tool result row missing after insert tool_call_id={tool_call_id}");
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

    #[test]
    fn persist_anchor_agent_trace_only_for_terminal_status() {
        assert!(!should_persist_anchor_agent_trace("running"));
        assert!(!should_persist_anchor_agent_trace("queued"));
        assert!(should_persist_anchor_agent_trace("completed"));
        assert!(should_persist_anchor_agent_trace("failed"));
        assert!(should_persist_anchor_agent_trace("cancelled"));
    }

    fn sample_msg(id: &str, anchor: Option<&str>, trace: Option<&str>) -> ChatMessage {
        ChatMessage {
            id: id.into(),
            role: Role::User,
            content: "hi".into(),
            status: "done".into(),
            created_at: 1,
            tool_calls: None,
            tool_call_id: None,
            tool_name: None,
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
        assert!(is_scoped_sub_message(&sample_msg(
            "m2",
            Some("anchor"),
            Some("t:a")
        )));
    }

    #[test]
    fn linkage_stamps_context_excluded() {
        let mut msg = sample_msg("m1", None, None);
        let link = SubMessageLinkage {
            anchor_message_id: "anchor".into(),
            trace_id: "task:explore".into(),
            task_id: "task".into(),
            spawn_depth: 1,
            agent_instance_id: "instance-a".into(),
        };
        link.stamp(&mut msg);
        assert_eq!(msg.anchor_message_id.as_deref(), Some("anchor"));
        assert_eq!(msg.agent_instance_id.as_deref(), Some("instance-a"));
        assert_eq!(msg.context_state.as_ref().map(|s| s.included), Some(false));
    }

    #[test]
    fn filter_scoped_messages_by_anchor_and_trace() {
        let link = SubMessageLinkage {
            anchor_message_id: "anchor".into(),
            trace_id: "task:explore".into(),
            task_id: "task".into(),
            spawn_depth: 1,
            agent_instance_id: "instance-a".into(),
        };
        let mut matching = sample_msg("sub1", Some("anchor"), Some("task:explore"));
        matching.agent_instance_id = Some("instance-a".into());
        let mut other = sample_msg("other", Some("anchor"), Some("task:coder"));
        other.agent_instance_id = Some("instance-a".into());
        let loaded =
            filter_scoped_messages(&[sample_msg("lead", None, None), matching, other], &link);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, "sub1");
    }

    #[test]
    fn filter_scoped_messages_excludes_previous_instance_with_same_task() {
        let link = SubMessageLinkage {
            anchor_message_id: "anchor".into(),
            trace_id: "task:explore".into(),
            task_id: "task".into(),
            spawn_depth: 1,
            agent_instance_id: "instance-new".into(),
        };
        let mut old = sample_msg("old", Some("anchor"), Some("task:explore"));
        old.agent_instance_id = Some("instance-old".into());
        let mut current = sample_msg("current", Some("anchor"), Some("task:explore"));
        current.agent_instance_id = Some("instance-new".into());

        let loaded = filter_scoped_messages(&[old, current], &link);

        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, "current");
    }

    #[test]
    fn clear_scoped_linkage_restores_loop_inclusion() {
        let mut msg = sample_msg("m1", Some("anchor"), Some("task:explore"));
        msg.agent_instance_id = Some("instance-a".into());
        let link = SubMessageLinkage {
            anchor_message_id: "anchor".into(),
            trace_id: "task:explore".into(),
            task_id: "task".into(),
            spawn_depth: 1,
            agent_instance_id: "instance-a".into(),
        };
        link.stamp(&mut msg);
        assert!(!crate::message_context::is_context_included(&msg));
        clear_scoped_linkage_for_loop(&mut msg);
        assert!(msg.agent_instance_id.is_none());
        assert!(crate::message_context::is_context_included(&msg));
        assert!(crate::message_context::is_sub_agent_loop_included(&msg));
    }

    #[test]
    fn clear_scoped_linkage_keeps_compression_exclusion() {
        let mut msg = sample_msg("m1", Some("anchor"), Some("task:explore"));
        msg.context_state = Some(MessageContextState {
            included: false,
            excluded_reason: Some(crate::models::ExcludedReason::ContextCompression),
        });
        clear_scoped_linkage_for_loop(&mut msg);
        assert!(!crate::message_context::is_sub_agent_loop_included(&msg));
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
            tool_name: None,
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
            agent_instance_id: None,
            computer_target: None,
            parent_tool_call_id: None,
            anchor_message_id: None,
            summary_line: None,
            task_id: Some("task".into()),
            agent_id: Some("explore".into()),
            search_tool_call_ids: None,
        }];
        // DB may be unavailable in unit tests; history mutation is still required.
        sync_anchor_agent_trace_index("conv", &mut history, "lead", &traces);
        assert_eq!(history[0].agent_trace.as_ref().map(|t| t.len()), Some(1));
        assert_eq!(
            history[0].agent_trace.as_ref().unwrap()[0].id,
            "task:explore"
        );
    }
}
