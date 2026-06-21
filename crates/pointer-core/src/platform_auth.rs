//! Pointer desktop OAuth (PKCE + refresh token + encrypted local persistence).

use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::Utc;
use std::collections::HashMap;

use parking_lot::RwLock;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::platform_endpoints;
use crate::storage;

const DEFAULT_LOOPBACK_PORT: u16 = 19427;
/// 从首选端口起依次尝试绑定（含首选共 N 个端口）。
const LOOPBACK_PORT_SCAN_COUNT: u16 = 32;
const EXPIRY_BUFFER_SEC: i64 = 300;
/// 等待浏览器 OAuth 回调的最长时间（秒）。
pub const OAUTH_CALLBACK_TIMEOUT_SEC: u64 = 300;
/// 桌面 OAuth 成功后跳转官网首页时携带的 query 名；官网据此展示一次性提示（见 `docs/internals/desktop-oauth-web-integration.md`）。
pub const DESKTOP_OAUTH_SUCCESS_QUERY: &str = "desktop_oauth";
/// 与 [`DESKTOP_OAUTH_SUCCESS_QUERY`] 搭配的值。
pub const DESKTOP_OAUTH_SUCCESS_VALUE: &str = "success";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformUserSummary {
    pub id: String,
    pub nickname: Option<String>,
    #[serde(default, rename = "isPlatformAdmin")]
    pub is_platform_admin: bool,
    #[serde(default, rename = "includedTokens")]
    pub included_tokens: u64,
    #[serde(default, rename = "consumedTokens")]
    pub consumed_tokens: u64,
    #[serde(default, rename = "tokenQuotaExhausted")]
    pub token_quota_exhausted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformSession {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: i64,
    pub agent_id: String,
    pub user: PlatformUserSummary,
}

/// LLM credentials from the last successful token exchange (not persisted).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlatformLoginCredentials {
    pub api_key: Option<String>,
    pub llm_provider: Option<String>,
    #[serde(default)]
    pub provider_api_keys: HashMap<String, String>,
    #[serde(default, rename = "mediaOss", alias = "media_oss")]
    pub media_oss: Option<PlatformMediaOssCredentials>,
}

/// OSS credentials from platform login (endpoint + AccessKey only).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlatformMediaOssCredentials {
    pub endpoint: String,
    #[serde(default, rename = "accessKeyId", alias = "access_key_id")]
    pub access_key_id: String,
    #[serde(default, rename = "accessKeySecret", alias = "access_key_secret")]
    pub access_key_secret: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlatformSessionView {
    pub logged_in: bool,
    pub expires_at: Option<i64>,
    pub user_nickname: Option<String>,
    #[serde(default, rename = "isPlatformAdmin")]
    pub is_platform_admin: bool,
    #[serde(default, rename = "includedTokens")]
    pub included_tokens: u64,
    #[serde(default, rename = "consumedTokens")]
    pub consumed_tokens: u64,
    #[serde(default, rename = "tokenQuotaExhausted")]
    pub token_quota_exhausted: bool,
}

#[derive(Debug)]
pub struct PlatformAuthManager {
    inner: RwLock<Option<PlatformSession>>,
    /// 串行化 refresh / 换票，避免并发使用同一 refresh token。
    refresh_lock: Mutex<()>,
    http: reqwest::Client,
    /// 进行中的 `run_platform_login_flow`；`cancel_pending_login` 可中止等待回调。
    login_cancel: RwLock<Option<CancellationToken>>,
}

