//! In-flight browser login attempts: OAuth `state` ↔ browser binding.
//!
//! A `state` token alone is not enough to finish a login — the browser that
//! started the flow also carries [`OAUTH_STATE_COOKIE`], and the callback must
//! present both. That keeps a stolen/guessed `state` from being redeemed in
//! someone else's browser, and the entry is single-use either way.

use axum::http::{header, HeaderMap, HeaderValue};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::web_session;

/// The user must complete the browser round-trip within this window.
pub const OAUTH_PENDING_TTL: Duration = Duration::from_secs(600);
/// Short-lived cookie that ties the pending entry to the starting browser.
pub const OAUTH_STATE_COOKIE: &str = "pointer_oauth_state";

/// One login attempt: everything the callback needs that must not travel
/// through the browser.
#[derive(Debug, Clone, PartialEq)]
pub struct PendingLogin {
    /// Registry id the login was started for. A callback may only redeem an
    /// entry that belongs to its own provider.
    pub provider_id: String,
    pub verifier: String,
    pub redirect_uri: String,
    /// 发起授权时用的 OAuth client_id。换码必须沿用同一个，否则控制面会
    /// 判「授权码与客户端不匹配」。
    pub client_id: String,
    /// OIDC `nonce` sent on the authorize request, echoed back in the ID token.
    pub nonce: Option<String>,
    pub created_at: Instant,
}

impl PendingLogin {
    pub fn new(provider_id: impl Into<String>, verifier: String, redirect_uri: String) -> Self {
        Self {
            provider_id: provider_id.into(),
            verifier,
            redirect_uri,
            client_id: pointer_core::platform_endpoints::oauth_client_id(),
            nonce: None,
            created_at: Instant::now(),
        }
    }

    /// 指定本次登录用的 OAuth client_id（服务端用 `pointer-server`）。
    pub fn with_client_id(mut self, client_id: impl Into<String>) -> Self {
        self.client_id = client_id.into();
        self
    }

    pub fn with_nonce(mut self, nonce: String) -> Self {
        self.nonce = Some(nonce);
        self
    }
}

/// Why a pending entry could not be redeemed. Codes only — never a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingError {
    /// Unknown, already redeemed, or expired `state`.
    InvalidState,
    /// The `state` belongs to a different flow (or a different provider).
    ProviderMismatch,
    /// Too many logins are already in flight; the new one was refused.
    AtCapacity,
}

/// In-memory pending logins, keyed by `state`.
#[derive(Default)]
pub struct PendingLogins {
    inner: RwLock<HashMap<String, PendingLogin>>,
}

impl PendingLogins {
    /// Record a new attempt without a capacity check (tests only — the server
    /// always goes through [`Self::start_capped`]).
    #[cfg(test)]
    pub fn start(&self, state: &str, login: PendingLogin) {
        let state = state.trim();
        if state.is_empty() {
            return;
        }
        self.purge_expired();
        self.inner.write().insert(state.to_string(), login);
    }

    /// True when one more in-flight login still fits (`0` = unlimited).
    ///
    /// Callers use this to refuse early, before spending an authorize round-trip.
    pub fn has_capacity(&self, max_pending: usize) -> bool {
        if max_pending == 0 {
            return true;
        }
        self.purge_expired();
        self.inner.read().len() < max_pending
    }

    /// Record a new attempt, refusing it when the store is at `max_pending`.
    ///
    /// Over-capacity starts are rejected, never evicted: dropping an entry that
    /// a user is still working on would surface as a mystery `invalid_state`.
    pub fn start_capped(
        &self,
        state: &str,
        login: PendingLogin,
        max_pending: usize,
    ) -> Result<(), PendingError> {
        let state = state.trim();
        if state.is_empty() {
            return Err(PendingError::InvalidState);
        }
        self.purge_expired();
        let mut guard = self.inner.write();
        if max_pending > 0 && guard.len() >= max_pending {
            return Err(PendingError::AtCapacity);
        }
        guard.insert(state.to_string(), login);
        Ok(())
    }

