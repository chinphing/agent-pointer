//! Inner orchestration (`run_chat_inner`): settings, compression, supervisor vs single-agent loop.

use crate::agents::{
    delegatable_sub_agents_system_block, AgentOrchestrator, AGENT_MODE_SUPERVISOR,
};
use crate::llm_token_stats::ChatLlmTokenSession;
use crate::tools::file::ConversationWorkspaceGuard;
use crate::models::{effective_reasoning_in_messages, ChatMessage, StreamEvent};
use crate::provider::OpenAIProvider;
use anyhow::{anyhow, Result};
use std::time::Instant;

use super::app_state::AppState;
use super::emit::emit;
use super::session_budget::SessionToolBudget;
use super::session_model::prepare_session_llm_settings;
use std::path::Path;

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
    let Some((last_user_id, last_user_content)) = latest_real_user_turn(history) else {
        return conversation_id.to_string();
    };
    let resume_intent = crate::task_board::looks_like_resume_intent(last_user_content);
    if resume_intent {
        if let Some(active_key) = state.get_active_main_task_board_key(conversation_id) {
            if is_parent_board_unfinished(state.task_board_store.as_ref(), &active_key) {
                state.set_active_main_task_board_key(conversation_id, &active_key);
                log::info!(
                    "task_board_main_key: resume_intent=true reuse_active conversation_id={} store_key={}",
                    conversation_id,
                    active_key
                );
                return active_key;
            }
        }
    }

    let key = crate::task_board::main_turn_task_board_store_key(conversation_id, last_user_id);
    state.set_main_task_board_binding(conversation_id, &key, last_user_id);
    state.set_active_main_task_board_key(conversation_id, &key);
    log::info!(
        "task_board_main_key: selected conversation_id={} resume_intent={} store_key={} anchor_message_id={}",
        conversation_id,
        resume_intent,
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
    let tool_rounds_used_single_start = req.tool_rounds_used_single_start;
    let tool_rounds_used_supervisor_start = req.tool_rounds_used_supervisor_start;
    let workspace_root = req.workspace_root.clone();
    let run_id = req.run_id.as_str();
    let cancel = ctx.cancel.clone();
    // Resolve effective workspace. When the user cleared the composer, always resolve to
    // session sandbox even if a stale non-empty workspaceRoot was still in the payload.
    let payload_workspace = workspace_root.trim();
    let mut effective_workspace = if req.workspace_inherit_disabled == Some(true) {
        resolve_effective_workspace(conversation_id, Some(true)).unwrap_or_else(|e| {
            log::warn!("session workspace resolution failed: {e:#}; using empty");
            String::new()
        })
    } else if payload_workspace.is_empty() {
        resolve_effective_workspace(conversation_id, req.workspace_inherit_disabled).unwrap_or_else(|e| {
            log::warn!("session workspace resolution failed: {e:#}; using empty");
            String::new()
        })
    } else {
        payload_workspace.to_string()
    };

    let sandbox_existed_before = crate::session_sandbox::SessionSandbox::path(conversation_id)
        .map(|p| p.exists())
        .unwrap_or(false);
    let ui_workspace_before = workspace_baseline_for_ui(
        conversation_id,
        payload_workspace,
        req.workspace_inherit_disabled == Some(true),
        sandbox_existed_before,
    );

    effective_workspace =
        ensure_session_sandbox_at_run_start(conversation_id, &effective_workspace)?;

    let _workspace_guard = ConversationWorkspaceGuard::enter(effective_workspace.clone());
    if effective_workspace.trim() != ui_workspace_before.trim() {
        let is_ephemeral = ui_workspace_before.trim().is_empty()
            && (crate::session_sandbox::SessionSandbox::is_path_for(
                conversation_id,
                Path::new(effective_workspace.trim()),
            )
            .unwrap_or(false)
                || crate::session_sandbox::SessionSandbox::is_sandbox(Path::new(
                    effective_workspace.trim(),
                ))
                .unwrap_or(false));
        emit(
            &stream,
            StreamEvent::WorkspaceUpdated {
                conversation_id: conversation_id.to_string(),
                workspace_root: effective_workspace.clone(),
                is_ephemeral_sandbox: is_ephemeral,
            },
        );
    }
    // Restore from auth.dat / refresh near-expiry tokens before gating chat.
    match state.platform_auth.refresh_if_needed().await {
        Ok(Some((_session, creds))) => {
            if creds.api_key.is_some()
                || !creds.provider_api_keys.is_empty()
                || creds.media_oss.is_some()
            {
                state.apply_login_credentials(&creds);
            } else if let Ok(Some(fetched)) = state.platform_auth.fetch_llm_credentials().await {
                state.apply_login_credentials(&fetched);
            }
        }
        Ok(None) => {}
        Err(e) => {
            log::warn!("platform_auth: refresh before chat failed: {e:#}");
        }
    }
    if state.platform_auth.session_view().logged_in {
        if let Err(e) = state.platform_auth.ensure_llm_allowed().await {
            let msg = if e.to_string().contains("token_quota_exhausted") {
                "套餐 Token 额度已用尽，请前往 Openpointer 官网充值或联系管理员。".to_string()
            } else {
                e.to_string()
            };
            return Err(anyhow!(msg));
        }
    } else {
        return Err(anyhow!("请先登录 Pointer 账户"));
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
    );
    if api_key.is_empty() {
        return Err(anyhow!(
            "尚未配置 API Key（{}），请先登录平台账户或在设置中配置密钥",
            settings.active_provider_id
        ));
    }
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
    // apply_media persists attachments and clears wire base64; upsert + notify UI.
    for msg in ctx.history.iter() {
        if matches!(msg.role, crate::models::Role::User)
            && msg.attachments.as_ref().is_some_and(|a| !a.is_empty())
        {
            super::conversation_persist::upsert_message(conversation_id, msg);
            crate::stream_broadcast::broadcast_stream(&StreamEvent::UserMessageAttachmentsUpdated {
                conversation_id: conversation_id.to_string(),
                message_id: msg.id.clone(),
                attachments: msg.attachments.clone().unwrap_or_default(),
                content: Some(msg.content.clone()),
            });
        }
    }
    let agent_plan = AgentOrchestrator::build_plan(
        &state.agents,
        &state.skills,
        &state.tools,
        ctx.enabled_skill_ids,
        &effective_agent_mode,
        lead_worker_id.as_deref(),
    );
    let mut agent_plan = agent_plan;
    if crate::channel_outbound::is_im_conversation(conversation_id) {
        agent_plan
            .system_prompts
            .push(crate::channel_outbound::im_session_commands_block(&state.agents));
    }
    if agent_plan.mode != AGENT_MODE_SUPERVISOR {
        if let Some(block) =
            delegatable_sub_agents_system_block(&state.agents, &agent_plan.allow_agents)
        {
            agent_plan.system_prompts.push(block);
        }
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
    let mut llm_token_session =
        ChatLlmTokenSession::new(run_id.to_string(), conversation_id.to_string(), lead_role, model_name);
    let lead_scope = llm_token_session.lead_scope.clone();

    let t_compress = Instant::now();
    crate::context_compression::maybe_compress_history(
        ctx.history,
        &settings,
        &provider,
        conversation_id,
        &stream,
        cancel.clone(),
        crate::context_compression::CompressionUiContext::main(lead_scope),
        Some(state.memory_store.as_ref()),
    )
    .await;
    log::info!(
        "run_chat_inner: maybe_compress_history finished conversation_id={} wall_ms={} history_messages={}",
        conversation_id,
        t_compress.elapsed().as_millis(),
        ctx.history.len(),
    );

    let max_cap = settings.max_tool_rounds.clamp(1, 10_000);

    if agent_plan.mode == AGENT_MODE_SUPERVISOR {
        if tool_rounds_used_supervisor_start >= max_cap {
            state.computer_state.mark_cancelled(conversation_id);
            return Err(anyhow!(
                "本会话在团队模式下工具调用轮次已达上限（{}），请新开对话或在设置中调高上限。",
                max_cap
            ));
        }
        let mut tool_budget = SessionToolBudget::new(max_cap, tool_rounds_used_supervisor_start);
        let mut sup_ctx = super::context::SupervisorLoopContext {
            session: super::context::SessionRefsArc {
                stream: &stream,
                state: state.clone(),
                conversation_id,
                cancel: cancel.clone(),
            },
            history: ctx.history,
            enabled_skill_ids: ctx.enabled_skill_ids,
            provider,
            tool_budget: &mut tool_budget,
            llm_stats: &mut llm_token_session.stats,
            run_id,
        };
        let r = super::supervisor::run_supervisor_chat(&mut sup_ctx).await;
        tool_budget.sync_out(ctx.consumed_supervisor);
        return r;
    }

    if tool_rounds_used_single_start >= max_cap {
        return Err(anyhow!(
            "本会话在单智能体模式下工具调用轮次已达上限（{}），请新开对话或在设置中调高上限。",
            max_cap
        ));
    }
    let mut tool_budget = SessionToolBudget::new(max_cap, tool_rounds_used_single_start);
    let reasoning_in_messages = effective_reasoning_in_messages(&provider.settings);
    let main_task_board_store_key = choose_main_task_board_store_key(
        state.as_ref(),
        conversation_id,
        ctx.history,
    );

    let lead_profile = state
        .agents
        .get(&agent_plan.lead_agent_id)
        .map(|a| a.def().profile.clone())
        .unwrap_or(crate::agents::AgentProfile::General);

    let mut initial_assistant_id: Option<String> = None;
    if lead_profile == crate::agents::AgentProfile::Computer
        && settings.computer_standalone_planner_enabled
    {
        let aid = super::util::new_id("msg");
        ctx.history.push(ChatMessage {
            id: aid.clone(),
            role: crate::models::Role::Assistant,
            content: String::new(),
            status: "streaming".into(),
            created_at: super::util::now_ms(),
            tool_calls: None,
            tool_call_id: None,
            error_message: None,
            reasoning: None,
            thoughts: Some(crate::task_board::planner::PLANNER_PHASE_THOUGHTS.into()),
            headline: None,
            raw_content: None,
            tool_raw_output: None,
            agent_id: Some(agent_plan.lead_agent_id.clone()),
            agent_instance_id: None,
            agent_name: None,
            agent_trace: None,
            image_slot_labels: None,
            images_base64: None,
            computer_round_screen_rel_path: None,
            ui_bindings: None,
            context_state: None,
            attachments: None,
            anchor_message_id: None,
            trace_id: None,
            task_id: None,
            spawn_depth: None,
        });
        emit(
            &stream,
            StreamEvent::MessageStart {
                message_id: aid.clone(),
                conversation_id: conversation_id.to_string(),
            },
        );
        initial_assistant_id = Some(aid);
    }

    let planner_ui = if let Some(aid) = initial_assistant_id.as_ref() {
        Some(crate::task_board::planner::PlannerUiTarget {
            stream: &stream,
            state: state.as_ref(),
            message_id: aid,
            trace_id: None,
            scoped_message_id: None,
        })
    } else {
        None
    };

    let planner_outcome = crate::task_board::planner::run_planner_loop(
        crate::task_board::planner::PlannerRunInput {
            state: state.as_ref(),
            provider: &provider,
            settings: &settings,
            main_history: ctx.history,
            conversation_id,
            store_key: &main_task_board_store_key,
            lead_agent_id: &agent_plan.lead_agent_id,
            lead_profile: lead_profile.clone(),
            cancel: &cancel,
            llm_stats: &mut llm_token_session.stats,
            run_id,
            stream: &stream,
            context: crate::task_board::planner::PlannerContext::MainTurn,
            system_dynamic: &[],
            ui: planner_ui,
        },
    )
    .await;

    if let Some(aid) = initial_assistant_id.as_ref() {
        crate::task_board::planner::exclude_ui_shell_from_lead_context(ctx.history, aid);
    }

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
        planner_outcome,
        initial_assistant_id,
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

/// Resolve the effective workspace for a conversation when the frontend sends
/// an empty `workspaceRoot`.
///
/// Desktop sessions:
///   1. The most recent *other* conversation's `workspace_root` (inherits last
///      active session's directory), unless the user cleared workspace in composer.
///   2. Session sandbox path (`{app_data}/session-sandboxes/{conversation_id}/`;
///      directory is created on first chat run, not here).
///
/// IM sessions (Feishu / DingTalk / WeCom / Weixin): always use a per-conversation
/// session sandbox when no explicit `workspaceRoot` was stored — never inherit another
/// conversation's project directory.
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

/// Baseline workspace the UI already knows about (stored meta or an on-disk sandbox).
fn workspace_baseline_from_parts(
    payload_workspace: &str,
    inherit_disabled: bool,
    stored_workspace: &str,
    existing_sandbox_path: Option<&str>,
) -> String {
    if !inherit_disabled {
        return payload_workspace.trim().to_string();
    }
    let stored = stored_workspace.trim();
    if !stored.is_empty() {
        return stored.to_string();
    }
    if let Some(path) = existing_sandbox_path {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    String::new()
}

fn workspace_baseline_for_ui(
    conversation_id: &str,
    payload_workspace: &str,
    inherit_disabled: bool,
    sandbox_existed_before: bool,
) -> String {
    let stored = stored_conversation_workspace(conversation_id);
    let sandbox_path = if sandbox_existed_before {
        crate::session_sandbox::SessionSandbox::path(conversation_id)
            .ok()
            .map(|p| p.display().to_string())
    } else {
        None
    };
    workspace_baseline_from_parts(
        payload_workspace,
        inherit_disabled,
        &stored,
        sandbox_path.as_deref(),
    )
}

/// Resolve the per-conversation session sandbox, reusing stored or on-disk paths when present.
fn resolve_session_sandbox_workspace(conversation_id: &str, reason: &str) -> Result<String> {
    let stored = stored_conversation_workspace(conversation_id);
    if is_existing_workspace_dir(&stored) {
        log::info!(
            "resolve_effective_workspace: {reason} reusing stored workspace for conversation_id={conversation_id}: {stored}",
            conversation_id = conversation_id,
            stored = stored
        );
        return Ok(stored);
    }

    let sandbox_path = crate::session_sandbox::SessionSandbox::path(conversation_id)?;
    if sandbox_path.exists() {
        let path = sandbox_path.display().to_string();
        log::info!(
            "resolve_effective_workspace: {reason} reusing existing session sandbox for conversation_id={conversation_id}: {path}",
            conversation_id = conversation_id,
            path = path
        );
        return Ok(path);
    }

    let path = sandbox_path.display().to_string();
    log::info!(
        "resolve_effective_workspace: {reason} session sandbox (lazy) for conversation_id={conversation_id}: {path}",
        conversation_id = conversation_id,
        path = path
    );
    Ok(path)
}

fn resolve_effective_workspace(
    conversation_id: &str,
    inherit_disabled_override: Option<bool>,
) -> Result<String> {
    if crate::channel_outbound::is_im_conversation(conversation_id) {
        return resolve_session_sandbox_workspace(conversation_id, "IM session sandbox");
    }

    let inherit_disabled = inherit_disabled_override
        .unwrap_or_else(|| workspace_inherit_disabled(conversation_id));
    if inherit_disabled {
        return resolve_session_sandbox_workspace(
            conversation_id,
            "user cleared workspace; session sandbox",
        );
    }

    // Try the last-active conversation's workspace (skip ourselves).
    // Meta-only query: avoids loading every message of every conversation.
    if let Ok(store) = crate::conversation_store::global_store() {
        match store.latest_other_workspace_root(conversation_id) {
            Ok(Some(ws)) => {
                log::info!(
                    "resolve_effective_workspace: inheriting workspace for conversation_id={}: {}",
                    conversation_id,
                    ws
                );
                return Ok(ws);
            }
            Ok(None) => {
                log::info!(
                    "resolve_effective_workspace: no inheritable workspace for conversation_id={}; falling back to session sandbox",
                    conversation_id
                );
            }
            Err(e) => {
                log::warn!(
                    "resolve_effective_workspace: latest_other_workspace_root failed for conversation_id={}: {e}",
                    conversation_id
                );
            }
        }
    }

    resolve_session_sandbox_workspace(conversation_id, "session sandbox fallback")
}

fn workspace_inherit_disabled(conversation_id: &str) -> bool {
    crate::conversation_store::global_store()
        .ok()
        .and_then(|store| store.workspace_inherit_disabled(conversation_id).ok())
        .unwrap_or(false)
}

/// Create the on-disk session sandbox when this run resolved to that path.
fn ensure_session_sandbox_at_run_start(conversation_id: &str, workspace: &str) -> Result<String> {
    let trimmed = workspace.trim();
    if trimmed.is_empty() {
        return Ok(workspace.to_string());
    }
    if crate::session_sandbox::SessionSandbox::is_path_for(conversation_id, Path::new(trimmed))? {
        let existed = crate::session_sandbox::SessionSandbox::path(conversation_id)?
            .exists();
        let path = crate::session_sandbox::SessionSandbox::ensure(conversation_id)?;
        if existed {
            log::info!(
                "ensure_session_sandbox_at_run_start: reusing session sandbox for conversation_id={conversation_id}: {}",
                path.display()
            );
        } else {
            log::info!(
                "ensure_session_sandbox_at_run_start: created session sandbox for conversation_id={conversation_id}: {}",
                path.display()
            );
            super::conversation_persist::patch_ephemeral_workspace(
                conversation_id,
                &path.display().to_string(),
            );
        }
        return Ok(path.display().to_string());
    }
    Ok(workspace.to_string())
}

#[cfg(test)]
mod workspace_tests {
    use super::workspace_baseline_from_parts;

    #[test]
    fn workspace_baseline_uses_stored_when_inherit_disabled() {
        let baseline = workspace_baseline_from_parts("", true, "stored", None);
        assert_eq!(baseline, "stored");
    }

    #[test]
    fn workspace_baseline_uses_existing_sandbox_when_stored_empty() {
        let baseline = workspace_baseline_from_parts(
            "",
            true,
            "",
            Some("/tmp/session-sandboxes/conv_a"),
        );
        assert_eq!(baseline, "/tmp/session-sandboxes/conv_a");
    }

    #[test]
    fn workspace_baseline_uses_payload_when_inherit_enabled() {
        let baseline = workspace_baseline_from_parts("/projects/foo", false, "", None);
        assert_eq!(baseline, "/projects/foo");
    }
}
