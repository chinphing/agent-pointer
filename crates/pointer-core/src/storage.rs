use crate::models::{
    ensure_agent_model_refs_have_provider, ensure_provider_generation_defaults,
    ensure_provider_model_capability_defaults, merge_user_platform, AgentModelRef, ChatMessage,
    Conversation, ConversationMeta, ConversationSearchHit, ModelRuntimeOverrides, ModelSettings,
    PlatformSettings, Project, ProjectCreationResult, ProjectCursor, ProjectPage, ProviderConfig,
    UserSettings,
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

#[cfg(test)]
static TEST_APP_DATA_DIR: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

#[cfg(test)]
static TEST_APP_DATA_DIR_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Serialize tests that replace the process-global app data directory.
#[cfg(test)]
pub fn test_app_data_dir_lock() -> std::sync::MutexGuard<'static, ()> {
    // 容忍 panic 污染：单测断言失败不应让后续测试连锁 PoisonError。
    TEST_APP_DATA_DIR_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Test hook: force `data_dir()` to a temp dir for the current process.
/// Must be called before the first `data_dir()` resolution in the process.
#[cfg(test)]
pub fn set_test_app_data_dir(dir: PathBuf) {
    *TEST_APP_DATA_DIR.lock().unwrap() = Some(dir);
    // Clear the once-cell so data_dir() re-resolves to the test dir.
    // OnceLock has no public reset; a private raw pointer swap is unsafe, so
    // instead we re-check TEST_APP_DATA_DIR first inside data_dir().
}

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
    #[cfg(test)]
    {
        if let Some(dir) = TEST_APP_DATA_DIR.lock().unwrap().clone() {
            return Ok(dir);
        }
    }
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

/// Inject `DATA_DIR` into a terminal child env map (Pointer app data root).
/// Same directory as conversations / sandboxes / skills under the host.
pub fn apply_data_dir_env(env: &mut HashMap<String, String>) {
    match app_data_dir() {
        Ok(dir) => {
            let path = dir.to_string_lossy();
            if !path.is_empty() {
                env.insert("DATA_DIR".into(), path.into_owned());
            }
        }
        Err(e) => {
            log::warn!("DATA_DIR: app_data_dir unavailable for terminal child: {e:#}");
        }
    }
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

/// Read-only model directory returned by the platform login API. Kept separate
/// from user settings so platform-owned catalog entries never become editable
/// user configuration.
fn platform_model_catalog_cache_path() -> Result<PathBuf> {
    Ok(data_dir()?.join("platform_model_catalog.json"))
}
fn auth_dat_path() -> Result<PathBuf> {
    Ok(data_dir()?.join("auth.dat"))
}
fn key_path() -> Result<PathBuf> {
    Ok(data_dir()?.join("key.dat"))
}

#[derive(Debug, Deserialize, Serialize)]
struct PlatformModelCatalogCache {
    catalog: HashMap<String, Vec<String>>,
    #[serde(default)]
    providers: Vec<crate::platform_auth::PlatformProviderTemplate>,
    #[serde(default)]
    tier_defaults: serde_json::Value,
    #[serde(default)]
    hash: Option<String>,
}

/// 完整平台目录缓存（模型名目录 + 服务商模板 + 档位默认 + 版本）。
pub struct PlatformModelCatalogCacheData {
    pub catalog: HashMap<String, Vec<String>>,
    pub providers: Vec<crate::platform_auth::PlatformProviderTemplate>,
    pub tier_defaults: serde_json::Value,
    pub hash: Option<String>,
}

fn platform_model_catalog_hash(catalog: &HashMap<String, Vec<String>>) -> String {
    let canonical: std::collections::BTreeMap<_, _> = catalog.iter().collect();
    let bytes = serde_json::to_vec(&canonical).expect("catalog map is JSON-safe");
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}

/// Load the full platform-owned directory cache (models + provider templates + tier defaults).
/// Legacy cache files containing only provider -> models remain supported.
pub fn load_platform_model_catalog_cache_full() -> Result<PlatformModelCatalogCacheData> {
    let path = platform_model_catalog_cache_path()?;
    if !path.exists() {
        return Ok(PlatformModelCatalogCacheData {
            catalog: HashMap::new(),
            providers: Vec::new(),
            tier_defaults: serde_json::Value::Null,
            hash: None,
        });
    }
    let raw = fs::read_to_string(&path)?;
    let parsed = serde_json::from_str::<PlatformModelCatalogCache>(&raw)
        .map(|entry| {
            (
                entry.catalog,
                entry.providers,
                entry.tier_defaults,
                entry.hash,
            )
        })
        .or_else(|_| {
            serde_json::from_str::<HashMap<String, Vec<String>>>(&raw)
                .map(|catalog| (catalog, Vec::new(), serde_json::Value::Null, None))
        });
    match parsed {
        Ok((catalog, providers, tier_defaults, hash)) => {
            let hash = hash
                .or_else(|| (!catalog.is_empty()).then(|| platform_model_catalog_hash(&catalog)));
            Ok(PlatformModelCatalogCacheData {
                catalog,
                providers,
                tier_defaults,
                hash,
            })
        }
        Err(err) => {
            log::warn!("storage: ignoring malformed platform model catalog cache: {err}");
            Ok(PlatformModelCatalogCacheData {
                catalog: HashMap::new(),
                providers: Vec::new(),
                tier_defaults: serde_json::Value::Null,
                hash: None,
            })
        }
    }
}

/// Load the last non-empty platform-owned model catalog. Missing or malformed
/// cache data is treated as absent. This client builds providers from cached
/// `platformProviders` templates, not from the name-list catalog.
pub fn load_platform_model_catalog_cache() -> Result<HashMap<String, Vec<String>>> {
    Ok(load_platform_model_catalog_cache_full()?.catalog)
}

pub fn platform_model_catalog_cache_hash() -> Result<Option<String>> {
    Ok(load_platform_model_catalog_cache_full()?.hash)
}

/// Atomically replace the separate read-only platform model catalog cache.
/// This file deliberately does not share the user-settings write path.
pub fn save_platform_model_catalog_cache_full(
    catalog: &HashMap<String, Vec<String>>,
    providers: &Vec<crate::platform_auth::PlatformProviderTemplate>,
    tier_defaults: &serde_json::Value,
    hash: Option<&str>,
) -> Result<()> {
    if catalog.is_empty() && providers.is_empty() {
        return Ok(());
    }
    let path = platform_model_catalog_cache_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension(format!("json.{}.tmp", uuid::Uuid::new_v4()));
    let entry = PlatformModelCatalogCache {
        catalog: catalog.clone(),
        providers: providers.clone(),
        tier_defaults: tier_defaults.clone(),
        hash: hash.map(str::to_owned).or_else(|| {
            if !catalog.is_empty() {
                Some(platform_model_catalog_hash(catalog))
            } else {
                None
            }
        }),
    };
    fs::write(&tmp, serde_json::to_vec_pretty(&entry)?)?;
    fs::rename(&tmp, &path).with_context(|| {
        format!(
            "replace platform model catalog cache {} with {}",
            path.display(),
            tmp.display()
        )
    })?;
    Ok(())
}

pub fn save_platform_model_catalog_cache_with_hash(
    catalog: &HashMap<String, Vec<String>>,
    hash: Option<&str>,
) -> Result<()> {
    save_platform_model_catalog_cache_full(catalog, &Vec::new(), &serde_json::Value::Null, hash)
}

pub fn save_platform_model_catalog_cache(catalog: &HashMap<String, Vec<String>>) -> Result<()> {
    save_platform_model_catalog_cache_full(catalog, &Vec::new(), &serde_json::Value::Null, None)
}

#[derive(Debug, Serialize, Deserialize)]
struct StoredModelOverrides {
    #[serde(default, rename = "reasoningInMessages")]
    reasoning_in_messages: Option<bool>,
    #[serde(default)]
    temperature: Option<f32>,
    #[serde(default, rename = "topP")]
    top_p: Option<f32>,
    #[serde(default, rename = "maxTokens")]
    max_tokens: Option<u32>,
    #[serde(default, rename = "contextBudgetTokens")]
    context_budget_tokens: Option<u32>,
    #[serde(default, rename = "enableThinking")]
    enable_thinking: Option<bool>,
    #[serde(default, rename = "thinkingBudget")]
    thinking_budget: Option<u32>,
    #[serde(default, rename = "reasoningEffort")]
    reasoning_effort: Option<String>,
    #[serde(default, rename = "thinkingProtocol")]
    thinking_protocol: Option<String>,
    #[serde(default, rename = "thinkingIntensity")]
    thinking_intensity: Option<String>,
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
    #[serde(default, rename = "topP")]
    top_p: Option<f32>,
    #[serde(default, rename = "maxTokens")]
    max_tokens: Option<u32>,
    #[serde(default, rename = "contextBudgetTokens")]
    context_budget_tokens: Option<u32>,
    #[serde(default, rename = "enableThinking")]
    enable_thinking: Option<bool>,
    #[serde(default, rename = "thinkingBudget")]
    thinking_budget: Option<u32>,
    #[serde(default, rename = "reasoningEffort")]
    reasoning_effort: Option<String>,
    #[serde(default, rename = "thinkingProtocol")]
    thinking_protocol: Option<String>,
    #[serde(default, rename = "thinkingIntensity")]
    thinking_intensity: Option<String>,
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
    #[serde(
        default = "default_max_sub_agent_tool_rounds",
        rename = "maxSubAgentToolRounds"
    )]
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
    256 * 1024
}

