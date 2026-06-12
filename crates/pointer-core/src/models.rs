use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashMap;

use crate::agents::computer::tier::{
    ADVANCED_THINKING_BUDGET, DEFAULT_MODEL_ADVANCED, DEFAULT_MODEL_INTERMEDIATE,
    DEFAULT_MODEL_PRIMARY, PRIMARY_INTERMEDIATE_THINKING_BUDGET,
};

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
    /// UI-only Chinese label (not sent to the LLM).
    #[serde(default, rename = "displayLabel", skip_serializing_if = "Option::is_none")]
    pub display_label: Option<String>,
    /// UI-only short parameter summary (not sent to the LLM).
    #[serde(default, rename = "displaySummary", skip_serializing_if = "Option::is_none")]
    pub display_summary: Option<String>,
}

/// Sub-agent collapsed-header counters (frontend-only; persisted with conversations).
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SubAgentToolStats {
    #[serde(default)]
    pub search_count: u32,
    #[serde(default)]
    pub read_count: u32,
    #[serde(default)]
    pub write_count: u32,
    #[serde(default)]
    pub terminal_count: u32,
    #[serde(default)]
    pub web_search_count: u32,
    #[serde(default)]
    pub mouse_count: u32,
    #[serde(default)]
    pub input_count: u32,
    #[serde(default)]
    pub other_count: u32,
}

/// Sub-agent streaming UI state (tool calls, thoughts, collapsed summary); not sent to the LLM.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SubAgentSessionUi {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thoughts: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headline: Option<String>,
    #[serde(default, rename = "toolNamePreview", skip_serializing_if = "Option::is_none")]
    pub tool_name_preview: Option<String>,
    #[serde(default, rename = "responseTextDraft", skip_serializing_if = "Option::is_none")]
    pub response_text_draft: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    #[serde(default, rename = "rawContent", skip_serializing_if = "Option::is_none")]
    pub raw_content: Option<String>,
    #[serde(default, rename = "contentStreaming")]
    pub content_streaming: bool,
    #[serde(default, rename = "toolCalls", skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(default)]
    pub stats: SubAgentToolStats,
    #[serde(default, rename = "summaryLine", skip_serializing_if = "Option::is_none")]
    pub summary_line: Option<String>,
    #[serde(default)]
    pub collapsed: bool,
    #[serde(default, rename = "userExpanded")]
    pub user_expanded: bool,
}

/// Whether a delegated `computer` sub-task automates Pointer itself or external apps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComputerOperationTarget {
    #[serde(rename = "self")]
    SelfApp,
    External,
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
    /// UI indentation: 0 = top-level (lead / supervisor), 1 = delegated sub-agent step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depth: Option<u32>,
    /// Delegated sub-agent UI session (tool rows, stats, collapsed state).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<SubAgentSessionUi>,
    /// Set on `run_subagent` → `computer` traces; controls dock-bar shrink in the desktop client.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "computerTarget")]
    pub computer_target: Option<ComputerOperationTarget>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageUiBindings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_board_anchor: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExcludedReason {
    ContextCompression,
    TaskBoardTrim,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageContextState {
    pub included: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub excluded_reason: Option<ExcludedReason>,
}

impl Default for MessageContextState {
    fn default() -> Self {
        Self {
            included: true,
            excluded_reason: None,
        }
    }
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
    /// User-visible reasoning summary from the model’s last structured turn (`thoughts` in JSON, or legacy XML).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thoughts: Option<String>,
    /// Short title from the model’s last structured turn (`headline` in JSON, or legacy XML).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headline: Option<String>,
    #[serde(default, rename = "rawContent")]
    pub raw_content: Option<String>,
    #[serde(
        default,
        rename = "toolRawOutput",
        skip_serializing_if = "Option::is_none"
    )]
    pub tool_raw_output: Option<String>,
    #[serde(default, rename = "agentId")]
    pub agent_id: Option<String>,
    /// Runtime agent launch UUID (one per lead / sub-agent invocation).
    #[serde(default, rename = "agentInstanceId", skip_serializing_if = "Option::is_none")]
    pub agent_instance_id: Option<String>,
    #[serde(default, rename = "agentName")]
    pub agent_name: Option<String>,
    #[serde(default, rename = "agentTrace")]
    pub agent_trace: Option<Vec<AgentTrace>>,
    /// PNG (or other) images as raw base64 payloads for vision APIs. Serialized for the UI only when
    /// present; ephemeral computer screen inject uses this without persisting to conversation files.
    #[serde(default, rename = "imagesBase64", skip_serializing_if = "Option::is_none")]
    pub images_base64: Option<Vec<String>>,
    /// Slot labels prepended in the API request immediately before each `images_base64` entry (same length).
    #[serde(
        default,
        rename = "imageSlotLabels",
        skip_serializing_if = "Option::is_none"
    )]
    pub image_slot_labels: Option<Vec<String>>,
    /// Path relative to app `computer-captures/` for this turn’s annotated JPEG (lazy UI load); serialized when set.
    #[serde(
        default,
        rename = "computerRoundScreenRelPath",
        skip_serializing_if = "Option::is_none"
    )]
    pub computer_round_screen_rel_path: Option<String>,
    /// UI mount hints (e.g. TaskBoard anchor); persisted with conversation.
    #[serde(default, rename = "uiBindings", skip_serializing_if = "Option::is_none")]
    pub ui_bindings: Option<MessageUiBindings>,
    /// Whether this message is included in LLM context.
    #[serde(default, rename = "contextState", skip_serializing_if = "Option::is_none")]
    pub context_state: Option<MessageContextState>,
    /// User-attached files/images (metadata persisted; base64 wire-only via `contentBase64`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attachments: Option<Vec<MediaAttachment>>,
}

/// User message attachment (Composer / channels).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaAttachment {
    pub id: String,
    /// `image`, `document`, `audio`, `file`
    pub kind: String,
    #[serde(rename = "mimeType")]
    pub mime_type: String,
    #[serde(rename = "fileName")]
    pub file_name: String,
    #[serde(default, rename = "sizeBytes")]
    pub size_bytes: u64,
    #[serde(default, rename = "storageRelPath", skip_serializing_if = "Option::is_none")]
    pub storage_rel_path: Option<String>,
    /// Wire-only payload; stripped before conversation persist.
    #[serde(default, rename = "contentBase64", skip_serializing_if = "Option::is_none")]
    pub content_base64: Option<String>,
    #[serde(default, rename = "derivedText", skip_serializing_if = "Option::is_none")]
    pub derived_text: Option<String>,
    /// Absolute local path for assistant reply `MEDIA:` preview in App UI.
    #[serde(default, rename = "localAbsPath", skip_serializing_if = "Option::is_none")]
    pub local_abs_path: Option<String>,
}

/// User-selected performance mode for image / audio / video understanding.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaUnderstandingModes {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video: Option<String>,
}

/// Independent models for media understanding / generation (does not switch the primary chat model).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaModelOverrides {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<AgentModelRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio: Option<AgentModelRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video: Option<AgentModelRef>,
    /// Image generation tool (`image_generate`); defaults to Qwen Wan 2.7 or Doubao Seedream.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "imageGeneration")]
    pub image_generation: Option<AgentModelRef>,
    /// Video generation tool (`video_generate`); defaults to Qwen Wan 2.7 or Doubao Seedance 1.5.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "videoGeneration")]
    pub video_generation: Option<AgentModelRef>,
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
    /// Per-conversation workspace root for coder/file tools (session UI only).
    #[serde(default, rename = "workspaceRoot", skip_serializing_if = "String::is_empty")]
    pub workspace_root: String,
    /// True when the user explicitly picked `workspace_root` in the composer (not auto sandbox).
    #[serde(default, rename = "workspaceUserSet", skip_serializing_if = "is_false_bool")]
    pub workspace_user_set: bool,
    /// User cleared workspace in composer; do not inherit another session's directory.
    #[serde(
        default,
        rename = "workspaceInheritDisabled",
        skip_serializing_if = "is_false_bool"
    )]
    pub workspace_inherit_disabled: bool,
    /// Per-conversation lead worker when `agent_mode` is `single`.
    #[serde(
        default = "default_lead_agent_id",
        rename = "leadAgentId",
        skip_serializing_if = "is_default_session_lead_agent"
    )]
    pub lead_agent_id: String,
    /// Per-conversation orchestration mode (`single` or `supervisor`).
    #[serde(
        default = "default_agent_mode",
        rename = "agentMode",
        skip_serializing_if = "is_default_session_agent_mode"
    )]
    pub agent_mode: String,
}

fn is_default_session_lead_agent(id: &str) -> bool {
    id.trim().is_empty() || id.trim() == default_lead_agent_id()
}

fn is_default_session_agent_mode(mode: &str) -> bool {
    mode.trim().is_empty() || mode.trim() == default_agent_mode()
}

fn is_false_bool(v: &bool) -> bool {
    !*v
}

/// Conversation shell fields for P1 meta-only persistence (no messages).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationMeta {
    pub id: String,
    pub title: String,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
    #[serde(rename = "updatedAt")]
    pub updated_at: i64,
    #[serde(default, rename = "skillIds")]
    pub skill_ids: Vec<String>,
    #[serde(default, rename = "toolRoundsUsed")]
    pub tool_rounds_used: u32,
    #[serde(default, rename = "toolRoundsUsedSupervisor")]
    pub tool_rounds_used_supervisor: u32,
    #[serde(
        default,
        rename = "computerMonitorId",
        skip_serializing_if = "Option::is_none"
    )]
    pub computer_monitor_id: Option<String>,
    #[serde(default, rename = "workspaceRoot", skip_serializing_if = "String::is_empty")]
    pub workspace_root: String,
    #[serde(default, rename = "workspaceUserSet", skip_serializing_if = "is_false_bool")]
    pub workspace_user_set: bool,
    #[serde(
        default,
        rename = "workspaceInheritDisabled",
        skip_serializing_if = "is_false_bool"
    )]
    pub workspace_inherit_disabled: bool,
    #[serde(
        default = "default_lead_agent_id",
        rename = "leadAgentId",
        skip_serializing_if = "is_default_session_lead_agent"
    )]
    pub lead_agent_id: String,
    #[serde(
        default = "default_agent_mode",
        rename = "agentMode",
        skip_serializing_if = "is_default_session_agent_mode"
    )]
    pub agent_mode: String,
}

impl From<&Conversation> for ConversationMeta {
    fn from(c: &Conversation) -> Self {
        Self {
            id: c.id.clone(),
            title: c.title.clone(),
            created_at: c.created_at,
            updated_at: c.updated_at,
            skill_ids: c.skill_ids.clone(),
            tool_rounds_used: c.tool_rounds_used,
            tool_rounds_used_supervisor: c.tool_rounds_used_supervisor,
            computer_monitor_id: c.computer_monitor_id.clone(),
            workspace_root: c.workspace_root.clone(),
            workspace_user_set: c.workspace_user_set,
            workspace_inherit_disabled: c.workspace_inherit_disabled,
            lead_agent_id: c.lead_agent_id.clone(),
            agent_mode: c.agent_mode.clone(),
        }
    }
}

/// Usable desktop rectangle excluding OS chrome (macOS Dock / menu bar, Windows taskbar).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct MonitorWorkArea {
    pub left: i32,
    pub top: i32,
    pub width: i32,
    pub height: i32,
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
    /// When set, window placement should use this instead of full `left`/`top`/`width`/`height`.
    #[serde(default, rename = "workArea", skip_serializing_if = "Option::is_none")]
    pub work_area: Option<MonitorWorkArea>,
}

/// Per-model overrides for runtime/API behavior. Unset fields inherit from the parent provider.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModelRuntimeOverrides {
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "reasoningInMessages")]
    pub reasoning_in_messages: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "maxTokens")]
    pub max_tokens: Option<u32>,
    /// Qwen: `enable_thinking` on the chat/completions request.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "enableThinking")]
    pub enable_thinking: Option<bool>,
    /// Qwen: `thinking_budget` when deep thinking is enabled.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "thinkingBudget")]
    pub thinking_budget: Option<u32>,
    /// DeepSeek: `reasoning_effort` — `high` or `max`.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "reasoningEffort")]
    pub reasoning_effort: Option<String>,
    /// Whether the model accepts vision / image understanding input.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "supportsVision")]
    pub supports_vision: Option<bool>,
    /// Whether the model can generate images (`image_generate`).
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "canGenerateImage")]
    pub can_generate_image: Option<bool>,
    /// Whether the model can generate videos (`video_generate`).
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "canGenerateVideo")]
    pub can_generate_video: Option<bool>,
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
    /// Default creativity when a model has no per-model `temperature`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    /// Default max output tokens when a model has no per-model `max_tokens`.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "maxTokens")]
    pub max_tokens: Option<u32>,
    /// Key = model id string (same as entries in `models`). Values override provider default.
    #[serde(default, rename = "modelConfigs")]
    pub model_configs: HashMap<String, ModelRuntimeOverrides>,
    /// Qwen: default `enable_thinking` for models without a per-model override.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "enableThinking")]
    pub enable_thinking: Option<bool>,
    /// Qwen: default `thinking_budget` when deep thinking is enabled.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "thinkingBudget")]
    pub thinking_budget: Option<u32>,
    /// DeepSeek: default `reasoning_effort` — `high` or `max`.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "reasoningEffort")]
    pub reasoning_effort: Option<String>,
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

