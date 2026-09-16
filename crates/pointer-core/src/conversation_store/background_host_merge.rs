//! Keep terminal background-host tool rows when a later short-list sync
//! still carries the in-memory spawn snapshot (`running`).

use crate::models::{ChatMessage, Role, ToolCall};

pub(crate) fn merge_incoming_over_stored(
    incoming: &ChatMessage,
    stored: &ChatMessage,
) -> ChatMessage {
    let mut out = incoming.clone();
    match incoming.role {
        Role::Assistant => {
            if let (Some(incoming_calls), Some(stored_calls)) =
                (incoming.tool_calls.as_ref(), stored.tool_calls.as_ref())
            {
                let (merged, kept) = merge_tool_calls(incoming_calls, stored_calls);
                if kept > 0 {
                    log::info!(
                        "conversation_store: kept {} terminal background host tool(s) message_id={}",
                        kept,
                        incoming.id
                    );
                    out.tool_calls = Some(merged);
                }
            }
            if !has_in_progress_tool(&out) && is_open_message_status(&out.status) {
                out.status = if stored.status == "done" || stored.status == "completed" {
                    stored.status.clone()
                } else {
                    "done".into()
                };
            }
        }
        Role::Tool => {
            if let Some(kept) = keep_terminal_job_handle(&incoming.content, &stored.content) {
                log::info!(
                    "conversation_store: kept terminal background handle message_id={} tool_call_id={}",
                    incoming.id,
                    incoming.tool_call_id.as_deref().unwrap_or("")
                );
                out.content = kept;
            }
        }
        _ => {}
    }
    out
}

fn merge_tool_calls(incoming: &[ToolCall], stored: &[ToolCall]) -> (Vec<ToolCall>, usize) {
    let mut kept = 0usize;
    let merged = incoming
        .iter()
        .map(|inc| {
            let Some(prev) = stored.iter().find(|s| s.id == inc.id) else {
                return inc.clone();
            };
            if should_keep_stored_host(inc, prev) {
                kept += 1;
                prev.clone()
            } else {
                inc.clone()
            }
        })
        .collect();
    (merged, kept)
}

fn should_keep_stored_host(incoming: &ToolCall, stored: &ToolCall) -> bool {
    if !(is_background_host_tool(incoming) || is_background_host_tool(stored)) {
        return false;
    }
    is_in_progress_status(&incoming.status) && is_terminal_status(&stored.status)
}

fn is_background_host_tool(tc: &ToolCall) -> bool {
    if looks_like_job_handle(tc.result.as_deref()) {
        return true;
    }
    let name = tc.name.rsplit('.').next().unwrap_or(tc.name.as_str());
    let Ok(args) = serde_json::from_str::<serde_json::Value>(&tc.arguments) else {
        return false;
    };
    if name == "run_subagent" {
        return args.get("background") == Some(&serde_json::Value::Bool(true));
    }
    if name == "terminal" {
        return args
            .get("blockUntilMs")
            .and_then(|v| v.as_f64())
            .is_some_and(|n| n >= 0.0);
    }
    false
}

fn looks_like_job_handle(raw: Option<&str>) -> bool {
    let Some(raw) = raw.filter(|s| !s.trim().is_empty()) else {
        return false;
    };
    job_handle_status(raw).is_some()
}

fn job_handle_status(raw: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(raw).ok()?;
    let obj = v.as_object()?;
    let job_id = obj.get("jobId")?.as_str()?.trim();
    let kind = obj.get("kind")?.as_str()?.trim();
    if job_id.is_empty() || (kind != "subagent" && kind != "terminal") {
        return None;
    }
    if obj.contains_key("stdout") || obj.contains_key("exitCode") || obj.contains_key("content") {
        return None;
    }
    Some(
        obj.get("status")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase(),
    )
}

fn keep_terminal_job_handle(incoming: &str, stored: &str) -> Option<String> {
    let incoming_status = job_handle_status(incoming)?;
    let stored_status = job_handle_status(stored)?;
    if is_in_progress_status(&incoming_status) && is_terminal_job_handle(&stored_status) {
        Some(stored.to_string())
    } else {
        None
    }
}

