use crate::json_tool_caller::JsonToolFinishDiagnostics;
use crate::llm_token_stats::LlmUsageSnapshot;
use crate::models::{ChatMessage, ModelSettings, SystemPromptSections, ToolCall};
use anyhow::{anyhow, Result};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::Write;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

const LLM_HTTP_MAX_ATTEMPTS: u32 = 6;
/// OpenAI / Anthropic 429 responses may include Retry-After (seconds) or retry-after-ms.
const LLM_RATE_LIMIT_HEADER_MAX_SECS: f64 = 120.0;
/// DashScope 百炼官方示例: base_delay=1, max_delay=60, wait_random_exponential(min=1, max=60).
const LLM_RATE_LIMIT_BACKOFF_INITIAL_SECS: f64 = 1.0;
const LLM_RATE_LIMIT_BACKOFF_MAX_SECS: f64 = 60.0;
const LLM_TRANSIENT_BACKOFF_INITIAL_MS: u64 = 500;
const LLM_TRANSIENT_BACKOFF_MAX_MS: u64 = 8_000;
const LLM_CHAT_BASE_TIMEOUT_SECS: u64 = 180;
/// DashScope burst throttling: server-side queue (3–120s per 百炼文档).
const DASHSCOPE_WAIT_TIMEOUT_SECS: u64 = 30;

fn is_dashscope_url(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    lower.contains("dashscope.aliyuncs.com") || lower.contains("dashscope-intl.aliyuncs.com")
}

fn llm_http_timeout_secs(url: &str, stream: bool, base_secs: u64) -> u64 {
    if !is_dashscope_url(url) {
        return base_secs;
    }
    if stream {
        // 流式：客户端超时需大于 Wait-Timeout（首个 chunk 前可能排队）。
        base_secs.max(DASHSCOPE_WAIT_TIMEOUT_SECS.saturating_add(1))
    } else {
        // 非流式：超时 = 基础超时 + Wait-Timeout。
        base_secs.saturating_add(DASHSCOPE_WAIT_TIMEOUT_SECS)
    }
}

fn build_llm_http_client(url: &str, stream: bool, base_timeout_secs: u64) -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .timeout(Duration::from_secs(llm_http_timeout_secs(
            url,
            stream,
            base_timeout_secs,
        )))
        .build()?)
}

fn is_retryable_llm_http_status(status: reqwest::StatusCode) -> bool {
    status == reqwest::StatusCode::TOO_MANY_REQUESTS
        || status == reqwest::StatusCode::REQUEST_TIMEOUT
        || status.is_server_error()
}

fn is_rate_limit_status(status: reqwest::StatusCode) -> bool {
    status == reqwest::StatusCode::TOO_MANY_REQUESTS
}

fn is_retryable_llm_transport_error(err: &reqwest::Error) -> bool {
    err.is_timeout() || err.is_connect() || err.is_request() || err.is_body()
}

/// Parse `Retry-After` / `retry-after-ms` per OpenAI & Anthropic rate-limit guidance.
fn parse_retry_after_secs(headers: &reqwest::header::HeaderMap) -> Option<f64> {
    if let Some(raw) = headers.get("retry-after-ms").and_then(|v| v.to_str().ok()) {
        if let Ok(ms) = raw.trim().parse::<f64>() {
            if ms > 0.0 {
                return Some(ms / 1000.0);
            }
        }
    }
    let raw = headers.get("retry-after").and_then(|v| v.to_str().ok())?;
    let trimmed = raw.trim();
    if let Ok(secs) = trimmed.parse::<f64>() {
        if secs > 0.0 {
            return Some(secs);
        }
    }
    if let Ok(dt) = chrono::DateTime::parse_from_rfc2822(trimmed) {
        let wait = dt.timestamp() as f64 - chrono::Utc::now().timestamp() as f64;
        if wait > 0.0 {
            return Some(wait);
        }
    }
    None
}

fn retry_jitter_ms(base_ms: u64) -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0) as u64;
    // 75%–100% of base (OpenAI cookbook: add random jitter to backoff).
    let jitter_num = 750 + (n % 251);
    base_ms.saturating_mul(jitter_num) / 1000
}

fn llm_rate_limit_additive_jitter_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0) as u64;
    // 百炼原生示例: sleep_time = backoff + random.uniform(0, 1)
    n % 1001
}