impl PlatformAuthManager {
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(None),
            refresh_lock: Mutex::new(()),
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(60))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
            login_cancel: RwLock::new(None),
        }
    }

    fn replace_login_cancel_token(&self) -> CancellationToken {
        let mut guard = self.login_cancel.write();
        if let Some(old) = guard.take() {
            old.cancel();
        }
        let token = CancellationToken::new();
        *guard = Some(token.clone());
        token
    }

    fn clear_login_cancel_token(&self) {
        let _ = self.login_cancel.write().take();
    }

    /// 取消当前正在等待浏览器回调的登录（若有）。
    pub fn cancel_pending_login(&self) {
        if let Some(token) = self.login_cancel.write().take() {
            token.cancel();
            log::info!("platform_auth: login cancelled by user");
        }
    }

    pub fn session_view(&self) -> PlatformSessionView {
        let g = self.inner.read();
        match g.as_ref() {
            Some(s) => PlatformSessionView {
                logged_in: !s.access_token.is_empty() && !Self::is_expired(s.expires_at),
                expires_at: Some(s.expires_at),
                user_nickname: s.user.nickname.clone(),
                is_platform_admin: s.user.is_platform_admin,
                included_tokens: s.user.included_tokens,
                consumed_tokens: s.user.consumed_tokens,
                token_quota_exhausted: s.user.token_quota_exhausted,
            },
            None => PlatformSessionView::default(),
        }
    }

    pub fn is_platform_admin(&self) -> bool {
        self.inner
            .read()
            .as_ref()
            .map(|s| s.user.is_platform_admin)
            .unwrap_or(false)
    }

    pub fn access_token(&self) -> Option<String> {
        let g = self.inner.read();
        g.as_ref().and_then(|s| {
            if Self::is_expired(s.expires_at) {
                None
            } else {
                Some(s.access_token.clone())
            }
        })
    }

    pub fn set_session(&self, session: PlatformSession) {
        if !session.refresh_token.trim().is_empty() {
            if let Err(e) = storage::save_platform_refresh_token(&session.refresh_token) {
                log::warn!("platform_auth: save refresh to auth.dat failed: {e}");
            }
        }
        *self.inner.write() = Some(session);
    }

    /// Partner OAuth exchange on cloud agent instances (no refresh token, not persisted).
    pub fn set_partner_session(&self, session: PlatformSession) {
        *self.inner.write() = Some(session);
        log::info!("platform_auth: partner session applied (cloud agent)");
    }

    pub fn clear_session(&self) {
        let _guard = self.refresh_lock.blocking_lock();
        self.clear_session_inner();
    }

    fn clear_session_inner(&self) {
        let refresh = self.inner.read().as_ref().map(|s| s.refresh_token.clone());
        *self.inner.write() = None;
        if let Err(e) = storage::clear_platform_refresh_token() {
            log::warn!("platform_auth: clear auth.dat failed: {e}");
        }
        if let Some(rt) = refresh.filter(|s| !s.trim().is_empty()) {
            let _ = tauri_fire_and_forget_revoke(rt);
        }
    }

    fn sanitize_media_oss(
        media: Option<PlatformMediaOssCredentials>,
    ) -> Option<PlatformMediaOssCredentials> {
        media.filter(|m| {
            !m.endpoint.trim().is_empty()
                && !m.access_key_id.trim().is_empty()
                && !m.access_key_secret.trim().is_empty()
        })
    }

    fn credentials_have_payload(creds: &PlatformLoginCredentials) -> bool {
        creds.api_key.as_ref().is_some_and(|k| !k.trim().is_empty())
            || !creds.provider_api_keys.is_empty()
            || creds.media_oss.is_some()
    }

    fn merge_login_credentials(
        primary: PlatformLoginCredentials,
        overlay: Option<PlatformLoginCredentials>,
    ) -> PlatformLoginCredentials {
        let Some(overlay) = overlay else {
            return primary;
        };
        PlatformLoginCredentials {
            api_key: overlay
                .api_key
                .filter(|k| !k.trim().is_empty())
                .or(primary.api_key),
            llm_provider: overlay
                .llm_provider
                .filter(|p| !p.trim().is_empty())
                .or(primary.llm_provider),
            provider_api_keys: if overlay.provider_api_keys.is_empty() {
                primary.provider_api_keys
            } else {
                overlay.provider_api_keys
            },
            media_oss: overlay.media_oss.or(primary.media_oss),
        }
    }

    fn credentials_from_partner_response(
        parsed: PartnerLlmCredentialResponse,
    ) -> Option<PlatformLoginCredentials> {
        let media_oss = Self::sanitize_media_oss(parsed.media_oss);
        if parsed.ok {
            return Some(PlatformLoginCredentials {
                api_key: parsed.api_key.filter(|k| !k.trim().is_empty()),
                llm_provider: parsed.llm_provider.filter(|p| !p.trim().is_empty()),
                provider_api_keys: parsed.provider_api_keys,
                media_oss,
            });
        }
        if media_oss.is_some() {
            return Some(PlatformLoginCredentials {
                api_key: None,
                llm_provider: None,
                provider_api_keys: HashMap::new(),
                media_oss,
            });
        }
        None
    }

    fn valid_session_if_fresh(&self) -> Option<(PlatformSession, PlatformLoginCredentials)> {
        let g = self.inner.read();
        g.as_ref().and_then(|s| {
            if Self::is_expired(s.expires_at) {
                None
            } else {
                Some((s.clone(), PlatformLoginCredentials::default()))
            }
        })
    }

    fn resolve_refresh_token(&self) -> String {
        match self
            .inner
            .read()
            .as_ref()
            .map(|s| s.refresh_token.clone())
        {
            Some(rt) if !rt.is_empty() => rt,
            _ => match storage::load_platform_refresh_token() {
                Ok(Some(r)) => r,
                Ok(None) => String::new(),
                Err(e) => {
                    log::warn!("platform_auth: auth.dat load failed: {e}");
                    String::new()
                }
            },
        }
    }

    fn is_refresh_auth_failure(err: &anyhow::Error) -> bool {
        let msg = err.to_string();
        msg.contains("401") || msg.contains("invalid_refresh_token")
    }

    fn is_expired(expires_at: i64) -> bool {
        let now = Utc::now().timestamp();
        expires_at <= now + EXPIRY_BUFFER_SEC
    }

    fn api_base() -> String {
        platform_endpoints::api_base()
    }

    fn web_base() -> String {
        platform_endpoints::web_base()
    }

    fn client_id() -> String {
        platform_endpoints::oauth_client_id()
    }

    /// 环境变量指定的首选 loopback 端口（默认 19427）。
    fn preferred_loopback_port() -> u16 {
        std::env::var("POINTER_OAUTH_LOOPBACK_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_LOOPBACK_PORT)
    }

    pub fn redirect_uri_for_port(port: u16) -> String {
        format!("http://127.0.0.1:{port}/callback")
    }

    pub fn build_authorize_url_for_port(port: u16, code_challenge: &str, state: &str) -> String {
        let web = Self::web_base();
        let client_id = Self::client_id();
        let redirect = Self::redirect_uri_for_port(port);
        format!(
            "{web}/oauth/authorize?client_id={client_id}&redirect_uri={}&code_challenge={code_challenge}&code_challenge_method=S256&state={state}",
            urlencoding_encode(&redirect),
        )
    }

    pub fn generate_pkce() -> (String, String) {
        let mut bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut bytes);
        let verifier = URL_SAFE_NO_PAD.encode(bytes);
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        (verifier, challenge)
    }

    pub async fn exchange_authorization_code(
        &self,
        code: &str,
        code_verifier: &str,
        state: &str,
        redirect_uri: &str,
    ) -> Result<(PlatformSession, PlatformLoginCredentials)> {
        let client_env = crate::client_env::collect_login_client_env();
        let body = serde_json::json!({
            "grant_type": "authorization_code",
            "client_id": Self::client_id(),
            "code": code,
            "code_verifier": code_verifier,
            "redirect_uri": redirect_uri,
            "client_env": client_env,
        });
        let _guard = self.refresh_lock.lock().await;
        let (session, creds) = self.post_token(body).await?;
        if state != "pointer-app" {
            log::debug!("platform_auth: oauth state={state}");
        }
        self.set_session(session.clone());
        Ok((session, creds))
    }

    pub async fn refresh_if_needed(
        &self,
    ) -> Result<Option<(PlatformSession, PlatformLoginCredentials)>> {
        if let Some(fresh) = self.valid_session_if_fresh() {
            return Ok(Some(fresh));
        }

        let _guard = self.refresh_lock.lock().await;

        if let Some(fresh) = self.valid_session_if_fresh() {
            return Ok(Some(fresh));
        }

        let refresh = self.resolve_refresh_token();
        if refresh.is_empty() {
            return Ok(None);
        }
        let body = serde_json::json!({
            "grant_type": "refresh_token",
            "client_id": Self::client_id(),
            "refresh_token": refresh,
        });
        match self.post_token(body).await {
            Ok((session, creds)) => {
                self.set_session(session.clone());
                Ok(Some((session, creds)))
            }
            Err(e) => {
                log::warn!("platform_auth: refresh failed: {e}");
                if Self::is_refresh_auth_failure(&e) && self.valid_session_if_fresh().is_none() {
                    self.clear_session_inner();
                }
                Err(e)
            }
        }
    }

    pub async fn ensure_access_token(&self) -> Result<String> {
        if let Some(tok) = self.access_token() {
            return Ok(tok);
        }
        if let Some((s, _)) = self.refresh_if_needed().await? {
            return Ok(s.access_token);
        }
        Err(anyhow!("platform_login_required"))
    }

    pub async fn load_persisted_session(&self) -> Result<Option<PlatformLoginCredentials>> {
        match self.refresh_if_needed().await {
            Ok(Some((_session, token_creds))) => {
                let fetched = self.fetch_llm_credentials().await?;
                let merged = Self::merge_login_credentials(token_creds, fetched);
                if Self::credentials_have_payload(&merged) {
                    Ok(Some(merged))
                } else {
                    Ok(None)
                }
            }
            Ok(None) => Ok(None),
            Err(e) => {
                log::warn!("platform_auth: startup refresh failed: {e}");
                Ok(None)
            }
        }
    }

    /// Back-compat alias; prefer [`load_persisted_session`].
    pub async fn load_from_keyring(&self) -> Result<bool> {
        Ok(self.load_persisted_session().await?.is_some())
    }

    async fn post_token(
        &self,
        body: serde_json::Value,
    ) -> Result<(PlatformSession, PlatformLoginCredentials)> {
        let url = format!("{}/auth/app/token", Self::api_base().trim_end_matches('/'));
        let resp = self
            .http
            .post(&url)
            .json(&body)
            .send()
            .await
            .context("token request failed")?;
        let status = resp.status();
        let text = resp.text().await.context("read token response")?;
        if !status.is_success() {
            return Err(anyhow!("token exchange failed ({status}): {text}"));
        }
        let parsed: AppTokenResponse =
            serde_json::from_str(&text).context("parse token response")?;
        let expires_at = Utc::now().timestamp() + parsed.expires_in as i64;
        let session = PlatformSession {
            access_token: parsed.access_token,
            refresh_token: parsed.refresh_token,
            expires_at,
            agent_id: parsed.agent_id,
            user: PlatformUserSummary {
                id: parsed.user.id,
                nickname: parsed.user.nickname,
                is_platform_admin: parsed.user.is_platform_admin,
                included_tokens: parsed.user.included_tokens,
                consumed_tokens: parsed.user.consumed_tokens,
                token_quota_exhausted: parsed.user.token_quota_exhausted,
            },
        };
        let creds = PlatformLoginCredentials {
            api_key: parsed.api_key.filter(|k| !k.trim().is_empty()),
            llm_provider: parsed.llm_provider.filter(|p| !p.trim().is_empty()),
            provider_api_keys: parsed.provider_api_keys,
            media_oss: Self::sanitize_media_oss(parsed.media_oss),
        };
        Ok((session, creds))
    }

    pub fn platform_agent_id(&self) -> Option<String> {
        self.inner
            .read()
            .as_ref()
            .map(|s| s.agent_id.clone())
            .filter(|id| !id.is_empty())
    }

    pub fn token_quota_exhausted(&self) -> bool {
        self.inner
            .read()
            .as_ref()
            .map(|s| s.user.token_quota_exhausted)
            .unwrap_or(false)
    }

    pub async fn report_token_usage_multipart(
        &self,
        metadata: &serde_json::Value,
        zip_path: Option<&std::path::Path>,
    ) -> Result<()> {
        let token = self.ensure_access_token().await?;
        let url = format!(
            "{}/auth/partner/token-usage",
            Self::api_base().trim_end_matches('/')
        );
        let meta_str =
            serde_json::to_string(metadata).context("serialize token usage metadata")?;
        let mut form = reqwest::multipart::Form::new().text("metadata", meta_str);
        if let Some(path) = zip_path {
            let bytes = tokio::fs::read(path)
                .await
                .with_context(|| format!("read history archive {}", path.display()))?;
            let part = reqwest::multipart::Part::bytes(bytes)
                .file_name("history.zip")
                .mime_str("application/zip")
                .context("zip mime")?;
            form = form.part("history_archive", part);
        }
        let resp = self
            .http
            .post(&url)
            .header("Authorization", format!("Bearer {token}"))
            .multipart(form)
            .send()
            .await
            .context("token usage multipart report failed")?;
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            self.clear_session();
            return Err(anyhow!("platform_token_expired"));
        }
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "token usage multipart report failed: HTTP {status}: {text}"
            ));
        }
        Ok(())
    }

    pub async fn report_token_usage(&self, body: serde_json::Value) -> Result<()> {
        let token = self.ensure_access_token().await?;
        let url = format!(
            "{}/auth/partner/token-usage",
            Self::api_base().trim_end_matches('/')
        );
        let resp = self
            .http
            .post(&url)
            .header("Authorization", format!("Bearer {token}"))
            .json(&body)
            .send()
            .await
            .context("token usage report failed")?;
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            self.clear_session();
            return Err(anyhow!("platform_token_expired"));
        }
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("token usage report failed: HTTP {status}: {text}"));
        }
        Ok(())
    }

    pub async fn fetch_llm_credentials(&self) -> Result<Option<PlatformLoginCredentials>> {
        let token = self.ensure_access_token().await?;
        let url = format!(
            "{}/auth/partner/llm-credentials",
            Self::api_base().trim_end_matches('/')
        );
        let resp = self
            .http
            .get(&url)
            .header("Authorization", format!("Bearer {token}"))
            .send()
            .await?;
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            self.clear_session();
            return Err(anyhow!("platform_token_expired"));
        }
        if !resp.status().is_success() {
            return Ok(None);
        }
        let parsed: PartnerLlmCredentialResponse = resp.json().await?;
        if parsed.error_code.as_deref() == Some("token_quota_exhausted") {
            return Err(anyhow!("token_quota_exhausted"));
        }
        if !parsed.ok {
            return Ok(Self::credentials_from_partner_response(parsed));
        }
        if parsed.included_tokens.is_some() || parsed.consumed_tokens.is_some() {
            let mut guard = self.inner.write();
            if let Some(session) = guard.as_mut() {
                if let Some(v) = parsed.included_tokens {
                    session.user.included_tokens = v;
                }
                if let Some(v) = parsed.consumed_tokens {
                    session.user.consumed_tokens = v;
                }
            }
            log::info!(
                "platform_auth: llm-credentials token quota included={:?} consumed={:?}",
                parsed.included_tokens,
                parsed.consumed_tokens
            );
        }
        Ok(Self::credentials_from_partner_response(parsed))
    }

    /// Refresh partner LLM credentials; block when platform reports quota exhausted.
    pub async fn ensure_llm_allowed(&self) -> Result<()> {
        if !self.session_view().logged_in {
            return Ok(());
        }
        if self.token_quota_exhausted() {
            return Err(anyhow!("token_quota_exhausted"));
        }
        match self.fetch_llm_credentials().await {
            Ok(Some(_)) => Ok(()),
            Ok(None) => Ok(()),
            Err(e) => Err(e),
        }
    }

    pub async fn fetch_llm_api_key(&self) -> Result<Option<String>> {
        Ok(self
            .fetch_llm_credentials()
            .await?
            .and_then(|c| c.api_key))
    }
}