pub const DEFAULT_MODEL_TEMPERATURE: f32 = 0.7;
pub const DEFAULT_MODEL_MAX_TOKENS: u32 = 2048;
/// Qwen `thinking_budget` when deep thinking is enabled and no explicit budget is set.
pub const DEFAULT_THINKING_BUDGET: u32 = 2048;

fn active_provider_and_model<'a>(
    settings: &'a ModelSettings,
) -> Option<(&'a ProviderConfig, &'a str)> {
    let provider = settings
        .providers
        .iter()
        .find(|p| p.id == settings.active_provider_id)
        .or_else(|| settings.providers.first())?;
    let model = settings.model.trim();
    if model.is_empty() {
        return None;
    }
    Some((provider, model))
}

/// Creativity (`temperature`) for the **active** provider + **current** `settings.model`.
pub fn effective_temperature(settings: &ModelSettings) -> f32 {
    if let Some((p, model)) = active_provider_and_model(settings) {
        if let Some(t) = p
            .model_configs
            .get(model)
            .and_then(|o| o.temperature)
        {
            return t;
        }
        if let Some(t) = p.temperature {
            return t;
        }
    }
    if settings.temperature.is_finite() && settings.temperature >= 0.0 {
        settings.temperature
    } else {
        DEFAULT_MODEL_TEMPERATURE
    }
}

/// Max output tokens for the **active** provider + **current** `settings.model`.
pub fn effective_max_tokens(settings: &ModelSettings) -> u32 {
    if let Some((p, model)) = active_provider_and_model(settings) {
        if let Some(n) = p.model_configs.get(model).and_then(|o| o.max_tokens) {
            return n.max(64);
        }
        if let Some(n) = p.max_tokens {
            return n.max(64);
        }
    }
    settings.max_tokens.max(64)
}

/// Migrate legacy global `temperature` / `max_tokens` onto each provider default.
///
/// Do **not** auto-fill `model_configs` for every model: an empty entry means「同上」(inherit
/// provider). Filling per-model entries on load made「同上」 impossible to persist.
pub fn ensure_provider_generation_defaults(settings: &mut ModelSettings) {
    let global_temp = if settings.temperature.is_finite() && settings.temperature >= 0.0 {
        settings.temperature
    } else {
        DEFAULT_MODEL_TEMPERATURE
    };
    let global_max = settings.max_tokens.max(64);
    for provider in &mut settings.providers {
        if provider.temperature.is_none() {
            provider.temperature = Some(global_temp);
        }
        if provider.max_tokens.is_none() {
            provider.max_tokens = Some(global_max);
        }
    }
}

/// Fixed provider defaults for vision (no model-name heuristics).
/// Qwen: all models support vision; DeepSeek: none do.
pub fn provider_default_supports_vision(provider_id: &str) -> Option<bool> {
    match provider_id.trim().to_ascii_lowercase().as_str() {
        "qwen" => Some(true),
        "deepseek" => Some(false),
        _ => None,
    }
}

fn infer_model_generation_capability_flags(model: &str) -> ModelRuntimeOverrides {
    let m = model.trim().to_ascii_lowercase();
    let mut over = ModelRuntimeOverrides::default();
    if m.is_empty() {
        return over;
    }
    if m.contains("image") || m.contains("seedream") || (m.contains("wan2.") && m.contains("image")) {
        over.can_generate_image = Some(true);
    }
    if m.contains("t2v")
        || m.contains("seedance")
        || m.contains("happyhorse")
        || (m.contains("wan2.") && !m.contains("image"))
    {
        over.can_generate_video = Some(true);
    }
    over
}

fn resolve_supports_vision(
    provider_id: &str,
    model_over: Option<&ModelRuntimeOverrides>,
) -> bool {
    model_over
        .and_then(|o| o.supports_vision)
        .or_else(|| provider_default_supports_vision(provider_id))
        .unwrap_or(false)
}

/// Seed provider-fixed vision defaults and generation flags on provider models when unset.
pub fn ensure_provider_model_capability_defaults(settings: &mut ModelSettings) {
    for provider in &mut settings.providers {
        let provider_id = provider.id.clone();
        let models: Vec<String> = provider.models.clone();
        for model in models {
            let inferred = infer_model_generation_capability_flags(&model);
            let entry = provider.model_configs.entry(model).or_default();
            if provider_id.eq_ignore_ascii_case("deepseek") {
                entry.supports_vision = Some(false);
            } else if provider_id.eq_ignore_ascii_case("qwen") {
                if entry.supports_vision.is_none() {
                    entry.supports_vision = Some(true);
                }
            } else if entry.supports_vision.is_none() {
                if let Some(v) = provider_default_supports_vision(&provider_id) {
                    entry.supports_vision = Some(v);
                }
            }
            if entry.can_generate_image.is_none() {
                entry.can_generate_image = inferred.can_generate_image;
            }
            if entry.can_generate_video.is_none() {
                entry.can_generate_video = inferred.can_generate_video;
            }
        }
    }
}

pub fn model_capability_flags(
    settings: &ModelSettings,
    provider_id: &str,
    model: &str,
) -> (bool, bool, bool) {
    let inferred = infer_model_generation_capability_flags(model);
    let provider = settings
        .providers
        .iter()
        .find(|p| p.id == provider_id)
        .or_else(|| settings.providers.first());
    let Some(p) = provider else {
        return (
            provider_default_supports_vision(provider_id).unwrap_or(false),
            inferred.can_generate_image.unwrap_or(false),
            inferred.can_generate_video.unwrap_or(false),
        );
    };
    let over = p.model_configs.get(model.trim());
    (
        resolve_supports_vision(&p.id, over),
        over.and_then(|o| o.can_generate_image)
            .or(inferred.can_generate_image)
            .unwrap_or(false),
        over.and_then(|o| o.can_generate_video)
            .or(inferred.can_generate_video)
            .unwrap_or(false),
    )
}

pub fn default_qwen_provider_models() -> Vec<String> {
    vec![
        "qwen3.5-plus".into(),
        "qwen3.5-27b".into(),
        "qwen3.5-flash".into(),
        "qwen3.7-max".into(),
        "qwen3.7-plus".into(),
        "qwen3.6-plus".into(),
        "qwen3.6-27b".into(),
        "qwen3.6-flash".into(),
        "wan2.7-image-pro".into(),
        "qwen-image-2.0-pro".into(),
        "happyhorse-1.0-t2v".into(),
        "happyhorse-1.0-i2v".into(),
    ]
}

pub fn default_doubao_provider_models() -> Vec<String> {
    vec![
        "doubao-seedream-5-0-lite-260128".into(),
        "doubao-seedream-4-5-251128".into(),
        "doubao-seedance-2-0-fast-260128".into(),
        "doubao-seedance-2-0-260128".into(),
    ]
}

