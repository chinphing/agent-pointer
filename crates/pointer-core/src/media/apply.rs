//! Persist user message attachments before the LLM round (no auto understanding).

use crate::media::filename::normalize_inbound_filename;
use crate::media::store::{read_media_bytes, save_attachment_bytes};
use crate::models::{ChatMessage, MediaAttachment, ModelSettings, Role};
use anyhow::{Context, Result};
use base64::Engine;

pub const INLINE_IMAGE_MAX_BYTES: usize = 2 * 1024 * 1024;
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
fn process_attachment_persist_only(
    conversation_id: &str,
    att: &mut MediaAttachment,
) -> Result<()> {
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
    att.content_base64 = None;
    att.size_bytes = bytes.len() as u64;
    att.file_name = normalize_inbound_filename(&att.file_name);
    Ok(())
}

/// Persist wire attachments only; model context comes from API manifest in `make_openai_messages`.
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
            if let Err(e) = process_attachment_persist_only(conversation_id, att) {
                log::warn!(
                    "media: persist attachment {} failed: {:#}",
                    att.file_name,
                    e
                );
            }
        }

        msg.attachments = Some(attachments);
    }
    Ok(())
}