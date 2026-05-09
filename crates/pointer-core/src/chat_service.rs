use crate::agents::{
    agent_requires_workspace, register_builtin_agents, AgentDef, AgentOrchestrator, AgentRegistry,
    AgentRunLimits, AgentRunResult, AgentTask, DEFAULT_AGENT_ID, SUPERVISOR_AGENT_ID,
    AGENT_MODE_SUPERVISOR,
};
use crate::models::{AgentTrace, ChatMessage, Role, StreamEvent, ToolCall};
use crate::provider::{OpenAIProvider, ProviderEvent};
use crate::skills::SkillRegistry;
use crate::storage;
use crate::tools::parse_tool_call_arguments;
use crate::tools::terminal::run_terminal_command_streaming;
use crate::tools::ToolRegistry;
use anyhow::{anyhow, Result};
use chrono::Local;
use parking_lot::Mutex;
use std::collections::{HashMap, HashSet, VecDeque};
use std::env;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
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
    pub cancels: Mutex<HashMap<String, CancellationToken>>,
    pub approvals: Mutex<HashMap<String, oneshot::Sender<bool>>>,
}

impl AppState {
    pub fn new() -> Self {
        let tools = Arc::new(ToolRegistry::new());
        crate::tools::builtin::register_all(&tools);
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
        Self {
            tools,
            skills,
            agents,
            cancels: Mutex::new(HashMap::new()),
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

pub type StreamTx = mpsc::UnboundedSender<StreamEvent>;

fn emit(tx: &StreamTx, ev: StreamEvent) {
    let _ = tx.send(ev);
}

fn build_env_context() -> String {
    let os = env::consts::OS;
    let os_label = match os {
        "macos" => "macOS",
        "windows" => "Windows",
        "linux" => "Linux",
        other => other,
    };
    let locale = env::var("LANG")
        .ok()
        .and_then(|lang| {
            let lang = lang.split('.').next().unwrap_or(&lang);
            if lang.starts_with("zh") {
                Some("中文")
            } else if lang.starts_with("en") {
                Some("English")
            } else {
                None
            }
        })
        .unwrap_or("未知");
    let now = Local::now().format("%Y-%m-%d %H:%M:%S %Z");

    format!(
        "当前环境信息：\n- 操作系统：{}\n- 系统语言：{}\n- 当前时间：{}",
        os_label, locale, now
    )
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
        emit(
            &stream,
            StreamEvent::Error {
                message_id: None,
                message: err.to_string(),
            },
        );
    }
    let max_tr = storage::load_settings()
        .map(|s| s.max_tool_rounds)
        .unwrap_or(100);
    let single_total = tool_rounds_used_single_start.saturating_add(consumed_single);
    let supervisor_total =
        tool_rounds_used_supervisor_start.saturating_add(consumed_supervisor);
    emit(
        &stream,
        StreamEvent::Done {
            conversation_id,
            tool_rounds_used_total: Some(single_total),
            tool_rounds_used_supervisor_total: Some(supervisor_total),
            max_tool_rounds: Some(max_tr),
        },
    );
    result
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
    if effective_agent_mode != AGENT_MODE_SUPERVISOR {
        if let Some(exec) = state.agents.get(&agent_plan.lead_agent_id) {
            let def = exec.def();
            if agent_requires_workspace(&def) {
                let w = settings.workspace_root.trim();
                if w.is_empty() {
                    return Err(anyhow!(
                        "当前编码智能体需要工作区目录，请先在界面选择项目文件夹。"
                    ));
                }
                if !PathBuf::from(w).is_dir() {
                    return Err(anyhow!("工作区目录无效，请重新选择。"));
                }
            }
        }
    }
    let provider = OpenAIProvider::new(settings.clone(), api_key);

    crate::context_compression::maybe_compress_history(
        history,
        &settings,
        &provider,
        conversation_id,
        &stream,
        cancel.clone(),
    )
    .await;

    let max_cap = settings.max_tool_rounds.clamp(1, 10_000);

    if agent_plan.mode == AGENT_MODE_SUPERVISOR {
        if tool_rounds_used_supervisor_start >= max_cap {
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

        let tools_json = state.tools.openai_tools(&agent_plan.allowed_tool_names);
        let (tx, mut rx) = mpsc::channel::<ProviderEvent>(64);
        let prov = OpenAIProvider::new(provider.settings.clone(), provider.api_key.clone());
        let history_clone = history.clone();
        let mut prompts_with_env = vec![build_env_context()];
        prompts_with_env.extend(agent_plan.system_prompts.clone());
        prompts_with_env.extend(state.tools.prompt_context(&agent_plan.allowed_tool_names));
        let prompts_clone = prompts_with_env;
        let cancel_clone = cancel.clone();
        let tools_clone = tools_json.clone();
        let send_handle = tokio::spawn(async move {
            prov.stream_chat(
                &history_clone,
                &prompts_clone,
                tools_clone,
                tx,
                cancel_clone,
            )
            .await
        });

        let mut content_buf = String::new();
        let mut reasoning_buf = String::new();
        let mut final_tool_calls: Vec<ToolCall> = Vec::new();
        let mut finish_reason = String::from("stop");

        while let Some(ev) = rx.recv().await {
            match ev {
                ProviderEvent::ContentDelta(t) => {
                    content_buf.push_str(&t);
                    emit(
                        &stream,
                        StreamEvent::Delta {
                            message_id: assistant_id.clone(),
                            text: t,
                        },
                    );
                }
                ProviderEvent::ReasoningDelta(t) => {
                    reasoning_buf.push_str(&t);
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
                            .get_def(&name)
                            .map(|d| d.risk_level)
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
                ProviderEvent::Finish { reason, tool_calls } => {
                    finish_reason = reason;
                    final_tool_calls = tool_calls;
                }
            }
        }

        match send_handle.await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
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
                    },
                );
                tool_budget.sync_out(consumed_single);
                return Err(e);
            }
            Err(e) => {
                tool_budget.sync_out(consumed_single);
                return Err(anyhow!("任务异常：{e}"));
            }
        }

        let assistant_msg = ChatMessage {
            id: assistant_id.clone(),
            role: Role::Assistant,
            content: content_buf.clone(),
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
                            t.risk_level = state
                                .tools
                                .get_def(&t.name)
                                .map(|d| d.risk_level)
                                .or(Some("low".into()));
                            t
                        })
                        .collect(),
                )
            },
            tool_call_id: None,
            error_message: None,
            reasoning: if reasoning_buf.is_empty() {
                None
            } else {
                Some(reasoning_buf)
            },
            agent_id: Some(agent_plan.lead_agent_id.clone()),
            agent_name: Some(agent_plan.lead_agent_name.clone()),
            agent_trace: if agent_trace.is_empty() {
                None
            } else {
                Some(agent_trace.clone())
            },
        };
        history.push(assistant_msg);
        emit(
            &stream,
            StreamEvent::MessageEnd {
                message_id: assistant_id.clone(),
            },
        );

        if final_tool_calls.is_empty() {
            let _ = finish_reason;
            tool_budget.sync_out(consumed_single);
            return Ok(());
        }

        let mut any_executed = false;
        for tc in &final_tool_calls {
            if cancel.is_cancelled() {
                tool_budget.sync_out(consumed_single);
                return Err(anyhow!("已停止生成"));
            }
            let def = state.tools.get_def(&tc.name);
            let requires_approval = tool_approval_mode == "manual"
                && def.as_ref().map(|d| d.requires_approval).unwrap_or(false);

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
            let started = Instant::now();
            let args_value = parse_tool_call_arguments(&tc.arguments);

            let is_terminal = tc.name == "terminal";
            let msg_id_for_stream = assistant_id.clone();
            let tc_id_for_stream = tc.id.clone();
            let stream_for_terminal = stream.clone();

            let exec = if is_terminal {
                tokio::task::spawn_blocking(move || {
                    run_terminal_command_streaming(args_value, move |output| {
                        let _ = stream_for_terminal.send(StreamEvent::TerminalOutputDelta {
                            message_id: msg_id_for_stream.clone(),
                            tool_call_id: tc_id_for_stream.clone(),
                            output: output.to_string(),
                        });
                    })
                    .map(|r| {
                        serde_json::json!({
                            "exitCode": r.exit_code,
                            "success": r.success,
                            "timedOut": r.timed_out,
                            "durationMs": r.duration_ms,
                            "stdout": r.stdout,
                            "stderr": r.stderr,
                            "stdoutTruncated": r.stdout_truncated,
                            "stderrTruncated": r.stderr_truncated,
                        })
                        .to_string()
                    })
                })
                .await
                .map_err(|e| anyhow!("终端执行线程异常: {e}"))?
            } else {
                state.tools.invoke(&tc.name, args_value)
            };

            let duration = started.elapsed().as_millis() as u64;
            match exec {
                Ok(out) => {
                    let preview = truncate_str(&out, 800);
                    emit(
                        &stream,
                        StreamEvent::ToolCallStatus {
                            message_id: assistant_id.clone(),
                            tool_call_id: tc.id.clone(),
                            status: "success".into(),
                            result: Some(preview),
                            error: None,
                            duration_ms: Some(duration),
                        },
                    );
                    history.push(tool_result_msg(&tc.id, &out));
                }
                Err(e) => {
                    let err = e.to_string();
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

    let env_context = build_env_context();
    let tasks = match plan_agent_tasks(
        &provider,
        &state,
        history,
        &limits,
        cancel.clone(),
        &env_context,
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
            let mut pre = String::from("\n\n【前置任务输出摘要】\n");
            for d in &task.depends_on {
                if let Some(r) = results_by_id.get(d) {
                    pre.push_str(&format!(
                        "--- 任务 {} ---\n{}\n",
                        d,
                        truncate_str(&r.content, 2000)
                    ));
                }
            }
            task_run.instruction = format!("{pre}\n\n【当前任务说明】\n{}", task.instruction);
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

    let final_answer = synthesize_final_answer(&provider, history, &results, cancel).await?;
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
        content: final_answer,
        status: "completed".into(),
        created_at: now_ms(),
        tool_calls: None,
        tool_call_id: None,
        error_message: None,
        reasoning: None,
        agent_id: Some("supervisor".into()),
        agent_name: Some(sup_name),
        agent_trace: Some(agent_trace),
    });
    emit(
        &stream,
        StreamEvent::MessageEnd {
            message_id: assistant_id,
        },
    );
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
) -> Result<Vec<AgentTask>> {
    let workers = state.agents.enabled_workers();
    let roster = agent_roster(&workers);
    let prompt = format!(
        "{}\n\n你是 Supervisor。请把用户最新请求拆解为最多 {} 个子 Agent 任务。\n\n规划原则：同一仓库内的代码实现、调试、重构应**优先合并为一条** agentId 为 coder 的任务（instruction 写清全部要求）；仅当需要 reviewer 独立把关、或存在明显非代码子任务时再拆多条。若拆成多条且有先后依赖，使用 dependsOn 填入前置任务的 id 字符串数组。\n\n可用 Agent：\n{}\n\n只返回 JSON 数组，不要 Markdown。数组元素格式：{{\"id\":\"task_1\",\"agentId\":\"coder\",\"title\":\"简短标题\",\"instruction\":\"给该 Agent 的完整任务说明\",\"dependsOn\":[]}}。agentId 必须来自可用 Agent。常规问答使用 default。涉及写代码、读仓库、跑测试优先 coder。",
        env_context,
        limits.max_sub_agents,
        roster
    );
    let raw = provider
        .chat_once(history, &[prompt], Vec::new(), cancel, None)
        .await?;
    parse_agent_tasks(&raw, &workers, limits)
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
) -> Result<AgentRunResult> {
    let agent = state
        .agents
        .get(&task.agent_id)
        .or_else(|| state.agents.get(DEFAULT_AGENT_ID))
        .ok_or_else(|| anyhow!("未找到 Agent: {}", task.agent_id))?;
    let def = agent.def();
    let mut skill_ids = if enabled_skill_ids.is_empty() {
        def.default_skill_ids.clone()
    } else {
        enabled_skill_ids.to_vec()
    };
    if !def.access_policy.allow_skills.is_empty() {
        skill_ids.retain(|id| def.access_policy.allow_skills.contains(id));
    }
    skill_ids.retain(|id| !def.access_policy.deny_skills.contains(id));
    skill_ids.sort();
    skill_ids.dedup();

    let (skill_prompts, session_tools) = state.skills.progressive_context(&skill_ids);
    let allowed_tools = resolve_agent_tools(&def, &session_tools, &state.tools);
    let env_context = build_env_context();
    let mut prompts = vec![env_context, format!(
        "当前子 Agent：{} ({})\nprofile: {:?}\ndescription: {}\n\n{}\n\n你只负责完成 Supervisor 通过下一条「用户」消息下发的子任务。该用户消息仅为任务说明（可含前置任务输出摘要），**不含**主会话聊天记录。输出应包含结论、关键依据、风险或未完成项。\n可用工具名：{}",
        def.name,
        def.id,
        def.profile,
        def.description,
        agent.system_prompt(),
        if allowed_tools.is_empty() { "无".into() } else { allowed_tools.join(", ") }
    )];
    prompts.extend(skill_prompts);
    prompts.extend(state.tools.prompt_context(&allowed_tools));

    let tools_json = state.tools.openai_tools(&allowed_tools);
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
        agent_id: None,
        agent_name: None,
        agent_trace: None,
    }];
    let mut content = String::new();
    let mut reasoning = String::new();

    loop {
        if cancel.is_cancelled() {
            return Err(anyhow!("已停止生成"));
        }

        if tool_budget.remaining() == 0 {
            return Err(anyhow!(
                "编排（Supervisor）模式下工具调用轮次已达上限（{}）。请新开对话。",
                max_cap
            ));
        }

        let (tx, mut rx) = mpsc::channel::<ProviderEvent>(64);
        let prov = OpenAIProvider::new(provider.settings.clone(), provider.api_key.clone());
        let history_clone = local_history.clone();
        let prompts_clone = prompts.clone();
        let tools_clone = tools_json.clone();
        let cancel_clone = cancel.clone();
        let handle = tokio::spawn(async move {
            prov.stream_chat(
                &history_clone,
                &prompts_clone,
                tools_clone,
                tx,
                cancel_clone,
            )
            .await
        });

        let round_message_id = new_id("agent_msg");
        let mut round_content = String::new();
        let mut round_reasoning = String::new();
        let mut final_tool_calls: Vec<ToolCall> = Vec::new();

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
                    round_reasoning.push_str(&delta);
                    reasoning.push_str(&delta);
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
                                    .get_def(&name)
                                    .map(|tool| tool.risk_level)
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
                ProviderEvent::Finish { tool_calls, .. } => {
                    final_tool_calls = tool_calls;
                }
            }
        }

        match handle.await {
            Ok(Ok(())) => {}
            Ok(Err(err)) => return Err(err),
            Err(err) => return Err(anyhow!("子 Agent 任务异常：{err}")),
        }

        local_history.push(ChatMessage {
            id: round_message_id,
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
                            tool_call.risk_level = state
                                .tools
                                .get_def(&tool_call.name)
                                .map(|tool| tool.risk_level)
                                .or(Some("low".into()));
                            tool_call
                        })
                        .collect(),
                )
            },
            tool_call_id: None,
            error_message: None,
            reasoning: if round_reasoning.is_empty() {
                None
            } else {
                Some(round_reasoning)
            },
            agent_id: Some(def.id.clone()),
            agent_name: Some(def.name.clone()),
            agent_trace: None,
        });

        if final_tool_calls.is_empty() {
            return Ok(AgentRunResult {
                task_id: task.id.clone(),
                agent_id: def.id,
                agent_name: def.name,
                content,
                reasoning: if reasoning.is_empty() {
                    None
                } else {
                    Some(reasoning)
                },
            });
        }

        let mut any_executed = false;
        for tool_call in &final_tool_calls {
            if cancel.is_cancelled() {
                return Err(anyhow!("已停止生成"));
            }
            if !allowed_tools.contains(&tool_call.name) {
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

            let tool_def = state.tools.get_def(&tool_call.name);
            let requires_approval = tool_approval_mode == "manual"
                && tool_def
                    .as_ref()
                    .map(|tool| tool.requires_approval)
                    .unwrap_or(false);

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
            let started = Instant::now();
            let args_value = parse_tool_call_arguments(&tool_call.arguments);

            let is_terminal = tool_call.name == "terminal";
            let msg_id_for_stream = message_id.to_string();
            let tc_id_for_stream = tool_call.id.clone();
            let stream_for_terminal = stream.clone();

            let exec = if is_terminal {
                tokio::task::spawn_blocking(move || {
                    run_terminal_command_streaming(args_value, move |output| {
                        let _ = stream_for_terminal.send(StreamEvent::TerminalOutputDelta {
                            message_id: msg_id_for_stream.clone(),
                            tool_call_id: tc_id_for_stream.clone(),
                            output: output.to_string(),
                        });
                    })
                    .map(|r| {
                        serde_json::json!({
                            "exitCode": r.exit_code,
                            "success": r.success,
                            "timedOut": r.timed_out,
                            "durationMs": r.duration_ms,
                            "stdout": r.stdout,
                            "stderr": r.stderr,
                            "stdoutTruncated": r.stdout_truncated,
                            "stderrTruncated": r.stderr_truncated,
                        })
                        .to_string()
                    })
                })
                .await
                .map_err(|e| anyhow!("终端执行线程异常: {e}"))?
            } else {
                state.tools.invoke(&tool_call.name, args_value)
            };

            let duration = started.elapsed().as_millis() as u64;
            match exec {
                Ok(output) => {
                    emit(
                        stream,
                        StreamEvent::ToolCallStatus {
                            message_id: message_id.to_string(),
                            tool_call_id: tool_call.id.clone(),
                            status: "success".into(),
                            result: Some(truncate_str(&output, 800)),
                            error: None,
                            duration_ms: Some(duration),
                        },
                    );
                    local_history.push(tool_result_msg(&tool_call.id, &output));
                }
                Err(err) => {
                    let err = err.to_string();
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
                reasoning: if reasoning.is_empty() {
                    None
                } else {
                    Some(reasoning)
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
) -> Result<String> {
    let mut report = String::new();
    for result in results {
        report.push_str(&format!(
            "## {} ({})\n任务: {}\n{}\n\n",
            result.agent_name, result.agent_id, result.task_id, result.content
        ));
    }
    let env_context = build_env_context();
    let prompt = format!(
        "{}\n\n你是 Supervisor。基于以下子 Agent 独立执行结果，面向用户输出最终答案。\n要求：整合重复内容，解决冲突；不要编造子 Agent 未提供的事实；必要时简要说明参与的 Agent。\n\n子 Agent 结果：\n{}",
        env_context,
        if report.is_empty() { "无可用子 Agent 结果，请基于对话直接给出谨慎答复。".into() } else { report }
    );
    provider
        .chat_once(history, &[prompt], Vec::new(), cancel, None)
        .await
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
            task.title = format!("子任务 {}", idx + 1);
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
        .unwrap_or_else(|| "完成用户请求".into());
    let lower = latest.to_lowercase();
    let workers = state.agents.enabled_workers();
    let preferred = if lower.contains("代码")
        || lower.contains("实现")
        || lower.contains("bug")
        || lower.contains("error")
        || lower.contains("rust")
        || lower.contains("vue")
    {
        "coder"
    } else if lower.contains("分析") || lower.contains("计算") || lower.contains("数据") {
        "analyst"
    } else if lower.contains("写") || lower.contains("文档") || lower.contains("总结") {
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
        title: "处理用户请求".into(),
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
        agent_id: None,
        agent_name: None,
        agent_trace: None,
    }
}

fn truncate_str(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n).collect::<String>())
    }
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
