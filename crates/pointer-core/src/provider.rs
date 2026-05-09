use crate::models::{ChatMessage, ModelSettings, ToolCall};
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
    },
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: Vec<Value>,
    stream: bool,
    temperature: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<&'a str>,
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
        tools: Vec<Value>,
        cancel: CancellationToken,
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

        let openai_msgs = crate::models::make_openai_messages(messages, system_prompts);
        let has_tools = !tools.is_empty();
        let req = ChatRequest {
            model: &self.settings.model,
            messages: openai_msgs,
            stream: false,
            temperature: self.settings.temperature,
            max_tokens: Some(self.settings.max_tokens),
            tools,
            tool_choice: if has_tools { Some("auto") } else { None },
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
        Ok(message
            .content
            .or(message.reasoning_content)
            .unwrap_or_default())
    }

    pub async fn stream_chat(
        &self,
        messages: &[ChatMessage],
        system_prompts: &[String],
        tools: Vec<Value>,
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

        let openai_msgs = crate::models::make_openai_messages(messages, system_prompts);
        let has_tools = !tools.is_empty();
        let req = ChatRequest {
            model: &self.settings.model,
            messages: openai_msgs,
            stream: true,
            temperature: self.settings.temperature,
            max_tokens: Some(self.settings.max_tokens),
            tools,
            tool_choice: if has_tools { Some("auto") } else { None },
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

        // 累积工具调用：(id, name, args_string)
        let mut tool_acc: Vec<(String, String, String)> = Vec::new();
        let mut finish_reason = String::from("stop");

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
                            let _ = tx.send(ProviderEvent::ContentDelta(c)).await;
                        }
                    }
                    if let Some(r) = ch.delta.reasoning_content {
                        if !r.is_empty() {
                            let _ = tx.send(ProviderEvent::ReasoningDelta(r)).await;
                        }
                    }
                    if let Some(tcs) = ch.delta.tool_calls {
                        for tc in tcs {
                            let idx = tc.index as usize;
                            while tool_acc.len() <= idx {
                                tool_acc.push((String::new(), String::new(), String::new()));
                            }
                            let entry = &mut tool_acc[idx];
                            let mut started_now = false;
                            if let Some(id) = tc.id.as_ref() {
                                if !id.is_empty() && entry.0.is_empty() {
                                    entry.0 = id.clone();
                                }
                            }
                            if let Some(f) = tc.function.as_ref() {
                                if let Some(name) = f.name.as_ref() {
                                    if !name.is_empty() && entry.1.is_empty() {
                                        entry.1 = name.clone();
                                        started_now = true;
                                    }
                                }
                            }
                            if started_now && !entry.0.is_empty() {
                                let _ = tx
                                    .send(ProviderEvent::ToolCallStart {
                                        index: tc.index,
                                        id: entry.0.clone(),
                                        name: entry.1.clone(),
                                    })
                                    .await;
                            }
                            if let Some(f) = tc.function {
                                if let Some(args) = f.arguments {
                                    if !args.is_empty() {
                                        entry.2.push_str(&args);
                                        let _ = tx
                                            .send(ProviderEvent::ToolCallArgsDelta {
                                                index: tc.index,
                                                tool_call_id: entry.0.clone(),
                                                args,
                                            })
                                            .await;
                                    }
                                }
                            }
                        }
                    }
                    if let Some(reason) = ch.finish_reason {
                        finish_reason = reason;
                    }
                }
            }
        }

        let tool_calls: Vec<ToolCall> = tool_acc
            .into_iter()
            .filter(|(id, name, _)| !id.is_empty() || !name.is_empty())
            .map(|(id, name, args)| ToolCall {
                id: if id.is_empty() {
                    format!("call_{}", rand_id())
                } else {
                    id
                },
                name,
                arguments: if args.is_empty() { "{}".into() } else { args },
                status: "pending".into(),
                result: None,
                error: None,
                duration_ms: None,
                risk_level: None,
            })
            .collect();

        let _ = tx
            .send(ProviderEvent::Finish {
                reason: finish_reason,
                tool_calls,
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