fn urlencoding_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[derive(Debug, Deserialize)]
struct AppTokenResponse {
    access_token: String,
    refresh_token: String,
    expires_in: u64,
    agent_id: String,
    user: AppTokenUser,
    api_key: Option<String>,
    llm_provider: Option<String>,
    #[serde(default)]
    provider_api_keys: HashMap<String, String>,
    #[serde(default, rename = "mediaOss", alias = "media_oss")]
    media_oss: Option<PlatformMediaOssCredentials>,
}

#[derive(Debug, Deserialize)]
struct AppTokenUser {
    id: String,
    nickname: Option<String>,
    #[serde(default, rename = "is_platform_admin")]
    is_platform_admin: bool,
    #[serde(default, rename = "included_tokens")]
    included_tokens: u64,
    #[serde(default, rename = "consumed_tokens")]
    consumed_tokens: u64,
    #[serde(default, rename = "token_quota_exhausted")]
    token_quota_exhausted: bool,
}

async fn tauri_fire_and_forget_revoke(refresh_token: String) -> Result<()> {
    let url = format!(
        "{}/auth/app/logout",
        PlatformAuthManager::api_base().trim_end_matches('/')
    );
    let _ = reqwest::Client::new()
        .post(url)
        .json(&serde_json::json!({ "refresh_token": refresh_token }))
        .send()
        .await;
    Ok(())
}

