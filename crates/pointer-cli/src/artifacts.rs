//! Run observability artifacts for the headless CLI.
//!
//! When `--out-dir` is given, `pointer-cli` writes four artifacts next to the
//! existing `result.json`:
//!
//! - `events.jsonl` — one JSON line per [`AgentEvent`], in emission order. The
//!   same lines are mirrored to stdout when `--events jsonl` is passed.
//! - `agent.log` — human-readable, timestamped rendering of the event stream
//!   plus the token-usage summary.
//! - `usage.json` — per-run token accounting read back from
//!   [`pointer_core::token_usage_store`] (the same store the desktop/server
//!   hosts populate), so it works for any provider that reports `usage`.
//! - `trajectory.json` — an ATIF v1.7 trajectory (Harbor-validated) built from
//!   the persisted conversation messages.
//!
//! Artifact writing is a side channel: a failure is reported on stderr but does
//! **not** change the run's semantic exit code.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use pointer_core::agent_events::{AgentEvent, AgentEventBus};
use pointer_core::models::{ChatMessage, Role};
use serde_json::{json, Value};

/// Max characters kept for a single tool observation in `trajectory.json`.
/// Large terminal dumps would otherwise bloat the trajectory beyond what the
/// Harbor viewer / validator need.
const OBSERVATION_MAX_CHARS: usize = 16_000;

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Epoch milliseconds → RFC 3339 UTC (ATIF `timestamp` format).
fn iso8601(ms: i64) -> Option<String> {
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|dt| dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
}

fn truncate_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let head: String = text.chars().take(max).collect();
    format!("{head}\n…[truncated {} chars]", text.chars().count() - max)
}

/// Parse a persisted tool-call `arguments` JSON string into a JSON object.
/// ATIF requires `arguments` to be a dict; anything unparseable is wrapped so
/// no information is silently dropped.
fn parse_arguments(raw: &str) -> Value {
    match serde_json::from_str::<Value>(raw) {
        Ok(Value::Object(map)) => Value::Object(map),
        Ok(other) => json!({ "_value": other }),
        Err(_) => json!({ "_raw": raw }),
    }
}

/// One recorded event: the raw payload plus the wall-clock time it was seen.
#[derive(Clone)]
struct Recorded {
    at_ms: i64,
    value: Value,
}

struct Sink {
    file: Option<File>,
    stdout: bool,
}

struct RecorderState {
    events: Mutex<Vec<Recorded>>,
    sink: Mutex<Sink>,
}

/// Live recorder for the dispatcher's [`AgentEventBus`].
///
/// `start` subscribes before dispatch and drains the broadcast channel on a
/// background task, so long runs do not lose early events to the channel
/// capacity. `stop` flushes any buffered events and joins the task.
pub struct EventRecorder {
    state: Arc<RecorderState>,
    stop: Arc<tokio::sync::Notify>,
    handle: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl EventRecorder {
    /// `out_dir` — when set, append JSON lines to `<out_dir>/events.jsonl`.
    /// `mirror_stdout` — also print each line to stdout (`--events jsonl`).
    pub fn new(out_dir: Option<&Path>, mirror_stdout: bool) -> Result<Self> {
        let file = match out_dir {
            Some(dir) => {
                let path = dir.join("events.jsonl");
                Some(
                    File::create(&path)
                        .with_context(|| format!("failed to create {}", path.display()))?,
                )
            }
            None => None,
        };
        Ok(Self {
            state: Arc::new(RecorderState {
                events: Mutex::new(Vec::new()),
                sink: Mutex::new(Sink {
                    file,
                    stdout: mirror_stdout,
                }),
            }),
            stop: Arc::new(tokio::sync::Notify::new()),
            handle: Mutex::new(None),
        })
    }

    /// Subscribe to `bus` and start draining on the current tokio runtime.
    pub fn start(&self, bus: &AgentEventBus) {
        let mut rx = bus.subscribe();
        let state = self.state.clone();
        let stop = self.stop.clone();
        let handle = tokio::spawn(async move {
            loop {
                tokio::select! {
                    biased;
                    _ = stop.notified() => {
                        // Drain whatever is already buffered, then exit.
                        while let Ok(ev) = rx.try_recv() {
                            record(&state, &ev);
                        }
                        break;
                    }
                    received = rx.recv() => match received {
                        Ok(ev) => record(&state, &ev),
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            eprintln!(
                                "pointer-cli: warning: event stream lagged, {n} event(s) dropped"
                            );
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    },
                }
            }
        });
        *self.handle.lock().unwrap() = Some(handle);
    }

    /// Flush buffered events and stop the drain task.
    pub async fn stop(&self) {
        self.stop.notify_one();
        let handle = self.handle.lock().unwrap().take();
        if let Some(handle) = handle {
            let _ = handle.await;
        }
    }

    /// Snapshot of the recorded events (arrival-ordered).
    pub fn events(&self) -> Vec<(i64, Value)> {
        self.state
            .events
            .lock()
            .unwrap()
            .iter()
            .map(|r| (r.at_ms, r.value.clone()))
            .collect()
    }
}

fn record(state: &Arc<RecorderState>, ev: &AgentEvent) {
    let at_ms = now_ms();
    let value = serde_json::to_value(ev).unwrap_or(Value::Null);
    let line = serde_json::to_string(ev).unwrap_or_else(|_| "{}".to_string());
    state
        .events
        .lock()
        .unwrap()
        .push(Recorded { at_ms, value });
    let mut sink = state.sink.lock().unwrap();
    if let Some(file) = sink.file.as_mut() {
        let _ = file.write_all(line.as_bytes());
        let _ = file.write_all(b"\n");
    }
    if sink.stdout {
        println!("{line}");
        let _ = std::io::stdout().flush();
    }
}

/// Token usage aggregated for one CLI run.
#[derive(Default, Clone)]
pub struct UsageSummary {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub thinking_tokens: u64,
    pub cached_tokens: u64,
    pub total_tokens: u64,
    pub llm_rounds: u64,
    /// Per `(agent_instance_id, model)` rows, verbatim from the store.
    pub rows: Vec<Value>,
}

impl UsageSummary {
    fn add(&mut self, item: &pointer_core::token_usage_store::TokenUsageListItem) {
        self.prompt_tokens = self.prompt_tokens.saturating_add(item.prompt_tokens);
        self.completion_tokens = self.completion_tokens.saturating_add(item.completion_tokens);
        self.thinking_tokens = self.thinking_tokens.saturating_add(item.thinking_tokens);
        self.cached_tokens = self.cached_tokens.saturating_add(item.cached_tokens);
        self.total_tokens = self.total_tokens.saturating_add(item.total_tokens);
        self.llm_rounds = self.llm_rounds.saturating_add(item.llm_rounds);
        self.rows.push(json!({
            "agent_instance_id": item.agent_instance_id,
            "agent_role_id": item.agent_role_id,
            "model_name": item.model_name,
            "prompt_tokens": item.prompt_tokens,
            "completion_tokens": item.completion_tokens,
            "thinking_tokens": item.thinking_tokens,
            "cached_tokens": item.cached_tokens,
            "total_tokens": item.total_tokens,
            "llm_rounds": item.llm_rounds,
            "source": item.source,
        }));
    }
}

/// Read back this run's usage rows from the process-wide token usage store.
/// Failures are non-fatal (returned as an empty summary + stderr warning).
pub fn load_usage(conversation_id: &str) -> UsageSummary {
    let mut summary = UsageSummary::default();
    match pointer_core::token_usage_store::list_token_usage(None, None) {
        Ok(list) => {
            for item in list.items.iter().filter(|i| i.conversation_id == conversation_id) {
                summary.add(item);
            }
        }
        Err(err) => {
            eprintln!("pointer-cli: warning: failed to read token usage: {err:#}");
        }
    }
    summary
}

/// Identity + terminal state of the run, shared by the artifact writers.
pub struct RunMeta<'a> {
    pub conversation_id: &'a str,
    pub run_id: &'a str,
    pub model: &'a str,
    pub status: &'a str,
    pub exit_code: i32,
    pub error: Option<&'a str>,
}

