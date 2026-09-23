//! Resolve persisted attachment metadata by `pointer-media://` ref.

use crate::media::path_hint::MEDIA_URI_SCHEME;
use crate::models::MediaAttachment;
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
    find_attachment(conversation_id, rel, tail_id, |att| {
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
    find_attachment(conversation_id, id, "", |att| att.id == id)
}

pub fn conversation_user_attachments(conversation_id: &str) -> Result<Vec<MediaAttachment>> {
    let store = crate::conversation_store::global_store()?;
    store.load_recent_user_attachments(conversation_id, 3)
}

fn find_attachment(
    conversation_id: &str,
    needle: &str,
    alt_needle: &str,
    matches: impl Fn(&MediaAttachment) -> bool,
) -> Result<Option<MediaAttachment>> {
    let store = crate::conversation_store::global_store()?;
    store.find_user_attachment(conversation_id, needle, alt_needle, matches)
}
