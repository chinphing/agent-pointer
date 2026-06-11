use axum::{
    extract::{DefaultBodyLimit, Path, Query, State},
    http::StatusCode,
    response::sse::{Event, KeepAlive, Sse},
    response::IntoResponse,
    routing::{get, post, put},
    Json, Router,
};
use futures_util::Stream;
use pointer_channels::{
    adapters::register_builtin_channels,
    adapters::weixin::qr_login::QrLoginState,
    registration::ChannelRegistrationState,
    gateway::ChannelGateway,
    registry::ChannelRegistry,
};
use pointer_core::{
    agents::computer::capture_debug,
    agents::AgentDef,
    chat_service::{run_chat, AppState},
    models::{
        ComputerAnnotatedPreview, ChatMediaPreview, ComputerMonitor, Conversation, EffectiveSettingsView,
        ModelSettings, PlatformSettings, SendChatPayload, SkillDef, SkillImportResult, StreamEvent,
        ToolDef, UserSettings,
    },
    provider::OpenAIProvider,
    storage,
};
mod channels;

use channels::{
    approve_channel_pairing, channel_webhook, get_channel_webhook_url, get_channels_config,
    list_channel_pairing_pending, list_channels,     channel_registration_status, start_channel_registration, start_weixin_login, update_channels,
    weixin_login_status,
};
use base64::Engine;
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
pub(crate) struct ServerState {
    core: Arc<AppState>,
    events: broadcast::Sender<StreamEvent>,
    channel_gateway: Arc<ChannelGateway>,
    qr_login: Arc<QrLoginState>,
    registration: Arc<ChannelRegistrationState>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    pointer_core::logging::init_backtrace_defaults();

    const DEFAULT_LOG_FILTER: &str =
        "warn,pointer_core=info,pointer_server=info,pointer_channels=info";
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
    core.start_background_tasks();
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
    let mut channel_registry = ChannelRegistry::new();
    register_builtin_channels(&mut channel_registry);
    let channel_gateway = Arc::new(ChannelGateway::new(core.clone(), channel_registry)?);
    pointer_channels::install_channel_outbound_bridge(
        channel_gateway.clone(),
        core.tools.clone(),
    );
    let cancel = tokio_util::sync::CancellationToken::new();
    channel_gateway.spawn_weixin_monitors(cancel.clone());
    channel_gateway.spawn_wecom_monitors(cancel.clone());
    channel_gateway.spawn_feishu_monitors(cancel.clone());
    channel_gateway.spawn_dingtalk_monitors(cancel.clone());

    let state = ServerState {
        core,
        events,
        channel_gateway,
        qr_login: Arc::new(QrLoginState::new()),
        registration: Arc::new(ChannelRegistrationState::new()),
    };