/// Write `<out_dir>/usage.json`.
pub fn write_usage_json(out_dir: &Path, meta: &RunMeta<'_>, usage: &UsageSummary) -> Result<PathBuf> {
    let doc = json!({
        "conversation_id": meta.conversation_id,
        "run_id": meta.run_id,
        "model": meta.model,
        "totals": {
            "prompt_tokens": usage.prompt_tokens,
            "completion_tokens": usage.completion_tokens,
            "thinking_tokens": usage.thinking_tokens,
            "cached_tokens": usage.cached_tokens,
            "total_tokens": usage.total_tokens,
            "llm_rounds": usage.llm_rounds,
            "cost_usd": Value::Null,
        },
        "by_agent_instance": usage.rows,
        "source": "pointer-core/token_usage_store",
        "generated_at": iso8601(now_ms()),
    });
    let path = out_dir.join("usage.json");
    std::fs::write(&path, serde_json::to_vec_pretty(&doc)?)
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(path)
}

fn event_kind(value: &Value) -> &str {
    value.get("kind").and_then(Value::as_str).unwrap_or("unknown")
}

fn field<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("-")
}

/// Render one recorded event as a human-readable log line.
fn log_line(at_ms: i64, value: &Value) -> String {
    let ts = iso8601(at_ms).unwrap_or_else(|| "-".to_string());
    let seq = value
        .get("seq")
        .map(|s| s.to_string())
        .unwrap_or_else(|| "-".to_string());
    match event_kind(value) {
        "run_queued" => format!("{ts} INFO  run_queued run_id={} seq={seq}", field(value, "run_id")),
        "run_started" => format!(
            "{ts} INFO  run_started run_id={} conversation_id={} seq={seq}",
            field(value, "run_id"),
            field(value, "conversation_id")
        ),
        "assistant_delta" => format!(
            "{ts} DEBUG assistant_delta message_id={} chars={} seq={seq}",
            field(value, "message_id"),
            value.get("text").and_then(Value::as_str).map(str::chars).map(Iterator::count).unwrap_or(0)
        ),
        "tool_call_start" => format!(
            "{ts} INFO  tool_call_start tool={} call_id={} seq={seq}",
            field(value, "tool_name"),
            field(value, "tool_call_id")
        ),
        "tool_call_result" => format!(
            "{ts} INFO  tool_call_result status={} call_id={} seq={seq}",
            field(value, "status"),
            field(value, "tool_call_id")
        ),
        "run_finished" => format!(
            "{ts} INFO  run_finished run_id={} conversation_id={} seq={seq}",
            field(value, "run_id"),
            field(value, "conversation_id")
        ),
        "run_failed" => format!(
            "{ts} ERROR run_failed run_id={} error={} seq={seq}",
            field(value, "run_id"),
            field(value, "error")
        ),
        "run_cancelled" => format!(
            "{ts} WARN  run_cancelled run_id={} conversation_id={} seq={seq}",
            field(value, "run_id"),
            field(value, "conversation_id")
        ),
        other => format!("{ts} INFO  {other} seq={seq}"),
    }
}

