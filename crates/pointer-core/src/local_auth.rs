//! Local admin-token authentication for standalone pointer-server deployments.

use crate::platform_auth::{
    PlatformAuthManager, PlatformLoginCredentials, PlatformSession, PlatformUserSummary,
};
use chrono::Utc;
use std::sync::Arc;

const ENV_ADMIN_TOKEN: &str = "POINTER_SERVER_ADMIN_TOKEN";
const LOCAL_USER_ID: &str = "local-admin";

/// Returns the configured admin token (toml / env). Empty when unset.
pub fn configured_admin_token() -> String {
    std::env::var(ENV_ADMIN_TOKEN).unwrap_or_default().trim().to_string()
}

/// Whether a non-empty admin token is configured.
pub fn admin_token_configured() -> bool {
    !configured_admin_token().is_empty()
}

/// Validate a login attempt against the configured admin token.
pub fn verify_admin_token(candidate: &str) -> bool {
    let expected = configured_admin_token();
    if expected.is_empty() {
        log::warn!("local_auth: admin token login attempted but {ENV_ADMIN_TOKEN} is not set");
        return false;
    }
    let candidate = candidate.trim();
    if candidate.is_empty() {
        return false;
    }
    subtle::ConstantTimeEq::ct_eq(candidate.as_bytes(), expected.as_bytes()).into()
}

/// Build an in-memory platform session for standalone admin login (no OAuth refresh).
pub fn create_local_auth_manager() -> Arc<PlatformAuthManager> {
    let auth = Arc::new(PlatformAuthManager::new());
    let expires_at = Utc::now().timestamp() + 365 * 24 * 3600;
    auth.set_partner_session(PlatformSession {
        access_token: "local-session".into(),
        refresh_token: String::new(),
        expires_at,
        agent_id: String::new(),
        user: PlatformUserSummary {
            id: LOCAL_USER_ID.into(),
            nickname: Some("Admin".into()),
            is_platform_admin: true,
            included_tokens: 0,
            consumed_tokens: 0,
            token_quota_exhausted: false,
        },
    });
    auth
}

/// Empty credentials — LLM keys are injected from server config at startup.
pub fn empty_local_credentials() -> PlatformLoginCredentials {
    PlatformLoginCredentials::default()
}

/// User id stamped on conversations for standalone admin sessions.
pub fn local_user_id() -> &'static str {
    LOCAL_USER_ID
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn env_guard() -> MutexGuard<'static, ()> {
        ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn verify_rejects_when_unconfigured() {
        let _guard = env_guard();
        std::env::remove_var(ENV_ADMIN_TOKEN);
        assert!(!verify_admin_token("anything"));
    }

    #[test]
    fn verify_matches_configured_token() {
        let _guard = env_guard();
        std::env::set_var(ENV_ADMIN_TOKEN, "secret-token");
        assert!(verify_admin_token("secret-token"));
        assert!(!verify_admin_token("wrong"));
        std::env::remove_var(ENV_ADMIN_TOKEN);
    }
}
