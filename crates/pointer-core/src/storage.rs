use crate::models::{
    ensure_agent_model_refs_have_provider, merge_user_platform, AgentModelRef, ChatMessage,
    Conversation, ConversationMeta, ConversationSearchHit, ModelRuntimeOverrides, ModelSettings,
    PlatformSettings, Project, ProjectCreationResult, ProjectCursor, ProjectPage, ProviderConfig,
    UserSettings,
};
use anyhow::{anyhow, Context, Result};
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

/// Test-only `~/.pointer` override. Same idea as [`TEST_APP_DATA_DIR`]: tests
/// point the pointer home at a temp dir **without** mutating `HOME` /
/// `POINTER_HOME`, which every other thread in the test process reads.
#[cfg(test)]
static TEST_POINTER_HOME: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

/// RAII override for [`pointer_home_dir`]; restores the previous value on drop
/// (even on panic). Take [`test_app_data_dir_lock`] first — both overrides are
/// process-global.
#[cfg(test)]
pub(crate) struct TestPointerHome {
    previous: Option<PathBuf>,
}

#[cfg(test)]
impl TestPointerHome {
    pub(crate) fn set(dir: impl Into<PathBuf>) -> Self {
        let mut slot = TEST_POINTER_HOME
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let previous = slot.replace(dir.into());
        Self { previous }
    }
}

#[cfg(test)]
impl Drop for TestPointerHome {
    fn drop(&mut self) {
        let mut slot = TEST_POINTER_HOME
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *slot = self.previous.take();
    }
}

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

