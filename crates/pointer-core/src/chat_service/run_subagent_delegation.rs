//! Lead-agent `run_subagent` tool: validate target, spawn sub loop, emit trace steps.

use std::path::Path;
use std::time::Instant;

use crate::agent_instance_scope::AgentInstanceScope;
use crate::agents::agent_ui::agent_display_label;
use crate::agents::{AgentDef, AgentRunResult, AgentTask};
use crate::chat_service::self_fork::SelfForkSnapshot;
use crate::llm_token_stats::ConversationLlmStats;
use crate::models::ChatMessage;
use crate::models::{AgentTrace, ComputerOperationTarget, StreamEvent};
use crate::provider::OpenAIProvider;
use crate::session_sandbox::SessionSandbox;
use crate::tools::run_subagent::{
    resolve_computer_operation_target, validate_run_subagent_workspace, validate_spawn_depth,
};
use anyhow::Result;

use super::app_state::AppState;
use super::emit::{emit, emit_agent_step, merge_agent_trace, publish_agent_step, trace_id_opt};
use super::session_budget::SessionToolBudget;
use super::util::{new_id, truncate_str};
use tokio_util::sync::CancellationToken;

pub(super) type ToolExecResult = Result<(String, bool, Option<String>), anyhow::Error>;

/// Serialize a completed sub-agent result for the lead model **without** echoing the
/// internal `taskId`. The model must not see a finished task's machine id, otherwise it
/// tends to copy it onto the next, unrelated `run_subagent` call — colliding the child
/// board store key / trace id (child key and trace are derived from taskId alone for
/// registered agents). Keep the JSON shape otherwise identical.
fn serialize_subagent_result_without_task_id(
    result: &AgentRunResult,
) -> Result<String, serde_json::Error> {
    let mut obj = serde_json::Map::new();
    obj.insert(
        "agentId".to_string(),
        serde_json::Value::String(result.agent_id.clone()),
    );
    obj.insert(
        "agentName".to_string(),
        serde_json::Value::String(result.agent_name.clone()),
    );
    obj.insert(
        "content".to_string(),
        serde_json::Value::String(result.content.clone()),
    );
    // Parent context is the handoff body only. Sub-agent thinking stays on
    // scoped assistant rows and must not round-trip into this tool result.
    serde_json::to_string(&serde_json::Value::Object(obj))
}

pub(super) struct PreparedSubagentOutcome {
    pub tool_call_id: String,
    pub task_id: String,
    pub trace: AgentTrace,
    pub usage: ConversationLlmStats,
    pub exec: ToolExecResult,
}

/// Owned-outcome child definition for the parallel subagent wave (`self` or `explore`).
pub(crate) enum OwnedSubagentSource {
    SelfFork(SelfForkSnapshot),
    Registered(AgentDef),
}

pub(crate) struct OwnedSubagentExecutionInput<'a> {
    pub stream: &'a super::StreamTx,
    pub state: &'a AppState,
    pub conversation_id: &'a str,
    pub cancel: CancellationToken,
    pub provider: OpenAIProvider,
    pub parent_task_board_store_key: String,
    pub message_id: String,
    pub tool_call_id: String,
    pub run_id: String,
    pub task: AgentTask,
    pub source: OwnedSubagentSource,
    /// Lead-enabled skill ids (used by registered explore; ignored for self-fork snapshot).
    pub enabled_skill_ids: Vec<String>,
    pub agent_skill_overrides: std::collections::HashMap<String, Vec<String>>,
    pub child_spawn_depth: u32,
    pub max_spawn_depth: u32,
    /// Host tool-pass trace (nested spawn); lead-owned forks leave this empty.
    pub host_trace_id: Option<String>,
    pub host_scoped_message_id: Option<String>,
}

pub(super) struct SubagentCommitContext<'a> {
    pub stream: &'a super::StreamTx,
    pub conversation_id: &'a str,
    pub message_id: &'a str,
    pub history: Option<&'a mut Vec<ChatMessage>>,
    pub agent_trace: &'a mut Vec<AgentTrace>,
    pub llm_stats: &'a mut ConversationLlmStats,
}

pub(super) struct PendingSubagentOutcome {
    tool_call_id: String,
    task_id: String,
    trace: AgentTrace,
    exec: ToolExecResult,
}

pub(super) struct RecordedSubagentOutcome {
    tool_call_id: String,
    task_id: String,
    trace: AgentTrace,
}

pub(super) fn failed_owned_subagent_outcome(
    run_id: &str,
    conversation_id: &str,
    tool_call_id: &str,
    task: AgentTask,
    source: &OwnedSubagentSource,
    child_spawn_depth: u32,
    error: String,
) -> PreparedSubagentOutcome {
    let def = match source {
        OwnedSubagentSource::SelfFork(snapshot) => &snapshot.def,
        OwnedSubagentSource::Registered(def) => def,
    };
    let instance_scope = AgentInstanceScope::new(run_id, conversation_id, def.id.as_str());
    log::warn!(
        "run_subagent owned-wave preparation failed conversation_id={} task_id={} tool_call_id={} agent_id={}: {}",
        conversation_id,
        task.id,
        tool_call_id,
        def.id,
        error
    );
    PreparedSubagentOutcome {
        tool_call_id: tool_call_id.to_string(),
        task_id: task.id.clone(),
        trace: build_subagent_trace(
            &task,
            def,
            &instance_scope,
            child_spawn_depth,
            None,
            Some(tool_call_id),
            None,
            "failed",
            Some(error.clone()),
        ),
        usage: ConversationLlmStats::default(),
        exec: Ok((format!("ERROR: {error}"), false, Some(error))),
    }
}

