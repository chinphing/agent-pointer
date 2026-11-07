//! 登录会话与用户摘要。

use serde::{Deserialize, Serialize};

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_roundtrips_with_camel_case() {
        let session = PlatformSession {
            access_token: "at".into(),
            refresh_token: "rt".into(),
            expires_at: 1_700_000_000,
            agent_id: "agent-1".into(),
            user: PlatformUserSummary {
                id: "u1".into(),
                nickname: Some("王国宏".into()),
                is_platform_admin: true,
                included_tokens: 10,
                consumed_tokens: 3,
                token_quota_exhausted: false,
            },
        };
        let json = serde_json::to_string(&session).expect("serialize");
        assert!(json.contains("\"isPlatformAdmin\":true"));
        assert!(json.contains("\"tokenQuotaExhausted\":false"));

        let back: PlatformSession = serde_json::from_str(&json).expect("parse");
        assert_eq!(back.user.id, "u1");
        assert!(back.user.is_platform_admin);
    }

    #[test]
    fn missing_optional_fields_default() {
        let raw = r#"{"id":"u1","nickname":null}"#;
        let user: PlatformUserSummary = serde_json::from_str(raw).expect("parse");
        assert!(!user.is_platform_admin);
        assert_eq!(user.included_tokens, 0);
    }
}
