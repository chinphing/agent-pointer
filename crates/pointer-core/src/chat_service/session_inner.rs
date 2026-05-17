//! Inner orchestration (`run_chat_inner`): settings, compression, supervisor vs single-agent loop.

use crate::agents::{
    delegatable_sub_agents_system_block, AgentOrchestrator, AGENT_MODE_SUPERVISOR,
};
use crate::llm_token_stats::ChatLlmTokenSession;
use crate::models::{effective_reasoning_in_messages, ChatMessage};
use crate::provider::OpenAIProvider;
use crate::storage;
use anyhow::{anyhow, Result};
use std::sync::Arc;
use std::time::Instant;
use tokio_util::sync::CancellationToken;

use super::app_state::AppState;
use super::session_budget::SessionToolBudget;
use super::session_model::apply_session_agent_model_defaults;
use super::StreamTx;

pub(super) async fn run_chat_inner(
    stream: StreamTx,
    state: Arc<AppState>,
    conversation_id: &str,
    history: &mut Vec<ChatMessage>,
    enabled_skill_ids: &[String],
    request_agent_mode: Option<&str>,
    tool_rounds_used_single_start: u32,
    tool_rounds_used_supervisor_start: u32,
    consumed_single: &mut u32,
    consumed_supervisor: &mut u32,
    cancel: CancellationToken,
) -> Result<()> {
    let mut settings = storage::load_settings()?;
    let api_key = storage::load_api_key()?
        .ok_or_else(|| anyhow!("尚未配置 API Key，请先在设置中保存密钥"))?;
    settings.api_key = api_key.clone();
    let tool_approval_mode = settings.tool_approval_mode.clone();
    let effective_agent_mode = request_agent_mode
        .filter(|mode| !mode.trim().is_empty())
        .unwrap_or(&settings.agent_mode)
        .to_string();
    apply_session_agent_model_defaults(&mut settings, &effective_agent_mode);
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
    let mut llm_token_session = ChatLlmTokenSession::new(conversation_id.to_string());

    let t_compress = Instant::now();
    crate::context_compression::maybe_compress_history(
        history,
        &settings,
        &provider,
        conversation_id,
        &stream,
        cancel.clone(),
        crate::context_compression::CompressionUiContext::main(),
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
                "本会话在编排（Supervisor）模式下工具调用轮次已达上限（{}），请新开对话或在设置中调高上限。",
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

    super::single_agent::run_single_agent_loop(
        stream,
        state,
        conversation_id,
        history,
        enabled_skill_ids,
        &agent_plan,
        &provider,
        &settings,
        tool_approval_mode.as_str(),
        &mut tool_budget,
        consumed_single,
        max_cap,
        cancel,
        &mut llm_token_session,
        reasoning_in_messages,
    )
    .await
}