pub(super) fn cancelled_owned_subagent_outcome(
    run_id: &str,
    conversation_id: &str,
    tool_call_id: &str,
    task: AgentTask,
    source: &OwnedSubagentSource,
    child_spawn_depth: u32,
) -> PreparedSubagentOutcome {
    let def = match source {
        OwnedSubagentSource::SelfFork(snapshot) => &snapshot.def,
        OwnedSubagentSource::Registered(def) => def,
    };
    let instance_scope = AgentInstanceScope::new(run_id, conversation_id, def.id.as_str());
    log::info!(
        "run_subagent owned-wave skipped conversation_id={} task_id={} tool_call_id={} agent_id={} (cancelled)",
        conversation_id,
        task.id,
        tool_call_id,
        def.id
    );
    PreparedSubagentOutcome {
        tool_call_id: tool_call_id.to_string(),
        task_id: task.id.clone(),
        trace: build_subagent_trace(
            &task,
            def,
            &instance_scope,
            child_spawn_depth,
            None,
            Some(tool_call_id),
            None,
            "cancelled",
            Some("cancelled".into()),
        ),
        usage: ConversationLlmStats::default(),
        exec: Ok(("ERROR: cancelled".into(), false, Some("cancelled".into()))),
    }
}

impl PendingSubagentOutcome {
    pub(super) async fn record_tool_result<F, Fut>(self, recorder: F) -> RecordedSubagentOutcome
    where
        F: FnOnce(ToolExecResult) -> Fut,
        Fut: std::future::Future<Output = ()>,
    {
        recorder(self.exec).await;
        RecordedSubagentOutcome {
            tool_call_id: self.tool_call_id,
            task_id: self.task_id,
            trace: self.trace,
        }
    }
}

fn merge_usage(parent: &mut ConversationLlmStats, child: ConversationLlmStats) {
    parent.llm_rounds = parent.llm_rounds.saturating_add(child.llm_rounds);
    parent.sum_prompt = parent.sum_prompt.saturating_add(child.sum_prompt);
    parent.sum_completion = parent.sum_completion.saturating_add(child.sum_completion);
    parent.sum_total = parent.sum_total.saturating_add(child.sum_total);
    parent.sum_reasoning = parent.sum_reasoning.saturating_add(child.sum_reasoning);
    parent.sum_cache_hit = parent.sum_cache_hit.saturating_add(child.sum_cache_hit);
    parent.sum_cache_miss = parent.sum_cache_miss.saturating_add(child.sum_cache_miss);
    parent.tool_invocations = parent
        .tool_invocations
        .saturating_add(child.tool_invocations);
    parent.rounds_missing_usage = parent
        .rounds_missing_usage
        .saturating_add(child.rounds_missing_usage);
    if child.last_round_prompt_tokens.is_some() {
        parent.last_round_prompt_tokens = child.last_round_prompt_tokens;
    }
}

pub(super) fn commit_subagent_outcome(
    parent: &mut SubagentCommitContext<'_>,
    outcome: PreparedSubagentOutcome,
) -> PendingSubagentOutcome {
    let PreparedSubagentOutcome {
        tool_call_id,
        task_id,
        trace,
        usage,
        exec,
    } = outcome;
    merge_usage(parent.llm_stats, usage);
    merge_agent_trace(parent.agent_trace, trace.clone());
    log::info!(
        "run_subagent prepared ordered commit conversation_id={} task_id={} tool_call_id={}",
        parent.conversation_id,
        task_id,
        tool_call_id
    );
    PendingSubagentOutcome {
        tool_call_id,
        task_id,
        trace,
        exec,
    }
}

pub(super) fn finalize_subagent_outcome(
    parent: &mut SubagentCommitContext<'_>,
    outcome: RecordedSubagentOutcome,
) {
    if let Some(history) = parent.history.as_deref_mut() {
        super::sub_message::sync_anchor_agent_trace_index(
            parent.conversation_id,
            history,
            parent.message_id,
            parent.agent_trace,
        );
    }
    publish_agent_step(parent.stream, parent.message_id, outcome.trace);
    log::info!(
        "run_subagent finalized outcome conversation_id={} task_id={} tool_call_id={}",
        parent.conversation_id,
        outcome.task_id,
        outcome.tool_call_id
    );
}

/// Push terminal UI as soon as this child finishes so parallel siblings do not
/// keep the host row and nested frame on「执行中」until the whole wave joins.
/// `finalize_subagent_outcome` / `record_tool_exec_outcome` emit again later
/// (idempotent). Cancelled-before-start skips this — those never showed running.
fn publish_owned_subagent_ui_finished(
    stream: &super::StreamTx,
    message_id: &str,
    tool_call_id: &str,
    trace: &AgentTrace,
    exec: &ToolExecResult,
    duration_ms: u64,
    host_trace_id: Option<&str>,
    host_scoped_message_id: Option<&str>,
) {
    publish_agent_step(stream, message_id, trace.clone());
    let (status, result, error) = match exec {
        Ok((body, true, _)) => ("success", Some(truncate_str(body, 800)), None),
        Ok((body, false, note)) => ("failed", Some(truncate_str(body, 800)), note.clone()),
        Err(err) => ("failed", None, Some(err.to_string())),
    };
    emit(
        stream,
        StreamEvent::ToolCallStatus {
            message_id: message_id.to_string(),
            tool_call_id: tool_call_id.to_string(),
            status: status.into(),
            result,
            error,
            duration_ms: Some(duration_ms),
            display_label: None,
            display_summary: None,
            trace_id: trace_id_opt(host_trace_id),
            scoped_message_id: trace_id_opt(host_scoped_message_id),
        },
    );
}