fn llm_rate_limit_backoff_ms(attempt: u32, headers: &reqwest::header::HeaderMap) -> u64 {
    if let Some(secs) = parse_retry_after_secs(headers) {
        let capped = secs.min(LLM_RATE_LIMIT_HEADER_MAX_SECS);
        return (capped * 1000.0).ceil() as u64;
    }
    let exp = LLM_RATE_LIMIT_BACKOFF_INITIAL_SECS
        * 2f64.powi(attempt.saturating_sub(1) as i32);
    let secs = exp.min(LLM_RATE_LIMIT_BACKOFF_MAX_SECS);
    (secs * 1000.0).ceil() as u64 + llm_rate_limit_additive_jitter_ms()
}

fn llm_transient_backoff_ms(attempt: u32) -> u64 {
    let exp = LLM_TRANSIENT_BACKOFF_INITIAL_MS
        .saturating_mul(1u64 << attempt.saturating_sub(1).min(4));
    let base = exp.min(LLM_TRANSIENT_BACKOFF_MAX_MS);
    retry_jitter_ms(base)
}

fn llm_retry_delay_ms(
    attempt: u32,
    status: reqwest::StatusCode,
    headers: &reqwest::header::HeaderMap,
) -> u64 {
    if is_rate_limit_status(status) {
        llm_rate_limit_backoff_ms(attempt, headers)
    } else {
        llm_transient_backoff_ms(attempt)
    }
}

async fn llm_http_sleep(delay_ms: u64, cancel: &CancellationToken) -> Result<()> {
    tokio::select! {
        _ = cancel.cancelled() => Err(anyhow!("cancelled")),
        _ = tokio::time::sleep(Duration::from_millis(delay_ms)) => Ok(()),
    }
}

async fn post_chat_with_retry(
    client: &reqwest::Client,
    url: &str,
    api_key: &str,
    wire_body: &Value,
    cancel: &CancellationToken,
    operation: &str,
) -> Result<reqwest::Response> {
    for attempt in 1..=LLM_HTTP_MAX_ATTEMPTS {
        let mut req = client.post(url).bearer_auth(api_key).json(wire_body);
        if is_dashscope_url(url) {
            req = req.header(
                "X-DashScope-Wait-Timeout",
                DASHSCOPE_WAIT_TIMEOUT_SECS.to_string(),
            );
        }
        let send_result = tokio::select! {
            _ = cancel.cancelled() => return Err(anyhow!("cancelled")),
            r = req.send() => r,
        };
        match send_result {
            Ok(resp) => {
                if resp.status().is_success() {
                    return Ok(resp);
                }
                let status = resp.status();
                let retryable = is_retryable_llm_http_status(status);
                let headers = resp.headers().clone();
                let body_preview = resp.text().await.unwrap_or_default();
                if !retryable || attempt >= LLM_HTTP_MAX_ATTEMPTS {
                    return Err(anyhow!(
                        "HTTP {}: {}",
                        status,
                        truncate(&body_preview, 400)
                    ));
                }
                let delay_ms = llm_retry_delay_ms(attempt, status, &headers);
                log::warn!(
                    "{operation}: HTTP {status} attempt {attempt}/{LLM_HTTP_MAX_ATTEMPTS}, \
                     retrying after {delay_ms}ms{}",
                    if is_rate_limit_status(status) {
                        parse_retry_after_secs(&headers)
                            .map(|s| format!(" (Retry-After={s:.3}s)"))
                            .unwrap_or_default()
                    } else {
                        String::new()
                    }
                );
                llm_http_sleep(delay_ms, cancel).await?;
            }
            Err(err)
                if attempt < LLM_HTTP_MAX_ATTEMPTS && is_retryable_llm_transport_error(&err) =>
            {
                let delay_ms = llm_transient_backoff_ms(attempt);
                log::warn!(
                    "{operation}: transport error attempt {attempt}/{LLM_HTTP_MAX_ATTEMPTS}: {err:#}, \
                     retrying after {delay_ms}ms"
                );
                llm_http_sleep(delay_ms, cancel).await?;
            }
            Err(err) => return Err(err.into()),
        }
    }
    Err(anyhow!("LLM HTTP request failed after {LLM_HTTP_MAX_ATTEMPTS} attempts"))
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
/// 扩展参数（千问/DeepSeek 等）在配置侧为结构化字段，序列化后展平到请求体根级。
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
    crate::models::flatten_chat_extra_body_on_wire(body, settings)
}

