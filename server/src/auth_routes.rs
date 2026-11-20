//! Browser login routes: one entry per configured provider, one session exit.
//!
//! Everything the login page can do is derived from
//! [`pointer_core::auth_provider::AuthProviderRegistry`]:
//!
//! * `GET  /api/auth/providers`            → what to render
//! * `POST /api/auth/login/start[?provider]` → how to begin (redirect URL / captcha)
//! * `POST /api/auth/login/finish[?provider]` → submit credentials (`local_password`)
//! * `GET  /api/auth/callback/:provider`   → how to finish a redirect login
//!
//! Every successful login ends in [`issue_session`], the only place a web
//! session is created. Failures never put provider text in the redirect: the
//! browser only ever sees a [`LoginError`] code.

use axum::{
    extract::{ConnectInfo, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Redirect, Response},
    Json,
};
use pointer_core::auth_rate_limit::{self, StartVerdict};
use std::net::SocketAddr;

use pointer_core::auth_provider::{
    AuthCodeRequest, AuthProviderKind, AuthorizeRequest, LoginEntry, PasswordCredentials,
};
use pointer_core::platform_auth::{PlatformAuthManager, PlatformLoginCredentials};
use pointer_core::web_request_auth::WebSessionAuthKind;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

use crate::auth_pending::{self, PendingLogin};
use crate::web_session::{self, WebSessionStore};
use crate::ServerState;

/// Closed set of login failures exposed to the browser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginError {
    MissingCode,
    InvalidState,
    UnknownProvider,
    ProviderDisabled,
    ProviderDenied,
    ExchangeFailed,
    ServerAccessDenied,
    PasswordLoginRequired,
    TicketEntryOnly,
    TicketMissing,
    TicketExpired,
    TicketReplayed,
    TicketSignatureInvalid,
    TicketInvalid,
    UpstreamError,
}

impl LoginError {
    pub fn as_code(self) -> &'static str {
        match self {
            LoginError::MissingCode => "missing_code",
            LoginError::InvalidState => "invalid_state",
            LoginError::UnknownProvider => "unknown_provider",
            LoginError::ProviderDisabled => "provider_disabled",
            LoginError::ProviderDenied => "provider_denied",
            LoginError::ExchangeFailed => "exchange_failed",
            LoginError::ServerAccessDenied => "server_access_denied",
            LoginError::PasswordLoginRequired => "password_login_required",
            LoginError::TicketEntryOnly => "ticket_entry_only",
            LoginError::TicketMissing => "ticket_missing",
            LoginError::TicketExpired => "ticket_expired",
            LoginError::TicketReplayed => "ticket_replayed",
            LoginError::TicketSignatureInvalid => "ticket_signature_invalid",
            LoginError::TicketInvalid => "ticket_invalid",
            LoginError::UpstreamError => "upstream_error",
        }
    }

    /// Browser redirect back to the SPA with an error code (never a message).
    pub fn redirect(self) -> Response {
        Redirect::temporary(&format!("/?platform_login_error={}", self.as_code())).into_response()
    }
}

/// Map a provider failure onto a specific browser-facing code.
///
/// The driver returns prose (anyhow), so this is the one place that turns it
/// into the closed set of codes: missing / expired / replayed / bad signature /
/// bad claims / upstream. A generic `exchange_failed` hides exactly the
/// difference an operator needs when a portal ticket is rejected.
fn classify_provider_failure(message: &str) -> LoginError {
    let msg = message.to_ascii_lowercase();
    if msg.contains("ticket is empty") {
        return LoginError::TicketMissing;
    }
    if msg.contains("ticket expired") {
        return LoginError::TicketExpired;
    }
    if msg.contains("already used") || msg.contains("replay") {
        return LoginError::TicketReplayed;
    }
    if msg.contains("signature")
        || msg.contains("not a compact jws")
        || msg.contains("not base64url")
        || msg.contains("not json")
        || msg.contains("alg `")
    {
        return LoginError::TicketSignatureInvalid;
    }
    if msg.contains("ticket has no subject") || msg.contains("audience does not match") {
        return LoginError::TicketInvalid;
    }
    if msg.contains("ticket lookup")
        || msg.contains("could not be sent")
        || msg.contains("timed out")
        || msg.contains("connection")
        || msg.contains("dns")
    {
        return LoginError::UpstreamError;
    }
    LoginError::ExchangeFailed
}

/// Result of a successful login: the cookie value and who it belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssuedSession {
    pub session_id: String,
    pub user_id: String,
}

