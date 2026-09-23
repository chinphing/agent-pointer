//! Continue a finished sub-agent on the same thread (Codex `followup_task`).
//!
//! The parent, while talking to the user, passes `followupInstanceId`.
//! A new job starts; scoped history for that `agentInstanceId` is restored.

use crate::agent_instance_scope::AgentInstanceScope;
use crate::models::{ChatMessage, Role};

use super::app_state::AppState;
use super::job_supervisor::FollowupReserve;

/// Transcript restored for a follow-up. Fresh rows are persisted only once the
/// worker loop actually starts, under that run's host message.
#[derive(Clone)]
pub(crate) struct ResumedWorkerHistory {
    pub messages: Vec<ChatMessage>,
    pub fresh_message_ids: Vec<String>,
}

pub(super) struct PreparedWorkerFollowup {
    pub instance_scope: AgentInstanceScope,
    pub history: ResumedWorkerHistory,
    pub task_id: String,
    pub spawn_depth: u32,
    pub reserve: Option<FollowupReserve>,
}

pub(super) fn prepare_worker_followup(
    state: &AppState,
    conversation_id: &str,
    run_id: &str,
    instance_id: &str,
    expected_agent_id: &str,
    parent_agent_id: &str,
    goal: &str,
    context: &str,
) -> Result<PreparedWorkerFollowup, String> {
    let instance_id = instance_id.trim();
    if instance_id.is_empty() {
        let msg = "followupInstanceId is empty".to_string();
        log::warn!("worker_followup: {msg} conversation_id={conversation_id}");
        return Err(msg);
    }
    let Some(reserve) = state
        .jobs
        .try_reserve_followup(conversation_id, instance_id)
    else {
        let msg = format!(
            "worker {instance_id} is still running; wait for it to finish before followupInstanceId"
        );
        log::info!(
            "worker_followup: refused busy conversation_id={conversation_id} agent_instance_id={instance_id}"
        );
        return Err(msg);
    };
    let loaded = match state.session_index.load_scoped_sub_messages_for_trace(
        conversation_id,
        "",
        "",
        Some(instance_id),
    ) {
        Ok(rows) => rows,
        Err(err) => {
            log::error!(
                "worker_followup: load failed conversation_id={conversation_id} agent_instance_id={instance_id}: {err:#}"
            );
            return Err(format!(
                "could not load worker transcript for followupInstanceId {instance_id}"
            ));
        }
    };
    if loaded.is_empty() {
        let msg = format!("no worker transcript for followupInstanceId {instance_id}");
        log::warn!("worker_followup: {msg} conversation_id={conversation_id}");
        return Err(msg);
    }
    let stored_agent = loaded.iter().rev().find_map(|m| {
        m.agent_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    });
    if let Some(stored) = stored_agent.as_deref() {
        if !followup_agent_matches(expected_agent_id, stored, parent_agent_id) {
            let msg = format!(
                "followupInstanceId {instance_id} belongs to {stored}, not {expected_agent_id}"
            );
            log::warn!("worker_followup: {msg} conversation_id={conversation_id}");
            return Err(msg);
        }
    }
    let task_id = loaded
        .iter()
        .find_map(|m| {
            m.task_id
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        })
        .unwrap_or_else(|| {
            log::warn!(
                "worker_followup: transcript has no task id conversation_id={conversation_id} agent_instance_id={instance_id}"
            );
            format!("followup_{instance_id}")
        });
    let spawn_depth = loaded
        .iter()
        .find_map(|m| m.spawn_depth)
        .unwrap_or_else(|| {
            log::warn!(
                "worker_followup: transcript has no spawn depth conversation_id={conversation_id} agent_instance_id={instance_id}; using 1"
            );
            1
        });
    let role = stored_agent
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| expected_agent_id.to_string());
    let original_goal = original_assigned_goal(state, conversation_id, instance_id);
    let (messages, fresh) =
        resume_history_from_loaded(loaded, goal, context, original_goal.as_deref());
    let fresh_message_ids = fresh.iter().map(|msg| msg.id.clone()).collect();
    log::info!(
        "worker_followup: resume conversation_id={conversation_id} agent_instance_id={instance_id} task_id={task_id} history_messages={} agent_id={role}",
        messages.len()
    );
    Ok(PreparedWorkerFollowup {
        instance_scope: AgentInstanceScope::with_instance_id(
            run_id,
            conversation_id,
            role,
            instance_id,
        ),
        history: ResumedWorkerHistory {
            messages,
            fresh_message_ids,
        },
        task_id,
        spawn_depth,
        reserve: Some(reserve),
    })
}

