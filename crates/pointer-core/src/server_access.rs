//! pointer-server access control: restrict platform OAuth to configured user ids.
//!
//! When `POINTER_SERVER_ALLOWED_USER_IDS` is empty, any platform user may log in
//! (development / trusted intranet). When set, only listed user ids may obtain
//! or use a browser session.

use anyhow::{bail, Result};
use std::sync::OnceLock;

const ENV_ALLOWED_USER_IDS: &str = "POINTER_SERVER_ALLOWED_USER_IDS";
const ENV_REQUIRE_ALLOWED_USERS: &str = "POINTER_SERVER_REQUIRE_ALLOWED_USERS";

static ALLOWED_USER_IDS: OnceLock<Vec<String>> = OnceLock::new();

fn parse_allowed_user_ids_from_env() -> Vec<String> {
    std::env::var(ENV_ALLOWED_USER_IDS)
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// Cached allowlist from env (populated after server config load).
pub fn allowed_user_ids() -> &'static [String] {
    ALLOWED_USER_IDS
        .get_or_init(parse_allowed_user_ids_from_env)
        .as_slice()
}

/// Whether a non-empty allowlist is configured.
pub fn access_restriction_enabled() -> bool {
    !allowed_user_ids().is_empty()
}

/// Empty allowlist => allow all; otherwise user id must match exactly.
pub fn is_user_allowed_in(ids: &[String], user_id: &str) -> bool {
    if ids.is_empty() {
        return true;
    }
    let trimmed = user_id.trim();
    !trimmed.is_empty() && ids.iter().any(|id| id == trimmed)
}

/// Empty allowlist => allow all; otherwise user id must match exactly.
pub fn is_user_allowed(user_id: &str) -> bool {
    is_user_allowed_in(allowed_user_ids(), user_id)
}

/// Returns `server_access_denied` when the user is not on the allowlist.
pub fn ensure_user_allowed(user_id: &str) -> Result<()> {
    if is_user_allowed(user_id) {
        Ok(())
    } else {
        bail!("server_access_denied");
    }
}

fn parse_bool_env(key: &str) -> bool {
    match std::env::var(key) {
        Ok(v) => {
            let t = v.trim().to_ascii_lowercase();
            t == "1" || t == "true" || t == "yes" || t == "on"
        }
        Err(_) => false,
    }
}

pub fn require_allowed_users_configured() -> bool {
    parse_bool_env(ENV_REQUIRE_ALLOWED_USERS)
}

fn is_public_deployment_url(url: &str) -> bool {
    let trimmed = url.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return false;
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("http://127.0.0.1")
        || lower.starts_with("http://localhost")
        || lower.starts_with("https://127.0.0.1")
        || lower.starts_with("https://localhost")
    {
        return false;
    }
    true
}

/// Validate access config at pointer-server startup.
pub fn validate_server_access_at_startup(public_url: Option<&str>) -> Result<()> {
    let require = require_allowed_users_configured();
    let has_list = access_restriction_enabled();
    if require && !has_list {
        bail!("{ENV_REQUIRE_ALLOWED_USERS} is set but {ENV_ALLOWED_USER_IDS} is empty");
    }
    if let Some(url) = public_url {
        if is_public_deployment_url(url) && !has_list && !require {
            log::warn!(
                "server_access: public_url={url} but {ENV_ALLOWED_USER_IDS} is empty — \
                 any platform user can log in; set allowed_user_ids in pointer-server.toml"
            );
        }
    }
    if has_list {
        log::info!(
            "server_access: login restricted to {} platform user(s)",
            allowed_user_ids().len()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_allowlist_permits_all() {
        assert!(is_user_allowed_in(&[], "any-user"));
    }

    #[test]
    fn configured_allowlist_restricts() {
        let ids = vec!["user-a".into(), "user-b".into()];
        assert!(is_user_allowed_in(&ids, "user-a"));
        assert!(is_user_allowed_in(&ids, "user-b"));
        assert!(!is_user_allowed_in(&ids, "user-c"));
    }

    #[test]
    fn public_url_detection() {
        assert!(!is_public_deployment_url("http://127.0.0.1:8787"));
        assert!(!is_public_deployment_url("http://localhost:8787"));
        assert!(is_public_deployment_url("https://pointer.example.com"));
    }
}
