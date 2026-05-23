use axum::{
    extract::{DefaultBodyLimit, Path, Query, State},
    http::StatusCode,
    response::sse::{Event, KeepAlive, Sse},
    response::IntoResponse,
    routing::{get, post, put},
    Json, Router,
};
use futures_util::Stream;
use pointer_core::{
    agents::computer::capture_debug,
    agents::AgentDef,
    chat_service::{run_chat, AppState},
    models::{
        ComputerAnnotatedPreview, ComputerMonitor, Conversation, EffectiveSettingsView,
        ModelSettings, PlatformSettings, SendChatPayload, SkillDef, SkillImportResult, StreamEvent,
        ToolDef, UserSettings,
    },
    provider::OpenAIProvider,
    storage,
};
use serde::Deserialize;

#[derive(Deserialize)]
struct ConversationPreviewQuery {
    #[serde(rename = "conversationId")]
    conversation_id: String,
}
use std::{
    convert::Infallible,
    env,
    net::SocketAddr,
    path::PathBuf,
    sync::Arc,
    time::Duration,
};
use tokio::sync::{broadcast, mpsc};
use tower_http::cors::CorsLayer;

#[derive(Clone)]
struct ServerState {
    core: Arc<AppState>,
    events: broadcast::Sender<StreamEvent>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    pointer_core::logging::init_backtrace_defaults();