/// `self` continues a fork whose stored id is the parent role, not the literal `self`.
fn followup_agent_matches(expected: &str, stored: &str, parent_agent_id: &str) -> bool {
    if expected == stored {
        return true;
    }
    let parent = parent_agent_id.trim();
    expected == "self" && !parent.is_empty() && stored == parent
}

fn original_assigned_goal(
    state: &AppState,
    conversation_id: &str,
    instance_id: &str,
) -> Option<String> {
    let tool_msg = match state
        .session_index
        .load_first_tool_message_containing(conversation_id, instance_id)
    {
        Ok(Some(msg)) => msg,
        Ok(None) => {
            log::warn!(
                "worker_followup: original goal not found conversation_id={conversation_id} agent_instance_id={instance_id}"
            );
            return None;
        }
        Err(err) => {
            log::warn!(
                "worker_followup: load tool message for original goal failed conversation_id={conversation_id} agent_instance_id={instance_id}: {err:#}"
            );
            return None;
        }
    };
    let Some(tool_call_id) = tool_msg.tool_call_id.filter(|id| !id.trim().is_empty()) else {
        log::warn!(
            "worker_followup: tool row has no tool_call_id conversation_id={conversation_id} agent_instance_id={instance_id} message_id={}",
            tool_msg.id
        );
        return None;
    };
    let host = match state.session_index.load_assistant_message_with_tool_call(
        conversation_id,
        &tool_call_id,
        "run_subagent",
    ) {
        Ok(Some(msg)) => msg,
        Ok(None) => {
            log::warn!(
                "worker_followup: original goal not found conversation_id={conversation_id} agent_instance_id={instance_id}"
            );
            return None;
        }
        Err(err) => {
            log::warn!(
                "worker_followup: load host tool call for original goal failed conversation_id={conversation_id} tool_call_id={tool_call_id}: {err:#}"
            );
            return None;
        }
    };
    let goal = host
        .tool_calls
        .as_ref()
        .and_then(|calls| {
            calls
                .iter()
                .find(|call| call.id == tool_call_id && call.name == "run_subagent")
        })
        .and_then(|call| {
            serde_json::from_str::<serde_json::Value>(&call.arguments)
                .ok()
                .and_then(|value| {
                    value
                        .get("goal")
                        .and_then(|v| v.as_str())
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_string)
                })
        });
    if goal.is_none() {
        log::warn!(
            "worker_followup: original run_subagent has no goal conversation_id={conversation_id} tool_call_id={tool_call_id}"
        );
    }
    goal
}

/// Close tool calls that never received a result, then append the follow-up instruction.
/// Returns `(full history, rows that were not in the loaded transcript)`.
pub(super) fn resume_history_from_loaded(
    mut history: Vec<ChatMessage>,
    goal: &str,
    context: &str,
    original_goal: Option<&str>,
) -> (Vec<ChatMessage>, Vec<ChatMessage>) {
    let mut fresh = close_open_tool_calls(&mut history);
    let mut instruction =
        ChatMessage::user_text(followup_user_content(goal, context, original_goal));
    instruction.agent_instance_id = history.iter().find_map(|m| m.agent_instance_id.clone());
    fresh.push(instruction.clone());
    history.push(instruction);
    (history, fresh)
}

fn followup_user_content(goal: &str, context: &str, original_goal: Option<&str>) -> String {
    let mut out = String::from(
        "Continue this same worker. The transcript above is your earlier work on this task.\n\
         Apply the new instruction. Do not restart from scratch.\n",
    );
    if let Some(original) = original_goal.map(str::trim).filter(|s| !s.is_empty()) {
        out.push_str("\nOriginal assigned task:\n");
        out.push_str(original);
        out.push('\n');
    }
    out.push_str("\nNew instruction:\n");
    out.push_str(goal.trim());
    let context = context.trim();
    if !context.is_empty() {
        out.push_str("\n\nContext:\n");
        out.push_str(context);
    }
    out
}

