use crate::models::{
    ensure_agent_model_refs_have_provider, ensure_provider_generation_defaults,
    ensure_provider_model_capability_defaults, filter_openrouter_providers, merge_user_platform,
    AgentModelRef, ChatMessage, Conversation, ConversationMeta, ConversationSearchHit,
    ModelRuntimeOverrides, ModelSettings, PersistedLocalPlatformSettings, PlatformSettings,
    Project, ProjectCreationResult, ProjectCursor, ProjectPage, ProviderConfig, UserSettings,
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Once, OnceLock};

/// Production subfolder under the OS user data directory (`dirs::data_dir()`).
pub const APP_DATA_SUBDIR: &str = "PointerApp";

/// Default subfolder for debug builds (`cargo run` / `tauri dev`) so dev and release can run side by side.
pub const APP_DATA_SUBDIR_DEV: &str = "PointerAppDev";

static RESOLVED_APP_DATA_DIR: OnceLock<PathBuf> = OnceLock::new();

static LEGACY_MIGRATION_ONCE: Once = Once::new();

fn default_app_data_subdir() -> &'static str {
    if cfg!(debug_assertions) {
        APP_DATA_SUBDIR_DEV
    } else {
        APP_DATA_SUBDIR
    }
}

fn compute_app_data_dir() -> Result<PathBuf> {
    if let Ok(raw) = std::env::var("POINTER_APP_DATA_DIR") {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            let dir = PathBuf::from(trimmed);
            if !dir.exists() {
                fs::create_dir_all(&dir)?;
            }
            log::info!("storage: using POINTER_APP_DATA_DIR={}", dir.display());
            return Ok(dir);
        }
    }

    let base = dirs::data_dir().context("无法获取数据目录")?;
    let subdir = std::env::var("POINTER_APP_DATA_SUBDIR")
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| default_app_data_subdir().to_string());
    let dir = base.join(&subdir);
    if !dir.exists() {
        fs::create_dir_all(&dir)?;
    }
    log::info!("storage: app data dir={} (subdir={subdir})", dir.display());
    Ok(dir)
}

fn data_dir() -> Result<PathBuf> {
    if let Some(dir) = RESOLVED_APP_DATA_DIR.get() {
        return Ok(dir.clone());
    }
    let dir = compute_app_data_dir()?;
    let _ = RESOLVED_APP_DATA_DIR.set(dir.clone());
    Ok(dir)
}

pub fn app_data_dir() -> Result<PathBuf> {
    data_dir()
}

/// Flatten a conversation id into a single cross-platform directory name.
///
/// IM ids contain `:` (e.g. `wecom:default:wecom:dm:chat:sender`); Windows rejects `:` in paths.
pub fn sanitize_storage_dir_segment(id: &str) -> String {
    let mut out: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    while out.contains("__") {
        out = out.replace("__", "_");
    }
    out.trim_matches('_').to_string()
}

/// User-managed environment variables for terminal subprocesses (`{app_data_dir}/.env`).
pub fn user_env_file_path() -> Result<PathBuf> {
    Ok(data_dir()?.join(".env"))
}

fn settings_path() -> Result<PathBuf> {
    Ok(data_dir()?.join("settings.json"))
}
fn settings_migrated_path() -> Result<PathBuf> {
    Ok(data_dir()?.join("settings.json.migrated"))
}
fn user_settings_path() -> Result<PathBuf> {
    Ok(data_dir()?.join("user_settings.json"))
}
fn local_platform_settings_path() -> Result<PathBuf> {
    Ok(data_dir()?.join("local_platform_settings.json"))
}
fn auth_dat_path() -> Result<PathBuf> {
    Ok(data_dir()?.join("auth.dat"))
}
fn key_path() -> Result<PathBuf> {
    Ok(data_dir()?.join("key.dat"))
}
#[derive(Debug, Serialize, Deserialize)]
struct StoredModelOverrides {
    #[serde(default, rename = "reasoningInMessages")]
    reasoning_in_messages: Option<bool>,
    #[serde(default)]
    temperature: Option<f32>,
    #[serde(default, rename = "maxTokens")]
    max_tokens: Option<u32>,
    #[serde(default, rename = "enableThinking")]
    enable_thinking: Option<bool>,
    #[serde(default, rename = "thinkingBudget")]
    thinking_budget: Option<u32>,
    #[serde(default, rename = "reasoningEffort")]
    reasoning_effort: Option<String>,
    /// Legacy; absorbed on load, not written back.
    #[serde(default, rename = "extraBody")]
    extra_body: Option<serde_json::Value>,
    /// Legacy; absorbed on load, not written back.
    #[serde(default, rename = "thinkingEnabled")]
    thinking_enabled: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize)]
