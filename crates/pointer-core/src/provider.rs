use crate::json_tool_caller::{
    extract_json_streaming_partial, finalize_json_tool_envelope, JsonFeedLane, JsonStreamingPartial,
    JsonToolFinishDiagnostics, JsonToolParser,
};
use crate::llm_token_stats::LlmUsageSnapshot;
use crate::models::{ChatMessage, ModelSettings, ToolCall};
use crate::response_xml::xml_tool_arguments_to_json_string;
use crate::response_xml::{XmlToolCall, XmlToolEnvelope};
use anyhow::{anyhow, Result};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::Write;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

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
    },
}

/// chat/completions 请求**不**携带 `tools` / `tool_choice`（部分网关拒绝空 `tools: []`）。
/// 本应用要求 assistant 正文为 **JSON 对象**（`response_format: json_object`），在应用侧解析工具信封；
/// 不启用服务商原生 function calling。
///
/// 非标准参数通过顶层 `extra_body` 传递（JSON 对象），由服务商或网关解析；与 OpenAI Python SDK 的 `extra_body={...}` 对应。
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
    #[serde(skip_serializing_if = "Option::is_none", rename = "response_format")]
    response_format: Option<Value>,
    #[serde(skip_serializing_if = "skip_extra_body", rename = "extra_body")]
    extra_body: Option<Value>,
}

#[derive(Deserialize, Debug, Clone)]
struct StreamUsage {
    #[serde(default)]
    prompt_tokens: Option<u32>,
    #[serde(default)]
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

/// Non-streaming chat/completions result including optional `usage`.
#[derive(Debug)]
pub struct ChatOnceOutput {
    pub text: String,
    pub usage: Option<LlmUsageSnapshot>,
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
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .build()?;
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
        system_prompts: &[String],
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

