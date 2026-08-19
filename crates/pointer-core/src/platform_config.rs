//! In-memory platform configuration and defaults.

use parking_lot::RwLock;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, OnceLock};

use crate::models::{
    apply_platform_tier_defaults, ensure_agent_model_refs_have_provider,
    ensure_provider_generation_defaults, ensure_provider_model_capability_defaults,
    merge_user_platform, MediaOssConfig, ModelRuntimeOverrides, ModelSettings, PlatformSettings,
    ProviderConfig, UserSettings,
};
use crate::platform_auth::PlatformMediaOssCredentials;
use crate::storage;

pub const DEFAULT_PLATFORM_MEDIA_OSS_BUCKET: &str = "pointer-app-media";
pub const DEFAULT_PLATFORM_MEDIA_OSS_REGION: &str = "cn-hangzhou";

static GLOBAL_PLATFORM_CONFIG: OnceLock<SharedPlatformConfig> = OnceLock::new();

/// Register process-wide platform config for code paths without [`AppState`] (tools, etc.).
pub fn register_global_platform_config(cfg: SharedPlatformConfig) {
    let _ = GLOBAL_PLATFORM_CONFIG.set(cfg);
}

/// Test helper: replace the process-wide platform config (OnceLock may already be set).
#[cfg(test)]
pub fn replace_global_platform_config_for_test(platform: PlatformSettings) {
    if let Some(cfg) = GLOBAL_PLATFORM_CONFIG.get() {
        *cfg.write() = platform;
        return;
    }
    let _ = GLOBAL_PLATFORM_CONFIG.set(Arc::new(RwLock::new(platform)));
}

/// Effective settings when only user persistence + optional global platform config exist.
pub fn effective_settings_global() -> ModelSettings {
    let user = test_user_settings_or_default();
    let platform = GLOBAL_PLATFORM_CONFIG
        .get()
        .map(|p| p.read().clone())
        .unwrap_or_else(PlatformSettings::default);
    finalize_merged_settings(merge_user_platform(&user, &platform))
}

#[cfg(test)]
static GLOBAL_USER_SETTINGS_FOR_TEST: OnceLock<RwLock<UserSettings>> = OnceLock::new();

/// Serialize tests that mutate process-global user settings / platform config
/// (terminalEnvOverrides etc.). Tests that write these globals must hold this
/// lock across their whole read+assert window so concurrent tests cannot swap
/// the value in between.
#[cfg(test)]
pub(crate) fn settings_test_lock() -> std::sync::MutexGuard<'static, ()> {
    static SETTINGS_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    SETTINGS_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// Test helper: inject user settings for `effective_settings_global()`.
#[cfg(test)]
pub fn replace_global_user_settings_for_test(user: UserSettings) {
    if let Some(g) = GLOBAL_USER_SETTINGS_FOR_TEST.get() {
        *g.write() = user;
        return;
    }
    let _ = GLOBAL_USER_SETTINGS_FOR_TEST.set(RwLock::new(user));
}

#[cfg(test)]
fn test_user_settings_or_default() -> UserSettings {
    GLOBAL_USER_SETTINGS_FOR_TEST
        .get()
        .map(|g| g.read().clone())
        .unwrap_or_else(|| storage::load_user_settings().unwrap_or_default())
}

#[cfg(not(test))]
fn test_user_settings_or_default() -> UserSettings {
    storage::load_user_settings().unwrap_or_default()
}

pub type SharedPlatformConfig = Arc<RwLock<PlatformSettings>>;

pub struct PlatformConfigManager {
    inner: SharedPlatformConfig,
}

impl PlatformConfigManager {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(PlatformSettings::default())),
        }
    }

    pub fn shared(&self) -> SharedPlatformConfig {
        self.inner.clone()
    }

    pub fn read(&self) -> parking_lot::RwLockReadGuard<'_, PlatformSettings> {
        self.inner.read()
    }

    pub fn write(&self) -> parking_lot::RwLockWriteGuard<'_, PlatformSettings> {
        self.inner.write()
    }

    pub fn replace(&self, settings: PlatformSettings) {
        *self.inner.write() = settings;
    }

    pub fn effective_model_settings(&self, user: &UserSettings) -> ModelSettings {
        let platform = self.inner.read().clone();
        finalize_merged_settings(merge_user_platform(user, &platform))
    }
}

