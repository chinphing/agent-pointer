//! Route tool invocations to specialized handlers or the default registry.

mod ask_user;
mod media;
pub(super) mod registry;
mod skill_import;
pub(super) mod subagent;
pub(super) mod terminal;
pub(super) mod web_search;

use std::sync::atomic::AtomicBool;

use crate::agents::AgentProfile;
use crate::dispatcher::TriggerSource;
use crate::models::ToolCall;
use crate::provider::OpenAIProvider;
use tokio_util::sync::CancellationToken;

use super::super::app_state::AppState;
use super::super::app_state::ToolExecutionScope;
use super::super::StreamTx;
use super::types::{LeadToolPassConfig, SubToolPassConfig, ToolExecResult, ToolInvocationStats};

pub(super) async fn execute_tool_invocation(
    stream: &StreamTx,
    state: &AppState,
    provider: &OpenAIProvider,
    conversation_id: &str,
    parent_task_board_store_key: &str,
    message_id: &str,
    history: &mut Vec<crate::models::ChatMessage>,
    tc: &ToolCall,
    tool_id: &str,
    args_value: serde_json::Value,
    lead: Option<&mut LeadToolPassConfig<'_>>,
    sub: Option<&mut SubToolPassConfig<'_>>,
    cancel: &CancellationToken,
    stats: &mut ToolInvocationStats<'_>,
    trigger_source: &Option<TriggerSource>,
    ask_user_deferred: &AtomicBool,
) -> ToolExecResult {
    let _ = ask_user_deferred;
    let execution_scope = ToolExecutionScope::from_agent_contexts(
        conversation_id,
        lead.as_ref()
            .map(|config| config.instance_scope.agent_instance_id.as_str()),
        sub.as_ref()
            .map(|config| config.instance_scope.agent_instance_id.as_str()),
        tc.id.as_str(),
    );
    match tool_id {
        "ask_user" => {
            ask_user::dispatch_ask_user(
                stream,
                state,
                tc,
                args_value,
                cancel,
                trigger_source,
                conversation_id,
                message_id,
            )
            .await
        }
        "terminal" => {
            terminal::run_terminal_tool(
                stream,
                state,
                conversation_id,
                message_id,
                tc,
                args_value,
                cancel,
                sub.as_ref().map(|s| s.trace_id.clone()),
                sub.as_ref().map(|s| s.scoped_message_id.clone()),
                provider.settings.workspace_root.clone(),
                execution_scope,
            )
            .await
        }
        "web_search" => {
            let lead_scope = match stats {
                ToolInvocationStats::TokenSession(session) => Some(&session.lead_scope),
                ToolInvocationStats::Conversation(_) => None,
            };
            let invocation = web_search::prepare_web_search_invocation(
                lead_scope,
                lead.as_ref().map(|l| l.lead_agent_id),
                sub.as_deref(),
            )?;
            web_search::dispatch_web_search(
                stream,
                provider,
                message_id,
                history,
                tc,
                args_value,
                cancel,
                invocation,
            )
            .await
        }
        "image_generate" | "video_generate" => {
            media::dispatch_media_generate(
                stream,
                provider,
                conversation_id,
                message_id,
                tc,
                tool_id,
                args_value,
                cancel,
                lead.as_ref().map(|l| l.run_id),
                sub.as_ref().map(|s| s.instance_scope.run_id.as_str()),
            )
            .await
        }
        "media_understand" => {
            media::dispatch_media_understand(
                stream,
                provider,
                conversation_id,
                message_id,
                tc,
                args_value,
                cancel,
                lead.as_ref().map(|l| l.run_id),
                sub.as_ref().map(|s| s.instance_scope.run_id.as_str()),
            )
            .await
        }
        "run_subagent" if lead.is_some() || sub.is_some() => {
            subagent::dispatch_run_subagent(
                stream,
                state,
                provider,
                conversation_id,
                parent_task_board_store_key,
                message_id,
                tc,
                args_value,
                cancel,
                stats,
                history,
                lead,
                sub,
            )
            .await
        }
        "skill_import" => skill_import::dispatch_skill_import(
            stream,
            state,
            conversation_id,
            tool_id,
            args_value,
            lead,
        ),
        _ => registry::dispatch_registry_invoke(
            state,
            conversation_id,
            tool_id,
            args_value,
            &provider.settings.workspace_root,
            lead.as_deref(),
            sub.as_deref(),
            execution_scope,
        )
        .await,
    }
}

/// Parallel-safe invoke path (no shared mut `lead` / `sub` / `history`).
pub(super) async fn invoke_prepared_parallel(
    stream: &StreamTx,
    state: &AppState,
    provider: &OpenAIProvider,
    conversation_id: &str,
    _task_board_store_key: &str,
    message_id: &str,
    tc: &ToolCall,
    tool_id: &str,
    args_value: serde_json::Value,
    workspace_root: &str,
    lead_profile: Option<AgentProfile>,
    sub_profile: Option<AgentProfile>,
    lead_run_id: Option<&str>,
    sub_run_id: Option<&str>,
    agent_instance_id: Option<&str>,
    web_search_invocation: Option<web_search::WebSearchInvocation>,
    web_search_history: Option<&[crate::models::ChatMessage]>,
    cancel: &CancellationToken,
) -> ToolExecResult {
    let execution_scope =
        ToolExecutionScope::new(conversation_id, agent_instance_id, tc.id.as_str());
    match tool_id {
        "ask_user" => Err(anyhow::anyhow!("ask_user must not run in parallel wave")),
        "terminal" => {
            terminal::run_terminal_tool(
                stream,
                state,
                conversation_id,
                message_id,
                tc,
                args_value,
                cancel,
                None,
                None,
                provider.settings.workspace_root.clone(),
                execution_scope,
            )
            .await
        }
        "web_search" => {
            let invocation = web_search_invocation
                .ok_or_else(|| anyhow::anyhow!("web_search invocation context missing"))?;
            let history = web_search_history
                .ok_or_else(|| anyhow::anyhow!("web_search history context missing"))?;
            web_search::dispatch_web_search(
                stream,
                provider,
                message_id,
                history,
                tc,
                args_value,
                cancel,
                invocation,
            )
            .await
        }
        "image_generate" | "video_generate" => {
            media::dispatch_media_generate(
                stream,
                provider,
                conversation_id,
                message_id,
                tc,
                tool_id,
                args_value,
                cancel,
                lead_run_id,
                sub_run_id,
            )
            .await
        }
        "media_understand" => {
            media::dispatch_media_understand(
                stream,
                provider,
                conversation_id,
                message_id,
                tc,
                args_value,
                cancel,
                lead_run_id,
                sub_run_id,
            )
            .await
        }
        "run_subagent" => Err(anyhow::anyhow!(
            "run_subagent must not run in parallel wave"
        )),
        _ => registry::dispatch_registry_invoke_with_profile(
            state,
            conversation_id,
            tool_id,
            args_value,
            workspace_root,
            lead_profile
                .or(sub_profile)
                .unwrap_or(AgentProfile::General),
            execution_scope,
        )
        .await,
    }
}
