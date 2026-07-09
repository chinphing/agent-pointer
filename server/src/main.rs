use axum::{
    body::Bytes,
    extract::{DefaultBodyLimit, Multipart, Path, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode, Uri},
    middleware,
    response::sse::{Event, KeepAlive, Sse},
    response::{IntoResponse, Redirect, Response},
    routing::{delete, get, post, put},
    Json, Router,
};
use futures_util::Stream;
use pointer_channels::{
    adapters::register_builtin_channels,
    adapters::weixin::qr_login::QrLoginState,
    gateway::ChannelGateway,
    monitor_supervisor::MonitorSupervisor,
    registration::ChannelRegistrationState,
    registry::ChannelRegistry,
};
use pointer_core::{
    agents::computer::capture_debug,
    agents::AgentDef,
    chat_service::AppState,
    dispatcher::{
        DeliverTarget, RunDispatcher, RunHandle, RunOutcome, TriggerMeta, TriggerRequest,
        TriggerSource,
    },
    models::{
        ComputerAnnotatedPreview, ChatMediaPreview, ComputerMonitor, Conversation,
        WebEffectiveSettingsView,
        ModelSettings, PlatformSettings, SendChatPayload, SkillDef, SkillImportResult, StreamEvent,
        ToolDef, UserSettings,
    },
    platform_auth::{PlatformAuthManager, PlatformSessionView},
    platform_config::apply_login_media_oss,
    provider::OpenAIProvider,
    storage,
};
mod channels;
mod local_auth;
mod web_session;

use web_session::WebSessionStore;

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
    collections::HashMap,
    convert::Infallible,
    env,
    net::SocketAddr,
    path::{PathBuf},
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};
use parking_lot::RwLock;
use rand::RngCore;
use tokio::sync::broadcast;
use tower_http::cors::CorsLayer;

/// Lifetime of a pending PKCE login entry. The user must complete the
/// browser OAuth flow within this window or the callback will reject it.
const OAUTH_PENDING_TTL: Duration = Duration::from_secs(600);

#[derive(Clone)]
pub(crate) struct ServerState {
    core: Arc<AppState>,
    /// Unified run dispatcher: the callable / event-triggered entry point.
    /// `POST /api/chat` and `POST /api/runs` (Phase 4) route through it.
    dispatcher: Arc<RunDispatcher>,
    events: broadcast::Sender<StreamEvent>,
    channel_gateway: Arc<ChannelGateway>,
    channel_monitors: Arc<MonitorSupervisor>,
    qr_login: Arc<QrLoginState>,
    registration: Arc<ChannelRegistrationState>,
    /// PKCE verifiers keyed by `state` for in-flight browser OAuth logins.
    oauth_pending: Arc<RwLock<HashMap<String, PkcePending>>>,
    /// Per-browser platform OAuth sessions (cookie `pointer_web_session`).
    web_sessions: Arc<WebSessionStore>,
}

#[derive(Debug, Clone)]
struct PkcePending {
    verifier: String,
    redirect_uri: String,
    created_at: Instant,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    pointer_core::logging::init_backtrace_defaults();

    // --machine-id / --machine-id-json: print binding material and exit (no config/license needed)
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--machine-id-json") {
        match pointer_core::license::current_machine_identity() {
            Ok(view) => {
                println!("{}", serde_json::to_string_pretty(&view)?);
                return Ok(());
            }
            Err(e) => {
                eprintln!("error: failed to read machine identity: {e}");
                std::process::exit(1);
            }
        }
    }
    if args.iter().any(|a| a == "--machine-id") {
        match pointer_core::license::current_machine_id() {
            Ok(id) => {
                println!("{}", id);
                return Ok(());
            }
            Err(e) => {
                eprintln!("error: failed to read machine id: {e}");
                std::process::exit(1);
            }
        }
    }

    let loaded_config = pointer_core::server_config::load_server_config()?;

    const DEFAULT_LOG_FILTER: &str =
        "warn,pointer_core=info,pointer_server=info,pointer_channels=info";
    let log_dir = pointer_core::logging::desktop_log_dir();
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
    if let Some(result) = loaded_config {
        log::info!("pointer-server: config file {}", result.path.display());
    }
    pointer_core::license::validate_license_at_startup()?;
    pointer_core::tls::ensure_rustls_crypto_provider();

    if let Err(err) = pointer_core::skills::external::install_deploy_bundled_skills() {
        log::warn!("bundled skills install failed: {err:#}");
    }

    storage::set_platform_auth_persist_enabled(false);
    if pointer_core::deployment_mode::is_standalone() {
        log::info!("pointer-server: standalone mode — local admin auth + config-injected LLM keys");
    } else {
        log::info!("pointer-server: web mode — auth.dat persistence disabled; per-browser cookie sessions");
    }

    let core = Arc::new(AppState::new());
    if pointer_core::deployment_mode::is_standalone() {
        let mut platform = core.platform_config.write();
        pointer_core::server_config::apply_llm_providers_from_config(&mut platform);
        log::info!(
            "pointer-server: standalone LLM providers applied (active_provider={})",
            platform.active_provider_id
        );
    }
    core.start_background_tasks();
    let web_sessions = Arc::new(WebSessionStore::default());
    match resolve_server_public_url() {
        Some(url) => log::info!("platform_auth: server public url = {url}"),
        None => {
            if pointer_core::deployment_mode::is_standalone() {
                log::warn!(
                    "platform_auth: POINTER_SERVER_PUBLIC_URL not configured (standalone); \
                     OAuth routes disabled, use POST /api/auth/local/login"
                );
            } else {
                log::warn!(
                    "platform_auth: POINTER_SERVER_PUBLIC_URL not configured; \
                     /api/auth/login/start will return 500 until set in pointer-server.toml [server].public_url"
                );
            }
        }
    }
    if !pointer_core::deployment_mode::is_standalone() {
        pointer_core::server_access::validate_server_access_at_startup(
            resolve_server_public_url().as_deref(),
        )?;
    }
    let (events, _) = broadcast::channel::<StreamEvent>(512);
    // Bridge global stream_broadcast -> server events so the SSE endpoint
    // (`GET /api/chat/:id/stream`) keeps working regardless of who calls
    // `run_chat`. The dispatcher calls `run_chat`, which emits via
    // `publish_stream` -> `broadcast_stream` -> this callback -> `events`.
    {
        let ev_tx = events.clone();
        pointer_core::stream_broadcast::subscribe_stream(Arc::new(move |ev| {
            // No subscribers -> send fails; ignore (matches existing behavior).
            let _ = ev_tx.send(ev);
        }));
    }
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
    let channel_monitors = Arc::new(MonitorSupervisor::new());
    channel_monitors.start(channel_gateway.clone());
    let registration = Arc::new(ChannelRegistrationState::with_completion(
        channel_gateway.clone(),
        channel_monitors.clone(),
    ));

    let state = ServerState {
        core: core.clone(),
        dispatcher: Arc::new(core.build_dispatcher()),
        events,
        channel_gateway,
        channel_monitors,
        qr_login: Arc::new(QrLoginState::new()),
        registration,
        oauth_pending: Arc::new(RwLock::new(HashMap::new())),
        web_sessions: web_sessions.clone(),
    };

    // Phase 5: start the cron scheduler. The server (web host) enables it by
    // default in platform mode; standalone defaults off. `POINTER_SCHEDULER_ENABLED=0`
    // explicitly disables it; `=1` enables in standalone.
    let scheduler_enabled = env::var("POINTER_SCHEDULER_ENABLED")
        .map(|v| v != "0" && v.to_ascii_lowercase() != "false")
        .unwrap_or(!pointer_core::deployment_mode::is_standalone());
    if scheduler_enabled {
        let _scheduler = pointer_core::scheduler::Scheduler::start(
            pointer_core::scheduler::Scheduler::new(state.core.clone(), state.dispatcher.clone()),
        );
        log::info!("server: cron scheduler enabled (ticker started)");
    } else {
        log::info!("server: cron scheduler disabled by POINTER_SCHEDULER_ENABLED");
    }

    let app = Router::new()
        .route("/api/health", get(|| async { "ok" }))
        .route("/api/ready", get(api_ready))
        .route("/api/platform/session", get(get_platform_session))
        .route("/api/auth/login/start", post(start_platform_login))
        .route("/api/auth/local/login", post(local_auth::local_login))
        .route("/api/auth/oauth/callback", get(platform_oauth_callback))
        .route("/api/auth/logout", post(platform_logout))
        .route("/api/auth/refresh", post(refresh_platform_session))
        .route("/api/license/status", get(local_auth::license_status))
        .route("/api/license/reload", post(local_auth::license_reload))
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
        .route("/api/task-board/work-items", get(list_work_items))
        .route("/api/task-board/work-item-stats", get(work_item_stats))
        .route(
            "/api/computer/annotated-preview",
            get(preview_computer_annotated_screen),
        )
        .route(
            "/api/computer/round-screen-preview",
            get(preview_computer_round_screen),
        )
        .route("/api/computer/manual-snapshot", post(manual_computer_snapshot))
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
        .route(
            "/api/conversations/:conversation_id",
            delete(delete_conversation_handler),
        )
        .route("/api/conversations/meta", get(load_conversation_metas).put(save_conversation_meta))
        .route("/api/conversations/search", get(search_conversations_handler))
        .route(
            "/api/conversations/:conversation_id/messages",
            get(load_conversation_messages_handler),
        )
        .route(
            "/api/conversations/:conversation_id/messages/append",
            post(append_conversation_messages),
        )
        .route("/api/experiences/pinned", get(list_pinned_experiences))
        .route("/api/experiences/home", get(get_experience_home))
        .route("/api/experiences/search", get(search_experiences))
        .route("/api/experiences/:slug", get(get_experience_detail))
        .route("/api/chat/media-preview", get(preview_chat_media))
        .route("/api/chat/media-download", get(download_chat_media))
        .route("/api/chat/media-stream", get(stream_chat_media))
        .route("/api/chat/media-ref-preview", get(preview_media_ref))
        .route("/api/chat/save-attachment", post(save_chat_attachment))
        .route("/api/chat/upload-video-oss", post(upload_composer_video_oss))
        .route("/api/media/deps", get(check_media_deps))
        .route("/api/chat", post(send_chat))
        .route("/api/chat/:conversation_id/cancel", post(cancel_chat))
        .route(
            "/api/chat/:conversation_id/abort-terminal",
            post(abort_terminal_command),
        )
        .route("/api/chat/:conversation_id/stream", get(chat_stream))
        // Phase 4: unified callable / event-triggered HTTP surface.
        // `POST /api/runs` accepts a TriggerRequest and returns a RunHandle;
        // `GET /api/runs/:id/events` streams AgentEvents (SSE); cancel via
        // `POST /api/runs/:id/cancel`. Generic webhook ingress lives at
        // `POST /api/webhooks/:src` (Bearer auth).
        .route("/api/runs", post(create_run))
        .route("/api/runs/:run_id", get(get_run))
        .route("/api/runs/:run_id/events", get(run_events))
        .route("/api/runs/:run_id/cancel", post(cancel_run))
        .route("/api/dispatcher/queue", get(get_dispatcher_queue_snapshot))
        .route(
            "/api/webhooks/:src/upload",
            post(webhook_upload).layer(DefaultBodyLimit::max(
                pointer_core::webhook_attachment::MAX_WEBHOOK_UPLOAD_BYTES + 1024,
            )),
        )
        .route("/api/webhooks/:src", post(webhook_ingress))
        .route(
            "/api/webhooks/:src/runs/:run_id",
            get(get_webhook_run),
        )
        .route(
            "/api/webhooks/config",
            get(get_webhook_config).post(set_webhook_source_token),
        )
        .route(
            "/api/webhooks/config/legacy",
            axum::routing::delete(clear_webhook_legacy_token),
        )
        .route(
            "/api/webhooks/config/:src/token",
            get(reveal_webhook_source_token),
        )
        .route(
            "/api/webhooks/config/:src",
            axum::routing::patch(patch_webhook_source)
                .delete(clear_webhook_source_token),
        )
        // Phase 5: cron job management for the scheduler.
        .route(
            "/api/cron-jobs",
            get(list_cron_jobs).post(create_cron_job),
        )
        .route(
            "/api/cron-jobs/:job_id",
            axum::routing::patch(update_cron_job).delete(delete_cron_job),
        )
        .route("/api/tools/:tool_call_id/approve", post(approve_tool_call))
        .route("/api/terminal-input/:request_id/submit", post(submit_terminal_input))
        .route(
            "/api/terminal-input/:request_id/dismiss",
            post(dismiss_terminal_input),
        )
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
        );

    let static_dir = resolve_static_dir();
    let app = maybe_with_static_files(app, static_dir.clone())
        .layer(DefaultBodyLimit::max(20 * 1024 * 1024))
        .layer(CorsLayer::permissive())
        .layer(middleware::from_fn_with_state(
            web_sessions.clone(),
            web_session::web_session_middleware,
        ))
        .with_state(state);

    let addr: SocketAddr = std::env::var("POINTER_SERVER_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:8787".into())
        .parse()?;
    log::info!("pointer-server: bind address {addr} (POINTER_SERVER_ADDR)");
    if static_dir.is_some() {
        println!("Pointer web server listening on http://{addr} (API + static UI)");
    } else {
        println!(
            "Pointer web server listening on http://{addr} (API only; run npm run server:build for integrated UI)"
        );
    }
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn get_settings(State(state): State<ServerState>) -> Result<Json<WebEffectiveSettingsView>, ApiError> {
    require_platform_access(&state)?;
    Ok(Json(WebEffectiveSettingsView(state.core.effective_settings_view())))
}

