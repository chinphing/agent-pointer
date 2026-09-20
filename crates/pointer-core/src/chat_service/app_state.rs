use parking_lot::{Mutex, RwLock};
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{oneshot, Mutex as TokioMutex, OwnedMutexGuard};
use tokio_util::sync::CancellationToken;

use crate::agents::register_builtin_agents;
use crate::extensions::ExtensionRegistry;
use crate::models::{DebugSessionSettings, EffectiveSettingsView, PlatformSettings, UserSettings};
use crate::platform_auth::{PlatformLoginCredentials, SharedPlatformAuth};
use crate::platform_config::{
    apply_login_llm_credentials, apply_login_llm_provider_api_keys, apply_login_media_oss,
    apply_login_platform_providers, finalize_merged_settings, PlatformConfigManager,
    SharedPlatformConfig,
};
use crate::skills::SkillRegistry;
use crate::storage;
use crate::tools::ToolRegistry;
use crate::web_request_auth::WebSessionAuth;

const TERMINAL_SCOPE_SEP: &str = "\u{1f}ptr_tool_scope\u{1f}";

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ToolExecutionScope {
    pub conversation_id: String,
    pub agent_instance_id: Option<String>,
    pub tool_call_id: String,
}

impl ToolExecutionScope {
    pub fn new(
        conversation_id: impl Into<String>,
        agent_instance_id: Option<&str>,
        tool_call_id: impl Into<String>,
    ) -> Self {
        Self {
            conversation_id: conversation_id.into(),
            agent_instance_id: agent_instance_id.map(str::to_string),
            tool_call_id: tool_call_id.into(),
        }
    }

    /// Prefer sub-agent / self-fork instance, then lead; `None` keeps legacy keys.
    pub fn from_agent_contexts(
        conversation_id: impl Into<String>,
        lead_instance_id: Option<&str>,
        sub_instance_id: Option<&str>,
        tool_call_id: impl Into<String>,
    ) -> Self {
        Self::new(
            conversation_id,
            sub_instance_id.or(lead_instance_id),
            tool_call_id,
        )
    }

    fn instance_key(&self) -> &str {
        self.agent_instance_id
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("legacy")
    }

    pub fn abort_key(&self) -> String {
        format!(
            "{}{TERMINAL_SCOPE_SEP}{}{TERMINAL_SCOPE_SEP}{}",
            self.conversation_id,
            self.instance_key(),
            self.tool_call_id
        )
    }

    pub fn output_trace_key(&self) -> String {
        self.abort_key()
    }

    pub fn pending_input_key(&self, request_id: &str) -> String {
        format!(
            "{}{TERMINAL_SCOPE_SEP}{}",
            self.abort_key(),
            request_id.trim()
        )
    }

    pub fn log_fields(&self) -> String {
        format!(
            "conversation_id={} agent_instance_id={} tool_call_id={}",
            self.conversation_id,
            self.instance_key(),
            self.tool_call_id
        )
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct TerminalPendingInputKey {
    scope: ToolExecutionScope,
    request_id: String,
}

#[derive(Default)]
pub struct FileWriteLockManager {
    locks: TokioMutex<HashMap<PathBuf, Arc<TokioMutex<()>>>>,
}

impl FileWriteLockManager {
    fn normalize_existing_or_future_path(path: &Path) -> anyhow::Result<PathBuf> {
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            let root = crate::tools::file::resolve_tool_workspace_root()
                .map_err(|error| anyhow::anyhow!("无法解析文件锁工作目录: {error}"))?;
            root.join(path)
        };
        // Callers (file_write/file_edit) must pass already-resolved paths.
        // Strip `.` and reject `..` before walking parents so missing targets
        // cannot absorb ParentDir into an existing ancestor and escape the key.
        let mut cleaned = PathBuf::new();
        for component in absolute.components() {
            match component {
                Component::Prefix(_) | Component::RootDir => {
                    cleaned.push(component.as_os_str());
                }
                Component::CurDir => {}
                Component::ParentDir => {
                    return Err(anyhow::anyhow!("文件锁路径含非法组件"));
                }
                Component::Normal(part) => cleaned.push(part),
            }
        }
        let mut probe = cleaned.clone();
        let mut missing_suffix = PathBuf::new();
        let canonical_base = loop {
            if probe.exists() {
                break probe
                    .canonicalize()
                    .map_err(|error| anyhow::anyhow!("无法规范化文件锁路径: {error}"))?;
            }
            let Some(name) = probe.file_name().map(PathBuf::from) else {
                return Err(anyhow::anyhow!(
                    "无法为不存在的目标找到可规范化父目录: {}",
                    absolute.display()
                ));
            };
            let mut next_suffix = name;
            next_suffix.push(&missing_suffix);
            missing_suffix = next_suffix;
            if !probe.pop() {
                return Err(anyhow::anyhow!(
                    "无法为不存在的目标找到可规范化父目录: {}",
                    absolute.display()
                ));
            }
        };
        for component in missing_suffix.components() {
            if !matches!(component, Component::Normal(_)) {
                return Err(anyhow::anyhow!("文件锁路径含非法组件"));
            }
        }
        let normalized = if missing_suffix.as_os_str().is_empty() {
            canonical_base
        } else {
            canonical_base.join(missing_suffix)
        };
        #[cfg(windows)]
        {
            Ok(PathBuf::from(normalized.to_string_lossy().to_lowercase()))
        }
        #[cfg(not(windows))]
        {
            Ok(normalized)
        }
    }

    pub async fn lock_path(
        &self,
        path: &Path,
    ) -> anyhow::Result<(OwnedMutexGuard<()>, Duration, PathBuf)> {
        let normalized = Self::normalize_existing_or_future_path(path)?;
        let lock = {
            let mut locks = self.locks.lock().await;
            locks
                .entry(normalized.clone())
                .or_insert_with(|| Arc::new(TokioMutex::new(())))
                .clone()
        };
        let wait_started = Instant::now();
        let guard = lock.lock_owned().await;
        Ok((guard, wait_started.elapsed(), normalized))
    }
}

fn remove_terminal_input_by_request_id(
    pending: &mut HashMap<
        TerminalPendingInputKey,
        std::sync::mpsc::Sender<crate::tools::terminal::TerminalInputResolution>,
    >,
    request_id: &str,
) -> Option<std::sync::mpsc::Sender<crate::tools::terminal::TerminalInputResolution>> {
    let matches: Vec<_> = pending
        .keys()
        .filter(|key| key.request_id == request_id)
        .cloned()
        .collect();
    match matches.as_slice() {
        [] => None,
        [key] => pending.remove(key),
        _ => {
            log::warn!(
                "terminal: ambiguous pending input request_id={} matching_scopes={}",
                request_id,
                matches.len()
            );
            None
        }
    }
}

fn open_conversation_store_with_fallback() -> Arc<crate::conversation_store::ConversationStore> {
    match crate::conversation_store::global_store() {
        Ok(store) => store,
        Err(e) => {
            log::warn!("conversation_store: open failed ({e:#}); using temp db");
            let temp_path = std::env::temp_dir()
                .join(format!("pointer-conversations-{}.db", uuid::Uuid::new_v4()));
            match crate::conversation_store::ConversationStore::open(temp_path) {
                Ok(store) => Arc::new(store),
                Err(e2) => {
                    log::error!("conversation_store: temp db failed ({e2:#}); using in-memory db");
                    Arc::new(
                        crate::conversation_store::ConversationStore::open(
                            std::path::PathBuf::from(":memory:"),
                        )
                        .expect("conversation_store in-memory db"),
                    )
                }
            }
        }
    }
}

/// 按 PluginRegistry 当前状态装配/注销插件能力（幂等）。
/// - `Enabled` 插件 → [`crate::plugins::activation::activate_plugin`] 注册其工具/skill/agent/rule；
/// - 其余（Discovered/Disabled/NeedsReauth/Rejected）→ 先按 plugin_id 注销（防止上次残留）。
fn apply_plugins(
    tools: &crate::tools::ToolRegistry,
    skills: &crate::skills::SkillRegistry,
    agents: &crate::agents::AgentRegistry,
    extensions: &crate::extensions::ExtensionRegistry,
    hooks: &crate::dispatcher::HookRegistry,
    plugins: &crate::plugins::registry::PluginRegistry,
) {
    // 目录被外部删除的插件已从 registry 消失（遍历不到），先注销其残留能力。
    for id in plugins.take_disappeared() {
        crate::plugins::activation::deactivate_plugin(
            tools, skills, agents, extensions, hooks, &id,
        );
    }
    for record in plugins.list() {
        if record.status == crate::plugins::registry::PluginStatus::Enabled {
            if let Err(err) = crate::plugins::activation::activate_plugin(
                tools, skills, agents, extensions, hooks, &record,
            ) {
                log::warn!("plugin {}: 装配失败: {err:#}", record.id);
            }
        } else {
            crate::plugins::activation::deactivate_plugin(
                tools, skills, agents, extensions, hooks, &record.id,
            );
        }
    }
}

/// 按 PluginRegistry 当前状态对齐 MCP 会话：`Enabled` 插件建立会话并注册其工具
/// （已有会话则跳过，避免重复启动子进程）；其余状态关闭会话（工具由
/// `deactivate_plugin` 注销）。在启动扫描 / 导入后全量对齐时调用。
fn sync_mcp_sessions(
    tools: &crate::tools::ToolRegistry,
    plugins: &crate::plugins::registry::PluginRegistry,
    sessions: &crate::plugins::mcp::McpSessionManager,
) {
    for record in plugins.list() {
        if record.status == crate::plugins::registry::PluginStatus::Enabled {
            if sessions.has_session(&record.id) {
                continue;
            }
            match crate::plugins::mcp::activate_mcp_servers(tools, &record) {
                Ok(clients) => sessions.register(&record.id, clients),
                Err(e) => log::warn!("plugin {}: MCP 装配失败: {e:#}", record.id),
            }
        } else {
            let n = sessions.shutdown_plugin(&record.id);
            if n > 0 {
                log::info!("plugin {}: 关闭 {} 个 MCP 会话", record.id, n);
            }
        }
    }
}

/// P2b：按全局 MCP 配置对齐会话（不绑定插件状态）：有声明且无会话 → 启动 +
/// 注册工具（命名 `mcp.<server>.<tool>`）；无声明 → 关闭并清理。
fn sync_global_mcp_sessions(
    tools: &crate::tools::ToolRegistry,
    global_mcp: &RwLock<GlobalMcpConfig>,
    sessions: &crate::plugins::mcp::McpSessionManager,
) {
    use crate::plugins::mcp::GLOBAL_MCP_KEY;
    let cfg = global_mcp.read();
    if cfg.decls.is_empty() {
        let n = sessions.shutdown_plugin(GLOBAL_MCP_KEY);
        if n > 0 {
            log::info!("全局 MCP: 关闭 {n} 个会话（配置已清空）");
        }
        return;
    }
    if sessions.has_session(GLOBAL_MCP_KEY) {
        return;
    }
    match crate::plugins::mcp::activate_global_mcp_servers(tools, &cfg.decls, &cfg.base_dir) {
        Ok(clients) => sessions.register(GLOBAL_MCP_KEY, clients),
        Err(e) => {
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            sessions.mark_failure(GLOBAL_MCP_KEY, &format!("{e:#}"), now_ms);
            log::warn!("全局 MCP: 装配失败: {e:#}");
        }
    }
}

/// MCP watchdog 轮询间隔（毫秒）。
const MCP_WATCHDOG_INTERVAL_MS: u64 = 2_000;

/// MCP watchdog 单轮：对已启用且声明了 MCP server 的插件，若会话不存在或
/// 全部进程已退出，则（受指数退避约束）重建会话：关旧进程 → 注销 MCP 工具
/// （保留 sidecar 工具）→ 重新启动 + 注册。连续失败达上限进入 degraded。
/// P2b：同样处理全局（非插件）MCP 会话（key = `__global__`）。
/// pub(crate)：供 e2e 测试手动驱动一轮。
pub(crate) fn mcp_watchdog_cycle(
    tools: &crate::tools::ToolRegistry,
    plugins: &crate::plugins::registry::PluginRegistry,
    sessions: &crate::plugins::mcp::McpSessionManager,
    global_mcp: &RwLock<GlobalMcpConfig>,
) {
    use crate::plugins::mcp::GLOBAL_MCP_KEY;
    use crate::plugins::registry::PluginStatus;
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    for record in plugins.list() {
        if record.status != PluginStatus::Enabled {
            continue;
        }
        if record.manifest.mcp_servers.server.is_empty() {
            continue;
        }
        let alive = sessions.has_session(&record.id) && sessions.any_alive(&record.id);
        if alive {
            continue;
        }
        let st = sessions.status(&record.id).unwrap_or_default();
        if st.degraded {
            continue;
        }
        if now_ms < st.next_attempt_ms {
            continue;
        }
        // 重建：shutdown 幂等（无会话也安全）
        sessions.shutdown_plugin(&record.id);
        tools.unregister_mcp_by_plugin(&record.id);
        match crate::plugins::mcp::activate_mcp_servers(tools, &record) {
            Ok(clients) => {
                sessions.register(&record.id, clients);
                log::info!("plugin {}: MCP 会话已（重新）建立", record.id);
            }
            Err(e) => {
                sessions.mark_failure(&record.id, &format!("{e:#}"), now_ms);
                log::warn!(
                    "plugin {}: MCP 会话建立失败（第 {} 次）: {e:#}",
                    record.id,
                    st.restart_count + 1
                );
            }
        }
    }

    // P2b：全局（非插件）MCP——配置 clone 后释放锁（避免激活阻塞热重载写锁）。
    let (decls, base_dir) = {
        let cfg = global_mcp.read();
        (cfg.decls.clone(), cfg.base_dir.clone())
    };
    if decls.is_empty() {
        return;
    }
    let alive = sessions.has_session(GLOBAL_MCP_KEY) && sessions.any_alive(GLOBAL_MCP_KEY);
    if alive {
        return;
    }
    let st = sessions.status(GLOBAL_MCP_KEY).unwrap_or_default();
    if st.degraded {
        return;
    }
    if now_ms < st.next_attempt_ms {
        return;
    }
    sessions.shutdown_plugin(GLOBAL_MCP_KEY);
    tools.unregister_mcp_by_plugin(GLOBAL_MCP_KEY);
    match crate::plugins::mcp::activate_global_mcp_servers(tools, &decls, &base_dir) {
        Ok(clients) => {
            sessions.register(GLOBAL_MCP_KEY, clients);
            log::info!("全局 MCP: 会话已（重新）建立");
        }
        Err(e) => {
            sessions.mark_failure(GLOBAL_MCP_KEY, &format!("{e:#}"), now_ms);
            log::warn!(
                "全局 MCP: 会话建立失败（第 {} 次）: {e:#}",
                st.restart_count + 1
            );
        }
    }
}

/// 把当前 Enabled 插件的注册技能幂等合并进 general 的 agentSkillOverrides（落盘）。
/// 独立自由函数：`AppState::new` 阶段实例尚未构造完成，可直接基于 registry 调用。
fn reconcile_enabled_plugin_skills(
    skills: &crate::skills::SkillRegistry,
    plugins: &crate::plugins::registry::PluginRegistry,
) -> anyhow::Result<()> {
    let enabled_ids: Vec<String> = plugins
        .list()
        .iter()
        .filter(|r| r.status == crate::plugins::registry::PluginStatus::Enabled)
        .map(|r| r.id.clone())
        .collect();
    if enabled_ids.is_empty() {
        return Ok(());
    }
    let mut user = crate::storage::load_user_settings().unwrap_or_default();
    let mut changed = false;
    for plugin_id in &enabled_ids {
        let skill_ids: Vec<String> = skills
            .list()
            .iter()
            .filter(|s| s.plugin_id.as_deref() == Some(plugin_id.as_str()))
            .map(|s| s.id.clone())
            .collect();
        let overrides = user
            .agent_skill_overrides
            .entry("general".to_string())
            .or_default();
        for id in &skill_ids {
            if !overrides.contains(id) {
                overrides.push(id.clone());
                changed = true;
            }
        }
    }
    if changed {
        crate::storage::save_user_settings(&user)?;
    }
    Ok(())
}

/// P2b：全局（非插件）MCP 配置——来自 `pointer-server.toml` 的
/// `[[mcp_servers.server]]` 声明 + 配置文件所在目录（相对 command 解析基准）。
#[derive(Debug, Clone, Default)]
pub struct GlobalMcpConfig {
    pub decls: Vec<crate::plugins::manifest::McpServerDecl>,
    pub base_dir: PathBuf,
}

impl GlobalMcpConfig {
    /// 读取全局 MCP 配置：**界面配置（user_settings）优先**；为空时回退
    /// `pointer-server.toml`（兼容旧配置）。桌面/测试未走 `load_server_config`
    /// 时轻量重解析（只读文件，无 env/部署模式副作用）。
    pub fn from_server_config() -> Self {
        let user = crate::storage::load_user_settings().unwrap_or_default();
        if !user.global_mcp_servers.is_empty() {
            return Self {
                decls: user.global_mcp_servers,
                base_dir: PathBuf::from("."),
            };
        }
        if let Some((decls, base_dir)) = crate::server_config::mcp_servers_from_config() {
            return Self { decls, base_dir };
        }
        match crate::server_config::reload_mcp_servers_config() {
            Ok(Some((decls, base_dir))) => Self { decls, base_dir },
            _ => Self::default(),
        }
    }
}

/// P2b：全局 MCP server 视图（管理 API / UI 展示）。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalMcpServerView {
    pub name: String,
    pub command: String,
    pub transport: String,
    /// healthy | crashed | degraded | stopped
    pub status: String,
    pub restart_count: u32,
    pub last_error: Option<String>,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub url: Option<String>,
    pub headers: Option<HashMap<String, String>>,
    /// 该 server 当前已注册的工具（未连接/注册失败时为空）。
    pub tools: Vec<crate::tools::McpToolBrief>,
}

