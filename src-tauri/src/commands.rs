use pointer_core::agents::computer::capture_debug;
use pointer_core::agents::AgentDef;
use pointer_core::chat_service::{run_chat, AppState};
use pointer_core::models::{
    ChatMediaPreview, ChatMessage, ComputerAnnotatedPreview, ComputerMonitor, Conversation,
    EffectiveSettingsView, ModelSettings, PlatformSettings, SendChatPayload, SkillDef,
    SkillImportResult, StreamEvent, ToolDef, UserSettings,
};

use pointer_core::provider::OpenAIProvider;
use pointer_core::storage;
use base64::Engine;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::mpsc;

pub const STREAM_EVENT: &str = "chat://stream";

#[tauri::command]
pub async fn send_chat(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    payload: SendChatPayload,
) -> Result<(), String> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn(async move {
        let (tx, mut rx) = mpsc::unbounded_channel::<StreamEvent>();
        tauri::async_runtime::spawn(async move {
            while rx.recv().await.is_some() {}
        });
        let _ = run_chat(
            tx,
            st,
            payload.conversation_id,
            payload.messages,
            payload.enabled_skill_ids,
            payload.agent_mode,
            payload.lead_agent_id.clone(),
            payload.tool_rounds_used,
            payload.tool_rounds_used_supervisor,
            payload.workspace_root,
        )
        .await;
    });
    Ok(())
}

#[tauri::command]
pub fn cancel_chat(state: State<'_, Arc<AppState>>, conversation_id: String) -> Result<(), String> {
    state.cancel(&conversation_id);
    Ok(())
}