async fn update_user_settings(
    State(state): State<ServerState>,
    Json(mut user): Json<UserSettings>,
) -> Result<Json<WebEffectiveSettingsView>, ApiError> {
    require_platform_access(&state)?;
    if user.theme.trim().is_empty() {
        user.theme = "system".into();
    }
    state.core.save_user_settings(&user)?;
    Ok(Json(WebEffectiveSettingsView(state.core.effective_settings_view())))
}

async fn update_platform_settings(
    State(_state): State<ServerState>,
    Json(_platform): Json<PlatformSettings>,
) -> Result<Json<WebEffectiveSettingsView>, ApiError> {
    Err(ApiError(anyhow::anyhow!(
        "web runtime: platform settings are read-only"
    )))
}

async fn update_settings(
    State(state): State<ServerState>,
    Json(mut settings): Json<ModelSettings>,
) -> Result<Json<WebEffectiveSettingsView>, ApiError> {
    require_platform_access(&state)?;
    let platform = state.core.platform_config.read().clone();
    pointer_core::models::preserve_platform_debug_settings_in_model(&mut settings, &platform);
    state
        .core
        .apply_session_platform_preferences(&settings)?;
    state.core.sync_dispatcher_concurrency(&state.dispatcher);
    Ok(Json(WebEffectiveSettingsView(state.core.effective_settings_view())))
}

async fn update_agent_settings(
    State(state): State<ServerState>,
    Json(mut settings): Json<ModelSettings>,
) -> Result<Json<WebEffectiveSettingsView>, ApiError> {
    require_platform_access(&state)?;
    let platform = state.core.platform_config.read().clone();
    pointer_core::models::preserve_platform_debug_settings_in_model(&mut settings, &platform);
    state
        .core
        .update_agent_settings(&settings)
        .map_err(ApiError)?;
    state.core.sync_dispatcher_concurrency(&state.dispatcher);
    Ok(Json(WebEffectiveSettingsView(state.core.effective_settings_view())))
}

#[derive(Deserialize)]
struct KeyPayload {
    #[serde(rename = "api_key")]
    _api_key: String,
}

async fn set_api_key(Json(_payload): Json<KeyPayload>) -> Result<StatusCode, ApiError> {
    Ok(StatusCode::NO_CONTENT)
}

async fn clear_api_key() -> Result<StatusCode, ApiError> {
    Ok(StatusCode::NO_CONTENT)
}

async fn test_connection(State(state): State<ServerState>) -> Result<Json<u128>, ApiError> {
    require_platform_access(&state)?;
    let settings = state.core.effective_settings();
    if settings.api_key.is_empty() {
        return Err(ApiError(anyhow::anyhow!("尚未配置 API Key")));
    }
    let api_key = settings.api_key.clone();
    let provider = OpenAIProvider::new(settings, api_key);
    Ok(Json(provider.test().await?))
}

async fn list_skills(State(state): State<ServerState>) -> Result<Json<Vec<SkillDef>>, ApiError> {
    require_platform_access(&state)?;
    Ok(Json(state.core.skills.list()))
}

async fn reload_skill_meta(
    State(state): State<ServerState>,
) -> Result<Json<Vec<SkillDef>>, ApiError> {
    require_platform_access(&state)?;
    state.core.skills.reload_meta()?;
    Ok(Json(state.core.skills.list()))
}

async fn import_skill_zip(
    State(state): State<ServerState>,
    body: axum::body::Bytes,
) -> Result<Json<SkillImportResult>, ApiError> {
    require_platform_access(&state)?;
    Ok(Json(state.core.skills.import_zip(&body)?))
}

async fn probe_external_skills(
    State(state): State<ServerState>,
) -> Result<Json<pointer_core::skills::external_probe::ExternalSkillsProbeResult>, ApiError> {
    require_platform_access(&state)?;
    Ok(Json(
        pointer_core::skills::external_probe::probe_external_skill_sources()?,
    ))
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
    require_platform_access(&state)?;
    let result = pointer_core::skills::external_probe::import_external_skills(&body.source_ids)?;
    state.core.skills.reload_meta()?;
    Ok(Json(result))
}

async fn dismiss_external_skills_prompt(
    State(state): State<ServerState>,
) -> Result<StatusCode, ApiError> {
    require_platform_access(&state)?;
    pointer_core::skills::external_probe::dismiss_external_skills_prompt()?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_tools(State(state): State<ServerState>) -> Result<Json<Vec<ToolDef>>, ApiError> {
    require_platform_access(&state)?;
    Ok(Json(state.core.tools.list_defs()))
}

async fn list_agents(State(state): State<ServerState>) -> Result<Json<Vec<AgentDef>>, ApiError> {
    require_platform_access(&state)?;
    Ok(Json(state.core.agents.list()))
}

fn resolve_work_items_store_key(
    core: &AppState,
    conversation_id: &str,
    task_id: Option<&str>,
    rehydrate: bool,
) -> String {
    use pointer_core::task_board::resolve_store_key_for_read;
    use pointer_core::task_board::work_item::try_rehydrate_work_items_if_empty;
    let parent_key = core
        .get_active_main_task_board_key(conversation_id)
        .unwrap_or_else(|| conversation_id.to_string());
    let store_key = resolve_store_key_for_read(
        core.task_board_store.as_ref(),
        core.task_board_store.work_items.as_ref(),
        conversation_id,
        task_id,
        &parent_key,
    );
    if rehydrate {
        if let Err(e) = try_rehydrate_work_items_if_empty(
            core.task_board_store.as_ref(),
            core.task_board_store.work_items.as_ref(),
            &store_key,
        ) {
            log::warn!("work_items: rehydrate failed store_key={store_key}: {e}");
        }
    }
    store_key
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
    require_platform_access(&state)?;
    use pointer_core::task_board::resolve_store_key_for_read;
    let parent_key = state
        .core
        .get_active_main_task_board_key(&q.conversation_id)
        .unwrap_or_else(|| q.conversation_id.clone());
    let store_key = resolve_store_key_for_read(
        state.core.task_board_store.as_ref(),
        state.core.task_board_store.work_items.as_ref(),
        &q.conversation_id,
        q.task_id.as_deref(),
        &parent_key,
    );
    Ok(Json(
        state
            .core
            .task_board_store
            .document(&store_key)
            .to_value(),
    ))
}

#[derive(Deserialize)]
struct WorkItemsQuery {
    #[serde(rename = "conversationId")]
    conversation_id: String,
    #[serde(default, rename = "taskId")]
    task_id: Option<String>,
    #[serde(default, rename = "batchId")]
    batch_id: Option<String>,
    #[serde(default)]
    offset: Option<u32>,
    #[serde(default)]
    limit: Option<u32>,
}

async fn list_work_items(
    Query(q): Query<WorkItemsQuery>,
    State(state): State<ServerState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_platform_access(&state)?;
    use pointer_core::task_board::work_item::list_work_items_json;
    let store_key = resolve_work_items_store_key(
        state.core.as_ref(),
        &q.conversation_id,
        q.task_id.as_deref(),
        true,
    );
    let batch = q.batch_id.as_deref().map(str::trim).filter(|s| !s.is_empty());
    Ok(Json(list_work_items_json(
        state.core.task_board_store.work_items.as_ref(),
        &store_key,
        batch,
        q.offset.unwrap_or(0),
        q.limit.unwrap_or(50),
    )))
}

async fn work_item_stats(
    Query(q): Query<WorkItemsQuery>,
    State(state): State<ServerState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_platform_access(&state)?;
    use pointer_core::task_board::work_item::work_item_stats_json;
    let store_key = resolve_work_items_store_key(
        state.core.as_ref(),
        &q.conversation_id,
        q.task_id.as_deref(),
        true,
    );
    let batch = q.batch_id.as_deref().map(str::trim).filter(|s| !s.is_empty());
    Ok(Json(work_item_stats_json(
        state.core.task_board_store.work_items.as_ref(),
        &store_key,
        batch,
    )))
}

/// Same as Tauri `preview_computer_annotated_screen`: last cached annotated PNG from a screen inject.
/// Requires `?conversationId=...` to select the session.
async fn preview_computer_annotated_screen(
    Query(q): Query<ConversationPreviewQuery>,
    State(state): State<ServerState>,
) -> Result<Json<ComputerAnnotatedPreview>, ApiError> {
    require_platform_access(&state)?;
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
    State(state): State<ServerState>,
    Query(q): Query<RoundScreenQuery>,
) -> Result<Json<ComputerAnnotatedPreview>, ApiError> {
    require_platform_access(&state)?;
    Ok(Json(
        capture_debug::read_computer_capture_preview(&q.rel_path).map_err(ApiError::from)?,
    ))
}

async fn manual_computer_snapshot(
    State(state): State<ServerState>,
) -> Result<Response, ApiError> {
    require_platform_access(&state)?;
    let jpeg =
        capture_debug::capture_manual_desktop_snapshot_jpeg().map_err(ApiError::from)?;
    let mut response = Response::new(jpeg.into());
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("image/jpeg"),
    );
    response
        .headers_mut()
        .insert(header::CONTENT_DISPOSITION, HeaderValue::from_static("inline"));
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}