struct StoredProvider {
    id: String,
    name: String,
    #[serde(rename = "baseUrl")]
    base_url: String,
    #[serde(default, rename = "apiKey")]
    api_key: String,
    models: Vec<String>,
    #[serde(default, rename = "reasoningInMessages")]
    reasoning_in_messages: Option<bool>,
    #[serde(default)]
    temperature: Option<f32>,
    #[serde(default, rename = "maxTokens")]
    max_tokens: Option<u32>,
    #[serde(default, rename = "enableThinking")]
    enable_thinking: Option<bool>,
    #[serde(default, rename = "thinkingBudget")]
    thinking_budget: Option<u32>,
    #[serde(default, rename = "reasoningEffort")]
    reasoning_effort: Option<String>,
    /// Legacy; absorbed on load, not written back.
    #[serde(default, rename = "extraBody")]
    extra_body: Option<serde_json::Value>,
    /// Legacy; absorbed on load, not written back.
    #[serde(default, rename = "thinkingEnabled")]
    thinking_enabled: Option<bool>,
    #[serde(default, rename = "modelConfigs")]
    model_configs: HashMap<String, StoredModelOverrides>,
}

#[derive(Debug, Serialize, Deserialize)]
struct StoredSettings {
    providers: Vec<StoredProvider>,
    #[serde(default, rename = "activeProviderId")]
    active_provider_id: String,
    model: String,
    temperature: f32,
    max_tokens: u32,
    #[serde(default = "default_tool_approval_mode")]
    tool_approval_mode: String,
    #[serde(default = "default_agent_mode")]
    agent_mode: String,
    #[serde(default, rename = "workspaceRoot")]
    workspace_root: String,
    #[serde(default, rename = "leadAgentId")]
    lead_agent_id: String,
    #[serde(
        default = "default_context_compression_enabled",
        rename = "contextCompressionEnabled"
    )]
    context_compression_enabled: bool,
    #[serde(
        default = "default_context_budget_tokens",
        rename = "contextBudgetTokens",
        alias = "contextBudgetChars"
    )]
    context_budget_tokens: u32,
    #[serde(
        default = "default_context_keep_recent_user_turns",
        rename = "contextKeepRecentUserTurns"
    )]
    context_keep_recent_user_turns: u32,
    #[serde(
        default = "default_context_summary_max_tokens",
        rename = "contextSummaryMaxTokens"
    )]
    context_summary_max_tokens: u32,
    #[serde(default = "default_max_tool_rounds", rename = "maxToolRounds")]
    max_tool_rounds: u32,
    #[serde(default = "default_max_tool_rounds", rename = "maxSubAgentToolRounds")]
    max_sub_agent_tool_rounds: u32,
    #[serde(
        default = "default_max_sub_agent_spawn_depth",
        rename = "maxSubAgentSpawnDepth"
    )]
    max_sub_agent_spawn_depth: u32,
    #[serde(
        default = "default_raw_content_view_enabled",
        rename = "rawContentViewEnabled"
    )]
    raw_content_view_enabled: bool,
    #[serde(
        default = "default_debug_dump_llm_prompts",
        rename = "debugDumpLlmPrompts"
    )]
    debug_dump_llm_prompts: bool,
    #[serde(default = "default_debug_menus_enabled", rename = "debugMenusEnabled")]
    debug_menus_enabled: bool,
    #[serde(default, rename = "agentDefaultModels")]
    agent_default_models: HashMap<String, serde_json::Value>,
    #[serde(default, rename = "agentTaskBoardHistoryTrim")]
    agent_task_board_history_trim: HashMap<String, bool>,
    #[serde(default = "default_computer_human_like", rename = "computerHumanLike")]
    computer_human_like: bool,
    #[serde(
        default = "default_computer_initial_tier",
        rename = "computerInitialTier"
    )]
    computer_initial_tier: String,
    #[serde(
        default = "default_computer_annotated_screen_view_enabled",
        rename = "computerAnnotatedScreenViewEnabled"
    )]
    computer_annotated_screen_view_enabled: bool,
    #[serde(default = "default_theme")]
    theme: String,
    #[serde(default, rename = "agentUiOverrides")]
    agent_ui_overrides: HashMap<String, crate::agents::AgentUiConfig>,
    /// Legacy global toggle; applied to each provider when that provider has no explicit value.
    #[serde(default, rename = "reasoningInMessages")]
    legacy_reasoning_in_messages: Option<bool>,
}

