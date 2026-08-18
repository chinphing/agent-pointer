use crate::json_tool_caller::JsonToolFinishDiagnostics;
use crate::llm_token_stats::LlmUsageSnapshot;
use crate::models::{ChatMessage, ModelSettings, SystemPromptSections, ToolCall};
use anyhow::{anyhow, Result};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::Write;
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

static LLM_USER_AGENT: OnceLock<String> = OnceLock::new();

/// Configure the host-specific User-Agent used by OpenAI-compatible LLM requests.
///
/// Desktop registers `pointer-app` during startup. Server hosts intentionally
/// leave this unset and retain reqwest's default behavior.
pub fn set_llm_user_agent(value: &str) -> Result<()> {
    let value = value.trim();
    if value.is_empty() {
        return Err(anyhow!("LLM User-Agent must not be empty"));
    }
    reqwest::header::HeaderValue::from_str(value)
        .map_err(|error| anyhow!("invalid LLM User-Agent: {error}"))?;
    if let Some(existing) = LLM_USER_AGENT.get() {
        if existing == value {
            return Ok(());
        }
        return Err(anyhow!("LLM User-Agent already configured as '{existing}'"));
    }
    LLM_USER_AGENT
        .set(value.to_string())
        .map_err(|_| anyhow!("failed to configure LLM User-Agent"))?;
    log::info!("provider: configured LLM User-Agent={value}");
    Ok(())
}

fn llm_http_client(timeout: Duration) -> Result<reqwest::Client> {
    let builder = reqwest::Client::builder().timeout(timeout);
    let builder = if let Some(user_agent) = LLM_USER_AGENT.get() {
        builder.user_agent(user_agent)
    } else {
        builder
    };
    Ok(builder.build()?)
}

#[derive(Debug, Clone)]
pub enum ProviderEvent {
    ContentDelta(String),
    ReasoningDelta(String),
    ToolCallStart {
        index: u32,
        id: String,
        name: String,
    },
    ToolCallArgsDelta {
        index: u32,
        tool_call_id: String,
        args: String,
    },
    /// 正文 `content` 中已解析出完整 JSON 工具信封（与 [`Finish`] 使用相同稳定 `tool_calls[].id`）。
    JsonToolStreamingReady {
        tool_calls: Vec<ToolCall>,
        thoughts: Option<String>,
        headline: Option<String>,
    },
    /// 流式阶段：partial JSON 修复后可读出的 `thoughts` / `headline` / `tool_name` / `response` 的 `tool_args.text`。
    AssistantJsonPartial {
        thoughts: Option<String>,
        headline: Option<String>,
        tool_name: Option<String>,
        response_text: Option<String>,
    },
    Finish {
        reason: String,
        tool_calls: Vec<ToolCall>,
        json: JsonToolFinishDiagnostics,
        /// From JSON `thoughts` in the completed envelope (if any).
        thoughts: Option<String>,
        /// From JSON `headline` in the completed envelope (if any).
        headline: Option<String>,
        /// From final stream chunk `usage` when `stream_options.include_usage` is supported.
        usage: Option<LlmUsageSnapshot>,
        /// Model id sent on the chat/completions request (token reporting source of truth).
        model: String,
    },
}

/// chat/completions 请求**不**携带 `tools` / `tool_choice`（部分网关拒绝空 `tools: []`）。
/// 本应用默认使用 provider 原生 `tools` / `tool_calls`；当工具列表为空时不发送 `tools`。
///
/// 扩展参数（结构化 thinking / Hermes `extraBody`）序列化后一律展平到请求体根级。
fn skip_extra_body(v: &Option<Value>) -> bool {
    match v {
        None => true,
        Some(Value::Null) => true,
        Some(Value::Object(o)) if o.is_empty() => true,
        _ => false,
    }
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: Vec<Value>,
    stream: bool,
    temperature: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "stream_options")]
    stream_options: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<&'a str>,
    #[serde(skip_serializing_if = "skip_extra_body", rename = "extra_body")]
    extra_body: Option<Value>,
}

fn chat_request_wire_json(req: &ChatRequest<'_>, settings: &ModelSettings) -> Value {
    let body = serde_json::to_value(req).expect("ChatRequest serializes");
    let wire = crate::models::flatten_chat_extra_body_on_wire(body, settings);
    log_openai_compat_wire_debug(&wire);
    wire
}

/// Final OpenAI-compatible body after strategy flatten (no messages / tools / keys).
fn log_openai_compat_wire_debug(wire: &Value) {
    if !log::log_enabled!(log::Level::Debug) {
        return;
    }
    let Some(obj) = wire.as_object() else {
        log::debug!("openai_compat_request {wire}");
        return;
    };
    let mut out = serde_json::Map::new();
    for (k, v) in obj {
        match k.as_str() {
            "messages" => {
                let n = v.as_array().map(|a| a.len()).unwrap_or(0);
                out.insert(k.clone(), json!({ "omitted": n }));
            }
            "tools" => {
                let n = v.as_array().map(|a| a.len()).unwrap_or(0);
                out.insert(k.clone(), json!({ "omitted": n }));
            }
            _ => {
                out.insert(k.clone(), v.clone());
            }
        }
    }
    log::debug!("openai_compat_request {}", Value::Object(out));
}

#[derive(Deserialize, Debug, Clone)]
struct StreamUsage {
    #[serde(default)]
    prompt_tokens: Option<u32>,
    #[serde(default)]
    completion_tokens: Option<u32>,
    #[serde(default)]
    total_tokens: Option<u32>,
    /// Legacy / DashScope-native path for some models (part of prompt/input tokens).
    #[serde(default)]
    cached_tokens: Option<u32>,
    #[serde(default)]
    prompt_tokens_details: Option<PromptTokensDetails>,
    #[serde(default)]
    completion_tokens_details: Option<CompletionTokensDetails>,
}

#[derive(Deserialize, Debug, Clone, Default)]
struct PromptTokensDetails {
    #[serde(default)]
    cached_tokens: Option<u32>,
}

#[derive(Deserialize, Debug, Clone, Default)]
struct CompletionTokensDetails {
    #[serde(default)]
    reasoning_tokens: Option<u32>,
}

#[derive(Deserialize, Debug)]
struct StreamChunk {
    #[serde(default)]
    choices: Vec<StreamChoice>,
    #[serde(default)]
    usage: Option<StreamUsage>,
}
#[derive(Deserialize, Debug)]
struct StreamChoice {
    #[serde(default)]
    delta: StreamDelta,
    #[serde(default)]
    finish_reason: Option<String>,
}
#[derive(Deserialize, Debug)]
struct ChatOnceApiResponse {
    #[serde(default)]
    choices: Vec<ChatChoice>,
    #[serde(default)]
    usage: Option<StreamUsage>,
}
#[derive(Deserialize, Debug)]
struct ChatChoice {
    message: ChatResponseMessage,
    #[serde(default)]
    finish_reason: Option<String>,
}
#[derive(Deserialize, Debug, Default)]
struct ChatResponseMessage {
    #[serde(default)]
    content: Option<String>,
    /// DeepSeek / many OpenAI-compat servers.
    #[serde(default)]
    reasoning_content: Option<String>,
    /// vLLM / some Qwen thinking builds use `reasoning` instead.
    #[serde(default)]
    reasoning: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<ChatApiToolCall>>,
}

