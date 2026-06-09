use anyhow::Result;
use serde_json::json;

use crate::http_client::HttpClient;

const ACCESS_TOKEN_URL: &str = "https://api.dingtalk.com/v1.0/oauth2/accessToken";

pub async fn access_token(
    http: &HttpClient,
    client_id: &str,
    client_secret: &str,
) -> Result<String> {
    let cache_key = format!("dingtalk:{client_id}");
    if let Some(t) = http.get_cached_token(&cache_key) {
        return Ok(t);
    }
    let body = json!({
        "appKey": client_id,
        "appSecret": client_secret,
    });
    let resp = http.post_json(ACCESS_TOKEN_URL, &[], &body).await?;
    let token = resp
        .get("accessToken")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("dingtalk missing accessToken"))?
        .to_string();
    let expire = resp
        .get("expireIn")
        .and_then(|v| v.as_u64())
        .unwrap_or(7200);
    http.set_cached_token(&cache_key, token.clone(), expire);
    Ok(token)
}