impl Default for PlatformConfigManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Attach active-provider api key + has_key to merged runtime settings.
pub fn finalize_merged_settings(mut settings: ModelSettings) -> ModelSettings {
    if let Some(p) = settings
        .providers
        .iter()
        .find(|p| p.id == settings.active_provider_id)
        .or_else(|| settings.providers.first())
    {
        settings.api_key = p.api_key.clone();
        settings.has_key = !p.api_key.is_empty();
    } else {
        settings.api_key.clear();
        settings.has_key = false;
    }
    ensure_agent_model_refs_have_provider(&mut settings);
    ensure_provider_generation_defaults(&mut settings);
    ensure_provider_model_capability_defaults(&mut settings);
    settings
}

pub fn apply_login_credentials_to_model_settings(
    settings: &mut ModelSettings,
    creds: &crate::platform_auth::PlatformLoginCredentials,
) {
    apply_login_platform_providers(&mut settings.providers, &creds.platform_providers);
    apply_login_llm_provider_api_keys(&mut settings.providers, &creds.provider_api_keys);
    apply_login_llm_credentials(
        &mut settings.providers,
        creds.api_key.as_deref(),
        creds.llm_provider.as_deref(),
    );
    if !creds.tier_defaults.is_null() {
        let providers = settings.providers.clone();
        apply_platform_tier_defaults(settings, &creds.tier_defaults, &providers);
    }
    if let Some(media) = creds.media_oss.as_ref() {
        apply_login_media_oss(&mut settings.media_oss, Some(media));
    }
    if let Some(p) = settings
        .providers
        .iter()
        .find(|p| p.id == settings.active_provider_id)
    {
        settings.api_key = p.api_key.clone();
        settings.has_key = !p.api_key.trim().is_empty();
    }
}

/// Inject OAuth-issued LLM credentials into provider list.
pub fn apply_login_llm_credentials(
    providers: &mut Vec<ProviderConfig>,
    api_key: Option<&str>,
    llm_provider: Option<&str>,
) {
    let key = api_key.map(str::trim).filter(|s| !s.is_empty());
    let Some(key) = key else {
        log::info!("platform_config: login token has no api_key; providers unchanged");
        return;
    };
    let provider_id = resolve_llm_provider_id(llm_provider, providers);
    let Some(pid) = provider_id else {
        log::warn!(
            "platform_config: no provider match for llm_provider={:?}",
            llm_provider
        );
        return;
    };
    for p in providers.iter_mut() {
        if p.id == pid {
            p.api_key = key.to_string();
            log::debug!("platform_config: injected api_key for provider {pid}");
            return;
        }
    }
    log::warn!("platform_config: provider id {pid} not found in platform config");
}

/// 按平台下发的服务商完整模板创建/更新运行时 provider（客户端不再内置任何平台服务商）。
///
/// 非空目录是完整清单：会移除已不在目录中的 `source=platform` 服务商，
/// 以便平台下线服务商后客户端立即同步。空目录不改动现有列表。
pub fn apply_login_platform_providers(
    providers: &mut Vec<ProviderConfig>,
    platform_providers: &[crate::platform_auth::PlatformProviderTemplate],
) {
    if platform_providers.is_empty() {
        return;
    }
    let incoming_ids: HashSet<String> = platform_providers
        .iter()
        .map(|tpl| tpl.id.trim().to_string())
        .filter(|id| !id.is_empty())
        .collect();
    for tpl in platform_providers {
        let id = tpl.id.trim();
        if id.is_empty() {
            log::warn!("platform_config: skip platform provider without id");
            continue;
        }
        let name = tpl.name.trim().to_string();
        let base_url = tpl.base_url.trim().to_string();
        if name.is_empty() || base_url.is_empty() {
            log::warn!("platform_config: skip platform provider {id}: missing name/baseUrl");
            continue;
        }
        let models: Vec<String> = tpl
            .models
            .iter()
            .map(|m| m.name.trim().to_string())
            .filter(|m| !m.is_empty())
            .collect();
        if models.is_empty() {
            log::warn!("platform_config: skip platform provider {id}: no models");
            continue;
        }
        let model_configs: HashMap<String, ModelRuntimeOverrides> = tpl
            .models
            .iter()
            .map(|m| {
                (
                    m.name.trim().to_string(),
                    ModelRuntimeOverrides {
                        reasoning_in_messages: m.reasoning_in_messages,
                        temperature: m.temperature,
                        max_tokens: m.max_tokens,
                        enable_thinking: m.enable_thinking,
                        thinking_budget: m.thinking_budget,
                        reasoning_effort: m.reasoning_effort.clone(),
                        thinking_protocol: None,
                        thinking_intensity: m.thinking_intensity.clone(),
                        supports_vision: m.supports_vision,
                        can_generate_image: m.can_generate_image,
                        can_generate_video: m.can_generate_video,
                        ..Default::default()
                    },
                )
            })
            .filter(|(model_name, _)| !model_name.is_empty())
            .collect();
        if let Some(p) = providers.iter_mut().find(|p| p.id == id) {
            p.name = name;
            p.base_url = base_url;
            p.models = models;
            p.model_configs = model_configs;
            p.reasoning_in_messages = tpl.reasoning_in_messages;
            p.enable_thinking = tpl.enable_thinking;
            p.thinking_budget = tpl.thinking_budget;
            p.reasoning_effort = tpl.reasoning_effort.clone();
            p.thinking_protocol = None;
            p.thinking_intensity = tpl.thinking_intensity.clone();
            if tpl.temperature.is_some() {
                p.temperature = tpl.temperature;
            }
            if tpl.max_tokens.is_some() {
                p.max_tokens = tpl.max_tokens;
            }
            p.source = Some("platform".into());
        } else {
            providers.push(ProviderConfig {
                id: id.to_string(),
                name,
                base_url,
                api_key: String::new(),
                models,
                reasoning_in_messages: tpl.reasoning_in_messages,
                temperature: tpl.temperature,
                max_tokens: tpl.max_tokens,
                model_configs,
                enable_thinking: tpl.enable_thinking,
                thinking_budget: tpl.thinking_budget,
                reasoning_effort: tpl.reasoning_effort.clone(),
                thinking_protocol: None,
                thinking_intensity: tpl.thinking_intensity.clone(),
                extra_body: None,
                source: Some("platform".into()),
            });
        }
    }
    let before = providers.len();
    providers.retain(|p| p.source.as_deref() != Some("platform") || incoming_ids.contains(&p.id));
    if providers.len() != before {
        log::info!(
            "platform_config: dropped {} platform provider(s) no longer in directory",
            before - providers.len()
        );
    }
}

