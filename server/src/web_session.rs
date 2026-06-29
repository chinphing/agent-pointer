//! Browser-scoped platform OAuth sessions for pointer-server (cookie `pointer_web_session`).

use axum::{
    body::Body,
    http::{header, HeaderMap, Request},
    middleware::Next,
    response::Response,
};
use parking_lot::RwLock;
use pointer_core::platform_auth::{PlatformAuthManager, PlatformLoginCredentials};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const WEB_SESSION_COOKIE: &str = "pointer_web_session";
const WEB_SESSION_MAX_AGE_SEC: u64 = 30 * 24 * 3600;

#[derive(Clone)]
pub struct WebSessionEntry {
    pub auth: Arc<PlatformAuthManager>,
    pub creds: PlatformLoginCredentials,
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
    ) -> String {
        let id = random_session_id();
        self.inner.write().insert(
            id.clone(),
            WebSessionEntry {
                auth,
                creds,
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

pub async fn web_session_middleware(
    req: Request<Body>,
    next: Next,
) -> Response {
    let session_store = req
        .extensions()
        .get::<Arc<WebSessionStore>>()
        .cloned();
    let Some(store) = session_store else {
        return next.run(req).await;
    };
    store.purge_expired();
    let session_id = session_id_from_headers(req.headers());
    let Some(session_id) = session_id else {
        return next.run(req).await;
    };
    let Some(entry) = store.get(&session_id) else {
        return next.run(req).await;
    };
    pointer_core::web_request_auth::run_scoped(entry.auth, entry.creds, || async move {
        next.run(req).await
    })
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