/// Test hook: drop the override installed by [`set_test_app_data_dir`].
///
/// A test that installs an override **must** clear it before returning, even on
/// failure. `TEST_APP_DATA_DIR` is process-global, so a leftover temp path makes
/// unrelated tests resolve their storage into a directory that no longer exists
/// (observed: `webhook_attachment::tests::save_and_validate_roundtrip` passed
/// alone but failed in the full run).
#[cfg(test)]
pub fn clear_test_app_data_dir() {
    *TEST_APP_DATA_DIR.lock().unwrap() = None;
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

/// `~/.pointer` —— 跨安装共享的 Pointer home（技能、插件、AGENTS.md 都在下面）。
///
/// `POINTER_HOME` 可覆盖，供测试与多环境隔离使用。**只认非空值**：空串或纯空白
/// 一律退回真实主目录，否则一个写错的环境变量会把插件/技能目录指到进程 cwd。
///
/// 这里曾有三份各自实现：插件认 `POINTER_HOME`，技能与 AGENTS.md 不认，
/// 结果隔离环境下插件目录被隔离、企业目录缓存却读真实 home。
pub fn pointer_home_dir() -> Result<PathBuf> {
    #[cfg(test)]
    {
        if let Some(dir) = test_pointer_home_override() {
            std::fs::create_dir_all(&dir)?;
            return Ok(dir);
        }
    }
    resolve_pointer_home(std::env::var("POINTER_HOME").ok().as_deref())
}

/// 同 [`pointer_home_dir`]，但不创建目录（只判断路径是否存在时用）。
pub fn pointer_home_dir_no_create() -> Option<PathBuf> {
    #[cfg(test)]
    {
        if let Some(dir) = test_pointer_home_override() {
            return Some(dir);
        }
    }
    resolve_pointer_home_no_create(std::env::var("POINTER_HOME").ok().as_deref())
}

#[cfg(test)]
fn test_pointer_home_override() -> Option<PathBuf> {
    TEST_POINTER_HOME
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

/// 纯函数形式，便于不碰进程环境变量地测覆盖逻辑。
fn resolve_pointer_home(raw: Option<&str>) -> Result<PathBuf> {
    if let Some(dir) = resolve_pointer_home_no_create(raw) {
        std::fs::create_dir_all(&dir)?;
        return Ok(dir);
    }
    Err(anyhow!("无法解析用户主目录"))
}

fn resolve_pointer_home_no_create(raw: Option<&str>) -> Option<PathBuf> {
    if let Some(raw) = raw {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return Some(PathBuf::from(trimmed));
        }
    }
    dirs::home_dir().map(|home| home.join(".pointer"))
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

/// 按用户隔离的运行时设置目录（`data/user-settings/<uid>.json`）。
fn user_overlay_path(uid: &str) -> Result<PathBuf> {
    let segment = sanitize_storage_dir_segment(uid);
    Ok(data_dir()?.join("user-settings").join(format!("{segment}.json")))
}

/// 读取某个用户的运行时覆盖；文件不存在或损坏时返回默认（等于「没有覆盖」）。
pub fn load_user_runtime_overlay(uid: &str) -> crate::models::UserRuntimeOverlay {
    let uid = uid.trim();
    if uid.is_empty() {
        return Default::default();
    }
    let Ok(path) = user_overlay_path(uid) else {
        return Default::default();
    };
    let Ok(raw) = fs::read_to_string(&path) else {
        return Default::default();
    };
    serde_json::from_str(&raw).unwrap_or_else(|e| {
        log::warn!("user-settings: parse {} failed: {e}", path.display());
        Default::default()
    })
}

pub fn save_user_runtime_overlay(
    uid: &str,
    overlay: &crate::models::UserRuntimeOverlay,
) -> Result<()> {
    let uid = uid.trim();
    if uid.is_empty() {
        anyhow::bail!("user-settings: empty user id");
    }
    let path = user_overlay_path(uid)?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let raw = serde_json::to_string_pretty(overlay)?;
    // 临时文件 + rename：迁移会在写成功后清理全局，这里必须「要么旧内容、要么新
    // 内容」。半截写入既会被并发读成「没有覆盖」，崩溃后还会让该用户的值永久丢失。
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, raw)?;
    if let Err(err) = fs::rename(&tmp, &path) {
        let _ = fs::remove_file(&tmp);
        return Err(err.into());
    }
    Ok(())
}

/// 当前线程绑定的会话用户的覆盖（没有会话用户时为空）。
pub fn current_user_runtime_overlay() -> crate::models::UserRuntimeOverlay {
    match crate::session_user_env::current_session_user_id() {
        Some(uid) if !uid.trim().is_empty() => load_user_runtime_overlay(&uid),
        _ => Default::default(),
    }
}

/// 该用户是否已有覆盖文件。
fn user_overlay_exists(uid: &str) -> bool {
    user_overlay_path(uid).map(|p| p.exists()).unwrap_or(false)
}

/// 串行化「读全局文件 → 定向改 JSON → 落盘」。
///
/// 迁移是低频事件，但可能被多个读线程同时触发；没有这把锁，两个读-改-写会互相
/// 覆盖，甚至把刚清空的字段又写回来。
static GLOBAL_SETTINGS_WRITE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// 一次性迁移的收尾：把三项从全局 `user_settings.json` 里去掉并落盘，
/// 让它们不再充当所有人的共享默认值。
///
/// 定向改 JSON，不走 [`save_user_settings`]：那条会重新加密 provider key
/// （每次密文都不同）、丢掉平台来源 provider，不是幂等写。
/// 每次都从磁盘重新读，所以并发/重复调用不会把刚清空的字段写回来；
/// 写盘用临时文件 + rename，读方永远看到完整文件。
fn clear_legacy_runtime_settings_in_global() -> Result<()> {
    const LEGACY_KEYS: [&str; 3] = [
        "terminalEnvOverrides",
        "maxParallelToolCalls",
        "maxParallelSubAgents",
    ];
    let _guard = GLOBAL_SETTINGS_WRITE_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let path = user_settings_path()?;
    if !path.exists() {
        return Ok(());
    }
    let raw = fs::read_to_string(&path)?;
    let mut doc: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(doc) => doc,
        Err(e) => {
            log::warn!(
                "user-settings: clear legacy global runtime settings skipped (unparsable {}): {e}",
                path.display()
            );
            return Ok(());
        }
    };
    let Some(obj) = doc.as_object_mut() else {
        log::warn!(
            "user-settings: clear legacy global runtime settings skipped (not a json object): {}",
            path.display()
        );
        return Ok(());
    };
    let mut removed = false;
    for key in LEGACY_KEYS {
        if obj.remove(key).is_some() {
            removed = true;
        }
    }
    if !removed {
        return Ok(());
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(&doc)?)?;
    if let Err(err) = fs::rename(&tmp, &path) {
        let _ = fs::remove_file(&tmp);
        return Err(err.into());
    }
    log::info!(
        "user-settings: cleared legacy global runtime settings after migration ({})",
        path.display()
    );
    Ok(())
}

