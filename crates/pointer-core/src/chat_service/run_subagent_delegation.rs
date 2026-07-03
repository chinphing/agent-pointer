//! Lead-agent `run_subagent` tool: validate target, spawn sub loop, emit trace steps.

use std::path::Path;

use crate::agents::agent_ui::agent_display_label;
use crate::agents::AgentTask;
use crate::models::{AgentTrace, StreamEvent};
use crate::session_sandbox::SessionSandbox;
use crate::tools::run_subagent::{
    resolve_computer_operation_target, validate_spawn_depth,
};
use crate::provider::OpenAIProvider;
use anyhow::Result;

use super::emit::{agent_trace_step_id, emit, emit_agent_step};
use super::session_budget::SessionToolBudget;
use super::util::{new_id, truncate_str};

/// Emits `WorkspaceUpdated` on drop so the parent workspace is restored in the UI
/// after sub-agent delegation (thread-local is already restored by `AgentWorkspaceGuard`).
struct SubagentWorkspaceRestore {
    stream: super::StreamTx,
    conversation_id: String,
    prior_workspace: String,
    restore: bool,
}

impl Drop for SubagentWorkspaceRestore {
    fn drop(&mut self) {
        if !self.restore {
            return;
        }
        let ephemeral = SessionSandbox::is_sandbox(Path::new(self.prior_workspace.trim()))
            .unwrap_or(false);
        emit(
            &self.stream,
            StreamEvent::WorkspaceUpdated {
                conversation_id: self.conversation_id.clone(),
                workspace_root: self.prior_workspace.clone(),
                is_ephemeral_sandbox: ephemeral,
            },
        );
    }
}

fn emit_subagent_trace_step(
    stream: &super::StreamTx,
    ctx: &mut super::context::SubagentDelegationContext<'_>,
    agent: AgentTrace,
) {
    emit_agent_step(stream, ctx.message_id, ctx.agent_trace, agent);
    if let Some(history) = ctx.history.as_deref_mut() {
        super::sub_message::sync_anchor_agent_trace_index(
            ctx.session.conversation_id,
            history,
            ctx.message_id,
            ctx.agent_trace,
        );
    }
}