pub fn default_media_generation_overrides() -> MediaModelOverrides {
    MediaModelOverrides {
        image_generation: Some(AgentModelRef {
            provider_id: "doubao".into(),
            model: "doubao-seedream-5-0-lite-260128".into(),
        }),
        video_generation: Some(AgentModelRef {
            provider_id: "doubao".into(),
            model: "doubao-seedance-2-0-fast-260128".into(),
        }),
        ..MediaModelOverrides::default()
    }
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

fn normalize_reasoning_effort(s: &str) -> Option<String> {
    match s.trim().to_ascii_lowercase().as_str() {
        "high" => Some("high".into()),
        "max" => Some("max".into()),
        _ => None,
    }
}

fn effective_enable_thinking(
    provider: &ProviderConfig,
    model_over: Option<&ModelRuntimeOverrides>,
) -> Option<bool> {
    model_over
        .and_then(|o| o.enable_thinking)
        .or(provider.enable_thinking)
}

fn effective_thinking_budget(
    provider: &ProviderConfig,
    model_over: Option<&ModelRuntimeOverrides>,
) -> u32 {
    model_over
        .and_then(|o| o.thinking_budget)
        .or(provider.thinking_budget)
        .filter(|&n| n > 0)
        .unwrap_or(DEFAULT_THINKING_BUDGET)
}

fn effective_reasoning_effort(
    provider: &ProviderConfig,
    model_over: Option<&ModelRuntimeOverrides>,
) -> Option<String> {
    model_over
        .and_then(|o| o.reasoning_effort.as_deref())
        .or(provider.reasoning_effort.as_deref())
        .and_then(|s| normalize_reasoning_effort(s))
}

/// Extension fields for **active** provider + **current** `settings.model` (per-model overrides win).
pub fn effective_chat_extra_body(settings: &ModelSettings) -> Option<Value> {
    let (provider, model) = active_provider_and_model(settings)?;
    let model_over = provider.model_configs.get(model);
    let mut m = Map::new();

    if provider_uses_dashscope_compatible_api(provider) {
        let enable = settings
            .round_enable_thinking
            .or_else(|| effective_enable_thinking(provider, model_over));
        if let Some(enable) = enable {
            m.insert("enable_thinking".into(), Value::Bool(enable));
            if enable {
                let budget = settings
                    .round_thinking_budget
                    .unwrap_or_else(|| effective_thinking_budget(provider, model_over));
                m.insert("thinking_budget".into(), Value::Number(budget.into()));
            }
        }
    }

    if provider_uses_deepseek_api(provider) {
        if let Some(effort) = effective_reasoning_effort(provider, model_over) {
            m.insert("reasoning_effort".into(), Value::String(effort));
        }
    }

    if m.is_empty() {
        None
    } else {
        Some(Value::Object(m))
    }
}

/// Absorb legacy `extraBody` JSON and `thinkingEnabled` / `thinkingBudget` into structured fields.
pub fn absorb_legacy_extension_config(
    enable_thinking: &mut Option<bool>,
    thinking_budget: &mut Option<u32>,
    reasoning_effort: &mut Option<String>,
    legacy_enable: Option<bool>,
    legacy_budget: Option<u32>,
    extra_body: Option<Value>,
) {
    if enable_thinking.is_none() {
        if let Some(b) = legacy_enable {
            *enable_thinking = Some(b);
        }
    }
    if thinking_budget.is_none() {
        if let Some(n) = legacy_budget.filter(|&n| n > 0) {
            *thinking_budget = Some(n);
        }
    }
    let Some(Value::Object(o)) = extra_body else {
        return;
    };
    if enable_thinking.is_none() {
        if let Some(b) = o.get("enable_thinking").and_then(|v| v.as_bool()) {
            *enable_thinking = Some(b);
        }
    }
    if thinking_budget.is_none() {
        if let Some(n) = o.get("thinking_budget").and_then(|v| v.as_u64()) {
            *thinking_budget = Some(n as u32);
        }
    }
    if reasoning_effort.is_none() {
        if let Some(s) = o
            .get("reasoning_effort")
            .and_then(|v| v.as_str())
            .and_then(|s| normalize_reasoning_effort(s))
        {
            *reasoning_effort = Some(s);
        }
    }
}

/// DashScope / DeepSeek：扩展参数写在请求体根级，不用 `extra_body` 包裹。
pub fn chat_request_flattens_extra_body(settings: &ModelSettings) -> bool {
    let Some((provider, _)) = active_provider_and_model(settings) else {
        return false;
    };
    provider_uses_dashscope_compatible_api(provider) || provider_uses_deepseek_api(provider)
}

pub fn provider_uses_deepseek_api(provider: &ProviderConfig) -> bool {
    if provider.id.eq_ignore_ascii_case("deepseek") {
        return true;
    }
    provider
        .base_url
        .to_ascii_lowercase()
        .contains("api.deepseek.com")
}

/// When [`chat_request_flattens_extra_body`], lift `extra_body` object keys to the request root.
pub fn flatten_chat_extra_body_on_wire(mut body: Value, settings: &ModelSettings) -> Value {
    if !chat_request_flattens_extra_body(settings) {
        return body;
    }
    let Value::Object(ref mut map) = body else {
        return body;
    };
    let Some(extra) = map.remove("extra_body") else {
        return body;
    };
    let Value::Object(extra_map) = extra else {
        return body;
    };
    for (k, v) in extra_map {
        map.entry(k).or_insert(v);
    }
    body
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
    /// When agentMode is single, which worker id leads (kebab-case). Empty = computer agent.
    #[serde(default, rename = "leadAgentId")]
    pub lead_agent_id: String,
    /// When true, summarize older turns via a separate model call when estimated context exceeds budget.
    #[serde(default = "default_context_compression_enabled", rename = "contextCompressionEnabled")]
    pub context_compression_enabled: bool,
    /// Estimated token budget for included messages; compression runs when heuristic exceeds this.
    #[serde(
        default = "default_context_budget_tokens",
        rename = "contextBudgetTokens",
        alias = "contextBudgetChars"
    )]
    pub context_budget_tokens: u32,
    /// Keep this many most recent user messages (and everything after the cutoff) verbatim.
    #[serde(default = "default_context_keep_recent_user_turns", rename = "contextKeepRecentUserTurns")]
    pub context_keep_recent_user_turns: u32,
    /// Max tokens for the one-off summarization chat completion.
    #[serde(default = "default_context_summary_max_tokens", rename = "contextSummaryMaxTokens")]
    pub context_summary_max_tokens: u32,
    /// Max tool-call rounds per assistant turn. Default 100.
    #[serde(default = "default_max_tool_rounds", rename = "maxToolRounds")]
    pub max_tool_rounds: u32,
    /// Max tool-call rounds **inside** each `run_sub_agent` run (separate from the lead conversation pool).
    #[serde(default = "default_max_tool_rounds", rename = "maxSubAgentToolRounds")]
    pub max_sub_agent_tool_rounds: u32,
    /// When true, chat UI shows the assistant “原始输出” inspector (code icon); includes wire text and API reasoning for debug, not inline in the bubble.
    #[serde(default = "default_raw_content_view_enabled", rename = "rawContentViewEnabled")]
    pub raw_content_view_enabled: bool,
    /// When true, each LLM round writes request `messages` + params under app data `logs/llm_prompts/`.
    #[serde(default = "default_debug_dump_llm_prompts", rename = "debugDumpLlmPrompts")]
    pub debug_dump_llm_prompts: bool,
    /// When true, settings UI exposes debug sections (independent of raw wire / prompt dump toggles).
    #[serde(default = "default_debug_menus_enabled", rename = "debugMenusEnabled")]
    pub debug_menus_enabled: bool,
    /// Debug UI switch: show child task boards under parent board panel.
    #[serde(
        default = "default_task_board_show_child_boards",
        rename = "taskBoardShowChildBoards"
    )]
    pub task_board_show_child_boards: bool,
    /// Migration flag: append task board runtime markdown as the last user message each round.
    #[serde(default = "default_user_dynamic_inject_enabled", rename = "userDynamicInjectEnabled")]
    pub user_dynamic_inject_enabled: bool,
    /// Per-agent default LLM: worker id or `"supervisor"` → explicit provider + model.
    #[serde(default, rename = "agentDefaultModels", deserialize_with = "deserialize_agent_default_models", serialize_with = "serialize_agent_default_models")]
    pub agent_default_models: HashMap<String, AgentModelRef>,
    /// When true for a worker id, successful `task_board` updates hard-trim older history (no LLM).
    #[serde(default, rename = "agentTaskBoardHistoryTrim")]
    pub agent_task_board_history_trim: HashMap<String, bool>,
    /// When true, computer agent uses Bézier / jitter mouse paths by default (`human_like` preset).
    #[serde(default = "default_computer_human_like", rename = "computerHumanLike")]
    pub computer_human_like: bool,
    /// Starting vision tier for new computer conversations (`primary` | `intermediate` | `advanced`).
    #[serde(default = "default_computer_initial_tier", rename = "computerInitialTier")]
    pub computer_initial_tier: String,
    /// When true, Computer Use assistant messages show the annotated screenshot preview action.
    #[serde(default = "default_computer_annotated_screen_view_enabled", rename = "computerAnnotatedScreenViewEnabled")]
    pub computer_annotated_screen_view_enabled: bool,
    /// DaTi CAPTCHA API endpoint.
    #[serde(default = "default_dati_api_url", rename = "datiApiUrl")]
    pub dati_api_url: String,
    /// DaTi CAPTCHA API authcode.
    #[serde(default = "default_dati_authcode", rename = "datiAuthcode")]
    pub dati_authcode: String,
    /// DaTi CAPTCHA question type number.
    #[serde(default = "default_dati_typeno", rename = "datiTypeno")]
    pub dati_typeno: String,
    /// DaTi CAPTCHA developer author.
    #[serde(default = "default_dati_author", rename = "datiAuthor")]
    pub dati_author: String,
    /// Pixel adjustment applied to the final point of slider CAPTCHA drags.
    #[serde(default = "default_captcha_slider_offset_px", rename = "captchaSliderOffsetPx")]
    pub captcha_slider_offset_px: i32,
    /// When true, Composer shows the monitor picker for the computer agent.
    #[serde(default = "default_computer_show_monitor_picker", rename = "computerShowMonitorPicker")]
    pub computer_show_monitor_picker: bool,
    #[serde(default = "default_memory_enabled", rename = "memoryEnabled")]
    pub memory_enabled: bool,
    #[serde(default = "default_user_profile_enabled", rename = "userProfileEnabled")]
    pub user_profile_enabled: bool,
    #[serde(default = "default_memory_char_limit", rename = "memoryCharLimit")]
    pub memory_char_limit: u32,
    #[serde(default = "default_user_char_limit", rename = "userCharLimit")]
    pub user_char_limit: u32,
    #[serde(default = "default_memory_nudge_interval", rename = "memoryNudgeInterval")]
    pub memory_nudge_interval: u32,
    #[serde(default = "default_background_review_enabled", rename = "backgroundReviewEnabled")]
    pub background_review_enabled: bool,
    #[serde(default = "default_skill_creation_nudge_interval", rename = "skillCreationNudgeInterval")]
    pub skill_creation_nudge_interval: u32,
    #[serde(default = "default_curator_enabled", rename = "curatorEnabled")]
    pub curator_enabled: bool,
    #[serde(default = "default_curator_idle_hours", rename = "curatorIdleHours")]
    pub curator_idle_hours: u32,
    #[serde(default = "default_curator_interval_days", rename = "curatorIntervalDays")]
    pub curator_interval_days: u32,
    /// UI theme: `light`, `dark`, or `system`.
    #[serde(default = "default_theme", rename = "theme")]
    pub theme: String,
    /// Per-agent UI overrides keyed by agent id.
    #[serde(default, rename = "agentUiOverrides")]
    pub agent_ui_overrides: HashMap<String, crate::agents::AgentUiConfig>,
    /// Model id for DashScope web search tool calls (defaults to `qwen3-max` when empty).
    #[serde(default = "default_web_search_model_setting", rename = "webSearchModel")]
    pub web_search_model: String,
    /// Independent models for attachment understanding (image/audio/video).
    #[serde(default, rename = "mediaModelOverrides")]
    pub media_model_overrides: MediaModelOverrides,
    /// User-selected performance mode per agent (`general`, `coder`, …).
    #[serde(default, rename = "agentPerformanceModes")]
    pub agent_performance_modes: HashMap<String, String>,
    /// User-selected performance mode for media understanding kinds.
    #[serde(default, rename = "mediaUnderstandingModes")]
    pub media_understanding_modes: MediaUnderstandingModes,
    /// Debug: agent id → performance mode → LLM profile.
    #[serde(default, rename = "agentModeLlm")]
    pub agent_mode_llm: HashMap<String, HashMap<String, ComputerTierLlmConfig>>,
    /// Debug: media kind → performance mode → LLM profile.
    #[serde(default, rename = "mediaModeLlm")]
    pub media_mode_llm: HashMap<String, HashMap<String, ComputerTierLlmConfig>>,
    /// Per-request override (e.g. computer tier); not persisted.
    #[serde(skip)]
    pub round_enable_thinking: Option<bool>,
    #[serde(skip)]
    pub round_thinking_budget: Option<u32>,
}

macro_rules! build_cfg_str {
    ($name:literal, $default:expr) => {{
        option_env!(concat!("POINTER_BUILD_", $name))
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| $default.to_string())
    }};
}

macro_rules! build_cfg_bool {
    ($name:literal, $default:expr) => {{
        match option_env!(concat!("POINTER_BUILD_", $name))
            .map(str::trim)
            .map(|v| v.to_ascii_lowercase())
            .as_deref()
        {
            Some("1" | "true" | "yes" | "on") => true,
            Some("0" | "false" | "no" | "off") => false,
            _ => $default,
        }
    }};
}

macro_rules! build_cfg_u32 {
    ($name:literal, $default:expr) => {{
        option_env!(concat!("POINTER_BUILD_", $name))
            .map(str::trim)
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or($default)
    }};
}

macro_rules! build_cfg_i32 {
    ($name:literal, $default:expr) => {{
        option_env!(concat!("POINTER_BUILD_", $name))
            .map(str::trim)
            .and_then(|v| v.parse::<i32>().ok())
            .unwrap_or($default)
    }};
}

macro_rules! build_cfg_f32 {
    ($name:literal, $default:expr) => {{
        option_env!(concat!("POINTER_BUILD_", $name))
            .map(str::trim)
            .and_then(|v| v.parse::<f32>().ok())
            .unwrap_or($default)
    }};
}

fn default_theme() -> String {
    build_cfg_str!("THEME", "system")
}

fn default_active_provider_id() -> String {
    build_cfg_str!("ACTIVE_PROVIDER_ID", "qwen")
}

fn default_model_name() -> String {
    build_cfg_str!("MODEL", "qwen3.5-plus")
}

fn default_model_temperature() -> f32 {
    build_cfg_f32!("TEMPERATURE", 0.7)
}

fn default_model_max_tokens() -> u32 {
    build_cfg_u32!("MAX_TOKENS", 2048)
}

fn default_workspace_root() -> String {
    build_cfg_str!("WORKSPACE_ROOT", "")
}

fn default_lead_agent_id() -> String {
    build_cfg_str!("LEAD_AGENT_ID", "general")
}

fn default_computer_initial_tier() -> String {
    build_cfg_str!("COMPUTER_INITIAL_TIER", "intermediate")
}

fn default_computer_human_like() -> bool {
    true
}

pub fn ensure_agent_model_refs_have_provider(settings: &mut ModelSettings) {
    let ap = settings.active_provider_id.clone();
    for v in settings.agent_default_models.values_mut() {
        v.ensure_provider_or(&ap);
    }
}

fn default_tool_approval_mode() -> String {
    build_cfg_str!("TOOL_APPROVAL_MODE", "auto")
}

fn default_agent_mode() -> String {
    build_cfg_str!("AGENT_MODE", "single")
}

fn default_context_compression_enabled() -> bool {
    build_cfg_bool!("CONTEXT_COMPRESSION_ENABLED", true)
}

fn default_context_budget_tokens() -> u32 {
    build_cfg_u32!("CONTEXT_BUDGET_TOKENS", 120_000)
}

fn default_context_keep_recent_user_turns() -> u32 {
    build_cfg_u32!("CONTEXT_KEEP_RECENT_USER_TURNS", 6)
}

fn default_context_summary_max_tokens() -> u32 {
    build_cfg_u32!("CONTEXT_SUMMARY_MAX_TOKENS", 2048)
}

fn default_max_tool_rounds() -> u32 {
    build_cfg_u32!("MAX_TOOL_ROUNDS", 100)
}

fn default_raw_content_view_enabled() -> bool {
    build_cfg_bool!("RAW_CONTENT_VIEW_ENABLED", false)
}

fn default_debug_dump_llm_prompts() -> bool {
    build_cfg_bool!("DEBUG_DUMP_LLM_PROMPTS", false)
}

fn default_debug_menus_enabled() -> bool {
    build_cfg_bool!("DEBUG_MENUS_ENABLED", false)
}

fn default_task_board_show_child_boards() -> bool {
    false
}

fn default_user_dynamic_inject_enabled() -> bool {
    build_cfg_bool!("USER_DYNAMIC_INJECT_ENABLED", true)
}

fn default_computer_annotated_screen_view_enabled() -> bool {
    build_cfg_bool!("COMPUTER_ANNOTATED_SCREEN_VIEW_ENABLED", false)
}

fn default_computer_show_monitor_picker() -> bool {
    build_cfg_bool!("COMPUTER_SHOW_MONITOR_PICKER", true)
}

fn default_dati_api_url() -> String {
    build_cfg_str!("DATI_API_URL", "")
}

fn default_dati_authcode() -> String {
    build_cfg_str!("DATI_AUTHCODE", "")
}

fn default_dati_typeno() -> String {
    build_cfg_str!("DATI_TYPENO", "")
}

fn default_dati_author() -> String {
    build_cfg_str!("DATI_AUTHOR", "")
}

fn default_captcha_slider_offset_px() -> i32 {
    build_cfg_i32!("CAPTCHA_SLIDER_OFFSET_PX", 0)
}