impl ChatResponseMessage {
    fn reasoning_text(&self) -> Option<&str> {
        coalesce_reasoning(self.reasoning_content.as_deref(), self.reasoning.as_deref())
    }
}
#[derive(Deserialize, Debug, Default)]
struct ChatApiToolCall {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    function: Option<ChatApiToolFn>,
}
#[derive(Deserialize, Debug, Default)]
struct ChatApiToolFn {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}
#[derive(Deserialize, Debug, Default)]
struct StreamDelta {
    #[serde(default)]
    content: Option<String>,
    /// DeepSeek / many OpenAI-compat servers.
    #[serde(default)]
    reasoning_content: Option<String>,
    /// vLLM / some Qwen thinking builds use `reasoning` instead.
    #[serde(default)]
    reasoning: Option<String>,
    /// Absorbed from provider SSE; native `tool_calls` are unused (JSON-in-content only). Kept for serde + forward-compat.
    #[serde(default)]
    #[allow(dead_code)]
    tool_calls: Option<Vec<StreamToolCall>>,
}

impl StreamDelta {
    fn reasoning_text(&self) -> Option<&str> {
        coalesce_reasoning(self.reasoning_content.as_deref(), self.reasoning.as_deref())
    }
}

/// Prefer canonical `reasoning_content`; fall back to `reasoning` (vLLM/Qwen).
fn coalesce_reasoning<'a>(
    reasoning_content: Option<&'a str>,
    reasoning: Option<&'a str>,
) -> Option<&'a str> {
    if let Some(s) = reasoning_content {
        if !s.is_empty() {
            return Some(s);
        }
    }
    reasoning.filter(|s| !s.is_empty())
}
/// Native streaming `tool_calls` shape (ignored: we use JSON-in-content only). Kept for serde + forward-compat.
#[allow(dead_code)]
#[derive(Deserialize, Debug)]
struct StreamToolCall {
    index: u32,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    function: Option<StreamFn>,
}
#[allow(dead_code)]
#[derive(Deserialize, Debug)]
struct StreamFn {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

#[derive(Clone)]
pub struct OpenAIProvider {
    pub settings: ModelSettings,
    pub api_key: String,
    pub trace: Option<LlmTraceScope>,
}

/// Optional observability scope attached to a provider instance. When set,
/// `chat_once_with_thinking_override` / `stream_chat_wired` emit an `LlmCall`
/// span (trace id = `run_id`) on completion.
#[derive(Clone)]
pub struct LlmTraceScope {
    pub bus: std::sync::Arc<crate::observability::TraceBus>,
    pub run_id: String,
    pub conversation_id: String,
    pub label: String,
}

/// Ends + emits an in-flight LLM span on drop, so `?` early-returns still
/// produce a span. The wrapper sets status/attributes before drop.
struct LlmSpanGuard {
    bus: std::sync::Arc<crate::observability::TraceBus>,
    span: crate::observability::TraceEvent,
}

impl LlmSpanGuard {
    fn new(scope: &LlmTraceScope) -> Self {
        let mut span = crate::observability::TraceEvent::new(
            scope.run_id.clone(),
            uuid::Uuid::new_v4().to_string(),
            crate::observability::SpanKind::LlmCall,
            scope.label.clone(),
        );
        span.parent_span_id = Some("run-root".to_string());
        span.run_id = scope.run_id.clone();
        span.conversation_id = scope.conversation_id.clone();
        Self {
            bus: scope.bus.clone(),
            span,
        }
    }

    fn set_error(&mut self, code: &str, message: impl Into<String>) {
        self.span.set_error(code, message);
    }
}

impl Drop for LlmSpanGuard {
    fn drop(&mut self) {
        if self.span.ended_at_ms.is_none() {
            self.span.end();
        }
        self.bus.emit(self.span.clone());
    }
}

/// Built HTTP request for `stream_chat` (`openai_msgs` Values already dropped after serialize).
pub struct StreamChatWire {
    pub url: String,
    pub wire_body: Value,
    pub api_message_count: usize,
    pub build_openai_messages_ms: u128,
    pub system_prompt_block_count: usize,
    pub base_message_count: usize,
    pub injected_tail_count: usize,
}

#[derive(Debug, Clone, Default)]
struct NativeToolCallState {
    id: String,
    name: String,
    arguments: String,
    started: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ConsoleStreamLane {
    Reasoning,
    Output,
}

/// Non-streaming chat/completions result including optional `usage`.
#[derive(Debug)]
pub struct ChatOnceOutput {
    pub text: String,
    pub usage: Option<LlmUsageSnapshot>,
    /// Model id sent on the chat/completions request (token reporting source of truth).
    pub model: String,
    /// Native tool calls when the request included `tools`.
    pub tool_calls: Vec<ToolCall>,
    /// Reasoning / thinking channel when returned separately from `content`.
    pub reasoning_content: Option<String>,
    /// The `finish_reason` from the first choice (e.g. `stop`, `length`, `content_filter`).
    pub finish_reason: Option<String>,
}

fn parse_chat_once_tool_calls(raw: Option<&[ChatApiToolCall]>) -> Vec<ToolCall> {
    let Some(calls) = raw else {
        return vec![];
    };
    calls
        .iter()
        .enumerate()
        .filter_map(|(i, tc)| {
            let func = tc.function.as_ref()?;
            let name = func.name.as_deref()?.trim();
            if name.is_empty() {
                return None;
            }
            let id = tc
                .id
                .as_deref()
                .filter(|s| !s.is_empty())
                .map(String::from)
                .unwrap_or_else(|| format!("chatonce_tc_{i}"));
            Some(ToolCall {
                id,
                name: name.to_string(),
                arguments: func.arguments.clone().unwrap_or_else(|| "{}".into()),
                status: "done".into(),
                result: None,
                error: None,
                duration_ms: None,
                risk_level: None,
                display_label: None,
                display_summary: None,
            })
        })
        .collect()
}

impl OpenAIProvider {
    pub fn new(settings: ModelSettings, api_key: String) -> Self {
        Self {
            settings,
            api_key,
            trace: None,
        }
    }

    pub fn with_trace(mut self, scope: LlmTraceScope) -> Self {
        self.trace = Some(scope);
        self
    }

    pub async fn test(&self) -> Result<u128> {
        let start = std::time::Instant::now();
        // 用一次最小化 chat 请求测试连通性
        let base_url = self
            .settings
            .providers
            .iter()
            .find(|p| p.id == self.settings.active_provider_id)
            .map(|p| p.base_url.clone())
            .unwrap_or_else(|| {
                self.settings
                    .providers
                    .first()
                    .map(|p| p.base_url.clone())
                    .unwrap_or_default()
            });

        let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
        let body = json!({
            "model": self.settings.model,
            "messages": [{"role":"user","content":"ping"}],
            "stream": false,
            "max_tokens": 4
        });
        let client = llm_http_client(Duration::from_secs(20))?;
        let resp = client
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await?;
        if !resp.status().is_success() {
            let s = resp.status();
            let t = resp.text().await.unwrap_or_default();
            return Err(anyhow!("HTTP {}: {}", s, truncate(&t, 200)));
        }
        Ok(start.elapsed().as_millis())
    }

    pub async fn chat_once(
        &self,
        messages: &[ChatMessage],
        system: &SystemPromptSections,
        native_tools: Vec<Value>,
        cancel: CancellationToken,
        max_tokens_override: Option<u32>,
        dump_label: Option<&str>,
    ) -> Result<ChatOnceOutput> {
        self.chat_once_with_thinking_override(
            messages,
            system,
            native_tools,
            cancel,
            max_tokens_override,
            dump_label,
            false,
        )
        .await
    }

