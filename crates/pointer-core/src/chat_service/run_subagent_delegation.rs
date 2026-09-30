//! Lead-agent `run_subagent` tool: validate target, spawn sub loop, emit trace steps.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
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
    resolve_computer_operation_target, validate_background_target, validate_run_subagent_workspace,
    validate_spawn_depth,
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
    if !result.agent_instance_id.trim().is_empty() {
        obj.insert(
            "agentInstanceId".to_string(),
            serde_json::Value::String(result.agent_instance_id.clone()),
        );
    }
    // Parent context is the handoff body only. Sub-agent thinking stays on
    // scoped assistant rows and must not round-trip into this tool result.
    serde_json::to_string(&serde_json::Value::Object(obj))
}

/// Still-running jobs this worker started, so the caller can `job.await` them.
/// Bodies stay out; finished jobs are not listed.
fn handoff_with_open_background_jobs(
    state: &AppState,
    conversation_id: &str,
    agent_instance_id: &str,
    json: String,
) -> String {
    let jobs = state
        .jobs
        .open_jobs_spawned_by(conversation_id, agent_instance_id);
    if jobs.is_empty() {
        return json;
    }
    let mut value: serde_json::Value = match serde_json::from_str(&json) {
        Ok(value) => value,
        Err(err) => {
            log::warn!(
                "run_subagent: open background jobs not attached; result is not json conversation_id={conversation_id} agent_instance_id={agent_instance_id}: {err}"
            );
            return json;
        }
    };
    let Some(obj) = value.as_object_mut() else {
        log::warn!(
            "run_subagent: open background jobs not attached; result is not an object conversation_id={conversation_id} agent_instance_id={agent_instance_id}"
        );
        return json;
    };
    let items: Vec<serde_json::Value> = jobs
        .iter()
        .map(|job| {
            let mut item = serde_json::json!({
                "jobId": job.job_id,
                "status": job.status,
                "kind": job.kind,
            });
            if let Some(title) = job.title.as_ref().filter(|title| !title.trim().is_empty()) {
                item["title"] = serde_json::Value::String(title.clone());
            }
            item
        })
        .collect();
    log::info!(
        "run_subagent: open background jobs entered handoff conversation_id={conversation_id} agent_instance_id={agent_instance_id} count={}",
        items.len()
    );
    obj.insert(
        "openBackgroundJobs".to_string(),
        serde_json::Value::Array(items),
    );
    match serde_json::to_string(&value) {
        Ok(attached) => attached,
        Err(err) => {
            log::warn!(
                "run_subagent: open background jobs reserialize failed conversation_id={conversation_id} agent_instance_id={agent_instance_id}: {err}"
            );
            json
        }
    }
}

pub(super) struct PreparedSubagentOutcome {
    pub tool_call_id: String,
    pub task_id: String,
    pub trace: AgentTrace,
    pub usage: ConversationLlmStats,
    pub exec: ToolExecResult,
}

/// Owned-outcome child definition for the parallel subagent wave (`self` or `explore`).
#[derive(Clone)]
pub(crate) enum OwnedSubagentSource {
    SelfFork(SelfForkSnapshot),
    Registered(AgentDef),
}