fn default_web_search_model_setting() -> String {
    build_cfg_str!("WEB_SEARCH_MODEL", "")
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
                    models: default_qwen_provider_models(),
                    reasoning_in_messages: None,
                    temperature: None,
                    max_tokens: None,
                    model_configs: HashMap::new(),
                    enable_thinking: None,
                    thinking_budget: None,
                    reasoning_effort: None,
                },
                ProviderConfig {
                    id: "deepseek".into(),
                    name: "深度求索".into(),
                    base_url: "https://api.deepseek.com/v1".into(),
                    api_key: String::new(),
                    models: vec!["deepseek-v4-flash".into(), "deepseek-v4-pro".into()],
                    reasoning_in_messages: Some(true),
                    temperature: None,
                    max_tokens: None,
                    model_configs: HashMap::new(),
                    enable_thinking: None,
                    thinking_budget: None,
                    reasoning_effort: None,
                },
                ProviderConfig {
                    id: "doubao".into(),
                    name: "豆包".into(),
                    base_url: "https://ark.cn-beijing.volces.com/api/v3".into(),
                    api_key: String::new(),
                    models: default_doubao_provider_models(),
                    reasoning_in_messages: None,
                    temperature: None,
                    max_tokens: None,
                    model_configs: HashMap::new(),
                    enable_thinking: None,
                    thinking_budget: None,
                    reasoning_effort: None,
                },
            ],
            active_provider_id: default_active_provider_id(),
            model: default_model_name(),
            api_key: String::new(),
            temperature: default_model_temperature(),
            max_tokens: default_model_max_tokens(),
            has_key: false,
            tool_approval_mode: default_tool_approval_mode(),
            agent_mode: default_agent_mode(),
            workspace_root: default_workspace_root(),
            lead_agent_id: default_lead_agent_id(),
            context_compression_enabled: default_context_compression_enabled(),
            context_budget_tokens: default_context_budget_tokens(),
            context_keep_recent_user_turns: default_context_keep_recent_user_turns(),
            context_summary_max_tokens: default_context_summary_max_tokens(),
            max_tool_rounds: default_max_tool_rounds(),
            max_sub_agent_tool_rounds: default_max_tool_rounds(),
            raw_content_view_enabled: default_raw_content_view_enabled(),
            debug_dump_llm_prompts: default_debug_dump_llm_prompts(),
            debug_menus_enabled: default_debug_menus_enabled(),
            task_board_show_child_boards: default_task_board_show_child_boards(),
            user_dynamic_inject_enabled: default_user_dynamic_inject_enabled(),
            agent_default_models: HashMap::new(),
            agent_task_board_history_trim: HashMap::new(),
            computer_human_like: true,
            computer_initial_tier: default_computer_initial_tier(),
            computer_annotated_screen_view_enabled: default_computer_annotated_screen_view_enabled(),
            dati_api_url: default_dati_api_url(),
            dati_authcode: default_dati_authcode(),
            dati_typeno: default_dati_typeno(),
            dati_author: default_dati_author(),
            captcha_slider_offset_px: default_captcha_slider_offset_px(),
            computer_show_monitor_picker: default_computer_show_monitor_picker(),
            memory_enabled: default_memory_enabled(),
            user_profile_enabled: default_user_profile_enabled(),
            memory_char_limit: default_memory_char_limit(),
            user_char_limit: default_user_char_limit(),
            memory_nudge_interval: default_memory_nudge_interval(),
            background_review_enabled: default_background_review_enabled(),
            skill_creation_nudge_interval: default_skill_creation_nudge_interval(),
            curator_enabled: default_curator_enabled(),
            curator_idle_hours: default_curator_idle_hours(),
            curator_interval_days: default_curator_interval_days(),
            theme: default_theme(),
            agent_ui_overrides: HashMap::new(),
            web_search_model: default_web_search_model_setting(),
            media_model_overrides: default_media_generation_overrides(),
            agent_performance_modes: HashMap::new(),
            media_understanding_modes: MediaUnderstandingModes::default(),
            agent_mode_llm: default_agent_mode_llm(),
            media_mode_llm: default_media_mode_llm(),
            round_enable_thinking: None,
            round_thinking_budget: None,
        }
    }
}

pub const DEFAULT_WEB_SEARCH_MODEL: &str = "qwen3-max-2026-01-23";

/// Effective model id for DashScope `web_search` tool (`Generation` API + `enable_search`).
///
/// Independent from per-agent chat defaults (e.g. `research` may orchestrate on `qwen3.6-plus`
/// while search calls use `qwen3-max`). Env `POINTER_WEB_SEARCH_MODEL` and `webSearchModel`
/// override the default.
pub fn effective_web_search_model(settings: &ModelSettings, _agent_id: Option<&str>) -> String {
    if let Ok(m) = std::env::var("POINTER_WEB_SEARCH_MODEL") {
        let m = m.trim();
        if !m.is_empty() {
            return m.to_string();
        }
    }
    let configured = settings.web_search_model.trim();
    if !configured.is_empty() {
        return configured.to_string();
    }
    DEFAULT_WEB_SEARCH_MODEL.to_string()
}

/// First Qwen provider, or any provider whose base URL is DashScope compatible.
pub fn find_dashscope_provider(settings: &ModelSettings) -> Option<&ProviderConfig> {
    settings
        .providers
        .iter()
        .find(|p| p.id.eq_ignore_ascii_case("qwen"))
        .or_else(|| {
            settings
                .providers
                .iter()
                .find(|p| provider_uses_dashscope_compatible_api(p))
        })
}

// ── User / platform config split ─────────────────────────────────────────────

fn default_enabled_skill_ids() -> Vec<String> {
    crate::skills::DEFAULT_ENABLED_SKILL_IDS
        .iter()
        .map(|s| (*s).to_string())
        .collect()
}

fn default_memory_enabled() -> bool {
    true
}

fn default_user_profile_enabled() -> bool {
    true
}

fn default_memory_char_limit() -> u32 {
    crate::memory::DEFAULT_MEMORY_CHAR_LIMIT as u32
}

fn default_user_char_limit() -> u32 {
    crate::memory::DEFAULT_USER_CHAR_LIMIT as u32
}

fn default_memory_nudge_interval() -> u32 {
    10
}

fn default_background_review_enabled() -> bool {
    true
}

fn default_skill_creation_nudge_interval() -> u32 {
    10
}

fn default_curator_enabled() -> bool {
    true
}

fn default_curator_idle_hours() -> u32 {
    2
}

fn default_curator_interval_days() -> u32 {
    7
}

fn default_computer_auto_compact() -> bool {
    true
}

/// Persisted user preferences (theme, optional UI cache).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserSettings {
    #[serde(default = "default_theme", rename = "theme")]
    pub theme: String,
    #[serde(default, rename = "userNickname")]
    pub user_nickname: Option<String>,
    /// Globally enabled skill ids (UI + runtime when lead agent is `general`).
    #[serde(default = "default_enabled_skill_ids", rename = "enabledSkillIds")]
    pub enabled_skill_ids: Vec<String>,
    #[serde(default = "default_memory_enabled", rename = "memoryEnabled")]
    pub memory_enabled: bool,
    #[serde(default = "default_user_profile_enabled", rename = "userProfileEnabled")]
    pub user_profile_enabled: bool,
    #[serde(default = "default_memory_char_limit", rename = "memoryCharLimit")]
    pub memory_char_limit: u32,
    #[serde(default = "default_user_char_limit", rename = "userCharLimit")]
    pub user_char_limit: u32,
    #[serde(default = "default_memory_nudge_interval", rename = "memoryNudgeInterval")]
    pub memory_nudge_interval: u32,
    #[serde(default = "default_background_review_enabled", rename = "backgroundReviewEnabled")]
    pub background_review_enabled: bool,
    #[serde(default = "default_skill_creation_nudge_interval", rename = "skillCreationNudgeInterval")]
    pub skill_creation_nudge_interval: u32,
    #[serde(default = "default_curator_enabled", rename = "curatorEnabled")]
    pub curator_enabled: bool,
    #[serde(default = "default_curator_idle_hours", rename = "curatorIdleHours")]
    pub curator_idle_hours: u32,
    #[serde(default = "default_curator_interval_days", rename = "curatorIntervalDays")]
    pub curator_interval_days: u32,
    #[serde(default = "default_computer_auto_compact", rename = "computerAutoCompact")]
    pub computer_auto_compact: bool,
}

impl Default for UserSettings {
    fn default() -> Self {
        Self {
            theme: default_theme(),
            user_nickname: None,
            enabled_skill_ids: default_enabled_skill_ids(),
            memory_enabled: default_memory_enabled(),
            user_profile_enabled: default_user_profile_enabled(),
            memory_char_limit: default_memory_char_limit(),
            user_char_limit: default_user_char_limit(),
            memory_nudge_interval: default_memory_nudge_interval(),
            background_review_enabled: default_background_review_enabled(),
            skill_creation_nudge_interval: default_skill_creation_nudge_interval(),
            curator_enabled: default_curator_enabled(),
            curator_idle_hours: default_curator_idle_hours(),
            curator_interval_days: default_curator_interval_days(),
            computer_auto_compact: default_computer_auto_compact(),
        }
    }
}

/// Per-tier LLM overrides for Computer Use Agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputerTierLlmConfig {
    #[serde(rename = "providerId")]
    pub provider_id: String,
    pub model: String,
    #[serde(default, rename = "enableThinking")]
    pub enable_thinking: bool,
    #[serde(default, rename = "thinkingBudget")]
    pub thinking_budget: Option<u32>,
}

/// In-memory platform configuration (not persisted across restarts).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformSettings {
    pub providers: Vec<ProviderConfig>,
    #[serde(rename = "activeProviderId")]
    pub active_provider_id: String,
    pub model: String,
    pub temperature: f32,
    #[serde(rename = "maxTokens")]
    pub max_tokens: u32,
    #[serde(default = "default_tool_approval_mode", rename = "toolApprovalMode")]
    pub tool_approval_mode: String,
    #[serde(default = "default_agent_mode", rename = "agentMode")]
    pub agent_mode: String,
    #[serde(default, rename = "workspaceRoot")]
    pub workspace_root: String,
    #[serde(default, rename = "leadAgentId")]
    pub lead_agent_id: String,
    #[serde(default = "platform_default_context_compression_enabled", rename = "contextCompressionEnabled")]
    pub context_compression_enabled: bool,
    #[serde(
        default = "platform_default_context_budget_tokens",
        rename = "contextBudgetTokens",
        alias = "contextBudgetChars"
    )]
    pub context_budget_tokens: u32,
    #[serde(default = "platform_default_context_keep_recent_user_turns", rename = "contextKeepRecentUserTurns")]
    pub context_keep_recent_user_turns: u32,
    #[serde(default = "platform_default_context_summary_max_tokens", rename = "contextSummaryMaxTokens")]
    pub context_summary_max_tokens: u32,
    #[serde(default = "platform_default_max_tool_rounds", rename = "maxToolRounds")]
    pub max_tool_rounds: u32,
    #[serde(default = "platform_default_max_tool_rounds", rename = "maxSubAgentToolRounds")]
    pub max_sub_agent_tool_rounds: u32,
    #[serde(default = "platform_default_raw_content_view_enabled", rename = "rawContentViewEnabled")]
    pub raw_content_view_enabled: bool,
    #[serde(default = "default_debug_dump_llm_prompts", rename = "debugDumpLlmPrompts")]
    pub debug_dump_llm_prompts: bool,
    #[serde(default = "default_debug_menus_enabled", rename = "debugMenusEnabled")]
    pub debug_menus_enabled: bool,
    #[serde(
        default = "default_task_board_show_child_boards",
        rename = "taskBoardShowChildBoards"
    )]
    pub task_board_show_child_boards: bool,
    #[serde(default = "default_user_dynamic_inject_enabled", rename = "userDynamicInjectEnabled")]
    pub user_dynamic_inject_enabled: bool,
    #[serde(default, rename = "agentDefaultModels", deserialize_with = "deserialize_agent_default_models", serialize_with = "serialize_agent_default_models")]
    pub agent_default_models: HashMap<String, AgentModelRef>,
    #[serde(default, rename = "agentTaskBoardHistoryTrim")]
    pub agent_task_board_history_trim: HashMap<String, bool>,
    #[serde(default = "default_computer_human_like", rename = "computerHumanLike")]
    pub computer_human_like: bool,
    #[serde(default = "default_computer_initial_tier", rename = "computerInitialTier")]
    pub computer_initial_tier: String,
    #[serde(default = "default_computer_annotated_screen_view_enabled", rename = "computerAnnotatedScreenViewEnabled")]
    pub computer_annotated_screen_view_enabled: bool,
    #[serde(default = "default_dati_api_url", rename = "datiApiUrl")]
    pub dati_api_url: String,
    #[serde(default = "default_dati_authcode", rename = "datiAuthcode")]
    pub dati_authcode: String,
    #[serde(default = "default_dati_typeno", rename = "datiTypeno")]
    pub dati_typeno: String,
    #[serde(default = "default_dati_author", rename = "datiAuthor")]
    pub dati_author: String,
    #[serde(default = "default_captcha_slider_offset_px", rename = "captchaSliderOffsetPx")]
    pub captcha_slider_offset_px: i32,
    #[serde(default = "default_computer_show_monitor_picker", rename = "computerShowMonitorPicker")]
    pub computer_show_monitor_picker: bool,
    #[serde(default, rename = "agentUiOverrides")]
    pub agent_ui_overrides: HashMap<String, crate::agents::AgentUiConfig>,
    /// Model id for DashScope web search tool calls (empty = default `qwen3-max`).
    #[serde(default = "default_web_search_model_setting", rename = "webSearchModel")]
    pub web_search_model: String,
    #[serde(default, rename = "mediaModelOverrides")]
    pub media_model_overrides: MediaModelOverrides,
    #[serde(default, rename = "agentPerformanceModes")]
    pub agent_performance_modes: HashMap<String, String>,
    #[serde(default, rename = "mediaUnderstandingModes")]
    pub media_understanding_modes: MediaUnderstandingModes,
    #[serde(default = "default_computer_tier_llm", rename = "computerTierLlm")]
    pub computer_tier_llm: HashMap<String, ComputerTierLlmConfig>,
    #[serde(default = "default_agent_mode_llm", rename = "agentModeLlm")]
    pub agent_mode_llm: HashMap<String, HashMap<String, ComputerTierLlmConfig>>,
    #[serde(default = "default_media_mode_llm", rename = "mediaModeLlm")]
    pub media_mode_llm: HashMap<String, HashMap<String, ComputerTierLlmConfig>>,
}