/// 把某个用户的覆盖合并进 `UserSettings`（就地）。
///
/// 一次性迁移：升级前三项存在全局文件里（谁最后写的算谁的）。第一个读到的
/// 用户把它落成自己的覆盖，之后全局那份不再参与。
pub fn apply_user_runtime_overlay(user: &mut UserSettings, uid: &str) {
    let uid = uid.trim();
    if uid.is_empty() {
        return;
    }
    let legacy_has_values = !user.terminal_env_overrides.is_empty()
        || user.max_parallel_tool_calls.is_some()
        || user.max_parallel_sub_agents.is_some();
    if legacy_has_values && !user_overlay_exists(uid) {
        let migrated = crate::models::UserRuntimeOverlay {
            terminal_env_overrides: Some(user.terminal_env_overrides.clone()),
            max_parallel_tool_calls: user.max_parallel_tool_calls,
            max_parallel_sub_agents: user.max_parallel_sub_agents,
        };
        match save_user_runtime_overlay(uid, &migrated) {
            Ok(()) => {
                // 迁移成功才清理全局：写失败时保持现状，下次读取还能重试。
                if let Err(err) = clear_legacy_runtime_settings_in_global() {
                    log::warn!(
                        "user-settings: clear legacy global runtime settings failed: {err:#}"
                    );
                }
                // 内存里也清空，否则调用方看到的仍是全局值。这个用户自己那份
                // 随后由下面的覆盖合并补回。
                user.terminal_env_overrides.clear();
                user.max_parallel_tool_calls = None;
                user.max_parallel_sub_agents = None;
            }
            Err(err) => {
                log::warn!("user-settings: migrate legacy runtime settings failed: {err:#}");
            }
        }
    }
    let overlay = load_user_runtime_overlay(uid);
    if let Some(v) = overlay.terminal_env_overrides {
        user.terminal_env_overrides = v;
    }
    if let Some(v) = overlay.max_parallel_tool_calls {
        user.max_parallel_tool_calls = Some(v);
    }
    if let Some(v) = overlay.max_parallel_sub_agents {
        user.max_parallel_sub_agents = Some(v);
    }
}

/// 把某个用户的覆盖合并进已合并好的 `ModelSettings`（就地）。
pub fn apply_user_runtime_overlay_to_model_settings(
    settings: &mut crate::models::ModelSettings,
    uid: &str,
) {
    let overlay = load_user_runtime_overlay(uid);
    if let Some(v) = overlay.terminal_env_overrides {
        settings.terminal_env_overrides = v;
    }
    if let Some(v) = overlay.max_parallel_tool_calls {
        settings.max_parallel_tool_calls = Some(v);
    }
    if let Some(v) = overlay.max_parallel_sub_agents {
        settings.max_parallel_sub_agents = Some(v);
    }
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
        default = "default_max_children_per_agent",
        rename = "maxChildrenPerAgent"
    )]
    max_children_per_agent: u32,
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
        default = "default_computer_delegation",
        rename = "computerDelegation"
    )]
    computer_delegation: String,
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

fn default_max_children_per_agent() -> u32 {
    crate::models::max_children_per_agent_ceiling()
}

fn default_raw_content_view_enabled() -> bool {
    true
}

fn default_theme() -> String {
    "system".into()
}

fn default_computer_human_like() -> bool {
    true
}

fn default_computer_delegation() -> String {
    crate::models::default_computer_delegation()
}

fn default_debug_dump_llm_prompts() -> bool {
    false
}

fn default_debug_menus_enabled() -> bool {
    false
}

/// Run once per process: migrate legacy `settings.json` theme → `user_settings.json`,
/// then lift a legacy `contextBudgetTokens` on disk.
pub fn ensure_legacy_settings_migrated() {
    LEGACY_MIGRATION_ONCE.call_once(|| {
        if let Err(e) = migrate_legacy_settings_if_needed() {
            log::warn!("storage: legacy settings migration failed: {e}");
        }
        if let Err(e) = migrate_legacy_context_budget_on_disk() {
            log::warn!("storage: legacy context budget migration failed: {e}");
        }
    });
}

/// Rewrite a legacy `contextBudgetTokens` on disk, once per process.
///
/// `contextBudgetTokens` used to default to 100k–120k. `migrate_legacy_context_budget_tokens`
/// lifts those to 256k, but it only ever ran on the merged **in-memory** copy:
/// `effective_settings()` calls `finalize_merged_settings` on every LLM round, so a legacy value
/// on disk was re-migrated — and re-logged at `info` — once per round, hundreds of times per
/// session, while the file never changed. One real 11-minute session logged it 897 times.
///
/// This edits the JSON in place instead of round-tripping through `UserSettings`:
/// `load_user_settings` decrypts `enc:v1:` provider keys, so writing that struct back would put
/// plaintext keys on disk, and `save_user_settings` calls this function's own `Once` cell
/// (`ensure_legacy_settings_migrated`) and would deadlock. Only one number changes here.
fn migrate_legacy_context_budget_on_disk() -> Result<()> {
    let path = user_settings_path()?;
    if !path.exists() {
        return Ok(());
    }
    let raw = fs::read_to_string(&path)?;
    let Ok(mut doc) = serde_json::from_str::<serde_json::Value>(&raw) else {
        // Unparseable file: `load_user_settings` already falls back to defaults, and
        // rewriting it here would destroy whatever the user has.
        return Ok(());
    };
    let Some((from, to)) = migrate_context_budget_field(&mut doc) else {
        return Ok(());
    };
    log::info!("storage: migrate legacy context_budget_tokens on disk {from} -> {to}");
    fs::write(&path, serde_json::to_vec_pretty(&doc)?)?;
    Ok(())
}

