//! Webhook bearer tokens (Phase 6+): per-source, encrypted-at-rest, first-write-only.
//!
//! Each ingress source (`POST /api/webhooks/:src`) has its own token stored under
//! `app_secrets` label `webhook_token:{src}`. By default auth accepts
//! `Authorization: Bearer` or `X-Pointer-Token` (OpenClaw-style alternate header).
//! Each source may optionally configure a custom header name (e.g. `X-Codeup-Token`).
//!
//! Legacy single-token rows (`webhook_bearer_token`) remain readable as a fallback
//! for all sources until migrated to per-source tokens.

use anyhow::Result;
use subtle::ConstantTimeEq;

use crate::conversation_store::ConversationStore;

const LEGACY_LABEL: &str = "webhook_bearer_token";
const TOKEN_LABEL_PREFIX: &str = "webhook_token:";

/// Stable webhook source key (`webhook:{src}`). Stored on UI rows; transcript
/// lives under [`current_webhook_session_id`] / `current_session_id`.
pub fn webhook_session_key(src: &str) -> String {
    format!("webhook:{src}")
}

/// Alias for the stable source key (backward compat).
pub fn webhook_conversation_id(src: &str) -> String {
    webhook_session_key(src)
}

pub use crate::conversation_store::webhook_sources::{
    current_webhook_session_id, WebhookSessionMode,
};

/// Sanitize a delivery / idempotency id for use in a session key segment.
pub fn sanitize_delivery_id(raw: &str) -> String {
    let out: String = raw
        .trim()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(64)
        .collect();
    if out.is_empty() {
        new_delivery_id()
    } else {
        out
    }
}

/// Generate a fresh delivery id when none is provided by the caller or headers.
pub fn new_delivery_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

/// Session id for per-delivery webhook mode.
pub fn per_delivery_session_id(src: &str, delivery_id: &str) -> String {
    format!("webhook:{src}:{}", sanitize_delivery_id(delivery_id))
}

/// Resolve delivery id from webhook headers and optional idempotency key.
pub fn extract_delivery_id(headers: &http::HeaderMap, idempotency_key: Option<&str>) -> String {
    if let Some(key) = idempotency_key.map(str::trim).filter(|s| !s.is_empty()) {
        return sanitize_delivery_id(key);
    }
    const HEADER_CANDIDATES: &[&str] = &["x-github-delivery", "svix-id", "x-request-id"];
    for name in HEADER_CANDIDATES {
        if let Some(value) = headers
            .get(*name)
            .and_then(|v| v.to_str().ok())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            return sanitize_delivery_id(value);
        }
    }
    new_delivery_id()
}

/// True when `conversation_id` is the legacy or dated webhook session for `:src`.
pub fn conversation_id_matches_webhook_src(conversation_id: &str, src: &str) -> bool {
    let id = conversation_id.trim();
    let src = src.trim();
    if id.is_empty() || src.is_empty() {
        return false;
    }
    if id == webhook_session_key(src) {
        return true;
    }
    let prefix = format!("webhook:{src}:");
    id.starts_with(&prefix) && id.len() > prefix.len()
}

/// Reverse-map a media directory segment to a webhook session id for `:src`.
pub fn webhook_session_id_from_storage_segment(src: &str, segment: &str) -> Option<String> {
    let legacy_seg = crate::storage::sanitize_storage_dir_segment(&webhook_session_key(src));
    if segment == legacy_seg {
        return Some(webhook_session_key(src));
    }
    let prefix = format!("{legacy_seg}_");
    let date = segment.strip_prefix(&prefix)?;
    if date.len() == 8 && date.chars().all(|c| c.is_ascii_digit()) {
        Some(format!("webhook:{src}:{date}"))
    } else if !date.is_empty() {
        Some(format!("webhook:{src}:{date}"))
    } else {
        None
    }
}

/// Display title for a webhook session row / UI shell.
pub fn webhook_session_title(src: &str) -> String {
    format!("[Webhook] {src}")
}

