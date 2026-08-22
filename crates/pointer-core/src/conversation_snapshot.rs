//! Build redacted conversation snapshots for token usage archives.

use anyhow::Result;
use serde_json::{json, Value};
use std::io::Write;

use crate::models::{ChatMessage, Role};

const MAX_CONTENT_CHARS: usize = 8_000;
const MAX_SNAPSHOT_BYTES: usize = 256 * 1024;

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    format!("{}…", s.chars().take(max).collect::<String>())
}

fn image_placeholders(msg: &ChatMessage) -> Option<Vec<String>> {
    let labels = msg.image_slot_labels.as_deref();
    let imgs = msg.images_base64.as_deref()?;
    if imgs.is_empty() {
        return None;
    }
    Some(
        imgs.iter()
            .enumerate()
            .map(|(i, _)| {
                if let Some(labels) = labels {
                    if let Some(lbl) = labels.get(i) {
                        if !lbl.trim().is_empty() {
                            return format!("[image:{lbl}]");
                        }
                    }
                }
                format!("[image:{}]", i + 1)
            })
            .collect(),
    )
}

fn message_to_snapshot(msg: &ChatMessage) -> Option<Value> {
    match msg.role {
        Role::System => None,
        Role::User => {
            let mut o = json!({
                "id": msg.id,
                "role": "user",
                "content": truncate_chars(&msg.content, MAX_CONTENT_CHARS),
                "created_at": msg.created_at,
            });
            if let Some(imgs) = image_placeholders(msg) {
                o["images"] = json!(imgs);
            } else if msg.computer_round_screen_rel_path.is_some() {
                o["images"] = json!(["[computer_screen]"]);
            }
            Some(o)
        }
        Role::Assistant => Some(json!({
            "id": msg.id,
            "role": "assistant",
            "content": truncate_chars(&msg.content, MAX_CONTENT_CHARS),
            "agent_role_id": msg.agent_id,
            "agent_instance_id": msg.agent_instance_id,
            "agent_name": msg.agent_name,
            "created_at": msg.created_at,
        })),
        Role::Tool => {
            let name = msg
                .tool_calls
                .as_ref()
                .and_then(|t| t.first())
                .map(|t| t.name.as_str())
                .unwrap_or("tool");
            let result = msg.content.trim();
            Some(json!({
                "id": msg.id,
                "role": "tool",
                "tool_name": name,
                "content": truncate_chars(result, 2_000),
                "created_at": msg.created_at,
            }))
        }
    }
}

/// Messages for one agent instance report (assistant lines + triggering user).
pub fn build_snapshot_messages(history: &[ChatMessage], agent_instance_id: &str) -> Vec<Value> {
    let mut out = Vec::new();
    let mut last_user: Option<Value> = None;
    for msg in history {
        if matches!(msg.role, Role::User) {
            if let Some(v) = message_to_snapshot(msg) {
                last_user = Some(v);
            }
            continue;
        }
        if matches!(msg.role, Role::Assistant) {
            if msg.agent_instance_id.as_deref() != Some(agent_instance_id) {
                continue;
            }
            if let Some(u) = last_user.take() {
                out.push(u);
            }
            if let Some(v) = message_to_snapshot(msg) {
                out.push(v);
            }
        }
    }
    out
}

pub fn build_snapshot_json(
    conversation_id: &str,
    agent_instance_id: &str,
    agent_role_id: &str,
    history: &[ChatMessage],
) -> Value {
    let mut messages = build_snapshot_messages(history, agent_instance_id);
    while serde_json::to_vec(&messages).map(|v| v.len()).unwrap_or(0) > MAX_SNAPSHOT_BYTES {
        if messages.is_empty() {
            break;
        }
        log::warn!(
            "conversation_snapshot: trim oldest message conversation_id={} agent_instance_id={}",
            conversation_id,
            agent_instance_id
        );
        messages.remove(0);
    }
    json!({
        "conversation_id": conversation_id,
        "agent_instance_id": agent_instance_id,
        "agent_role_id": agent_role_id,
        "messages": messages,
    })
}

pub fn write_snapshot_zip(path: &std::path::Path, snapshot: &Value) -> Result<()> {
    let file = std::fs::File::create(path)?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    zip.start_file("conversation_snapshot.json", options)?;
    let bytes = serde_json::to_vec_pretty(snapshot)?;
    zip.write_all(&bytes)?;
    zip.finish()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ChatMessage, Role};

    #[test]
    fn snapshot_filters_by_agent_instance_and_redacts_system() {
        let inst = "11111111-1111-1111-1111-111111111111";
        let other = "22222222-2222-2222-2222-222222222222";
        let history = vec![
            ChatMessage {
                id: "sys".into(),
                role: Role::System,
                content: "secret system".into(),
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
                tool_raw_output: None,
            },
            ChatMessage {
                id: "u1".into(),
                role: Role::User,
                content: "hello".into(),
                status: "done".into(),
                created_at: 2,
                tool_calls: None,
                tool_call_id: None,
                tool_name: None,
                error_message: None,
                reasoning: None,
                thoughts: None,
                headline: None,
                raw_content: None,
                agent_id: None,
                agent_instance_id: None,
                agent_name: None,
                agent_trace: None,
                image_slot_labels: None,
                images_base64: Some(vec!["base64data".into()]),
                computer_round_screen_rel_path: None,
                ui_bindings: None,
                context_state: None,
                attachments: None,
                anchor_message_id: None,
                trace_id: None,
                task_id: None,
                spawn_depth: None,
                tool_raw_output: None,
            },
            ChatMessage {
                id: "a1".into(),
                role: Role::Assistant,
                content: "hi".into(),
                status: "done".into(),
                created_at: 3,
                tool_calls: None,
                tool_call_id: None,
                tool_name: None,
                error_message: None,
                reasoning: None,
                thoughts: None,
                headline: None,
                raw_content: None,
                agent_id: Some("coder".into()),
                agent_instance_id: Some(inst.into()),
                agent_name: Some("Coder".into()),
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
                tool_raw_output: None,
            },
            ChatMessage {
                id: "a2".into(),
                role: Role::Assistant,
                content: "other".into(),
                status: "done".into(),
                created_at: 4,
                tool_calls: None,
                tool_call_id: None,
                tool_name: None,
                error_message: None,
                reasoning: None,
                thoughts: None,
                headline: None,
                raw_content: None,
                agent_id: Some("explore".into()),
                agent_instance_id: Some(other.into()),
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
                tool_raw_output: None,
            },
        ];
        let snap = build_snapshot_json("conv-1", inst, "coder", &history);
        let msgs = snap["messages"].as_array().expect("messages array");
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0]["role"], "user");
        assert!(msgs[0]["images"]
            .as_array()
            .unwrap()
            .first()
            .unwrap()
            .as_str()
            .unwrap()
            .starts_with("[image:"));
        assert_eq!(msgs[1]["role"], "assistant");
        assert_eq!(msgs[1]["agent_instance_id"], inst);
    }
}
