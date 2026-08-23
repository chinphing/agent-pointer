//! Pointer desktop OAuth (PKCE + refresh token + encrypted local persistence).

use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::Utc;
use std::collections::HashMap;

use parking_lot::RwLock;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
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
/// Initial refresh try + this many retries on transient network errors.
const REFRESH_TRANSIENT_RETRY_COUNT: u32 = 3;
/// Total post_token attempts for a refresh (1 initial + [`REFRESH_TRANSIENT_RETRY_COUNT`]).
const REFRESH_TRANSIENT_MAX_ATTEMPTS: u32 = 1 + REFRESH_TRANSIENT_RETRY_COUNT;
/// 等待浏览器 OAuth 回调的最长时间（秒）。
pub const OAUTH_CALLBACK_TIMEOUT_SEC: u64 = 300;
/// bind 后本机回环自检超时。
const LOOPBACK_PROBE_TIMEOUT_SEC: u64 = 2;
/// 连接建立后等待首字节；超时则视为 Chrome 预连接并关掉（避免占住 keep-alive）。
const LOOPBACK_FIRST_BYTE_TIMEOUT_MS: u64 = 2_500;
/// 已有首字节后，读完 HTTP 头的最长时间。
const LOOPBACK_HEADERS_TIMEOUT_SEC: u64 = 30;
/// 上次成功绑定的 loopback 端口；下次从下一个端口起绑，避免浏览器复用旧 keep-alive。
static LAST_LOOPBACK_PORT: AtomicU16 = AtomicU16::new(0);
/// 桌面 OAuth 成功后跳转官网首页时携带的 query 名；官网据此展示一次性提示（见 `docs/developer/desktop-oauth-web-integration.md`）。
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

/// 平台服务商完整模板（由平台目录下发；客户端据此创建服务商，不再本地内置）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlatformProviderTemplate {
    #[serde(default)]
    pub id: String,
    #[serde(default, rename = "sortOrder", alias = "sort_order")]
    pub sort_order: Option<u32>,
    #[serde(default)]
    pub name: String,
    #[serde(default, rename = "baseUrl", alias = "base_url")]
    pub base_url: String,
    #[serde(default, deserialize_with = "deserialize_platform_models")]
    pub models: Vec<PlatformProviderModel>,
    #[serde(
        default,
        rename = "reasoningInMessages",
        alias = "reasoning_in_messages"
    )]
    pub reasoning_in_messages: Option<bool>,
    #[serde(default, rename = "enableThinking", alias = "enable_thinking")]
    pub enable_thinking: Option<bool>,
    #[serde(default, rename = "thinkingBudget", alias = "thinking_budget")]
    pub thinking_budget: Option<u32>,
    #[serde(default, rename = "reasoningEffort", alias = "reasoning_effort")]
    pub reasoning_effort: Option<String>,
    #[serde(default, rename = "thinkingProtocol", alias = "thinking_protocol")]
    pub thinking_protocol: Option<String>,
    #[serde(default, rename = "thinkingIntensity", alias = "thinking_intensity")]
    pub thinking_intensity: Option<String>,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default, rename = "topP", alias = "top_p")]
    pub top_p: Option<f32>,
    #[serde(default, rename = "maxTokens", alias = "max_tokens")]
    pub max_tokens: Option<u32>,
    #[serde(
        default,
        rename = "contextBudgetTokens",
        alias = "context_budget_tokens"
    )]
    pub context_budget_tokens: Option<u32>,
}

/// 平台模型条目（name + 模型级参数；未设置的字段继承服务商级默认）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlatformProviderModel {
    #[serde(default)]
    pub name: String,
    #[serde(
        default,
        rename = "reasoningInMessages",
        alias = "reasoning_in_messages"
    )]
    pub reasoning_in_messages: Option<bool>,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default, rename = "topP", alias = "top_p")]
    pub top_p: Option<f32>,
    #[serde(default, rename = "maxTokens", alias = "max_tokens")]
    pub max_tokens: Option<u32>,
    #[serde(
        default,
        rename = "contextBudgetTokens",
        alias = "context_budget_tokens"
    )]
    pub context_budget_tokens: Option<u32>,
    #[serde(default, rename = "enableThinking", alias = "enable_thinking")]
    pub enable_thinking: Option<bool>,
    #[serde(default, rename = "thinkingBudget", alias = "thinking_budget")]
    pub thinking_budget: Option<u32>,
    #[serde(default, rename = "reasoningEffort", alias = "reasoning_effort")]
    pub reasoning_effort: Option<String>,
    #[serde(default, rename = "thinkingProtocol", alias = "thinking_protocol")]
    pub thinking_protocol: Option<String>,
    #[serde(default, rename = "thinkingIntensity", alias = "thinking_intensity")]
    pub thinking_intensity: Option<String>,
    #[serde(default, rename = "supportsVision", alias = "supports_vision")]
    pub supports_vision: Option<bool>,
    #[serde(default, rename = "supportsAudio", alias = "supports_audio")]
    pub supports_audio: Option<bool>,
    #[serde(default, rename = "canGenerateImage", alias = "can_generate_image")]
    pub can_generate_image: Option<bool>,
    #[serde(default, rename = "canGenerateVideo", alias = "can_generate_video")]
    pub can_generate_video: Option<bool>,
}

/// models 数组元素兼容纯字符串模型名与 {name, ...模型级参数} 对象。
fn deserialize_platform_models<'de, D>(
    deserializer: D,
) -> Result<Vec<PlatformProviderModel>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw: Vec<serde_json::Value> = serde::Deserialize::deserialize(deserializer)?;
    let mut out = Vec::with_capacity(raw.len());
    for value in raw {
        if let Some(name) = value.as_str() {
            let name = name.trim();
            if !name.is_empty() {
                out.push(PlatformProviderModel {
                    name: name.to_string(),
                    ..Default::default()
                });
            }
        } else if let Ok(model) = serde_json::from_value::<PlatformProviderModel>(value) {
            if !model.name.trim().is_empty() {
                out.push(model);
            }
        }
    }
    Ok(out)
}

