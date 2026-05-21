//! Lead-agent `run_subagent` tool: validate target, spawn sub loop, emit trace steps.

use crate::agents::AgentTask;
use crate::llm_token_stats::ConversationLlmStats;
use crate::models::AgentTrace;
use crate::provider::OpenAIProvider;
use anyhow::Result;
use tokio_util::sync::CancellationToken;

use super::app_state::AppState;
use super::emit::{agent_trace_step_id, emit_agent_step};
use super::session_budget::SessionToolBudget;
use super::util::{new_id, truncate_str};
use super::StreamTx;

pub(super) async fn run_subagent_delegation(
    stream: &StreamTx,
    state: &AppState,
    provider: &OpenAIProvider,
    conversation_id: &str,
    message_id: &str,
    args_value: serde_json::Value,
    allow_agents: &[String],
    enabled_skill_ids: &[String],
    agent_trace: &mut Vec<AgentTrace>,
    cancel: &CancellationToken,
    llm_stats: &mut ConversationLlmStats,
) -> Result<(String, bool, Option<String>), anyhow::Error> {
    let parsed = crate::tools::run_subagent::parse_run_subagent_args(&args_value);
    match parsed {
        Err(msg) => Ok((format!("ERROR: {msg}"), false, Some(msg))),
        Ok((agent_id, instruction, title, task_id_raw)) => {
            match crate::tools::run_subagent::validate_run_subagent_target(
                &state.agents,
                allow_agents,
                &agent_id,
            ) {
                Err(msg) => Ok((format!("ERROR: {msg}"), false, Some(msg))),
                Ok(def) => {
                    let tid = if task_id_raw.trim().is_empty() {
                        new_id("sub_task")
                    } else {
                        task_id_raw.trim().to_string()
                    };
                    let task = AgentTask {
                        id: tid,
                        agent_id: agent_id.clone(),
                        title: if title.trim().is_empty() {
                            format!("Delegated: {agent_id}")
                        } else {
                            title
                        },
                        instruction,
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
                            name: def.name.clone(),
                            role: def.role.clone(),
                            status: "running".into(),
                            detail: Some(detail),
                            content: None,
                            depth: Some(1),
                        },
                    );
                    let sub_cap = provider
                        .settings
                        .max_sub_agent_tool_rounds
                        .clamp(1, 10_000);
                    let mut sub_budget = SessionToolBudget::new(sub_cap, 0);
                    match Box::pin(super::sub_agent::run_sub_agent(
                        provider,
                        state,
                        stream,
                        conversation_id,
                        message_id,
                        agent_trace,
                        enabled_skill_ids,
                        &task,
                        &mut sub_budget,
                        cancel.clone(),
                        true,
                        llm_stats,
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
                                    name: def.name.clone(),
                                    role: def.role.clone(),
                                    status: "completed".into(),
                                    detail: Some(truncate_str(&result.content, 160)),
                                    content: Some(result.content.clone()),
                                    depth: Some(1),
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
                                    name: def.name.clone(),
                                    role: def.role.clone(),
                                    status: "failed".into(),
                                    detail: Some(e.to_string()),
                                    content: None,
                                    depth: Some(1),
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