impl OwnedSubagentSource {
    /// `AgentTrace.delegation` value: tells a self fork apart from a real worker.
    pub(crate) fn delegation_kind(&self) -> &'static str {
        match self {
            OwnedSubagentSource::SelfFork(_) => "self",
            OwnedSubagentSource::Registered(_) => "registered",
        }
    }
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
    /// Owner chain of the agent that issued this spawn (empty = lead).
    pub issuer_chain: Vec<String>,
    /// Restored chain when resuming a worker; `None` mints
    /// `issuer_chain + [child instance id]`.
    pub resume_agent_chain: Option<Vec<String>>,
    /// Host tool-pass trace (nested spawn); lead-owned forks leave this empty.
    pub host_trace_id: Option<String>,
    pub host_scoped_message_id: Option<String>,
    /// `AgentTrace.id` of the issuing agent instance (nested spawn); lead-owned spawns
    /// leave this empty. Written as the child trace's `parent_trace_id`.
    pub issuer_trace_id: Option<String>,
    pub state_arc: std::sync::Arc<AppState>,
    /// Foreground join writes a worker preview onto the host `run_subagent` row.
    /// Background spawn keeps that row as a job handle; skip the preview status event.
    pub emit_host_tool_status: bool,
    /// Pre-minted child thread id (background register). Foreground mints in execute.
    pub instance_scope: Option<AgentInstanceScope>,
    /// When set, this nested loop is a background worker job.
    pub background_job_id: Option<String>,
    /// Restored worker transcript for `followupInstanceId`. `None` starts fresh.
    pub resume_history: Option<super::worker_followup::ResumedWorkerHistory>,
    /// Keeps this instance claimed until the foreground run returns.
    pub followup_reserve: Option<super::job_supervisor::FollowupReserve>,
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
    issuer_trace_id: Option<&str>,
    child_spawn_depth: u32,
    preset_instance: Option<AgentInstanceScope>,
    error: String,
) -> PreparedSubagentOutcome {
    let def = match source {
        OwnedSubagentSource::SelfFork(snapshot) => &snapshot.def,
        OwnedSubagentSource::Registered(def) => def,
    };
    let instance_scope = preset_instance
        .unwrap_or_else(|| AgentInstanceScope::new(run_id, conversation_id, def.id.as_str()));
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
            issuer_trace_id,
            Some(source.delegation_kind()),
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
    issuer_trace_id: Option<&str>,
    child_spawn_depth: u32,
    preset_instance: Option<AgentInstanceScope>,
) -> PreparedSubagentOutcome {
    let def = match source {
        OwnedSubagentSource::SelfFork(snapshot) => &snapshot.def,
        OwnedSubagentSource::Registered(def) => def,
    };
    let instance_scope = preset_instance
        .unwrap_or_else(|| AgentInstanceScope::new(run_id, conversation_id, def.id.as_str()));
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
            issuer_trace_id,
            Some(source.delegation_kind()),
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
    emit_host_tool_status: bool,
) {
    publish_agent_step(stream, message_id, trace.clone());
    if !emit_host_tool_status {
        return;
    }
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
            input.issuer_trace_id.as_deref(),
            input.child_spawn_depth,
            input.instance_scope.clone(),
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
        issuer_chain,
        resume_agent_chain,
        host_trace_id,
        host_scoped_message_id,
        issuer_trace_id,
        state_arc,
        emit_host_tool_status,
        instance_scope: preset_instance,
        background_job_id,
        resume_history,
        followup_reserve,
    } = input;
    let _followup_reserve = followup_reserve;
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
    let instance_scope = preset_instance
        .unwrap_or_else(|| definition_source.new_instance_scope(&run_id, conversation_id));
    // Child chain = issuer chain + own instance id; a resumed worker keeps the
    // chain it was originally spawned with.
    let agent_chain: Vec<String> = match resume_agent_chain {
        Some(chain) => chain,
        None => {
            let mut chain = issuer_chain;
            chain.push(instance_scope.agent_instance_id.clone());
            chain
        }
    };
    log::info!(
        "run_subagent owned-wave start conversation_id={} task_id={} tool_call_id={} agent_id={} agent_instance_id={}",
        conversation_id,
        task.id,
        tool_call_id,
        def_for_trace.id,
        instance_scope.agent_instance_id
    );
    let parent_trace_id = issuer_trace_id.as_deref();
    let delegation = source.delegation_kind();

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
            parent_trace_id,
            Some(delegation),
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
        agent_chain: &agent_chain,
        state_arc,
        background_job_id,
        resume_history,
    };
    let run_result = Box::pin(super::sub_agent::run_sub_agent(&mut sub_ctx)).await;
    let (trace, exec) = match run_result {
        Ok(result) => match serialize_subagent_result_without_task_id(&result) {
            Ok(json) => {
                let json = handoff_with_open_background_jobs(
                    state,
                    conversation_id,
                    &instance_scope.agent_instance_id,
                    json,
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
                        "completed",
                        Some(truncate_str(&result.content, 160)),
                        parent_trace_id,
                        Some(delegation),
                    ),
                    Ok((json, true, None)),
                )
            }
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
                        parent_trace_id,
                        Some(delegation),
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
                    parent_trace_id,
                    Some(delegation),
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
        emit_host_tool_status,
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

pub(crate) struct BackgroundOwnedSpawn {
    pub stream: super::StreamTx,
    pub state: Arc<AppState>,
    pub conversation_id: String,
    pub cancel: CancellationToken,
    pub settings: crate::models::ModelSettings,
    pub api_key: String,
    pub parent_task_board_store_key: String,
    pub message_id: String,
    pub tool_call_id: String,
    pub run_id: String,
    pub task: AgentTask,
    pub source: OwnedSubagentSource,
    pub enabled_skill_ids: Vec<String>,
    pub agent_skill_overrides: HashMap<String, Vec<String>>,
    pub child_spawn_depth: u32,
    pub max_spawn_depth: u32,
    pub host_trace_id: Option<String>,
    pub host_scoped_message_id: Option<String>,
    /// `AgentTrace.id` of the issuing agent instance (nested spawn); `None` for the lead.
    pub issuer_trace_id: Option<String>,
    pub instance_scope: AgentInstanceScope,
    /// Owner chain of the agent that issued this spawn (empty = lead). Becomes
    /// the job's `owner_chain` and the prefix of the child's own chain.
    pub issuer_chain: Vec<String>,
    /// Restored chain when this spawn resumes a worker (`followupInstanceId`);
    /// `None` mints `issuer_chain + [child instance id]`.
    pub resume_agent_chain: Option<Vec<String>>,
    pub resume_history: Option<super::worker_followup::ResumedWorkerHistory>,
    pub followup_reserve: Option<super::job_supervisor::FollowupReserve>,
}

pub(crate) fn emit_background_jobs(
    stream: &super::StreamTx,
    conversation_id: &str,
    jobs: &super::job_supervisor::JobSupervisor,
) {
    let event = jobs.background_jobs_event(conversation_id);
    log_background_jobs_occupancy(&event);
    emit(stream, event);
}

pub(crate) fn publish_background_jobs(
    conversation_id: &str,
    jobs: &super::job_supervisor::JobSupervisor,
) {
    let event = jobs.background_jobs_event(conversation_id);
    log_background_jobs_occupancy(&event);
    crate::stream_broadcast::publish_global_stream(event);
}

fn log_background_jobs_occupancy(event: &StreamEvent) {
    let StreamEvent::BackgroundJobs {
        conversation_id,
        running_count,
        jobs,
    } = event
    else {
        return;
    };
    let terminals = jobs.iter().filter(|job| job.kind == "terminal").count();
    log::info!(
        "background_jobs occupancy conversation_id={conversation_id} running_count={running_count} terminals={terminals}"
    );
}

/// Register a background job and return `jobId` without joining the child.
pub(crate) fn mint_owned_child_instance_scope(
    source: &OwnedSubagentSource,
    run_id: &str,
    conversation_id: &str,
) -> AgentInstanceScope {
    let role = match source {
        OwnedSubagentSource::SelfFork(snapshot) => snapshot.def.id.as_str(),
        OwnedSubagentSource::Registered(def) => def.id.as_str(),
    };
    AgentInstanceScope::new(run_id, conversation_id, role)
}

pub(crate) fn spawn_background_owned_subagent(spawn: BackgroundOwnedSpawn) -> String {
    let agent_id = match &spawn.source {
        OwnedSubagentSource::SelfFork(snapshot) => snapshot.def.id.clone(),
        OwnedSubagentSource::Registered(def) => def.id.clone(),
    };
    let kind = super::job_supervisor::JobKind::Subagent(super::job_supervisor::JobKindSubagent {
        tool_call_id: spawn.tool_call_id.clone(),
        message_id: spawn.message_id.clone(),
        agent_id,
        title: spawn.task.title.clone(),
        agent_instance_id: spawn.instance_scope.agent_instance_id.clone(),
    });
    let job_id = spawn.state.jobs.register(
        &spawn.conversation_id,
        kind,
        spawn.cancel.clone(),
        &spawn.run_id,
        spawn.issuer_chain.clone(),
    );
    emit_background_jobs(&spawn.stream, &spawn.conversation_id, &spawn.state.jobs);
    log::info!(
        "run_subagent background spawn job_id={job_id} conversation_id={} tool_call_id={} task_id={}",
        spawn.conversation_id,
        spawn.tool_call_id,
        spawn.task.id
    );
    tokio::spawn(run_background_owned_subagent(job_id.clone(), spawn));
    job_id
}

async fn run_background_owned_subagent(job_id: String, mut spawn: BackgroundOwnedSpawn) {
    // Held until this task returns so a second follow-up cannot start on the same instance.
    let _followup_reserve = spawn.followup_reserve.take();
    let started = Instant::now();
    let cap =
        crate::tools::parallel::ParallelLimits::from_settings(&spawn.state.effective_settings())
            .max_parallel_sub_agents;
    let cap = super::job_supervisor::JobSupervisor::slot_cap_from(cap);
    let _lease = if super::job_supervisor::worker_needs_root_slot(spawn.child_spawn_depth) {
        match spawn
            .state
            .jobs
            .acquire_root(&spawn.conversation_id, cap, &spawn.cancel)
            .await
        {
            Some(lease) => Some(lease),
            None => {
                log::info!(
                    "run_subagent background cancelled before slot job_id={job_id} conversation_id={}",
                    spawn.conversation_id
                );
                let outcome = cancelled_owned_subagent_outcome(
                    &spawn.run_id,
                    &spawn.conversation_id,
                    &spawn.tool_call_id,
                    spawn.task.clone(),
                    &spawn.source,
                    spawn.issuer_trace_id.as_deref(),
                    spawn.child_spawn_depth,
                    Some(spawn.instance_scope.clone()),
                );
                super::deferred_token_finalize::finish_job_and_maybe_finalize_arc(
                    &spawn.state,
                    &job_id,
                    super::job_supervisor::JobStatus::Cancelled,
                    None,
                    Some("cancelled".into()),
                )
                .await;
                let duration_ms = started.elapsed().as_millis() as u64;
                publish_owned_subagent_ui_finished(
                    &spawn.stream,
                    &spawn.message_id,
                    &spawn.tool_call_id,
                    &outcome.trace,
                    &outcome.exec,
                    duration_ms,
                    spawn.host_trace_id.as_deref(),
                    spawn.host_scoped_message_id.as_deref(),
                    false,
                );
                complete_background_host_tool(
                    &spawn.stream,
                    &spawn.conversation_id,
                    &spawn.message_id,
                    &spawn.tool_call_id,
                    &job_id,
                    super::job_supervisor::JobStatus::Cancelled,
                    Some("cancelled"),
                    Some(duration_ms),
                    spawn.host_trace_id.as_deref(),
                    spawn.host_scoped_message_id.as_deref(),
                    Some(spawn.instance_scope.agent_instance_id.as_str()),
                );
                emit_background_jobs(&spawn.stream, &spawn.conversation_id, &spawn.state.jobs);
                return;
            }
        }
    } else {
        if let Err(msg) = spawn.state.jobs.acquire_nested(&spawn.conversation_id) {
            log::error!(
                "run_subagent background nested refused job_id={job_id} conversation_id={}: {msg}",
                spawn.conversation_id
            );
            super::deferred_token_finalize::finish_job_and_maybe_finalize_arc(
                &spawn.state,
                &job_id,
                super::job_supervisor::JobStatus::Failed,
                None,
                Some(msg.clone()),
            )
            .await;
            let outcome = failed_owned_subagent_outcome(
                &spawn.run_id,
                &spawn.conversation_id,
                &spawn.tool_call_id,
                spawn.task.clone(),
                &spawn.source,
                spawn.issuer_trace_id.as_deref(),
                spawn.child_spawn_depth,
                Some(spawn.instance_scope.clone()),
                msg.clone(),
            );
            let duration_ms = started.elapsed().as_millis() as u64;
            publish_owned_subagent_ui_finished(
                &spawn.stream,
                &spawn.message_id,
                &spawn.tool_call_id,
                &outcome.trace,
                &outcome.exec,
                duration_ms,
                spawn.host_trace_id.as_deref(),
                spawn.host_scoped_message_id.as_deref(),
                false,
            );
            complete_background_host_tool(
                &spawn.stream,
                &spawn.conversation_id,
                &spawn.message_id,
                &spawn.tool_call_id,
                &job_id,
                super::job_supervisor::JobStatus::Failed,
                Some(msg.as_str()),
                Some(duration_ms),
                spawn.host_trace_id.as_deref(),
                spawn.host_scoped_message_id.as_deref(),
                Some(spawn.instance_scope.agent_instance_id.as_str()),
            );
            emit_background_jobs(&spawn.stream, &spawn.conversation_id, &spawn.state.jobs);
            return;
        }
        None
    };
    spawn.state.jobs.mark_running(&job_id);
    let provider = OpenAIProvider::new(spawn.settings.clone(), spawn.api_key.clone());
    let input = OwnedSubagentExecutionInput {
        stream: &spawn.stream,
        state: spawn.state.as_ref(),
        conversation_id: &spawn.conversation_id,
        cancel: spawn.cancel.clone(),
        provider,
        parent_task_board_store_key: spawn.parent_task_board_store_key.clone(),
        message_id: spawn.message_id.clone(),
        tool_call_id: spawn.tool_call_id.clone(),
        run_id: spawn.run_id.clone(),
        task: spawn.task.clone(),
        source: spawn.source.clone(),
        enabled_skill_ids: spawn.enabled_skill_ids.clone(),
        agent_skill_overrides: spawn.agent_skill_overrides.clone(),
        child_spawn_depth: spawn.child_spawn_depth,
        max_spawn_depth: spawn.max_spawn_depth,
        issuer_chain: spawn.issuer_chain.clone(),
        resume_agent_chain: spawn.resume_agent_chain.clone(),
        host_trace_id: spawn.host_trace_id.clone(),
        host_scoped_message_id: spawn.host_scoped_message_id.clone(),
        issuer_trace_id: spawn.issuer_trace_id.clone(),
        state_arc: spawn.state.clone(),
        emit_host_tool_status: false,
        instance_scope: Some(spawn.instance_scope.clone()),
        background_job_id: Some(job_id.clone()),
        resume_history: spawn.resume_history.clone(),
        followup_reserve: None,
    };
    let outcome = execute_owned_subagent(input).await;
    drop(_lease);
    let cancelled = spawn.cancel.is_cancelled();
    let (status, content, error) = match &outcome.exec {
        Ok((body, true, _)) => (
            super::job_supervisor::JobStatus::Completed,
            Some(body.clone()),
            None,
        ),
        Ok((body, false, note)) => {
            let st = if cancelled {
                super::job_supervisor::JobStatus::Cancelled
            } else {
                super::job_supervisor::JobStatus::Failed
            };
            (st, Some(body.clone()), note.clone())
        }
        Err(err) => {
            let st = if cancelled {
                super::job_supervisor::JobStatus::Cancelled
            } else {
                super::job_supervisor::JobStatus::Failed
            };
            (st, None, Some(err.to_string()))
        }
    };
    super::deferred_token_finalize::finish_job_and_maybe_finalize_arc(
        &spawn.state,
        &job_id,
        status,
        content,
        error,
    )
    .await;
    let duration_ms = started.elapsed().as_millis() as u64;
    log::info!(
        "run_subagent background finished job_id={job_id} conversation_id={} tool_call_id={} status={} duration_ms={}",
        spawn.conversation_id,
        spawn.tool_call_id,
        status.as_str(),
        duration_ms
    );
    complete_background_host_tool(
        &spawn.stream,
        &spawn.conversation_id,
        &spawn.message_id,
        &spawn.tool_call_id,
        &job_id,
        status,
        outcome
            .exec
            .as_ref()
            .ok()
            .and_then(|(_, _, note)| note.clone())
            .or_else(|| outcome.exec.as_ref().err().map(|e| e.to_string()))
            .as_deref(),
        Some(duration_ms),
        spawn.host_trace_id.as_deref(),
        spawn.host_scoped_message_id.as_deref(),
        Some(spawn.instance_scope.agent_instance_id.as_str()),
    );
    emit_background_jobs(&spawn.stream, &spawn.conversation_id, &spawn.state.jobs);
}

/// Host `run_subagent` result for a background spawn: a handle, never the worker body.
/// Aligns with Cursor (do not splice background answers into the original Task) and
/// Codex (`spawn_agent` stays an id; content comes from wait/notification).
/// `kind` is `subagent` or `terminal` so the parent can tell them apart.
pub(crate) fn background_job_handle_json(
    job_id: &str,
    status: super::job_supervisor::JobStatus,
    kind: &'static str,
    agent_instance_id: Option<&str>,
) -> String {
    let mut obj = serde_json::json!({
        "jobId": job_id,
        "status": status.as_str(),
        "kind": kind,
    });
    if let Some(id) = agent_instance_id.filter(|s| !s.trim().is_empty()) {
        obj["agentInstanceId"] = serde_json::Value::String(id.to_string());
    }
    obj.to_string()
}

fn host_ui_status_for_job(status: super::job_supervisor::JobStatus) -> &'static str {
    match status {
        super::job_supervisor::JobStatus::Completed => "success",
        _ => "failed",
    }
}