/// Provider entries we do not ship or persist (legacy / third-party).
pub fn is_openrouter_provider(p: &ProviderConfig) -> bool {
    if p.id.eq_ignore_ascii_case("openrouter") {
        return true;
    }
    let name = p.name.to_ascii_lowercase();
    if name.contains("openrouter") || name.contains("open router") {
        return true;
    }
    let url = p.base_url.to_ascii_lowercase();
    url.contains("openrouter.ai")
}

pub fn filter_openrouter_providers(providers: Vec<ProviderConfig>) -> Vec<ProviderConfig> {
    providers
        .into_iter()
        .filter(|p| !is_openrouter_provider(p))
        .collect()
}

/// Disk-safe desktop agent preferences (智能体 section). Excludes model-service and session-only fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedLocalPlatformSettings {
    #[serde(default = "default_tool_approval_mode", rename = "toolApprovalMode")]
    pub tool_approval_mode: String,
    #[serde(
        default = "default_user_dynamic_inject_enabled",
        rename = "userDynamicInjectEnabled"
    )]
    pub user_dynamic_inject_enabled: bool,
    #[serde(default = "default_computer_human_like", rename = "computerHumanLike")]
    pub computer_human_like: bool,
    #[serde(default = "default_computer_initial_tier", rename = "computerInitialTier")]
    pub computer_initial_tier: String,
    #[serde(
        default = "platform_default_context_compression_enabled",
        rename = "contextCompressionEnabled"
    )]
    pub context_compression_enabled: bool,
    #[serde(
        default = "platform_default_context_budget_tokens",
        rename = "contextBudgetTokens",
        alias = "contextBudgetChars"
    )]
    pub context_budget_tokens: u32,
    #[serde(
        default = "platform_default_context_keep_recent_user_turns",
        rename = "contextKeepRecentUserTurns"
    )]
    pub context_keep_recent_user_turns: u32,
    #[serde(
        default = "platform_default_context_summary_max_tokens",
        rename = "contextSummaryMaxTokens"
    )]
    pub context_summary_max_tokens: u32,
    #[serde(default = "platform_default_max_tool_rounds", rename = "maxToolRounds")]
    pub max_tool_rounds: u32,
    #[serde(default = "default_agent_mode", rename = "agentMode")]
    pub agent_mode: String,
    #[serde(default, rename = "leadAgentId")]
    pub lead_agent_id: String,
    #[serde(default, rename = "workspaceRoot")]
    pub workspace_root: String,
    #[serde(default = "default_captcha_slider_offset_px", rename = "captchaSliderOffsetPx")]
    pub captcha_slider_offset_px: i32,
    #[serde(default, rename = "mediaModelOverrides")]
    pub media_model_overrides: MediaModelOverrides,
    #[serde(default, rename = "agentPerformanceModes")]
    pub agent_performance_modes: HashMap<String, String>,
    #[serde(default, rename = "mediaUnderstandingModes")]
    pub media_understanding_modes: MediaUnderstandingModes,
}

impl PersistedLocalPlatformSettings {
    pub fn from_platform(platform: &PlatformSettings) -> Self {
        Self {
            tool_approval_mode: platform.tool_approval_mode.clone(),
            user_dynamic_inject_enabled: platform.user_dynamic_inject_enabled,
            computer_human_like: platform.computer_human_like,
            computer_initial_tier: platform.computer_initial_tier.clone(),
            context_compression_enabled: platform.context_compression_enabled,
            context_budget_tokens: platform.context_budget_tokens,
            context_keep_recent_user_turns: platform.context_keep_recent_user_turns,
            context_summary_max_tokens: platform.context_summary_max_tokens,
            max_tool_rounds: platform.max_tool_rounds,
            agent_mode: platform.agent_mode.clone(),
            lead_agent_id: platform.lead_agent_id.clone(),
            workspace_root: platform.workspace_root.clone(),
            captcha_slider_offset_px: platform.captcha_slider_offset_px,
            media_model_overrides: platform.media_model_overrides.clone(),
            agent_performance_modes: platform.agent_performance_modes.clone(),
            media_understanding_modes: platform.media_understanding_modes.clone(),
        }
    }

    pub fn into_platform(self) -> PlatformSettings {
        let mut platform = PlatformSettings::default();
        self.apply_onto(&mut platform);
        platform
    }

    /// Merge persisted agent fields onto runtime platform.
    pub fn apply_onto(&self, platform: &mut PlatformSettings) {
        platform.tool_approval_mode = self.tool_approval_mode.clone();
        platform.user_dynamic_inject_enabled = self.user_dynamic_inject_enabled;
        platform.computer_human_like = self.computer_human_like;
        platform.computer_initial_tier = self.computer_initial_tier.clone();
        platform.context_compression_enabled = self.context_compression_enabled;
        platform.context_budget_tokens = self.context_budget_tokens;
        platform.context_keep_recent_user_turns = self.context_keep_recent_user_turns;
        platform.context_summary_max_tokens = self.context_summary_max_tokens;
        platform.max_tool_rounds = self.max_tool_rounds;
        platform.agent_mode = if self.agent_mode.trim().is_empty() {
            default_agent_mode()
        } else {
            self.agent_mode.clone()
        };
        platform.lead_agent_id = if self.lead_agent_id.trim().is_empty() {
            PlatformSettings::default().lead_agent_id
        } else {
            self.lead_agent_id.clone()
        };
        platform.workspace_root = self.workspace_root.clone();
        platform.captcha_slider_offset_px = self.captcha_slider_offset_px;
        platform.media_model_overrides = self.media_model_overrides.clone();
        platform.agent_performance_modes = self.agent_performance_modes.clone();
        platform.media_understanding_modes = self.media_understanding_modes.clone();
    }
}

fn default_computer_tier_llm() -> HashMap<String, ComputerTierLlmConfig> {
    let mut m = HashMap::new();
    m.insert(
        "primary".into(),
        ComputerTierLlmConfig {
            provider_id: "qwen".into(),
            model: DEFAULT_MODEL_PRIMARY.into(),
            enable_thinking: true,
            thinking_budget: Some(PRIMARY_INTERMEDIATE_THINKING_BUDGET),
        },
    );
    m.insert(
        "intermediate".into(),
        ComputerTierLlmConfig {
            provider_id: "qwen".into(),
            model: DEFAULT_MODEL_INTERMEDIATE.into(),
            enable_thinking: true,
            thinking_budget: Some(PRIMARY_INTERMEDIATE_THINKING_BUDGET),
        },
    );
    m.insert(
        "advanced".into(),
        ComputerTierLlmConfig {
            provider_id: "qwen".into(),
            model: DEFAULT_MODEL_ADVANCED.into(),
            enable_thinking: true,
            thinking_budget: Some(ADVANCED_THINKING_BUDGET),
        },
    );
    m
}

fn mode_llm_entry(provider_id: &str, model: &str, budget: u32) -> ComputerTierLlmConfig {
    ComputerTierLlmConfig {
        provider_id: provider_id.into(),
        model: model.into(),
        enable_thinking: true,
        thinking_budget: Some(budget),
    }
}

fn default_agent_mode_llm() -> HashMap<String, HashMap<String, ComputerTierLlmConfig>> {
    let mut general = HashMap::new();
    general.insert("fast".into(), mode_llm_entry("deepseek", "deepseek-v4-flash", 2048));
    general.insert(
        "standard".into(),
        mode_llm_entry("deepseek", "deepseek-v4-pro", 2048),
    );
    general.insert("expert".into(), mode_llm_entry("qwen", "qwen3.7-plus", 8192));

    let mut coder = HashMap::new();
    coder.insert("fast".into(), mode_llm_entry("deepseek", "deepseek-v4-flash", 2048));
    coder.insert("standard".into(), mode_llm_entry("deepseek", "deepseek-v4-pro", 4096));
    coder.insert("expert".into(), mode_llm_entry("qwen", "qwen3.7-max", 8192));

    let mut m = HashMap::new();
    m.insert("general".into(), general);
    m.insert("coder".into(), coder);
    m
}

fn default_media_mode_llm() -> HashMap<String, HashMap<String, ComputerTierLlmConfig>> {
    let mut image = HashMap::new();
    image.insert("fast".into(), mode_llm_entry("qwen", "qwen3.5-flash", 2048));
    image.insert("standard".into(), mode_llm_entry("qwen", "qwen3.5-plus", 2048));
    image.insert("expert".into(), mode_llm_entry("qwen", "qwen3.6-plus", 8192));

    let mut audio = HashMap::new();
    audio.insert(
        "fast".into(),
        mode_llm_entry("qwen", "qwen3-asr-flash", 2048),
    );
    audio.insert(
        "standard".into(),
        mode_llm_entry("qwen", "qwen3-asr-flash", 2048),
    );
    audio.insert(
        "expert".into(),
        mode_llm_entry("qwen", "qwen3-asr-flash", 8192),
    );

    let mut video = HashMap::new();
    video.insert("fast".into(), mode_llm_entry("qwen", "qwen3.5-flash", 2048));
    video.insert("standard".into(), mode_llm_entry("qwen", "qwen3.5-plus", 2048));
    video.insert("expert".into(), mode_llm_entry("qwen", "qwen3.6-plus", 8192));

    let mut m = HashMap::new();
    m.insert("image".into(), image);
    m.insert("audio".into(), audio);
    m.insert("video".into(), video);
    m
}

fn default_platform_agent_models() -> HashMap<String, AgentModelRef> {
    let mut m = HashMap::new();
    m.insert(
        "coder".into(),
        AgentModelRef {
            provider_id: "deepseek".into(),
            model: "deepseek-v4-pro".into(),
        },
    );
    m.insert(
        "explore".into(),
        AgentModelRef {
            provider_id: "deepseek".into(),
            model: "deepseek-v4-flash".into(),
        },
    );
    m.insert(
        "computer".into(),
        AgentModelRef {
            provider_id: "qwen".into(),
            model: "qwen3.5-plus".into(),
        },
    );
    m.insert(
        "general".into(),
        AgentModelRef {
            provider_id: "deepseek".into(),
            model: "deepseek-v4-flash".into(),
        },
    );
    m.insert(
        "supervisor".into(),
        AgentModelRef {
            provider_id: "deepseek".into(),
            model: "deepseek-v4-pro".into(),
        },
    );
    m.insert(
        "research".into(),
        AgentModelRef {
            provider_id: "qwen".into(),
            model: "qwen3.6-plus".into(),
        },
    );
    m
}

fn platform_default_context_compression_enabled() -> bool {
    true
}

fn platform_default_context_budget_tokens() -> u32 {
    100_000
}

fn platform_default_context_keep_recent_user_turns() -> u32 {
    3
}

fn platform_default_context_summary_max_tokens() -> u32 {
    1024
}

fn platform_default_max_tool_rounds() -> u32 {
    200
}

fn platform_default_raw_content_view_enabled() -> bool {
    false
}

fn platform_default_temperature() -> f32 {
    0.3
}

fn platform_default_max_tokens() -> u32 {
    64_000
}

impl Default for PlatformSettings {
    fn default() -> Self {
        Self {
            providers: vec![
                ProviderConfig {
                    id: "qwen".into(),
                    name: "千问".into(),
                    base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1".into(),
                    api_key: String::new(),
                    models: default_qwen_provider_models(),
                    reasoning_in_messages: Some(false),
                    temperature: Some(platform_default_temperature()),
                    max_tokens: Some(platform_default_max_tokens()),
                    model_configs: HashMap::new(),
                    enable_thinking: Some(true),
                    thinking_budget: Some(2048),
                    reasoning_effort: None,
                },
                ProviderConfig {
                    id: "deepseek".into(),
                    name: "深度求索".into(),
                    base_url: "https://api.deepseek.com/v1".into(),
                    api_key: String::new(),
                    models: vec!["deepseek-v4-flash".into(), "deepseek-v4-pro".into()],
                    reasoning_in_messages: Some(true),
                    temperature: Some(platform_default_temperature()),
                    max_tokens: Some(platform_default_max_tokens()),
                    model_configs: HashMap::new(),
                    enable_thinking: None,
                    thinking_budget: None,
                    reasoning_effort: None,
                },
                ProviderConfig {
                    id: "doubao".into(),
                    name: "豆包".into(),
                    base_url: "https://ark.cn-beijing.volces.com/api/v3".into(),
                    api_key: String::new(),
                    models: default_doubao_provider_models(),
                    reasoning_in_messages: None,
                    temperature: Some(platform_default_temperature()),
                    max_tokens: Some(platform_default_max_tokens()),
                    model_configs: HashMap::new(),
                    enable_thinking: None,
                    thinking_budget: None,
                    reasoning_effort: None,
                },
            ],
            active_provider_id: default_active_provider_id(),
            model: default_model_name(),
            temperature: build_cfg_f32!("TEMPERATURE", platform_default_temperature()),
            max_tokens: build_cfg_u32!("MAX_TOKENS", platform_default_max_tokens()),
            tool_approval_mode: default_tool_approval_mode(),
            agent_mode: default_agent_mode(),
            workspace_root: default_workspace_root(),
            lead_agent_id: default_lead_agent_id(),
            context_compression_enabled: platform_default_context_compression_enabled(),
            context_budget_tokens: platform_default_context_budget_tokens(),
            context_keep_recent_user_turns: platform_default_context_keep_recent_user_turns(),
            context_summary_max_tokens: platform_default_context_summary_max_tokens(),
            max_tool_rounds: platform_default_max_tool_rounds(),
            max_sub_agent_tool_rounds: platform_default_max_tool_rounds(),
            raw_content_view_enabled: platform_default_raw_content_view_enabled(),
            debug_dump_llm_prompts: default_debug_dump_llm_prompts(),
            debug_menus_enabled: default_debug_menus_enabled(),
            task_board_show_child_boards: default_task_board_show_child_boards(),
            user_dynamic_inject_enabled: default_user_dynamic_inject_enabled(),
            agent_default_models: default_platform_agent_models(),
            agent_task_board_history_trim: HashMap::new(),
            computer_human_like: true,
            computer_initial_tier: default_computer_initial_tier(),
            computer_annotated_screen_view_enabled: default_computer_annotated_screen_view_enabled(),
            dati_api_url: default_dati_api_url(),
            dati_authcode: default_dati_authcode(),
            dati_typeno: default_dati_typeno(),
            dati_author: default_dati_author(),
            captcha_slider_offset_px: default_captcha_slider_offset_px(),
            computer_show_monitor_picker: default_computer_show_monitor_picker(),
            agent_ui_overrides: HashMap::new(),
            web_search_model: default_web_search_model_setting(),
            media_model_overrides: default_media_generation_overrides(),
            agent_performance_modes: HashMap::new(),
            media_understanding_modes: MediaUnderstandingModes::default(),
            computer_tier_llm: default_computer_tier_llm(),
            agent_mode_llm: default_agent_mode_llm(),
            media_mode_llm: default_media_mode_llm(),
        }
    }
}

