use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
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
    /// Text inside XML `<thoughts>` for the last complete `<response>` in this turn (UI + persistence).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thoughts: Option<String>,
    /// Text inside XML `<headline>` for the last complete `<response>` in this turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headline: Option<String>,
    #[serde(default, rename = "rawContent")]
    pub raw_content: Option<String>,
    #[serde(default, rename = "agentId")]
    pub agent_id: Option<String>,
    #[serde(default, rename = "agentName")]
    pub agent_name: Option<String>,
    #[serde(default, rename = "agentTrace")]
    pub agent_trace: Option<Vec<AgentTrace>>,
    /// PNG (or other) images as raw base64 payloads for vision APIs. Serialized for the UI only when
    /// present; ephemeral computer screen inject uses this without persisting to conversation files.
    #[serde(default, rename = "imagesBase64", skip_serializing_if = "Option::is_none")]
    pub images_base64: Option<Vec<String>>,
    /// Path relative to app `computer-captures/` for this turn’s annotated PNG (lazy UI load); serialized when set.
    #[serde(
        default,
        rename = "computerRoundScreenRelPath",
        skip_serializing_if = "Option::is_none"
    )]
    pub computer_round_screen_rel_path: Option<String>,
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
    /// Selected desktop monitor id for Computer agent (session UX). Empty/None = auto (monitor under cursor).
    #[serde(
        default,
        rename = "computerMonitorId",
        skip_serializing_if = "Option::is_none"
    )]
    pub computer_monitor_id: Option<String>,
}

/// Desktop monitor descriptor for Computer agent screen selection (UI).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputerMonitor {
    /// Stable id derived from monitor bounds: `{left},{top},{width},{height}`.
    pub id: String,
    pub left: i32,
    pub top: i32,
    pub width: i32,
    pub height: i32,
    #[serde(default, rename = "isPrimary")]
    pub is_primary: bool,
}

/// Per-model overrides for runtime/API behavior. Unset fields inherit from the parent provider.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModelRuntimeOverrides {
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "reasoningInMessages")]
    pub reasoning_in_messages: Option<bool>,
    /// Serialized as chat/completions top-level `extra_body` (JSON object). Shallow-merged over the provider default for the active model.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "extraBody")]
    pub extra_body: Option<Value>,
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
    /// Default `extra_body` for chat/completions (JSON object). Merged shallowly with the active model’s `modelConfigs[model].extraBody` when set.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "extraBody")]
    pub extra_body: Option<Value>,
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

/// Build `extra_body` object from legacy `thinkingEnabled` / `thinkingBudget` (disk migration).
pub fn legacy_thinking_to_extra_body(enable: Option<bool>, budget: Option<u32>) -> Option<Value> {
    if enable.is_none() && budget.is_none() {
        return None;
    }
    let mut m = Map::new();
    if let Some(b) = enable {
        m.insert("enable_thinking".into(), Value::Bool(b));
    }
    if let Some(n) = budget.filter(|&n| n > 0) {
        m.insert(
            "thinking_budget".into(),
            Value::Number(serde_json::Number::from(n)),
        );
    }
    if m.is_empty() {
        None
    } else {
        Some(Value::Object(m))
    }
}

/// Shallow-merge two JSON objects; `overlay` keys replace `base`. If one side is not an object, returns a clone of the non-base side when possible.
pub fn merge_shallow_json_objects(base: Option<&Value>, overlay: Option<&Value>) -> Option<Value> {
    match (base, overlay) {
        (None, None) => None,
        (Some(b), None) => Some(b.clone()),
        (None, Some(o)) => Some(o.clone()),
        (Some(b), Some(o)) => match (b, o) {
            (Value::Object(a), Value::Object(c)) => {
                let mut out = a.clone();
                for (k, v) in c {
                    out.insert(k.clone(), v.clone());
                }
                Some(Value::Object(out))
            }
            (Value::Object(_), _) => Some(o.clone()),
            (_, Value::Object(_)) => Some(o.clone()),
            _ => Some(o.clone()),
        },
    }
}