/// One configured webhook source (camelCase JSON view).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebhookSourceView {
    pub src: String,
    /// Full token for settings UI copy (local IPC / trusted settings surface).
    pub token: String,
    pub preview: String,
    /// Optional custom auth header; null = default Bearer + X-Pointer-Token.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_header_name: Option<String>,
    /// `daily` or `per_delivery`.
    #[serde(default)]
    pub session_mode: WebhookSessionMode,
    /// Full ingress URL for this source (host fills in; empty on desktop).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub url: String,
    /// Stable source key (`webhook:{src}`).
    pub conversation_id: String,
    /// Active session id (`webhook:{src}:{yyyymmdd}`); null until first ingress.
    pub current_session_id: Option<String>,
    /// Whether any session for this source has stored messages.
    pub has_transcript: bool,
}

/// Aggregate webhook config for the automation settings UI.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebhookConfigView {
    pub sources: Vec<WebhookSourceView>,
    pub url_template: String,
    /// Deprecated global token still on disk (backward compat).
    pub legacy_configured: bool,
    pub legacy_preview: Option<String>,
}

/// Web-safe webhook source row: preview only, no bearer token.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebhookSourcePublicView {
    pub src: String,
    pub preview: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_header_name: Option<String>,
    #[serde(default)]
    pub session_mode: WebhookSessionMode,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub url: String,
    pub conversation_id: String,
    pub current_session_id: Option<String>,
    pub has_transcript: bool,
}

/// Web-safe webhook config for pointer-server HTTP API.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebhookConfigPublicView {
    pub sources: Vec<WebhookSourcePublicView>,
    pub url_template: String,
    pub legacy_configured: bool,
    pub legacy_preview: Option<String>,
}

/// Full bearer token for one webhook source (settings reveal; requires platform auth).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebhookTokenRevealView {
    pub src: String,
    pub token: String,
    pub preview: String,
}

impl From<WebhookSourceView> for WebhookSourcePublicView {
    fn from(source: WebhookSourceView) -> Self {
        Self {
            src: source.src,
            preview: source.preview,
            auth_header_name: source.auth_header_name,
            session_mode: source.session_mode,
            url: source.url,
            conversation_id: source.conversation_id,
            current_session_id: source.current_session_id,
            has_transcript: source.has_transcript,
        }
    }
}

impl From<WebhookConfigView> for WebhookConfigPublicView {
    fn from(view: WebhookConfigView) -> Self {
        Self {
            sources: view.sources.into_iter().map(Into::into).collect(),
            url_template: view.url_template,
            legacy_configured: view.legacy_configured,
            legacy_preview: view.legacy_preview,
        }
    }
}

pub struct WebhookTokenStore<'a> {
    store: &'a ConversationStore,
}

impl<'a> WebhookTokenStore<'a> {
    pub fn new(store: &'a ConversationStore) -> Self {
        Self { store }
    }

    /// Normalize and validate a webhook source id from the URL path segment.
    pub fn normalize_src(src: &str) -> Result<String> {
        let trimmed = src.trim();
        if trimmed.is_empty() {
            anyhow::bail!("webhook source must not be empty");
        }
        if trimmed.len() > 64 {
            anyhow::bail!("webhook source too long (max 64 chars)");
        }
        let ok = trimmed
            .chars()
            .enumerate()
            .all(|(i, c)| {
                c.is_ascii_alphanumeric() || (i > 0 && (c == '-' || c == '_'))
            });
        if !ok {
            anyhow::bail!(
                "webhook source must start with a letter or digit and contain only letters, digits, '-' or '_'"
            );
        }
        Ok(trimmed.to_string())
    }

    fn token_label(src: &str) -> String {
        format!("{TOKEN_LABEL_PREFIX}{src}")
    }

    fn src_from_label(label: &str) -> Option<String> {
        label.strip_prefix(TOKEN_LABEL_PREFIX).map(str::to_string)
    }

