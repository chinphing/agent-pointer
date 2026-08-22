//! Resolve attachment refs for task-board init (and related tools).

use crate::models::{ChatMessage, MediaAttachment, Role};
use crate::storage;

use super::attachment_lookup::find_attachment_by_media_ref;
use super::manifest::{
    attachment_has_remote, attachment_local_abs_path, format_user_attachments_api_manifest,
};
use super::path_hint::MEDIA_URI_SCHEME;

const DELEGATION_ATTACHMENTS_HEADER: &str = "## User attachments (delegated)";
const DELEGATION_ATTACHMENTS_NOTE: &str =
    "For task-board init context (list files) — expand targets into wi_* rows in global_milestones; not for on-screen file UI.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentRefSpec {
    pub id: Option<String>,
    pub ref_uri: Option<String>,
}

fn attachment_forwardable(att: &MediaAttachment) -> bool {
    attachment_local_abs_path(att).is_some() || attachment_has_remote(att)
}

/// Parse `attachments[]` from tool args (string id/ref or object).
pub fn parse_attachment_specs_from_args(args: &serde_json::Value) -> Vec<AttachmentRefSpec> {
    let Some(arr) = args.get("attachments").and_then(|v| v.as_array()) else {
        return vec![];
    };
    let mut out = Vec::new();
    for el in arr {
        if let Some(s) = el.as_str() {
            let t = s.trim();
            if t.is_empty() {
                continue;
            }
            if t.contains(MEDIA_URI_SCHEME) {
                out.push(AttachmentRefSpec {
                    id: None,
                    ref_uri: Some(t.to_string()),
                });
            } else {
                out.push(AttachmentRefSpec {
                    id: Some(t.to_string()),
                    ref_uri: None,
                });
            }
            continue;
        }
        let Some(obj) = el.as_object() else {
            continue;
        };
        let id = obj
            .get("id")
            .or_else(|| obj.get("attachmentId"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let ref_uri = obj
            .get("ref")
            .or_else(|| obj.get("mediaRef"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        if id.is_some() || ref_uri.is_some() {
            out.push(AttachmentRefSpec { id, ref_uri });
        }
    }
    out
}

/// Collect attachments from the most recent user messages (lead history).
pub fn collect_recent_user_attachments(
    history: &[ChatMessage],
    max_user_messages: usize,
) -> Vec<MediaAttachment> {
    let cap = max_user_messages.max(1);
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut user_msgs = 0usize;
    for msg in history.iter().rev() {
        if !matches!(msg.role, Role::User) {
            continue;
        }
        user_msgs += 1;
        if user_msgs > cap {
            break;
        }
        let Some(atts) = msg.attachments.as_ref() else {
            continue;
        };
        for att in atts {
            if !attachment_forwardable(att) {
                log::warn!(
                    "delegation: skip non-forwardable attachment id={} file={}",
                    att.id,
                    att.file_name
                );
                continue;
            }
            if seen.insert(att.id.clone()) {
                out.push(att.clone());
            }
        }
    }
    out.reverse();
    out
}

fn find_attachment_by_id_in_messages(
    messages: &[ChatMessage],
    id: &str,
) -> Option<MediaAttachment> {
    let needle = id.trim();
    if needle.is_empty() {
        return None;
    }
    for msg in messages.iter().rev() {
        if !matches!(msg.role, Role::User) {
            continue;
        }
        let Some(atts) = &msg.attachments else {
            continue;
        };
        for att in atts {
            if att.id == needle {
                return Some(att.clone());
            }
        }
    }
    None
}

fn resolve_one_spec(
    conversation_id: &str,
    history: Option<&[ChatMessage]>,
    spec: &AttachmentRefSpec,
) -> Option<MediaAttachment> {
    if let Some(ref_uri) = spec
        .ref_uri
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        match find_attachment_by_media_ref(conversation_id, ref_uri) {
            Ok(Some(att)) => return Some(att),
            Ok(None) => {
                log::warn!("delegation: attachment ref not found: {ref_uri}");
            }
            Err(e) => {
                log::warn!("delegation: attachment ref lookup failed {ref_uri}: {e:#}");
            }
        }
    }
    if let Some(id) = spec.id.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        if let Some(hist) = history {
            if let Some(att) = find_attachment_by_id_in_messages(hist, id) {
                return Some(att);
            }
        }
        if let Ok(messages) = storage::load_conversation_messages(conversation_id) {
            if let Some(att) = find_attachment_by_id_in_messages(&messages, id) {
                return Some(att);
            }
        }
        log::warn!("delegation: attachment id not found: {id}");
    }
    None
}

/// Resolve explicit `attachments[]` specs to persisted [`MediaAttachment`] rows.
pub fn resolve_attachment_specs(
    conversation_id: &str,
    history: Option<&[ChatMessage]>,
    specs: &[AttachmentRefSpec],
) -> Vec<MediaAttachment> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for spec in specs {
        let Some(att) = resolve_one_spec(conversation_id, history, spec) else {
            continue;
        };
        if !attachment_forwardable(&att) {
            log::warn!(
                "delegation: skip non-forwardable attachment id={} file={}",
                att.id,
                att.file_name
            );
            continue;
        }
        if seen.insert(att.id.clone()) {
            out.push(att);
        }
    }
    out
}

pub fn format_delegation_attachments_block(attachments: &[MediaAttachment]) -> String {
    let manifest = format_user_attachments_api_manifest(attachments);
    if manifest.is_empty() {
        return String::new();
    }
    format!("{DELEGATION_ATTACHMENTS_HEADER}\n\n{DELEGATION_ATTACHMENTS_NOTE}\n\n{manifest}")
}

fn context_already_lists_attachment(existing: &str, att: &MediaAttachment) -> bool {
    let ctx = existing;
    if let Some(path) = super::manifest::attachment_local_abs_path(att) {
        if ctx.contains(path.as_str()) {
            return true;
        }
    }
    if let Some(rel) = att
        .storage_rel_path
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if ctx.contains(rel) {
            return true;
        }
    }
    ctx.contains(att.id.as_str())
}