pub(super) async fn run_subagent_delegation(
    ctx: &mut super::context::SubagentDelegationContext<'_>,
) -> Result<(String, bool, Option<String>), anyhow::Error> {
    let stream = ctx.session.stream;
    let state = ctx.session.state;
    let provider = ctx.provider;
    let conversation_id = ctx.session.conversation_id;
    let parent_task_board_store_key = ctx.parent_task_board_store_key;
    let message_id = ctx.message_id;
    let tool_call_id = ctx.tool_call_id;
    let args_value = ctx.args_value.clone();
    let run_id = ctx.run_id;
    let allow_agents = ctx.allow_agents;
    let enabled_skill_ids = ctx.enabled_skill_ids;
    let cancel = ctx.session.cancel;
    let max_spawn_depth = provider.settings.max_sub_agent_spawn_depth.max(1);
    let parsed = crate::tools::run_subagent::parse_run_subagent_args(&args_value);
    match parsed {
        Err(msg) => Ok((format!("ERROR: {msg}"), false, Some(msg))),
        Ok(parsed) => {
            let child_spawn_depth = match validate_spawn_depth(ctx.parent_spawn_depth, max_spawn_depth)
            {
                Ok(d) => d,
                Err(msg) => return Ok((format!("ERROR: {msg}"), false, Some(msg))),
            };
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
                    let prior_workspace = sub_settings.workspace_root.clone();
                    let explicit_ws = if def.id == "coder" {
                        parsed.workspace_root.as_deref()
                    } else {
                        None
                    };
                    let workspace_restore =
                        match crate::workspace_delegation::ensure_subagent_workspace(
                            conversation_id,
                            explicit_ws,
                            &mut sub_settings,
                        ) {
                            Ok(ephemeral) => {
                                let workspace_changed = sub_settings.workspace_root.trim()
                                    != prior_workspace.trim();
                                if workspace_changed {
                                    emit(
                                        stream,
                                        StreamEvent::WorkspaceUpdated {
                                            conversation_id: conversation_id.to_string(),
                                            workspace_root: sub_settings.workspace_root.clone(),
                                            is_ephemeral_sandbox: ephemeral
                                                && explicit_ws.is_none()
                                                && prior_workspace.trim().is_empty(),
                                        },
                                    );
                                }
                                Some(SubagentWorkspaceRestore {
                                    stream: stream.clone(),
                                    conversation_id: conversation_id.to_string(),
                                    prior_workspace,
                                    restore: workspace_changed,
                                })
                            }
                            Err(e) => {
                                let msg = e.to_string();
                                log::warn!(
                                    "run_subagent workspace failed conversation_id={conversation_id} sub_agent={}: {msg}",
                                    def.id
                                );
                                return Ok((format!("ERROR: {msg}"), false, Some(msg)));
                            }
                        };
                    let _workspace_restore = workspace_restore;
                    let sub_provider =
                        OpenAIProvider::new(sub_settings, provider.api_key.clone());
                    let context = parsed.context.trim().to_string();
                    let task = AgentTask {
                        id: tid,
                        agent_id: agent_id.clone(),
                        title: if parsed.title.trim().is_empty() {
                            format!("Delegated: {agent_id}")
                        } else {
                            parsed.title
                        },
                        goal: parsed.goal,
                        context,
                        depends_on: vec![],
                    };
                    log::info!(
                        "run_subagent start conversation_id={} message_id={} sub_agent={} task_id={} spawn_depth={}",
                        conversation_id,
                        message_id,
                        def.id,
                        task.id,
                        child_spawn_depth
                    );
                    let detail = if task.title.len() > 200 {
                        format!("{}…", &task.title[..200])
                    } else {
                        task.title.clone()
                    };
                    let computer_target = (def.id == "computer").then(|| {
                        resolve_computer_operation_target(
                            &task.goal,
                            &task.context,
                            &task.title,
                            parsed.computer_target,
                        )
                    });
                    if def.id == "computer" {
                        log::info!(
                            "run_subagent computer target conversation_id={} task_id={} computer_target={:?}",
                            conversation_id,
                            task.id,
                            computer_target
                        );
                    }
                    let make_trace =
                        |status: &str, detail: Option<String>| AgentTrace {
                            id: agent_trace_step_id(&task.id, &def.id),
                            name: agent_display_label(&def),
                            role: def.role.clone(),
                            status: status.into(),
                            detail,
                            content: None,
                            depth: Some(child_spawn_depth),
                            session: None,
                            computer_target,
                        collapsed: false,
                        user_expanded: false,

                        };
                    emit_subagent_trace_step(
                        stream,
                        ctx,
                        make_trace("running", Some(detail)),
                    );
                    let sub_cap = provider
                        .settings
                        .max_sub_agent_tool_rounds
                        .clamp(1, 10_000);
                    let mut sub_budget = SessionToolBudget::new(sub_cap, 0);
                    let mut sub_ctx = super::context::SubAgentLoopContext {
                        session: super::context::SessionRefs {
                            stream,
                            state,
                            conversation_id,
                            cancel: &cancel,
                        },
                        provider: &sub_provider,
                        parent_task_board_store_key,
                        message_id,
                        agent_trace: ctx.agent_trace,
                        enabled_skill_ids,
                        task: &task,
                        sub_tool_budget: &mut sub_budget,
                        llm_stats: ctx.llm_stats,
                        run_id,
                        spawn_depth: child_spawn_depth,
                        max_spawn_depth,
                    };
                    match Box::pin(super::sub_agent::run_sub_agent(&mut sub_ctx))
                    .await
                    {
                        Ok(result) => {
                            log::info!(
                                "run_subagent completed conversation_id={} sub_agent={} task_id={} spawn_depth={}",
                                conversation_id,
                                result.agent_id,
                                result.task_id,
                                child_spawn_depth
                            );
                            let json = serde_json::to_string(&result).unwrap_or_else(|e| {
                                log::warn!("run_subagent result serialize failed: {e}");
                                r#"{"error":"serialize_failed"}"#.to_string()
                            });
                            emit_subagent_trace_step(
                                stream,
                                ctx,
                                make_trace(
                                    "completed",
                                    Some(truncate_str(&result.content, 160)),
                                ),
                            );
                            Ok((json, true, None))
                        }
                        Err(e) => {
                            log::warn!(
                                "run_subagent failed conversation_id={}: {e:#}",
                                conversation_id
                            );
                            emit_subagent_trace_step(
                                stream,
                                ctx,
                                make_trace("failed", Some(e.to_string())),
                            );
                            Ok((format!("ERROR: {e}"), false, None))
                        }
                    }
                }
            }
        }
    }
}