#[derive(Debug, Deserialize)]
struct PartnerLlmCredentialResponse {
    ok: bool,
    api_key: Option<String>,
    llm_provider: Option<String>,
    #[serde(default)]
    provider_api_keys: HashMap<String, String>,
    #[serde(default)]
    error_code: Option<String>,
    #[serde(default, rename = "included_tokens")]
    included_tokens: Option<u64>,
    #[serde(default, rename = "consumed_tokens")]
    consumed_tokens: Option<u64>,
    #[serde(default, rename = "mediaOss", alias = "media_oss")]
    media_oss: Option<PlatformMediaOssCredentials>,
}

/// 从首选端口起扫描，绑定第一个可用的 127.0.0.1 端口。
pub async fn bind_loopback_listener() -> Result<(tokio::net::TcpListener, u16)> {
    let start = PlatformAuthManager::preferred_loopback_port();
    let mut last_err: Option<std::io::Error> = None;
    for offset in 0..LOOPBACK_PORT_SCAN_COUNT {
        let port = start.saturating_add(offset);
        match tokio::net::TcpListener::bind(format!("127.0.0.1:{port}")).await {
            Ok(listener) => {
                if offset > 0 {
                    log::info!(
                        "platform_auth: preferred port {start} busy, using 127.0.0.1:{port} for oauth callback"
                    );
                } else {
                    log::info!("platform_auth: loopback oauth callback on 127.0.0.1:{port}");
                }
                return Ok((listener, port));
            }
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
                last_err = Some(e);
                continue;
            }
            Err(e) => return Err(e).context("bind loopback oauth listener"),
        }
    }
    let end = start.saturating_add(LOOPBACK_PORT_SCAN_COUNT.saturating_sub(1));
    if let Some(e) = last_err {
        Err(anyhow!(
            "no free loopback port in range {start}..{end} (last error: {e})"
        ))
    } else {
        Err(anyhow!("no free loopback port in range {start}..{end}"))
    }
}