fn default_context_keep_recent_user_turns() -> u32 {
    6
}

fn default_context_summary_max_tokens() -> u32 {
    2048
}

fn default_max_tool_rounds() -> u32 {
    5000
}

fn default_max_sub_agent_tool_rounds() -> u32 {
    500
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
    let mut user = stored_settings_to_user(&stored);
    if user.theme.trim().is_empty() {
        user.theme = default_theme();
    }
    write_user_settings_file(&user)?;
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
    // Backfill platform defaults that may be missing from older user_settings.json
    // (built-in provider model lists and per-agent default models). User-owned
    // customizations are preserved; only missing defaults are added.
    crate::models::ensure_user_settings_defaults(&mut user);
    // Decrypt provider keys that were persisted inside user_settings.json
    // (enc:v1:<base64>). Older data stored plaintext — keep it as-is.
    for provider in &mut user.providers {
        if let Some(rest) = provider.api_key.strip_prefix("enc:v1:") {
            let blob =
                match base64::Engine::decode(&base64::engine::general_purpose::STANDARD, rest) {
                    Ok(b) => b,
                    Err(e) => {
                        log::warn!(
                            "storage: provider {} key base64 decode failed: {e}; key cleared",
                            provider.id
                        );
                        provider.api_key.clear();
                        continue;
                    }
                };
            match crate::local_secret::decrypt_local_secret_with_info(
                &blob,
                b"provider-api-keys-v1",
            ) {
                Ok(key) => provider.api_key = key,
                Err(e) => {
                    log::warn!(
                        "storage: provider {} key decrypt failed: {e}; key cleared",
                        provider.id
                    );
                    provider.api_key.clear();
                }
            }
        }
    }
    // Legacy provider_keys.enc compat: backfill keys for providers whose json
    // entry has no key. Read-only — save_user_settings no longer writes this file.
    if let Ok(legacy_keys) = load_provider_api_keys() {
        for (provider_id, key) in legacy_keys {
            if let Some(provider) = user.providers.iter_mut().find(|p| p.id == provider_id) {
                if provider.api_key.is_empty() {
                    provider.api_key = key;
                }
            }
        }
    }
    Ok(user)
}

