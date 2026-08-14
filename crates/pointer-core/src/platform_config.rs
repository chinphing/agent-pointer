//! In-memory platform configuration and defaults.

use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use crate::models::{
    ensure_agent_model_refs_have_provider, ensure_provider_generation_defaults,
    ensure_provider_model_capability_defaults, merge_user_platform, MediaOssConfig, ModelSettings,
    PlatformSettings, ProviderConfig, UserSettings,
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
    apply_login_llm_provider_api_keys(&mut settings.providers, &creds.provider_api_keys);
    apply_login_llm_credentials(
        &mut settings.providers,
        creds.api_key.as_deref(),
        creds.llm_provider.as_deref(),
    );
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
pub fn apply_login_media_oss(media_oss: &mut MediaOssConfig, media: Option<&PlatformMediaOssCredentials>) {
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

fn resolve_llm_provider_id(
    llm_provider: Option<&str>,
    providers: &[ProviderConfig],
) -> Option<String> {
    if let Some(raw) = llm_provider.map(str::trim).filter(|s| !s.is_empty()) {
        let lower = raw.to_ascii_lowercase();
        if providers.iter().any(|p| p.id == lower) {
            return Some(lower);
        }
        if lower == "aliyun_qwen" || lower == "qwen" {
            return Some("qwen".into());
        }
        if lower.contains("deepseek") {
            return Some("deepseek".into());
        }
        for p in providers {
            if p.name.to_ascii_lowercase().contains(&lower) || lower.contains(&p.id) {
                return Some(p.id.clone());
            }
        }
    }
    providers.first().map(|p| p.id.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ComputerTierLlmConfig, UserSettings};

    #[test]
    fn apply_login_maps_aliyun_qwen() {
        let mut platform = PlatformSettings::default();
        apply_login_llm_credentials(&mut platform.providers, Some("sk-test"), Some("aliyun_qwen"));
        let qwen = platform.providers.iter().find(|p| p.id == "qwen").unwrap();
        assert_eq!(qwen.api_key, "sk-test");
    }

    #[test]
    fn apply_login_provider_api_keys_maps_aliyun_qwen_alias() {
        let mut platform = PlatformSettings::default();
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
    fn default_agent_model_is_deepseek_flash() {
        let user = UserSettings::default();
        assert_eq!(
            user.agent_default_models
                .get("general")
                .map(|r| r.model.as_str()),
            Some("deepseek-v4-flash")
        );
    }
}