fn abandon_child_board(
    state: &AppState,
    conversation_id: &str,
    parent_task_board_store_key: &str,
    task_id: &str,
    agent_instance_id: Option<&str>,
) {
    let child_board_key = match agent_instance_id.filter(|id| !id.trim().is_empty()) {
        Some(instance_id) => crate::task_board::sub_agent_task_board_store_key_for_instance(
            parent_task_board_store_key,
            task_id.trim(),
            instance_id,
        ),
        None => crate::task_board::sub_agent_task_board_store_key(
            parent_task_board_store_key,
            task_id.trim(),
        ),
    };
    match state
        .task_board_store
        .apply(&child_board_key, "abandon", &serde_json::json!({}))
    {
        Ok(_) => log::info!(
            "run_subagent: child board abandoned conversation_id={} store_key={}",
            conversation_id,
            child_board_key
        ),
        Err(err) => log::warn!(
            "run_subagent: child board abandon skipped conversation_id={} store_key={}: {err:#}",
            conversation_id,
            child_board_key
        ),
    }
}

pub(super) async fn execute_owned_subagent(
    input: OwnedSubagentExecutionInput<'_>,
) -> PreparedSubagentOutcome {
    if input.cancel.is_cancelled() {
        return cancelled_owned_subagent_outcome(
            &input.run_id,
            input.conversation_id,
            &input.tool_call_id,
            input.task,
            &input.source,
            input.child_spawn_depth,
        );
    }
    let OwnedSubagentExecutionInput {
        stream,
        state,
        conversation_id,
        cancel,
        provider,
        parent_task_board_store_key,
        message_id,
        tool_call_id,
        run_id,
        task,
        source,
        enabled_skill_ids,
        agent_skill_overrides,
        child_spawn_depth,
        max_spawn_depth,
        host_trace_id,
        host_scoped_message_id,
    } = input;
    let empty_overrides = std::collections::HashMap::new();
    let (definition_source, skill_ids, overrides, def_for_trace) = match &source {
        OwnedSubagentSource::SelfFork(snapshot) => (
            super::sub_agent_prompt::SubAgentDefinitionSource::Snapshot(snapshot),
            snapshot.skill_ids.as_slice(),
            &empty_overrides,
            &snapshot.def,
        ),
        OwnedSubagentSource::Registered(def) => (
            super::sub_agent_prompt::SubAgentDefinitionSource::Registered(&task),
            enabled_skill_ids.as_slice(),
            &agent_skill_overrides,
            def,
        ),
    };
    let instance_scope = definition_source.new_instance_scope(&run_id, conversation_id);
    log::info!(
        "run_subagent owned-wave start conversation_id={} task_id={} tool_call_id={} agent_id={} agent_instance_id={}",
        conversation_id,
        task.id,
        tool_call_id,
        def_for_trace.id,
        instance_scope.agent_instance_id
    );

    // Emit running before sub_message_start / tool events so the UI can nest the
    // frame under this run_subagent row for the whole lifetime (not only at commit).
    let started = Instant::now();
    publish_agent_step(
        stream,
        &message_id,
        build_subagent_trace(
            &task,
            def_for_trace,
            &instance_scope,
            child_spawn_depth,
            None,
            Some(tool_call_id.as_str()),
            Some(message_id.as_str()),
            "running",
            Some(truncate_str(&task.title, 200)),
        ),
    );

    let sub_cap =
        crate::models::clamp_max_sub_agent_tool_rounds(provider.settings.max_sub_agent_tool_rounds);
    let mut sub_budget = SessionToolBudget::new(sub_cap, 0);
    let mut child_trace = Vec::new();
    let mut child_usage = ConversationLlmStats::default();
    let mut sub_ctx = super::context::SubAgentLoopContext {
        session: super::context::SessionRefs {
            stream,
            state,
            conversation_id,
            cancel: &cancel,
        },
        provider: &provider,
        parent_task_board_store_key: &parent_task_board_store_key,
        message_id: &message_id,
        agent_trace: &mut child_trace,
        enabled_skill_ids: skill_ids,
        agent_skill_overrides: overrides,
        task: &task,
        definition_source,
        instance_scope: instance_scope.clone(),
        sub_tool_budget: &mut sub_budget,
        llm_stats: &mut child_usage,
        spawn_depth: child_spawn_depth,
        max_spawn_depth,
    };
    let run_result = Box::pin(super::sub_agent::run_sub_agent(&mut sub_ctx)).await;
    let (trace, exec) = match run_result {
        Ok(result) => match serialize_subagent_result_without_task_id(&result) {
            Ok(json) => (
                build_subagent_trace(
                    &task,
                    def_for_trace,
                    &instance_scope,
                    child_spawn_depth,
                    None,
                    Some(tool_call_id.as_str()),
                    Some(message_id.as_str()),
                    "completed",
                    Some(truncate_str(&result.content, 160)),
                ),
                Ok((json, true, None)),
            ),
            Err(err) => {
                log::warn!(
                    "run_subagent owned-wave result serialize failed conversation_id={} task_id={}: {err}",
                    conversation_id,
                    task.id
                );
                let message = format!("sub-agent result serialization failed: {err}");
                (
                    build_subagent_trace(
                        &task,
                        def_for_trace,
                        &instance_scope,
                        child_spawn_depth,
                        None,
                        Some(tool_call_id.as_str()),
                        Some(message_id.as_str()),
                        "failed",
                        Some(message.clone()),
                    ),
                    Ok((format!("ERROR: {message}"), false, Some(message))),
                )
            }
        },
        Err(err) => {
            let cancelled = cancel.is_cancelled();
            let status = if cancelled { "cancelled" } else { "failed" };
            let error_note = if cancelled {
                "cancelled".to_string()
            } else {
                err.to_string()
            };
            log::warn!(
                "run_subagent owned-wave ended conversation_id={} task_id={} agent_id={} status={}: {err:#}",
                conversation_id,
                task.id,
                def_for_trace.id,
                status
            );
            abandon_child_board(
                state,
                conversation_id,
                &parent_task_board_store_key,
                &task.id,
                Some(&instance_scope.agent_instance_id),
            );
            (
                build_subagent_trace(
                    &task,
                    def_for_trace,
                    &instance_scope,
                    child_spawn_depth,
                    None,
                    Some(tool_call_id.as_str()),
                    Some(message_id.as_str()),
                    status,
                    Some(error_note.clone()),
                ),
                Ok((format!("ERROR: {error_note}"), false, Some(error_note))),
            )
        }
    };
    log::info!(
        "run_subagent owned-wave prepared conversation_id={} task_id={} tool_call_id={} agent_id={} status={} llm_rounds={} total_tokens={}",
        conversation_id,
        task.id,
        tool_call_id,
        def_for_trace.id,
        trace.status,
        child_usage.llm_rounds,
        child_usage.sum_total
    );
    let duration_ms = started.elapsed().as_millis() as u64;
    publish_owned_subagent_ui_finished(
        stream,
        &message_id,
        &tool_call_id,
        &trace,
        &exec,
        duration_ms,
        host_trace_id.as_deref(),
        host_scoped_message_id.as_deref(),
    );
    log::info!(
        "run_subagent owned-wave ui finished conversation_id={} task_id={} tool_call_id={} status={} duration_ms={}",
        conversation_id,
        task.id,
        tool_call_id,
        trace.status,
        duration_ms
    );
    PreparedSubagentOutcome {
        tool_call_id,
        task_id: task.id,
        trace,
        usage: child_usage,
        exec,
    }
}

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
        let ephemeral =
            SessionSandbox::is_sandbox(Path::new(self.prior_workspace.trim())).unwrap_or(false);
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