/// Write `<out_dir>/agent.log` from the recorded event stream + usage summary.
pub fn write_agent_log(
    out_dir: &Path,
    events: &[(i64, Value)],
    meta: &RunMeta<'_>,
    usage: &UsageSummary,
) -> Result<PathBuf> {
    let mut out = String::new();
    out.push_str("# pointer-cli run log\n");
    out.push_str(&format!(
        "# run_id={} conversation_id={} model={} events={}\n",
        meta.run_id,
        meta.conversation_id,
        meta.model,
        events.len()
    ));
    for (at_ms, value) in events {
        out.push_str(&log_line(*at_ms, value));
        out.push('\n');
    }
    out.push_str(&format!(
        "# summary status={} exit_code={} error={}\n",
        meta.status,
        meta.exit_code,
        meta.error.unwrap_or("-")
    ));
    out.push_str(&format!(
        "# usage prompt_tokens={} completion_tokens={} thinking_tokens={} cached_tokens={} total_tokens={} llm_rounds={} cost_usd=unknown\n",
        usage.prompt_tokens,
        usage.completion_tokens,
        usage.thinking_tokens,
        usage.cached_tokens,
        usage.total_tokens,
        usage.llm_rounds
    ));
    let path = out_dir.join("agent.log");
    std::fs::write(&path, out).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(path)
}

/// Everything the ATIF builder needs about the finished run.
pub struct TrajectoryContext<'a> {
    pub instruction: &'a str,
    pub conversation_id: &'a str,
    pub run_id: &'a str,
    pub model: &'a str,
    pub status: &'a str,
    pub exit_code: i32,
    pub messages: &'a [ChatMessage],
    pub usage: &'a UsageSummary,
}

/// One scoped sub-agent transcript: the persisted rows that share one
/// `agent_instance_id` (or, for legacy rows, one `trace_id` / `task_id`).
struct SubAgentGroup<'a> {
    anchor_message_id: Option<String>,
    agent_name: Option<String>,
    agent_id: Option<String>,
    agent_instance_id: Option<String>,
    trace_id: Option<String>,
    task_id: Option<String>,
    spawn_depth: Option<u32>,
    rows: Vec<&'a ChatMessage>,
}

/// Grouping key for scoped sub-agent rows. `agent_instance_id` identifies one
/// runtime invocation; `trace_id` / `task_id` are the legacy fallback.
fn sub_agent_group_key(msg: &ChatMessage) -> String {
    if let Some(instance) = msg
        .agent_instance_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return format!("instance:{instance}");
    }
    let trace = msg.trace_id.as_deref().unwrap_or("").trim();
    let task = msg.task_id.as_deref().unwrap_or("").trim();
    if !trace.is_empty() || !task.is_empty() {
        return format!("trace:{trace}|task:{task}");
    }
    let anchor = msg.anchor_message_id.as_deref().unwrap_or("").trim();
    format!("anchor:{anchor}")
}

/// `tool_call_id` → observation content, from the `role: tool` rows of `messages`.
fn collect_tool_results(messages: &[&ChatMessage]) -> HashMap<String, String> {
    let mut tool_results: HashMap<String, String> = HashMap::new();
    for msg in messages {
        if matches!(msg.role, Role::Tool) {
            if let Some(id) = msg.tool_call_id.as_ref() {
                if !msg.content.trim().is_empty() {
                    tool_results.insert(id.clone(), msg.content.clone());
                }
            }
        }
    }
    tool_results
}

/// Map one assistant message to an ATIF `source: agent` step, folding the
/// matching `role: tool` rows into `observation.results`.
fn agent_step(
    step_id: usize,
    msg: &ChatMessage,
    model: &str,
    tool_results: &HashMap<String, String>,
) -> Value {
    let text = if !msg.content.trim().is_empty() {
        msg.content.clone()
    } else {
        msg.raw_content.clone().unwrap_or_default()
    };
    let mut step = json!({
        "step_id": step_id,
        "source": "agent",
        "message": text,
        "model_name": model,
    });
    if let Some(ts) = iso8601(msg.created_at) {
        step["timestamp"] = json!(ts);
    }

    if let Some(tool_calls) = msg.tool_calls.as_ref().filter(|c| !c.is_empty()) {
        let calls: Vec<Value> = tool_calls
            .iter()
            .map(|tc| {
                json!({
                    "tool_call_id": tc.id,
                    "function_name": tc.name,
                    "arguments": parse_arguments(&tc.arguments),
                })
            })
            .collect();
        let results: Vec<Value> = tool_calls
            .iter()
            .map(|tc| {
                let content = tool_results
                    .get(&tc.id)
                    .cloned()
                    .or_else(|| tc.result.clone())
                    .or_else(|| tc.error.clone())
                    .unwrap_or_default();
                json!({
                    "source_call_id": tc.id,
                    "content": truncate_chars(&content, OBSERVATION_MAX_CHARS),
                })
            })
            .collect();
        step["tool_calls"] = json!(calls);
        step["observation"] = json!({ "results": results });
    }

    step
}

