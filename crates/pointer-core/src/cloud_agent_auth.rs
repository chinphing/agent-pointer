//! Pointer agent OAuth code exchange for cloud-hosted pointer-server instances.
//!
//! Env:
//! - `POINTER_API_BASE` — control plane API root (legacy `OPENPOINTER_API_BASE` still read)
//! - `POINTER_OAUTH_CLIENT_SECRET` — shared secret for `POST /auth/oauth/exchange-code`

use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use serde::Deserialize;
use std::collections::HashMap;

use crate::platform_auth::{PlatformLoginCredentials, PlatformSession, PlatformUserSummary};

const EXCHANGE_TIMEOUT_SEC: u64 = 20;

fn env_with_legacy(primary: &str, legacy: &str) -> Option<String> {
    for key in [primary, legacy] {
        if let Ok(value) = std::env::var(key) {
            let trimmed = value.trim().trim_end_matches('/').to_string();
            if !trimmed.is_empty() {
                return Some(trimmed);
            }
        }
    }
    None
}

pub fn control_plane_api_base() -> String {
    env_with_legacy("POINTER_API_BASE", "OPENPOINTER_API_BASE").unwrap_or_else(|| {
        crate::platform_endpoints::api_base()
            .trim_end_matches('/')
            .to_string()
    })
}

pub fn control_plane_oauth_client_secret() -> String {
    env_with_legacy(
        "POINTER_OAUTH_CLIENT_SECRET",
        "OPENPOINTER_OAUTH_CLIENT_SECRET",
    )
    .unwrap_or_default()
}

pub fn is_cloud_auth_configured() -> bool {
    if crate::deployment_mode::is_standalone() {
        return false;
    }
    !control_plane_api_base().is_empty()
}

fn exchange_has_usable_llm_key(parsed: &OAuthCodeExchangeResponse) -> bool {
    if parsed
        .api_key
        .as_ref()
        .is_some_and(|k| !k.trim().is_empty())
    {
        return true;
    }
    parsed
        .provider_api_keys
        .values()
        .any(|k| !k.trim().is_empty())
}

pub async fn exchange_agent_oauth_code(
    code: &str,
    state: &str,
) -> Result<(PlatformSession, PlatformLoginCredentials)> {
    let code = code.trim();
    let state = state.trim();
    if code.is_empty() || state.is_empty() {
        return Err(anyhow!("missing_code_or_state"));
    }
    let base = control_plane_api_base();
    if base.is_empty() {
        return Err(anyhow!("pointer_not_configured"));
    }
    let secret = control_plane_oauth_client_secret();
    let url = format!("{base}/auth/oauth/exchange-code");
    let body = serde_json::json!({
        "code": code,
        "state": state,
        "client_secret": if secret.is_empty() { serde_json::Value::Null } else { serde_json::Value::String(secret) },
    });
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(EXCHANGE_TIMEOUT_SEC))
        .build()
        .context("build pointer http client")?;
    let resp = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .with_context(|| format!("POST {url}"))?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        let detail = parse_api_detail(&text).unwrap_or_else(|| format!("HTTP {status}"));
        if detail.contains("invalid_client_secret") {
            log::warn!(
                "cloud_agent_auth: exchange failed: {detail} — set POINTER_OAUTH_CLIENT_SECRET \
                 (pointer-server.toml [pointer] oauth_client_secret) to the same value as \
                 API env THIRD_PARTY_OAUTH_EXCHANGE_SECRET; this is not JWT_SECRET"
            );
        } else {
            log::warn!("cloud_agent_auth: exchange failed: {detail}");
        }
        return Err(anyhow!(detail));
    }
    let parsed: OAuthCodeExchangeResponse =
        serde_json::from_str(&text).context("parse oauth exchange response")?;
    if parsed.access_token.trim().is_empty() {
        return Err(anyhow!("exchange_missing_access_token"));
    }
    if !exchange_has_usable_llm_key(&parsed) {
        return Err(anyhow!("exchange_missing_api_key"));
    }
    let expires_at = Utc::now().timestamp() + parsed.expires_in.max(60) as i64;
    let session = PlatformSession {
        access_token: parsed.access_token,
        refresh_token: String::new(),
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
        media_oss: parsed.media_oss,
    };
    log::info!(
        "cloud_agent_auth: exchange ok agent_id={} user={}",
        session.agent_id,
        session.user.id
    );
    Ok((session, creds))
}

fn parse_api_detail(text: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    if let Some(d) = v.get("detail") {
        if let Some(s) = d.as_str() {
            return Some(s.to_string());
        }
        return Some(d.to_string());
    }
    None
}

#[derive(Debug, Deserialize)]
struct OAuthCodeExchangeResponse {
    access_token: String,
    expires_in: u64,
    agent_id: String,
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
    platform_providers: Vec<crate::platform_auth::PlatformProviderTemplate>,
    #[serde(default, rename = "tierDefaults", alias = "tier_defaults")]
    tier_defaults: serde_json::Value,
    #[serde(default, rename = "mediaOss", alias = "media_oss")]
    media_oss: Option<crate::platform_auth::PlatformMediaOssCredentials>,
    user: OAuthExchangeUser,
}

#[derive(Debug, Deserialize)]
struct OAuthExchangeUser {
    id: String,
    nickname: Option<String>,
    #[serde(default, rename = "isPlatformAdmin", alias = "is_platform_admin")]
    is_platform_admin: bool,
    #[serde(default, rename = "includedTokens", alias = "included_tokens")]
    included_tokens: u64,
    #[serde(default, rename = "consumedTokens", alias = "consumed_tokens")]
    consumed_tokens: u64,
    #[serde(
        default,
        rename = "tokenQuotaExhausted",
        alias = "token_quota_exhausted"
    )]
    token_quota_exhausted: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exchange_requires_usable_key() {
        let parsed = OAuthCodeExchangeResponse {
            access_token: "t".into(),
            expires_in: 3600,
            agent_id: "a".into(),
            api_key: None,
            llm_provider: None,
            provider_api_keys: HashMap::new(),
            model_catalog: HashMap::new(),
            platform_providers: Vec::new(),
            tier_defaults: serde_json::Value::Null,
            media_oss: None,
            user: OAuthExchangeUser {
                id: "u".into(),
                nickname: None,
                is_platform_admin: false,
                included_tokens: 0,
                consumed_tokens: 0,
                token_quota_exhausted: false,
            },
        };
        assert!(!exchange_has_usable_llm_key(&parsed));
    }
}
