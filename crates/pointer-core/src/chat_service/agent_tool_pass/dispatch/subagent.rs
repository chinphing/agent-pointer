//! Nested `run_subagent` delegation (lead or sub scope).

use super::super::types::{LeadToolPassConfig, SubToolPassConfig, ToolExecResult, ToolInvocationStats};

pub(super) async fn dispatch_run_subagent(
    stream: &super::super::super::StreamTx,
    state: &super::super::super::app_state::AppState,
    provider: &crate::provider::OpenAIProvider,
    conversation_id: &str,
    parent_task_board_store_key: &str,
    message_id: &str,
    tc: &crate::models::ToolCall,
    args_value: serde_json::Value,
    cancel: &tokio_util::sync::CancellationToken,
    stats: &mut ToolInvocationStats<'_>,
    lead: Option<&mut LeadToolPassConfig<'_>>,
    sub: Option<&mut SubToolPassConfig<'_>>,
) -> ToolExecResult {
    let llm_stats = match stats {
        ToolInvocationStats::TokenSession(s) => &mut s.stats,
        ToolInvocationStats::Conversation(s) => s,
    };
    if let Some(lead_cfg) = lead {
        let mut deleg = super::super::super::context::SubagentDelegationContext {
            session: super::super::super::context::SessionRefs {
                stream,
                state,
                conversation_id,
                cancel,
            },
            parent_task_board_store_key,
            message_id,
            provider,
            run_id: lead_cfg.run_id,
            allow_agents: lead_cfg.allow_agents,
            enabled_skill_ids: lead_cfg.enabled_skill_ids.as_slice(),
            agent_trace: lead_cfg.agent_trace,
            llm_stats,
            tool_call_id: &tc.id,
            args_value,
        };
        return super::super::super::run_subagent_delegation::run_subagent_delegation(&mut deleg)
            .await;
    }
    if let Some(sub_cfg) = sub {
        let empty_skills: &[String] = &[];
        let mut deleg = super::super::super::context::SubagentDelegationContext {
            session: super::super::super::context::SessionRefs {
                stream,
                state,
                conversation_id,
                cancel,
            },
            parent_task_board_store_key,
            message_id,
            provider,
            run_id: &sub_cfg.instance_scope.run_id,
            allow_agents: sub_cfg.allow_agents,
            enabled_skill_ids: empty_skills,
            agent_trace: sub_cfg.agent_trace,
            llm_stats,
            tool_call_id: &tc.id,
            args_value,
        };
        return super::super::super::run_subagent_delegation::run_subagent_delegation(&mut deleg)
            .await;
    }
    unreachable!("run_subagent dispatch requires lead or sub scope")
}