fn default_tool_approval_mode() -> String {
    "auto".into()
}

fn default_agent_mode() -> String {
    "single".into()
}

fn default_context_compression_enabled() -> bool {
    true
}

fn default_context_budget_tokens() -> u32 {
    120_000
}

fn default_context_keep_recent_user_turns() -> u32 {
    6
}

fn default_context_summary_max_tokens() -> u32 {
    2048
}

fn default_max_tool_rounds() -> u32 {
    100
}

fn default_max_sub_agent_spawn_depth() -> u32 {
    2
}

fn default_raw_content_view_enabled() -> bool {
    true
}

fn default_theme() -> String {
    "system".into()
}

fn default_computer_initial_tier() -> String {
    "intermediate".into()
}

fn default_computer_human_like() -> bool {
    true
}

fn default_debug_dump_llm_prompts() -> bool {
    false
}

fn default_debug_menus_enabled() -> bool {
    false
}

fn default_computer_annotated_screen_view_enabled() -> bool {
    false
}

/// Run once per process: migrate legacy `settings.json` theme → `user_settings.json`.
pub fn ensure_legacy_settings_migrated() {
    LEGACY_MIGRATION_ONCE.call_once(|| {
        if let Err(e) = migrate_legacy_settings_if_needed() {
            log::warn!("storage: legacy settings migration failed: {e}");
        }
    });
}

fn migrate_legacy_settings_if_needed() -> Result<()> {
    let legacy = settings_path()?;
    let migrated = settings_migrated_path()?;
    if !legacy.exists() || migrated.exists() {
        return Ok(());
    }
    log::info!("storage: migrating legacy settings.json → user_settings.json");
    let raw = fs::read_to_string(&legacy)?;
    let stored: StoredSettings = serde_json::from_str(&raw).unwrap_or_default();
    let theme = if stored.theme.is_empty() {
        default_theme()
    } else {
        stored.theme.clone()
    };
    let user = UserSettings {
        theme,
        user_nickname: None,
        enabled_skill_ids: Vec::new(),
        ..UserSettings::default()
    };
    write_user_settings_file(&user)?;
    let platform = stored_settings_to_platform(&stored);
    save_local_platform_from_runtime(&platform)?;
    fs::rename(&legacy, &migrated)?;
    log::info!(
        "storage: legacy settings.json renamed to {}",
        migrated.display()
    );
    if key_path()?.exists() {
        if let Err(e) = fs::remove_file(key_path()?) {
            log::warn!("storage: failed to remove deprecated key.dat: {e}");
        } else {
            log::info!("storage: removed deprecated key.dat");
        }
    }
    Ok(())
}

pub fn load_user_settings() -> Result<UserSettings> {
    ensure_legacy_settings_migrated();
    let path = user_settings_path()?;
    if !path.exists() {
        return Ok(UserSettings::default());
    }
    let raw = fs::read_to_string(&path)?;
    let mut user: UserSettings = serde_json::from_str(&raw).unwrap_or_default();
    user.media_oss = Default::default();
    Ok(user)
}

fn write_user_settings_file(user: &UserSettings) -> Result<()> {
    fs::write(user_settings_path()?, serde_json::to_vec_pretty(user)?)?;
    Ok(())
}

pub fn save_user_settings(user: &UserSettings) -> Result<()> {
    ensure_legacy_settings_migrated();
    let mut to_save = user.clone();
    to_save.media_oss = Default::default();
    write_user_settings_file(&to_save)
}