fn push_subagent_ref(result: &mut Value, reference: Value) {
    match result
        .get_mut("subagent_trajectory_ref")
        .and_then(Value::as_array_mut)
    {
        Some(refs) => refs.push(reference),
        None => result["subagent_trajectory_ref"] = json!([reference]),
    }
}

/// Attach a `SubagentTrajectoryRef` to the observation of the step that
/// spawned a sub-agent. Prefers the exact `run_subagent` tool call (matched by
/// `task_hint`, i.e. the persisted `taskId`), then the next unused
/// `run_subagent` call on the step, and finally a standalone observation result
/// (ATIF allows a null `source_call_id`).
fn attach_subagent_ref(step: &mut Value, task_hint: Option<&str>, reference: Value) {
    if !step.get("observation").is_some_and(Value::is_object) {
        step["observation"] = json!({ "results": [] });
    }
    if !step["observation"]
        .get("results")
        .is_some_and(Value::is_array)
    {
        step["observation"]["results"] = json!([]);
    }

    let run_subagent_ids: Vec<String> = step
        .get("tool_calls")
        .and_then(Value::as_array)
        .map(|calls| {
            calls
                .iter()
                .filter(|call| {
                    call.get("function_name").and_then(Value::as_str) == Some("run_subagent")
                })
                .filter_map(|call| call.get("tool_call_id").and_then(Value::as_str))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();

    let mut target: Option<String> = None;
    if let Some(hint) = task_hint {
        if run_subagent_ids.iter().any(|id| id.as_str() == hint) {
            target = Some(hint.to_string());
        }
    }
    if target.is_none() {
        let existing = step["observation"]["results"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        target = run_subagent_ids
            .iter()
            .find(|id| {
                !existing.iter().any(|result| {
                    result.get("source_call_id").and_then(Value::as_str) == Some(id.as_str())
                        && result.get("subagent_trajectory_ref").is_some()
                })
            })
            .cloned();
    }

    let results = step["observation"]["results"]
        .as_array_mut()
        .expect("observation.results is an array");
    if let Some(call_id) = target {
        if let Some(slot) = results.iter_mut().find(|result| {
            result.get("source_call_id").and_then(Value::as_str) == Some(call_id.as_str())
        }) {
            push_subagent_ref(slot, reference);
            return;
        }
        results.push(json!({
            "source_call_id": call_id,
            "subagent_trajectory_ref": [reference],
        }));
        return;
    }
    results.push(json!({ "subagent_trajectory_ref": [reference] }));
}

/// A sub-agent trajectory whose own steps are built; nested children are
/// attached once the whole anchor tree is known.
struct PreparedSubAgent {
    steps: Vec<Value>,
    step_index: HashMap<String, usize>,
    trajectory_id: String,
    session_id: String,
    agent_name: String,
    agent_id: Option<String>,
    agent_instance_id: Option<String>,
    trace_id: Option<String>,
    task_id: Option<String>,
    spawn_depth: Option<u32>,
    anchor_message_id: Option<String>,
}

/// Finish one embedded sub-agent trajectory, recursing into nested delegations.
fn finish_sub_agent(
    idx: usize,
    prepared: &mut [Option<PreparedSubAgent>],
    children: &[Vec<usize>],
    model: &str,
) -> Value {
    let mut node = prepared[idx]
        .take()
        .expect("sub-agent group is finished exactly once");
    let total_steps = node.steps.len();

    let mut embedded: Vec<Value> = Vec::new();
    for &child in &children[idx] {
        let (child_anchor, child_task, child_ref) = {
            let Some(child_node) = prepared[child].as_ref() else {
                continue;
            };
            (
                child_node.anchor_message_id.clone(),
                child_node.task_id.clone(),
                json!({
                    "trajectory_id": child_node.trajectory_id,
                    "session_id": child_node.session_id,
                    "extra": { "agent_name": child_node.agent_name },
                }),
            )
        };
        let child_doc = finish_sub_agent(child, prepared, children, model);
        if let Some(anchor) = child_anchor {
            if let Some(&step_pos) = node.step_index.get(&anchor) {
                attach_subagent_ref(&mut node.steps[step_pos], child_task.as_deref(), child_ref);
            }
        }
        embedded.push(child_doc);
    }

    let mut extra = serde_json::Map::new();
    if let Some(value) = node.agent_id {
        extra.insert("agent_id".into(), json!(value));
    }
    if let Some(value) = node.agent_instance_id {
        extra.insert("agent_instance_id".into(), json!(value));
    }
    if let Some(value) = node.trace_id {
        extra.insert("trace_id".into(), json!(value));
    }
    if let Some(value) = node.task_id {
        extra.insert("task_id".into(), json!(value));
    }
    if let Some(value) = node.spawn_depth {
        extra.insert("spawn_depth".into(), json!(value));
    }
    if let Some(value) = node.anchor_message_id {
        extra.insert("anchor_message_id".into(), json!(value));
    }

    let mut doc = json!({
        "schema_version": "ATIF-v1.7",
        "session_id": node.session_id,
        "trajectory_id": node.trajectory_id,
        "agent": {
            "name": node.agent_name,
            "version": env!("CARGO_PKG_VERSION"),
            "model_name": model,
        },
        "steps": node.steps,
        "final_metrics": { "total_steps": total_steps },
    });
    if !embedded.is_empty() {
        doc["subagent_trajectories"] = json!(embedded);
        doc["notes"] =
            json!("Generated by pointer-cli; nested delegations are embedded in `subagent_trajectories`.");
    }
    if !extra.is_empty() {
        doc["extra"] = Value::Object(extra);
    }
    doc
}

/// Build an ATIF v1.7 trajectory from the persisted conversation.
///
/// Mapping:
/// - step 1 → `source: user`, the CLI instruction;
/// - each top-level assistant message → `source: agent` with its `tool_calls`;
/// - `role: tool` rows are attached as `observation.results` on the assistant
///   step that issued the matching `tool_call_id` (ATIF requires the
///   `source_call_id` to reference a tool call on the same step);
/// - scoped sub-agent rows (`anchor_message_id` set) are **not** lead steps:
///   they are embedded as complete ATIF sub-trajectories under
///   `subagent_trajectories` (one per `agent_instance_id`) and referenced from
///   the step that spawned them via
///   `observation.results[].subagent_trajectory_ref`. Nested delegations are
///   embedded under the sub-trajectory they belong to.
pub fn build_trajectory(ctx: &TrajectoryContext<'_>) -> Value {
    // Partition persisted rows: lead timeline vs scoped sub-agent transcripts.
    let mut lead_messages: Vec<&ChatMessage> = Vec::new();
    let mut groups: Vec<SubAgentGroup<'_>> = Vec::new();
    let mut group_index: HashMap<String, usize> = HashMap::new();
    for msg in ctx.messages {
        if msg.anchor_message_id.is_none() {
            lead_messages.push(msg);
            continue;
        }
        let key = sub_agent_group_key(msg);
        let idx = *group_index.entry(key).or_insert_with(|| {
            groups.push(SubAgentGroup {
                anchor_message_id: None,
                agent_name: None,
                agent_id: None,
                agent_instance_id: None,
                trace_id: None,
                task_id: None,
                spawn_depth: None,
                rows: Vec::new(),
            });
            groups.len() - 1
        });
        let group = &mut groups[idx];
        group.anchor_message_id = group
            .anchor_message_id
            .take()
            .or_else(|| msg.anchor_message_id.clone());
        group.agent_name = group.agent_name.take().or_else(|| msg.agent_name.clone());
        group.agent_id = group.agent_id.take().or_else(|| msg.agent_id.clone());
        group.agent_instance_id = group
            .agent_instance_id
            .take()
            .or_else(|| msg.agent_instance_id.clone());
        group.trace_id = group.trace_id.take().or_else(|| msg.trace_id.clone());
        group.task_id = group.task_id.take().or_else(|| msg.task_id.clone());
        group.spawn_depth = group.spawn_depth.or(msg.spawn_depth);
        group.rows.push(msg);
    }

    // ---- lead timeline ----
    let lead_tool_results = collect_tool_results(&lead_messages);
    let mut steps: Vec<Value> = Vec::new();
    steps.push(json!({
        "step_id": 1,
        "source": "user",
        "message": ctx.instruction,
    }));
    let mut lead_step_index: HashMap<&str, usize> = HashMap::new();
    for msg in &lead_messages {
        if !matches!(msg.role, Role::Assistant) {
            continue;
        }
        let step = agent_step(steps.len() + 1, msg, ctx.model, &lead_tool_results);
        lead_step_index.insert(msg.id.as_str(), steps.len());
        steps.push(step);
    }
    let lead_step_count = steps.len();

    // ---- sub-agent transcripts ----
    // The anchor decides nesting: a row anchored to a lead message is a root
    // sub-agent, one anchored to another sub-agent's row is nested under it.
    let mut message_owner: HashMap<&str, usize> = HashMap::new();
    for (idx, group) in groups.iter().enumerate() {
        for row in &group.rows {
            message_owner.insert(row.id.as_str(), idx);
        }
    }
    let mut parent: Vec<Option<usize>> = vec![None; groups.len()];
    for (idx, group) in groups.iter().enumerate() {
        if let Some(owner) = group
            .anchor_message_id
            .as_deref()
            .and_then(|anchor| message_owner.get(anchor).copied())
        {
            if owner != idx {
                parent[idx] = Some(owner);
            }
        }
    }
    // Defensive: break any anchor cycle so the tree stays finite.
    let anchors = parent.clone();
    for (idx, start) in anchors.iter().enumerate() {
        let mut seen = HashSet::new();
        let mut cursor = *start;
        while let Some(node) = cursor {
            if node == idx || !seen.insert(node) {
                parent[idx] = None;
                break;
            }
            cursor = anchors[node];
        }
    }
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); groups.len()];
    let mut roots: Vec<usize> = Vec::new();
    for (idx, parent_idx) in parent.iter().enumerate() {
        match parent_idx {
            Some(parent_idx) => children[*parent_idx].push(idx),
            None => roots.push(idx),
        }
    }

    let mut prepared: Vec<Option<PreparedSubAgent>> = Vec::with_capacity(groups.len());
    let mut used_trajectory_ids: HashSet<String> = HashSet::new();
    for (idx, group) in groups.iter().enumerate() {
        let group_tool_results = collect_tool_results(&group.rows);
        let mut group_steps: Vec<Value> = Vec::new();
        let mut group_step_index: HashMap<String, usize> = HashMap::new();
        for row in &group.rows {
            match row.role {
                Role::Assistant => {
                    let step =
                        agent_step(group_steps.len() + 1, row, ctx.model, &group_tool_results);
                    group_step_index.insert(row.id.clone(), group_steps.len());
                    group_steps.push(step);
                }
                Role::User => group_steps.push(json!({
                    "step_id": group_steps.len() + 1,
                    "source": "user",
                    "message": row.content,
                })),
                Role::System => group_steps.push(json!({
                    "step_id": group_steps.len() + 1,
                    "source": "system",
                    "message": row.content,
                })),
                Role::Tool => {}
            }
        }
        if group_steps.is_empty() {
            // A transcript that only carries tool rows still needs one step to
            // satisfy ATIF's `steps` min_length=1.
            group_steps.push(json!({
                "step_id": 1,
                "source": "agent",
                "message": "",
                "model_name": ctx.model,
            }));
        }

        let identity = group
            .agent_instance_id
            .clone()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| group.trace_id.clone().filter(|s| !s.trim().is_empty()))
            .or_else(|| group.task_id.clone().filter(|s| !s.trim().is_empty()))
            .unwrap_or_else(|| format!("subagent-{idx}"));
        let mut trajectory_id = format!("{}::{}", ctx.run_id, identity);
        let mut suffix = 1;
        while !used_trajectory_ids.insert(trajectory_id.clone()) {
            trajectory_id = format!("{}::{}#{suffix}", ctx.run_id, identity);
            suffix += 1;
        }

        prepared.push(Some(PreparedSubAgent {
            steps: group_steps,
            step_index: group_step_index,
            trajectory_id,
            session_id: ctx.conversation_id.to_string(),
            agent_name: group
                .agent_name
                .clone()
                .or_else(|| group.agent_id.clone())
                .unwrap_or_else(|| "pointer-cli-subagent".to_string()),
            agent_id: group.agent_id.clone(),
            agent_instance_id: group.agent_instance_id.clone(),
            trace_id: group.trace_id.clone(),
            task_id: group.task_id.clone(),
            spawn_depth: group.spawn_depth,
            anchor_message_id: group.anchor_message_id.clone(),
        }));
    }

    let mut embedded: Vec<Value> = Vec::new();
    for &root in &roots {
        let (anchor, task_hint, reference) = {
            let Some(node) = prepared[root].as_ref() else {
                continue;
            };
            (
                node.anchor_message_id.clone(),
                node.task_id.clone(),
                json!({
                    "trajectory_id": node.trajectory_id,
                    "session_id": node.session_id,
                    "extra": { "agent_name": node.agent_name },
                }),
            )
        };
        let doc = finish_sub_agent(root, &mut prepared, &children, ctx.model);
        if let Some(anchor) = anchor {
            if let Some(&step_pos) = lead_step_index.get(anchor.as_str()) {
                attach_subagent_ref(&mut steps[step_pos], task_hint.as_deref(), reference);
            }
        }
        embedded.push(doc);
    }

    let notes = if embedded.is_empty() {
        "Generated by pointer-cli; lead timeline only (no sub-agent rows in this run).".to_string()
    } else {
        format!(
            "Generated by pointer-cli; {} sub-agent trajectory(ies) embedded in `subagent_trajectories` (lead `steps` = {}).",
            embedded.len(),
            lead_step_count
        )
    };

    let mut trajectory = json!({
        "schema_version": "ATIF-v1.7",
        "session_id": ctx.conversation_id,
        "trajectory_id": ctx.run_id,
        "agent": {
            "name": "pointer-cli",
            "version": env!("CARGO_PKG_VERSION"),
            "model_name": ctx.model,
        },
        "steps": steps,
        "notes": notes,
        "final_metrics": {
            "total_prompt_tokens": ctx.usage.prompt_tokens,
            "total_completion_tokens": ctx.usage.completion_tokens,
            "total_cached_tokens": ctx.usage.cached_tokens,
            "total_steps": lead_step_count,
        },
        "extra": {
            "status": ctx.status,
            "exit_code": ctx.exit_code,
            "trigger_source": "http_runs",
            "llm_rounds": ctx.usage.llm_rounds,
        },
    });
    if !embedded.is_empty() {
        trajectory["subagent_trajectories"] = json!(embedded);
    }
    trajectory
}