/// LLM credentials from the last successful token exchange (not persisted).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlatformLoginCredentials {
    pub api_key: Option<String>,
    pub llm_provider: Option<String>,
    #[serde(default)]
    pub provider_api_keys: HashMap<String, String>,
    /// Read-only model catalog returned by the platform; never persisted to user settings.
    #[serde(
        default,
        rename = "modelCatalog",
        alias = "model_catalog",
        alias = "providerModels"
    )]
    pub model_catalog: HashMap<String, Vec<String>>,
    /// 平台服务商完整模板（id -> name/baseUrl/models/默认参数）；客户端据此创建服务商。
    #[serde(default, rename = "platformProviders", alias = "platform_providers")]
    pub platform_providers: Vec<PlatformProviderTemplate>,
    /// 场景档位默认（agent/media/computer 快速/标准/高级默认模型映射）。
    #[serde(default, rename = "tierDefaults", alias = "tier_defaults")]
    pub tier_defaults: serde_json::Value,
    #[serde(default, rename = "modelCatalogHash", alias = "model_catalog_hash")]
    pub model_catalog_hash: Option<String>,
    #[serde(default, rename = "mediaOss", alias = "media_oss")]
    pub media_oss: Option<PlatformMediaOssCredentials>,
}

/// Whether credentials carry LLM API keys usable for headless automation runs.
pub fn credentials_have_llm_keys(creds: &PlatformLoginCredentials) -> bool {
    creds.api_key.as_ref().is_some_and(|k| !k.trim().is_empty())
        || creds
            .provider_api_keys
            .values()
            .any(|k| !k.trim().is_empty())
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
    /// Set by `clear_session_async` so a concurrent refresh/login cannot
    /// re-create the session (via `set_session`) after logout clears it.
    /// Reset to `false` at the start of a new login (`exchange_authorization_code`).
    logging_out: AtomicBool,
    http: reqwest::Client,
    /// 进行中的 `run_platform_login_flow`；`cancel_pending_login` 可中止等待回调。
    login_cancel: RwLock<Option<CancellationToken>>,
}

impl PlatformAuthManager {
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(None),
            refresh_lock: Mutex::new(()),
            logging_out: AtomicBool::new(false),
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

    pub fn platform_user_id(&self) -> Option<String> {
        self.inner
            .read()
            .as_ref()
            .map(|s| s.user.id.clone())
            .filter(|id| !id.trim().is_empty())
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
        if self.logging_out.load(Ordering::SeqCst) {
            log::info!("platform_auth: set_session skipped (logout in progress)");
            return;
        }
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
        self.logging_out.store(true, Ordering::SeqCst);
        let _guard = self.refresh_lock.blocking_lock();
        self.clear_session_inner();
    }