    let app = Router::new()
        .route("/api/health", get(|| async { "ok" }))
        .route("/api/settings", get(get_settings).put(update_settings))
        .route("/api/agent-settings", put(update_agent_settings))
        .route("/api/user-settings", put(update_user_settings))
        .route("/api/platform-settings", put(update_platform_settings))
        .route("/api/key", post(set_api_key).delete(clear_api_key))
        .route("/api/test-connection", post(test_connection))
        .route("/api/skills", get(list_skills).post(import_skill_zip))
        .route("/api/skills/reload-meta", post(reload_skill_meta))
        .route("/api/skills/external-probe", get(probe_external_skills))
        .route("/api/skills/import-external", post(import_external_skills))
        .route("/api/skills/external-probe/dismiss", post(dismiss_external_skills_prompt))
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
            "/api/computer/monitor-pick/:conversation_id/confirm",
            post(confirm_computer_monitor_pick),
        )
        .route(
            "/api/computer/monitor-pick/:conversation_id/cancel",
            post(cancel_computer_monitor_pick),
        )
        .route(
            "/api/conversations",
            get(load_conversations).put(save_conversations),
        )
        .route("/api/conversations/meta", put(save_conversation_meta))
        .route(
            "/api/conversations/:conversation_id/messages/append",
            post(append_conversation_messages),
        )
        .route("/api/experiences/pinned", get(list_pinned_experiences))
        .route("/api/experiences/home", get(get_experience_home))
        .route("/api/experiences/search", get(search_experiences))
        .route("/api/experiences/:slug", get(get_experience_detail))
        .route("/api/chat/media-preview", get(preview_chat_media))
        .route("/api/chat/media-ref-preview", get(preview_media_ref))
        .route("/api/chat/save-attachment", post(save_chat_attachment))
        .route("/api/media/deps", get(check_media_deps))
        .route("/api/chat", post(send_chat))
        .route("/api/chat/:conversation_id/cancel", post(cancel_chat))
        .route(
            "/api/chat/:conversation_id/abort-terminal",
            post(abort_terminal_command),
        )
        .route("/api/chat/:conversation_id/stream", get(chat_stream))
        .route("/api/tools/:tool_call_id/approve", post(approve_tool_call))
        .route(
            "/webhooks/:channel/:account_id",
            post(channel_webhook).get(channel_webhook),
        )
        .route("/api/channels", get(list_channels).put(update_channels))
        .route("/api/channels/config", get(get_channels_config))
        .route(
            "/api/channels/:channel/:account_id/webhook-url",
            get(get_channel_webhook_url),
        )
        .route(
            "/api/channels/weixin/:account_id/login/start",
            post(start_weixin_login),
        )
        .route(
            "/api/channels/weixin/:account_id/login/status",
            get(weixin_login_status),
        )
        .route(
            "/api/channels/:channel/:account_id/register/start",
            post(start_channel_registration),
        )
        .route(
            "/api/channels/:channel/:account_id/register/status",
            get(channel_registration_status),
        )
        .route(
            "/api/channels/:channel/:account_id/pairing/approve",
            post(approve_channel_pairing),
        )
        .route(
            "/api/channels/:channel/:account_id/pairing/pending",
            get(list_channel_pairing_pending),
        )
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
    state
        .core
        .apply_session_platform_preferences(&settings)?;
    Ok(Json(state.core.effective_settings_view()))
}

async fn update_agent_settings(
    State(state): State<ServerState>,
    Json(settings): Json<ModelSettings>,
) -> Result<Json<EffectiveSettingsView>, ApiError> {
    state
        .core
        .update_agent_settings(&settings)
        .map_err(ApiError)?;
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
    Ok(Json(state.core.skills.list()))
}

async fn reload_skill_meta(
    State(state): State<ServerState>,
) -> Result<Json<Vec<SkillDef>>, ApiError> {
    state.core.skills.reload_meta()?;
    Ok(Json(state.core.skills.list()))
}

async fn import_skill_zip(
    State(state): State<ServerState>,
    body: axum::body::Bytes,
) -> Result<Json<SkillImportResult>, ApiError> {
    Ok(Json(state.core.skills.import_zip(&body)?))
}

async fn probe_external_skills() -> Result<Json<pointer_core::skills::external_probe::ExternalSkillsProbeResult>, ApiError> {
    Ok(Json(pointer_core::skills::external_probe::probe_external_skill_sources()?))
}

#[derive(serde::Deserialize)]
struct ImportExternalSkillsBody {
    #[serde(rename = "sourceIds")]
    source_ids: Vec<String>,
}

async fn import_external_skills(
    State(state): State<ServerState>,
    Json(body): Json<ImportExternalSkillsBody>,
) -> Result<Json<SkillImportResult>, ApiError> {
    let result = pointer_core::skills::external_probe::import_external_skills(&body.source_ids)?;
    state.core.skills.reload_meta()?;
    Ok(Json(result))
}