/// P2b：全局 MCP 总览（decls + 运行状态）。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalMcpView {
    pub servers: Vec<GlobalMcpServerView>,
    pub enabled: bool,
}

pub struct AppState {
    pub tools: Arc<ToolRegistry>,
    pub skills: Arc<SkillRegistry>,
    pub agents: Arc<crate::agents::AgentRegistry>,
    pub computer_state: Arc<crate::agents::computer::ComputerState>,
    pub platform_auth: SharedPlatformAuth,
    pub platform_config: SharedPlatformConfig,
    pub task_board_store: Arc<crate::task_board::TaskBoardStore>,
    pub file_write_locks: Arc<FileWriteLockManager>,
    pub memory_store: Arc<crate::memory::MemoryStore>,
    pub session_index: Arc<crate::session_search::SessionIndex>,
    /// Lifecycle hooks aligned with Python `call_extensions(extension_point, …)`.
    pub extensions: Arc<ExtensionRegistry>,
    pub cancels: Mutex<HashMap<String, CancellationToken>>,
    /// When set, the in-flight `terminal` tool for that conversation kills its subprocess (host-only; does not cancel the LLM turn).
    pub terminal_run_abort: Mutex<HashMap<ToolExecutionScope, Arc<AtomicBool>>>,
    pub approvals: Mutex<HashMap<String, oneshot::Sender<bool>>>,
    /// Pending `ask_user` calls keyed by their tool call id.
    pub ask_user_pending: Mutex<HashMap<String, oneshot::Sender<Vec<String>>>>,
    /// IM Hermes-style `ask_user`: base conversation → pending registration.
    pub im_ask_user: Arc<crate::im_ask_user::ImAskUserRegistry>,
    /// Blocks `run_subagent` → computer until the UI confirms monitor selection.
    pub monitor_picks: Mutex<HashMap<String, oneshot::Sender<Result<(), String>>>>,
    /// Pending terminal stdin submissions keyed by `request_id`.
    terminal_input_pending: Mutex<
        HashMap<
            TerminalPendingInputKey,
            std::sync::mpsc::Sender<crate::tools::terminal::TerminalInputResolution>,
        >,
    >,
    /// Long-lived user-owned PTY sessions shown by the workspace console.
    pub console_sessions: Arc<crate::console_session::ConsoleSessionManager>,
    /// Active main-agent task board key per conversation.
    pub active_main_task_boards: Mutex<HashMap<String, String>>,
    /// Main task board anchor bindings: conversation -> (store_key -> user_message_id).
    pub task_board_anchor_by_store_key: Mutex<HashMap<String, HashMap<String, String>>>,
    /// Last user/chat activity for curator idle detection.
    pub last_activity_at: Mutex<Instant>,
    /// Prevents overlapping curator LLM passes.
    pub curator_llm_running: AtomicBool,
    /// Reused by webhook/cron/IM on pointer-server (browser OAuth LLM credentials).
    automation_web_session: Arc<RwLock<Option<WebSessionAuth>>>,
    /// Server-global LLM credential snapshot; survives OAuth expiry and browser logout.
    automation_llm_creds: Arc<RwLock<Option<PlatformLoginCredentials>>>,
    /// Run observability span bus (non-blocking `try_send`); instrumentation points
    /// (run / tool / LLM / approval / retry) emit spans through this handle.
    pub trace_bus: Arc<crate::observability::TraceBus>,
    /// Shared lifecycle hook registry (built-in + host extra). Reused by the
    /// dispatcher so built-in hooks are registered exactly once.
    pub hooks: Arc<crate::dispatcher::HookRegistry>,
    /// Pointer 原生插件注册表（P1）：发现 / 授权 / 状态机；启用时由
    /// [`AppState::apply_plugins`] 装配能力到 tools/skills/agents/extensions。
    pub plugins: Arc<crate::plugins::registry::PluginRegistry>,
    /// 插件 MCP server 会话（P2）：启用时启动子进程并注册其工具；禁用/卸载时关闭。
    pub mcp_sessions: Arc<crate::plugins::mcp::McpSessionManager>,
    /// P2b 全局（非插件）MCP 配置（来自 pointer-server.toml；热重载由管理 API 驱动）。
    pub global_mcp: Arc<RwLock<GlobalMcpConfig>>,
    /// Background `run_subagent` / (later) terminal jobs. Does not hold the session lane.
    pub jobs: super::job_supervisor::JobSupervisor,
    /// Parks token `finalize_run` until background jobs for that `run_id` finish.
    pub deferred_token_finalize: super::deferred_token_finalize::DeferredTokenFinalizeStore,
}

