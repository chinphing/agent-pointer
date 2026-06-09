use pointer_core::agents::computer::capture_debug;
use pointer_core::agents::AgentDef;
use pointer_core::chat_service::{run_chat, AppState};
use pointer_core::models::{
    ChatMediaPreview, ComputerAnnotatedPreview, ComputerMonitor, Conversation,
    EffectiveSettingsView, ModelSettings, PlatformSettings, SendChatPayload, SkillDef,
    SkillImportResult, StreamEvent, ToolDef, UserSettings,
};

use pointer_core::provider::OpenAIProvider;
use pointer_core::storage;
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
        let app_for_events = app.clone();
        tauri::async_runtime::spawn(async move {
            while let Some(ev) = rx.recv().await {
                let _ = app_for_events.emit(STREAM_EVENT, ev);
            }
        });
        let _ = run_chat(
            tx,
            st,
            payload.conversation_id,
            payload.messages,
            payload.enabled_skill_ids,
            payload.agent_mode,
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
pub fn save_conversations(conversations: Vec<Conversation>) -> Result<(), String> {
    storage::save_conversations(&conversations).map_err(|e| e.to_string())
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