async fn dismiss_external_skills_prompt() -> Result<StatusCode, ApiError> {
    pointer_core::skills::external_probe::dismiss_external_skills_prompt()?;
    Ok(StatusCode::NO_CONTENT)
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
                "暂无桌面截图：请先完成一次截图处理（发送 Computer 消息），或确认会话 ID 正确。"
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

#[derive(Deserialize)]
struct ChatMediaQuery {
    #[serde(rename = "storageRelPath")]
    storage_rel_path: String,
}

async fn preview_chat_media(
    Query(q): Query<ChatMediaQuery>,
) -> Result<Json<ChatMediaPreview>, ApiError> {
    Ok(Json(
        pointer_core::media::read_chat_media_preview(&q.storage_rel_path).map_err(ApiError::from)?,
    ))
}

#[derive(Deserialize)]
struct MediaRefQuery {
    #[serde(rename = "mediaRef")]
    media_ref: String,
}

async fn preview_media_ref(
    Query(q): Query<MediaRefQuery>,
) -> Result<Json<ChatMediaPreview>, ApiError> {
    let extra_roots = pointer_channels::config::load_channels_config()
        .ok()
        .map(|c| c.meta.media_local_roots)
        .unwrap_or_default();
    Ok(Json(
        pointer_core::media::read_media_ref_preview(&q.media_ref, &extra_roots)
            .map_err(ApiError::from)?,
    ))
}

#[derive(Deserialize)]
struct SaveChatAttachmentPayload {
    #[serde(rename = "conversationId")]
    conversation_id: String,
    #[serde(rename = "attachmentId")]
    attachment_id: String,
    #[serde(rename = "contentBase64")]
    content_base64: String,
    #[serde(rename = "fileName")]
    file_name: String,
}

#[derive(serde::Serialize)]
struct SaveChatAttachmentResponse {
    #[serde(rename = "storageRelPath")]
    storage_rel_path: String,
}

async fn save_chat_attachment(
    Json(payload): Json<SaveChatAttachmentPayload>,
) -> Result<Json<SaveChatAttachmentResponse>, ApiError> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(payload.content_base64.trim())
        .map_err(|e| ApiError(anyhow::anyhow!("decode attachment base64: {e}")))?;
    let storage_rel_path = pointer_core::media::save_attachment_bytes(
        &payload.conversation_id,
        &payload.attachment_id,
        &bytes,
        &payload.file_name,
    )
    .map_err(ApiError::from)?;
    Ok(Json(SaveChatAttachmentResponse { storage_rel_path }))
}

async fn check_media_deps() -> Json<pointer_core::media::MediaDepsStatus> {
    Json(pointer_core::media::MediaDepsStatus::probe())
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

async fn confirm_computer_monitor_pick(
    State(state): State<ServerState>,
    axum::extract::Path(conversation_id): axum::extract::Path<String>,
) -> Result<StatusCode, ApiError> {
    if state.core.confirm_computer_monitor_pick(&conversation_id) {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::from(anyhow::anyhow!("no pending monitor pick")))
    }
}

async fn cancel_computer_monitor_pick(
    State(state): State<ServerState>,
    axum::extract::Path(conversation_id): axum::extract::Path<String>,
) -> Result<StatusCode, ApiError> {
    if state.core.cancel_computer_monitor_pick(&conversation_id) {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::from(anyhow::anyhow!("no pending monitor pick")))
    }
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

async fn save_conversation_meta(
    Json(metas): Json<Vec<pointer_core::models::ConversationMeta>>,
) -> Result<StatusCode, ApiError> {
    storage::save_conversation_meta(&metas)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn append_conversation_messages(
    axum::extract::Path(conversation_id): axum::extract::Path<String>,
    Json(messages): Json<Vec<pointer_core::models::ChatMessage>>,
) -> Result<Json<u32>, ApiError> {
    let written = storage::append_conversation_messages(&conversation_id, &messages)?;
    Ok(Json(written))
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
            payload.workspace_root,
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

#[derive(Deserialize)]
struct PinnedExperiencesQuery {
    limit: Option<u32>,
}

async fn list_pinned_experiences(
    Query(q): Query<PinnedExperiencesQuery>,
) -> Result<Json<Vec<pointer_core::experiences::ExperienceListItem>>, ApiError> {
    let n = q.limit.unwrap_or(3).max(1).min(10) as usize;
    let rows = pointer_core::experiences::fetch_pinned_experiences(n).await?;
    Ok(Json(rows))
}

async fn get_experience_home() -> Result<Json<pointer_core::experiences::ExperienceHomeResponse>, ApiError> {
    let home = pointer_core::experiences::fetch_experience_home().await?;
    Ok(Json(home))
}

#[derive(Deserialize)]
struct SearchExperiencesQuery {
    q: Option<String>,
    limit: Option<u32>,
}

async fn search_experiences(
    Query(q): Query<SearchExperiencesQuery>,
) -> Result<Json<Vec<pointer_core::experiences::ExperienceListItem>>, ApiError> {
    let query = q.q.unwrap_or_default();
    let n = q.limit.unwrap_or(20).max(1).min(50) as usize;
    let rows = pointer_core::experiences::fetch_experience_search(&query, n).await?;
    Ok(Json(rows))
}

async fn get_experience_detail(
    Path(slug): Path<String>,
) -> Result<Json<pointer_core::experiences::ExperienceDetail>, ApiError> {
    let detail = pointer_core::experiences::fetch_experience_detail(&slug).await?;
    Ok(Json(detail))
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