fn migrate_planner_settings_json(value: &mut serde_json::Value) {
    let obj = match value.as_object_mut() {
        Some(o) => o,
        None => return,
    };
    obj.remove("taskBoardPlannerEnabled");
    obj.remove("taskBoardWorkItemsEnabled");
    obj.remove("taskBoardComputerNoExecInit");
    obj.remove("computerStandalonePlannerEnabled");
}

/// Desktop-only persisted agent preferences (智能体 section).
pub fn load_local_platform_settings() -> Result<Option<PlatformSettings>> {
    ensure_local_platform_imported()?;
    let path = local_platform_settings_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&path)?;
    let contains_sensitive_dati = local_platform_contains_sensitive_dati_keys(&raw);
    if let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&raw) {
        migrate_planner_settings_json(&mut value);
        if let Ok(persisted) = serde_json::from_value::<PersistedLocalPlatformSettings>(value) {
            if contains_sensitive_dati {
                log::warn!(
                    "storage: local_platform_settings.json contains sensitive DaTi keys; rewriting sanitized file"
                );
                save_local_platform_settings(&persisted)?;
            }
            return Ok(Some(persisted.into_platform()));
        }
    }
    // Legacy file written as full PlatformSettings (may contain apiKey / debug fields).
    if let Ok(legacy) = serde_json::from_str::<PlatformSettings>(&raw) {
        log::warn!("storage: sanitizing legacy local_platform_settings.json (strip secrets/debug/openrouter)");
        let mut legacy = legacy;
        legacy.providers = filter_openrouter_providers(legacy.providers);
        let persisted = PersistedLocalPlatformSettings::from_platform(&legacy);
        save_local_platform_settings(&persisted)?;
        return Ok(Some(persisted.into_platform()));
    }
    Ok(None)
}

fn local_platform_contains_sensitive_dati_keys(raw: &str) -> bool {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(raw) else {
        return false;
    };
    let Some(obj) = v.as_object() else {
        return false;
    };
    obj.contains_key("datiApiUrl")
        || obj.contains_key("datiAuthcode")
        || obj.contains_key("datiTypeno")
        || obj.contains_key("datiAuthor")
}

pub fn save_local_platform_settings(persisted: &PersistedLocalPlatformSettings) -> Result<()> {
    fs::write(
        local_platform_settings_path()?,
        serde_json::to_vec_pretty(persisted)?,
    )?;
    Ok(())
}

pub fn save_local_platform_from_runtime(platform: &PlatformSettings) -> Result<()> {
    save_local_platform_settings(&PersistedLocalPlatformSettings::from_platform(platform))
}

/// One-time import for installs that migrated theme before local platform persistence existed.
fn ensure_local_platform_imported() -> Result<()> {
    let local = local_platform_settings_path()?;
    if local.exists() {
        return Ok(());
    }
    let migrated = settings_migrated_path()?;
    if !migrated.exists() {
        return Ok(());
    }
    log::info!("storage: importing local platform settings from settings.json.migrated");
    let raw = fs::read_to_string(&migrated)?;
    let stored: StoredSettings = serde_json::from_str(&raw).unwrap_or_default();
    let platform = stored_settings_to_platform(&stored);
    save_local_platform_from_runtime(&platform)?;
    log::info!("storage: wrote local_platform_settings.json from legacy backup");
    Ok(())
}

fn stored_model_overrides_to_runtime(v: &StoredModelOverrides) -> ModelRuntimeOverrides {
    ModelRuntimeOverrides {
        reasoning_in_messages: v.reasoning_in_messages,
        temperature: v.temperature,
        max_tokens: v.max_tokens,
        enable_thinking: v.enable_thinking.or(v.thinking_enabled),
        thinking_budget: v.thinking_budget,
        reasoning_effort: v.reasoning_effort.clone(),
        supports_vision: None,
        can_generate_image: None,
        can_generate_video: None,
    }
}

fn stored_provider_to_platform(
    p: &StoredProvider,
    legacy_reasoning: Option<bool>,
) -> ProviderConfig {
    ProviderConfig {
        id: p.id.clone(),
        name: p.name.clone(),
        base_url: p.base_url.clone(),
        api_key: p.api_key.clone(),
        models: p.models.clone(),
        reasoning_in_messages: p.reasoning_in_messages.or(legacy_reasoning),
        temperature: p.temperature,
        max_tokens: p.max_tokens,
        model_configs: p
            .model_configs
            .iter()
            .map(|(k, v)| (k.clone(), stored_model_overrides_to_runtime(v)))
            .collect(),
        enable_thinking: p.enable_thinking.or(p.thinking_enabled),
        thinking_budget: p.thinking_budget,
        reasoning_effort: p.reasoning_effort.clone(),
    }
}