/// Inject per-provider OAuth-issued LLM credentials (provider id -> api key).
pub fn apply_login_llm_provider_api_keys(
    providers: &mut Vec<ProviderConfig>,
    provider_api_keys: &HashMap<String, String>,
) {
    if provider_api_keys.is_empty() {
        return;
    }
    for (raw_provider, raw_key) in provider_api_keys {
        let key = raw_key.trim();
        if key.is_empty() {
            continue;
        }
        let Some(pid) = resolve_llm_provider_id(Some(raw_provider.as_str()), providers) else {
            log::warn!("platform_config: skip unknown provider {raw_provider}");
            continue;
        };
        if let Some(p) = providers.iter_mut().find(|p| p.id == pid) {
            p.api_key = key.to_string();
            log::debug!("platform_config: injected api_key for provider {pid}");
        } else {
            log::warn!("platform_config: provider id {pid} not found in platform config");
        }
    }
}

fn parse_oss_region_from_endpoint(endpoint: &str) -> Option<String> {
    let lower = endpoint.trim().to_ascii_lowercase();
    let needle = "oss-";
    let Some(start) = lower.find(needle) else {
        return None;
    };
    let rest = &lower[start + needle.len()..];
    let end = rest.find(".aliyuncs.com")?;
    let region = rest[..end].trim();
    if region.is_empty() {
        None
    } else {
        Some(region.to_string())
    }
}

/// `https://my-bucket.oss-cn-hangzhou.aliyuncs.com` → `my-bucket`
fn parse_oss_bucket_from_endpoint(endpoint: &str) -> Option<String> {
    let mut host = endpoint.trim();
    if let Some(rest) = host.strip_prefix("https://") {
        host = rest;
    } else if let Some(rest) = host.strip_prefix("http://") {
        host = rest;
    }
    let host = host.split('/').next().unwrap_or(host);
    let marker = ".oss-";
    let idx = host.find(marker)?;
    let bucket = host[..idx].trim();
    if bucket.is_empty() {
        None
    } else {
        Some(bucket.to_string())
    }
}