impl AppState {
    pub fn new() -> Self {
        crate::shell_env::bootstrap_process_path_from_login_shell();

        let platform_mgr = PlatformConfigManager::new();
        storage::ensure_legacy_settings_migrated();

        let platform_config = platform_mgr.shared();
        match storage::load_platform_model_catalog_cache_full() {
            Ok(data) if !data.providers.is_empty() => {
                let mut platform = platform_config.write();
                platform.model_catalog = data.catalog.clone();
                platform.tier_defaults = data.tier_defaults.clone();
                apply_login_platform_providers(&mut platform.providers, &data.providers);
                log::info!("platform_config: loaded cached platform model catalog");
            }
            Ok(_) => {}
            Err(err) => log::warn!("platform_config: failed to load cached model catalog: {err:#}"),
        }

        let tools = Arc::new(ToolRegistry::new());
        let task_board_store = match crate::task_board::open_default_persistence() {
            Some(db) => {
                std::sync::Arc::new(crate::task_board::TaskBoardStore::with_persistence(db))
            }
            None => {
                log::warn!("task_board: sqlite persistence unavailable; in-memory only");
                std::sync::Arc::new(crate::task_board::TaskBoardStore::new())
            }
        };
        let memory_store = match crate::memory::MemoryStore::open_default() {
            Ok(store) => Arc::new(store),
            Err(e) => {
                log::warn!("memory: open failed ({e:#}); using empty in-memory store");
                Arc::new(crate::memory::MemoryStore::open_in_dir(
                    crate::storage::app_data_dir()
                        .unwrap_or_else(|_| std::env::temp_dir())
                        .join("memories"),
                ))
            }
        };
        crate::tools::builtin::register_all(&tools, task_board_store.clone());
        crate::memory::register_memory_tool(&tools, memory_store.clone());
        let session_index = open_conversation_store_with_fallback();
        crate::session_search::register_session_search_tool(&tools, session_index.clone());
        crate::tools::cron_job::register(&tools, session_index.clone());
        let skills = Arc::new(SkillRegistry::new());
        crate::skills::builtin::register_all(&skills);
        crate::tools::builtin::register_skill_tools(&tools, skills.clone());
        if let Err(err) = skills.reload_meta() {
            log::warn!("load external skills failed: {err}");
        }
        let user_settings = storage::load_user_settings().unwrap_or_default();
        crate::tools::parallel::ParallelLimits::from_settings(
            &platform_mgr.effective_model_settings(&user_settings),
        )
        .log_startup(&platform_mgr.effective_model_settings(&user_settings));
        let agents = Arc::new(crate::agents::AgentRegistry::new());
        register_builtin_agents(&agents);
        if let Err(err) = agents.reload_external() {
            log::warn!("load external agents failed: {err}");
        }
        let platform_auth = Arc::new(crate::platform_auth::PlatformAuthManager::new());
        let computer_state = Arc::new(crate::agents::computer::ComputerState::new(
            &agents,
            platform_auth.clone(),
            platform_config.clone(),
        ));
        crate::tools::builtin::register_computer_tools(&tools, computer_state.clone());
        let mut extension_registry = ExtensionRegistry::new();
        crate::extensions::register_builtin_extensions(&mut extension_registry);
        crate::platform_config::register_global_platform_config(platform_config.clone());
        // Observability pipeline: `start_default()` works both inside a Tokio
        // runtime (consumer spawned on it) and outside one (dedicated thread),
        // so the desktop host (Tauri sync setup closure) also exports traces.
        let trace_bus = Arc::new(crate::observability::start_default());
        let hooks = crate::dispatcher::HookRegistry::new();
        crate::dispatcher::hooks::register_builtin_hooks(&hooks);
        let hooks = Arc::new(hooks);

        // Pointer 原生插件（P1）：扫描 + 装配已启用插件到 tools/skills/agents/extensions。
        let plugins = Arc::new(crate::plugins::registry::PluginRegistry::new());
        if let Err(err) = plugins.scan() {
            log::warn!("plugin scan failed: {err:#}");
        }
        apply_plugins(
            &tools,
            &skills,
            &agents,
            &extension_registry,
            &hooks,
            &plugins,
        );
        let mcp_sessions = Arc::new(crate::plugins::mcp::McpSessionManager::new());
        sync_mcp_sessions(&tools, &plugins, &mcp_sessions);
        // P2b：全局（非插件）MCP——pointer-server.toml 装配；不绑定插件状态。
        let global_mcp = Arc::new(RwLock::new(GlobalMcpConfig::from_server_config()));
        sync_global_mcp_sessions(&tools, &global_mcp, &mcp_sessions);
        // 注意：启动时的插件技能兜底（reconcile）由 server/Tauri 启动流程调用
        // `init_launch` 完成，避免 AppState::new 写入
        // user_settings（保持构造无副作用，测试隔离契约）。

        // AGENTS.md: `~/.pointer/AGENTS.md` then git-root → workspace (system cacheable).
        crate::plugins::agents_md::log_agents_md_startup();

        let state = Self {
            tools,
            skills,
            agents,
            computer_state,
            platform_auth,
            platform_config,
            task_board_store,
            file_write_locks: Arc::new(FileWriteLockManager::default()),
            memory_store,
            session_index,
            extensions: Arc::new(extension_registry),
            cancels: Mutex::new(HashMap::new()),
            terminal_run_abort: Mutex::new(HashMap::new()),
            approvals: Mutex::new(HashMap::new()),
            ask_user_pending: Mutex::new(HashMap::new()),
            im_ask_user: Arc::new(crate::im_ask_user::ImAskUserRegistry::new()),
            monitor_picks: Mutex::new(HashMap::new()),
            terminal_input_pending: Mutex::new(HashMap::new()),
            console_sessions: Arc::new(crate::console_session::ConsoleSessionManager::default()),
            active_main_task_boards: Mutex::new(HashMap::new()),
            task_board_anchor_by_store_key: Mutex::new(HashMap::new()),
            last_activity_at: Mutex::new(Instant::now()),
            curator_llm_running: AtomicBool::new(false),
            automation_web_session: Arc::new(RwLock::new(None)),
            automation_llm_creds: Arc::new(RwLock::new(None)),
            trace_bus,
            hooks,
            plugins,
            mcp_sessions,
            global_mcp,
            jobs: super::job_supervisor::JobSupervisor::new(),
            deferred_token_finalize:
                super::deferred_token_finalize::DeferredTokenFinalizeStore::new(),
        };
        state.spawn_mcp_watchdog();
        state
    }

    /// 按 PluginRegistry 当前状态装配已启用插件（幂等）：对每个 `Enabled` 插件
    /// 注册能力到 tools/skills/agents/extensions；其余状态先按 plugin_id 注销。
    /// enable/disable/uninstall 后调用（由 server/Tauri 命令层驱动）。
    pub fn apply_plugins(&self) {
        apply_plugins(
            &self.tools,
            &self.skills,
            &self.agents,
            &self.extensions,
            &self.hooks,
            &self.plugins,
        );
    }