/// The single session exit. Order is fixed:
/// access allowlist → session store → cookie.
///
/// `enforce_allowlist` comes from
/// [`AuthProviderKind::requires_allowlist`]: the operator's own `local_password`
/// account is the break-glass login and is exempt, every federated identity is
/// checked. Split out from [`issue_session`] so it can be tested without a
/// whole `ServerState`.
pub(crate) fn issue_session_into(
    store: &WebSessionStore,
    headers: &mut HeaderMap,
    auth: Arc<PlatformAuthManager>,
    creds: PlatformLoginCredentials,
    kind: WebSessionAuthKind,
    enforce_allowlist: bool,
    secure: bool,
) -> Result<IssuedSession, LoginError> {
    let user_id = auth.platform_user_id().unwrap_or_default();
    if enforce_allowlist
        && pointer_core::server_access::access_restriction_enabled()
        && !pointer_core::server_access::is_user_allowed(&user_id)
    {
        log::warn!("server_access: rejected login user_id={user_id} not in allowed_user_ids");
        return Err(LoginError::ServerAccessDenied);
    }
    let session_id = store.insert(auth, creds, kind);
    web_session::set_session_cookie(headers, &session_id, secure);
    Ok(IssuedSession {
        session_id,
        user_id,
    })
}

/// Issue a browser session for an already-authenticated user.
pub(crate) fn issue_session(
    state: &ServerState,
    headers: &mut HeaderMap,
    auth: Arc<PlatformAuthManager>,
    creds: PlatformLoginCredentials,
    kind: WebSessionAuthKind,
    provider_kind: AuthProviderKind,
) -> Result<IssuedSession, LoginError> {
    let issued = issue_session_into(
        &state.web_sessions,
        headers,
        auth,
        creds,
        kind,
        provider_kind.requires_allowlist(),
        crate::cookie_secure(),
    )?;
    crate::sync_automation_web_session(state);
    Ok(issued)
}

#[derive(Debug, Serialize)]
pub struct ProvidersResponse {
    pub providers: Vec<pointer_core::auth_provider::ProviderView>,
}

/// `GET /api/auth/providers` — enabled providers for the login page.
pub async fn list_providers(State(state): State<ServerState>) -> Json<ProvidersResponse> {
    Json(ProvidersResponse {
        providers: state.auth_providers.enabled_views(),
    })
}

#[derive(Debug, Serialize)]
pub struct AuthModeResponse {
    pub mode: &'static str,
}

/// `GET /api/auth/mode` — deployment mode, so the SPA can pick its login view.
pub async fn auth_mode() -> Json<AuthModeResponse> {
    let mode = if pointer_core::deployment_mode::is_standalone() {
        "standalone"
    } else {
        "platform"
    };
    Json(AuthModeResponse { mode })
}

#[derive(Debug, Default, Deserialize)]
pub struct LoginStartQuery {
    /// Registry id; omitted uses the deployment default.
    #[serde(default)]
    provider: Option<String>,
}

/// `POST /api/auth/login/start[?provider=<id>]` — begin a browser login.
///
/// `local_password` is the one non-redirect entry: it only hands back a captcha
/// challenge. Credentials are submitted to [`finish_login`].
pub async fn start_login(
    State(state): State<ServerState>,
    Query(q): Query<LoginStartQuery>,
    peer: Option<ConnectInfo<SocketAddr>>,
    headers: HeaderMap,
) -> Result<Response, (StatusCode, String)> {
    let client = client_key(&state, peer.as_ref().map(|info| info.0), &headers);
    match state.login_limiter.check_start(&client, auth_rate_limit::now_ms()) {
        StartVerdict::Allow => {}
        StartVerdict::RateLimited { retry_after_secs } => {
            log::warn!("auth: login/start rate limited client={client} retry_after={retry_after_secs}s");
            return Err((StatusCode::TOO_MANY_REQUESTS, "rate_limited".to_string()));
        }
        StartVerdict::Backoff { retry_after_secs } => {
            log::warn!("auth: login/start during failure backoff client={client} retry_after={retry_after_secs}s");
            return Err((StatusCode::TOO_MANY_REQUESTS, "too_many_attempts".to_string()));
        }
    }
    match state.auth_providers.login_entry(q.provider.as_deref()) {
        LoginEntry::Redirect { id, .. } => start_redirect_login(&state, id).await,
        LoginEntry::Platform { id } => start_platform_login(&state, id),
        LoginEntry::LocalPassword { id } => Ok(local_password_challenge(&state, id)),
        LoginEntry::Ticket { id, .. } => {
            log::warn!("auth: `{id}` is entered by an inbound ticket, not a login start");
            Err(bad_request(LoginError::TicketEntryOnly))
        }
        LoginEntry::Disabled { id } => {
            log::warn!("auth: login start for disabled provider `{id}`");
            Err(bad_request(LoginError::ProviderDisabled))
        }
        LoginEntry::Unknown => Err(bad_request(LoginError::UnknownProvider)),
    }
}

