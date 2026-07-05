//! Per-`session_user_id` layout under the Pointer app data directory.

use std::path::PathBuf;

use crate::session_sandbox::ANONYMOUS_SEGMENT;
use crate::storage::{app_data_dir, sanitize_storage_dir_segment};

/// Directory name segment for one `session_user_id` (flat, cross-platform).
pub fn user_storage_segment(session_user_id: &str) -> String {
    let uid = session_user_id.trim();
    if uid.is_empty() {
        ANONYMOUS_SEGMENT.to_string()
    } else {
        sanitize_storage_dir_segment(uid)
    }
}

pub fn session_user_id_for_conversation(conversation_id: &str) -> String {
    crate::conversation_store::global_store()
        .ok()
        .and_then(|store| store.session_user_id(conversation_id).ok())
        .unwrap_or_default()
}

pub fn memories_root_dir() -> anyhow::Result<PathBuf> {
    Ok(app_data_dir()?.join("memories"))
}

pub fn memories_dir_for(session_user_id: &str) -> anyhow::Result<PathBuf> {
    Ok(memories_root_dir()?.join(user_storage_segment(session_user_id)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anonymous_user_uses_anonymous_segment() {
        assert_eq!(user_storage_segment(""), ANONYMOUS_SEGMENT);
        assert_eq!(user_storage_segment("   "), ANONYMOUS_SEGMENT);
    }

    #[test]
    fn im_user_id_is_flat_segment() {
        let seg = user_storage_segment("user@im.wechat");
        assert!(!seg.contains('/'));
        assert!(!seg.contains(':'));
    }
}