fn write_user_settings_file(user: &UserSettings) -> Result<()> {
    fs::write(user_settings_path()?, serde_json::to_vec_pretty(user)?)?;
    Ok(())
}

fn provider_keys_path() -> Result<PathBuf> {
    Ok(data_dir()?.join("provider_keys.enc"))
}

/// Legacy (read-only): decrypt user-typed provider keys saved by the old
/// `provider_keys.enc` sidecar. Kept for backward compat during the
/// key-in-json migration; new saves write keys into user_settings.json.
fn load_provider_api_keys() -> Result<Vec<(String, String)>> {
    let path = provider_keys_path()?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let blob = fs::read(&path)?;
    let payload =
        crate::local_secret::decrypt_local_secret_with_info(&blob, b"provider-api-keys-v1")
            .map_err(|e| {
                log::warn!(
                    "storage: provider_keys.enc decrypt failed: {e}; keys will need re-entry"
                );
                e
            })?;
    Ok(serde_json::from_str(&payload).unwrap_or_default())
}

/// Persist the full user settings layer (user configuration incl. debug fields).
/// Provider keys the user typed are encrypted (AES-256-GCM, machine-bound) and
/// stored inline in user_settings.json as `enc:v1:<base64>` on each provider's
/// apiKey. Platform-injected keys never pass through here — they live only in
/// platform memory (see `update_user_settings` filtering by source).
pub fn save_user_settings(user: &UserSettings) -> Result<()> {
    ensure_legacy_settings_migrated();
    let mut to_save = user.clone();
    to_save.media_oss = Default::default();
    to_save
        .providers
        .retain(|p| p.source.as_deref() != Some("platform"));
    // Encrypt non-empty, non-masked keys inline. "****" is the web redaction
    // sentinel and must not be persisted — it is re-attached from existing data
    // by update_user_settings before reaching here.
    for provider in &mut to_save.providers {
        if provider.api_key.trim().is_empty() || provider.api_key.trim() == "****" {
            provider.api_key.clear();
        } else if !provider.api_key.starts_with("enc:v1:") {
            let blob = crate::local_secret::encrypt_local_secret_with_info(
                &provider.api_key,
                b"provider-api-keys-v1",
            )?;
            provider.api_key = format!(
                "enc:v1:{}",
                base64::Engine::encode(&base64::engine::general_purpose::STANDARD, blob)
            );
        }
    }
    write_user_settings_file(&to_save)
}