/// `POST /api/auth/login/finish[?provider=<id>]` — submit credentials.
///
/// Only `local_password` takes credentials; every redirect provider finishes at
/// its callback, so it is rejected here instead of silently ignoring the body.
pub async fn finish_login(
    State(state): State<ServerState>,
    Query(q): Query<LoginStartQuery>,
    peer: Option<ConnectInfo<SocketAddr>>,
    headers: HeaderMap,
    Json(input): Json<LocalLoginBody>,
) -> Result<Response, (StatusCode, String)> {
    let client = client_key(&state, peer.as_ref().map(|info| info.0), &headers);
    let now = auth_rate_limit::now_ms();
    if let Some(retry_after_secs) = state.login_limiter.in_backoff(&client, now) {
        log::warn!("auth: login/finish during backoff client={client} retry_after={retry_after_secs}s");
        return Err((StatusCode::TOO_MANY_REQUESTS, "too_many_attempts".to_string()));
    }
    let result = match state.auth_providers.login_entry(q.provider.as_deref()) {
        LoginEntry::LocalPassword { id } => local_password_finish(&state, id, input).await,
        LoginEntry::Redirect { id, .. } | LoginEntry::Platform { id } => {
            log::warn!("auth: credential submit for redirect provider `{id}`");
            Err(bad_request(LoginError::PasswordLoginRequired))
        }
        LoginEntry::Ticket { id, .. } => {
            log::warn!("auth: credential submit for ticket provider `{id}`");
            Err(bad_request(LoginError::TicketEntryOnly))
        }
        LoginEntry::Disabled { id } => {
            log::warn!("auth: credential submit for disabled provider `{id}`");
            Err(bad_request(LoginError::ProviderDisabled))
        }
        LoginEntry::Unknown => Err(bad_request(LoginError::UnknownProvider)),
    };
    // A rejected credential attempt starts a cooldown for that client; a success
    // clears it so a user who just fat-fingered once is not punished.
    match &result {
        Ok(_) => state.login_limiter.record_finish_success(&client),
        Err((status, _)) if *status == StatusCode::UNAUTHORIZED => {
            state.login_limiter.record_finish_failure(&client, now)
        }
        Err(_) => {}
    }
    result
}

/// Rate-limit bucket for this request (trusted proxies decide whether the
/// forwarded header is read at all — see `client_ip`).
fn client_key(
    state: &ServerState,
    peer: Option<SocketAddr>,
    headers: &HeaderMap,
) -> String {
    let trusted = &state.login_limiter.config().trusted_proxies;
    crate::client_ip::client_key(crate::client_ip::resolve_client_ip(
        peer.map(|addr| addr.ip()),
        headers,
        trusted,
    ))
}

fn pending_start_error(provider_id: &str, error: auth_pending::PendingError) -> (StatusCode, String) {
    match error {
        auth_pending::PendingError::AtCapacity => {
            log::warn!("auth: in-flight login capacity reached provider={provider_id}");
            (
                StatusCode::TOO_MANY_REQUESTS,
                "login_capacity_reached".to_string(),
            )
        }
        other => {
            log::warn!("auth: cannot record pending login provider={provider_id}: {other:?}");
            (StatusCode::BAD_REQUEST, "invalid_state".to_string())
        }
    }
}

fn bad_request(error: LoginError) -> (StatusCode, String) {
    (StatusCode::BAD_REQUEST, error.as_code().to_string())
}