/// Lift `contextBudgetTokens` in a raw `user_settings.json` document.
///
/// Returns the `(from, to)` pair when a change was made, `None` when the field is
/// absent, not a number, or already current. Split out from the file IO so the
/// rewrite can be tested without touching the real settings path.
fn migrate_context_budget_field(doc: &mut serde_json::Value) -> Option<(u32, u32)> {
    let current = doc.get("contextBudgetTokens")?.as_u64()? as u32;
    let migrated = crate::models::migrate_legacy_context_budget_tokens(current);
    if migrated == current {
        return None;
    }
    doc["contextBudgetTokens"] = serde_json::json!(migrated);
    Some((current, migrated))
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

/// 读取全局 `user_settings.json`（不含任何用户覆盖）。
/// 读取用户设置：全局文件 + 当前线程会话用户的运行时覆盖。
///
/// 没有会话用户（桌面端、启动期）时等价于 [`load_user_settings_global`]。
pub fn load_user_settings() -> Result<UserSettings> {
    let mut user = load_user_settings_global()?;
    if let Some(uid) = crate::session_user_env::current_session_user_id() {
        if !uid.trim().is_empty() {
            apply_user_runtime_overlay(&mut user, uid.trim());
        }
    }
    Ok(user)
}

pub fn load_user_settings_global() -> Result<UserSettings> {
    ensure_legacy_settings_migrated();
    let path = user_settings_path()?;
    if !path.exists() {
        return Ok(UserSettings::default());
    }
    let raw = fs::read_to_string(&path)?;
    let mut user: UserSettings = serde_json::from_str(&raw).unwrap_or_default();
    user.media_oss = Default::default();
    // Normalize older user_settings.json: drop legacy placeholders and the
    // compiled-in computer tier seed. User-owned customizations are preserved.
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
    // 运行时设置按用户存（`user-settings/<uid>.json`），全局文件不保存：
    // 否则一个用户的值会变成所有人的默认值。
    to_save.terminal_env_overrides.clear();
    to_save.max_parallel_tool_calls = None;
    to_save.max_parallel_sub_agents = None;
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
        // 旧配置里的 workspaceRoot 不再迁移：工作区是会话级概念。
        lead_agent_id: stored.lead_agent_id.clone(),
        context_compression_enabled: true,
        context_budget_tokens: stored.context_budget_tokens,
        context_keep_recent_user_turns: stored.context_keep_recent_user_turns,
        context_summary_max_tokens: stored.context_summary_max_tokens,
        max_tool_rounds: stored.max_tool_rounds,
        max_sub_agent_tool_rounds: stored.max_sub_agent_tool_rounds,
        max_sub_agent_spawn_depth: stored.max_sub_agent_spawn_depth,
        max_children_per_agent: crate::models::clamp_max_children_per_agent(
            stored.max_children_per_agent,
        ),
        raw_content_view_enabled: stored.raw_content_view_enabled,
        debug_dump_llm_prompts: stored.debug_dump_llm_prompts,
        debug_menus_enabled: stored.debug_menus_enabled,
        agent_default_models: normalize_disk_agent_defaults(&stored.agent_default_models, &active),
        agent_task_board_history_trim: stored.agent_task_board_history_trim.clone(),
        computer_human_like: stored.computer_human_like,
        computer_delegation: stored.computer_delegation.clone(),
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
            workspace_root: s.workspace_root,
            lead_agent_id: s.lead_agent_id,
            context_compression_enabled: s.context_compression_enabled,
            context_budget_tokens: s.context_budget_tokens,
            context_keep_recent_user_turns: s.context_keep_recent_user_turns,
            context_summary_max_tokens: s.context_summary_max_tokens,
            max_tool_rounds: s.max_tool_rounds,
            max_sub_agent_tool_rounds: s.max_sub_agent_tool_rounds,
            max_sub_agent_spawn_depth: s.max_sub_agent_spawn_depth,
            max_children_per_agent: s.max_children_per_agent,
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
            computer_delegation: s.computer_delegation.clone(),
            theme: s.theme.clone(),
            agent_ui_overrides: s.agent_ui_overrides.clone(),
            legacy_reasoning_in_messages: None,
        }
    }
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

