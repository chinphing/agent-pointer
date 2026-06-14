use anyhow::Result;
use serde_json::json;
use serde_json::Value;

use crate::http_client::HttpClient;
use crate::token_cache::DINGTALK_EARLY_REFRESH_SECS;

const ACCESS_TOKEN_URL: &str = "https://api.dingtalk.com/v1.0/oauth2/accessToken";

pub fn dingtalk_token_cache_key(client_id: &str) -> String {
    format!("dingtalk:{client_id}")
}

pub fn invalidate_access_token(http: &HttpClient, client_id: &str) {
    http.invalidate_cached_token(&dingtalk_token_cache_key(client_id));
}

pub async fn access_token(
    http: &HttpClient,
    client_id: &str,
    client_secret: &str,
) -> Result<String> {
    let cache_key = dingtalk_token_cache_key(client_id);
    if let Some(t) = http.get_cached_token(&cache_key) {
        return Ok(t);
    }
    fetch_access_token(http, client_id, client_secret).await
}

async fn fetch_access_token(
    http: &HttpClient,
    client_id: &str,
    client_secret: &str,
) -> Result<String> {
    let cache_key = dingtalk_token_cache_key(client_id);
    let body = json!({
        "appKey": client_id,
        "appSecret": client_secret,
    });
    let resp = http.post_json(ACCESS_TOKEN_URL, &[], &body).await?;
    let token = resp
        .get("accessToken")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("dingtalk missing accessToken"))?
        .to_string();
    let expire = resp
        .get("expireIn")
        .and_then(|v| v.as_u64())
        .unwrap_or(7200);
    http.set_cached_token_with_early_refresh(
        &cache_key,
        token.clone(),
        expire,
        DINGTALK_EARLY_REFRESH_SECS,
    );
    Ok(token)
}

/// OpenAPI may return `{ code: "accesstoken.expired" }` with HTTP 400.
pub fn check_openapi_token_response(resp: &Value, step: &str) -> Result<()> {
    if let Some(code) = resp.get("code").and_then(|v| v.as_str()) {
        if code.eq_ignore_ascii_case("accesstoken.expired") {
            anyhow::bail!("dingtalk openapi {step} failed: accesstoken.expired (code={code})");
        }
    }
    Ok(())
}
