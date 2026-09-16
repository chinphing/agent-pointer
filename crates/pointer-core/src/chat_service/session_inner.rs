//! Inner orchestration (`run_chat_inner`): settings, supervisor vs single-agent loop.

use crate::agents::{delegatable_sub_agents_system_block, AgentOrchestrator};
use crate::dispatcher::TriggerSource;
use crate::llm_token_stats::ChatLlmTokenSession;
use crate::models::{effective_reasoning_in_messages, ChatMessage, ModelSettings, StreamEvent};
use crate::provider::OpenAIProvider;
use crate::tools::file::ConversationWorkspaceGuard;
use anyhow::{anyhow, Result};

use super::app_state::AppState;
use super::emit::emit;
use super::session_budget::SessionToolBudget;
use super::session_model::prepare_session_llm_settings;
use std::path::Path;
use std::time::Instant;

fn settings_have_llm_key(settings: &ModelSettings) -> bool {
    settings.has_key
        || settings
            .providers
            .iter()
            .any(|p| !p.api_key.trim().is_empty())
}

fn latest_real_user_turn(history: &[ChatMessage]) -> Option<(&str, &str)> {
    history.iter().rev().find_map(|m| {
        if !matches!(m.role, crate::models::Role::User) {
            return None;
        }
        if crate::message_context::is_synthetic_user_content(&m.content) {
            return None;
        }
        Some((m.id.as_str(), m.content.as_str()))
    })
}

fn is_parent_board_unfinished(store: &crate::task_board::TaskBoardStore, store_key: &str) -> bool {
    let doc = store.document(store_key);
    !matches!(
        doc.meta.status,
        crate::task_board::MetaStatus::Completed | crate::task_board::MetaStatus::Failed
    )
}

fn choose_main_task_board_store_key(
    state: &AppState,
    conversation_id: &str,
    history: &[ChatMessage],
) -> String {
    let Some((last_user_id, _last_user_content)) = latest_real_user_turn(history) else {
        return conversation_id.to_string();
    };

    // Surface the active unfinished board so planner/execution can see it.
    // Reuse vs new board is decided by the model: no init → continue; init → fresh board.
    if let Some(active_key) = state.get_active_main_task_board_key(conversation_id) {
        if !crate::task_board::is_child_store_key(&active_key)
            && is_parent_board_unfinished(state.task_board_store.as_ref(), &active_key)
        {
            state.set_main_task_board_binding(conversation_id, &active_key, last_user_id);
            state.set_active_main_task_board_key(conversation_id, &active_key);
            log::debug!(
                "task_board_main_key: reuse_active conversation_id={} store_key={} anchor_message_id={}",
                conversation_id,
                active_key,
                last_user_id
            );
            return active_key;
        }
    }

    let key = crate::task_board::main_turn_task_board_store_key(conversation_id, last_user_id);
    state.set_main_task_board_binding(conversation_id, &key, last_user_id);
    state.set_active_main_task_board_key(conversation_id, &key);
    log::info!(
        "task_board_main_key: new_turn conversation_id={} store_key={} anchor_message_id={}",
        conversation_id,
        key,
        last_user_id
    );
    key
}