fn normalize_disk_agent_defaults(
    raw: &HashMap<String, serde_json::Value>,
    legacy_active_provider: &str,
) -> HashMap<String, AgentModelRef> {
    let mut out = HashMap::new();
    for (k, v) in raw {
        let mut r = match AgentModelRef::from_json_value_flexible(v.clone()) {
            Some(x) => x,
            None => continue,
        };
        if r.provider_id.trim().is_empty() {
            r.provider_id = legacy_active_provider.to_string();
        }
        out.insert(k.clone(), r);
    }
    out
}

fn stored_settings_to_platform(stored: &StoredSettings) -> PlatformSettings {
    let legacy_reasoning = stored.legacy_reasoning_in_messages;
    let active = if stored.active_provider_id.trim().is_empty() {
        "qwen".to_string()
    } else {
        stored.active_provider_id.clone()
    };
    let mut platform = PlatformSettings {
        providers: stored
            .providers
            .iter()
            .map(|p| stored_provider_to_platform(p, legacy_reasoning))
            .collect(),
        active_provider_id: active.clone(),
        model: stored.model.clone(),
        temperature: stored.temperature,
        max_tokens: stored.max_tokens,
        tool_approval_mode: stored.tool_approval_mode.clone(),
        agent_mode: stored.agent_mode.clone(),
        workspace_root: stored.workspace_root.clone(),
        lead_agent_id: stored.lead_agent_id.clone(),
        context_compression_enabled: stored.context_compression_enabled,
        context_budget_tokens: stored.context_budget_tokens,
        context_keep_recent_user_turns: stored.context_keep_recent_user_turns,
        context_summary_max_tokens: stored.context_summary_max_tokens,
        max_tool_rounds: stored.max_tool_rounds,
        max_sub_agent_tool_rounds: stored.max_sub_agent_tool_rounds,
        max_sub_agent_spawn_depth: stored.max_sub_agent_spawn_depth,
        raw_content_view_enabled: stored.raw_content_view_enabled,
        debug_dump_llm_prompts: stored.debug_dump_llm_prompts,
        debug_menus_enabled: stored.debug_menus_enabled,
        agent_default_models: normalize_disk_agent_defaults(&stored.agent_default_models, &active),
        agent_task_board_history_trim: stored.agent_task_board_history_trim.clone(),
        computer_human_like: stored.computer_human_like,
        computer_initial_tier: stored.computer_initial_tier.clone(),
        computer_annotated_screen_view_enabled: stored.computer_annotated_screen_view_enabled,
        agent_ui_overrides: stored.agent_ui_overrides.clone(),
        ..PlatformSettings::default()
    };
    let mut merged = merge_user_platform(&UserSettings::default(), &platform);
    ensure_agent_model_refs_have_provider(&mut merged);
    platform.agent_default_models = merged.agent_default_models;
    platform
}

static PLATFORM_AUTH_PERSIST_ENABLED: AtomicBool = AtomicBool::new(true);

/// When `false`, skip reading/writing `auth.dat` (pointer-server multi-user web mode).
pub fn set_platform_auth_persist_enabled(enabled: bool) {
    PLATFORM_AUTH_PERSIST_ENABLED.store(enabled, Ordering::SeqCst);
    if !enabled {
        log::info!("storage: platform auth.dat persistence disabled");
    }
}

pub fn platform_auth_persist_enabled() -> bool {
    PLATFORM_AUTH_PERSIST_ENABLED.load(Ordering::SeqCst)
}

pub fn save_platform_refresh_token(refresh: &str) -> Result<()> {
    if !platform_auth_persist_enabled() {
        return Ok(());
    }
    let blob = crate::local_secret::encrypt_local_secret(refresh)?;
    fs::write(auth_dat_path()?, blob)?;
    Ok(())
}