    /// Build a UI view row for one configured source.
    pub fn source_view(&self, src: String, token: String, url: String) -> Result<WebhookSourceView> {
        use crate::conversation_store::webhook_sources;
        let preview = mask_token(&token);
        let conversation_id = webhook_session_key(&src);
        let legacy_has = self.store.message_count(&conversation_id)? > 0;
        let record = self.store.webhook_sources_get(&src)?;
        let current_session_id =
            webhook_sources::resolve_view_session_id(record.as_ref(), &src);
        let auth_header_name = record
            .as_ref()
            .and_then(|r| r.auth_header_name.clone())
            .filter(|s| !s.trim().is_empty());
        let session_mode = record
            .as_ref()
            .map(|r| r.session_mode)
            .unwrap_or(WebhookSessionMode::PerDelivery);
        let has_transcript = legacy_has
            || current_session_id
                .as_ref()
                .map(|id| self.store.message_count(id))
                .transpose()?
                .unwrap_or(0)
                > 0;
        Ok(WebhookSourceView {
            src,
            token,
            preview,
            auth_header_name,
            session_mode,
            url,
            conversation_id,
            current_session_id,
            has_transcript,
        })
    }

    /// Normalize optional custom auth header name from UI/API input.
    pub fn normalize_auth_header_name(name: &str) -> Result<Option<String>> {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Ok(None);
        }
        if trimmed.len() > 64 {
            anyhow::bail!("auth header name too long (max 64 chars)");
        }
        let ok = trimmed.chars().enumerate().all(|(i, c)| {
            if i == 0 {
                c.is_ascii_alphabetic()
            } else {
                c.is_ascii_alphanumeric() || c == '-'
            }
        });
        if !ok {
            anyhow::bail!(
                "auth header name must start with a letter and contain only letters, digits, or '-'"
            );
        }
        Ok(Some(trimmed.to_string()))
    }

    /// Auth header configured for `:src` (None = default Bearer + X-Pointer-Token).
    pub fn auth_header_name_for_source(&self, src: &str) -> Result<Option<String>> {
        let src = Self::normalize_src(src)?;
        let name = self.store.webhook_sources_auth_header_name(&src)?;
        Ok(name.filter(|s| !s.trim().is_empty()))
    }

    /// List all per-source tokens (src + full token).
    pub fn list_sources(&self) -> Result<Vec<(String, String)>> {
        let rows = self.store.app_secret_list_by_prefix(TOKEN_LABEL_PREFIX)?;
        let mut out = Vec::new();
        for (label, _) in rows {
            if let Some(src) = Self::src_from_label(&label) {
                if let Some(token) = self.get_token_for_label(&label)? {
                    out.push((src, token));
                }
            }
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(out)
    }

    /// Set token for a source (first-write-only per source).
    pub fn set_source_token(
        &self,
        src: &str,
        token: &str,
        auth_header_name: Option<&str>,
        session_mode: Option<WebhookSessionMode>,
    ) -> Result<bool> {
        let src = Self::normalize_src(src)?;
        let trimmed = token.trim();
        if trimmed.is_empty() {
            anyhow::bail!("webhook token must not be empty");
        }
        let auth_header_name = auth_header_name
            .map(Self::normalize_auth_header_name)
            .transpose()?
            .flatten();
        let label = Self::token_label(&src);
        if self.store.app_secret_has(&label)? {
            log::warn!("webhook_config: set_source_token refused (already configured src={src})");
            return Ok(false);
        }
        let blob = crate::local_secret::encrypt_local_secret(trimmed)?;
        let inserted = self.store.app_secret_try_insert(&label, &blob)?;
        if inserted {
            self.store.webhook_sources_ensure_row(&src)?;
            self.store
                .webhook_sources_set_auth_header_name(&src, auth_header_name.as_deref())?;
            if let Some(mode) = session_mode {
                self.store.webhook_sources_set_session_mode(&src, mode)?;
            }
            log::info!(
                "webhook_config: source token set src={src} auth_header={:?} (encrypted)",
                auth_header_name
            );
        }
        Ok(inserted)
    }

    pub fn is_source_configured(&self, src: &str) -> Result<bool> {
        let src = Self::normalize_src(src)?;
        self.store.app_secret_has(&Self::token_label(&src))
    }

    /// Return the per-source token for settings reveal (not legacy/env fallback).
    pub fn reveal_source_token(&self, src: &str) -> Result<Option<String>> {
        let src = Self::normalize_src(src)?;
        self.get_token_for_label(&Self::token_label(&src))
    }

    pub fn clear_source_token(&self, src: &str) -> Result<bool> {
        let src = Self::normalize_src(src)?;
        let deleted = self.store.app_secret_delete(&Self::token_label(&src))?;
        if deleted {
            let _ = self.store.webhook_sources_delete(&src);
            log::info!("webhook_config: source token cleared src={src}");
        }
        Ok(deleted)
    }

    /// Whether the deprecated global token exists.
    pub fn is_legacy_configured(&self) -> Result<bool> {
        self.store.app_secret_has(LEGACY_LABEL)
    }

    pub fn legacy_preview(&self) -> Result<Option<String>> {
        self.preview_for_label(LEGACY_LABEL)
    }

    pub fn clear_legacy_token(&self) -> Result<bool> {
        let deleted = self.store.app_secret_delete(LEGACY_LABEL)?;
        if deleted {
            log::info!("webhook_config: legacy global token cleared");
        }
        Ok(deleted)
    }

    /// Update session grouping mode for a configured source.
    pub fn set_session_mode(&self, src: &str, mode: WebhookSessionMode) -> Result<()> {
        let src = Self::normalize_src(src)?;
        if !self.store.app_secret_has(&Self::token_label(&src))? {
            anyhow::bail!("webhook token not configured for this source");
        }
        self.store.webhook_sources_ensure_row(&src)?;
        self.store.webhook_sources_set_session_mode(&src, mode)
    }

    pub fn session_mode_for_source(&self, src: &str) -> Result<WebhookSessionMode> {
        let src = Self::normalize_src(src)?;
        Ok(self
            .store
            .webhook_sources_get(&src)?
            .map(|r| r.session_mode)
            .unwrap_or(WebhookSessionMode::PerDelivery))
    }

    /// Resolve expected token for `:src` at request time.
    /// Order: per-source token → legacy global → `POINTER_WEBHOOK_BEARER_TOKEN` env.
    pub fn resolve_for_source(&self, src: &str) -> Result<Option<String>> {
        let src = Self::normalize_src(src)?;
        if let Some(t) = self.get_token_for_label(&Self::token_label(&src))? {
            return Ok(Some(t));
        }
        if let Some(t) = self.get_token_for_label(LEGACY_LABEL)? {
            return Ok(Some(t));
        }
        let env_token = std::env::var("POINTER_WEBHOOK_BEARER_TOKEN").unwrap_or_default();
        if env_token.trim().is_empty() {
            Ok(None)
        } else {
            Ok(Some(env_token))
        }
    }

    /// Constant-time token check for ingress.
    pub fn verify_for_source(&self, src: &str, provided: &str) -> Result<bool> {
        let provided = provided.trim();
        if provided.is_empty() {
            return Ok(false);
        }
        let Some(expected) = self.resolve_for_source(src)? else {
            return Ok(false);
        };
        Ok(constant_time_eq(provided.as_bytes(), expected.as_bytes()))
    }

    fn get_token_for_label(&self, label: &str) -> Result<Option<String>> {
        let Some(blob) = self.store.app_secret_get(label)? else {
            return Ok(None);
        };
        match crate::local_secret::decrypt_local_secret(&blob) {
            Ok(s) if !s.is_empty() => Ok(Some(s)),
            Ok(_) => Ok(None),
            Err(e) => {
                log::warn!("webhook_config: decrypt failed label={label}: {e}");
                Ok(None)
            }
        }
    }

    fn preview_for_label(&self, label: &str) -> Result<Option<String>> {
        let Some(token) = self.get_token_for_label(label)? else {
            return Ok(None);
        };
        Ok(Some(mask_token(&token)))
    }
}