    /// Auxiliary non-streaming completion that suppresses provider thinking tokens.
    ///
    /// This is intended for bounded structured text such as context summaries, where
    /// hidden reasoning must not consume the output budget and truncate the answer.
    pub async fn chat_once_without_thinking(
        &self,
        messages: &[ChatMessage],
        system: &SystemPromptSections,
        native_tools: Vec<Value>,
        cancel: CancellationToken,
        max_tokens_override: Option<u32>,
        dump_label: Option<&str>,
    ) -> Result<ChatOnceOutput> {
        self.chat_once_with_thinking_override(
            messages,
            system,
            native_tools,
            cancel,
            max_tokens_override,
            dump_label,
            true,
        )
        .await
    }

    async fn chat_once_with_thinking_override(
        &self,
        messages: &[ChatMessage],
        system: &SystemPromptSections,
        native_tools: Vec<Value>,
        cancel: CancellationToken,
        max_tokens_override: Option<u32>,
        dump_label: Option<&str>,
        disable_thinking: bool,
    ) -> Result<ChatOnceOutput> {
        let mut guard = self.trace.as_ref().map(LlmSpanGuard::new);
        let result = self
            .chat_once_with_thinking_override_inner(
                messages,
                system,
                native_tools,
                cancel,
                max_tokens_override,
                dump_label,
                disable_thinking,
            )
            .await;
        match &result {
            Ok(out) => {
                if let Some(g) = guard.as_mut() {
                    g.span.attributes = match &out.usage {
                        Some(u) => serde_json::json!({
                            "tokens_in": u.prompt_tokens,
                            "tokens_out": u.output_tokens(),
                            "tokens_cached": u.cached_tokens,
                            "model": out.model,
                        }),
                        None => serde_json::json!({ "model": out.model }),
                    };
                }
            }
            Err(e) => {
                if let Some(g) = guard.as_mut() {
                    g.set_error("llm_failed", e.to_string());
                }
            }
        }
        result
    }

    async fn chat_once_with_thinking_override_inner(
        &self,
        messages: &[ChatMessage],
        system: &SystemPromptSections,
        native_tools: Vec<Value>,
        cancel: CancellationToken,
        max_tokens_override: Option<u32>,
        dump_label: Option<&str>,
        disable_thinking: bool,
    ) -> Result<ChatOnceOutput> {
        let base_url = self
            .settings
            .providers
            .iter()
            .find(|p| p.id == self.settings.active_provider_id)
            .map(|p| p.base_url.clone())
            .unwrap_or_else(|| {
                self.settings
                    .providers
                    .first()
                    .map(|p| p.base_url.clone())
                    .unwrap_or_default()
            });

        crate::message_context::try_log_context_excluded_messages(
            &self.settings,
            messages,
            "chat_once",
            dump_label,
            crate::message_context::LlmHistoryScope::Lead,
        );
        let max_tok =
            max_tokens_override.unwrap_or(crate::models::effective_max_tokens(&self.settings));
        let mut extra_body = crate::models::effective_chat_extra_body(&self.settings);
        if disable_thinking {
            crate::models::apply_thinking_disabled_to_extra_body(&self.settings, &mut extra_body);
        }
        // Build wire in a scope so `openai_msgs` / ChatRequest drop before the HTTP round-trip.
        let (url, wire_body) = {
            let openai_msgs = crate::models::make_openai_messages(
                messages,
                system,
                crate::models::effective_reasoning_in_messages(&self.settings),
                crate::models::qwen_explicit_system_cache_enabled(&self.settings),
                crate::media::model_supports_vision(&self.settings),
                crate::message_context::LlmHistoryScope::Lead,
            );
            crate::llm_prompt_dump::try_dump_round(
                &self.settings,
                dump_label,
                "chat_once",
                false,
                max_tok,
                &openai_msgs,
            );
            let tools_empty = native_tools.is_empty();
            let req = ChatRequest {
                model: &self.settings.model,
                messages: openai_msgs,
                stream: false,
                temperature: crate::models::effective_temperature(&self.settings),
                max_tokens: Some(max_tok),
                stream_options: None,
                tools: if tools_empty {
                    None
                } else {
                    Some(native_tools)
                },
                tool_choice: if tools_empty { None } else { Some("auto") },
                extra_body,
            };
            let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
            let wire_body = chat_request_wire_json(&req, &self.settings);
            (url, wire_body)
        };
        crate::llm_prompt_dump::try_log_openai_chat_request_json(
            &self.settings,
            "chat_once",
            dump_label,
            &url,
            &wire_body,
        );
        let client = llm_http_client(Duration::from_secs(180))?;
        let resp = tokio::select! {
            _ = cancel.cancelled() => return Err(anyhow!("cancelled")),
            r = client
                .post(&url)
                .bearer_auth(&self.api_key)
                .json(&wire_body)
                .send() => r?,
        };
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("HTTP {}: {}", status, truncate(&text, 400)));
        }
        let parsed: ChatOnceApiResponse = resp.json().await?;
        let finish_reason = parsed
            .choices
            .first()
            .and_then(|ch| ch.finish_reason.clone());
        let message = parsed
            .choices
            .into_iter()
            .next()
            .map(|choice| choice.message)
            .ok_or_else(|| anyhow!("模型未返回候选结果"))?;
        let text = message
            .content
            .clone()
            .or_else(|| message.reasoning_text().map(|s| s.to_string()))
            .unwrap_or_default();
        let usage = parsed.usage.as_ref().map(snapshot_from_stream_usage);
        let tool_calls = parse_chat_once_tool_calls(message.tool_calls.as_deref());
        Ok(ChatOnceOutput {
            text,
            usage,
            model: self.settings.model.clone(),
            tool_calls,
            reasoning_content: None,
            finish_reason,
        })
    }