fn build_subagent_trace(
    task: &AgentTask,
    def: &AgentDef,
    instance_scope: &AgentInstanceScope,
    child_spawn_depth: u32,
    computer_target: Option<ComputerOperationTarget>,
    parent_tool_call_id: Option<&str>,
    anchor_message_id: Option<&str>,
    status: &str,
    detail: Option<String>,
) -> AgentTrace {
    AgentTrace {
        id: super::sub_agent_prompt::sub_agent_trace_id(task, def, instance_scope),
        name: agent_display_label(def),
        role: def.role.clone(),
        status: status.into(),
        detail,
        content: None,
        depth: Some(child_spawn_depth),
        session: None,
        computer_target,
        collapsed: true,
        user_expanded: false,
        agent_instance_id: Some(instance_scope.agent_instance_id.clone()),
        parent_tool_call_id: parent_tool_call_id
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(str::to_string),
        anchor_message_id: anchor_message_id
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(str::to_string),
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
            let child_spawn_depth =
                match validate_spawn_depth(ctx.parent_spawn_depth, max_spawn_depth) {
                    Ok(d) => d,
                    Err(msg) => return Ok((format!("ERROR: {msg}"), false, Some(msg))),
                };
            if let Err(msg) = validate_run_subagent_workspace(&parsed) {
                return Ok((format!("ERROR: {msg}"), false, Some(msg)));
            }
            let agent_id = parsed.agent_id;
            match crate::tools::run_subagent::validate_run_subagent_target(
                &state.agents,
                allow_agents,
                ctx.current_agent_id,
                &agent_id,
            ) {
                Err(msg) => Ok((format!("ERROR: {msg}"), false, Some(msg))),
                Ok(target) => {
                    let def = match target {
                        crate::tools::run_subagent::RunSubagentTarget::Registered(def) => def,
                        crate::tools::run_subagent::RunSubagentTarget::SelfFork => {
                            let msg =
                                "self-fork execution is not available in this implementation stage"
                                    .to_string();
                            log::warn!(
                                "run_subagent self-fork execution unavailable conversation_id={conversation_id}"
                            );
                            return Ok((format!("ERROR: {msg}"), false, Some(msg)));
                        }
                    };
                    let tid = if parsed.task_id.trim().is_empty() {
                        new_id("sub_task")
                    } else {
                        parsed.task_id.trim().to_string()
                    };
                    if def.id == "computer" {
                        if let Err(e) =
                            super::computer_monitor_pick::ensure_computer_monitor_for_subagent(
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
                            if cancel.is_cancelled() {
                                return Err(anyhow::anyhow!("已停止生成"));
                            }
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
                                let workspace_changed =
                                    sub_settings.workspace_root.trim() != prior_workspace.trim();
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
                    let sub_provider = OpenAIProvider::new(sub_settings, provider.api_key.clone());
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
                    let detail = truncate_str(&task.title, 200);
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
                    let definition_source =
                        super::sub_agent_prompt::SubAgentDefinitionSource::Registered(&task);
                    let instance_scope =
                        definition_source.new_instance_scope(run_id, conversation_id);
                    let make_trace = |status: &str, detail: Option<String>| {
                        build_subagent_trace(
                            &task,
                            &def,
                            &instance_scope,
                            child_spawn_depth,
                            computer_target,
                            Some(tool_call_id),
                            Some(message_id),
                            status,
                            detail,
                        )
                    };
                    emit_subagent_trace_step(stream, ctx, make_trace("running", Some(detail)));
                    let sub_cap = crate::models::clamp_max_sub_agent_tool_rounds(
                        provider.settings.max_sub_agent_tool_rounds,
                    );
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
                        agent_skill_overrides: ctx.agent_skill_overrides,
                        task: &task,
                        definition_source,
                        instance_scope: instance_scope.clone(),
                        sub_tool_budget: &mut sub_budget,
                        llm_stats: ctx.llm_stats,
                        spawn_depth: child_spawn_depth,
                        max_spawn_depth,
                    };
                    match Box::pin(super::sub_agent::run_sub_agent(&mut sub_ctx)).await {
                        Ok(result) => {
                            log::info!(
                                "run_subagent completed conversation_id={} sub_agent={} task_id={} spawn_depth={}",
                                conversation_id,
                                result.agent_id,
                                result.task_id,
                                child_spawn_depth
                            );
                            let json = serialize_subagent_result_without_task_id(&result)
                                .unwrap_or_else(|e| {
                                    log::warn!("run_subagent result serialize failed: {e}");
                                    r#"{"error":"serialize_failed"}"#.to_string()
                                });
                            emit_subagent_trace_step(
                                stream,
                                ctx,
                                make_trace("completed", Some(truncate_str(&result.content, 160))),
                            );
                            Ok((json, true, None))
                        }
                        Err(e) => {
                            let cancelled = cancel.is_cancelled();
                            log::warn!(
                                "run_subagent failed conversation_id={}: {e:#}",
                                conversation_id
                            );
                            abandon_child_board(
                                state,
                                conversation_id,
                                parent_task_board_store_key,
                                &task.id,
                                None,
                            );
                            emit_subagent_trace_step(
                                stream,
                                ctx,
                                make_trace(
                                    if cancelled { "cancelled" } else { "failed" },
                                    Some(e.to_string()),
                                ),
                            );
                            // Cancel must fail the parent tool pass so the lead loop
                            // stops; otherwise the session lane stays occupied and the
                            // next user message queues with no reply.
                            if cancelled {
                                return Err(anyhow::anyhow!("已停止生成"));
                            }
                            Ok((format!("ERROR: {e}"), false, None))
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod trace_tests {
    use super::{
        build_subagent_trace, commit_subagent_outcome, execute_owned_subagent,
        failed_owned_subagent_outcome, finalize_subagent_outcome,
        publish_owned_subagent_ui_finished, serialize_subagent_result_without_task_id,
        OwnedSubagentExecutionInput, OwnedSubagentSource, PreparedSubagentOutcome,
        SubagentCommitContext,
    };
    use crate::agent_instance_scope::AgentInstanceScope;
    use crate::agents::{
        AccessPolicy, AgentDef, AgentProfile, AgentRunResult, AgentTask, AgentUiConfig,
        SkillsPolicy,
    };
    use crate::llm_token_stats::ConversationLlmStats;
    use crate::models::{ChatMessage, Role};
    use std::collections::HashMap;

    #[test]
    fn subagent_result_serialization_omits_task_id() {
        let result = AgentRunResult {
            task_id: "sub_task_should_not_leak".into(),
            agent_id: "coder".into(),
            agent_name: "氛围编程".into(),
            content: "handoff body".into(),
            reasoning: Some("thinking".into()),
        };
        let json = serialize_subagent_result_without_task_id(&result).unwrap();
        assert!(!json.contains("sub_task_should_not_leak"));
        assert!(!json.contains("taskId"));
        assert!(json.contains("\"agentId\":\"coder\""));
        assert!(json.contains("\"agentName\":\"氛围编程\""));
        assert!(json.contains("\"content\":\"handoff body\""));
        assert!(!json.contains("reasoning"));
    }

    #[test]
    fn child_trace_carries_current_agent_instance_id() {
        let task = AgentTask {
            id: "task-1".into(),
            agent_id: "self".into(),
            title: "Fork".into(),
            goal: "Inspect".into(),
            context: String::new(),
            depends_on: vec![],
        };
        let def = AgentDef {
            id: "current-agent".into(),
            name: "Current Agent".into(),
            description: "snapshot".into(),
            role: "worker".into(),
            profile: AgentProfile::Coder,
            default_skill_ids: vec![],
            skills_policy: SkillsPolicy::InheritsFromParent,
            access_policy: AccessPolicy::default(),
            builtin: false,
            enabled: true,
            tool_names: vec![],
            source: None,
            resource_files: vec![],
            allow_agents: vec![],
            config: HashMap::new(),
            ui: AgentUiConfig::default(),
            plugin_id: None,
        };
        let scope = AgentInstanceScope::with_instance_id(
            "run",
            "conversation",
            "current-agent",
            "instance-1",
        );

        let trace = build_subagent_trace(
            &task,
            &def,
            &scope,
            1,
            None,
            Some("call-1"),
            None,
            "running",
            None,
        );

        assert_eq!(trace.agent_instance_id.as_deref(), Some("instance-1"));
        assert_eq!(trace.parent_tool_call_id.as_deref(), Some("call-1"));
    }

    fn anchor_message() -> ChatMessage {
        ChatMessage {
            id: "anchor".into(),
            role: Role::Assistant,
            content: String::new(),
            status: "streaming".into(),
            created_at: 0,
            tool_calls: None,
            tool_call_id: None,
            tool_name: None,
            error_message: None,
            reasoning: None,
            thoughts: None,
            headline: None,
            raw_content: None,
            tool_raw_output: None,
            agent_id: None,
            agent_instance_id: None,
            agent_name: None,
            agent_trace: None,
            image_slot_labels: None,
            images_base64: None,
            computer_round_screen_rel_path: None,
            ui_bindings: None,
            context_state: None,
            attachments: None,
            anchor_message_id: None,
            trace_id: None,
            task_id: None,
            spawn_depth: None,
        }
    }

    fn completed_trace_def() -> AgentDef {
        AgentDef {
            id: "current-agent".into(),
            name: "Current Agent".into(),
            description: "snapshot".into(),
            role: "worker".into(),
            profile: AgentProfile::Coder,
            default_skill_ids: vec![],
            skills_policy: SkillsPolicy::InheritsFromParent,
            access_policy: AccessPolicy::default(),
            builtin: false,
            enabled: true,
            tool_names: vec![],
            source: None,
            resource_files: vec![],
            allow_agents: vec![],
            config: HashMap::new(),
            ui: AgentUiConfig::default(),
            plugin_id: None,
        }
    }

    fn completed_trace() -> crate::models::AgentTrace {
        let task = AgentTask {
            id: "task-1".into(),
            agent_id: "self".into(),
            title: "Fork".into(),
            goal: "Inspect".into(),
            context: String::new(),
            depends_on: vec![],
        };
        let def = completed_trace_def();
        let scope = AgentInstanceScope::with_instance_id(
            "run",
            "conversation",
            "current-agent",
            "instance-1",
        );
        build_subagent_trace(
            &task,
            &def,
            &scope,
            1,
            None,
            Some("call-1"),
            None,
            "completed",
            Some("done".into()),
        )
    }

    #[test]
    fn owned_wave_publishes_terminal_ui_when_child_finishes() {
        let (stream, mut events) = crate::models::ChatStreamSender::pair("conversation", "user");
        let exec = Ok((r#"{"content":"done"}"#.into(), true, None));
        publish_owned_subagent_ui_finished(
            &stream,
            "anchor",
            "call-1",
            &completed_trace(),
            &exec,
            42,
            None,
            None,
        );
        assert!(matches!(
            events.try_recv(),
            Ok(crate::models::StreamEvent::AgentStep { agent, .. })
                if agent.status == "completed"
        ));
        assert!(matches!(
            events.try_recv(),
            Ok(crate::models::StreamEvent::ToolCallStatus {
                tool_call_id,
                status,
                duration_ms,
                ..
            }) if tool_call_id == "call-1"
                && status == "success"
                && duration_ms == Some(42)
        ));
        assert!(events.try_recv().is_err());
    }

    #[test]
    fn prepared_outcome_does_not_mutate_parent_before_commit() {
        let history = vec![anchor_message()];
        let traces: Vec<crate::models::AgentTrace> = Vec::new();
        let stats = ConversationLlmStats {
            llm_rounds: 2,
            sum_total: 20,
            ..Default::default()
        };

        let _outcome = PreparedSubagentOutcome {
            tool_call_id: "call-1".into(),
            task_id: "task-1".into(),
            trace: completed_trace(),
            usage: ConversationLlmStats {
                llm_rounds: 1,
                sum_total: 7,
                ..Default::default()
            },
            exec: Ok((r#"{"content":"done"}"#.into(), true, None)),
        };

        assert_eq!(history.len(), 1);
        assert!(history[0].agent_trace.is_none());
        assert!(traces.is_empty());
        assert_eq!(stats.llm_rounds, 2);
        assert_eq!(stats.sum_total, 20);
    }

    #[test]
    fn preparation_failure_returns_owned_failed_outcome() {
        let snapshot = crate::chat_service::self_fork::SelfForkSnapshot {
            def: completed_trace_def(),
            system_prompt: "captured".into(),
            skill_ids: vec![],
            skill_prompts: vec![],
            allowed_tools: vec![],
            workspace_root: "/tmp".into(),
        };
        let task = AgentTask {
            id: "task-failed".into(),
            agent_id: "self".into(),
            title: "Failed preparation".into(),
            goal: "work".into(),
            context: String::new(),
            depends_on: vec![],
        };

        let outcome = failed_owned_subagent_outcome(
            "run",
            "conversation",
            "call-failed",
            task,
            &OwnedSubagentSource::SelfFork(snapshot),
            2,
            "spawn depth limit".into(),
        );

        assert_eq!(outcome.tool_call_id, "call-failed");
        assert_eq!(outcome.trace.status, "failed");
        let (result, ok, error) = outcome.exec.unwrap();
        assert!(!ok);
        assert!(result.contains("spawn depth limit"));
        assert_eq!(error.as_deref(), Some("spawn depth limit"));
    }

    #[tokio::test]
    async fn commit_records_result_before_anchor_and_final_status() {
        let (stream, mut events) = crate::models::ChatStreamSender::pair("conversation", "user");
        let mut history = vec![anchor_message()];
        history[0].tool_calls = Some(vec![crate::models::ToolCall {
            id: "call-1".into(),
            name: "run_subagent".into(),
            arguments: "{}".into(),
            status: "running".into(),
            result: None,
            error: None,
            duration_ms: None,
            risk_level: None,
            display_label: None,
            display_summary: None,
        }]);
        let mut traces = Vec::new();
        let mut stats = ConversationLlmStats {
            llm_rounds: 2,
            sum_prompt: 10,
            sum_completion: 10,
            sum_total: 20,
            sum_reasoning: 3,
            sum_cache_hit: 6,
            sum_cache_miss: 4,
            tool_invocations: 4,
            rounds_missing_usage: 1,
            last_round_prompt_tokens: Some(5),
        };
        let outcome = PreparedSubagentOutcome {
            tool_call_id: "call-1".into(),
            task_id: "task-1".into(),
            trace: completed_trace(),
            usage: ConversationLlmStats {
                llm_rounds: 1,
                sum_prompt: 7,
                sum_completion: 4,
                sum_total: 11,
                sum_reasoning: 2,
                sum_cache_hit: 5,
                sum_cache_miss: 2,
                tool_invocations: 3,
                rounds_missing_usage: 0,
                last_round_prompt_tokens: Some(7),
            },
            exec: Ok((r#"{"content":"done"}"#.into(), true, None)),
        };

        let pending = commit_subagent_outcome(
            &mut SubagentCommitContext {
                stream: &stream,
                conversation_id: "conversation",
                message_id: "anchor",
                history: Some(&mut history),
                agent_trace: &mut traces,
                llm_stats: &mut stats,
            },
            outcome,
        );

        assert_eq!(traces.len(), 1);
        assert_eq!(traces[0].status, "completed");
        assert!(history[0].agent_trace.is_none());
        assert_eq!(stats.llm_rounds, 3);
        assert_eq!(stats.sum_prompt, 17);
        assert_eq!(stats.sum_completion, 14);
        assert_eq!(stats.sum_total, 31);
        assert_eq!(stats.sum_reasoning, 5);
        assert_eq!(stats.sum_cache_hit, 11);
        assert_eq!(stats.sum_cache_miss, 6);
        assert_eq!(stats.tool_invocations, 7);
        assert_eq!(stats.rounds_missing_usage, 1);
        assert_eq!(stats.last_round_prompt_tokens, Some(7));
        assert!(
            events.try_recv().is_err(),
            "commit must not publish final status"
        );

        let recorded = pending
            .record_tool_result(|exec| async {
                let (content, ok, _) = exec.unwrap();
                assert!(ok);
                crate::conversation_transcript::insert_tool_result_in_history(
                    &mut history,
                    "anchor",
                    "call-1",
                    &content,
                );
            })
            .await;

        assert!(
            history
                .iter()
                .any(|message| message.tool_call_id.as_deref() == Some("call-1")),
            "tool result must be recorded before finalization"
        );
        assert!(history[0].agent_trace.is_none());
        assert!(
            events.try_recv().is_err(),
            "result recording must happen before AgentStep"
        );

        finalize_subagent_outcome(
            &mut SubagentCommitContext {
                stream: &stream,
                conversation_id: "conversation",
                message_id: "anchor",
                history: Some(&mut history),
                agent_trace: &mut traces,
                llm_stats: &mut stats,
            },
            recorded,
        );

        assert_eq!(history[0].agent_trace.as_ref().map(Vec::len), Some(1));
        assert!(matches!(
            events.try_recv(),
            Ok(crate::models::StreamEvent::AgentStep { .. })
        ));
    }

    #[tokio::test]
    async fn cancelled_self_fork_returns_owned_cancelled_outcome() {
        let (stream, mut events) = crate::models::ChatStreamSender::pair("conversation", "user");
        let state = crate::chat_service::AppState::new();
        let cancel = tokio_util::sync::CancellationToken::new();
        cancel.cancel();
        let snapshot = crate::chat_service::self_fork::SelfForkSnapshot {
            def: AgentDef {
                id: "current-agent".into(),
                name: "Current Agent".into(),
                description: "snapshot".into(),
                role: "worker".into(),
                profile: AgentProfile::Coder,
                default_skill_ids: vec![],
                skills_policy: SkillsPolicy::InheritsFromParent,
                access_policy: AccessPolicy::default(),
                builtin: false,
                enabled: true,
                tool_names: vec![],
                source: None,
                resource_files: vec![],
                allow_agents: vec![],
                config: HashMap::new(),
                ui: AgentUiConfig::default(),
                plugin_id: None,
            },
            system_prompt: "active prompt".into(),
            skill_ids: vec![],
            skill_prompts: vec![],
            allowed_tools: vec![],
            workspace_root: "/tmp".into(),
        };
        let provider = crate::provider::OpenAIProvider::new(
            crate::models::ModelSettings::default(),
            String::new(),
        );

        let outcome = execute_owned_subagent(OwnedSubagentExecutionInput {
            stream: &stream,
            state: &state,
            conversation_id: "conversation",
            cancel,
            provider,
            parent_task_board_store_key: "parent-board".into(),
            message_id: "anchor".into(),
            tool_call_id: "call-cancel".into(),
            run_id: "run".into(),
            task: AgentTask {
                id: "task-cancel".into(),
                agent_id: "self".into(),
                title: "Cancelled fork".into(),
                goal: "Inspect".into(),
                context: String::new(),
                depends_on: vec![],
            },
            source: OwnedSubagentSource::SelfFork(snapshot),
            enabled_skill_ids: vec![],
            agent_skill_overrides: HashMap::new(),
            child_spawn_depth: 1,
            max_spawn_depth: 2,
            host_trace_id: None,
            host_scoped_message_id: None,
        })
        .await;

        assert_eq!(outcome.tool_call_id, "call-cancel");
        assert_eq!(outcome.task_id, "task-cancel");
        assert_eq!(outcome.trace.status, "cancelled");
        assert_eq!(outcome.usage.llm_rounds, 0);
        let (_, success, error) = outcome.exec.unwrap();
        assert!(!success);
        assert_eq!(error.as_deref(), Some("cancelled"));
        let streamed: Vec<_> = std::iter::from_fn(|| events.try_recv().ok()).collect();
        assert!(
            streamed
                .iter()
                .all(|event| !matches!(event, crate::models::StreamEvent::AgentStep { .. })),
            "cancelled-before-start must not emit running; commit owns the terminal step"
        );
    }

    #[tokio::test]
    async fn same_task_self_forks_are_isolated_until_owned_commit() {
        let (stream, mut events) = crate::models::ChatStreamSender::pair("conversation", "user");
        let state = crate::chat_service::AppState::new();
        let history = vec![anchor_message()];
        let traces: Vec<crate::models::AgentTrace> = Vec::new();
        let stats = ConversationLlmStats::default();

        let execute = |tool_call_id: &str| {
            let cancel = tokio_util::sync::CancellationToken::new();
            cancel.cancel();
            execute_owned_subagent(OwnedSubagentExecutionInput {
                stream: &stream,
                state: &state,
                conversation_id: "same-task-isolation",
                cancel,
                provider: crate::provider::OpenAIProvider::new(
                    crate::models::ModelSettings::default(),
                    String::new(),
                ),
                parent_task_board_store_key: "parent-board".into(),
                message_id: "anchor".into(),
                tool_call_id: tool_call_id.into(),
                run_id: "run".into(),
                task: AgentTask {
                    id: "user-reused-task-id".into(),
                    agent_id: "self".into(),
                    title: "Cancelled fork".into(),
                    goal: "Inspect".into(),
                    context: String::new(),
                    depends_on: vec![],
                },
                source: OwnedSubagentSource::SelfFork(
                    crate::chat_service::self_fork::SelfForkSnapshot {
                        def: AgentDef {
                            id: "current-agent".into(),
                            name: "Current Agent".into(),
                            description: "snapshot".into(),
                            role: "worker".into(),
                            profile: AgentProfile::Coder,
                            default_skill_ids: vec![],
                            skills_policy: SkillsPolicy::InheritsFromParent,
                            access_policy: AccessPolicy::default(),
                            builtin: false,
                            enabled: true,
                            tool_names: vec![],
                            source: None,
                            resource_files: vec![],
                            allow_agents: vec![],
                            config: HashMap::new(),
                            ui: AgentUiConfig::default(),
                            plugin_id: None,
                        },
                        system_prompt: "active prompt".into(),
                        skill_ids: vec![],
                        skill_prompts: vec![],
                        allowed_tools: vec![],
                        workspace_root: "/tmp".into(),
                    },
                ),
                enabled_skill_ids: vec![],
                agent_skill_overrides: HashMap::new(),
                child_spawn_depth: 1,
                max_spawn_depth: 2,
                host_trace_id: None,
                host_scoped_message_id: None,
            })
        };

        let first = execute("call-1").await;
        let second = execute("call-2").await;

        assert_ne!(first.trace.id, second.trace.id);
        assert_ne!(
            first.trace.agent_instance_id,
            second.trace.agent_instance_id
        );
        assert_eq!(first.task_id, second.task_id);
        assert_eq!(history.len(), 1);
        assert!(history[0].agent_trace.is_none());
        assert!(traces.is_empty());
        assert_eq!(stats.llm_rounds, 0);
        let streamed: Vec<_> = std::iter::from_fn(|| events.try_recv().ok()).collect();
        assert!(
            streamed.is_empty(),
            "cancelled-before-start must not stream steps; isolation is in prepared traces until commit"
        );
        assert_eq!(first.trace.status, "cancelled");
        assert_eq!(second.trace.status, "cancelled");
        assert_eq!(first.trace.parent_tool_call_id.as_deref(), Some("call-1"));
        assert_eq!(second.trace.parent_tool_call_id.as_deref(), Some("call-2"));
    }
}