/// 在已绑定的 listener 上等待单次 OAuth 回调 GET。
pub async fn wait_loopback_on_listener(
    listener: tokio::net::TcpListener,
    expected_state: &str,
    timeout_sec: u64,
) -> Result<(String, String)> {
    let accept = tokio::time::timeout(Duration::from_secs(timeout_sec), listener.accept())
        .await
        .context("oauth callback timeout")?
        .context("accept failed")?;
    let (mut stream, _) = accept;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut buf = vec![0u8; 8192];
    let n = stream.read(&mut buf).await.context("read callback")?;
    let req = String::from_utf8_lossy(&buf[..n]);
    let first_line = req.lines().next().unwrap_or("");
    let path = first_line
        .split_whitespace()
        .nth(1)
        .unwrap_or("/callback");
    let (code, state) = parse_callback_query(path, expected_state)?;
    let location = desktop_oauth_success_redirect_url();
    let response = format!(
        "HTTP/1.1 302 Found\r\nLocation: {location}\r\nConnection: close\r\nContent-Length: 0\r\n\r\n"
    );
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.shutdown().await;
    Ok((code, state))
}

/// 桌面 OAuth 回调成功后跳转官网首页（带一次性提示用的 query）。
pub fn desktop_oauth_success_redirect_url() -> String {
    let home = format!(
        "{}/",
        platform_endpoints::web_base().trim_end_matches('/')
    );
    format!(
        "{home}?{DESKTOP_OAUTH_SUCCESS_QUERY}={DESKTOP_OAUTH_SUCCESS_VALUE}"
    )
}