fn is_in_progress_status(status: &str) -> bool {
    matches!(
        status.trim().to_ascii_lowercase().as_str(),
        "running" | "pending" | "pending_approval"
    )
}

fn is_terminal_status(status: &str) -> bool {
    matches!(
        status.trim().to_ascii_lowercase().as_str(),
        "success" | "failed" | "rejected"
    )
}

fn is_terminal_job_handle(status: &str) -> bool {
    matches!(status, "completed" | "failed" | "cancelled" | "canceled")
}

fn is_open_message_status(status: &str) -> bool {
    matches!(status, "streaming" | "pending")
}

fn has_in_progress_tool(msg: &ChatMessage) -> bool {
    msg.tool_calls
        .as_ref()
        .is_some_and(|calls| calls.iter().any(|tc| is_in_progress_status(&tc.status)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(status: &str, handle_status: &str) -> ToolCall {
        ToolCall {
            id: "call_bg".into(),
            name: "run_subagent".into(),
            arguments: r#"{"agentId":"explore","background":true,"goal":"x"}"#.into(),
            status: status.into(),
            result: Some(format!(
                r#"{{"jobId":"job_1","status":"{handle_status}","kind":"subagent"}}"#
            )),
            error: None,
            duration_ms: None,
            risk_level: None,
            display_label: None,
            display_summary: None,
        }
    }

    fn assistant(calls: Vec<ToolCall>, status: &str) -> ChatMessage {
        ChatMessage {
            id: "msg_host".into(),
            role: Role::Assistant,
            content: "go".into(),
            status: status.into(),
            created_at: 1,
            tool_calls: Some(calls),
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
            images_base64: None,
            image_slot_labels: None,
            computer_round_screen_rel_path: None,
            ui_bindings: None,
            context_state: None,
            attachments: None,
            anchor_message_id: None,
            trace_id: None,
            task_id: None,
            spawn_depth: None,
        }
    }

    #[test]
    fn short_list_running_does_not_clobber_persisted_success() {
        let stored = assistant(vec![host("success", "completed")], "done");
        let incoming = assistant(vec![host("running", "running")], "streaming");
        let merged = merge_incoming_over_stored(&incoming, &stored);
        let tc = &merged.tool_calls.as_ref().unwrap()[0];
        assert_eq!(tc.status, "success");
        assert!(tc.result.as_deref().unwrap().contains("completed"));
        assert_eq!(merged.status, "done");
    }

    #[test]
    fn persist_success_overwrites_running() {
        let stored = assistant(vec![host("running", "running")], "streaming");
        let incoming = assistant(vec![host("success", "completed")], "streaming");
        let merged = merge_incoming_over_stored(&incoming, &stored);
        assert_eq!(merged.tool_calls.as_ref().unwrap()[0].status, "success");
    }

    #[test]
    fn ordinary_tool_running_still_updates() {
        let mut stored_tc = host("success", "completed");
        stored_tc.name = "file_read".into();
        stored_tc.arguments = r#"{"path":"a"}"#.into();
        stored_tc.result = Some("ok".into());
        let mut incoming_tc = stored_tc.clone();
        incoming_tc.status = "running".into();
        incoming_tc.result = None;
        let stored = assistant(vec![stored_tc], "done");
        let incoming = assistant(vec![incoming_tc], "streaming");
        let merged = merge_incoming_over_stored(&incoming, &stored);
        assert_eq!(merged.tool_calls.as_ref().unwrap()[0].status, "running");
    }

    #[test]
    fn tool_row_keeps_completed_handle() {
        let mut stored = assistant(vec![], "completed");
        stored.role = Role::Tool;
        stored.id = "tool_call_bg".into();
        stored.tool_call_id = Some("call_bg".into());
        stored.content = r#"{"jobId":"job_1","status":"completed","kind":"subagent"}"#.into();
        let mut incoming = stored.clone();
        incoming.content = r#"{"jobId":"job_1","status":"running","kind":"subagent"}"#.into();
        let merged = merge_incoming_over_stored(&incoming, &stored);
        assert!(merged.content.contains("completed"));
    }
}