    /// P2③：MCP 健康检查 watchdog。周期探测已启用插件的 MCP 会话：崩溃 /
    /// 启动失败按指数退避重启（见 [`mcp_watchdog_cycle`]），达上限标记
    /// `degraded`（UI 插件状态显示）。仅在 tokio 运行时内启动（server / Tauri
    /// 默认满足；`AppState::new` 的同步构造测试不启动）。
    pub fn spawn_mcp_watchdog(&self) {
        if tokio::runtime::Handle::try_current().is_err() {
            return;
        }
        let tools = self.tools.clone();
        let plugins = self.plugins.clone();
        let sessions = self.mcp_sessions.clone();
        let global_mcp = self.global_mcp.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(MCP_WATCHDOG_INTERVAL_MS))
                    .await;
                mcp_watchdog_cycle(&tools, &plugins, &sessions, &global_mcp);
            }
        });
    }

    /// P2b：热重载全局 MCP 配置（管理 API / 测试用）：全量关闭旧会话 → 注销
    /// 全局 MCP 工具 → 更新配置 → 重新装配。不重启进程。
    pub fn reload_global_mcp(
        &self,
        decls: Vec<crate::plugins::manifest::McpServerDecl>,
        base_dir: PathBuf,
    ) -> anyhow::Result<()> {
        use crate::plugins::mcp::GLOBAL_MCP_KEY;
        self.mcp_sessions.shutdown_plugin(GLOBAL_MCP_KEY);
        self.tools.unregister_mcp_by_plugin(GLOBAL_MCP_KEY);
        *self.global_mcp.write() = GlobalMcpConfig { decls, base_dir };
        sync_global_mcp_sessions(&self.tools, &self.global_mcp, &self.mcp_sessions);
        Ok(())
    }

    /// P2b：从 `pointer-server.toml` 重新解析并热重载全局 MCP（管理 API 用）。
    /// 无配置文件 / 非 toml / 解析失败时清空全局 MCP（等价于停用）。
    pub fn reload_global_mcp_from_config(&self) -> anyhow::Result<GlobalMcpView> {
        let (decls, base_dir) =
            crate::server_config::reload_mcp_servers_config()?.unwrap_or_default();
        self.reload_global_mcp(decls, base_dir)?;
        Ok(self.global_mcp_view())
    }

    /// P2b：**界面直接配置**——保存全局 MCP server 列表到用户设置并热重载。
    /// 相对 command 以进程 cwd 为基准（建议界面填写绝对路径）。
    pub fn save_global_mcp_servers(
        &self,
        decls: Vec<crate::plugins::manifest::McpServerDecl>,
    ) -> anyhow::Result<GlobalMcpView> {
        let mut user = self.load_user_settings();
        user.global_mcp_servers = decls.clone();
        self.save_user_settings(&user)?;
        self.reload_global_mcp(decls, PathBuf::from("."))?;
        Ok(self.global_mcp_view())
    }

    /// P2b：全局 MCP 运行状态视图（管理 API / UI）。
    pub fn global_mcp_view(&self) -> GlobalMcpView {
        use crate::plugins::mcp::GLOBAL_MCP_KEY;
        let cfg = self.global_mcp.read();
        let st = self.mcp_sessions.status(GLOBAL_MCP_KEY);
        let has = self.mcp_sessions.has_session(GLOBAL_MCP_KEY);
        let alive = has && self.mcp_sessions.any_alive(GLOBAL_MCP_KEY);
        let degraded = st.as_ref().is_some_and(|s| s.degraded);
        let servers = cfg
            .decls
            .iter()
            .map(|d| GlobalMcpServerView {
                name: d.name.clone(),
                command: d.command.clone(),
                transport: d.transport.clone(),
                status: if degraded {
                    "degraded".to_string()
                } else if alive {
                    "healthy".to_string()
                } else if has {
                    "crashed".to_string()
                } else {
                    "stopped".to_string()
                },
                restart_count: st.as_ref().map(|s| s.restart_count).unwrap_or(0),
                last_error: st.as_ref().and_then(|s| s.last_error.clone()),
                args: d.args.clone(),
                env: d.env.clone(),
                url: d.url.clone(),
                headers: d.headers.clone(),
                tools: self.tools.mcp_tools_for_server(GLOBAL_MCP_KEY, &d.name),
            })
            .collect();
        GlobalMcpView {
            servers,
            enabled: !cfg.decls.is_empty(),
        }
    }

    /// 启动兜底（所有入口统一调用）：刷新技能元数据，并把当前 Enabled 插件的
    /// 技能兜底合并进 general 启用列表。覆盖「旧代码已启用过插件 / 重启后首次
    /// 加载」场景，使插件技能始终自动启用。
    /// server / Tauri 启动流程必须经此初始化，避免行为分叉。
    pub fn init_launch(&self) -> anyhow::Result<usize> {
        let count = self.skills.reload_meta()?;
        self.reconcile_enabled_plugin_skills()?;
        Ok(count)
    }

    /// 遍历当前 Enabled 插件，把其注册技能幂等合并进 general 的 agentSkillOverrides。
    fn reconcile_enabled_plugin_skills(&self) -> anyhow::Result<()> {
        reconcile_enabled_plugin_skills(&self.skills, &self.plugins)
    }

    /// 授权并启用插件（一次性完成授权 + enable + 装配 + 自动启用其技能）。
    /// 粒度化：只装配目标插件，避免全量重装配影响其他插件。
    pub fn plugin_enable(&self, id: &str) -> anyhow::Result<()> {
        let status = self.plugins.get(id).map(|r| r.status.clone());
        if let Some(crate::plugins::registry::PluginStatus::Rejected(reason)) = status {
            return Err(anyhow::anyhow!("插件 {id} 校验失败: {reason}"));
        }
        // P2b：同名 MCP server 冲突——全局优先，插件启用时拒绝（避免静默覆盖）。
        {
            let global = self.global_mcp.read();
            if !global.decls.is_empty() {
                let Some(record) = self.plugins.get(id) else {
                    return Err(anyhow::anyhow!("插件 {id} 不存在"));
                };
                for decl in &record.manifest.mcp_servers.server {
                    if global.decls.iter().any(|g| g.name == decl.name) {
                        return Err(anyhow::anyhow!(
                            "插件 {id} 的 MCP server `{}` 与全局 MCP 同名（全局优先），请先在设置面板移除/改名全局配置",
                            decl.name
                        ));
                    }
                }
            }
        }
        self.plugins.authorize(id)?;
        self.plugins.enable(id)?;
        let record = self
            .plugins
            .get(id)
            .ok_or_else(|| anyhow::anyhow!("插件 {id} 不存在"))?;
        crate::plugins::activation::activate_plugin(
            &self.tools,
            &self.skills,
            &self.agents,
            &self.extensions,
            &self.hooks,
            &record,
        )?;
        sync_mcp_sessions(&self.tools, &self.plugins, &self.mcp_sessions);
        self.auto_enable_plugin_skills(id)?;
        Ok(())
    }

    /// 插件启用后，把该插件注册的技能自动加入通用助手的启用列表并落盘，
    /// 使技能面板显示「已启用」且会话默认加载。
    /// 插件技能只能由插件 enable/disable 生命周期管理（UI 不可手动切换），
    /// 因此这里只追加不删除；卸载时由 `remove_plugin_skills_from_overrides` 清理。
    /// 幂等：仅追加不删除；插件技能未启用时也允许（如只有工具/规则）。
    fn auto_enable_plugin_skills(&self, plugin_id: &str) -> anyhow::Result<()> {
        let skill_ids: Vec<String> = self
            .skills
            .list()
            .iter()
            .filter(|s| s.plugin_id.as_deref() == Some(plugin_id))
            .map(|s| s.id.clone())
            .collect();
        if skill_ids.is_empty() {
            return Ok(());
        }
        let mut user = self.load_user_settings();
        let overrides = user
            .agent_skill_overrides
            .entry("general".to_string())
            .or_default();
        let mut changed = false;
        for id in &skill_ids {
            if !overrides.contains(id) {
                overrides.push(id.clone());
                changed = true;
            }
        }
        if changed {
            self.save_user_settings(&user)?;
        }
        log::info!(
            "plugin {}: 自动启用 {} 个技能 → general",
            plugin_id,
            skill_ids.len()
        );
        Ok(())
    }

    /// 禁用插件并注销其能力（粒度化：只注销目标插件，不动其他插件）。
    pub fn plugin_disable(&self, id: &str) -> anyhow::Result<()> {
        self.plugins.disable(id)?;
        crate::plugins::activation::deactivate_plugin(
            &self.tools,
            &self.skills,
            &self.agents,
            &self.extensions,
            &self.hooks,
            id,
        );
        sync_mcp_sessions(&self.tools, &self.plugins, &self.mcp_sessions);
        Ok(())
    }

    /// 卸载插件并注销其能力，同时从其技能启用列表中移除插件技能。
    pub fn plugin_uninstall(&self, id: &str) -> anyhow::Result<()> {
        // 卸载前收集该插件注册的技能 id（卸载后 registry 已注销，取不到）
        let plugin_skill_ids: Vec<String> = self
            .skills
            .list()
            .iter()
            .filter(|s| s.plugin_id.as_deref() == Some(id))
            .map(|s| s.id.clone())
            .collect();
        self.plugins.uninstall(id)?;
        // uninstall 已把记录从 registry 删除，apply_plugins 遍历不到 → 显式注销能力
        crate::plugins::activation::deactivate_plugin(
            &self.tools,
            &self.skills,
            &self.agents,
            &self.extensions,
            &self.hooks,
            id,
        );
        sync_mcp_sessions(&self.tools, &self.plugins, &self.mcp_sessions);
        self.remove_plugin_skills_from_overrides(id, &plugin_skill_ids)?;
        Ok(())
    }

    /// 卸载插件后，把该插件的技能 id 从所有 agent 的启用列表移除并落盘，
    /// 避免残留 id 出现在 `<available_skills>` 注入占位。
    /// 幂等：仅移除不新增；插件技能为空时也允许。
    fn remove_plugin_skills_from_overrides(
        &self,
        plugin_id: &str,
        skill_ids: &[String],
    ) -> anyhow::Result<()> {
        if skill_ids.is_empty() {
            return Ok(());
        }
        let mut user = self.load_user_settings();
        let mut changed = false;
        for ids in user.agent_skill_overrides.values_mut() {
            let before = ids.len();
            ids.retain(|s| !skill_ids.contains(s));
            if ids.len() != before {
                changed = true;
            }
        }
        if changed {
            self.save_user_settings(&user)?;
        }
        log::info!(
            "plugin {}: 从启用列表移除 {} 个技能",
            plugin_id,
            skill_ids.len()
        );
        Ok(())
    }

    /// 从目录批量导入插件（写入 `~/.pointer/plugins/<id>/`），随后重新扫描并装配。
    /// 导入插件：目录 → 自动识别 Pointer / Codex / Claude 候选批量导入；
    /// `.zip` 文件 → 解压后同样自动识别导入。返回全部导入报告。
    pub fn plugin_import(
        &self,
        source: &std::path::Path,
    ) -> anyhow::Result<Vec<crate::plugins::importer::ImportReport>> {
        let target_root = crate::plugins::user_plugins_dir()?;
        if source.is_file() {
            let bytes = std::fs::read(source)
                .map_err(|e| anyhow::anyhow!("无法读取文件 {}: {e}", source.display()))?;
            return self.plugin_import_zip(&bytes);
        }
        let reports = crate::plugins::importer::import_plugin_directory_all(source, &target_root)?;
        self.plugins.scan()?;
        self.apply_plugins();
        Ok(reports)
    }

    /// 从 zip 字节导入插件（解压 → 目录导入 → 装配）。
    pub fn plugin_import_zip(
        &self,
        bytes: &[u8],
    ) -> anyhow::Result<Vec<crate::plugins::importer::ImportReport>> {
        let target_root = crate::plugins::user_plugins_dir()?;
        let reports = crate::plugins::importer::import_plugin_zip(bytes, &target_root)?;
        self.plugins.scan()?;
        self.apply_plugins();
        Ok(reports)
    }

    /// 发现目录（含一层子目录）中的可导入插件候选，供 UI 列出。
    pub fn plugin_discover(
        &self,
        dir: &std::path::Path,
    ) -> anyhow::Result<Vec<crate::plugins::importer::DiscoveredPlugin>> {
        crate::plugins::importer::discover_plugin_packages(dir)
    }

    /// 列出插件记录（发现/授权/启用状态），供 server / Tauri 命令层返回。
    pub fn plugin_list(&self) -> Vec<crate::plugins::registry::PluginRecord> {
        self.plugins.list()
    }

    /// 探测本机外部插件来源（Claude Code / Codex），供「导入」入口列出可导入项。
    pub fn plugin_probe_external(
        &self,
    ) -> anyhow::Result<crate::plugins::external_probe::ExternalPluginsProbeResult> {
        crate::plugins::external_probe::probe_external_plugin_sources()
    }

    /// 按来源 id 导入外部插件（转原生格式并装配）。
    pub fn plugin_import_external(
        &self,
        source_id: &str,
    ) -> anyhow::Result<crate::plugins::importer::ImportReport> {
        let report = crate::plugins::external_probe::import_external_plugin(source_id)?;
        self.plugins.scan()?;
        self.apply_plugins();
        Ok(report)
    }

    /// Persist LLM keys for headless automation (webhook / cron / IM).
    pub fn remember_automation_llm_creds(&self, creds: &PlatformLoginCredentials) {
        if !crate::platform_auth::credentials_have_llm_keys(creds) {
            return;
        }
        *self.automation_llm_creds.write() = Some(creds.clone());
        log::debug!("automation: cached LLM credentials for headless runs");
    }

    /// Browser OAuth session reused by webhook/cron (pointer-server).
    pub fn set_automation_web_session(&self, auth: Option<WebSessionAuth>) {
        if let Some(ref session) = auth {
            self.remember_automation_llm_creds(&session.creds);
        }
        *self.automation_web_session.write() = auth;
    }

    pub fn automation_web_session_auth(&self) -> Option<WebSessionAuth> {
        self.automation_web_session.read().clone()
    }

    /// Auth + creds for headless runs: live browser session, else cached LLM keys.
    ///
    /// Standalone local login stores empty platform creds (LLM keys live in
    /// settings); still return that session so cron / idle push get `web_session`.
    pub fn automation_execution_auth(&self) -> Option<WebSessionAuth> {
        if let Some(live) = self.automation_web_session.read().clone() {
            if crate::platform_auth::credentials_have_llm_keys(&live.creds) {
                return Some(live);
            }
            if live.kind == crate::web_request_auth::WebSessionAuthKind::Local {
                return Some(live);
            }
        }
        let creds = self.automation_llm_creds.read().clone()?;
        if !crate::platform_auth::credentials_have_llm_keys(&creds) {
            return None;
        }
        Some(WebSessionAuth {
            kind: crate::web_request_auth::WebSessionAuthKind::Platform,
            auth: self.platform_auth.clone(),
            creds,
        })
    }

    pub fn touch_activity(&self) {
        *self.last_activity_at.lock() = Instant::now();
    }

    pub fn idle_duration(&self) -> std::time::Duration {
        self.last_activity_at.lock().elapsed()
    }

    pub fn start_background_tasks(self: &Arc<Self>) {
        crate::skills::curator::start_background_loop(self.clone());
    }

    /// Build a [`crate::dispatcher::RunDispatcher`] backed by this state.
    /// Hosts (Tauri / server) construct one dispatcher after wrapping
    /// `AppState` in `Arc` and share it across all trigger sources. The
    /// dispatcher reuses this `AppState` (and its `run_chat` entry), so the
    /// existing execution path is unchanged. Built-in lifecycle hooks
    /// (structured logging) are pre-registered.
    pub fn build_dispatcher(self: &Arc<Self>) -> crate::dispatcher::RunDispatcher {
        self.build_dispatcher_with_extra_finished_hooks(Vec::new())
    }

    /// Like [`Self::build_dispatcher`], but lets the host register extra
    /// [`OnRunFinishedHook`]s after the built-in lifecycle hooks. Used by the
    /// Run → IM outbound bus: the host builds the `ChannelGateway` first,
    /// wraps `ImDeliverHook` around it, and passes it here so the dispatcher
    /// fires it on every terminal run. Re-registering the same `override_key`
    /// replaces the prior hook (registry semantics), so hosts may safely call
    /// this with a fresh vec on every rebuild.
    pub fn build_dispatcher_with_extra_finished_hooks(
        self: &Arc<Self>,
        extra: Vec<Arc<dyn crate::dispatcher::OnRunFinishedHook>>,
    ) -> crate::dispatcher::RunDispatcher {
        if let Err(e) = self.session_index.runs_reconcile_interrupted() {
            log::warn!("dispatcher: reconcile interrupted runs failed: {e:#}");
        }
        let hooks = (*self.hooks).clone();
        for hook in extra {
            hooks.register_on_run_finished(hook);
        }
        let max = self.resolve_max_concurrent_runs();
        let dispatcher = crate::dispatcher::RunDispatcher::with_hooks_and_max_concurrent(
            self.clone(),
            Arc::new(hooks),
            max,
        );
        crate::chat_service::idle_job_push::install(&dispatcher);
        dispatcher
    }

    /// Global dispatcher concurrency cap from merged user settings.
    pub fn resolve_max_concurrent_runs(&self) -> usize {
        crate::dispatcher::resolve_max_concurrent_runs(&self.effective_settings())
    }

    pub fn sync_dispatcher_concurrency(&self, dispatcher: &crate::dispatcher::RunDispatcher) {
        dispatcher.set_max_concurrent(self.resolve_max_concurrent_runs());
    }

    pub fn load_user_settings(&self) -> UserSettings {
        storage::load_user_settings().unwrap_or_default()
    }

    /// Deprecated legacy field accessor. Skill resolve no longer uses
    /// `enabledSkillIds`; prefer [`Self::default_run_agent_skill_overrides`].
    pub fn default_run_enabled_skill_ids(&self) -> Vec<String> {
        Vec::new()
    }

    /// Per-agent skill overrides for automated runs (IM, cron, webhook) and UI
    /// requests that omit an explicit override map.
    pub fn default_run_agent_skill_overrides(
        &self,
    ) -> std::collections::HashMap<String, Vec<String>> {
        self.load_user_settings().agent_skill_overrides
    }

    pub fn active_platform_auth(&self) -> Arc<crate::platform_auth::PlatformAuthManager> {
        crate::web_request_auth::scoped_auth(&self.platform_auth)
    }

    pub fn save_user_settings(&self, user: &UserSettings) -> anyhow::Result<()> {
        storage::save_user_settings(user)
    }

    pub fn effective_settings(&self) -> crate::models::ModelSettings {
        let user = self.load_user_settings();
        let platform = self.platform_config.read().clone();
        let mut settings =
            finalize_merged_settings(crate::models::merge_user_platform(&user, &platform));
        if let Some(creds) = crate::web_request_auth::scoped_login_creds() {
            crate::platform_config::apply_login_credentials_to_model_settings(
                &mut settings,
                &creds,
            );
        }
        settings
    }

    pub fn effective_settings_view(&self) -> EffectiveSettingsView {
        let user = self.load_user_settings();
        let platform = self.platform_config.read().clone();
        let merged = self.effective_settings();
        let is_platform_admin = self.active_platform_auth().is_platform_admin();
        EffectiveSettingsView {
            user,
            platform,
            merged,
            can_edit_platform: is_platform_admin,
            is_platform_admin,
        }
    }

    pub fn update_platform_settings(
        &self,
        patch: PlatformSettings,
    ) -> anyhow::Result<EffectiveSettingsView> {
        if !self.active_platform_auth().is_platform_admin() {
            anyhow::bail!("only platform admins may edit platform settings");
        }
        // Platform settings are in-memory only: providers (runtime keys), media_oss,
        // and server-side DaTi config. User-owned fields live in user_settings.json.
        let mut platform = self.platform_config.write();
        if !patch.providers.is_empty() {
            platform.providers = patch.providers;
        }
        if !patch.media_oss.bucket.trim().is_empty() {
            platform.media_oss = patch.media_oss;
        }
        if !patch.dati_api_url.trim().is_empty() {
            platform.dati_api_url = patch.dati_api_url;
            platform.dati_authcode = patch.dati_authcode;
            platform.dati_typeno = patch.dati_typeno;
            platform.dati_author = patch.dati_author;
        }
        drop(platform);
        Ok(self.effective_settings_view())
    }

    pub fn update_debug_session_settings(
        &self,
        mut incoming: DebugSessionSettings,
    ) -> anyhow::Result<EffectiveSettingsView> {
        if !self.active_platform_auth().is_platform_admin() {
            anyhow::bail!("only platform admins may edit debug session settings");
        }
        let active_provider_id = incoming.active_provider_id.trim().to_string();
        if active_provider_id.is_empty() {
            anyhow::bail!("active provider id is required");
        }
        let model = incoming.model.trim().to_string();
        if model.is_empty() {
            anyhow::bail!("active model is required");
        }
        let active_provider = incoming
            .providers
            .iter()
            .find(|provider| provider.id == active_provider_id)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "active provider '{}' is not present in debug providers",
                    active_provider_id
                )
            })?;
        if !active_provider
            .models
            .iter()
            .any(|candidate| candidate == &model)
        {
            anyhow::bail!(
                "active model '{}' is not configured for provider '{}'",
                model,
                active_provider_id
            );
        }
        incoming.active_provider_id = active_provider_id;
        incoming.model = model;

        // 会话级调试（DebugSessionSettings 契约：must never be persisted）。
        // 只更新 platform 内存 providers（临时调试），不写 user_settings.json，
        // 避免调试 provider 覆盖用户/平台默认配置。
        let mut platform = self.platform_config.write();
        let existing_keys: HashMap<String, String> = platform
            .providers
            .iter()
            .map(|provider| (provider.id.clone(), provider.api_key.clone()))
            .collect();
        for provider in &mut incoming.providers {
            if provider.api_key.trim().is_empty() || provider.api_key == "****" {
                if let Some(api_key) = existing_keys.get(provider.id.as_str()) {
                    provider.api_key = api_key.clone();
                }
            }
        }
        platform.providers = incoming.providers;
        let provider_count = platform.providers.len();
        drop(platform);

        log::info!(
            "debug_session_settings: session providers updated (in-memory only) count={}",
            provider_count
        );
        Ok(self.effective_settings_view())
    }

    /// 用户配置板块统一保存入口：前端发 UserSettings 快照（user 层全量 + 板块
    /// patch），这里做两类保护后直接落盘——
    /// 1. WEB 非 admin GET 会剥掉调试字段，回传 serde 默认值会清掉服务端调试
    ///    配置；非 admin 保存时用现有 user 值强改回（admin round-trip 正常更新）。
    ///    三档模型映射不是调试字段，非 admin PUT 必须保留 incoming 值。
    /// 2. WEB 非 admin 的 providers apiKey 被脱敏成 "****"/空，不能因此清掉用户
    ///    加密保存的 key；空/脱敏时回填现有用户 key。
    /// 平台注入 key 不进入 user 层：`source=platform` 的服务商条目一律不落盘
    /// （与服务商 id 无关；前端 fork 须标 `source=user`）。
    pub fn update_user_settings(
        &self,
        mut incoming: UserSettings,
    ) -> anyhow::Result<EffectiveSettingsView> {
        if incoming.theme.trim().is_empty() {
            incoming.theme = "system".into();
        }
        let existing = self.load_user_settings();
        if !self.active_platform_auth().is_platform_admin() {
            crate::models::preserve_platform_debug_settings_in_user(&mut incoming, &existing);
        }
        // 后端内存 key 池：user 层现有 key（load 时已从 json 解密回填到内存）
        // + platform 内存注入 key（OAuth / 登录）。前端不回传原始 key：
        // 用户没改提交空/"****"，这里从后端内存回填；用户显式输入的新 key
        // 非空，直接落盘（不走回填）。
        let mut key_pool: HashMap<String, String> = existing
            .providers
            .iter()
            .map(|p| (p.id.clone(), p.api_key.clone()))
            .collect();
        for p in &self.platform_config.read().providers {
            key_pool
                .entry(p.id.clone())
                .or_insert_with(|| p.api_key.clone());
        }
        for provider in &mut incoming.providers {
            if provider.api_key.trim().is_empty() || provider.api_key.trim() == "****" {
                if let Some(key) = key_pool.get(&provider.id) {
                    provider.api_key = key.clone();
                }
            }
        }
        // 只按 source 分层：source=platform 永不进用户文件（与目录里有哪些
        // id 无关）。用户 fork 必须标 source=user 才会落盘。
        incoming
            .providers
            .retain(|p| p.source.as_deref() != Some("platform"));
        self.save_user_settings(&incoming)?;
        Ok(self.effective_settings_view())
    }

    pub fn apply_login_credentials(&self, creds: &PlatformLoginCredentials) {
        self.remember_automation_llm_creds(creds);
        let mut platform = self.platform_config.write();
        // 平台服务商模板创建/更新（客户端不再内置任何平台服务商）。
        apply_login_platform_providers(&mut platform.providers, &creds.platform_providers);
        if !creds.model_catalog.is_empty() {
            platform.model_catalog = creds.model_catalog.clone();
        }
        if !creds.tier_defaults.is_null() {
            platform.tier_defaults = creds.tier_defaults.clone();
        }
        if !creds.model_catalog.is_empty() || !creds.platform_providers.is_empty() {
            if let Err(err) = storage::save_platform_model_catalog_cache_full(
                &creds.model_catalog,
                &creds.platform_providers,
                &creds.tier_defaults,
                creds.model_catalog_hash.as_deref(),
            ) {
                log::warn!("platform_config: failed to cache platform model catalog: {err:#}");
            }
        }
        apply_login_llm_provider_api_keys(&mut platform.providers, &creds.provider_api_keys);
        apply_login_llm_credentials(
            &mut platform.providers,
            creds.api_key.as_deref(),
            creds.llm_provider.as_deref(),
        );
        if let Some(media) = creds.media_oss.as_ref() {
            apply_login_media_oss(&mut platform.media_oss, Some(media));
        }
    }

    pub fn cancel(&self, conversation_id: &str) {
        self.cancel_with_options(conversation_id, true);
    }

    /// Cancel the active lead turn (and related waits).
    /// When `cancel_background_jobs` is false, JobSupervisor jobs keep running
    /// (Codex-style: new message / end-wait ends sync only). Hard stop uses true.
    pub fn cancel_with_options(&self, conversation_id: &str, cancel_background_jobs: bool) {
        let base = crate::channel_outbound::im_base_conversation_id(conversation_id);
        let cancel_keys: Vec<String> = {
            let guard = self.cancels.lock();
            guard
                .keys()
                .filter(|k| {
                    k.as_str() == conversation_id
                        || crate::channel_outbound::im_base_conversation_id(k) == base
                })
                .cloned()
                .collect()
        };
        for key in &cancel_keys {
            if let Some(token) = self.cancels.lock().get(key) {
                token.cancel();
            }
            if cancel_background_jobs {
                let _ = self.abort_terminal_command(key, None);
            }
        }
        if cancel_keys.is_empty() {
            if let Some(token) = self.cancels.lock().get(conversation_id) {
                token.cancel();
            }
            if cancel_background_jobs {
                let _ = self.abort_terminal_command(conversation_id, None);
            }
        }

        let approvals: Vec<_> = self.approvals.lock().drain().collect();
        for (_, tx) in approvals {
            let _ = tx.send(false);
        }

        // Drop IM ask_user waiters for this session, then drain remaining oneshots
        // (desktop cancel historically clears all pending ask_user).
        let im_pending = self.im_ask_user.clear_matching(conversation_id);
        {
            let mut pending = self.ask_user_pending.lock();
            for p in &im_pending {
                if let Some(tx) = pending.remove(&p.tool_call_id) {
                    drop(tx);
                }
            }
            let rest: Vec<_> = pending.drain().collect();
            for (_, tx) in rest {
                drop(tx);
            }
        }

        let monitor_picks: Vec<_> = self.monitor_picks.lock().drain().collect();
        for (_, tx) in monitor_picks {
            let _ = tx.send(Err("已停止生成".into()));
        }
        let pending_inputs = {
            let mut pending = self.terminal_input_pending.lock();
            let keys: Vec<_> = pending
                .keys()
                .filter(|key| {
                    key.scope.conversation_id == conversation_id
                        || crate::channel_outbound::im_base_conversation_id(
                            &key.scope.conversation_id,
                        ) == base
                })
                .cloned()
                .collect();
            keys.into_iter()
                .filter_map(|key| pending.remove(&key))
                .collect::<Vec<_>>()
        };
        for tx in pending_inputs {
            let _ = tx.send(crate::tools::terminal::TerminalInputResolution::Cancelled);
        }

        if cancel_background_jobs {
            let cancelled_jobs = self.jobs.cancel_conversation(conversation_id);
            if cancelled_jobs > 0 {
                log::info!(
                    "app_state: cancel fan-out background jobs conversation_id={conversation_id} count={cancelled_jobs}"
                );
            }
        } else {
            log::info!(
                "app_state: soft cancel conversation_id={conversation_id} (background jobs kept)"
            );
        }
    }

    /// Cancel one or more background jobs without stopping the lead turn.
    /// `job_ids` omit/empty = every background job in this conversation.
    pub fn cancel_background_jobs(
        &self,
        conversation_id: &str,
        job_ids: Option<&[String]>,
    ) -> Vec<String> {
        let cancelled = self.jobs.cancel_ids(conversation_id, job_ids);
        if !cancelled.is_empty() {
            log::info!(
                "app_state: cancel_background_jobs conversation_id={conversation_id} count={} ids={cancelled:?}",
                cancelled.len()
            );
            crate::chat_service::run_subagent_delegation::publish_background_jobs(
                conversation_id,
                &self.jobs,
            );
        }
        cancelled
    }

    /// Resolve a pending IM `ask_user` from free-text. Returns:
    /// - `Ok(true)` resolved
    /// - `Ok(false)` no pending
    /// - `Err(hint)` pending but text could not be parsed (still waiting)
    pub fn try_resolve_im_ask_user(
        &self,
        base_conversation_id: &str,
        user_text: &str,
    ) -> Result<bool, String> {
        let Some(pending) = self.im_ask_user.peek(base_conversation_id) else {
            return Ok(false);
        };
        let selected = crate::im_ask_user::parse_im_ask_user_reply(&pending.args, user_text)?;
        let Some(pending) = self.im_ask_user.take(base_conversation_id) else {
            return Ok(false);
        };
        if self.submit_ask_user(&pending.tool_call_id, selected) {
            log::info!(
                "im_ask_user: resolved base_conv={base_conversation_id} tool={}",
                pending.tool_call_id
            );
            Ok(true)
        } else {
            log::warn!(
                "im_ask_user: oneshot missing for tool={} base_conv={base_conversation_id}",
                pending.tool_call_id
            );
            Err("选项已过期，请重新发起提问。".into())
        }
    }

    /// Kill the subprocess for a **`terminal`** tool invocation.
    /// When `tool_call_id` is set, only that invocation is aborted; otherwise all terminals in the conversation.
    /// Does **not** cancel the LLM stream or the rest of the turn. Returns **true** if at least one run was registered.
    pub fn abort_terminal_command(
        &self,
        conversation_id: &str,
        tool_call_id: Option<&str>,
    ) -> bool {
        let runs = self.terminal_run_abort.lock();
        let mut any = false;
        let mut matched = 0usize;
        for (scope, flag) in runs.iter() {
            if scope.conversation_id == conversation_id
                && tool_call_id.is_none_or(|id| scope.tool_call_id == id)
            {
                flag.store(true, Ordering::SeqCst);
                any = true;
                matched += 1;
                log::info!("terminal: abort requested {}", scope.log_fields());
            }
        }
        // Callers can only target a terminal by (conversation, tool_call_id); the
        // per-instance agent scope is not part of that public contract. Tool call ids
        // are randomized per stream so concurrent self-forks should never collide.
        // If they somehow do, aborting one id would kill multiple instances' terminals,
        // so surface it instead of silently over-aborting.
        if tool_call_id.is_some() && matched > 1 {
            log::warn!(
                "terminal: abort by tool_call_id matched {matched} scopes conversation_id={conversation_id} tool_call_id={:?}; possible cross-instance collision",
                tool_call_id
            );
        }
        any
    }

    pub fn register_terminal_abort_flag(&self, scope: ToolExecutionScope, flag: Arc<AtomicBool>) {
        log::info!("terminal: registered abort scope {}", scope.log_fields());
        self.terminal_run_abort.lock().insert(scope, flag);
    }

    pub fn clear_terminal_abort_flag(&self, scope: &ToolExecutionScope) {
        if self.terminal_run_abort.lock().remove(scope).is_none() {
            log::warn!(
                "terminal: abort scope missing during cleanup {}",
                scope.log_fields()
            );
        }
    }

    pub fn approve_tool_call(&self, tool_call_id: &str, approved: bool) -> bool {
        if let Some(tx) = self.approvals.lock().remove(tool_call_id) {
            let _ = tx.send(approved);
            true
        } else {
            false
        }
    }

    pub fn submit_ask_user(&self, tool_call_id: &str, selected: Vec<String>) -> bool {
        if let Some(tx) = self.ask_user_pending.lock().remove(tool_call_id) {
            let _ = tx.send(selected);
            true
        } else {
            false
        }
    }

    pub fn confirm_computer_monitor_pick(&self, conversation_id: &str) -> bool {
        if let Some(tx) = self.monitor_picks.lock().remove(conversation_id) {
            let _ = tx.send(Ok(()));
            true
        } else {
            false
        }
    }

    pub fn cancel_computer_monitor_pick(&self, conversation_id: &str) -> bool {
        if let Some(tx) = self.monitor_picks.lock().remove(conversation_id) {
            let _ = tx.send(Err("屏幕选择已取消".into()));
            true
        } else {
            false
        }
    }

    pub fn register_terminal_input_wait(
        &self,
        scope: ToolExecutionScope,
        request_id: String,
        tx: std::sync::mpsc::Sender<crate::tools::terminal::TerminalInputResolution>,
    ) {
        let key = TerminalPendingInputKey { scope, request_id };
        if self
            .terminal_input_pending
            .lock()
            .insert(key.clone(), tx)
            .is_some()
        {
            log::warn!(
                "terminal: replaced pending input request_id={} {}",
                key.request_id,
                key.scope.log_fields()
            );
        }
    }

    pub fn clear_terminal_input_wait(&self, scope: &ToolExecutionScope, request_id: &str) {
        let key = TerminalPendingInputKey {
            scope: scope.clone(),
            request_id: request_id.to_string(),
        };
        self.terminal_input_pending.lock().remove(&key);
    }

    pub fn submit_terminal_input(&self, request_id: &str, text: String) -> bool {
        let tx = remove_terminal_input_by_request_id(
            &mut self.terminal_input_pending.lock(),
            request_id,
        );
        if let Some(tx) = tx {
            let _ = tx.send(crate::tools::terminal::TerminalInputResolution::Submit(
                text,
            ));
            true
        } else {
            false
        }
    }

    pub fn dismiss_terminal_input(&self, request_id: &str) -> bool {
        let tx = remove_terminal_input_by_request_id(
            &mut self.terminal_input_pending.lock(),
            request_id,
        );
        if let Some(tx) = tx {
            let _ = tx.send(crate::tools::terminal::TerminalInputResolution::Dismiss);
            true
        } else {
            false
        }
    }

    pub fn set_main_task_board_binding(
        &self,
        conversation_id: &str,
        store_key: &str,
        anchor_message_id: &str,
    ) {
        if conversation_id.trim().is_empty()
            || store_key.trim().is_empty()
            || anchor_message_id.trim().is_empty()
        {
            log::warn!(
                "task_board_binding: invalid args conversation_id={} store_key={} anchor_message_id={}",
                conversation_id,
                store_key,
                anchor_message_id
            );
            return;
        }
        let mut map = self.task_board_anchor_by_store_key.lock();
        map.entry(conversation_id.to_string())
            .or_default()
            .insert(store_key.to_string(), anchor_message_id.to_string());
    }

    pub fn get_main_task_board_anchor(
        &self,
        conversation_id: &str,
        store_key: &str,
    ) -> Option<String> {
        self.task_board_anchor_by_store_key
            .lock()
            .get(conversation_id)
            .and_then(|m| m.get(store_key).cloned())
            .or_else(|| crate::task_board::anchor_message_id_from_main_turn_key(store_key))
    }

    pub fn clear_active_main_task_board_key(&self, conversation_id: &str) {
        if conversation_id.trim().is_empty() {
            return;
        }
        self.active_main_task_boards.lock().remove(conversation_id);
    }

    pub fn set_active_main_task_board_key(&self, conversation_id: &str, store_key: &str) {
        if conversation_id.trim().is_empty() || store_key.trim().is_empty() {
            return;
        }
        if crate::task_board::is_child_store_key(store_key) {
            log::warn!(
                "task_board_main_key: reject_child_active conversation_id={} store_key={}",
                conversation_id,
                store_key
            );
            return;
        }
        self.active_main_task_boards
            .lock()
            .insert(conversation_id.to_string(), store_key.to_string());
    }

    pub fn get_active_main_task_board_key(&self, conversation_id: &str) -> Option<String> {
        {
            let mut active = self.active_main_task_boards.lock();
            if let Some(k) = active.get(conversation_id).cloned() {
                if !crate::task_board::is_child_store_key(&k) {
                    return Some(k);
                }
                log::warn!(
                    "task_board_main_key: drop_invalid_active conversation_id={} store_key={}",
                    conversation_id,
                    k
                );
                active.remove(conversation_id);
            }
        }
        let prefix = format!(
            "{}{}",
            conversation_id.trim(),
            crate::task_board::coordination::main_turn::MAIN_TURN_KEY_SEP
        );
        let mut keys = self.task_board_store.list_store_keys_by_prefix(&prefix);
        keys.retain(|k| !crate::task_board::is_child_store_key(k));
        if !keys.is_empty() {
            let pick = keys
                .iter()
                .find(|k| {
                    let doc = self.task_board_store.document(k);
                    !matches!(
                        doc.meta.status,
                        crate::task_board::MetaStatus::Completed
                            | crate::task_board::MetaStatus::Failed
                    )
                })
                .cloned()
                .unwrap_or_else(|| keys.remove(0));
            self.active_main_task_boards
                .lock()
                .insert(conversation_id.to_string(), pick.clone());
            return Some(pick);
        }
        let legacy = self.task_board_store.document(conversation_id);
        if !legacy.board_is_empty() || !legacy.meta.goal.trim().is_empty() {
            return Some(conversation_id.to_string());
        }
        None
    }
}

