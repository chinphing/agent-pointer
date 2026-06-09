//! Inner orchestration (`run_chat_inner`): settings, compression, supervisor vs single-agent loop.

use crate::agents::{
    delegatable_sub_agents_system_block, AgentOrchestrator, AGENT_MODE_SUPERVISOR,
};
use crate::llm_token_stats::ChatLlmTokenSession;
use crate::tools::file::ConversationWorkspaceGuard;
use crate::models::{effective_reasoning_in_messages, ChatMessage};
use crate::provider::OpenAIProvider;
use anyhow::{anyhow, Result};
use std::sync::Arc;
use std::time::Instant;
use tokio_util::sync::CancellationToken;

use super::app_state::AppState;
use super::session_budget::SessionToolBudget;
use super::session_model::prepare_session_llm_settings;
use super::StreamTx;

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
    tool_rounds_used_single_start: u32,
    tool_rounds_used_supervisor_start: u32,
    workspace_root: String,
    consumed_single: &mut u32,
    consumed_supervisor: &mut u32,
    cancel: CancellationToken,
    run_id: &str,
) -> Result<()> {
    let _workspace_guard = ConversationWorkspaceGuard::enter(workspace_root.clone());
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
    if !workspace_root.trim().is_empty() {
        settings.workspace_root = workspace_root.trim().to_string();
    }
    let tool_approval_mode = settings.tool_approval_mode.clone();
    let effective_agent_mode = request_agent_mode
        .filter(|mode| !mode.trim().is_empty())
        .unwrap_or(&settings.agent_mode)
        .to_string();
    let api_key = prepare_session_llm_settings(&mut settings, &effective_agent_mode);
    if api_key.is_empty() {
        return Err(anyhow!(
            "尚未配置 API Key（{}），请先登录平台账户或在设置中配置密钥",
            settings.active_provider_id
        ));
    }
    let lead_worker_id = settings.lead_agent_id.trim();
    let lead_opt = if lead_worker_id.is_empty() {
        None
    } else {
        Some(lead_worker_id)
    };
    let agent_plan = AgentOrchestrator::build_plan(
        &state.agents,
        &state.skills,
        &state.tools,
        enabled_skill_ids,
        &effective_agent_mode,
        lead_opt,
    );
    let mut agent_plan = agent_plan;
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
    let lead_role = if lead_worker_id.is_empty() {
        effective_agent_mode.clone()
    } else {
        lead_worker_id.to_string()
    };
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

    let review_due = crate::memory::should_run_memory_review(
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

    if review_due {
        crate::memory::spawn_memory_background_review(
            state,
            provider,
            conversation_id.to_string(),
            history.clone(),
            settings,
            stream,
        );
    }

    Ok(())
}