/// Mask token for UI: `****` + last 4 chars.
pub fn mask_token(token: &str) -> String {
    let len = token.chars().count();
    if len <= 4 {
        return "****".into();
    }
    let tail: String = token.chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect();
    format!("****{tail}")
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.ct_eq(b).into()
}

/// Rejection reason for webhook ingress / upload handlers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebhookIngressAuthError {
    InvalidSrc(String),
    NotConfigured,
    Unauthorized,
}

impl std::fmt::Display for WebhookIngressAuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSrc(msg) => f.write_str(msg),
            Self::NotConfigured => {
                f.write_str("webhook ingress disabled: no token configured for this source")
            }
            Self::Unauthorized => f.write_str("unauthorized"),
        }
    }
}

impl std::error::Error for WebhookIngressAuthError {}

/// Verify webhook bearer token for `:src` and return the normalized source id.
pub fn authorize_webhook_ingress(
    store: &WebhookTokenStore<'_>,
    src: &str,
    headers: &http::HeaderMap,
) -> Result<String, WebhookIngressAuthError> {
    let normalized_src = WebhookTokenStore::normalize_src(src)
        .map_err(|e| WebhookIngressAuthError::InvalidSrc(e.to_string()))?;
    let configured = store
        .resolve_for_source(&normalized_src)
        .map_err(|e| WebhookIngressAuthError::InvalidSrc(e.to_string()))?
        .is_some();
    if !configured {
        log::warn!("webhook ingress rejected: no token configured (src={normalized_src})");
        return Err(WebhookIngressAuthError::NotConfigured);
    }
    let auth_header_name = store
        .auth_header_name_for_source(&normalized_src)
        .map_err(|e| WebhookIngressAuthError::InvalidSrc(e.to_string()))?;
    let provided = extract_webhook_token(headers, auth_header_name.as_deref());
    let ok = store
        .verify_for_source(&normalized_src, provided)
        .map_err(|e| WebhookIngressAuthError::InvalidSrc(e.to_string()))?;
    if !ok {
        log::warn!(
            "webhook ingress rejected: bad bearer token (src={normalized_src} header={auth_header_name:?})"
        );
        return Err(WebhookIngressAuthError::Unauthorized);
    }
    Ok(normalized_src)
}

