//! Webhook multipart upload and attachment reference validation.

use anyhow::{anyhow, Result};
use base64::Engine;
use serde::Serialize;
use uuid::Uuid;

use crate::media::layout::parse_storage_rel;
use crate::media::reply_attachments::{infer_attachment_kind, infer_attachment_mime};
use crate::media::store::{media_abs_path, save_attachment_bytes};
use crate::models::MediaAttachment;
use crate::storage::sanitize_storage_dir_segment;

/// Max upload size for `POST /api/webhooks/:src/upload` (aligned with IM inbound).
pub const MAX_WEBHOOK_UPLOAD_BYTES: usize = 30 * 1024 * 1024;

/// Max decoded size for inline `contentBase64` on webhook JSON bodies.
pub const MAX_WEBHOOK_INLINE_BASE64_BYTES: usize = 6 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebhookUploadResponse {
    pub attachment_id: String,
    pub storage_rel_path: String,
    pub kind: String,
    pub mime_type: String,
    pub file_name: String,
    pub size_bytes: u64,
}

/// Persist one webhook upload under `conversation-media/{conversationId}/`.
pub fn save_webhook_upload(
    conversation_id: &str,
    file_name: &str,
    mime_hint: Option<&str>,
    bytes: &[u8],
) -> Result<WebhookUploadResponse> {
    if bytes.is_empty() {
        anyhow::bail!("empty file");
    }
    if bytes.len() > MAX_WEBHOOK_UPLOAD_BYTES {
        anyhow::bail!("file too large (max {} bytes)", MAX_WEBHOOK_UPLOAD_BYTES);
    }
    let file_name = file_name.trim();
    if file_name.is_empty() {
        anyhow::bail!("fileName required");
    }
    let attachment_id = format!("wh-{}", Uuid::new_v4());
    let kind = infer_attachment_kind(file_name);
    let mime_type = infer_attachment_mime(file_name, mime_hint);
    let storage_rel_path =
        save_attachment_bytes(conversation_id, &attachment_id, bytes, file_name)?;
    log::info!(
        "webhook upload: saved {} ({} bytes) -> {}",
        file_name,
        bytes.len(),
        storage_rel_path
    );
    Ok(WebhookUploadResponse {
        attachment_id,
        storage_rel_path,
        kind,
        mime_type,
        file_name: file_name.to_string(),
        size_bytes: bytes.len() as u64,
    })
}

/// Ensure webhook JSON `attachments` reference only this conversation's stored files.
pub fn validate_webhook_attachments(
    conversation_id: &str,
    attachments: &[MediaAttachment],
) -> Result<()> {
    for att in attachments {
        if let Some(raw) = att
            .content_base64
            .as_deref()
            .filter(|s| !s.trim().is_empty())
        {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(raw.trim())
                .map_err(|e| anyhow!("attachment {} invalid base64: {e}", att.file_name))?;
            if bytes.is_empty() {
                anyhow::bail!("attachment {} empty base64 payload", att.file_name);
            }
            if bytes.len() > MAX_WEBHOOK_INLINE_BASE64_BYTES {
                anyhow::bail!(
                    "attachment {} too large for inline base64 (max {} bytes); use upload endpoint",
                    att.file_name,
                    MAX_WEBHOOK_INLINE_BASE64_BYTES
                );
            }
            continue;
        }
        if att
            .remote_url
            .as_deref()
            .map(str::trim)
            .is_some_and(|s| !s.is_empty())
        {
            continue;
        }
        let rel = att
            .storage_rel_path
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                anyhow!(
                    "attachment {} requires storageRelPath, contentBase64, or remoteUrl",
                    att.file_name
                )
            })?;
        validate_storage_rel_for_conversation(conversation_id, rel)?;
        let path = media_abs_path(rel)?;
        if !path.is_file() {
            anyhow::bail!(
                "attachment file not found: {} (upload first via POST /api/webhooks/:src/upload)",
                att.file_name
            );
        }
    }
    Ok(())
}

fn validate_storage_rel_for_conversation(conversation_id: &str, rel: &str) -> Result<()> {
    let rel = rel.trim().trim_start_matches('/');
    if rel.is_empty() || rel.contains("..") {
        anyhow::bail!("invalid storageRelPath");
    }
    let conv = sanitize_storage_dir_segment(conversation_id.trim());
    if conv.is_empty() {
        anyhow::bail!("invalid conversationId");
    }
    let parsed = parse_storage_rel(rel)?;
    if parsed.conversation_segment != conv {
        anyhow::bail!(
            "storageRelPath must be under this webhook conversation (expected segment `{conv}`, got `{}`; pass upload response `conversationId` on trigger)",
            parsed.conversation_segment
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_foreign_storage_rel_path() {
        let err = validate_webhook_attachments(
            "webhook:ci:20260629",
            &[MediaAttachment {
                id: "a1".into(),
                kind: "file".into(),
                mime_type: "application/octet-stream".into(),
                file_name: "x.bin".into(),
                size_bytes: 0,
                storage_rel_path: Some("other_conv/a1_x.bin".into()),
                content_base64: None,
                derived_text: None,
                local_abs_path: None,
                remote_url: None,
                oss_object_key: None,
            }],
        )
        .unwrap_err();
        assert!(err.to_string().contains("under this webhook conversation"));
    }

    #[test]
    fn rejects_foreign_storage_rel_path_new_layout() {
        let err = validate_webhook_attachments(
            "webhook:ci:20260707",
            &[MediaAttachment {
                id: "a1".into(),
                kind: "file".into(),
                mime_type: "application/octet-stream".into(),
                file_name: "x.bin".into(),
                size_bytes: 0,
                storage_rel_path: Some("_anonymous/other_conv/a1_x.bin".into()),
                content_base64: None,
                derived_text: None,
                local_abs_path: None,
                remote_url: None,
                oss_object_key: None,
            }],
        )
        .unwrap_err();
        assert!(err.to_string().contains("under this webhook conversation"));
    }

    #[test]
    fn save_and_validate_roundtrip() {
        let conv = format!("webhook_test_{}", Uuid::new_v4());
        let saved = save_webhook_upload(&conv, "note.txt", Some("text/plain"), b"hello").unwrap();
        assert_eq!(saved.kind, "document");
        validate_webhook_attachments(
            &conv,
            &[MediaAttachment {
                id: saved.attachment_id.clone(),
                kind: saved.kind.clone(),
                mime_type: saved.mime_type.clone(),
                file_name: saved.file_name.clone(),
                size_bytes: saved.size_bytes,
                storage_rel_path: Some(saved.storage_rel_path.clone()),
                content_base64: None,
                derived_text: None,
                local_abs_path: None,
                remote_url: None,
                oss_object_key: None,
            }],
        )
        .unwrap();
        let abs = media_abs_path(&saved.storage_rel_path).unwrap();
        assert!(abs.is_file());
        assert!(
            saved
                .storage_rel_path
                .contains(&format!("{}/", sanitize_storage_dir_segment(&conv))),
            "expected user-scoped storage rel path"
        );
        if let Some(parent) = abs.parent() {
            let _ = std::fs::remove_dir_all(parent);
        }
    }
}