    /// Async variant of [`clear_session`] for use inside async runtimes.
    /// The sync version uses `tokio::Mutex::blocking_lock` which panics inside
    /// a tokio multi-threaded runtime; callers in `async fn` paths must use this.
    ///
    /// Sets `logging_out` so a concurrent refresh/login cannot re-create the
    /// session via `set_session` after we clear it. Acquires `refresh_lock`
    /// with a short timeout so logout never hangs on a stuck/slow refresh
    /// (e.g. platform API unreachable); on timeout it proceeds without the
    /// lock — the `logging_out` flag makes that race-safe.
    pub async fn clear_session_async(&self) {
        self.logging_out.store(true, Ordering::SeqCst);
        match tokio::time::timeout(Duration::from_secs(3), self.refresh_lock.lock()).await {
            Ok(_guard) => self.clear_session_inner(),
            Err(_) => {
                log::warn!(
                    "platform_auth: logout timed out waiting for refresh_lock after 3s; \
                     clearing session without lock (a concurrent refresh may have been in flight)"
                );
                self.clear_session_inner();
            }
        }
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
            model_catalog: if overlay.model_catalog.is_empty() {
                primary.model_catalog
            } else {
                overlay.model_catalog
            },
            platform_providers: if overlay.platform_providers.is_empty() {
                primary.platform_providers
            } else {
                overlay.platform_providers
            },
            tier_defaults: if overlay.tier_defaults.is_null() {
                primary.tier_defaults
            } else {
                overlay.tier_defaults
            },
            model_catalog_hash: overlay.model_catalog_hash.or(primary.model_catalog_hash),
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
                model_catalog: parsed.model_catalog,
                platform_providers: parsed.platform_providers,
                tier_defaults: parsed.tier_defaults,
                model_catalog_hash: None,
                media_oss,
            });
        }
        if media_oss.is_some() {
            return Some(PlatformLoginCredentials {
                api_key: None,
                llm_provider: None,
                provider_api_keys: HashMap::new(),
                model_catalog: HashMap::new(),
                platform_providers: Vec::new(),
                tier_defaults: serde_json::Value::Null,
                model_catalog_hash: None,
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
        match self.inner.read().as_ref().map(|s| s.refresh_token.clone()) {
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

    /// Parse `http_status=NNN` (preferred) or `token exchange failed (NNN` from error text.
    fn parse_http_status_from_error(err: &anyhow::Error) -> Option<u16> {
        let msg = err.to_string();
        if let Some(rest) = msg.split("http_status=").nth(1) {
            let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            if let Ok(code) = digits.parse::<u16>() {
                return Some(code);
            }
        }
        if let Some(rest) = msg.split("token exchange failed (").nth(1) {
            let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            if let Ok(code) = digits.parse::<u16>() {
                return Some(code);
            }
        }
        if let Some(rest) = msg.split("HTTP ").nth(1) {
            let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            if let Ok(code) = digits.parse::<u16>() {
                return Some(code);
            }
        }
        None
    }

    /// True when refresh/balance failed because the session/token itself was rejected.
    fn is_refresh_auth_failure(err: &anyhow::Error) -> bool {
        let msg = err.to_string();
        if msg.contains("invalid_refresh_token") || msg.contains("platform_token_expired") {
            return true;
        }
        matches!(Self::parse_http_status_from_error(err), Some(401 | 403))
    }

    /// Transient transport / upstream failures — keep `auth.dat`, do not force re-login.
    /// Prefer HTTP status: 408/429/5xx (and no status = transport) retry; 401/403 do not.
    pub fn is_refresh_transient_failure(&self, err: &anyhow::Error) -> bool {
        let _ = self;
        if Self::is_refresh_auth_failure(err) {
            return false;
        }
        match Self::parse_http_status_from_error(err) {
            Some(408 | 429) => true,
            Some(code) if (500..=599).contains(&code) => true,
            Some(_) => false,
            None => true,
        }
    }

    /// Backoff after transient platform HTTP failure `attempt` (1-based failed try).
    fn refresh_transient_retry_delay(attempt: u32) -> Duration {
        match attempt {
            1 => Duration::from_millis(800),
            2 => Duration::from_millis(2_000),
            _ => Duration::from_millis(4_000),
        }
    }

    async fn sleep_transient_retry(&self, attempt: u32, op: &str) {
        let delay = Self::refresh_transient_retry_delay(attempt);
        log::info!(
            "platform_auth: retrying {op} in {}ms (retry {}/{})",
            delay.as_millis(),
            attempt,
            REFRESH_TRANSIENT_RETRY_COUNT
        );
        tokio::time::sleep(delay).await;
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

    pub fn build_authorize_url_for_redirect(
        redirect_uri: &str,
        code_challenge: &str,
        state: &str,
    ) -> String {
        let web = Self::web_base();
        let client_id = Self::client_id();
        format!(
            "{web}/oauth/authorize?client_id={client_id}&redirect_uri={}&code_challenge={code_challenge}&code_challenge_method=S256&state={state}",
            urlencoding_encode(redirect_uri),
        )
    }

    pub fn build_authorize_url_for_port(port: u16, code_challenge: &str, state: &str) -> String {
        Self::build_authorize_url_for_redirect(
            &Self::redirect_uri_for_port(port),
            code_challenge,
            state,
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
        // A previous logout may have set this flag; clear it so the new login
        // session can be stored by `set_session` below.
        self.logging_out.store(false, Ordering::SeqCst);
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

        // Transient network blips: retry with backoff before failing the chat turn.
        let mut last_err: Option<anyhow::Error> = None;
        for attempt in 1..=REFRESH_TRANSIENT_MAX_ATTEMPTS {
            match self.post_token(body.clone()).await {
                Ok((session, creds)) => {
                    self.set_session(session.clone());
                    if attempt > 1 {
                        log::info!(
                            "platform_auth: refresh succeeded after transient retry attempt={attempt}/{}",
                            REFRESH_TRANSIENT_MAX_ATTEMPTS
                        );
                    }
                    return Ok(Some((session, creds)));
                }
                Err(e) if Self::is_refresh_auth_failure(&e) => {
                    log::warn!("platform_auth: refresh auth failure: {e}");
                    if self.valid_session_if_fresh().is_none() {
                        self.clear_session_inner();
                    }
                    return Err(e);
                }
                Err(e) => {
                    log::warn!(
                        "platform_auth: refresh transient failure attempt={attempt}/{}: {e:#}",
                        REFRESH_TRANSIENT_MAX_ATTEMPTS
                    );
                    last_err = Some(e);
                    if attempt < REFRESH_TRANSIENT_MAX_ATTEMPTS {
                        self.sleep_transient_retry(attempt, "refresh").await;
                    }
                }
            }
        }

        let err = last_err.unwrap_or_else(|| anyhow!("platform refresh failed"));
        log::warn!(
            "platform_auth: refresh gave up after {} transient attempts: {err:#}",
            REFRESH_TRANSIENT_MAX_ATTEMPTS
        );
        Err(err)
    }

    pub async fn ensure_access_token(&self) -> Result<String> {
        if let Some(tok) = self.access_token() {
            return Ok(tok);
        }
        match self.refresh_if_needed().await {
            Ok(Some((s, _))) => Ok(s.access_token),
            Ok(None) => Err(anyhow!("platform_login_required")),
            Err(e) if self.is_refresh_transient_failure(&e) => {
                log::warn!("platform_auth: ensure_access_token transient refresh failure: {e:#}");
                Err(anyhow!("网络异常，暂时无法刷新登录态，请稍后重试"))
            }
            Err(e) => Err(e),
        }
    }

    pub async fn load_persisted_session(&self) -> Result<Option<PlatformLoginCredentials>> {
        match self.refresh_if_needed().await {
            Ok(Some((_session, token_creds))) => {
                let fetched = match self.fetch_llm_credentials().await {
                    Ok(creds) => creds,
                    Err(e) => {
                        let msg = e.to_string();
                        if msg.contains("token_quota_exhausted") {
                            log::warn!(
                                "platform_auth: startup llm-credentials blocked (quota exhausted)"
                            );
                        } else {
                            log::warn!(
                                "platform_auth: startup llm-credentials fetch failed: {e:#}"
                            );
                        }
                        None
                    }
                };
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
            // Stable `http_status=` tag so clients classify auth vs transient without brittle substrings.
            return Err(anyhow!(
                "token exchange failed http_status={} ({status}): {text}",
                status.as_u16()
            ));
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
            model_catalog: parsed.model_catalog,
            platform_providers: parsed.platform_providers,
            tier_defaults: parsed.tier_defaults,
            model_catalog_hash: None,
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

    fn set_token_quota_exhausted(&self, exhausted: bool) {
        let mut guard = self.inner.write();
        if let Some(session) = guard.as_mut() {
            if session.user.token_quota_exhausted != exhausted {
                session.user.token_quota_exhausted = exhausted;
                log::info!(
                    "platform_auth: token_quota_exhausted set to {exhausted} user_id={}",
                    session.user.id
                );
            }
        }
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
        let meta_str = serde_json::to_string(metadata).context("serialize token usage metadata")?;
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
            self.clear_session_async().await;
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
            self.clear_session_async().await;
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
            .await
            .context("llm-credentials request failed")?;
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            self.clear_session_async().await;
            return Err(anyhow!("platform_token_expired"));
        }
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            log::warn!("platform_auth: llm-credentials HTTP {status}: {text}");
            // Fail closed for callers that treat Ok(None) as "no keys"; gate uses Err path via ensure_llm_allowed.
            return Ok(None);
        }
        let parsed: PartnerLlmCredentialResponse = resp.json().await?;
        if parsed.error_code.as_deref() == Some("token_quota_exhausted") {
            self.set_token_quota_exhausted(true);
            return Err(anyhow!("token_quota_exhausted"));
        }
        if !parsed.ok {
            let code = parsed.error_code.as_deref().unwrap_or("llm_unavailable");
            log::warn!("platform_auth: llm-credentials unavailable error_code={code}");
            return Err(anyhow!("{code}"));
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
        self.set_token_quota_exhausted(false);
        Ok(Self::credentials_from_partner_response(parsed))
    }

    /// Live balance check for chat-start gate. **No-op in standalone mode.**
    ///
    /// Uses `GET /auth/partner/balance` (not llm-credentials). Login / key refresh unchanged.
    /// Transient network/5xx errors retry with the same backoff as token refresh; still fail-closed
    /// after retries. Soft overdraft on charge remains server-side.
    pub async fn ensure_llm_allowed(&self) -> Result<()> {
        self.ensure_llm_allowed_with_balance().await.map(|_| ())
    }

    /// Same gate as [`Self::ensure_llm_allowed`], returning the successful lightweight
    /// balance payload so the chat owner may refresh the read-only model catalog.
    pub async fn ensure_llm_allowed_with_balance(&self) -> Result<Option<PartnerBalanceResponse>> {
        if crate::deployment_mode::is_standalone() || !self.session_view().logged_in {
            return Ok(None);
        }
        match self.fetch_partner_balance().await {
            Ok(bal) => {
                self.set_token_quota_exhausted(bal.token_quota_exhausted);
                if bal.token_quota_exhausted {
                    return Err(anyhow!("token_quota_exhausted"));
                }
                Ok(Some(bal))
            }
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("token_quota_exhausted") {
                    self.set_token_quota_exhausted(true);
                    return Err(e);
                }
                log::warn!("platform_auth: partner balance check failed: {e:#}");
                if Self::is_refresh_auth_failure(&e) {
                    return Err(e);
                }
                if self.is_refresh_transient_failure(&e) {
                    return Err(anyhow!("网络异常，请检查网络链接是否正常，然后重试。"));
                }
                Err(e)
            }
        }
    }

    /// Fetch the full catalog only when the balance response advertises a new version.
    /// A refresh failure is intentionally returned to the caller as a soft error; it must
    /// not change the outcome of the already-successful balance gate.
    pub async fn refresh_llm_credentials_if_model_catalog_changed(
        &self,
        server_hash: Option<&str>,
    ) -> Result<Option<PlatformLoginCredentials>> {
        let Some(server_hash) = server_hash.filter(|hash| !hash.trim().is_empty()) else {
            return Ok(None);
        };
        if storage::platform_model_catalog_cache_hash()?.as_deref() == Some(server_hash) {
            return Ok(None);
        }
        let Some(mut creds) = self.fetch_llm_credentials().await? else {
            return Ok(None);
        };
        if creds.platform_providers.is_empty() {
            return Ok(None);
        }
        creds.model_catalog_hash = Some(server_hash.to_owned());
        Ok(Some(creds))
    }

    /// Query official account balance for run_chat gate (with transient retries).
    pub async fn fetch_partner_balance(&self) -> Result<PartnerBalanceResponse> {
        let url = format!(
            "{}/auth/partner/balance",
            Self::api_base().trim_end_matches('/')
        );
        let mut last_err: Option<anyhow::Error> = None;
        for attempt in 1..=REFRESH_TRANSIENT_MAX_ATTEMPTS {
            // Re-resolve access token each attempt (may refresh after a prior 401 race).
            let token = self.ensure_access_token().await?;
            match self.fetch_partner_balance_once(&url, &token).await {
                Ok(parsed) => {
                    if attempt > 1 {
                        log::info!(
                            "platform_auth: partner balance succeeded after transient retry attempt={attempt}/{}",
                            REFRESH_TRANSIENT_MAX_ATTEMPTS
                        );
                    }
                    log::info!(
                        "platform_auth: partner balance_yuan={} exhausted={}",
                        parsed.balance_yuan,
                        parsed.token_quota_exhausted
                    );
                    return Ok(parsed);
                }
                Err(e) if Self::is_refresh_auth_failure(&e) => {
                    log::warn!("platform_auth: partner balance auth failure: {e:#}");
                    return Err(e);
                }
                Err(e) if self.is_refresh_transient_failure(&e) => {
                    log::warn!(
                        "platform_auth: partner balance transient failure attempt={attempt}/{}: {e:#}",
                        REFRESH_TRANSIENT_MAX_ATTEMPTS
                    );
                    last_err = Some(e);
                    if attempt < REFRESH_TRANSIENT_MAX_ATTEMPTS {
                        self.sleep_transient_retry(attempt, "partner balance").await;
                    }
                }
                Err(e) => {
                    log::warn!("platform_auth: partner balance non-retryable failure: {e:#}");
                    return Err(e);
                }
            }
        }
        let err = last_err.unwrap_or_else(|| anyhow!("partner balance request failed"));
        log::warn!(
            "platform_auth: partner balance gave up after {} transient attempts: {err:#}",
            REFRESH_TRANSIENT_MAX_ATTEMPTS
        );
        Err(err)
    }

    async fn fetch_partner_balance_once(
        &self,
        url: &str,
        token: &str,
    ) -> Result<PartnerBalanceResponse> {
        let resp = self
            .http
            .get(url)
            .header("Authorization", format!("Bearer {token}"))
            .send()
            .await
            .context("partner balance request failed")?;
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            self.clear_session_async().await;
            return Err(anyhow!("platform_token_expired"));
        }
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "partner balance request failed http_status={} (HTTP {status}): {text}",
                status.as_u16()
            ));
        }
        resp.json().await.context("parse partner balance")
    }

    pub async fn fetch_llm_api_key(&self) -> Result<Option<String>> {
        Ok(self.fetch_llm_credentials().await?.and_then(|c| c.api_key))
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
    #[serde(
        default,
        rename = "modelCatalog",
        alias = "model_catalog",
        alias = "providerModels"
    )]
    model_catalog: HashMap<String, Vec<String>>,
    #[serde(default, rename = "platformProviders", alias = "platform_providers")]
    platform_providers: Vec<PlatformProviderTemplate>,
    #[serde(default, rename = "tierDefaults", alias = "tier_defaults")]
    tier_defaults: serde_json::Value,
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
    #[serde(
        default,
        rename = "modelCatalog",
        alias = "model_catalog",
        alias = "providerModels"
    )]
    model_catalog: HashMap<String, Vec<String>>,
    #[serde(default, rename = "platformProviders", alias = "platform_providers")]
    platform_providers: Vec<PlatformProviderTemplate>,
    #[serde(default, rename = "tierDefaults", alias = "tier_defaults")]
    tier_defaults: serde_json::Value,
    #[serde(default)]
    error_code: Option<String>,
    #[serde(default, rename = "included_tokens")]
    included_tokens: Option<u64>,
    #[serde(default, rename = "consumed_tokens")]
    consumed_tokens: Option<u64>,
    #[serde(default, rename = "mediaOss", alias = "media_oss")]
    media_oss: Option<PlatformMediaOssCredentials>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PartnerBalanceResponse {
    pub balance_yuan: String,
    #[serde(default, rename = "token_quota_exhausted")]
    pub token_quota_exhausted: bool,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default, rename = "modelCatalogHash", alias = "model_catalog_hash")]
    pub model_catalog_hash: Option<String>,
}