pub(crate) fn emit_and_persist_host_tool_finish(
    stream: &super::StreamTx,
    conversation_id: &str,
    message_id: &str,
    tool_call_id: &str,
    ui_status: &str,
    result: String,
    error: Option<&str>,
    duration_ms: Option<u64>,
    host_trace_id: Option<&str>,
    host_scoped_message_id: Option<&str>,
) {
    emit(
        stream,
        StreamEvent::ToolCallStatus {
            message_id: message_id.to_string(),
            tool_call_id: tool_call_id.to_string(),
            status: ui_status.into(),
            result: Some(result.clone()),
            error: error.map(str::to_string),
            duration_ms,
            display_label: None,
            display_summary: None,
            trace_id: trace_id_opt(host_trace_id),
            scoped_message_id: trace_id_opt(host_scoped_message_id),
        },
    );
    persist_background_host_tool_finish(
        conversation_id,
        message_id,
        tool_call_id,
        ui_status,
        Some(result),
        error,
        duration_ms,
    );
}

fn complete_background_host_tool(
    stream: &super::StreamTx,
    conversation_id: &str,
    message_id: &str,
    tool_call_id: &str,
    job_id: &str,
    job_status: super::job_supervisor::JobStatus,
    error: Option<&str>,
    duration_ms: Option<u64>,
    host_trace_id: Option<&str>,
    host_scoped_message_id: Option<&str>,
    agent_instance_id: Option<&str>,
) {
    let handle = background_job_handle_json(job_id, job_status, "subagent", agent_instance_id);
    let ui_status = host_ui_status_for_job(job_status);
    emit_and_persist_host_tool_finish(
        stream,
        conversation_id,
        message_id,
        tool_call_id,
        ui_status,
        handle,
        error,
        duration_ms,
        host_trace_id,
        host_scoped_message_id,
    );
}