#[cfg(test)]
mod active_main_task_board_tests {
    use super::{AppState, FileWriteLockManager, ToolExecutionScope};
    use crate::models::DebugSessionSettings;
    use crate::platform_auth::{PlatformSession, PlatformUserSummary};
    use crate::task_board::{main_turn_task_board_store_key, sub_agent_task_board_store_key};
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    fn seed_running_child_board(state: &AppState, conv: &str, turn: &str, task_id: &str) -> String {
        let parent = main_turn_task_board_store_key(conv, turn);
        let child = sub_agent_task_board_store_key(&parent, task_id);
        state
            .task_board_store
            .apply(
                &child,
                "init",
                &json!({
                    "goal": "child goal",
                    "items": [{"id": "m1", "title": "S", "status": "pending"}]
                }),
            )
            .expect("init child");
        child
    }

    /// Point `storage::data_dir()` at a fresh temp dir so AppState tests never
    /// read/write the developer's real user_settings.json / provider_keys.enc.
    struct TestDataDirGuard {
        _dir: tempfile::TempDir,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    fn isolate_app_data_dir() -> TestDataDirGuard {
        let lock = crate::storage::test_app_data_dir_lock();
        let dir = tempfile::tempdir().expect("temp data dir");
        crate::storage::set_test_app_data_dir(dir.path().to_path_buf());
        TestDataDirGuard {
            _dir: dir,
            _lock: lock,
        }
    }

