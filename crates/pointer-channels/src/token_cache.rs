//! Access-token cache margins aligned with vendor docs.
//!
//! - Feishu SDK: refresh ~3 minutes before `expire`.
//! - DingTalk: cache slightly below 7200s (doc suggests ~7000s).
//! - WeCom: `expires_in` is 7200s; re-fetch on 40014 / 42001.

/// Feishu server SDK invalidates tenant tokens 3 minutes early.
pub const FEISHU_EARLY_REFRESH_SECS: u64 = 180;

/// DingTalk doc recommends caching below 7200s (e.g. 7000s).
pub const DINGTALK_EARLY_REFRESH_SECS: u64 = 200;

/// WeCom `expires_in` is normally 7200s; refresh with a safety margin.
pub const WECOM_EARLY_REFRESH_SECS: u64 = 180;

pub fn is_feishu_invalid_token_error(err: &anyhow::Error) -> bool {
    let msg = format!("{err:#}");
    msg.contains("99991663") || msg.contains("Invalid access token")
}

/// DingTalk oapi errcode 40001 / 40014 / 42001, or OpenAPI `accesstoken.expired`.
pub fn is_dingtalk_invalid_token_error(err: &anyhow::Error) -> bool {
    let msg = format!("{err:#}");
    msg.contains("(errcode=40001)")
        || msg.contains("(errcode=40014)")
        || msg.contains("(errcode=42001)")
        || msg.contains("accesstoken.expired")
        || msg.contains("不合法的access_token")
        || msg.contains("access_token超时")
        || msg.contains("invalid access_token")
}

/// WeCom global errcode 40014 / 42001.
pub fn is_wecom_invalid_token_error(err: &anyhow::Error) -> bool {
    let msg = format!("{err:#}");
    msg.contains("(errcode=40014)")
        || msg.contains("(errcode=42001)")
        || msg.contains("不合法的access_token")
        || msg.contains("access_token已过期")
        || msg.contains("access_token超时")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_feishu_token_error() {
        let err = anyhow::anyhow!("POST failed 400: code 99991663 Invalid access token");
        assert!(is_feishu_invalid_token_error(&err));
    }

    #[test]
    fn detects_dingtalk_oapi_token_errors() {
        let err = anyhow::anyhow!("dingtalk oapi media/upload failed: timeout (errcode=42001)");
        assert!(is_dingtalk_invalid_token_error(&err));
    }

    #[test]
    fn detects_wecom_token_errors() {
        let err = anyhow::anyhow!("wecom message/send failed: expired (errcode=42001)");
        assert!(is_wecom_invalid_token_error(&err));
    }
}