/// Start a code flow for a redirect provider (`oauth2_userinfo` / `oidc`).
async fn start_redirect_login(
    state: &ServerState,
    provider_id: &str,
) -> Result<Response, (StatusCode, String)> {
    let max_pending = state.login_limiter.config().max_pending;
    if !state.oauth_pending.has_capacity(max_pending) {
        log::warn!("auth: in-flight login capacity reached provider={provider_id}");
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            "login_capacity_reached".to_string(),
        ));
    }
    let public_url = public_url()?;
    let driver = state.auth_providers.driver(provider_id).map_err(|e| {
        log::warn!("auth provider: cannot start login for `{provider_id}`: {e:#}");
        bad_request(LoginError::UnknownProvider)
    })?;
    let redirect_uri = format!("{public_url}{}", callback_path(state, provider_id));
    let (verifier, challenge) = PlatformAuthManager::generate_pkce();
    let state_token = auth_pending::random_token();
    let nonce = auth_pending::random_token();
    let url = driver
        .authorize_url(&AuthorizeRequest {
            redirect_uri: &redirect_uri,
            state: &state_token,
            code_challenge: Some(&challenge),
            nonce: Some(&nonce),
        })
        .await
        .map_err(|e| {
            log::warn!("auth provider: authorize url failed for `{provider_id}`: {e:#}");
            (
                StatusCode::BAD_GATEWAY,
                "provider_authorize_failed".to_string(),
            )
        })?
        .ok_or_else(|| {
            log::warn!("auth provider: `{provider_id}` has no authorize endpoint");
            bad_request(LoginError::UnknownProvider)
        })?;
    state
        .oauth_pending
        .start_capped(
            &state_token,
            PendingLogin::new(provider_id, verifier, redirect_uri).with_nonce(nonce),
            max_pending,
        )
        .map_err(|e| pending_start_error(provider_id, e))?;
    log::info!("auth provider: login start provider={provider_id}");
    Ok(authorize_response(&state_token, &url))
}

/// Start the cloud control-plane OAuth flow.
fn start_platform_login(
    state: &ServerState,
    provider_id: &str,
) -> Result<Response, (StatusCode, String)> {
    let max_pending = state.login_limiter.config().max_pending;
    if !state.oauth_pending.has_capacity(max_pending) {
        log::warn!("auth: in-flight login capacity reached provider={provider_id}");
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            "login_capacity_reached".to_string(),
        ));
    }
    let public_url = public_url()?;
    let redirect_uri = format!("{public_url}{}", callback_path(state, provider_id));
    let (verifier, challenge) = PlatformAuthManager::generate_pkce();
    let state_token = auth_pending::random_token();
    // 服务端用自己的 client_id（默认 pointer-server）：桌面端的 pointer-desktop
    // 只接受 loopback 回跳，服务端的外部地址会被拒。
    //
    // 这个 id 要随 pending 一起存下来——控制面把授权码绑定到客户端，换码时
    // 换成别的 id 会被判「授权码与客户端不匹配」。
    let client_id = pointer_core::platform_endpoints::server_oauth_client_id();
    state
        .oauth_pending
        .start_capped(
            &state_token,
            PendingLogin::new(provider_id, verifier, redirect_uri.clone())
                .with_client_id(client_id.clone()),
            max_pending,
        )
        .map_err(|e| pending_start_error(provider_id, e))?;
    let url = PlatformAuthManager::build_authorize_url_for_redirect_as(
        &client_id,
        &redirect_uri,
        &challenge,
        &state_token,
    );
    log::info!("platform_auth: login start state={state_token}");
    Ok(authorize_response(&state_token, &url))
}

fn authorize_response(state_token: &str, url: &str) -> Response {
    let mut resp = Json(json!({ "authorize_url": url })).into_response();
    auth_pending::set_state_cookie(resp.headers_mut(), state_token, crate::cookie_secure());
    resp
}

/// Callback path this provider is registered under (configured, else unified).
fn callback_path(state: &ServerState, provider_id: &str) -> String {
    state
        .auth_providers
        .get(provider_id)
        .map(|config| config.callback_path(provider_id))
        .unwrap_or_else(|| pointer_core::auth_provider::default_callback_path(provider_id))
}

fn public_url() -> Result<String, (StatusCode, String)> {    crate::resolve_server_public_url().ok_or_else(|| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "POINTER_SERVER_PUBLIC_URL 未配置；请在 pointer-server.toml [server].public_url 设置外部可达地址"
                .to_string(),
        )
    })
}

