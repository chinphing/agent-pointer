use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
    pub status: String,
    #[serde(default)]
    pub result: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default, rename = "durationMs")]
    pub duration_ms: Option<u64>,
    #[serde(default, rename = "riskLevel")]
    pub risk_level: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTrace {
    pub id: String,
    pub name: String,
    pub role: String,
    pub status: String,
    #[serde(default)]
    pub detail: Option<String>,
    #[serde(default)]
    pub content: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: String,
    pub role: Role,
    #[serde(default)]
    pub content: String,
    pub status: String,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
    #[serde(default, rename = "toolCalls")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(default, rename = "toolCallId")]
    pub tool_call_id: Option<String>,
    #[serde(default, rename = "errorMessage")]
    pub error_message: Option<String>,
    #[serde(default)]
    pub reasoning: Option<String>,
    #[serde(default, rename = "agentId")]
    pub agent_id: Option<String>,
    #[serde(default, rename = "agentName")]
    pub agent_name: Option<String>,
    #[serde(default, rename = "agentTrace")]
    pub agent_trace: Option<Vec<AgentTrace>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conversation {
    pub id: String,
    pub title: String,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
    #[serde(rename = "updatedAt")]
    pub updated_at: i64,
    pub messages: Vec<ChatMessage>,
    #[serde(default, rename = "skillIds")]
    pub skill_ids: Vec<String>,
    /// Cumulative tool rounds for **single-agent** replies in this conversation.
    #[serde(default, rename = "toolRoundsUsed")]
    pub tool_rounds_used: u32,
    /// Cumulative tool rounds for **Supervisor** runs (all sub-agents) in this conversation.
    #[serde(default, rename = "toolRoundsUsedSupervisor")]
    pub tool_rounds_used_supervisor: u32,
}

/// Per-model overrides for runtime/API behavior. Unset fields inherit from the parent provider.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModelRuntimeOverrides {
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "reasoningInMessages")]
    pub reasoning_in_messages: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub id: String,
    pub name: String,
    #[serde(rename = "baseUrl")]
    pub base_url: String,
    #[serde(default, rename = "apiKey")]
    pub api_key: String,
    pub models: Vec<String>,
    /// Default for all models under this provider when `model_configs[model]` has no override.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "reasoningInMessages")]
    pub reasoning_in_messages: Option<bool>,
    /// Key = model id string (same as entries in `models`). Values override provider default.
    #[serde(default, rename = "modelConfigs")]
    pub model_configs: HashMap<String, ModelRuntimeOverrides>,
}

