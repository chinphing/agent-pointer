use crate::models::{ChatMessage, Role, StreamEvent, ToolCall};
use crate::provider::{OpenAIProvider, ProviderEvent};
use crate::skills::SkillRegistry;
use crate::storage;
use crate::tools::ToolRegistry;
use anyhow::{anyhow, Result};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

const MAX_TOOL_ROUNDS: usize = 6;

pub struct AppState {
    pub tools: Arc<ToolRegistry>,
    pub skills: Arc<SkillRegistry>,
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
        Self {
            tools,
            skills,
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

pub async fn run_chat(
    stream: StreamTx,
    state: Arc<AppState>,
    conversation_id: String,
    mut history: Vec<ChatMessage>,
    enabled_skill_ids: Vec<String>,
) -> Result<()> {
    let cancel = CancellationToken::new();
    state
        .cancels
        .lock()
        .insert(conversation_id.clone(), cancel.clone());

    let result = run_chat_inner(
        stream.clone(),
        state.clone(),
        &conversation_id,
        &mut history,
        &enabled_skill_ids,
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
    emit(&stream, StreamEvent::Done { conversation_id });
    result
}

async fn run_chat_inner(
    stream: StreamTx,
    state: Arc<AppState>,
    conversation_id: &str,
    history: &mut Vec<ChatMessage>,
    enabled_skill_ids: &[String],
    cancel: CancellationToken,
) -> Result<()> {
    let mut settings = storage::load_settings()?;
    let api_key = storage::load_api_key()?
        .ok_or_else(|| anyhow!("尚未配置 API Key，请先在设置中保存密钥"))?;
    settings.api_key = api_key.clone();
    let tool_approval_mode = settings.tool_approval_mode.clone();

    let (skill_prompts, allowed_tool_names) = state.skills.progressive_context(enabled_skill_ids);
    let provider = OpenAIProvider::new(settings.clone(), api_key);

    for round in 0..MAX_TOOL_ROUNDS {
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

        let tools_json = state.tools.openai_tools(&allowed_tool_names);
        let (tx, mut rx) = mpsc::channel::<ProviderEvent>(64);
        let prov = OpenAIProvider::new(provider.settings.clone(), provider.api_key.clone());
        let history_clone = history.clone();
        let prompts_clone = skill_prompts.clone();
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
                return Err(e);
            }
            Err(e) => return Err(anyhow!("任务异常：{e}")),
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
            return Ok(());
        }

        let mut any_executed = false;
        for tc in &final_tool_calls {
            if cancel.is_cancelled() {
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
            let args_value: serde_json::Value =
                serde_json::from_str(&tc.arguments).unwrap_or(serde_json::Value::Null);
            let exec = state.tools.invoke(&tc.name, args_value);
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
            return Ok(());
        }
        if round + 1 >= MAX_TOOL_ROUNDS {
            return Err(anyhow!("已达到最大工具调用轮次 ({MAX_TOOL_ROUNDS})"));
        }
    }

    Ok(())
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
