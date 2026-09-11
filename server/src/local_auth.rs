//! Standalone local username/password login + SVG captcha for pointer-server.

use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use pointer_core::{
    local_auth::{
        create_local_auth_manager, empty_local_credentials, verify_local_password,
        warn_if_deprecated_admin_token_configured,
    },
    platform_auth::PlatformSessionView,
    web_request_auth::WebSessionAuthKind,
};
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::web_session;
use crate::ServerState;

const CAPTCHA_TTL: Duration = Duration::from_secs(5 * 60);
const CAPTCHA_LEN: usize = 4;
/// Ambiguity-avoiding alphabet (no 0/O, 1/I/l).
const CAPTCHA_ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

struct CaptchaEntry {
    answer: String,
    expires_at: Instant,
}

/// In-memory one-time captcha challenges (process-local).
#[derive(Default)]
pub struct CaptchaStore {
    inner: Mutex<HashMap<String, CaptchaEntry>>,
}

impl CaptchaStore {
    pub fn issue(&self) -> (String, String) {
        self.purge_expired();
        let answer = random_captcha_text();
        let id = random_id();
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard.insert(
            id.clone(),
            CaptchaEntry {
                answer: answer.clone(),
                expires_at: Instant::now() + CAPTCHA_TTL,
            },
        );
        log::info!("local_auth: issued captcha id={id}");
        if cfg!(debug_assertions) {
            // Local smoke tests: answer is only logged in debug builds.
            log::info!("local_auth: captcha answer (debug only) id={id} answer={answer}");
        }
        (id, answer)
    }

    /// Consume and verify. Returns false on missing/expired/mismatch (entry removed either way).
    pub fn verify_and_consume(&self, id: &str, attempt: &str) -> bool {
        let id = id.trim();
        if id.is_empty() {
            return false;
        }
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let Some(entry) = guard.remove(id) else {
            log::warn!("local_auth: captcha id not found id={id}");
            return false;
        };
        if Instant::now() > entry.expires_at {
            log::warn!("local_auth: captcha expired id={id}");
            return false;
        }
        let ok = entry.answer.eq_ignore_ascii_case(attempt.trim());
        if !ok {
            log::warn!("local_auth: captcha mismatch id={id}");
        }
        ok
    }

    fn purge_expired(&self) {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        guard.retain(|_, e| e.expires_at > now);
    }
}

fn random_captcha_text() -> String {
    let mut rng = rand::thread_rng();
    (0..CAPTCHA_LEN)
        .map(|_| {
            let idx = rng.gen_range(0..CAPTCHA_ALPHABET.len());
            CAPTCHA_ALPHABET[idx] as char
        })
        .collect()
}