    /// 把插件主目录指向临时目录（防测试污染真实 `~/.pointer/plugins`）。
    /// 返回旧值以便测试结束后恢复。
    fn isolate_plugins_home() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().expect("temp plugins home");
        std::env::set_var("POINTER_HOME", tmp.path());
        tmp
    }

    /// 插件启用后：其技能自动加入 general 的 agentSkillOverrides（落盘），
    /// 且来源标注带 plugin_id（前端据此显示「插件 · superpowers」）。
    #[test]
    fn plugin_enable_auto_enables_plugin_skills() {
        let _guard = isolate_app_data_dir();
        let _plugins_home = isolate_plugins_home();
        let state = AppState::new();

        // 构造示例插件（含 skills/demo-skill）到用户插件目录
        let plugins_dir = crate::plugins::user_plugins_dir().expect("plugins dir");
        let plugin_id = "com.example.auto";
        crate::plugins::activation::write_example_plugin(&plugins_dir, plugin_id).expect("write");

        state.plugins.scan().expect("scan");
        state.plugin_enable(plugin_id).expect("enable");

        // 技能已注册且带 plugin_id
        let skill = state
            .skills
            .get("demo-skill")
            .expect("plugin skill registered");
        assert_eq!(skill.plugin_id.as_deref(), Some(plugin_id));
        assert_eq!(skill.provenance, "external");

        // 自动加入 general 启用列表并落盘
        let user = state.load_user_settings();
        let general = user
            .agent_skill_overrides
            .get("general")
            .cloned()
            .unwrap_or_default();
        assert!(
            general.contains(&"demo-skill".to_string()),
            "general={general:?}"
        );

        // 再次 enable 幂等（不重复追加）
        state.plugin_enable(plugin_id).expect("enable again");
        let user2 = state.load_user_settings();
        let general2 = user2
            .agent_skill_overrides
            .get("general")
            .cloned()
            .unwrap_or_default();
        let count = general2
            .iter()
            .filter(|s| s.as_str() == "demo-skill")
            .count();
        assert_eq!(count, 1, "idempotent: general2={general2:?}");
    }