pub(super) async fn run_chat_inner(
    ctx: &mut super::context::ChatRunContext<'_>,
    req: &super::context::ChatRunRequest,
) -> Result<()> {
    let stream = ctx.stream.clone();
    let state = ctx.state.clone();
    let conversation_id = ctx.conversation_id;
    let request_agent_mode = req.agent_mode.as_deref();
    let request_lead_agent_id = req.lead_agent_id_override.as_deref();
    let request_performance_mode = req.performance_mode_override.as_deref();
    let tool_rounds_used_single_start = req.tool_rounds_used_single_start;
    let workspace_root = req.workspace_root.clone();
    let run_id = req.run_id.as_str();
    let cancel = ctx.cancel.clone();

    // Refresh platform session and gate chat before binding session_user_id or persisting media.
    let web_session = crate::web_request_auth::scoped_login_creds().is_some();
    let is_local_session = crate::web_request_auth::is_local_scoped_session();
    let skip_platform_refresh = crate::deployment_mode::is_standalone() && is_local_session;
    let mut refresh_transient_error: Option<String> = None;
    let auth_refresh_t = Instant::now();
    if !skip_platform_refresh {
        match state.active_platform_auth().refresh_if_needed().await {
            Ok(Some((_session, creds))) => {
                if !web_session {
                    if creds.api_key.is_some()
                        || !creds.provider_api_keys.is_empty()
                        || creds.media_oss.is_some()
                    {
                        state.apply_login_credentials(&creds);
                    } else if let Ok(Some(fetched)) =
                        state.active_platform_auth().fetch_llm_credentials().await
                    {
                        state.apply_login_credentials(&fetched);
                    }
                }
            }
            Ok(None) => {}
            Err(e) => {
                log::warn!("platform_auth: refresh before chat failed: {e:#}");
                if state
                    .active_platform_auth()
                    .is_refresh_transient_failure(&e)
                {
                    refresh_transient_error = Some(e.to_string());
                }
            }
        }
    }
    crate::logging::log_phase_elapsed_extra(
        "platform_auth_refresh",
        conversation_id,
        auth_refresh_t.elapsed().as_millis(),
        &format!("skipped={skip_platform_refresh}"),
    );
    // Leftover pending from a previous turn whose end-of-run flush skipped
    // (expired access token, or refresh failed). Session is fresh now — do not
    // wait until this turn ends.
    if let Err(e) =
        crate::token_usage_store::flush_unsent_reports(&state.active_platform_auth()).await
    {
        log::warn!("token_usage_store: flush before chat failed: {e}");
    }
    let platform_logged_in = state.active_platform_auth().session_view().logged_in;
    let is_automation = req
        .trigger_source
        .is_some_and(TriggerSource::is_headless_automation);
    let has_local_llm = settings_have_llm_key(&state.effective_settings());
    if platform_logged_in || web_session {
        // Platform balance gate once per user turn (GET /auth/partner/balance).
        // Standalone never applies. Login / llm-credentials unchanged.
        if !crate::deployment_mode::is_standalone() && platform_logged_in {
            let balance_t = Instant::now();
            match state
                .active_platform_auth()
                .ensure_llm_allowed_with_balance()
                .await
            {
                Ok(Some(balance)) => {
                    match state
                        .active_platform_auth()
                        .refresh_llm_credentials_if_model_catalog_changed(
                            balance.model_catalog_hash.as_deref(),
                        )
                        .await
                    {
                        Ok(Some(creds)) => state.apply_login_credentials(&creds),
                        Ok(None) => {}
                        Err(err) => log::warn!(
                            "platform_auth: model catalog refresh after balance check failed; keeping cached catalog: {err:#}"
                        ),
                    }
                }
                Ok(None) => {}
                Err(e) => {
                    let raw = e.to_string();
                    let msg = if raw.contains("token_quota_exhausted") {
                        "账户余额已用尽，请前往 Pointer 官网余额页充值。".to_string()
                    } else if raw.contains("网络异常") {
                        // Already normalized after balance-check retries.
                        raw
                    } else if state
                        .active_platform_auth()
                        .is_refresh_transient_failure(&e)
                    {
                        "网络异常，请检查网络链接是否正常，然后重试。".to_string()
                    } else {
                        raw
                    };
                    return Err(anyhow!(msg));
                }
            }
            crate::logging::log_phase_elapsed(
                "platform_balance_gate",
                conversation_id,
                balance_t.elapsed().as_millis(),
            );
        }
    } else if is_automation && has_local_llm {
        log::info!(
            "automation trigger {:?}: using local LLM credentials without platform login conversation_id={}",
            req.trigger_source,
            conversation_id
        );
    } else if is_automation {
        return Err(anyhow!(
            "自动化触发需要 LLM 凭证：云实例请先从桌面「打开云主机」或 Web 端完成一次登录；自部署请在设置 → 模型配置中填写 API Key"
        ));
    } else if let Some(detail) = refresh_transient_error {
        // Access token expired and refresh hit a transport/upstream blip — not a real logout.
        log::warn!(
            "platform_auth: chat gated by transient refresh failure conversation_id={conversation_id} detail={detail}"
        );
        return Err(anyhow!("网络异常，暂时无法验证登录态，请稍后重试"));
    } else {
        let msg = if crate::deployment_mode::is_standalone() {
            "请先登录"
        } else {
            "请先登录 Pointer 账户"
        };
        return Err(anyhow!(msg));
    }

    if platform_logged_in || is_local_session {
        // Prefer the logged-in session user id (SSO `sub`, OAuth id, or password `local-admin`).
        // Do not force `local-admin` for all Local Cookie sessions — that collapses multi-user SSO.
        let uid = state
            .active_platform_auth()
            .platform_user_id()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_default();
        if !uid.is_empty() {
            if let Err(e) = state
                .session_index
                .ensure_session_user_id(conversation_id, uid.as_str())
            {
                log::warn!(
                    "session_user_id ensure failed conversation_id={conversation_id}: {e:#}"
                );
            }
        }
    }
    let session_user_id = state
        .session_index
        .session_user_id(conversation_id)
        .unwrap_or_default();

    if let Err(e) = state.memory_store.ensure_loaded(session_user_id.as_str()) {
        log::warn!("memory: ensure session user failed conversation_id={conversation_id}: {e:#}");
    }

    let payload_workspace = workspace_root.trim();
    let inherit_disabled =
        req.workspace_inherit_disabled == Some(true) || workspace_inherit_disabled(conversation_id);

    let workspace_t = Instant::now();
    let default_path_before = crate::session_sandbox::SessionSandbox::default_path(
        conversation_id,
        session_user_id.as_str(),
    )
    .ok()
    .map(|p| p.exists())
    .unwrap_or(false);

    let ui_workspace_before = workspace_baseline_for_ui(
        conversation_id,
        payload_workspace,
        inherit_disabled,
        session_user_id.as_str(),
        default_path_before,
    );

    let effective_workspace = resolve_run_workspace(
        conversation_id,
        payload_workspace,
        inherit_disabled,
        session_user_id.as_str(),
    )
    .unwrap_or_else(|e| {
        log::warn!("session workspace resolution failed: {e:#}; using empty");
        String::new()
    });

    let effective_workspace = ensure_workspace_at_run_start(
        conversation_id,
        &effective_workspace,
        session_user_id.as_str(),
    )?;
    crate::logging::log_phase_elapsed_extra(
        "ensure_workspace",
        conversation_id,
        workspace_t.elapsed().as_millis(),
        &format!("root={effective_workspace}"),
    );

    let _workspace_guard = ConversationWorkspaceGuard::enter(effective_workspace.clone());
    let _work_dir_guard =
        crate::session_work_dir_env::SessionWorkDirGuard::enter(effective_workspace.clone());

    let _session_user_guard = crate::session_user_env::SessionUserIdGuard::enter(session_user_id);

    if effective_workspace.trim() != ui_workspace_before.trim() {
        let is_ephemeral = ui_workspace_before.trim().is_empty()
            && crate::session_sandbox::SessionSandbox::is_sandbox(Path::new(
                effective_workspace.trim(),
            ))
            .unwrap_or(false);
        emit(
            &stream,
            StreamEvent::WorkspaceUpdated {
                conversation_id: conversation_id.to_string(),
                workspace_root: effective_workspace.clone(),
                is_ephemeral_sandbox: is_ephemeral,
            },
        );
    }
    let mut settings = state.effective_settings();
    if !effective_workspace.trim().is_empty() {
        settings.workspace_root = effective_workspace.trim().to_string();
    }
    let tool_approval_mode = settings.tool_approval_mode.clone();
    let effective_agent_mode = request_agent_mode
        .filter(|mode| !mode.trim().is_empty())
        .unwrap_or(&settings.agent_mode)
        .to_string();
    let lead_worker_id: Option<String> = request_lead_agent_id
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .or_else(|| {
            let id = settings.lead_agent_id.trim();
            if id.is_empty() {
                None
            } else {
                Some(id.to_string())
            }
        });
    let api_key = prepare_session_llm_settings(
        &mut settings,
        &effective_agent_mode,
        lead_worker_id.as_deref(),
        request_performance_mode,
    );
    if api_key.is_empty() {
        return Err(anyhow!(
            "尚未配置 API Key（{}），请先登录账户或在设置中配置密钥",
            settings.active_provider_id
        ));
    }
    crate::context_compression::remember_session_llm(conversation_id, &settings, &api_key, None);
    let media_t = Instant::now();
    if let Err(e) = crate::media::apply_media_to_history(
        ctx.history,
        &settings,
        run_id,
        conversation_id,
        &api_key,
        &cancel,
    )
    .await
    {
        log::warn!("media: apply_media_to_history failed: {:#}", e);
    }
    crate::logging::log_phase_elapsed(
        "apply_media_to_history",
        conversation_id,
        media_t.elapsed().as_millis(),
    );
    // apply_media persists attachments and clears wire base64; upsert + notify UI.
    for msg in ctx.history.iter() {
        if matches!(msg.role, crate::models::Role::User)
            && msg.attachments.as_ref().is_some_and(|a| !a.is_empty())
        {
            super::conversation_persist::upsert_message(conversation_id, msg);
            crate::stream_broadcast::broadcast_stream(
                &StreamEvent::UserMessageAttachmentsUpdated {
                    conversation_id: conversation_id.to_string(),
                    message_id: msg.id.clone(),
                    attachments: msg.attachments.clone().unwrap_or_default(),
                    content: Some(msg.content.clone()),
                },
            );
        }
    }
    let agent_plan = AgentOrchestrator::build_plan(
        &state.agents,
        &state.skills,
        &state.tools,
        &[],
        &req.agent_skill_overrides,
        &effective_agent_mode,
        lead_worker_id.as_deref(),
    );
    let mut agent_plan = agent_plan;
    // Effective list for inherit / skill_import persistence (not a resolve input).
    *ctx.enabled_skill_ids = agent_plan.resolved_skill_ids.clone();
    if crate::channel_outbound::is_im_conversation(conversation_id) {
        agent_plan
            .system_prompts
            .push(crate::channel_outbound::im_session_commands_block(
                &state.agents,
            ));
    }
    // Cron: Hermes prepends guidance onto the user message in the scheduler;
    // do not inject a cron system block here.
    if !matches!(req.trigger_source, Some(TriggerSource::Cron)) && req.im_auto_deliver {
        agent_plan
            .system_prompts
            .push(crate::scheduler::auto_deliver_system_prompt());
    }
    if let Some(block) =
        delegatable_sub_agents_system_block(&state.agents, &agent_plan.allow_agents)
    {
        agent_plan.system_prompts.push(block);
    }
    let provider = OpenAIProvider::new(settings.clone(), api_key);
    let model_name = if settings.model.trim().is_empty() {
        None
    } else {
        Some(settings.model.clone())
    };
    let lead_role = lead_worker_id
        .clone()
        .unwrap_or_else(|| effective_agent_mode.clone());
    let lead_instance_id = match crate::conversation_store::global_store() {
        Ok(store) => match store.ensure_lead_agent_instance(conversation_id) {
            Ok(id) => id,
            Err(err) => {
                log::error!(
                    "session: ensure_lead_agent_instance failed conversation_id={conversation_id}: {err:#}"
                );
                uuid::Uuid::new_v4().to_string()
            }
        },
        Err(err) => {
            log::error!(
                "session: conversation store unavailable for lead instance conversation_id={conversation_id}: {err:#}"
            );
            uuid::Uuid::new_v4().to_string()
        }
    };
    agent_plan
        .system_prompts
        .push(crate::agent_instance_scope::agent_instance_id_system_line(
            &lead_instance_id,
        ));
    let mut llm_token_session = ChatLlmTokenSession::new(
        run_id.to_string(),
        conversation_id.to_string(),
        lead_role,
        model_name,
        lead_instance_id,
    );
    crate::context_compression::remember_session_llm(
        conversation_id,
        &settings,
        &provider.api_key,
        Some(llm_token_session.lead_scope.clone()),
    );

    let max_cap = settings.max_tool_rounds.clamp(1, 10_000);
    // Tool-round cap is enforced per user turn (each run_chat invocation), not
    // cumulatively across the whole conversation. Cron ticks, follow-up sends,
    // and long transcripts therefore each get a fresh budget up to max_cap.
    let tool_budget_single_start = 0;

    let main_task_board_store_key =
        choose_main_task_board_store_key(state.as_ref(), conversation_id, ctx.history);

    let mut tool_budget = SessionToolBudget::new(max_cap, tool_budget_single_start);
    let reasoning_in_messages = effective_reasoning_in_messages(&provider.settings);

    let memory_due = crate::memory::memory_review_due_for(
        &settings,
        &agent_plan.allowed_tool_names,
        ctx.history,
    );

    let mut lead_ctx = super::context::LeadAgentLoopContext {
        session: super::context::SessionRefsArc {
            stream: &stream,
            state: state.clone(),
            conversation_id,
            cancel: cancel.clone(),
        },
        history: ctx.history,
        enabled_skill_ids: ctx.enabled_skill_ids,
        agent_skill_overrides: &req.agent_skill_overrides,
        agent_plan: &agent_plan,
        provider: &provider,
        settings: &settings,
        main_task_board_store_key: &main_task_board_store_key,
        tool_approval_mode: tool_approval_mode.as_str(),
        tool_budget: &mut tool_budget,
        consumed_single: ctx.consumed_single,
        max_cap,
        token_session: &mut llm_token_session,
        reasoning_in_messages,
        trigger_source: req.trigger_source,
    };
    super::single_agent::run_single_agent_loop(&mut lead_ctx).await?;

    let new_tool_total = tool_rounds_used_single_start.saturating_add(*ctx.consumed_single);
    let skill_due = crate::memory::skill_review_due_for(
        &settings,
        &agent_plan.allowed_tool_names,
        new_tool_total,
    );
    if let Some(kind) = crate::memory::resolve_review_kind(memory_due, skill_due) {
        crate::memory::spawn_background_review(
            state,
            provider,
            conversation_id.to_string(),
            ctx.history.clone(),
            settings,
            ctx.enabled_skill_ids.clone(),
            kind,
            stream,
        );
    }

    Ok(())
}