/// 从首选端口起扫描，绑定第一个可用的 127.0.0.1 端口。
///
/// 每次登录尽量换端口：浏览器会对 `127.0.0.1:port` 做 TCP/HTTP keep-alive，
/// 第二次仍绑同一端口时，回调 GET 可能打到已失效的旧连接上，表现为
/// 「第一次登录成功、退出后再登卡住」（仅空 TCP、无 HTTP）。
pub async fn bind_loopback_listener() -> Result<(tokio::net::TcpListener, u16)> {
    let preferred = PlatformAuthManager::preferred_loopback_port();
    let last = LAST_LOOPBACK_PORT.load(Ordering::SeqCst);
    let start_offset =
        if last >= preferred && last < preferred.saturating_add(LOOPBACK_PORT_SCAN_COUNT) {
            // Prefer the port after the last successful bind.
            ((last - preferred) as u32 + 1) % (LOOPBACK_PORT_SCAN_COUNT as u32)
        } else {
            0
        };

    let mut last_err: Option<std::io::Error> = None;
    for i in 0..LOOPBACK_PORT_SCAN_COUNT {
        let offset = ((start_offset + i as u32) % (LOOPBACK_PORT_SCAN_COUNT as u32)) as u16;
        let port = preferred.saturating_add(offset);
        match tokio::net::TcpListener::bind(format!("127.0.0.1:{port}")).await {
            Ok(listener) => {
                LAST_LOOPBACK_PORT.store(port, Ordering::SeqCst);
                log::info!("platform_auth: bound port={port}");
                return Ok((listener, port));
            }
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
                last_err = Some(e);
                continue;
            }
            Err(e) => return Err(e).context("bind loopback oauth listener"),
        }
    }
    let end = preferred.saturating_add(LOOPBACK_PORT_SCAN_COUNT.saturating_sub(1));
    if let Some(e) = last_err {
        Err(anyhow!(
            "no free loopback port in range {preferred}..{end} (last error: {e})"
        ))
    } else {
        Err(anyhow!("no free loopback port in range {preferred}..{end}"))
    }
}