fn persist_background_host_tool_finish(
    conversation_id: &str,
    message_id: &str,
    tool_call_id: &str,
    status: &str,
    result: Option<String>,
    error: Option<&str>,
    duration_ms: Option<u64>,
) {
    let store = match crate::conversation_store::global_store() {
        Ok(store) => store,
        Err(err) => {
            log::warn!(
                "run_subagent background persist skipped conversation_id={conversation_id}: {err:#}"
            );
            return;
        }
    };
    let mut msg = match store.load_message(conversation_id, message_id) {
        Ok(Some(msg)) => msg,
        Ok(None) => {
            log::warn!(
                "run_subagent background persist missed message conversation_id={conversation_id} message_id={message_id}"
            );
            return;
        }
        Err(err) => {
            log::warn!(
                "run_subagent background load_message failed conversation_id={conversation_id} message_id={message_id}: {err:#}"
            );
            return;
        }
    };
    let Some(tc) = msg
        .tool_calls
        .as_mut()
        .and_then(|calls| calls.iter_mut().find(|c| c.id == tool_call_id))
    else {
        log::warn!(
            "run_subagent background persist missed tool_call conversation_id={conversation_id} tool_call_id={tool_call_id}"
        );
        return;
    };
    tc.status = status.to_string();
    if let Some(result) = result.clone() {
        tc.result = Some(result);
    }
    tc.error = error.map(str::to_string);
    if let Some(duration_ms) = duration_ms {
        tc.duration_ms = Some(duration_ms);
    }
    let host_still_open = msg.tool_calls.as_ref().is_some_and(|calls| {
        calls.iter().any(|c| {
            matches!(
                c.status.as_str(),
                "running" | "pending" | "pending_approval"
            )
        })
    });
    if !host_still_open && matches!(msg.status.as_str(), "streaming" | "pending") {
        msg.status = "done".into();
    }
    super::conversation_persist::upsert_message(conversation_id, &msg);
    if let Some(handle) = result {
        match store.load_tool_message_by_call_id(conversation_id, tool_call_id) {
            Ok(Some(mut tool_msg)) => {
                tool_msg.content = handle;
                if tool_msg.status == "streaming" || tool_msg.status.is_empty() {
                    tool_msg.status = "completed".into();
                }
                super::conversation_persist::upsert_message(conversation_id, &tool_msg);
            }
            Ok(None) => {}
            Err(err) => {
                log::warn!(
                    "run_subagent background load tool message failed conversation_id={conversation_id} tool_call_id={tool_call_id}: {err:#}"
                );
            }
        }
    }
    log::info!(
        "background host persisted tool conversation_id={conversation_id} tool_call_id={tool_call_id} status={status}"
    );
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
    let persist = super::sub_message::should_persist_anchor_agent_trace(&agent.status);
    emit_agent_step(stream, ctx.message_id, ctx.agent_trace, agent);
    if !persist {
        return;
    }
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
    parent_trace_id: Option<&str>,
    delegation: Option<&str>,
) -> AgentTrace {
    AgentTrace {
        // Phase G: UI / store primary key is SpawnId (= agent_instance_id).
        // ChatMessage.trace_id still uses sub_agent_trace_id for SQLite / SSE compat.
        id: instance_scope.agent_instance_id.clone(),
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
        summary_line: None,
        task_id: Some(task.id.clone()),
        agent_id: Some(def.id.clone()),
        search_tool_call_ids: None,
        parent_trace_id: parent_trace_id
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(str::to_string),
        delegation: delegation
            .map(str::trim)
            .filter(|kind| !kind.is_empty())
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
    let issuer_trace_id = ctx.issuer_trace_id;
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
            if let Err(msg) = validate_background_target(&parsed) {
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
                    let mut follow = if let Some(id) = parsed.followup_instance_id.as_deref() {
                        match super::worker_followup::prepare_worker_followup(
                            state,
                            conversation_id,
                            run_id,
                            id,
                            &agent_id,
                            ctx.current_agent_id,
                            &parsed.goal,
                            &parsed.context,
                        ) {
                            Ok(prepared) => Some(prepared),
                            Err(msg) => {
                                return Ok((format!("ERROR: {msg}"), false, Some(msg)));
                            }
                        }
                    } else {
                        None
                    };
                    let child_spawn_depth = follow
                        .as_ref()
                        .map(|item| item.spawn_depth)
                        .unwrap_or(child_spawn_depth);
                    let tid = if let Some(item) = follow.as_ref() {
                        item.task_id.clone()
                    } else if parsed.task_id.trim().is_empty() {
                        new_id("sub_task")
                    } else {
                        parsed.task_id.trim().to_string()
                    };
                    let resume_history = follow.as_ref().map(|item| item.history.clone());
                    let mut follow_reserve = None;
                    if parsed.background {
                        let task = AgentTask {
                            id: tid,
                            agent_id: agent_id.clone(),
                            title: if parsed.title.trim().is_empty() {
                                format!("Delegated: {agent_id}")
                            } else {
                                parsed.title.clone()
                            },
                            goal: parsed.goal.clone(),
                            context: parsed.context.trim().to_string(),
                            depends_on: vec![],
                        };
                        if let Some(item) = follow.as_mut() {
                            follow_reserve = item.reserve.take();
                        }
                        let child_scope = if let Some(item) = follow.as_ref() {
                            item.instance_scope.clone()
                        } else {
                            mint_owned_child_instance_scope(
                                &OwnedSubagentSource::Registered(def.clone()),
                                run_id,
                                conversation_id,
                            )
                        };
                        let job_id = spawn_background_owned_subagent(BackgroundOwnedSpawn {
                            stream: stream.clone(),
                            state: ctx.state_arc.clone(),
                            conversation_id: conversation_id.to_string(),
                            cancel: CancellationToken::new(),
                            settings: provider.settings.clone(),
                            api_key: provider.api_key.clone(),
                            parent_task_board_store_key: parent_task_board_store_key.to_string(),
                            message_id: message_id.to_string(),
                            tool_call_id: tool_call_id.to_string(),
                            run_id: run_id.to_string(),
                            task,
                            source: OwnedSubagentSource::Registered(def),
                            enabled_skill_ids: enabled_skill_ids.to_vec(),
                            agent_skill_overrides: ctx.agent_skill_overrides.clone(),
                            child_spawn_depth,
                            max_spawn_depth,
                            host_trace_id: None,
                            host_scoped_message_id: None,
                            issuer_trace_id: issuer_trace_id.map(str::to_string),
                            instance_scope: child_scope.clone(),
                            issuer_chain: ctx.issuer_chain.to_vec(),
                            resume_agent_chain: follow
                                .as_ref()
                                .map(|item| item.agent_chain.clone()),
                            resume_history: resume_history.clone(),
                            followup_reserve: follow_reserve,
                        });
                        let body = background_job_handle_json(
                            &job_id,
                            super::job_supervisor::JobStatus::Running,
                            "subagent",
                            Some(child_scope.agent_instance_id.as_str()),
                        );
                        log::info!(
                            "run_subagent serial background spawn conversation_id={conversation_id} tool_call_id={tool_call_id} job_id={job_id}"
                        );
                        return Ok((body, true, None));
                    }
                    let slot_cap = super::job_supervisor::JobSupervisor::slot_cap_from(
                        crate::tools::parallel::ParallelLimits::from_settings(&provider.settings)
                            .max_parallel_sub_agents,
                    );
                    let _worker_slot = if super::job_supervisor::worker_needs_root_slot(
                        child_spawn_depth,
                    ) {
                        match state
                            .jobs
                            .acquire_root(conversation_id, slot_cap, cancel)
                            .await
                        {
                            Some(lease) => Some(lease),
                            None => {
                                let msg = "cancelled".to_string();
                                log::info!(
                                    "run_subagent serial cancelled before root slot conversation_id={conversation_id} tool_call_id={tool_call_id}"
                                );
                                return Ok((format!("ERROR: {msg}"), false, Some(msg)));
                            }
                        }
                    } else {
                        if let Err(msg) = state.jobs.acquire_nested(conversation_id) {
                            return Ok((format!("ERROR: {msg}"), false, Some(msg)));
                        }
                        None
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
                                return Err(anyhow::anyhow!(crate::i18n::generation_stopped_msg()));
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
                    let instance_scope = if let Some(item) = follow.as_ref() {
                        item.instance_scope.clone()
                    } else {
                        definition_source.new_instance_scope(run_id, conversation_id)
                    };
                    // Child chain = issuer chain + own instance id; a resumed
                    // worker keeps the chain it was originally spawned with.
                    let agent_chain: Vec<String> = match follow.as_ref() {
                        Some(item) => item.agent_chain.clone(),
                        None => {
                            let mut chain = ctx.issuer_chain.to_vec();
                            chain.push(instance_scope.agent_instance_id.clone());
                            chain
                        }
                    };
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
                            issuer_trace_id,
                            Some("registered"),
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
                        agent_chain: &agent_chain,
                        state_arc: ctx.state_arc.clone(),
                        background_job_id: None,
                        resume_history: resume_history.clone(),
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
                            let json = handoff_with_open_background_jobs(
                                state,
                                conversation_id,
                                &instance_scope.agent_instance_id,
                                json,
                            );
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
                                return Err(anyhow::anyhow!(crate::i18n::generation_stopped_msg()));
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
        background_job_handle_json, build_subagent_trace, commit_subagent_outcome,
        complete_background_host_tool, execute_owned_subagent, failed_owned_subagent_outcome,
        finalize_subagent_outcome, publish_owned_subagent_ui_finished,
        serialize_subagent_result_without_task_id, OwnedSubagentExecutionInput,
        OwnedSubagentSource, PreparedSubagentOutcome, SubagentCommitContext,
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
            agent_instance_id: "inst-fg".into(),
        };
        let json = serialize_subagent_result_without_task_id(&result).unwrap();
        assert!(!json.contains("sub_task_should_not_leak"));
        assert!(!json.contains("taskId"));
        assert!(json.contains("\"agentId\":\"coder\""));
        assert!(json.contains("\"agentName\":\"氛围编程\""));
        assert!(json.contains("\"content\":\"handoff body\""));
        assert!(json.contains("\"agentInstanceId\":\"inst-fg\""));
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
            None,
            Some("registered"),
        );

        assert_eq!(trace.id, "instance-1");
        assert_eq!(trace.agent_instance_id.as_deref(), Some("instance-1"));
        assert_eq!(trace.parent_tool_call_id.as_deref(), Some("call-1"));
        assert_eq!(trace.task_id.as_deref(), Some("task-1"));
        assert_eq!(trace.agent_id.as_deref(), Some("current-agent"));
        // Lead-spawned worker: no parent trace, but the delegation kind is recorded.
        assert_eq!(trace.parent_trace_id, None);
        assert_eq!(trace.delegation.as_deref(), Some("registered"));
    }

    /// Nested spawn: the issuing agent instance id becomes the child's `parent_trace_id`
    /// and a self fork is flagged so the UI can tell it apart from a real worker.
    #[test]
    fn nested_child_trace_records_parent_trace_id_and_self_fork_delegation() {
        let task = AgentTask {
            id: "task-2".into(),
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
            "instance-2",
        );

        let trace = build_subagent_trace(
            &task,
            &def,
            &scope,
            2,
            None,
            Some("call-2"),
            Some("round-message-1"),
            "running",
            None,
            Some("instance-1"),
            Some("self"),
        );

        assert_eq!(trace.id, "instance-2");
        assert_eq!(trace.depth, Some(2));
        assert_eq!(trace.parent_trace_id.as_deref(), Some("instance-1"));
        assert_eq!(trace.delegation.as_deref(), Some("self"));
        // The anchor stays this layer's own scoped row, never the lead message.
        assert_eq!(trace.anchor_message_id.as_deref(), Some("round-message-1"));
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
            agent_chain: None,
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
            None,
            Some("self"),
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
            true,
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
    fn background_job_handle_is_id_and_status_not_worker_body() {
        let json = background_job_handle_json(
            "job_1",
            crate::chat_service::job_supervisor::JobStatus::Completed,
            "subagent",
            None,
        );
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["jobId"], "job_1");
        assert_eq!(v["status"], "completed");
        assert_eq!(v["kind"], "subagent");
        assert!(v.get("content").is_none());
    }

    #[test]
    fn background_host_finish_keeps_handle_on_host_row() {
        let (stream, mut events) = crate::models::ChatStreamSender::pair("conversation", "user");
        let exec = Ok((
            r#"{"content":"worker handoff markdown"}"#.into(),
            true,
            None,
        ));
        publish_owned_subagent_ui_finished(
            &stream,
            "anchor",
            "call-1",
            &completed_trace(),
            &exec,
            42,
            None,
            None,
            false,
        );
        assert!(matches!(
            events.try_recv(),
            Ok(crate::models::StreamEvent::AgentStep { .. })
        ));
        assert!(events.try_recv().is_err());

        complete_background_host_tool(
            &stream,
            "conversation",
            "anchor",
            "call-1",
            "job_1",
            crate::chat_service::job_supervisor::JobStatus::Completed,
            None,
            Some(42),
            None,
            None,
            None,
        );
        match events.try_recv() {
            Ok(crate::models::StreamEvent::ToolCallStatus {
                tool_call_id,
                status,
                result,
                ..
            }) => {
                assert_eq!(tool_call_id, "call-1");
                assert_eq!(status, "success");
                let body = result.expect("handle");
                assert!(body.contains("job_1"));
                assert!(body.contains("completed"));
                assert!(body.contains("subagent"));
                assert!(!body.contains("worker handoff"));
            }
            other => panic!("expected host handle status, got {other:?}"),
        }
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

        let preset = AgentInstanceScope::with_instance_id(
            "run",
            "conversation",
            "self",
            "preset-instance-id",
        );
        let outcome = failed_owned_subagent_outcome(
            "run",
            "conversation",
            "call-failed",
            task,
            &OwnedSubagentSource::SelfFork(snapshot),
            Some("parent-instance-id"),
            2,
            Some(preset),
            "spawn depth limit".into(),
        );

        assert_eq!(outcome.tool_call_id, "call-failed");
        assert_eq!(outcome.trace.status, "failed");
        assert_eq!(
            outcome.trace.agent_instance_id.as_deref(),
            Some("preset-instance-id")
        );
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
        let state = std::sync::Arc::new(crate::chat_service::AppState::new());
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
            state: state.as_ref(),
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
            issuer_chain: Vec::new(),
            resume_agent_chain: None,
            host_trace_id: None,
            host_scoped_message_id: None,
            issuer_trace_id: None,
            state_arc: state.clone(),
            emit_host_tool_status: true,
            instance_scope: None,
            background_job_id: None,
            resume_history: None,
            followup_reserve: None,
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
        let state = std::sync::Arc::new(crate::chat_service::AppState::new());
        let history = vec![anchor_message()];
        let traces: Vec<crate::models::AgentTrace> = Vec::new();
        let stats = ConversationLlmStats::default();

        let execute = |tool_call_id: &str| {
            let cancel = tokio_util::sync::CancellationToken::new();
            cancel.cancel();
            execute_owned_subagent(OwnedSubagentExecutionInput {
                stream: &stream,
                state: state.as_ref(),
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
                issuer_chain: Vec::new(),
                resume_agent_chain: None,
                host_trace_id: None,
                host_scoped_message_id: None,
                issuer_trace_id: None,
                state_arc: state.clone(),
                emit_host_tool_status: true,
                instance_scope: None,
                background_job_id: None,
                resume_history: None,
                followup_reserve: None,
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