/// Kill only the in-flight **`terminal`** subprocess for this conversation (does not stop the LLM turn).
#[tauri::command]
pub fn abort_terminal_command(
    state: State<'_, Arc<AppState>>,
    conversation_id: String,
) -> Result<bool, String> {
    Ok(state.abort_terminal_command(&conversation_id))
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
pub fn get_settings(state: State<'_, Arc<AppState>>) -> Result<EffectiveSettingsView, String> {
    Ok(state.effective_settings_view())
}

#[tauri::command]
pub fn update_user_settings(
    state: State<'_, Arc<AppState>>,
    mut user: UserSettings,
) -> Result<EffectiveSettingsView, String> {
    if user.theme.trim().is_empty() {
        user.theme = "system".into();
    }
    state.save_user_settings(&user).map_err(|e| e.to_string())?;
    Ok(state.effective_settings_view())
}

#[tauri::command]
pub fn update_platform_settings(
    state: State<'_, Arc<AppState>>,
    platform: PlatformSettings,
) -> Result<EffectiveSettingsView, String> {
    state
        .update_platform_settings(platform)
        .map_err(|e| e.to_string())
}

/// Back-compat: session preferences in memory only; agent section uses update_agent_settings.
#[tauri::command]
pub fn update_settings(
    state: State<'_, Arc<AppState>>,
    settings: ModelSettings,
) -> Result<EffectiveSettingsView, String> {
    state
        .apply_session_platform_preferences(&settings)
        .map_err(|e| e.to_string())?;
    Ok(state.effective_settings_view())
}

#[tauri::command]
pub fn update_agent_settings(
    state: State<'_, Arc<AppState>>,
    settings: ModelSettings,
) -> Result<EffectiveSettingsView, String> {
    state
        .update_agent_settings(&settings)
        .map_err(|e| e.to_string())
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
    state.skills.reload_meta().map_err(|e| e.to_string())?;
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
pub fn probe_external_skills() -> Result<pointer_core::skills::external_probe::ExternalSkillsProbeResult, String> {
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
    pointer_core::skills::external_probe::dismiss_external_skills_prompt().map_err(|e| e.to_string())
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
    use pointer_core::task_board::sub_agent_task_board_store_key;
    let store_key = match task_id.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(tid) => {
            let parent_key = state
                .get_active_main_task_board_key(&conversation_id)
                .unwrap_or_else(|| conversation_id.clone());
            let preferred = sub_agent_task_board_store_key(&parent_key, tid);
            let preferred_doc = state.task_board_store.document(&preferred);
            if !preferred_doc.board_is_empty() || !preferred_doc.meta.goal.trim().is_empty() {
                preferred
            } else {
                let suffix = format!("\u{1f}ptr_sub_agent\u{1f}{tid}");
                let matches: Vec<String> = state
                    .task_board_store
                    .list_store_keys_by_prefix(&conversation_id)
                    .into_iter()
                    .filter(|k| k.ends_with(&suffix))
                    .collect();
                matches.into_iter().next().unwrap_or(preferred)
            }
        }
        None => state
            .get_active_main_task_board_key(&conversation_id)
            .unwrap_or_else(|| conversation_id.clone()),
    };
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
            image_mime: pointer_core::agents::computer::screen::image_data_url_mime(&img).to_string(),
            caption: "Annotated screenshot".into(),
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
pub fn preview_chat_media(storage_rel_path: String) -> Result<ChatMediaPreview, String> {
    pointer_core::media::read_chat_media_preview(&storage_rel_path).map_err(|e| e.to_string())
}

/// Reveal a local file in Finder (macOS) or file manager (other platforms).
#[tauri::command]
pub fn reveal_in_finder(path: String) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .args(["-R", &path])
            .spawn()
            .map_err(|e| format!("打开 Finder 失败: {e}"))?;
        return Ok(());
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .args(["/select,", &path])
            .spawn()
            .map_err(|e| format!("打开文件管理器失败: {e}"))?;
        return Ok(());
    }
    #[cfg(target_os = "linux")]
    {
        // Try common file managers
        for (cmd, args) in &[
            ("xdg-open", vec![std::path::Path::new(&path).parent().unwrap_or(std::path::Path::new("/")).to_string_lossy().to_string()]),
            ("nautilus", vec![path.clone()]),
            ("dolphin", vec![format!("--select={path}")]),
            ("nemo", vec![path.clone()]),
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

const MAX_LOCAL_ATTACHMENT_BYTES: u64 = 30 * 1024 * 1024;

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
    let path_buf = pointer_core::media::access::normalize_user_path(&path).map_err(|e| e.to_string())?;
    let canonical = path_buf
        .canonicalize()
        .unwrap_or(path_buf);
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
pub fn read_local_file_for_attachment(path: String) -> Result<LocalFileAttachmentPayload, String> {
    let path_buf = pointer_core::media::access::normalize_user_path(&path).map_err(|e| e.to_string())?;
    if !path_buf.is_file() {
        return Err(format!("文件不存在: {}", path_buf.display()));
    }
    let meta = fs::metadata(&path_buf).map_err(|e| format!("读取文件信息失败: {e}"))?;
    if meta.len() > MAX_LOCAL_ATTACHMENT_BYTES {
        let limit_mb = MAX_LOCAL_ATTACHMENT_BYTES / (1024 * 1024);
        return Err(format!("文件超过 {limit_mb} MB 上限"));
    }
    let bytes = fs::read(&path_buf).map_err(|e| format!("读取文件失败: {e}"))?;
    let file_name = path_buf
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("attachment")
        .to_string();
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
    let extra_roots = pointer_channels::config::load_channels_config()
        .ok()
        .map(|c| c.meta.media_local_roots)
        .unwrap_or_default();
    pointer_core::media::read_media_ref_preview(&media_ref, &extra_roots).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_chat_attachment(
    conversation_id: String,
    attachment_id: String,
    content_base64: String,
    file_name: String,
) -> Result<String, String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(content_base64.trim())
        .map_err(|e| format!("decode attachment base64: {e}"))?;
    pointer_core::media::save_attachment_bytes(
        &conversation_id,
        &attachment_id,
        &bytes,
        &file_name,
    )
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
    storage::load_conversations().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn load_conversation_messages(conversation_id: String) -> Result<Vec<ChatMessage>, String> {
    storage::load_conversation_messages(&conversation_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_conversations(conversations: Vec<Conversation>) -> Result<(), String> {
    storage::save_conversations(&conversations).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_conversation_meta(metas: Vec<pointer_core::models::ConversationMeta>) -> Result<(), String> {
    storage::save_conversation_meta(&metas).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn append_conversation_messages(
    conversation_id: String,
    messages: Vec<pointer_core::models::ChatMessage>,
) -> Result<u32, String> {
    storage::append_conversation_messages(&conversation_id, &messages).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_pinned_experiences(limit: Option<u32>) -> Result<Vec<pointer_core::experiences::ExperienceListItem>, String> {
    let n = limit.unwrap_or(3).max(1).min(10) as usize;
    pointer_core::experiences::fetch_pinned_experiences(n)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_experience_home() -> Result<pointer_core::experiences::ExperienceHomeResponse, String> {
    pointer_core::experiences::fetch_experience_home()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn search_experiences(query: String, limit: Option<u32>) -> Result<Vec<pointer_core::experiences::ExperienceListItem>, String> {
    let n = limit.unwrap_or(20).max(1).min(50) as usize;
    pointer_core::experiences::fetch_experience_search(&query, n)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_experience_detail(slug: String) -> Result<pointer_core::experiences::ExperienceDetail, String> {
    pointer_core::experiences::fetch_experience_detail(&slug)
        .await
        .map_err(|e| e.to_string())
}