    /// Redeem a `state` exactly once, and only for the provider that started it.
    ///
    /// The entry is removed even when the provider does not match: a mismatch is
    /// either a bug or a probe, and neither should leave a usable entry behind.
    pub fn take(&self, state: &str, provider_id: &str) -> Result<PendingLogin, PendingError> {
        let state = state.trim();
        if state.is_empty() {
            return Err(PendingError::InvalidState);
        }
        let Some(login) = self.inner.write().remove(state) else {
            return Err(PendingError::InvalidState);
        };
        if login.created_at.elapsed() >= OAUTH_PENDING_TTL {
            return Err(PendingError::InvalidState);
        }
        if login.provider_id != provider_id {
            return Err(PendingError::ProviderMismatch);
        }
        Ok(login)
    }

    pub fn purge_expired(&self) {
        let now = Instant::now();
        self.inner
            .write()
            .retain(|_, login| now.duration_since(login.created_at) < OAUTH_PENDING_TTL);
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.inner.read().len()
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// 32 random bytes, hex-encoded. Used for `state` and OIDC `nonce`.
pub fn random_token() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// The callback must present the same `state` the browser was handed.
pub fn state_cookie_matches(cookie_state: Option<&str>, query_state: &str) -> bool {
    match cookie_state {
        Some(cookie) => constant_time_eq(cookie.trim(), query_state.trim()),
        None => false,
    }
}

fn constant_time_eq(a: &str, b: &str) -> bool {
    let a = a.as_bytes();
    let b = b.as_bytes();
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

pub fn state_from_headers(headers: &HeaderMap) -> Option<String> {
    web_session::cookie_value(headers, OAUTH_STATE_COOKIE)
}

pub fn set_state_cookie(headers: &mut HeaderMap, state: &str, secure: bool) {
    let mut value = format!(
        "{OAUTH_STATE_COOKIE}={state}; Path=/; HttpOnly; SameSite=Lax; Max-Age={}",
        OAUTH_PENDING_TTL.as_secs()
    );
    if secure {
        value.push_str("; Secure");
    }
    if let Ok(header_value) = HeaderValue::from_str(&value) {
        headers.append(header::SET_COOKIE, header_value);
    }
}

pub fn clear_state_cookie(headers: &mut HeaderMap, secure: bool) {
    let mut value = format!("{OAUTH_STATE_COOKIE}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0");
    if secure {
        value.push_str("; Secure");
    }
    if let Ok(header_value) = HeaderValue::from_str(&value) {
        headers.append(header::SET_COOKIE, header_value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pending(provider_id: &str) -> PendingLogin {
        PendingLogin::new(
            provider_id,
            "verifier-1".into(),
            "https://pointer.example/cb".into(),
        )
    }

    #[test]
    fn take_is_single_use() {
        let store = PendingLogins::default();
        store.start("state-1", pending("platform"));

        let taken = store
            .take("state-1", "platform")
            .expect("first redeem works");
        assert_eq!(taken.verifier, "verifier-1");
        assert_eq!(taken.provider_id, "platform");

        assert_eq!(
            store.take("state-1", "platform"),
            Err(PendingError::InvalidState)
        );
        assert!(store.is_empty());
    }

    #[test]
    fn take_rejects_a_state_from_another_provider() {
        let store = PendingLogins::default();
        store.start("state-1", pending("dingtalk"));

        assert_eq!(
            store.take("state-1", "feishu"),
            Err(PendingError::ProviderMismatch)
        );
        // A mismatch burns the entry, so it cannot be retried elsewhere.
        assert!(store.is_empty());
    }

    #[test]
    fn take_rejects_unknown_and_empty_state() {
        let store = PendingLogins::default();
        assert_eq!(
            store.take("nope", "platform"),
            Err(PendingError::InvalidState)
        );
        assert_eq!(
            store.take("   ", "platform"),
            Err(PendingError::InvalidState)
        );
    }

    #[test]
    fn expired_entries_are_not_redeemable() {
        let store = PendingLogins::default();
        let mut login = pending("platform");
        login.created_at = Instant::now() - OAUTH_PENDING_TTL - Duration::from_secs(1);
        store.start("state-old", login);
        assert_eq!(
            store.take("state-old", "platform"),
            Err(PendingError::InvalidState)
        );

        // `start` also sweeps expired entries, so they cannot accumulate.
        store.start("state-fresh", pending("platform"));
        let mut stale = pending("platform");
        stale.created_at = Instant::now() - OAUTH_PENDING_TTL - Duration::from_secs(1);
        store.start("state-stale", stale);
        assert_eq!(store.len(), 2);
        store.start("state-newer", pending("platform"));
        assert_eq!(store.len(), 2);
        assert_eq!(
            store.take("state-stale", "platform"),
            Err(PendingError::InvalidState)
        );
    }

    #[test]
    fn browser_binding_requires_the_same_state_cookie() {
        assert!(state_cookie_matches(Some("state-1"), "state-1"));
        assert!(state_cookie_matches(Some(" state-1 "), " state-1 "));
        assert!(!state_cookie_matches(None, "state-1"));
        assert!(!state_cookie_matches(Some("state-2"), "state-1"));
        assert!(!state_cookie_matches(Some("state-1"), ""));
        assert!(!state_cookie_matches(Some("short"), "a-much-longer-state"));
    }

    #[test]
    fn tokens_are_hex_and_unique() {
        let a = random_token();
        let b = random_token();
        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }

    #[test]
    fn cookie_helpers_round_trip() {
        let mut headers = HeaderMap::new();
        set_state_cookie(&mut headers, "state-1", false);
        let raw = headers
            .get(header::SET_COOKIE)
            .and_then(|value| value.to_str().ok())
            .expect("set-cookie present");
        assert!(raw.starts_with("pointer_oauth_state=state-1;"));
        assert!(raw.contains("HttpOnly"));
        assert!(raw.contains("SameSite=Lax"));
        assert!(!raw.contains("Secure"));

        let mut request = HeaderMap::new();
        request.insert(
            header::COOKIE,
            HeaderValue::from_static("other=1; pointer_oauth_state=state-1"),
        );
        assert_eq!(state_from_headers(&request).as_deref(), Some("state-1"));

        let mut cleared = HeaderMap::new();
        clear_state_cookie(&mut cleared, true);
        let raw = cleared
            .get(header::SET_COOKIE)
            .and_then(|value| value.to_str().ok())
            .expect("clear cookie present");
        assert!(raw.contains("Max-Age=0"));
        assert!(raw.contains("Secure"));
    }

    /// 换码必须沿用发起授权时的 client_id：控制面把授权码绑定到客户端，
    /// 两边不一致会返回「授权码与客户端不匹配」。
    #[test]
    fn pending_keeps_the_client_id_used_at_authorize() {
        let pending = PendingLogin::new("platform", "v".into(), "https://cb".into())
            .with_client_id("pointer-server");
        assert_eq!(pending.client_id, "pointer-server");
        // 不指定时退回默认（桌面端 client）
        let default_pending = PendingLogin::new("platform", "v".into(), "https://cb".into());
        assert_eq!(
            default_pending.client_id,
            pointer_core::platform_endpoints::oauth_client_id()
        );
    }

    fn login(provider: &str) -> PendingLogin {
        PendingLogin::new(provider, "verifier".to_string(), "https://cb".to_string())
    }

    #[test]
    fn capacity_cap_rejects_new_starts_without_evicting_in_flight_ones() {
        let store = PendingLogins::default();
        store
            .start_capped("state-a", login("p"), 1)
            .expect("first start fits");
        assert!(!store.has_capacity(1));
        assert_eq!(
            store.start_capped("state-b", login("p"), 1),
            Err(PendingError::AtCapacity)
        );
        // The in-flight login still works: we never evicted it.
        assert!(store.take("state-a", "p").is_ok());
        assert!(store.has_capacity(1));
        store
            .start_capped("state-b", login("p"), 1)
            .expect("slot freed");
    }

    #[test]
    fn zero_capacity_limit_means_unlimited() {
        let store = PendingLogins::default();
        for i in 0..50 {
            store
                .start_capped(&format!("state-{i}"), login("p"), 0)
                .expect("unlimited");
        }
        assert_eq!(store.len(), 50);
    }

    #[test]
    fn expired_entries_free_capacity() {
        let store = PendingLogins::default();
        let mut stale = login("p");
        stale.created_at = Instant::now() - OAUTH_PENDING_TTL - Duration::from_secs(1);
        store.start("state-old", stale);
        // The expired entry is purged before the cap is applied.
        store
            .start_capped("state-new", login("p"), 1)
            .expect("expired entry does not count");
        assert_eq!(store.len(), 1);
    }
}
