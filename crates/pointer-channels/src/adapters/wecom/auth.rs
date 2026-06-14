use anyhow::Result;
use serde_json::Value;

use crate::http_client::HttpClient;
use crate::token_cache::WECOM_EARLY_REFRESH_SECS;

pub fn wecom_token_cache_key(corp_id: &str) -> String {
    format!("wecom:{corp_id}")
}

pub fn invalidate_access_token(http: &HttpClient, corp_id: &str) {
    http.invalidate_cached_token(&wecom_token_cache_key(corp_id));
}

pub async fn access_token(http: &HttpClient, corp_id: &str, secret: &str) -> Result<String> {
    let cache_key = wecom_token_cache_key(corp_id);
    if let Some(t) = http.get_cached_token(&cache_key) {
        return Ok(t);
    }
    fetch_access_token(http, corp_id, secret).await
}

async fn fetch_access_token(http: &HttpClient, corp_id: &str, secret: &str) -> Result<String> {
    let cache_key = wecom_token_cache_key(corp_id);
    let url = format!(
        "https://qyapi.weixin.qq.com/cgi-bin/gettoken?corpid={corp_id}&corpsecret={secret}"
    );
    let resp = http.get_json(&url, &[]).await?;
    check_wecom_api_response(&resp, "gettoken")?;
    let token = resp
        .get("access_token")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("wecom missing access_token"))?
        .to_string();
    let expire = resp
        .get("expires_in")
        .and_then(|v| v.as_u64())
        .unwrap_or(7200);
    http.set_cached_token_with_early_refresh(
        &cache_key,
        token.clone(),
        expire,
        WECOM_EARLY_REFRESH_SECS,
    );
    Ok(token)
}

/// WeCom business APIs return HTTP 200 with `{ errcode, errmsg }`.
pub fn check_wecom_api_response(resp: &Value, step: &str) -> Result<()> {
    let errcode = resp.get("errcode").and_then(|v| v.as_i64()).unwrap_or(0);
    if errcode == 0 {
        return Ok(());
    }
    let errmsg = resp
        .get("errmsg")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown error");
    anyhow::bail!("wecom {step} failed: {errmsg} (errcode={errcode})");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::token_cache::is_wecom_invalid_token_error;

    #[test]
    fn check_wecom_api_response_ok() {
        let v = serde_json::json!({"errcode": 0, "errmsg": "ok"});
        assert!(check_wecom_api_response(&v, "test").is_ok());
    }

    #[test]
    fn check_wecom_api_response_token_expired() {
        let v = serde_json::json!({"errcode": 42001, "errmsg": "access_token expired"});
        let err = check_wecom_api_response(&v, "send").unwrap_err();
        assert!(is_wecom_invalid_token_error(&err));
    }
}