/// API response: user + platform slices and merged runtime view.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectiveSettingsView {
    pub user: UserSettings,
    pub platform: PlatformSettings,
    pub merged: ModelSettings,
    #[serde(rename = "canEditPlatform")]
    pub can_edit_platform: bool,
    #[serde(rename = "isPlatformAdmin")]
    pub is_platform_admin: bool,
}

/// Merge persisted user settings with in-memory platform config.
pub fn merge_user_platform(user: &UserSettings, platform: &PlatformSettings) -> ModelSettings {
    ModelSettings {
        providers: platform.providers.clone(),
        active_provider_id: platform.active_provider_id.clone(),
        model: platform.model.clone(),
        api_key: String::new(),
        temperature: platform.temperature,
        max_tokens: platform.max_tokens,
        has_key: platform.providers.iter().any(|p| !p.api_key.is_empty()),
        tool_approval_mode: platform.tool_approval_mode.clone(),
        agent_mode: platform.agent_mode.clone(),
        workspace_root: platform.workspace_root.clone(),
        lead_agent_id: platform.lead_agent_id.clone(),
        context_compression_enabled: platform.context_compression_enabled,
        context_budget_tokens: platform.context_budget_tokens,
        context_keep_recent_user_turns: platform.context_keep_recent_user_turns,
        context_summary_max_tokens: platform.context_summary_max_tokens,
        max_tool_rounds: platform.max_tool_rounds,
        max_sub_agent_tool_rounds: platform.max_sub_agent_tool_rounds,
        raw_content_view_enabled: platform.raw_content_view_enabled,
        debug_dump_llm_prompts: platform.debug_dump_llm_prompts,
        debug_menus_enabled: platform.debug_menus_enabled,
        task_board_show_child_boards: platform.task_board_show_child_boards,
        user_dynamic_inject_enabled: platform.user_dynamic_inject_enabled,
        agent_default_models: platform.agent_default_models.clone(),
        agent_task_board_history_trim: platform.agent_task_board_history_trim.clone(),
        computer_human_like: platform.computer_human_like,
        computer_initial_tier: platform.computer_initial_tier.clone(),
        computer_annotated_screen_view_enabled: platform.computer_annotated_screen_view_enabled,
        dati_api_url: platform.dati_api_url.clone(),
        dati_authcode: platform.dati_authcode.clone(),
        dati_typeno: platform.dati_typeno.clone(),
        dati_author: platform.dati_author.clone(),
        captcha_slider_offset_px: platform.captcha_slider_offset_px,
        computer_show_monitor_picker: platform.computer_show_monitor_picker,
        memory_enabled: user.memory_enabled,
        user_profile_enabled: user.user_profile_enabled,
        memory_char_limit: user.memory_char_limit,
        user_char_limit: user.user_char_limit,
        memory_nudge_interval: user.memory_nudge_interval,
        background_review_enabled: user.background_review_enabled,
        skill_creation_nudge_interval: user.skill_creation_nudge_interval,
        curator_enabled: user.curator_enabled,
        curator_idle_hours: user.curator_idle_hours,
        curator_interval_days: user.curator_interval_days,
        theme: user.theme.clone(),
        agent_ui_overrides: platform.agent_ui_overrides.clone(),
        web_search_model: platform.web_search_model.clone(),
        media_model_overrides: platform.media_model_overrides.clone(),
        agent_performance_modes: platform.agent_performance_modes.clone(),
        media_understanding_modes: platform.media_understanding_modes.clone(),
        agent_mode_llm: platform.agent_mode_llm.clone(),
        media_mode_llm: platform.media_mode_llm.clone(),
        round_enable_thinking: None,
        round_thinking_budget: None,
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
    /// `system` (bundled under data dir) or `user` (`~/.pointer/skills`).
    #[serde(default = "default_skill_provenance", rename = "provenance")]
    pub provenance: String,
    /// Whether review / skill_patch may modify this skill.
    #[serde(default = "default_skill_mutable", rename = "mutable")]
    pub mutable: bool,
}

fn default_skill_provenance() -> String {
    "user".to_string()
}

fn default_skill_mutable() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillImportResult {
    pub imported: Vec<SkillDef>,
    pub skipped: Vec<String>,
}

/// Tool identity exposed to the UI / API. Human-readable docs and argument shapes live in markdown
/// (`tools/prompts/*.md`, `agents/coder/prompts/*.md`, `agents/computer/tools/prompts/*.md`). Native OpenAI `tools` payloads use
/// empty `parameters` objects; the wire format carries real argument structure in JSON (legacy XML path may still exist).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDef {
    pub name: String,
}

/// Annotated desktop screenshot for UI preview (same style as model vision inject).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputerAnnotatedPreview {
    #[serde(rename = "imageBase64")]
    pub image_base64: String,
    /// `image/jpeg` or `image/png` for UI `data:` URLs.
    #[serde(rename = "imageMime", default = "default_computer_preview_mime")]
    pub image_mime: String,
    pub caption: String,
}

pub fn default_computer_preview_mime() -> String {
    "image/jpeg".to_string()
}

/// Chat attachment bytes for UI bubble reload.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMediaPreview {
    pub data_base64: String,
    pub mime_type: String,
    pub file_name: String,
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
    /// Workspace root for this conversation run (overrides global settings when non-empty).
    #[serde(default, rename = "workspaceRoot", skip_serializing_if = "String::is_empty")]
    pub workspace_root: String,
    /// Session lead worker override for this run (`single` mode).
    #[serde(default, rename = "leadAgentId", skip_serializing_if = "Option::is_none")]
    pub lead_agent_id: Option<String>,
}

/// Metadata emitted when context compression replaces older turns with a summary.
#[derive(Debug, Clone, Serialize)]
pub struct ContextCompressionInfo {
    /// `budget` or `tool_limit`
    pub reason: String,
    #[serde(rename = "messagesBefore")]
    pub messages_before: u32,
    #[serde(rename = "messagesAfter")]
    pub messages_after: u32,
    #[serde(rename = "droppedCount")]
    pub dropped_count: u32,
    #[serde(rename = "keepRecentUserTurns")]
    pub keep_recent_user_turns: u32,
    /// `main` or `sub_agent`
    pub scope: String,
    #[serde(skip_serializing_if = "Option::is_none", rename = "subAgentId")]
    pub sub_agent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "subAgentName")]
    pub sub_agent_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "taskId")]
    pub task_id: Option<String>,
}

/// Cited source entry for web-search stream UI events.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WebSearchSourceEntry {
    pub index: u32,
    pub title: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub site_name: Option<String>,
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
        #[serde(default, skip_serializing_if = "Option::is_none", rename = "traceId")]
        trace_id: Option<String>,
    },
    ReasoningDelta {
        #[serde(rename = "messageId")]
        message_id: String,
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none", rename = "traceId")]
        trace_id: Option<String>,
    },
    /// Progressive `thoughts` / `headline` / `tool_name` / `response` body (`tool_args.text`) from partial JSON repair while streaming.
    AssistantJsonPartial {
        #[serde(rename = "messageId")]
        message_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        thoughts: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        headline: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none", rename = "toolName")]
        tool_name: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none", rename = "responseText")]
        response_text: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none", rename = "traceId")]
        trace_id: Option<String>,
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
        #[serde(default, skip_serializing_if = "Option::is_none", rename = "traceId")]
        trace_id: Option<String>,
    },
    ToolCallArgsDelta {
        #[serde(rename = "messageId")]
        message_id: String,
        #[serde(rename = "toolCallId")]
        tool_call_id: String,
        #[serde(rename = "argsDelta")]
        args_delta: String,
        #[serde(default, skip_serializing_if = "Option::is_none", rename = "traceId")]
        trace_id: Option<String>,
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
        #[serde(default, skip_serializing_if = "Option::is_none", rename = "displayLabel")]
        display_label: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none", rename = "displaySummary")]
        display_summary: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none", rename = "traceId")]
        trace_id: Option<String>,
    },
    TerminalOutputDelta {
        #[serde(rename = "messageId")]
        message_id: String,
        #[serde(rename = "toolCallId")]
        tool_call_id: String,
        #[serde(rename = "output")]
        output: String,
        #[serde(default, skip_serializing_if = "Option::is_none", rename = "traceId")]
        trace_id: Option<String>,
    },
    WebSearchOutputDelta {
        #[serde(rename = "messageId")]
        message_id: String,
        #[serde(rename = "toolCallId")]
        tool_call_id: String,
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none", rename = "traceId")]
        trace_id: Option<String>,
    },
    WebSearchSourcesReady {
        #[serde(rename = "messageId")]
        message_id: String,
        #[serde(rename = "toolCallId")]
        tool_call_id: String,
        sources: Vec<WebSearchSourceEntry>,
        #[serde(rename = "searchCount")]
        search_count: u32,
        #[serde(default, skip_serializing_if = "Option::is_none", rename = "traceId")]
        trace_id: Option<String>,
    },
    MessageEnd {
        #[serde(rename = "messageId")]
        message_id: String,
        /// 与持久化助手消息对齐的最终正文（已去掉 XML 工具块等）
        #[serde(skip_serializing_if = "Option::is_none")]
        content: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none", rename = "rawContent")]
        raw_content: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none", rename = "toolRawOutput")]
        tool_raw_output: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        thoughts: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        headline: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none", rename = "traceId")]
        trace_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        attachments: Option<Vec<MediaAttachment>>,
    },
    /// Synthetic user row so the model (and UI history) see recovery instructions mid-run.
    InjectedUserMessage {
        #[serde(rename = "conversationId")]
        conversation_id: String,
        #[serde(rename = "messageId")]
        message_id: String,
        content: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        attachments: Option<Vec<MediaAttachment>>,
    },
    /// User message attachments processed (ASR, storage path, etc.).
    UserMessageAttachmentsUpdated {
        #[serde(rename = "conversationId")]
        conversation_id: String,
        #[serde(rename = "messageId")]
        message_id: String,
        attachments: Vec<MediaAttachment>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        content: Option<String>,
    },
    /// IM `/new` or idle reset created a new desktop sidebar row for the same IM thread.
    ImSessionForked {
        #[serde(rename = "conversationId")]
        conversation_id: String,
        #[serde(rename = "baseConversationId")]
        base_conversation_id: String,
        title: String,
        #[serde(rename = "sessionEpoch")]
        session_epoch: u32,
        #[serde(rename = "leadAgentId")]
        lead_agent_id: String,
        #[serde(rename = "agentMode")]
        agent_mode: String,
    },
    /// IM session agent / mode changed (mirror sidebar + Composer).
    ImSessionAgentChanged {
        #[serde(rename = "conversationId")]
        conversation_id: String,
        #[serde(rename = "baseConversationId")]
        base_conversation_id: String,
        #[serde(rename = "leadAgentId")]
        lead_agent_id: String,
        #[serde(rename = "agentMode")]
        agent_mode: String,
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
    /// Task-board trim marked earlier messages excluded from LLM context (UI patch only).
    ContextTrimApplied {
        #[serde(rename = "conversationId")]
        conversation_id: String,
        #[serde(rename = "excludedMessageIds")]
        excluded_message_ids: Vec<String>,
    },
    /// Main-thread context compression: soft-exclude prefix + insert summary user row.
    ContextCompressionApplied {
        #[serde(rename = "conversationId")]
        conversation_id: String,
        #[serde(rename = "excludedMessageIds")]
        excluded_message_ids: Vec<String>,
        #[serde(rename = "insertBeforeMessageId")]
        insert_before_message_id: String,
        #[serde(rename = "summaryMessage")]
        summary_message: ChatMessage,
        compression: ContextCompressionInfo,
    },
    /// Sub-agent local history was compressed; main thread messages are unchanged.
    ContextCompressed {
        #[serde(rename = "conversationId")]
        conversation_id: String,
        #[serde(rename = "messageId")]
        message_id: String,
        compression: ContextCompressionInfo,
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
    /// Supervisor finished planning; UI may show a task checklist.
    SupervisorPlan {
        #[serde(rename = "conversationId")]
        conversation_id: String,
        #[serde(rename = "messageId")]
        message_id: String,
        tasks: Vec<SupervisorPlanTask>,
    },
    /// Task board document changed (for chat UI panel).
    TaskBoardUpdated {
        #[serde(rename = "conversationId")]
        conversation_id: String,
        #[serde(rename = "storeKey")]
        store_key: String,
        #[serde(default, skip_serializing_if = "Option::is_none", rename = "anchorMessageId")]
        anchor_message_id: Option<String>,
        document: serde_json::Value,
    },
    /// Skill catalog changed (import / reload); UI should refresh the skill list.
    SkillsUpdated {
        #[serde(rename = "conversationId")]
        conversation_id: String,
        #[serde(rename = "importedIds")]
        imported_ids: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none", rename = "enabledIds")]
        enabled_ids: Option<Vec<String>>,
    },
    /// Conversation workspace root changed mid-run (e.g. general → coder delegation).
    WorkspaceUpdated {
        #[serde(rename = "conversationId")]
        conversation_id: String,
        #[serde(rename = "workspaceRoot")]
        workspace_root: String,
        #[serde(rename = "isEphemeralSandbox")]
        is_ephemeral_sandbox: bool,
    },
    /// Sub-agent computer delegation blocked until the user picks a monitor.
    ComputerMonitorPickRequired {
        #[serde(rename = "conversationId")]
        conversation_id: String,
        #[serde(rename = "messageId")]
        message_id: String,
        #[serde(rename = "toolCallId")]
        tool_call_id: String,
        monitors: Vec<ComputerMonitor>,
    },
    /// Monitor selection applied (auto single-monitor or user pick).
    ComputerMonitorUpdated {
        #[serde(rename = "conversationId")]
        conversation_id: String,
        #[serde(rename = "monitorId")]
        monitor_id: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupervisorPlanTask {
    pub id: String,
    pub title: String,
    #[serde(rename = "agentId")]
    pub agent_id: String,
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

/// The UI stores tool output on `assistant.toolCalls[].result` and may omit `role: tool` rows.
/// Before OpenAI-wire serialization we expand into canonical assistant + synthetic `role: tool`
/// rows (one per call id), so native tool-calling providers receive complete context.
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
                            tool_raw_output: None,
                            agent_id: None,
                            agent_instance_id: None,
                            agent_name: None,
                            agent_trace: None,
                            image_slot_labels: None,
                            images_base64: None,
                            computer_round_screen_rel_path: None,
        ui_bindings: None,
            context_state: None,
        attachments: None,
            });
                    }
                    i = j;
                    continue;
                }
            }
        }
        if matches!(m.role, Role::Tool) {
            log::warn!(
                "expand_tool_messages_for_openai_request: skipping orphan tool message id={} (no preceding assistant with tool_calls)",
                m.id
            );
        } else {
            out.push(m.clone());
        }
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