/// Keep free-form `extraBody` keys after absorbing structured thinking fields.
fn leftover_extra_body(extra: Option<&serde_json::Value>) -> Option<serde_json::Value> {
    let Some(serde_json::Value::Object(map)) = extra else {
        return None;
    };
    let mut out = serde_json::Map::new();
    for (k, v) in map {
        if matches!(
            k.as_str(),
            "enable_thinking" | "thinking_budget" | "reasoning_effort" | "thinking"
        ) {
            continue;
        }
        out.insert(k.clone(), v.clone());
    }
    if out.is_empty() {
        None
    } else {
        Some(serde_json::Value::Object(out))
    }
}

fn stored_model_overrides_to_runtime(v: &StoredModelOverrides) -> ModelRuntimeOverrides {
    let mut enable_thinking = v.enable_thinking.or(v.thinking_enabled);
    let mut thinking_budget = v.thinking_budget;
    let mut reasoning_effort = v.reasoning_effort.clone();
    crate::models::absorb_legacy_extension_config(
        &mut enable_thinking,
        &mut thinking_budget,
        &mut reasoning_effort,
        None,
        None,
        v.extra_body.clone(),
    );
    ModelRuntimeOverrides {
        reasoning_in_messages: v.reasoning_in_messages,
        temperature: v.temperature,
        top_p: v.top_p,
        max_tokens: v.max_tokens,
        context_budget_tokens: v.context_budget_tokens,
        enable_thinking,
        thinking_budget,
        reasoning_effort,
        thinking_protocol: v.thinking_protocol.clone(),
        thinking_intensity: v.thinking_intensity.clone(),
        supports_vision: None,
        supports_audio: None,
        can_generate_image: None,
        can_generate_video: None,
        extra_body: leftover_extra_body(v.extra_body.as_ref()),
    }
}