/// Resolve workspace for a chat run.
///
/// Priority:
/// 1. User cleared workspace (`inherit_disabled`) → default sandbox
/// 2. Non-empty payload from client → user project path
/// 3. This conversation's persisted user pick (`workspace_user_set`)
/// 4. Default sandbox `{session-sandboxes}/{session_user_id}/` or `_anonymous/{conversation_id}/`
fn resolve_run_workspace(
    conversation_id: &str,
    payload_workspace: &str,
    inherit_disabled: bool,
    session_user_id: &str,
) -> Result<String> {
    if inherit_disabled {
        return default_sandbox_display(conversation_id, session_user_id, "user cleared workspace");
    }

    let payload = payload_workspace.trim();
    if !payload.is_empty() && is_existing_workspace_dir(payload) {
        return Ok(payload.to_string());
    }

    if let Ok(store) = crate::conversation_store::global_store() {
        if store.workspace_user_set(conversation_id).unwrap_or(false) {
            let stored = store.workspace_root(conversation_id).unwrap_or_default();
            let stored = stored.trim();
            if is_existing_workspace_dir(stored)
                && !crate::session_sandbox::SessionSandbox::is_sandbox(Path::new(stored))
                    .unwrap_or(false)
            {
                log::info!(
                    "resolve_run_workspace: conversation user pick conversation_id={conversation_id}: {stored}"
                );
                return Ok(stored.to_string());
            }
        } else {
            let stored = store.workspace_root(conversation_id).unwrap_or_default();
            let stored = stored.trim();
            if is_existing_workspace_dir(stored)
                && crate::session_sandbox::SessionSandbox::is_sandbox(Path::new(stored))
                    .unwrap_or(false)
            {
                return default_sandbox_display(
                    conversation_id,
                    session_user_id,
                    "persisted sandbox",
                );
            }
        }
    }

    default_sandbox_display(conversation_id, session_user_id, "default sandbox")
}