/// Merged `extra_body` for **active** provider + **current** `settings.model` (model object keys win).
pub fn effective_chat_extra_body(settings: &ModelSettings) -> Option<Value> {
    let provider = settings
        .providers
        .iter()
        .find(|p| p.id == settings.active_provider_id)
        .or_else(|| settings.providers.first())?;
    let model = settings.model.trim();
    let prov = provider.extra_body.as_ref();
    let mdl = provider
        .model_configs
        .get(model)
        .and_then(|x| x.extra_body.as_ref());
    merge_shallow_json_objects(prov, mdl)
}

/// Per-agent default LLM routing: explicit provider + model (no inferring provider from model id).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentModelRef {
    #[serde(rename = "providerId")]
    pub provider_id: String,
    pub model: String,
}

impl AgentModelRef {
    pub fn from_json_value_flexible(v: serde_json::Value) -> Option<Self> {
        use serde_json::Value;
        match v {
            Value::String(s) => {
                if s.trim().is_empty() {
                    return None;
                }
                Some(Self {
                    provider_id: String::new(),
                    model: s,
                })
            }
            Value::Object(map) => {
                let pid = map
                    .get("providerId")
                    .or_else(|| map.get("provider_id"))
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .trim();
                let model = map.get("model").and_then(|x| x.as_str()).unwrap_or("").trim();
                if model.is_empty() {
                    return None;
                }
                Some(Self {
                    provider_id: pid.to_string(),
                    model: model.to_string(),
                })
            }
            _ => None,
        }
    }

    pub fn ensure_provider_or(&mut self, fallback_active_provider: &str) {
        if self.provider_id.trim().is_empty() {
            self.provider_id = fallback_active_provider.trim().to_string();
        }
    }
}

fn deserialize_agent_default_models<'de, D>(
    deserializer: D,
) -> Result<HashMap<String, AgentModelRef>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw: HashMap<String, serde_json::Value> = HashMap::deserialize(deserializer)?;
    Ok(raw
        .into_iter()
        .filter_map(|(k, v)| AgentModelRef::from_json_value_flexible(v).map(|r| (k, r)))
        .collect())
}

fn serialize_agent_default_models<S>(
    map: &HashMap<String, AgentModelRef>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    use serde::ser::SerializeMap;
    let mut m = serializer.serialize_map(Some(map.len()))?;
    for (k, v) in map {
        m.serialize_entry(k, v)?;
    }
    m.end()
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
    /// When true, chat UI shows the assistant “原始输出” inspector (code icon); includes wire text and API reasoning for debug, not inline in the bubble.
    #[serde(default = "default_raw_content_view_enabled", rename = "rawContentViewEnabled")]
    pub raw_content_view_enabled: bool,
    /// When true, each LLM round writes request `messages` + params under app data `logs/llm_prompts/`.
    #[serde(default, rename = "debugDumpLlmPrompts")]
    pub debug_dump_llm_prompts: bool,
    /// Per-agent default LLM: worker id or `"supervisor"` → explicit provider + model.
    #[serde(default, rename = "agentDefaultModels", deserialize_with = "deserialize_agent_default_models", serialize_with = "serialize_agent_default_models")]
    pub agent_default_models: HashMap<String, AgentModelRef>,
}