/// After bind: connect to self to verify localhost is not blocked.
pub async fn probe_loopback(listener: &tokio::net::TcpListener, port: u16) -> Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    log::info!("platform_auth: loopback probe start port={port}");
    let addr = format!("127.0.0.1:{port}");
    let probe = async {
        let accept_fut = listener.accept();
        let connect_fut = tokio::net::TcpStream::connect(&addr);
        let ((mut inbound, _), mut outbound) =
            tokio::try_join!(accept_fut, connect_fut).context("loopback probe connect/accept")?;
        outbound
            .write_all(b"GET /probe HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
            .await
            .context("loopback probe write request")?;
        let mut buf = [0u8; 512];
        let n = inbound
            .read(&mut buf)
            .await
            .context("loopback probe read request")?;
        if n == 0 {
            anyhow::bail!("loopback probe empty read");
        }
        inbound
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
            .await
            .context("loopback probe write response")?;
        let _ = outbound.read(&mut buf).await;
        let _ = inbound.shutdown().await;
        let _ = outbound.shutdown().await;
        Ok::<(), anyhow::Error>(())
    };

    match tokio::time::timeout(Duration::from_secs(LOOPBACK_PROBE_TIMEOUT_SEC), probe).await {
        Ok(Ok(())) => {
            log::info!("platform_auth: loopback probe ok port={port}");
            Ok(())
        }
        Ok(Err(e)) => Err(e).context(format!(
            "本机回环不可用（127.0.0.1:{port}）：请检查防火墙、安全软件、VPN 或系统代理是否拦截 localhost"
        )),
        Err(_) => Err(anyhow!(
            "本机回环自检超时（127.0.0.1:{port}）：请检查防火墙、安全软件、VPN 或系统代理是否拦截 localhost"
        )),
    }
}