    /// Non-streaming completion with explicit OpenAI-style message list (multimodal user parts).
    pub async fn chat_once_wire_messages(
        &self,
        messages: Vec<Value>,
        cancel: CancellationToken,
        max_tokens_override: Option<u32>,
        dump_label: Option<&str>,
    ) -> Result<ChatOnceOutput> {
        let base_url = self
            .settings
            .providers
            .iter()
            .find(|p| p.id == self.settings.active_provider_id)
            .map(|p| p.base_url.clone())
            .unwrap_or_else(|| {
                self.settings
                    .providers
                    .first()
                    .map(|p| p.base_url.clone())
                    .unwrap_or_default()
            });
        let max_tok =
            max_tokens_override.unwrap_or(crate::models::effective_max_tokens(&self.settings));
        let extra_body = crate::models::effective_chat_extra_body(&self.settings);
        crate::llm_prompt_dump::try_dump_round(
            &self.settings,
            dump_label,
            "chat_once_wire",
            false,
            max_tok,
            &messages,
        );
        let req = ChatRequest {
            model: &self.settings.model,
            messages,
            stream: false,
            temperature: crate::models::effective_temperature(&self.settings),
            max_tokens: Some(max_tok),
            stream_options: None,
            tools: None,
            tool_choice: None,
            extra_body,
        };
        let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
        let wire_body = chat_request_wire_json(&req, &self.settings);
        let client = llm_http_client(Duration::from_secs(180))?;
        let resp = tokio::select! {
            _ = cancel.cancelled() => return Err(anyhow!("cancelled")),
            r = client
                .post(&url)
                .bearer_auth(&self.api_key)
                .json(&wire_body)
                .send() => r?,
        };
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("HTTP {}: {}", status, truncate(&text, 400)));
        }
        let parsed: ChatOnceApiResponse = resp.json().await?;
        let finish_reason = parsed
            .choices
            .first()
            .and_then(|ch| ch.finish_reason.clone());
        let message = parsed
            .choices
            .into_iter()
            .next()
            .map(|choice| choice.message)
            .ok_or_else(|| anyhow!("模型未返回候选结果"))?;
        let text = message
            .content
            .clone()
            .or_else(|| message.reasoning_text().map(|s| s.to_string()))
            .unwrap_or_default();
        let usage = parsed.usage.as_ref().map(snapshot_from_stream_usage);
        let tool_calls = parse_chat_once_tool_calls(message.tool_calls.as_deref());
        Ok(ChatOnceOutput {
            text,
            usage,
            model: self.settings.model.clone(),
            tool_calls,
            reasoning_content: None,
            finish_reason,
        })
    }

    /// Position / Verify pipeline modules — stream + thinking + virtual submit tools.
    /// Structured result comes from `tool_calls[].arguments`, not `content` or `response_format`.
    pub async fn stream_pipeline_module_with_tools(
        &self,
        messages: Vec<Value>,
        system_text: Option<&str>,
        tools: Vec<Value>,
        cancel: CancellationToken,
        max_tokens_override: Option<u32>,
        dump_label: Option<&str>,
    ) -> Result<ChatOnceOutput> {
        let max_tok =
            max_tokens_override.unwrap_or(crate::models::effective_max_tokens(&self.settings));
        let mut wire_messages = Vec::new();
        if let Some(sys) = system_text.filter(|s| !s.trim().is_empty()) {
            wire_messages.push(json!({ "role": "system", "content": sys }));
        }
        wire_messages.extend(messages);
        let extra_body = crate::models::effective_chat_extra_body(&self.settings);
        log::info!(
            "stream_pipeline_module_tools: stream+thinking+tools={} dump_label={dump_label:?} \
             enable_thinking={:?}",
            tools.len(),
            extra_body
                .as_ref()
                .and_then(|eb| eb.get("enable_thinking"))
                .and_then(|v| v.as_bool())
        );
        self.stream_wire_messages_collect_with_tools(
            wire_messages,
            max_tok,
            extra_body,
            tools,
            cancel,
            dump_label,
            "pipeline_module_tools",
        )
        .await
    }

    /// Non-streaming completion with explicit wire messages and optional system + response_format.
    pub async fn chat_once_wire_messages_with_response_format(
        &self,
        messages: Vec<Value>,
        system_text: Option<&str>,
        response_format: Option<Value>,
        cancel: CancellationToken,
        max_tokens_override: Option<u32>,
        dump_label: Option<&str>,
    ) -> Result<ChatOnceOutput> {
        let base_url = self
            .settings
            .providers
            .iter()
            .find(|p| p.id == self.settings.active_provider_id)
            .map(|p| p.base_url.clone())
            .unwrap_or_else(|| {
                self.settings
                    .providers
                    .first()
                    .map(|p| p.base_url.clone())
                    .unwrap_or_default()
            });
        let max_tok =
            max_tokens_override.unwrap_or(crate::models::effective_max_tokens(&self.settings));
        let mut extra_body =
            crate::models::effective_chat_extra_body(&self.settings).unwrap_or_else(|| json!({}));
        let has_response_format = response_format.is_some();
        if let Some(rf) = response_format {
            if let Some(obj) = extra_body.as_object_mut() {
                obj.insert("response_format".into(), rf);
            } else {
                extra_body = json!({ "response_format": rf });
            }
        }
        let mut wire_messages = Vec::new();
        if let Some(sys) = system_text.filter(|s| !s.trim().is_empty()) {
            wire_messages.push(json!({ "role": "system", "content": sys }));
        }
        wire_messages.extend(messages);
        crate::llm_prompt_dump::try_dump_round(
            &self.settings,
            dump_label,
            "chat_once_wire_schema",
            false,
            max_tok,
            &wire_messages,
        );
        if thinking_enabled_for_wire_request(&self.settings) && !has_response_format {
            log::info!(
                "chat_once_wire_schema: enable_thinking=true, stream without response_format dump_label={dump_label:?}"
            );
            let thinking_extra = crate::models::effective_chat_extra_body(&self.settings);
            return self
                .stream_wire_messages_collect(
                    wire_messages,
                    max_tok,
                    thinking_extra,
                    cancel,
                    dump_label,
                    "chat_once_wire_schema",
                    false,
                )
                .await;
        }
        if thinking_enabled_for_wire_request(&self.settings) && has_response_format {
            log::warn!(
                "chat_once_wire_schema: response_format present — disabling thinking for structured output dump_label={dump_label:?}"
            );
            let mut disabled = Some(extra_body);
            crate::models::apply_thinking_disabled_to_extra_body(&self.settings, &mut disabled);
            extra_body = disabled.unwrap_or_else(|| json!({}));
        }
        let req = ChatRequest {
            model: &self.settings.model,
            messages: wire_messages,
            stream: false,
            temperature: crate::models::effective_temperature(&self.settings),
            max_tokens: Some(max_tok),
            stream_options: None,
            tools: None,
            tool_choice: None,
            extra_body: if skip_extra_body(&Some(extra_body.clone())) {
                None
            } else {
                Some(extra_body)
            },
        };
        let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
        let wire_body = chat_request_wire_json(&req, &self.settings);
        let client = llm_http_client(Duration::from_secs(180))?;
        let resp = tokio::select! {
            _ = cancel.cancelled() => return Err(anyhow!("cancelled")),
            r = client
                .post(&url)
                .bearer_auth(&self.api_key)
                .json(&wire_body)
                .send() => r?,
        };
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("HTTP {}: {}", status, truncate(&text, 400)));
        }
        let parsed: ChatOnceApiResponse = resp.json().await?;
        let finish_reason = parsed
            .choices
            .first()
            .and_then(|ch| ch.finish_reason.clone());
        let message = parsed
            .choices
            .into_iter()
            .next()
            .map(|choice| choice.message)
            .ok_or_else(|| anyhow!("模型未返回候选结果"))?;
        let content = message.content.clone().unwrap_or_default();
        let reasoning_content = message
            .reasoning_text()
            .map(|s| s.to_string())
            .filter(|s| !s.trim().is_empty());
        let text = if content.trim().is_empty() {
            reasoning_content.clone().unwrap_or_default()
        } else {
            content
        };
        let usage = parsed.usage.as_ref().map(snapshot_from_stream_usage);
        let tool_calls = parse_chat_once_tool_calls(message.tool_calls.as_deref());
        Ok(ChatOnceOutput {
            text,
            usage,
            model: self.settings.model.clone(),
            tool_calls,
            reasoning_content,
            finish_reason,
        })
    }

    /// Streaming collect for wire messages (Decision-style SSE deltas).
    async fn stream_wire_messages_collect(
        &self,
        wire_messages: Vec<Value>,
        max_tok: u32,
        extra_body: Option<Value>,
        cancel: CancellationToken,
        dump_label: Option<&str>,
        dump_phase: &str,
        // When true, use `content` or fall back to `reasoning` for `text` (legacy non-pipeline).
        allow_reasoning_text_fallback: bool,
    ) -> Result<ChatOnceOutput> {
        let base_url = self
            .settings
            .providers
            .iter()
            .find(|p| p.id == self.settings.active_provider_id)
            .map(|p| p.base_url.clone())
            .unwrap_or_else(|| {
                self.settings
                    .providers
                    .first()
                    .map(|p| p.base_url.clone())
                    .unwrap_or_default()
            });
        crate::llm_prompt_dump::try_dump_round(
            &self.settings,
            dump_label,
            dump_phase,
            true,
            max_tok,
            &wire_messages,
        );
        let stream_options = if stream_include_usage_enabled() {
            Some(json!({"include_usage": true}))
        } else {
            None
        };
        let req = ChatRequest {
            model: &self.settings.model,
            messages: wire_messages,
            stream: true,
            temperature: crate::models::effective_temperature(&self.settings),
            max_tokens: Some(max_tok),
            stream_options,
            tools: None,
            tool_choice: None,
            extra_body,
        };
        let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
        let wire_body = chat_request_wire_json(&req, &self.settings);
        crate::llm_prompt_dump::try_log_openai_chat_request_json(
            &self.settings,
            dump_phase,
            dump_label,
            &url,
            &wire_body,
        );
        let client = llm_http_client(Duration::from_secs(180))?;
        let resp = tokio::select! {
            _ = cancel.cancelled() => return Err(anyhow!("cancelled")),
            r = client
                .post(&url)
                .bearer_auth(&self.api_key)
                .json(&wire_body)
                .send() => r?,
        };
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("HTTP {}: {}", status, truncate(&text, 400)));
        }
        let mut reasoning_buf = String::new();
        let mut content_buf = String::new();
        let mut last_usage: Option<LlmUsageSnapshot> = None;
        let mut last_finish_reason: Option<String> = None;
        let stream_raw_to_console = raw_llm_stream_to_console_enabled();
        let mut last_console_lane: Option<ConsoleStreamLane> = None;
        let mut stream = resp.bytes_stream();
        let mut buf = String::new();
        loop {
            let item = tokio::select! {
                _ = cancel.cancelled() => return Err(anyhow!("cancelled")),
                v = stream.next() => v,
            };
            let chunk = match item {
                Some(c) => c?,
                None => break,
            };
            buf.push_str(&String::from_utf8_lossy(&chunk));
            while let Some(pos) = buf.find('\n') {
                let line = buf[..pos].trim().to_string();
                buf.drain(..=pos);
                if line.is_empty() {
                    continue;
                }
                let data = match line.strip_prefix("data:") {
                    Some(rest) => rest.trim(),
                    None => continue,
                };
                if data == "[DONE]" {
                    break;
                }
                let parsed: StreamChunk = match serde_json::from_str(data) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                for ch in parsed.choices.iter() {
                    if let Some(ref c) = ch.delta.content {
                        if !c.is_empty() {
                            if stream_raw_to_console {
                                write_llm_stream_chunk_to_stderr(
                                    c,
                                    ConsoleStreamLane::Output,
                                    &mut last_console_lane,
                                );
                            }
                            content_buf.push_str(c);
                        }
                    }
                    if let Some(r) = ch.delta.reasoning_text() {
                        if stream_raw_to_console {
                            write_llm_stream_chunk_to_stderr(
                                r,
                                ConsoleStreamLane::Reasoning,
                                &mut last_console_lane,
                            );
                        }
                        reasoning_buf.push_str(r);
                    }
                    if let Some(ref reason) = ch.finish_reason {
                        last_finish_reason = Some(reason.clone());
                    }
                }
                if let Some(ref u) = parsed.usage {
                    last_usage = Some(snapshot_from_stream_usage(u));
                }
            }
        }
        let reasoning_content = if reasoning_buf.trim().is_empty() {
            None
        } else {
            Some(reasoning_buf)
        };
        let text = if !content_buf.trim().is_empty() {
            content_buf
        } else if allow_reasoning_text_fallback {
            reasoning_content.clone().unwrap_or_default()
        } else {
            String::new()
        };
        log::info!(
            "stream_wire_collect phase={dump_phase} dump_label={dump_label:?} \
             reasoning_chars={} content_chars={} reasoning_tokens={:?} finish_reason={last_finish_reason:?}",
            reasoning_content.as_ref().map(|s| s.chars().count()).unwrap_or(0),
            text.chars().count(),
            last_usage.as_ref().map(|u| u.reasoning_tokens),
        );
        Ok(ChatOnceOutput {
            text,
            usage: last_usage,
            model: self.settings.model.clone(),
            tool_calls: vec![],
            reasoning_content,
            finish_reason: last_finish_reason,
        })
    }

    /// Streaming collect with virtual pipeline tools (reasoning + tool_calls).
    async fn stream_wire_messages_collect_with_tools(
        &self,
        wire_messages: Vec<Value>,
        max_tok: u32,
        extra_body: Option<Value>,
        tools: Vec<Value>,
        cancel: CancellationToken,
        dump_label: Option<&str>,
        dump_phase: &str,
    ) -> Result<ChatOnceOutput> {
        let base_url = self
            .settings
            .providers
            .iter()
            .find(|p| p.id == self.settings.active_provider_id)
            .map(|p| p.base_url.clone())
            .unwrap_or_else(|| {
                self.settings
                    .providers
                    .first()
                    .map(|p| p.base_url.clone())
                    .unwrap_or_default()
            });
        crate::llm_prompt_dump::try_dump_round(
            &self.settings,
            dump_label,
            dump_phase,
            true,
            max_tok,
            &wire_messages,
        );
        let stream_options = if stream_include_usage_enabled() {
            Some(json!({"include_usage": true}))
        } else {
            None
        };
        let tools_empty = tools.is_empty();
        let req = ChatRequest {
            model: &self.settings.model,
            messages: wire_messages,
            stream: true,
            temperature: crate::models::effective_temperature(&self.settings),
            max_tokens: Some(max_tok),
            stream_options,
            tools: if tools_empty { None } else { Some(tools) },
            tool_choice: if tools_empty { None } else { Some("auto") },
            extra_body,
        };
        let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
        let wire_body = chat_request_wire_json(&req, &self.settings);
        crate::llm_prompt_dump::try_log_openai_chat_request_json(
            &self.settings,
            dump_phase,
            dump_label,
            &url,
            &wire_body,
        );
        let client = llm_http_client(Duration::from_secs(180))?;
        let resp = tokio::select! {
            _ = cancel.cancelled() => return Err(anyhow!("cancelled")),
            r = client
                .post(&url)
                .bearer_auth(&self.api_key)
                .json(&wire_body)
                .send() => r?,
        };
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("HTTP {}: {}", status, truncate(&text, 400)));
        }
        let mut reasoning_buf = String::new();
        let mut content_buf = String::new();
        let stream_tool_session_id = rand_id();
        let mut tool_states: BTreeMap<u32, NativeToolCallState> = BTreeMap::new();
        let mut last_usage: Option<LlmUsageSnapshot> = None;
        let mut last_finish_reason: Option<String> = None;
        let stream_raw_to_console = raw_llm_stream_to_console_enabled();
        let mut last_console_lane: Option<ConsoleStreamLane> = None;
        let mut stream = resp.bytes_stream();
        let mut buf = String::new();
        loop {
            let item = tokio::select! {
                _ = cancel.cancelled() => return Err(anyhow!("cancelled")),
                v = stream.next() => v,
            };
            let chunk = match item {
                Some(c) => c?,
                None => break,
            };
            buf.push_str(&String::from_utf8_lossy(&chunk));
            while let Some(pos) = buf.find('\n') {
                let line = buf[..pos].trim().to_string();
                buf.drain(..=pos);
                if line.is_empty() {
                    continue;
                }
                let data = match line.strip_prefix("data:") {
                    Some(rest) => rest.trim(),
                    None => continue,
                };
                if data == "[DONE]" {
                    break;
                }
                let parsed: StreamChunk = match serde_json::from_str(data) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                for ch in parsed.choices.iter() {
                    if let Some(ref c) = ch.delta.content {
                        if !c.is_empty() {
                            if stream_raw_to_console {
                                write_llm_stream_chunk_to_stderr(
                                    c,
                                    ConsoleStreamLane::Output,
                                    &mut last_console_lane,
                                );
                            }
                            content_buf.push_str(c);
                        }
                    }
                    if let Some(r) = ch.delta.reasoning_text() {
                        if stream_raw_to_console {
                            write_llm_stream_chunk_to_stderr(
                                r,
                                ConsoleStreamLane::Reasoning,
                                &mut last_console_lane,
                            );
                        }
                        reasoning_buf.push_str(r);
                    }
                    if let Some(ref reason) = ch.finish_reason {
                        last_finish_reason = Some(reason.clone());
                    }
                    if let Some(ref calls) = ch.delta.tool_calls {
                        for call in calls {
                            let idx = call.index;
                            let state = tool_states.entry(idx).or_default();
                            if state.id.is_empty() {
                                if let Some(id) = call.id.as_ref().filter(|s| !s.trim().is_empty())
                                {
                                    state.id = id.clone();
                                }
                            }
                            if let Some(function) = &call.function {
                                if let Some(name_part) =
                                    function.name.as_ref().filter(|s| !s.is_empty())
                                {
                                    state.name.push_str(name_part);
                                }
                                if let Some(args_part) =
                                    function.arguments.as_ref().filter(|s| !s.is_empty())
                                {
                                    state.arguments.push_str(args_part);
                                }
                            }
                            if state.id.is_empty() {
                                state.id = format!("pipeline_tool_{stream_tool_session_id}_{idx}");
                            }
                        }
                    }
                }
                if let Some(ref u) = parsed.usage {
                    last_usage = Some(snapshot_from_stream_usage(u));
                }
            }
        }
        let tool_calls = native_tool_calls_from_states(&tool_states);
        let reasoning_content = if reasoning_buf.trim().is_empty() {
            None
        } else {
            Some(reasoning_buf)
        };
        log::info!(
            "stream_wire_collect_tools phase={dump_phase} dump_label={dump_label:?} \
             reasoning_chars={} content_chars={} tool_calls={} reasoning_tokens={:?} finish_reason={last_finish_reason:?}",
            reasoning_content.as_ref().map(|s| s.chars().count()).unwrap_or(0),
            content_buf.chars().count(),
            tool_calls.len(),
            last_usage.as_ref().map(|u| u.reasoning_tokens),
        );
        Ok(ChatOnceOutput {
            text: content_buf,
            usage: last_usage,
            model: self.settings.model.clone(),
            tool_calls,
            reasoning_content,
            finish_reason: last_finish_reason,
        })
    }

    /// Serialize stream chat request to wire JSON without holding openai message Values afterward.
    pub fn build_stream_chat_wire(
        &self,
        base_messages: &[ChatMessage],
        injected_tail: &[ChatMessage],
        system: &SystemPromptSections,
        native_tools: Vec<Value>,
        dump_label: Option<&str>,
        history_scope: crate::message_context::LlmHistoryScope,
    ) -> Result<StreamChatWire> {
        let base_url = self
            .settings
            .providers
            .iter()
            .find(|p| p.id == self.settings.active_provider_id)
            .map(|p| p.base_url.clone())
            .unwrap_or_else(|| {
                self.settings
                    .providers
                    .first()
                    .map(|p| p.base_url.clone())
                    .unwrap_or_default()
            });

        let t_build = Instant::now();
        crate::message_context::try_log_context_excluded_messages(
            &self.settings,
            base_messages,
            "stream_chat",
            dump_label,
            history_scope,
        );
        let openai_msgs = crate::models::make_openai_messages_with_inject(
            base_messages,
            injected_tail,
            system,
            crate::models::effective_reasoning_in_messages(&self.settings),
            crate::models::qwen_explicit_system_cache_enabled(&self.settings),
            crate::media::model_supports_vision(&self.settings),
            history_scope,
        );
        let build_openai_messages_ms = t_build.elapsed().as_millis();
        let api_message_count = openai_msgs.len();
        crate::llm_prompt_dump::try_dump_round(
            &self.settings,
            dump_label,
            "stream_chat",
            true,
            crate::models::effective_max_tokens(&self.settings),
            &openai_msgs,
        );
        let extra_body = crate::models::effective_chat_extra_body(&self.settings);
        let stream_options = if stream_include_usage_enabled() {
            Some(json!({"include_usage": true}))
        } else {
            None
        };
        let tools_empty = native_tools.is_empty();
        let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
        // Scope drops `openai_msgs` / ChatRequest before returning; only `wire_body` is retained.
        let wire_body = {
            let req = ChatRequest {
                model: &self.settings.model,
                messages: openai_msgs,
                stream: true,
                temperature: crate::models::effective_temperature(&self.settings),
                max_tokens: Some(crate::models::effective_max_tokens(&self.settings)),
                stream_options,
                tools: if tools_empty {
                    None
                } else {
                    Some(native_tools)
                },
                tool_choice: if tools_empty { None } else { Some("auto") },
                extra_body,
            };
            chat_request_wire_json(&req, &self.settings)
        };
        crate::llm_prompt_dump::try_log_openai_chat_request_json(
            &self.settings,
            "stream_chat",
            dump_label,
            &url,
            &wire_body,
        );
        let injected_image_slots: usize = injected_tail
            .iter()
            .map(|m| m.images_base64.as_ref().map(|v| v.len()).unwrap_or(0))
            .sum();
        log::info!(
            "stream_chat_wire_built dump_label={:?} history_scope={:?} base_messages={} injected_tail={} injected_image_slots={} api_message_count={} build_openai_messages_ms={} cloned_history=false",
            dump_label,
            history_scope,
            base_messages.len(),
            injected_tail.len(),
            injected_image_slots,
            api_message_count,
            build_openai_messages_ms,
        );
        Ok(StreamChatWire {
            url,
            wire_body,
            api_message_count,
            build_openai_messages_ms,
            system_prompt_block_count: system.slice_count(),
            base_message_count: base_messages.len(),
            injected_tail_count: injected_tail.len(),
        })
    }

    pub async fn stream_chat(
        &self,
        messages: &[ChatMessage],
        system: &SystemPromptSections,
        native_tools: Vec<Value>,
        tx: mpsc::Sender<ProviderEvent>,
        cancel: CancellationToken,
        dump_label: Option<&str>,
        history_scope: crate::message_context::LlmHistoryScope,
    ) -> Result<()> {
        let wire = self.build_stream_chat_wire(
            messages,
            &[],
            system,
            native_tools,
            dump_label,
            history_scope,
        )?;
        self.stream_chat_wired(wire, tx, cancel, dump_label).await
    }

    /// POST a previously built stream wire body and drain SSE into `tx`.
    pub async fn stream_chat_wired(
        &self,
        wire: StreamChatWire,
        tx: mpsc::Sender<ProviderEvent>,
        cancel: CancellationToken,
        dump_label: Option<&str>,
    ) -> Result<()> {
        let mut guard = self.trace.as_ref().map(LlmSpanGuard::new);
        let result = self
            .stream_chat_wired_inner(wire, tx, cancel, dump_label, guard.as_mut())
            .await;
        if let Err(e) = &result {
            if let Some(g) = guard.as_mut() {
                g.set_error("llm_failed", e.to_string());
            }
        }
        result
    }

    async fn stream_chat_wired_inner(
        &self,
        wire: StreamChatWire,
        tx: mpsc::Sender<ProviderEvent>,
        cancel: CancellationToken,
        dump_label: Option<&str>,
        guard: Option<&mut LlmSpanGuard>,
    ) -> Result<()> {
        let stream_t0 = Instant::now();
        let StreamChatWire {
            url,
            wire_body,
            api_message_count,
            build_openai_messages_ms,
            system_prompt_block_count,
            ..
        } = wire;
        let client = llm_http_client(Duration::from_secs(180))?;

        let t_http = Instant::now();
        let resp = tokio::select! {
            _ = cancel.cancelled() => return Err(anyhow!("cancelled")),
            r = client
                .post(&url)
                .bearer_auth(&self.api_key)
                .json(&wire_body)
                .send() => r?,
        };
        let http_until_headers_ms = t_http.elapsed().as_millis();
        if crate::logging::internal_runtime_log_enabled() {
            log::debug!(
                "stream_chat: build_openai_messages_ms={} http_until_response_headers_ms={} api_message_count={} system_prompt_block_count={} dump_label={:?} pre_body_stream_wall_ms={}",
                build_openai_messages_ms,
                http_until_headers_ms,
                api_message_count,
                system_prompt_block_count,
                dump_label,
                stream_t0.elapsed().as_millis()
            );
        }

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("HTTP {}: {}", status, truncate(&text, 400)));
        }

        let mut content_buf = String::new();
        let mut finish_reason = String::from("stop");
        let stream_tool_session_id = rand_id();
        let mut tool_states: BTreeMap<u32, NativeToolCallState> = BTreeMap::new();
        let stream_raw_to_console = raw_llm_stream_to_console_enabled();
        let mut last_console_lane: Option<ConsoleStreamLane> = None;
        let mut last_usage: Option<LlmUsageSnapshot> = None;
        // Client TTFT: POST start → first non-empty content / reasoning / tool_calls delta.
        let mut first_token_logged = false;

        let mut stream = resp.bytes_stream();
        let mut buf = String::new();

        loop {
            let item = tokio::select! {
                _ = cancel.cancelled() => return Err(anyhow!("cancelled")),
                v = stream.next() => v,
            };
            let chunk = match item {
                Some(c) => c?,
                None => break,
            };
            buf.push_str(&String::from_utf8_lossy(&chunk));

            while let Some(pos) = buf.find('\n') {
                let line = buf[..pos].trim().to_string();
                buf.drain(..=pos);
                if line.is_empty() {
                    continue;
                }
                let data = match line.strip_prefix("data:") {
                    Some(rest) => rest.trim(),
                    None => continue,
                };
                if data == "[DONE]" {
                    break;
                }
                let parsed: StreamChunk = match serde_json::from_str(data) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                for ch in parsed.choices.iter() {
                    if let Some(ref c) = ch.delta.content {
                        if !c.is_empty() {
                            log_stream_first_token(
                                &mut first_token_logged,
                                t_http,
                                http_until_headers_ms,
                                "content",
                                dump_label,
                            );
                            if stream_raw_to_console {
                                write_llm_stream_chunk_to_stderr(
                                    c,
                                    ConsoleStreamLane::Output,
                                    &mut last_console_lane,
                                );
                            }
                            content_buf.push_str(c);
                            let _ = tx.send(ProviderEvent::ContentDelta(c.clone())).await;
                        }
                    }
                    if let Some(r) = ch.delta.reasoning_text() {
                        if !r.is_empty() {
                            log_stream_first_token(
                                &mut first_token_logged,
                                t_http,
                                http_until_headers_ms,
                                "reasoning",
                                dump_label,
                            );
                        }
                        if stream_raw_to_console {
                            write_llm_stream_chunk_to_stderr(
                                r,
                                ConsoleStreamLane::Reasoning,
                                &mut last_console_lane,
                            );
                        }
                        let _ = tx.send(ProviderEvent::ReasoningDelta(r.to_string())).await;
                    }
                    if let Some(ref calls) = ch.delta.tool_calls {
                        if !calls.is_empty() {
                            log_stream_first_token(
                                &mut first_token_logged,
                                t_http,
                                http_until_headers_ms,
                                "tool_calls",
                                dump_label,
                            );
                        }
                        for call in calls {
                            let idx = call.index;
                            let state = tool_states.entry(idx).or_default();
                            if state.id.is_empty() {
                                if let Some(id) = call.id.as_ref().filter(|s| !s.trim().is_empty())
                                {
                                    state.id = id.clone();
                                }
                            }
                            if let Some(function) = &call.function {
                                if let Some(name_part) =
                                    function.name.as_ref().filter(|s| !s.is_empty())
                                {
                                    state.name.push_str(name_part);
                                }
                                if let Some(args_part) =
                                    function.arguments.as_ref().filter(|s| !s.is_empty())
                                {
                                    state.arguments.push_str(args_part);
                                }
                            }
                            if state.id.is_empty() {
                                state.id = format!("native_{stream_tool_session_id}_{idx}");
                            }
                            if !state.started && !state.name.trim().is_empty() {
                                state.started = true;
                                let _ = tx
                                    .send(ProviderEvent::ToolCallStart {
                                        index: idx,
                                        id: state.id.clone(),
                                        name: state.name.trim().to_string(),
                                    })
                                    .await;
                            }
                            if let Some(function) = &call.function {
                                if let Some(args_part) =
                                    function.arguments.as_ref().filter(|s| !s.is_empty())
                                {
                                    let _ = tx
                                        .send(ProviderEvent::ToolCallArgsDelta {
                                            index: idx,
                                            tool_call_id: state.id.clone(),
                                            args: args_part.clone(),
                                        })
                                        .await;
                                }
                            }
                        }
                    }
                    if let Some(ref reason) = ch.finish_reason {
                        finish_reason = reason.clone();
                    }
                }
                if let Some(ref u) = parsed.usage {
                    let snap = snapshot_from_stream_usage(u);
                    log::debug!(
                        "stream usage chunk: total={} prompt={} completion={} reasoning={}",
                        snap.total_tokens,
                        snap.prompt_tokens,
                        snap.completion_tokens,
                        snap.reasoning_tokens
                    );
                    last_usage = Some(snap);
                }
            }
        }

        let tool_calls = native_tool_calls_from_states(&tool_states);
        let mut json_diag = JsonToolFinishDiagnostics::default();
        json_diag.attempted_tool_json = !content_buf.trim().is_empty();
        json_diag.fragment_complete = true;
        if tool_calls.is_empty() && content_buf.contains("\"tool_name\"") {
            json_diag.parse_error = Some(
                "legacy json envelope detected; native tool calling mode expects provider tool_calls"
                    .to_string(),
            );
        }

        if let Some(g) = guard {
            g.span.attributes = match &last_usage {
                Some(u) => serde_json::json!({
                    "tokens_in": u.prompt_tokens,
                    "tokens_out": u.output_tokens(),
                    "tokens_cached": u.cached_tokens,
                    "model": self.settings.model,
                }),
                None => serde_json::json!({ "model": self.settings.model }),
            };
        }

        let _ = tx
            .send(ProviderEvent::Finish {
                reason: finish_reason,
                tool_calls,
                json: json_diag,
                thoughts: None,
                headline: None,
                usage: last_usage,
                model: self.settings.model.clone(),
            })
            .await;
        Ok(())
    }
}

