//! Standalone admin-token login handlers for pointer-server.

use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use pointer_core::{
    local_auth::{create_local_auth_manager, empty_local_credentials, verify_admin_token},
    platform_auth::PlatformSessionView,
    web_request_auth::WebSessionAuthKind,
};
use serde::Deserialize;

use crate::web_session;
use crate::ServerState;

#[derive(Deserialize)]
pub struct LocalLoginBody {
    pub token: String,
}

/// `POST /api/auth/local/login` — authenticate with the configured admin token.
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
    if !verify_admin_token(&body.token) {
        log::warn!("local_auth: rejected admin token login attempt");
        return Err((StatusCode::UNAUTHORIZED, "invalid_token".into()));
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
    web_session::set_session_cookie(
        resp.headers_mut(),
        &session_id,
        crate::cookie_secure(),
    );
    Ok(resp)
}

/// `GET /api/license/status` — current license claims (no signature).
pub async fn license_status(
) -> Json<pointer_core::license::LicenseStatusView> {
    Json(pointer_core::license::active_license_status_view())
}

/// `POST /api/license/reload` — re-read license key from env / config file mapping.
pub async fn license_reload() -> Result<Json<pointer_core::license::LicenseStatusView>, (StatusCode, String)> {
    match pointer_core::license::reload_license_from_env() {
        Ok(view) => Ok(Json(view)),
        Err(e) => {
            log::warn!("license: reload failed: {e:#}");
            Err((StatusCode::BAD_REQUEST, e.to_string()))
        }
    }
}
