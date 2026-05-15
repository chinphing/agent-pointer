use crate::agents::{
    expand_agent_prompt_placeholders, register_builtin_agents,
    rendered_communication_public_inject, rendered_json_wire_format_tail_inject, AgentDef,
    AgentOrchestrator, AgentProfile,
    AgentRegistry, AgentRunLimits, AgentRunResult, AgentTask, SessionInjectVars, DEFAULT_AGENT_ID,
    SUPERVISOR_AGENT_ID, AGENT_MODE_SUPERVISOR,
};
use crate::extensions::{
    BeforeMainLlmCallContext, ExtensionRegistry, MessageLoopPromptsAfterContext,
};
use crate::llm_token_stats::{ChatLlmTokenSession, ConversationLlmStats};
use crate::models::{
    effective_reasoning_in_messages, effective_max_tokens, AgentTrace, ChatMessage, Role, StreamEvent, ToolCall,
};
use crate::provider::{OpenAIProvider, ProviderEvent};
use crate::skills::SkillRegistry;
use crate::storage;
use crate::tools::merge_tool_method_from_qualified_name;
use crate::tools::parse_tool_call_arguments;
use crate::tools::response::response_text_from_args;
use crate::tools::validate_envelope_tool_batch;
use crate::tools::terminal::{run_terminal_command_streaming, terminal_stream_tool_status};
use crate::tools::ToolRegistry;
use anyhow::{anyhow, Result};
use serde_json::Value;
use std::backtrace::Backtrace;
use parking_lot::Mutex;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

/// Per-pool cap: `max` tool cycles; pool is either single-agent or Supervisor (sub-agents) for the conversation.
#[derive(Debug)]
struct SessionToolBudget {
    max: u32,
    used_before_request: u32,
    consumed_this_request: u32,
}

impl SessionToolBudget {
    fn new(max: u32, used_before_request: u32) -> Self {
        Self {
            max,
            used_before_request,
            consumed_this_request: 0,
        }
    }

    fn cap(&self) -> u32 {
        self.max
    }

    fn record_tool_cycle(&mut self) {
        self.consumed_this_request = self.consumed_this_request.saturating_add(1);
    }

    fn remaining(&self) -> u32 {
        self.max
            .saturating_sub(self.used_before_request)
            .saturating_sub(self.consumed_this_request)
    }

    fn is_exhausted(&self) -> bool {
        self.used_before_request
            .saturating_add(self.consumed_this_request)
            >= self.max
    }

    fn sync_out(&self, out: &mut u32) {
        *out = self.consumed_this_request;
    }
}

pub struct AppState {
    pub tools: Arc<ToolRegistry>,
    pub skills: Arc<SkillRegistry>,
    pub agents: Arc<AgentRegistry>,
    pub computer_state: Arc<crate::agents::computer::ComputerState>,
    pub task_board_store: Arc<crate::tools::task_board::TaskBoardStore>,
    /// Lifecycle hooks aligned with Python `call_extensions(extension_point, …)`.
    pub extensions: Arc<ExtensionRegistry>,
    pub cancels: Mutex<HashMap<String, CancellationToken>>,
    /// When set, the in-flight `terminal` tool for that conversation kills its subprocess (host-only; does not cancel the LLM turn).
    pub terminal_run_abort: Mutex<HashMap<String, Arc<AtomicBool>>>,
    pub approvals: Mutex<HashMap<String, oneshot::Sender<bool>>>,
}

impl AppState {
    pub fn new() -> Self {
        let tools = Arc::new(ToolRegistry::new());
        let task_board_store = Arc::new(crate::tools::task_board::TaskBoardStore::default());
        crate::tools::builtin::register_all(&tools, task_board_store.clone());
        let skills = Arc::new(SkillRegistry::new());
        crate::skills::builtin::register_all(&skills);
        crate::tools::builtin::register_skill_tools(&tools, skills.clone());
        if let Err(err) = skills.reload_external() {
            log::warn!("load external skills failed: {err}");
        }
        let agents = Arc::new(AgentRegistry::new());
        register_builtin_agents(&agents);
        if let Err(err) = agents.reload_external() {
            log::warn!("load external agents failed: {err}");
        }
        let computer_state = Arc::new(crate::agents::computer::ComputerState::new(&agents));
        crate::tools::builtin::register_computer_tools(&tools, computer_state.clone());
        let mut extension_registry = ExtensionRegistry::new();
        crate::extensions::register_builtin_extensions(&mut extension_registry);
        extension_registry.register_before_main_llm_call(Arc::new(
            crate::extensions::task_board_hook::TaskBoardSnapshotHook,
        ));
        Self {
            tools,
            skills,
            agents,
            computer_state,
            task_board_store,
            extensions: Arc::new(extension_registry),
            cancels: Mutex::new(HashMap::new()),
            terminal_run_abort: Mutex::new(HashMap::new()),
            approvals: Mutex::new(HashMap::new()),
        }
    }

    pub fn cancel(&self, conversation_id: &str) {
        if let Some(token) = self.cancels.lock().get(conversation_id) {
            token.cancel();
        }
        let approvals: Vec<_> = self.approvals.lock().drain().collect();
        for (_, tx) in approvals {
            let _ = tx.send(false);
        }
    }

    /// Kill only the subprocess for the current **`terminal`** tool in this conversation.
    /// Does **not** cancel the LLM stream or the rest of the turn. Returns **true** if a run was registered.
    pub fn abort_terminal_command(&self, conversation_id: &str) -> bool {
        self.terminal_run_abort
            .lock()
            .get(conversation_id)
            .map(|f| {
                f.store(true, Ordering::SeqCst);
                true
            })
            .unwrap_or(false)
    }

