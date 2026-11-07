//! 登录/刷新凭证与 OSS 凭据。

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::provider::PlatformProviderTemplate;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentials_deserialize_from_control_plane_shape() {
        let raw = r#"{
            "provider_api_keys": {"qwen": "sk-1"},
            "modelCatalog": {"qwen": ["qwen3.5-plus"]},
            "platformProviders": [{"id": "qwen", "name": "阿里云百炼"}],
            "tierDefaults": {"agentModeLlm": {}},
            "modelCatalogHash": "abc123",
            "mediaOss": {"endpoint": "oss-cn-hangzhou.aliyuncs.com"}
        }"#;
        let creds: PlatformLoginCredentials = serde_json::from_str(raw).expect("parse");
        assert_eq!(creds.provider_api_keys.get("qwen").map(String::as_str), Some("sk-1"));
        assert_eq!(creds.model_catalog["qwen"], vec!["qwen3.5-plus"]);
        assert_eq!(creds.platform_providers.len(), 1);
        assert_eq!(creds.model_catalog_hash.as_deref(), Some("abc123"));
        assert!(creds.media_oss.is_some());
        assert!(credentials_have_llm_keys(&creds));
    }

    #[test]
    fn snake_case_aliases_still_parse() {
        let raw = r#"{
            "provider_api_keys": {"qwen": "sk-2"},
            "model_catalog": {"qwen": ["m"]},
            "platform_providers": [],
            "tier_defaults": null,
            "model_catalog_hash": "h"
        }"#;
        let creds: PlatformLoginCredentials = serde_json::from_str(raw).expect("parse");
        assert_eq!(creds.provider_api_keys.len(), 1);
        assert_eq!(creds.model_catalog_hash.as_deref(), Some("h"));
    }

    #[test]
    fn empty_credentials_have_no_keys() {
        let creds = PlatformLoginCredentials::default();
        assert!(!credentials_have_llm_keys(&creds));
        assert!(creds.model_catalog.is_empty());
        assert!(creds.platform_providers.is_empty());
    }

    #[test]
    fn blank_key_is_not_counted() {
        let mut creds = PlatformLoginCredentials::default();
        creds.provider_api_keys.insert("qwen".into(), "   ".into());
        assert!(!credentials_have_llm_keys(&creds));
        creds.api_key = Some("sk-real".into());
        assert!(credentials_have_llm_keys(&creds));
    }
}