async fn api_ready() -> Result<&'static str, StatusCode> {
    if WEB_DIST.get().is_some() {
        Ok("ok")
    } else {
        Err(StatusCode::SERVICE_UNAVAILABLE)
    }
}

#[derive(Deserialize)]
struct ChatMediaQuery {
    #[serde(rename = "storageRelPath")]
    storage_rel_path: String,
}

async fn preview_chat_media(
    State(state): State<ServerState>,
    Query(q): Query<ChatMediaQuery>,
) -> Result<Json<ChatMediaPreview>, ApiError> {
    require_platform_access(&state)?;
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
    State(state): State<ServerState>,
    Query(q): Query<MediaRefQuery>,
) -> Result<Json<ChatMediaPreview>, ApiError> {
    require_platform_access(&state)?;
    Ok(Json(
        pointer_core::media::read_media_ref_preview(&q.media_ref).map_err(ApiError::from)?,
    ))
}

fn attachment_content_disposition(file_name: &str, inline: bool) -> HeaderValue {
    let kind = if inline { "inline" } else { "attachment" };
    let safe: String = file_name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' { c } else { '_' })
        .collect();
    let fallback = if safe.is_empty() { "attachment".into() } else { safe };
    HeaderValue::from_str(&format!("{kind}; filename=\"{fallback}\""))
        .unwrap_or_else(|_| HeaderValue::from_static("attachment"))
}

async fn download_chat_media(
    State(state): State<ServerState>,
    Query(q): Query<ChatMediaQuery>,
) -> Result<Response, ApiError> {
    require_platform_access(&state)?;
    let (path, mime_type, file_name) =
        pointer_core::media::chat_media_file_meta(&q.storage_rel_path).map_err(ApiError::from)?;
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|e| ApiError(anyhow::anyhow!("read media: {e}")))?;
    let mut response = Response::new(bytes.into());
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(&mime_type).unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
    );
    response
        .headers_mut()
        .insert(header::CONTENT_DISPOSITION, attachment_content_disposition(&file_name, false));
    Ok(response)
}

async fn stream_chat_media(
    State(state): State<ServerState>,
    Query(q): Query<ChatMediaQuery>,
) -> Result<Response, ApiError> {
    require_platform_access(&state)?;
    let (path, mime_type, file_name) =
        pointer_core::media::chat_media_file_meta(&q.storage_rel_path).map_err(ApiError::from)?;
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|e| ApiError(anyhow::anyhow!("read media: {e}")))?;
    let mut response = Response::new(bytes.into());
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(&mime_type).unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
    );
    response
        .headers_mut()
        .insert(header::CONTENT_DISPOSITION, attachment_content_disposition(&file_name, true));
    Ok(response)
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
    State(state): State<ServerState>,
    Json(payload): Json<SaveChatAttachmentPayload>,
) -> Result<Json<SaveChatAttachmentResponse>, ApiError> {
    require_platform_access(&state)?;
    let uid = state
        .core
        .active_platform_auth()
        .platform_user_id()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| ApiError(anyhow::anyhow!("platform_login_required")))?;
    state
        .core
        .session_index
        .ensure_session_user_id(&payload.conversation_id, &uid)
        .map_err(ApiError::from)?;
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

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct UploadVideoOssResponse {
    remote_url: String,
    oss_object_key: String,
    storage_rel_path: Option<String>,
}

