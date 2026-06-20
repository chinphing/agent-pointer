//! Resolve persisted attachment metadata by `pointer-media://` ref.

use crate::media::path_hint::MEDIA_URI_SCHEME;
use crate::models::{MediaAttachment, Role};
use crate::storage;
use anyhow::Result;

fn rel_from_media_ref(raw: &str) -> &str {
    raw.trim()
        .strip_prefix(MEDIA_URI_SCHEME)
        .unwrap_or(raw.trim())
        .trim_start_matches('/')
}

/// Find the most recent user attachment matching a manifest `ref`.
pub fn find_attachment_by_media_ref(
    conversation_id: &str,
    media_ref: &str,
) -> Result<Option<MediaAttachment>> {
    let rel = rel_from_media_ref(media_ref);
    if rel.is_empty() {
        return Ok(None);
    }
    let tail_id = rel.rsplit('/').next().unwrap_or(rel);
    let messages = storage::load_conversation_messages(conversation_id)?;
    for msg in messages.iter().rev() {
        if !matches!(msg.role, Role::User) {
            continue;
        }
        let Some(atts) = &msg.attachments else {
            continue;
        };
        for att in atts {
            if att
                .storage_rel_path
                .as_deref()
                .map(|p| p.trim() == rel)
                .unwrap_or(false)
            {
                return Ok(Some(att.clone()));
            }
            if att.id == tail_id
                || att
                    .storage_rel_path
                    .as_deref()
                    .is_some_and(|p| p.rsplit('/').next() == Some(tail_id))
                || tail_id
                    .strip_prefix(&format!("{}_", att.id))
                    .is_some_and(|rest| !rest.is_empty())
            {
                return Ok(Some(att.clone()));
            }
        }
    }
    Ok(None)
}
