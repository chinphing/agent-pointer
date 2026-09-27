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
use tauri::{AppHandle, Emitter, State};

pub const STREAM_EVENT: &str = "chat://stream";

/// Resolve a user-visible message by i18n key using the current UI locale.
fn ui_text(key: &str) -> String {
    pointer_core::i18n::t(key, pointer_core::i18n::current_ui_locale()).to_string()
}

/// Like [`ui_text`] but substitutes `{name}` placeholders.
fn ui_textf(key: &str, args: &[(&str, &str)]) -> String {
    pointer_core::i18n::tf(key, pointer_core::i18n::current_ui_locale(), args)
}

fn platform_list_scope(state: &AppState) -> pointer_core::conversation_store::ListScope {
    let auth = state.active_platform_auth();
    pointer_core::conversation_store::ListScope::from_viewer(
        auth.is_platform_admin(),
        &auth.platform_user_id().unwrap_or_default(),
    )
}

#[tauri::command]
pub async fn list_workspace_directory(
    workspace_root: String,
    relative_path: Option<String>,
) -> Result<Vec<pointer_core::workspace_read::WorkspaceEntry>, String> {
    let result = tauri::async_runtime::spawn_blocking(move || {
        pointer_core::workspace_read::list_directory(
            Path::new(&workspace_root),
            relative_path.as_deref(),
        )
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("spawn_blocking failed: {e}"))?;
    result
}

#[tauri::command]
pub fn search_workspace_entries(
    workspace_root: String,
    query: String,
    limit: Option<u32>,
) -> Result<Vec<pointer_core::workspace_read::WorkspaceEntry>, String> {
    let limit = limit.unwrap_or(80).clamp(1, 200) as usize;
    pointer_core::workspace_read::search_entries(Path::new(&workspace_root), &query, limit)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn read_workspace_file(
    workspace_root: String,
    relative_path: String,
) -> Result<pointer_core::workspace_read::WorkspaceFilePreview, String> {
    let result = tauri::async_runtime::spawn_blocking(move || {
        pointer_core::workspace_read::read_file(Path::new(&workspace_root), &relative_path)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("spawn_blocking failed: {e}"))?;
    result
}

#[tauri::command]
pub fn delete_workspace_path(workspace_root: String, relative_path: String) -> Result<(), String> {
    pointer_core::workspace_read::delete_path(Path::new(&workspace_root), &relative_path)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_workspace_git_status(
    workspace_root: String,
) -> Result<pointer_core::workspace_read::GitStatusResponse, String> {
    let result = tauri::async_runtime::spawn_blocking(move || {
        pointer_core::workspace_read::git_status_response(Path::new(&workspace_root))
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("spawn_blocking failed: {e}"))?;
    result
}

#[tauri::command]
pub async fn get_workspace_git_diff(
    workspace_root: String,
    relative_path: String,
    status: Option<String>,
) -> Result<pointer_core::workspace_read::GitDiff, String> {
    let result = tauri::async_runtime::spawn_blocking(move || {
        pointer_core::workspace_read::git_diff(
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
        agent_mode: payload.agent_mode,
        lead_agent_id: payload.lead_agent_id,
        performance_mode: payload.performance_mode,
        tool_rounds_used_single_start: payload.tool_rounds_used,
        tool_rounds_used_supervisor_start: payload.tool_rounds_used_supervisor,
        workspace_root: payload.workspace_root,
        workspace_inherit_disabled: payload.workspace_inherit_disabled,
        deliver: DeliverTarget::None,
        web_session_auth: None,
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
    state
        .console_sessions
        .create(
            &workspace_root,
            &conversation_id,
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
        Err(pointer_core::i18n::t(
            "err.tool_call_not_pending",
            pointer_core::i18n::current_ui_locale(),
        )
        .into())
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
        Err(pointer_core::i18n::t(
            "err.ask_user_not_pending",
            pointer_core::i18n::current_ui_locale(),
        )
        .into())
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
        Err(pointer_core::i18n::t(
            "err.terminal_input_not_pending",
            pointer_core::i18n::current_ui_locale(),
        )
        .into())
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
        Err(pointer_core::i18n::t(
            "err.terminal_input_not_pending",
            pointer_core::i18n::current_ui_locale(),
        )
        .into())
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
        return Err(pointer_core::i18n::t(
            "err.api_key_missing",
            pointer_core::i18n::current_ui_locale(),
        )
        .into());
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

fn view_plugin(state: &AppState, plugin_id: &str) -> Result<PluginView, String> {
    let record = state.plugins.get(plugin_id).ok_or_else(|| {
        pointer_core::i18n::tf(
            "err.plugin_not_found",
            pointer_core::i18n::current_ui_locale(),
            &[("plugin_id", plugin_id)],
        )
    })?;
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

/// Returns the last annotated PNG from [`capture_and_annotate`] for a conversation.
#[tauri::command]
pub fn preview_computer_annotated_screen(
    conversation_id: String,
    state: State<'_, Arc<AppState>>,
) -> Result<ComputerAnnotatedPreview, String> {
    state
        .computer_state
        .cached_annotated_for_conversation(&conversation_id)
        .map(|(img, _monitor)| ComputerAnnotatedPreview {
            image_base64: pointer_core::agents::computer::screen::encode_image_to_base64(&img),
            image_mime: pointer_core::agents::computer::screen::image_data_url_mime(&img)
                .to_string(),
            caption: "Annotated screenshot".into(),
        })
        .ok_or_else(|| {
            pointer_core::i18n::t(
                "err.no_desktop_screenshot",
                pointer_core::i18n::current_ui_locale(),
            )
            .into()
        })
}

/// Load a saved annotated PNG by path relative to `computer-captures/` (from `AssistantRoundScreen`).
#[tauri::command]
pub fn preview_computer_round_screen(rel_path: String) -> Result<ComputerAnnotatedPreview, String> {
    capture_debug::read_computer_capture_preview(&rel_path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn preview_chat_media(storage_rel_path: String) -> Result<ChatMediaPreview, String> {
    pointer_core::media::read_chat_media_preview(&storage_rel_path).map_err(|e| e.to_string())
}

/// Absolute path for a saved conversation-media file (for desktop video preview via convertFileSrc).
#[tauri::command]
pub fn get_chat_media_local_path(storage_rel_path: String) -> Result<String, String> {
    let path = pointer_core::media::media_abs_path(&storage_rel_path).map_err(|e| e.to_string())?;
    if !path.is_file() {
        return Err(pointer_core::i18n::tf(
            "err.media_file_missing",
            pointer_core::i18n::current_ui_locale(),
            &[("path", &path.display().to_string())],
        ));
    }
    Ok(path.to_string_lossy().to_string())
}

/// Write raw bytes (base64) to an absolute path chosen by the user (e.g. chart PNG export).
#[tauri::command]
pub fn save_bytes_to_path(path: String, content_base64: String) -> Result<(), String> {
    let loc = pointer_core::i18n::current_ui_locale();
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err(pointer_core::i18n::t("err.save_path_empty", loc).into());
    }
    let path_buf =
        pointer_core::media::access::normalize_user_path(trimmed).map_err(|e| e.to_string())?;
    if path_buf
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .is_empty()
    {
        return Err(pointer_core::i18n::t("err.save_path_invalid", loc).into());
    }
    if let Some(parent) = path_buf.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            std::fs::create_dir_all(parent).map_err(|e| {
                pointer_core::i18n::tf("err.create_dir_failed", loc, &[("e", &e.to_string())])
            })?;
        }
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(content_base64.trim())
        .map_err(|e| format!("decode base64: {e}"))?;
    if bytes.is_empty() {
        return Err(pointer_core::i18n::t("err.file_content_empty", loc).into());
    }
    std::fs::write(&path_buf, &bytes).map_err(|e| {
        pointer_core::i18n::tf("err.write_failed", loc, &[("e", &e.to_string())])
    })?;
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
        let loc = pointer_core::i18n::current_ui_locale();
        std::process::Command::new("open")
            .args(["-R", &display])
            .spawn()
            .map_err(|e| {
                pointer_core::i18n::tf("err.open_finder_failed", loc, &[("e", &e.to_string())])
            })?;
        return Ok(());
    }
    #[cfg(target_os = "windows")]
    {
        let loc = pointer_core::i18n::current_ui_locale();
        std::process::Command::new("explorer")
            .args(["/select,", &display])
            .spawn()
            .map_err(|e| {
                pointer_core::i18n::tf(
                    "err.open_file_manager_failed",
                    loc,
                    &[("e", &e.to_string())],
                )
            })?;
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
        Err(pointer_core::i18n::t(
            "err.file_manager_unavailable",
            pointer_core::i18n::current_ui_locale(),
        )
        .into())
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        Err(pointer_core::i18n::t(
            "err.platform_unsupported",
            pointer_core::i18n::current_ui_locale(),
        )
        .into())
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
        return Err(ui_textf(
            "err.file_missing",
            &[("path", &path.display().to_string())],
        ));
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
        Err(ui_text("err.platform_unsupported"))
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

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoOssUploadResult {
    pub remote_url: String,
    pub oss_object_key: String,
    pub storage_rel_path: Option<String>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaOssUploadStatus {
    pub configured: bool,
    pub message: Option<String>,
}

/// Whether the backend can upload Composer videos to OSS (same check as upload commands).
#[tauri::command]
pub fn get_media_oss_upload_status(state: State<'_, Arc<AppState>>) -> MediaOssUploadStatus {
    use pointer_core::media::resolve_media_oss_config;

    let settings = state.effective_settings_view().merged;
    if resolve_media_oss_config(&settings.media_oss).is_some() {
        return MediaOssUploadStatus {
            configured: true,
            message: None,
        };
    }
    MediaOssUploadStatus {
        configured: false,
        message: Some(
            "视频上传需要平台 OSS 配置，请登录 Pointer 账户或联系管理员在官网配置 OSS".into(),
        ),
    }
}

/// Upload a Composer video attachment to OSS (path-based; emits progress events).
#[tauri::command]
pub async fn upload_composer_video_to_oss(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    conversation_id: String,
    attachment_id: String,
    path: String,
    file_name: String,
    mime_type: String,
    compress: bool,
) -> Result<VideoOssUploadResult, String> {
    use pointer_core::media::{resolve_media_oss_config, upload_composer_video_from_path};
    use serde_json::json;

    log::info!(
        "upload_composer_video_to_oss: attachment={attachment_id} path={path} file={file_name} compress={compress}"
    );
    let settings = state.effective_settings_view().merged;
    if resolve_media_oss_config(&settings.media_oss).is_none() {
        log::warn!(
            "upload_composer_video_to_oss: rejected attachment={attachment_id} — OSS not configured"
        );
        return Err("OSS 未配置，请登录 Pointer 账户或联系管理员在官网配置 OSS".into());
    }
    let path_buf = pointer_core::media::access::normalize_user_path(&path).map_err(|e| {
        log::warn!("upload_composer_video_to_oss: invalid path {path}: {e}");
        e.to_string()
    })?;
    if !path_buf.is_file() {
        log::warn!(
            "upload_composer_video_to_oss: file missing attachment={attachment_id} path={}",
            path_buf.display()
        );
        return Err(format!("文件不存在: {}", path_buf.display()));
    }
    let last_pct = std::sync::Arc::new(std::sync::Mutex::new(0u32));
    let aid = attachment_id.clone();
    let app_handle = app.clone();
    let on_progress: std::sync::Arc<dyn Fn(u64, u64) + Send + Sync> =
        std::sync::Arc::new(move |loaded: u64, total: u64| {
            let pct = if total == 0 {
                0
            } else {
                ((loaded.saturating_mul(100)) / total).min(100) as u32
            };
            let mut last = last_pct.lock().expect("progress mutex");
            if pct != *last {
                *last = pct;
                let _ = app_handle.emit(
                    "composer-video-oss-progress",
                    json!({
                        "attachmentId": aid,
                        "loaded": loaded,
                        "total": total,
                        "percent": pct,
                    }),
                );
            }
        });
    let result = upload_composer_video_from_path(
        &settings.media_oss,
        &attachment_id,
        &path_buf,
        &file_name,
        &mime_type,
        compress,
        Some(conversation_id.trim()).filter(|c| !c.is_empty()),
        on_progress,
    )
    .await
    .map_err(format_video_oss_upload_error)?;
    Ok(VideoOssUploadResult {
        remote_url: result.remote_url,
        oss_object_key: result.object_key,
        storage_rel_path: result.storage_rel_path,
    })
}

fn format_video_oss_upload_error(e: impl std::fmt::Display) -> String {
    let msg = e.to_string();
    if msg.contains("NoSuchBucket") {
        return format!(
            "OSS Bucket 不存在，请在阿里云创建对应 Bucket 或将 Endpoint 配置为「https://<bucket>.oss-<region>.aliyuncs.com」格式。详情：{msg}"
        );
    }
    if msg.contains("InvalidAccessKeyId") || msg.contains("SignatureDoesNotMatch") {
        return format!("OSS 凭据无效，请检查官网 OSS 配置中的 AccessKey。详情：{msg}");
    }
    if msg.to_ascii_lowercase().contains("timeout") || msg.contains("timed out") {
        return format!("视频上传超时，请检查网络后重试。详情：{msg}");
    }
    msg
}

/// Upload Composer video bytes to OSS (web file picker on desktop).
#[tauri::command]
pub async fn upload_composer_video_bytes_to_oss(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    conversation_id: String,
    attachment_id: String,
    file_name: String,
    mime_type: String,
    bytes: Vec<u8>,
    compress: bool,
) -> Result<VideoOssUploadResult, String> {
    use pointer_core::media::{resolve_media_oss_config, upload_composer_video_bytes};
    use serde_json::json;

    log::info!(
        "upload_composer_video_bytes_to_oss: attachment={attachment_id} file={file_name} bytes={} compress={compress}",
        bytes.len()
    );
    let settings = state.effective_settings_view().merged;
    if resolve_media_oss_config(&settings.media_oss).is_none() {
        log::warn!(
            "upload_composer_video_bytes_to_oss: rejected attachment={attachment_id} — OSS not configured"
        );
        return Err("OSS 未配置，请登录 Pointer 账户或联系管理员在官网配置 OSS".into());
    }
    let last_pct = std::sync::Arc::new(std::sync::Mutex::new(0u32));
    let aid = attachment_id.clone();
    let app_handle = app.clone();
    let on_progress: std::sync::Arc<dyn Fn(u64, u64) + Send + Sync> =
        std::sync::Arc::new(move |loaded: u64, total: u64| {
            let pct = if total == 0 {
                0
            } else {
                ((loaded.saturating_mul(100)) / total).min(100) as u32
            };
            let mut last = last_pct.lock().expect("progress mutex");
            if pct != *last {
                *last = pct;
                let _ = app_handle.emit(
                    "composer-video-oss-progress",
                    json!({
                        "attachmentId": aid,
                        "loaded": loaded,
                        "total": total,
                        "percent": pct,
                    }),
                );
            }
        });
    let result = upload_composer_video_bytes(
        &settings.media_oss,
        &attachment_id,
        &bytes,
        &file_name,
        &mime_type,
        compress,
        Some(conversation_id.trim()).filter(|c| !c.is_empty()),
        0,
        on_progress,
    )
    .await
    .map_err(format_video_oss_upload_error)?;
    Ok(VideoOssUploadResult {
        remote_url: result.remote_url,
        oss_object_key: result.object_key,
        storage_rel_path: result.storage_rel_path,
    })
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
pub fn preview_media_ref(media_ref: String) -> Result<ChatMediaPreview, String> {
    pointer_core::media::read_media_ref_preview(&media_ref).map_err(|e| e.to_string())
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
pub fn check_media_deps() -> pointer_core::media::MediaDepsStatus {
    pointer_core::media::MediaDepsStatus::probe()
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
pub fn create_project(
    state: State<'_, Arc<AppState>>,
    name: String,
    workspace_root: String,
) -> Result<ProjectCreationResult, String> {
    let uid = state
        .active_platform_auth()
        .platform_user_id()
        .unwrap_or_default();
    storage::create_project(&name, &workspace_root, &uid).map_err(|e| e.to_string())
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
pub fn update_project(
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
    let scope = platform_list_scope(&state);
    let items =
        storage::list_conversation_outline(&scope, &conversation_id).map_err(|e| e.to_string())?;
    log::info!(
        "tauri::list_conversation_outline: id={conversation_id} returned {} rows",
        items.len()
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

#[tauri::command]
pub async fn list_pinned_experiences(
    limit: Option<u32>,
) -> Result<Vec<pointer_core::experiences::ExperienceListItem>, String> {
    let n = limit.unwrap_or(3).max(1).min(10) as usize;
    pointer_core::experiences::fetch_pinned_experiences(n)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_experience_home(
) -> Result<pointer_core::experiences::ExperienceHomeResponse, String> {
    pointer_core::experiences::fetch_experience_home()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn search_experiences(
    query: String,
    limit: Option<u32>,
) -> Result<Vec<pointer_core::experiences::ExperienceListItem>, String> {
    let n = limit.unwrap_or(20).max(1).min(50) as usize;
    pointer_core::experiences::fetch_experience_search(&query, n)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_experience_detail(
    slug: String,
) -> Result<pointer_core::experiences::ExperienceDetail, String> {
    pointer_core::experiences::fetch_experience_detail(&slug)
        .await
        .map_err(|e| e.to_string())
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
    pub agent_mode: Option<String>,
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
    let new = pointer_core::conversation_store::cron_jobs::NewCronJob {
        id: &args.id,
        label: &args.label,
        cron_expr: &cron_expr,
        schedule_kind,
        schedule_raw: Some(schedule_input),
        next_run_at_ms: next_override,
        conversation_id: &args.conversation_id,
        prompt_text: &args.prompt_text,
        agent_mode: args.agent_mode.as_deref(),
        lead_agent_id: args.lead_agent_id.as_deref(),
        enabled: args.enabled,
        deliver: deliver.as_deref(),
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
