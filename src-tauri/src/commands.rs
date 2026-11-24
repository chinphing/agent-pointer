use pointer_core::agents::computer::capture_debug;
use pointer_core::agents::AgentDef;
use pointer_core::chat_service::AppState;
use pointer_core::chat_service::GlobalMcpView;
use pointer_core::dispatcher::{
    DeliverTarget, RunDispatcher, RunQueueSnapshot, TriggerMeta, TriggerRequest, TriggerSource,
};
use pointer_core::models::{
    ChatMediaPreview, ChatMessage, ComputerAnnotatedPreview, ComputerMonitor, Conversation,
    ConversationSearchHit, DebugSessionSettings, EffectiveSettingsView, PlatformSettings, Project,
    ProjectCreationResult, ProjectCursor, ProjectPage, SendChatPayload, SkillDef,
    SkillImportResult, ToolDef, UserSettings,
};
use pointer_core::plugins::registry::PluginView;

use base64::Engine;
use pointer_core::provider::OpenAIProvider;
use pointer_core::storage;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use tauri::{AppHandle, State};

pub const STREAM_EVENT: &str = "chat://stream";

fn platform_list_scope(state: &AppState) -> pointer_core::conversation_store::ListScope {
    let auth = state.active_platform_auth();
    // 与 Web 端 `platform_list_scope` 同一判定：`AppState::is_admin_session`。
    pointer_core::conversation_store::ListScope::from_viewer(
        state.is_admin_session(),
        &auth.platform_user_id().unwrap_or_default(),
    )
}

/// 面板命令的执行环境：按会话解析并绑定（沙箱档位下工作区在沙箱里）。
///
/// 与 Web 端 `/api/workspace/*` 走同一个入口
/// （[`pointer_core::env::session_env::bind_workspace_request`]）。面板请求不是对话
/// 回合，线程上没有会话绑定，**不能**退回 `effective_env()`——沙箱档位部署下那是
/// 「拒绝执行」环境，连解析成 local 档位的 admin 都会报「执行环境未绑定」。
///
/// 没有会话 id 时由同一入口按身份 + 部署档位解析：local 档位照旧用宿主，
/// 沙箱档位显式报错（无法凭空定位沙箱），绝不静默读宿主。
fn panel_binding(
    conversation_id: Option<&str>,
    workspace_root: &str,
) -> Result<pointer_core::env::session_env::WorkspaceRequestEnv, String> {
    let id = conversation_id.map(str::trim).filter(|id| !id.is_empty());
    if let Some(id) = id {
        if pointer_core::env::session_env::env_for_conversation(id).is_none() {
            let cfg = pointer_core::env::provider::ProviderConfig::from_env();
            if let Ok(cfg) = cfg {
                if let Ok(resolved) = cfg.resolve(None) {
                    if resolved.backend == pointer_core::env::provider::Backend::Docker {
                        return Err(
                            "会话环境还没有建立，无法打开工作区。请先发送一条消息。".to_string()
                        );
                    }
                }
            }
        }
    }
    pointer_core::env::session_env::bind_workspace_request(
        id,
        id.map(pointer_core::user_storage::session_user_id_for_conversation)
            .as_deref(),
        Path::new(workspace_root),
    )
    .map_err(|err| format!("{err:#}"))
}

#[tauri::command]
pub async fn list_workspace_directory(
    conversation_id: Option<String>,
    workspace_root: String,
    relative_path: Option<String>,
) -> Result<Vec<pointer_core::workspace_read::WorkspaceEntry>, String> {
    let session = {
        let bound = panel_binding(conversation_id.as_deref(), &workspace_root)?;
        bound.session()
    };
    let result = tauri::async_runtime::spawn_blocking(move || {
        let _guard = pointer_core::env::EnvironmentGuard::enter(session.clone());
        pointer_core::workspace_read::list_directory(
            &*session,
            Path::new(&workspace_root),
            relative_path.as_deref(),
        )
        .map_err(|e| format!("{e:#}"))
    })
    .await
    .map_err(|e| format!("spawn_blocking failed: {e}"))?;
    result
}

