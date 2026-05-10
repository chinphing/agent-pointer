use crate::models::{ChatMessage, ModelSettings, ToolCall};
use crate::xml_tool_caller::{
    xml_tool_arguments_to_json_string, XmlToolFinishDiagnostics, XmlToolParser,
};
use anyhow::{anyhow, Result};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;
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
    Finish {
        reason: String,
        tool_calls: Vec<ToolCall>,
        xml: XmlToolFinishDiagnostics,
    },
}

/// 所有 chat/completions 请求均附带：空 `tools` + `tool_choice: "none"`，
/// 显式关闭服务商原生 function calling（本应用仅解析 assistant 正文中的 XML 工具协议）。
#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: Vec<Value>,
    stream: bool,
    temperature: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    tools: &'static [Value],
    tool_choice: &'static str,
}

#[derive(Deserialize, Debug)]
struct StreamChunk {
    #[serde(default)]
    choices: Vec<StreamChoice>,
}
#[derive(Deserialize, Debug)]
struct StreamChoice {
    #[serde(default)]
    delta: StreamDelta,
    #[serde(default)]
    finish_reason: Option<String>,
}
#[derive(Deserialize, Debug)]
struct ChatResponse {
    #[serde(default)]
    choices: Vec<ChatChoice>,
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
    #[serde(default)]
    tool_calls: Option<Vec<StreamToolCall>>,
}
#[derive(Deserialize, Debug)]
struct StreamToolCall {
    index: u32,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    function: Option<StreamFn>,
}
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
            "max_tokens": 4,
            "tools": [],
            "tool_choice": "none"
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
    ) -> Result<String> {
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
        let req = ChatRequest {
            model: &self.settings.model,
            messages: openai_msgs,
            stream: false,
            temperature: self.settings.temperature,
            max_tokens: Some(max_tokens_override.unwrap_or(self.settings.max_tokens)),
            tools: &[],
            tool_choice: "none",
        };
        let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
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
        let parsed: ChatResponse = resp.json().await?;
        let message = parsed
            .choices
            .into_iter()
            .next()
            .map(|choice| choice.message)
            .ok_or_else(|| anyhow!("模型未返回候选结果"))?;
        let out = message
            .content
            .clone()
            .or_else(|| message.reasoning_content.clone())
            .unwrap_or_default();
        log::debug!(
            "model={} chat_once (non-stream) raw_chars={}\n--- raw body ---\n{}\n--- end ---",
            self.settings.model,
            out.chars().count(),
            out
        );
        Ok(out)
    }

    pub async fn stream_chat(
        &self,
        messages: &[ChatMessage],
        system_prompts: &[String],
        tx: mpsc::Sender<ProviderEvent>,
        cancel: CancellationToken,
    ) -> Result<()> {
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
        let req = ChatRequest {
            model: &self.settings.model,
            messages: openai_msgs,
            stream: true,
            temperature: self.settings.temperature,
            max_tokens: Some(self.settings.max_tokens),
            tools: &[],
            tool_choice: "none",
        };

        let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
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

        let mut content_buf = String::new();
        // 仅用于调试日志：与 content 流并列的 reasoning 流
        let mut reasoning_buf = String::new();
        let mut finish_reason = String::from("stop");
        let mut xml_parser = XmlToolParser::new();

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
                for ch in parsed.choices {
                    if let Some(c) = ch.delta.content {
                        if !c.is_empty() {
                            content_buf.push_str(&c);
                            xml_parser.feed(&c);
                            let _ = tx.send(ProviderEvent::ContentDelta(c)).await;
                        }
                    }
                    if let Some(r) = ch.delta.reasoning_content {
                        if !r.is_empty() {
                            reasoning_buf.push_str(&r);
                            // 部分模型把工具 XML 写在 reasoning_content、正文 content 为空；必须一并喂给解析器，否则会话在无工具调用下提前结束。
                            xml_parser.feed(&r);
                            let _ = tx.send(ProviderEvent::ReasoningDelta(r)).await;
                        }
                    }
                    if let Some(ref tcs) = ch.delta.tool_calls {
                        if !tcs.is_empty() {
                            log::debug!(
                                "ignoring provider-native delta.tool_calls (count={}); requests use tool_choice=none",
                                tcs.len()
                            );
                        }
                    }
                    if let Some(reason) = ch.finish_reason {
                        finish_reason = reason;
                    }
                }
            }
        }

        log::debug!(
            "model={} finish_reason={} raw_content_chars={} raw_reasoning_chars={}\n--- raw content ---\n{}\n--- end raw content ---",
            self.settings.model,
            finish_reason,
            content_buf.chars().count(),
            reasoning_buf.chars().count(),
            content_buf,
        );
        if !reasoning_buf.is_empty() {
            log::debug!(
                "--- raw reasoning_content ---\n{}\n--- end raw reasoning ---",
                reasoning_buf,
            );
        }

        let attempted_tool_xml = content_buf.contains("<tool_name>")
            || content_buf.contains("<tool_args>")
            || reasoning_buf.contains("<tool_name>")
            || reasoning_buf.contains("<tool_args>");

        let xml_complete = xml_parser.is_complete();
        let tool_calls = if xml_complete {
            if let Some(xml_call) = xml_parser.parse() {
                let id = format!("xml_{}", rand_id());
                let args_json = xml_tool_arguments_to_json_string(&xml_call.arguments);
                vec![ToolCall {
                    id,
                    name: xml_call.name,
                    arguments: args_json,
                    status: "pending".into(),
                    result: None,
                    error: None,
                    duration_ms: None,
                    risk_level: None,
                }]
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        let mut parse_error = None;
        if xml_complete && tool_calls.is_empty() {
            parse_error = xml_parser.last_parse_error().map(str::to_string);
        }

        let xml = XmlToolFinishDiagnostics {
            attempted_tool_xml,
            fragment_complete: xml_complete,
            parse_error,
        };

        if tool_calls.is_empty() {
            let saw_response_markup = content_buf.contains("<response>")
                || content_buf.contains("</response>")
                || reasoning_buf.contains("<response>")
                || reasoning_buf.contains("</response>");
            if saw_response_markup && !xml_complete {
                log::warn!(
                    "model={} xml tool: stream ended without a complete closing </response> (tool_choice=none; empty tool_calls). If you see xml_tool_caller::parse_response_xml failed above, the fragment was complete but invalid XML.",
                    self.settings.model
                );
            }
        }

        let _ = tx
            .send(ProviderEvent::Finish {
                reason: finish_reason,
                tool_calls,
                xml,
            })
            .await;
        Ok(())
    }
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