/// Extract webhook token from request headers.
/// When `auth_header_name` is set, only that header is read (raw value).
/// Otherwise accepts `Authorization: Bearer` or `X-Pointer-Token`.
pub fn extract_webhook_token<'a>(
    headers: &'a http::HeaderMap,
    auth_header_name: Option<&str>,
) -> &'a str {
    if let Some(name) = auth_header_name.filter(|s| !s.trim().is_empty()) {
        return headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("");
    }
    if let Some(v) = headers.get(http::header::AUTHORIZATION) {
        if let Ok(s) = v.to_str() {
            if let Some(token) = s.strip_prefix("Bearer ") {
                let t = token.trim();
                if !t.is_empty() {
                    return t;
                }
            }
        }
    }
    headers
        .get("x-pointer-token")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversation_store::ConversationStore;

    fn store() -> ConversationStore {
        let dir = std::env::temp_dir().join(format!(
            "pointer-webhook-config-test-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        ConversationStore::open(dir.join("conversations.db")).unwrap()
    }

    fn cleanup_test_secrets(s: &ConversationStore) {
        for prefix in [LEGACY_LABEL, "webhook_token:"] {
            if let Ok(rows) = s.app_secret_list_by_prefix(prefix) {
                for (label, _) in rows {
                    let _ = s.app_secret_delete(&label);
                }
            }
        }
    }

    #[test]
    fn reveal_source_token_only_per_source_not_legacy() {
        let s = store();
        cleanup_test_secrets(&s);
        let blob = crate::local_secret::encrypt_local_secret("legacy-global").unwrap();
        assert!(s.app_secret_try_insert(LEGACY_LABEL, &blob).unwrap());
        let ts = WebhookTokenStore::new(&s);
        assert!(ts.set_source_token("github", "per-source", None, None).unwrap());
        assert_eq!(
            ts.reveal_source_token("github").unwrap().as_deref(),
            Some("per-source")
        );
        assert!(ts.reveal_source_token("gitlab").unwrap().is_none());
    }

    #[test]
    fn per_source_first_write_and_verify() {
        let s = store();
        cleanup_test_secrets(&s);
        let ts = WebhookTokenStore::new(&s);
        assert!(ts.set_source_token("github", "secret-abcd1234", None, None).unwrap());
        assert!(!ts.set_source_token("github", "other", None, None).unwrap());
        assert!(ts.set_source_token("gitlab", "gitlab-token-9999", None, None).unwrap());
        let list = ts.list_sources().unwrap();
        assert_eq!(list.len(), 2);
        assert!(ts.verify_for_source("github", "secret-abcd1234").unwrap());
        assert!(!ts.verify_for_source("github", "wrong").unwrap());
        assert!(ts.verify_for_source("gitlab", "gitlab-token-9999").unwrap());
        assert!(!ts.verify_for_source("gitlab", "secret-abcd1234").unwrap());
        assert!(ts.clear_source_token("github").unwrap());
        assert!(!ts.verify_for_source("github", "secret-abcd1234").unwrap());
    }

    #[test]
    fn normalize_src_rejects_invalid() {
        assert!(WebhookTokenStore::normalize_src("").is_err());
        assert!(WebhookTokenStore::normalize_src("-bad").is_err());
        assert!(WebhookTokenStore::normalize_src("ok-source_1").is_ok());
    }

    #[test]
    fn legacy_fallback_when_no_per_source() {
        let s = store();
        cleanup_test_secrets(&s);
        let blob = crate::local_secret::encrypt_local_secret("legacy-global").unwrap();
        assert!(s.app_secret_try_insert(LEGACY_LABEL, &blob).unwrap());
        let ts = WebhookTokenStore::new(&s);
        assert!(ts.verify_for_source("any-src", "legacy-global").unwrap());
        assert!(ts.is_legacy_configured().unwrap());
    }

    #[test]
    fn custom_auth_header_name_stored_and_used_for_extract() {
        let s = store();
        cleanup_test_secrets(&s);
        let ts = WebhookTokenStore::new(&s);
        assert!(
            ts.set_source_token("codeup", "tok-codeup", Some("X-Codeup-Token"), None)
                .unwrap()
        );
        assert_eq!(
            ts.auth_header_name_for_source("codeup").unwrap().as_deref(),
            Some("X-Codeup-Token")
        );
        let mut headers = http::HeaderMap::new();
        headers.insert(
            http::HeaderName::from_static("x-codeup-token"),
            http::HeaderValue::from_static("tok-codeup"),
        );
        assert_eq!(
            extract_webhook_token(&headers, Some("X-Codeup-Token")),
            "tok-codeup"
        );
        headers.insert(
            http::header::AUTHORIZATION,
            http::HeaderValue::from_static("Bearer tok-codeup"),
        );
        assert_eq!(
            extract_webhook_token(&headers, Some("X-Codeup-Token")),
            "tok-codeup"
        );
        assert_eq!(extract_webhook_token(&headers, None), "tok-codeup");
    }

    #[test]
    fn normalize_auth_header_name_rejects_invalid() {
        assert!(WebhookTokenStore::normalize_auth_header_name("").unwrap().is_none());
        assert!(WebhookTokenStore::normalize_auth_header_name("  ").unwrap().is_none());
        assert!(WebhookTokenStore::normalize_auth_header_name("-Bad").is_err());
        assert_eq!(
            WebhookTokenStore::normalize_auth_header_name("X-Codeup-Token")
                .unwrap()
                .as_deref(),
            Some("X-Codeup-Token")
        );
    }

    #[test]
    fn webhook_session_id_from_storage_segment_dated_and_legacy() {
        assert_eq!(
            webhook_session_id_from_storage_segment("test", "webhook_test_20260708"),
            Some("webhook:test:20260708".into())
        );
        assert_eq!(
            webhook_session_id_from_storage_segment("test", "webhook_test"),
            Some("webhook:test".into())
        );
        assert!(conversation_id_matches_webhook_src("webhook:test:20260708", "test"));
        assert!(conversation_id_matches_webhook_src("webhook:test", "test"));
        assert!(conversation_id_matches_webhook_src("webhook:test:del-abc", "test"));
        assert!(!conversation_id_matches_webhook_src("webhook:ci:20260708", "test"));
        assert_eq!(
            webhook_session_id_from_storage_segment("test", "webhook_test_del_abc"),
            Some("webhook:test:del_abc".into())
        );
    }

    #[test]
    fn extract_delivery_id_prefers_idempotency_key() {
        let mut headers = http::HeaderMap::new();
        headers.insert(
            "x-github-delivery",
            http::HeaderValue::from_static("gh-del"),
        );
        assert_eq!(
            extract_delivery_id(&headers, Some("idem-key")),
            "idem-key"
        );
        assert_eq!(
            extract_delivery_id(&headers, None),
            "gh-del"
        );
    }
}