    /// 禁用后重新启用：技能再次注册回 registry，且仍处于启用状态（幂等恢复）。
    #[test]
    fn plugin_disable_then_enable_restores_skills() {
        let _guard = isolate_app_data_dir();
        let _plugins_home = isolate_plugins_home();
        let state = AppState::new();

        let plugins_dir = crate::plugins::user_plugins_dir().expect("plugins dir");
        let plugin_id = "com.example.auto";
        crate::plugins::activation::write_example_plugin(&plugins_dir, plugin_id).expect("write");

        state.plugins.scan().expect("scan");
        state.plugin_enable(plugin_id).expect("enable");
        assert!(state.skills.get("demo-skill").is_some());
        assert!(state
            .load_user_settings()
            .agent_skill_overrides
            .get("general")
            .cloned()
            .unwrap_or_default()
            .contains(&"demo-skill".to_string()));

        // 禁用：技能从 registry 注销；启用列表保留（设计：禁用不清理）
        state.plugin_disable(plugin_id).expect("disable");
        assert!(state.skills.get("demo-skill").is_none());
        assert!(state
            .load_user_settings()
            .agent_skill_overrides
            .get("general")
            .cloned()
            .unwrap_or_default()
            .contains(&"demo-skill".to_string()));

        // 重新启用：技能恢复注册 + 仍在启用列表（幂等不重复）
        state.plugin_enable(plugin_id).expect("enable again");
        assert!(state.skills.get("demo-skill").is_some());
        let general = state
            .load_user_settings()
            .agent_skill_overrides
            .get("general")
            .cloned()
            .unwrap_or_default();
        assert!(
            general.contains(&"demo-skill".to_string()),
            "general={general:?}"
        );
        let count = general
            .iter()
            .filter(|s| s.as_str() == "demo-skill")
            .count();
        assert_eq!(count, 1, "no dup: general={general:?}");
    }

    /// 卸载插件后：其技能从所有 agent 的 agentSkillOverrides 移除（落盘），
    /// 技能 registry 注销；重新安装启用可再次自动加入。
    #[test]
    fn plugin_uninstall_removes_plugin_skills_from_overrides() {
        let _guard = isolate_app_data_dir();
        let _plugins_home = isolate_plugins_home();
        let state = AppState::new();

        let plugins_dir = crate::plugins::user_plugins_dir().expect("plugins dir");
        let plugin_id = "com.example.auto";
        crate::plugins::activation::write_example_plugin(&plugins_dir, plugin_id).expect("write");

        state.plugins.scan().expect("scan");
        state.plugin_enable(plugin_id).expect("enable");

        // 预置：general 已有 demo-skill；给 coder 也手动加一个
        {
            let mut user = state.load_user_settings();
            let overrides = user
                .agent_skill_overrides
                .entry("general".to_string())
                .or_default();
            if !overrides.contains(&"demo-skill".to_string()) {
                overrides.push("demo-skill".to_string());
            }
            let coder = user
                .agent_skill_overrides
                .entry("coder".to_string())
                .or_default();
            coder.push("demo-skill".to_string());
            coder.push("other-skill".to_string());
            state.save_user_settings(&user).expect("save");
        }

        state.plugin_uninstall(plugin_id).expect("uninstall");

        // 技能注册已注销
        assert!(state.skills.get("demo-skill").is_none());

        // general / coder 中 demo-skill 均被移除，非插件技能保留
        let user = state.load_user_settings();
        let general = user
            .agent_skill_overrides
            .get("general")
            .cloned()
            .unwrap_or_default();
        assert!(
            !general.contains(&"demo-skill".to_string()),
            "general={general:?}"
        );
        let coder = user
            .agent_skill_overrides
            .get("coder")
            .cloned()
            .unwrap_or_default();
        assert!(
            !coder.contains(&"demo-skill".to_string()),
            "coder={coder:?}"
        );
        assert!(
            coder.contains(&"other-skill".to_string()),
            "coder={coder:?}"
        );
    }

    #[test]
    fn app_state_loads_cached_platform_catalog_without_persisting_user_settings() {
        let _guard = isolate_app_data_dir();
        let catalog = HashMap::from([("aliyun_qwen".to_string(), vec!["qwen-cached".to_string()])]);
        let providers = vec![crate::platform_auth::PlatformProviderTemplate {
            id: "qwen".into(),
            name: "千问".into(),
            base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1".into(),
            models: vec![crate::platform_auth::PlatformProviderModel {
                name: "qwen-cached".into(),
                ..Default::default()
            }],
            ..Default::default()
        }];
        crate::storage::save_platform_model_catalog_cache_full(
            &catalog,
            &providers,
            &serde_json::Value::Null,
            None,
        )
        .expect("cache catalog");

        let state = AppState::new();
        let qwen = state
            .platform_config
            .read()
            .providers
            .iter()
            .find(|provider| provider.id == "qwen")
            .expect("qwen provider")
            .clone();
        assert_eq!(qwen.models, vec!["qwen-cached"]);
        assert_eq!(state.platform_config.read().model_catalog, catalog);
        assert!(!crate::storage::app_data_dir()
            .expect("data dir")
            .join("user_settings.json")
            .exists());
    }

    #[test]
    fn update_user_settings_preserves_encrypted_user_key_on_masked_roundtrip() {
        let _guard = isolate_app_data_dir();
        let state = AppState::new();
        // 1. User types a key → encrypted into provider_keys.enc.
        let mut user = state.load_user_settings();
        user.providers
            .push(provider_fixture("custom-a", "", Some("user")));
        user.providers[0].api_key = "sk-user-typed".into();
        state.update_user_settings(user).expect("save typed key");
        assert_eq!(
            state.load_user_settings().providers[0].api_key,
            "sk-user-typed"
        );
        // 2. WEB non-admin GET redacts apiKey to "****"; PUT round-trip must not
        //    wipe the encrypted key (update_user_settings re-attaches existing).
        let mut masked = state.load_user_settings();
        for provider in &mut masked.providers {
            provider.api_key = "****".into();
        }
        state.update_user_settings(masked).expect("save masked");
        assert_eq!(
            state.load_user_settings().providers[0].api_key,
            "sk-user-typed"
        );
    }

    #[test]
    fn update_user_settings_empty_key_keeps_existing_encrypted_key() {
        let _guard = isolate_app_data_dir();
        let state = AppState::new();
        let mut user = state.load_user_settings();
        user.providers
            .push(provider_fixture("custom-a", "", Some("user")));
        user.providers[0].api_key = "sk-user-typed".into();
        state.update_user_settings(user).expect("save typed key");
        // Empty apiKey (client did not edit) keeps the previously encrypted key.
        let mut blank = state.load_user_settings();
        for provider in &mut blank.providers {
            provider.api_key.clear();
        }
        state.update_user_settings(blank).expect("save blank");
        assert_eq!(
            state.load_user_settings().providers[0].api_key,
            "sk-user-typed"
        );
    }

    fn provider_fixture(
        id: &str,
        key: &str,
        source: Option<&str>,
    ) -> crate::models::ProviderConfig {
        crate::models::ProviderConfig {
            id: id.into(),
            name: id.into(),
            base_url: format!("https://{id}.example.com/v1"),
            api_key: key.into(),
            models: vec!["m1".into()],
            model_configs: Default::default(),
            reasoning_in_messages: None,
            temperature: None,
            top_p: None,
            max_tokens: None,
            context_budget_tokens: None,
            enable_thinking: None,
            thinking_budget: None,
            reasoning_effort: None,
            thinking_protocol: None,
            thinking_intensity: None,
            extra_body: None,
            source: source.map(str::to_string),
        }
    }

    #[test]
    fn custom_provider_key_survives_blank_resave() {
        let _guard = isolate_app_data_dir();
        let state = AppState::new();
        // 1. First save: custom provider with key.
        let mut user = state.load_user_settings();
        user.providers
            .push(provider_fixture("custom-a", "sk-custom-a", Some("user")));
        state.update_user_settings(user).expect("save custom key");
        // 2. Re-edit without typing key → front-end sends empty apiKey; backend
        //    re-attaches from its in-memory key pool (persisted json → memory).
        let mut blank = state.load_user_settings();
        for p in &mut blank.providers {
            if p.id == "custom-a" {
                p.api_key.clear();
            }
        }
        state.update_user_settings(blank).expect("save blank");
        let after = state.load_user_settings();
        let key = after
            .providers
            .iter()
            .find(|p| p.id == "custom-a")
            .unwrap()
            .api_key
            .clone();
        assert_eq!(
            key, "sk-custom-a",
            "user-layer custom key must survive blank resave"
        );
    }

    #[test]
    fn platform_injected_provider_key_survives_blank_resave() {
        let _guard = isolate_app_data_dir();
        let state = AppState::new();
        // Server.toml custom provider injected into platform (in-memory).
        state
            .platform_config
            .write()
            .providers
            .push(provider_fixture("vllm-local", "sk-vllm", Some("platform")));
        // Front-end merged view shows vllm-local (source=platform); user edits
        // models only, sends blank key.
        let mut user = state.load_user_settings();
        user.providers
            .push(provider_fixture("vllm-local", "", Some("platform")));
        state.update_user_settings(user).expect("save blank key");
        // Platform-injected key must not be persisted into user layer…
        let persisted = state.load_user_settings();
        assert!(
            !persisted.providers.iter().any(|p| p.id == "vllm-local"),
            "platform-only provider must not be written into user layer"
        );
        // …but the merged/effective view must still expose it from platform memory.
        let merged_key = state
            .effective_settings()
            .providers
            .iter()
            .find(|p| p.id == "vllm-local")
            .unwrap()
            .api_key
            .clone();
        assert_eq!(
            merged_key, "sk-vllm",
            "platform-injected key should survive blank resave"
        );
    }

    #[test]
    fn platform_provider_fork_with_new_key_persists_to_user_layer() {
        let _guard = isolate_app_data_dir();
        let state = AppState::new();
        // Server.toml custom provider injected into platform (in-memory).
        state
            .platform_config
            .write()
            .providers
            .push(provider_fixture("vllm-local", "sk-vllm", Some("platform")));
        // User edits the platform provider and explicitly types a new key → the
        // front-end marks it source=user (fork) and submits the new key.
        let mut user = state.load_user_settings();
        user.providers
            .push(provider_fixture("vllm-local", "sk-user-new", Some("user")));
        state.update_user_settings(user).expect("save fork");
        // Forked provider persists with the user's explicit key.
        let persisted = state.load_user_settings();
        let p = persisted
            .providers
            .iter()
            .find(|p| p.id == "vllm-local")
            .unwrap();
        assert_eq!(p.api_key, "sk-user-new");
    }

    #[test]
    fn debug_session_settings_replace_runtime_model_configuration() {
        let _guard = isolate_app_data_dir();
        let state = AppState::new();
        let merged = state.effective_settings();
        let mut debug = DebugSessionSettings::from(&merged);
        if debug.providers.is_empty() {
            debug
                .providers
                .push(provider_fixture("qwen", "", Some("user")));
        }
        let mut provider = debug.providers[0].clone();
        provider.id = "session-provider".into();
        provider.name = "Session Provider".into();
        provider.models = vec!["session-chat".into(), "session-worker".into()];
        provider.api_key = "session-secret".into();
        debug.providers = vec![provider];
        debug.active_provider_id = "session-provider".into();
        debug.model = "session-chat".into();
        debug.temperature = 0.42;
        debug.max_tokens = 4321;
        for config in debug.computer_tier_llm.values_mut() {
            config.provider_id = "session-provider".into();
            config.model = "session-worker".into();
        }
        for modes in debug.agent_mode_llm.values_mut() {
            for config in modes.values_mut() {
                config.provider_id = "session-provider".into();
                config.model = "session-worker".into();
            }
        }
        for modes in debug.media_mode_llm.values_mut() {
            for config in modes.values_mut() {
                config.provider_id = "session-provider".into();
                config.model = "session-worker".into();
            }
        }
        debug.computer_pipeline_llm.decision = "session-worker".into();
        debug.computer_pipeline_llm.position = "session-worker".into();
        debug.computer_pipeline_llm.verify = "session-worker".into();

        let denied = state
            .update_debug_session_settings(debug.clone())
            .expect_err("non-admin update must be rejected");
        assert!(denied.to_string().contains("platform admins"));
        state.platform_auth.set_session(PlatformSession {
            access_token: "test-admin".into(),
            refresh_token: String::new(),
            expires_at: i64::MAX,
            agent_id: "test-agent".into(),
            user: PlatformUserSummary {
                id: "test-admin".into(),
                nickname: Some("Test Admin".into()),
                is_platform_admin: true,
                included_tokens: 0,
                consumed_tokens: 0,
                token_quota_exhausted: false,
            },
        });
        let view = state
            .update_debug_session_settings(debug.clone())
            .expect("debug session update");

        // 会话级调试只更新 platform 内存 providers，不写入 user_settings.json。
        assert_eq!(
            state.platform_config.read().providers[0].id,
            "session-provider"
        );
        assert_eq!(state.platform_config.read().providers.len(), 1);
        // merged 仍来自持久化 user 层（平台默认），未被调试覆盖。
        assert_ne!(view.merged.active_provider_id, "session-provider");
        assert_ne!(view.merged.model, "session-chat");
        // 持久化 user_settings 未被污染：本地默认不再内置平台服务商。
        let persisted = state.load_user_settings();
        assert_eq!(persisted.providers.len(), 0);
        assert!(!persisted
            .providers
            .iter()
            .any(|p| p.id == "session-provider"));
        assert_ne!(persisted.active_provider_id, "session-provider");
    }