    pub fn approve_tool_call(&self, tool_call_id: &str, approved: bool) -> bool {
        if let Some(tx) = self.approvals.lock().remove(tool_call_id) {
            let _ = tx.send(approved);
            true
        } else {
            false
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

pub type StreamTx = crate::models::ChatStreamSender;

fn emit(tx: &StreamTx, ev: StreamEvent) {
    if tx.send(ev).is_err() {
        log::warn!("stream event not delivered (frontend channel closed)");
    }
}

/// Appends `[Environment]` + **calendar date only** (see [`crate::env_prompt::build_environment_system_prompt_slice`])
/// as the **last** `system_prompts` slice (after `before_main_llm_call` hooks such as `[TASK_BOARD]`).
fn push_env_context_last_in_system_prompts(system_prompts: &mut Vec<String>) {
    system_prompts.push(format!(
        "[Environment]\n{}",
        crate::env_prompt::build_environment_system_prompt_slice()
    ));
}

/// After `[Environment]`, re-state the JSON-only wire contract (recency) when tools are enabled.
fn push_json_wire_format_tail(system_prompts: &mut Vec<String>, tools_appendix_enabled: bool) {
    if !tools_appendix_enabled {
        return;
    }
    if let Some(block) = rendered_json_wire_format_tail_inject() {
        system_prompts.push(block);
    }
}

/// Drop a failed non-JSON assistant turn from API history so the model is not trained on plain prose.
fn rollback_failed_json_assistant_turn(history: &mut Vec<ChatMessage>, assistant_id: &str) {
    if history.last().is_some_and(|m| {
        matches!(m.role, Role::Assistant) && m.id == assistant_id
    }) {
        history.pop();
        log::info!(
            "rolled back non-JSON assistant turn from API history (assistant_id={assistant_id})"
        );
    }
}

pub async fn run_chat(
    stream: StreamTx,
    state: Arc<AppState>,
    conversation_id: String,
    mut history: Vec<ChatMessage>,
    enabled_skill_ids: Vec<String>,
    agent_mode: Option<String>,
    tool_rounds_used_single_start: u32,
    tool_rounds_used_supervisor_start: u32,
) -> Result<()> {
    log::info!(
        "run_chat start conversation_id={} incoming_history_messages={} enabled_skill_ids={} request_agent_mode={:?} tool_rounds_used_single_start={} tool_rounds_used_supervisor_start={}",
        conversation_id,
        history.len(),
        enabled_skill_ids.len(),
        agent_mode,
        tool_rounds_used_single_start,
        tool_rounds_used_supervisor_start,
    );

    let cancel = CancellationToken::new();
    state
        .cancels
        .lock()
        .insert(conversation_id.clone(), cancel.clone());

    let mut consumed_single = 0u32;
    let mut consumed_supervisor = 0u32;
    let result = run_chat_inner(
        stream.clone(),
        state.clone(),
        &conversation_id,
        &mut history,
        &enabled_skill_ids,
        agent_mode.as_deref(),
        tool_rounds_used_single_start,
        tool_rounds_used_supervisor_start,
        &mut consumed_single,
        &mut consumed_supervisor,
        cancel.clone(),
    )
    .await;

    state.cancels.lock().remove(&conversation_id);

    if let Err(err) = &result {
        // Root cause is in `err` (often an HTTP/API message). `Backtrace::capture()` here only
        // shows the async poll point (e.g. chat_service + tokio), not the failing await site.
        log::error!("run_chat failed conversation_id={} error={:#}", conversation_id, err);
        log::debug!(
            "run_chat failure poll-point backtrace (for deep debugging):\n{}",
            Backtrace::capture()
        );
        emit(
            &stream,
            StreamEvent::Error {
                message_id: None,
                message: err.to_string(),
            },
        );
        log::info!(
            "run_chat emitted StreamEvent::Error (session-level) conversation_id={} message_len_chars={}",
            conversation_id,
            err.to_string().chars().count(),
        );
    }
    let max_tr = storage::load_settings()
        .map(|s| s.max_tool_rounds)
        .unwrap_or(100);
    let single_total = tool_rounds_used_single_start.saturating_add(consumed_single);
    let supervisor_total =
        tool_rounds_used_supervisor_start.saturating_add(consumed_supervisor);
    log::info!(
        "run_chat conversation end: emitting StreamEvent::Done conversation_id={} run_outcome={} history_messages_final={} consumed_this_run_single={} consumed_this_run_supervisor={} cumulative_tool_rounds_single={} cumulative_tool_rounds_supervisor={} max_tool_rounds_attached={}",
        conversation_id,
        if result.is_ok() { "Ok" } else { "Err" },
        history.len(),
        consumed_single,
        consumed_supervisor,
        single_total,
        supervisor_total,
        max_tr,
    );
    let done_conversation_id = conversation_id.clone();
    if stream
        .send(StreamEvent::Done {
            conversation_id,
            tool_rounds_used_total: Some(single_total),
            tool_rounds_used_supervisor_total: Some(supervisor_total),
            max_tool_rounds: Some(max_tr),
        })
        .is_err()
    {
        log::warn!(
            "run_chat: StreamEvent::Done not delivered (stream receiver dropped) conversation_id={}",
            done_conversation_id
        );
    }
    log::info!(
        "run_chat finished after Done emit final_result_is_ok={}",
        result.is_ok(),
    );
    result
}

/// When tools appendix is enabled but this turn produced no executable tool call, inject a user-line
/// for the next model turn. Public format rules are already in the system prompts each round via
/// [`rendered_communication_public_inject`] / [`expand_agent_prompt_placeholders`]; this message only states the failure and JSON escaping hints.
fn json_tool_empty_calls_retry_message(
    diag: &crate::json_tool_caller::JsonToolFinishDiagnostics,
    tools_appendix_enabled: bool,
    finish_reason: &str,
    max_tokens: u32,
) -> Option<String> {
    if !tools_appendix_enabled {
        return None;
    }
    const ESCAPE_NOTE: &str = "在 JSON 的 `tool_args` 字符串字段中正确转义引号与换行；长文本（如 `file:write` 的 `content`、`file:edit` 的 `oldString`/`newString`）必须作为合法 JSON 字符串。勿在模型输出外再包一层 Markdown 代码围栏，也勿在 JSON 对象前后加说明文字。";

    let intro = if is_output_length_limited_finish_reason(finish_reason) {
        format!(
            "【环境反馈】本回合输出因达到 **max_tokens** 上限（finish_reason={finish_reason}）被截断，JSON 工具信封不完整，未能执行工具。\n\n\
             请缩小本回合输出并重新发送**一个**完整 JSON 对象（拆分大段编辑、分多轮写入）。"
        )
    } else if diag.attempted_tool_json {
        if diag.fragment_complete {
            let detail = diag
                .parse_error
                .as_deref()
                .filter(|s| !s.is_empty())
                .unwrap_or("无法解析为合法的工具 JSON 信封（根对象需含 `tool_name` 与 `tool_args` 等字段）");
            format!(
                "【环境反馈】本回合输出中包含工具相关 JSON 字段，但解析失败：{detail}。\n\n请按系统提示中的**公共输出约定**重新输出**唯一**一个 JSON 对象（无围栏、无 JSON 外的说明文字）。"
            )
        } else {
            "【环境反馈】本回合检测到工具相关 JSON 片段（如 `\"tool_name\"` / `\"tool_args\"`），但在流结束前仍未形成可解析的完整 JSON 对象，因此未能执行任何工具。\n\n请按系统提示中的**公共输出约定**重新输出**唯一**一个 JSON 对象，并确保花括号与引号闭合完整。".to_string()
        }
    } else {
        "【环境反馈】本回合未输出 JSON 工具信封，而是普通对话文字（本应用不接受纯文本 assistant 回复）。\n\n\
         请**只**输出**一个** JSON 对象，不要用 Markdown 围栏，不要在 JSON 外写任何说明。最小示例：\n\
         {\"thoughts\":\"简要推理\",\"headline\":\"短标题\",\"tool_name\":\"response\",\"tool_args\":{\"text\":\"给用户看的完整回复\"}}\n\n\
         若要调用工具，把 tool_name / tool_args 换成对应工具（如 file:read、terminal）。"
            .to_string()
    };

    let mut body = format!("{intro}\n\n【JSON】{ESCAPE_NOTE}");
    if is_output_length_limited_finish_reason(finish_reason) {
        body.push_str(&output_length_retry_supplement(max_tokens, finish_reason));
    }
    if let Some(head) = diag
        .consumed_fragment_head
        .as_deref()
        .filter(|s| !s.is_empty())
    {
        body.push_str("\n\n【你上一回合输出的开头片段（供对照修正）】\n");
        body.push_str(head);
    }
    Some(body)
}

fn json_tool_envelope_batch_retry_message(err: &str) -> String {
    format!(
        "【环境反馈】本回合工具调用组合不符合协议：{err}\n\n\
         当使用 `sidecar_tools` 数组时：仅允许将白名单侧车工具（例如 `task_board:patch`）放在其中每一项；根级必须恰好保留一对主工具 `tool_name`/`tool_args`，且不得仅为侧车工具。\n\
         若无 `sidecar_tools`，则仍只使用根级单工具。请按系统提示中的 JSON 约定重新输出完整的 JSON 对象。"
    )
}

fn is_output_length_limited_finish_reason(finish_reason: &str) -> bool {
    matches!(
        finish_reason.trim().to_ascii_lowercase().as_str(),
        "length" | "max_tokens"
    )
}

fn output_length_retry_supplement(max_tokens: u32, finish_reason: &str) -> String {
    format!(
        "\n\n【输出长度】本回合因 **输出 token 上限** 被截断（finish_reason={finish_reason}，配置 max_tokens≈{max_tokens}）。\
         请**缩小**本回合 JSON：拆分 `file:edit` / `file:write`、缩短 `tool_args` 里的长字符串，分多轮完成；仍须输出**完整闭合**的单一 JSON 对象。"
    )
}

/// Stream/HTTP failures that often follow truncated or oversized model JSON output.
fn is_recoverable_provider_stream_error(err: &anyhow::Error) -> bool {
    let s = err.to_string().to_ascii_lowercase();
    s.contains("decoding response body")
        || s.contains("error decoding")
        || s.contains("unexpected eof")
        || s.contains("connection reset")
        || s.contains("broken pipe")
        || s.contains("incomplete message")
        || s.contains("body completed")
}

fn provider_stream_recoverable_retry_message(err: &anyhow::Error, max_tokens: u32) -> String {
    format!(
        "【环境反馈】本回合模型输出异常（{err}），常见于输出过长导致 JSON 被截断或流传输中断。\n\n\
         请缩小本回合 payload（拆分编辑、减少单次 `content` / `oldString` 长度），重新输出**一个**完整 JSON 工具信封。\
         当前 max_tokens≈{max_tokens}。{length_hint}",
        length_hint = output_length_retry_supplement(max_tokens, "length")
    )
}

fn push_injected_format_retry_turn(
    stream: &StreamTx,
    conversation_id: &str,
    history: &mut Vec<ChatMessage>,
    hint: String,
) {
    let retry_id = new_id("fmt_retry");
    emit(
        stream,
        StreamEvent::InjectedUserMessage {
            conversation_id: conversation_id.to_string(),
            message_id: retry_id.clone(),
            content: hint.clone(),
        },
    );
    history.push(ChatMessage {
        id: retry_id,
        role: Role::User,
        content: hint,
        status: "done".into(),
        created_at: now_ms(),
        tool_calls: None,
        tool_call_id: None,
        error_message: None,
        reasoning: None,
        thoughts: None,
        headline: None,
        raw_content: None,
        agent_id: None,
        agent_name: None,
        agent_trace: None,
        images_base64: None,
        computer_round_screen_rel_path: None,
    });
}

/// Host-only binding for `task_board` and computer tools so models cannot spoof another session id.
fn inject_host_task_board_conversation_id(
    tool_id: &str,
    args: serde_json::Value,
    conversation_id: &str,
) -> serde_json::Value {
    let requires_injection = tool_id == "task_board"
        || crate::agents::computer::is_desktop_vision_log_tool(tool_id)
        || crate::agents::computer::is_desktop_post_delay_tool(tool_id);
    if !requires_injection {
        return args;
    }
    let mut map = if let serde_json::Value::Object(m) = args {
        m
    } else {
        serde_json::Map::new()
    };
    map.insert(
        "_conversation_id".to_string(),
        serde_json::Value::String(conversation_id.to_string()),
    );
    serde_json::Value::Object(map)
}

/// Key for [`crate::tools::task_board::TaskBoardStore`] during Supervisor **sub-agent** runs.
///
/// Isolated from the main chat `conversation_id` board: sub-agents do not read or write the
/// lead session’s task board unless the Supervisor copies state into instructions.
fn sub_agent_task_board_store_key(main_conversation_id: &str, supervisor_task_id: &str) -> String {
    format!(
        "{main}\x1fptr_sub_agent\x1f{task}",
        main = main_conversation_id.trim(),
        task = supervisor_task_id.trim()
    )
}

fn apply_session_agent_model_defaults(
    settings: &mut crate::models::ModelSettings,
    effective_agent_mode: &str,
) {
    let mode = effective_agent_mode.trim();
    let key = if mode == AGENT_MODE_SUPERVISOR {
        SUPERVISOR_AGENT_ID.to_string()
    } else {
        let id = settings.lead_agent_id.trim();
        if id.is_empty() {
            DEFAULT_AGENT_ID.to_string()
        } else {
            id.to_string()
        }
    };
    if let Some(pref) = settings.agent_default_models.get(&key) {
        if !pref.provider_id.trim().is_empty() {
            settings.active_provider_id = pref.provider_id.trim().to_string();
        }
        if !pref.model.trim().is_empty() {
            settings.model = pref.model.trim().to_string();
        }
    }
}

async fn run_chat_inner(
    stream: StreamTx,
    state: Arc<AppState>,
    conversation_id: &str,
    history: &mut Vec<ChatMessage>,
    enabled_skill_ids: &[String],
    request_agent_mode: Option<&str>,
    tool_rounds_used_single_start: u32,
    tool_rounds_used_supervisor_start: u32,
    consumed_single: &mut u32,
    consumed_supervisor: &mut u32,
    cancel: CancellationToken,
) -> Result<()> {
    let mut settings = storage::load_settings()?;
    let api_key = storage::load_api_key()?
        .ok_or_else(|| anyhow!("尚未配置 API Key，请先在设置中保存密钥"))?;
    settings.api_key = api_key.clone();
    let tool_approval_mode = settings.tool_approval_mode.clone();
    let effective_agent_mode = request_agent_mode
        .filter(|mode| !mode.trim().is_empty())
        .unwrap_or(&settings.agent_mode)
        .to_string();
    apply_session_agent_model_defaults(&mut settings, &effective_agent_mode);
    let lead_worker_id = settings.lead_agent_id.trim();
    let lead_opt = if lead_worker_id.is_empty() {
        None
    } else {
        Some(lead_worker_id)
    };
    let agent_plan = AgentOrchestrator::build_plan(
        &state.agents,
        &state.skills,
        &state.tools,
        enabled_skill_ids,
        &effective_agent_mode,
        lead_opt,
    );
    let provider = OpenAIProvider::new(settings.clone(), api_key);
    let mut llm_token_session = ChatLlmTokenSession::new(conversation_id.to_string());

    let t_compress = Instant::now();
    crate::context_compression::maybe_compress_history(
        history,
        &settings,
        &provider,
        conversation_id,
        &stream,
        cancel.clone(),
    )
    .await;
    log::info!(
        "run_chat_inner: maybe_compress_history finished conversation_id={} wall_ms={} history_messages={}",
        conversation_id,
        t_compress.elapsed().as_millis(),
        history.len(),
    );

    let max_cap = settings.max_tool_rounds.clamp(1, 10_000);

    if agent_plan.mode == AGENT_MODE_SUPERVISOR {
        if tool_rounds_used_supervisor_start >= max_cap {
            state.computer_state.mark_cancelled(conversation_id);
            return Err(anyhow!(
                "本会话在编排（Supervisor）模式下工具调用轮次已达上限（{}），请新开对话或在设置中调高上限。",
                max_cap
            ));
        }
        let mut tool_budget = SessionToolBudget::new(max_cap, tool_rounds_used_supervisor_start);
        let r = run_supervisor_chat(
            stream,
            state,
            conversation_id,
            history,
            enabled_skill_ids,
            provider,
            &mut tool_budget,
            cancel,
            effective_reasoning_in_messages(&settings),
            &mut llm_token_session.stats,
        )
        .await;
        tool_budget.sync_out(consumed_supervisor);
        return r;
    }

    if tool_rounds_used_single_start >= max_cap {
        return Err(anyhow!(
            "本会话在单智能体模式下工具调用轮次已达上限（{}），请新开对话或在设置中调高上限。",
            max_cap
        ));
    }
    let mut tool_budget = SessionToolBudget::new(max_cap, tool_rounds_used_single_start);
    let reasoning_in_messages = effective_reasoning_in_messages(&provider.settings);

    loop {
        if cancel.is_cancelled() {
            tool_budget.sync_out(consumed_single);
            return Err(anyhow!("已停止生成"));
        }

        if tool_budget.remaining() == 0 {
            tool_budget.sync_out(consumed_single);
            return Err(anyhow!(
                "本会话单智能体工具调用轮次已达上限（{}）。请新开对话。",
                max_cap
            ));
        }

        let assistant_id = new_id("msg");

        let tools_system_appendix = crate::tools_system_appendix::generate_tools_system_appendix(
            &state.tools,
            &agent_plan.allowed_tool_names,
        );
        let tools_appendix_enabled = !tools_system_appendix.is_empty();
        let (tx, mut rx) = mpsc::channel::<ProviderEvent>(64);
        let prov = OpenAIProvider::new(provider.settings.clone(), provider.api_key.clone());
        let lead_profile = state
            .agents
            .get(&agent_plan.lead_agent_id)
            .map(|a| a.def().profile.clone())
            .unwrap_or(AgentProfile::General);

        emit(
            &stream,
            StreamEvent::MessageStart {
                message_id: assistant_id.clone(),
                conversation_id: conversation_id.to_string(),
            },
        );

        let mut agent_trace = Vec::new();
        if agent_plan.mode == "supervisor" {
            let trace = AgentTrace {
                id: agent_plan.lead_agent_id.clone(),
                name: agent_plan.lead_agent_name.clone(),
                role: "supervisor".into(),
                status: "running".into(),
                detail: Some("正在拆解任务、调度专家 Agent 并整合结果".into()),
                content: None,
            };
            agent_trace.push(trace.clone());
            emit(
                &stream,
                StreamEvent::AgentStep {
                    message_id: assistant_id.clone(),
                    agent: trace,
                },
            );
        }

        let round_prep = Instant::now();
        let t = Instant::now();
        let mut history_for_api = history.clone();
        let clone_ms = t.elapsed().as_millis();
        let mut prompts_after_ctx = MessageLoopPromptsAfterContext {
            computer_state: state.computer_state.as_ref(),
            lead_agent_profile: lead_profile.clone(),
            messages: &mut history_for_api,
            conversation_id,
            stream: Some(&stream),
            round_assistant_message_id: Some(assistant_id.clone()),
            round_screen_dump_prefix: None,
        };
        let t = Instant::now();
        state
            .extensions
            .run_message_loop_prompts_after(&mut prompts_after_ctx)
            .await?;
        let message_loop_prompts_after_ms = t.elapsed().as_millis();

        let t = Instant::now();
        let mut prompts_with_env = Vec::new();
        let session_vars = SessionInjectVars {
            workspace_root: settings.workspace_root.trim(),
        };
        if let Some(block) = rendered_communication_public_inject() {
            prompts_with_env.push(block);
        }
        prompts_with_env.extend(
            agent_plan
                .system_prompts
                .iter()
                .map(|p| expand_agent_prompt_placeholders(p, &session_vars)),
        );
        if !tools_system_appendix.is_empty() {
            prompts_with_env.push(tools_system_appendix);
        }
        let assemble_system_prompts_ms = t.elapsed().as_millis();

        let t = Instant::now();
        let mut before_llm_ctx = BeforeMainLlmCallContext {
            computer_state: state.computer_state.as_ref(),
            lead_agent_profile: lead_profile,
            system_prompts: &mut prompts_with_env,
            conversation_id,
            task_board_store: state.task_board_store.clone(),
            task_board_store_key: conversation_id,
        };
        state
            .extensions
            .run_before_main_llm_call(&mut before_llm_ctx)
            .await?;
        push_env_context_last_in_system_prompts(&mut prompts_with_env);
        push_json_wire_format_tail(&mut prompts_with_env, tools_appendix_enabled);
        let before_main_llm_tail_ms = t.elapsed().as_millis();
        log::info!(
            "run_chat single_agent pre_stream_chat conversation_id={} assistant_id={} history_messages={} clone_ms={} message_loop_prompts_after_ms={} assemble_system_prompts_ms={} before_main_llm_tail_ms={} pre_stream_total_ms={}",
            conversation_id,
            assistant_id,
            history.len(),
            clone_ms,
            message_loop_prompts_after_ms,
            assemble_system_prompts_ms,
            before_main_llm_tail_ms,
            round_prep.elapsed().as_millis(),
        );
        let prompts_clone = prompts_with_env;
        let cancel_clone = cancel.clone();
        let dump_lbl = format!("{}_{}", conversation_id, assistant_id);
        let send_handle = tokio::spawn(async move {
            prov.stream_chat(
                &history_for_api,
                &prompts_clone,
                tx,
                cancel_clone,
                Some(dump_lbl.as_str()),
            )
            .await
        });

        let mut raw_content_buf = String::new();
        let mut reasoning_buf = String::new();
        let mut final_tool_calls: Vec<ToolCall> = Vec::new();
        let mut finish_reason = String::from("stop");
        let mut json_finish_diag = crate::json_tool_caller::JsonToolFinishDiagnostics::default();
        let mut xml_thoughts: Option<String> = None;
        let mut xml_headline: Option<String> = None;
        let mut streamed_tool_call_ids: HashSet<String> = HashSet::new();

        while let Some(ev) = rx.recv().await {
            match ev {
                ProviderEvent::ContentDelta(t) => {
                    raw_content_buf.push_str(&t);
                    emit(
                        &stream,
                        StreamEvent::RawContentDelta {
                            message_id: assistant_id.clone(),
                            text: t.clone(),
                        },
                    );
                    emit(
                        &stream,
                        StreamEvent::Delta {
                            message_id: assistant_id.clone(),
                            text: t.clone(),
                        },
                    );
                }
                ProviderEvent::ReasoningDelta(t) => {
                    if reasoning_in_messages {
                        reasoning_buf.push_str(&t);
                    }
                    // 与 `reasoning_in_messages` 解耦：界面「原始输出」可展示推理；持久化/API 仍由 `reasoning` 字段是否写入控制。
                    emit(
                        &stream,
                        StreamEvent::ReasoningDelta {
                            message_id: assistant_id.clone(),
                            text: t,
                        },
                    );
                }
                ProviderEvent::ToolCallStart { id, name, .. } => {
                    let tc = ToolCall {
                        id: id.clone(),
                        name: name.clone(),
                        arguments: String::new(),
                        status: "pending".into(),
                        result: None,
                        error: None,
                        duration_ms: None,
                        risk_level: state
                            .tools
                            .tool_risk_level_for_invocation(&name, &parse_tool_call_arguments(""))
                            .or(Some("low".into())),
                    };
                    emit(
                        &stream,
                        StreamEvent::ToolCallStart {
                            message_id: assistant_id.clone(),
                            tool_call: tc,
                        },
                    );
                }
                ProviderEvent::ToolCallArgsDelta {
                    tool_call_id, args, ..
                } => {
                    emit(
                        &stream,
                        StreamEvent::ToolCallArgsDelta {
                            message_id: assistant_id.clone(),
                            tool_call_id,
                            args_delta: args,
                        },
                    );
                }
                ProviderEvent::JsonToolStreamingReady { tool_calls, .. } => {
                    for tc in &tool_calls {
                        if streamed_tool_call_ids.insert(tc.id.clone()) {
                            let mut t = tc.clone();
                            let args_v = parse_tool_call_arguments(&t.arguments);
                            t.risk_level = state
                                .tools
                                .tool_risk_level_for_invocation(&t.name, &args_v)
                                .or(Some("low".into()));
                            emit(
                                &stream,
                                StreamEvent::ToolCallStart {
                                    message_id: assistant_id.clone(),
                                    tool_call: t,
                                },
                            );
                        }
                    }
                }
                ProviderEvent::AssistantJsonPartial {
                    thoughts,
                    headline,
                    tool_name,
                    response_text,
                } => {
                    emit(
                        &stream,
                        StreamEvent::AssistantJsonPartial {
                            message_id: assistant_id.clone(),
                            thoughts,
                            headline,
                            tool_name,
                            response_text,
                        },
                    );
                }
                ProviderEvent::Finish {
                    reason,
                    tool_calls,
                    json,
                    thoughts,
                    headline,
                    usage,
                } => {
                    finish_reason = reason;
                    json_finish_diag = json;
                    xml_thoughts = thoughts;
                    xml_headline = headline;
                    llm_token_session.stats.record_llm_round(usage.as_ref());
                    // 流式已发过 ToolCallStart 的 id 不再重复发送。
                    for tc in &tool_calls {
                        if streamed_tool_call_ids.insert(tc.id.clone()) {
                            let mut t = tc.clone();
                            let args_v = parse_tool_call_arguments(&t.arguments);
                            t.risk_level = state
                                .tools
                                .tool_risk_level_for_invocation(&t.name, &args_v)
                                .or(Some("low".into()));
                            emit(
                                &stream,
                                StreamEvent::ToolCallStart {
                                    message_id: assistant_id.clone(),
                                    tool_call: t,
                                },
                            );
                        }
                    }
                    final_tool_calls = tool_calls;
                }
            }
        }

        match send_handle.await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                if tools_appendix_enabled && is_recoverable_provider_stream_error(&e) {
                    log::warn!(
                        "recoverable provider stream error conversation_id={} assistant_id={}: {e:#}",
                        conversation_id,
                        assistant_id
                    );
                    emit(
                        &stream,
                        StreamEvent::MessageEnd {
                            message_id: assistant_id.clone(),
                            content: None,
                            raw_content: None,
                            thoughts: None,
                            headline: None,
                        },
                    );
                    let hint =
                        provider_stream_recoverable_retry_message(&e, effective_max_tokens(&settings));
                    push_injected_format_retry_turn(&stream, conversation_id, history, hint);
                    tool_budget.sync_out(consumed_single);
                    if tool_budget.is_exhausted() {
                        let hint = format!(
                            "单智能体模式下工具调用累计已达上限（{} 轮，含此前消息）。建议新开对话；将尝试压缩上下文以便查看摘要。",
                            max_cap
                        );
                        emit(
                            &stream,
                            StreamEvent::ToolRoundsExhausted {
                                conversation_id: conversation_id.to_string(),
                                max_rounds: max_cap,
                                message: hint,
                                will_retry_after_compress: settings.context_compression_enabled,
                            },
                        );
                        let _ = crate::context_compression::maybe_compress_after_tool_round_limit(
                            history,
                            &settings,
                            &provider,
                            conversation_id,
                            &stream,
                            cancel.clone(),
                            true,
                        )
                        .await;
                        tool_budget.sync_out(consumed_single);
                        state.computer_state.mark_cancelled(conversation_id);
                        return Err(anyhow!(
                            "单智能体模式下工具调用轮次已达上限（{max_cap}）。请新开对话或在设置中调高上限。"
                        ));
                    }
                    continue;
                }
                emit(
                    &stream,
                    StreamEvent::Error {
                        message_id: Some(assistant_id.clone()),
                        message: e.to_string(),
                    },
                );
                emit(
                    &stream,
                    StreamEvent::MessageEnd {
                        message_id: assistant_id.clone(),
                        content: None,
                        raw_content: None,
                        thoughts: None,
                        headline: None,
                    },
                );
                tool_budget.sync_out(consumed_single);
                state.computer_state.mark_cancelled(conversation_id);
                return Err(e);
            }
            Err(e) => {
                tool_budget.sync_out(consumed_single);
                state.computer_state.mark_cancelled(conversation_id);
                return Err(anyhow!("任务异常：{e}"));
            }
        }

        let assistant_msg = ChatMessage {
            id: assistant_id.clone(),
            role: Role::Assistant,
            content: extract_user_visible_content(&raw_content_buf),
            status: if final_tool_calls.is_empty() {
                "completed".into()
            } else {
                "streaming".into()
            },
            created_at: now_ms(),
            tool_calls: if final_tool_calls.is_empty() {
                None
            } else {
                Some(
                    final_tool_calls
                        .iter()
                        .cloned()
                        .map(|mut t| {
                            let args_v = parse_tool_call_arguments(&t.arguments);
                            t.risk_level = state
                                .tools
                                .tool_risk_level_for_invocation(&t.name, &args_v)
                                .or(Some("low".into()));
                            t
                        })
                        .collect(),
                )
            },
            tool_call_id: None,
            error_message: None,
            reasoning: if reasoning_in_messages && !reasoning_buf.is_empty() {
                Some(reasoning_buf)
            } else {
                None
            },
            thoughts: xml_thoughts,
            headline: xml_headline,
            raw_content: if raw_content_buf.is_empty() {
                None
            } else {
                Some(raw_content_buf.clone())
            },
            agent_id: Some(agent_plan.lead_agent_id.clone()),
            agent_name: Some(agent_plan.lead_agent_name.clone()),
            agent_trace: if agent_trace.is_empty() {
                None
            } else {
                Some(agent_trace.clone())
            },
            images_base64: None,
            computer_round_screen_rel_path: None,
        };
        history.push(assistant_msg.clone());
        emit(
            &stream,
            StreamEvent::MessageEnd {
                message_id: assistant_id.clone(),
                content: Some(assistant_msg.content.clone()),
                raw_content: assistant_msg.raw_content.clone(),
                thoughts: assistant_msg.thoughts.clone(),
                headline: assistant_msg.headline.clone(),
            },
        );

        if final_tool_calls.is_empty() {
            if let Some(hint) = json_tool_empty_calls_retry_message(
                &json_finish_diag,
                tools_appendix_enabled,
                &finish_reason,
                effective_max_tokens(&settings),
            ) {
                rollback_failed_json_assistant_turn(history, &assistant_id);
                push_injected_format_retry_turn(&stream, conversation_id, history, hint);
                // Format-only retry: no tool executed; do not consume the tool-round budget.
                tool_budget.sync_out(consumed_single);
                if tool_budget.is_exhausted() {
                    let hint = format!(
                        "单智能体模式下工具调用累计已达上限（{} 轮，含此前消息）。建议新开对话；将尝试压缩上下文以便查看摘要。",
                        max_cap
                    );
                    emit(
                        &stream,
                        StreamEvent::ToolRoundsExhausted {
                            conversation_id: conversation_id.to_string(),
                            max_rounds: max_cap,
                            message: hint,
                            will_retry_after_compress: settings.context_compression_enabled,
                        },
                    );
                    let _ = crate::context_compression::maybe_compress_after_tool_round_limit(
                        history,
                        &settings,
                        &provider,
                        conversation_id,
                        &stream,
                        cancel.clone(),
                        true,
                    )
                    .await;
                    tool_budget.sync_out(consumed_single);
                    state.computer_state.mark_cancelled(conversation_id);
                    return Err(anyhow!(
                        "单智能体模式下工具调用轮次已达上限（{max_cap}）。请新开对话或在设置中调高上限。"
                    ));
                }
                continue;
            }
            let _ = finish_reason;
            tool_budget.sync_out(consumed_single);
            return Ok(());
        }

        if let Err(err) = validate_envelope_tool_batch(&state.tools, &final_tool_calls) {
            log::warn!("tool envelope batch rejected: {err}");
            let hint = json_tool_envelope_batch_retry_message(&err);
            let retry_id = new_id("msg");
            emit(
                &stream,
                StreamEvent::InjectedUserMessage {
                    conversation_id: conversation_id.to_string(),
                    message_id: retry_id.clone(),
                    content: hint.clone(),
                },
            );
            history.push(ChatMessage {
                id: retry_id,
                role: Role::User,
                content: hint,
                status: "done".into(),
                created_at: now_ms(),
                tool_calls: None,
                tool_call_id: None,
                error_message: None,
                reasoning: None,
                thoughts: None,
                headline: None,
                raw_content: None,
                agent_id: None,
                agent_name: None,
                agent_trace: None,
                images_base64: None,
                computer_round_screen_rel_path: None,
            });
            // Format-only retry: no tool executed; do not consume the tool-round budget.
            tool_budget.sync_out(consumed_single);
            if tool_budget.is_exhausted() {
                let hint = format!(
                    "单智能体模式下工具调用累计已达上限（{} 轮，含此前消息）。建议新开对话；将尝试压缩上下文以便查看摘要。",
                    max_cap
                );
                emit(
                    &stream,
                    StreamEvent::ToolRoundsExhausted {
                        conversation_id: conversation_id.to_string(),
                        max_rounds: max_cap,
                        message: hint,
                        will_retry_after_compress: settings.context_compression_enabled,
                    },
                );
                let _ = crate::context_compression::maybe_compress_after_tool_round_limit(
                    history,
                    &settings,
                    &provider,
                    conversation_id,
                    &stream,
                    cancel.clone(),
                    true,
                )
                .await;
                tool_budget.sync_out(consumed_single);
                state.computer_state.mark_cancelled(conversation_id);
                return Err(anyhow!(
                    "单智能体模式下工具调用轮次已达上限（{max_cap}）。请新开对话或在设置中调高上限。"
                ));
            }
            continue;
        }

        let mut any_executed = false;
        for tc in &final_tool_calls {
            if cancel.is_cancelled() {
                tool_budget.sync_out(consumed_single);
                state.computer_state.mark_cancelled(conversation_id);
                return Err(anyhow!("已停止生成"));
            }

            let args_value = parse_tool_call_arguments(&tc.arguments);
            let (mut tool_id, args_value) = merge_tool_method_from_qualified_name(&tc.name, args_value);
            tool_id = tool_id.trim().to_string();
            let args_value =
                inject_host_task_board_conversation_id(&tool_id, args_value, conversation_id);
            if tool_id.is_empty() {
                let err = "工具名为空：请检查 <tool_name>（例如 mouse:click_index、composite_action、response）。";
                emit(
                    &stream,
                    StreamEvent::ToolCallStatus {
                        message_id: assistant_id.clone(),
                        tool_call_id: tc.id.clone(),
                        status: "failed".into(),
                        result: None,
                        error: Some(err.to_string()),
                        duration_ms: Some(0),
                    },
                );
                history.push(tool_result_msg(&tc.id, &format!("ERROR: {err}")));
                any_executed = true;
                continue;
            }

            if tool_id == "response" {
                let message = response_text_from_args(&args_value).unwrap_or("");

                if !message.is_empty() {
                    emit(
                        &stream,
                        StreamEvent::Delta {
                            message_id: assistant_id.clone(),
                            text: message.to_string(),
                        },
                    );
                }

                emit(
                    &stream,
                    StreamEvent::ToolCallStatus {
                        message_id: assistant_id.clone(),
                        tool_call_id: tc.id.clone(),
                        status: "success".into(),
                        result: Some("已回复用户".into()),
                        error: None,
                        duration_ms: Some(0),
                    },
                );

                // PyProjects: `response` does not append a separate tool-result history line; finalize
                // the same assistant row (user-visible `content`, keep `raw_content` for the API).
                let mut wire_thoughts: Option<String> = None;
                let mut wire_headline: Option<String> = None;
                if let Some(last) = history.last_mut() {
                    if last.id == assistant_id && matches!(last.role, Role::Assistant) {
                        last.content = message.to_string();
                        last.tool_calls = None;
                        last.status = "completed".into();
                        wire_thoughts = last.thoughts.clone();
                        wire_headline = last.headline.clone();
                    }
                }

                emit(
                    &stream,
                    StreamEvent::MessageEnd {
                        message_id: assistant_id.clone(),
                        content: Some(message.to_string()),
                        raw_content: if raw_content_buf.is_empty() {
                            None
                        } else {
                            Some(raw_content_buf.clone())
                        },
                        thoughts: wire_thoughts,
                        headline: wire_headline,
                    },
                );

                tool_budget.sync_out(consumed_single);
                return Ok(());
            }

            let requires_approval = tool_approval_mode == "manual"
                && state.tools.tool_invocation_needs_approval(&tool_id, &args_value);

            if requires_approval {
                emit(
                    &stream,
                    StreamEvent::ToolCallStatus {
                        message_id: assistant_id.clone(),
                        tool_call_id: tc.id.clone(),
                        status: "pending_approval".into(),
                        result: None,
                        error: None,
                        duration_ms: None,
                    },
                );
                let (atx, arx) = oneshot::channel::<bool>();
                state.approvals.lock().insert(tc.id.clone(), atx);
                let approved = tokio::select! {
                    _ = cancel.cancelled() => {
                        state.approvals.lock().remove(&tc.id);
                        false
                    }
                    v = arx => v.unwrap_or(false),
                };
                if !approved {
                    let err = "用户已拒绝该工具调用".to_string();
                    emit(
                        &stream,
                        StreamEvent::ToolCallStatus {
                            message_id: assistant_id.clone(),
                            tool_call_id: tc.id.clone(),
                            status: "rejected".into(),
                            result: None,
                            error: Some(err.clone()),
                            duration_ms: None,
                        },
                    );
                    history.push(tool_result_msg(&tc.id, &err));
                    any_executed = true;
                    continue;
                }
            }

            emit(
                &stream,
                StreamEvent::ToolCallStatus {
                    message_id: assistant_id.clone(),
                    tool_call_id: tc.id.clone(),
                    status: "running".into(),
                    result: None,
                    error: None,
                    duration_ms: None,
                },
            );
            llm_token_session.stats.record_tool_invocation();
            let started = Instant::now();

            let is_terminal = tool_id == "terminal";
            let msg_id_for_stream = assistant_id.clone();
            let tc_id_for_stream = tc.id.clone();
            let stream_for_terminal = stream.clone();
            let args_for_desktop_log = args_value.clone();

            let exec: Result<(String, bool, Option<String>), anyhow::Error> = if is_terminal {
                let cancel_terminal = cancel.clone();
                let abort_flag = Arc::new(AtomicBool::new(false));
                {
                    let mut m = state.terminal_run_abort.lock();
                    if let Some(old) = m.insert(conversation_id.to_string(), abort_flag.clone()) {
                        old.store(true, Ordering::SeqCst);
                    }
                }
                let cleanup_id = conversation_id.to_string();
                let join = tokio::task::spawn_blocking(move || {
                    run_terminal_command_streaming(
                        args_value,
                        move |output| {
                            let _ = stream_for_terminal.send(StreamEvent::TerminalOutputDelta {
                                message_id: msg_id_for_stream.clone(),
                                tool_call_id: tc_id_for_stream.clone(),
                                output: output.to_string(),
                            });
                        },
                        Some(cancel_terminal),
                        Some(abort_flag),
                    )
                    .map(|r| {
                        let (ok, err_note) = terminal_stream_tool_status(&r);
                        let body = serde_json::json!({
                            "exitCode": r.exit_code,
                            "success": r.success,
                            "timedOut": r.timed_out,
                            "cancelled": r.cancelled,
                            "runAborted": r.run_aborted,
                            "durationMs": r.duration_ms,
                            "stdout": r.stdout,
                            "stderr": r.stderr,
                            "stdoutTruncated": r.stdout_truncated,
                            "stderrTruncated": r.stderr_truncated,
                        })
                        .to_string();
                        (body, ok, err_note)
                    })
                })
                .await;
                state.terminal_run_abort.lock().remove(&cleanup_id);
                join.map_err(|e| anyhow!("终端执行线程异常: {e}"))?
            } else {
                state
                    .tools
                    .invoke(&tool_id, args_value)
                    .map(|out| (out, true, None))
            };

            let duration = started.elapsed().as_millis() as u64;
            match exec {
                Ok((out, ok, err_note)) => {
                    let failed_note = desktop_tool_failure_note(ok, &err_note, &out);
                    state.computer_state.record_desktop_tool_if_applicable(
                        conversation_id,
                        &tool_id,
                        &args_for_desktop_log,
                        failed_note.as_deref(),
                    );
                    if ok && crate::agents::computer::is_desktop_post_delay_tool(tool_id.as_str()) {
                        let delay_ms = crate::agents::computer::post_desktop_action_delay_ms_from_tool_args(
                            &args_for_desktop_log,
                        );
                        log::info!(
                            "desktop post_action sleep {}ms before next capture (tool={})",
                            delay_ms,
                            tool_id
                        );
                        tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                    }
                    let preview = truncate_str(&out, 800);
                    emit(
                        &stream,
                        StreamEvent::ToolCallStatus {
                            message_id: assistant_id.clone(),
                            tool_call_id: tc.id.clone(),
                            status: if ok { "success".into() } else { "failed".into() },
                            result: Some(preview),
                            error: err_note,
                            duration_ms: Some(duration),
                        },
                    );
                    history.push(tool_result_msg(&tc.id, &out));
                }
                Err(e) => {
                    let err = e.to_string();
                    let err_snip = truncate_str(&err, 400);
                    state.computer_state.record_desktop_tool_if_applicable(
                        conversation_id,
                        &tool_id,
                        &args_for_desktop_log,
                        Some(err_snip.as_str()),
                    );
                    emit(
                        &stream,
                        StreamEvent::ToolCallStatus {
                            message_id: assistant_id.clone(),
                            tool_call_id: tc.id.clone(),
                            status: "failed".into(),
                            result: None,
                            error: Some(err.clone()),
                            duration_ms: Some(duration),
                        },
                    );
                    history.push(tool_result_msg(&tc.id, &format!("ERROR: {err}")));
                }
            }
            any_executed = true;
        }

        if !any_executed {
            tool_budget.sync_out(consumed_single);
            return Ok(());
        }
        tool_budget.record_tool_cycle();
        tool_budget.sync_out(consumed_single);

        if tool_budget.is_exhausted() {
            let hint = format!(
                "单智能体模式下工具调用累计已达上限（{} 轮，含此前消息）。建议新开对话；将尝试压缩上下文以便查看摘要。",
                max_cap
            );
            emit(
                &stream,
                StreamEvent::ToolRoundsExhausted {
                    conversation_id: conversation_id.to_string(),
                    max_rounds: max_cap,
                    message: hint,
                    will_retry_after_compress: settings.context_compression_enabled,
                },
            );
            let _ = crate::context_compression::maybe_compress_after_tool_round_limit(
                history,
                &settings,
                &provider,
                conversation_id,
                &stream,
                cancel.clone(),
                true,
            )
            .await;
            tool_budget.sync_out(consumed_single);
            state.computer_state.mark_cancelled(conversation_id);
            return Err(anyhow!(
                "单智能体模式下工具调用轮次已达上限（{max_cap}）。请新开对话或在设置中调高上限。"
            ));
        }
    }
}

async fn run_supervisor_chat(
    stream: StreamTx,
    state: Arc<AppState>,
    conversation_id: &str,
    history: &mut Vec<ChatMessage>,
    enabled_skill_ids: &[String],
    provider: OpenAIProvider,
    tool_budget: &mut SessionToolBudget,
    cancel: CancellationToken,
    reasoning_in_messages: bool,
    llm_stats: &mut ConversationLlmStats,
) -> Result<()> {
    if cancel.is_cancelled() {
        return Err(anyhow!("已停止生成"));
    }

    let assistant_id = new_id("msg");
    emit(
        &stream,
        StreamEvent::MessageStart {
            message_id: assistant_id.clone(),
            conversation_id: conversation_id.to_string(),
        },
    );

    let limits = AgentRunLimits::default();
    let mut agent_trace = Vec::new();
    let sup_meta = state.agents.get(SUPERVISOR_AGENT_ID);
    let sup_name = sup_meta
        .as_ref()
        .map(|a| a.def().name.clone())
        .unwrap_or_else(|| "Supervisor".into());
    emit_agent_step(
        &stream,
        &assistant_id,
        &mut agent_trace,
        AgentTrace {
            id: "supervisor".into(),
            name: sup_name.clone(),
            role: "supervisor".into(),
            status: "planning".into(),
            detail: Some("正在规划子 Agent 执行任务".into()),
            content: None,
        },
    );

    let env_context = crate::env_prompt::build_environment_context_full();
    let tasks = match plan_agent_tasks(
        &provider,
        &state,
        history,
        &limits,
        cancel.clone(),
        &env_context,
        conversation_id,
        &assistant_id,
        llm_stats,
    )
    .await
    {
        Ok(tasks) => tasks,
        Err(err) => {
            log::warn!("agent planning failed, fallback to default agent: {err}");
            fallback_agent_tasks(&state, history, &limits)
        }
    };
    let tasks = sort_agent_tasks_topologically(tasks);
    let tasks: Vec<_> = tasks.into_iter().take(limits.max_sub_agents).collect();

    let mut results = Vec::new();
    let mut results_by_id: HashMap<String, AgentRunResult> = HashMap::new();
    for task in tasks {
        if cancel.is_cancelled() {
            return Err(anyhow!("已停止生成"));
        }
        let agent = state
            .agents
            .get(&task.agent_id)
            .or_else(|| state.agents.get(DEFAULT_AGENT_ID))
            .ok_or_else(|| anyhow!("未找到可执行 Agent: {}", task.agent_id))?;
        let def = agent.def();
        emit_agent_step(
            &stream,
            &assistant_id,
            &mut agent_trace,
            AgentTrace {
                id: def.id.clone(),
                name: def.name.clone(),
                role: def.role.clone(),
                status: "running".into(),
                detail: Some(if task.title.is_empty() {
                    task.instruction.clone()
                } else {
                    task.title.clone()
                }),
                content: Some(String::new()),
            },
        );

        let mut task_run = task.clone();
        if !task.depends_on.is_empty() {
            let mut pre = String::from("\n\n[Prior task outputs]\n");
            for d in &task.depends_on {
                if let Some(r) = results_by_id.get(d) {
                    pre.push_str(&format!(
                        "--- task {} ---\n{}\n",
                        d,
                        truncate_str(&r.content, 2000)
                    ));
                }
            }
            task_run.instruction = format!("{pre}\n\n[Current task]\n{}", task.instruction);
        }

        match run_sub_agent(
            &provider,
            &state,
            &stream,
            conversation_id,
            &assistant_id,
            &mut agent_trace,
            enabled_skill_ids,
            &task_run,
            tool_budget,
            cancel.clone(),
            reasoning_in_messages,
            llm_stats,
        )
        .await
        {
            Ok(result) => {
                results_by_id.insert(task.id.clone(), result.clone());
                emit_agent_step(
                    &stream,
                    &assistant_id,
                    &mut agent_trace,
                    AgentTrace {
                        id: def.id,
                        name: def.name,
                        role: def.role,
                        status: "completed".into(),
                        detail: Some(truncate_str(&result.content, 160)),
                        content: Some(result.content.clone()),
                    },
                );
                results.push(result);
            }
            Err(err) => {
                emit_agent_step(
                    &stream,
                    &assistant_id,
                    &mut agent_trace,
                    AgentTrace {
                        id: def.id,
                        name: def.name,
                        role: def.role,
                        status: "failed".into(),
                        detail: Some(err.to_string()),
                        content: None,
                    },
                );
            }
        }
    }

    emit_agent_step(
        &stream,
        &assistant_id,
        &mut agent_trace,
        AgentTrace {
            id: "supervisor".into(),
            name: sup_name.clone(),
            role: "supervisor".into(),
            status: "summarizing".into(),
            detail: Some("正在整合子 Agent 结果".into()),
            content: None,
        },
    );

    let final_answer = synthesize_final_answer(
        &provider,
        history,
        &results,
        cancel,
        conversation_id,
        &assistant_id,
        llm_stats,
    )
    .await?;
    if !final_answer.is_empty() {
        emit(
            &stream,
            StreamEvent::Delta {
                message_id: assistant_id.clone(),
                text: final_answer.clone(),
            },
        );
    }

    history.push(ChatMessage {
        id: assistant_id.clone(),
        role: Role::Assistant,
        content: final_answer.clone(),
        status: "completed".into(),
        created_at: now_ms(),
        tool_calls: None,
        tool_call_id: None,
        error_message: None,
        reasoning: None,
        thoughts: None,
        headline: None,
        raw_content: None,
        agent_id: Some("supervisor".into()),
        agent_name: Some(sup_name),
        agent_trace: Some(agent_trace),
        images_base64: None,
        computer_round_screen_rel_path: None,
    });
    emit(
        &stream,
        StreamEvent::MessageEnd {
            message_id: assistant_id,
            content: Some(final_answer),
            raw_content: None,
            thoughts: None,
            headline: None,
        },
    );
    state.computer_state.mark_ended(conversation_id);
    Ok(())
}

/// Topological order by `dependsOn` (task ids). Unknown dependency ids are ignored. On cycle, keep planner order.
fn sort_agent_tasks_topologically(tasks: Vec<AgentTask>) -> Vec<AgentTask> {
    let n = tasks.len();
    if n <= 1 {
        return tasks;
    }
    let id_set: HashSet<_> = tasks.iter().map(|t| t.id.as_str()).collect();
    let mut indeg: HashMap<String, usize> = HashMap::new();
    let mut adj: HashMap<String, Vec<String>> = HashMap::new();
    for t in &tasks {
        let c = t
            .depends_on
            .iter()
            .filter(|d| id_set.contains(d.as_str()))
            .count();
        indeg.insert(t.id.clone(), c);
    }
    for t in &tasks {
        for d in &t.depends_on {
            if id_set.contains(d.as_str()) {
                adj.entry(d.clone()).or_default().push(t.id.clone());
            }
        }
    }
    let mut q: VecDeque<String> = VecDeque::new();
    for t in &tasks {
        if indeg.get(&t.id).copied().unwrap_or(0) == 0 {
            q.push_back(t.id.clone());
        }
    }
    let mut order_ids = Vec::new();
    while let Some(u) = q.pop_front() {
        order_ids.push(u.clone());
        for v in adj.get(&u).into_iter().flatten() {
            let e = indeg.entry(v.clone()).or_insert(0);
            if *e > 0 {
                *e -= 1;
            }
            if *e == 0 {
                q.push_back(v.clone());
            }
        }
    }
    if order_ids.len() != n {
        log::warn!("agent task graph has cycle or inconsistent deps; using planner order");
        return tasks;
    }
    let mut by_id: HashMap<String, AgentTask> = tasks.into_iter().map(|t| (t.id.clone(), t)).collect();
    order_ids
        .into_iter()
        .filter_map(|id| by_id.remove(&id))
        .collect()
}

async fn plan_agent_tasks(
    provider: &OpenAIProvider,
    state: &AppState,
    history: &[ChatMessage],
    limits: &AgentRunLimits,
    cancel: CancellationToken,
    env_context: &str,
    conversation_id: &str,
    assistant_message_id: &str,
    llm_stats: &mut ConversationLlmStats,
) -> Result<Vec<AgentTask>> {
    let workers = state.agents.enabled_workers();
    let roster = agent_roster(&workers);
    let prompt = format!(
        "{}\n\nYou are the Supervisor. Decompose the user's latest request into at most {} sub-agent tasks.\n\nPlanning rules: for work in one repo (implementation, debugging, refactor), **prefer a single** task with agentId `coder` and a complete `instruction`; split only when an independent reviewer pass or a clearly non-code subtask is needed. If multiple tasks have ordering, set `dependsOn` to an array of prerequisite task ids.\n\nAvailable agents:\n{}\n\nReturn **only** a JSON array (no Markdown). Element shape: {{\"id\":\"task_1\",\"agentId\":\"coder\",\"title\":\"short title\",\"instruction\":\"full instructions for that agent\",\"dependsOn\":[]}}. `agentId` must be from the list above. Use `default` for general Q&A; prefer `coder` for code, repo reads, and tests.",
        env_context,
        limits.max_sub_agents,
        roster
    );
    let dump_lbl = format!("{conversation_id}_{assistant_message_id}_supervisor_plan");
    let out = provider
        .chat_once(
            history,
            &[prompt],
            cancel,
            None,
            Some(dump_lbl.as_str()),
        )
        .await?;
    llm_stats.record_llm_round(out.usage.as_ref());
    parse_agent_tasks(&out.text, &workers, limits)
        .or_else(|| Some(fallback_agent_tasks(state, history, limits)))
        .ok_or_else(|| anyhow!("无法生成 Agent 任务计划"))
}

async fn run_sub_agent(
    provider: &OpenAIProvider,
    state: &AppState,
    stream: &StreamTx,
    conversation_id: &str,
    message_id: &str,
    agent_trace: &mut Vec<AgentTrace>,
    enabled_skill_ids: &[String],
    task: &AgentTask,
    tool_budget: &mut SessionToolBudget,
    cancel: CancellationToken,
    reasoning_in_messages: bool,
    llm_stats: &mut ConversationLlmStats,
) -> Result<AgentRunResult> {
    let agent = state
        .agents
        .get(&task.agent_id)
        .or_else(|| state.agents.get(DEFAULT_AGENT_ID))
        .ok_or_else(|| anyhow!("未找到 Agent: {}", task.agent_id))?;
    let def = agent.def();
    let mut skill_ids = enabled_skill_ids.to_vec();
    if !def.access_policy.allow_skills.is_empty() {
        skill_ids.retain(|id| def.access_policy.allow_skills.contains(id));
    }
    skill_ids.retain(|id| !def.access_policy.deny_skills.contains(id));
    skill_ids.sort();
    skill_ids.dedup();

    let (skill_prompts, session_tools) = state.skills.progressive_context(&skill_ids);
    let allowed_tools = resolve_agent_tools(&def, &session_tools, &state.tools);
    let sub_task_board_key = sub_agent_task_board_store_key(conversation_id, task.id.trim());
    let session_vars = SessionInjectVars {
        workspace_root: provider.settings.workspace_root.trim(),
    };
    let expanded_role = expand_agent_prompt_placeholders(&agent.system_prompt(), &session_vars);
    let sub_agent_header = format!(
        "Sub-agent: {} ({})\nprofile: {:?}\ndescription: {}\n\n{}\n\nComplete only the subtask delivered in the next user message from the Supervisor. That message is task instructions (it may include a digest of prior task outputs) and does **not** include the main chat history. Your output should state conclusions, key evidence, risks, or open items.\nAllowed tools: {}",
        def.name,
        def.id,
        def.profile,
        def.description,
        expanded_role,
        if allowed_tools.is_empty() {
            "none".into()
        } else {
            allowed_tools.join(", ")
        }
    );
    let mut prompts = Vec::new();
    if let Some(block) = rendered_communication_public_inject() {
        prompts.push(block);
    }
    prompts.push(sub_agent_header);
    prompts.extend(skill_prompts);

    let tools_system_appendix = crate::tools_system_appendix::generate_tools_system_appendix(
        &state.tools,
        &allowed_tools,
    );
    let tool_approval_mode = storage::load_settings()
        .map(|settings| settings.tool_approval_mode)
        .unwrap_or_else(|_| "auto".into());
    let max_cap = tool_budget.cap();
    let mut local_history = vec![ChatMessage {
        id: new_id("sub_task"),
        role: Role::User,
        content: task.instruction.clone(),
        status: "done".into(),
        created_at: now_ms(),
        tool_calls: None,
        tool_call_id: None,
        error_message: None,
        reasoning: None,
        thoughts: None,
        headline: None,
        raw_content: None,
        agent_id: None,
        agent_name: None,
        agent_trace: None,
        images_base64: None,
        computer_round_screen_rel_path: None,
    }];
    let mut content = String::new();
    let mut reasoning = String::new();

    loop {
        if cancel.is_cancelled() {
            state.computer_state.mark_cancelled(conversation_id);
            return Err(anyhow!("已停止生成"));
        }

        if tool_budget.remaining() == 0 {
            state.computer_state.mark_cancelled(conversation_id);
            return Err(anyhow!(
                "编排（Supervisor）模式下工具调用轮次已达上限（{}）。请新开对话。",
                max_cap
            ));
        }

        let round_message_id = new_id("agent_msg");
        let (tx, mut rx) = mpsc::channel::<ProviderEvent>(64);
        let prov = OpenAIProvider::new(provider.settings.clone(), provider.api_key.clone());
        let round_prep = Instant::now();
        let t = Instant::now();
        let mut history_for_api = local_history.clone();
        let clone_ms = t.elapsed().as_millis();
        let mut prompts_after_ctx = MessageLoopPromptsAfterContext {
            computer_state: state.computer_state.as_ref(),
            lead_agent_profile: def.profile.clone(),
            messages: &mut history_for_api,
            conversation_id,
            stream: Some(stream),
            round_assistant_message_id: Some(message_id.to_string()),
            round_screen_dump_prefix: Some(round_message_id.clone()),
        };
        let t = Instant::now();
        state
            .extensions
            .run_message_loop_prompts_after(&mut prompts_after_ctx)
            .await?;
        let message_loop_prompts_after_ms = t.elapsed().as_millis();

        let t = Instant::now();
        let mut prompts_clone = prompts.clone();
        if !tools_system_appendix.is_empty() {
            prompts_clone.push(tools_system_appendix.clone());
        }
        let assemble_system_prompts_ms = t.elapsed().as_millis();

        let t = Instant::now();
        let mut before_llm_ctx = BeforeMainLlmCallContext {
            computer_state: state.computer_state.as_ref(),
            lead_agent_profile: def.profile.clone(),
            system_prompts: &mut prompts_clone,
            conversation_id,
            task_board_store: state.task_board_store.clone(),
            task_board_store_key: sub_task_board_key.as_str(),
        };
        state
            .extensions
            .run_before_main_llm_call(&mut before_llm_ctx)
            .await?;
        push_env_context_last_in_system_prompts(&mut prompts_clone);
        push_json_wire_format_tail(
            &mut prompts_clone,
            !tools_system_appendix.is_empty(),
        );
        let before_main_llm_tail_ms = t.elapsed().as_millis();
        log::info!(
            "run_chat supervisor_sub_agent pre_stream_chat conversation_id={} task_id={} message_id={} local_history_messages={} clone_ms={} message_loop_prompts_after_ms={} assemble_system_prompts_ms={} before_main_llm_tail_ms={} pre_stream_total_ms={}",
            conversation_id,
            task.id,
            message_id,
            local_history.len(),
            clone_ms,
            message_loop_prompts_after_ms,
            assemble_system_prompts_ms,
            before_main_llm_tail_ms,
            round_prep.elapsed().as_millis(),
        );
        let cancel_clone = cancel.clone();
        let dump_lbl = format!("{}_{}_sub_{}", conversation_id, message_id, task.id);
        let handle = tokio::spawn(async move {
            prov.stream_chat(
                &history_for_api,
                &prompts_clone,
                tx,
                cancel_clone,
                Some(dump_lbl.as_str()),
            )
            .await
        });

        let mut round_content = String::new();
        let mut round_reasoning = String::new();
        let mut final_tool_calls: Vec<ToolCall> = Vec::new();
        let mut finish_reason = String::from("stop");
        let mut json_finish_diag = crate::json_tool_caller::JsonToolFinishDiagnostics::default();
        let mut round_thoughts: Option<String> = None;
        let mut round_headline: Option<String> = None;
        let mut streamed_round_tool_call_ids: HashSet<String> = HashSet::new();

        while let Some(ev) = rx.recv().await {
            match ev {
                ProviderEvent::ContentDelta(delta) => {
                    round_content.push_str(&delta);
                    content.push_str(&delta);
                    emit_agent_content_delta(
                        stream,
                        message_id,
                        agent_trace,
                        &def,
                        task,
                        content.clone(),
                    );
                }
                ProviderEvent::ReasoningDelta(delta) => {
                    if reasoning_in_messages {
                        round_reasoning.push_str(&delta);
                        reasoning.push_str(&delta);
                    }
                    emit(
                        stream,
                        StreamEvent::ReasoningDelta {
                            message_id: message_id.to_string(),
                            text: delta,
                        },
                    );
                }
                ProviderEvent::ToolCallStart { id, name, .. } => {
                    emit(
                        stream,
                        StreamEvent::ToolCallStart {
                            message_id: message_id.to_string(),
                            tool_call: ToolCall {
                                id,
                                name: name.clone(),
                                arguments: String::new(),
                                status: "pending".into(),
                                result: None,
                                error: None,
                                duration_ms: None,
                                risk_level: state
                                    .tools
                                    .tool_risk_level_for_invocation(
                                        &name,
                                        &parse_tool_call_arguments(""),
                                    )
                                    .or(Some("low".into())),
                            },
                        },
                    );
                }
                ProviderEvent::ToolCallArgsDelta {
                    tool_call_id, args, ..
                } => {
                    emit(
                        stream,
                        StreamEvent::ToolCallArgsDelta {
                            message_id: message_id.to_string(),
                            tool_call_id,
                            args_delta: args,
                        },
                    );
                }
                ProviderEvent::JsonToolStreamingReady { tool_calls, .. } => {
                    for tc in &tool_calls {
                        if streamed_round_tool_call_ids.insert(tc.id.clone()) {
                            let mut t = tc.clone();
                            let args_v = parse_tool_call_arguments(&t.arguments);
                            t.risk_level = state
                                .tools
                                .tool_risk_level_for_invocation(&t.name, &args_v)
                                .or(Some("low".into()));
                            emit(
                                stream,
                                StreamEvent::ToolCallStart {
                                    message_id: message_id.to_string(),
                                    tool_call: t,
                                },
                            );
                        }
                    }
                }
                ProviderEvent::AssistantJsonPartial {
                    thoughts,
                    headline,
                    tool_name,
                    response_text,
                } => {
                    emit(
                        stream,
                        StreamEvent::AssistantJsonPartial {
                            message_id: message_id.to_string(),
                            thoughts,
                            headline,
                            tool_name,
                            response_text,
                        },
                    );
                }
                ProviderEvent::Finish {
                    reason,
                    tool_calls,
                    json,
                    thoughts,
                    headline,
                    usage,
                } => {
                    finish_reason = reason;
                    json_finish_diag = json;
                    round_thoughts = thoughts;
                    round_headline = headline;
                    llm_stats.record_llm_round(usage.as_ref());
                    for tc in &tool_calls {
                        if streamed_round_tool_call_ids.insert(tc.id.clone()) {
                            let mut t = tc.clone();
                            let args_v = parse_tool_call_arguments(&t.arguments);
                            t.risk_level = state
                                .tools
                                .tool_risk_level_for_invocation(&t.name, &args_v)
                                .or(Some("low".into()));
                            emit(
                                stream,
                                StreamEvent::ToolCallStart {
                                    message_id: message_id.to_string(),
                                    tool_call: t,
                                },
                            );
                        }
                    }
                    final_tool_calls = tool_calls;
                }
            }
        }

        let tools_appendix_enabled = !tools_system_appendix.is_empty();

        match handle.await {
            Ok(Ok(())) => {}
            Ok(Err(err)) => {
                if tools_appendix_enabled && is_recoverable_provider_stream_error(&err) {
                    log::warn!(
                        "recoverable provider stream error sub_agent task_id={} agent={}: {err:#}",
                        task.id,
                        def.id
                    );
                    let hint = provider_stream_recoverable_retry_message(
                        &err,
                        effective_max_tokens(&provider.settings),
                    );
                    local_history.push(ChatMessage {
                        id: new_id("fmt_retry"),
                        role: Role::User,
                        content: hint,
                        status: "done".into(),
                        created_at: now_ms(),
                        tool_calls: None,
                        tool_call_id: None,
                        error_message: None,
                        reasoning: None,
                        thoughts: None,
                        headline: None,
                        raw_content: None,
                        agent_id: None,
                        agent_name: None,
                        agent_trace: None,
                        images_base64: None,
                        computer_round_screen_rel_path: None,
                    });
                    if tool_budget.is_exhausted() {
                        let hint = format!(
                            "编排（Supervisor）模式下工具调用累计已达上限（{} 轮，含子 Agent）。建议新开对话。",
                            max_cap
                        );
                        emit(
                            stream,
                            StreamEvent::ToolRoundsExhausted {
                                conversation_id: conversation_id.to_string(),
                                max_rounds: max_cap,
                                message: hint,
                                will_retry_after_compress: provider.settings.context_compression_enabled,
                            },
                        );
                        let _ = crate::context_compression::maybe_compress_after_tool_round_limit(
                            &mut local_history,
                            &provider.settings,
                            provider,
                            conversation_id,
                            stream,
                            cancel.clone(),
                            false,
                        )
                        .await;
                        state.computer_state.mark_cancelled(conversation_id);
                        return Err(anyhow!(
                            "编排（Supervisor）模式下工具调用轮次已达上限（{max_cap}）。请新开对话。"
                        ));
                    }
                    continue;
                }
                state.computer_state.mark_cancelled(conversation_id);
                return Err(err);
            }
            Err(err) => {
                state.computer_state.mark_cancelled(conversation_id);
                return Err(anyhow!("子 Agent 任务异常：{err}"));
            }
        }

        local_history.push(ChatMessage {
            id: round_message_id.clone(),
            role: Role::Assistant,
            content: round_content,
            status: if final_tool_calls.is_empty() {
                "completed".into()
            } else {
                "streaming".into()
            },
            created_at: now_ms(),
            tool_calls: if final_tool_calls.is_empty() {
                None
            } else {
                Some(
                    final_tool_calls
                        .iter()
                        .cloned()
                        .map(|mut tool_call| {
                            let args_v = parse_tool_call_arguments(&tool_call.arguments);
                            tool_call.risk_level = state
                                .tools
                                .tool_risk_level_for_invocation(&tool_call.name, &args_v)
                                .or(Some("low".into()));
                            tool_call
                        })
                        .collect(),
                )
            },
            tool_call_id: None,
            error_message: None,
            reasoning: if reasoning_in_messages && !round_reasoning.is_empty() {
                Some(round_reasoning)
            } else {
                None
            },
            thoughts: round_thoughts,
            headline: round_headline,
            raw_content: None,
            agent_id: Some(def.id.clone()),
            agent_name: Some(def.name.clone()),
            agent_trace: None,
            images_base64: None,
            computer_round_screen_rel_path: None,
        });

        if final_tool_calls.is_empty() {
            if let Some(hint) = json_tool_empty_calls_retry_message(
                &json_finish_diag,
                tools_appendix_enabled,
                &finish_reason,
                effective_max_tokens(&provider.settings),
            ) {
                rollback_failed_json_assistant_turn(&mut local_history, &round_message_id);
                local_history.push(ChatMessage {
                    id: new_id("fmt_retry"),
                    role: Role::User,
                    content: hint,
                    status: "done".into(),
                    created_at: now_ms(),
                    tool_calls: None,
                    tool_call_id: None,
                    error_message: None,
                    reasoning: None,
                    thoughts: None,
                    headline: None,
                    raw_content: None,
                    agent_id: None,
                    agent_name: None,
                    agent_trace: None,
                    images_base64: None,
                    computer_round_screen_rel_path: None,
                });
                // Format-only retry: no tool executed; do not consume the tool-round budget.
                if tool_budget.is_exhausted() {
                    let hint = format!(
                        "编排（Supervisor）模式下工具调用累计已达上限（{} 轮，含子 Agent）。建议新开对话。",
                        max_cap
                    );
                    emit(
                        stream,
                        StreamEvent::ToolRoundsExhausted {
                            conversation_id: conversation_id.to_string(),
                            max_rounds: max_cap,
                            message: hint,
                            will_retry_after_compress: provider.settings.context_compression_enabled,
                        },
                    );
                    let _ = crate::context_compression::maybe_compress_after_tool_round_limit(
                        &mut local_history,
                        &provider.settings,
                        provider,
                        conversation_id,
                        stream,
                        cancel.clone(),
                        false,
                    )
                    .await;
                    state.computer_state.mark_cancelled(conversation_id);
                    return Err(anyhow!(
                        "编排（Supervisor）模式下工具调用轮次已达上限（{max_cap}）。请新开对话。"
                    ));
                }
                continue;
            }
            return Ok(AgentRunResult {
                task_id: task.id.clone(),
                agent_id: def.id,
                agent_name: def.name,
                content,
                reasoning: if reasoning_in_messages && !reasoning.is_empty() {
                    Some(reasoning)
                } else {
                    None
                },
            });
        }

        if let Err(err) = validate_envelope_tool_batch(&state.tools, &final_tool_calls) {
            log::warn!("sub-agent tool envelope batch rejected: {err}");
            let hint = json_tool_envelope_batch_retry_message(&err);
            local_history.push(ChatMessage {
                id: new_id("fmt_retry"),
                role: Role::User,
                content: hint,
                status: "done".into(),
                created_at: now_ms(),
                tool_calls: None,
                tool_call_id: None,
                error_message: None,
                reasoning: None,
                thoughts: None,
                headline: None,
                raw_content: None,
                agent_id: None,
                agent_name: None,
                agent_trace: None,
                images_base64: None,
                computer_round_screen_rel_path: None,
            });
            // Format-only retry: no tool executed; do not consume the tool-round budget.
            if tool_budget.is_exhausted() {
                let hint = format!(
                    "编排（Supervisor）模式下工具调用累计已达上限（{} 轮，含子 Agent）。建议新开对话。",
                    max_cap
                );
                emit(
                    stream,
                    StreamEvent::ToolRoundsExhausted {
                        conversation_id: conversation_id.to_string(),
                        max_rounds: max_cap,
                        message: hint,
                        will_retry_after_compress: provider.settings.context_compression_enabled,
                    },
                );
                let _ = crate::context_compression::maybe_compress_after_tool_round_limit(
                    &mut local_history,
                    &provider.settings,
                    provider,
                    conversation_id,
                    stream,
                    cancel.clone(),
                    false,
                )
                .await;
                state.computer_state.mark_cancelled(conversation_id);
                return Err(anyhow!(
                    "编排（Supervisor）模式下工具调用轮次已达上限（{max_cap}）。请新开对话。"
                ));
            }
            continue;
        }

        let mut any_executed = false;
        for tool_call in &final_tool_calls {
            if cancel.is_cancelled() {
                return Err(anyhow!("已停止生成"));
            }

            let args_value = parse_tool_call_arguments(&tool_call.arguments);
            let (mut tool_id, args_value) =
                merge_tool_method_from_qualified_name(&tool_call.name, args_value);
            tool_id = tool_id.trim().to_string();
            let args_value =
                inject_host_task_board_conversation_id(&tool_id, args_value, &sub_task_board_key);
            if tool_id.is_empty() {
                let err = "工具名为空：请检查 <tool_name>（例如 mouse:click_index、composite_action、response）。";
                emit(
                    stream,
                    StreamEvent::ToolCallStatus {
                        message_id: message_id.to_string(),
                        tool_call_id: tool_call.id.clone(),
                        status: "failed".into(),
                        result: None,
                        error: Some(err.to_string()),
                        duration_ms: Some(0),
                    },
                );
                local_history.push(tool_result_msg(&tool_call.id, &format!("ERROR: {err}")));
                any_executed = true;
                continue;
            }

            if tool_id == "response" {
                let message = response_text_from_args(&args_value).unwrap_or("");

                if !message.is_empty() {
                    emit(
                        stream,
                        StreamEvent::Delta {
                            message_id: message_id.to_string(),
                            text: message.to_string(),
                        },
                    );
                }

                emit(
                    stream,
                    StreamEvent::ToolCallStatus {
                        message_id: message_id.to_string(),
                        tool_call_id: tool_call.id.clone(),
                        status: "success".into(),
                        result: Some("已回复用户".into()),
                        error: None,
                        duration_ms: Some(0),
                    },
                );

                if let Some(last) = local_history.last_mut() {
                    if last.id == round_message_id && matches!(last.role, Role::Assistant) {
                        last.content = message.to_string();
                        last.tool_calls = None;
                        last.status = "completed".into();
                    }
                }

                return Ok(AgentRunResult {
                    task_id: task.id.clone(),
                    agent_id: def.id.clone(),
                    agent_name: def.name.clone(),
                    content: message.to_string(),
                    reasoning: if reasoning_in_messages && !reasoning.is_empty() {
                        Some(reasoning)
                    } else {
                        None
                    },
                });
            }

            if !allowed_tools.contains(&tool_id) {
                let err = format!("Agent {} 不允许调用工具: {}", def.id, tool_call.name);
                emit(
                    stream,
                    StreamEvent::ToolCallStatus {
                        message_id: message_id.to_string(),
                        tool_call_id: tool_call.id.clone(),
                        status: "failed".into(),
                        result: None,
                        error: Some(err.clone()),
                        duration_ms: None,
                    },
                );
                local_history.push(tool_result_msg(&tool_call.id, &format!("ERROR: {err}")));
                any_executed = true;
                continue;
            }

            let requires_approval = tool_approval_mode == "manual"
                && state.tools.tool_invocation_needs_approval(&tool_id, &args_value);

            if requires_approval {
                emit(
                    stream,
                    StreamEvent::ToolCallStatus {
                        message_id: message_id.to_string(),
                        tool_call_id: tool_call.id.clone(),
                        status: "pending_approval".into(),
                        result: None,
                        error: None,
                        duration_ms: None,
                    },
                );
                let (approval_tx, approval_rx) = oneshot::channel::<bool>();
                state
                    .approvals
                    .lock()
                    .insert(tool_call.id.clone(), approval_tx);
                let approved = tokio::select! {
                    _ = cancel.cancelled() => {
                        state.approvals.lock().remove(&tool_call.id);
                        false
                    }
                    value = approval_rx => value.unwrap_or(false),
                };
                if !approved {
                    let err = "用户已拒绝该工具调用".to_string();
                    emit(
                        stream,
                        StreamEvent::ToolCallStatus {
                            message_id: message_id.to_string(),
                            tool_call_id: tool_call.id.clone(),
                            status: "rejected".into(),
                            result: None,
                            error: Some(err.clone()),
                            duration_ms: None,
                        },
                    );
                    local_history.push(tool_result_msg(&tool_call.id, &err));
                    any_executed = true;
                    continue;
                }
            }

            emit(
                stream,
                StreamEvent::ToolCallStatus {
                    message_id: message_id.to_string(),
                    tool_call_id: tool_call.id.clone(),
                    status: "running".into(),
                    result: None,
                    error: None,
                    duration_ms: None,
                },
            );
            llm_stats.record_tool_invocation();
            let started = Instant::now();

            let is_terminal = tool_id == "terminal";
            let msg_id_for_stream = message_id.to_string();
            let tc_id_for_stream = tool_call.id.clone();
            let stream_for_terminal = stream.clone();
            let args_for_desktop_log = args_value.clone();

            let exec: Result<(String, bool, Option<String>), anyhow::Error> = if is_terminal {
                let cancel_terminal = cancel.clone();
                let abort_flag = Arc::new(AtomicBool::new(false));
                {
                    let mut m = state.terminal_run_abort.lock();
                    if let Some(old) = m.insert(conversation_id.to_string(), abort_flag.clone()) {
                        old.store(true, Ordering::SeqCst);
                    }
                }
                let cleanup_id = conversation_id.to_string();
                let join = tokio::task::spawn_blocking(move || {
                    run_terminal_command_streaming(
                        args_value,
                        move |output| {
                            let _ = stream_for_terminal.send(StreamEvent::TerminalOutputDelta {
                                message_id: msg_id_for_stream.clone(),
                                tool_call_id: tc_id_for_stream.clone(),
                                output: output.to_string(),
                            });
                        },
                        Some(cancel_terminal),
                        Some(abort_flag),
                    )
                    .map(|r| {
                        let (ok, err_note) = terminal_stream_tool_status(&r);
                        let body = serde_json::json!({
                            "exitCode": r.exit_code,
                            "success": r.success,
                            "timedOut": r.timed_out,
                            "cancelled": r.cancelled,
                            "runAborted": r.run_aborted,
                            "durationMs": r.duration_ms,
                            "stdout": r.stdout,
                            "stderr": r.stderr,
                            "stdoutTruncated": r.stdout_truncated,
                            "stderrTruncated": r.stderr_truncated,
                        })
                        .to_string();
                        (body, ok, err_note)
                    })
                })
                .await;
                state.terminal_run_abort.lock().remove(&cleanup_id);
                join.map_err(|e| anyhow!("终端执行线程异常: {e}"))?
            } else {
                state
                    .tools
                    .invoke(&tool_id, args_value)
                    .map(|out| (out, true, None))
            };

            let duration = started.elapsed().as_millis() as u64;
            match exec {
                Ok((output, ok, err_note)) => {
                    let failed_note = desktop_tool_failure_note(ok, &err_note, &output);
                    state.computer_state.record_desktop_tool_if_applicable(
                        conversation_id,
                        &tool_id,
                        &args_for_desktop_log,
                        failed_note.as_deref(),
                    );
                    if ok && crate::agents::computer::is_desktop_post_delay_tool(tool_id.as_str()) {
                        let delay_ms = crate::agents::computer::post_desktop_action_delay_ms_from_tool_args(
                            &args_for_desktop_log,
                        );
                        log::info!(
                            "desktop post_action sleep {}ms before next capture (tool={})",
                            delay_ms,
                            tool_id
                        );
                        tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                    }
                    let preview = truncate_str(&output, 800);
                    emit(
                        stream,
                        StreamEvent::ToolCallStatus {
                            message_id: message_id.to_string(),
                            tool_call_id: tool_call.id.clone(),
                            status: if ok { "success".into() } else { "failed".into() },
                            result: Some(preview),
                            error: err_note,
                            duration_ms: Some(duration),
                        },
                    );
                    local_history.push(tool_result_msg(&tool_call.id, &output));
                }
                Err(err) => {
                    let err = err.to_string();
                    let err_snip = truncate_str(&err, 400);
                    state.computer_state.record_desktop_tool_if_applicable(
                        conversation_id,
                        &tool_id,
                        &args_for_desktop_log,
                        Some(err_snip.as_str()),
                    );
                    emit(
                        stream,
                        StreamEvent::ToolCallStatus {
                            message_id: message_id.to_string(),
                            tool_call_id: tool_call.id.clone(),
                            status: "failed".into(),
                            result: None,
                            error: Some(err.clone()),
                            duration_ms: Some(duration),
                        },
                    );
                    local_history.push(tool_result_msg(&tool_call.id, &format!("ERROR: {err}")));
                }
            }
            any_executed = true;
        }

        if !any_executed {
            return Ok(AgentRunResult {
                task_id: task.id.clone(),
                agent_id: def.id,
                agent_name: def.name,
                content,
                reasoning: if reasoning_in_messages && !reasoning.is_empty() {
                    Some(reasoning)
                } else {
                    None
                },
            });
        }
        tool_budget.record_tool_cycle();

        if tool_budget.is_exhausted() {
            let hint = format!(
                "编排（Supervisor）模式下工具调用累计已达上限（{} 轮，含子 Agent）。建议新开对话。",
                max_cap
            );
            emit(
                stream,
                StreamEvent::ToolRoundsExhausted {
                    conversation_id: conversation_id.to_string(),
                    max_rounds: max_cap,
                    message: hint,
                    will_retry_after_compress: provider.settings.context_compression_enabled,
                },
            );
            let _ = crate::context_compression::maybe_compress_after_tool_round_limit(
                &mut local_history,
                &provider.settings,
                provider,
                conversation_id,
                stream,
                cancel.clone(),
                false,
            )
            .await;
            state.computer_state.mark_cancelled(conversation_id);
            return Err(anyhow!(
                "编排（Supervisor）模式下工具调用轮次已达上限（{max_cap}）。请新开对话。"
            ));
        }
    }
}

async fn synthesize_final_answer(
    provider: &OpenAIProvider,
    history: &[ChatMessage],
    results: &[AgentRunResult],
    cancel: CancellationToken,
    conversation_id: &str,
    assistant_message_id: &str,
    llm_stats: &mut ConversationLlmStats,
) -> Result<String> {
    let mut report = String::new();
    for result in results {
        report.push_str(&format!(
            "## {} ({})\nTask: {}\n{}\n\n",
            result.agent_name, result.agent_id, result.task_id, result.content
        ));
    }
    let env_context = crate::env_prompt::build_environment_context_full();
    let prompt = format!(
        "{}\n\nYou are the Supervisor. From the sub-agent results below, write the final user-facing answer.\nRequirements: merge duplicates and resolve conflicts; do not state facts that sub-agents did not support; briefly note which agents contributed when helpful.\n\nSub-agent results:\n{}",
        env_context,
        if report.is_empty() {
            "No sub-agent results; answer cautiously from the conversation only.".into()
        } else {
            report
        }
    );
    let dump_lbl = format!("{conversation_id}_{assistant_message_id}_supervisor_synthesize");
    let out = provider
        .chat_once(
            history,
            &[prompt],
            cancel,
            None,
            Some(dump_lbl.as_str()),
        )
        .await?;
    llm_stats.record_llm_round(out.usage.as_ref());
    Ok(out.text)
}

fn emit_agent_content_delta(
    stream: &StreamTx,
    message_id: &str,
    trace: &mut Vec<AgentTrace>,
    def: &AgentDef,
    task: &AgentTask,
    content: String,
) {
    emit_agent_step(
        stream,
        message_id,
        trace,
        AgentTrace {
            id: def.id.clone(),
            name: def.name.clone(),
            role: def.role.clone(),
            status: "running".into(),
            detail: Some(if task.title.is_empty() {
                task.instruction.clone()
            } else {
                task.title.clone()
            }),
            content: Some(content),
        },
    );
}

fn emit_agent_step(
    stream: &StreamTx,
    message_id: &str,
    trace: &mut Vec<AgentTrace>,
    agent: AgentTrace,
) {
    if let Some(existing) = trace.iter_mut().find(|item| item.id == agent.id) {
        *existing = agent.clone();
    } else {
        trace.push(agent.clone());
    }
    emit(
        stream,
        StreamEvent::AgentStep {
            message_id: message_id.to_string(),
            agent,
        },
    );
}

fn parse_agent_tasks(
    raw: &str,
    workers: &[AgentDef],
    limits: &AgentRunLimits,
) -> Option<Vec<AgentTask>> {
    let json_text = extract_json_array(raw)?;
    let mut tasks: Vec<AgentTask> = serde_json::from_str(&json_text).ok()?;
    let allowed: Vec<_> = workers.iter().map(|agent| agent.id.as_str()).collect();
    tasks.retain(|task| allowed.contains(&task.agent_id.as_str()));
    for (idx, task) in tasks.iter_mut().enumerate() {
        if task.id.trim().is_empty() {
            task.id = format!("task_{}", idx + 1);
        }
        if task.title.trim().is_empty() {
            task.title = format!("Sub-task {}", idx + 1);
        }
    }
    tasks.truncate(limits.max_sub_agents);
    if tasks.is_empty() {
        None
    } else {
        Some(tasks)
    }
}

fn extract_json_array(raw: &str) -> Option<String> {
    let start = raw.find('[')?;
    let end = raw.rfind(']')?;
    (start <= end).then(|| raw[start..=end].to_string())
}

fn fallback_agent_tasks(
    state: &AppState,
    history: &[ChatMessage],
    limits: &AgentRunLimits,
) -> Vec<AgentTask> {
    let latest = history
        .iter()
        .rev()
        .find(|message| matches!(message.role, Role::User))
        .map(|message| message.content.clone())
        .unwrap_or_else(|| "Fulfill the user request".into());
    let lower = latest.to_lowercase();
    let workers = state.agents.enabled_workers();
    let preferred = if lower.contains("代码")
        || lower.contains("实现")
        || lower.contains("code")
        || lower.contains("implement")
        || lower.contains("bug")
        || lower.contains("error")
        || lower.contains("rust")
        || lower.contains("vue")
    {
        "coder"
    } else if lower.contains("分析")
        || lower.contains("计算")
        || lower.contains("数据")
        || lower.contains("analy")
        || lower.contains("calculat")
        || lower.contains("data")
    {
        "analyst"
    } else if lower.contains("写")
        || lower.contains("文档")
        || lower.contains("总结")
        || lower.contains("write")
        || lower.contains("doc")
        || lower.contains("summar")
    {
        "writer"
    } else {
        DEFAULT_AGENT_ID
    };
    let agent_id = workers
        .iter()
        .find(|agent| agent.id == preferred)
        .or_else(|| workers.iter().find(|agent| agent.id == DEFAULT_AGENT_ID))
        .or_else(|| workers.first())
        .map(|agent| agent.id.clone())
        .unwrap_or_else(|| DEFAULT_AGENT_ID.into());
    vec![AgentTask {
        id: "task_1".into(),
        agent_id,
        title: "Handle user request".into(),
        instruction: latest,
        depends_on: Vec::new(),
    }]
    .into_iter()
    .take(limits.max_sub_agents)
    .collect()
}

fn agent_roster(agents: &[AgentDef]) -> String {
    agents
        .iter()
        .map(|agent| {
            format!(
                "- id: {}\n  name: {}\n  profile: {:?}\n  description: {}",
                agent.id, agent.name, agent.profile, agent.description
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn resolve_agent_tools(
    agent: &AgentDef,
    session_tools: &[String],
    tools: &ToolRegistry,
) -> Vec<String> {
    let mut names = if agent.access_policy.allow_tools.is_empty() {
        session_tools.to_vec()
    } else {
        agent.access_policy.allow_tools.clone()
    };
    names.retain(|name| {
        tools.get_def(name).is_some() && !agent.access_policy.deny_tools.contains(name)
    });

    if !agent.access_policy.deny_tools.iter().any(|d| d == "response")
        && tools.get_def("response").is_some()
        && !names.contains(&"response".into())
    {
        names.push("response".into());
    }

    names.sort();
    names.dedup();
    names
}

fn tool_result_msg(tool_call_id: &str, content: &str) -> ChatMessage {
    ChatMessage {
        id: new_id("tool"),
        role: Role::Tool,
        content: content.to_string(),
        status: "completed".into(),
        created_at: now_ms(),
        tool_calls: None,
        tool_call_id: Some(tool_call_id.to_string()),
        error_message: None,
        reasoning: None,
        thoughts: None,
        headline: None,
        raw_content: None,
        agent_id: None,
        agent_name: None,
        agent_trace: None,
        images_base64: None,
        computer_round_screen_rel_path: None,
    }
}

fn truncate_str(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n).collect::<String>())
    }
}

/// When the tool run did not succeed, short text for `[Recent desktop tool calls]` (`FAILED: …`).
fn desktop_tool_failure_note(ok: bool, err_note: &Option<String>, tool_output: &str) -> Option<String> {
    if ok {
        return None;
    }
    let mut parts: Vec<String> = Vec::new();
    if let Some(e) = err_note.as_ref().map(|s| s.trim()).filter(|s| !s.is_empty()) {
        parts.push(e.to_string());
    }
    let out = truncate_str(tool_output, 200);
    if !out.trim().is_empty() {
        parts.push(out);
    }
    if parts.is_empty() {
        Some("failed".into())
    } else {
        Some(parts.join(" | "))
    }
}

/// Remove structured tool JSON (or legacy XML) from assistant `content` for the user-visible bubble.
fn extract_user_visible_content(raw: &str) -> String {
    let trimmed = raw.trim();
    if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
        if let Value::Object(ref obj) = v {
            if obj.get("tool_name").and_then(|x| x.as_str()) == Some("response") {
                return obj
                    .get("tool_args")
                    .and_then(|a| a.get("text"))
                    .and_then(|t| t.as_str())
                    .unwrap_or("")
                    .to_string();
            }
            return String::new();
        }
    }
    extract_user_visible_content_xml_legacy(raw)
}

fn extract_user_visible_content_xml_legacy(raw: &str) -> String {
    let mut result = String::with_capacity(raw.len());
    let mut remaining = raw;

    while let Some(start) = remaining.find("<response>") {
        if start > 0 {
            result.push_str(&remaining[..start]);
        }
        if let Some(end) = remaining[start..].find("</response>") {
            remaining = &remaining[start + end + 11..];
        } else {
            result.push_str(&remaining[start..]);
            break;
        }
    }

    if !remaining.is_empty() {
        result.push_str(remaining);
    }

    result.trim().to_string()
}

fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn new_id(prefix: &str) -> String {
    format!("{prefix}_{}", uuid::Uuid::new_v4().simple())
}

#[cfg(test)]
mod extract_user_visible_tests {
    use super::extract_user_visible_content;