async fn upload_composer_video_oss(
    State(state): State<ServerState>,
    mut multipart: Multipart,
) -> Result<Json<UploadVideoOssResponse>, ApiError> {
    require_platform_access(&state)?;
    let mut conversation_id = String::new();
    let mut attachment_id = String::new();
    let mut file_name = String::new();
    let mut mime_type = String::from("video/mp4");
    let mut file_bytes: Option<Vec<u8>> = None;
    let mut compress = false;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| ApiError(anyhow::anyhow!("multipart: {e}")))?
    {
        match field.name() {
            Some("conversationId") => {
                conversation_id = field
                    .text()
                    .await
                    .map_err(|e| ApiError(anyhow::anyhow!("conversationId: {e}")))?
                    .trim()
                    .to_string();
            }
            Some("attachmentId") => {
                attachment_id = field
                    .text()
                    .await
                    .map_err(|e| ApiError(anyhow::anyhow!("attachmentId: {e}")))?
                    .trim()
                    .to_string();
            }
            Some("fileName") => {
                file_name = field
                    .text()
                    .await
                    .map_err(|e| ApiError(anyhow::anyhow!("fileName: {e}")))?
                    .trim()
                    .to_string();
            }
            Some("mimeType") => {
                let t = field
                    .text()
                    .await
                    .map_err(|e| ApiError(anyhow::anyhow!("mimeType: {e}")))?
                    .trim()
                    .to_string();
                if !t.is_empty() {
                    mime_type = t;
                }
            }
            Some("compress") => {
                let v = field
                    .text()
                    .await
                    .map_err(|e| ApiError(anyhow::anyhow!("compress: {e}")))?
                    .trim()
                    .to_ascii_lowercase();
                compress = matches!(v.as_str(), "1" | "true" | "yes");
            }
            Some("file") => {
                file_bytes = Some(
                    field
                        .bytes()
                        .await
                        .map_err(|e| ApiError(anyhow::anyhow!("file bytes: {e}")))?
                        .to_vec(),
                );
            }
            _ => {}
        }
    }

    if attachment_id.is_empty() || file_name.is_empty() {
        return Err(ApiError(anyhow::anyhow!("attachmentId and fileName required")));
    }
    let bytes = file_bytes.ok_or_else(|| ApiError(anyhow::anyhow!("file field required")))?;
    let settings = state.core.effective_settings();
    let last_pct = std::sync::Arc::new(std::sync::Mutex::new(0u32));
    let attachment_id_log = attachment_id.clone();
    let on_progress: std::sync::Arc<dyn Fn(u64, u64) + Send + Sync> =
        std::sync::Arc::new(move |loaded, total| {
            let pct = if total == 0 {
                0
            } else {
                ((loaded.saturating_mul(100)) / total).min(100) as u32
            };
            let mut last = last_pct.lock().expect("progress mutex");
            if pct != *last {
                *last = pct;
                log::info!(
                    "upload-video-oss {attachment_id_log}: {pct}% ({loaded}/{total})"
                );
            }
        });
    let result = pointer_core::media::upload_composer_video_bytes(
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
    .map_err(ApiError::from)?;
    Ok(Json(UploadVideoOssResponse {
        remote_url: result.remote_url,
        oss_object_key: result.object_key,
        storage_rel_path: result.storage_rel_path,
    }))
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

/// pointer-server web: conversation history requires a logged-in browser session.
fn require_platform_login(state: &ServerState) -> Result<(), ApiError> {
    if state.core.active_platform_auth().session_view().logged_in {
        return Ok(());
    }
    if pointer_core::deployment_mode::is_standalone() {
        return Err(ApiError(anyhow::anyhow!("local_login_required")));
    }
    Err(ApiError(anyhow::anyhow!("platform_login_required")))
}

fn require_allowed_platform_user(state: &ServerState) -> Result<(), ApiError> {
    if pointer_core::deployment_mode::is_standalone()
        && pointer_core::web_request_auth::is_local_scoped_session()
    {
        return Ok(());
    }
    if !pointer_core::server_access::access_restriction_enabled() {
        return Ok(());
    }
    let user_id = state
        .core
        .active_platform_auth()
        .platform_user_id()
        .ok_or_else(|| ApiError(anyhow::anyhow!("platform_login_required")))?;
    pointer_core::server_access::ensure_user_allowed(&user_id).map_err(|_| {
        log::warn!("server_access: rejected API request for user_id={user_id}");
        ApiError(anyhow::anyhow!("server_access_denied"))
    })
}

fn require_platform_access(state: &ServerState) -> Result<(), ApiError> {
    require_platform_login(state)?;
    require_allowed_platform_user(state)
}

pub(crate) fn require_platform_access_status(state: &ServerState) -> Result<(), StatusCode> {
    require_platform_access(state).map_err(|e| {
        let msg = e.0.to_string();
        if msg.contains("server_access_denied") {
            StatusCode::FORBIDDEN
        } else {
            StatusCode::UNAUTHORIZED
        }
    })
}

async fn load_conversations(
    State(state): State<ServerState>,
) -> Result<Json<Vec<Conversation>>, ApiError> {
    require_platform_access(&state)?;
    Ok(Json(storage::load_conversations()?))
}

#[derive(Deserialize)]
struct ConversationMetasQuery {
    /// Cursor `updated_at_ms` (exclusive). Omit for the first page.
    cursor_updated_at: Option<i64>,
    /// Cursor `id` (exclusive, paired with `cursor_updated_at`).
    cursor_id: Option<String>,
    /// Page size (1..=500, default 50).
    limit: Option<i64>,
}

/// `GET /api/conversations/meta` — cursor-paginated meta-only list (no messages).
/// Sort order: `(updated_at_ms DESC, id DESC)`. Pass `cursor_updated_at` +
/// `cursor_id` from the last row of the previous page to fetch the next.
async fn load_conversation_metas(
    State(state): State<ServerState>,
    Query(q): Query<ConversationMetasQuery>,
) -> Result<Json<Vec<pointer_core::models::ConversationMeta>>, ApiError> {
    require_platform_access(&state)?;
    let limit = q.limit.unwrap_or(50);
    let cursor = match (q.cursor_updated_at, q.cursor_id) {
        (Some(ts), Some(id)) => Some((ts, id)),
        (Some(_), None) | (None, Some(_)) => {
            return Err(ApiError::from(anyhow::anyhow!(
                "cursor_updated_at and cursor_id must both be set or both be omitted"
            )))
        }
        (None, None) => None,
    };
    let cursor_dbg = match &cursor {
        Some((ts, id)) => format!("({}, {})", ts, id),
        None => "none".to_string(),
    };
    let metas = storage::load_conversation_metas(cursor, limit)?;
    log::info!(
        "server: load_conversation_metas cursor={} limit={} returned {} rows",
        cursor_dbg,
        limit,
        metas.len()
    );
    Ok(Json(metas))
}

#[derive(serde::Deserialize)]
struct ConversationSearchQuery {
    q: String,
    limit: Option<i64>,
}

/// `GET /api/conversations/search?q=...` — FTS sidebar search (messages + title/preview).
async fn search_conversations_handler(
    State(state): State<ServerState>,
    Query(q): Query<ConversationSearchQuery>,
) -> Result<Json<Vec<pointer_core::models::ConversationSearchHit>>, ApiError> {
    require_platform_access(&state)?;
    let limit = q.limit.unwrap_or(50);
    let hits = storage::search_conversations(&q.q, limit)?;
    log::info!(
        "server: search_conversations q={:?} limit={} returned {} rows",
        q.q.trim(),
        limit,
        hits.len()
    );
    Ok(Json(hits))
}

/// `GET /api/conversations/:id/messages` — full message list for one
/// conversation, ordered by `position ASC`. Replaces the legacy web pattern of
/// re-fetching every conversation and filtering client-side.
async fn load_conversation_messages_handler(
    State(state): State<ServerState>,
    Path(conversation_id): Path<String>,
) -> Result<Json<Vec<pointer_core::models::ChatMessage>>, ApiError> {
    require_platform_access(&state)?;
    let messages = storage::load_conversation_messages(&conversation_id)?;
    log::info!(
        "server: load_conversation_messages conversation_id={} returned {} rows",
        conversation_id,
        messages.len()
    );
    Ok(Json(messages))
}

async fn save_conversations(
    State(state): State<ServerState>,
    Json(conversations): Json<Vec<Conversation>>,
) -> Result<StatusCode, ApiError> {
    require_platform_access(&state)?;
    storage::save_conversations(&conversations)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn save_conversation_meta(
    State(state): State<ServerState>,
    Json(metas): Json<Vec<pointer_core::models::ConversationMeta>>,
) -> Result<StatusCode, ApiError> {
    require_platform_access(&state)?;
    let platform_user_id = state.core.active_platform_auth().platform_user_id();
    storage::save_conversation_meta_with_platform_user(&metas, platform_user_id.as_deref())?;
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_conversation_handler(
    State(state): State<ServerState>,
    Path(conversation_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    require_platform_access(&state)?;
    storage::delete_conversation(&conversation_id)?;
    log::info!("server: deleted conversation id={conversation_id}");
    Ok(StatusCode::NO_CONTENT)
}

async fn append_conversation_messages(
    State(state): State<ServerState>,
    axum::extract::Path(conversation_id): axum::extract::Path<String>,
    Json(messages): Json<Vec<pointer_core::models::ChatMessage>>,
) -> Result<Json<u32>, ApiError> {
    require_platform_access(&state)?;
    let written = storage::append_conversation_messages(&conversation_id, &messages)?;
    Ok(Json(written))
}

async fn send_chat(
    State(state): State<ServerState>,
    headers: HeaderMap,
    Json(payload): Json<SendChatPayload>,
) -> Result<StatusCode, ApiError> {
    require_platform_access(&state)?;
    let web_session_auth = web_session::lookup_session_auth(&state.web_sessions, &headers)
        .or_else(|| {
            pointer_core::web_request_auth::capture_web_session_auth(&state.core.platform_auth)
        });
    if web_session_auth.is_none() {
        log::warn!(
            "send_chat: no browser session auth (conversation_id={})",
            payload.conversation_id
        );
    }
    let req = TriggerRequest {
        run_id: None,
        idempotency_key: None,
        conversation_id: Some(payload.conversation_id),
        trigger_source: TriggerSource::HttpRuns,
        trigger_meta: TriggerMeta::empty(),
        lane: None,
        messages: payload.messages,
        enabled_skill_ids: payload.enabled_skill_ids,
        agent_mode: payload.agent_mode,
        lead_agent_id: payload.lead_agent_id,
        tool_rounds_used_single_start: payload.tool_rounds_used,
        tool_rounds_used_supervisor_start: payload.tool_rounds_used_supervisor,
        workspace_root: payload.workspace_root,
        workspace_inherit_disabled: payload.workspace_inherit_disabled,
        deliver: DeliverTarget::None,
        web_session_auth,
    };
    if let Err(e) = state.dispatcher.dispatch(req).await {
        log::error!("send_chat: dispatch failed: {e:#}");
    }
    Ok(StatusCode::ACCEPTED)
}

// ---- Phase 4: HTTP Runs API + generic webhook ----

/// `POST /api/runs` — accept a unified run request. The caller supplies the
/// full message history (same contract as `POST /api/chat`); `trigger_source`
/// is forced to `HttpRuns` regardless of the request body. Returns a
/// `RunHandle` (run id + accept status). Subscribe to
/// `GET /api/runs/:id/events` for progress.
async fn create_run(
    State(state): State<ServerState>,
    headers: HeaderMap,
    Json(mut body): Json<TriggerRequest>,
) -> Result<(StatusCode, Json<RunHandle>), ApiError> {
    require_platform_access(&state)?;
    body.trigger_source = TriggerSource::HttpRuns;
    body.trigger_meta.webhook_source = None;
    body.deliver = DeliverTarget::None;
    body.web_session_auth = web_session::lookup_session_auth(&state.web_sessions, &headers)
        .or_else(|| {
            pointer_core::web_request_auth::capture_web_session_auth(&state.core.platform_auth)
        });
    let handle = state.dispatcher.dispatch(body).await.map_err(ApiError::from)?;
    log::info!(
        "runs-api: accepted run_id={} conv={} status={:?}",
        handle.run_id,
        handle
            .reused_run_id
            .as_deref()
            .unwrap_or("new"),
        handle.status
    );
    Ok((StatusCode::ACCEPTED, Json(handle)))
}

/// `GET /api/dispatcher/queue` — lane queue snapshot for settings UI.
async fn get_dispatcher_queue_snapshot(
    State(state): State<ServerState>,
) -> Result<Json<pointer_core::dispatcher::RunQueueSnapshot>, ApiError> {
    require_platform_access(&state)?;
    Ok(Json(state.dispatcher.queue_snapshot()))
}

/// Read-only view of a persisted run record (mirrors `RunRecord` minus the
/// raw JSON blobs). Returned by `GET /api/runs/:id`.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct RunView {
    run_id: String,
    conversation_id: String,
    trigger_source: String,
    status: String,
    created_at_ms: i64,
    started_at_ms: Option<i64>,
    finished_at_ms: Option<i64>,
    error: Option<String>,
}

impl RunView {
    fn from_record(rec: &pointer_core::conversation_store::runs::RunRecord) -> Self {
        Self {
            run_id: rec.run_id.clone(),
            conversation_id: rec.conversation_id.clone(),
            trigger_source: rec.trigger_source.clone(),
            status: rec.status.clone(),
            created_at_ms: rec.created_at_ms,
            started_at_ms: rec.started_at_ms,
            finished_at_ms: rec.finished_at_ms,
            error: rec.error.clone(),
        }
    }
}

/// `GET /api/runs/:id` — return the persisted status snapshot, or 404.
async fn get_run(
    State(state): State<ServerState>,
    Path(run_id): Path<String>,
) -> Result<axum::response::Response, ApiError> {
    require_platform_access(&state)?;
    match state.dispatcher.run_status(&run_id) {
        Some(rec) => Ok(Json(RunView::from_record(&rec)).into_response()),
        None => Ok(status_text(
            StatusCode::NOT_FOUND,
            format!("run not found: {run_id}"),
        )),
    }
}

/// `GET /api/runs/:id/events` — SSE stream of `AgentEvent`s for one run.
///
/// Race-free terminal handling: subscribe to the bus BEFORE reading the runs
/// table. If the run already reached a terminal state before this SSE opened,
/// synthesize a terminal event from the table and close immediately.
/// Otherwise stream live events until a terminal event arrives.
async fn run_events(
    State(state): State<ServerState>,
    Path(run_id): Path<String>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    require_platform_access(&state)?;
    let dispatcher = state.dispatcher.clone();
    let run_id_for_stream = run_id.clone();

    let stream = async_stream::stream! {
        // Subscribe first so we never miss a terminal emitted after this point.
        let mut rx = dispatcher.subscribe_events();

        // Then snapshot the persisted state. If terminal, synthesize + close.
        if let Some(rec) = dispatcher.run_status(&run_id) {
            if pointer_core::conversation_store::runs::is_terminal_status_str(&rec.status) {
                let synth = terminal_agent_event_from_record(&rec);
                if let Some(ev) = synth {
                    if let Ok(s) = serde_json::to_string(&*ev) {
                        yield Ok(Event::default().data(s));
                    }
                }
                return;
            }
        }

        // Live stream: filter by run_id, emit until terminal.
        while let Ok(ev) = rx.recv().await {
            if ev.run_id() != run_id_for_stream {
                continue;
            }
            let is_terminal = matches!(
                *ev,
                pointer_core::agent_events::AgentEvent::RunFinished { .. }
                    | pointer_core::agent_events::AgentEvent::RunFailed { .. }
                    | pointer_core::agent_events::AgentEvent::RunCancelled { .. }
            );
            if let Ok(s) = serde_json::to_string(&*ev) {
                yield Ok(Event::default().data(s));
            }
            if is_terminal {
                return;
            }
        }
    };

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

/// Build a synthetic terminal `AgentEvent` from a persisted terminal
/// `RunRecord`, so late SSE subscribers still observe a terminal frame.
fn terminal_agent_event_from_record(
    rec: &pointer_core::conversation_store::runs::RunRecord,
) -> Option<std::sync::Arc<pointer_core::agent_events::AgentEvent>> {
    let ts = rec.finished_at_ms.unwrap_or(0) as u64;
    let conv = rec.conversation_id.clone();
    let run_id = rec.run_id.clone();
    let ev = match rec.status.as_str() {
        "finished" => pointer_core::agent_events::AgentEvent::RunFinished {
            run_id,
            conversation_id: conv,
            seq: 0,
            ts,
        },
        "failed" => pointer_core::agent_events::AgentEvent::RunFailed {
            run_id,
            conversation_id: conv,
            error: rec.error.clone().unwrap_or_default(),
            seq: 0,
            ts,
        },
        "cancelled" => pointer_core::agent_events::AgentEvent::RunCancelled {
            run_id,
            conversation_id: conv,
            seq: 0,
            ts,
        },
        _ => return None,
    };
    Some(std::sync::Arc::new(ev))
}

/// `POST /api/runs/:id/cancel` — cancel a queued or running run. No-op (204)
/// if the run is unknown or already terminal.
async fn cancel_run(
    State(state): State<ServerState>,
    Path(run_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    require_platform_access(&state)?;
    state.dispatcher.cancel(&run_id);
    Ok(StatusCode::NO_CONTENT)
}

/// Blocking webhook response (OpenClaw-aligned `{ ok, runId, text? }`).
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct WebhookBlockingResponse {
    ok: bool,
    run_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    conversation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

async fn finish_webhook_blocking(
    state: &ServerState,
    run_id: String,
    conversation_id: String,
    timeout_seconds: Option<u64>,
) -> Result<axum::response::Response, ApiError> {
    let timeout_secs = timeout_seconds.unwrap_or(120).clamp(1, 600);
    let wait_fut = state.dispatcher.wait(&run_id);
    let outcome = match tokio::time::timeout(Duration::from_secs(timeout_secs), wait_fut).await {
        Ok(Ok(o)) => o,
        Ok(Err(e)) => {
            log::warn!("webhook blocking: wait failed run_id={run_id}: {e:#}");
            return Ok((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(WebhookBlockingResponse {
                    ok: false,
                    run_id,
                    conversation_id: Some(conversation_id),
                    text: None,
                    error: Some(format!("wait failed: {e:#}")),
                }),
            )
                .into_response());
        }
        Err(_) => {
            log::warn!(
                "webhook blocking: timed out run_id={run_id} after {timeout_secs}s"
            );
            return Ok((
                StatusCode::GATEWAY_TIMEOUT,
                Json(WebhookBlockingResponse {
                    ok: false,
                    run_id,
                    conversation_id: Some(conversation_id),
                    text: None,
                    error: Some(format!("agent run timed out after {timeout_secs}s")),
                }),
            )
                .into_response());
        }
    };

    match outcome {
        RunOutcome::Finished {
            conversation_id: conv,
            ..
        } => {
            let text = pointer_core::webhook_result::last_assistant_text(
                &state.core.session_index,
                &conv,
            )
            .map_err(ApiError::from)?;
            log::info!(
                "webhook blocking: finished run_id={run_id} conv={conv} text_len={}",
                text.as_ref().map(|t| t.len()).unwrap_or(0)
            );
            Ok((
                StatusCode::OK,
                Json(WebhookBlockingResponse {
                    ok: true,
                    run_id,
                    conversation_id: Some(conv),
                    text,
                    error: None,
                }),
            )
                .into_response())
        }
        RunOutcome::Failed {
            conversation_id: conv,
            error,
            ..
        } => {
            log::warn!("webhook blocking: failed run_id={run_id}: {error}");
            Ok((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(WebhookBlockingResponse {
                    ok: false,
                    run_id,
                    conversation_id: Some(conv),
                    text: None,
                    error: Some(error),
                }),
            )
                .into_response())
        }
        RunOutcome::Cancelled {
            conversation_id: conv,
            ..
        } => {
            log::warn!("webhook blocking: cancelled run_id={run_id}");
            Ok((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(WebhookBlockingResponse {
                    ok: false,
                    run_id,
                    conversation_id: Some(conv),
                    text: None,
                    error: Some("run cancelled".into()),
                }),
            )
                .into_response())
        }
    }
}

/// Map webhook auth failures to HTTP status + message.
fn webhook_auth_status(err: pointer_core::webhook_config::WebhookIngressAuthError) -> (StatusCode, String) {
    use pointer_core::webhook_config::WebhookIngressAuthError;
    match err {
        WebhookIngressAuthError::InvalidSrc(msg) => (StatusCode::BAD_REQUEST, msg),
        WebhookIngressAuthError::NotConfigured => (
            StatusCode::UNAUTHORIZED,
            err.to_string(),
        ),
        WebhookIngressAuthError::Unauthorized => (StatusCode::UNAUTHORIZED, err.to_string()),
    }
}

/// `POST /api/webhooks/:src/upload` — multipart file upload for webhook attachments.
/// Auth matches [`webhook_ingress`]. Returns attachment metadata to reference from
/// a subsequent `POST /api/webhooks/:src` JSON body (`attachments[].storageRelPath`).
async fn webhook_upload(
    State(state): State<ServerState>,
    Path(src): Path<String>,
    headers: axum::http::HeaderMap,
    mut multipart: Multipart,
) -> Result<axum::response::Response, ApiError> {
    let store = pointer_core::webhook_config::WebhookTokenStore::new(&state.core.session_index);
    let normalized_src = match pointer_core::webhook_config::authorize_webhook_ingress(
        &store,
        &src,
        &headers,
    ) {
        Ok(s) => s,
        Err(e) => {
            let (status, msg) = webhook_auth_status(e);
            return Ok(status_text(status, msg));
        }
    };

    let mut conversation_id: Option<String> = None;
    let mut file_name = String::new();
    let mut mime_type: Option<String> = None;
    let mut file_bytes: Option<Vec<u8>> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| ApiError(anyhow::anyhow!("multipart: {e}")))?
    {
        match field.name() {
            Some("conversationId") => {
                conversation_id = Some(
                    field
                        .text()
                        .await
                        .map_err(|e| ApiError(anyhow::anyhow!("conversationId: {e}")))?
                        .trim()
                        .to_string(),
                );
            }
            Some("fileName") => {
                file_name = field
                    .text()
                    .await
                    .map_err(|e| ApiError(anyhow::anyhow!("fileName: {e}")))?
                    .trim()
                    .to_string();
            }
            Some("mimeType") => {
                let v = field
                    .text()
                    .await
                    .map_err(|e| ApiError(anyhow::anyhow!("mimeType: {e}")))?
                    .trim()
                    .to_string();
                if !v.is_empty() {
                    mime_type = Some(v);
                }
            }
            Some("file") => {
                if file_name.is_empty() {
                    if let Some(name) = field.file_name().map(str::to_string) {
                        file_name = name;
                    }
                }
                let bytes = field
                    .bytes()
                    .await
                    .map_err(|e| ApiError(anyhow::anyhow!("file bytes: {e}")))?;
                file_bytes = Some(bytes.to_vec());
            }
            _ => {}
        }
    }

    let bytes = file_bytes.ok_or_else(|| {
        ApiError(anyhow::anyhow!("multipart field `file` required"))
    })?;
    if file_name.is_empty() {
        return Ok(status_text(
            StatusCode::UNPROCESSABLE_ENTITY,
            "fileName required (field or multipart filename)",
        ));
    }

    let conversation_id = match conversation_id.filter(|s| !s.trim().is_empty()) {
        Some(id) => id,
        None => state
            .core
            .session_index
            .resolve_webhook_ingress_session(&normalized_src, None)
            .map_err(ApiError::from)?,
    };

    if let Err(e) = state
        .core
        .session_index
        .adopt_webhook_upload_session(&normalized_src, &conversation_id)
    {
        log::warn!(
            "webhook upload: adopt session failed src={normalized_src} conv={conversation_id}: {e:#}"
        );
    }

    let saved = match pointer_core::webhook_attachment::save_webhook_upload(
        &conversation_id,
        &file_name,
        mime_type.as_deref(),
        &bytes,
    ) {
        Ok(v) => v,
        Err(e) => {
            let msg = e.to_string();
            let status = if msg.contains("too large") {
                StatusCode::PAYLOAD_TOO_LARGE
            } else {
                StatusCode::UNPROCESSABLE_ENTITY
            };
            log::warn!("webhook upload rejected (src={normalized_src}): {msg}");
            return Ok(status_text(status, msg));
        }
    };

    Ok(Json(serde_json::json!({
        "conversationId": conversation_id,
        "attachmentId": saved.attachment_id,
        "storageRelPath": saved.storage_rel_path,
        "kind": saved.kind,
        "mimeType": saved.mime_type,
        "fileName": saved.file_name,
        "sizeBytes": saved.size_bytes,
    }))
    .into_response())
}

/// `POST /api/webhooks/:src` — generic authenticated webhook ingress. The
/// `:src` path segment labels the webhook source (recorded in trigger_meta).
/// Auth: `Authorization: Bearer <token>` or `X-Pointer-Token: <token>`.
/// The token must match the one configured for this `:src` (or the legacy
/// global token / env fallback when no per-source token exists).
/// Each source may optionally configure a custom auth header name instead.
pub(crate) fn sync_automation_web_session(state: &ServerState) {
    let auth = state.web_sessions.any_session_auth();
    if auth.is_some() {
        log::info!("server: automation web session available for webhook/cron");
    }
    state.core.set_automation_web_session(auth);
}

async fn webhook_ingress(
    State(state): State<ServerState>,
    Path(src): Path<String>,
    headers: axum::http::HeaderMap,
    body: Bytes,
) -> Result<axum::response::Response, ApiError> {
    let store = pointer_core::webhook_config::WebhookTokenStore::new(&state.core.session_index);
    let normalized_src = match pointer_core::webhook_config::authorize_webhook_ingress(
        &store,
        &src,
        &headers,
    ) {
        Ok(s) => s,
        Err(e) => {
            let (status, msg) = webhook_auth_status(e);
            return Ok(status_text(status, msg));
        }
    };

    let parsed = match pointer_core::webhook_ingress::parse_webhook_body(&body, &normalized_src) {
        Ok(p) => p,
        Err(pointer_core::webhook_ingress::WebhookParseError::PayloadTooLarge { .. }) => {
            return Ok(status_text(
                StatusCode::PAYLOAD_TOO_LARGE,
                "webhook payload too large",
            ));
        }
        Err(pointer_core::webhook_ingress::WebhookParseError::InvalidJson(msg)) => {
            return Ok(status_text(StatusCode::BAD_REQUEST, msg));
        }
        Err(e @ pointer_core::webhook_ingress::WebhookParseError::EmptyBody)
        | Err(e @ pointer_core::webhook_ingress::WebhookParseError::MessageBuild(_)) => {
            log::warn!("webhook ingress rejected: bad body (src={normalized_src}): {e}");
            return Ok(status_text(StatusCode::UNPROCESSABLE_ENTITY, e.to_string()));
        }
    };
    if parsed.used_raw_body_fallback {
        log::info!(
            "webhook ingress: raw body fallback src={normalized_src} bytes={}",
            body.len()
        );
    }

    let body = parsed.body;
    let delivery_id = pointer_core::webhook_config::extract_delivery_id(
        &headers,
        body.idempotency_key.as_deref(),
    );
    let conversation_id = if let Some(ref explicit) = body.conversation_id {
        explicit.clone()
    } else {
        state
            .core
            .session_index
            .resolve_webhook_ingress_session(
                &normalized_src,
                Some(delivery_id.as_str()),
            )
            .map_err(ApiError::from)?
    };
    let conversation_id = pointer_core::webhook_ingress::reconcile_webhook_conversation_id(
        &normalized_src,
        &conversation_id,
        parsed.inbound.attachments.as_deref(),
    );

    let messages = match pointer_core::webhook_ingress::build_webhook_dispatch_messages(
        &state.core.session_index,
        &conversation_id,
        &parsed.inbound,
    ) {
        Ok(m) if m.is_empty() => {
            return Ok(status_text(
                StatusCode::UNPROCESSABLE_ENTITY,
                "webhook body must contain `text`, `message`, `messages`, or `attachments`",
            ));
        }
        Ok(m) => m,
        Err(e) => {
            log::warn!("webhook ingress rejected: bad body (src={normalized_src}): {e:#}");
            return Ok(status_text(StatusCode::UNPROCESSABLE_ENTITY, e.to_string()));
        }
    };

    let blocking = body.blocking;
    let timeout_seconds = body.timeout_seconds;

    // Surface the inbound user turn in open cron/webhook views (same as cron scheduler / IM).
    if let Some(user_msg) =
        pointer_core::webhook_ingress::last_inbound_user_message(&messages)
    {
        pointer_core::stream_broadcast::broadcast_stream(&StreamEvent::InjectedUserMessage {
            conversation_id: conversation_id.clone(),
            message_id: user_msg.id.clone(),
            content: user_msg.content.clone(),
            attachments: user_msg.attachments.clone(),
        });
    }

    let req = TriggerRequest {
        run_id: None,
        idempotency_key: body.idempotency_key,
        conversation_id: Some(conversation_id.clone()),
        trigger_source: TriggerSource::Webhook,
        trigger_meta: TriggerMeta {
            webhook_source: Some(normalized_src.clone()),
            ..TriggerMeta::empty()
        },
        lane: None,
        messages,
        enabled_skill_ids: body.enabled_skill_ids,
        agent_mode: body.agent_mode,
        lead_agent_id: body.lead_agent_id,
        tool_rounds_used_single_start: 0,
        tool_rounds_used_supervisor_start: 0,
        workspace_root: body.workspace_root,
        workspace_inherit_disabled: None,
        deliver: DeliverTarget::None,
        web_session_auth: {
            sync_automation_web_session(&state);
            state.core.automation_execution_auth()
        },
    };

    let handle = state.dispatcher.dispatch(req).await.map_err(ApiError::from)?;
    if blocking {
        return finish_webhook_blocking(
            &state,
            handle.run_id.clone(),
            conversation_id,
            timeout_seconds,
        )
        .await;
    }
    log::info!(
        "webhook ingress: src={} accepted run_id={} conv={}",
        normalized_src,
        handle.run_id,
        conversation_id
    );
    Ok((StatusCode::ACCEPTED, Json(handle)).into_response())
}

// ---- Phase 5: cron job CRUD ----

/// `GET /api/cron-jobs` — list all cron jobs (enabled and disabled).
async fn list_cron_jobs(
    State(state): State<ServerState>,
) -> Result<Json<Vec<pointer_core::conversation_store::cron_jobs::CronJobView>>, ApiError> {
    require_platform_access(&state)?;
    let rows = state
        .core
        .session_index
        .cron_jobs_list_all()
        .map_err(ApiError::from)?;
    Ok(Json(
        rows.iter()
            .map(pointer_core::conversation_store::cron_jobs::CronJobView::from_record)
            .collect(),
    ))
}

#[derive(Deserialize)]
struct CreateCronJobBody {
    id: String,
    label: String,
    #[serde(rename = "cronExpr")]
    cron_expr: String,
    /// Ignored: each cron job owns a dedicated `cron:{id}` session. Retained on
    /// the wire for backward compatibility with older frontends.
    #[serde(default, rename = "conversationId")]
    conversation_id: String,
    #[serde(rename = "promptText")]
    prompt_text: String,
    #[serde(default, rename = "agentMode")]
    agent_mode: Option<String>,
    #[serde(default, rename = "leadAgentId")]
    lead_agent_id: Option<String>,
    #[serde(default = "default_true")]
    enabled: bool,
}

fn default_true() -> bool {
    true
}

/// `POST /api/cron-jobs` — create a new scheduled job. The cron expression is
/// validated up front (a parse failure rejects the request with 400).
async fn create_cron_job(
    State(state): State<ServerState>,
    Json(body): Json<CreateCronJobBody>,
) -> Result<(StatusCode, Json<pointer_core::conversation_store::cron_jobs::CronJobView>), ApiError> {
    require_platform_access(&state)?;
    // Validate the cron expression before persisting.
    if pointer_core::conversation_store::cron_jobs::next_run_ms(&body.cron_expr, &chrono::Local::now())
        .is_none()
    {
        return Err(ApiError(anyhow::anyhow!(
            "invalid cron expression: {}",
            body.cron_expr
        )));
    }
    let new = pointer_core::conversation_store::cron_jobs::NewCronJob {
        id: &body.id,
        label: &body.label,
        cron_expr: &body.cron_expr,
        conversation_id: &body.conversation_id,
        prompt_text: &body.prompt_text,
        agent_mode: body.agent_mode.as_deref(),
        lead_agent_id: body.lead_agent_id.as_deref(),
        enabled: body.enabled,
    };
    let inserted = state
        .core
        .session_index
        .cron_jobs_insert(&new)
        .map_err(ApiError::from)?;
    if !inserted {
        return Err(ApiError(anyhow::anyhow!(
            "cron job already exists: {}",
            body.id
        )));
    }
    let rec = state
        .core
        .session_index
        .cron_jobs_get(&body.id)
        .map_err(ApiError::from)?
        .ok_or_else(|| ApiError(anyhow::anyhow!("cron job vanished after insert: {}", body.id)))?;
    log::info!(
        "cron-jobs: created id={} label={} expr={}",
        body.id,
        body.label,
        body.cron_expr
    );
    Ok((
        StatusCode::CREATED,
        Json(pointer_core::conversation_store::cron_jobs::CronJobView::from_record(&rec)),
    ))
}

#[derive(Deserialize)]
struct UpdateCronJobBody {
    /// Toggles the job enabled flag. Other fields are immutable via this
    /// endpoint (edit by delete + recreate).
    enabled: Option<bool>,
}

/// `PATCH /api/cron-jobs/:id` — toggle enable/disable.
async fn update_cron_job(
    State(state): State<ServerState>,
    Path(job_id): Path<String>,
    Json(body): Json<UpdateCronJobBody>,
) -> Result<axum::response::Response, ApiError> {
    require_platform_access(&state)?;
    if let Some(enabled) = body.enabled {
        let ok = state
            .core
            .session_index
            .cron_jobs_set_enabled(&job_id, enabled)
            .map_err(ApiError::from)?;
        if !ok {
            return Ok(status_text(
                StatusCode::NOT_FOUND,
                format!("cron job not found: {job_id}"),
            ));
        }
    }
    match state.core.session_index.cron_jobs_get(&job_id).map_err(ApiError::from)? {
        Some(rec) => Ok(Json(pointer_core::conversation_store::cron_jobs::CronJobView::from_record(&rec)).into_response()),
        None => Ok(status_text(
            StatusCode::NOT_FOUND,
            format!("cron job not found: {job_id}"),
        )),
    }
}

/// `DELETE /api/cron-jobs/:id` — remove a cron job.
async fn delete_cron_job(
    State(state): State<ServerState>,
    Path(job_id): Path<String>,
) -> Result<axum::response::Response, ApiError> {
    require_platform_access(&state)?;
    let ok = state
        .core
        .session_index
        .cron_jobs_delete(&job_id)
        .map_err(ApiError::from)?;
    if ok {
        Ok(StatusCode::NO_CONTENT.into_response())
    } else {
        Ok(status_text(
            StatusCode::NOT_FOUND,
            format!("cron job not found: {job_id}"),
        ))
    }
}

// ---- Phase 6: webhook token config (per-source, UI-settable, encrypted) ----

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SetWebhookSourceTokenBody {
    src: String,
    token: String,
    #[serde(default)]
    auth_header_name: Option<String>,
    #[serde(default)]
    session_mode: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct PatchWebhookSourceBody {
    #[serde(default)]
    session_mode: Option<String>,
}

/// `GET /api/webhooks/:src/runs/:runId` — poll async webhook run status/result.
async fn get_webhook_run(
    State(state): State<ServerState>,
    Path((src, run_id)): Path<(String, String)>,
    headers: axum::http::HeaderMap,
) -> Result<axum::response::Response, ApiError> {
    let store = pointer_core::webhook_config::WebhookTokenStore::new(&state.core.session_index);
    let normalized_src = match pointer_core::webhook_config::authorize_webhook_ingress(
        &store,
        &src,
        &headers,
    ) {
        Ok(s) => s,
        Err(e) => {
            let (status, msg) = webhook_auth_status(e);
            return Ok(status_text(status, msg));
        }
    };
    let view = pointer_core::webhook_result::webhook_run_view_for_source(
        &state.core.session_index,
        &normalized_src,
        &run_id,
    )
    .map_err(ApiError::from)?;
    match view {
        Some(v) => Ok(Json(v).into_response()),
        None => Ok(status_text(StatusCode::NOT_FOUND, "run not found")),
    }
}

fn webhook_url_template() -> String {
    "http://{host}:{port}/api/webhooks/{src}".into()
}

fn webhook_url_for_src(src: &str) -> String {
    format!("http://{{host}}:{{port}}/api/webhooks/{src}")
}

fn build_webhook_config_view(
    session_index: &pointer_core::conversation_store::ConversationStore,
) -> Result<pointer_core::webhook_config::WebhookConfigView, ApiError> {
    let store = pointer_core::webhook_config::WebhookTokenStore::new(session_index);
    let sources: Vec<pointer_core::webhook_config::WebhookSourceView> = store
        .list_sources()
        .map_err(ApiError::from)?
        .into_iter()
        .map(|(src, token)| {
            let url = webhook_url_for_src(&src);
            store
                .source_view(src, token, url)
                .map_err(ApiError::from)
        })
        .collect::<Result<Vec<_>, ApiError>>()?;
    let legacy_configured = store.is_legacy_configured().map_err(ApiError::from)?;
    let legacy_preview = if legacy_configured {
        store.legacy_preview().map_err(ApiError::from)?
    } else {
        None
    };
    Ok(pointer_core::webhook_config::WebhookConfigView {
        sources,
        url_template: webhook_url_template(),
        legacy_configured,
        legacy_preview,
    })
}

/// `GET /api/webhooks/config` — list configured sources + URL template.
async fn get_webhook_config(
    State(state): State<ServerState>,
) -> Result<Json<pointer_core::webhook_config::WebhookConfigPublicView>, ApiError> {
    require_platform_access(&state)?;
    Ok(Json(
        build_webhook_config_view(&state.core.session_index)?.into(),
    ))
}

/// `POST /api/webhooks/config` — set token for a source (first-write only).
async fn set_webhook_source_token(
    State(state): State<ServerState>,
    Json(body): Json<SetWebhookSourceTokenBody>,
) -> Result<axum::response::Response, ApiError> {
    require_platform_access(&state)?;
    let store = pointer_core::webhook_config::WebhookTokenStore::new(&state.core.session_index);
    if store
        .is_source_configured(&body.src)
        .map_err(ApiError::from)?
    {
        return Ok(status_text(
            StatusCode::CONFLICT,
            "webhook token already configured for this source; clear it first to rotate",
        ));
    }
    match store.set_source_token(
        &body.src,
        &body.token,
        body.auth_header_name.as_deref(),
        body.session_mode
            .as_deref()
            .map(pointer_core::conversation_store::webhook_sources::WebhookSessionMode::parse)
            .transpose()
            .map_err(ApiError::from)?,
    ) {
        Ok(true) => {
            let view: pointer_core::webhook_config::WebhookConfigPublicView =
                build_webhook_config_view(&state.core.session_index)?.into();
            Ok(Json(view).into_response())
        }
        Ok(false) => Ok(status_text(
            StatusCode::CONFLICT,
            "webhook token already configured for this source",
        )),
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("must not be empty") || msg.contains("webhook source") {
                Ok(status_text(StatusCode::BAD_REQUEST, msg))
            } else {
                Err(ApiError(e))
            }
        }
    }
}

/// `GET /api/webhooks/config/:src/token` — reveal bearer token for settings copy.
async fn reveal_webhook_source_token(
    State(state): State<ServerState>,
    Path(src): Path<String>,
) -> Result<Json<pointer_core::webhook_config::WebhookTokenRevealView>, ApiError> {
    require_platform_access(&state)?;
    let store = pointer_core::webhook_config::WebhookTokenStore::new(&state.core.session_index);
    let normalized = pointer_core::webhook_config::WebhookTokenStore::normalize_src(&src)
        .map_err(ApiError::from)?;
    let Some(token) = store.reveal_source_token(&normalized).map_err(ApiError::from)? else {
        return Err(ApiError(anyhow::anyhow!(
            "webhook token not configured for this source"
        )));
    };
    log::info!("webhook_config: token revealed src={normalized}");
    Ok(Json(pointer_core::webhook_config::WebhookTokenRevealView {
        src: normalized,
        token: token.clone(),
        preview: pointer_core::webhook_config::mask_token(&token),
    }))
}

/// `PATCH /api/webhooks/config/:src` — update per-source webhook settings.
async fn patch_webhook_source(
    State(state): State<ServerState>,
    Path(src): Path<String>,
    Json(body): Json<PatchWebhookSourceBody>,
) -> Result<Json<pointer_core::webhook_config::WebhookConfigPublicView>, ApiError> {
    require_platform_access(&state)?;
    let store = pointer_core::webhook_config::WebhookTokenStore::new(&state.core.session_index);
    if let Some(mode_raw) = body.session_mode {
        let mode = pointer_core::conversation_store::webhook_sources::WebhookSessionMode::parse(
            &mode_raw,
        )
        .map_err(ApiError::from)?;
        store.set_session_mode(&src, mode).map_err(|e| {
            let msg = e.to_string();
            if msg.contains("not configured") {
                ApiError(anyhow::anyhow!(msg))
            } else {
                ApiError(e)
            }
        })?;
    }
    Ok(Json(
        build_webhook_config_view(&state.core.session_index)?.into(),
    ))
}

/// `DELETE /api/webhooks/config/:src` — clear a source token.
async fn clear_webhook_source_token(
    State(state): State<ServerState>,
    Path(src): Path<String>,
) -> Result<StatusCode, ApiError> {
    require_platform_access(&state)?;
    let store = pointer_core::webhook_config::WebhookTokenStore::new(&state.core.session_index);
    let ok = store.clear_source_token(&src).map_err(ApiError::from)?;
    if ok {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Ok(StatusCode::NOT_FOUND)
    }
}

/// `DELETE /api/webhooks/config/legacy` — clear deprecated global token.
async fn clear_webhook_legacy_token(
    State(state): State<ServerState>,
) -> Result<StatusCode, ApiError> {
    require_platform_access(&state)?;
    let store = pointer_core::webhook_config::WebhookTokenStore::new(&state.core.session_index);
    let ok = store.clear_legacy_token().map_err(ApiError::from)?;
    if ok {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Ok(StatusCode::NOT_FOUND)
    }
}

async fn cancel_chat(
    State(state): State<ServerState>,
    Path(conversation_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    require_platform_access(&state)?;
    state.core.cancel(&conversation_id);
    Ok(StatusCode::NO_CONTENT)
}

async fn abort_terminal_command(
    State(state): State<ServerState>,
    Path(conversation_id): Path<String>,
    payload: Option<Json<AbortTerminalPayload>>,
) -> Result<axum::Json<serde_json::Value>, ApiError> {
    require_platform_access(&state)?;
    let tool_call_id = payload.and_then(|Json(p)| p.tool_call_id);
    let aborted = state
        .core
        .abort_terminal_command(&conversation_id, tool_call_id.as_deref());
    Ok(axum::Json(serde_json::json!({ "aborted": aborted })))
}

#[derive(Deserialize, Default)]
struct AbortTerminalPayload {
    #[serde(default, rename = "toolCallId")]
    tool_call_id: Option<String>,
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
    require_platform_access(&state)?;
    if state
        .core
        .approve_tool_call(&tool_call_id, payload.approved)
    {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError(anyhow::anyhow!("未找到待审批的工具调用")))
    }
}

#[derive(Deserialize)]
struct TerminalInputPayload {
    text: String,
}

async fn submit_terminal_input(
    State(state): State<ServerState>,
    Path(request_id): Path<String>,
    Json(payload): Json<TerminalInputPayload>,
) -> Result<StatusCode, ApiError> {
    require_platform_access(&state)?;
    if state.core.submit_terminal_input(&request_id, payload.text) {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError(anyhow::anyhow!("未找到待输入的终端请求")))
    }
}

async fn dismiss_terminal_input(
    State(state): State<ServerState>,
    Path(request_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    require_platform_access(&state)?;
    if state.core.dismiss_terminal_input(&request_id) {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError(anyhow::anyhow!("未找到待输入的终端请求")))
    }
}

async fn chat_stream(
    State(state): State<ServerState>,
    Path(conversation_id): Path<String>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    require_platform_access(&state)?;
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
                        StreamEvent::ContextTrimApplied { conversation_id: id, .. } => {
                            id == &conversation_id
                        }
                        StreamEvent::ContextCompressionApplied { conversation_id: id, .. } => {
                            id == &conversation_id
                        }
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
    Ok(Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    ))
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

/// Resolve Vue production bundle directory (`dist/`).
fn resolve_static_dir() -> Option<PathBuf> {
    if let Ok(raw) = env::var("POINTER_SERVER_STATIC_DIR") {
        let path = PathBuf::from(raw.trim());
        if path.is_dir() {
            return path.canonicalize().ok();
        }
        log::warn!(
            "POINTER_SERVER_STATIC_DIR={} is not a directory; static hosting disabled",
            path.display()
        );
        return None;
    }

    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(cwd) = env::current_dir() {
        candidates.push(cwd.join("dist"));
    }
    if let Ok(exe) = env::current_exe() {
        if let Some(parent) = exe.parent() {
            candidates.push(parent.join("dist"));
            candidates.push(parent.join("../../dist"));
        }
    }

    for candidate in candidates {
        if let Ok(canonical) = candidate.canonicalize() {
            if canonical.is_dir() {
                return Some(canonical);
            }
        }
    }
    None
}

/// When `dist/` exists, serve the Vue SPA from the same process (API routes take precedence).
fn maybe_with_static_files(api: Router<ServerState>, static_dir: Option<PathBuf>) -> Router<ServerState> {
    let Some(dir) = static_dir else {
        log::info!("pointer-server: no dist/ found; API-only mode");
        return api;
    };
    let _ = WEB_DIST.set(dir);
    log::info!(
        "pointer-server: serving web UI from {}",
        WEB_DIST.get().map(|p| p.display().to_string()).unwrap_or_default()
    );
    api.fallback(get(spa_fallback))
}

static WEB_DIST: OnceLock<PathBuf> = OnceLock::new();

async fn get_platform_session(State(state): State<ServerState>) -> Json<PlatformSessionView> {
    Json(state.core.active_platform_auth().session_view())
}

#[derive(Deserialize)]
struct OAuthCallbackQuery {
    code: String,
    state: String,
}

/// `POST /api/auth/login/start` — begin a PKCE browser OAuth flow.
/// Returns `{ authorize_url }` for the frontend to redirect to.
async fn start_platform_login(
    State(state): State<ServerState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if pointer_core::deployment_mode::is_standalone() {
        return Err(ApiError(anyhow::anyhow!(
            "platform OAuth disabled in standalone mode; use POST /api/auth/local/login"
        )));
    }
    let public_url = resolve_server_public_url().ok_or_else(|| {
        ApiError(anyhow::anyhow!(
            "POINTER_SERVER_PUBLIC_URL 未配置；请在 pointer-server.toml [server].public_url 设置外部可达地址"
        ))
    })?;
    let redirect_uri = format!("{public_url}/api/auth/oauth/callback");
    let (verifier, challenge) = PlatformAuthManager::generate_pkce();
    let state_token = random_state_token();
    purge_expired_oauth_pending(&state.oauth_pending);
    state.oauth_pending.write().insert(
        state_token.clone(),
        PkcePending {
            verifier,
            redirect_uri: redirect_uri.clone(),
            created_at: Instant::now(),
        },
    );
    let url = PlatformAuthManager::build_authorize_url_for_redirect(
        &redirect_uri,
        &challenge,
        &state_token,
    );
    log::info!("platform_auth: login start state={state_token}");
    Ok(Json(serde_json::json!({ "authorize_url": url })))
}

/// `GET /api/auth/oauth/callback` — terminal hop of the PKCE flow.
/// Browser lands here with `?code=&state=`; server exchanges the code for
/// access/refresh tokens, persists the refresh token via `set_session`,
/// then redirects back to the SPA root with a status query.
async fn platform_oauth_callback(
    State(state): State<ServerState>,
    Query(q): Query<OAuthCallbackQuery>,
) -> Result<Response, (StatusCode, String)> {
    if pointer_core::deployment_mode::is_standalone() {
        return Err((StatusCode::NOT_FOUND, "platform_oauth_disabled".into()));
    }
    if q.code.trim().is_empty() {
        log::warn!("platform_auth: callback missing code");
        return Err((StatusCode::BAD_REQUEST, "missing_code".into()));
    }
    let pending = state.oauth_pending.write().remove(&q.state);
    let PkcePending {
        verifier,
        redirect_uri,
        ..
    } = pending.ok_or_else(|| {
        log::warn!("platform_auth: callback unknown/expired state={}", q.state);
        (
            StatusCode::BAD_REQUEST,
            "invalid_or_expired_state".into(),
        )
    })?;
    let auth = Arc::new(PlatformAuthManager::new());
    match auth
        .exchange_authorization_code(&q.code, &verifier, &q.state, &redirect_uri)
        .await
    {
        Ok((session, creds)) => {
            if pointer_core::server_access::access_restriction_enabled()
                && !pointer_core::server_access::is_user_allowed(&session.user.id)
            {
                log::warn!(
                    "platform_auth: rejected OAuth login user_id={} not in allowed_user_ids",
                    session.user.id
                );
                return Ok(
                    Redirect::temporary("/?platform_login_error=server_access_denied").into_response(),
                );
            }
            auth.set_session(session);
            let session_id = state.web_sessions.insert(
                auth,
                creds,
                pointer_core::web_request_auth::WebSessionAuthKind::Platform,
            );
            sync_automation_web_session(&state);
            log::info!("platform_auth: callback ok state={} web_session={session_id}", q.state);
            let mut resp = Redirect::temporary("/?platform_login=success").into_response();
            web_session::set_session_cookie(resp.headers_mut(), &session_id, cookie_secure());
            Ok(resp)
        }
        Err(e) => {
            log::warn!("platform_auth: exchange failed: {e:#}");
            let msg = urlencoding_encode(&e.to_string());
            Ok(Redirect::temporary(&format!("/?platform_login_error={msg}")).into_response())
        }
    }
}

/// `POST /api/auth/logout` — clear browser web session cookie and in-memory auth.
async fn platform_logout(
    headers: HeaderMap,
    State(state): State<ServerState>,
) -> Response {
    if let Some(session_id) = web_session::session_id_from_headers(&headers) {
        if let Some(entry) = state.web_sessions.get(&session_id) {
            entry.auth.clear_session_async().await;
        }
        state.web_sessions.remove(&session_id);
    }
    sync_automation_web_session(&state);
    let mut platform = state.core.platform_config.write();
    apply_login_media_oss(&mut platform, None);
    log::info!("platform_auth: logout");
    let mut resp = StatusCode::NO_CONTENT.into_response();
    web_session::clear_session_cookie(resp.headers_mut(), cookie_secure());
    resp
}

/// `POST /api/auth/refresh` — refresh access token if near expiry and re-pull
/// LLM credentials. Mirrors the desktop `refresh_platform_session` command.
async fn refresh_platform_session(
    headers: HeaderMap,
    State(state): State<ServerState>,
) -> Result<Json<PlatformSessionView>, ApiError> {
    let is_local = pointer_core::deployment_mode::is_standalone()
        && pointer_core::web_request_auth::is_local_scoped_session();
    if !is_local {
        state
            .core
            .active_platform_auth()
            .refresh_if_needed()
            .await
            .map_err(ApiError::from)?;
    }
    if state.core.active_platform_auth().session_view().logged_in {
        require_allowed_platform_user(&state)?;
        if !is_local {
            if let Ok(Some(creds)) = state
                .core
                .active_platform_auth()
                .fetch_llm_credentials()
                .await
            {
                if let Some(session_id) = web_session::session_id_from_headers(&headers) {
                    state.web_sessions.update_creds(&session_id, creds);
                } else {
                    state.core.apply_login_credentials(&creds);
                }
            }
        }
    }
    Ok(Json(
        state.core.active_platform_auth().session_view(),
    ))
}

pub(crate) fn cookie_secure() -> bool {
    resolve_server_public_url()
        .is_some_and(|url| url.to_ascii_lowercase().starts_with("https://"))
}

/// Resolve the externally-reachable base URL for OAuth `redirect_uri`.
/// Prefers `POINTER_SERVER_PUBLIC_URL`; falls back to the host portion of
/// `POINTER_SERVER_ADDR`. For loopback binds (127.0.0.1 / 0.0.0.0 / localhost)
/// the fallback uses `http://127.0.0.1:{port}` so local development works
/// without configuration — the browser is on the same machine and can reach
/// the callback. Production deployments behind a public domain should set
/// `POINTER_SERVER_PUBLIC_URL` explicitly to override this.
fn resolve_server_public_url() -> Option<String> {
    if let Ok(raw) = env::var("POINTER_SERVER_PUBLIC_URL") {
        let trimmed = raw.trim().trim_end_matches('/').to_string();
        if !trimmed.is_empty() {
            return Some(trimmed);
        }
    }
    let addr = env::var("POINTER_SERVER_ADDR").unwrap_or_else(|_| "127.0.0.1:8787".into());
    let (host, port) = match addr.rsplit_once(':') {
        Some((h, p)) => (h.trim(), p.trim()),
        None => return None,
    };
    if host.is_empty() || port.is_empty() {
        return None;
    }
    if host == "0.0.0.0" || host == "127.0.0.1" || host.eq_ignore_ascii_case("localhost") {
        // Loopback bind: the browser is local, so 127.0.0.1:{port} is reachable.
        return Some(format!("http://127.0.0.1:{port}"));
    }
    Some(format!("http://{host}:{port}"))
}

fn random_state_token() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    hex_encode(&bytes)
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

fn purge_expired_oauth_pending(pending: &RwLock<HashMap<String, PkcePending>>) {
    let now = Instant::now();
    let mut guard = pending.write();
    guard.retain(|_, v| now.duration_since(v.created_at) < OAUTH_PENDING_TTL);
}

async fn spa_fallback(
    State(state): State<ServerState>,
    uri: Uri,
) -> Result<Response, StatusCode> {
    if let Some(resp) = try_cloud_oauth_exchange(&state, &uri).await {
        return Ok(resp);
    }
    let root = WEB_DIST.get().ok_or(StatusCode::NOT_FOUND)?;
    let rel = uri.path().trim_start_matches('/');
    let candidate = if rel.is_empty() {
        root.join("index.html")
    } else {
        root.join(rel)
    };
    if candidate.is_file() {
        return serve_static_file(&candidate).await;
    }
    serve_static_file(&root.join("index.html")).await
}

async fn try_cloud_oauth_exchange(state: &ServerState, uri: &Uri) -> Option<Response> {
    let query = uri.query()?;
    let code = form_query_param(query, "code")?;
    let oauth_state = form_query_param(query, "state")?;
    if !pointer_core::cloud_agent_auth::is_cloud_auth_configured() {
        log::warn!("cloud oauth: OPENPOINTER_API_BASE not configured");
        return Some(Redirect::temporary("/?cloud_auth_error=not_configured").into_response());
    }
    match pointer_core::cloud_agent_auth::exchange_agent_oauth_code(&code, &oauth_state).await {
        Ok((session, creds)) => {
            if pointer_core::server_access::access_restriction_enabled()
                && !pointer_core::server_access::is_user_allowed(&session.user.id)
            {
                log::warn!(
                    "cloud oauth: rejected user_id={} not in allowed_user_ids",
                    session.user.id
                );
                return Some(
                    Redirect::temporary("/?cloud_auth_error=server_access_denied").into_response(),
                );
            }
            let auth = Arc::new(PlatformAuthManager::new());
            auth.set_partner_session(session);
            let session_id = state.web_sessions.insert(
                auth,
                creds,
                pointer_core::web_request_auth::WebSessionAuthKind::Platform,
            );
            sync_automation_web_session(&state);
            log::info!("cloud oauth: exchange succeeded, redirecting to /");
            let mut resp = Redirect::temporary("/").into_response();
            web_session::set_session_cookie(resp.headers_mut(), &session_id, cookie_secure());
            Some(resp)
        }
        Err(e) => {
            log::warn!("cloud oauth: exchange failed: {e:#}");
            Some(Redirect::temporary("/?cloud_auth_error=1").into_response())
        }
    }
}

fn form_query_param(query: &str, key: &str) -> Option<String> {
    for (k, v) in form_urlencoded_parse(query) {
        if k == key {
            let trimmed = v.trim().to_string();
            if !trimmed.is_empty() {
                return Some(trimmed);
            }
        }
    }
    None
}

fn form_urlencoded_parse(query: &str) -> Vec<(String, String)> {
    query
        .split('&')
        .filter_map(|pair| {
            let mut parts = pair.splitn(2, '=');
            let k = parts.next()?.trim();
            if k.is_empty() {
                return None;
            }
            let v = parts.next().unwrap_or("").trim();
            Some((percent_decode(k), percent_decode(v)))
        })
        .collect()
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(&input[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Percent-encode a string for use as a single URL query value.
/// Encodes everything except RFC 3986 unreserved characters.
fn urlencoding_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

async fn serve_static_file(path: &std::path::Path) -> Result<Response, StatusCode> {
    let bytes = tokio::fs::read(path)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    let mut response = Response::new(bytes.into());
    if let Ok(value) = HeaderValue::from_str(static_content_type(path)) {
        response.headers_mut().insert(header::CONTENT_TYPE, value);
    }
    if path.file_name().and_then(|n| n.to_str()) == Some("index.html") {
        response.headers_mut().insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-cache"),
        );
    }
    Ok(response)
}

fn static_content_type(path: &std::path::Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("ico") => "image/x-icon",
        Some("json") => "application/json; charset=utf-8",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        Some("ttf") => "font/ttf",
        Some("map") => "application/json; charset=utf-8",
        _ => "application/octet-stream",
    }
}

struct ApiError(anyhow::Error);

impl From<anyhow::Error> for ApiError {
    fn from(err: anyhow::Error) -> Self {
        Self(err)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        let msg = self.0.to_string();
        if msg.contains("platform_login_required") {
            return (StatusCode::UNAUTHORIZED, msg).into_response();
        }
        if msg.contains("server_access_denied") {
            return (StatusCode::FORBIDDEN, msg).into_response();
        }
        (StatusCode::INTERNAL_SERVER_ERROR, msg).into_response()
    }
}

/// Build a plain status+text error response (used by webhook auth failures
/// where a specific HTTP status is required without touching `ApiError`).
fn status_text(status: StatusCode, msg: impl Into<String>) -> axum::response::Response {
    (status, msg.into()).into_response()
}