/// Inject OAuth-issued media OSS credentials into media_oss config.
pub fn apply_login_media_oss(
    media_oss: &mut MediaOssConfig,
    media: Option<&PlatformMediaOssCredentials>,
) {
    let Some(raw) = media else {
        *media_oss = MediaOssConfig::default();
        return;
    };
    let endpoint = raw.endpoint.trim();
    let access_key_id = raw.access_key_id.trim();
    let access_key_secret = raw.access_key_secret.trim();
    if endpoint.is_empty() || access_key_id.is_empty() || access_key_secret.is_empty() {
        *media_oss = MediaOssConfig::default();
        log::info!("platform_config: login token has no usable media_oss; cleared");
        return;
    }
    let region = parse_oss_region_from_endpoint(endpoint)
        .unwrap_or_else(|| DEFAULT_PLATFORM_MEDIA_OSS_REGION.to_string());
    let bucket = parse_oss_bucket_from_endpoint(endpoint)
        .unwrap_or_else(|| DEFAULT_PLATFORM_MEDIA_OSS_BUCKET.to_string());
    *media_oss = MediaOssConfig {
        enabled: true,
        bucket,
        region,
        endpoint: endpoint.to_string(),
        access_key_id: access_key_id.to_string(),
        access_key_secret: access_key_secret.to_string(),
        ..MediaOssConfig::default()
    };
    log::debug!("platform_config: injected media_oss from platform login");
}

fn provider_id_in_list(providers: &[ProviderConfig], id: &str) -> Option<String> {
    providers
        .iter()
        .find(|p| p.id.eq_ignore_ascii_case(id))
        .map(|p| p.id.clone())
}

