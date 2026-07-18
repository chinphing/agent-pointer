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
    find_attachment(conversation_id, |att| {
        let tail_id = rel.rsplit('/').next().unwrap_or(rel);
        att.storage_rel_path
            .as_deref()
            .is_some_and(|p| p.trim() == rel)
            || att.id == tail_id
            || att
                .storage_rel_path
                .as_deref()
                .is_some_and(|p| p.rsplit('/').next() == Some(tail_id))
            || tail_id
                .strip_prefix(&format!("{}_", att.id))
                .is_some_and(|rest| !rest.is_empty())
    })
}

pub fn find_attachment_by_id(
    conversation_id: &str,
    attachment_id: &str,
) -> Result<Option<MediaAttachment>> {
    let id = attachment_id.trim();
    if id.is_empty() {
        return Ok(None);
    }
    find_attachment(conversation_id, |att| att.id == id)
}

pub fn conversation_user_attachments(conversation_id: &str) -> Result<Vec<MediaAttachment>> {
    let messages = storage::load_conversation_messages(conversation_id)?;
    let mut out = Vec::new();
    for msg in messages.iter().rev() {
        if matches!(msg.role, Role::User) {
            if let Some(atts) = &msg.attachments {
                out.extend(atts.iter().cloned());
            }
        }
    }
    Ok(out)
}

fn find_attachment(
    conversation_id: &str,
    matches: impl Fn(&MediaAttachment) -> bool,
) -> Result<Option<MediaAttachment>> {
    let messages = storage::load_conversation_messages(conversation_id)?;
    for msg in messages.iter().rev() {
        if !matches!(msg.role, Role::User) {
            continue;
        }
        let Some(atts) = &msg.attachments else {
            continue;
        };
        if let Some(att) = atts.iter().find(|att| matches(att)) {
            return Ok(Some(att.clone()));
        }
    }
    Ok(None)
}
