//! In-memory platform configuration and defaults.

use parking_lot::RwLock;
use std::sync::{Arc, OnceLock};

use crate::models::{
    ensure_agent_model_refs_have_provider, ensure_provider_generation_defaults, merge_user_platform,
    ModelSettings, PlatformSettings, ProviderConfig, UserSettings,
};
use crate::storage;

static GLOBAL_PLATFORM_CONFIG: OnceLock<SharedPlatformConfig> = OnceLock::new();

/// Register process-wide platform config for code paths without [`AppState`] (tools, etc.).
pub fn register_global_platform_config(cfg: SharedPlatformConfig) {
    let _ = GLOBAL_PLATFORM_CONFIG.set(cfg);
}

/// Effective settings when only user persistence + optional global platform config exist.
pub fn effective_settings_global() -> ModelSettings {
    let user = storage::load_user_settings().unwrap_or_default();
    let platform = GLOBAL_PLATFORM_CONFIG
        .get()
        .map(|p| p.read().clone())
        .unwrap_or_else(PlatformSettings::default);
    finalize_merged_settings(merge_user_platform(&user, &platform))
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
    settings
}

/// Inject OAuth-issued LLM credentials into platform provider list.
pub fn apply_login_llm_credentials(
    platform: &mut PlatformSettings,
    api_key: Option<&str>,
    llm_provider: Option<&str>,
) {
    let key = api_key.map(str::trim).filter(|s| !s.is_empty());
    let Some(key) = key else {
        log::info!("platform_config: login token has no api_key; providers unchanged");
        return;
    };
    let provider_id = resolve_llm_provider_id(llm_provider, &platform.providers);
    let Some(pid) = provider_id else {
        log::warn!(
            "platform_config: no provider match for llm_provider={:?}",
            llm_provider
        );
        return;
    };
    for p in &mut platform.providers {
        if p.id == pid {
            p.api_key = key.to_string();
            log::info!("platform_config: injected api_key for provider {pid}");
            return;
        }
    }
    log::warn!("platform_config: provider id {pid} not found in platform config");
}

fn resolve_llm_provider_id(llm_provider: Option<&str>, providers: &[ProviderConfig]) -> Option<String> {
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

    #[test]
    fn apply_login_maps_aliyun_qwen() {
        let mut platform = PlatformSettings::default();
        apply_login_llm_credentials(&mut platform, Some("sk-test"), Some("aliyun_qwen"));
        let qwen = platform.providers.iter().find(|p| p.id == "qwen").unwrap();
        assert_eq!(qwen.api_key, "sk-test");
    }
}