/// DashScope / 百炼 OpenAI 兼容接口：千问显式 Context Cache（`cache_control.type = ephemeral`）。
/// 见 https://help.aliyun.com/zh/model-studio/context-cache
pub fn qwen_explicit_system_cache_enabled(settings: &ModelSettings) -> bool {
    let Some((provider, model)) = active_provider_and_model(settings) else {
        return false;
    };
    if !provider_uses_dashscope_compatible_api(provider) {
        return false;
    }
    qwen_model_supports_explicit_cache(model)
}

pub fn provider_uses_dashscope_compatible_api(provider: &ProviderConfig) -> bool {
    if provider.id.eq_ignore_ascii_case("qwen") {
        return true;
    }
    let url = provider.base_url.to_ascii_lowercase();
    url.contains("dashscope.aliyuncs.com") || url.contains("dashscope-intl.aliyuncs.com")
}

fn qwen_model_supports_explicit_cache(model: &str) -> bool {
    let m = model.trim().to_ascii_lowercase();
    m.starts_with("qwen")
}

/// System prompt slices for `stream_chat`: **cacheable** (stable per session) vs **dynamic** (per round).
#[derive(Debug, Clone, Default)]
pub struct SystemPromptSections {
    /// COMMUNICATION_PUBLIC, agent prompts, tool appendix — stable across tool rounds.
    pub cacheable: Vec<String>,
    /// Per-round slices only (e.g. `[TASK_BOARD]` from `before_main_llm_call` hooks).
    pub dynamic: Vec<String>,
}

impl SystemPromptSections {
    pub fn is_empty(&self) -> bool {
        self.cacheable.is_empty() && self.dynamic.is_empty()
    }

    pub fn slice_count(&self) -> usize {
        self.cacheable.len() + self.dynamic.len()
    }

    /// One-shot callers (`chat_once`) with no per-round dynamic tail.
    pub fn all_cacheable(parts: Vec<String>) -> Self {
        Self {
            cacheable: parts,
            dynamic: Vec::new(),
        }
    }
}

fn join_prompt_slices(slices: &[String]) -> String {
    slices
        .iter()
        .map(|s| s.as_str())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Append HTTP `system` message(s) from [`SystemPromptSections`].
///
/// When `explicit_system_cache` is on and `cacheable` is non-empty, the cache marker sits on the
/// **cacheable** block only; `dynamic` (typically `[TASK_BOARD]` only) follows as a second content part.
fn push_openai_system_messages(
    out: &mut Vec<serde_json::Value>,
    sections: &SystemPromptSections,
    explicit_system_cache: bool,
) {
    if sections.is_empty() {
        return;
    }
    let cacheable_text = join_prompt_slices(&sections.cacheable);
    let dynamic_text = join_prompt_slices(&sections.dynamic);

    if explicit_system_cache && !cacheable_text.is_empty() {
        let mut parts = vec![serde_json::json!({
            "type": "text",
            "text": cacheable_text,
            "cache_control": { "type": "ephemeral" }
        })];
        if !dynamic_text.is_empty() {
            parts.push(serde_json::json!({
                "type": "text",
                "text": dynamic_text
            }));
        }
        out.push(serde_json::json!({
            "role": "system",
            "content": parts
        }));
        return;
    }

    let mut merged = sections.cacheable.clone();
    merged.extend(sections.dynamic.clone());
    let system_text = join_prompt_slices(&merged);
    if !system_text.is_empty() {
        out.push(serde_json::json!({
            "role": "system",
            "content": system_text
        }));
    }
}

fn flatten_user_images_to_text(m: &ChatMessage) -> String {
    let mut content = m.content.clone();
    let Some(imgs) = m.images_base64.as_ref() else {
        return content;
    };
    if imgs.is_empty() {
        return content;
    }
    let labels = m.image_slot_labels.as_deref();
    for (i, _) in imgs.iter().enumerate() {
        let lab = labels
            .and_then(|labs| labs.get(i))
            .filter(|s| !s.trim().is_empty())
            .map(|s| s.as_str())
            .unwrap_or("[image attachment]");
        if !content.is_empty() {
            content.push_str("\n\n");
        }
        content.push_str(lab);
        content.push_str(" (image not inlined: primary model does not support vision)");
    }
    content
}

pub fn make_openai_messages(
    msgs: &[ChatMessage],
    system: &SystemPromptSections,
    include_reasoning_in_api: bool,
    explicit_system_cache: bool,
    inline_vision: bool,
) -> Vec<serde_json::Value> {
    let included = crate::message_context::filter_context_messages(msgs);
    let expanded = expand_tool_messages_for_openai_request(&included);
    let mut out: Vec<serde_json::Value> = Vec::new();
    push_openai_system_messages(&mut out, system, explicit_system_cache);
    for m in &expanded {
        match m.role {
            Role::System => out.push(serde_json::json!({
                "role": "system", "content": m.content
            })),
            Role::User => {
                if let Some(ref imgs) = m.images_base64 {
                    if !imgs.is_empty() && !inline_vision {
                        log::warn!(
                            "make_openai_messages: stripping {} inline image(s); model does not support vision",
                            imgs.len()
                        );
                        out.push(serde_json::json!({
                            "role": "user",
                            "content": flatten_user_images_to_text(m)
                        }));
                        continue;
                    }
                    if !imgs.is_empty() {
                        let mut parts: Vec<serde_json::Value> = Vec::new();
                        if !m.content.trim().is_empty() {
                            parts.push(serde_json::json!({
                                "type": "text",
                                "text": m.content
                            }));
                        }
                        let labels = m.image_slot_labels.as_deref();
                        if let Some(labs) = labels {
                            if labs.len() != imgs.len() {
                                log::warn!(
                                    "user message image_slot_labels len {} != images_base64 len {}",
                                    labs.len(),
                                    imgs.len()
                                );
                            }
                        }
                        for (i, b64) in imgs.iter().enumerate() {
                            if let Some(lab) = labels.and_then(|labs| labs.get(i)) {
                                if !lab.trim().is_empty() {
                                    parts.push(serde_json::json!({
                                        "type": "text",
                                        "text": format!("{lab}\n")
                                    }));
                                }
                            }
                            let mime = crate::agents::computer::vision::screen::image_data_url_mime_from_base64(b64);
                            let url = format!("data:{mime};base64,{b64}");
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
                if let Some(tcs) = &m.tool_calls {
                    let tool_calls: Vec<serde_json::Value> = tcs
                        .iter()
                        .filter(|t| {
                            !t.id.trim().is_empty()
                                && !t.name.trim().is_empty()
                                && tool_registry_base_name(&t.name) != "response"
                        })
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
                    if !tool_calls.is_empty() {
                        obj.insert("tool_calls".into(), serde_json::Value::Array(tool_calls));
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
            thoughts: None,
            headline: None,
            raw_content: None,
            tool_raw_output: None,
            agent_id: None,
            agent_instance_id: None,
            agent_name: None,
            agent_trace: None,
            image_slot_labels: None,
            images_base64: None,
            computer_round_screen_rel_path: None,
        ui_bindings: None,
            context_state: None,
        attachments: None,
            }
    }

    #[test]
    fn system_prompt_uses_ephemeral_cache_control_on_cacheable_only() {
        let system = SystemPromptSections {
            cacheable: vec!["static system".into()],
            dynamic: vec!["task board".into()],
        };
        let out = make_openai_messages(&[], &system, false, true, false);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["role"], "system");
        let content = out[0]["content"].as_array().expect("multipart system");
        assert_eq!(content.len(), 2);
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[0]["text"], "static system");
        assert_eq!(content[0]["cache_control"]["type"], "ephemeral");
        assert_eq!(content[1]["text"], "task board");
        assert!(content[1].get("cache_control").is_none());
    }

    #[test]
    fn system_prompt_plain_string_when_cache_disabled() {
        let system = SystemPromptSections::all_cacheable(vec!["static system".into()]);
        let out = make_openai_messages(&[], &system, false, false, false);
        assert_eq!(out[0]["content"], "static system");
    }

    #[test]
    fn user_message_with_images_uses_multipart_content() {
        let mut u = msg(Role::User);
        u.content = "see screen".into();
        u.images_base64 = Some(vec!["iVBORw0KGgo=".into()]);
        let out = make_openai_messages(&[u], &SystemPromptSections::default(), false, false, true);
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
    fn user_message_without_vision_flattens_images_to_text() {
        let mut u = msg(Role::User);
        u.content = "what is this".into();
        u.images_base64 = Some(vec!["iVBORw0KGgo=".into()]);
        let out = make_openai_messages(&[u], &SystemPromptSections::default(), false, false, false);
        assert_eq!(out[0]["role"], "user");
        let content = out[0]["content"].as_str().expect("text content");
        assert!(content.contains("what is this"));
        assert!(content.contains("does not support vision"));
        assert!(out[0]["content"].as_array().is_none());
    }

    #[test]
    fn user_message_interleaves_slot_label_before_each_image() {
        let mut u = msg(Role::User);
        u.content = "[CUR_SCREEN] preamble".into();
        u.image_slot_labels = Some(vec!["[Screen after action]".into()]);
        u.images_base64 = Some(vec!["iVBORw0KGgo=".into()]);
        let out = make_openai_messages(&[u], &SystemPromptSections::default(), false, false, true);
        let content = out[0]["content"].as_array().expect("multipart content");
        assert_eq!(content.len(), 3);
        assert_eq!(content[0]["text"], "[CUR_SCREEN] preamble");
        assert_eq!(content[1]["text"], "[Screen after action]\n");
        assert_eq!(content[2]["type"], "image_url");
    }

    #[test]
    fn assistant_includes_reasoning_content_when_present() {
        let mut a = msg(Role::Assistant);
        a.content = "answer".into();
        a.reasoning = Some("step 1…".into());
        let out = make_openai_messages(&[a], &SystemPromptSections::default(), true, false, false);
        assert_eq!(out[0]["role"], "assistant");
        assert_eq!(out[0]["content"], "answer");
        assert_eq!(out[0]["reasoning_content"], "step 1…");
    }

    #[test]
    fn assistant_omits_reasoning_content_when_disabled() {
        let mut a = msg(Role::Assistant);
        a.content = "answer".into();
        a.reasoning = Some("hidden".into());
        let out = make_openai_messages(&[a], &SystemPromptSections::default(), false, false, false);
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
            display_label: None,
            display_summary: None,
        }]);
        let mut t = msg(Role::Tool);
        t.tool_call_id = Some("call_abc".into());
        t.content = "{}".into();

        let out = make_openai_messages(&[a, t], &SystemPromptSections::default(), true, false, false);
        assert_eq!(out.len(), 2, "assistant + tool");
        assert_eq!(out[0]["role"], "assistant");
        assert!(out[0].as_object().unwrap().get("tool_calls").is_some());
        assert_eq!(out[0]["content"], "x");
        assert_eq!(out[1]["role"], "tool");
        assert_eq!(out[1]["tool_call_id"], "call_abc");
        assert_eq!(out[1]["content"], "{}");
    }

    #[test]
    fn synthesizes_inline_tool_as_tool_message_after_expand() {
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
            display_label: None,
            display_summary: None,
        }]);
        let out = make_openai_messages(&[a], &SystemPromptSections::default(), true, false, false);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0]["role"], "assistant");
        assert_eq!(out[0]["content"], "calling");
        assert_eq!(out[1]["role"], "tool");
        assert_eq!(out[1]["tool_call_id"], "call_inline");
        assert_eq!(out[1]["content"], "file body");
    }

    #[test]
    fn excluded_messages_omitted_from_openai_request() {
        let mut excluded = msg(Role::User);
        excluded.content = "old turn".into();
        excluded.context_state = Some(MessageContextState {
            included: false,
            excluded_reason: Some(ExcludedReason::ContextCompression),
        });
        let mut included = msg(Role::User);
        included.content = "recent turn".into();
        let out = make_openai_messages(
            &[excluded, included],
            &SystemPromptSections::default(),
            false,
            false,
            false,
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["content"], "recent turn");
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
            display_label: None,
            display_summary: None,
        }]);
        let out = make_openai_messages(&[a], &SystemPromptSections::default(), false, false, false);
        assert_eq!(out[0]["content"], "visible");
        assert!(out[0].as_object().unwrap().get("tool_calls").is_some());
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
            display_label: None,
            display_summary: None,
        }]);

        let out = make_openai_messages(&[a], &SystemPromptSections::default(), false, false, false);
        assert_eq!(out.len(), 1, "assistant only");
        assert_eq!(out[0]["role"], "assistant");
        assert_eq!(out[0]["content"], "");
        assert!(out[0].as_object().unwrap().get("tool_calls").is_none());
    }

    #[test]
    fn orphan_tool_message_skipped_on_wire() {
        let mut assistant = msg(Role::Assistant);
        assistant.content = "done".into();
        assistant.tool_calls = Some(vec![ToolCall {
            id: "call_ok".into(),
            name: "terminal".into(),
            arguments: "{}".into(),
            status: "success".into(),
            result: Some("ok".into()),
            error: None,
            duration_ms: None,
            risk_level: None,
            display_label: None,
            display_summary: None,
        }]);
        let mut orphan = msg(Role::Tool);
        orphan.id = "tool_orphan".into();
        orphan.content = "stale db tail".into();
        orphan.tool_call_id = Some("call_orphan".into());

        let out = make_openai_messages(
            &[assistant.clone(), orphan],
            &SystemPromptSections::default(),
            false,
            false,
            false,
        );
        assert_eq!(out.len(), 2, "assistant + synthesized tool from inline result");
        assert_eq!(out[0]["role"], "assistant");
        assert_eq!(out[1]["role"], "tool");
        assert_eq!(out[1]["tool_call_id"], "call_ok");
    }

    #[test]
    fn orphan_tool_after_user_skipped_on_wire() {
        let user = msg(Role::User);
        let mut orphan = msg(Role::Tool);
        orphan.id = "tool_orphan".into();
        orphan.content = "orphan".into();
        orphan.tool_call_id = Some("call_orphan".into());

        let out = make_openai_messages(
            &[user, orphan],
            &SystemPromptSections::default(),
            false,
            false,
            false,
        );
        assert_eq!(out.len(), 1, "user only; orphan tool dropped");
        assert_eq!(out[0]["role"], "user");
    }
}

