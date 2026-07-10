//! Local username/password authentication for standalone pointer-server deployments.
//!
//! Config stores `username` + `password_hmac` (HMAC-SHA256 hex) + `hmac_secret`.
//! Plaintext passwords are never written to TOML.

use crate::platform_auth::{
    PlatformAuthManager, PlatformLoginCredentials, PlatformSession, PlatformUserSummary,
};
use chrono::Utc;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::sync::Arc;
use subtle::ConstantTimeEq;

type HmacSha256 = Hmac<Sha256>;

const ENV_ADMIN_USERNAME: &str = "POINTER_SERVER_ADMIN_USERNAME";
const ENV_ADMIN_PASSWORD_HMAC: &str = "POINTER_SERVER_ADMIN_PASSWORD_HMAC";
const ENV_AUTH_HMAC_SECRET: &str = "POINTER_SERVER_AUTH_HMAC_SECRET";
/// Deprecated; logged at startup when still present.
pub const ENV_ADMIN_TOKEN_DEPRECATED: &str = "POINTER_SERVER_ADMIN_TOKEN";

const LOCAL_USER_ID: &str = "local-admin";

/// Returns the configured admin username (toml / env). Empty when unset.
pub fn configured_admin_username() -> String {
    std::env::var(ENV_ADMIN_USERNAME)
        .unwrap_or_default()
        .trim()
        .to_string()
}

/// Returns the configured password HMAC hex digest (toml / env). Empty when unset.
pub fn configured_password_hmac() -> String {
    std::env::var(ENV_ADMIN_PASSWORD_HMAC)
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
}

/// Returns the HMAC secret used to hash passwords (toml / env). Empty when unset.
pub fn configured_auth_hmac_secret() -> String {
    std::env::var(ENV_AUTH_HMAC_SECRET)
        .unwrap_or_default()
        .trim()
        .to_string()
}

/// Whether username, password_hmac, and hmac_secret are all configured.
pub fn local_password_auth_configured() -> bool {
    !configured_admin_username().is_empty()
        && !configured_password_hmac().is_empty()
        && !configured_auth_hmac_secret().is_empty()
}

/// HMAC-SHA256(key=secret, msg=password) as lowercase hex.
pub fn hmac_sha256_hex(secret: &str, password: &str) -> String {
    let mut mac =
        HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC accepts any key length");
    mac.update(password.as_bytes());
    let result = mac.finalize().into_bytes();
    hex_encode(&result)
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0xf) as usize] as char);
    }
    out
}

fn ct_eq_str(a: &str, b: &str) -> bool {
    let a = a.as_bytes();
    let b = b.as_bytes();
    if a.len() != b.len() {
        // Still compare against self to keep timing closer when lengths differ.
        let _ = a.ct_eq(a);
        return false;
    }
    a.ct_eq(b).into()
}

/// Validate a login attempt against the configured username + password HMAC.
pub fn verify_local_password(username: &str, password: &str) -> bool {
    if !local_password_auth_configured() {
        log::warn!(
            "local_auth: password login attempted but username/password_hmac/hmac_secret incomplete \
             ({ENV_ADMIN_USERNAME}/{ENV_ADMIN_PASSWORD_HMAC}/{ENV_AUTH_HMAC_SECRET})"
        );
        return false;
    }
    let username = username.trim();
    let password = password.trim();
    if username.is_empty() || password.is_empty() {
        return false;
    }
    let expected_user = configured_admin_username();
    if !ct_eq_str(username, &expected_user) {
        return false;
    }
    let secret = configured_auth_hmac_secret();
    let expected_hmac = configured_password_hmac();
    let actual = hmac_sha256_hex(&secret, password);
    ct_eq_str(&actual, &expected_hmac)
}

/// Warn once when the deprecated admin_token env is still set.
pub fn warn_if_deprecated_admin_token_configured() {
    let legacy = std::env::var(ENV_ADMIN_TOKEN_DEPRECATED).unwrap_or_default();
    if !legacy.trim().is_empty() {
        log::warn!(
            "local_auth: {ENV_ADMIN_TOKEN_DEPRECATED} is deprecated and ignored; \
             configure username / password_hmac / hmac_secret under [auth.local] instead"
        );
    }
}

/// Build an in-memory platform session for standalone admin login (no OAuth refresh).
pub fn create_local_auth_manager() -> Arc<PlatformAuthManager> {
    let auth = Arc::new(PlatformAuthManager::new());
    let expires_at = Utc::now().timestamp() + 365 * 24 * 3600;
    let nickname = {
        let u = configured_admin_username();
        if u.is_empty() {
            "Admin".into()
        } else {
            u
        }
    };
    auth.set_partner_session(PlatformSession {
        access_token: "local-session".into(),
        refresh_token: String::new(),
        expires_at,
        agent_id: String::new(),
        user: PlatformUserSummary {
            id: LOCAL_USER_ID.into(),
            nickname: Some(nickname),
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

    fn clear_auth_env() {
        std::env::remove_var(ENV_ADMIN_USERNAME);
        std::env::remove_var(ENV_ADMIN_PASSWORD_HMAC);
        std::env::remove_var(ENV_AUTH_HMAC_SECRET);
        std::env::remove_var(ENV_ADMIN_TOKEN_DEPRECATED);
    }

    fn set_auth_env(user: &str, secret: &str, password: &str) {
        let digest = hmac_sha256_hex(secret, password);
        std::env::set_var(ENV_ADMIN_USERNAME, user);
        std::env::set_var(ENV_ADMIN_PASSWORD_HMAC, &digest);
        std::env::set_var(ENV_AUTH_HMAC_SECRET, secret);
    }

    #[test]
    fn hmac_hex_is_stable() {
        let a = hmac_sha256_hex("secret", "password");
        let b = hmac_sha256_hex("secret", "password");
        assert_eq!(a, b);
        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn verify_rejects_when_unconfigured() {
        let _guard = env_guard();
        clear_auth_env();
        assert!(!verify_local_password("admin", "anything"));
        assert!(!local_password_auth_configured());
    }

    #[test]
    fn verify_matches_configured_password() {
        let _guard = env_guard();
        clear_auth_env();
        set_auth_env("admin", "hmac-secret-key", "s3cret");
        assert!(verify_local_password("admin", "s3cret"));
        assert!(!verify_local_password("admin", "wrong"));
        assert!(!verify_local_password("other", "s3cret"));
        clear_auth_env();
    }

    #[test]
    fn verify_trims_username_and_password() {
        let _guard = env_guard();
        clear_auth_env();
        set_auth_env("admin", "k", "pw");
        assert!(verify_local_password("  admin  ", "  pw  "));
        clear_auth_env();
    }
}
