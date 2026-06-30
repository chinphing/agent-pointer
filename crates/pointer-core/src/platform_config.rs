//! In-memory platform configuration and defaults.

use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use crate::models::{
    ensure_agent_model_refs_have_provider, ensure_provider_generation_defaults,
    ensure_provider_model_capability_defaults, filter_openrouter_providers, merge_user_platform,
    MediaOssConfig, ModelSettings, PlatformSettings, ProviderConfig, UserSettings,
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
    ensure_provider_model_capability_defaults(&mut settings);
    settings
}

/// Build platform settings from merged UI/runtime model settings.
pub fn platform_settings_from_model_settings(s: &ModelSettings) -> PlatformSettings {
    PlatformSettings {
        providers: s.providers.clone(),
        active_provider_id: s.active_provider_id.clone(),
        model: s.model.clone(),
        temperature: s.temperature,
        max_tokens: s.max_tokens,
        tool_approval_mode: s.tool_approval_mode.clone(),
        agent_mode: s.agent_mode.clone(),
        workspace_root: s.workspace_root.clone(),
        lead_agent_id: s.lead_agent_id.clone(),
        context_compression_enabled: s.context_compression_enabled,
        context_budget_tokens: s.context_budget_tokens,
        context_keep_recent_user_turns: s.context_keep_recent_user_turns,
        context_summary_max_tokens: s.context_summary_max_tokens,
        max_tool_rounds: s.max_tool_rounds,
        max_sub_agent_tool_rounds: s.max_sub_agent_tool_rounds,
        max_sub_agent_spawn_depth: s.max_sub_agent_spawn_depth,
        raw_content_view_enabled: s.raw_content_view_enabled,
        debug_dump_llm_prompts: s.debug_dump_llm_prompts,
        debug_menus_enabled: s.debug_menus_enabled,
        task_board_show_child_boards: s.task_board_show_child_boards,
        computer_standalone_planner_enabled: s.computer_standalone_planner_enabled,
        user_dynamic_inject_enabled: s.user_dynamic_inject_enabled,
        agent_default_models: s.agent_default_models.clone(),
        agent_task_board_history_trim: s.agent_task_board_history_trim.clone(),
        computer_human_like: s.computer_human_like,
        computer_initial_tier: s.computer_initial_tier.clone(),
        computer_annotated_screen_view_enabled: s.computer_annotated_screen_view_enabled,
        dati_api_url: s.dati_api_url.clone(),
        dati_authcode: s.dati_authcode.clone(),
        dati_typeno: s.dati_typeno.clone(),
        dati_author: s.dati_author.clone(),
        captcha_slider_offset_px: s.captcha_slider_offset_px,
        computer_show_monitor_picker: s.computer_show_monitor_picker,
        computer_auto_switch_monitor: s.computer_auto_switch_monitor,
        agent_ui_overrides: s.agent_ui_overrides.clone(),
        web_search_model: s.web_search_model.clone(),
        media_model_overrides: s.media_model_overrides.clone(),
        agent_performance_modes: s.agent_performance_modes.clone(),
        media_understanding_modes: s.media_understanding_modes.clone(),
        computer_tier_llm: PlatformSettings::default().computer_tier_llm,
        agent_mode_llm: s.agent_mode_llm.clone(),
        media_mode_llm: s.media_mode_llm.clone(),
        media_oss: s.media_oss.clone(),
    }
}

/// Apply UI-edited preferences while preserving empty incoming provider keys.
pub fn merge_platform_preferences(incoming: &ModelSettings, existing: &PlatformSettings) -> PlatformSettings {
    let mut next = platform_settings_from_model_settings(incoming);
    next.computer_tier_llm = existing.computer_tier_llm.clone();
    // Per-agent/per-mode LLM config: start with existing then overlay incoming
    // on top so that incoming values always take priority.
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
    next.providers = filter_openrouter_providers(next.providers);
    let preserved_keys: HashMap<String, String> = existing
        .providers
        .iter()
        .map(|p| (p.id.clone(), p.api_key.clone()))
        .collect();
    for provider in &mut next.providers {
        if provider.api_key.trim().is_empty() {
            if let Some(key) = preserved_keys.get(&provider.id) {
                provider.api_key = key.clone();
            }
        }
    }
    // DaTi config is server/build-time only; web clients never send these fields.
    next.dati_api_url = existing.dati_api_url.clone();
    next.dati_authcode = existing.dati_authcode.clone();
    next.dati_typeno = existing.dati_typeno.clone();
    next.dati_author = existing.dati_author.clone();
    next
}

