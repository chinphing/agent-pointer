//! Route tool invocations to specialized handlers or the default registry.

mod ask_user;
mod job;
mod locate;
mod grounding;
mod input;
mod media;
mod modified_click;
mod mouse;
pub(super) mod registry;
mod skill_import;
pub(super) mod subagent;
pub(super) mod terminal;
pub(super) mod web_search;

use std::sync::atomic::AtomicBool;

use crate::agents::AgentProfile;
use crate::chat_service::job_supervisor::JobCaller;
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
    run_id: Option<&str>,
    tool_span_id: Option<&str>,
    state_arc: std::sync::Arc<AppState>,
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
                state_arc,
                conversation_id,
                message_id,
                tc,
                args_value,
                cancel,
                sub.as_ref().map(|s| s.trace_id.clone()),
                sub.as_ref().map(|s| s.scoped_message_id.clone()),
                provider.settings.workspace_root.clone(),
                execution_scope,
                run_id.unwrap_or(""),
                sub.as_ref()
                    .map(|config| config.agent_chain.as_slice())
                    .unwrap_or(&[]),
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
                stream, provider, message_id, history, tc, args_value, cancel, invocation,
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
        // Mouse tools accept either x/y or a description. With a description the
        // tool grounds first, and grounding needs the provider — which the sync
        // ToolHandler closure does not carry. Hence the async path.
        //
        // `mouse_drag_from_to_at` must be here too: it grounds **two** points
        // (grab and drop) and rewrites them into x1/y1/x2/y2 before the tool
        // runs. Falling through to `dispatch_registry_invoke` skips that rewrite
        // and the tool sees `from_bbox`/`to_bbox` with no coordinates.
        "mouse_click_at" | "mouse_double_click_at" | "mouse_right_click_at"
        | "mouse_hover_at" | "mouse_drag_from_to_at" | "mouse_down" | "mouse_up" => {
            mouse::dispatch_mouse(state, conversation_id, provider, tc, args_value, cancel).await
        }
        // input_at takes {target_desc, bbox} like the pointing tools: ground
        // first, then focus/clear/type. Same provider requirement.
        "input_at" => {
            input::dispatch_input_at(state, conversation_id, provider, tc, args_value, cancel).await
        }
        // Multi-select by described targets — grounds each position before
        // touching the mouse, so a partial selection cannot happen.
        "modified_click_select_at" | "modified_click_range_select_at" => {
            modified_click::dispatch_modified_click(
                state,
                conversation_id,
                provider,
                tc,
                args_value,
                cancel,
            )
            .await
        }
        "locate_element" => {
            locate::dispatch_locate_element(
                state,
                conversation_id,
                provider,
                tc,
                args_value,
                cancel,
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
                state_arc,
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
        "job" => {
            let caller = match sub.as_ref() {
                Some(config) => JobCaller::Instance(
                    config.instance_scope.agent_instance_id.as_str(),
                ),
                None => JobCaller::Lead,
            };
            job::dispatch_job(state, conversation_id, args_value, cancel, caller).await
        }
        _ => {
            registry::dispatch_registry_invoke(
                state,
                conversation_id,
                tool_id,
                args_value,
                &provider.settings.workspace_root,
                lead.as_deref(),
                sub.as_deref(),
                execution_scope,
                run_id,
                tool_span_id,
            )
            .await
        }
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
    // Issuer chain of a sub-agent wave member (empty/None = lead).
    agent_chain: Option<Vec<String>>,
    web_search_invocation: Option<web_search::WebSearchInvocation>,
    web_search_history: Option<&[crate::models::ChatMessage]>,
    cancel: &CancellationToken,
    tool_span_id: Option<String>,
    state_arc: std::sync::Arc<AppState>,
) -> ToolExecResult {
    let execution_scope =
        ToolExecutionScope::new(conversation_id, agent_instance_id, tc.id.as_str());
    match tool_id {
        "ask_user" => Err(anyhow::anyhow!("ask_user must not run in parallel wave")),
        "terminal" => {
            let run_id = sub_run_id.or(lead_run_id).unwrap_or("");
            let owner_chain: &[String] = if sub_run_id.is_some() {
                agent_chain.as_deref().unwrap_or(&[])
            } else {
                &[]
            };
            terminal::run_terminal_tool(
                stream,
                state,
                state_arc,
                conversation_id,
                message_id,
                tc,
                args_value,
                cancel,
                None,
                None,
                provider.settings.workspace_root.clone(),
                execution_scope,
                run_id,
                owner_chain,
            )
            .await
        }
        "web_search" => {
            let invocation = web_search_invocation
                .ok_or_else(|| anyhow::anyhow!("web_search invocation context missing"))?;
            let history = web_search_history
                .ok_or_else(|| anyhow::anyhow!("web_search history context missing"))?;
            web_search::dispatch_web_search(
                stream, provider, message_id, history, tc, args_value, cancel, invocation,
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
        // Registered as `ToolConflictClass::Computer`, so the batch planner
        // runs it serially; this arm exists for completeness/symmetry.
        // Mouse tools accept either x/y or a description. With a description the
        // tool grounds first, and grounding needs the provider — which the sync
        // ToolHandler closure does not carry. Hence the async path.
        "mouse_click_at" | "mouse_double_click_at" | "mouse_right_click_at"
        | "mouse_hover_at" | "mouse_drag_from_to_at" | "mouse_down" | "mouse_up" => {
            mouse::dispatch_mouse(state, conversation_id, provider, tc, args_value, cancel).await
        }
        // input_at takes {target_desc, bbox} like the pointing tools: ground
        // first, then focus/clear/type. Same provider requirement.
        "input_at" => {
            input::dispatch_input_at(state, conversation_id, provider, tc, args_value, cancel).await
        }
        // Multi-select by described targets — grounds each position before
        // touching the mouse, so a partial selection cannot happen.
        "modified_click_select_at" | "modified_click_range_select_at" => {
            modified_click::dispatch_modified_click(
                state,
                conversation_id,
                provider,
                tc,
                args_value,
                cancel,
            )
            .await
        }
        "locate_element" => {
            locate::dispatch_locate_element(
                state,
                conversation_id,
                provider,
                tc,
                args_value,
                cancel,
            )
            .await
        }
        _ => {
            registry::dispatch_registry_invoke_with_profile(
                state,
                conversation_id,
                tool_id,
                args_value,
                workspace_root,
                lead_profile
                    .or(sub_profile)
                    .unwrap_or(AgentProfile::General),
                execution_scope,
                lead_run_id.or(sub_run_id),
                tool_span_id.as_deref(),
            )
            .await
        }
    }
}