/// Log once per stream: client TTFT from HTTP POST start to first useful SSE delta.
fn log_stream_first_token(
    logged: &mut bool,
    t_http: Instant,
    http_until_headers_ms: u128,
    kind: &str,
    dump_label: Option<&str>,
) {
    if *logged {
        return;
    }
    *logged = true;
    log::info!(
        "stream_chat: first_token_ms={} kind={} http_until_headers_ms={} dump_label={:?}",
        t_http.elapsed().as_millis(),
        kind,
        http_until_headers_ms,
        dump_label
    );
}

fn stream_include_usage_enabled() -> bool {
    match std::env::var("POINTER_STREAM_INCLUDE_USAGE") {
        Ok(v) if v == "0" || v.eq_ignore_ascii_case("false") => false,
        _ => true,
    }
}

fn thinking_enabled_for_wire_request(settings: &ModelSettings) -> bool {
    crate::models::thinking_enabled_in_extra_body(
        crate::models::effective_chat_extra_body(settings).as_ref(),
    )
}

fn snapshot_from_stream_usage(u: &StreamUsage) -> LlmUsageSnapshot {
    let reasoning = u
        .completion_tokens_details
        .as_ref()
        .and_then(|d| d.reasoning_tokens)
        .unwrap_or(0);
    let prompt_tokens = u.prompt_tokens.unwrap_or(0);
    let completion_tokens = u.completion_tokens.unwrap_or(0);
    let mut total_tokens = u.total_tokens.unwrap_or(0);
    if total_tokens == 0 {
        total_tokens = prompt_tokens.saturating_add(completion_tokens);
    }
    // Prefer OpenAI-compatible details; fall back to top-level `cached_tokens`.
    let cached_tokens = u
        .prompt_tokens_details
        .as_ref()
        .and_then(|d| d.cached_tokens)
        .or(u.cached_tokens)
        .unwrap_or(0)
        .min(prompt_tokens);
    LlmUsageSnapshot {
        prompt_tokens,
        completion_tokens,
        total_tokens,
        reasoning_tokens: reasoning,
        cached_tokens,
    }
}

