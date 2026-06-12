//! Inner orchestration (`run_chat_inner`): settings, compression, supervisor vs single-agent loop.

use crate::agents::{
    delegatable_sub_agents_system_block, AgentOrchestrator, AGENT_MODE_SUPERVISOR,
};
use crate::llm_token_stats::ChatLlmTokenSession;
use crate::tools::file::ConversationWorkspaceGuard;
use crate::models::{effective_reasoning_in_messages, ChatMessage, StreamEvent};
use crate::provider::OpenAIProvider;
use anyhow::{anyhow, Result};
use std::sync::Arc;
use std::time::Instant;
use tokio_util::sync::CancellationToken;

use super::app_state::AppState;
use super::emit::emit;
use super::session_budget::SessionToolBudget;
use super::session_model::prepare_session_llm_settings;
use super::StreamTx;
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
    stream: StreamTx,
    state: Arc<AppState>,
    conversation_id: &str,
    history: &mut Vec<ChatMessage>,
    enabled_skill_ids: &mut Vec<String>,
    request_agent_mode: Option<&str>,
    request_lead_agent_id: Option<&str>,
    tool_rounds_used_single_start: u32,
    tool_rounds_used_supervisor_start: u32,
    workspace_root: String,
    consumed_single: &mut u32,
    consumed_supervisor: &mut u32,
    cancel: CancellationToken,
    run_id: &str,
) -> Result<()> {
    // Resolve effective workspace: payload → last-active conversation → session sandbox.
    let effective_workspace = if workspace_root.trim().is_empty() {
        resolve_effective_workspace(conversation_id, &state).unwrap_or_else(|e| {
            log::warn!("session workspace resolution failed: {e:#}; using empty");
            String::new()
        })
    } else {
        workspace_root.trim().to_string()
    };

    let _workspace_guard = ConversationWorkspaceGuard::enter(effective_workspace.clone());
    if effective_workspace.trim() != workspace_root.trim() {
        let is_ephemeral = workspace_root.trim().is_empty()
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
    // Restore from auth.dat / refresh near-expiry tokens before gating chat.
    match state.platform_auth.refresh_if_needed().await {
        Ok(Some((_session, creds))) => {
            if creds.api_key.is_some() || !creds.provider_api_keys.is_empty() {
                state.apply_login_credentials(&creds);
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
        history,
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
    // apply_media sets storage_rel_path, derived_text (ASR), clears wire base64; upsert + notify UI.
    for msg in history.iter() {
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
        enabled_skill_ids,
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
        history,
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
        history.len(),
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
        let r = super::supervisor::run_supervisor_chat(
            stream,
            state,
            conversation_id,
            history,
            enabled_skill_ids,
            provider,
            &mut tool_budget,
            cancel,
            effective_reasoning_in_messages(&settings),
            &mut llm_token_session.stats,
            run_id,
        )
        .await;
        tool_budget.sync_out(consumed_supervisor);
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
        history,
    );

    let memory_due = crate::memory::memory_review_due_for(
        &settings,
        &agent_plan.allowed_tool_names,
        history,
    );

    super::single_agent::run_single_agent_loop(
        stream.clone(),
        state.clone(),
        conversation_id,
        history,
        enabled_skill_ids,
        &agent_plan,
        &provider,
        &settings,
        &main_task_board_store_key,
        tool_approval_mode.as_str(),
        &mut tool_budget,
        consumed_single,
        max_cap,
        cancel,
        &mut llm_token_session,
        reasoning_in_messages,
    )
    .await?;

    let new_tool_total = tool_rounds_used_single_start.saturating_add(*consumed_single);
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
            history.clone(),
            settings,
            enabled_skill_ids.clone(),
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
///      active session's directory).
///   2. Session sandbox (`{app_data}/session-sandboxes/{conversation_id}/`).
///
/// IM sessions (Feishu / DingTalk / WeCom / Weixin): always use a per-conversation
/// session sandbox when no explicit `workspaceRoot` was stored — never inherit another
/// conversation's project directory.
fn resolve_effective_workspace(
    conversation_id: &str,
    _state: &AppState,
) -> Result<String> {
    if crate::channel_outbound::is_im_conversation(conversation_id) {
        let sandbox = crate::session_sandbox::SessionSandbox::ensure(conversation_id)
            .map(|p| p.display().to_string())?;
        log::info!(
            "resolve_effective_workspace: IM session sandbox for conversation_id={conversation_id}: {sandbox}",
            conversation_id = conversation_id,
            sandbox = sandbox
        );
        return Ok(sandbox);
    }

    // Try the last-active conversation's workspace (skip ourselves).
    if let Ok(store) = crate::conversation_store::global_store() {
        if let Ok(convs) = store.load_all() {
            for conv in &convs {
                if conv.id != conversation_id && !conv.workspace_root.trim().is_empty() {
                    let ws = conv.workspace_root.trim().to_string();
                    log::info!(
                        "resolve_effective_workspace: inheriting from conversation_id={}: {}",
                        conv.id,
                        ws
                    );
                    return Ok(ws);
                }
            }
        }
    }

    // Fallback: create a session sandbox.
    let sandbox = crate::session_sandbox::SessionSandbox::ensure(conversation_id)
        .map(|p| p.display().to_string())?;
    log::info!(
        "resolve_effective_workspace: using session sandbox for conversation_id={conversation_id}: {sandbox}",
        conversation_id = conversation_id,
        sandbox = sandbox
    );
    Ok(sandbox)
}