fn parse_callback_query(path: &str, expected_state: &str) -> Result<(String, String)> {
    let query = path.split('?').nth(1).unwrap_or("");
    let mut code = None;
    let mut state = None;
    for part in query.split('&') {
        let mut kv = part.splitn(2, '=');
        let k = kv.next().unwrap_or("");
        let v = kv.next().unwrap_or("");
        let v = percent_decode(v);
        match k {
            "code" => code = Some(v),
            "state" => state = Some(v),
            _ => {}
        }
    }
    let code = code.ok_or_else(|| anyhow!("missing code in callback"))?;
    let state = state.unwrap_or_default();
    if state != expected_state {
        return Err(anyhow!("oauth state mismatch"));
    }
    Ok((code, state))
}

fn percent_decode(s: &str) -> String {
    let mut out = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(
                std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("00"),
                16,
            ) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn open_url_in_browser(url: &str) -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;

        // OAuth authorize URLs contain `&` between query params. Passing the URL as an
        // unquoted argument to `cmd /C start` makes cmd treat each `&segment` as a new
        // command (e.g. `redirect_uri=...` fails as "not recognized"). Use rundll32 so
        // the URL is a single CreateProcess argument with no cmd parsing.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        std::process::Command::new("rundll32")
            .arg("url.dll,FileProtocolHandler")
            .arg(url)
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .context("open browser")?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(url)
            .spawn()
            .context("open browser")?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(url)
            .spawn()
            .context("open browser")?;
    }
    Ok(())
}