pub fn load_platform_refresh_token() -> Result<Option<String>> {
    if !platform_auth_persist_enabled() {
        return Ok(None);
    }
    let path = auth_dat_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let blob = fs::read(&path)?;
    match crate::local_secret::decrypt_local_secret(&blob) {
        Ok(s) if !s.is_empty() => Ok(Some(s)),
        Ok(_) => Ok(None),
        Err(e) => {
            log::warn!("storage: auth.dat decrypt failed: {e}; removing file");
            let _ = fs::remove_file(&path);
            Err(e)
        }
    }
}

pub fn clear_platform_refresh_token() -> Result<()> {
    if !platform_auth_persist_enabled() {
        return Ok(());
    }
    let path = auth_dat_path()?;
    if path.exists() {
        fs::remove_file(&path)?;
    }
    Ok(())
}

/// Legacy helper: user theme + code-default platform (no in-memory admin overrides).
#[deprecated(note = "use AppState::effective_settings or PlatformConfigManager")]
pub fn load_settings() -> Result<ModelSettings> {
    ensure_legacy_settings_migrated();
    let user = load_user_settings()?;
    let platform = PlatformSettings::default();
    let mut settings = merge_user_platform(&user, &platform);
    ensure_agent_model_refs_have_provider(&mut settings);
    ensure_provider_generation_defaults(&mut settings);
    ensure_provider_model_capability_defaults(&mut settings);
    if let Some(p) = settings
        .providers
        .iter()
        .find(|p| p.id == settings.active_provider_id)
    {
        settings.api_key = p.api_key.clone();
        settings.has_key = !p.api_key.is_empty();
    }
    Ok(settings)
}

impl Default for StoredSettings {
    fn default() -> Self {
        let s = ModelSettings::default();
        Self {
            providers: s
                .providers
                .iter()
                .map(|p| StoredProvider {
                    id: p.id.clone(),
                    name: p.name.clone(),
                    base_url: p.base_url.clone(),
                    api_key: p.api_key.clone(),
                    models: p.models.clone(),
                    reasoning_in_messages: p.reasoning_in_messages,
                    temperature: p.temperature,
                    max_tokens: p.max_tokens,
                    model_configs: p
                        .model_configs
                        .iter()
                        .map(|(k, v)| {
                            (
                                k.clone(),
                                StoredModelOverrides {
                                    reasoning_in_messages: v.reasoning_in_messages,
                                    temperature: v.temperature,
                                    max_tokens: v.max_tokens,
                                    enable_thinking: v.enable_thinking,
                                    thinking_budget: v.thinking_budget,
                                    reasoning_effort: v.reasoning_effort.clone(),
                                    extra_body: None,
                                    thinking_enabled: None,
                                },
                            )
                        })
                        .collect(),
                    enable_thinking: p.enable_thinking,
                    thinking_budget: p.thinking_budget,
                    reasoning_effort: p.reasoning_effort.clone(),
                    extra_body: None,
                    thinking_enabled: None,
                })
                .collect(),
            active_provider_id: s.active_provider_id,
            model: s.model,
            temperature: s.temperature,
            max_tokens: s.max_tokens,
            tool_approval_mode: s.tool_approval_mode,
            agent_mode: s.agent_mode,
            workspace_root: s.workspace_root,
            lead_agent_id: s.lead_agent_id,
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
            agent_default_models: s
                .agent_default_models
                .iter()
                .map(|(k, v)| {
                    (
                        k.clone(),
                        json!({ "providerId": v.provider_id, "model": v.model }),
                    )
                })
                .collect(),
            agent_task_board_history_trim: s.agent_task_board_history_trim.clone(),
            computer_human_like: s.computer_human_like,
            computer_initial_tier: s.computer_initial_tier.clone(),
            computer_annotated_screen_view_enabled: s.computer_annotated_screen_view_enabled,
            theme: s.theme.clone(),
            agent_ui_overrides: s.agent_ui_overrides.clone(),
            legacy_reasoning_in_messages: None,
        }
    }
}

/// Deprecated: platform settings are in-memory only.
#[deprecated(note = "use update_platform_settings API")]
pub fn save_settings(_s: &ModelSettings) -> Result<()> {
    Ok(())
}

/// Deprecated: API keys live in platform config memory.
#[deprecated(note = "keys are injected via OAuth into platform config")]
pub fn save_api_key(_key: &str) -> Result<()> {
    Ok(())
}

