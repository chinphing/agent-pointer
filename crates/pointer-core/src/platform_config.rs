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

/// Apply UI-edited preferences to the persisted user layer (user_settings.json).
/// Providers come in as part of the merged snapshot; user-typed keys are kept
/// (masked/empty falls back to the previously encrypted user key). Keys that
/// match the runtime platform (OAuth / server.toml injected) are excluded so
/// platform credentials never land in the persisted user layer.
pub fn merge_user_preferences(
    incoming: &ModelSettings,
    existing: &UserSettings,
    platform: &PlatformSettings,
) -> UserSettings {
    let mut next = UserSettings {
        providers: incoming.providers.clone(),
        active_provider_id: incoming.active_provider_id.clone(),
        model: incoming.model.clone(),
        temperature: incoming.temperature,
        max_tokens: incoming.max_tokens,
        tool_approval_mode: incoming.tool_approval_mode.clone(),
        agent_mode: incoming.agent_mode.clone(),
        workspace_root: incoming.workspace_root.clone(),
        lead_agent_id: incoming.lead_agent_id.clone(),
        context_compression_enabled: incoming.context_compression_enabled,
        context_budget_tokens: incoming.context_budget_tokens,
        context_keep_recent_user_turns: incoming.context_keep_recent_user_turns,
        context_summary_max_tokens: incoming.context_summary_max_tokens,
        max_tool_rounds: incoming.max_tool_rounds,
        max_sub_agent_tool_rounds: incoming.max_sub_agent_tool_rounds,
        max_sub_agent_spawn_depth: incoming.max_sub_agent_spawn_depth,
        raw_content_view_enabled: incoming.raw_content_view_enabled,
        debug_dump_llm_prompts: incoming.debug_dump_llm_prompts,
        terminal_env_overrides: incoming.terminal_env_overrides.clone(),
        debug_menus_enabled: incoming.debug_menus_enabled,
        task_board_show_child_boards: incoming.task_board_show_child_boards,
        user_dynamic_inject_enabled: incoming.user_dynamic_inject_enabled,
        agent_default_models: incoming.agent_default_models.clone(),
        agent_task_board_history_trim: incoming.agent_task_board_history_trim.clone(),
        computer_human_like: incoming.computer_human_like,
        computer_initial_tier: incoming.computer_initial_tier.clone(),
        computer_annotated_screen_view_enabled: incoming.computer_annotated_screen_view_enabled,
        captcha_slider_offset_px: incoming.captcha_slider_offset_px,
        computer_show_monitor_picker: incoming.computer_show_monitor_picker,
        computer_auto_switch_monitor: incoming.computer_auto_switch_monitor,
        agent_ui_overrides: incoming.agent_ui_overrides.clone(),
        web_search_model: incoming.web_search_model.clone(),
        media_model_overrides: incoming.media_model_overrides.clone(),
        agent_performance_modes: incoming.agent_performance_modes.clone(),
        media_understanding_modes: incoming.media_understanding_modes.clone(),
        computer_tier_llm: incoming.computer_tier_llm.clone(),
        computer_pipeline_llm: incoming.computer_pipeline_llm.clone(),
        agent_mode_llm: incoming.agent_mode_llm.clone(),
        media_mode_llm: incoming.media_mode_llm.clone(),
        max_parallel_tool_calls: incoming.max_parallel_tool_calls,
        max_parallel_sub_agents: incoming.max_parallel_sub_agents,
        max_parallel_media_jobs: incoming.max_parallel_media_jobs,
        max_concurrent_runs: incoming.max_concurrent_runs,
        parallel_tool_execution_enabled: incoming.parallel_tool_execution_enabled,
        ..existing.clone()
    };
    // Computer tier / pipeline LLM: overlay incoming on existing so a partial
    // snapshot never wipes tiers the client did not send.
    {
        let mut merged = existing.computer_tier_llm.clone();
        for (tier, config) in &next.computer_tier_llm {
            merged.insert(tier.clone(), config.clone());
        }
        next.computer_tier_llm = merged;
    }
    next.computer_pipeline_llm = incoming.computer_pipeline_llm.clone();
    // Per-agent/per-mode LLM config: start with existing then overlay incoming.
    {
        let mut merged = existing.agent_mode_llm.clone();
        for (agent_id, modes) in &next.agent_mode_llm {
            let agent_entry = merged.entry(agent_id.clone()).or_default();
            for (mode, config) in modes {
                agent_entry.insert(mode.clone(), config.clone());
            }
        }
        next.agent_mode_llm = merged;
    }
    {
        let mut merged = existing.media_mode_llm.clone();
        for (kind, modes) in &next.media_mode_llm {
            let kind_entry = merged.entry(kind.clone()).or_default();
            for (mode, config) in modes {
                kind_entry.insert(mode.clone(), config.clone());
            }
        }
        next.media_mode_llm = merged;
    }
    // Providers are user-owned. Persist only keys the user typed: incoming may
    // carry a freshly typed key; masked/empty entries fall back to the previously
    // encrypted user key (existing). Keys that equal the runtime platform key
    // (OAuth / server.toml injected) are treated as platform-injected and dropped,
    // so platform credentials never land in the persisted user layer.
    let existing_keys: HashMap<String, String> = existing
        .providers
        .iter()
        .map(|p| (p.id.clone(), p.api_key.clone()))
        .collect();
    let platform_keys: HashMap<String, String> = platform
        .providers
        .iter()
        .map(|p| (p.id.clone(), p.api_key.clone()))
        .collect();
    for provider in &mut next.providers {
        let incoming_key = provider.api_key.trim();
        let is_platform_key = platform_keys
            .get(&provider.id)
            .map(|k| !k.trim().is_empty() && k.trim() == incoming_key)
            .unwrap_or(false);
        if incoming_key.is_empty() || incoming_key == "****" || is_platform_key {
            if let Some(key) = existing_keys.get(&provider.id) {
                provider.api_key = key.clone();
            } else {
                provider.api_key.clear();
            }
        }
    }
    next
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
    fn merge_user_preferences_overlays_computer_tier_llm() {
        let mut existing = UserSettings::default();
        existing.computer_tier_llm.insert(
            "primary".into(),
            ComputerTierLlmConfig {
                provider_id: "qwen".into(),
                model: "existing-model".into(),
                enable_thinking: false,
                thinking_budget: None,
            },
        );
        existing.computer_tier_llm.insert(
            "advanced".into(),
            ComputerTierLlmConfig {
                provider_id: "qwen".into(),
                model: "existing-advanced".into(),
                enable_thinking: true,
                thinking_budget: Some(4096),
            },
        );
        existing.computer_pipeline_llm.verify = "existing-verify".into();

        let mut incoming = ModelSettings::default();
        incoming.computer_tier_llm.clear();
        incoming.computer_tier_llm.insert(
            "primary".into(),
            ComputerTierLlmConfig {
                provider_id: "openrouter".into(),
                model: "incoming-primary".into(),
                enable_thinking: true,
                thinking_budget: Some(2048),
            },
        );
        incoming.computer_tier_llm.insert(
            "intermediate".into(),
            ComputerTierLlmConfig {
                provider_id: "openrouter".into(),
                model: "incoming-intermediate".into(),
                enable_thinking: false,
                thinking_budget: None,
            },
        );
        incoming.computer_pipeline_llm.verify = "incoming-verify".into();

        let platform = PlatformSettings::default();
        let merged = merge_user_preferences(&incoming, &existing, &platform);
        assert_eq!(
            merged.computer_tier_llm.get("primary").unwrap().model,
            "incoming-primary"
        );
        assert_eq!(
            merged.computer_tier_llm.get("primary").unwrap().provider_id,
            "openrouter"
        );
        assert_eq!(
            merged.computer_tier_llm.get("intermediate").unwrap().model,
            "incoming-intermediate"
        );
        // Existing-only tiers are retained, incoming-only tiers added.
        assert_eq!(
            merged.computer_tier_llm.get("advanced").unwrap().model,
            "existing-advanced"
        );
        assert_eq!(merged.computer_pipeline_llm.verify, "incoming-verify");
    }

    #[test]
    fn merge_user_preferences_keeps_typed_key_and_skips_platform_key() {
        // User typed a key on an existing provider → preserved.
        let mut existing = UserSettings::default();
        existing.providers[0].api_key = "sk-user-typed".into();
        let mut incoming = ModelSettings::default();
        incoming.providers[0].api_key = "sk-incoming".into();
        let platform = PlatformSettings::default();
        let merged = merge_user_preferences(&incoming, &existing, &platform);
        assert_eq!(merged.providers[0].api_key, "sk-incoming");
        // Masked / empty incoming falls back to the previously typed user key.
        let mut incoming2 = ModelSettings::default();
        incoming2.providers[0].api_key = "****".into();
        let merged2 = merge_user_preferences(&incoming2, &existing, &platform);
        assert_eq!(merged2.providers[0].api_key, "sk-user-typed");
        // Platform runtime keys are NOT folded into the persisted user layer.
        let mut platform2 = PlatformSettings::default();
        platform2.providers[0].api_key = "sk-platform-injected".into();
        let mut incoming3 = ModelSettings::default();
        incoming3.providers[0].api_key = "sk-platform-injected".into();
        let existing3 = UserSettings::default();
        let merged3 = merge_user_preferences(&incoming3, &existing3, &platform2);
        assert!(merged3.providers[0].api_key.is_empty());
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