pub type ToolMap = HashMap<String, ToolDef>;

#[cfg(test)]
mod qwen_explicit_cache_tests {
    use super::*;

    #[test]
    fn enabled_for_default_qwen_provider() {
        let s = ModelSettings::default();
        assert!(qwen_explicit_system_cache_enabled(&s));
    }

    #[test]
    fn disabled_for_deepseek_provider() {
        let mut s = ModelSettings::default();
        s.active_provider_id = "deepseek".into();
        s.model = "deepseek-v4-flash".into();
        assert!(!qwen_explicit_system_cache_enabled(&s));
    }

    #[test]
    fn enabled_for_custom_dashscope_base_url() {
        let mut s = ModelSettings::default();
        s.providers[0].id = "custom".into();
        s.providers[0].base_url = "https://dashscope.aliyuncs.com/compatible-mode/v1".into();
        s.model = "qwen-plus".into();
        assert!(qwen_explicit_system_cache_enabled(&s));
    }
}

#[cfg(test)]
mod model_capability_vision_tests {
    use super::*;

    #[test]
    fn qwen_models_default_support_vision() {
        let s = ModelSettings::default();
        let (vision, _, _) = model_capability_flags(&s, "qwen", "qwen3.5-plus");
        assert!(vision);
        let (vision, _, _) = model_capability_flags(&s, "qwen", "qwen3.5-flash");
        assert!(vision);
    }

    #[test]
    fn deepseek_models_do_not_support_vision() {
        let mut s = ModelSettings::default();
        ensure_provider_model_capability_defaults(&mut s);
        let (vision, _, _) = model_capability_flags(&s, "deepseek", "deepseek-v4-flash");
        assert!(!vision);
        let (vision, _, _) = model_capability_flags(&s, "deepseek", "deepseek-v4-pro");
        assert!(!vision);
    }

    #[test]
    fn ensure_provider_resets_deepseek_vision_false() {
        let mut s = ModelSettings::default();
        {
            let ds = s.providers.iter_mut().find(|p| p.id == "deepseek").unwrap();
            ds.model_configs.insert(
                "deepseek-v4-flash".into(),
                ModelRuntimeOverrides {
                    supports_vision: Some(true),
                    ..Default::default()
                },
            );
        }
        ensure_provider_model_capability_defaults(&mut s);
        let ds = s.providers.iter().find(|p| p.id == "deepseek").unwrap();
        let entry = ds.model_configs.get("deepseek-v4-flash").unwrap();
        assert_eq!(entry.supports_vision, Some(false));
    }
}

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
mod effective_generation_tests {
    use super::*;

    #[test]
    fn effective_temperature_provider_default() {
        let mut s = ModelSettings::default();
        s.model = "qwen3.5-plus".into();
        s.temperature = 0.2;
        s.providers[0].temperature = Some(0.9);
        assert!((effective_temperature(&s) - 0.9).abs() < f32::EPSILON);
    }

    #[test]
    fn effective_max_tokens_provider_default() {
        let mut s = ModelSettings::default();
        s.model = "qwen3.5-plus".into();
        s.max_tokens = 512;
        s.providers[0].max_tokens = Some(8192);
        assert_eq!(effective_max_tokens(&s), 8192);
    }

    #[test]
    fn effective_temperature_model_override() {
        let mut s = ModelSettings::default();
        s.model = "qwen3.5-plus".into();
        s.temperature = 0.2;
        s.providers[0].model_configs.insert(
            "qwen3.5-plus".into(),
            ModelRuntimeOverrides {
                temperature: Some(1.1),
                ..Default::default()
            },
        );
        assert!((effective_temperature(&s) - 1.1).abs() < f32::EPSILON);
    }

    #[test]
    fn effective_max_tokens_model_override() {
        let mut s = ModelSettings::default();
        s.model = "qwen3.5-plus".into();
        s.max_tokens = 512;
        s.providers[0].model_configs.insert(
            "qwen3.5-plus".into(),
            ModelRuntimeOverrides {
                max_tokens: Some(4096),
                ..Default::default()
            },
        );
        assert_eq!(effective_max_tokens(&s), 4096);
    }

    #[test]
    fn ensure_provider_generation_defaults_fills_provider_not_models() {
        let mut s = ModelSettings::default();
        s.temperature = 0.55;
        s.max_tokens = 3000;
        s.providers[0].temperature = None;
        s.providers[0].max_tokens = None;
        ensure_provider_generation_defaults(&mut s);
        let p = &s.providers[0];
        assert!((p.temperature.unwrap() - 0.55).abs() < f32::EPSILON);
        assert_eq!(p.max_tokens.unwrap(), 3000);
        assert!(p.model_configs.get("qwen3.5-plus").is_none());
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
        s.providers[0].enable_thinking = Some(true);
        s.providers[0].thinking_budget = Some(100);
        let m = s.model.clone();
        s.providers[0].model_configs.insert(
            m,
            ModelRuntimeOverrides {
                thinking_budget: Some(500),
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

    #[test]
    fn deepseek_reasoning_effort_on_wire() {
        let mut s = ModelSettings::default();
        s.active_provider_id = "deepseek".into();
        s.model = "deepseek-v4-flash".into();
        s.providers[1].reasoning_effort = Some("max".into());
        let v = effective_chat_extra_body(&s).expect("effort");
        assert_eq!(
            v.get("reasoning_effort"),
            Some(&Value::String("max".into()))
        );
    }

    #[test]
    fn flatten_extra_body_to_root_for_deepseek() {
        let mut s = ModelSettings::default();
        s.active_provider_id = "deepseek".into();
        s.model = "deepseek-v4-flash".into();
        assert!(chat_request_flattens_extra_body(&s));
        let body = serde_json::json!({
            "model": "deepseek-v4-flash",
            "extra_body": {"reasoning_effort": "high"}
        });
        let out = flatten_chat_extra_body_on_wire(body, &s);
        let o = out.as_object().unwrap();
        assert!(!o.contains_key("extra_body"));
        assert_eq!(
            o.get("reasoning_effort"),
            Some(&Value::String("high".into()))
        );
    }

    #[test]
    fn flatten_extra_body_to_root_for_qwen() {
        let s = ModelSettings::default();
        assert!(chat_request_flattens_extra_body(&s));
        let body = serde_json::json!({
            "model": "qwen-plus",
            "extra_body": {"enable_thinking": true, "thinking_budget": 100}
        });
        let out = flatten_chat_extra_body_on_wire(body, &s);
        let o = out.as_object().unwrap();
        assert!(!o.contains_key("extra_body"));
        assert_eq!(o.get("enable_thinking"), Some(&Value::Bool(true)));
        assert_eq!(
            o.get("thinking_budget"),
            Some(&Value::Number(100.into()))
        );
    }

    #[test]
    fn agent_trace_session_round_trips_in_conversation_json() {
        let trace = AgentTrace {
            id: "task-1:computer".into(),
            name: "电脑操控".into(),
            role: "worker".into(),
            status: "completed".into(),
            detail: None,
            content: None,
            depth: Some(1),
            computer_target: Some(ComputerOperationTarget::External),
            session: Some(SubAgentSessionUi {
                thoughts: Some("done".into()),
                stats: SubAgentToolStats {
                    mouse_count: 2,
                    input_count: 1,
                    other_count: 3,
                    ..Default::default()
                },
                summary_line: Some("电脑操控 · 已完成 · 鼠标 2 次 · 输入 1 次 · 其他 3 次".into()),
                collapsed: true,
                ..Default::default()
            }),
        };
        let conv = Conversation {
            id: "c1".into(),
            title: "t".into(),
            created_at: 1,
            updated_at: 1,
            messages: vec![ChatMessage {
                id: "m1".into(),
                role: Role::Assistant,
                content: String::new(),
                status: "done".into(),
                created_at: 1,
                tool_calls: None,
                tool_call_id: None,
                error_message: None,
                reasoning: None,
                thoughts: None,
                headline: None,
                raw_content: None,
                tool_raw_output: None,
                agent_id: None,
                agent_instance_id: None,
                agent_name: None,
                agent_trace: Some(vec![trace]),
                images_base64: None,
                image_slot_labels: None,
                computer_round_screen_rel_path: None,
                ui_bindings: None,
                context_state: None,
        attachments: None,
            }],
            skill_ids: vec![],
            tool_rounds_used: 0,
            tool_rounds_used_supervisor: 0,
            computer_monitor_id: None,
            workspace_root: String::new(),
            workspace_user_set: false,
            workspace_inherit_disabled: false,
            lead_agent_id: default_lead_agent_id(),
            agent_mode: default_agent_mode(),
        };
        let json = serde_json::to_string(&conv).expect("serialize");
        let back: Conversation = serde_json::from_str(&json).expect("deserialize");
        let session = back.messages[0]
            .agent_trace
            .as_ref()
            .and_then(|t| t.first())
            .and_then(|t| t.session.as_ref())
            .expect("session persisted");
        assert_eq!(session.stats.mouse_count, 2);
        assert_eq!(session.stats.input_count, 1);
        assert_eq!(session.collapsed, true);
    }
}
