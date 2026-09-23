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
    adapters::register_builtin_channels, adapters::weixin::qr_login::QrLoginState,
    gateway::ChannelGateway, monitor_supervisor::MonitorSupervisor,
    registration::ChannelRegistrationState, registry::ChannelRegistry,
};
use pointer_core::{
    agents::computer::capture_debug,
    agents::AgentDef,
    chat_service::AppState,
    chat_service::GlobalMcpView,
    dispatcher::{
        DeliverTarget, RunDispatcher, RunHandle, RunOutcome, TriggerMeta, TriggerRequest,
        TriggerSource,
    },
    models::{
        ChatMediaPreview, ComputerAnnotatedPreview, ComputerMonitor, Conversation,
        DebugSessionSettings, PlatformSettings, SendChatPayload, SkillDef, SkillImportResult,
        StreamEvent, ToolDef, UserSettings, WebEffectiveSettingsView,
    },
    platform_auth::{PlatformAuthManager, PlatformSessionView},
    platform_config::apply_login_media_oss,
    plugins::manifest::McpServerDecl,
    plugins::registry::PluginView,
    provider::OpenAIProvider,
    storage,
};
mod channels;
mod local_auth;
mod web_session;

use web_session::WebSessionStore;

use channels::{
    approve_channel_pairing, channel_registration_status, channel_webhook, get_channel_webhook_url,
    get_channels_config, list_channel_pairing_pending, list_channels, start_channel_registration,
    start_weixin_login, update_channels, weixin_login_status,
};
use serde::Deserialize;
use tower_http::{
    compression::{
        predicate::{NotForContentType, Predicate, SizeAbove},
        CompressionLayer,
    },
    cors::{AllowHeaders, AllowMethods, AllowOrigin, CorsLayer},
};

#[derive(Deserialize)]
struct ConversationPreviewQuery {
    #[serde(rename = "conversationId")]
    conversation_id: String,
}

#[derive(Deserialize)]
struct WorkspacePathQuery {
    #[serde(rename = "workspaceRoot")]
    workspace_root: String,
    #[serde(default, rename = "relativePath")]
    relative_path: Option<String>,
    /// Optional porcelain status (e.g. `M.`, `MM`, `??`) for Git full-file review.
    #[serde(default)]
    status: Option<String>,
}

async fn list_workspace_directory(
    State(state): State<ServerState>,
    Query(q): Query<WorkspacePathQuery>,
) -> Result<Json<Vec<pointer_core::workspace_read::WorkspaceEntry>>, ApiError> {
    require_platform_access(&state)?;
    Ok(Json(pointer_core::workspace_read::list_directory(
        std::path::Path::new(&q.workspace_root),
        q.relative_path.as_deref(),
    )?))
}

#[derive(Debug, Deserialize)]
struct WorkspaceSearchQuery {
    workspace_root: String,
    #[serde(default)]
    query: String,
    #[serde(default)]
    limit: Option<u32>,
}

async fn search_workspace_entries(
    State(state): State<ServerState>,
    Query(q): Query<WorkspaceSearchQuery>,
) -> Result<Json<Vec<pointer_core::workspace_read::WorkspaceEntry>>, ApiError> {
    require_platform_access(&state)?;
    let limit = q.limit.unwrap_or(80).clamp(1, 200) as usize;
    Ok(Json(pointer_core::workspace_read::search_entries(
        std::path::Path::new(&q.workspace_root),
        &q.query,
        limit,
    )?))
}

async fn read_workspace_file(
    State(state): State<ServerState>,
    Query(q): Query<WorkspacePathQuery>,
) -> Result<Json<pointer_core::workspace_read::WorkspaceFilePreview>, ApiError> {
    require_platform_access(&state)?;
    let relative_path = q.relative_path.as_deref().unwrap_or("");
    Ok(Json(pointer_core::workspace_read::read_file(
        std::path::Path::new(&q.workspace_root),
        relative_path,
    )?))
}

/// Stream workspace file bytes for inline image/PDF preview in the web UI.
async fn stream_workspace_file_media(
    State(state): State<ServerState>,
    Query(q): Query<WorkspacePathQuery>,
) -> Result<Response, ApiError> {
    require_platform_access(&state)?;
    let relative_path = q.relative_path.as_deref().unwrap_or("");
    let media = pointer_core::workspace_read::resolve_file_for_media(
        std::path::Path::new(&q.workspace_root),
        relative_path,
    )
    .map_err(ApiError::from)?;
    log::info!(
        "workspace media stream path={} file={}",
        media.path.display(),
        media.file_name
    );
    stream_media_file_response(&media.path, &media.mime_type, &media.file_name, true).await
}

async fn delete_workspace_path(
    State(state): State<ServerState>,
    Query(q): Query<WorkspacePathQuery>,
) -> Result<StatusCode, ApiError> {
    require_platform_access(&state)?;
    let relative_path = q.relative_path.as_deref().unwrap_or("");
    pointer_core::workspace_read::delete_path(
        std::path::Path::new(&q.workspace_root),
        relative_path,
    )?;
    Ok(StatusCode::NO_CONTENT)
}

async fn get_workspace_git_status(
    State(state): State<ServerState>,
    Query(q): Query<WorkspacePathQuery>,
) -> Result<Json<pointer_core::workspace_read::GitStatusResponse>, ApiError> {
    require_platform_access(&state)?;
    Ok(Json(pointer_core::workspace_read::git_status_response(
        std::path::Path::new(&q.workspace_root),
    )?))
}

async fn get_workspace_git_diff(
    State(state): State<ServerState>,
    Query(q): Query<WorkspacePathQuery>,
) -> Result<Json<pointer_core::workspace_read::GitDiff>, ApiError> {
    require_platform_access(&state)?;
    let relative_path = q.relative_path.as_deref().unwrap_or("");
    Ok(Json(pointer_core::workspace_read::git_diff(
        std::path::Path::new(&q.workspace_root),
        relative_path,
        q.status.as_deref(),
    )?))
}

#[derive(Debug, Deserialize)]
struct TurnFileDiffQuery {
    #[serde(rename = "conversationId")]
    conversation_id: String,
    #[serde(rename = "turnId")]
    turn_id: String,
    #[serde(rename = "workspaceRoot")]
    workspace_root: String,
    path: String,
}

async fn get_turn_file_diff(
    State(state): State<ServerState>,
    Query(q): Query<TurnFileDiffQuery>,
) -> Result<Json<pointer_core::turn_file_baseline::TurnFileDiff>, ApiError> {
    require_platform_access(&state)?;
    Ok(Json(pointer_core::turn_file_baseline::turn_file_diff(
        &q.conversation_id,
        &q.turn_id,
        std::path::Path::new(&q.workspace_root),
        &q.path,
    )?))
}

#[derive(Debug, Deserialize)]
struct TurnFileChangesQuery {
    #[serde(rename = "conversationId")]
    conversation_id: String,
    /// Comma-separated lead turn ids (page window; typically ≤8).
    #[serde(rename = "turnIds")]
    turn_ids: String,
}