    #[test]
    fn get_active_main_task_board_key_skips_child_from_prefix_scan() {
        let state = AppState::new();
        let conv = "conv-active-child";
        seed_running_child_board(&state, conv, "user-1", "sub_task_x");
        assert_eq!(state.get_active_main_task_board_key(conv), None);
    }

    #[test]
    fn get_active_main_task_board_key_prefers_unfinished_parent_over_child() {
        let state = AppState::new();
        let conv = "conv-parent-wins";
        let turn = "user-1";
        let parent = main_turn_task_board_store_key(conv, turn);
        state
            .task_board_store
            .apply(
                &parent,
                "init",
                &json!({
                    "goal": "parent",
                    "items": [{"id": "m1", "title": "P", "status": "pending"}]
                }),
            )
            .expect("init parent");
        let child = seed_running_child_board(&state, conv, turn, "sub_task_y");
        assert_eq!(
            state.get_active_main_task_board_key(conv).as_deref(),
            Some(parent.as_str())
        );
        assert_ne!(
            state.get_active_main_task_board_key(conv).as_deref(),
            Some(child.as_str())
        );
    }

    #[test]
    fn set_active_main_task_board_key_rejects_child() {
        let state = AppState::new();
        let conv = "conv-reject-child";
        let parent = main_turn_task_board_store_key(conv, "user-1");
        let child = sub_agent_task_board_store_key(&parent, "sub1");
        state.set_active_main_task_board_key(conv, &child);
        assert_eq!(state.get_active_main_task_board_key(conv), None);
    }

    #[test]
    fn terminal_scope_keys_isolate_same_conversation_and_tool_call() {
        let first =
            ToolExecutionScope::new("conv-shared", Some("fork-instance-a"), "tool-call-shared");
        let second =
            ToolExecutionScope::new("conv-shared", Some("fork-instance-b"), "tool-call-shared");

        assert_ne!(first.abort_key(), second.abort_key());
        assert_ne!(first.output_trace_key(), second.output_trace_key());
        assert_ne!(
            first.pending_input_key("request-shared"),
            second.pending_input_key("request-shared")
        );
    }

    #[test]
    fn tool_execution_scope_prefers_sub_then_lead_then_legacy() {
        let from_sub = ToolExecutionScope::from_agent_contexts(
            "conv",
            Some("lead-instance"),
            Some("sub-instance"),
            "tc-1",
        );
        let from_lead =
            ToolExecutionScope::from_agent_contexts("conv", Some("lead-instance"), None, "tc-1");
        let legacy = ToolExecutionScope::from_agent_contexts("conv", None, None, "tc-1");

        assert!(from_sub.abort_key().contains("sub-instance"));
        assert!(!from_sub.abort_key().contains("lead-instance"));
        assert!(from_lead.abort_key().contains("lead-instance"));
        assert!(!from_lead.abort_key().contains("legacy"));
        assert!(legacy.abort_key().contains("legacy"));
        assert_ne!(from_lead.abort_key(), legacy.abort_key());
    }

    #[test]
    fn conversation_cancel_clears_pending_terminal_inputs_for_all_scopes() {
        let state = AppState::new();
        let first = ToolExecutionScope::new("conv-inputs", Some("fork-a"), "tc-a");
        let second = ToolExecutionScope::new("conv-inputs", Some("fork-b"), "tc-b");
        let other = ToolExecutionScope::new("conv-other", Some("fork-a"), "tc-a");
        let (tx_a, rx_a) = std::sync::mpsc::channel();
        let (tx_b, rx_b) = std::sync::mpsc::channel();
        let (tx_other, rx_other) = std::sync::mpsc::channel();

        state.register_terminal_input_wait(first, "req-a".into(), tx_a);
        state.register_terminal_input_wait(second, "req-b".into(), tx_b);
        state.register_terminal_input_wait(other, "req-other".into(), tx_other);

        state.cancel("conv-inputs");

        assert!(matches!(
            rx_a.try_recv(),
            Ok(crate::tools::terminal::TerminalInputResolution::Cancelled)
        ));
        assert!(matches!(
            rx_b.try_recv(),
            Ok(crate::tools::terminal::TerminalInputResolution::Cancelled)
        ));
        assert!(rx_other.try_recv().is_err());
    }

    #[test]
    fn conversation_cancel_aborts_every_terminal_scope() {
        let state = AppState::new();
        let first = ToolExecutionScope::new("conv-cancel", Some("fork-a"), "tool-call");
        let second = ToolExecutionScope::new("conv-cancel", Some("fork-b"), "tool-call");
        let unrelated = ToolExecutionScope::new("conv-other", Some("fork-a"), "tool-call");
        let first_flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let second_flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let unrelated_flag = Arc::new(std::sync::atomic::AtomicBool::new(false));

        state.register_terminal_abort_flag(first, first_flag.clone());
        state.register_terminal_abort_flag(second, second_flag.clone());
        state.register_terminal_abort_flag(unrelated, unrelated_flag.clone());

        assert!(state.abort_terminal_command("conv-cancel", None));
        assert!(first_flag.load(Ordering::SeqCst));
        assert!(second_flag.load(Ordering::SeqCst));
        assert!(!unrelated_flag.load(Ordering::SeqCst));
    }

    #[test]
    fn soft_cancel_keeps_background_jobs_and_skips_terminal_abort() {
        use crate::chat_service::job_supervisor::{JobKind, JobKindSubagent, JobStatus};
        use tokio_util::sync::CancellationToken;

        let state = AppState::new();
        let conv = "conv-soft";
        let job_token = CancellationToken::new();
        let job_id = state.jobs.register(
            conv,
            JobKind::Subagent(JobKindSubagent {
                tool_call_id: "tc".into(),
                message_id: "m".into(),
                agent_id: "explore".into(),
                title: "t".into(),
                agent_instance_id: "inst".into(),
            }),
            job_token.clone(),
            "run-soft",
        );
        state.jobs.mark_running(&job_id);

        let scope = ToolExecutionScope::new(conv, None, "tc-term");
        let abort_flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
        state.register_terminal_abort_flag(scope, abort_flag.clone());

        state.cancel_with_options(conv, false);

        assert!(!job_token.is_cancelled());
        assert!(!abort_flag.load(Ordering::SeqCst));
        assert_eq!(state.jobs.running_count_for_conversation(conv), 1);
        assert_eq!(
            state.jobs.list(conv, false)[0].status,
            JobStatus::Running.as_str()
        );

        state.cancel_with_options(conv, true);
        assert!(job_token.is_cancelled());
        assert!(abort_flag.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn file_write_locks_serialize_same_canonical_path() {
        let manager = Arc::new(FileWriteLockManager::default());
        let tmp = tempfile::tempdir().expect("tmp");
        let path = tmp.path().join("same.txt");
        std::fs::write(&path, "seed").expect("seed");
        let aliases = [path.clone(), tmp.path().join(".").join("same.txt")];
        let active = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let mut tasks = Vec::new();

        for path in aliases {
            let manager = manager.clone();
            let active = active.clone();
            let peak = peak.clone();
            tasks.push(tokio::spawn(async move {
                let (_guard, _, _) = manager.lock_path(&path).await.expect("lock");
                let now = active.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(now, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(20)).await;
                active.fetch_sub(1, Ordering::SeqCst);
            }));
        }
        for task in tasks {
            task.await.expect("task");
        }

        assert_eq!(peak.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn file_write_lock_normalizes_missing_target_from_canonical_parent() {
        let manager = FileWriteLockManager::default();
        let tmp = tempfile::tempdir().expect("tmp");
        let direct = tmp.path().join("future.txt");
        let alias = tmp.path().join(".").join("future.txt");

        let (first_guard, _, first_key) = manager.lock_path(&direct).await.expect("first lock");
        drop(first_guard);
        let (_second_guard, _, second_key) = manager.lock_path(&alias).await.expect("second lock");

        assert_eq!(first_key, second_key);
        assert!(first_key.is_absolute());
        assert!(first_key.ends_with("future.txt"));
    }

    #[tokio::test]
    async fn file_write_lock_rejects_parent_dir_components_for_missing_target() {
        let manager = FileWriteLockManager::default();
        let tmp = tempfile::tempdir().expect("tmp");
        let nested = tmp.path().join("nested");
        std::fs::create_dir_all(&nested).expect("nested");
        let sneaky = nested.join("..").join("..").join("outside.txt");

        let err = manager
            .lock_path(&sneaky)
            .await
            .expect_err("parent components must not bypass lock normalization");
        let message = format!("{err:#}");
        assert!(
            message.contains("非法组件") || message.contains("无法"),
            "unexpected error: {message}"
        );
    }

    #[tokio::test]
    async fn file_write_locks_allow_different_canonical_paths_to_overlap() {
        let manager = Arc::new(FileWriteLockManager::default());
        let tmp = tempfile::tempdir().expect("tmp");
        let paths = [tmp.path().join("first.txt"), tmp.path().join("second.txt")];
        let active = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let barrier = Arc::new(tokio::sync::Barrier::new(2));
        let mut tasks = Vec::new();

        for path in paths {
            let manager = manager.clone();
            let active = active.clone();
            let peak = peak.clone();
            let barrier = barrier.clone();
            tasks.push(tokio::spawn(async move {
                let (_guard, _, _) = manager.lock_path(&path).await.expect("lock");
                let now = active.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(now, Ordering::SeqCst);
                barrier.wait().await;
                active.fetch_sub(1, Ordering::SeqCst);
            }));
        }
        for task in tasks {
            task.await.expect("task");
        }

        assert_eq!(peak.load(Ordering::SeqCst), 2);
    }
}

#[cfg(test)]
mod automation_execution_auth_tests {
    use super::AppState;
    use crate::local_auth::{create_local_auth_manager, empty_local_credentials};
    use crate::platform_auth::PlatformLoginCredentials;
    use crate::web_request_auth::{WebSessionAuth, WebSessionAuthKind};

    /// Point `storage::data_dir()` at a fresh temp dir so AppState tests never
    /// read/write the developer's real user_settings.json / provider_keys.enc.
    struct TestDataDirGuard {
        _dir: tempfile::TempDir,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    fn isolate_app_data_dir() -> TestDataDirGuard {
        let lock = crate::storage::test_app_data_dir_lock();
        let dir = tempfile::tempdir().expect("temp data dir");
        crate::storage::set_test_app_data_dir(dir.path().to_path_buf());
        TestDataDirGuard {
            _dir: dir,
            _lock: lock,
        }
    }

    #[test]
    fn automation_execution_auth_returns_local_session_without_platform_llm_keys() {
        let _guard = isolate_app_data_dir();
        let state = AppState::new();
        let auth = create_local_auth_manager();
        let creds = empty_local_credentials();
        assert!(!crate::platform_auth::credentials_have_llm_keys(&creds));
        state.set_automation_web_session(Some(WebSessionAuth {
            kind: WebSessionAuthKind::Local,
            auth,
            creds,
        }));
        let got = state
            .automation_execution_auth()
            .expect("local session should be usable for headless runs");
        assert_eq!(got.kind, WebSessionAuthKind::Local);
    }

    #[test]
    fn automation_execution_auth_skips_platform_session_without_llm_keys() {
        let _guard = isolate_app_data_dir();
        let state = AppState::new();
        state.set_automation_web_session(Some(WebSessionAuth {
            kind: WebSessionAuthKind::Platform,
            auth: state.platform_auth.clone(),
            creds: PlatformLoginCredentials::default(),
        }));
        assert!(state.automation_execution_auth().is_none());
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}