/// 流式：将模型增量原文连续写到 **stderr**（无换行、无序号前缀）。需调试模式；关闭：`POINTER_STREAM_RAW_LLM_TO_STDOUT=0`。
fn raw_llm_stream_to_console_enabled() -> bool {
    crate::logging::raw_llm_console_segments_enabled()
}

fn write_llm_stream_chunk_to_stderr(
    text: &str,
    lane: ConsoleStreamLane,
    last_lane: &mut Option<ConsoleStreamLane>,
) {
    if text.is_empty() {
        return;
    }
    let mut err = std::io::stderr().lock();
    if last_lane.is_none() || *last_lane != Some(lane) {
        let marker = match lane {
            ConsoleStreamLane::Reasoning => "\n[推理|reasoning]\n",
            ConsoleStreamLane::Output => "\n[输出|output]\n",
        };
        let _ = std::io::Write::write_all(&mut err, marker.as_bytes());
        *last_lane = Some(lane);
    }
    let _ = std::io::Write::write_all(&mut err, text.as_bytes());
    let _ = err.flush();
}

fn native_tool_calls_from_states(states: &BTreeMap<u32, NativeToolCallState>) -> Vec<ToolCall> {
    states
        .iter()
        .filter_map(|(idx, state)| {
            let name = state.name.trim();
            if name.is_empty() {
                return None;
            }
            let id = if state.id.trim().is_empty() {
                format!("native_tool_{idx}")
            } else {
                state.id.clone()
            };
            Some(ToolCall {
                id,
                name: name.to_string(),
                arguments: state.arguments.clone(),
                status: "pending".into(),
                result: None,
                error: None,
                duration_ms: None,
                risk_level: None,
                display_label: None,
                display_summary: None,
            })
        })
        .collect()
}