fn stored_provider_to_platform(
    p: &StoredProvider,
    legacy_reasoning: Option<bool>,
) -> ProviderConfig {
    let mut enable_thinking = p.enable_thinking.or(p.thinking_enabled);
    let mut thinking_budget = p.thinking_budget;
    let mut reasoning_effort = p.reasoning_effort.clone();
    crate::models::absorb_legacy_extension_config(
        &mut enable_thinking,
        &mut thinking_budget,
        &mut reasoning_effort,
        None,
        None,
        p.extra_body.clone(),
    );
    ProviderConfig {
        id: p.id.clone(),
        name: p.name.clone(),
        base_url: p.base_url.clone(),
        api_key: p.api_key.clone(),
        models: p.models.clone(),
        reasoning_in_messages: p.reasoning_in_messages.or(legacy_reasoning),
        temperature: p.temperature,
        top_p: p.top_p,
        max_tokens: p.max_tokens,
        context_budget_tokens: p.context_budget_tokens,
        model_configs: p
            .model_configs
            .iter()
            .map(|(k, v)| (k.clone(), stored_model_overrides_to_runtime(v)))
            .collect(),
        enable_thinking,
        thinking_budget,
        reasoning_effort,
        thinking_protocol: p.thinking_protocol.clone(),
        thinking_intensity: p.thinking_intensity.clone(),
        extra_body: leftover_extra_body(p.extra_body.as_ref()),
        source: Some("user".into()),
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

fn stored_settings_to_user(stored: &StoredSettings) -> UserSettings {
    let legacy_reasoning = stored.legacy_reasoning_in_messages;
    let active = if stored.active_provider_id.trim().is_empty() {
        stored
            .providers
            .first()
            .map(|p| p.id.clone())
            .unwrap_or_default()
    } else {
        stored.active_provider_id.clone()
    };
    let mut user = UserSettings {
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
        context_compression_enabled: true,
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
        theme: stored.theme.clone(),
        agent_ui_overrides: stored.agent_ui_overrides.clone(),
        ..UserSettings::default()
    };
    let mut merged = merge_user_platform(&user, &PlatformSettings::default());
    ensure_agent_model_refs_have_provider(&mut merged);
    user.agent_default_models = merged.agent_default_models;
    user
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
                    top_p: p.top_p,
                    max_tokens: p.max_tokens,
                    context_budget_tokens: p.context_budget_tokens,
                    model_configs: p
                        .model_configs
                        .iter()
                        .map(|(k, v)| {
                            (
                                k.clone(),
                                StoredModelOverrides {
                                    reasoning_in_messages: v.reasoning_in_messages,
                                    temperature: v.temperature,
                                    top_p: v.top_p,
                                    max_tokens: v.max_tokens,
                                    context_budget_tokens: v.context_budget_tokens,
                                    enable_thinking: v.enable_thinking,
                                    thinking_budget: v.thinking_budget,
                                    reasoning_effort: v.reasoning_effort.clone(),
                                    thinking_protocol: v.thinking_protocol.clone(),
                                    thinking_intensity: v.thinking_intensity.clone(),
                                    extra_body: v.extra_body.clone(),
                                    thinking_enabled: None,
                                },
                            )
                        })
                        .collect(),
                    enable_thinking: p.enable_thinking,
                    thinking_budget: p.thinking_budget,
                    reasoning_effort: p.reasoning_effort.clone(),
                    thinking_protocol: p.thinking_protocol.clone(),
                    thinking_intensity: p.thinking_intensity.clone(),
                    extra_body: p.extra_body.clone(),
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
    scope: &crate::conversation_store::ListScope,
    cursor: Option<(i64, String)>,
    limit: i64,
) -> Result<Vec<ConversationMeta>> {
    crate::conversation_store::global_store()?.load_metas(scope, cursor, limit)
}

/// Single conversation meta by id, respecting sidebar [`ListScope`] visibility.
pub fn load_conversation_meta(
    scope: &crate::conversation_store::ListScope,
    id: &str,
) -> Result<Option<ConversationMeta>> {
    let id = id.trim();
    if id.is_empty() {
        return Ok(None);
    }
    let Some(meta) = crate::conversation_store::global_store()?.load_meta(id)? else {
        return Ok(None);
    };
    match scope.filter_uid() {
        None => Ok(Some(meta)),
        Some(uid) => {
            let stored =
                crate::conversation_store::normalize_session_user_id(&meta.session_user_id);
            if stored == uid {
                Ok(Some(meta))
            } else {
                log::info!("storage: load_conversation_meta denied id={id} (out of list scope)");
                Ok(None)
            }
        }
    }
}

pub fn load_projects(
    scope: &crate::conversation_store::ListScope,
    cursor: Option<ProjectCursor>,
    limit: i64,
) -> Result<ProjectPage> {
    crate::conversation_store::global_store()?.load_projects(scope, cursor, limit)
}

pub fn load_sidebar_projects(scope: &crate::conversation_store::ListScope) -> Result<Vec<Project>> {
    crate::conversation_store::global_store()?.load_sidebar_projects(scope)
}

pub fn load_project(
    id: &str,
    scope: &crate::conversation_store::ListScope,
) -> Result<Option<Project>> {
    crate::conversation_store::global_store()?.load_project(id, scope)
}

pub fn load_project_conversation_metas(
    project_id: &str,
    scope: &crate::conversation_store::ListScope,
    cursor: Option<(i64, String)>,
    limit: i64,
) -> Result<Vec<ConversationMeta>> {
    crate::conversation_store::global_store()?.load_project_metas(project_id, scope, cursor, limit)
}

pub fn create_project(
    name: &str,
    workspace_root: &str,
    session_user_id: &str,
) -> Result<ProjectCreationResult> {
    crate::conversation_store::global_store()?.create_project(name, workspace_root, session_user_id)
}

pub fn update_project(
    id: &str,
    session_user_id: &str,
    name: Option<&str>,
    workspace_root: Option<&str>,
    is_pinned: Option<bool>,
    is_archived: Option<bool>,
) -> Result<Project> {
    crate::conversation_store::global_store()?.update_project(
        id,
        session_user_id,
        name,
        workspace_root,
        is_pinned,
        is_archived,
    )
}

pub fn delete_project(id: &str, session_user_id: &str) -> Result<()> {
    crate::conversation_store::global_store()?.delete_project(id, session_user_id)
}

/// FTS-backed sidebar search (full message bodies + title/preview supplement).
pub fn search_conversations(
    scope: &crate::conversation_store::ListScope,
    query: &str,
    limit: i64,
) -> Result<Vec<ConversationSearchHit>> {
    crate::conversation_store::global_store()?.search_conversations(scope, query, limit)
}

pub fn list_conversation_search_matches(
    scope: &crate::conversation_store::ListScope,
    conversation_id: &str,
    query: &str,
) -> Result<Vec<crate::models::ConversationSearchMatch>> {
    crate::conversation_store::global_store()?.list_conversation_search_matches(
        scope,
        conversation_id,
        query,
    )
}

pub fn list_conversation_outline(
    scope: &crate::conversation_store::ListScope,
    conversation_id: &str,
) -> Result<Vec<crate::models::ConversationOutlineItem>> {
    crate::conversation_store::global_store()?.list_conversation_outline(scope, conversation_id)
}

pub fn load_conversation_messages(conversation_id: &str) -> Result<Vec<ChatMessage>> {
    crate::conversation_store::global_store()?.load_messages(conversation_id)
}

/// One transcript row by id. `Ok(None)` when the id is missing.
pub fn load_conversation_message(
    conversation_id: &str,
    message_id: &str,
) -> Result<Option<ChatMessage>> {
    let id = message_id.trim();
    if id.is_empty() {
        log::warn!(
            "storage: load_conversation_message skipped; empty message_id conversation_id={conversation_id}"
        );
        return Ok(None);
    }
    crate::conversation_store::global_store()?.load_message(conversation_id, id)
}

pub fn load_conversation_messages_page(
    conversation_id: &str,
    opts: &crate::conversation_store::LoadMessagesPageOpts,
) -> Result<crate::conversation_store::MessagePage> {
    crate::conversation_store::global_store()?.load_messages_page(conversation_id, opts)
}

pub fn load_scoped_sub_messages_for_trace(
    conversation_id: &str,
    anchor_message_id: &str,
    trace_id: &str,
    agent_instance_id: Option<&str>,
) -> Result<Vec<ChatMessage>> {
    crate::conversation_store::global_store()?.load_scoped_sub_messages_for_trace(
        conversation_id,
        anchor_message_id,
        trace_id,
        agent_instance_id,
    )
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
/// Returns the `{message_id, position}` rows actually written so the frontend can
/// attach SQLite positions to its in-memory messages after `persistAppend`.
pub fn append_conversation_messages(
    conversation_id: &str,
    messages: &[ChatMessage],
) -> Result<Vec<crate::conversation_store::AppendedMessageRow>> {
    crate::conversation_session::append_missing(conversation_id, messages)
}

#[cfg(test)]
mod platform_model_catalog_cache_tests {
    use super::*;

    #[test]
    fn platform_catalog_cache_round_trips_without_touching_user_settings() {
        let _lock = test_app_data_dir_lock();
        let dir = tempfile::tempdir().expect("temp data dir");
        set_test_app_data_dir(dir.path().to_path_buf());
        let catalog = HashMap::from([("aliyun_qwen".to_string(), vec!["qwen-next".to_string()])]);

        save_platform_model_catalog_cache(&catalog).expect("write catalog cache");

        assert_eq!(
            load_platform_model_catalog_cache().expect("read catalog cache"),
            catalog
        );
        assert!(platform_model_catalog_cache_path()
            .expect("cache path")
            .exists());
        assert!(
            !user_settings_path().expect("user settings path").exists(),
            "platform catalog cache must not create or modify user_settings.json"
        );
    }

    #[test]
    fn legacy_platform_catalog_cache_loads_with_computed_hash() {
        let _lock = test_app_data_dir_lock();
        let dir = tempfile::tempdir().expect("temp data dir");
        set_test_app_data_dir(dir.path().to_path_buf());
        let catalog = HashMap::from([("qwen".to_string(), vec!["qwen-plus".to_string()])]);
        fs::write(
            platform_model_catalog_cache_path().expect("cache path"),
            serde_json::to_vec(&catalog).expect("serialize legacy catalog"),
        )
        .expect("write legacy cache");

        let data = load_platform_model_catalog_cache_full().expect("load legacy cache");
        assert_eq!(data.catalog, catalog);
        assert_eq!(data.hash, Some(platform_model_catalog_hash(&catalog)));
        assert!(data.providers.is_empty());
        assert!(data.tier_defaults.is_null());
    }

    #[test]
    fn malformed_platform_catalog_cache_falls_back_to_empty_catalog() {
        let _lock = test_app_data_dir_lock();
        let dir = tempfile::tempdir().expect("temp data dir");
        set_test_app_data_dir(dir.path().to_path_buf());
        fs::write(
            platform_model_catalog_cache_path().expect("cache path"),
            "not json",
        )
        .expect("write malformed cache");

        assert!(load_platform_model_catalog_cache()
            .expect("malformed cache is recoverable")
            .is_empty());
    }
}