#[derive(Deserialize, Debug, Clone)]
struct StreamUsage {
    #[serde(default, alias = "input_tokens")]
    prompt_tokens: Option<u32>,
    #[serde(default, alias = "output_tokens")]
    completion_tokens: Option<u32>,
    #[serde(default)]
    total_tokens: Option<u32>,
    #[serde(default)]
    completion_tokens_details: Option<CompletionTokensDetails>,
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
}
#[derive(Deserialize, Debug, Default)]
struct ChatResponseMessage {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    reasoning_content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<ChatApiToolCall>>,
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
    #[serde(default)]
    reasoning_content: Option<String>,
    /// Absorbed from provider SSE; native `tool_calls` are unused (JSON-in-content only). Kept for serde + forward-compat.
    #[serde(default)]
    #[allow(dead_code)]
    tool_calls: Option<Vec<StreamToolCall>>,
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

pub struct OpenAIProvider {
    pub settings: ModelSettings,
    pub api_key: String,
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
        Self { settings, api_key }
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
        let client = build_llm_http_client(&url, false, 20)?;
        let cancel = CancellationToken::new();
        let _resp = post_chat_with_retry(&client, &url, &self.api_key, &body, &cancel, "test").await?;
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
        );
        let openai_msgs = crate::models::make_openai_messages(
            messages,
            system,
            crate::models::effective_reasoning_in_messages(&self.settings),
            crate::models::qwen_explicit_system_cache_enabled(&self.settings),
            crate::media::model_supports_vision(&self.settings),
        );
        let max_tok = max_tokens_override.unwrap_or(crate::models::effective_max_tokens(&self.settings));
        let extra_body = crate::models::effective_chat_extra_body(&self.settings);
        crate::llm_prompt_dump::try_dump_round(
            &self.settings,
            dump_label,
            "chat_once",
            false,
            max_tok,
            &openai_msgs,
        );
        let req = ChatRequest {
            model: &self.settings.model,
            messages: openai_msgs,
            stream: false,
            temperature: crate::models::effective_temperature(&self.settings),
            max_tokens: Some(max_tok),
            stream_options: None,
            tools: if native_tools.is_empty() {
                None
            } else {
                Some(native_tools)
            },
            tool_choice: Some("auto"),
            extra_body,
        };
        let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
        let wire_body = chat_request_wire_json(&req, &self.settings);
        crate::llm_prompt_dump::try_log_openai_chat_request_json(
            &self.settings,
            "chat_once",
            dump_label,
            &url,
            &wire_body,
        );
        let client = build_llm_http_client(&url, false, LLM_CHAT_BASE_TIMEOUT_SECS)?;
        let resp = post_chat_with_retry(
            &client,
            &url,
            &self.api_key,
            &wire_body,
            &cancel,
            "chat_once",
        )
        .await?;
        let parsed: ChatOnceApiResponse = resp.json().await?;
        let message = parsed
            .choices
            .into_iter()
            .next()
            .map(|choice| choice.message)
            .ok_or_else(|| anyhow!("模型未返回候选结果"))?;
        let text = message
            .content
            .clone()
            .or_else(|| message.reasoning_content.clone())
            .unwrap_or_default();
        let usage = parsed.usage.as_ref().map(snapshot_from_stream_usage);
        let tool_calls = parse_chat_once_tool_calls(message.tool_calls.as_deref());
        Ok(ChatOnceOutput {
            text,
            usage,
            model: self.settings.model.clone(),
            tool_calls,
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
        let max_tok = max_tokens_override.unwrap_or(crate::models::effective_max_tokens(&self.settings));
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
        let client = build_llm_http_client(&url, false, LLM_CHAT_BASE_TIMEOUT_SECS)?;
        let resp = post_chat_with_retry(
            &client,
            &url,
            &self.api_key,
            &wire_body,
            &cancel,
            "chat_once_wire",
        )
        .await?;
        let parsed: ChatOnceApiResponse = resp.json().await?;
        let message = parsed
            .choices
            .into_iter()
            .next()
            .map(|choice| choice.message)
            .ok_or_else(|| anyhow!("模型未返回候选结果"))?;
        let text = message
            .content
            .clone()
            .or_else(|| message.reasoning_content.clone())
            .unwrap_or_default();
        let usage = parsed.usage.as_ref().map(snapshot_from_stream_usage);
        let tool_calls = parse_chat_once_tool_calls(message.tool_calls.as_deref());
        Ok(ChatOnceOutput {
            text,
            usage,
            model: self.settings.model.clone(),
            tool_calls,
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
    ) -> Result<()> {
        let stream_t0 = Instant::now();
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
            messages,
            "stream_chat",
            dump_label,
        );
        let openai_msgs = crate::models::make_openai_messages(
            messages,
            system,
            crate::models::effective_reasoning_in_messages(&self.settings),
            crate::models::qwen_explicit_system_cache_enabled(&self.settings),
            crate::media::model_supports_vision(&self.settings),
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
        let req = ChatRequest {
            model: &self.settings.model,
            messages: openai_msgs,
            stream: true,
            temperature: crate::models::effective_temperature(&self.settings),
            max_tokens: Some(crate::models::effective_max_tokens(&self.settings)),
            stream_options,
            tools: if native_tools.is_empty() {
                None
            } else {
                Some(native_tools)
            },
            tool_choice: Some("auto"),
            extra_body,
        };

        let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
        let wire_body = chat_request_wire_json(&req, &self.settings);
        crate::llm_prompt_dump::try_log_openai_chat_request_json(
            &self.settings,
            "stream_chat",
            dump_label,
            &url,
            &wire_body,
        );
        let client = build_llm_http_client(&url, true, LLM_CHAT_BASE_TIMEOUT_SECS)?;
        let t_http = Instant::now();
        let resp = post_chat_with_retry(
            &client,
            &url,
            &self.api_key,
            &wire_body,
            &cancel,
            "stream_chat",
        )
        .await?;
        let http_until_headers_ms = t_http.elapsed().as_millis();
        if crate::logging::internal_runtime_log_enabled() {
            log::debug!(
                "stream_chat: build_openai_messages_ms={} http_until_response_headers_ms={} api_message_count={} system_prompt_block_count={} dump_label={:?} pre_body_stream_wall_ms={}",
                build_openai_messages_ms,
                http_until_headers_ms,
                api_message_count,
                system.slice_count(),
                dump_label,
                stream_t0.elapsed().as_millis()
            );
        }

        let mut content_buf = String::new();
        let mut finish_reason = String::from("stop");
        let stream_tool_session_id = rand_id();
        let mut tool_states: BTreeMap<u32, NativeToolCallState> = BTreeMap::new();
        let stream_raw_to_console = raw_llm_stream_to_console_enabled();
        let mut last_console_lane: Option<ConsoleStreamLane> = None;
        let mut last_usage: Option<LlmUsageSnapshot> = None;

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
                            let _ = tx.send(ProviderEvent::ContentDelta(c.clone())).await;
                        }
                    }
                    if let Some(ref r) = ch.delta.reasoning_content {
                        if !r.is_empty() {
                            if stream_raw_to_console {
                                write_llm_stream_chunk_to_stderr(
                                    r,
                                    ConsoleStreamLane::Reasoning,
                                    &mut last_console_lane,
                                );
                            }
                            let _ = tx.send(ProviderEvent::ReasoningDelta(r.clone())).await;
                        }
                    }
                    if let Some(ref calls) = ch.delta.tool_calls {
                        for call in calls {
                            let idx = call.index;
                            let state = tool_states.entry(idx).or_default();
                            if state.id.is_empty() {
                                if let Some(id) = call.id.as_ref().filter(|s| !s.trim().is_empty()) {
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
                                state.id =
                                    format!("native_{stream_tool_session_id}_{idx}");
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

fn stream_include_usage_enabled() -> bool {
    match std::env::var("POINTER_STREAM_INCLUDE_USAGE") {
        Ok(v) if v == "0" || v.eq_ignore_ascii_case("false") => false,
        _ => true,
    }
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
    LlmUsageSnapshot {
        prompt_tokens,
        completion_tokens,
        total_tokens,
        reasoning_tokens: reasoning,
    }
}

/// 流式：将模型增量原文连续写到 **stderr**（无换行、无序号前缀）。需调试模式；关闭：`POINTER_STREAM_RAW_LLM_TO_STDOUT=0`。
fn raw_llm_stream_to_console_enabled() -> bool {
    if !crate::logging::internal_runtime_log_enabled() {
        return false;
    }
    match std::env::var("POINTER_STREAM_RAW_LLM_TO_STDOUT") {
        Ok(v) if v == "0" || v.eq_ignore_ascii_case("false") => false,
        _ => true,
    }
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
mod llm_http_retry_tests {
    use super::*;
    use reqwest::header::{HeaderMap, HeaderValue};

    #[test]
    fn retryable_http_statuses_include_rate_limit_and_server_errors() {
        assert!(is_retryable_llm_http_status(
            reqwest::StatusCode::TOO_MANY_REQUESTS
        ));
        assert!(is_retryable_llm_http_status(reqwest::StatusCode::INTERNAL_SERVER_ERROR));
        assert!(is_retryable_llm_http_status(reqwest::StatusCode::BAD_GATEWAY));
        assert!(!is_retryable_llm_http_status(reqwest::StatusCode::BAD_REQUEST));
        assert!(!is_retryable_llm_http_status(reqwest::StatusCode::UNAUTHORIZED));
    }

    #[test]
    fn parse_retry_after_ms_header() {
        let mut headers = HeaderMap::new();
        headers.insert("retry-after-ms", HeaderValue::from_static("2500"));
        let secs = parse_retry_after_secs(&headers).expect("retry-after-ms");
        assert!((secs - 2.5).abs() < f64::EPSILON);
    }

    #[test]
    fn parse_retry_after_seconds_header() {
        let mut headers = HeaderMap::new();
        headers.insert("retry-after", HeaderValue::from_static("30"));
        let secs = parse_retry_after_secs(&headers).expect("retry-after");
        assert!((secs - 30.0).abs() < f64::EPSILON);
    }

    #[test]
    fn rate_limit_backoff_honors_retry_after_header() {
        let mut headers = HeaderMap::new();
        headers.insert("retry-after", HeaderValue::from_static("45"));
        let delay = llm_rate_limit_backoff_ms(1, &headers);
        assert_eq!(delay, 45_000, "Retry-After should be honored exactly");
    }

    #[test]
    fn rate_limit_backoff_without_header_matches_dashscope_example() {
        let headers = HeaderMap::new();
        let first = llm_rate_limit_backoff_ms(1, &headers);
        let second = llm_rate_limit_backoff_ms(2, &headers);
        assert!(first >= 1_000 && first <= 2_000, "first backoff ~1s+jitter, got {first}");
        assert!(second >= 2_000 && second <= 3_000, "second backoff ~2s+jitter, got {second}");
    }

    #[test]
    fn dashscope_url_detection_and_timeout() {
        assert!(is_dashscope_url(
            "https://dashscope.aliyuncs.com/compatible-mode/v1/chat/completions"
        ));
        assert!(is_dashscope_url(
            "https://dashscope-intl.aliyuncs.com/compatible-mode/v1/chat/completions"
        ));
        assert!(!is_dashscope_url("https://api.openai.com/v1/chat/completions"));
        assert_eq!(
            llm_http_timeout_secs(
                "https://dashscope.aliyuncs.com/compatible-mode/v1/chat/completions",
                false,
                LLM_CHAT_BASE_TIMEOUT_SECS
            ),
            LLM_CHAT_BASE_TIMEOUT_SECS + DASHSCOPE_WAIT_TIMEOUT_SECS
        );
        assert_eq!(
            llm_http_timeout_secs(
                "https://dashscope.aliyuncs.com/compatible-mode/v1/chat/completions",
                true,
                LLM_CHAT_BASE_TIMEOUT_SECS
            ),
            LLM_CHAT_BASE_TIMEOUT_SECS
        );
    }

    #[test]
    fn transient_backoff_stays_shorter_than_rate_limit_default() {
        let headers = HeaderMap::new();
        let rate = llm_rate_limit_backoff_ms(1, &headers);
        let transient = llm_transient_backoff_ms(1);
        assert!(transient < rate, "transient={transient} rate={rate}");
    }

    #[test]
    fn snapshot_from_dashscope_input_output_tokens() {
        let u: StreamUsage = serde_json::from_value(serde_json::json!({
            "input_tokens": 1200,
            "output_tokens": 340
        }))
        .expect("deserialize");
        let snap = snapshot_from_stream_usage(&u);
        assert_eq!(snap.prompt_tokens, 1200);
        assert_eq!(snap.completion_tokens, 340);
        assert_eq!(snap.total_tokens, 1540);
    }
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
}