pub fn persist_local_platform_settings(platform: &PlatformSettings) {
    if let Err(e) = storage::save_local_platform_from_runtime(platform) {
        log::warn!("platform_config: failed to persist local platform settings: {e}");
    }
}

pub fn apply_login_credentials_to_model_settings(
    settings: &mut ModelSettings,
    creds: &crate::platform_auth::PlatformLoginCredentials,
) {
    let mut scratch = PlatformSettings {
        providers: settings.providers.clone(),
        active_provider_id: settings.active_provider_id.clone(),
        ..PlatformSettings::default()
    };
    apply_login_llm_provider_api_keys(&mut scratch, &creds.provider_api_keys);
    apply_login_llm_credentials(
        &mut scratch,
        creds.api_key.as_deref(),
        creds.llm_provider.as_deref(),
    );
    if let Some(media) = creds.media_oss.as_ref() {
        apply_login_media_oss(&mut scratch, Some(media));
    }
    for sp in &mut settings.providers {
        if let Some(tp) = scratch.providers.iter().find(|p| p.id == sp.id) {
            if !tp.api_key.trim().is_empty() {
                sp.api_key = tp.api_key.clone();
            }
        }
    }
    if let Some(p) = settings
        .providers
        .iter()
        .find(|p| p.id == settings.active_provider_id)
    {
        settings.api_key = p.api_key.clone();
        settings.has_key = !p.api_key.trim().is_empty();
    }
    if !scratch.media_oss.bucket.trim().is_empty() {
        settings.media_oss = scratch.media_oss.clone();
    }
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

/// Inject per-provider OAuth-issued LLM credentials (provider id -> api key).
pub fn apply_login_llm_provider_api_keys(
    platform: &mut PlatformSettings,
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
        let Some(pid) = resolve_llm_provider_id(Some(raw_provider.as_str()), &platform.providers) else {
            log::warn!("platform_config: skip unknown provider {raw_provider}");
            continue;
        };
        if let Some(p) = platform.providers.iter_mut().find(|p| p.id == pid) {
            p.api_key = key.to_string();
            log::info!("platform_config: injected api_key for provider {pid}");
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

/// Inject OAuth-issued media OSS credentials into in-memory platform settings.
pub fn apply_login_media_oss(
    platform: &mut PlatformSettings,
    media: Option<&PlatformMediaOssCredentials>,
) {
    let Some(raw) = media else {
        platform.media_oss = MediaOssConfig::default();
        return;
    };
    let endpoint = raw.endpoint.trim();
    let access_key_id = raw.access_key_id.trim();
    let access_key_secret = raw.access_key_secret.trim();
    if endpoint.is_empty() || access_key_id.is_empty() || access_key_secret.is_empty() {
        platform.media_oss = MediaOssConfig::default();
        log::info!("platform_config: login token has no usable media_oss; cleared");
        return;
    }
    let region = parse_oss_region_from_endpoint(endpoint).unwrap_or_else(|| {
        DEFAULT_PLATFORM_MEDIA_OSS_REGION.to_string()
    });
    let bucket = parse_oss_bucket_from_endpoint(endpoint)
        .unwrap_or_else(|| DEFAULT_PLATFORM_MEDIA_OSS_BUCKET.to_string());
    platform.media_oss = MediaOssConfig {
        enabled: true,
        bucket,
        region,
        endpoint: endpoint.to_string(),
        access_key_id: access_key_id.to_string(),
        access_key_secret: access_key_secret.to_string(),
        ..MediaOssConfig::default()
    };
    log::info!("platform_config: injected media_oss from platform login");
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
    use crate::models::PersistedLocalPlatformSettings;

    #[test]
    fn apply_login_maps_aliyun_qwen() {
        let mut platform = PlatformSettings::default();
        apply_login_llm_credentials(&mut platform, Some("sk-test"), Some("aliyun_qwen"));
        let qwen = platform.providers.iter().find(|p| p.id == "qwen").unwrap();
        assert_eq!(qwen.api_key, "sk-test");
    }

    #[test]
    fn apply_login_provider_api_keys_maps_aliyun_qwen_alias() {
        let mut platform = PlatformSettings::default();
        let mut keys = HashMap::new();
        keys.insert("aliyun_qwen".into(), "sk-qwen".into());
        keys.insert("deepseek".into(), "sk-ds".into());
        apply_login_llm_provider_api_keys(&mut platform, &keys);
        assert_eq!(
            platform.providers.iter().find(|p| p.id == "qwen").unwrap().api_key,
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
    fn persisted_local_platform_keeps_agent_fields_only() {
        let mut platform = PlatformSettings::default();
        platform.providers[0].api_key = "sk-secret".into();
        platform.providers.push(ProviderConfig {
            id: "openrouter".into(),
            name: "OpenRouter".into(),
            base_url: "https://openrouter.ai/api/v1".into(),
            api_key: String::new(),
            models: vec!["gpt-4o".into()],
            reasoning_in_messages: None,
            temperature: None,
            max_tokens: None,
            model_configs: HashMap::new(),
            enable_thinking: None,
            thinking_budget: None,
            reasoning_effort: None,
        });
        platform.active_provider_id = "qwen".into();
        platform.model = "qwen3.5-plus".into();
        platform.temperature = 0.9;
        platform.max_tokens = 8192;
        platform.tool_approval_mode = "manual".into();
        platform.computer_human_like = true;
        platform.computer_initial_tier = "intermediate".into();
        platform.context_compression_enabled = false;
        platform.context_budget_tokens = 99_000;
        platform.max_tool_rounds = 42;
        platform.agent_mode = "single".into();
        platform.lead_agent_id = "coder".into();
        platform.workspace_root = "/tmp/pointer-workspace".into();
        platform.raw_content_view_enabled = true;
        platform.debug_dump_llm_prompts = true;
        platform.computer_annotated_screen_view_enabled = true;
        platform.agent_ui_overrides.insert(
            "computer".into(),
            crate::agents::AgentUiConfig {
                show_computer_monitor_picker: Some(false),
                ..Default::default()
            },
        );

        let json = serde_json::to_string(&PersistedLocalPlatformSettings::from_platform(&platform)).unwrap();
        assert!(!json.contains("sk-secret"));
        assert!(!json.contains("apiKey"));
        assert!(!json.contains("openrouter"));
        assert!(!json.contains("OpenRouter"));
        assert!(!json.contains("activeProviderId"));
        assert!(!json.contains("temperature"));
        assert!(!json.contains("rawContentViewEnabled"));
        assert!(!json.contains("debugDumpLlmPrompts"));
        assert!(!json.contains("computerAnnotatedScreenViewEnabled"));
        assert!(!json.contains("agentUiOverrides"));
        assert!(!json.contains("datiApiUrl"));
        assert!(!json.contains("datiAuthcode"));
        assert!(!json.contains("datiTypeno"));
        assert!(!json.contains("datiAuthor"));
        assert!(json.contains("toolApprovalMode"));
        assert!(json.contains("manual"));
        assert!(json.contains("computerHumanLike"));
        assert!(json.contains("maxToolRounds"));
        assert!(json.contains("leadAgentId"));
        assert!(json.contains("coder"));
        assert!(json.contains("workspaceRoot"));
        assert!(json.contains("/tmp/pointer-workspace"));

        let loaded = PersistedLocalPlatformSettings::from_platform(&platform).into_platform();
        assert_eq!(loaded.tool_approval_mode, "manual");
        assert!(loaded.computer_human_like);
        assert_eq!(loaded.computer_initial_tier, "intermediate");
        assert!(!loaded.context_compression_enabled);
        assert_eq!(loaded.context_budget_tokens, 99_000);
        assert_eq!(loaded.max_tool_rounds, 42);
        assert_eq!(loaded.agent_mode, "single");
        assert_eq!(loaded.lead_agent_id, "coder");
        assert_eq!(loaded.workspace_root, "/tmp/pointer-workspace");
        assert_eq!(loaded.tool_approval_mode, platform.tool_approval_mode);
        assert_eq!(loaded.model, PlatformSettings::default().model);
        assert!(loaded.providers.iter().all(|p| p.api_key.is_empty()));
    }

    #[test]
    fn merge_platform_preferences_preserves_dati_fields() {
        let mut existing = PlatformSettings::default();
        existing.dati_api_url = "https://dati.example".into();
        existing.dati_authcode = "keep-auth".into();
        existing.dati_typeno = "501057".into();
        existing.dati_author = "keep-author".into();
        let incoming = ModelSettings::default();
        let merged = merge_platform_preferences(&incoming, &existing);
        assert_eq!(merged.dati_api_url, "https://dati.example");
        assert_eq!(merged.dati_authcode, "keep-auth");
        assert_eq!(merged.dati_typeno, "501057");
        assert_eq!(merged.dati_author, "keep-author");
    }

    #[test]
    fn default_agent_model_is_deepseek_flash() {
        let platform = PlatformSettings::default();
        assert_eq!(
            platform
                .agent_default_models
                .get("general")
                .map(|r| r.model.as_str()),
            Some("deepseek-v4-flash")
        );
    }
}