pub fn ensure_agent_model_refs_have_provider(settings: &mut ModelSettings) {
    let ap = settings.active_provider_id.clone();
    for v in settings.agent_default_models.values_mut() {
        v.ensure_provider_or(&ap);
    }
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

fn default_raw_content_view_enabled() -> bool {
    true
}

fn default_debug_dump_llm_prompts() -> bool {
    false
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
                    extra_body: None,
                },
                ProviderConfig {
                    id: "deepseek".into(),
                    name: "深度求索".into(),
                    base_url: "https://api.deepseek.com/v1".into(),
                    api_key: String::new(),
                    models: vec!["deepseek-v4-flash".into(), "deepseek-v4-pro".into()],
                    reasoning_in_messages: Some(true),
                    model_configs: HashMap::new(),
                    extra_body: None,
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
            raw_content_view_enabled: default_raw_content_view_enabled(),
            debug_dump_llm_prompts: default_debug_dump_llm_prompts(),
            agent_default_models: HashMap::new(),
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

/// Tool identity exposed to the UI / API. Human-readable docs and argument shapes live in markdown
/// (`tools/prompts/*.md`, `agents/computer/tools/prompts/*.md`). Native OpenAI `tools` payloads use
/// empty `parameters` objects; XML tool calling carries real argument structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDef {
    pub name: String,
}

/// Annotated desktop screenshot for UI preview (same style as model vision inject).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputerAnnotatedPreview {
    #[serde(rename = "imageBase64")]
    pub image_base64: String,
    pub caption: String,
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
    RawContentDelta {
        #[serde(rename = "messageId")]
        message_id: String,
        text: String,
    },
    ReasoningDelta {
        #[serde(rename = "messageId")]
        message_id: String,
        text: String,
    },
    /// 正文 XML 工具块尚未闭合时，已能读出的 `<thoughts>` / `<headline>` / `<tool_name>`（流式渐进更新）。
    AssistantXmlPartial {
        #[serde(rename = "messageId")]
        message_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        thoughts: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        headline: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none", rename = "toolName")]
        tool_name: Option<String>,
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
        /// 与持久化助手消息对齐的最终正文（已去掉 XML 工具块等）
        #[serde(skip_serializing_if = "Option::is_none")]
        content: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none", rename = "rawContent")]
        raw_content: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        thoughts: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        headline: Option<String>,
    },
    /// Synthetic user row so the model (and UI history) see recovery instructions mid-run.
    InjectedUserMessage {
        #[serde(rename = "conversationId")]
        conversation_id: String,
        #[serde(rename = "messageId")]
        message_id: String,
        content: String,
    },
    /// Short assistant-role line in the thread (e.g. desktop capture status); not from the model.
    InjectedAssistantMessage {
        #[serde(rename = "conversationId")]
        conversation_id: String,
        #[serde(rename = "messageId")]
        message_id: String,
        content: String,
    },
    /// Replace `content` of an existing [`InjectedAssistantMessage`] with the same `message_id`.
    InjectedAssistantMessageUpdate {
        #[serde(rename = "conversationId")]
        conversation_id: String,
        #[serde(rename = "messageId")]
        message_id: String,
        content: String,
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
    /// Ephemeral UI hint only (not persisted, not sent to the model).
    UiToast {
        #[serde(rename = "conversationId")]
        conversation_id: String,
        message: String,
        /// e.g. `success`, `error`, `warning`
        level: String,
    },
    /// Annotated screen for a specific assistant message (this LLM round’s inject).
    AssistantRoundScreen {
        #[serde(rename = "conversationId")]
        conversation_id: String,
        #[serde(rename = "messageId")]
        message_id: String,
        /// Path relative to `{data_dir}/PointerApp/computer-captures/` (annotated PNG).
        #[serde(rename = "annotatedRelPath")]
        annotated_rel_path: String,
    },
}

/// Channel used to push [`StreamEvent`] updates to the Pointer UI (Tauri / web SSE).
pub type ChatStreamSender = tokio::sync::mpsc::UnboundedSender<StreamEvent>;

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

/// The UI stores tool output on `assistant.toolCalls[].result` and often omits separate `role: tool`
/// rows. We first **expand** to canonical assistant + synthetic `role: tool` rows (one per call id),
/// then **`flatten_tool_rounds_computer_style_for_api`** matches PyProjects/Computer: assistant keeps
/// full model text (`raw_content` if set, else `content`); each tool becomes a **`user`** message
/// with JSON `{"tool_name","tool_result"}`. The **`response`** tool is excluded (Python
/// `ResponseTool.after_execution` does not call `hist_add_tool_result`). No OpenAI-native `tool_calls`
/// / `role: tool` in HTTP JSON.
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
                        if tool_registry_base_name(&tc.name) == "response" {
                            continue;
                        }
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

fn tool_registry_base_name(name: &str) -> &str {
    match name.trim().split_once(':') {
        Some((base, rest)) if !base.is_empty() && !rest.trim().is_empty() => base.trim(),
        _ => name.trim(),
    }
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

/// Body text sent as assistant `content` on the wire (full XML / model output when available).
fn assistant_wire_content(m: &ChatMessage) -> String {
    if let Some(ref r) = m.raw_content {
        if !r.trim().is_empty() {
            return r.clone();
        }
    }
    m.content.clone()
}

/// Align with PyProjects `Agent.hist_add_ai_response` + `hist_add_tool_result`: assistant message
/// then **user** messages carrying `{"tool_name","tool_result"}` JSON (see `python/helpers/tool.py`).
fn flatten_tool_rounds_computer_style_for_api(msgs: &[ChatMessage]) -> Vec<ChatMessage> {
    let mut out: Vec<ChatMessage> = Vec::with_capacity(msgs.len());
    let mut i = 0usize;
    while i < msgs.len() {
        let m = &msgs[i];
        if matches!(m.role, Role::Assistant) {
            if let Some(ref tcs) = m.tool_calls {
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

                    let mut a = m.clone();
                    a.content = assistant_wire_content(m);
                    a.tool_calls = None;
                    a.raw_content = None;
                    out.push(a);

                    for tc in required {
                        if tool_registry_base_name(&tc.name) == "response" {
                            continue;
                        }
                        let body = by_id
                            .get(tc.id.as_str())
                            .cloned()
                            .unwrap_or_else(|| synthetic_tool_content_for_replay(tc));
                        let payload = serde_json::json!({
                            "tool_name": tc.name,
                            "tool_result": body
                        });
                        out.push(ChatMessage {
                            id: format!("tool_result_{}", tc.id),
                            role: Role::User,
                            content: payload.to_string(),
                            status: "done".into(),
                            created_at: m.created_at,
                            tool_calls: None,
                            tool_call_id: None,
                            error_message: None,
                            reasoning: None,
                            thoughts: None,
                            headline: None,
                            raw_content: None,
                            agent_id: m.agent_id.clone(),
                            agent_name: m.agent_name.clone(),
                            agent_trace: None,
                            images_base64: None,
                            computer_round_screen_rel_path: None,
                        });
                    }
                    i = j;
                    continue;
                }
            }
        }
        if matches!(m.role, Role::Tool) {
            let mut u = m.clone();
            u.role = Role::User;
            u.content = format!(
                "(orphan tool output, call_id={:?})\n{}",
                m.tool_call_id, m.content
            );
            u.tool_call_id = None;
            out.push(u);
            i += 1;
            continue;
        }
        let mut m2 = m.clone();
        if matches!(m2.role, Role::Assistant) {
            m2.content = assistant_wire_content(m);
            m2.tool_calls = None;
            m2.raw_content = None;
        }
        out.push(m2);
        i += 1;
    }
    out
}

pub fn make_openai_messages(
    msgs: &[ChatMessage],
    system_prompts: &[String],
    include_reasoning_in_api: bool,
) -> Vec<serde_json::Value> {
    let expanded = expand_tool_messages_for_openai_request(msgs);
    let flattened = flatten_tool_rounds_computer_style_for_api(&expanded);
    let mut out: Vec<serde_json::Value> = Vec::new();
    if !system_prompts.is_empty() {
        out.push(serde_json::json!({
            "role": "system",
            "content": system_prompts.join("\n\n")
        }));
    }
    for m in &flattened {
        match m.role {
            Role::System => out.push(serde_json::json!({
                "role": "system", "content": m.content
            })),
            Role::User => {
                if let Some(ref imgs) = m.images_base64 {
                    if !imgs.is_empty() {
                        let mut parts: Vec<serde_json::Value> = Vec::new();
                        if !m.content.trim().is_empty() {
                            parts.push(serde_json::json!({
                                "type": "text",
                                "text": m.content
                            }));
                        }
                        for b64 in imgs {
                            let url = format!("data:image/png;base64,{b64}");
                            parts.push(serde_json::json!({
                                "type": "image_url",
                                "image_url": { "url": url }
                            }));
                        }
                        out.push(serde_json::json!({
                            "role": "user",
                            "content": parts
                        }));
                        continue;
                    }
                }
                out.push(serde_json::json!({
                    "role": "user", "content": m.content
                }));
            }
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
                // 不在此序列化 `tool_calls`：工具结果已拆成后续 `user` JSON 消息。
                debug_assert!(
                    m.tool_calls.as_ref().map(|t| t.is_empty()).unwrap_or(true),
                    "assistant should not carry tool_calls after flatten"
                );
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

    #[test]
    fn user_message_with_images_uses_multipart_content() {
        let mut u = msg(Role::User);
        u.content = "see screen".into();
        u.images_base64 = Some(vec!["iVBORw0KGgo=".into()]);
        let out = make_openai_messages(&[u], &[], false);
        assert_eq!(out.len(), 1);
        let content = out[0]["content"].as_array().expect("multipart content");
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[1]["type"], "image_url");
        assert!(content[1]["image_url"]["url"]
            .as_str()
            .unwrap()
            .starts_with("data:image/png;base64,"));
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
    fn assistant_then_user_json_per_computer_style() {
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
        assert_eq!(out.len(), 2, "assistant + user(tool_result)");
        assert_eq!(out[0]["role"], "assistant");
        assert!(out[0].as_object().unwrap().get("tool_calls").is_none());
        assert_eq!(out[0]["content"], "x");
        assert_eq!(out[1]["role"], "user");
        let u: serde_json::Value =
            serde_json::from_str(out[1]["content"].as_str().unwrap()).unwrap();
        assert_eq!(u["tool_name"], "f");
        assert_eq!(u["tool_result"], "{}");
    }

    #[test]
    fn synthesizes_inline_tool_as_user_json_after_expand() {
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
        assert_eq!(out[0]["content"], "calling");
        assert_eq!(out[1]["role"], "user");
        let u: serde_json::Value =
            serde_json::from_str(out[1]["content"].as_str().unwrap()).unwrap();
        assert_eq!(u["tool_name"], "read");
        assert_eq!(u["tool_result"], "file body");
    }

    #[test]
    fn assistant_prefers_raw_content_on_wire() {
        let mut a = msg(Role::Assistant);
        a.content = "visible".into();
        a.raw_content = Some("<response><tool_name>x</tool_name></response>".into());
        a.tool_calls = Some(vec![ToolCall {
            id: "c1".into(),
            name: "wait".into(),
            arguments: "{}".into(),
            status: "success".into(),
            result: Some("done".into()),
            error: None,
            duration_ms: None,
            risk_level: None,
        }]);
        let out = make_openai_messages(&[a], &[], false);
        assert_eq!(out[0]["content"], "<response><tool_name>x</tool_name></response>");
    }

    #[test]
    fn response_tool_has_no_user_tool_result_message() {
        let mut a = msg(Role::Assistant);
        a.content = "".into();
        a.raw_content = Some("<response><tool_name>response</tool_name></response>".into());
        a.tool_calls = Some(vec![ToolCall {
            id: "c_resp".into(),
            name: "response".into(),
            arguments: r#"{"text":"Hi"}"#.into(),
            status: "success".into(),
            result: None,
            error: None,
            duration_ms: None,
            risk_level: None,
        }]);

        let out = make_openai_messages(&[a], &[], false);
        assert_eq!(out.len(), 1, "assistant only, like PyProjects response tool");
        assert_eq!(out[0]["role"], "assistant");
        assert_eq!(
            out[0]["content"],
            "<response><tool_name>response</tool_name></response>"
        );
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
                ..Default::default()
            },
        );
        assert!(effective_reasoning_in_messages(&s));
    }
}

#[cfg(test)]
mod effective_extra_body_tests {
    use super::*;

    #[test]
    fn effective_extra_body_none_by_default() {
        let s = ModelSettings::default();
        assert!(effective_chat_extra_body(&s).is_none());
    }

    #[test]
    fn merge_provider_then_model() {
        let mut s = ModelSettings::default();
        s.model = s.providers[0].models[0].clone();
        s.providers[0].extra_body = Some(serde_json::json!({"enable_thinking": true, "thinking_budget": 100}));
        let m = s.model.clone();
        s.providers[0].model_configs.insert(
            m,
            ModelRuntimeOverrides {
                extra_body: Some(serde_json::json!({"thinking_budget": 500})),
                ..Default::default()
            },
        );
        let v = effective_chat_extra_body(&s).expect("merged");
        let o = v.as_object().unwrap();
        assert_eq!(o.get("enable_thinking"), Some(&Value::Bool(true)));
        assert_eq!(
            o.get("thinking_budget"),
            Some(&Value::Number(500.into()))
        );
    }
}
