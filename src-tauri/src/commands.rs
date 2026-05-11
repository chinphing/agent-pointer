use pointer_core::agents::computer::capture_debug;
use pointer_core::agents::AgentDef;
use pointer_core::chat_service::{run_chat, AppState};
use pointer_core::models::{
    ComputerAnnotatedPreview, Conversation, ModelSettings, SendChatPayload, SkillDef,
    SkillImportResult, StreamEvent, ToolDef,
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
pub fn get_settings() -> Result<ModelSettings, String> {
    storage::load_settings().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_settings(settings: ModelSettings) -> Result<ModelSettings, String> {
    // 同步激活 Provider 的 Key 到 key.dat，确保后端请求时使用正确的密钥
    if let Some(provider) = settings.providers.iter().find(|p| p.id == settings.active_provider_id) {
        if !provider.api_key.is_empty() {
            storage::save_api_key(&provider.api_key).map_err(|e| e.to_string())?;
        } else {
            // 如果激活的 Provider 没有 Key，则清理旧 Key 防止残留
            let _ = storage::clear_api_key();
        }
    }
    storage::save_settings(&settings).map_err(|e| e.to_string())?;
    storage::load_settings().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_api_key(api_key: String) -> Result<(), String> {
    storage::save_api_key(&api_key).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn clear_api_key() -> Result<(), String> {
    storage::clear_api_key().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn test_connection() -> Result<u128, String> {
    let mut settings = storage::load_settings().map_err(|e| e.to_string())?;
    let api_key = storage::load_api_key()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "尚未配置 API Key".to_string())?;
    settings.api_key = api_key.clone();
    let provider = OpenAIProvider::new(settings, api_key);
    provider.test().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_skills(state: State<'_, Arc<AppState>>) -> Result<Vec<SkillDef>, String> {
    state.skills.reload_external().map_err(|e| e.to_string())?;
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

/// Returns the last annotated PNG from [`capture_and_annotate`] (e.g. screen inject). No new capture.
#[tauri::command]
pub fn preview_computer_annotated_screen(
    state: State<'_, Arc<AppState>>,
) -> Result<ComputerAnnotatedPreview, String> {
    state
        .computer_state
        .cached_annotated_preview()
        .ok_or_else(|| "暂无标注截图：需先在本会话中完成一次 Computer 屏幕注入（发消息触发），或检查标注服务是否正常。".into())
}

/// Load a saved annotated PNG by path relative to `computer-captures/` (from `AssistantRoundScreen`).
#[tauri::command]
pub fn preview_computer_round_screen(rel_path: String) -> Result<ComputerAnnotatedPreview, String> {
    capture_debug::read_computer_capture_preview(&rel_path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn load_conversations() -> Result<Vec<Conversation>, String> {
    storage::load_conversations().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_conversations(conversations: Vec<Conversation>) -> Result<(), String> {
    storage::save_conversations(&conversations).map_err(|e| e.to_string())
}