pub fn set_message_milestone(
    scope: &crate::conversation_store::ListScope,
    conversation_id: &str,
    message_id: &str,
    milestone: bool,
) -> Result<()> {
    crate::conversation_store::global_store()?.set_message_milestone(
        scope,
        conversation_id,
        message_id,
        milestone,
    )
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

/// One-shot migration of the legacy frontend turn-elapsed blob into
/// `messages.turn_elapsed_ms`. Idempotent and first-write-wins — see
/// [`crate::conversation_store::ConversationStore::backfill_turn_elapsed_ms`].
pub fn backfill_turn_elapsed_ms(
    entries: Vec<crate::conversation_store::TurnElapsedBackfillEntry>,
) -> Result<crate::conversation_store::TurnElapsedBackfillReport> {
    crate::conversation_store::global_store()?.backfill_turn_elapsed_ms(entries)
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

#[cfg(test)]
mod legacy_context_budget_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn lifts_a_legacy_value_and_reports_the_change() {
        let mut doc = json!({ "contextBudgetTokens": 122_880, "theme": "dark" });
        assert_eq!(
            migrate_context_budget_field(&mut doc),
            Some((122_880, 256 * 1024))
        );
        assert_eq!(doc["contextBudgetTokens"], json!(256 * 1024));
        // Only the one number changes.
        assert_eq!(doc["theme"], json!("dark"));
    }

    #[test]
    fn leaves_an_already_migrated_value_alone() {
        // The whole point: a second run must be a no-op, so the load path stops
        // re-migrating (and re-logging) on every LLM round.
        let mut doc = json!({ "contextBudgetTokens": 256 * 1024 });
        assert_eq!(migrate_context_budget_field(&mut doc), None);
    }

    #[test]
    fn a_deliberate_non_legacy_value_is_preserved() {
        let mut doc = json!({ "contextBudgetTokens": 200_000 });
        assert_eq!(migrate_context_budget_field(&mut doc), None);
        assert_eq!(doc["contextBudgetTokens"], json!(200_000));
    }

    #[test]
    fn missing_or_non_numeric_field_is_ignored() {
        let mut absent = json!({ "theme": "dark" });
        assert_eq!(migrate_context_budget_field(&mut absent), None);

        let mut text = json!({ "contextBudgetTokens": "262144" });
        assert_eq!(migrate_context_budget_field(&mut text), None);
        assert_eq!(text["contextBudgetTokens"], json!("262144"));
    }

    /// The end-to-end shape: a legacy file on disk becomes a current one, and the
    /// encrypted provider key survives untouched.
    ///
    /// That second half is the reason this rewrites the raw JSON instead of loading
    /// `UserSettings` and saving it back — `load_user_settings` decrypts `enc:v1:`
    /// keys, so a struct round-trip would put plaintext on disk.
    #[test]
    fn rewrites_the_file_and_leaves_encrypted_keys_alone() {
        let _guard = test_app_data_dir_lock();
        let dir = std::env::temp_dir().join(format!(
            "pointer-legacy-ctx-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("user_settings.json");
        std::fs::write(
            &path,
            serde_json::to_vec_pretty(&json!({
                "contextBudgetTokens": 122_880,
                "providers": [{ "id": "qwen", "apiKey": "enc:v1:ZmFrZQ==" }]
            }))
            .unwrap(),
        )
        .expect("write");
        set_test_app_data_dir(dir.clone());

        migrate_legacy_context_budget_on_disk().expect("migrate");

        let after: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("read")).expect("parse");
        assert_eq!(after["contextBudgetTokens"], json!(256 * 1024));
        assert_eq!(after["providers"][0]["apiKey"], json!("enc:v1:ZmFrZQ=="));

        // Idempotent: a second call must not rewrite the file.
        let mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
        std::thread::sleep(std::time::Duration::from_millis(20));
        migrate_legacy_context_budget_on_disk().expect("second migrate");
        let mtime_after = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
        assert_eq!(mtime, mtime_after, "second run must be a no-op");

        let _ = std::fs::remove_dir_all(&dir);
        set_test_app_data_dir(PathBuf::new());
    }

    /// Manual verification against the real `user_settings.json`.
    ///
    /// Ignored by default because it touches the developer's own settings file;
    /// run with `cargo test --release -p pointer-core --lib -- --ignored
    /// migrate_the_real_user_settings_file`. It only ever lifts a legacy
    /// `contextBudgetTokens` to the current default, which is what app startup
    /// does anyway — but it is a real file, so it stays opt-in.
    #[test]
    #[ignore]
    fn migrate_the_real_user_settings_file() {
        let path = user_settings_path().expect("settings path");
        let before: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("read")).expect("parse");
        let before_value = before["contextBudgetTokens"].clone();
        migrate_legacy_context_budget_on_disk().expect("migrate");
        let after: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("read")).expect("parse");
        println!(
            "contextBudgetTokens: {} -> {}",
            before_value, after["contextBudgetTokens"]
        );
        assert!(
            after["contextBudgetTokens"].as_u64().unwrap() != 122_880,
            "legacy value must be gone"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pointer_home_honors_override() {
        let tmp = tempfile::tempdir().unwrap();
        let raw = tmp.path().to_string_lossy().to_string();
        let dir = resolve_pointer_home(Some(&raw)).unwrap();
        assert_eq!(dir, tmp.path(), "POINTER_HOME 生效时应直接用它");
        assert!(dir.is_dir(), "应当建好目录");
    }

    #[test]
    fn pointer_home_trims_whitespace() {
        let tmp = tempfile::tempdir().unwrap();
        let raw = format!("  {}  ", tmp.path().to_string_lossy());
        let dir = resolve_pointer_home(Some(&raw)).unwrap();
        assert_eq!(dir, tmp.path(), "两端空白要吃掉，否则会建成一个带空格的怪目录");
    }

    #[test]
    fn empty_override_falls_back_to_real_home() {
        for raw in ["", "   ", "\t\n"] {
            assert_eq!(
                resolve_pointer_home(Some(raw)).unwrap(),
                dirs::home_dir().unwrap().join(".pointer"),
                "空值 {raw:?} 应退回 ~/.pointer"
            );
        }
        assert_eq!(
            resolve_pointer_home(None).unwrap(),
            dirs::home_dir().unwrap().join(".pointer")
        );
    }

    /// 技能、插件、AGENTS.md 三处必须解析到同一个 pointer home。
    ///
    /// 曾经是三份各自实现：插件认 `POINTER_HOME`，技能与 AGENTS.md 不认。
    /// 后果是隔离环境下插件目录被隔离、企业目录缓存却读真实 `~/.pointer`，
    /// 真机验证时企业插件区整块不出现，排查方向被带偏。
    #[test]
    fn all_modules_resolve_the_same_pointer_home() {
        // Injected pointer home: no `POINTER_HOME`/`HOME` mutation, so a
        // concurrent test cannot observe (or move) this root.
        let _lock = test_app_data_dir_lock();
        let tmp = tempfile::tempdir().unwrap();
        let _home = TestPointerHome::set(tmp.path());

        assert_eq!(pointer_home_dir().unwrap(), tmp.path());
        assert_eq!(
            crate::skills::external::pointer_home_dir().unwrap(),
            tmp.path(),
            "技能侧必须跟着 POINTER_HOME 走"
        );
        assert_eq!(
            crate::plugins::pointer_home_dir().unwrap(),
            tmp.path(),
            "插件侧必须跟着 POINTER_HOME 走"
        );
        assert_eq!(
            crate::skills::org_sync::plugins_root().unwrap(),
            tmp.path().join(crate::plugins::USER_PLUGINS_DIR_NAME),
            "企业下发的插件目录（含目录缓存）也必须在同一个 home 下"
        );
    }

    #[test]
    fn no_create_variant_does_not_touch_disk() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("not-created-yet");
        let raw = target.to_string_lossy().to_string();
        assert_eq!(resolve_pointer_home_no_create(Some(&raw)), Some(target.clone()));
        assert!(!target.exists(), "no_create 变体不该建目录");
    }

    #[test]
    fn override_beats_home_for_both_variants() {
        let tmp = tempfile::tempdir().unwrap();
        let raw = tmp.path().to_string_lossy().to_string();
        assert_eq!(
            resolve_pointer_home(Some(&raw)).unwrap(),
            resolve_pointer_home_no_create(Some(&raw)).unwrap()
        );
    }
}

#[cfg(test)]
mod user_overlay_tests {
    use super::*;

    /// 覆盖是进程级数据目录，测试必须串行并还原。
    fn with_temp_data_dir<F: FnOnce()>(f: F) {
        let _lock = test_app_data_dir_lock();
        let dir = std::env::temp_dir().join(format!(
            "pointer-overlay-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::create_dir_all(&dir);
        set_test_app_data_dir(dir.clone());
        f();
        clear_test_app_data_dir();
        let _ = fs::remove_dir_all(&dir);
    }

    /// 两个用户各自保存覆盖，互不覆盖；全局文件保持不动。
    #[test]
    fn overlays_are_isolated_per_user() {
        with_temp_data_dir(|| {
            let a = crate::models::UserRuntimeOverlay {
                terminal_env_overrides: Some(
                    [("PATH".to_string(), "/a/bin".to_string())].into_iter().collect(),
                ),
                max_parallel_tool_calls: Some(1),
                max_parallel_sub_agents: None,
            };
            let b = crate::models::UserRuntimeOverlay {
                terminal_env_overrides: None,
                max_parallel_tool_calls: Some(8),
                max_parallel_sub_agents: Some(2),
            };
            save_user_runtime_overlay("user-a", &a).expect("save a");
            save_user_runtime_overlay("user-b", &b).expect("save b");

            let loaded_a = load_user_runtime_overlay("user-a");
            let loaded_b = load_user_runtime_overlay("user-b");
            assert_eq!(
                loaded_a.terminal_env_overrides.as_ref().and_then(|m| m.get("PATH")),
                Some(&"/a/bin".to_string())
            );
            assert_eq!(loaded_a.max_parallel_tool_calls, Some(1));
            assert!(loaded_b.terminal_env_overrides.is_none());
            assert_eq!(loaded_b.max_parallel_tool_calls, Some(8));
            assert_eq!(loaded_b.max_parallel_sub_agents, Some(2));
        });
    }

    /// 全局文件不再保存这三项：保存后读回来必须是空，且覆盖文件才是唯一来源。
    #[test]
    fn global_file_never_stores_runtime_settings() {
        with_temp_data_dir(|| {
            let mut user = UserSettings::default();
            user.terminal_env_overrides =
                [("PATH".to_string(), "/global/bin".to_string())].into_iter().collect();
            user.max_parallel_tool_calls = Some(7);
            user.max_parallel_sub_agents = Some(3);
            save_user_settings(&user).expect("save");

            let raw = load_user_settings_global().expect("load");
            assert!(raw.terminal_env_overrides.is_empty());
            assert_eq!(raw.max_parallel_tool_calls, None);
            assert_eq!(raw.max_parallel_sub_agents, None);
        });
    }

    /// 升级前这三项存在全局文件里：第一个读到的用户把它落成自己的覆盖。
    #[test]
    fn legacy_global_values_migrate_to_first_reader() {
        with_temp_data_dir(|| {
            let mut user = UserSettings::default();
            user.max_parallel_tool_calls = Some(5);
            user.terminal_env_overrides = [("LANG".to_string(), "C.UTF-8".to_string())]
                .into_iter()
                .collect();

            save_user_runtime_overlay(
                "user-a",
                &crate::models::UserRuntimeOverlay {
                    terminal_env_overrides: Some(
                        [("PATH".to_string(), "/a".to_string())].into_iter().collect(),
                    ),
                    max_parallel_tool_calls: Some(1),
                    max_parallel_sub_agents: None,
                },
            )
            .expect("save");

            // user-b 没有覆盖文件 → 迁移全局值，成为 user-b 自己的覆盖
            apply_user_runtime_overlay(&mut user, "user-b");
            assert_eq!(user.max_parallel_tool_calls, Some(5));
            assert_eq!(user.terminal_env_overrides.get("LANG"), Some(&"C.UTF-8".to_string()));
            assert!(user_overlay_exists("user-b"));

            // user-a 有自己的覆盖 → 用自己那份，不受迁移影响
            apply_user_runtime_overlay(&mut user, "user-a");
            assert_eq!(user.max_parallel_tool_calls, Some(1));
            assert_eq!(user.terminal_env_overrides.get("PATH"), Some(&"/a".to_string()));

            // 全局已空的新用户拿到默认值（不再有人人共享的默认）
            let mut fresh = UserSettings::default();
            apply_user_runtime_overlay(&mut fresh, "user-c");
            assert_eq!(fresh.max_parallel_tool_calls, None);
            assert!(fresh.terminal_env_overrides.is_empty());
        });
    }
    /// 迁移成功 → 全局文件里三项消失、其他字段原样保留；迁移者本人仍读到自己的值。
    #[test]
    fn migration_clears_legacy_global_runtime_settings() {
        with_temp_data_dir(|| {
            let mut legacy = UserSettings::default();
            legacy.theme = "dark".into();
            legacy.ui_locale = "en".into();
            legacy.max_parallel_tool_calls = Some(5);
            legacy.max_parallel_sub_agents = Some(4);
            legacy.terminal_env_overrides = [("LANG".to_string(), "C.UTF-8".to_string())]
                .into_iter()
                .collect();
            write_user_settings_file(&legacy).expect("write legacy global");

            let mut user = load_user_settings_global().expect("load global");
            apply_user_runtime_overlay(&mut user, "first-user");

            // 迁移落成该用户自己的覆盖，本人仍读到这些值
            let overlay = load_user_runtime_overlay("first-user");
            assert_eq!(overlay.max_parallel_tool_calls, Some(5));
            assert_eq!(overlay.max_parallel_sub_agents, Some(4));
            assert_eq!(
                overlay
                    .terminal_env_overrides
                    .as_ref()
                    .and_then(|m| m.get("LANG")),
                Some(&"C.UTF-8".to_string())
            );
            assert_eq!(user.max_parallel_tool_calls, Some(5));
            assert_eq!(user.max_parallel_sub_agents, Some(4));

            // 全局文件：三项消失，其他字段原样
            let raw = fs::read_to_string(user_settings_path().expect("path")).expect("read global");
            let doc: serde_json::Value = serde_json::from_str(&raw).expect("global json");
            assert!(
                doc.get("terminalEnvOverrides").is_none(),
                "global must not keep terminalEnvOverrides: {doc}"
            );
            assert!(
                doc.get("maxParallelToolCalls").is_none(),
                "global must not keep maxParallelToolCalls: {doc}"
            );
            assert!(
                doc.get("maxParallelSubAgents").is_none(),
                "global must not keep maxParallelSubAgents: {doc}"
            );
            assert_eq!(doc.get("theme").and_then(|v| v.as_str()), Some("dark"));
            assert_eq!(doc.get("uiLocale").and_then(|v| v.as_str()), Some("en"));

            // 重新读全局：三项回到默认值
            let reread = load_user_settings_global().expect("reload global");
            assert!(reread.terminal_env_overrides.is_empty());
            assert_eq!(reread.max_parallel_tool_calls, None);
            assert_eq!(reread.max_parallel_sub_agents, None);
        });
    }

    /// 迁移之后，第二个用户拿不到第一个用户的值，也拿不到全局那份。
    #[test]
    fn second_user_gets_defaults_after_legacy_migration() {
        with_temp_data_dir(|| {
            let mut legacy = UserSettings::default();
            legacy.max_parallel_tool_calls = Some(5);
            legacy.max_parallel_sub_agents = Some(4);
            legacy.terminal_env_overrides = [("LANG".to_string(), "C.UTF-8".to_string())]
                .into_iter()
                .collect();
            write_user_settings_file(&legacy).expect("write legacy global");

            let mut first = load_user_settings_global().expect("load global");
            apply_user_runtime_overlay(&mut first, "first-user");
            assert_eq!(first.max_parallel_tool_calls, Some(5));

            let mut second = load_user_settings_global().expect("load global");
            apply_user_runtime_overlay(&mut second, "second-user");
            assert!(second.terminal_env_overrides.is_empty());
            assert_eq!(second.max_parallel_tool_calls, None);
            assert_eq!(second.max_parallel_sub_agents, None);
            assert!(!user_overlay_exists("second-user"));
        });
    }

    /// 覆盖写入失败 → 全局不被清理，读取不 panic（下次读取还能重试）。
    #[test]
    fn failed_overlay_write_keeps_legacy_global_runtime_settings() {
        with_temp_data_dir(|| {
            let mut legacy = UserSettings::default();
            legacy.max_parallel_tool_calls = Some(5);
            legacy.terminal_env_overrides = [("LANG".to_string(), "C.UTF-8".to_string())]
                .into_iter()
                .collect();
            write_user_settings_file(&legacy).expect("write legacy global");

            // 注入不可写：`user-settings` 先占成普通文件，create_dir_all 必然失败。
            let overlay_dir = user_settings_path()
                .expect("path")
                .parent()
                .expect("data dir")
                .join("user-settings");
            fs::write(&overlay_dir, b"not a dir").expect("block overlay dir");

            let mut user = load_user_settings_global().expect("load global");
            apply_user_runtime_overlay(&mut user, "blocked-user");

            // 迁移失败：全局保持现状
            let raw = fs::read_to_string(user_settings_path().expect("path")).expect("read global");
            let doc: serde_json::Value = serde_json::from_str(&raw).expect("global json");
            assert_eq!(
                doc.get("maxParallelToolCalls").and_then(|v| v.as_u64()),
                Some(5),
                "global must keep legacy values when migration failed: {doc}"
            );
            assert!(
                doc.get("terminalEnvOverrides").is_some(),
                "global must keep legacy env overrides when migration failed: {doc}"
            );

            // 读取没被阻断，内存里仍是全局值
            assert_eq!(user.max_parallel_tool_calls, Some(5));
            assert_eq!(user.terminal_env_overrides.get("LANG"), Some(&"C.UTF-8".to_string()));
            assert!(!user_overlay_exists("blocked-user"));
        });
    }
}
