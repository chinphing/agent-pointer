//! Browser-scoped platform OAuth sessions for pointer-server (cookie `pointer_web_session`).

use axum::{
    body::Body,
    extract::State,
    http::{header, HeaderMap, Request, StatusCode},
    middleware::Next,
    response::Response,
};
use axum::response::IntoResponse;
use parking_lot::RwLock;
use pointer_core::platform_auth::{PlatformAuthManager, PlatformLoginCredentials};
use pointer_core::web_request_auth::WebSessionAuthKind;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const WEB_SESSION_COOKIE: &str = "pointer_web_session";
const WEB_SESSION_MAX_AGE_SEC: u64 = 30 * 24 * 3600;

#[derive(Clone)]
pub struct WebSessionEntry {
    pub auth: Arc<PlatformAuthManager>,
    pub creds: PlatformLoginCredentials,
    pub kind: WebSessionAuthKind,
    created_at: Instant,
}

#[derive(Default)]
pub struct WebSessionStore {
    inner: RwLock<HashMap<String, WebSessionEntry>>,
}

impl WebSessionStore {
    pub fn insert(
        &self,
        auth: Arc<PlatformAuthManager>,
        creds: PlatformLoginCredentials,
        kind: WebSessionAuthKind,
    ) -> String {
        let id = random_session_id();
        self.inner.write().insert(
            id.clone(),
            WebSessionEntry {
                auth,
                creds,
                kind,
                created_at: Instant::now(),
            },
        );
        id
    }

    pub fn get(&self, session_id: &str) -> Option<WebSessionEntry> {
        let guard = self.inner.read();
        guard.get(session_id).cloned()
    }

    pub fn remove(&self, session_id: &str) {
        self.inner.write().remove(session_id);
    }

    pub fn update_creds(&self, session_id: &str, creds: PlatformLoginCredentials) {
        if let Some(entry) = self.inner.write().get_mut(session_id) {
            entry.creds = creds;
        }
    }

    /// Any active browser session (for webhook/cron on single-tenant cloud hosts).
    pub fn any_session_auth(&self) -> Option<pointer_core::web_request_auth::WebSessionAuth> {
        self.purge_expired();
        let guard = self.inner.read();
        guard.values().next().map(|entry| pointer_core::web_request_auth::WebSessionAuth {
            kind: entry.kind,
            auth: entry.auth.clone(),
            creds: entry.creds.clone(),
        })
    }

    pub fn purge_expired(&self) {
        let ttl = Duration::from_secs(WEB_SESSION_MAX_AGE_SEC);
        let now = Instant::now();
        self.inner
            .write()
            .retain(|_, entry| now.duration_since(entry.created_at) < ttl);
    }
}

pub fn session_id_from_headers(headers: &HeaderMap) -> Option<String> {
    let raw = headers.get(header::COOKIE)?.to_str().ok()?;
    for part in raw.split(';') {
        let part = part.trim();
        let prefix = format!("{WEB_SESSION_COOKIE}=");
        if let Some(value) = part.strip_prefix(&prefix) {
            let value = value.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

pub fn set_session_cookie(headers: &mut HeaderMap, session_id: &str, secure: bool) {
    let mut value = format!(
        "{WEB_SESSION_COOKIE}={session_id}; Path=/; HttpOnly; SameSite=Lax; Max-Age={WEB_SESSION_MAX_AGE_SEC}"
    );
    if secure {
        value.push_str("; Secure");
    }
    if let Ok(hv) = axum::http::HeaderValue::from_str(&value) {
        headers.append(header::SET_COOKIE, hv);
    }
}

pub fn clear_session_cookie(headers: &mut HeaderMap, secure: bool) {
    let mut value = format!("{WEB_SESSION_COOKIE}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0");
    if secure {
        value.push_str("; Secure");
    }
    if let Ok(hv) = axum::http::HeaderValue::from_str(&value) {
        headers.append(header::SET_COOKIE, hv);
    }
}

/// Resolve browser OAuth session from the cookie store (preferred over task-local capture).
pub fn lookup_session_auth(
    store: &WebSessionStore,
    headers: &HeaderMap,
) -> Option<pointer_core::web_request_auth::WebSessionAuth> {
    let session_id = session_id_from_headers(headers)?;
    let entry = store.get(&session_id)?;
    Some(pointer_core::web_request_auth::WebSessionAuth {
        kind: entry.kind,
        auth: entry.auth,
        creds: entry.creds,
    })
}

pub async fn web_session_middleware(
    State(store): State<Arc<WebSessionStore>>,
    req: Request<Body>,
    next: Next,
) -> Response {
    store.purge_expired();
    let session_id = session_id_from_headers(req.headers());
    let Some(session_id) = session_id else {
        return next.run(req).await;
    };
    let Some(entry) = store.get(&session_id) else {
        return next.run(req).await;
    };
    if entry.kind == WebSessionAuthKind::Platform
        && pointer_core::server_access::access_restriction_enabled()
    {
        if let Some(uid) = entry.auth.platform_user_id() {
            if !pointer_core::server_access::is_user_allowed(&uid) {
                log::warn!("server_access: evicting browser session for user_id={uid}");
                store.remove(&session_id);
                let mut resp = (StatusCode::FORBIDDEN, "server_access_denied").into_response();
                clear_session_cookie(resp.headers_mut(), cookie_secure_from_env());
                return resp;
            }
        }
    }
    pointer_core::web_request_auth::run_scoped(
        entry.auth,
        entry.creds,
        entry.kind,
        || async move { next.run(req).await },
    )
    .await
}

fn random_session_id() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    let mut out = String::with_capacity(32);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

fn cookie_secure_from_env() -> bool {
    std::env::var("POINTER_SERVER_PUBLIC_URL")
        .map(|u| u.trim().to_ascii_lowercase().starts_with("https://"))
        .unwrap_or(false)
}