fn http_response(status: &str, content_type: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

async fn write_http(
    stream: &mut tokio::net::TcpStream,
    status: &str,
    content_type: &str,
    body: &str,
) {
    use tokio::io::AsyncWriteExt;
    let response = http_response(status, content_type, body);
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.shutdown().await;
}

/// Read until `\r\n\r\n` (HTTP headers complete) or timeout / EOF.
/// Short first-byte wait drops Chrome idle preconnects; longer window finishes a real request.
async fn read_http_headers(
    stream: &mut tokio::net::TcpStream,
    first_byte_timeout: Duration,
    headers_timeout: Duration,
) -> Result<String> {
    use tokio::io::AsyncReadExt;

    let mut tmp = [0u8; 1024];
    let mut buf: Vec<u8> = Vec::with_capacity(2048);

    match tokio::time::timeout(first_byte_timeout, stream.read(&mut tmp)).await {
        Ok(Ok(0)) => anyhow::bail!("empty connection"),
        Ok(Ok(n)) => buf.extend_from_slice(&tmp[..n]),
        Ok(Err(e)) => return Err(e).context("read first byte"),
        Err(_) => anyhow::bail!("idle preconnect"),
    }

    let deadline = tokio::time::Instant::now() + headers_timeout;
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
        if buf.len() > 64 * 1024 {
            anyhow::bail!("http request too large");
        }
        let now = tokio::time::Instant::now();
        if now >= deadline {
            anyhow::bail!("headers timeout");
        }
        match tokio::time::timeout(deadline - now, stream.read(&mut tmp)).await {
            Ok(Ok(0)) => break,
            Ok(Ok(n)) => buf.extend_from_slice(&tmp[..n]),
            Ok(Err(e)) => return Err(e).context("read http headers"),
            Err(_) => anyhow::bail!("headers timeout"),
        }
    }
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

fn oauth_redirect_response(location: &str) -> String {
    format!(
        "HTTP/1.1 302 Found\r\n\
         Location: {location}\r\n\
         Cache-Control: no-store, no-cache, must-revalidate\r\n\
         Connection: close\r\n\
         Content-Length: 0\r\n\
         \r\n"
    )
}

/// Handle one accepted connection. Returns `Some(code)` on valid OAuth callback.
async fn handle_loopback_connection(
    mut stream: tokio::net::TcpStream,
    peer: std::net::SocketAddr,
    expected_state: &str,
    first_byte_timeout: Duration,
    headers_timeout: Duration,
) -> Option<String> {
    use tokio::io::AsyncWriteExt;

    let req = match read_http_headers(&mut stream, first_byte_timeout, headers_timeout).await {
        Ok(r) => r,
        Err(e) => {
            log::debug!("platform_auth: drop peer={peer}: {e:#}");
            let _ = stream.shutdown().await;
            return None;
        }
    };
    let first_line = req.lines().next().unwrap_or("");
    let path = first_line.split_whitespace().nth(1).unwrap_or("/callback");

    match parse_callback_query(path, expected_state) {
        Ok((code, _state)) => {
            log::info!("platform_auth: accepted oauth callback peer={peer}");
            let location = desktop_oauth_success_redirect_url();
            let response = oauth_redirect_response(&location);
            let _ = stream.write_all(response.as_bytes()).await;
            let _ = stream.shutdown().await;
            Some(code)
        }
        Err(e) => {
            log::warn!("platform_auth: ignored callback peer={peer}: {e:#}");
            write_http(
                &mut stream,
                "400 Bad Request",
                "text/plain; charset=utf-8",
                "invalid oauth callback",
            )
            .await;
            None
        }
    }
}

/// Accept until a valid `code`, timeout, or cancel. Connections are handled
/// concurrently so idle browser preconnects cannot block the real callback.
/// Returns `(code, listener)` so the caller can keep serving during token exchange.
pub async fn wait_loopback_on_listener(
    listener: tokio::net::TcpListener,
    expected_state: &str,
    timeout_sec: u64,
    cancel: CancellationToken,
) -> Result<(String, tokio::net::TcpListener)> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(timeout_sec);
    let expected = expected_state.to_string();
    let (code_tx, mut code_rx) = tokio::sync::mpsc::channel::<String>(1);
    let handlers_cancel = CancellationToken::new();

    let result = loop {
        if cancel.is_cancelled() {
            log::info!("platform_auth: listener closed (cancelled)");
            break Err(anyhow!("platform_login_cancelled"));
        }
        let now = tokio::time::Instant::now();
        if now >= deadline {
            log::info!("platform_auth: listener closed (oauth callback timeout)");
            break Err(anyhow!("oauth callback timeout"));
        }
        let left = deadline - now;
        tokio::select! {
            _ = cancel.cancelled() => {
                log::info!("platform_auth: listener closed (cancelled)");
                break Err(anyhow!("platform_login_cancelled"));
            }
            maybe_code = code_rx.recv() => {
                match maybe_code {
                    Some(code) => break Ok(code),
                    None => break Err(anyhow!("oauth callback channel closed")),
                }
            }
            accept = tokio::time::timeout(left, listener.accept()) => {
                match accept {
                    Err(_) => {
                        log::info!("platform_auth: listener closed (oauth callback timeout)");
                        break Err(anyhow!("oauth callback timeout"));
                    }
                    Ok(Err(e)) => break Err(anyhow!("accept failed: {e}")),
                    Ok(Ok((stream, peer))) => {
                        let tx = code_tx.clone();
                        let expected = expected.clone();
                        let handler_cancel = handlers_cancel.clone();
                        let first_byte_timeout = std::cmp::min(
                            left,
                            Duration::from_millis(LOOPBACK_FIRST_BYTE_TIMEOUT_MS),
                        );
                        let headers_timeout = std::cmp::min(
                            left,
                            Duration::from_secs(LOOPBACK_HEADERS_TIMEOUT_SEC),
                        );
                        tokio::spawn(async move {
                            let outcome = tokio::select! {
                                _ = handler_cancel.cancelled() => None,
                                code = handle_loopback_connection(
                                    stream,
                                    peer,
                                    &expected,
                                    first_byte_timeout,
                                    headers_timeout,
                                ) => code,
                            };
                            if let Some(code) = outcome {
                                let _ = tx.try_send(code);
                            }
                        });
                    }
                }
            }
        }
    };

    handlers_cancel.cancel();
    drop(code_tx);
    while code_rx.try_recv().is_ok() {}

    Ok((result?, listener))
}

/// After code is captured: 302 further hits to the success page until exchange finishes.
async fn serve_oauth_keepalive(listener: tokio::net::TcpListener, cancel: CancellationToken) {
    let response = oauth_redirect_response(&desktop_oauth_success_redirect_url());
    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                log::info!("platform_auth: listener closed");
                break;
            }
            accept = listener.accept() => {
                match accept {
                    Ok((mut stream, _peer)) => {
                        use tokio::io::AsyncWriteExt;
                        let _ = stream.write_all(response.as_bytes()).await;
                        let _ = stream.shutdown().await;
                    }
                    Err(e) => {
                        log::warn!("platform_auth: keepalive accept error: {e}");
                        log::info!("platform_auth: listener closed");
                        break;
                    }
                }
            }
        }
    }
}