    #[test]
    fn json_response_tool_text_visible() {
        let j = r#"{"thoughts":"t","headline":"h","tool_name":"response","tool_args":{"text":"Hello user"}}"#;
        assert_eq!(extract_user_visible_content(j), "Hello user");
    }

    #[test]
    fn json_non_response_hidden() {
        let j = r#"{"tool_name":"wait","tool_args":{"seconds":"1"}}"#;
        assert_eq!(extract_user_visible_content(j), "");
    }

    #[test]
    fn legacy_xml_only_response_still_stripped() {
        assert_eq!(
            extract_user_visible_content("<response><tool_name>x</tool_name></response>"),
            ""
        );
    }

    #[test]
    fn legacy_prose_outside_response_kept() {
        assert_eq!(
            extract_user_visible_content("Hi<response></response>"),
            "Hi"
        );
    }
}

#[cfg(test)]
mod sub_agent_task_board_key_tests {
    use super::sub_agent_task_board_store_key;

    #[test]
    fn key_is_not_raw_conversation_id() {
        let main = "conv-1";
        let k = sub_agent_task_board_store_key(main, "task_a");
        assert_ne!(k, main);
        assert!(k.contains("ptr_sub_agent"), "{k:?}");
        assert!(k.ends_with("task_a"), "{k:?}");
    }

    #[test]
    fn distinct_supervisor_task_ids_differ() {
        assert_ne!(
            sub_agent_task_board_store_key("c", "t1"),
            sub_agent_task_board_store_key("c", "t2")
        );
    }
}