/// Merge resolved attachments into sub-agent `context` (skip duplicates).
pub fn merge_attachments_into_context(existing: &str, attachments: &[MediaAttachment]) -> String {
    let to_add: Vec<MediaAttachment> = attachments
        .iter()
        .filter(|a| !context_already_lists_attachment(existing, a))
        .cloned()
        .collect();
    let block = format_delegation_attachments_block(&to_add);
    if block.is_empty() {
        return existing.trim().to_string();
    }
    let base = existing.trim();
    if base.is_empty() {
        block
    } else {
        format!("{base}\n\n{block}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Role;
    use serde_json::json;

    fn user_with_attachment(id: &str, file: &str, local: &str) -> ChatMessage {
        ChatMessage {
            id: "u1".into(),
            role: Role::User,
            content: "see file".into(),
            status: "done".into(),
            created_at: 0,
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
            attachments: Some(vec![MediaAttachment {
                id: id.into(),
                kind: "document".into(),
                mime_type: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
                    .into(),
                file_name: file.into(),
                size_bytes: 100,
                storage_rel_path: None,
                content_base64: None,
                derived_text: None,
                local_abs_path: Some(local.into()),
                remote_url: None,
                oss_object_key: None,
            }]),
            anchor_message_id: None,
            trace_id: None,
            task_id: None,
            spawn_depth: None,
        }
    }

    #[test]
    fn parse_attachment_specs_string_and_object() {
        let specs = parse_attachment_specs_from_args(&json!({
            "attachments": [
                "att_1",
                "pointer-media://conv/a/att_2.xlsx",
                { "attachmentId": "att_3", "ref": "pointer-media://conv/a/att_3.xlsx" }
            ]
        }));
        assert_eq!(specs.len(), 3);
        assert_eq!(specs[0].id.as_deref(), Some("att_1"));
        assert!(specs[1]
            .ref_uri
            .as_ref()
            .unwrap()
            .contains("pointer-media://"));
        assert_eq!(specs[2].id.as_deref(), Some("att_3"));
    }

    #[test]
    fn resolve_by_id_from_history() {
        let hist = vec![user_with_attachment("att_x", "book.xlsx", "/tmp/book.xlsx")];
        let resolved = resolve_attachment_specs(
            "conv",
            Some(&hist),
            &[AttachmentRefSpec {
                id: Some("att_x".into()),
                ref_uri: None,
            }],
        );
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].file_name, "book.xlsx");
    }

    #[test]
    fn merge_skips_duplicate_local_path() {
        let att = user_with_attachment("att_x", "book.xlsx", "/tmp/book.xlsx")
            .attachments
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        let ctx = "Lead context:\nlocalPath: /tmp/book.xlsx";
        let merged = merge_attachments_into_context(ctx, std::slice::from_ref(&att));
        assert_eq!(merged.trim(), ctx.trim());
    }

    #[test]
    fn merge_appends_delegation_block() {
        let att = user_with_attachment("att_x", "book.xlsx", "/tmp/book.xlsx")
            .attachments
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        let merged = merge_attachments_into_context("do the task", std::slice::from_ref(&att));
        assert!(merged.contains("do the task"));
        assert!(merged.contains(DELEGATION_ATTACHMENTS_HEADER));
        assert!(merged.contains("global_milestones"));
        assert!(merged.contains("book.xlsx"));
        assert!(merged.contains("/tmp/book.xlsx"));
    }
}