#[tauri::command]
pub fn search_workspace_entries(
    conversation_id: Option<String>,
    workspace_root: String,
    query: String,
    limit: Option<u32>,
) -> Result<Vec<pointer_core::workspace_read::WorkspaceEntry>, String> {
    let limit = limit.unwrap_or(80).clamp(1, 200) as usize;
    let bound = panel_binding(conversation_id.as_deref(), &workspace_root)?;
    pointer_core::workspace_read::search_entries(
        bound.env(),
        Path::new(&workspace_root),
        &query,
        limit,
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn read_workspace_file(
    conversation_id: Option<String>,
    workspace_root: String,
    relative_path: String,
) -> Result<pointer_core::workspace_read::WorkspaceFilePreview, String> {
    let session = {
        let bound = panel_binding(conversation_id.as_deref(), &workspace_root)?;
        bound.session()
    };
    let result = tauri::async_runtime::spawn_blocking(move || {
        let _guard = pointer_core::env::EnvironmentGuard::enter(session.clone());
        pointer_core::workspace_read::read_file(
            &*session,
            Path::new(&workspace_root),
            &relative_path,
        )
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("spawn_blocking failed: {e}"))?;
    result
}

#[tauri::command]
pub fn delete_workspace_path(
    conversation_id: Option<String>,
    workspace_root: String,
    relative_path: String,
) -> Result<(), String> {
    let bound = panel_binding(conversation_id.as_deref(), &workspace_root)?;
    pointer_core::workspace_read::delete_path(
        bound.env(),
        Path::new(&workspace_root),
        &relative_path,
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn workspace_policy() -> Result<pointer_core::workspace_resolve::WorkspacePolicy, String>
{
    let uid = pointer_core::session_user_env::current_session_user_id().unwrap_or_default();
    tauri::async_runtime::spawn_blocking(move || {
        pointer_core::workspace_service::policy_for_user(&uid).map_err(|e| format!("{e:#}"))
    })
    .await
    .map_err(|e| format!("spawn_blocking failed: {e}"))?
}

#[tauri::command]
pub async fn workspace_list_dirs(
    parent: Option<String>,
    conversation_id: Option<String>,
) -> Result<pointer_core::workspace_service::WorkspaceListResult, String> {
    let uid = pointer_core::session_user_env::current_session_user_id().unwrap_or_default();
    let cid = conversation_id
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| format!("browse-{uid}"));
    tauri::async_runtime::spawn_blocking(move || {
        let env = pointer_core::workspace_service::browse_env_for_user(&cid, &uid)
            .map_err(|e| format!("{e:#}"))?;
        pointer_core::workspace_service::list_directories(&uid, parent.as_deref(), env)
            .map_err(|e| format!("{e:#}"))
    })
    .await
    .map_err(|e| format!("spawn_blocking failed: {e}"))?
}

#[tauri::command]
pub async fn workspace_create_dir(
    parent: String,
    name: String,
    conversation_id: Option<String>,
) -> Result<pointer_core::workspace_service::WorkspaceDirEntry, String> {
    let uid = pointer_core::session_user_env::current_session_user_id().unwrap_or_default();
    let cid = conversation_id
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| format!("browse-{uid}"));
    tauri::async_runtime::spawn_blocking(move || {
        let env = pointer_core::workspace_service::browse_env_for_user(&cid, &uid)
            .map_err(|e| format!("{e:#}"))?;
        pointer_core::workspace_service::create_directory(&uid, &parent, &name, env)
            .map_err(|e| format!("{e:#}"))
    })
    .await
    .map_err(|e| format!("spawn_blocking failed: {e}"))?
}

#[tauri::command]
pub async fn workspace_select(
    path: String,
    conversation_id: Option<String>,
) -> Result<pointer_core::workspace_service::WorkspaceSelectResult, String> {
    let uid = pointer_core::session_user_env::current_session_user_id().unwrap_or_default();
    let cid = conversation_id
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| format!("browse-{uid}"));
    tauri::async_runtime::spawn_blocking(move || {
        let env = pointer_core::workspace_service::browse_env_for_user(&cid, &uid)
            .map_err(|e| format!("{e:#}"))?;
        pointer_core::workspace_service::select_workspace(
            &uid,
            &pointer_core::workspace_resolve::WorkspaceSelection { path },
            env,
        )
        .map_err(|e| format!("{e:#}"))
    })
    .await
    .map_err(|e| format!("spawn_blocking failed: {e}"))?
}

#[tauri::command]
pub async fn worktree_create(
    conversation_id: String,
    source_workspace_root: Option<String>,
    base: Option<String>,
) -> Result<pointer_core::worktree::ManagedWorktree, String> {
    tauri::async_runtime::spawn_blocking(move || {
        pointer_core::worktree::session::create_for_conversation(
            &conversation_id,
            source_workspace_root.as_deref(),
            base.as_deref(),
        )
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("spawn_blocking failed: {e}"))?
}

#[tauri::command]
pub async fn worktree_inspect(
    workspace_root: String,
    conversation_id: Option<String>,
) -> Result<Option<pointer_core::worktree::WorktreeInfo>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        pointer_core::worktree::session::inspect_workspace(
            &workspace_root,
            conversation_id.as_deref(),
        )
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("spawn_blocking failed: {e}"))?
}

/// 沙箱桌面入口（容器档位）。没有桌面时返回 `None`。
///
/// `rename_all = "camelCase"`：前端读的是 `wsUrl`。少了这条，`ws_url` 会以
/// snake_case 发过去，前端拿到 `undefined`，误判成「镜像不支持接管」。
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxDesktop {
    /// `vnc://127.0.0.1:<port>`，交给系统屏幕共享。
    pub url: String,
    /// `ws://127.0.0.1:<port>/websockify`，面板内 noVNC 直连。
    pub ws_url: Option<String>,
    pub password: Option<String>,
}

/// 桌面类命令专用的环境解析：只认**已经存在**的容器，不抢建沙箱。
///
/// 不能复用 `panel_binding`——它在 docker 后端故意返回宿主环境（面板读文件要的是同一份
/// bind mount），而桌面必须拿到容器本身。
///
/// 两步：
/// 1. `env_for_conversation_peek` —— 只查不刷新活跃时间。面板每隔几秒轮询一次，
///    用普通查询会把 `last_used` 一直顶到"刚用过"，30 分钟空闲回收永远不触发。
/// 2. 缓存空（应用重启过）→ 收养已在跑的容器，避免"容器还在、按钮却没了"。
fn desktop_env(
    conversation_id: Option<&str>,
) -> Option<std::sync::Arc<dyn pointer_core::env::Environment>> {
    let id = conversation_id.map(str::trim).filter(|id| !id.is_empty())?;
    if let Some(env) = pointer_core::env::session_env::env_for_conversation_peek(id) {
        return Some(env);
    }
    let cfg = pointer_core::env::provider::ProviderConfig::from_env().ok()?;
    let resolved = cfg.resolve(None).ok()?;
    if resolved.backend != pointer_core::env::provider::Backend::Docker {
        return None;
    }
    pointer_core::env::session_env::adopt_docker_env(&cfg, id, "", None)
}

/// 接管页打开/关闭时调用：pin 住容器，避免正在接管时被空闲回收掉。
#[tauri::command]
pub async fn set_sandbox_desktop_active(
    conversation_id: String,
    active: bool,
) -> Result<(), String> {
    let id = conversation_id.trim();
    if id.is_empty() {
        return Ok(());
    }
    pointer_core::env::session_env::set_pinned(id, active);
    Ok(())
}

#[tauri::command]
pub async fn get_sandbox_desktop(
    conversation_id: Option<String>,
    _workspace_root: String,
) -> Result<Option<SandboxDesktop>, String> {
    let Some(env) = desktop_env(conversation_id.as_deref()) else {
        return Ok(None);
    };
    Ok(env.desktop_endpoint().map(|ep| SandboxDesktop {
        url: ep.vnc_url,
        ws_url: ep.ws_url,
        password: ep.password,
    }))
}

/// 打开沙箱桌面（macOS 用「屏幕共享」）。面板内接管连不上时的退路。
#[tauri::command]
pub async fn open_sandbox_desktop(
    conversation_id: Option<String>,
    _workspace_root: String,
) -> Result<Option<String>, String> {
    let Some(env) = desktop_env(conversation_id.as_deref()) else {
        return Ok(None);
    };
    let Some(endpoint) = env.desktop_endpoint() else {
        return Ok(None);
    };
    let url = endpoint.vnc_url;
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(target_os = "windows") {
        "explorer"
    } else {
        "xdg-open"
    };
    std::process::Command::new(opener)
        .arg(&url)
        .spawn()
        .map_err(|e| format!("打开桌面失败: {e}"))?;
    Ok(Some(url))
}

#[tauri::command]
pub async fn get_workspace_git_status(
    conversation_id: Option<String>,
    workspace_root: String,
) -> Result<pointer_core::workspace_read::GitStatusResponse, String> {
    let session = {
        let bound = panel_binding(conversation_id.as_deref(), &workspace_root)?;
        bound.session()
    };
    let result = tauri::async_runtime::spawn_blocking(move || {
        let _guard = pointer_core::env::EnvironmentGuard::enter(session.clone());
        pointer_core::workspace_read::git_status_response(
            &*session,
            Path::new(&workspace_root),
        )
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("spawn_blocking failed: {e}"))?;
    result
}

#[tauri::command]
pub async fn get_workspace_git_diff(
    conversation_id: Option<String>,
    workspace_root: String,
    relative_path: String,
    status: Option<String>,
) -> Result<pointer_core::workspace_read::GitDiff, String> {
    let session = {
        let bound = panel_binding(conversation_id.as_deref(), &workspace_root)?;
        bound.session()
    };
    let result = tauri::async_runtime::spawn_blocking(move || {
        let _guard = pointer_core::env::EnvironmentGuard::enter(session.clone());
        pointer_core::workspace_read::git_diff(
            &*session,
            Path::new(&workspace_root),
            &relative_path,
            status.as_deref(),
        )
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("spawn_blocking failed: {e}"))?;
    result
}

#[tauri::command]
pub async fn get_turn_file_diff(
    conversation_id: String,
    turn_id: String,
    workspace_root: String,
    path: String,
) -> Result<pointer_core::turn_file_baseline::TurnFileDiff, String> {
    let result = tauri::async_runtime::spawn_blocking(move || {
        pointer_core::turn_file_baseline::turn_file_diff(
            &conversation_id,
            &turn_id,
            Path::new(&workspace_root),
            &path,
        )
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("spawn_blocking failed: {e}"))?;
    result
}

#[tauri::command]
pub async fn list_turn_file_changes(
    conversation_id: String,
    turn_ids: Vec<String>,
) -> Result<Vec<pointer_core::turn_file_baseline::TurnFileChangesForTurn>, String> {
    let result = tauri::async_runtime::spawn_blocking(move || {
        pointer_core::turn_file_baseline::list_turn_file_changes(&conversation_id, &turn_ids)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("spawn_blocking failed: {e}"))?;
    result
}

#[tauri::command]
pub async fn save_turn_file_changes(
    conversation_id: String,
    turn_id: String,
    files: Vec<pointer_core::turn_file_baseline::TurnFileChangeEntry>,
) -> Result<(), String> {
    let result = tauri::async_runtime::spawn_blocking(move || {
        pointer_core::turn_file_baseline::save_turn_file_changes(&conversation_id, &turn_id, &files)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("spawn_blocking failed: {e}"))?;
    result
}

/// Build a [`TriggerRequest`] from the IPC payload. Centralized so the IPC
/// trigger source stays consistent with HTTP / webhook / cron paths.
pub(crate) fn trigger_request_from_payload(payload: SendChatPayload) -> TriggerRequest {
    TriggerRequest {
        run_id: None,
        idempotency_key: None,
        conversation_id: Some(payload.conversation_id),
        trigger_source: TriggerSource::Ipc,
        trigger_meta: TriggerMeta::empty(),
        lane: None,
        messages: payload.messages,
        enabled_skill_ids: payload.enabled_skill_ids,
        agent_skill_overrides: payload.agent_skill_overrides,
        lead_agent_id: payload.lead_agent_id,
        performance_mode: payload.performance_mode,
        tool_rounds_used_single_start: payload.tool_rounds_used,
        workspace_root: payload.workspace_root,
        workspace_inherit_disabled: payload.workspace_inherit_disabled,
        deliver: DeliverTarget::None,
        web_session_auth: None,
        access_host: None,
    }
}

#[tauri::command]
pub async fn send_chat(
    _app: AppHandle,
    dispatcher: State<'_, Arc<RunDispatcher>>,
    payload: SendChatPayload,
) -> Result<(), String> {
    let d = dispatcher.inner().clone();
    tauri::async_runtime::spawn(async move {
        let req = trigger_request_from_payload(payload);
        if let Err(e) = d.dispatch(req).await {
            log::error!("send_chat: dispatch failed: {e:#}");
        }
    });
    Ok(())
}

#[tauri::command]
pub async fn cancel_chat(
    dispatcher: State<'_, Arc<RunDispatcher>>,
    conversation_id: String,
    cancel_background_jobs: Option<bool>,
) -> Result<(), String> {
    let cancel_bg = cancel_background_jobs.unwrap_or(true);
    dispatcher
        .cancel_conversation_and_wait(&conversation_id, cancel_bg)
        .await;
    Ok(())
}

/// Cancel background jobs by id (omit / empty = all in conversation). Does not stop the lead turn.
#[tauri::command]
pub fn cancel_background_jobs(
    state: State<'_, Arc<AppState>>,
    conversation_id: String,
    job_ids: Option<Vec<String>>,
) -> Result<Vec<String>, String> {
    let ids = job_ids.as_deref().filter(|v| !v.is_empty());
    Ok(state.cancel_background_jobs(&conversation_id, ids))
}

#[tauri::command]
pub fn create_console_session(
    state: State<'_, Arc<AppState>>,
    workspace_root: String,
    conversation_id: String,
    cwd: Option<String>,
    cols: u16,
    rows: u16,
) -> Result<pointer_core::console_session::ConsoleSessionInfo, String> {
    // 档位判定要用真实身份：桌面端就是本地操作者（`owner_user_id` 的 standalone 回落）。
    // 取不到就给空串，交给 core 拒绝（既不在宿主执行，也不建沙箱）。
    let session_user_id = state.runtime_settings_user_id().unwrap_or_default();
    // 归属口径与 Web 路由一致：管理员可以打开别人的会话，档位仍按请求身份解析。
    let request_is_admin = state.is_admin_session();
    state
        .console_sessions
        .create(
            &workspace_root,
            &conversation_id,
            &session_user_id,
            request_is_admin,
            cwd.as_deref(),
            cols,
            rows,
        )
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn write_console_session(
    state: State<'_, Arc<AppState>>,
    session_id: String,
    data: String,
) -> Result<(), String> {
    state
        .console_sessions
        .write(&session_id, &data)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn resize_console_session(
    state: State<'_, Arc<AppState>>,
    session_id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    state
        .console_sessions
        .resize(&session_id, cols, rows)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn close_console_session(
    state: State<'_, Arc<AppState>>,
    session_id: String,
) -> Result<bool, String> {
    Ok(state.console_sessions.close(&session_id))
}

/// Kill only the in-flight **`terminal`** subprocess for this conversation (does not stop the LLM turn).
#[tauri::command]
pub fn abort_terminal_command(
    state: State<'_, Arc<AppState>>,
    conversation_id: String,
    tool_call_id: Option<String>,
) -> Result<bool, String> {
    Ok(state.abort_terminal_command(&conversation_id, tool_call_id.as_deref()))
}

#[tauri::command]
pub fn approve_tool_call(
    state: State<'_, Arc<AppState>>,
    tool_call_id: String,
    approved: bool,
) -> Result<(), String> {
    if state.approve_tool_call(&tool_call_id, approved) {
        Ok(())
    } else {
        Err("未找到待审批的工具调用".into())
    }
}

#[tauri::command]
pub fn submit_ask_user(
    state: State<'_, Arc<AppState>>,
    tool_call_id: String,
    selected: Vec<String>,
) -> Result<(), String> {
    if state.submit_ask_user(&tool_call_id, selected) {
        Ok(())
    } else {
        Err("未找到待选择的 ask_user 请求".into())
    }
}

#[tauri::command]
pub fn submit_terminal_input(
    state: State<'_, Arc<AppState>>,
    request_id: String,
    text: String,
) -> Result<(), String> {
    if state.submit_terminal_input(&request_id, text) {
        Ok(())
    } else {
        Err("未找到待输入的终端请求".into())
    }
}

#[tauri::command]
pub fn dismiss_terminal_input(
    state: State<'_, Arc<AppState>>,
    request_id: String,
) -> Result<(), String> {
    if state.dismiss_terminal_input(&request_id) {
        Ok(())
    } else {
        Err("未找到待输入的终端请求".into())
    }
}

#[tauri::command]
pub fn get_settings(state: State<'_, Arc<AppState>>) -> Result<EffectiveSettingsView, String> {
    Ok(state.effective_settings_view())
}

#[tauri::command]
pub fn update_user_settings(
    state: State<'_, Arc<AppState>>,
    user: UserSettings,
) -> Result<EffectiveSettingsView, String> {
    state.update_user_settings(user).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_platform_settings(
    state: State<'_, Arc<AppState>>,
    dispatcher: State<'_, Arc<RunDispatcher>>,
    platform: PlatformSettings,
) -> Result<EffectiveSettingsView, String> {
    let view = state
        .update_platform_settings(platform)
        .map_err(|e| e.to_string())?;
    state.sync_dispatcher_concurrency(&dispatcher);
    Ok(view)
}

#[tauri::command]
pub fn update_debug_session_settings(
    state: State<'_, Arc<AppState>>,
    settings: DebugSessionSettings,
) -> Result<DebugSessionSettings, String> {
    let view = state
        .update_debug_session_settings(settings)
        .map_err(|error| {
            log::warn!("debug_session_settings: desktop update failed: {error:#}");
            error.to_string()
        })?;
    Ok(DebugSessionSettings::from(&view.merged))
}

#[tauri::command]
pub fn set_api_key(_api_key: String) -> Result<(), String> {
    Ok(())
}

#[tauri::command]
pub fn clear_api_key() -> Result<(), String> {
    Ok(())
}

#[tauri::command]
pub async fn test_connection(state: State<'_, Arc<AppState>>) -> Result<u128, String> {
    let settings = state.effective_settings();
    if settings.api_key.is_empty() {
        return Err("尚未配置 API Key".into());
    }
    let api_key = settings.api_key.clone();
    let provider = OpenAIProvider::new(settings, api_key);
    provider.test().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_skills(state: State<'_, Arc<AppState>>) -> Result<Vec<SkillDef>, String> {
    Ok(state.skills.list())
}

#[tauri::command]
pub fn reload_skill_meta(state: State<'_, Arc<AppState>>) -> Result<Vec<SkillDef>, String> {
    state.init_launch().map_err(|e| e.to_string())?;
    Ok(state.skills.list())
}

#[tauri::command]
pub fn import_skill_zip(
    state: State<'_, Arc<AppState>>,
    zip_data: Vec<u8>,
) -> Result<SkillImportResult, String> {
    state
        .skills
        .import_zip(&zip_data)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn probe_external_skills(
) -> Result<pointer_core::skills::external_probe::ExternalSkillsProbeResult, String> {
    pointer_core::skills::external_probe::probe_external_skill_sources().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn import_external_skills(
    state: State<'_, Arc<AppState>>,
    source_ids: Vec<String>,
) -> Result<SkillImportResult, String> {
    let result = pointer_core::skills::external_probe::import_external_skills(&source_ids)
        .map_err(|e| e.to_string())?;
    state.skills.reload_meta().map_err(|e| e.to_string())?;
    Ok(result)
}

#[tauri::command]
pub fn dismiss_external_skills_prompt() -> Result<(), String> {
    pointer_core::skills::external_probe::dismiss_external_skills_prompt()
        .map_err(|e| e.to_string())
}

// ---- Plugin management commands (P1) ----

#[tauri::command]
pub fn list_plugins(state: State<'_, Arc<AppState>>) -> Result<Vec<PluginView>, String> {
    Ok(state
        .plugin_list()
        .iter()
        .map(|r| {
            let mut v = PluginView::from_record(r);
            // P2③：MCP 连续重启失败进入 degraded（UI 插件状态显示）。
            if state.mcp_sessions.is_degraded(&r.id) {
                v.status = "degraded".to_string();
            }
            v
        })
        .collect())
}

// ---- 企业下发插件（按需安装）----

/// 企业目录里的一条插件。`installed=false` 表示只在清单里，还没下载到本地。
#[derive(serde::Serialize)]
pub struct OrgPluginView {
    pub slug: String,
    pub name: String,
    #[serde(rename = "pluginId")]
    pub plugin_id: Option<String>,
    pub version: String,
    #[serde(rename = "sizeBytes")]
    pub size_bytes: i64,
    /// `optional` / `default_on`
    pub activation: String,
    pub installed: bool,
}

/// 列出企业提供的插件（读上次同步下来的目录缓存，不联网）。
#[tauri::command]
pub fn list_org_plugins(state: State<'_, Arc<AppState>>) -> Result<Vec<OrgPluginView>, String> {
    use pointer_core::skills::org_sync::{read_catalog_cache, PackageKind};
    let installed: std::collections::HashSet<String> =
        state.plugin_list().iter().map(|r| r.id.clone()).collect();
    Ok(read_catalog_cache(PackageKind::Plugin)
        .into_iter()
        .map(|item| OrgPluginView {
            installed: item
                .plugin_id
                .as_deref()
                .map(|id| installed.contains(id))
                .unwrap_or(false),
            slug: item.slug,
            name: item.name,
            plugin_id: item.plugin_id,
            version: item.version,
            size_bytes: item.size_bytes,
            activation: item.activation,
        })
        .collect())
}

/// 按需安装一个企业插件（`optional` 的走这里；`default_on` 后台已自动下过）。
#[tauri::command]
pub async fn install_org_plugin(
    state: State<'_, Arc<AppState>>,
    slug: String,
) -> Result<(), String> {
    use pointer_core::skills::org_sync::{install_package, PackageKind};
    install_package(state.platform_auth.as_ref(), PackageKind::Plugin, &slug)
        .await
        .map_err(|e| e.to_string())?;
    state.plugins.scan().map_err(|e| e.to_string())?;
    state.apply_plugins();
    Ok(())
}

fn view_plugin(state: &AppState, plugin_id: &str) -> Result<PluginView, String> {
    let record = state
        .plugins
        .get(plugin_id)
        .ok_or_else(|| format!("插件不存在: {plugin_id}"))?;
    let mut v = PluginView::from_record(&record);
    if state.mcp_sessions.is_degraded(plugin_id) {
        v.status = "degraded".to_string();
    }
    Ok(v)
}

#[tauri::command]
pub fn enable_plugin(
    state: State<'_, Arc<AppState>>,
    plugin_id: String,
) -> Result<PluginView, String> {
    state.plugin_enable(&plugin_id).map_err(|e| e.to_string())?;
    view_plugin(&state, &plugin_id)
}

#[tauri::command]
pub fn disable_plugin(
    state: State<'_, Arc<AppState>>,
    plugin_id: String,
) -> Result<PluginView, String> {
    state
        .plugin_disable(&plugin_id)
        .map_err(|e| e.to_string())?;
    view_plugin(&state, &plugin_id)
}

#[tauri::command]
pub fn uninstall_plugin(state: State<'_, Arc<AppState>>, plugin_id: String) -> Result<(), String> {
    state
        .plugin_uninstall(&plugin_id)
        .map_err(|e| e.to_string())
}

// ---- Global MCP management commands (P2b) ----

#[tauri::command]
pub fn list_mcp_servers(state: State<'_, Arc<AppState>>) -> Result<GlobalMcpView, String> {
    Ok(state.global_mcp_view())
}

#[tauri::command]
pub fn save_mcp_servers(
    state: State<'_, Arc<AppState>>,
    servers: Vec<pointer_core::plugins::manifest::McpServerDecl>,
) -> Result<GlobalMcpView, String> {
    state
        .save_global_mcp_servers(servers)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn reload_mcp_servers(state: State<'_, Arc<AppState>>) -> Result<GlobalMcpView, String> {
    state
        .reload_global_mcp_from_config()
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn restart_mcp_server(state: State<'_, Arc<AppState>>) -> Result<GlobalMcpView, String> {
    let (decls, base_dir) = {
        let cfg = state.global_mcp.read();
        (cfg.decls.clone(), cfg.base_dir.clone())
    };
    state
        .reload_global_mcp(decls, base_dir)
        .map_err(|e| e.to_string())?;
    Ok(state.global_mcp_view())
}

#[tauri::command]
pub fn import_plugin(
    state: State<'_, Arc<AppState>>,
    source: String,
) -> Result<Vec<pointer_core::plugins::importer::ImportReport>, String> {
    let source_path = std::path::PathBuf::from(source.trim());
    state.plugin_import(&source_path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn import_plugin_zip(
    state: State<'_, Arc<AppState>>,
    zip_data: Vec<u8>,
) -> Result<Vec<pointer_core::plugins::importer::ImportReport>, String> {
    state
        .plugin_import_zip(&zip_data)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn discover_plugins(
    state: State<'_, Arc<AppState>>,
    dir: String,
) -> Result<Vec<pointer_core::plugins::importer::DiscoveredPlugin>, String> {
    let dir_path = std::path::PathBuf::from(dir.trim());
    state.plugin_discover(&dir_path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn probe_external_plugins(
    state: State<'_, Arc<AppState>>,
) -> Result<pointer_core::plugins::external_probe::ExternalPluginsProbeResult, String> {
    state.plugin_probe_external().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn import_external_plugin(
    state: State<'_, Arc<AppState>>,
    source_id: String,
) -> Result<pointer_core::plugins::importer::ImportReport, String> {
    state
        .plugin_import_external(&source_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_tools(state: State<'_, Arc<AppState>>) -> Result<Vec<ToolDef>, String> {
    Ok(state.tools.list_defs())
}

#[tauri::command]
pub fn list_agents(state: State<'_, Arc<AppState>>) -> Result<Vec<AgentDef>, String> {
    Ok(state.agents.list())
}

#[tauri::command]
pub fn get_task_board_snapshot(
    state: State<'_, Arc<AppState>>,
    conversation_id: String,
    task_id: Option<String>,
) -> Result<serde_json::Value, String> {
    use pointer_core::task_board::resolve_store_key_for_read;
    let parent_key = state
        .get_active_main_task_board_key(&conversation_id)
        .unwrap_or_else(|| conversation_id.clone());
    let store_key = resolve_store_key_for_read(
        state.task_board_store.as_ref(),
        &conversation_id,
        task_id.as_deref(),
        &parent_key,
    );
    Ok(state.task_board_store.document(&store_key).to_value())
}

/// Returns the last captured screenshot from [`capture_screen`] for a conversation.
#[tauri::command]
pub fn preview_computer_annotated_screen(
    conversation_id: String,
    state: State<'_, Arc<AppState>>,
) -> Result<ComputerAnnotatedPreview, String> {
    state
        .computer_state
        .cached_capture_for_conversation(&conversation_id)
        .map(|(img, _monitor)| ComputerAnnotatedPreview {
            image_base64: pointer_core::agents::computer::screen::encode_image_to_base64(&img),
            image_mime: pointer_core::agents::computer::screen::image_data_url_mime(&img)
                .to_string(),
            caption: "Screenshot".into(),
        })
        .ok_or_else(|| {
            "暂无桌面截图：请先完成一次截图处理（发送 Computer 消息），或确认会话 ID 正确。".into()
        })
}

/// Load a saved annotated PNG by path relative to `computer-captures/` (from `AssistantRoundScreen`).
#[tauri::command]
pub fn preview_computer_round_screen(rel_path: String) -> Result<ComputerAnnotatedPreview, String> {
    capture_debug::read_computer_capture_preview(&rel_path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn preview_chat_media(
    storage_rel_path: String,
    conversation_id: Option<String>,
) -> Result<ChatMediaPreview, String> {
    let env = conversation_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .and_then(pointer_core::env::session_env::env_for_conversation);
    pointer_core::media::read_chat_media_preview_in_env(&storage_rel_path, env)
        .map_err(|e| e.to_string())
}

/// Absolute path for a saved conversation-media file (for desktop video preview via convertFileSrc).
#[tauri::command]
pub fn get_chat_media_local_path(
    storage_rel_path: String,
    conversation_id: Option<String>,
) -> Result<String, String> {
    if let Some(id) = conversation_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
    {
        if let Some(env) = pointer_core::env::session_env::env_for_conversation(id) {
            if !env.is_denied()
                && env.kind() != pointer_core::env::EnvKind::Local
                && !env.path_map().is_host_passthrough()
            {
                log::info!(
                    "chat media local path refused: sandbox environment conversation={id} rel={storage_rel_path}"
                );
                return Err("沙箱里的媒体没有本机文件地址".into());
            }
        }
    }
    let path = pointer_core::media::media_abs_path(&storage_rel_path).map_err(|e| e.to_string())?;
    if !path.is_file() {
        return Err(format!("媒体文件不存在: {}", path.display()));
    }
    Ok(path.to_string_lossy().to_string())
}

/// Write raw bytes (base64) to an absolute path chosen by the user (e.g. chart PNG export).
#[tauri::command]
pub fn save_bytes_to_path(path: String, content_base64: String) -> Result<(), String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err("保存路径为空".into());
    }
    let path_buf =
        pointer_core::media::access::normalize_user_path(trimmed).map_err(|e| e.to_string())?;
    if path_buf
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .is_empty()
    {
        return Err("保存路径无效".into());
    }
    if let Some(parent) = path_buf.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            std::fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {e}"))?;
        }
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(content_base64.trim())
        .map_err(|e| format!("decode base64: {e}"))?;
    if bytes.is_empty() {
        return Err("文件内容为空".into());
    }
    std::fs::write(&path_buf, &bytes).map_err(|e| format!("写入失败: {e}"))?;
    log::info!(
        "save_bytes_to_path: wrote {} bytes to {}",
        bytes.len(),
        path_buf.display()
    );
    Ok(())
}

/// Reveal a local file in Finder (macOS) or file manager (other platforms).
#[tauri::command]
pub fn reveal_in_finder(path: String) -> Result<(), String> {
    let path_buf = resolve_reveal_path(&path).map_err(|e| e.to_string())?;
    let display = path_buf.display().to_string();
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .args(["-R", &display])
            .spawn()
            .map_err(|e| format!("打开 Finder 失败: {e}"))?;
        return Ok(());
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .args(["/select,", &display])
            .spawn()
            .map_err(|e| format!("打开文件管理器失败: {e}"))?;
        return Ok(());
    }
    #[cfg(target_os = "linux")]
    {
        // Try common file managers
        for (cmd, args) in &[
            (
                "xdg-open",
                vec![path_buf
                    .parent()
                    .unwrap_or(std::path::Path::new("/"))
                    .to_string_lossy()
                    .to_string()],
            ),
            ("nautilus", vec![display.clone()]),
            ("dolphin", vec![format!("--select={display}")]),
            ("nemo", vec![display.clone()]),
        ] {
            if std::process::Command::new(cmd).args(args).spawn().is_ok() {
                return Ok(());
            }
        }
        Err("未找到可用的文件管理器".into())
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        Err("当前平台不支持")
    }
}

fn resolve_reveal_path(raw: &str) -> Result<std::path::PathBuf, String> {
    pointer_core::media::resolve_local_media_path(raw).map_err(|e| e.to_string())
}

fn max_attachment_bytes(file_name: &str) -> u64 {
    pointer_core::media::composer_attachment_max_bytes(file_name)
}

#[derive(serde::Serialize)]
pub struct LocalFileAttachmentPayload {
    pub file_name: String,
    pub mime_type: String,
    pub size_bytes: u64,
    pub content_base64: String,
}

fn open_path_with_system_default(path: &Path) -> Result<(), String> {
    if !path.is_file() {
        return Err(format!("文件不存在: {}", path.display()));
    }
    let path_str = path.to_string_lossy().to_string();
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&path_str)
            .spawn()
            .map_err(|e| format!("打开文件失败: {e}"))?;
        return Ok(());
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", &path_str])
            .spawn()
            .map_err(|e| format!("打开文件失败: {e}"))?;
        return Ok(());
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&path_str)
            .spawn()
            .map_err(|e| format!("打开文件失败: {e}"))?;
        return Ok(());
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        Err("当前平台不支持".into())
    }
}

fn mime_from_file_name(file_name: &str) -> String {
    let ext = Path::new(file_name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "pdf" => "application/pdf",
        "txt" => "text/plain",
        "md" => "text/markdown",
        "json" => "application/json",
        "csv" => "text/csv",
        "zip" => "application/zip",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        _ => "application/octet-stream",
    }
    .to_string()
}

/// Open a local file with the OS default application.
#[tauri::command]
pub fn open_path_with_default_app(path: String) -> Result<(), String> {
    let path_buf =
        pointer_core::media::access::normalize_user_path(&path).map_err(|e| e.to_string())?;
    let canonical = path_buf.canonicalize().unwrap_or(path_buf);
    open_path_with_system_default(&canonical)
}

/// Open a saved chat attachment (`conversation-media/...`) with the OS default application.
#[tauri::command]
pub fn open_chat_media(storage_rel_path: String) -> Result<(), String> {
    let path = pointer_core::media::media_abs_path(&storage_rel_path).map_err(|e| e.to_string())?;
    open_path_with_system_default(&path)
}

/// Read a user-selected local file for composer attachment upload (any directory).
#[tauri::command]
pub fn get_local_file_size(path: String) -> Result<u64, String> {
    let path_buf =
        pointer_core::media::access::normalize_user_path(&path).map_err(|e| e.to_string())?;
    if !path_buf.is_file() {
        return Err(format!("文件不存在: {}", path_buf.display()));
    }
    let meta = fs::metadata(&path_buf).map_err(|e| format!("读取文件信息失败: {e}"))?;
    Ok(meta.len())
}

/// Read a user-selected local file for composer attachment upload (any directory).
#[tauri::command]
pub fn read_local_file_for_attachment(path: String) -> Result<LocalFileAttachmentPayload, String> {
    let path_buf =
        pointer_core::media::access::normalize_user_path(&path).map_err(|e| e.to_string())?;
    if !path_buf.is_file() {
        return Err(format!("文件不存在: {}", path_buf.display()));
    }
    let meta = fs::metadata(&path_buf).map_err(|e| format!("读取文件信息失败: {e}"))?;
    let file_name = path_buf
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("attachment")
        .to_string();
    if pointer_core::media::is_video_file_name(&file_name) {
        return Err("视频请通过 OSS 上传：使用文件选择后自动上传，勿直接读取整文件到内存".into());
    }
    let limit = max_attachment_bytes(&file_name);
    if meta.len() > limit {
        let limit_mb = limit / (1024 * 1024);
        return Err(format!("文件超过 {limit_mb} MB 上限"));
    }
    let bytes = fs::read(&path_buf).map_err(|e| format!("读取文件失败: {e}"))?;
    let mime_type = mime_from_file_name(&file_name);
    let content_base64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Ok(LocalFileAttachmentPayload {
        file_name,
        mime_type,
        size_bytes: meta.len(),
        content_base64,
    })
}

#[tauri::command]
pub fn preview_media_ref(
    media_ref: String,
    conversation_id: Option<String>,
) -> Result<ChatMediaPreview, String> {
    // 沙箱档位下媒体引用是环境内路径（`/data/...`），宿主上没有这个文件。
    // 这里只**取会话已建好的环境**（不新建沙箱）：会话在跑时一定在缓存里，
    // 取不到就按宿主读——与 Web 端 `/api/chat/media-ref-preview` 同一口径。
    let env = conversation_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .and_then(pointer_core::env::session_env::env_for_conversation);
    pointer_core::media::read_media_ref_preview_in_env(&media_ref, env.as_deref())
        .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub async fn save_chat_attachment(
    state: State<'_, Arc<AppState>>,
    conversation_id: String,
    attachment_id: String,
    content_base64: String,
    file_name: String,
) -> Result<String, String> {
    // Access may have expired while the UI still shows the last session snapshot.
    let uid = crate::platform_auth_gate::require_platform_user_id(state.inner()).await?;
    state
        .session_index
        .ensure_session_user_id(&conversation_id, &uid)
        .map_err(|e| e.to_string())?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(content_base64.trim())
        .map_err(|e| format!("decode attachment base64: {e}"))?;
    pointer_core::media::ensure_composer_attachment_size(bytes.len() as u64, &file_name)
        .map_err(|e| e.to_string())?;
    pointer_core::media::save_attachment_bytes(&conversation_id, &attachment_id, &bytes, &file_name)
        .map_err(|e| e.to_string())
}

/// Desktop Composer: copy a local path into the session sandbox (streamed, no base64 IPC).
#[tauri::command]
pub async fn save_chat_attachment_from_path(
    state: State<'_, Arc<AppState>>,
    conversation_id: String,
    path: String,
    file_name: Option<String>,
) -> Result<String, String> {
    let uid = crate::platform_auth_gate::require_platform_user_id(state.inner()).await?;
    state
        .session_index
        .ensure_session_user_id(&conversation_id, &uid)
        .map_err(|e| e.to_string())?;
    let path_buf =
        pointer_core::media::access::normalize_user_path(&path).map_err(|e| e.to_string())?;
    let name = file_name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            path_buf
                .file_name()
                .and_then(|n| n.to_str())
                .filter(|s| !s.is_empty())
                .unwrap_or("attachment")
                .to_string()
        });
    let conv = conversation_id.clone();
    let src = path_buf.clone();
    tauri::async_runtime::spawn_blocking(move || {
        pointer_core::media::save_attachment_from_path(&conv, &src, &name)
    })
    .await
    .map_err(|e| format!("save attachment from path join: {e}"))?
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_computer_monitors() -> Result<Vec<ComputerMonitor>, String> {
    pointer_core::agents::computer::screen::list_monitors().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_computer_conversation_monitor(
    state: State<'_, Arc<AppState>>,
    conversation_id: String,
    monitor_id: Option<String>,
) -> Result<(), String> {
    state
        .computer_state
        .set_conversation_monitor(&conversation_id, monitor_id);
    Ok(())
}

#[tauri::command]
pub fn confirm_computer_monitor_pick(
    state: State<'_, Arc<AppState>>,
    conversation_id: String,
) -> Result<(), String> {
    if state.confirm_computer_monitor_pick(&conversation_id) {
        Ok(())
    } else {
        Err("no pending monitor pick".into())
    }
}

#[tauri::command]
pub fn cancel_computer_monitor_pick(
    state: State<'_, Arc<AppState>>,
    conversation_id: String,
) -> Result<(), String> {
    if state.cancel_computer_monitor_pick(&conversation_id) {
        Ok(())
    } else {
        Err("no pending monitor pick".into())
    }
}

#[tauri::command]
pub fn load_conversations() -> Result<Vec<Conversation>, String> {
    let conversations = storage::load_conversations().map_err(|e| e.to_string())?;
    let messages: usize = conversations.iter().map(|c| c.messages.len()).sum();
    log::info!(
        "tauri::load_conversations: conversations={} messages={messages}",
        conversations.len()
    );
    Ok(conversations)
}

/// Cursor-paginated, meta-only conversation list (no messages).
/// Sort order: `(updated_at_ms DESC, id DESC)`.
/// Pass `cursor_updated_at = None` and `cursor_id = None` for the first page;
/// pass the last row of the previous page to fetch the next.
#[tauri::command]
pub fn load_conversation_metas(
    state: State<'_, Arc<AppState>>,
    cursor_updated_at: Option<i64>,
    cursor_id: Option<String>,
    limit: Option<i64>,
) -> Result<Vec<pointer_core::models::ConversationMeta>, String> {
    let cursor = match (cursor_updated_at, cursor_id) {
        (Some(ts), Some(id)) => Some((ts, id)),
        (Some(_), None) | (None, Some(_)) => {
            return Err("cursor_updated_at and cursor_id must both be set or both be null".into())
        }
        (None, None) => None,
    };
    let limit = limit.unwrap_or(50);
    let scope = platform_list_scope(&state);
    storage::load_conversation_metas(&scope, cursor, limit).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn load_projects(
    state: State<'_, Arc<AppState>>,
    cursor_last_activity_at: Option<i64>,
    cursor_id: Option<String>,
    limit: Option<i64>,
) -> Result<ProjectPage, String> {
    let scope = platform_list_scope(&state);
    let cursor = match (cursor_last_activity_at, cursor_id) {
        (Some(last_activity_at), Some(id)) => Some(ProjectCursor {
            last_activity_at,
            id,
        }),
        (None, None) => None,
        _ => return Err("project cursor fields must both be set or omitted".into()),
    };
    storage::load_projects(&scope, cursor, limit.unwrap_or(20)).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn load_sidebar_projects(state: State<'_, Arc<AppState>>) -> Result<Vec<Project>, String> {
    let scope = platform_list_scope(&state);
    storage::load_sidebar_projects(&scope).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn load_project(
    state: State<'_, Arc<AppState>>,
    project_id: String,
) -> Result<Option<Project>, String> {
    let scope = platform_list_scope(&state);
    storage::load_project(&project_id, &scope).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn load_project_conversation_metas(
    state: State<'_, Arc<AppState>>,
    project_id: String,
    cursor_updated_at: Option<i64>,
    cursor_id: Option<String>,
    limit: Option<i64>,
) -> Result<Vec<pointer_core::models::ConversationMeta>, String> {
    let scope = platform_list_scope(&state);
    let cursor = match (cursor_updated_at, cursor_id) {
        (Some(updated_at), Some(id)) => Some((updated_at, id)),
        (None, None) => None,
        _ => return Err("conversation cursor fields must both be set or omitted".into()),
    };
    storage::load_project_conversation_metas(&project_id, &scope, cursor, limit.unwrap_or(20))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn create_project(
    state: State<'_, Arc<AppState>>,
    name: String,
    workspace_root: String,
) -> Result<ProjectCreationResult, String> {
    let uid = state
        .active_platform_auth()
        .platform_user_id()
        .unwrap_or_default();
    let selected = accept_workspace_path(uid.clone(), workspace_root).await?;
    storage::create_project(&name, &selected, &uid).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn create_directory(parent_path: String, name: String) -> Result<String, String> {
    let parent = Path::new(&parent_path);
    let trimmed_name = name.trim();
    if trimmed_name.is_empty()
        || trimmed_name == "."
        || trimmed_name == ".."
        || trimmed_name.contains('/')
        || trimmed_name.contains('\\')
    {
        return Err("目录名称无效".into());
    }
    if !parent.is_dir() {
        return Err("父目录不存在或不可访问".into());
    }
    let target = parent.join(trimmed_name);
    fs::create_dir(&target).map_err(|e| e.to_string())?;
    Ok(target.to_string_lossy().into_owned())
}

#[tauri::command]
pub async fn update_project(
    state: State<'_, Arc<AppState>>,
    id: String,
    name: Option<String>,
    workspace_root: Option<String>,
    is_pinned: Option<bool>,
    is_archived: Option<bool>,
) -> Result<Project, String> {
    let uid = state
        .active_platform_auth()
        .platform_user_id()
        .unwrap_or_default();
    let workspace_root = match workspace_root {
        Some(path) => Some(accept_workspace_path(uid.clone(), path).await?),
        None => None,
    };
    storage::update_project(
        &id,
        &uid,
        name.as_deref(),
        workspace_root.as_deref(),
        is_pinned,
        is_archived,
    )
    .map_err(|e| e.to_string())
}

async fn accept_workspace_path(uid: String, path: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let cid = format!("browse-{uid}");
        let env = pointer_core::workspace_service::browse_env_for_user(&cid, &uid)
            .map_err(|e| format!("{e:#}"))?;
        pointer_core::workspace_service::select_workspace(
            &uid,
            &pointer_core::workspace_resolve::WorkspaceSelection { path },
            env,
        )
        .map(|selected| selected.path)
        .map_err(|e| format!("{e:#}"))
    })
    .await
    .map_err(|e| format!("spawn_blocking failed: {e}"))?
}

#[tauri::command]
pub fn delete_project(state: State<'_, Arc<AppState>>, id: String) -> Result<(), String> {
    let uid = state
        .active_platform_auth()
        .platform_user_id()
        .unwrap_or_default();
    storage::delete_project(&id, &uid).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn search_conversations(
    state: State<'_, Arc<AppState>>,
    query: String,
    limit: Option<i64>,
) -> Result<Vec<ConversationSearchHit>, String> {
    let limit = limit.unwrap_or(50);
    let scope = platform_list_scope(&state);
    storage::search_conversations(&scope, &query, limit).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_conversation_search_matches(
    state: State<'_, Arc<AppState>>,
    conversation_id: String,
    query: String,
) -> Result<Vec<pointer_core::models::ConversationSearchMatch>, String> {
    let scope = platform_list_scope(&state);
    storage::list_conversation_search_matches(&scope, &conversation_id, &query)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_conversation_outline(
    state: State<'_, Arc<AppState>>,
    conversation_id: String,
) -> Result<Vec<pointer_core::models::ConversationOutlineItem>, String> {
    let outline_t = std::time::Instant::now();
    let scope = platform_list_scope(&state);
    let items =
        storage::list_conversation_outline(&scope, &conversation_id).map_err(|e| e.to_string())?;
    log::info!(
        "tauri::list_conversation_outline: id={conversation_id} returned {} rows",
        items.len()
    );
    pointer_core::logging::log_phase_elapsed_extra(
        "tauri_list_conversation_outline",
        &conversation_id,
        outline_t.elapsed().as_millis(),
        &format!("rows={}", items.len()),
    );
    Ok(items)
}

#[tauri::command]
pub fn set_message_milestone(
    state: State<'_, Arc<AppState>>,
    conversation_id: String,
    message_id: String,
    milestone: bool,
) -> Result<(), String> {
    let scope = platform_list_scope(&state);
    storage::set_message_milestone(&scope, &conversation_id, &message_id, milestone)
        .map_err(|e| e.to_string())?;
    log::info!(
        "tauri::set_message_milestone: id={conversation_id} message_id={message_id} milestone={milestone}"
    );
    Ok(())
}

/// One-shot migration of the legacy frontend `pointer.chat.turn-elapsed.v1`
/// blob into `messages.turn_elapsed_ms`.
///
/// The frontend parses the blob and passes `(conversationId, messageId,
/// elapsedMs)` pairs. The store dedupes/validates them, writes in one
/// transaction with "first write wins", and flips the `store_meta` flag so a
/// replay is a no-op. Logging stays count-only: no ids.
#[tauri::command]
pub fn backfill_turn_elapsed(
    entries: Vec<pointer_core::conversation_store::TurnElapsedBackfillEntry>,
) -> Result<pointer_core::conversation_store::TurnElapsedBackfillReport, String> {
    let report = storage::backfill_turn_elapsed_ms(entries).map_err(|e| e.to_string())?;
    log::info!(
        "tauri::backfill_turn_elapsed: received={} updated={} skipped={} already_done={} duration_ms={}",
        report.received,
        report.updated,
        report.skipped,
        report.already_done,
        report.duration_ms
    );
    Ok(report)
}

#[tauri::command]
pub fn load_conversation_message(
    conversation_id: String,
    message_id: String,
) -> Result<Option<ChatMessage>, String> {
    let message = storage::load_conversation_message(&conversation_id, &message_id)
        .map_err(|e| e.to_string())?;
    if message.is_none() {
        log::warn!(
            "tauri::load_conversation_message: missing conversation_id={conversation_id} message_id={message_id}"
        );
    } else {
        log::info!(
            "tauri::load_conversation_message: conversation_id={conversation_id} message_id={message_id}"
        );
    }
    Ok(message)
}

#[tauri::command]
pub fn load_conversation_messages(conversation_id: String) -> Result<Vec<ChatMessage>, String> {
    let messages =
        storage::load_conversation_messages(&conversation_id).map_err(|e| e.to_string())?;
    log::info!(
        "tauri::load_conversation_messages: id={conversation_id} returned {} messages",
        messages.len()
    );
    Ok(messages)
}

#[derive(Debug, Clone, serde::Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LoadConversationMessagesPageArgs {
    pub limit_turns: Option<u32>,
    pub before_position: Option<i64>,
    pub after_position: Option<i64>,
    pub around_message_id: Option<String>,
    #[serde(default, rename = "includeScopedSubMessages")]
    pub include_scoped_sub_messages: Option<bool>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadScopedSubMessagesArgs {
    #[serde(default)]
    pub anchor_message_id: String,
    #[serde(default)]
    pub trace_id: String,
    pub agent_instance_id: Option<String>,
}

#[tauri::command]
pub fn load_scoped_sub_messages_for_trace(
    conversation_id: String,
    args: LoadScopedSubMessagesArgs,
) -> Result<Vec<pointer_core::models::ChatMessage>, String> {
    let instance = args
        .agent_instance_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    storage::load_scoped_sub_messages_for_trace(
        &conversation_id,
        args.anchor_message_id.trim(),
        args.trace_id.trim(),
        instance,
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn load_conversation_messages_page(
    conversation_id: String,
    opts: Option<LoadConversationMessagesPageArgs>,
) -> Result<pointer_core::conversation_store::MessagePage, String> {
    let opts = opts.unwrap_or_default();
    let page_opts = pointer_core::conversation_store::LoadMessagesPageOpts {
        limit_turns: opts.limit_turns,
        before_position: opts.before_position,
        after_position: opts.after_position,
        around_message_id: opts.around_message_id,
        include_scoped_sub_messages: opts.include_scoped_sub_messages.unwrap_or(false),
    };
    let page = storage::load_conversation_messages_page(&conversation_id, &page_opts)
        .map_err(|e| e.to_string())?;
    log::info!(
        "tauri::load_conversation_messages_page: id={conversation_id} returned {} of {}",
        page.messages.len(),
        page.message_count
    );
    Ok(page)
}

#[tauri::command]
pub fn load_conversation_meta(
    state: State<'_, Arc<AppState>>,
    conversation_id: String,
) -> Result<Option<pointer_core::models::ConversationMeta>, String> {
    let scope = platform_list_scope(&state);
    storage::load_conversation_meta(&scope, &conversation_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_conversation(conversation_id: String) -> Result<(), String> {
    storage::delete_conversation(&conversation_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_conversation_meta(
    state: State<'_, Arc<AppState>>,
    metas: Vec<pointer_core::models::ConversationMeta>,
) -> Result<(), String> {
    let platform_user_id = if state.active_platform_auth().session_view().logged_in {
        state.active_platform_auth().platform_user_id()
    } else {
        None
    };
    storage::save_conversation_meta_with_platform_user(&metas, platform_user_id.as_deref())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn append_conversation_messages(
    conversation_id: String,
    messages: Vec<pointer_core::models::ChatMessage>,
) -> Result<Vec<pointer_core::conversation_store::AppendedMessageRow>, String> {
    storage::append_conversation_messages(&conversation_id, &messages).map_err(|e| e.to_string())
}

// ---- Phase 5/6: cron jobs + webhook token config (desktop IPC) ----
// These mirror the server HTTP endpoints so the frontend Automation panel
// works identically on desktop (Tauri IPC) and web (HTTP).

#[tauri::command]
pub fn get_dispatcher_queue_snapshot(
    dispatcher: State<'_, Arc<RunDispatcher>>,
) -> RunQueueSnapshot {
    dispatcher.queue_snapshot()
}

#[tauri::command]
pub fn list_cron_jobs(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<pointer_core::conversation_store::cron_jobs::CronJobView>, String> {
    let rows = state
        .session_index
        .cron_jobs_list_all()
        .map_err(|e| e.to_string())?;
    Ok(rows
        .iter()
        .map(pointer_core::conversation_store::cron_jobs::CronJobView::from_record)
        .collect())
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateCronJobArgs {
    pub id: String,
    pub label: String,
    /// Recurring 6-field cron, or omit when `schedule` is set.
    #[serde(default)]
    pub cron_expr: String,
    /// Friendly / one-shot schedule (`30m`, `daily@9:30`, …). Preferred when set.
    #[serde(default)]
    pub schedule: Option<String>,
    /// Ignored: each cron job owns a dedicated `cron:{id}` session. Retained on
    /// the wire for backward compatibility with older frontends.
    #[serde(default)]
    pub conversation_id: String,
    pub prompt_text: String,
    #[serde(default)]
    pub lead_agent_id: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Optional Run → IM delivery spec (e.g. "feishu", "feishu:ou_xxx",
    /// comma-separated, "all"). Empty / None = no IM push after the run.
    #[serde(default)]
    pub deliver: Option<String>,
}

fn default_true() -> bool {
    true
}

#[tauri::command]
pub fn create_cron_job(
    state: State<'_, Arc<AppState>>,
    channel_gateway: State<'_, Arc<pointer_channels::ChannelGateway>>,
    args: CreateCronJobArgs,
) -> Result<pointer_core::conversation_store::cron_jobs::CronJobView, String> {
    let schedule_input = args
        .schedule
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(args.cron_expr.trim());
    if schedule_input.is_empty() {
        return Err("schedule or cronExpr is required".into());
    }
    let parsed = pointer_core::tools::cron_job::schedule::parse_schedule(schedule_input)
        .map_err(|e| e.to_string())?;
    let (cron_expr, schedule_kind, next_override) = match parsed {
        pointer_core::tools::cron_job::schedule::ParsedSchedule::Recurring { cron_expr } => {
            if pointer_core::conversation_store::cron_jobs::next_run_ms_now(&cron_expr).is_none() {
                return Err(format!("invalid cron expression: {cron_expr}"));
            }
            (
                cron_expr,
                pointer_core::conversation_store::cron_jobs::SCHEDULE_KIND_CRON,
                None,
            )
        }
        pointer_core::tools::cron_job::schedule::ParsedSchedule::Once { fire_at_ms } => (
            pointer_core::tools::cron_job::schedule::ONCE_CRON_PLACEHOLDER.to_string(),
            pointer_core::conversation_store::cron_jobs::SCHEDULE_KIND_ONCE,
            Some(fire_at_ms),
        ),
    };
    let deliver = args
        .deliver
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .and_then(|s| pointer_channels::im_delivery::normalize_deliver_spec(s));
    pointer_channels::im_delivery::validate_deliver_spec(
        deliver.as_deref(),
        &channel_gateway.config(),
    )?;
    let owner_user_id = state
        .active_platform_auth()
        .platform_user_id()
        .map(|uid| uid.trim().to_string())
        .filter(|uid| !uid.is_empty());
    let new = pointer_core::conversation_store::cron_jobs::NewCronJob {
        id: &args.id,
        label: &args.label,
        cron_expr: &cron_expr,
        schedule_kind,
        schedule_raw: Some(schedule_input),
        next_run_at_ms: next_override,
        conversation_id: &args.conversation_id,
        prompt_text: &args.prompt_text,
        lead_agent_id: args.lead_agent_id.as_deref(),
        enabled: args.enabled,
        deliver: deliver.as_deref(),
        // Owner = the desktop operator's platform identity. The scheduler
        // copies it onto the job's cron session row so the run resolves the
        // creator's environment tier / settings / memory.
        owner_user_id: owner_user_id.as_deref(),
    };
    let inserted = state
        .session_index
        .cron_jobs_insert(&new)
        .map_err(|e| e.to_string())?;
    if !inserted {
        return Err(format!("cron job already exists: {}", args.id));
    }
    let rec = state
        .session_index
        .cron_jobs_get(&args.id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("cron job vanished after insert: {}", args.id))?;
    log::info!(
        "cron-jobs: created id={} label={} kind={} expr={}",
        args.id,
        args.label,
        schedule_kind,
        cron_expr
    );
    Ok(pointer_core::conversation_store::cron_jobs::CronJobView::from_record(&rec))
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCronJobArgs {
    pub enabled: Option<bool>,
    /// Optional Run → IM deliver spec. Pass empty string to clear.
    pub deliver: Option<String>,
}

#[tauri::command]
pub fn update_cron_job(
    state: State<'_, Arc<AppState>>,
    channel_gateway: State<'_, Arc<pointer_channels::ChannelGateway>>,
    job_id: String,
    args: UpdateCronJobArgs,
) -> Result<pointer_core::conversation_store::cron_jobs::CronJobView, String> {
    if let Some(enabled) = args.enabled {
        let ok = state
            .session_index
            .cron_jobs_set_enabled(&job_id, enabled)
            .map_err(|e| e.to_string())?;
        if !ok {
            return Err(format!("cron job not found: {job_id}"));
        }
    }
    if let Some(ref deliver) = args.deliver {
        let deliver = if deliver.trim().is_empty() {
            None
        } else {
            pointer_channels::im_delivery::normalize_deliver_spec(deliver)
        };
        pointer_channels::im_delivery::validate_deliver_spec(
            deliver.as_deref(),
            &channel_gateway.config(),
        )?;
        let ok = state
            .session_index
            .cron_jobs_update_deliver(&job_id, deliver.as_deref())
            .map_err(|e| e.to_string())?;
        if !ok {
            return Err(format!("cron job not found: {job_id}"));
        }
    }
    state
        .session_index
        .cron_jobs_get(&job_id)
        .map_err(|e| e.to_string())?
        .map(|r| pointer_core::conversation_store::cron_jobs::CronJobView::from_record(&r))
        .ok_or_else(|| format!("cron job not found: {job_id}"))
}

#[tauri::command]
pub fn list_cron_delivery_targets(
    channel_gateway: State<'_, Arc<pointer_channels::ChannelGateway>>,
) -> Result<Vec<pointer_channels::im_delivery::DeliveryTargetInfo>, String> {
    let cfg = channel_gateway.config().clone();
    Ok(pointer_channels::im_delivery::list_home_delivery_targets(
        &cfg,
    ))
}

#[tauri::command]
pub fn delete_cron_job(state: State<'_, Arc<AppState>>, job_id: String) -> Result<bool, String> {
    state
        .session_index
        .cron_jobs_delete(&job_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_webhook_config(
    state: State<'_, Arc<AppState>>,
) -> Result<pointer_core::webhook_config::WebhookConfigView, String> {
    let store = pointer_core::webhook_config::WebhookTokenStore::new(&state.session_index);
    let sources: Vec<pointer_core::webhook_config::WebhookSourceView> = store
        .list_sources()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|(src, token)| {
            store
                .source_view(src, token, String::new())
                .map_err(|e| e.to_string())
        })
        .collect::<Result<Vec<_>, String>>()?;
    let legacy_configured = store.is_legacy_configured().map_err(|e| e.to_string())?;
    let legacy_preview = if legacy_configured {
        store.legacy_preview().map_err(|e| e.to_string())?
    } else {
        None
    };
    Ok(pointer_core::webhook_config::WebhookConfigView {
        sources,
        url_template: String::new(),
        legacy_configured,
        legacy_preview,
    })
}

#[tauri::command]
pub fn set_webhook_source_token(
    state: State<'_, Arc<AppState>>,
    src: String,
    token: String,
    auth_header_name: Option<String>,
    session_mode: Option<String>,
) -> Result<pointer_core::webhook_config::WebhookConfigView, String> {
    let store = pointer_core::webhook_config::WebhookTokenStore::new(&state.session_index);
    if store
        .is_source_configured(&src)
        .map_err(|e| e.to_string())?
    {
        return Err(
            "webhook token already configured for this source; clear it first to rotate".into(),
        );
    }
    let parsed_mode = session_mode
        .as_deref()
        .map(pointer_core::conversation_store::webhook_sources::WebhookSessionMode::parse)
        .transpose()
        .map_err(|e| e.to_string())?;
    let inserted = store
        .set_source_token(&src, &token, auth_header_name.as_deref(), parsed_mode)
        .map_err(|e| e.to_string())?;
    if !inserted {
        return Err("webhook token already configured for this source".into());
    }
    get_webhook_config(state)
}

#[tauri::command]
pub fn patch_webhook_source(
    state: State<'_, Arc<AppState>>,
    src: String,
    session_mode: Option<String>,
) -> Result<pointer_core::webhook_config::WebhookConfigView, String> {
    let store = pointer_core::webhook_config::WebhookTokenStore::new(&state.session_index);
    if let Some(mode_raw) = session_mode {
        let mode =
            pointer_core::conversation_store::webhook_sources::WebhookSessionMode::parse(&mode_raw)
                .map_err(|e| e.to_string())?;
        store
            .set_session_mode(&src, mode)
            .map_err(|e| e.to_string())?;
    }
    get_webhook_config(state)
}

#[tauri::command]
pub fn reveal_webhook_source_token(
    state: State<'_, Arc<AppState>>,
    src: String,
) -> Result<pointer_core::webhook_config::WebhookTokenRevealView, String> {
    let store = pointer_core::webhook_config::WebhookTokenStore::new(&state.session_index);
    let normalized = pointer_core::webhook_config::WebhookTokenStore::normalize_src(&src)
        .map_err(|e| e.to_string())?;
    let Some(token) = store
        .reveal_source_token(&normalized)
        .map_err(|e| e.to_string())?
    else {
        return Err("webhook token not configured for this source".into());
    };
    log::info!("webhook_config: token revealed src={normalized}");
    Ok(pointer_core::webhook_config::WebhookTokenRevealView {
        src: normalized,
        token: token.clone(),
        preview: pointer_core::webhook_config::mask_token(&token),
    })
}

#[tauri::command]
pub fn clear_webhook_source_token(
    state: State<'_, Arc<AppState>>,
    src: String,
) -> Result<bool, String> {
    let store = pointer_core::webhook_config::WebhookTokenStore::new(&state.session_index);
    store.clear_source_token(&src).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn clear_webhook_legacy_token(state: State<'_, Arc<AppState>>) -> Result<bool, String> {
    let store = pointer_core::webhook_config::WebhookTokenStore::new(&state.session_index);
    store.clear_legacy_token().map_err(|e| e.to_string())
}

#[cfg(test)]
mod desktop_tests {
    use super::*;
    use pointer_core::env::provider::{Backend, ProviderConfig, Tier};

    fn docker_available() -> bool {
        std::process::Command::new("docker")
            .arg("info")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    /// 容器在跑时，`get_sandbox_desktop` 必须返回带 ws 地址的端点。
    ///
    /// 回归的是「命令层拿错环境」这个 bug：原来走 `panel_binding` 的前身，
    /// docker 后端下它故意返回宿主 LocalEnv（面板读文件要同一份 bind mount），
    /// 桌面命令因此永远拿到 `None`，面板按钮根本不出现。
    #[tokio::test]
    #[ignore = "需要本机 docker"]
    async fn get_sandbox_desktop_returns_ws_when_container_running() {
        if !docker_available() {
            eprintln!("skip: docker 不可用");
            return;
        }
        let dir = std::env::temp_dir().join(format!("pointer-cmd-desktop-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let cfg = ProviderConfig {
            tier: Some(Tier::Desktop),
            admin_tier: Default::default(),
            backend: Some(Backend::Docker),
            sandbox_url: None,
            volume_host_path: None,
            template: Some("pointer-sandbox-desktop:latest".to_string()),
            admins: Vec::new(),
            accounts: Default::default(),
            tier_backends: Default::default(),
            platform: true,
        };
        pointer_core::env::session_env::resolve_session_env(
            &cfg,
            "it-cmd-desktop",
            "tester",
            Some(&dir),
        )
        .expect("resolve")
        .expect("docker 后端应返回环境");

        let got = get_sandbox_desktop(Some("it-cmd-desktop".to_string()), String::new())
            .await
            .expect("命令不应报错")
            .expect("容器在跑时必须返回桌面端点");

        assert!(
            got.url.starts_with("vnc://127.0.0.1:"),
            "vnc 地址形态不对：{}",
            got.url
        );
        let ws = got.ws_url.expect("应带 ws 地址");
        assert!(
            ws.starts_with("ws://127.0.0.1:") && ws.ends_with("/websockify"),
            "ws 地址形态不对：{ws}"
        );
        assert!(got.password.is_some(), "容器档位应带 VNC 密码");

        let _ = std::process::Command::new("docker")
            .args(["rm", "-f", "pointer-sbx-it-cmd-desktop"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 序列化必须是 camelCase：前端读 `wsUrl`，用 snake_case 会拿到 undefined，
    /// 表现成「镜像不支持接管」——这个坑真踩过（2026-11-18 手工验收）。
    #[test]
    fn sandbox_desktop_serializes_camel_case() {
        let desktop = SandboxDesktop {
            url: "vnc://127.0.0.1:1".to_string(),
            ws_url: Some("ws://127.0.0.1:2/websockify".to_string()),
            password: Some("abcd2345".to_string()),
        };
        let json = serde_json::to_string(&desktop).expect("serialize");
        assert!(json.contains("\"wsUrl\""), "{json}");
        assert!(!json.contains("ws_url"), "{json}");
    }

    /// 没有容器时必须返回 `None`，不能顺手把沙箱建起来。
    #[tokio::test]
    async fn get_sandbox_desktop_is_none_without_container() {
        let got = get_sandbox_desktop(Some("no-such-conversation".to_string()), String::new())
            .await
            .expect("命令不应报错");
        assert!(got.is_none(), "没有容器时不能返回端点");
    }
}