async fn list_turn_file_changes(
    State(state): State<ServerState>,
    Query(q): Query<TurnFileChangesQuery>,
) -> Result<Json<Vec<pointer_core::turn_file_baseline::TurnFileChangesForTurn>>, ApiError> {
    require_platform_access(&state)?;
    let turn_ids: Vec<String> = q
        .turn_ids
        .split(',')
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .collect();
    Ok(Json(
        pointer_core::turn_file_baseline::list_turn_file_changes(&q.conversation_id, &turn_ids)?,
    ))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SaveTurnFileChangesBody {
    conversation_id: String,
    turn_id: String,
    files: Vec<pointer_core::turn_file_baseline::TurnFileChangeEntry>,
}

async fn save_turn_file_changes(
    State(state): State<ServerState>,
    Json(body): Json<SaveTurnFileChangesBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_platform_access(&state)?;
    pointer_core::turn_file_baseline::save_turn_file_changes(
        &body.conversation_id,
        &body.turn_id,
        &body.files,
    )?;
    Ok(Json(serde_json::json!({ "ok": true })))
}
use parking_lot::RwLock;
use rand::RngCore;
use std::{
    collections::HashMap,
    convert::Infallible,
    env,
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};
use tokio::sync::broadcast;

/// Lifetime of a pending PKCE login entry. The user must complete the
/// browser OAuth flow within this window or the callback will reject it.
const OAUTH_PENDING_TTL: Duration = Duration::from_secs(600);

#[derive(Clone)]
pub(crate) struct ServerState {
    core: Arc<AppState>,
    /// Unified run dispatcher: the callable / event-triggered entry point.
    /// `POST /api/chat` and `POST /api/runs` (Phase 4) route through it.
    dispatcher: Arc<RunDispatcher>,
    events: broadcast::Sender<pointer_core::stream_broadcast::StreamBroadcastItem>,
    channel_gateway: Arc<ChannelGateway>,
    channel_monitors: Arc<MonitorSupervisor>,
    qr_login: Arc<QrLoginState>,
    registration: Arc<ChannelRegistrationState>,
    /// PKCE verifiers keyed by `state` for in-flight browser OAuth logins.
    oauth_pending: Arc<RwLock<HashMap<String, PkcePending>>>,
    /// Per-browser platform OAuth sessions (cookie `pointer_web_session`).
    web_sessions: Arc<WebSessionStore>,
    /// Standalone login captcha challenges (in-memory, one-time).
    captcha_store: Arc<local_auth::CaptchaStore>,
    /// Standalone SSO ticket `jti` one-time store.
    sso_nonces: Arc<pointer_core::local_sso::SsoNonceStore>,
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

    let args: Vec<String> = std::env::args().collect();

    // --hash-password: print password_hmac for [auth.local] (needs hmac_secret).
    // Usage: pointer-server --hash-password [--secret SECRET] [PASSWORD]
    // If PASSWORD omitted, read one line from stdin.
    if let Some(idx) = args.iter().position(|a| a == "--hash-password") {
        let mut secret = std::env::var("POINTER_SERVER_AUTH_HMAC_SECRET").unwrap_or_default();
        let mut password = String::new();
        let mut i = idx + 1;
        while i < args.len() {
            if args[i] == "--secret" {
                i += 1;
                if i >= args.len() {
                    eprintln!("error: --secret requires a value");
                    std::process::exit(1);
                }
                secret = args[i].clone();
            } else if password.is_empty() && !args[i].starts_with('-') {
                password = args[i].clone();
            } else {
                eprintln!("error: unexpected argument {}", args[i]);
                std::process::exit(1);
            }
            i += 1;
        }
        if secret.trim().is_empty() {
            // Try loading config so hmac_secret from TOML is available.
            let _ = pointer_core::server_config::load_server_config();
            secret = std::env::var("POINTER_SERVER_AUTH_HMAC_SECRET").unwrap_or_default();
        }
        if secret.trim().is_empty() {
            eprintln!(
                "error: hmac_secret required (pass --secret, set POINTER_SERVER_AUTH_HMAC_SECRET, \
                 or configure [auth.local].hmac_secret in pointer-server.toml)"
            );
            std::process::exit(1);
        }
        if password.is_empty() {
            use std::io::Read;
            let mut buf = String::new();
            if let Err(e) = std::io::stdin().read_to_string(&mut buf) {
                eprintln!("error: failed to read password from stdin: {e}");
                std::process::exit(1);
            }
            password = buf.trim_end_matches(['\r', '\n']).to_string();
        }
        if password.is_empty() {
            eprintln!("error: empty password");
            std::process::exit(1);
        }
        let digest = pointer_core::local_auth::hmac_sha256_hex(secret.trim(), &password);
        println!("password_hmac = \"{digest}\"");
        return Ok(());
    }

    // --mint-sso-ticket: print a short-lived SSO ticket for standalone ?sso= login.
    // Usage: pointer-server --mint-sso-ticket --sub USER_ID [--name NICK] [--ttl SECS]
    //        [--secret SECRET] [--audience AUD]
    if let Some(idx) = args.iter().position(|a| a == "--mint-sso-ticket") {
        let _ = pointer_core::server_config::load_server_config();
        let mut secret = std::env::var("POINTER_SERVER_SSO_SECRET").unwrap_or_default();
        let mut audience = std::env::var("POINTER_SERVER_SSO_AUDIENCE").unwrap_or_default();
        let mut sub = String::new();
        let mut name: Option<String> = None;
        let mut ttl: i64 = 120;
        let mut i = idx + 1;
        while i < args.len() {
            match args[i].as_str() {
                "--secret" => {
                    i += 1;
                    if i >= args.len() {
                        eprintln!("error: --secret requires a value");
                        std::process::exit(1);
                    }
                    secret = args[i].clone();
                }
                "--audience" => {
                    i += 1;
                    if i >= args.len() {
                        eprintln!("error: --audience requires a value");
                        std::process::exit(1);
                    }
                    audience = args[i].clone();
                }
                "--sub" => {
                    i += 1;
                    if i >= args.len() {
                        eprintln!("error: --sub requires a value");
                        std::process::exit(1);
                    }
                    sub = args[i].clone();
                }
                "--name" => {
                    i += 1;
                    if i >= args.len() {
                        eprintln!("error: --name requires a value");
                        std::process::exit(1);
                    }
                    name = Some(args[i].clone());
                }
                "--ttl" => {
                    i += 1;
                    if i >= args.len() {
                        eprintln!("error: --ttl requires a value");
                        std::process::exit(1);
                    }
                    ttl = args[i].parse().unwrap_or(120);
                }
                other => {
                    eprintln!("error: unexpected argument {other}");
                    std::process::exit(1);
                }
            }
            i += 1;
        }
        if secret.trim().is_empty() || audience.trim().is_empty() || sub.trim().is_empty() {
            eprintln!(
                "error: --mint-sso-ticket requires --sub and SSO secret/audience \
                 (--secret/--audience or [auth.local.sso] / POINTER_SERVER_SSO_*)"
            );
            std::process::exit(1);
        }
        let now = chrono::Utc::now().timestamp();
        match pointer_core::local_sso::mint_sso_ticket(
            secret.trim(),
            audience.trim(),
            sub.trim(),
            name.as_deref(),
            ttl,
            now,
        ) {
            Ok(ticket) => {
                println!("{ticket}");
                return Ok(());
            }
            Err(e) => {
                eprintln!("error: {e:#}");
                std::process::exit(1);
            }
        }
    }

    // --machine-id / --machine-id-json: print binding material and exit (no config/license needed)
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
    if let Err(err) = pointer_core::logging::init_runtime_logging(&log_dir, DEFAULT_LOG_FILTER) {
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
        log::info!(
            "pointer-server: standalone mode — local password/SSO auth + config-injected LLM keys"
        );
        pointer_core::local_auth::warn_if_deprecated_admin_token_configured();
        if !pointer_core::local_auth::local_password_auth_configured() {
            log::warn!(
                "pointer-server: standalone password auth incomplete — set [auth.local] username, \
                 password_hmac, and hmac_secret (or matching POINTER_SERVER_ADMIN_* env vars)"
            );
        }
        if pointer_core::local_sso::local_sso_configured() {
            log::info!(
                "pointer-server: standalone SSO enabled (audience={})",
                pointer_core::local_sso::configured_sso_audience()
            );
        } else {
            log::info!(
                "pointer-server: standalone SSO not configured — set [auth.local.sso] secret + audience \
                 for third-party ?sso= login"
            );
        }
    } else {
        log::info!(
            "pointer-server: web mode — auth.dat persistence disabled; per-browser cookie sessions"
        );
    }

    let core = Arc::new(AppState::new());
    // 启动兜底（统一入口）：把已启用插件的技能合并进 general 启用列表（旧代码启用过的插件也能自动恢复）。
    if let Err(e) = core.init_launch() {
        log::warn!("plugin skill reconcile at startup failed: {e:#}");
    }
    core.start_background_tasks();
    let web_sessions = Arc::new(WebSessionStore::default());
    match resolve_server_public_url() {
        Some(url) => log::info!("platform_auth: server public url = {url}"),
        None => {
            if pointer_core::deployment_mode::is_standalone() {
                log::warn!(
                    "platform_auth: POINTER_SERVER_PUBLIC_URL not configured (standalone); \
                     OAuth routes disabled, use username/password via POST /api/auth/local/login"
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
    // Larger buffer: weak clients / high-frequency tool output lag the SSE
    // consumer; when the ring overflows we emit `resync` (see chat_stream).
    let (events, _) =
        broadcast::channel::<pointer_core::stream_broadcast::StreamBroadcastItem>(4096);
    // Bridge global stream_broadcast -> server events so the SSE endpoint
    // (`GET /api/chat/:id/stream`) keeps working regardless of who calls
    // `run_chat`. The dispatcher calls `run_chat`, which emits via
    // `publish_stream` -> this callback -> `events`.
    {
        let ev_tx = events.clone();
        pointer_core::stream_broadcast::subscribe_stream(Arc::new(move |item| {
            // No subscribers -> send fails; ignore (matches existing behavior).
            let _ = ev_tx.send(item);
        }));
    }
    match capture_debug::purge_computer_captures_older_than_days(
        capture_debug::CAPTURE_RETENTION_DAYS,
    ) {
        Ok(removed) if removed > 0 => {
            let _ = events.send(pointer_core::stream_broadcast::StreamBroadcastItem {
                conversation_id: None,
                session_user_id: None,
                event: StreamEvent::UiToast {
                    conversation_id: String::new(),
                    message: "截图过期已清理".into(),
                    level: "warning".into(),
                },
            });
        }
        Ok(_) => {}
        Err(e) => log::warn!("computer capture purge failed: {e}"),
    }
    let mut channel_registry = ChannelRegistry::new();
    register_builtin_channels(&mut channel_registry);
    let channel_gateway = Arc::new(ChannelGateway::new(core.clone(), channel_registry)?);
    pointer_channels::install_channel_outbound_bridge(channel_gateway.clone(), core.tools.clone());
    let channel_monitors = Arc::new(MonitorSupervisor::new());
    channel_monitors.start(channel_gateway.clone());
    let registration = Arc::new(ChannelRegistrationState::with_completion(
        channel_gateway.clone(),
        channel_monitors.clone(),
    ));

    let state = ServerState {
        core: core.clone(),
        dispatcher: Arc::new(core.build_dispatcher_with_extra_finished_hooks(vec![
            std::sync::Arc::new(pointer_channels::im_deliver_hook::ImDeliverHook::new(
                channel_gateway.clone(),
            )),
        ])),
        events,
        channel_gateway,
        channel_monitors,
        qr_login: Arc::new(QrLoginState::new()),
        registration,
        oauth_pending: Arc::new(RwLock::new(HashMap::new())),
        web_sessions: web_sessions.clone(),
        captcha_store: Arc::new(local_auth::CaptchaStore::default()),
        sso_nonces: Arc::new(pointer_core::local_sso::SsoNonceStore::new()),
    };

    // Phase 5: start the cron scheduler. The server (web host) enables it by
    // default in every deployment mode. `POINTER_SCHEDULER_ENABLED=0` / `false`
    // explicitly disables it; `=1` / `true` forces it on.
    let scheduler_enabled = env::var("POINTER_SCHEDULER_ENABLED")
        .map(|v| v != "0" && v.to_ascii_lowercase() != "false")
        .unwrap_or(true);
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
        .route("/api/workspace/directory", get(list_workspace_directory))
        .route("/api/workspace/search", get(search_workspace_entries))
        .route("/api/workspace/file", get(read_workspace_file))
        .route(
            "/api/workspace/file-media",
            get(stream_workspace_file_media),
        )
        .route("/api/workspace/path", delete(delete_workspace_path))
        .route("/api/workspace/git/status", get(get_workspace_git_status))
        .route("/api/workspace/git/diff", get(get_workspace_git_diff))
        .route("/api/workspace/turn-file-diff", get(get_turn_file_diff))
        .route(
            "/api/workspace/turn-file-changes",
            get(list_turn_file_changes).post(save_turn_file_changes),
        )
        .route("/api/version", get(api_version))
        .route("/api/ready", get(api_ready))
        .route("/api/platform/session", get(get_platform_session))
        .route("/api/auth/mode", get(local_auth::auth_mode))
        .route("/api/auth/login/start", post(start_platform_login))
        .route("/api/auth/local/captcha", get(local_auth::local_captcha))
        .route("/api/auth/local/login", post(local_auth::local_login))
        .route("/api/auth/local/sso", get(standalone_sso_login))
        .route("/api/auth/oauth/callback", get(platform_oauth_callback))
        .route("/api/auth/logout", post(platform_logout))
        .route("/api/auth/refresh", post(refresh_platform_session))
        .route("/api/license/status", get(local_auth::license_status))
        .route("/api/license/reload", post(local_auth::license_reload))
        .route("/api/settings", get(get_settings))
        .route(
            "/api/debug-session-settings",
            put(update_debug_session_settings),
        )
        .route("/api/user-settings", put(update_user_settings))
        .route("/api/platform-settings", put(update_platform_settings))
        .route("/api/key", post(set_api_key).delete(clear_api_key))
        .route("/api/test-connection", post(test_connection))
        .route("/api/skills", get(list_skills).post(import_skill_zip))
        .route("/api/skills/reload-meta", post(reload_skill_meta))
        .route("/api/skills/external-probe", get(probe_external_skills))
        .route("/api/skills/import-external", post(import_external_skills))
        .route(
            "/api/skills/external-probe/dismiss",
            post(dismiss_external_skills_prompt),
        )
        .route("/api/plugins", get(list_plugins).post(import_plugin))
        .route("/api/plugins/import-zip", post(import_plugin_zip))
        .route("/api/plugins/discover", post(discover_plugins))
        .route("/api/plugins/:plugin_id/enable", post(enable_plugin))
        .route("/api/plugins/:plugin_id/disable", post(disable_plugin))
        .route("/api/plugins/:plugin_id/uninstall", post(uninstall_plugin))
        .route("/api/plugins/external-probe", get(probe_external_plugins))
        .route("/api/plugins/import-external", post(import_external_plugin))
        .route("/api/mcp", get(list_global_mcp).put(save_global_mcp))
        .route("/api/mcp/reload", post(reload_global_mcp))
        .route("/api/mcp/restart", post(restart_global_mcp))
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
        .route(
            "/api/computer/manual-snapshot",
            post(manual_computer_snapshot),
        )
        .route("/api/computer/monitors", get(list_computer_monitors))
        .route(
            "/api/computer/monitor",
            post(set_computer_conversation_monitor),
        )
        .route(
            "/api/computer/monitor-pick/:conversation_id/confirm",
            post(confirm_computer_monitor_pick),
        )
        .route(
            "/api/computer/monitor-pick/:conversation_id/cancel",
            post(cancel_computer_monitor_pick),
        )
        .route("/api/conversations", get(load_conversations))
        .route(
            "/api/conversations/:conversation_id",
            delete(delete_conversation_handler),
        )
        .route(
            "/api/conversations/:conversation_id/meta",
            get(load_conversation_meta_handler),
        )
        .route(
            "/api/conversations/meta",
            get(load_conversation_metas).put(save_conversation_meta),
        )
        .route("/api/projects/sidebar", get(load_sidebar_projects))
        .route("/api/projects", get(load_projects).post(create_project))
        .route(
            "/api/projects/:project_id",
            get(load_project)
                .patch(update_project)
                .delete(delete_project),
        )
        .route(
            "/api/projects/:project_id/conversations",
            get(load_project_conversation_metas),
        )
        .route(
            "/api/conversations/search",
            get(search_conversations_handler),
        )
        .route(
            "/api/conversations/:conversation_id/search-matches",
            get(list_conversation_search_matches_handler),
        )
        .route(
            "/api/conversations/:conversation_id/outline",
            get(list_conversation_outline_handler),
        )
        .route(
            "/api/conversations/:conversation_id/messages",
            get(load_conversation_messages_handler),
        )
        .route(
            "/api/conversations/:conversation_id/scoped-messages",
            get(load_scoped_sub_messages_handler),
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
        .route("/api/chat/media-ref-download", get(download_media_ref))
        .route("/api/media/public-download", get(public_media_download))
        .route(
            "/api/chat/save-attachment",
            post(save_chat_attachment).layer(DefaultBodyLimit::max(
                pointer_core::models::attachment_upload_http_body_limit(),
            )),
        )
        .route(
            "/api/chat/upload-video-oss",
            post(upload_composer_video_oss),
        )
        .route("/api/media/deps", get(check_media_deps))
        .route("/api/chat", post(send_chat))
        .route("/api/console/sessions", post(create_console_session))
        .route(
            "/api/console/sessions/:session_id",
            delete(close_console_session),
        )
        .route(
            "/api/console/sessions/:session_id/input",
            post(write_console_session),
        )
        .route(
            "/api/console/sessions/:session_id/resize",
            post(resize_console_session),
        )
        .route("/api/chat/:conversation_id/cancel", post(cancel_chat))
        .route(
            "/api/chat/:conversation_id/cancel-jobs",
            post(cancel_background_jobs),
        )
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
        .route("/api/webhooks/:src/runs/:run_id", get(get_webhook_run))
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
            axum::routing::patch(patch_webhook_source).delete(clear_webhook_source_token),
        )
        // Phase 5: cron job management for the scheduler.
        .route("/api/cron-jobs", get(list_cron_jobs).post(create_cron_job))
        .route(
            "/api/cron-jobs/delivery-targets",
            get(list_cron_delivery_targets),
        )
        .route(
            "/api/cron-jobs/:job_id",
            axum::routing::patch(update_cron_job).delete(delete_cron_job),
        )
        .route("/api/tools/:tool_call_id/approve", post(approve_tool_call))
        .route("/api/tools/:tool_call_id/ask-user", post(submit_ask_user))
        .route(
            "/api/terminal-input/:request_id/submit",
            post(submit_terminal_input),
        )
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

    let compression = CompressionLayer::new().compress_when(
        SizeAbove::new(1024)
            .and(NotForContentType::GRPC)
            .and(NotForContentType::IMAGES)
            .and(NotForContentType::SSE)
            // Media and archives are already compressed; skip redundant CPU work.
            .and(NotForContentType::const_new("application/pdf"))
            .and(NotForContentType::const_new("application/zip"))
            .and(NotForContentType::const_new("application/gzip"))
            .and(NotForContentType::const_new("application/x-gzip"))
            .and(NotForContentType::const_new("application/x-7z-compressed"))
            .and(NotForContentType::const_new("application/x-rar-compressed"))
            .and(NotForContentType::const_new("audio/"))
            .and(NotForContentType::const_new("video/")),
    );

    let static_dir = resolve_static_dir();
    let mut app = maybe_with_static_files(app, static_dir.clone())
        // `CompressionLayer` honors `Accept-Encoding`; event streams and already
        // compressed media are explicitly excluded above.
        .layer(compression)
        .layer(DefaultBodyLimit::max(20 * 1024 * 1024));
    if let Some(cors) = build_cors_layer()? {
        app = app.layer(cors);
    }
    let app = app
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
        log::info!(
            "pointer-server: web branding title={:?} composer_placeholder={:?} welcome_tip_title={:?} turn_elapsed_active={:?} turn_elapsed_done={:?}",
            resolve_web_page_title(),
            resolve_composer_placeholder(),
            resolve_optional_branding_env("POINTER_SERVER_WELCOME_TIP_TITLE"),
            resolve_optional_branding_env("POINTER_SERVER_TURN_ELAPSED_ACTIVE"),
            resolve_optional_branding_env("POINTER_SERVER_TURN_ELAPSED_DONE"),
        );
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

async fn get_settings(
    State(state): State<ServerState>,
) -> Result<Json<WebEffectiveSettingsView>, ApiError> {
    require_platform_access(&state)?;
    Ok(Json(WebEffectiveSettingsView(
        state.core.effective_settings_view(),
    )))
}

async fn update_user_settings(
    State(state): State<ServerState>,
    Json(user): Json<UserSettings>,
) -> Result<Json<WebEffectiveSettingsView>, ApiError> {
    require_platform_access(&state)?;
    let view = state.core.update_user_settings(user).map_err(ApiError)?;
    Ok(Json(WebEffectiveSettingsView(view)))
}

async fn update_platform_settings(
    State(_state): State<ServerState>,
    Json(_platform): Json<PlatformSettings>,
) -> Result<Json<WebEffectiveSettingsView>, ApiError> {
    Err(ApiError(anyhow::anyhow!(
        "web runtime: platform settings are read-only"
    )))
}

async fn update_debug_session_settings(
    State(state): State<ServerState>,
    Json(settings): Json<DebugSessionSettings>,
) -> Result<Json<DebugSessionSettings>, ApiError> {
    require_platform_access(&state)?;
    let view = state
        .core
        .update_debug_session_settings(settings)
        .map_err(|error| {
            log::warn!("debug_session_settings: web update failed: {error:#}");
            ApiError(error)
        })?;
    let mut response = DebugSessionSettings::from(&view.merged);
    pointer_core::models::redact_debug_session_settings_for_web(&mut response);
    Ok(Json(response))
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
    state.core.init_launch()?;
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

// ---- Plugin management API (P1) ----

async fn list_plugins(State(state): State<ServerState>) -> Result<Json<Vec<PluginView>>, ApiError> {
    require_platform_access(&state)?;
    let views = state
        .core
        .plugin_list()
        .iter()
        .map(|r| {
            let mut v = PluginView::from_record(r);
            // P2③：MCP 连续重启失败进入 degraded（UI 插件状态显示）。
            if state.core.mcp_sessions.is_degraded(&r.id) {
                v.status = "degraded".to_string();
            }
            v
        })
        .collect();
    Ok(Json(views))
}

// ---- Global MCP management API (P2b) ----

async fn list_global_mcp(
    State(state): State<ServerState>,
) -> Result<Json<GlobalMcpView>, ApiError> {
    require_platform_access(&state)?;
    Ok(Json(state.core.global_mcp_view()))
}

/// P2b：界面直接配置——保存全局 MCP server 列表（user_settings 持久化 + 热重载）。
async fn save_global_mcp(
    State(state): State<ServerState>,
    Json(servers): Json<Vec<McpServerDecl>>,
) -> Result<Json<GlobalMcpView>, ApiError> {
    require_platform_access(&state)?;
    let view = state.core.save_global_mcp_servers(servers)?;
    Ok(Json(view))
}

async fn reload_global_mcp(
    State(state): State<ServerState>,
) -> Result<Json<GlobalMcpView>, ApiError> {
    require_platform_access(&state)?;
    let view = state.core.reload_global_mcp_from_config()?;
    Ok(Json(view))
}

async fn restart_global_mcp(
    State(state): State<ServerState>,
) -> Result<Json<GlobalMcpView>, ApiError> {
    require_platform_access(&state)?;
    // 用当前配置全量重启（关旧 → 注销工具 → 重连）。
    let (decls, base_dir) = {
        let cfg = state.core.global_mcp.read();
        (cfg.decls.clone(), cfg.base_dir.clone())
    };
    state.core.reload_global_mcp(decls, base_dir)?;
    Ok(Json(state.core.global_mcp_view()))
}

#[derive(Deserialize)]
struct PluginImportBody {
    /// 源插件目录（自动识别 Pointer / Codex / Claude 候选）或 .zip 文件的绝对路径。
    source: String,
}

async fn import_plugin(
    State(state): State<ServerState>,
    Json(body): Json<PluginImportBody>,
) -> Result<Json<Vec<pointer_core::plugins::importer::ImportReport>>, ApiError> {
    require_platform_access(&state)?;
    let source = std::path::PathBuf::from(body.source.trim());
    Ok(Json(state.core.plugin_import(&source)?))
}

/// 上传 zip 字节导入插件（web 端文件上传；目录导入走 POST /api/plugins）。
async fn import_plugin_zip(
    State(state): State<ServerState>,
    body: axum::body::Bytes,
) -> Result<Json<Vec<pointer_core::plugins::importer::ImportReport>>, ApiError> {
    require_platform_access(&state)?;
    Ok(Json(state.core.plugin_import_zip(&body)?))
}

#[derive(Deserialize)]
struct PluginDiscoverBody {
    /// 用户选择的顶层目录（自动发现其中的插件候选）。
    dir: String,
}

async fn discover_plugins(
    State(state): State<ServerState>,
    Json(body): Json<PluginDiscoverBody>,
) -> Result<Json<Vec<pointer_core::plugins::importer::DiscoveredPlugin>>, ApiError> {
    require_platform_access(&state)?;
    let dir = std::path::PathBuf::from(body.dir.trim());
    Ok(Json(state.core.plugin_discover(&dir)?))
}

async fn enable_plugin(
    State(state): State<ServerState>,
    Path(plugin_id): Path<String>,
) -> Result<Json<PluginView>, ApiError> {
    require_platform_access(&state)?;
    state.core.plugin_enable(&plugin_id)?;
    let record = state
        .core
        .plugins
        .get(&plugin_id)
        .ok_or_else(|| ApiError(anyhow::anyhow!("插件不存在: {plugin_id}")))?;
    Ok(Json(PluginView::from_record(&record)))
}

async fn disable_plugin(
    State(state): State<ServerState>,
    Path(plugin_id): Path<String>,
) -> Result<Json<PluginView>, ApiError> {
    require_platform_access(&state)?;
    state.core.plugin_disable(&plugin_id)?;
    let record = state
        .core
        .plugins
        .get(&plugin_id)
        .ok_or_else(|| ApiError(anyhow::anyhow!("插件不存在: {plugin_id}")))?;
    Ok(Json(PluginView::from_record(&record)))
}

async fn uninstall_plugin(
    State(state): State<ServerState>,
    Path(plugin_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    require_platform_access(&state)?;
    state.core.plugin_uninstall(&plugin_id)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn probe_external_plugins(
    State(state): State<ServerState>,
) -> Result<Json<pointer_core::plugins::external_probe::ExternalPluginsProbeResult>, ApiError> {
    require_platform_access(&state)?;
    Ok(Json(state.core.plugin_probe_external()?))
}

#[derive(Deserialize)]
struct ImportExternalPluginBody {
    #[serde(rename = "sourceId")]
    source_id: String,
}

async fn import_external_plugin(
    State(state): State<ServerState>,
    Json(body): Json<ImportExternalPluginBody>,
) -> Result<Json<pointer_core::plugins::importer::ImportReport>, ApiError> {
    require_platform_access(&state)?;
    Ok(Json(state.core.plugin_import_external(&body.source_id)?))
}

async fn list_tools(State(state): State<ServerState>) -> Result<Json<Vec<ToolDef>>, ApiError> {
    require_platform_access(&state)?;
    Ok(Json(state.core.tools.list_defs()))
}

async fn list_agents(State(state): State<ServerState>) -> Result<Json<Vec<AgentDef>>, ApiError> {
    require_platform_access(&state)?;
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
    require_platform_access(&state)?;
    use pointer_core::task_board::resolve_store_key_for_read;
    let parent_key = state
        .core
        .get_active_main_task_board_key(&q.conversation_id)
        .unwrap_or_else(|| q.conversation_id.clone());
    let store_key = resolve_store_key_for_read(
        state.core.task_board_store.as_ref(),
        &q.conversation_id,
        q.task_id.as_deref(),
        &parent_key,
    );
    Ok(Json(
        state.core.task_board_store.document(&store_key).to_value(),
    ))
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
            image_mime: pointer_core::agents::computer::screen::image_data_url_mime(&img)
                .to_string(),
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

async fn manual_computer_snapshot(State(state): State<ServerState>) -> Result<Response, ApiError> {
    require_platform_access(&state)?;
    let jpeg = capture_debug::capture_manual_desktop_snapshot_jpeg().map_err(ApiError::from)?;
    let mut response = Response::new(jpeg.into());
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static("image/jpeg"));
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static("inline"),
    );
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

async fn api_version() -> Json<serde_json::Value> {
    let version = pointer_core::client_env::app_version();
    Json(serde_json::json!({"version": version}))
}

#[derive(Deserialize)]
struct ChatMediaQuery {
    #[serde(rename = "storageRelPath")]
    storage_rel_path: String,
    /// Optional display name for Content-Disposition (UTF-8 via filename*).
    #[serde(default, rename = "fileName")]
    file_name: Option<String>,
}

async fn preview_chat_media(
    State(state): State<ServerState>,
    Query(q): Query<ChatMediaQuery>,
) -> Result<Json<ChatMediaPreview>, ApiError> {
    require_platform_access(&state)?;
    Ok(Json(
        pointer_core::media::read_chat_media_preview(&q.storage_rel_path)
            .map_err(ApiError::from)?,
    ))
}

#[derive(Deserialize)]
struct MediaRefQuery {
    #[serde(rename = "mediaRef")]
    media_ref: String,
    /// Optional display name for Content-Disposition (UTF-8 via filename*).
    #[serde(default, rename = "fileName")]
    file_name: Option<String>,
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

async fn download_media_ref(
    State(state): State<ServerState>,
    Query(q): Query<MediaRefQuery>,
) -> Result<Response, ApiError> {
    require_platform_access(&state)?;
    let (path, mime_type, stored_name) =
        pointer_core::media::chat_media_ref_file_meta(&q.media_ref).map_err(ApiError::from)?;
    let file_name = preferred_download_file_name(q.file_name.as_deref(), &stored_name);
    log::info!(
        "media-ref-download start ref={} file={} path={}",
        q.media_ref,
        file_name,
        path.display()
    );
    stream_media_file_response(&path, &mime_type, &file_name, false).await
}

#[derive(Deserialize)]
struct PublicMediaDownloadQuery {
    token: String,
}

/// Unauthenticated time-limited download for IM large-file links (HMAC token).
async fn public_media_download(
    Query(q): Query<PublicMediaDownloadQuery>,
) -> Result<Response, ApiError> {
    let verified = pointer_core::media::verify_and_resolve_download(&q.token).map_err(|e| {
        let msg = e.to_string();
        log::warn!("public media download rejected: {msg}");
        ApiError(e)
    })?;
    log::info!(
        "public media download stream file={} path={}",
        verified.file_name,
        verified.path.display()
    );
    let mut response = stream_media_file_response(
        &verified.path,
        &verified.mime_type,
        &verified.file_name,
        false,
    )
    .await?;
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}

fn preferred_download_file_name(preferred: Option<&str>, stored: &str) -> String {
    let preferred = preferred.map(str::trim).filter(|s| !s.is_empty());
    preferred.unwrap_or(stored).to_string()
}

fn attachment_content_disposition(file_name: &str, inline: bool) -> HeaderValue {
    let kind = if inline { "inline" } else { "attachment" };
    let safe: String = file_name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let fallback = if safe.is_empty() {
        "attachment".into()
    } else {
        safe
    };
    // RFC 5987 so browsers keep original UTF-8 names (e.g. Chinese filenames).
    let encoded = urlencoding::encode(file_name);
    let value = format!("{kind}; filename=\"{fallback}\"; filename*=UTF-8''{encoded}");
    HeaderValue::from_str(&value).unwrap_or_else(|_| HeaderValue::from_static("attachment"))
}

/// Stream a file body so large downloads start before the whole file is in RAM.
async fn stream_media_file_response(
    path: &std::path::Path,
    mime_type: &str,
    file_name: &str,
    inline: bool,
) -> Result<Response, ApiError> {
    let file = tokio::fs::File::open(path)
        .await
        .map_err(|e| ApiError(anyhow::anyhow!("open media {}: {e}", path.display())))?;
    let len = match file.metadata().await {
        Ok(meta) => Some(meta.len()),
        Err(e) => {
            log::warn!(
                "media metadata failed path={} err={e}; streaming without Content-Length",
                path.display()
            );
            None
        }
    };
    let body = axum::body::Body::from_stream(tokio_util::io::ReaderStream::new(file));
    let mut response = Response::new(body);
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(mime_type)
            .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        attachment_content_disposition(file_name, inline),
    );
    if let Some(len) = len {
        if let Ok(v) = HeaderValue::from_str(&len.to_string()) {
            response.headers_mut().insert(header::CONTENT_LENGTH, v);
        }
    }
    Ok(response)
}

async fn download_chat_media(
    State(state): State<ServerState>,
    Query(q): Query<ChatMediaQuery>,
) -> Result<Response, ApiError> {
    require_platform_access(&state)?;
    let (path, mime_type, stored_name) =
        pointer_core::media::chat_media_file_meta(&q.storage_rel_path).map_err(ApiError::from)?;
    let file_name = preferred_download_file_name(q.file_name.as_deref(), &stored_name);
    log::info!(
        "chat media download stream path={} file={}",
        path.display(),
        file_name
    );
    stream_media_file_response(&path, &mime_type, &file_name, false).await
}

async fn stream_chat_media(
    State(state): State<ServerState>,
    Query(q): Query<ChatMediaQuery>,
) -> Result<Response, ApiError> {
    require_platform_access(&state)?;
    let (path, mime_type, stored_name) =
        pointer_core::media::chat_media_file_meta(&q.storage_rel_path).map_err(ApiError::from)?;
    let file_name = preferred_download_file_name(q.file_name.as_deref(), &stored_name);
    stream_media_file_response(&path, &mime_type, &file_name, true).await
}

#[derive(serde::Serialize)]
struct SaveChatAttachmentResponse {
    #[serde(rename = "storageRelPath")]
    storage_rel_path: String,
}

/// `POST /api/chat/save-attachment` — multipart fields:
/// `conversationId`, `attachmentId`, `fileName` (optional if file has filename), `file`.
async fn save_chat_attachment(
    State(state): State<ServerState>,
    mut multipart: Multipart,
) -> Result<Json<SaveChatAttachmentResponse>, ApiError> {
    require_platform_access(&state)?;
    let mut conversation_id = String::new();
    let mut attachment_id = String::new();
    let mut file_name = String::new();
    let mut file_bytes: Option<Vec<u8>> = None;

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
            Some("file") => {
                if file_name.is_empty() {
                    if let Some(name) = field.file_name().map(str::to_string) {
                        file_name = name;
                    }
                }
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

    if conversation_id.is_empty() || attachment_id.is_empty() {
        return Err(ApiError(anyhow::anyhow!(
            "conversationId and attachmentId required"
        )));
    }
    if file_name.is_empty() {
        return Err(ApiError(anyhow::anyhow!(
            "fileName required (field or multipart filename)"
        )));
    }
    let bytes = file_bytes.ok_or_else(|| ApiError(anyhow::anyhow!("file field required")))?;
    pointer_core::media::ensure_composer_attachment_size(bytes.len() as u64, &file_name)
        .map_err(ApiError::from)?;
    let uid = state
        .core
        .active_platform_auth()
        .platform_user_id()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| ApiError(anyhow::anyhow!("platform_login_required")))?;
    state
        .core
        .session_index
        .ensure_session_user_id(&conversation_id, &uid)
        .map_err(ApiError::from)?;
    log::info!(
        "save-attachment multipart conv={} id={} name={} bytes={}",
        conversation_id,
        attachment_id,
        file_name,
        bytes.len()
    );
    let storage_rel_path = pointer_core::media::save_attachment_bytes(
        &conversation_id,
        &attachment_id,
        &bytes,
        &file_name,
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
        return Err(ApiError(anyhow::anyhow!(
            "attachmentId and fileName required"
        )));
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
                log::info!("upload-video-oss {attachment_id_log}: {pct}% ({loaded}/{total})");
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

/// Current browser session user id (SSO `sub` / OAuth id / `local-admin`), or empty.
fn platform_session_user_id(state: &ServerState) -> String {
    state
        .core
        .active_platform_auth()
        .platform_user_id()
        .unwrap_or_default()
}

/// Sidebar / project list visibility: platform admin sees all users' rows.
fn platform_list_scope(state: &ServerState) -> pointer_core::conversation_store::ListScope {
    let auth = state.core.active_platform_auth();
    pointer_core::conversation_store::ListScope::from_viewer(
        auth.is_platform_admin(),
        &auth.platform_user_id().unwrap_or_default(),
    )
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
    let conversations = storage::load_conversations()?;
    let messages: usize = conversations.iter().map(|c| c.messages.len()).sum();
    log::info!(
        "server: load_conversations conversations={} messages={messages}",
        conversations.len()
    );
    Ok(Json(conversations))
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

/// `GET /api/conversations/:id/meta` — single conversation meta (ListScope-aware).
/// Returns `null` when missing or outside the caller's sidebar visibility scope.
async fn load_conversation_meta_handler(
    State(state): State<ServerState>,
    Path(conversation_id): Path<String>,
) -> Result<Json<Option<pointer_core::models::ConversationMeta>>, ApiError> {
    require_platform_access(&state)?;
    let scope = platform_list_scope(&state);
    let meta = storage::load_conversation_meta(&scope, &conversation_id)?;
    log::info!(
        "server: load_conversation_meta id={} found={}",
        conversation_id,
        meta.is_some()
    );
    Ok(Json(meta))
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
    let scope = platform_list_scope(&state);
    let metas = storage::load_conversation_metas(&scope, cursor, limit)?;
    log::info!(
        "server: load_conversation_metas cursor={} limit={} returned {} rows",
        cursor_dbg,
        limit,
        metas.len()
    );
    Ok(Json(metas))
}

#[derive(Deserialize)]
struct ProjectsQuery {
    cursor_last_activity_at: Option<i64>,
    cursor_id: Option<String>,
    limit: Option<i64>,
}

async fn load_sidebar_projects(
    State(state): State<ServerState>,
) -> Result<Json<Vec<pointer_core::models::Project>>, ApiError> {
    require_platform_access(&state)?;
    let scope = platform_list_scope(&state);
    Ok(Json(storage::load_sidebar_projects(&scope)?))
}

async fn load_projects(
    State(state): State<ServerState>,
    Query(q): Query<ProjectsQuery>,
) -> Result<Json<pointer_core::models::ProjectPage>, ApiError> {
    require_platform_access(&state)?;
    let scope = platform_list_scope(&state);
    let cursor = match (q.cursor_last_activity_at, q.cursor_id) {
        (Some(last_activity_at), Some(id)) => Some(pointer_core::models::ProjectCursor {
            last_activity_at,
            id,
        }),
        (None, None) => None,
        _ => {
            return Err(ApiError::from(anyhow::anyhow!(
                "project cursor fields must both be set or omitted"
            )))
        }
    };
    Ok(Json(storage::load_projects(
        &scope,
        cursor,
        q.limit.unwrap_or(20),
    )?))
}

#[derive(Deserialize)]
struct CreateProjectRequest {
    name: String,
    workspace_root: String,
}

async fn create_project(
    State(state): State<ServerState>,
    Json(input): Json<CreateProjectRequest>,
) -> Result<Json<pointer_core::models::ProjectCreationResult>, ApiError> {
    require_platform_access(&state)?;
    let uid = platform_session_user_id(&state);
    Ok(Json(storage::create_project(
        &input.name,
        &input.workspace_root,
        &uid,
    )?))
}

async fn load_project(
    State(state): State<ServerState>,
    Path(project_id): Path<String>,
) -> Result<Json<Option<pointer_core::models::Project>>, ApiError> {
    require_platform_access(&state)?;
    let scope = platform_list_scope(&state);
    Ok(Json(storage::load_project(&project_id, &scope)?))
}

#[derive(Deserialize)]
struct UpdateProjectRequest {
    name: Option<String>,
    workspace_root: Option<String>,
    is_pinned: Option<bool>,
    is_archived: Option<bool>,
}

async fn update_project(
    State(state): State<ServerState>,
    Path(project_id): Path<String>,
    Json(input): Json<UpdateProjectRequest>,
) -> Result<Json<pointer_core::models::Project>, ApiError> {
    require_platform_access(&state)?;
    let uid = platform_session_user_id(&state);
    Ok(Json(storage::update_project(
        &project_id,
        &uid,
        input.name.as_deref(),
        input.workspace_root.as_deref(),
        input.is_pinned,
        input.is_archived,
    )?))
}

async fn delete_project(
    State(state): State<ServerState>,
    Path(project_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    require_platform_access(&state)?;
    let uid = platform_session_user_id(&state);
    storage::delete_project(&project_id, &uid)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn load_project_conversation_metas(
    State(state): State<ServerState>,
    Path(project_id): Path<String>,
    Query(q): Query<ConversationMetasQuery>,
) -> Result<Json<Vec<pointer_core::models::ConversationMeta>>, ApiError> {
    require_platform_access(&state)?;
    let scope = platform_list_scope(&state);
    let cursor = match (q.cursor_updated_at, q.cursor_id) {
        (Some(ts), Some(id)) => Some((ts, id)),
        (None, None) => None,
        _ => {
            return Err(ApiError::from(anyhow::anyhow!(
                "cursor fields must both be set or omitted"
            )))
        }
    };
    Ok(Json(storage::load_project_conversation_metas(
        &project_id,
        &scope,
        cursor,
        q.limit.unwrap_or(20),
    )?))
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
    let scope = platform_list_scope(&state);
    let hits = storage::search_conversations(&scope, &q.q, limit)?;
    log::info!(
        "server: search_conversations q={:?} limit={} returned {} rows",
        q.q.trim(),
        limit,
        hits.len()
    );
    Ok(Json(hits))
}

#[derive(serde::Deserialize)]
struct ConversationSearchMatchesQuery {
    q: String,
}

async fn list_conversation_search_matches_handler(
    State(state): State<ServerState>,
    Path(conversation_id): Path<String>,
    Query(q): Query<ConversationSearchMatchesQuery>,
) -> Result<Json<Vec<pointer_core::models::ConversationSearchMatch>>, ApiError> {
    require_platform_access(&state)?;
    let scope = platform_list_scope(&state);
    let matches = storage::list_conversation_search_matches(&scope, &conversation_id, &q.q)?;
    Ok(Json(matches))
}

async fn list_conversation_outline_handler(
    State(state): State<ServerState>,
    Path(conversation_id): Path<String>,
) -> Result<Json<Vec<pointer_core::models::ConversationOutlineItem>>, ApiError> {
    require_platform_access(&state)?;
    let scope = platform_list_scope(&state);
    let items = storage::list_conversation_outline(&scope, &conversation_id)?;
    log::info!(
        "server: list_conversation_outline conversation_id={} returned {} rows",
        conversation_id,
        items.len()
    );
    Ok(Json(items))
}

/// `GET /api/conversations/:id/messages` — full list when no page query params;
/// turn window (`MessagePage`) when `limitTurns` / `beforePosition` /
/// `afterPosition` / `aroundMessageId` set.
#[derive(Deserialize)]
struct ConversationMessagesQuery {
    #[serde(default, rename = "limitTurns")]
    limit_turns: Option<u32>,
    #[serde(default, rename = "beforePosition")]
    before_position: Option<i64>,
    #[serde(default, rename = "afterPosition")]
    after_position: Option<i64>,
    #[serde(default, rename = "aroundMessageId")]
    around_message_id: Option<String>,
    #[serde(default, rename = "includeScopedSubMessages")]
    include_scoped_sub_messages: Option<bool>,
}

fn conversation_messages_wants_page(q: &ConversationMessagesQuery) -> bool {
    q.limit_turns.is_some()
        || q.before_position.is_some()
        || q.after_position.is_some()
        || q.around_message_id
            .as_ref()
            .is_some_and(|s| !s.trim().is_empty())
}

async fn load_conversation_messages_handler(
    State(state): State<ServerState>,
    Path(conversation_id): Path<String>,
    Query(q): Query<ConversationMessagesQuery>,
) -> Result<Response, ApiError> {
    require_platform_access(&state)?;
    if conversation_messages_wants_page(&q) {
        let opts = pointer_core::conversation_store::LoadMessagesPageOpts {
            limit_turns: q.limit_turns,
            before_position: q.before_position,
            after_position: q.after_position,
            around_message_id: q.around_message_id,
            include_scoped_sub_messages: q.include_scoped_sub_messages.unwrap_or(false),
        };
        let page = storage::load_conversation_messages_page(&conversation_id, &opts)?;
        log::info!(
            "server: load_conversation_messages_page conversation_id={} returned {} rows total={}",
            conversation_id,
            page.messages.len(),
            page.message_count
        );
        return Ok(Json(page).into_response());
    }
    let messages = storage::load_conversation_messages(&conversation_id)?;
    log::info!(
        "server: load_conversation_messages conversation_id={} returned {} rows",
        conversation_id,
        messages.len()
    );
    Ok(Json(messages).into_response())
}

#[derive(Deserialize)]
struct ScopedSubMessagesQuery {
    #[serde(default, rename = "anchorMessageId")]
    anchor_message_id: String,
    #[serde(default, rename = "traceId")]
    trace_id: String,
    #[serde(default, rename = "agentInstanceId")]
    agent_instance_id: Option<String>,
}

async fn load_scoped_sub_messages_handler(
    State(state): State<ServerState>,
    Path(conversation_id): Path<String>,
    Query(q): Query<ScopedSubMessagesQuery>,
) -> Result<Response, ApiError> {
    require_platform_access(&state)?;
    let instance = q
        .agent_instance_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let messages = storage::load_scoped_sub_messages_for_trace(
        &conversation_id,
        q.anchor_message_id.trim(),
        q.trace_id.trim(),
        instance,
    )?;
    Ok(Json(messages).into_response())
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
) -> Result<Json<Vec<pointer_core::conversation_store::AppendedMessageRow>>, ApiError> {
    require_platform_access(&state)?;
    let appended = storage::append_conversation_messages(&conversation_id, &messages)?;
    Ok(Json(appended))
}

async fn send_chat(
    State(state): State<ServerState>,
    headers: HeaderMap,
    Json(payload): Json<SendChatPayload>,
) -> Result<StatusCode, ApiError> {
    require_platform_access(&state)?;
    let web_session_auth =
        web_session::lookup_session_auth(&state.web_sessions, &headers).or_else(|| {
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
        agent_skill_overrides: payload.agent_skill_overrides,
        agent_mode: payload.agent_mode,
        lead_agent_id: payload.lead_agent_id,
        performance_mode: payload.performance_mode,
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
///
/// Optional hermes-style `"deliver": "feishu:ou_xxx"` string in the JSON body
/// is applied into `trigger_meta.extra.deliver` (structured `DeliverTarget` in
/// the body is ignored / forced to the string-derived marker).
async fn create_run(
    State(state): State<ServerState>,
    headers: HeaderMap,
    Json(mut raw): Json<serde_json::Value>,
) -> Result<(StatusCode, Json<RunHandle>), ApiError> {
    require_platform_access(&state)?;
    let deliver_str = match raw.get("deliver") {
        Some(serde_json::Value::String(s)) => Some(s.clone()),
        _ => None,
    };
    // TriggerRequest.deliver is a tagged enum; replace string with None so
    // serde succeeds, then apply the hermes string via apply_deliver_string.
    if deliver_str.is_some() {
        raw["deliver"] = serde_json::json!({ "kind": "none" });
    }
    let mut body: TriggerRequest = serde_json::from_value(raw)
        .map_err(|e| ApiError(anyhow::anyhow!("invalid run request body: {e}")))?;
    body.trigger_source = TriggerSource::HttpRuns;
    body.trigger_meta.webhook_source = None;
    body.deliver = pointer_core::dispatcher::apply_deliver_string(
        &mut body.trigger_meta,
        deliver_str.as_deref(),
    );
    body.web_session_auth = web_session::lookup_session_auth(&state.web_sessions, &headers)
        .or_else(|| {
            pointer_core::web_request_auth::capture_web_session_auth(&state.core.platform_auth)
        });
    let handle = state
        .dispatcher
        .dispatch(body)
        .await
        .map_err(ApiError::from)?;
    log::info!(
        "runs-api: accepted run_id={} conv={} status={:?} deliver={:?}",
        handle.run_id,
        handle.reused_run_id.as_deref().unwrap_or("new"),
        handle.status,
        deliver_str,
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
) -> Result<Response, ApiError> {
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

    Ok(sse_with_proxy_hints(
        Sse::new(stream).keep_alive(KeepAlive::default()),
    ))
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
            log::warn!("webhook blocking: timed out run_id={run_id} after {timeout_secs}s");
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
            let text =
                pointer_core::webhook_result::last_assistant_text(&state.core.session_index, &conv)
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
fn webhook_auth_status(
    err: pointer_core::webhook_config::WebhookIngressAuthError,
) -> (StatusCode, String) {
    use pointer_core::webhook_config::WebhookIngressAuthError;
    match err {
        WebhookIngressAuthError::InvalidSrc(msg) => (StatusCode::BAD_REQUEST, msg),
        WebhookIngressAuthError::NotConfigured => (StatusCode::UNAUTHORIZED, err.to_string()),
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
    let normalized_src =
        match pointer_core::webhook_config::authorize_webhook_ingress(&store, &src, &headers) {
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

    let bytes =
        file_bytes.ok_or_else(|| ApiError(anyhow::anyhow!("multipart field `file` required")))?;
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
    let normalized_src =
        match pointer_core::webhook_config::authorize_webhook_ingress(&store, &src, &headers) {
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
            .resolve_webhook_ingress_session(&normalized_src, Some(delivery_id.as_str()))
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
    if let Some(user_msg) = pointer_core::webhook_ingress::last_inbound_user_message(&messages) {
        pointer_core::stream_broadcast::broadcast_stream(&StreamEvent::InjectedUserMessage {
            conversation_id: conversation_id.clone(),
            message_id: user_msg.id.clone(),
            content: user_msg.content.clone(),
            attachments: user_msg.attachments.clone(),
            ui_bindings: user_msg.ui_bindings.clone(),
        });
    }

    let mut trigger_meta = TriggerMeta {
        webhook_source: Some(normalized_src.clone()),
        ..TriggerMeta::empty()
    };
    let deliver =
        pointer_core::dispatcher::apply_deliver_string(&mut trigger_meta, body.deliver.as_deref());
    let req = TriggerRequest {
        run_id: None,
        idempotency_key: body.idempotency_key,
        conversation_id: Some(conversation_id.clone()),
        trigger_source: TriggerSource::Webhook,
        trigger_meta,
        lane: None,
        messages,
        enabled_skill_ids: Vec::new(),
        // Empty → run_chat loads agentSkillOverrides from user_settings.
        agent_skill_overrides: HashMap::new(),
        agent_mode: body.agent_mode,
        lead_agent_id: body.lead_agent_id,
        performance_mode: None,
        tool_rounds_used_single_start: 0,
        tool_rounds_used_supervisor_start: 0,
        workspace_root: body.workspace_root,
        workspace_inherit_disabled: None,
        deliver,
        web_session_auth: {
            sync_automation_web_session(&state);
            state.core.automation_execution_auth()
        },
    };

    let handle = state
        .dispatcher
        .dispatch(req)
        .await
        .map_err(ApiError::from)?;
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
    #[serde(default, rename = "cronExpr")]
    cron_expr: String,
    /// Friendly / one-shot schedule; preferred when set.
    #[serde(default)]
    schedule: Option<String>,
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
    /// Optional Run → IM delivery spec (e.g. "feishu", "feishu:ou_xxx",
    /// comma-separated, "all"). Empty / None = no IM push after the run.
    #[serde(default)]
    deliver: Option<String>,
}

fn default_true() -> bool {
    true
}

/// `POST /api/cron-jobs` — create a new scheduled job. Accepts friendly
/// `schedule` (including one-shot `30m` / ISO) or raw `cronExpr`.
async fn create_cron_job(
    State(state): State<ServerState>,
    Json(body): Json<CreateCronJobBody>,
) -> Result<
    (
        StatusCode,
        Json<pointer_core::conversation_store::cron_jobs::CronJobView>,
    ),
    ApiError,
> {
    require_platform_access(&state)?;
    let schedule_input = body
        .schedule
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(body.cron_expr.trim());
    if schedule_input.is_empty() {
        return Err(ApiError(anyhow::anyhow!(
            "schedule or cronExpr is required"
        )));
    }
    let parsed = pointer_core::tools::cron_job::schedule::parse_schedule(schedule_input)
        .map_err(ApiError::from)?;
    let (cron_expr, schedule_kind, next_override) = match parsed {
        pointer_core::tools::cron_job::schedule::ParsedSchedule::Recurring { cron_expr } => {
            if pointer_core::conversation_store::cron_jobs::next_run_ms(
                &cron_expr,
                &chrono::Local::now(),
            )
            .is_none()
            {
                return Err(ApiError(anyhow::anyhow!(
                    "invalid cron expression: {cron_expr}"
                )));
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
    let deliver = body
        .deliver
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .and_then(|s| pointer_channels::im_delivery::normalize_deliver_spec(s));
    if let Err(e) = pointer_channels::im_delivery::validate_deliver_spec(
        deliver.as_deref(),
        &state.channel_gateway.config(),
    ) {
        return Err(ApiError(anyhow::anyhow!(e)));
    }
    let new = pointer_core::conversation_store::cron_jobs::NewCronJob {
        id: &body.id,
        label: &body.label,
        cron_expr: &cron_expr,
        schedule_kind,
        schedule_raw: Some(schedule_input),
        next_run_at_ms: next_override,
        conversation_id: &body.conversation_id,
        prompt_text: &body.prompt_text,
        agent_mode: body.agent_mode.as_deref(),
        lead_agent_id: body.lead_agent_id.as_deref(),
        enabled: body.enabled,
        deliver: deliver.as_deref(),
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
        .ok_or_else(|| {
            ApiError(anyhow::anyhow!(
                "cron job vanished after insert: {}",
                body.id
            ))
        })?;
    log::info!(
        "cron-jobs: created id={} label={} kind={} expr={}",
        body.id,
        body.label,
        schedule_kind,
        cron_expr
    );
    Ok((
        StatusCode::CREATED,
        Json(pointer_core::conversation_store::cron_jobs::CronJobView::from_record(&rec)),
    ))
}

#[derive(Deserialize)]
struct UpdateCronJobBody {
    /// Toggles the job enabled flag.
    enabled: Option<bool>,
    /// Optional Run → IM deliver spec. Pass empty string to clear.
    deliver: Option<String>,
}

/// `GET /api/cron-jobs/delivery-targets` — home-channel targets for the UI dropdown.
async fn list_cron_delivery_targets(
    State(state): State<ServerState>,
) -> Result<Json<Vec<pointer_channels::im_delivery::DeliveryTargetInfo>>, ApiError> {
    require_platform_access(&state)?;
    let cfg = state.channel_gateway.config().clone();
    Ok(Json(
        pointer_channels::im_delivery::list_home_delivery_targets(&cfg),
    ))
}

/// `PATCH /api/cron-jobs/:id` — toggle enable/disable and/or update deliver.
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
    if let Some(ref deliver) = body.deliver {
        let deliver = if deliver.trim().is_empty() {
            None
        } else {
            pointer_channels::im_delivery::normalize_deliver_spec(deliver)
        };
        if let Err(e) = pointer_channels::im_delivery::validate_deliver_spec(
            deliver.as_deref(),
            &state.channel_gateway.config(),
        ) {
            return Err(ApiError(anyhow::anyhow!(e)));
        }
        let ok = state
            .core
            .session_index
            .cron_jobs_update_deliver(&job_id, deliver.as_deref())
            .map_err(ApiError::from)?;
        if !ok {
            return Ok(status_text(
                StatusCode::NOT_FOUND,
                format!("cron job not found: {job_id}"),
            ));
        }
    }
    match state
        .core
        .session_index
        .cron_jobs_get(&job_id)
        .map_err(ApiError::from)?
    {
        Some(rec) => Ok(Json(
            pointer_core::conversation_store::cron_jobs::CronJobView::from_record(&rec),
        )
        .into_response()),
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
    let normalized_src =
        match pointer_core::webhook_config::authorize_webhook_ingress(&store, &src, &headers) {
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
            store.source_view(src, token, url).map_err(ApiError::from)
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
    let Some(token) = store
        .reveal_source_token(&normalized)
        .map_err(ApiError::from)?
    else {
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
        let mode =
            pointer_core::conversation_store::webhook_sources::WebhookSessionMode::parse(&mode_raw)
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

#[derive(Deserialize, Default)]
struct CancelChatPayload {
    /// Default true (Stop). Force-send / end-wait pass false to keep background jobs.
    #[serde(default = "default_true", rename = "cancelBackgroundJobs")]
    cancel_background_jobs: bool,
}

async fn cancel_chat(
    State(state): State<ServerState>,
    Path(conversation_id): Path<String>,
    body: Result<Json<CancelChatPayload>, axum::extract::rejection::JsonRejection>,
) -> Result<StatusCode, ApiError> {
    require_platform_access(&state)?;
    let cancel_bg = match body {
        Ok(Json(p)) => p.cancel_background_jobs,
        Err(err) => {
            // Empty / missing body → hard stop (legacy clients).
            log::info!("cancel_chat: body parse fallback conversation_id={conversation_id}: {err}");
            true
        }
    };
    state
        .dispatcher
        .cancel_conversation_and_wait(&conversation_id, cancel_bg)
        .await;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, Default)]
struct CancelJobsPayload {
    #[serde(default, rename = "jobIds")]
    job_ids: Vec<String>,
}

async fn cancel_background_jobs(
    State(state): State<ServerState>,
    Path(conversation_id): Path<String>,
    body: Result<Json<CancelJobsPayload>, axum::extract::rejection::JsonRejection>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_platform_access(&state)?;
    let job_ids = match body {
        Ok(Json(p)) => p.job_ids,
        Err(err) => {
            log::warn!(
                "cancel_background_jobs: body parse failed conversation_id={conversation_id}: {err}"
            );
            Vec::new()
        }
    };
    let ids = if job_ids.is_empty() {
        None
    } else {
        Some(job_ids.as_slice())
    };
    let cancelled = state.core.cancel_background_jobs(&conversation_id, ids);
    Ok(Json(serde_json::json!({ "cancelled": cancelled })))
}

#[derive(Deserialize)]
struct ConsoleSessionCreatePayload {
    #[serde(rename = "workspaceRoot")]
    workspace_root: String,
    #[serde(rename = "conversationId")]
    conversation_id: String,
    #[serde(default)]
    cwd: Option<String>,
    cols: u16,
    rows: u16,
}

#[derive(Deserialize)]
struct ConsoleSessionInputPayload {
    data: String,
}

#[derive(Deserialize)]
struct ConsoleSessionResizePayload {
    cols: u16,
    rows: u16,
}

async fn create_console_session(
    State(state): State<ServerState>,
    Json(payload): Json<ConsoleSessionCreatePayload>,
) -> Result<Json<pointer_core::console_session::ConsoleSessionInfo>, ApiError> {
    require_platform_access(&state)?;
    state
        .core
        .console_sessions
        .create(
            &payload.workspace_root,
            &payload.conversation_id,
            payload.cwd.as_deref(),
            payload.cols,
            payload.rows,
        )
        .map(Json)
        .map_err(ApiError::from)
}

async fn write_console_session(
    State(state): State<ServerState>,
    Path(session_id): Path<String>,
    Json(payload): Json<ConsoleSessionInputPayload>,
) -> Result<StatusCode, ApiError> {
    require_platform_access(&state)?;
    state
        .core
        .console_sessions
        .write(&session_id, &payload.data)
        .map_err(ApiError::from)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn resize_console_session(
    State(state): State<ServerState>,
    Path(session_id): Path<String>,
    Json(payload): Json<ConsoleSessionResizePayload>,
) -> Result<StatusCode, ApiError> {
    require_platform_access(&state)?;
    state
        .core
        .console_sessions
        .resize(&session_id, payload.cols, payload.rows)
        .map_err(ApiError::from)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn close_console_session(
    State(state): State<ServerState>,
    Path(session_id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_platform_access(&state)?;
    Ok(Json(serde_json::json!({
        "closed": state.core.console_sessions.close(&session_id)
    })))
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
struct AskUserPayload {
    selected: Vec<String>,
}

async fn submit_ask_user(
    State(state): State<ServerState>,
    Path(tool_call_id): Path<String>,
    Json(payload): Json<AskUserPayload>,
) -> Result<StatusCode, ApiError> {
    require_platform_access(&state)?;
    if state.core.submit_ask_user(&tool_call_id, payload.selected) {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError(anyhow::anyhow!("未找到待选择的 ask_user 请求")))
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
) -> Result<Response, ApiError> {
    require_platform_access(&state)?;
    let viewer_uid = platform_session_user_id(&state);
    let mut rx = state.events.subscribe();
    let sse_padding_enabled = pointer_core::server_config::sse_padding_enabled();
    let sse_padding_bytes = pointer_core::server_config::sse_padding_bytes();
    let stream = async_stream::stream! {
        if sse_padding_enabled && sse_padding_bytes > 0 {
            yield Ok(Event::default().comment("x".repeat(sse_padding_bytes)));
        }
        loop {
            match rx.recv().await {
                Ok(frame) => {
                    let belongs = pointer_core::stream_broadcast::sse_subscription_matches(
                        &conversation_id,
                        &viewer_uid,
                        frame.conversation_id.as_deref(),
                        frame.session_user_id.as_deref(),
                        &frame.event,
                    );
                    if belongs {
                        let data = serde_json::to_string(&frame.event).unwrap_or_else(|_| "{}".into());
                        yield Ok(Event::default().event("message").data(data));
                    }
                }
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    // Dropped events (often Done / deltas under weak networks).
                    // Tell the browser to reload messages + reconcile run state.
                    log::warn!(
                        "chat_stream lagged conversation_id={} skipped≈{}",
                        conversation_id,
                        skipped
                    );
                    yield Ok(
                        Event::default()
                            .event("resync")
                            .data(format!("{{\"skipped\":{skipped}}}")),
                    );
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    };
    Ok(sse_with_proxy_hints(
        Sse::new(stream).keep_alive(
            KeepAlive::new()
                .interval(Duration::from_secs(15))
                .text("keep-alive"),
        ),
    ))
}

/// Attach reverse-proxy hints that nginx/OpenResty honor for long-lived SSE.
///
/// `X-Accel-Buffering: no` disables nginx response buffering for this response
/// even when the location did not set `proxy_buffering off` (see nginx
/// `X-Accel-Buffering`). Timeouts (`proxy_read_timeout`) still must be
/// configured on the proxy — the origin cannot override them.
fn sse_with_proxy_hints<S>(sse: Sse<S>) -> Response
where
    S: Stream<Item = Result<Event, Infallible>> + Send + 'static,
{
    let mut res = sse.into_response();
    res.headers_mut().insert(
        header::HeaderName::from_static("x-accel-buffering"),
        HeaderValue::from_static("no"),
    );
    res
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

async fn get_experience_home(
) -> Result<Json<pointer_core::experiences::ExperienceHomeResponse>, ApiError> {
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

/// Deb / FHS install path for the Vue SPA (see `scripts/build-server-deb.mjs`).
const DEB_SHARE_STATIC_DIR: &str = "/usr/share/pointer-server/dist";

/// Resolve Vue production bundle directory (`dist/`).
///
/// Search order: `POINTER_SERVER_STATIC_DIR` → cwd/`dist` → exe-adjacent →
/// `{exe}/../../dist` → `/usr/share/pointer-server/dist` (Linux `.deb`).
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
    candidates.push(PathBuf::from(DEB_SHARE_STATIC_DIR));

    for candidate in candidates {
        if let Ok(canonical) = candidate.canonicalize() {
            if canonical.is_dir() {
                return Some(canonical);
            }
        }
    }
    None
}

/// Browser CORS layer. Off by default (same-origin). `POINTER_SERVER_CORS_ORIGINS=* `
/// mirrors any Origin; a comma-separated list is an exact allowlist.
/// `permissive()` / `*` cannot be used with `credentials: 'include'`.
fn build_cors_layer() -> anyhow::Result<Option<CorsLayer>> {
    cors_layer_from_mode(&pointer_core::server_config::cors_mode()?)
}

fn cors_layer_from_mode(
    mode: &pointer_core::server_config::CorsMode,
) -> anyhow::Result<Option<CorsLayer>> {
    use pointer_core::server_config::CorsMode;
    match mode {
        CorsMode::Disabled => {
            log::info!("pointer-server: CORS disabled (set POINTER_SERVER_CORS_ORIGINS to enable)");
            Ok(None)
        }
        CorsMode::MirrorAny => {
            log::info!("pointer-server: CORS enabled (mirror any Origin, credentials)");
            Ok(Some(
                CorsLayer::new()
                    .allow_origin(AllowOrigin::mirror_request())
                    .allow_methods(AllowMethods::mirror_request())
                    .allow_headers(AllowHeaders::mirror_request())
                    .allow_credentials(true),
            ))
        }
        CorsMode::Allowlist(origins) => {
            let values = origins
                .iter()
                .map(|origin| {
                    HeaderValue::from_str(origin).map_err(|e| {
                        anyhow::anyhow!(
                            "POINTER_SERVER_CORS_ORIGINS: invalid origin {origin:?}: {e}"
                        )
                    })
                })
                .collect::<anyhow::Result<Vec<_>>>()?;
            log::info!("pointer-server: CORS allowlist {}", origins.join(","));
            Ok(Some(
                CorsLayer::new()
                    .allow_origin(AllowOrigin::list(values))
                    .allow_methods(AllowMethods::mirror_request())
                    .allow_headers(AllowHeaders::mirror_request())
                    .allow_credentials(true),
            ))
        }
    }
}

/// When `dist/` exists, serve the Vue SPA from the same process (API routes take precedence).
fn maybe_with_static_files(
    api: Router<ServerState>,
    static_dir: Option<PathBuf>,
) -> Router<ServerState> {
    let Some(dir) = static_dir else {
        log::info!("pointer-server: no dist/ found; API-only mode");
        return api;
    };
    let _ = WEB_DIST.set(dir);
    log::info!(
        "pointer-server: serving web UI from {}",
        WEB_DIST
            .get()
            .map(|p| p.display().to_string())
            .unwrap_or_default()
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
            "platform OAuth disabled in standalone mode; use POST /api/auth/local/login with username/password"
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
        (StatusCode::BAD_REQUEST, "invalid_or_expired_state".into())
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
                    Redirect::temporary("/?platform_login_error=server_access_denied")
                        .into_response(),
                );
            }
            auth.set_session(session);
            let session_id = state.web_sessions.insert(
                auth,
                creds,
                pointer_core::web_request_auth::WebSessionAuthKind::Platform,
            );
            sync_automation_web_session(&state);
            log::info!(
                "platform_auth: callback ok state={} web_session={session_id}",
                q.state
            );
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
async fn platform_logout(headers: HeaderMap, State(state): State<ServerState>) -> Response {
    if let Some(session_id) = web_session::session_id_from_headers(&headers) {
        if let Some(entry) = state.web_sessions.get(&session_id) {
            entry.auth.clear_session_async().await;
        }
        state.web_sessions.remove(&session_id);
    }
    sync_automation_web_session(&state);
    let mut platform = state.core.platform_config.write();
    apply_login_media_oss(&mut platform.media_oss, None);
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
    Ok(Json(state.core.active_platform_auth().session_view()))
}

pub(crate) fn cookie_secure() -> bool {
    resolve_server_public_url().is_some_and(|url| url.to_ascii_lowercase().starts_with("https://"))
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

async fn spa_fallback(State(state): State<ServerState>, uri: Uri) -> Result<Response, StatusCode> {
    if let Some(resp) = try_standalone_sso(&state, &uri) {
        return Ok(resp);
    }
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

#[derive(Debug, Deserialize)]
struct SsoQuery {
    sso: Option<String>,
}

/// `GET /api/auth/local/sso?sso=` — standalone third-party SSO ticket login.
async fn standalone_sso_login(
    State(state): State<ServerState>,
    Query(q): Query<SsoQuery>,
) -> Response {
    complete_standalone_sso(&state, q.sso.as_deref().unwrap_or(""))
}

fn try_standalone_sso(state: &ServerState, uri: &Uri) -> Option<Response> {
    let query = uri.query()?;
    let ticket = form_query_param(query, "sso")?;
    Some(complete_standalone_sso(state, &ticket))
}

fn complete_standalone_sso(state: &ServerState, ticket: &str) -> Response {
    use axum::response::Redirect;
    if !pointer_core::deployment_mode::is_standalone() {
        log::warn!("local_sso: rejected — not in standalone mode");
        return Redirect::temporary("/?sso_error=not_standalone").into_response();
    }
    if ticket.trim().is_empty() {
        return Redirect::temporary("/?sso_error=missing").into_response();
    }
    if !pointer_core::local_sso::local_sso_configured() {
        log::warn!("local_sso: rejected — SSO not configured");
        return Redirect::temporary("/?sso_error=not_configured").into_response();
    }
    match pointer_core::local_sso::verify_sso_ticket_now(ticket, &state.sso_nonces) {
        Ok(identity) => {
            let user_id = identity.user_id.clone();
            let auth = pointer_core::local_auth::create_local_auth_manager_for_user(
                &user_id,
                identity.nickname,
                false,
            );
            let creds = pointer_core::local_auth::empty_local_credentials();
            let session_id = state.web_sessions.insert(
                auth,
                creds,
                pointer_core::web_request_auth::WebSessionAuthKind::Local,
            );
            sync_automation_web_session(state);
            log::info!("local_sso: login ok user_id={user_id} web_session={session_id}");
            let mut resp = Redirect::temporary("/").into_response();
            web_session::set_session_cookie(resp.headers_mut(), &session_id, cookie_secure());
            resp
        }
        Err(_) => Redirect::temporary("/?sso_error=invalid").into_response(),
    }
}

async fn try_cloud_oauth_exchange(state: &ServerState, uri: &Uri) -> Option<Response> {
    let query = uri.query()?;
    let code = form_query_param(query, "code")?;
    let oauth_state = form_query_param(query, "state")?;
    if !pointer_core::cloud_agent_auth::is_cloud_auth_configured() {
        log::warn!("cloud oauth: POINTER_API_BASE not configured");
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
    let mut bytes = tokio::fs::read(path)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    if path.file_name().and_then(|n| n.to_str()) == Some("index.html") {
        bytes = apply_web_branding(&bytes).into_bytes();
    }
    let mut response = Response::new(bytes.into());
    if let Ok(value) = HeaderValue::from_str(static_content_type(path)) {
        response.headers_mut().insert(header::CONTENT_TYPE, value);
    }
    if path.file_name().and_then(|n| n.to_str()) == Some("index.html") {
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    }
    Ok(response)
}

const DEFAULT_WEB_PAGE_TITLE: &str = "Pointer · AI 工作台";
const DEFAULT_COMPOSER_PLACEHOLDER: &str = "告诉我你想做什么";
const COMPOSER_PLACEHOLDER_META: &str = "pointer-composer-placeholder";
const WELCOME_TIP_TITLE_META: &str = "pointer-welcome-tip-title";
const WELCOME_TIP_BODY_META: &str = "pointer-welcome-tip-body";
const TURN_ELAPSED_ACTIVE_META: &str = "pointer-turn-elapsed-active";
const TURN_ELAPSED_DONE_META: &str = "pointer-turn-elapsed-done";
const BRAND_NAME_META: &str = "pointer-brand-name";
const BRAND_ICON_META: &str = "pointer-brand-icon";
const DESKTOP_SNAPSHOT_META: &str = "pointer-desktop-snapshot";
const DEFAULT_BRAND_NAME: &str = "Pointer";
const DEFAULT_BRAND_ICON: &str = "/app-icon.png";

/// Browser tab title from `POINTER_SERVER_PAGE_TITLE` / `[server].page_title`.
fn resolve_web_page_title() -> String {
    std::env::var("POINTER_SERVER_PAGE_TITLE")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_WEB_PAGE_TITLE.to_string())
}

/// Composer placeholder from `POINTER_SERVER_COMPOSER_PLACEHOLDER` /
/// `[server].composer_placeholder`.
fn resolve_composer_placeholder() -> String {
    std::env::var("POINTER_SERVER_COMPOSER_PLACEHOLDER")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_COMPOSER_PLACEHOLDER.to_string())
}

fn resolve_optional_branding_env(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn html_escape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

fn replace_html_title(html: &str, title: &str) -> String {
    let replacement = format!("<title>{title}</title>");
    if let Some(start) = html.find("<title>") {
        if let Some(end_rel) = html[start..].find("</title>") {
            let end = start + end_rel + "</title>".len();
            let mut out = String::with_capacity(html.len() + title.len());
            out.push_str(&html[..start]);
            out.push_str(&replacement);
            out.push_str(&html[end..]);
            return out;
        }
    }
    log::warn!("pointer-server: index.html missing <title>; injecting before </head>");
    if let Some(head_end) = html.find("</head>") {
        let mut out = String::with_capacity(html.len() + replacement.len() + 1);
        out.push_str(&html[..head_end]);
        out.push_str(&replacement);
        out.push('\n');
        out.push_str(&html[head_end..]);
        return out;
    }
    html.to_string()
}

fn meta_content_tag(name: &str, content: &str) -> String {
    format!(r#"<meta name="{name}" content="{content}" />"#)
}

fn replace_or_inject_meta(html: &str, name: &str, content: &str) -> String {
    let meta = meta_content_tag(name, content);
    let needle = format!(r#"name="{name}""#);
    if let Some(name_idx) = html.find(&needle) {
        let tag_start = html[..name_idx].rfind('<').unwrap_or(0);
        if let Some(tag_end_rel) = html[name_idx..].find('>') {
            let tag_end = name_idx + tag_end_rel + 1;
            let mut out = String::with_capacity(html.len() + meta.len());
            out.push_str(&html[..tag_start]);
            out.push_str(&meta);
            out.push_str(&html[tag_end..]);
            return out;
        }
    }
    if let Some(head_end) = html.find("</head>") {
        let mut out = String::with_capacity(html.len() + meta.len() + 1);
        out.push_str(&html[..head_end]);
        out.push_str(&meta);
        out.push('\n');
        out.push_str(&html[head_end..]);
        return out;
    }
    log::warn!("pointer-server: index.html missing </head>; cannot inject meta {name}");
    html.to_string()
}

/// Inject optional branding meta only when the env override is non-empty.
fn maybe_inject_optional_meta(html: &str, env_key: &str, meta_name: &str) -> String {
    match resolve_optional_branding_env(env_key) {
        Some(value) => replace_or_inject_meta(html, meta_name, &html_escape_text(&value)),
        None => html.to_string(),
    }
}

fn resolve_brand_name() -> String {
    resolve_optional_branding_env("POINTER_SERVER_BRAND_NAME")
        .unwrap_or_else(|| DEFAULT_BRAND_NAME.to_string())
}

fn resolve_brand_icon() -> String {
    resolve_optional_branding_env("POINTER_SERVER_BRAND_ICON")
        .unwrap_or_else(|| DEFAULT_BRAND_ICON.to_string())
}

/// `true` / `1` / `yes` / `on` → show; `false` / `0` / `no` / `off` → hide.
/// When unset, auto-detect host displays (hide on headless / non-UI hosts).
fn resolve_desktop_snapshot_enabled() -> bool {
    match std::env::var("POINTER_SERVER_DESKTOP_SNAPSHOT_ENABLED") {
        Ok(raw) => {
            let v = raw.trim().to_ascii_lowercase();
            if v.is_empty() {
                return host_has_desktop_display();
            }
            matches!(v.as_str(), "1" | "true" | "yes" | "on")
        }
        Err(_) => host_has_desktop_display(),
    }
}

fn host_has_desktop_display() -> bool {
    use std::sync::OnceLock;
    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(|| {
        match pointer_core::agents::computer::screen::list_monitors() {
            Ok(monitors) => {
                let ok = !monitors.is_empty();
                if !ok {
                    log::info!(
                        "pointer-server: no displays found; desktop snapshot button hidden (set POINTER_SERVER_DESKTOP_SNAPSHOT_ENABLED=true to force)"
                    );
                }
                ok
            }
            Err(err) => {
                log::info!(
                    "pointer-server: display probe failed ({err:#}); desktop snapshot button hidden (set POINTER_SERVER_DESKTOP_SNAPSHOT_ENABLED=true to force)"
                );
                false
            }
        }
    })
}

/// Rewrite SPA shell branding: tab `<title>`, composer placeholder, and optional
/// welcome tip / turn-elapsed label overrides.
fn apply_web_branding(html_bytes: &[u8]) -> String {
    let html = String::from_utf8_lossy(html_bytes);
    let mut out = replace_html_title(&html, &html_escape_text(&resolve_web_page_title()));
    out = replace_or_inject_meta(
        &out,
        COMPOSER_PLACEHOLDER_META,
        &html_escape_text(&resolve_composer_placeholder()),
    );
    out = maybe_inject_optional_meta(
        &out,
        "POINTER_SERVER_WELCOME_TIP_TITLE",
        WELCOME_TIP_TITLE_META,
    );
    out = maybe_inject_optional_meta(
        &out,
        "POINTER_SERVER_WELCOME_TIP_BODY",
        WELCOME_TIP_BODY_META,
    );
    out = maybe_inject_optional_meta(
        &out,
        "POINTER_SERVER_TURN_ELAPSED_ACTIVE",
        TURN_ELAPSED_ACTIVE_META,
    );
    out = maybe_inject_optional_meta(
        &out,
        "POINTER_SERVER_TURN_ELAPSED_DONE",
        TURN_ELAPSED_DONE_META,
    );
    out = replace_or_inject_meta(
        &out,
        BRAND_NAME_META,
        &html_escape_text(&resolve_brand_name()),
    );
    out = replace_or_inject_meta(
        &out,
        BRAND_ICON_META,
        &html_escape_text(&resolve_brand_icon()),
    );
    out = replace_or_inject_meta(
        &out,
        DESKTOP_SNAPSHOT_META,
        if resolve_desktop_snapshot_enabled() {
            "1"
        } else {
            "0"
        },
    );
    out
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
        if msg.contains("download link expired")
            || msg.contains("invalid download token")
            || msg.contains("malformed download token")
        {
            return (StatusCode::UNAUTHORIZED, msg).into_response();
        }
        if msg.contains("media file not found") || msg.contains("download path outside") {
            return (StatusCode::NOT_FOUND, msg).into_response();
        }
        if msg.contains("文件超过") || msg.contains("too large") {
            return (StatusCode::PAYLOAD_TOO_LARGE, msg).into_response();
        }
        (StatusCode::INTERNAL_SERVER_ERROR, msg).into_response()
    }
}

/// Build a plain status+text error response (used by webhook auth failures
/// where a specific HTTP status is required without touching `ApiError`).
fn status_text(status: StatusCode, msg: impl Into<String>) -> axum::response::Response {
    (status, msg.into()).into_response()
}

#[cfg(test)]
mod page_title_tests {
    use super::{
        apply_web_branding, html_escape_text, DEFAULT_COMPOSER_PLACEHOLDER, DEFAULT_WEB_PAGE_TITLE,
    };
    use std::sync::{Mutex, MutexGuard};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn env_guard() -> MutexGuard<'static, ()> {
        ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn clear_branding_env() {
        std::env::remove_var("POINTER_SERVER_PAGE_TITLE");
        std::env::remove_var("POINTER_SERVER_COMPOSER_PLACEHOLDER");
        std::env::remove_var("POINTER_SERVER_WELCOME_TIP_TITLE");
        std::env::remove_var("POINTER_SERVER_WELCOME_TIP_BODY");
        std::env::remove_var("POINTER_SERVER_TURN_ELAPSED_ACTIVE");
        std::env::remove_var("POINTER_SERVER_TURN_ELAPSED_DONE");
        std::env::remove_var("POINTER_SERVER_BRAND_NAME");
        std::env::remove_var("POINTER_SERVER_BRAND_ICON");
        std::env::remove_var("POINTER_SERVER_DESKTOP_SNAPSHOT_ENABLED");
    }

    #[test]
    fn rewrites_existing_title_and_injects_placeholder_meta() {
        let _guard = env_guard();
        std::env::set_var("POINTER_SERVER_PAGE_TITLE", "Acme · AI");
        std::env::set_var("POINTER_SERVER_COMPOSER_PLACEHOLDER", "有什么可以帮你？");
        let html = "<!doctype html><html><head><title>Pointer · AI 工作台</title></head><body></body></html>";
        let out = apply_web_branding(html.as_bytes());
        assert!(out.contains("<title>Acme · AI</title>"), "{out}");
        assert!(!out.contains("Pointer · AI 工作台"), "{out}");
        assert!(
            out.contains(r#"name="pointer-composer-placeholder" content="有什么可以帮你？""#),
            "{out}"
        );
        assert!(!out.contains("pointer-welcome-tip-title"), "{out}");
        assert!(!out.contains("pointer-turn-elapsed-active"), "{out}");
        clear_branding_env();
    }

    #[test]
    fn defaults_when_env_empty() {
        let _guard = env_guard();
        clear_branding_env();
        let html = "<head><title>old</title></head>";
        let out = apply_web_branding(html.as_bytes());
        assert!(
            out.contains(&format!("<title>{DEFAULT_WEB_PAGE_TITLE}</title>")),
            "{out}"
        );
        assert!(
            out.contains(&format!(
                r#"name="pointer-composer-placeholder" content="{DEFAULT_COMPOSER_PLACEHOLDER}""#
            )),
            "{out}"
        );
    }

    #[test]
    fn rewrites_existing_placeholder_meta() {
        let _guard = env_guard();
        clear_branding_env();
        std::env::set_var("POINTER_SERVER_COMPOSER_PLACEHOLDER", "定制提示");
        let html = r#"<head><title>t</title><meta name="pointer-composer-placeholder" content="告诉我你想做什么" /></head>"#;
        let out = apply_web_branding(html.as_bytes());
        assert!(
            out.contains(r#"name="pointer-composer-placeholder" content="定制提示""#),
            "{out}"
        );
        assert!(!out.contains("告诉我你想做什么"), "{out}");
        clear_branding_env();
    }

    #[test]
    fn injects_optional_welcome_and_elapsed_metas() {
        let _guard = env_guard();
        clear_branding_env();
        std::env::set_var("POINTER_SERVER_WELCOME_TIP_TITLE", "我是财务报销助手");
        std::env::set_var("POINTER_SERVER_WELCOME_TIP_BODY", "预计 10–30 分钟");
        std::env::set_var("POINTER_SERVER_TURN_ELAPSED_ACTIVE", "报销单填写中");
        std::env::set_var("POINTER_SERVER_TURN_ELAPSED_DONE", "报销单已填写");
        let html = "<head><title>t</title></head>";
        let out = apply_web_branding(html.as_bytes());
        assert!(
            out.contains(r#"name="pointer-welcome-tip-title" content="我是财务报销助手""#),
            "{out}"
        );
        assert!(
            out.contains(r#"name="pointer-welcome-tip-body" content="预计 10–30 分钟""#),
            "{out}"
        );
        assert!(
            out.contains(r#"name="pointer-turn-elapsed-active" content="报销单填写中""#),
            "{out}"
        );
        assert!(
            out.contains(r#"name="pointer-turn-elapsed-done" content="报销单已填写""#),
            "{out}"
        );
        clear_branding_env();
    }

    #[test]
    fn escapes_html_in_title() {
        assert_eq!(
            html_escape_text("A <B> & \"C\""),
            "A &lt;B&gt; &amp; &quot;C&quot;"
        );
    }
    #[test]
    fn injects_brand_name_icon_and_snapshot_meta() {
        let _guard = env_guard();
        clear_branding_env();
        std::env::set_var("POINTER_SERVER_BRAND_NAME", "财务助手");
        std::env::set_var("POINTER_SERVER_BRAND_ICON", "/branding/logo.png");
        std::env::set_var("POINTER_SERVER_DESKTOP_SNAPSHOT_ENABLED", "false");
        let html = "<head><title>old</title></head>";
        let out = apply_web_branding(html.as_bytes());
        assert!(
            out.contains(r#"name="pointer-brand-name" content="财务助手""#),
            "{out}"
        );
        assert!(
            out.contains(r#"name="pointer-brand-icon" content="/branding/logo.png""#),
            "{out}"
        );
        assert!(
            out.contains(r#"name="pointer-desktop-snapshot" content="0""#),
            "{out}"
        );
        clear_branding_env();
    }
}

#[cfg(test)]
mod cors_layer_tests {
    use super::cors_layer_from_mode;
    use pointer_core::server_config::CorsMode;

    #[test]
    fn disabled_builds_no_layer() {
        assert!(cors_layer_from_mode(&CorsMode::Disabled).unwrap().is_none());
    }

    #[test]
    fn mirror_and_allowlist_build_layers() {
        assert!(cors_layer_from_mode(&CorsMode::MirrorAny)
            .unwrap()
            .is_some());
        assert!(
            cors_layer_from_mode(&CorsMode::Allowlist(vec!["http://localhost:1420".into()]))
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn reject_origin_with_control_chars() {
        let err =
            cors_layer_from_mode(&CorsMode::Allowlist(vec!["http://localhost:1420\n".into()]))
                .unwrap_err();
        assert!(err.to_string().contains("invalid origin"), "{err}");
    }
}