        let openai_msgs = crate::models::make_openai_messages(
            messages,
            system_prompts,
            crate::models::effective_reasoning_in_messages(&self.settings),
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
            response_format: Some(json!({"type": "json_object"})),
            extra_body,
        };
        let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
        crate::llm_prompt_dump::try_log_openai_chat_request_json(
            &self.settings,
            "chat_once",
            dump_label,
            &url,
            &req,
        );
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(180))
            .build()?;
        let resp = tokio::select! {
            _ = cancel.cancelled() => return Err(anyhow!("cancelled")),
            r = client
                .post(&url)
                .bearer_auth(&self.api_key)
                .json(&req)
                .send() => r?,
        };
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("HTTP {}: {}", status, truncate(&text, 400)));
        }
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
        Ok(ChatOnceOutput { text, usage })
    }

    pub async fn stream_chat(
        &self,
        messages: &[ChatMessage],
        system_prompts: &[String],
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
        let openai_msgs = crate::models::make_openai_messages(
            messages,
            system_prompts,
            crate::models::effective_reasoning_in_messages(&self.settings),
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
            response_format: Some(json!({"type": "json_object"})),
            extra_body,
        };

        let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
        crate::llm_prompt_dump::try_log_openai_chat_request_json(
            &self.settings,
            "stream_chat",
            dump_label,
            &url,
            &req,
        );
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(180))
            .build()?;

        let t_http = Instant::now();
        let resp = tokio::select! {
            _ = cancel.cancelled() => return Err(anyhow!("cancelled")),
            r = client
                .post(&url)
                .bearer_auth(&self.api_key)
                .json(&req)
                .send() => r?,
        };
        let http_until_headers_ms = t_http.elapsed().as_millis();
        log::info!(
            "stream_chat: build_openai_messages_ms={} http_until_response_headers_ms={} api_message_count={} system_prompt_block_count={} dump_label={:?} pre_body_stream_wall_ms={}",
            build_openai_messages_ms,
            http_until_headers_ms,
            api_message_count,
            system_prompts.len(),
            dump_label,
            stream_t0.elapsed().as_millis()
        );

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("HTTP {}: {}", status, truncate(&text, 400)));
        }

        let mut content_buf = String::new();
        let mut reasoning_buf = String::new();
        let mut finish_reason = String::from("stop");
        let mut json_parser = JsonToolParser::new();
        let stream_json_session_id = rand_id();
        let mut accumulated_tool_calls: Vec<ToolCall> = Vec::new();
        let mut last_json_stream_meta: (Option<String>, Option<String>) = (None, None);
        let mut last_json_partial: Option<JsonStreamingPartial> = None;
        let stream_raw_to_console = raw_llm_stream_to_console_enabled();
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
                                write_llm_stream_chunk_to_stderr(c);
                            }
                            content_buf.push_str(c);
                            let _ = tx.send(ProviderEvent::ContentDelta(c.clone())).await;
                            json_parser.feed_lane(c, JsonFeedLane::Content);
                            let partial = extract_json_streaming_partial(&content_buf);
                            if partial.thoughts.is_some()
                                || partial.headline.is_some()
                                || partial.tool_name.is_some()
                                || partial.response_text.is_some()
                            {
                                if last_json_partial.as_ref() != Some(&partial) {
                                    last_json_partial = Some(partial.clone());
                                    let _ = tx
                                        .send(ProviderEvent::AssistantJsonPartial {
                                            thoughts: partial.thoughts,
                                            headline: partial.headline,
                                            tool_name: partial.tool_name,
                                            response_text: partial.response_text,
                                        })
                                        .await;
                                }
                            }
                        }
                    }
                    if let Some(ref r) = ch.delta.reasoning_content {
                        if !r.is_empty() {
                            if stream_raw_to_console {
                                write_llm_stream_chunk_to_stderr(r);
                            }
                            reasoning_buf.push_str(r);
                            json_parser.feed_lane(r, JsonFeedLane::Reasoning);
                            let _ = tx.send(ProviderEvent::ReasoningDelta(r.clone())).await;
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

        let (envelope, mut json_diag) =
            finalize_json_tool_envelope(&content_buf, &reasoning_buf);
        json_diag.feed_lane_tail = json_parser.feed_lane_tail.clone();

        if let Some(env) = envelope {
            let base_id = format!("json_{}_0", stream_json_session_id);
            let (tc, thoughts, headline) =
                tool_calls_from_xml_envelope(&self.settings.model, env, &base_id);
            if thoughts.is_some() {
                last_json_stream_meta.0 = thoughts.clone();
            }
            if headline.is_some() {
                last_json_stream_meta.1 = headline.clone();
            }
            accumulated_tool_calls.extend(tc.iter().cloned());
            let _ = tx
                .send(ProviderEvent::JsonToolStreamingReady {
                    tool_calls: tc,
                    thoughts,
                    headline,
                })
                .await;
        }

        let tool_calls = accumulated_tool_calls.clone();
        let finish_thoughts = last_json_stream_meta.0.clone();
        let finish_headline = last_json_stream_meta.1.clone();

        if !tool_calls.is_empty() {
            json_diag.parse_error = None;
        }

        let _ = tx
            .send(ProviderEvent::Finish {
                reason: finish_reason,
                tool_calls,
                json: json_diag,
                thoughts: finish_thoughts,
                headline: finish_headline,
                usage: last_usage,
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

/// 流式：将模型增量原文连续写到 **stderr**（无换行、无序号前缀）。关闭：`POINTER_STREAM_RAW_LLM_TO_STDOUT=0`。
fn raw_llm_stream_to_console_enabled() -> bool {
    match std::env::var("POINTER_STREAM_RAW_LLM_TO_STDOUT") {
        Ok(v) if v == "0" || v.eq_ignore_ascii_case("false") => false,
        _ => true,
    }
}

fn write_llm_stream_chunk_to_stderr(text: &str) {
    if text.is_empty() {
        return;
    }
    let mut err = std::io::stderr().lock();
    let _ = std::io::Write::write_all(&mut err, text.as_bytes());
    let _ = err.flush();
}

fn tool_calls_from_xml_envelope(
    model: &str,
    envelope: XmlToolEnvelope,
    base_id: &str,
) -> (Vec<ToolCall>, Option<String>, Option<String>) {
    if envelope.sidecar.is_empty() {
        return tool_calls_from_xml_tool_call(
            model,
            envelope.primary,
            Some(format!("{base_id}_p")),
        );
    }
    let mut finish_thoughts = None;
    let t = envelope.primary.thoughts.trim();
    if !t.is_empty() {
        finish_thoughts = Some(t.to_string());
    }
    let mut finish_headline = None;
    let h = envelope.primary.headline.trim();
    if !h.is_empty() {
        finish_headline = Some(h.to_string());
    }

    let mut out: Vec<ToolCall> = Vec::new();
    let mut idx: u32 = 0;
    for sc in envelope.sidecar {
        let id = format!("{base_id}_sc{idx}");
        idx += 1;
        let args_json = xml_tool_arguments_to_json_string(&sc.arguments);
        let name = sc.name.trim().to_string();
        out.push(ToolCall {
            id,
            name,
            arguments: args_json,
            status: "pending".into(),
            result: None,
            error: None,
            duration_ms: None,
            risk_level: None,
        });
    }
    let primary_id = format!("{base_id}_p");
    let args_json = xml_tool_arguments_to_json_string(&envelope.primary.arguments);
    let name = envelope.primary.name.trim().to_string();
    out.push(ToolCall {
        id: primary_id,
        name,
        arguments: args_json,
        status: "pending".into(),
        result: None,
        error: None,
        duration_ms: None,
        risk_level: None,
    });
    (out, finish_thoughts, finish_headline)
}

fn tool_calls_from_xml_tool_call(
    _model: &str,
    xml_call: XmlToolCall,
    tool_call_id: Option<String>,
) -> (Vec<ToolCall>, Option<String>, Option<String>) {
    let mut finish_thoughts = None;
    let t = xml_call.thoughts.trim();
    if !t.is_empty() {
        finish_thoughts = Some(t.to_string());
    }
    let mut finish_headline = None;
    let h = xml_call.headline.trim();
    if !h.is_empty() {
        finish_headline = Some(h.to_string());
    }
    let id = tool_call_id.unwrap_or_else(|| format!("xml_{}", rand_id()));
    let args_json = xml_tool_arguments_to_json_string(&xml_call.arguments);
    let name = xml_call.name.trim().to_string();
    let tc = vec![ToolCall {
        id,
        name,
        arguments: args_json,
        status: "pending".into(),
        result: None,
        error: None,
        duration_ms: None,
        risk_level: None,
    }];
    (tc, finish_thoughts, finish_headline)
}

fn truncate(s: &str, n: usize) -> String {
    if s.len() <= n {
        s.to_string()
    } else {
        format!("{}…", &s[..n])
    }
}

fn rand_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{:x}", n)
}
