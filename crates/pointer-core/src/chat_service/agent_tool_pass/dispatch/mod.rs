//! Route tool invocations to specialized handlers or the default registry.

mod media;
pub(super) mod registry;
mod skill_import;
pub(super) mod subagent;
pub(super) mod terminal;
pub(super) mod web_search;

use self::registry::resolve_workspace_root;
use crate::agents::AgentProfile;
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
    history: &mut Vec<crate::models::ChatMessage>,
    tc: &ToolCall,
    tool_id: &str,
    args_value: serde_json::Value,
    lead: Option<&mut LeadToolPassConfig<'_>>,
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
                sub.as_ref().map(|s| s.scoped_message_id.clone()),
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
        ),
    }
}

/// Parallel-safe invoke path (no shared mut `lead` / `sub` / `history`).
pub(super) async fn invoke_prepared_parallel(
    stream: &StreamTx,
    state: &AppState,
    provider: &OpenAIProvider,
    conversation_id: &str,
    task_board_store_key: &str,
    message_id: &str,
    tc: &ToolCall,
    tool_id: &str,
    args_value: serde_json::Value,
    workspace_root: &str,
    lead_profile: Option<AgentProfile>,
    sub_profile: Option<AgentProfile>,
    lead_run_id: Option<&str>,
    sub_run_id: Option<&str>,
    cancel: &CancellationToken,
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
                None,
                None,
                provider.settings.workspace_root.clone(),
            )
            .await
        }
        "web_search" => {
            let mut stats =
                ToolInvocationStats::Conversation(&mut crate::llm_token_stats::ConversationLlmStats::default());
            web_search::dispatch_web_search(
                stream,
                provider,
                message_id,
                &[],
                tc,
                args_value,
                cancel,
                &mut stats,
                None,
                None,
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
        _ => {
            let file_profile = lead_profile
                .or(sub_profile)
                .unwrap_or(AgentProfile::General);
            let _file_guard =
                crate::agents::FileToolLeadProfileGuard::enter(file_profile.clone());

            let session_user_id = state
                .session_index
                .session_user_id(conversation_id)
                .unwrap_or_default();
            let resolved_ws = resolve_workspace_root(conversation_id, &session_user_id, workspace_root);
            let _ws =
                crate::tools::file::ConversationWorkspaceGuard::enter(resolved_ws);

            if file_profile == AgentProfile::Computer {
                let _tier = crate::agents::computer::ComputerTierGuard::enter(
                    state.computer_state.tier_for_conversation(conversation_id),
                );
                let _ = task_board_store_key;
            }
            state
                .tools
                .invoke(tool_id, args_value)
                .map(|out| (out, true, None))
        }
    }
}