fn close_open_tool_calls(history: &mut Vec<ChatMessage>) -> Vec<ChatMessage> {
    let mut inserted = Vec::new();
    let mut i = 0;
    while i < history.len() {
        let calls = match history[i].tool_calls.clone() {
            Some(calls) if matches!(history[i].role, Role::Assistant) && !calls.is_empty() => calls,
            _ => {
                i += 1;
                continue;
            }
        };
        let mut j = i + 1;
        let mut seen = std::collections::HashSet::new();
        while j < history.len() && matches!(history[j].role, Role::Tool) {
            if let Some(id) = history[j].tool_call_id.clone() {
                seen.insert(id);
            }
            j += 1;
        }
        for call in calls {
            if call.id.trim().is_empty() || seen.contains(&call.id) {
                continue;
            }
            let mut msg = ChatMessage::user_text(
                "Interrupted before a result was returned. Continue from the transcript above.",
            );
            msg.role = Role::Tool;
            msg.tool_call_id = Some(call.id.clone());
            msg.tool_name = Some(call.name.clone());
            history.insert(j, msg.clone());
            inserted.push(msg);
            j += 1;
        }
        i = j;
    }
    inserted
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assistant_with_tool(id: &str) -> ChatMessage {
        let mut msg = ChatMessage::user_text("calling");
        msg.role = Role::Assistant;
        msg.tool_calls = Some(vec![crate::models::ToolCall {
            id: id.into(),
            name: "file".into(),
            arguments: "{}".into(),
            status: "running".into(),
            result: None,
            error: None,
            duration_ms: None,
            risk_level: None,
            display_label: None,
            display_summary: None,
        }]);
        msg.agent_id = Some("explore".into());
        msg.task_id = Some("task-1".into());
        msg.spawn_depth = Some(1);
        msg
    }

    #[test]
    fn resume_closes_open_tool_call_and_appends_instruction() {
        let prior = assistant_with_tool("call-1");
        let (history, fresh) = resume_history_from_loaded(vec![prior], "fix the path", "", None);
        assert_eq!(history.len(), 3);
        assert!(matches!(history[1].role, Role::Tool));
        assert_eq!(history[1].tool_call_id.as_deref(), Some("call-1"));
        assert!(matches!(history[2].role, Role::User));
        assert!(history[2].content.contains("fix the path"));
        assert!(history[2].content.contains("Do not restart from scratch"));
        assert_eq!(fresh.len(), 2);
    }

    #[test]
    fn resume_keeps_answered_tool_calls() {
        let mut tool = ChatMessage::user_text("ok");
        tool.role = Role::Tool;
        tool.tool_call_id = Some("call-1".into());
        let (history, fresh) = resume_history_from_loaded(
            vec![assistant_with_tool("call-1"), tool],
            "next",
            "note",
            None,
        );
        assert_eq!(
            history
                .iter()
                .filter(|m| matches!(m.role, Role::Tool))
                .count(),
            1
        );
        assert_eq!(fresh.len(), 1);
        assert!(history.last().unwrap().content.contains("note"));
    }

    #[test]
    fn self_followup_matches_only_the_parent_role() {
        assert!(followup_agent_matches("self", "general", "general"));
        assert!(followup_agent_matches("explore", "explore", "general"));
        assert!(followup_agent_matches("reviewer", "reviewer", "general"));
        assert!(!followup_agent_matches("self", "explore", "general"));
        assert!(!followup_agent_matches("self", "reviewer", "general"));
        assert!(!followup_agent_matches("coder", "explore", "general"));
    }

    #[test]
    fn resume_keeps_the_original_assigned_task() {
        let (history, _) = resume_history_from_loaded(
            vec![assistant_with_tool("call-1")],
            "fix the path",
            "",
            Some("What: map login\nDone when: file is cited"),
        );
        let last = history.last().unwrap().content.as_str();
        assert!(last.contains("Original assigned task:"));
        assert!(last.contains("map login"));
        assert!(last.contains("fix the path"));
    }
}