fn truncate(s: &str, n: usize) -> String {
    if s.len() <= n {
        return s.to_string();
    }
    let mut end = n;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &s[..end])
}

fn rand_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{:x}", n)
}

#[cfg(test)]
mod native_tool_call_tests {
    use super::*;

    #[test]
    fn native_tool_calls_from_states_orders_by_index() {
        let mut states: BTreeMap<u32, NativeToolCallState> = BTreeMap::new();
        states.insert(
            2,
            NativeToolCallState {
                id: "id_b".into(),
                name: "terminal".into(),
                arguments: r#"{"command":"echo b"}"#.into(),
                started: true,
            },
        );
        states.insert(
            1,
            NativeToolCallState {
                id: "id_a".into(),
                name: "file_read".into(),
                arguments: r#"{"path":"a.txt"}"#.into(),
                started: true,
            },
        );
        let calls = native_tool_calls_from_states(&states);
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].id, "id_a");
        assert_eq!(calls[0].name, "file_read");
        assert_eq!(calls[1].id, "id_b");
        assert_eq!(calls[1].name, "terminal");
    }

    #[test]
    fn native_tool_calls_filters_empty_name() {
        let mut states: BTreeMap<u32, NativeToolCallState> = BTreeMap::new();
        states.insert(
            0,
            NativeToolCallState {
                id: "id_0".into(),
                name: String::new(),
                arguments: "{}".into(),
                started: false,
            },
        );
        let calls = native_tool_calls_from_states(&states);
        assert!(calls.is_empty());
    }

    #[test]
    fn stream_delta_accepts_reasoning_alias_used_by_vllm_qwen() {
        let delta: StreamDelta =
            serde_json::from_str(r#"{"reasoning":" 9.","content":null}"#).unwrap();
        assert_eq!(delta.reasoning_text(), Some(" 9."));
        assert!(delta.reasoning_content.is_none());
    }

    #[test]
    fn stream_delta_prefers_reasoning_content_over_reasoning() {
        let delta: StreamDelta =
            serde_json::from_str(r#"{"reasoning_content":"a","reasoning":"b"}"#).unwrap();
        assert_eq!(delta.reasoning_text(), Some("a"));
    }

    #[test]
    fn chat_response_message_accepts_reasoning_alias() {
        let msg: ChatResponseMessage =
            serde_json::from_str(r#"{"content":"ok","reasoning":"think"}"#).unwrap();
        assert_eq!(msg.reasoning_text(), Some("think"));
    }

    #[test]
    fn snapshot_from_stream_usage_reads_prompt_tokens_details_cached_tokens() {
        let usage: StreamUsage = serde_json::from_str(
            r#"{
                "prompt_tokens": 1520,
                "completion_tokens": 85,
                "total_tokens": 1605,
                "prompt_tokens_details": { "cached_tokens": 1480 }
            }"#,
        )
        .unwrap();
        let snap = snapshot_from_stream_usage(&usage);
        assert_eq!(snap.prompt_tokens, 1520);
        assert_eq!(snap.cached_tokens, 1480);
        assert_eq!(snap.cache_hit_tokens(), 1480);
        assert_eq!(snap.cache_miss_tokens(), 40);
    }

    #[test]
    fn snapshot_from_stream_usage_falls_back_to_top_level_cached_tokens() {
        let usage: StreamUsage = serde_json::from_str(
            r#"{
                "prompt_tokens": 1000,
                "completion_tokens": 10,
                "total_tokens": 1010,
                "cached_tokens": 800
            }"#,
        )
        .unwrap();
        let snap = snapshot_from_stream_usage(&usage);
        assert_eq!(snap.cached_tokens, 800);
        assert_eq!(snap.cache_miss_tokens(), 200);
    }

    #[test]
    fn snapshot_from_stream_usage_clamps_cached_tokens_to_prompt() {
        let usage: StreamUsage = serde_json::from_str(
            r#"{
                "prompt_tokens": 100,
                "completion_tokens": 1,
                "total_tokens": 101,
                "prompt_tokens_details": { "cached_tokens": 999 }
            }"#,
        )
        .unwrap();
        let snap = snapshot_from_stream_usage(&usage);
        assert_eq!(snap.cached_tokens, 100);
        assert_eq!(snap.cache_miss_tokens(), 0);
    }
}