/// Write `<out_dir>/trajectory.json`.
pub fn write_trajectory(out_dir: &Path, trajectory: &Value) -> Result<PathBuf> {
    let path = out_dir.join("trajectory.json");
    std::fs::write(&path, serde_json::to_vec_pretty(trajectory)?)
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pointer_core::models::ToolCall;

    fn assistant_with_tool_call() -> ChatMessage {
        let mut msg = ChatMessage::user_text("");
        msg.role = Role::Assistant;
        msg.id = "msg_1".into();
        msg.content = String::new();
        msg.created_at = 1_700_000_000_000;
        msg.tool_calls = Some(vec![ToolCall {
            id: "call_1".into(),
            name: "terminal".into(),
            arguments: "{\"command\":\"echo hi\"}".into(),
            status: "success".into(),
            result: Some("{\"stdout\":\"hi\\n\"}".into()),
            error: None,
            duration_ms: Some(5),
            risk_level: None,
            display_label: None,
            display_summary: None,
        }]);
        msg
    }

    fn tool_row() -> ChatMessage {
        let mut msg = ChatMessage::user_text("");
        msg.role = Role::Tool;
        msg.tool_call_id = Some("call_1".into());
        msg.tool_name = Some("terminal".into());
        msg.content = "{\"stdout\":\"hi\\n\"}".into();
        msg
    }

    #[test]
    fn parse_arguments_wraps_non_objects() {
        assert_eq!(parse_arguments("{\"a\":1}")["a"], json!(1));
        assert_eq!(parse_arguments("[1,2]")["_value"], json!([1, 2]));
        assert_eq!(parse_arguments("not json")["_raw"], json!("not json"));
    }

    #[test]
    fn trajectory_maps_instruction_tool_call_and_reply() {
        let mut reply = ChatMessage::user_text("");
        reply.role = Role::Assistant;
        reply.content = "done".into();
        reply.created_at = 1_700_000_001_000;

        let messages = vec![
            ChatMessage::user_text("do the thing"),
            assistant_with_tool_call(),
            tool_row(),
            reply,
        ];
        let usage = UsageSummary {
            prompt_tokens: 11,
            completion_tokens: 7,
            total_tokens: 18,
            llm_rounds: 2,
            ..Default::default()
        };
        let ctx = TrajectoryContext {
            instruction: "do the thing",
            conversation_id: "cli-1",
            run_id: "run-1",
            model: "mock-model",
            status: "finished",
            exit_code: 0,
            messages: &messages,
            usage: &usage,
        };
        let traj = build_trajectory(&ctx);

        assert_eq!(traj["schema_version"], json!("ATIF-v1.7"));
        assert_eq!(traj["steps"].as_array().unwrap().len(), 3);
        assert_eq!(traj["steps"][0]["source"], json!("user"));
        assert_eq!(traj["steps"][0]["step_id"], json!(1));
        assert_eq!(traj["steps"][1]["source"], json!("agent"));
        assert_eq!(traj["steps"][1]["step_id"], json!(2));
        assert_eq!(
            traj["steps"][1]["tool_calls"][0]["tool_call_id"],
            json!("call_1")
        );
        assert_eq!(
            traj["steps"][1]["observation"]["results"][0]["source_call_id"],
            json!("call_1")
        );
        // `role: tool` rows are folded into the observation, not extra steps.
        assert_eq!(traj["steps"][2]["message"], json!("done"));
        assert_eq!(traj["final_metrics"]["total_steps"], json!(3));
        assert_eq!(traj["final_metrics"]["total_prompt_tokens"], json!(11));
    }

    /// Lead assistant turn that issues a `run_subagent` call.
    fn lead_with_run_subagent(message_id: &str, call_id: &str) -> ChatMessage {
        let mut msg = ChatMessage::user_text("");
        msg.role = Role::Assistant;
        msg.id = message_id.into();
        msg.content = String::new();
        msg.created_at = 1_700_000_000_000;
        msg.tool_calls = Some(vec![ToolCall {
            id: call_id.into(),
            name: "run_subagent".into(),
            arguments: "{\"agentId\":\"explore\"}".into(),
            status: "success".into(),
            result: Some("{\"status\":\"running\"}".into()),
            error: None,
            duration_ms: Some(1),
            risk_level: None,
            display_label: None,
            display_summary: None,
        }]);
        msg
    }

    /// One persisted scoped sub-agent row (the DB stamps these linkage fields).
    fn scoped_row(
        id: &str,
        role: Role,
        anchor: &str,
        instance: &str,
        content: &str,
    ) -> ChatMessage {
        let mut msg = ChatMessage::user_text(content);
        msg.id = id.into();
        msg.role = role;
        msg.anchor_message_id = Some(anchor.into());
        msg.agent_instance_id = Some(instance.into());
        msg.trace_id = Some(format!("task:{instance}"));
        msg.task_id = Some(format!("task:{instance}"));
        msg.spawn_depth = Some(1);
        msg
    }

    fn tool_row_for(call_id: &str, tool_name: &str, content: &str) -> ChatMessage {
        let mut msg = ChatMessage::user_text("");
        msg.role = Role::Tool;
        msg.tool_call_id = Some(call_id.into());
        msg.tool_name = Some(tool_name.into());
        msg.content = content.into();
        msg
    }

    fn trajectory_context<'a>(
        instruction: &'a str,
        messages: &'a [ChatMessage],
        usage: &'a UsageSummary,
    ) -> TrajectoryContext<'a> {
        TrajectoryContext {
            instruction,
            conversation_id: "cli-1",
            run_id: "run-1",
            model: "mock-model",
            status: "finished",
            exit_code: 0,
            messages,
            usage,
        }
    }

    #[test]
    fn trajectory_keeps_scoped_sub_agent_rows_out_of_lead_steps() {
        let mut scoped = assistant_with_tool_call();
        scoped.id = "scoped".into();
        scoped.anchor_message_id = Some("msg_1".into());
        scoped.agent_instance_id = Some("inst-1".into());
        scoped.tool_calls = None;
        let messages = vec![ChatMessage::user_text("x"), scoped];
        let usage = UsageSummary::default();
        let ctx = trajectory_context("x", &messages, &usage);
        let traj = build_trajectory(&ctx);
        assert_eq!(traj["steps"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn trajectory_embeds_scoped_sub_agent_rows() {
        let mut final_reply = ChatMessage::user_text("");
        final_reply.role = Role::Assistant;
        final_reply.content = "done".into();
        final_reply.created_at = 1_700_000_002_000;

        let mut sub_assistant =
            scoped_row("sub_assistant", Role::Assistant, "msg_1", "inst-1", "Found it");
        sub_assistant.agent_name = Some("Explore".into());
        sub_assistant.agent_id = Some("explore".into());
        sub_assistant.task_id = Some("call_sub_1".into());

        let messages = vec![
            ChatMessage::user_text("delegate"),
            lead_with_run_subagent("msg_1", "call_sub_1"),
            tool_row_for("call_sub_1", "run_subagent", "{\"agentInstanceId\":\"inst-1\"}"),
            scoped_row("sub_user", Role::User, "msg_1", "inst-1", "Explore the repo"),
            sub_assistant,
            final_reply,
        ];
        let usage = UsageSummary::default();
        let ctx = trajectory_context("delegate", &messages, &usage);
        let traj = build_trajectory(&ctx);

        // Lead timeline is unchanged: scoped rows are not lead steps.
        assert_eq!(traj["steps"].as_array().unwrap().len(), 3);
        assert_eq!(traj["steps"][1]["message"], json!(""));

        let subs = traj["subagent_trajectories"]
            .as_array()
            .expect("subagent_trajectories present");
        assert_eq!(subs.len(), 1);
        let sub = &subs[0];
        assert_eq!(sub["schema_version"], json!("ATIF-v1.7"));
        assert_eq!(sub["trajectory_id"], json!("run-1::inst-1"));
        assert_eq!(sub["session_id"], json!("cli-1"));
        assert_eq!(sub["agent"]["name"], json!("Explore"));
        assert_eq!(sub["agent"]["model_name"], json!("mock-model"));
        assert_eq!(sub["steps"].as_array().unwrap().len(), 2);
        assert_eq!(sub["steps"][0]["step_id"], json!(1));
        assert_eq!(sub["steps"][0]["source"], json!("user"));
        assert_eq!(sub["steps"][0]["message"], json!("Explore the repo"));
        assert_eq!(sub["steps"][1]["step_id"], json!(2));
        assert_eq!(sub["steps"][1]["source"], json!("agent"));
        assert_eq!(sub["steps"][1]["message"], json!("Found it"));
        assert_eq!(sub["final_metrics"]["total_steps"], json!(2));

        // The spawning lead step references the embedded sub-trajectory.
        let refs = traj["steps"][1]["observation"]["results"][0]["subagent_trajectory_ref"]
            .as_array()
            .expect("subagent_trajectory_ref present");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0]["trajectory_id"], json!("run-1::inst-1"));
        assert_eq!(refs[0]["session_id"], json!("cli-1"));
        assert!(
            traj["notes"].as_str().unwrap().contains("subagent_trajectories"),
            "notes should document the embedded sub-agents: {}",
            traj["notes"]
        );
    }

    #[test]
    fn trajectory_nests_grandchild_sub_agent_trajectory() {
        let mut child_assistant =
            scoped_row("sub_assistant", Role::Assistant, "msg_1", "inst-1", "spawn grandchild");
        child_assistant.tool_calls = Some(vec![ToolCall {
            id: "call_sub_2".into(),
            name: "run_subagent".into(),
            arguments: "{\"agentId\":\"self\"}".into(),
            status: "success".into(),
            result: Some("{}".into()),
            error: None,
            duration_ms: Some(1),
            risk_level: None,
            display_label: None,
            display_summary: None,
        }]);
        child_assistant.task_id = Some("call_sub_1".into());

        let mut grandchild = scoped_row("grandchild", Role::Assistant, "sub_assistant", "inst-2", "nested");
        grandchild.task_id = Some("call_sub_2".into());
        grandchild.spawn_depth = Some(2);

        let messages = vec![
            ChatMessage::user_text("delegate"),
            lead_with_run_subagent("msg_1", "call_sub_1"),
            tool_row_for("call_sub_1", "run_subagent", "{}"),
            scoped_row("sub_user", Role::User, "msg_1", "inst-1", "child task"),
            child_assistant,
            grandchild,
        ];
        let usage = UsageSummary::default();
        let ctx = trajectory_context("delegate", &messages, &usage);
        let traj = build_trajectory(&ctx);

        let subs = traj["subagent_trajectories"].as_array().unwrap();
        assert_eq!(subs.len(), 1, "grandchild must nest under its parent");
        let nested = subs[0]["subagent_trajectories"]
            .as_array()
            .expect("nested subagent_trajectories present");
        assert_eq!(nested.len(), 1);
        assert_eq!(nested[0]["trajectory_id"], json!("run-1::inst-2"));
        assert_eq!(nested[0]["steps"][0]["message"], json!("nested"));
        // The child's own step carries the ref to the grandchild.
        let refs = subs[0]["steps"][1]["observation"]["results"][0]["subagent_trajectory_ref"]
            .as_array()
            .expect("nested ref present");
        assert_eq!(refs[0]["trajectory_id"], json!("run-1::inst-2"));
    }

    #[test]
    fn trajectory_omits_subagent_field_without_sub_agents() {
        let messages = vec![
            ChatMessage::user_text("do the thing"),
            assistant_with_tool_call(),
            tool_row(),
        ];
        let usage = UsageSummary::default();
        let ctx = trajectory_context("do the thing", &messages, &usage);
        let traj = build_trajectory(&ctx);
        assert!(traj.get("subagent_trajectories").is_none());
        assert!(traj["notes"].as_str().unwrap().contains("no sub-agent rows"));
    }
}