/// Map login `llm_provider` to a row that **exists** in the current provider list.
/// Historical aliases (`aliyun_qwen` → `qwen`) apply only when that id is present.
fn resolve_llm_provider_id(
    llm_provider: Option<&str>,
    providers: &[ProviderConfig],
) -> Option<String> {
    if let Some(raw) = llm_provider.map(str::trim).filter(|s| !s.is_empty()) {
        let lower = raw.to_ascii_lowercase();
        if let Some(id) = provider_id_in_list(providers, &lower) {
            return Some(id);
        }
        if lower == "aliyun_qwen" || lower == "aliyun" {
            if let Some(id) = provider_id_in_list(providers, "qwen") {
                return Some(id);
            }
        }
        if lower.contains("deepseek") {
            if let Some(id) = provider_id_in_list(providers, "deepseek") {
                return Some(id);
            }
        }
        for p in providers {
            if p.name.to_ascii_lowercase().contains(&lower) || lower.contains(&p.id) {
                return Some(p.id.clone());
            }
        }
        if let Some(first) = providers.first() {
            log::warn!(
                "platform_config: llm_provider={raw:?} not in provider list; using {}",
                first.id
            );
            return Some(first.id.clone());
        }
        log::warn!("platform_config: llm_provider={raw:?} but provider list is empty");
        return None;
    }
    providers.first().map(|p| p.id.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::UserSettings;

    fn qwen_template() -> crate::platform_auth::PlatformProviderTemplate {
        crate::platform_auth::PlatformProviderTemplate {
            id: "qwen".into(),
            name: "千问".into(),
            base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1".into(),
            models: vec![
                crate::platform_auth::PlatformProviderModel {
                    name: "qwen3.5-plus".into(),
                    ..Default::default()
                },
                crate::platform_auth::PlatformProviderModel {
                    name: "qwen-next".into(),
                    ..Default::default()
                },
            ],
            reasoning_in_messages: Some(false),
            ..Default::default()
        }
    }

    fn deepseek_template() -> crate::platform_auth::PlatformProviderTemplate {
        crate::platform_auth::PlatformProviderTemplate {
            id: "deepseek".into(),
            name: "深度求索".into(),
            base_url: "https://api.deepseek.com/v1".into(),
            models: vec![crate::platform_auth::PlatformProviderModel {
                name: "deepseek-v4-flash".into(),
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    #[test]
    fn apply_login_maps_aliyun_qwen() {
        let mut platform = PlatformSettings::default();
        let templates = vec![qwen_template()];
        apply_login_platform_providers(&mut platform.providers, &templates);
        apply_login_llm_credentials(
            &mut platform.providers,
            Some("sk-test"),
            Some("aliyun_qwen"),
        );
        let qwen = platform.providers.iter().find(|p| p.id == "qwen").unwrap();
        assert_eq!(qwen.api_key, "sk-test");
        assert_eq!(qwen.source.as_deref(), Some("platform"));
    }

    #[test]
    fn resolve_llm_provider_missing_qwen_alias_uses_first() {
        let mut providers: Vec<ProviderConfig> = Vec::new();
        let mut tpl = qwen_template();
        tpl.id = "new-platform-llm".into();
        tpl.name = "新平台服务".into();
        apply_login_platform_providers(&mut providers, &[tpl]);
        assert_eq!(
            resolve_llm_provider_id(Some("aliyun_qwen"), &providers).as_deref(),
            Some("new-platform-llm")
        );
    }

    #[test]
    fn apply_login_platform_providers_creates_missing_providers() {
        let mut providers: Vec<ProviderConfig> = Vec::new();
        let templates = vec![qwen_template(), deepseek_template()];
        apply_login_platform_providers(&mut providers, &templates);
        assert_eq!(providers.len(), 2);
        let qwen = providers.iter().find(|p| p.id == "qwen").unwrap();
        assert_eq!(qwen.name, "千问");
        assert_eq!(
            qwen.base_url,
            "https://dashscope.aliyuncs.com/compatible-mode/v1"
        );
        assert_eq!(qwen.models, vec!["qwen3.5-plus", "qwen-next"]);
        assert_eq!(qwen.source.as_deref(), Some("platform"));
    }

    #[test]
    fn apply_login_provider_api_keys_maps_aliyun_qwen_alias() {
        let mut platform = PlatformSettings::default();
        let templates = vec![qwen_template(), deepseek_template()];
        apply_login_platform_providers(&mut platform.providers, &templates);

        let mut keys = HashMap::new();
        keys.insert("aliyun_qwen".into(), "sk-qwen".into());
        keys.insert("deepseek".into(), "sk-ds".into());
        apply_login_llm_provider_api_keys(&mut platform.providers, &keys);
        assert_eq!(
            platform
                .providers
                .iter()
                .find(|p| p.id == "qwen")
                .unwrap()
                .api_key,
            "sk-qwen"
        );
        assert_eq!(
            platform
                .providers
                .iter()
                .find(|p| p.id == "deepseek")
                .unwrap()
                .api_key,
            "sk-ds"
        );
    }

    #[test]
    fn default_agent_model_is_empty_without_platform_defaults() {
        let user = UserSettings::default();
        assert!(!user.agent_default_models.contains_key("general"));
    }

    #[test]
    fn apply_login_platform_providers_drops_removed_platform_providers() {
        let mut providers: Vec<ProviderConfig> = Vec::new();
        apply_login_platform_providers(&mut providers, &[qwen_template(), deepseek_template()]);
        assert_eq!(providers.len(), 2);
        apply_login_platform_providers(&mut providers, &[qwen_template()]);
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].id, "qwen");
    }

    #[test]
    fn apply_login_platform_providers_keeps_user_providers() {
        let mut providers = vec![ProviderConfig {
            id: "custom-llm".into(),
            name: "Custom".into(),
            base_url: "https://custom.example/v1".into(),
            api_key: String::new(),
            models: vec!["local".into()],
            reasoning_in_messages: None,
            temperature: None,
            max_tokens: None,
            model_configs: HashMap::new(),
            enable_thinking: None,
            thinking_budget: None,
            reasoning_effort: None,
            thinking_protocol: None,
            thinking_intensity: None,
            extra_body: None,
            source: Some("user".into()),
        }];
        apply_login_platform_providers(&mut providers, &[qwen_template()]);
        assert!(providers.iter().any(|p| p.id == "custom-llm"));
        assert!(providers.iter().any(|p| p.id == "qwen"));
    }

    #[test]
    fn login_applies_platform_providers_and_ignores_model_catalog() {
        let mut settings = ModelSettings::default();
        let creds = crate::platform_auth::PlatformLoginCredentials {
            platform_providers: vec![qwen_template()],
            model_catalog: HashMap::from([("qwen".into(), vec!["qwen3.5-plus".into()])]),
            ..Default::default()
        };
        apply_login_credentials_to_model_settings(&mut settings, &creds);
        let qwen = settings.providers.iter().find(|p| p.id == "qwen").unwrap();
        assert_eq!(qwen.models, vec!["qwen3.5-plus", "qwen-next"]);
    }

    #[test]
    fn apply_login_credentials_to_model_settings_creates_providers_from_templates() {
        let mut settings = ModelSettings::default();
        let creds = crate::platform_auth::PlatformLoginCredentials {
            platform_providers: vec![qwen_template()],
            tier_defaults: serde_json::json!({
                "agentModeLlm": {
                    "general": {
                        "fast": { "providerId": "qwen", "model": "qwen-next" }
                    }
                }
            }),
            ..Default::default()
        };
        apply_login_credentials_to_model_settings(&mut settings, &creds);
        assert!(settings.providers.iter().any(|p| p.id == "qwen"));
        let fast = settings
            .agent_mode_llm
            .get("general")
            .and_then(|m| m.get("fast"))
            .expect("platform tier default should be applied");
        assert_eq!(fast.model, "qwen-next");
    }
}