fn default_sandbox_display(
    conversation_id: &str,
    session_user_id: &str,
    reason: &str,
) -> Result<String> {
    let path =
        crate::session_sandbox::SessionSandbox::default_path(conversation_id, session_user_id)?;
    log::info!(
        "resolve_run_workspace: {reason} conversation_id={conversation_id} path={}",
        path.display()
    );
    Ok(path.display().to_string())
}

fn stored_conversation_workspace(conversation_id: &str) -> String {
    crate::conversation_store::global_store()
        .ok()
        .and_then(|store| store.workspace_root(conversation_id).ok())
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn is_existing_workspace_dir(path: &str) -> bool {
    let p = Path::new(path.trim());
    p.is_absolute() && p.is_dir()
}

fn workspace_baseline_for_ui(
    conversation_id: &str,
    payload_workspace: &str,
    inherit_disabled: bool,
    session_user_id: &str,
    default_sandbox_existed: bool,
) -> String {
    if !inherit_disabled {
        return payload_workspace.trim().to_string();
    }
    let stored = stored_conversation_workspace(conversation_id);
    if !stored.trim().is_empty() {
        return stored;
    }
    if default_sandbox_existed {
        if let Ok(path) =
            crate::session_sandbox::SessionSandbox::default_path(conversation_id, session_user_id)
        {
            return path.display().to_string();
        }
    }
    String::new()
}

fn workspace_inherit_disabled(conversation_id: &str) -> bool {
    crate::conversation_store::global_store()
        .ok()
        .and_then(|store| store.workspace_inherit_disabled(conversation_id).ok())
        .unwrap_or(false)
}

/// Create sandbox on disk when this run resolved to a sandbox path; persist ephemeral path.
fn ensure_workspace_at_run_start(
    conversation_id: &str,
    workspace: &str,
    session_user_id: &str,
) -> Result<String> {
    let trimmed = workspace.trim();
    if trimmed.is_empty() {
        return Ok(String::new());
    }
    if !crate::session_sandbox::SessionSandbox::is_sandbox(Path::new(trimmed))? {
        return Ok(trimmed.to_string());
    }
    let existed =
        crate::session_sandbox::SessionSandbox::default_path(conversation_id, session_user_id)?
            .exists();
    let path =
        crate::session_sandbox::SessionSandbox::ensure_default(conversation_id, session_user_id)?;
    if !existed {
        log::info!(
            "ensure_workspace_at_run_start: created sandbox conversation_id={conversation_id}: {}",
            path.display()
        );
        super::conversation_persist::patch_ephemeral_workspace(
            conversation_id,
            &path.display().to_string(),
        );
    } else {
        log::debug!(
            "ensure_workspace_at_run_start: reusing sandbox conversation_id={conversation_id}: {}",
            path.display()
        );
    }
    Ok(path.display().to_string())
}

#[cfg(test)]
mod workspace_tests {
    use super::{choose_main_task_board_store_key, workspace_baseline_for_ui};
    use crate::chat_service::app_state::AppState;
    use crate::models::ChatMessage;

    fn user_message(id: &str, content: &str) -> ChatMessage {
        let mut message = ChatMessage::user_text(content);
        message.id = id.to_string();
        message
    }

    #[test]
    fn main_task_board_key_binds_triggering_user_message() {
        let state = AppState::new();
        let history = vec![user_message("u-trigger", "start")];

        let key = choose_main_task_board_store_key(&state, "conv-bind", &history);

        assert_eq!(
            key,
            crate::task_board::main_turn_task_board_store_key("conv-bind", "u-trigger")
        );
        assert_eq!(
            state
                .get_main_task_board_anchor("conv-bind", &key)
                .as_deref(),
            Some("u-trigger")
        );
    }

    #[test]
    fn reused_parent_board_rebinds_to_current_triggering_user_message() {
        let state = AppState::new();
        let original = vec![user_message("u-original", "start")];
        let key = choose_main_task_board_store_key(&state, "conv-resume", &original);
        let resumed = vec![
            user_message("u-original", "start"),
            user_message("u-resume", "continue"),
        ];

        let reused = choose_main_task_board_store_key(&state, "conv-resume", &resumed);

        assert_eq!(reused, key);
        assert_eq!(
            state
                .get_main_task_board_anchor("conv-resume", &key)
                .as_deref(),
            Some("u-resume")
        );
    }

    #[test]
    fn workspace_baseline_uses_stored_when_inherit_disabled() {
        let baseline = workspace_baseline_for_ui("c1", "", true, "user-1", false);
        assert_eq!(baseline, "");
    }

    #[test]
    fn workspace_baseline_uses_payload_when_inherit_enabled() {
        let baseline = workspace_baseline_for_ui("c1", "/projects/foo", false, "user-1", false);
        assert_eq!(baseline, "/projects/foo");
    }
}
