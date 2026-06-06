//! Lead-agent `run_subagent` tool: validate target, spawn sub loop, emit trace steps.

use crate::agents::agent_ui::agent_display_label;
use crate::agents::AgentTask;
use crate::llm_token_stats::ConversationLlmStats;
use crate::models::{AgentTrace, StreamEvent};
use crate::provider::OpenAIProvider;
use anyhow::Result;
use tokio_util::sync::CancellationToken;

use super::app_state::AppState;
use super::emit::{agent_trace_step_id, emit, emit_agent_step};
use super::session_budget::SessionToolBudget;
use super::util::{new_id, truncate_str};
use super::StreamTx;

pub(super) async fn run_subagent_delegation(
    stream: &StreamTx,
    state: &AppState,
    provider: &OpenAIProvider,
    conversation_id: &str,
    parent_task_board_store_key: &str,
    message_id: &str,
    tool_call_id: &str,
    args_value: serde_json::Value,
    run_id: &str,
    allow_agents: &[String],
    enabled_skill_ids: &[String],
    agent_trace: &mut Vec<AgentTrace>,
    cancel: &CancellationToken,
    llm_stats: &mut ConversationLlmStats,
) -> Result<(String, bool, Option<String>), anyhow::Error> {
    let parsed = crate::tools::run_subagent::parse_run_subagent_args(&args_value);
    match parsed {
        Err(msg) => Ok((format!("ERROR: {msg}"), false, Some(msg))),
        Ok(parsed) => {
            let agent_id = parsed.agent_id;
            match crate::tools::run_subagent::validate_run_subagent_target(
                &state.agents,
                allow_agents,
                &agent_id,
            ) {
                Err(msg) => Ok((format!("ERROR: {msg}"), false, Some(msg))),
                Ok(def) => {
                    let tid = if parsed.task_id.trim().is_empty() {
                        new_id("sub_task")
                    } else {
                        parsed.task_id.trim().to_string()
                    };
                    if def.id == "computer" {
                        if let Err(e) = super::computer_monitor_pick::ensure_computer_monitor_for_subagent(
                            stream,
                            state,
                            &provider.settings,
                            conversation_id,
                            message_id,
                            tool_call_id,
                            cancel,
                        )
                        .await
                        {
                            let msg = e.to_string();
                            log::warn!(
                                "run_subagent computer monitor pick failed conversation_id={conversation_id}: {msg}"
                            );
                            return Ok((format!("ERROR: {msg}"), false, Some(msg)));
                        }
                    }
                    let mut sub_settings = provider.settings.clone();
                    if def.id == "coder" {
                        match crate::workspace_delegation::ensure_coder_delegation_workspace(
                            conversation_id,
                            parsed.workspace_root.as_deref(),
                        ) {
                            Ok((root, ephemeral)) => {
                                sub_settings.workspace_root = root.clone();
                                emit(
                                    stream,
                                    StreamEvent::WorkspaceUpdated {
                                        conversation_id: conversation_id.to_string(),
                                        workspace_root: root,
                                        is_ephemeral_sandbox: ephemeral,
                                    },
                                );
                            }
                            Err(e) => {
                                let msg = e.to_string();
                                log::warn!(
                                    "run_subagent coder workspace failed conversation_id={conversation_id}: {msg}"
                                );
                                return Ok((format!("ERROR: {msg}"), false, Some(msg)));
                            }
                        }
                    }
                    let sub_provider =
                        OpenAIProvider::new(sub_settings, provider.api_key.clone());
                    let task = AgentTask {
                        id: tid,
                        agent_id: agent_id.clone(),
                        title: if parsed.title.trim().is_empty() {
                            format!("Delegated: {agent_id}")
                        } else {
                            parsed.title
                        },
                        instruction: parsed.instruction,
                        depends_on: vec![],
                    };
                    log::info!(
                        "run_subagent start conversation_id={} message_id={} sub_agent={} task_id={}",
                        conversation_id,
                        message_id,
                        def.id,
                        task.id
                    );
                    let detail = if task.title.len() > 200 {
                        format!("{}…", &task.title[..200])
                    } else {
                        task.title.clone()
                    };
                    emit_agent_step(
                        stream,
                        message_id,
                        agent_trace,
                        AgentTrace {
                            id: agent_trace_step_id(&task.id, &def.id),
                            name: agent_display_label(&def),
                            role: def.role.clone(),
                            status: "running".into(),
                            detail: Some(detail),
                            content: None,
                            depth: Some(1),
                            session: None,
                        },
                    );
                    let sub_cap = provider
                        .settings
                        .max_sub_agent_tool_rounds
                        .clamp(1, 10_000);
                    let mut sub_budget = SessionToolBudget::new(sub_cap, 0);
                    match Box::pin(super::sub_agent::run_sub_agent(
                        &sub_provider,
                        state,
                        stream,
                        conversation_id,
                        parent_task_board_store_key,
                        message_id,
                        agent_trace,
                        enabled_skill_ids,
                        &task,
                        &mut sub_budget,
                        cancel.clone(),
                        true,
                        llm_stats,
                        run_id,
                    ))
                    .await
                    {
                        Ok(result) => {
                            log::info!(
                                "run_subagent completed conversation_id={} sub_agent={} task_id={}",
                                conversation_id,
                                result.agent_id,
                                result.task_id
                            );
                            let json = serde_json::to_string(&result).unwrap_or_else(|e| {
                                log::warn!("run_subagent result serialize failed: {e}");
                                r#"{"error":"serialize_failed"}"#.to_string()
                            });
                            emit_agent_step(
                                stream,
                                message_id,
                                agent_trace,
                                AgentTrace {
                                    id: agent_trace_step_id(&task.id, &def.id),
                                    name: agent_display_label(&def),
                                    role: def.role.clone(),
                                    status: "completed".into(),
                                    detail: Some(truncate_str(&result.content, 160)),
                                    content: None,
                                    depth: Some(1),
                                    session: None,
                                },
                            );
                            Ok((json, true, None))
                        }
                        Err(e) => {
                            log::warn!(
                                "run_subagent failed conversation_id={}: {e:#}",
                                conversation_id
                            );
                            emit_agent_step(
                                stream,
                                message_id,
                                agent_trace,
                                AgentTrace {
                                    id: agent_trace_step_id(&task.id, &def.id),
                                    name: agent_display_label(&def),
                                    role: def.role.clone(),
                                    status: "failed".into(),
                                    detail: Some(e.to_string()),
                                    content: None,
                                    depth: Some(1),
                                    session: None,
                                },
                            );
                            Ok((format!("ERROR: {e}"), false, None))
                        }
                    }
                }
            }
        }
    }
}