    const DEFAULT_LOG_FILTER: &str = "warn,pointer_core=info,pointer_server=info";
    let log_dir: PathBuf = env::var("POINTER_SERVER_LOG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| env::current_dir().unwrap_or_default().join("logs"));
    if let Err(err) =
        pointer_core::logging::init_runtime_logging(&log_dir, DEFAULT_LOG_FILTER)
    {
        eprintln!(
            "pointer-server: file logging unavailable ({err}); stderr-only. log_dir={}",
            log_dir.display()
        );
        pointer_core::logging::init_stderr_only_logging(DEFAULT_LOG_FILTER);
        pointer_core::logging::install_panic_hook();
    }

    let core = Arc::new(AppState::new());
    let (events, _) = broadcast::channel::<StreamEvent>(512);
    match capture_debug::purge_computer_captures_older_than_days(capture_debug::CAPTURE_RETENTION_DAYS) {
        Ok(removed) if removed > 0 => {
            let _ = events.send(StreamEvent::UiToast {
                conversation_id: String::new(),
                message: "截图过期已清理".into(),
                level: "warning".into(),
            });
        }
        Ok(_) => {}
        Err(e) => log::warn!("computer capture purge failed: {e}"),
    }
    let state = ServerState { core, events };

    let app = Router::new()
        .route("/api/health", get(|| async { "ok" }))
        .route("/api/settings", get(get_settings).put(update_settings))
        .route("/api/user-settings", put(update_user_settings))
        .route("/api/platform-settings", put(update_platform_settings))
        .route("/api/key", post(set_api_key).delete(clear_api_key))
        .route("/api/test-connection", post(test_connection))
        .route("/api/skills", get(list_skills).post(import_skill_zip))
        .route("/api/tools", get(list_tools))
        .route("/api/agents", get(list_agents))
        .route("/api/task-board/snapshot", get(get_task_board_snapshot))
        .route(
            "/api/computer/annotated-preview",
            get(preview_computer_annotated_screen),
        )
        .route(
            "/api/computer/round-screen-preview",
            get(preview_computer_round_screen),
        )
        .route("/api/computer/monitors", get(list_computer_monitors))
        .route("/api/computer/monitor", post(set_computer_conversation_monitor))
        .route(
            "/api/conversations",
            get(load_conversations).put(save_conversations),
        )
        .route("/api/chat", post(send_chat))
        .route("/api/chat/:conversation_id/cancel", post(cancel_chat))
        .route(
            "/api/chat/:conversation_id/abort-terminal",
            post(abort_terminal_command),
        )
        .route("/api/chat/:conversation_id/stream", get(chat_stream))
        .route("/api/tools/:tool_call_id/approve", post(approve_tool_call))
        .layer(DefaultBodyLimit::max(20 * 1024 * 1024))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr: SocketAddr = std::env::var("POINTER_SERVER_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:8787".into())
        .parse()?;
    println!("Pointer web server listening on http://{addr}");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn get_settings(State(state): State<ServerState>) -> Result<Json<EffectiveSettingsView>, ApiError> {
    Ok(Json(state.core.effective_settings_view()))
}

async fn update_user_settings(
    State(state): State<ServerState>,
    Json(mut user): Json<UserSettings>,
) -> Result<Json<EffectiveSettingsView>, ApiError> {
    if user.theme.trim().is_empty() {
        user.theme = "system".into();
    }
    state.core.save_user_settings(&user)?;
    Ok(Json(state.core.effective_settings_view()))
}

async fn update_platform_settings(
    State(state): State<ServerState>,
    Json(_platform): Json<PlatformSettings>,
) -> Result<Json<EffectiveSettingsView>, ApiError> {
    Err(ApiError(anyhow::anyhow!(
        "web runtime: platform settings are read-only"
    )))
}

async fn update_settings(
    State(state): State<ServerState>,
    Json(settings): Json<ModelSettings>,
) -> Result<Json<EffectiveSettingsView>, ApiError> {
    let user = UserSettings {
        theme: settings.theme.clone(),
        user_nickname: None,
    };
    state.core.save_user_settings(&user)?;
    Ok(Json(state.core.effective_settings_view()))
}

#[derive(Deserialize)]
struct KeyPayload {
    api_key: String,
}

async fn set_api_key(Json(_payload): Json<KeyPayload>) -> Result<StatusCode, ApiError> {
    Ok(StatusCode::NO_CONTENT)
}

async fn clear_api_key() -> Result<StatusCode, ApiError> {
    Ok(StatusCode::NO_CONTENT)
}

async fn test_connection(State(state): State<ServerState>) -> Result<Json<u128>, ApiError> {
    let settings = state.core.effective_settings();
    if settings.api_key.is_empty() {
        return Err(ApiError(anyhow::anyhow!("尚未配置 API Key")));
    }
    let api_key = settings.api_key.clone();
    let provider = OpenAIProvider::new(settings, api_key);
    Ok(Json(provider.test().await?))
}

async fn list_skills(State(state): State<ServerState>) -> Result<Json<Vec<SkillDef>>, ApiError> {
    state.core.skills.reload_external()?;
    Ok(Json(state.core.skills.list()))
}

async fn import_skill_zip(
    State(state): State<ServerState>,
    body: axum::body::Bytes,
) -> Result<Json<SkillImportResult>, ApiError> {
    Ok(Json(state.core.skills.import_zip(&body)?))
}

async fn list_tools(State(state): State<ServerState>) -> Json<Vec<ToolDef>> {
    Json(state.core.tools.list_defs())
}

async fn list_agents(State(state): State<ServerState>) -> Result<Json<Vec<AgentDef>>, ApiError> {
    Ok(Json(state.core.agents.list()))
}

#[derive(Deserialize)]
struct TaskBoardSnapshotQuery {
    #[serde(rename = "conversationId")]
    conversation_id: String,
    #[serde(default, rename = "taskId")]
    task_id: Option<String>,
}

async fn get_task_board_snapshot(
    Query(q): Query<TaskBoardSnapshotQuery>,
    State(state): State<ServerState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    use pointer_core::task_board::sub_agent_task_board_store_key;
    let store_key = match q.task_id.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(tid) => sub_agent_task_board_store_key(&q.conversation_id, tid),
        None => q.conversation_id.clone(),
    };
    Ok(Json(
        state
            .core
            .task_board_store
            .document(&store_key)
            .to_value(),
    ))
}

/// Same as Tauri `preview_computer_annotated_screen`: last cached annotated PNG from a screen inject.
/// Requires `?conversationId=...` to select the session.
async fn preview_computer_annotated_screen(
    Query(q): Query<ConversationPreviewQuery>,
    State(state): State<ServerState>,
) -> Result<Json<ComputerAnnotatedPreview>, ApiError> {
    state
        .core
        .computer_state
        .cached_annotated_for_conversation(&q.conversation_id)
        .map(|(img, _monitor)| ComputerAnnotatedPreview {
            image_base64: pointer_core::agents::computer::screen::encode_image_to_base64(&img),
            image_mime: pointer_core::agents::computer::screen::image_data_url_mime(&img).to_string(),
            caption: "Annotated screenshot".into(),
        })
        .map(Json)
        .ok_or_else(|| {
            ApiError(anyhow::anyhow!(
                "无标注图：请先完成一次桌面注入（发消息），或确认会话ID正确。"
            ))
        })
}

#[derive(Deserialize)]
struct RoundScreenQuery {
    #[serde(rename = "relPath")]
    rel_path: String,
}

/// Same as Tauri `preview_computer_round_screen`: load annotated PNG from `computer-captures/` by relative path.
async fn preview_computer_round_screen(
    Query(q): Query<RoundScreenQuery>,
) -> Result<Json<ComputerAnnotatedPreview>, ApiError> {
    Ok(Json(
        capture_debug::read_computer_capture_preview(&q.rel_path).map_err(ApiError::from)?,
    ))
}

async fn list_computer_monitors() -> Result<Json<Vec<ComputerMonitor>>, ApiError> {
    Ok(Json(
        pointer_core::agents::computer::screen::list_monitors().map_err(ApiError::from)?,
    ))
}

#[derive(Deserialize)]
struct SetMonitorPayload {
    #[serde(rename = "conversationId")]
    conversation_id: String,
    #[serde(default, rename = "monitorId")]
    monitor_id: Option<String>,
}

async fn set_computer_conversation_monitor(
    State(state): State<ServerState>,
    Json(payload): Json<SetMonitorPayload>,
) -> Result<StatusCode, ApiError> {
    state
        .core
        .computer_state
        .set_conversation_monitor(&payload.conversation_id, payload.monitor_id);
    Ok(StatusCode::NO_CONTENT)
}

async fn load_conversations() -> Result<Json<Vec<Conversation>>, ApiError> {
    Ok(Json(storage::load_conversations()?))
}

async fn save_conversations(
    Json(conversations): Json<Vec<Conversation>>,
) -> Result<StatusCode, ApiError> {
    storage::save_conversations(&conversations)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn send_chat(
    State(state): State<ServerState>,
    Json(payload): Json<SendChatPayload>,
) -> Result<StatusCode, ApiError> {
    let core = state.core.clone();
    let broadcast = state.events.clone();
    tokio::spawn(async move {
        let (tx, mut rx) = mpsc::unbounded_channel::<StreamEvent>();
        let forward = tokio::spawn(async move {
            while let Some(ev) = rx.recv().await {
                let _ = broadcast.send(ev);
            }
        });
        let _ = run_chat(
            tx,
            core,
            payload.conversation_id,
            payload.messages,
            payload.enabled_skill_ids,
            payload.agent_mode,
            payload.tool_rounds_used,
            payload.tool_rounds_used_supervisor,
        )
        .await;
        let _ = forward.await;
    });
    Ok(StatusCode::ACCEPTED)
}

async fn cancel_chat(
    State(state): State<ServerState>,
    Path(conversation_id): Path<String>,
) -> StatusCode {
    state.core.cancel(&conversation_id);
    StatusCode::NO_CONTENT
}

async fn abort_terminal_command(
    State(state): State<ServerState>,
    Path(conversation_id): Path<String>,
) -> axum::Json<serde_json::Value> {
    let aborted = state.core.abort_terminal_command(&conversation_id);
    axum::Json(serde_json::json!({ "aborted": aborted }))
}

#[derive(Deserialize)]
struct ApprovalPayload {
    approved: bool,
}

async fn approve_tool_call(
    State(state): State<ServerState>,
    Path(tool_call_id): Path<String>,
    Json(payload): Json<ApprovalPayload>,
) -> Result<StatusCode, ApiError> {
    if state
        .core
        .approve_tool_call(&tool_call_id, payload.approved)
    {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError(anyhow::anyhow!("未找到待审批的工具调用")))
    }
}

async fn chat_stream(
    State(state): State<ServerState>,
    Path(conversation_id): Path<String>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut rx = state.events.subscribe();
    let stream = async_stream::stream! {
        loop {
            match rx.recv().await {
                Ok(ev) => {
                    let belongs = conversation_id == "global" || match &ev {
                        StreamEvent::MessageStart { conversation_id: id, .. } => id == &conversation_id,
                        StreamEvent::Done {
                            conversation_id: id,
                            ..
                        } => id == &conversation_id,
                        StreamEvent::HistoryReplaced { conversation_id: id, .. } => id == &conversation_id,
                        StreamEvent::ToolRoundsExhausted { conversation_id: id, .. } => {
                            id == &conversation_id
                        }
                        StreamEvent::UiToast { conversation_id: id, .. } => {
                            id.is_empty() || id == &conversation_id
                        }
                        StreamEvent::InjectedUserMessage { conversation_id: id, .. } => {
                            id == &conversation_id
                        }
                        StreamEvent::InjectedAssistantMessage { conversation_id: id, .. } => {
                            id == &conversation_id
                        }
                        StreamEvent::InjectedAssistantMessageUpdate { conversation_id: id, .. } => {
                            id == &conversation_id
                        }
                        StreamEvent::AssistantRoundScreen { conversation_id: id, .. } => {
                            id == &conversation_id
                        }
                        _ => true,
                    };
                    if belongs {
                        let data = serde_json::to_string(&ev).unwrap_or_else(|_| "{}".into());
                        yield Ok(Event::default().event("message").data(data));
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    };
    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    )
}

struct ApiError(anyhow::Error);

impl From<anyhow::Error> for ApiError {
    fn from(err: anyhow::Error) -> Self {
        Self(err)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        (StatusCode::INTERNAL_SERVER_ERROR, self.0.to_string()).into_response()
    }
}
