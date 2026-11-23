//! Persist user message attachments before the LLM round (no auto understanding).

use crate::media::filename::normalize_inbound_filename;
use crate::media::store::{
    read_media_bytes, save_attachment_bytes, short_attachment_id_from_sandbox_rel,
};
use crate::models::{ChatMessage, MediaAttachment, ModelSettings, Role};
use anyhow::{Context, Result};
use base64::Engine;

pub const INLINE_IMAGE_MAX_BYTES: usize = 1024 * 1024;
pub const HARD_IMAGE_MAX_BYTES: usize = 6 * 1024 * 1024;

fn load_attachment_bytes(att: &MediaAttachment) -> Result<Vec<u8>> {
    if let Some(raw) = att
        .content_base64
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        return base64::engine::general_purpose::STANDARD
            .decode(raw.trim())
            .context("decode attachment base64");
    }
    if let Some(rel) = att
        .storage_rel_path
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        return read_media_bytes(rel);
    }
    anyhow::bail!(
        "attachment {} has no content_base64 or storage_rel_path",
        att.file_name
    );
}

/// Save wire bytes to conversation-media; do not mutate message content.
fn process_attachment_persist_only(conversation_id: &str, att: &mut MediaAttachment) -> Result<()> {
    let bytes = load_attachment_bytes(att)?;
    if att
        .storage_rel_path
        .as_deref()
        .unwrap_or("")
        .trim()
        .is_empty()
    {
        let rel = save_attachment_bytes(conversation_id, &att.id, &bytes, &att.file_name)?;
        att.storage_rel_path = Some(rel);
    }
    if let Some(short_id) = att
        .storage_rel_path
        .as_deref()
        .and_then(short_attachment_id_from_sandbox_rel)
    {
        att.id = short_id;
    } else if att
        .storage_rel_path
        .as_deref()
        .is_some_and(|rel| rel.starts_with(crate::media::store::SESSION_SANDBOXES_PREFIX))
    {
        log::warn!(
            "media: sandbox attachment has no recognizable short ID: {}",
            att.storage_rel_path.as_deref().unwrap_or_default()
        );
    }
    att.content_base64 = None;
    att.size_bytes = bytes.len() as u64;
    att.file_name = normalize_inbound_filename(&att.file_name);
    Ok(())
}

fn attachment_has_remote_url(att: &MediaAttachment) -> bool {
    att.remote_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .is_some()
}

/// Persist wire attachments and write `MEDIA:…?attachmentId=` into user `content`
/// so model I/O matches persisted text (no separate API attachment manifest).
pub async fn apply_media_to_history(
    history: &mut [ChatMessage],
    _settings: &ModelSettings,
    _run_id: &str,
    conversation_id: &str,
    _api_key: &str,
    _cancel: &tokio_util::sync::CancellationToken,
) -> Result<()> {
    for msg in history.iter_mut() {
        if !matches!(msg.role, Role::User) {
            continue;
        }
        let Some(mut attachments) = msg.attachments.take() else {
            continue;
        };
        if attachments.is_empty() {
            continue;
        }

        for att in attachments.iter_mut() {
            let is_oss_video = att.kind == "video" && attachment_has_remote_url(att);
            if is_oss_video {
                att.content_base64 = None;
                if let Some(rel) = att
                    .storage_rel_path
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    match crate::media::store::media_abs_path(rel) {
                        Ok(path) if path.is_file() => {
                            att.local_abs_path = Some(path.display().to_string());
                        }
                        Ok(path) => {
                            log::warn!(
                                "media: OSS video {} local backup missing at {} ({})",
                                att.file_name,
                                rel,
                                path.display()
                            );
                            att.storage_rel_path = None;
                        }
                        Err(e) => {
                            log::warn!(
                                "media: OSS video {} invalid storage_rel_path {rel}: {e:#}",
                                att.file_name
                            );
                            att.storage_rel_path = None;
                        }
                    }
                }
                att.file_name = normalize_inbound_filename(&att.file_name);
                continue;
            }
            if let Err(e) = process_attachment_persist_only(conversation_id, att) {
                log::warn!(
                    "media: persist attachment {} failed: {:#}",
                    att.file_name,
                    e
                );
            } else if let Some(rel) = att
                .storage_rel_path
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                // Host location for server IO. The model marker is built from
                // the storage key and the current session root, not this path.
                if let Ok(path) = crate::media::store::media_abs_path(rel) {
                    if path.is_file() {
                        att.local_abs_path = Some(path.display().to_string());
                    }
                }
            }
        }

        msg.content = crate::media::ensure_user_content_media_markers(&msg.content, &attachments);
        msg.attachments = Some(attachments);
    }
    Ok(())
}
