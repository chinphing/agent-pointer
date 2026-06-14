//! Nested `run_subagent` delegation (lead or sub scope).

use crate::models::ToolCall;
use crate::provider::OpenAIProvider;
use tokio_util::sync::CancellationToken;

use super::super::super::app_state::AppState;
use super::super::super::StreamTx;
use super::super::types::{LeadToolPassConfig, SubToolPassConfig, ToolExecResult, ToolInvocationStats};

pub(super) async fn dispatch_run_subagent(
    stream: &StreamTx,
    state: &AppState,
    provider: &OpenAIProvider,
    conversation_id: &str,
    parent_task_board_store_key: &str,
    message_id: &str,
    tc: &ToolCall,
    args_value: serde_json::Value,
    cancel: &CancellationToken,
    stats: &mut ToolInvocationStats<'_>,
    lead: Option<&mut LeadToolPassConfig<'_>>,
    sub: Option<&mut SubToolPassConfig<'_>>,
) -> ToolExecResult {
    let llm_stats = match stats {
        ToolInvocationStats::TokenSession(s) => &mut s.stats,
        ToolInvocationStats::Conversation(s) => s,
    };
    if let Some(lead_cfg) = lead {
        return super::super::super::run_subagent_delegation::run_subagent_delegation(
            stream,
            state,
            provider,
            conversation_id,
            parent_task_board_store_key,
            message_id,
            &tc.id,
            args_value,
            lead_cfg.run_id,
            lead_cfg.allow_agents,
            lead_cfg.enabled_skill_ids.as_slice(),
            lead_cfg.agent_trace,
            cancel,
            llm_stats,
        )
        .await;
    }
    if let Some(sub_cfg) = sub {
        let empty_skills: &[String] = &[];
        return super::super::super::run_subagent_delegation::run_subagent_delegation(
            stream,
            state,
            provider,
            conversation_id,
            parent_task_board_store_key,
            message_id,
            &tc.id,
            args_value,
            &sub_cfg.instance_scope.run_id,
            sub_cfg.allow_agents,
            empty_skills,
            sub_cfg.agent_trace,
            cancel,
            llm_stats,
        )
        .await;
    }
    unreachable!("run_subagent dispatch requires lead or sub scope")
}
