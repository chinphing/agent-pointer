//! Standalone SSO login tickets for third-party IdPs (no Pointer official exchange).
//!
//! Wire format (JWT compact HS256):
//! `base64url(header).base64url(payload).base64url(hmac-sha256)`
//!
//! Payload claims: `sub` (required), `aud`, `iat`, `exp`, `jti`, optional `name`.
//! Query param on the agent URL: `?sso=<ticket>`.

use anyhow::{anyhow, bail, Context, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use hmac::{Hmac, Mac};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use subtle::ConstantTimeEq;

type HmacSha256 = Hmac<Sha256>;

const ENV_SSO_ENABLED: &str = "POINTER_SERVER_SSO_ENABLED";
const ENV_SSO_SECRET: &str = "POINTER_SERVER_SSO_SECRET";
const ENV_SSO_SECRET_PREV: &str = "POINTER_SERVER_SSO_SECRET_PREV";
const ENV_SSO_AUDIENCE: &str = "POINTER_SERVER_SSO_AUDIENCE";
const ENV_SSO_MAX_SKEW_SECS: &str = "POINTER_SERVER_SSO_MAX_SKEW_SECS";

const DEFAULT_MAX_SKEW_SECS: i64 = 30;
const DEFAULT_TTL_SECS: i64 = 120;
const HEADER_JSON: &str = r#"{"alg":"HS256","typ":"SSO"}"#;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SsoClaims {
    pub sub: String,
    pub aud: String,
    pub iat: i64,
    pub exp: i64,
    pub jti: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// Verified SSO identity after ticket checks.
#[derive(Debug, Clone)]
pub struct SsoIdentity {
    pub user_id: String,
    pub nickname: Option<String>,
}

fn env_trimmed(key: &str) -> String {
    std::env::var(key).unwrap_or_default().trim().to_string()
}

fn env_truthy(key: &str) -> Option<bool> {
    let v = env_trimmed(key);
    if v.is_empty() {
        return None;
    }
    match v.to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

/// SSO is available when secret + audience are set, and not explicitly disabled.
pub fn local_sso_configured() -> bool {
    if env_truthy(ENV_SSO_ENABLED) == Some(false) {
        return false;
    }
    !configured_sso_secret().is_empty() && !configured_sso_audience().is_empty()
}

pub fn configured_sso_secret() -> String {
    env_trimmed(ENV_SSO_SECRET)
}

pub fn configured_sso_secret_prev() -> String {
    env_trimmed(ENV_SSO_SECRET_PREV)
}

pub fn configured_sso_audience() -> String {
    env_trimmed(ENV_SSO_AUDIENCE)
}

pub fn configured_sso_max_skew_secs() -> i64 {
    env_trimmed(ENV_SSO_MAX_SKEW_SECS)
        .parse::<i64>()
        .ok()
        .filter(|n| *n >= 0 && *n <= 300)
        .unwrap_or(DEFAULT_MAX_SKEW_SECS)
}

fn b64url_encode(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

fn b64url_decode(s: &str) -> Result<Vec<u8>> {
    URL_SAFE_NO_PAD
        .decode(s.as_bytes())
        .context("invalid base64url in SSO ticket")
}

fn hmac_sha256(secret: &[u8], message: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(secret).expect("HMAC accepts any key length");
    mac.update(message);
    mac.finalize().into_bytes().to_vec()
}

fn ct_eq_bytes(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        let _ = a.ct_eq(a);
        return false;
    }
    a.ct_eq(b).into()
}

/// Mint a short-lived SSO ticket (for tests / `pointer-server --mint-sso-ticket`).
pub fn mint_sso_ticket(
    secret: &str,
    audience: &str,
    user_id: &str,
    nickname: Option<&str>,
    ttl_secs: i64,
    now_unix: i64,
) -> Result<String> {
    let secret = secret.trim();
    let audience = audience.trim();
    let user_id = user_id.trim();
    if secret.is_empty() || audience.is_empty() || user_id.is_empty() {
        bail!("sso mint requires non-empty secret, audience, and user_id");
    }
    let ttl = if ttl_secs > 0 {
        ttl_secs
    } else {
        DEFAULT_TTL_SECS
    };
    let jti = uuid::Uuid::new_v4().to_string();
    let claims = SsoClaims {
        sub: user_id.to_string(),
        aud: audience.to_string(),
        iat: now_unix,
        exp: now_unix + ttl,
        jti,
        name: nickname
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
    };
    let header_b64 = b64url_encode(HEADER_JSON.as_bytes());
    let payload_b64 = b64url_encode(
        serde_json::to_string(&claims)
            .context("serialize SSO claims")?
            .as_bytes(),
    );
    let signing_input = format!("{header_b64}.{payload_b64}");
    let sig = hmac_sha256(secret.as_bytes(), signing_input.as_bytes());
    Ok(format!("{signing_input}.{}", b64url_encode(&sig)))
}

fn verify_signature(ticket: &str, secrets: &[&str]) -> Result<(SsoClaims, String)> {
    let parts: Vec<&str> = ticket.trim().split('.').collect();
    if parts.len() != 3 {
        bail!("sso_ticket_malformed");
    }
    let (header_b64, payload_b64, sig_b64) = (parts[0], parts[1], parts[2]);
    let header_raw = b64url_decode(header_b64)?;
    let header: serde_json::Value =
        serde_json::from_slice(&header_raw).context("sso_header_json")?;
    let alg = header.get("alg").and_then(|v| v.as_str()).unwrap_or("");
    if alg != "HS256" {
        bail!("sso_unsupported_alg");
    }
    let signing_input = format!("{header_b64}.{payload_b64}");
    let sig = b64url_decode(sig_b64)?;
    let mut ok = false;
    for secret in secrets {
        if secret.is_empty() {
            continue;
        }
        let expected = hmac_sha256(secret.as_bytes(), signing_input.as_bytes());
        if ct_eq_bytes(&sig, &expected) {
            ok = true;
            break;
        }
    }
    if !ok {
        bail!("sso_bad_signature");
    }
    let payload_raw = b64url_decode(payload_b64)?;
    let claims: SsoClaims = serde_json::from_slice(&payload_raw).context("sso_payload_json")?;
    let jti = claims.jti.clone();
    Ok((claims, jti))
}

/// In-memory one-time `jti` store (process-local).
#[derive(Default)]
pub struct SsoNonceStore {
    inner: Mutex<HashMap<String, Instant>>,
}

impl SsoNonceStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn purge_expired(map: &mut HashMap<String, Instant>) {
        let now = Instant::now();
        map.retain(|_, until| *until > now);
    }

    /// Returns true if `jti` was unused and is now consumed until `exp` (+ skew).
    pub fn consume(&self, jti: &str, exp_unix: i64, now_unix: i64) -> bool {
        let jti = jti.trim();
        if jti.is_empty() {
            return false;
        }
        let retain_secs = (exp_unix - now_unix).max(60) as u64 + 60;
        let until = Instant::now() + Duration::from_secs(retain_secs);
        let mut guard = self.inner.lock();
        Self::purge_expired(&mut guard);
        if guard.contains_key(jti) {
            return false;
        }
        guard.insert(jti.to_string(), until);
        true
    }
}

/// Verify ticket, audience, time window, and one-time `jti`.
pub fn verify_sso_ticket(
    ticket: &str,
    nonce_store: &SsoNonceStore,
    now_unix: i64,
) -> Result<SsoIdentity> {
    if !local_sso_configured() {
        bail!("sso_not_configured");
    }
    let primary = configured_sso_secret();
    let prev = configured_sso_secret_prev();
    let secrets: Vec<&str> = if prev.is_empty() {
        vec![primary.as_str()]
    } else {
        vec![primary.as_str(), prev.as_str()]
    };
    let (claims, jti) = verify_signature(ticket, &secrets)?;
    let sub = claims.sub.trim();
    if sub.is_empty() {
        bail!("sso_missing_sub");
    }
    let aud = configured_sso_audience();
    if claims.aud.trim() != aud {
        bail!("sso_audience_mismatch");
    }
    let skew = configured_sso_max_skew_secs();
    if claims.exp + skew < now_unix {
        bail!("sso_expired");
    }
    if claims.iat > now_unix + skew {
        bail!("sso_not_yet_valid");
    }
    if !nonce_store.consume(&jti, claims.exp, now_unix) {
        bail!("sso_replay");
    }
    Ok(SsoIdentity {
        user_id: sub.to_string(),
        nickname: claims
            .name
            .as_ref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
    })
}

/// Convenience for HTTP handlers: verify with wall-clock time.
pub fn verify_sso_ticket_now(
    ticket: &str,
    nonce_store: &Arc<SsoNonceStore>,
) -> Result<SsoIdentity> {
    let now = chrono::Utc::now().timestamp();
    verify_sso_ticket(ticket, nonce_store, now).map_err(|e| {
        log::warn!("local_sso: verify failed: {e:#}");
        anyhow!(e)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn env_guard() -> MutexGuard<'static, ()> {
        ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn clear_sso_env() {
        std::env::remove_var(ENV_SSO_SECRET);
        std::env::remove_var(ENV_SSO_SECRET_PREV);
        std::env::remove_var(ENV_SSO_AUDIENCE);
        std::env::remove_var(ENV_SSO_ENABLED);
        std::env::remove_var(ENV_SSO_MAX_SKEW_SECS);
    }

    #[test]
    fn mint_and_verify_roundtrip() {
        let _guard = env_guard();
        clear_sso_env();
        std::env::set_var(ENV_SSO_SECRET, "test-secret-key");
        std::env::set_var(ENV_SSO_AUDIENCE, "https://agent.example.com");
        let now = 1_700_000_000;
        let ticket = mint_sso_ticket(
            "test-secret-key",
            "https://agent.example.com",
            "user-42",
            Some("Alice"),
            120,
            now,
        )
        .expect("mint");
        let store = SsoNonceStore::new();
        let id = verify_sso_ticket(&ticket, &store, now + 5).expect("verify");
        assert_eq!(id.user_id, "user-42");
        assert_eq!(id.nickname.as_deref(), Some("Alice"));
        assert!(verify_sso_ticket(&ticket, &store, now + 5).is_err());
        clear_sso_env();
    }

    #[test]
    fn rejects_wrong_audience() {
        let _guard = env_guard();
        clear_sso_env();
        std::env::set_var(ENV_SSO_SECRET, "k");
        std::env::set_var(ENV_SSO_AUDIENCE, "https://right.example.com");
        let now = 1_700_000_000;
        let ticket = mint_sso_ticket("k", "https://wrong.example.com", "u", None, 60, now).unwrap();
        let store = SsoNonceStore::new();
        let err = verify_sso_ticket(&ticket, &store, now)
            .unwrap_err()
            .to_string();
        assert!(err.contains("audience"), "{err}");
        clear_sso_env();
    }

    #[test]
    fn accepts_prev_secret() {
        let _guard = env_guard();
        clear_sso_env();
        std::env::set_var(ENV_SSO_SECRET, "new-secret");
        std::env::set_var(ENV_SSO_SECRET_PREV, "old-secret");
        std::env::set_var(ENV_SSO_AUDIENCE, "aud");
        let now = 1_700_000_000;
        let ticket = mint_sso_ticket("old-secret", "aud", "u1", None, 60, now).unwrap();
        let store = SsoNonceStore::new();
        let id = verify_sso_ticket(&ticket, &store, now).unwrap();
        assert_eq!(id.user_id, "u1");
        clear_sso_env();
    }

    #[test]
    fn sso_sub_flows_to_terminal_session_user_id_env() {
        let _guard = env_guard();
        clear_sso_env();
        std::env::set_var(ENV_SSO_SECRET, "e2e-sso-secret");
        std::env::set_var(ENV_SSO_AUDIENCE, "http://127.0.0.1:18787");

        let now = chrono::Utc::now().timestamp();
        let store = SsoNonceStore::new();

        for (sub, nick) in [("e2e-zhangsan", "ZhangSan"), ("e2e-lisi", "LiSi")] {
            let ticket = mint_sso_ticket(
                "e2e-sso-secret",
                "http://127.0.0.1:18787",
                sub,
                Some(nick),
                120,
                now,
            )
            .expect("mint");
            let identity = verify_sso_ticket(&ticket, &store, now).expect("verify");
            assert_eq!(identity.user_id, sub);

            // Same as complete_standalone_sso → PlatformSession.user.id
            let auth = crate::local_auth::create_local_auth_manager_for_user(
                &identity.user_id,
                identity.nickname.clone(),
                false,
            );
            let uid = auth.platform_user_id().expect("platform user id");
            assert_eq!(uid, sub);

            // Same as run_chat SessionUserIdGuard + terminal child env
            let _user_guard = crate::session_user_env::SessionUserIdGuard::enter(uid.clone());
            let map = crate::dotenv::build_terminal_child_environment(&[]);
            assert_eq!(
                map.get("SESSION_USER_ID").map(String::as_str),
                Some(sub),
                "env map for {sub}"
            );

            // Real subprocess like `terminal` tool (unix)
            #[cfg(unix)]
            {
                let out = std::process::Command::new("sh")
                    .arg("-c")
                    .arg("printf %s \"$SESSION_USER_ID\"")
                    .env_clear()
                    .envs(map.iter().map(|(k, v)| (k.as_str(), v.as_str())))
                    .output()
                    .expect("spawn sh");
                assert!(
                    out.status.success(),
                    "stderr={}",
                    String::from_utf8_lossy(&out.stderr)
                );
                assert_eq!(
                    String::from_utf8_lossy(&out.stdout),
                    sub,
                    "shell SESSION_USER_ID for {sub}"
                );
            }
        }

        clear_sso_env();
    }
}
