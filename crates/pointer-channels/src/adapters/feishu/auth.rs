use anyhow::Result;
use serde_json::json;

use crate::http_client::HttpClient;
use crate::token_cache::FEISHU_EARLY_REFRESH_SECS;

const TENANT_TOKEN_URL: &str =
    "https://open.feishu.cn/open-apis/auth/v3/tenant_access_token/internal";

pub async fn tenant_access_token(
    http: &HttpClient,
    app_id: &str,
    app_secret: &str,
) -> Result<String> {
    let cache_key = feishu_token_cache_key(app_id);
    if let Some(t) = http.get_cached_token(&cache_key) {
        return Ok(t);
    }
    fetch_tenant_access_token(http, app_id, app_secret).await
}

pub fn feishu_token_cache_key(app_id: &str) -> String {
    format!("feishu:{app_id}")
}

pub fn invalidate_tenant_access_token(http: &HttpClient, app_id: &str) {
    http.invalidate_cached_token(&feishu_token_cache_key(app_id));
}

async fn fetch_tenant_access_token(
    http: &HttpClient,
    app_id: &str,
    app_secret: &str,
) -> Result<String> {
    let cache_key = feishu_token_cache_key(app_id);
    let body = json!({ "app_id": app_id, "app_secret": app_secret });
    let resp = http.post_json(TENANT_TOKEN_URL, &[], &body).await?;
    let code = resp.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
    if code != 0 {
        let msg = resp
            .get("msg")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        return Err(anyhow::anyhow!("feishu tenant_access_token failed code={code}: {msg}"));
    }
    let token = resp
        .get("tenant_access_token")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("missing tenant_access_token"))?
        .to_string();
    let expire = resp
        .get("expire")
        .and_then(|v| v.as_u64())
        .unwrap_or(7200);
    http.set_cached_token_with_early_refresh(
        &cache_key,
        token.clone(),
        expire,
        FEISHU_EARLY_REFRESH_SECS,
    );
    Ok(token)
}

pub fn auth_header(token: &str) -> String {
    format!("Bearer {token}")
}

pub fn normalize_feishu_key(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

pub fn guess_mime_from_name(file_name: &str) -> String {
    let lower = file_name.to_ascii_lowercase();
    if lower.ends_with(".png") {
        return "image/png".into();
    }
    if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        return "image/jpeg".into();
    }
    if lower.ends_with(".gif") {
        return "image/gif".into();
    }
    if lower.ends_with(".webp") {
        return "image/webp".into();
    }
    if lower.ends_with(".pdf") {
        return "application/pdf".into();
    }
    if lower.ends_with(".mp4") || lower.ends_with(".mov") {
        return "video/mp4".into();
    }
    if lower.ends_with(".mp3") {
        return "audio/mpeg".into();
    }
    if lower.ends_with(".ogg") || lower.ends_with(".opus") {
        return "audio/ogg".into();
    }
    if lower.ends_with(".wav") {
        return "audio/wav".into();
    }
    if lower.ends_with(".txt") {
        return "text/plain".into();
    }
    if lower.ends_with(".md") {
        return "text/markdown".into();
    }
    if lower.ends_with(".doc") {
        return "application/msword".into();
    }
    if lower.ends_with(".docx") {
        return "application/vnd.openxmlformats-officedocument.wordprocessingml.document".into();
    }
    if lower.ends_with(".xls") {
        return "application/vnd.ms-excel".into();
    }
    if lower.ends_with(".xlsx") {
        return "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet".into();
    }
    if lower.ends_with(".ppt") {
        return "application/vnd.ms-powerpoint".into();
    }
    if lower.ends_with(".pptx") {
        return "application/vnd.openxmlformats-officedocument.presentationml.presentation".into();
    }
    "application/octet-stream".into()
}

/// Feishu `im/v1/files` upload `file_type` (see open platform docs).
pub fn feishu_upload_file_type(mime_type: &str, file_name: &str) -> &'static str {
    let lower = file_name.trim().to_ascii_lowercase();
    if mime_type.starts_with("video/") || lower.ends_with(".mp4") || lower.ends_with(".mov") {
        return "mp4";
    }
    if mime_type.starts_with("audio/") {
        return "opus";
    }
    if mime_type == "application/pdf" || lower.ends_with(".pdf") {
        return "pdf";
    }
    if lower.ends_with(".doc") {
        return "doc";
    }
    if lower.ends_with(".xls") || lower.ends_with(".xlsx") {
        return "xls";
    }
    if lower.ends_with(".ppt") || lower.ends_with(".pptx") {
        return "ppt";
    }
    "stream"
}

pub fn kind_from_mime(mime: &str, fallback_kind: &str) -> String {
    let m = mime.trim().to_ascii_lowercase();
    if m.starts_with("image/") {
        return "image".into();
    }
    if m.starts_with("audio/") {
        return "audio".into();
    }
    if m.starts_with("video/") {
        return "video".into();
    }
    if m.starts_with("text/") || m == "application/pdf" || m == "application/json" {
        return "document".into();
    }
    if m == "application/msword"
        || m == "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        || m == "application/vnd.ms-excel"
        || m == "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
        || m == "application/vnd.ms-powerpoint"
        || m == "application/vnd.openxmlformats-officedocument.presentationml.presentation"
    {
        return "document".into();
    }
    fallback_kind.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feishu_upload_file_type_for_html() {
        assert_eq!(
            feishu_upload_file_type("text/html", "minesweeper.html"),
            "stream"
        );
    }

    #[test]
    fn feishu_upload_file_type_for_pdf() {
        assert_eq!(
            feishu_upload_file_type("application/pdf", "report.pdf"),
            "pdf"
        );
    }
}