pub async fn run_platform_login_flow(
    auth: Arc<PlatformAuthManager>,
) -> Result<(PlatformSession, PlatformLoginCredentials)> {
    let cancel = auth.replace_login_cancel_token();
    let result = run_platform_login_flow_inner(auth.clone(), cancel).await;
    auth.clear_login_cancel_token();
    result
}

async fn run_platform_login_flow_inner(
    auth: Arc<PlatformAuthManager>,
    cancel: CancellationToken,
) -> Result<(PlatformSession, PlatformLoginCredentials)> {
    let (verifier, challenge) = PlatformAuthManager::generate_pkce();
    let state = "pointer-app";

    let (listener, port) = bind_loopback_listener().await?;
    let redirect_uri = PlatformAuthManager::redirect_uri_for_port(port);

    let callback_task = tokio::spawn(async move {
        tokio::select! {
            _ = cancel.cancelled() => Err(anyhow!("platform_login_cancelled")),
            r = wait_loopback_on_listener(listener, state, OAUTH_CALLBACK_TIMEOUT_SEC) => r,
        }
    });

    let url = PlatformAuthManager::build_authorize_url_for_port(port, &challenge, state);
    open_url_in_browser(&url)?;

    let (code, _) = callback_task
        .await
        .context("oauth callback task join")??;

    auth.exchange_authorization_code(&code, &verifier, state, &redirect_uri)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_authorize_url_includes_pkce_and_encoded_redirect() {
        let url = PlatformAuthManager::build_authorize_url_for_port(19427, "challenge_abc", "pointer-app");
        assert!(url.starts_with(&format!("{}/oauth/authorize?", PlatformAuthManager::web_base())));
        assert!(url.contains("client_id=pointer-desktop"));
        assert!(url.contains("code_challenge=challenge_abc"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("state=pointer-app"));
        assert!(url.contains(
            "redirect_uri=http%3A%2F%2F127.0.0.1%3A19427%2Fcallback"
        ));
        assert!(url.contains('&'), "authorize URL must keep query separators");
    }

    #[test]
    fn redirect_uri_for_port_matches_loopback_callback() {
        assert_eq!(
            PlatformAuthManager::redirect_uri_for_port(19428),
            "http://127.0.0.1:19428/callback"
        );
    }

    #[test]
    fn desktop_oauth_success_redirect_includes_query_flag() {
        let url = super::desktop_oauth_success_redirect_url();
        assert!(url.starts_with("https://pointer.readflowai.com/"));
        assert!(url.contains("desktop_oauth=success"));
    }
}

pub type SharedPlatformAuth = Arc<PlatformAuthManager>;