/// Whether to persist/stream reasoning and send `reasoning_content` on the next request,
/// for the **active** provider + **current** `settings.model`.
pub fn effective_reasoning_in_messages(settings: &ModelSettings) -> bool {
    let provider = settings
        .providers
        .iter()
        .find(|p| p.id == settings.active_provider_id)
        .or_else(|| settings.providers.first());
    let Some(p) = provider else {
        return true;
    };
    let model = settings.model.trim();
    if let Some(over) = p.model_configs.get(model) {
        if let Some(v) = over.reasoning_in_messages {
            return v;
        }
    }
    p.reasoning_in_messages.unwrap_or(true)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelSettings {
    pub providers: Vec<ProviderConfig>,
    #[serde(rename = "activeProviderId")]
    pub active_provider_id: String,
    pub model: String,
    #[serde(default, rename = "apiKey")]
    pub api_key: String,
    pub temperature: f32,
    #[serde(rename = "maxTokens")]
    pub max_tokens: u32,
    #[serde(rename = "hasKey")]
    pub has_key: bool,
    #[serde(default = "default_tool_approval_mode", rename = "toolApprovalMode")]
    pub tool_approval_mode: String,
    #[serde(default = "default_agent_mode", rename = "agentMode")]
    pub agent_mode: String,
    /// Absolute path to workspace root for file tools & terminal default cwd (optional).
    #[serde(default, rename = "workspaceRoot")]
    pub workspace_root: String,
    /// When agentMode is single, which worker id leads (kebab-case). Empty = default agent.
    #[serde(default, rename = "leadAgentId")]
    pub lead_agent_id: String,
    /// When true, summarize older turns via a separate model call when estimated context exceeds budget.
    #[serde(default = "default_context_compression_enabled", rename = "contextCompressionEnabled")]
    pub context_compression_enabled: bool,
    /// Rough character budget for serialized messages; exceeding triggers compression when enabled.
    #[serde(default = "default_context_budget_chars", rename = "contextBudgetChars")]
    pub context_budget_chars: u32,
    /// Keep this many most recent user messages (and everything after the cutoff) verbatim.
    #[serde(default = "default_context_keep_recent_user_turns", rename = "contextKeepRecentUserTurns")]
    pub context_keep_recent_user_turns: u32,
    /// Max tokens for the one-off summarization chat completion.
    #[serde(default = "default_context_summary_max_tokens", rename = "contextSummaryMaxTokens")]
    pub context_summary_max_tokens: u32,
    /// Max tool-call rounds per assistant turn. Default 100.
    #[serde(default = "default_max_tool_rounds", rename = "maxToolRounds")]
    pub max_tool_rounds: u32,
}

fn default_tool_approval_mode() -> String {
    "auto".into()
}

fn default_agent_mode() -> String {
    "single".into()
}

fn default_context_compression_enabled() -> bool {
    true
}

fn default_context_budget_chars() -> u32 {
    120_000
}

fn default_context_keep_recent_user_turns() -> u32 {
    6
}

fn default_context_summary_max_tokens() -> u32 {
    1024
}

fn default_max_tool_rounds() -> u32 {
    100
}

impl Default for ModelSettings {
    fn default() -> Self {
        Self {
            providers: vec![
                ProviderConfig {
                    id: "qwen".into(),
                    name: "千问".into(),
                    base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1".into(),
                    api_key: String::new(),
                    models: vec![
                        "qwen3.5-plus".into(),
                        "qwen3.6-plus".into(),
                        "qwen3.5-flash".into(),
                        "qwen3.5-27b".into(),
                    ],
                    reasoning_in_messages: None,
                    model_configs: HashMap::new(),
                },
                ProviderConfig {
                    id: "deepseek".into(),
                    name: "深度求索".into(),
                    base_url: "https://api.deepseek.com/v1".into(),
                    api_key: String::new(),
                    models: vec!["deepseek-v4-flash".into(), "deepseek-v4-pro".into()],
                    reasoning_in_messages: Some(true),
                    model_configs: HashMap::new(),
                },
            ],
            active_provider_id: "qwen".into(),
            model: "qwen3.5-plus".into(),
            api_key: String::new(),
            temperature: 0.7,
            max_tokens: 2048,
            has_key: false,
            tool_approval_mode: default_tool_approval_mode(),
            agent_mode: default_agent_mode(),
            workspace_root: String::new(),
            lead_agent_id: String::new(),
            context_compression_enabled: default_context_compression_enabled(),
            context_budget_chars: default_context_budget_chars(),
            context_keep_recent_user_turns: default_context_keep_recent_user_turns(),
            context_summary_max_tokens: default_context_summary_max_tokens(),
            max_tool_rounds: default_max_tool_rounds(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillDef {
    pub id: String,
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    #[serde(rename = "systemPrompt")]
    pub system_prompt: String,
    #[serde(rename = "toolNames")]
    pub tool_names: Vec<String>,
    pub scenario: String,
    pub builtin: bool,
    #[serde(default, rename = "resourceFiles")]
    pub resource_files: Vec<String>,
    #[serde(default)]
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillImportResult {
    pub imported: Vec<SkillDef>,
    pub skipped: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    #[serde(rename = "parametersSchema")]
    pub parameters_schema: serde_json::Value,
    #[serde(rename = "riskLevel")]
    pub risk_level: String,
    #[serde(rename = "requiresApproval")]
    pub requires_approval: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendChatPayload {
    #[serde(rename = "conversationId")]
    pub conversation_id: String,
    pub messages: Vec<ChatMessage>,
    #[serde(default, rename = "enabledSkillIds")]
    pub enabled_skill_ids: Vec<String>,
    #[serde(default, rename = "agentMode")]
    pub agent_mode: Option<String>,
    /// Session cumulative tool rounds (single-agent mode) before this user message.
    #[serde(default, rename = "toolRoundsUsed")]
    pub tool_rounds_used: u32,
    /// Session cumulative tool rounds (Supervisor / sub-agents) before this user message.
    #[serde(default, rename = "toolRoundsUsedSupervisor")]
    pub tool_rounds_used_supervisor: u32,
}

/// Frontend stream event payload (mirrors src/types/chat.ts StreamEvent)
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StreamEvent {
    MessageStart {
        #[serde(rename = "messageId")]
        message_id: String,
        #[serde(rename = "conversationId")]
        conversation_id: String,
    },
    Delta {
        #[serde(rename = "messageId")]
        message_id: String,
        text: String,
    },
    ReasoningDelta {
        #[serde(rename = "messageId")]
        message_id: String,
        text: String,
    },
    AgentStep {
        #[serde(rename = "messageId")]
        message_id: String,
        #[serde(rename = "agent")]
        agent: AgentTrace,
    },
    ToolCallStart {
        #[serde(rename = "messageId")]
        message_id: String,
        #[serde(rename = "toolCall")]
        tool_call: ToolCall,
    },
    ToolCallArgsDelta {
        #[serde(rename = "messageId")]
        message_id: String,
        #[serde(rename = "toolCallId")]
        tool_call_id: String,
        #[serde(rename = "argsDelta")]
        args_delta: String,
    },
    ToolCallStatus {
        #[serde(rename = "messageId")]
        message_id: String,
        #[serde(rename = "toolCallId")]
        tool_call_id: String,
        status: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        result: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none", rename = "durationMs")]
        duration_ms: Option<u64>,
    },
    TerminalOutputDelta {
        #[serde(rename = "messageId")]
        message_id: String,
        #[serde(rename = "toolCallId")]
        tool_call_id: String,
        #[serde(rename = "output")]
        output: String,
    },
    MessageEnd {
        #[serde(rename = "messageId")]
        message_id: String,
    },
    Error {
        #[serde(skip_serializing_if = "Option::is_none", rename = "messageId")]
        message_id: Option<String>,
        message: String,
    },
    Done {
        #[serde(rename = "conversationId")]
        conversation_id: String,
        #[serde(skip_serializing_if = "Option::is_none", rename = "toolRoundsUsedTotal")]
        tool_rounds_used_total: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none", rename = "toolRoundsUsedSupervisorTotal")]
        tool_rounds_used_supervisor_total: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none", rename = "maxToolRounds")]
        max_tool_rounds: Option<u32>,
    },
    HistoryReplaced {
        #[serde(rename = "conversationId")]
        conversation_id: String,
        messages: Vec<ChatMessage>,
    },
    ToolRoundsExhausted {
        #[serde(rename = "conversationId")]
        conversation_id: String,
        #[serde(rename = "maxRounds")]
        max_rounds: u32,
        message: String,
        #[serde(rename = "willRetryAfterCompress")]
        will_retry_after_compress: bool,
    },
}

/// OpenAI-compatible request structures
#[derive(Debug, Clone, Serialize)]
pub struct OpenAIRequest<'a> {
    pub model: &'a str,
    pub messages: Vec<serde_json::Value>,
    pub stream: bool,
    pub temperature: f32,
    pub max_tokens: u32,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<&'a str>,
}

/// OpenAI-compatible APIs require: each `assistant` message that includes `tool_calls` must be
/// immediately followed by one `tool` message per `tool_call_id`. The UI stores tool output on
/// `assistant.toolCalls[].result` and often omits separate `role: tool` rows, so replay would 400.
/// This expands history for the wire format only.
fn expand_tool_messages_for_openai_request(msgs: &[ChatMessage]) -> Vec<ChatMessage> {
    let mut out: Vec<ChatMessage> = Vec::with_capacity(msgs.len());
    let mut i = 0usize;
    while i < msgs.len() {
        let m = &msgs[i];
        if matches!(m.role, Role::Assistant) {
            if let Some(tcs) = &m.tool_calls {
                let required: Vec<&ToolCall> = tcs.iter().filter(|t| !t.id.is_empty()).collect();
                if !required.is_empty() {
                    let mut j = i + 1;
                    while j < msgs.len() && matches!(msgs[j].role, Role::Tool) {
                        j += 1;
                    }
                    let following = &msgs[(i + 1)..j];
                    let mut by_id: HashMap<String, String> = HashMap::new();
                    for tm in following {
                        if let Some(id) = &tm.tool_call_id {
                            if !id.is_empty() {
                                by_id.insert(id.clone(), tm.content.clone());
                            }
                        }
                    }

                    out.push(m.clone());
                    for tc in required {
                        let content = by_id
                            .get(tc.id.as_str())
                            .cloned()
                            .unwrap_or_else(|| synthetic_tool_content_for_replay(tc));
                        out.push(ChatMessage {
                            id: format!("tool_{}", uuid::Uuid::new_v4().simple()),
                            role: Role::Tool,
                            content,
                            status: "done".into(),
                            created_at: m.created_at,
                            tool_calls: None,
                            tool_call_id: Some(tc.id.clone()),
                            error_message: None,
                            reasoning: None,
                            agent_id: None,
                            agent_name: None,
                            agent_trace: None,
                        });
                    }
                    i = j;
                    continue;
                }
            }
        }
        out.push(m.clone());
        i += 1;
    }
    out
}

fn synthetic_tool_content_for_replay(tc: &ToolCall) -> String {
    if let Some(e) = &tc.error {
        if !e.trim().is_empty() {
            return format!("ERROR: {e}");
        }
    }
    tc.result.clone().unwrap_or_else(|| {
        "{\"warning\":\"tool output missing in stored message history\"}".to_string()
    })
}

pub fn make_openai_messages(
    msgs: &[ChatMessage],
    system_prompts: &[String],
    include_reasoning_in_api: bool,
) -> Vec<serde_json::Value> {
    let expanded = expand_tool_messages_for_openai_request(msgs);
    let mut out: Vec<serde_json::Value> = Vec::new();
    if !system_prompts.is_empty() {
        out.push(serde_json::json!({
            "role": "system",
            "content": system_prompts.join("\n\n")
        }));
    }
    for m in &expanded {
        match m.role {
            Role::System => out.push(serde_json::json!({
                "role": "system", "content": m.content
            })),
            Role::User => out.push(serde_json::json!({
                "role": "user", "content": m.content
            })),
            Role::Assistant => {
                let mut obj = serde_json::Map::new();
                obj.insert("role".into(), "assistant".into());
                obj.insert(
                    "content".into(),
                    serde_json::Value::String(m.content.clone()),
                );
                // DeepSeek 等「思考模式」在流式里下发 `reasoning_content`；下一轮请求必须原样带回，
                // 否则 400 — 可由设置 `reasoningInMessages` 关闭（关闭后勿对该类模型开思考）。
                if include_reasoning_in_api {
                    if let Some(ref r) = m.reasoning {
                        if !r.is_empty() {
                            obj.insert(
                                "reasoning_content".into(),
                                serde_json::Value::String(r.clone()),
                            );
                        }
                    }
                }
                // Always serialize every stored tool call. `status` is UI/runtime only; the API
                // requires the preceding assistant message to list all `tool_calls` that have
                // following `role: tool` replies. Backend history often keeps `pending` until
                // the next round (frontend may update to success); filtering by status produced
                // empty `tool_calls` and DeepSeek/OpenAI-compatible servers return 400.
                if let Some(tcs) = &m.tool_calls {
                    let arr: Vec<_> = tcs
                        .iter()
                        .filter(|t| !t.id.is_empty())
                        .map(|t| {
                            serde_json::json!({
                                "id": t.id,
                                "type": "function",
                                "function": {
                                    "name": t.name,
                                    "arguments": t.arguments
                                }
                            })
                        })
                        .collect();
                    if !arr.is_empty() {
                        obj.insert("tool_calls".into(), serde_json::Value::Array(arr));
                    }
                }
                out.push(serde_json::Value::Object(obj));
            }
            Role::Tool => out.push(serde_json::json!({
                "role": "tool",
                "tool_call_id": m.tool_call_id.clone().unwrap_or_default(),
                "content": m.content
            })),
        }
    }
    out
}

#[cfg(test)]
mod make_openai_messages_tests {
    use super::*;

    fn msg(role: Role) -> ChatMessage {
        ChatMessage {
            id: "m".into(),
            role,
            content: String::new(),
            status: "done".into(),
            created_at: 0,
            tool_calls: None,
            tool_call_id: None,
            error_message: None,
            reasoning: None,
            agent_id: None,
            agent_name: None,
            agent_trace: None,
        }
    }

    #[test]
    fn assistant_includes_reasoning_content_when_present() {
        let mut a = msg(Role::Assistant);
        a.content = "answer".into();
        a.reasoning = Some("step 1…".into());
        let out = make_openai_messages(&[a], &[], true);
        assert_eq!(out[0]["role"], "assistant");
        assert_eq!(out[0]["content"], "answer");
        assert_eq!(out[0]["reasoning_content"], "step 1…");
    }

    #[test]
    fn assistant_omits_reasoning_content_when_disabled() {
        let mut a = msg(Role::Assistant);
        a.content = "answer".into();
        a.reasoning = Some("hidden".into());
        let out = make_openai_messages(&[a], &[], false);
        assert!(out[0].as_object().unwrap().get("reasoning_content").is_none());
    }

    #[test]
    fn includes_pending_tool_calls_for_api_replay() {
        let mut a = msg(Role::Assistant);
        a.content = "x".into();
        a.tool_calls = Some(vec![ToolCall {
            id: "call_abc".into(),
            name: "f".into(),
            arguments: "{}".into(),
            status: "pending".into(),
            result: None,
            error: None,
            duration_ms: None,
            risk_level: None,
        }]);
        let mut t = msg(Role::Tool);
        t.tool_call_id = Some("call_abc".into());
        t.content = "{}".into();

        let out = make_openai_messages(&[a, t], &[], true);
        let tcs = out[0]["tool_calls"].as_array().expect("tool_calls");
        assert_eq!(tcs.len(), 1);
        assert_eq!(tcs[0]["id"], "call_abc");
        assert_eq!(out[1]["role"], "tool");
    }

    #[test]
    fn synthesizes_tool_messages_when_only_inline_results_on_assistant() {
        let mut a = msg(Role::Assistant);
        a.content = "calling".into();
        a.tool_calls = Some(vec![ToolCall {
            id: "call_inline".into(),
            name: "read".into(),
            arguments: "{}".into(),
            status: "success".into(),
            result: Some("file body".into()),
            error: None,
            duration_ms: None,
            risk_level: None,
        }]);
        let out = make_openai_messages(&[a], &[], true);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0]["role"], "assistant");
        assert!(out[0]["tool_calls"].as_array().is_some());
        assert_eq!(out[1]["role"], "tool");
        assert_eq!(out[1]["tool_call_id"], "call_inline");
        assert_eq!(out[1]["content"], "file body");
    }
}

pub type ToolMap = HashMap<String, ToolDef>;

#[cfg(test)]
mod effective_reasoning_tests {
    use super::*;

    #[test]
    fn effective_reasoning_defaults_true() {
        let s = ModelSettings::default();
        assert!(effective_reasoning_in_messages(&s));
    }

    #[test]
    fn effective_reasoning_provider_off() {
        let mut s = ModelSettings::default();
        s.providers[0].reasoning_in_messages = Some(false);
        assert!(!effective_reasoning_in_messages(&s));
    }

    #[test]
    fn effective_reasoning_model_overrides_provider() {
        let mut s = ModelSettings::default();
        let m = s.providers[0].models[0].clone();
        s.model = m.clone();
        s.providers[0].reasoning_in_messages = Some(false);
        s.providers[0].model_configs.insert(
            m,
            ModelRuntimeOverrides {
                reasoning_in_messages: Some(true),
            },
        );
        assert!(effective_reasoning_in_messages(&s));
    }
}
