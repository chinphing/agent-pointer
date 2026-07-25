//! Relative path layout for conversation media under `{app_data}/conversation-media/`.

use anyhow::{bail, Result};

use crate::session_sandbox::ANONYMOUS_SEGMENT;
use crate::user_storage::user_storage_segment;

/// Parsed `storage_rel_path` under `conversation-media/`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedMediaRel {
    /// Sanitized user segment (`_anonymous` or `session_user_id`).
    pub user_segment: String,
    /// Sanitized conversation id segment.
    pub conversation_segment: String,
    /// File name within the conversation directory.
    pub file_name: String,
    /// `user/conversation/file` vs legacy `conversation/file`.
    pub legacy: bool,
}

pub fn build_storage_rel(session_user_id: &str, conversation_id: &str, file_name: &str) -> String {
    let user = user_storage_segment(session_user_id);
    let conv = crate::storage::sanitize_storage_dir_segment(conversation_id.trim());
    format!("{user}/{conv}/{file_name}")
}

pub fn parse_storage_rel(storage_rel_path: &str) -> Result<ParsedMediaRel> {
    let rel = storage_rel_path.trim().trim_start_matches('/');
    if rel.is_empty() || rel.contains("..") {
        bail!("invalid media rel path");
    }
    let parts: Vec<&str> = rel.split('/').filter(|p| !p.is_empty()).collect();
    match parts.len() {
        0 | 1 => bail!("invalid media rel path"),
        2 => Ok(ParsedMediaRel {
            user_segment: String::new(),
            conversation_segment: parts[0].to_string(),
            file_name: parts[1].to_string(),
            legacy: true,
        }),
        _ => Ok(ParsedMediaRel {
            user_segment: parts[0].to_string(),
            conversation_segment: parts[1].to_string(),
            file_name: parts[2..].join("/"),
            legacy: false,
        }),
    }
}

/// When `SESSION_USER_ID` is active, reject cross-user media paths (new layout only).
pub fn verify_storage_rel_access(storage_rel_path: &str) -> Result<()> {
    let actor = crate::session_user_env::current_session_user_id();
    let Some(ref actor) = actor else {
        return Ok(());
    };
    let parsed = parse_storage_rel(storage_rel_path)?;
    if parsed.legacy || parsed.user_segment == ANONYMOUS_SEGMENT {
        return Ok(());
    }
    let expected = user_storage_segment(actor);
    if parsed.user_segment != expected {
        log::warn!(
            "verify_storage_rel_access DENIED: path={storage_rel_path} parsed_user_segment={} actor={actor} expected={expected}",
            parsed.user_segment
        );
        bail!("media access denied for session user");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_and_parse_roundtrip() {
        let rel = build_storage_rel("user-a", "feishu:default:dm:x", "a.png");
        let parsed = parse_storage_rel(&rel).unwrap();
        assert!(!parsed.legacy);
        assert_eq!(parsed.user_segment, user_storage_segment("user-a"));
        assert_eq!(parsed.file_name, "a.png");
    }

    #[test]
    fn parse_legacy_two_part_path() {
        let parsed = parse_storage_rel("feishu_default_dm_x/a.png").unwrap();
        assert!(parsed.legacy);
        assert_eq!(parsed.conversation_segment, "feishu_default_dm_x");
    }

    #[test]
    fn verify_allows_anonymous_layout_for_logged_in_actor() {
        let rel = build_storage_rel("", "conv-1", "f.bin");
        let _guard = crate::session_user_env::SessionUserIdGuard::enter("user-a".into());
        assert!(verify_storage_rel_access(&rel).is_ok());
    }

    #[test]
    fn verify_blocks_cross_user_new_layout() {
        let rel = build_storage_rel("user-b", "conv-1", "f.bin");
        let _guard = crate::session_user_env::SessionUserIdGuard::enter("user-a".into());
        assert!(verify_storage_rel_access(&rel).is_err());
    }
}
