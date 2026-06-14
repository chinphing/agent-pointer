//! Route tool invocations to specialized handlers or the default registry.

mod media;
mod registry;
mod skill_import;
mod subagent;
mod terminal;
mod web_search;

use crate::models::ToolCall;
use crate::provider::OpenAIProvider;
use tokio_util::sync::CancellationToken;

use super::super::app_state::AppState;
use super::super::StreamTx;
use super::types::{LeadToolPassConfig, SubToolPassConfig, ToolExecResult, ToolInvocationStats};

pub(super) async fn execute_tool_invocation(
    stream: &StreamTx,
    state: &AppState,
    provider: &OpenAIProvider,
    conversation_id: &str,
    parent_task_board_store_key: &str,
    message_id: &str,
    history: &[crate::models::ChatMessage],
    tc: &ToolCall,
    tool_id: &str,
    args_value: serde_json::Value,
    mut lead: Option<&mut LeadToolPassConfig<'_>>,
    sub: Option<&mut SubToolPassConfig<'_>>,
    cancel: &CancellationToken,
    stats: &mut ToolInvocationStats<'_>,
) -> ToolExecResult {
    match tool_id {
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
                provider.settings.workspace_root.clone(),
            )
            .await
        }
        "web_search" => {
            web_search::dispatch_web_search(
                stream,
                provider,
                message_id,
                history,
                tc,
                args_value,
                cancel,
                stats,
                lead.as_ref().map(|l| l.lead_agent_id),
                sub,
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
            lead.as_deref(),
            sub.as_deref(),
        ),
    }
}