/// 桌面 OAuth 回调成功后跳转官网首页（带一次性提示用的 query）。
pub fn desktop_oauth_success_redirect_url() -> String {
    let home = format!("{}/", platform_endpoints::web_base().trim_end_matches('/'));
    format!("{home}?{DESKTOP_OAUTH_SUCCESS_QUERY}={DESKTOP_OAUTH_SUCCESS_VALUE}")
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
    probe_loopback(&listener, port).await?;
    let redirect_uri = PlatformAuthManager::redirect_uri_for_port(port);

    let wait_cancel = cancel.clone();
    let callback_task = tokio::spawn(async move {
        wait_loopback_on_listener(listener, state, OAUTH_CALLBACK_TIMEOUT_SEC, wait_cancel).await
    });

    let url = PlatformAuthManager::build_authorize_url_for_port(port, &challenge, state);
    if let Err(e) = open_url_in_browser(&url) {
        cancel.cancel();
        let _ = callback_task.await;
        return Err(e);
    }

    let (code, listener) = callback_task.await.context("oauth callback task join")??;

    let keepalive_cancel = CancellationToken::new();
    let kc = keepalive_cancel.clone();
    let keepalive_task = tokio::spawn(async move {
        serve_oauth_keepalive(listener, kc).await;
    });

    log::info!("platform_auth: exchange start");
    let result = auth
        .exchange_authorization_code(&code, &verifier, state, &redirect_uri)
        .await;
    match &result {
        Ok(_) => log::info!("platform_auth: exchange done ok"),
        Err(e) => log::warn!("platform_auth: exchange done err={e:#}"),
    }

    keepalive_cancel.cancel();
    if let Err(e) = keepalive_task.await {
        log::warn!("platform_auth: keepalive task join: {e}");
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refresh_auth_failure_uses_http_status_not_proxy_noise() {
        let auth = PlatformAuthManager::new();
        let auth_fail = anyhow!(
            "token exchange failed http_status=401 (401 Unauthorized): invalid_refresh_token"
        );
        let legacy_auth = anyhow!("token exchange failed (403 Forbidden): denied");
        let proxy_noise = anyhow!("upstream gateway 401 while connecting to CDN");
        let network = anyhow!("token request failed: error sending request for url");
        let gateway = anyhow!("token exchange failed http_status=502 (502 Bad Gateway): oops");
        let bad_request = anyhow!("token exchange failed http_status=400 (400 Bad Request): bad");
        assert!(!auth.is_refresh_transient_failure(&auth_fail));
        assert!(!auth.is_refresh_transient_failure(&legacy_auth));
        assert!(auth.is_refresh_transient_failure(&proxy_noise));
        assert!(auth.is_refresh_transient_failure(&network));
        assert!(auth.is_refresh_transient_failure(&gateway));
        assert!(!auth.is_refresh_transient_failure(&bad_request));
        assert_eq!(
            PlatformAuthManager::parse_http_status_from_error(&auth_fail),
            Some(401)
        );
        assert_eq!(
            PlatformAuthManager::parse_http_status_from_error(&gateway),
            Some(502)
        );
    }

    #[test]
    fn refresh_transient_retry_uses_backoff_schedule() {
        assert_eq!(REFRESH_TRANSIENT_MAX_ATTEMPTS, 4);
        assert_eq!(REFRESH_TRANSIENT_RETRY_COUNT, 3);
        assert_eq!(
            PlatformAuthManager::refresh_transient_retry_delay(1).as_millis(),
            800
        );
        assert_eq!(
            PlatformAuthManager::refresh_transient_retry_delay(2).as_millis(),
            2000
        );
        assert_eq!(
            PlatformAuthManager::refresh_transient_retry_delay(3).as_millis(),
            4000
        );
    }

    #[test]
    fn partner_balance_accepts_missing_or_present_model_catalog_hash() {
        let old: PartnerBalanceResponse = serde_json::from_str(
            r#"{"balance_yuan":"1.000","token_quota_exhausted":false,"message":null}"#,
        )
        .expect("old response remains compatible");
        assert_eq!(old.model_catalog_hash, None);

        let current: PartnerBalanceResponse = serde_json::from_str(
            r#"{"balance_yuan":"1.000","token_quota_exhausted":false,"modelCatalogHash":"abc"}"#,
        )
        .expect("new response parses");
        assert_eq!(current.model_catalog_hash.as_deref(), Some("abc"));
    }

    #[test]
    fn build_authorize_url_for_redirect_includes_pkce_and_encoded_redirect() {
        let url = PlatformAuthManager::build_authorize_url_for_redirect(
            "https://pointer.example.com/api/auth/oauth/callback",
            "challenge_abc",
            "pointer-app",
        );
        assert!(url.starts_with(&format!(
            "{}/oauth/authorize?",
            PlatformAuthManager::web_base()
        )));
        assert!(url.contains("client_id=pointer-desktop"));
        assert!(url.contains("code_challenge=challenge_abc"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("state=pointer-app"));
        assert!(url.contains(
            "redirect_uri=https%3A%2F%2Fpointer.example.com%2Fapi%2Fauth%2Foauth%2Fcallback"
        ));
        assert!(
            url.contains('&'),
            "authorize URL must keep query separators"
        );
    }

    #[test]
    fn build_authorize_url_includes_pkce_and_encoded_redirect() {
        let url = PlatformAuthManager::build_authorize_url_for_port(
            19427,
            "challenge_abc",
            "pointer-app",
        );
        assert!(url.starts_with(&format!(
            "{}/oauth/authorize?",
            PlatformAuthManager::web_base()
        )));
        assert!(url.contains("client_id=pointer-desktop"));
        assert!(url.contains("code_challenge=challenge_abc"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("state=pointer-app"));
        assert!(url.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A19427%2Fcallback"));
        assert!(
            url.contains('&'),
            "authorize URL must keep query separators"
        );
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

    #[test]
    fn parse_callback_query_accepts_valid_code() {
        let (code, state) =
            parse_callback_query("/callback?code=abc%201&state=pointer-app", "pointer-app")
                .expect("valid callback");
        assert_eq!(code, "abc 1");
        assert_eq!(state, "pointer-app");
    }

    #[test]
    fn parse_callback_query_rejects_missing_code_or_bad_state() {
        assert!(parse_callback_query("/callback?state=pointer-app", "pointer-app").is_err());
        assert!(parse_callback_query("/callback?code=x&state=other", "pointer-app").is_err());
    }

    #[tokio::test]
    async fn probe_loopback_succeeds_after_bind() {
        let (listener, port) = bind_loopback_listener().await.expect("bind");
        probe_loopback(&listener, port).await.expect("probe");
    }

    #[tokio::test]
    async fn wait_loopback_ignores_invalid_then_accepts_code() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind ephemeral");
        let port = listener.local_addr().expect("addr").port();
        let cancel = CancellationToken::new();
        let wait = tokio::spawn({
            let cancel = cancel.clone();
            async move { wait_loopback_on_listener(listener, "pointer-app", 15, cancel).await }
        });

        // Invalid first hit must not steal the accept slot permanently.
        {
            let mut stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{port}"))
                .await
                .expect("connect invalid");
            stream
                .write_all(b"GET /callback?foo=1 HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n")
                .await
                .expect("write invalid");
            let mut buf = [0u8; 256];
            let _ = stream.read(&mut buf).await;
        }

        let mut stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{port}"))
            .await
            .expect("connect valid");
        stream
            .write_all(
                b"GET /callback?code=test-code&state=pointer-app HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
            )
            .await
            .expect("write valid");
        let mut buf = vec![0u8; 1024];
        let n = stream.read(&mut buf).await.expect("read 302");
        let resp = String::from_utf8_lossy(&buf[..n]);
        assert!(resp.contains("302"), "expected 302, got {resp}");
        assert!(resp.contains("desktop_oauth=success"));

        let (code, listener) = wait.await.expect("join").expect("wait ok");
        assert_eq!(code, "test-code");

        let keepalive_cancel = CancellationToken::new();
        let kc = keepalive_cancel.clone();
        let keepalive = tokio::spawn(async move {
            serve_oauth_keepalive(listener, kc).await;
        });

        let mut stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{port}"))
            .await
            .expect("keepalive connect");
        stream
            .write_all(b"GET / HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n")
            .await
            .expect("keepalive write");
        let n = stream.read(&mut buf).await.expect("keepalive read");
        let resp = String::from_utf8_lossy(&buf[..n]);
        assert!(resp.contains("302"), "expected keepalive 302, got {resp}");
        assert!(resp.contains("desktop_oauth=success"));

        keepalive_cancel.cancel();
        keepalive.await.expect("keepalive join");
    }

    #[tokio::test]
    async fn wait_loopback_cancelled_before_code() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind ephemeral");
        let cancel = CancellationToken::new();
        let wait = tokio::spawn({
            let cancel = cancel.clone();
            async move { wait_loopback_on_listener(listener, "pointer-app", 30, cancel).await }
        });
        cancel.cancel();
        let err = wait.await.expect("join").expect_err("should cancel");
        assert!(
            err.to_string().contains("platform_login_cancelled"),
            "got {err:#}"
        );
    }

    /// Browser often TCP-preconnects before sending GET. Closing that socket early
    /// drops the later callback; we must keep reading and still accept other conns.
    #[tokio::test]
    async fn wait_loopback_preconnect_then_get_on_same_connection() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind ephemeral");
        let port = listener.local_addr().expect("addr").port();
        let cancel = CancellationToken::new();
        let wait = tokio::spawn({
            let cancel = cancel.clone();
            async move { wait_loopback_on_listener(listener, "pointer-app", 15, cancel).await }
        });

        let mut stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{port}"))
            .await
            .expect("preconnect");
        // Hold the connection idle briefly (simulates browser preconnect).
        tokio::time::sleep(Duration::from_millis(200)).await;
        stream
            .write_all(
                b"GET /callback?code=late-code&state=pointer-app HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
            )
            .await
            .expect("delayed GET");
        let mut buf = vec![0u8; 1024];
        let n = stream.read(&mut buf).await.expect("read 302");
        assert!(
            String::from_utf8_lossy(&buf[..n]).contains("302"),
            "expected 302"
        );

        let (code, _listener) = wait.await.expect("join").expect("wait ok");
        assert_eq!(code, "late-code");
    }

    #[tokio::test]
    async fn wait_loopback_preconnect_does_not_block_other_callback() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind ephemeral");
        let port = listener.local_addr().expect("addr").port();
        let cancel = CancellationToken::new();
        let wait = tokio::spawn({
            let cancel = cancel.clone();
            async move { wait_loopback_on_listener(listener, "pointer-app", 15, cancel).await }
        });

        let _preconnect = tokio::net::TcpStream::connect(format!("127.0.0.1:{port}"))
            .await
            .expect("preconnect idle");
        tokio::time::sleep(Duration::from_millis(50)).await;

        let mut stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{port}"))
            .await
            .expect("real callback");
        stream
            .write_all(
                b"GET /callback?code=other-conn&state=pointer-app HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
            )
            .await
            .expect("write");
        let mut buf = vec![0u8; 1024];
        let n = stream.read(&mut buf).await.expect("read");
        assert!(String::from_utf8_lossy(&buf[..n]).contains("302"));

        let (code, _) = wait.await.expect("join").expect("wait ok");
        assert_eq!(code, "other-conn");
    }

    #[tokio::test]
    async fn bind_loopback_rotates_port_across_calls() {
        // Reset so this test is deterministic regardless of prior tests.
        LAST_LOOPBACK_PORT.store(0, Ordering::SeqCst);
        let (l1, p1) = bind_loopback_listener().await.expect("bind1");
        let (l2, p2) = bind_loopback_listener().await.expect("bind2");
        assert_ne!(
            p1, p2,
            "second bind should rotate away from first port to avoid browser keep-alive reuse"
        );
        drop(l1);
        drop(l2);
        LAST_LOOPBACK_PORT.store(0, Ordering::SeqCst);
    }
}

pub type SharedPlatformAuth = Arc<PlatformAuthManager>;
