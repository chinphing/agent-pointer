//! Relative path layout for conversation media.
//!
//! New writes use versioned logical keys under `v2/…` that map into the user
//! data volume (`{user_key}-data`). Legacy `conversation-media/` and
//! two-segment paths remain readable (dual-read).

use anyhow::{bail, Result};

use crate::session_sandbox::ANONYMOUS_SEGMENT;
use crate::user_key::user_key;
use crate::user_storage::user_storage_segment;

/// Layout version prefix for new writes.
pub const LAYOUT_VERSION_PREFIX: &str = "v2";

/// Parsed `storage_rel_path` under conversation media / user data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedMediaRel {
    /// Sanitized user segment (`_anonymous` or `session_user_id` / user_key).
    pub user_segment: String,
    /// Sanitized conversation id segment.
    pub conversation_segment: String,
    /// File name within the conversation directory.
    pub file_name: String,
    /// `user/conversation/file` vs legacy `conversation/file`.
    pub legacy: bool,
    /// True when the path uses the `v2/...` user-data layout.
    pub versioned: bool,
    /// Kind under v2: attachments | media/chat | media/generated | captures.
    pub kind: Option<String>,
}

/// Build a legacy conversation-media relative path (dual-read / migration only).
pub fn build_storage_rel(session_user_id: &str, conversation_id: &str, file_name: &str) -> String {
    let user = user_storage_segment(session_user_id);
    let conv = crate::storage::sanitize_storage_dir_segment(conversation_id.trim());
    format!("{user}/{conv}/{file_name}")
}

/// Build a versioned logical key for new attachment / media writes.
pub fn build_v2_storage_rel(
    session_user_id: &str,
    conversation_id: &str,
    kind: &str,
    file_name: &str,
) -> String {
    let user = user_key(session_user_id);
    let conv = crate::storage::sanitize_storage_dir_segment(conversation_id.trim());
    let kind = kind.trim().trim_matches('/');
    format!("{LAYOUT_VERSION_PREFIX}/{user}/{kind}/{conv}/{file_name}")
}

pub fn parse_storage_rel(storage_rel_path: &str) -> Result<ParsedMediaRel> {
    let rel = storage_rel_path.trim().trim_start_matches('/');
    if rel.is_empty() || rel.contains("..") {
        bail!("invalid media rel path");
    }
    let parts: Vec<&str> = rel.split('/').filter(|p| !p.is_empty()).collect();
    if parts.first() == Some(&LAYOUT_VERSION_PREFIX) {
        // v2/{user_key}/{kind...}/{conv}/{file}
        if parts.len() < 5 {
            bail!("invalid v2 media rel path");
        }
        let user_segment = parts[1].to_string();
        let file_name = parts[parts.len() - 1].to_string();
        let conversation_segment = parts[parts.len() - 2].to_string();
        let kind = parts[2..parts.len() - 2].join("/");
        return Ok(ParsedMediaRel {
            user_segment,
            conversation_segment,
            file_name,
            legacy: false,
            versioned: true,
            kind: Some(kind),
        });
    }
    match parts.len() {
        0 | 1 => bail!("invalid media rel path"),
        2 => Ok(ParsedMediaRel {
            user_segment: String::new(),
            conversation_segment: parts[0].to_string(),
            file_name: parts[1].to_string(),
            legacy: true,
            versioned: false,
            kind: None,
        }),
        _ => Ok(ParsedMediaRel {
            user_segment: parts[0].to_string(),
            conversation_segment: parts[1].to_string(),
            file_name: parts[2..].join("/"),
            legacy: false,
            versioned: false,
            kind: None,
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
    let expected_key = user_key(actor);
    let expected_seg = user_storage_segment(actor);
    if parsed.user_segment != expected_key && parsed.user_segment != expected_seg {
        log::warn!(
            "verify_storage_rel_access DENIED: path={storage_rel_path} parsed_user_segment={} actor={actor} expected_key={expected_key} expected_seg={expected_seg}",
            parsed.user_segment
        );
        bail!("media access denied for session user");
    }
    Ok(())
}

/// Map a v2 logical key to a host absolute path under the user data volume.
pub fn resolve_v2_host_path(storage_rel_path: &str) -> Result<std::path::PathBuf> {
    let parsed = parse_storage_rel(storage_rel_path)?;
    if !parsed.versioned {
        bail!("not a v2 storage rel path");
    }
    let kind = parsed.kind.as_deref().unwrap_or("attachments");
    let base = crate::session_sandbox::SessionSandbox::sandboxes_base()?;
    let data_name = if parsed.user_segment == "anon" || parsed.user_segment.is_empty() {
        format!("anon-{}-data", parsed.conversation_segment)
    } else {
        format!("{}-data", parsed.user_segment)
    };
    let root = base.join(data_name);
    crate::env::mounts::ensure_data_layout(&root);
    let path = root
        .join(kind)
        .join(&parsed.conversation_segment)
        .join(&parsed.file_name);
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_and_parse_roundtrip() {
        let rel = build_storage_rel("user-a", "feishu:default:dm:x", "a.png");
        let parsed = parse_storage_rel(&rel).unwrap();
        assert!(!parsed.legacy);
        assert!(!parsed.versioned);
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
    fn v2_rel_roundtrip_and_access() {
        let rel = build_v2_storage_rel("alice@x.com", "conv-1", "attachments", "a.png");
        assert!(rel.starts_with("v2/"));
        let parsed = parse_storage_rel(&rel).unwrap();
        assert!(parsed.versioned);
        assert_eq!(parsed.kind.as_deref(), Some("attachments"));
        assert_eq!(parsed.file_name, "a.png");
        let _guard = crate::session_user_env::SessionUserIdGuard::enter("alice@x.com".into());
        assert!(verify_storage_rel_access(&rel).is_ok());
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

    #[test]
    fn verify_blocks_cross_user_v2_layout() {
        let rel = build_v2_storage_rel("user-b", "conv-1", "attachments", "f.bin");
        let _guard = crate::session_user_env::SessionUserIdGuard::enter("user-a".into());
        assert!(verify_storage_rel_access(&rel).is_err());
    }
}