#[deprecated(note = "keys are injected via OAuth into platform config")]
pub fn load_api_key() -> Result<Option<String>> {
    Ok(None)
}

#[deprecated(note = "keys are injected via OAuth into platform config")]
pub fn has_key() -> Result<bool> {
    Ok(false)
}

#[deprecated(note = "keys are injected via OAuth into platform config")]
pub fn clear_api_key() -> Result<()> {
    Ok(())
}

pub fn load_conversations() -> Result<Vec<Conversation>> {
    crate::conversation_store::global_store()?.load_all()
}

/// Cursor-paginated, meta-only conversation list (no messages). Sort order is
/// `(updated_at_ms DESC, id DESC)`. Pass `None` for the first page; pass the
/// last row of the previous page as the cursor to fetch the next.
pub fn load_conversation_metas(
    cursor: Option<(i64, String)>,
    limit: i64,
) -> Result<Vec<ConversationMeta>> {
    crate::conversation_store::global_store()?.load_metas(cursor, limit)
}

pub fn load_projects(cursor: Option<ProjectCursor>, limit: i64) -> Result<ProjectPage> {
    crate::conversation_store::global_store()?.load_projects(cursor, limit)
}

pub fn load_sidebar_projects() -> Result<Vec<Project>> {
    crate::conversation_store::global_store()?.load_sidebar_projects()
}

pub fn load_project(id: &str) -> Result<Option<Project>> {
    crate::conversation_store::global_store()?.load_project(id)
}

pub fn load_project_conversation_metas(
    project_id: &str,
    cursor: Option<(i64, String)>,
    limit: i64,
) -> Result<Vec<ConversationMeta>> {
    crate::conversation_store::global_store()?.load_project_metas(project_id, cursor, limit)
}

pub fn create_project(name: &str, workspace_root: &str) -> Result<ProjectCreationResult> {
    crate::conversation_store::global_store()?.create_project(name, workspace_root)
}

pub fn update_project(
    id: &str,
    name: Option<&str>,
    workspace_root: Option<&str>,
    is_pinned: Option<bool>,
    is_archived: Option<bool>,
) -> Result<Project> {
    crate::conversation_store::global_store()?.update_project(
        id,
        name,
        workspace_root,
        is_pinned,
        is_archived,
    )
}

pub fn delete_project(id: &str) -> Result<()> {
    crate::conversation_store::global_store()?.delete_project(id)
}

/// FTS-backed sidebar search (full message bodies + title/preview supplement).
pub fn search_conversations(query: &str, limit: i64) -> Result<Vec<ConversationSearchHit>> {
    crate::conversation_store::global_store()?.search_conversations(query, limit)
}

pub fn load_conversation_messages(conversation_id: &str) -> Result<Vec<ChatMessage>> {
    crate::conversation_store::global_store()?.load_messages(conversation_id)
}

pub fn save_conversations(list: &[Conversation]) -> Result<()> {
    crate::conversation_store::global_store()?.save_all(list)
}

pub fn save_conversation_meta(metas: &[ConversationMeta]) -> Result<()> {
    save_conversation_meta_with_platform_user(metas, None)
}

/// Persist conversation shell fields; optionally bind `session_user_id` when logged in.
pub fn save_conversation_meta_with_platform_user(
    metas: &[ConversationMeta],
    platform_user_id: Option<&str>,
) -> Result<()> {
    crate::conversation_store::global_store()?
        .save_meta_all_with_platform_user(metas, platform_user_id)
}

pub fn delete_conversation(conversation_id: &str) -> Result<()> {
    crate::conversation_store::global_store()?.delete_conversation(conversation_id)
}

/// Append messages whose ids are not yet in the DB (P0); does not delete existing rows.
pub fn append_conversation_messages(
    conversation_id: &str,
    messages: &[ChatMessage],
) -> Result<u32> {
    let store = crate::conversation_store::global_store()?;
    let written = store.append_missing_messages(conversation_id, messages)?;
    if written > 0 {
        let count = store.message_count(conversation_id)?;
        let preview = crate::conversation_store::conversation_preview(messages);
        store.flush_conversation_meta(conversation_id, count, &preview)?;
    }
    Ok(written)
}