/// `GET /api/auth/callback/:provider` — terminal hop for every provider kind.
pub async fn provider_callback(
    State(state): State<ServerState>,
    Path(provider_id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Response {
    provider_callback_for(&state, &provider_id, params, headers).await
}

/// Terminal hop for a provider whose callback path is configured explicitly
/// (`callback_path`, e.g. the control plane's legacy path).
pub async fn provider_callback_for(
    state: &ServerState,
    provider_id: &str,
    params: HashMap<String, String>,
    headers: HeaderMap,
) -> Response {
    match state.auth_providers.login_entry(Some(provider_id)) {
        LoginEntry::Platform { id } => platform_callback(state, id, &params, &headers).await,
        LoginEntry::Redirect { id, .. } => code_callback(state, id, &params, &headers).await,
        LoginEntry::Ticket { id, config } => {
            ticket_callback(state, id, config.token_lookup.ticket_param(), &params).await
        }
        LoginEntry::LocalPassword { .. } => LoginError::PasswordLoginRequired.redirect(),
        LoginEntry::Disabled { id } => {
            log::warn!("auth: callback for disabled provider `{id}`");
            LoginError::ProviderDisabled.redirect()
        }
        LoginEntry::Unknown => LoginError::UnknownProvider.redirect(),
    }
}

/// Finish a code flow: verify the browser binding, then exchange the code.
async fn code_callback(
    state: &ServerState,
    provider_id: &str,
    params: &HashMap<String, String>,
    headers: &HeaderMap,
) -> Response {
    let pending = match redeem_pending(state, headers, params, provider_id) {
        Ok(pending) => pending,
        Err(err) => {
            log::warn!("auth provider: callback rejected provider={provider_id} code={}", err.as_code());
            return err.redirect();
        }
    };
    if let Some(error) = params.get("error").map(|value| value.trim()).filter(|v| !v.is_empty()) {
        log::warn!("auth provider: provider={provider_id} returned error={error}");
        return LoginError::ProviderDenied.redirect();
    }
    let code = params.get("code").map(|value| value.trim()).unwrap_or("");
    if code.is_empty() {
        log::warn!("auth provider: callback missing code provider={provider_id}");
        return LoginError::MissingCode.redirect();
    }
    let driver = match state.auth_providers.driver(provider_id) {
        Ok(driver) => driver,
        Err(e) => {
            log::warn!("auth provider: callback for unusable provider `{provider_id}`: {e:#}");
            return LoginError::ProviderDisabled.redirect();
        }
    };
    let identity = match driver
        .authenticate(AuthCodeRequest {
            code,
            redirect_uri: &pending.redirect_uri,
            code_verifier: Some(&pending.verifier),
            nonce: pending.nonce.as_deref(),
        })
        .await
    {
        Ok(identity) => identity,
        Err(e) => {
            let code = classify_provider_failure(&format!("{e:#}"));
            log::warn!(
                "auth provider: login failed provider={provider_id} code={}: {e:#}",
                code.as_code()
            );
            return code.redirect();
        }
    };
    let kind = driver.kind();
    finish_federated_login(state, &identity.provider_id, &identity.subject, identity_nickname(&identity.claims), kind)
}

/// Finish the cloud control-plane flow.
async fn platform_callback(
    state: &ServerState,
    provider_id: &str,
    params: &HashMap<String, String>,
    headers: &HeaderMap,
) -> Response {
    let pending = match redeem_pending(state, headers, params, provider_id) {
        Ok(pending) => pending,
        Err(err) => {
            log::warn!("platform_auth: callback rejected code={}", err.as_code());
            return err.redirect();
        }
    };
    let code = params.get("code").map(|value| value.trim()).unwrap_or("");
    if code.is_empty() {
        log::warn!("platform_auth: callback missing code");
        return LoginError::MissingCode.redirect();
    }
    let state_token = params.get("state").map(String::as_str).unwrap_or_default();
    let auth = Arc::new(PlatformAuthManager::new());
    let (session, creds) = match auth
        .exchange_authorization_code(
            &pending.client_id,
            code,
            &pending.verifier,
            state_token,
            &pending.redirect_uri,
        )
        .await
    {
        Ok(result) => result,
        Err(e) => {
            log::warn!("platform_auth: exchange failed: {e:#}");
            return LoginError::ExchangeFailed.redirect();
        }
    };
    auth.set_session(session);
    let mut resp = success_redirect();
    match issue_session(
        state,
        resp.headers_mut(),
        auth,
        creds,
        WebSessionAuthKind::Platform,
        AuthProviderKind::Platform,
    ) {
        Ok(issued) => {
            log::info!(
                "platform_auth: login ok provider={provider_id} web_session={}",
                issued.session_id
            );
            resp
        }
        Err(err) => err.redirect(),
    }
}

/// Finish an inbound ticket login (the portal started it, not us).
async fn ticket_callback(
    state: &ServerState,
    provider_id: &str,
    ticket_param: &str,
    params: &HashMap<String, String>,
) -> Response {
    let ticket = params
        .get(ticket_param)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let Some(ticket) = ticket else {
        log::warn!("auth provider: callback missing `{ticket_param}` provider={provider_id}");
        return LoginError::TicketMissing.redirect();
    };
    let driver = match state.auth_providers.driver(provider_id) {
        Ok(driver) => driver,
        Err(e) => {
            log::warn!("auth provider: ticket callback for unusable `{provider_id}`: {e:#}");
            return LoginError::ProviderDisabled.redirect();
        }
    };
    let identity = match driver.authenticate_ticket(&ticket).await {
        Ok(identity) => identity,
        Err(e) => {
            let code = classify_provider_failure(&format!("{e:#}"));
            log::warn!(
                "auth provider: ticket login failed provider={provider_id} code={}: {e:#}",
                code.as_code()
            );
            return code.redirect();
        }
    };
    finish_federated_login(
        state,
        &identity.provider_id,
        &identity.subject,
        identity_nickname(&identity.claims),
        AuthProviderKind::TokenLookup,
    )
}

/// Shared tail for every federated identity: session user id, session, cookie.
fn finish_federated_login(
    state: &ServerState,
    provider_id: &str,
    subject: &str,
    nickname: Option<String>,
    provider_kind: AuthProviderKind,
) -> Response {
    let user_id = federated_user_id(provider_id, subject);
    let auth = pointer_core::local_auth::create_local_auth_manager_for_user(
        &user_id,
        nickname,
        false,
    );
    let creds = pointer_core::local_auth::empty_local_credentials();
    let mut resp = success_redirect();
    match issue_session(
        state,
        resp.headers_mut(),
        auth,
        creds,
        WebSessionAuthKind::Local,
        provider_kind,
    ) {
        Ok(issued) => {
            log::info!(
                "auth provider: login ok provider={provider_id} user_id={user_id} web_session={}",
                issued.session_id
            );
            resp
        }
        Err(err) => err.redirect(),
    }
}

fn success_redirect() -> Response {
    let mut resp = Redirect::temporary("/?auth=success").into_response();
    auth_pending::clear_state_cookie(resp.headers_mut(), crate::cookie_secure());
    resp
}

/// Verify the browser binding, then redeem the pending entry exactly once.
fn redeem_pending(
    state: &ServerState,
    headers: &HeaderMap,
    params: &HashMap<String, String>,
    provider_id: &str,
) -> Result<PendingLogin, LoginError> {
    let state_token = params.get("state").map(String::as_str).unwrap_or_default();
    let cookie_state = auth_pending::state_from_headers(headers);
    if !auth_pending::state_cookie_matches(cookie_state.as_deref(), state_token) {
        // 两种原因都报 invalid_state，日志里必须能分开：浏览器没带回 cookie
        // （换了窗口/设备、被第三方 cookie 策略拦掉），还是 cookie 与 query 的
        // state 不一致（页面被并发发起过两次）。
        // 不打印 state 本身——它等同于一次性的登录凭据。
        log::warn!(
            "platform_auth: invalid_state reason=cookie provider={provider_id} \
             cookie_present={} cookie_matches_query={} query_state_present={}",
            cookie_state.is_some(),
            cookie_state
                .as_deref()
                .is_some_and(|c| c.trim() == state_token.trim()),
            !state_token.is_empty()
        );
        return Err(LoginError::InvalidState);
    }
    match state.oauth_pending.take(state_token, provider_id) {
        Ok(pending) => Ok(pending),
        Err(err) => {
            // InvalidState = 内存里没有这条（过期 / 已被兑换 / 服务重启）
            // ProviderMismatch = 发起与回调解析到不同的 provider id
            log::warn!(
                "platform_auth: invalid_state reason=pending err={err:?} provider={provider_id}"
            );
            Err(LoginError::InvalidState)
        }
    }
}

/// Session user id for a federated identity. Capped so a hostile provider
/// cannot push an unbounded id into conversation rows.
fn federated_user_id(provider_id: &str, subject: &str) -> String {
    const MAX_SUBJECT_LEN: usize = 120;
    let subject: String = subject.trim().chars().take(MAX_SUBJECT_LEN).collect();
    format!("{}:{}", provider_id.trim(), subject)
}

fn identity_nickname(claims: &Value) -> Option<String> {
    for key in ["name", "preferred_username", "nickname", "email"] {
        if let Some(value) = claims.get(key).and_then(Value::as_str) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

#[derive(Deserialize)]
pub struct LocalLoginBody {
    pub username: String,
    pub password: String,
    #[serde(rename = "captchaId")]
    pub captcha_id: String,
    pub captcha: String,
}

/// `login/start` for `local_password`: hand back a one-time captcha challenge.
fn local_password_challenge(state: &ServerState, provider_id: &str) -> Response {
    let (captcha_id, answer) = state.captcha_store.issue();
    Json(json!({
        "provider": provider_id,
        "kind": AuthProviderKind::LocalPassword.as_str(),
        "captcha": {
            "captchaId": captcha_id,
            "imageSvg": crate::captcha::render_captcha_svg(&answer),
        }
    }))
    .into_response()
}

/// `login/finish` for `local_password`: captcha + password → session.
async fn local_password_finish(
    state: &ServerState,
    provider_id: &str,
    input: LocalLoginBody,
) -> Result<Response, (StatusCode, String)> {
    let driver = state.auth_providers.driver(provider_id).map_err(|e| {
        log::warn!("local_auth: local_password driver unavailable: {e:#}");
        bad_request(LoginError::ProviderDisabled)
    })?;
    pointer_core::local_auth::warn_if_deprecated_admin_token_configured();

    if !state
        .captcha_store
        .verify_and_consume(&input.captcha_id, &input.captcha)
    {
        log::warn!("local_auth: rejected login — invalid captcha");
        return Err((StatusCode::UNAUTHORIZED, "invalid_captcha".into()));
    }
    let identity = driver
        .authenticate_password(&PasswordCredentials {
            username: &input.username,
            password: &input.password,
        })
        .await
        .map_err(|e| {
            log::warn!("local_auth: rejected password login attempt: {e:#}");
            (StatusCode::UNAUTHORIZED, "invalid_credentials".to_string())
        })?;

    let auth = pointer_core::local_auth::create_local_auth_manager();
    let creds = pointer_core::local_auth::empty_local_credentials();
    let view = auth.session_view();
    let mut resp = Json(view).into_response();
    match issue_session(
        state,
        resp.headers_mut(),
        auth,
        creds,
        WebSessionAuthKind::Local,
        AuthProviderKind::LocalPassword,
    ) {
        Ok(issued) => {
            log::info!(
                "local_auth: login ok user_id={} web_session={}",
                identity.subject,
                issued.session_id
            );
            Ok(resp)
        }
        Err(err) => Err(bad_request(err)),
    }
}

/// `GET /api/license/status` — current license claims (no signature).
pub async fn license_status() -> Json<pointer_core::license::LicenseStatusView> {
    Json(pointer_core::license::active_license_status_view())
}

/// `POST /api/license/reload` — re-read the license key from env / config file.
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

/// Register a dedicated route for every provider whose `callback_path` differs
/// from the unified `/api/auth/callback/:provider`.
pub fn with_callback_routes(
    app: axum::Router<ServerState>,
    registry: &pointer_core::auth_provider::AuthProviderRegistry,
) -> axum::Router<ServerState> {
    let mut app = app;
    for (provider_id, path) in registry.callback_routes() {
        log::info!("pointer-server: `{provider_id}` callback path {path}");
        let provider = provider_id.to_string();
        app = app.route(
            &path,
            axum::routing::get(
                move |State(state): State<ServerState>,
                      Query(params): Query<HashMap<String, String>>,
                      headers: HeaderMap| {
                    let provider = provider.clone();
                    async move { provider_callback_for(&state, &provider, params, headers).await }
                },
            ),
        );
    }
    app
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local_auth(user_id: &str) -> Arc<PlatformAuthManager> {
        pointer_core::local_auth::create_local_auth_manager_for_user(user_id, None, false)
    }

    fn cookie_header(headers: &HeaderMap) -> String {
        headers
            .get(axum::http::header::SET_COOKIE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string()
    }

    fn issue(
        store: &WebSessionStore,
        headers: &mut HeaderMap,
        enforce_allowlist: bool,
        secure: bool,
    ) -> Result<IssuedSession, LoginError> {
        issue_session_into(
            store,
            headers,
            local_auth("local-admin"),
            pointer_core::local_auth::empty_local_credentials(),
            WebSessionAuthKind::Local,
            enforce_allowlist,
            secure,
        )
    }

    #[test]
    fn issue_session_stores_the_session_and_sets_the_cookie() {
        let store = WebSessionStore::default();
        let mut headers = HeaderMap::new();
        let issued = issue(&store, &mut headers, false, false).expect("session issued");

        assert_eq!(issued.user_id, "local-admin");
        assert_eq!(issued.session_id.len(), 32);
        let cookie = cookie_header(&headers);
        assert!(cookie.contains(&format!("pointer_web_session={}", issued.session_id)));
        assert!(cookie.contains("HttpOnly"));
        assert!(!cookie.contains("Secure"));

        let entry = store.get(&issued.session_id).expect("session stored");
        assert_eq!(entry.kind, WebSessionAuthKind::Local);
        assert_eq!(entry.auth.platform_user_id().as_deref(), Some("local-admin"));
    }

    #[test]
    fn issue_session_marks_secure_cookies_behind_https() {
        let store = WebSessionStore::default();
        let mut headers = HeaderMap::new();
        issue(&store, &mut headers, false, true).expect("session issued");
        assert!(cookie_header(&headers).contains("; Secure"));
    }

    #[test]
    fn local_password_login_is_not_gated_by_the_allowlist() {
        // `local_password` is the break-glass account: `requires_allowlist` is
        // false, so the gate is skipped even when a restriction is configured.
        assert!(!AuthProviderKind::LocalPassword.requires_allowlist());
        for kind in [
            AuthProviderKind::Platform,
            AuthProviderKind::Oidc,
            AuthProviderKind::Oauth2Userinfo,
            AuthProviderKind::TokenLookup,
        ] {
            assert!(kind.requires_allowlist(), "{kind:?} must be gated");
        }
    }

    #[test]
    fn login_error_codes_are_stable() {
        assert_eq!(LoginError::MissingCode.as_code(), "missing_code");
        assert_eq!(LoginError::InvalidState.as_code(), "invalid_state");
        assert_eq!(LoginError::UnknownProvider.as_code(), "unknown_provider");
        assert_eq!(LoginError::ProviderDisabled.as_code(), "provider_disabled");
        assert_eq!(LoginError::ProviderDenied.as_code(), "provider_denied");
        assert_eq!(LoginError::ExchangeFailed.as_code(), "exchange_failed");
        assert_eq!(
            LoginError::ServerAccessDenied.as_code(),
            "server_access_denied"
        );
        assert_eq!(
            LoginError::PasswordLoginRequired.as_code(),
            "password_login_required"
        );
        assert_eq!(LoginError::TicketEntryOnly.as_code(), "ticket_entry_only");
        assert_eq!(LoginError::TicketMissing.as_code(), "ticket_missing");
    }

    #[test]
    fn federated_user_id_namespaces_the_subject() {
        assert_eq!(federated_user_id("dingtalk", "union-1"), "dingtalk:union-1");
        assert_eq!(federated_user_id("corp_sso", "  "), "corp_sso:");
        let long = "x".repeat(400);
        let id = federated_user_id("oidc", &long);
        assert_eq!(id, format!("oidc:{}", "x".repeat(120)));
    }

    #[test]
    fn identity_nickname_prefers_name_then_falls_back() {
        assert_eq!(
            identity_nickname(&json!({"name": "张三", "email": "a@b.c"})),
            Some("张三".to_string())
        );
        assert_eq!(
            identity_nickname(&json!({"preferred_username": "zhangsan"})),
            Some("zhangsan".to_string())
        );
        assert_eq!(
            identity_nickname(&json!({"email": "a@b.c"})),
            Some("a@b.c".to_string())
        );
        assert_eq!(identity_nickname(&json!({"sub": "u-1"})), None);
        assert_eq!(identity_nickname(&json!({"name": "   "})), None);
    }
}

#[cfg(test)]
mod ticket_failure_tests {
    use super::{classify_provider_failure, LoginError};

    /// Each ticket failure the driver can report must keep its own code —
    /// `exchange_failed` for all of them told an operator nothing.
    #[test]
    fn ticket_failures_keep_their_own_codes() {
        let cases = [
            ("auth provider `portal`: ticket is empty", "ticket_missing"),
            ("auth provider `portal`: ticket expired", "ticket_expired"),
            ("auth provider `portal`: ticket was already used", "ticket_replayed"),
            (
                "auth provider `portal`: ticket signature is not valid",
                "ticket_signature_invalid",
            ),
            (
                "auth provider `portal`: ticket is not a compact JWS",
                "ticket_signature_invalid",
            ),
            (
                "auth provider `portal`: ticket has no subject at `sub`",
                "ticket_invalid",
            ),
            (
                "auth provider `portal`: ticket audience does not match",
                "ticket_invalid",
            ),
            (
                "auth provider `portal`: ticket lookup failed (HTTP 502): bad gateway",
                "upstream_error",
            ),
            ("auth provider `oidc`: discovery document has no `token_endpoint`", "exchange_failed"),
        ];
        for (message, expected) in cases {
            let code = classify_provider_failure(message).as_code();
            assert_eq!(code, expected, "message: {message}");
        }
    }

    #[test]
    fn codes_are_distinct_so_the_browser_can_translate_them() {
        let codes = [
            LoginError::TicketMissing.as_code(),
            LoginError::TicketExpired.as_code(),
            LoginError::TicketReplayed.as_code(),
            LoginError::TicketSignatureInvalid.as_code(),
            LoginError::TicketInvalid.as_code(),
            LoginError::UpstreamError.as_code(),
            LoginError::ExchangeFailed.as_code(),
        ];
        let mut unique = codes.to_vec();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), codes.len(), "duplicate login error codes: {codes:?}");
    }
}
