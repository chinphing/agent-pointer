//! Collision-safe identity key for persistent sandbox volumes.
//!
//! Volume names must stay flat and match `[A-Za-z0-9_-]+` for envd. Raw user ids
//! cannot be used: sanitizing them collapses distinct identities (for example
//! `alice@x` and `alice_x`) and can leak personal information into host paths.
//!
//! `user_key` is a stable hash of the authenticated id. Anonymous sessions share
//! the fixed key `anon`; their containers and temporary workspaces stay
//! conversation-scoped elsewhere.

use sha2::{Digest, Sha256};

/// Fixed key for empty / missing session user ids.
pub const ANON_USER_KEY: &str = "anon";

/// Number of hex chars taken from the SHA-256 digest.
///
/// 32 hex chars = 128 bits; enough to avoid practical collisions while keeping
/// `{key}-home` / `{key}-data` well under common volume-name limits.
const USER_KEY_HEX_LEN: usize = 32;

/// Stable volume-safe key for `session_user_id`.
///
/// Empty input maps to [`ANON_USER_KEY`]. Non-empty ids hash to `u` + lowercase hex.
pub fn user_key(session_user_id: &str) -> String {
    let trimmed = session_user_id.trim();
    if trimmed.is_empty() {
        return ANON_USER_KEY.to_string();
    }
    let digest = Sha256::digest(trimmed.as_bytes());
    let hex = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("u{}", &hex[..USER_KEY_HEX_LEN])
}

/// Host directory name for the persistent Home volume.
pub fn home_volume_name(session_user_id: &str) -> String {
    format!("{}-home", user_key(session_user_id))
}

/// Host directory name for the persistent Data volume.
pub fn data_volume_name(session_user_id: &str) -> String {
    format!("{}-data", user_key(session_user_id))
}

/// True when `name` is safe as an envd flat volume directory name.
pub fn is_volume_safe_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_id_is_anon() {
        assert_eq!(user_key(""), ANON_USER_KEY);
        assert_eq!(user_key("   "), ANON_USER_KEY);
        assert_eq!(home_volume_name(""), "anon-home");
        assert_eq!(data_volume_name(""), "anon-data");
    }

    #[test]
    fn same_id_is_stable() {
        assert_eq!(user_key("alice"), user_key("alice"));
        assert_eq!(user_key(" alice "), user_key("alice"));
    }

    #[test]
    fn sanitized_collisions_stay_distinct() {
        // storage::sanitize_storage_dir_segment would collapse these.
        assert_ne!(user_key("alice@x"), user_key("alice_x"));
        assert_ne!(user_key("a/b"), user_key("a_b"));
    }

    #[test]
    fn volume_names_are_envd_safe() {
        let key = user_key("alice@example.com");
        assert!(key.starts_with('u'));
        assert!(is_volume_safe_name(&key));
        assert!(is_volume_safe_name(&home_volume_name("alice@example.com")));
        assert!(is_volume_safe_name(&data_volume_name("alice@example.com")));
        assert!(!is_volume_safe_name("bad name"));
        assert!(!is_volume_safe_name("users/alice"));
    }

    #[test]
    fn does_not_embed_raw_identity() {
        let key = user_key("alice@example.com");
        assert!(!key.contains("alice"));
        assert!(!key.contains('@'));
        assert!(!key.contains('.'));
    }
}