fn random_id() -> String {
    let mut rng = rand::thread_rng();
    let mut bytes = [0u8; 16];
    rng.fill(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Render a simple SVG captcha (no image crate dependency).
pub fn render_captcha_svg(text: &str) -> String {
    let width = 140;
    let height = 48;
    let mut rng = rand::thread_rng();
    let mut noise = String::new();
    for _ in 0..6 {
        let x1: i32 = rng.gen_range(0..width);
        let y1: i32 = rng.gen_range(0..height);
        let x2: i32 = rng.gen_range(0..width);
        let y2: i32 = rng.gen_range(0..height);
        // `#` is literal in format!; do not write `##` (invalid CSS color → black box).
        noise.push_str(&format!(
            "<line x1=\"{x1}\" y1=\"{y1}\" x2=\"{x2}\" y2=\"{y2}\" stroke=\"#c5c9d1\" stroke-width=\"1\"/>"
        ));
    }
    let mut chars = String::new();
    for (i, ch) in text.chars().enumerate() {
        let x = 18 + i as i32 * 28;
        let y = 32 + rng.gen_range(-4..=4);
        let rot = rng.gen_range(-18..=18);
        chars.push_str(&format!(
            "<text x=\"{x}\" y=\"{y}\" font-family=\"ui-monospace,Menlo,Consolas,monospace\" font-size=\"26\" font-weight=\"700\" fill=\"#1f2937\" transform=\"rotate({rot} {x} {y})\">{ch}</text>"
        ));
    }
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"0 0 {width} {height}\" role=\"img\" aria-label=\"captcha\">\
<rect width=\"100%\" height=\"100%\" fill=\"#f3f4f6\"/>\
{noise}\
{chars}\
</svg>"
    )
}

#[derive(Serialize)]
pub struct AuthModeResponse {
    pub mode: &'static str,
}

/// `GET /api/auth/mode` — frontend chooses OAuth vs local password form.
pub async fn auth_mode() -> Json<AuthModeResponse> {
    let mode = if pointer_core::deployment_mode::is_standalone() {
        "standalone"
    } else {
        "platform"
    };
    Json(AuthModeResponse { mode })
}

#[derive(Serialize)]
pub struct CaptchaResponse {
    #[serde(rename = "captchaId")]
    pub captcha_id: String,
    #[serde(rename = "imageSvg")]
    pub image_svg: String,
}

/// `GET /api/auth/local/captcha` — issue a one-time SVG captcha (standalone only).
pub async fn local_captcha(
    State(state): State<ServerState>,
) -> Result<Json<CaptchaResponse>, (StatusCode, String)> {
    if !pointer_core::deployment_mode::is_standalone() {
        return Err((
            StatusCode::NOT_FOUND,
            "local_auth_only_in_standalone".into(),
        ));
    }
    let (captcha_id, answer) = state.captcha_store.issue();
    let image_svg = render_captcha_svg(&answer);
    Ok(Json(CaptchaResponse {
        captcha_id,
        image_svg,
    }))
}

#[derive(Deserialize)]
pub struct LocalLoginBody {
    pub username: String,
    pub password: String,
    #[serde(rename = "captchaId")]
    pub captcha_id: String,
    pub captcha: String,
}

/// `POST /api/auth/local/login` — username/password + captcha (standalone only).
pub async fn local_login(
    State(state): State<ServerState>,
    Json(body): Json<LocalLoginBody>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    if !pointer_core::deployment_mode::is_standalone() {
        return Err((
            StatusCode::NOT_FOUND,
            "local_auth_only_in_standalone".into(),
        ));
    }
    warn_if_deprecated_admin_token_configured();

    if !state
        .captcha_store
        .verify_and_consume(&body.captcha_id, &body.captcha)
    {
        log::warn!("local_auth: rejected login — invalid captcha");
        return Err((StatusCode::UNAUTHORIZED, "invalid_captcha".into()));
    }

    if !verify_local_password(&body.username, &body.password) {
        log::warn!("local_auth: rejected password login attempt");
        return Err((StatusCode::UNAUTHORIZED, "invalid_credentials".into()));
    }

    let auth = create_local_auth_manager();
    let creds = empty_local_credentials();
    let session_id = state
        .web_sessions
        .insert(auth.clone(), creds, WebSessionAuthKind::Local);
    crate::sync_automation_web_session(&state);
    log::info!("local_auth: admin login ok web_session={session_id}");
    let mut resp = Json(PlatformSessionView {
        logged_in: true,
        expires_at: auth.session_view().expires_at,
        user_nickname: auth.session_view().user_nickname,
        is_platform_admin: true,
        included_tokens: 0,
        consumed_tokens: 0,
        token_quota_exhausted: false,
    })
    .into_response();
    web_session::set_session_cookie(resp.headers_mut(), &session_id, crate::cookie_secure());
    Ok(resp)
}

/// `GET /api/license/status` — current license claims (no signature).
pub async fn license_status() -> Json<pointer_core::license::LicenseStatusView> {
    Json(pointer_core::license::active_license_status_view())
}

/// `POST /api/license/reload` — re-read license key from env / config file mapping.
pub async fn license_reload(
) -> Result<Json<pointer_core::license::LicenseStatusView>, (StatusCode, String)> {
    match pointer_core::license::reload_license_from_env() {
        Ok(view) => Ok(Json(view)),
        Err(e) => {
            log::warn!("license: reload failed: {e:#}");
            Err((StatusCode::BAD_REQUEST, e.to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captcha_verify_consumes_once() {
        let store = CaptchaStore::default();
        let (id, answer) = store.issue();
        assert!(store.verify_and_consume(&id, &answer));
        assert!(!store.verify_and_consume(&id, &answer));
    }

    #[test]
    fn captcha_mismatch_fails() {
        let store = CaptchaStore::default();
        let (id, _) = store.issue();
        assert!(!store.verify_and_consume(&id, "XXXX"));
    }

    #[test]
    fn captcha_svg_contains_chars() {
        let svg = render_captcha_svg("AB12");
        assert!(svg.contains("AB12".chars().next().unwrap()) || svg.contains(">A<"));
        assert!(svg.contains("<svg"));
        assert!(svg.contains(">A<") || svg.contains("A</text>"));
        // Valid CSS hex colors (a mistaken `##rrggbb` paints the whole tile black).
        assert!(svg.contains("fill=\"#f3f4f6\""));
        assert!(svg.contains("fill=\"#1f2937\""));
        assert!(svg.contains("stroke=\"#c5c9d1\""));
        assert!(!svg.contains("##"));
    }
}
