use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashMap;

use crate::agents::computer::tier::{
    DEFAULT_PIPELINE_POSITION_THINKING_BUDGET, DEFAULT_PIPELINE_VERIFY_THINKING_BUDGET,
};

/// Per-model overrides for runtime/API behavior. Unset fields inherit from the parent provider.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModelRuntimeOverrides {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "reasoningInMessages"
    )]
    pub reasoning_in_messages: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "maxTokens")]
    pub max_tokens: Option<u32>,
    /// Qwen: `enable_thinking` on the chat/completions request.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "enableThinking"
    )]
    pub enable_thinking: Option<bool>,
    /// Qwen: `thinking_budget` when deep thinking is enabled.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "thinkingBudget"
    )]
    pub thinking_budget: Option<u32>,
    /// DeepSeek: `reasoning_effort` — `high` or `max`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "reasoningEffort"
    )]
    pub reasoning_effort: Option<String>,
    /// Thinking wire protocol / strategy id (`budget` | `effort` | `openrouter` | …).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "thinkingProtocol"
    )]
    pub thinking_protocol: Option<String>,
    /// Unified product thinking intensity: `off` | `low` | `medium` | `high` | `max`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "thinkingIntensity"
    )]
    pub thinking_intensity: Option<String>,
    /// Whether the model accepts vision / image understanding input.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "supportsVision"
    )]
    pub supports_vision: Option<bool>,
    /// Whether the model can generate images (`image_generate`).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "canGenerateImage"
    )]
    pub can_generate_image: Option<bool>,
    /// Whether the model can generate videos (`video_generate`).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "canGenerateVideo"
    )]
    pub can_generate_video: Option<bool>,
    /// Hermes-style free-form chat/completions fields (merged into request root on wire).
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
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "reasoningInMessages"
    )]
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
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "enableThinking"
    )]
    pub enable_thinking: Option<bool>,
    /// Qwen: default `thinking_budget` when deep thinking is enabled.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "thinkingBudget"
    )]
    pub thinking_budget: Option<u32>,
    /// DeepSeek: default `reasoning_effort` — `high` or `max`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "reasoningEffort"
    )]
    pub reasoning_effort: Option<String>,
    /// Thinking wire protocol / strategy id (`budget` | `effort` | `openrouter` | …).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "thinkingProtocol"
    )]
    pub thinking_protocol: Option<String>,
    /// Unified product thinking intensity: `off` | `low` | `medium` | `high` | `max`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "thinkingIntensity"
    )]
    pub thinking_intensity: Option<String>,
    /// Hermes-style free-form chat/completions fields for all models under this provider
    /// (per-model `extraBody` overlays). Flattened to request root on wire.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "extraBody")]
    pub extra_body: Option<Value>,
    /// Runtime-only provenance: `user` (persisted user layer) or `platform`
    /// (injected by server.toml / OAuth / login). Computed by `merge_user_platform`
    /// on every merge; not a durable attribute — the disk value is re-derived each load.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
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
                let model = map
                    .get("model")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .trim();
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
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "imageGeneration"
    )]
    pub image_generation: Option<AgentModelRef>,
    /// Video generation tool (`video_generate`); defaults to Qwen Wan 2.7 or Doubao Seedance 1.5.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "videoGeneration"
    )]
    pub video_generation: Option<AgentModelRef>,
}
/// Whether to persist/stream reasoning and send `reasoning_content` on the next request,
/// for the **active** provider + **current** `settings.model`.
///
/// Default is **off**: reasoning is only carried when a provider (or per-model
/// override) explicitly opts in via `reasoningInMessages: true`. DeepSeek-style
/// thinking models that require `reasoning_content` round-trip must set it explicitly.
pub fn effective_reasoning_in_messages(settings: &ModelSettings) -> bool {
    let provider = settings
        .providers
        .iter()
        .find(|p| p.id == settings.active_provider_id)
        .or_else(|| settings.providers.first());
    let Some(p) = provider else {
        return false;
    };
    let model = settings.model.trim();
    if let Some(over) = p.model_configs.get(model) {
        if let Some(v) = over.reasoning_in_messages {
            return v;
        }
    }
    p.reasoning_in_messages.unwrap_or(false)
}

pub const DEFAULT_MODEL_TEMPERATURE: f32 = 0.7;
pub const DEFAULT_MODEL_MAX_TOKENS: u32 = 2048;
/// Qwen `thinking_budget` when deep thinking is enabled and no explicit budget is set.
pub const DEFAULT_THINKING_BUDGET: u32 = 2048;

pub(crate) fn active_provider_and_model<'a>(
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
        if let Some(t) = p.model_configs.get(model).and_then(|o| o.temperature) {
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
    if m.contains("image") || m.contains("seedream") || (m.contains("wan2.") && m.contains("image"))
    {
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

fn resolve_supports_vision(provider_id: &str, model_over: Option<&ModelRuntimeOverrides>) -> bool {
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

pub fn default_media_generation_overrides() -> MediaModelOverrides {
    // 平台模型配置全部由平台下发；本地不再内置图片/视频生成默认模型。
    MediaModelOverrides::default()
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

/// Extension fields for **active** provider + **current** `settings.model` (per-model overrides win).
///
/// Merge order (later wins): provider `extraBody` → model `extraBody` → structured
/// thinking strategy fields. Aligned with Hermes `custom_providers[].extra_body`
/// + OpenAI SDK root-level merge.
pub fn effective_chat_extra_body(settings: &ModelSettings) -> Option<Value> {
    let (provider, model) = active_provider_and_model(settings)?;
    let model_over = provider.model_configs.get(model);

    let mut m = match merge_shallow_json_objects(
        provider.extra_body.as_ref(),
        model_over.and_then(|o| o.extra_body.as_ref()),
    ) {
        Some(Value::Object(map)) => map,
        Some(_) => Map::new(),
        None => Map::new(),
    };
    crate::thinking_strategy::apply_thinking_strategy(
        settings, provider, model, model_over, &mut m,
    );
    if m.is_empty() {
        None
    } else {
        Some(Value::Object(m))
    }
}

/// Force thinking **off** via the active provider thinking strategy.
pub fn apply_thinking_disabled_to_extra_body(settings: &ModelSettings, extra: &mut Option<Value>) {
    crate::thinking_strategy::apply_thinking_disabled_strategy(settings, extra);
}

/// Whether structured wire helpers should treat thinking as currently enabled.
pub fn thinking_enabled_in_extra_body(extra: Option<&Value>) -> bool {
    crate::thinking_strategy::thinking_enabled_in_extra_body(extra)
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

/// Always flatten `extra_body` to the chat/completions request root (Hermes / OpenAI SDK).
pub fn chat_request_flattens_extra_body(_settings: &ModelSettings) -> bool {
    true
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
    #[serde(
        default = "default_context_compression_enabled",
        rename = "contextCompressionEnabled"
    )]
    pub context_compression_enabled: bool,
    /// Estimated token budget for included messages; compression runs when heuristic exceeds this.
    #[serde(
        default = "default_context_budget_tokens",
        rename = "contextBudgetTokens",
        alias = "contextBudgetChars"
    )]
    pub context_budget_tokens: u32,
    /// Keep this many most recent user messages (and everything after the cutoff) verbatim.
    #[serde(
        default = "default_context_keep_recent_user_turns",
        rename = "contextKeepRecentUserTurns"
    )]
    pub context_keep_recent_user_turns: u32,
    /// Max tokens for the one-off summarization chat completion.
    #[serde(
        default = "default_context_summary_max_tokens",
        rename = "contextSummaryMaxTokens"
    )]
    pub context_summary_max_tokens: u32,
    /// Max tool-call rounds per assistant turn. Default 100.
    #[serde(default = "default_max_tool_rounds", rename = "maxToolRounds")]
    pub max_tool_rounds: u32,
    /// Max tool-call rounds **inside** each `run_sub_agent` run (separate from the lead conversation pool).
    #[serde(default = "default_max_tool_rounds", rename = "maxSubAgentToolRounds")]
    pub max_sub_agent_tool_rounds: u32,
    /// Max nesting depth for `run_subagent` (1 = lead only; 2 = one nested level).
    #[serde(
        default = "default_max_sub_agent_spawn_depth",
        rename = "maxSubAgentSpawnDepth"
    )]
    pub max_sub_agent_spawn_depth: u32,
    /// When true, chat UI shows the assistant “原始输出” inspector (code icon); includes wire text and API reasoning for debug, not inline in the bubble.
    #[serde(
        default = "default_raw_content_view_enabled",
        rename = "rawContentViewEnabled"
    )]
    pub raw_content_view_enabled: bool,
    /// When true, each LLM round writes request `messages` + params under app data `logs/llm_prompts/`.
    #[serde(
        default = "default_debug_dump_llm_prompts",
        rename = "debugDumpLlmPrompts"
    )]
    pub debug_dump_llm_prompts: bool,
    /// Session KEY→VALUE overlays for `terminal` child env (override process / `.env`).
    /// Applied regardless of `debug_menus_enabled`; edit UI lives under debug「界面配置」.
    #[serde(default, rename = "terminalEnvOverrides")]
    pub terminal_env_overrides: HashMap<String, String>,
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
    #[serde(
        default = "default_user_dynamic_inject_enabled",
        rename = "userDynamicInjectEnabled"
    )]
    pub user_dynamic_inject_enabled: bool,
    /// Per-agent default LLM: worker id or `"supervisor"` → explicit provider + model.
    #[serde(
        default,
        rename = "agentDefaultModels",
        deserialize_with = "deserialize_agent_default_models",
        serialize_with = "serialize_agent_default_models"
    )]
    pub agent_default_models: HashMap<String, AgentModelRef>,
    /// When true for a worker id, successful `task_board` updates hard-trim older history (no LLM).
    #[serde(default, rename = "agentTaskBoardHistoryTrim")]
    pub agent_task_board_history_trim: HashMap<String, bool>,
    /// When true, computer agent uses Bézier / jitter mouse paths by default (`human_like` preset).
    #[serde(default = "default_computer_human_like", rename = "computerHumanLike")]
    pub computer_human_like: bool,
    /// Starting vision tier for new computer conversations (`primary` | `intermediate` | `advanced`).
    #[serde(
        default = "default_computer_initial_tier",
        rename = "computerInitialTier"
    )]
    pub computer_initial_tier: String,
    /// When true, Computer Use assistant messages show the annotated screenshot preview action.
    #[serde(
        default = "default_computer_annotated_screen_view_enabled",
        rename = "computerAnnotatedScreenViewEnabled"
    )]
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
    #[serde(
        default = "default_captcha_slider_offset_px",
        rename = "captchaSliderOffsetPx"
    )]
    pub captcha_slider_offset_px: i32,
    /// When true, Composer shows the monitor picker for the computer agent.
    #[serde(
        default = "default_computer_show_monitor_picker",
        rename = "computerShowMonitorPicker"
    )]
    pub computer_show_monitor_picker: bool,
    /// When true, skip manual monitor picker (default primary) and follow app window monitor after launch_app.
    #[serde(
        default = "default_computer_auto_switch_monitor",
        rename = "computerAutoSwitchMonitor"
    )]
    pub computer_auto_switch_monitor: bool,
    #[serde(default = "default_memory_enabled", rename = "memoryEnabled")]
    pub memory_enabled: bool,
    #[serde(
        default = "default_user_profile_enabled",
        rename = "userProfileEnabled"
    )]
    pub user_profile_enabled: bool,
    /// Global coding preferences injected as `[USER RULES]` (see `user_rules` module).
    #[serde(default, rename = "userCodingRules")]
    pub user_coding_rules: String,
    #[serde(default = "default_memory_char_limit", rename = "memoryCharLimit")]
    pub memory_char_limit: u32,
    #[serde(default = "default_user_char_limit", rename = "userCharLimit")]
    pub user_char_limit: u32,
    #[serde(
        default = "default_memory_nudge_interval",
        rename = "memoryNudgeInterval"
    )]
    pub memory_nudge_interval: u32,
    #[serde(
        default = "default_background_review_enabled",
        rename = "backgroundReviewEnabled"
    )]
    pub background_review_enabled: bool,
    #[serde(
        default = "default_skill_creation_nudge_interval",
        rename = "skillCreationNudgeInterval"
    )]
    pub skill_creation_nudge_interval: u32,
    #[serde(default = "default_curator_enabled", rename = "curatorEnabled")]
    pub curator_enabled: bool,
    #[serde(default = "default_curator_idle_hours", rename = "curatorIdleHours")]
    pub curator_idle_hours: u32,
    #[serde(
        default = "default_curator_interval_days",
        rename = "curatorIntervalDays"
    )]
    pub curator_interval_days: u32,
    /// UI theme: `light`, `dark`, or `system`.
    #[serde(default = "default_theme", rename = "theme")]
    pub theme: String,
    /// Per-agent UI overrides keyed by agent id.
    #[serde(default, rename = "agentUiOverrides")]
    pub agent_ui_overrides: HashMap<String, crate::agents::AgentUiConfig>,
    /// Model id for DashScope web search tool calls (defaults to `qwen3-max` when empty).
    #[serde(
        default = "default_web_search_model_setting",
        rename = "webSearchModel"
    )]
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
    /// Debug: computer tier → LLM profile.
    #[serde(default = "default_computer_tier_llm", rename = "computerTierLlm")]
    pub computer_tier_llm: HashMap<String, ComputerTierLlmConfig>,
    /// Debug: per-phase LLM for computer host verify pipeline.
    #[serde(default, rename = "computerPipelineLlm")]
    pub computer_pipeline_llm: ComputerPipelineLlmSettings,
    #[serde(default, rename = "mediaOss")]
    pub media_oss: MediaOssConfig,
    /// Max concurrent tool invocations per batch (`None` → min(CPU cores, 8)).
    #[serde(
        default,
        rename = "maxParallelToolCalls",
        skip_serializing_if = "Option::is_none"
    )]
    pub max_parallel_tool_calls: Option<u32>,
    /// Max concurrent `run_subagent` invocations (`None` → min(CPU cores, 8)).
    #[serde(
        default,
        rename = "maxParallelSubAgents",
        skip_serializing_if = "Option::is_none"
    )]
    pub max_parallel_sub_agents: Option<u32>,
    /// Max concurrent media tool jobs (`None` → min(CPU cores, 8)).
    #[serde(
        default,
        rename = "maxParallelMediaJobs",
        skip_serializing_if = "Option::is_none"
    )]
    pub max_parallel_media_jobs: Option<u32>,
    /// Max concurrent dispatcher runs (chat, webhook, cron, …).
    #[serde(default = "default_max_concurrent_runs", rename = "maxConcurrentRuns")]
    pub max_concurrent_runs: u32,
    /// When false, all tool calls in one assistant turn run serially.
    #[serde(
        default = "default_parallel_tool_execution_enabled",
        rename = "parallelToolExecutionEnabled"
    )]
    pub parallel_tool_execution_enabled: bool,
    /// Per-request override (e.g. computer tier); not persisted.
    #[serde(skip)]
    pub round_enable_thinking: Option<bool>,
    #[serde(skip)]
    pub round_thinking_budget: Option<u32>,
    #[serde(skip)]
    pub round_reasoning_effort: Option<String>,
    #[serde(skip)]
    pub round_thinking_intensity: Option<String>,
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
    // 平台模型配置全部由平台下发；本地默认无激活服务商（build 配置可覆盖）。
    build_cfg_str!("ACTIVE_PROVIDER_ID", "")
}

fn default_model_name() -> String {
    build_cfg_str!("MODEL", "")
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

pub(crate) fn default_lead_agent_id() -> String {
    build_cfg_str!("LEAD_AGENT_ID", "general")
}

fn default_computer_initial_tier() -> String {
    build_cfg_str!("COMPUTER_INITIAL_TIER", "intermediate")
}

fn default_computer_human_like() -> bool {
    true
}

/// Clean up legacy settings loaded from disk:
/// 1. `session-provider`/`session-worker` placeholder entries left by the legacy
///    debug-session write path are removed so the UI falls back to real defaults;
/// 2. legacy user-layer platform providers (qwen/deepseek/doubao) and tier maps
///    pointing at them are removed — platform model config now comes entirely
///    from the platform directory on login.
/// Existing user choices are preserved; platform defaults are no longer backfilled
/// locally (platform model config comes from the platform directory on login).
pub fn ensure_user_settings_defaults(user: &mut UserSettings) {
    strip_session_placeholder_model_configs(user);
    strip_legacy_platform_config(user);
}

/// 清理旧版本地平台配置：移除用户层 qwen/deepseek/doubao 服务商及其档位映射。
/// 平台模型配置现全部由平台目录下发（登录后创建服务商并补齐档位默认），
/// 本地不再保留任何平台模型固定配置。
fn strip_legacy_platform_config(user: &mut UserSettings) {
    const LEGACY: [&str; 3] = ["qwen", "deepseek", "doubao"];
    user.providers.retain(|p| !LEGACY.contains(&p.id.as_str()));
    for (_, modes) in user.agent_mode_llm.iter_mut() {
        modes.retain(|_, cfg| !LEGACY.contains(&cfg.provider_id.as_str()));
    }
    for (_, modes) in user.media_mode_llm.iter_mut() {
        modes.retain(|_, cfg| !LEGACY.contains(&cfg.provider_id.as_str()));
    }
    user.computer_tier_llm
        .retain(|_, cfg| !LEGACY.contains(&cfg.provider_id.as_str()));
    user.agent_default_models
        .retain(|_, r| !LEGACY.contains(&r.provider_id.as_str()));
    let pipeline = &mut user.computer_pipeline_llm;
    for (model, pid) in [
        (&mut pipeline.decision, &mut pipeline.decision_provider_id),
        (&mut pipeline.position, &mut pipeline.position_provider_id),
        (&mut pipeline.verify, &mut pipeline.verify_provider_id),
    ] {
        if LEGACY.contains(&pid.as_str()) {
            *model = String::new();
            *pid = String::new();
        }
    }
    let overrides = &mut user.media_model_overrides;
    for slot in [
        &mut overrides.image_generation,
        &mut overrides.video_generation,
        &mut overrides.image,
        &mut overrides.audio,
        &mut overrides.video,
    ] {
        if let Some(r) = slot {
            if LEGACY.contains(&r.provider_id.as_str()) {
                *slot = None;
            }
        }
    }
}

/// Remove `session-provider` / `session-worker` entries from per-mode LLM maps.
/// These were written into user_settings.json by the legacy debug-session save
/// path; deleting them makes the UI fall back to the platform defaults.
fn strip_session_placeholder_model_configs(user: &mut UserSettings) {
    fn is_session_placeholder(cfg: &ComputerTierLlmConfig) -> bool {
        cfg.provider_id
            .trim()
            .eq_ignore_ascii_case("session-provider")
            || cfg.model.trim().eq_ignore_ascii_case("session-worker")
    }
    for (_, modes) in user.agent_mode_llm.iter_mut() {
        modes.retain(|_, cfg| !is_session_placeholder(cfg));
    }
    for (_, modes) in user.media_mode_llm.iter_mut() {
        modes.retain(|_, cfg| !is_session_placeholder(cfg));
    }
    user.computer_tier_llm
        .retain(|_, cfg| !is_session_placeholder(cfg));

    let pipeline = &mut user.computer_pipeline_llm;
    if pipeline
        .decision
        .trim()
        .eq_ignore_ascii_case("session-worker")
        || pipeline
            .decision_provider_id
            .trim()
            .eq_ignore_ascii_case("session-provider")
    {
        pipeline.decision = default_pipeline_model_decision();
        pipeline.decision_provider_id = default_computer_llm_provider();
    }
    if pipeline
        .position
        .trim()
        .eq_ignore_ascii_case("session-worker")
        || pipeline
            .position_provider_id
            .trim()
            .eq_ignore_ascii_case("session-provider")
    {
        pipeline.position = default_pipeline_model_position();
        pipeline.position_provider_id = default_computer_llm_provider();
    }
    if pipeline
        .verify
        .trim()
        .eq_ignore_ascii_case("session-worker")
        || pipeline
            .verify_provider_id
            .trim()
            .eq_ignore_ascii_case("session-provider")
    {
        pipeline.verify = default_pipeline_model_verify();
        pipeline.verify_provider_id = default_computer_llm_provider();
    }
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

pub(crate) fn default_agent_mode() -> String {
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

fn default_max_sub_agent_spawn_depth() -> u32 {
    build_cfg_u32!("MAX_SUB_AGENT_SPAWN_DEPTH", 2)
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

fn default_computer_auto_switch_monitor() -> bool {
    true
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

fn default_parallel_tool_execution_enabled() -> bool {
    true
}

fn default_max_concurrent_runs() -> u32 {
    4
}

impl Default for ModelSettings {
    fn default() -> Self {
        Self {
            providers: Vec::new(),
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
            max_sub_agent_spawn_depth: default_max_sub_agent_spawn_depth(),
            raw_content_view_enabled: default_raw_content_view_enabled(),
            debug_dump_llm_prompts: default_debug_dump_llm_prompts(),
            terminal_env_overrides: HashMap::new(),
            debug_menus_enabled: default_debug_menus_enabled(),
            task_board_show_child_boards: default_task_board_show_child_boards(),
            user_dynamic_inject_enabled: default_user_dynamic_inject_enabled(),
            agent_default_models: HashMap::new(),
            agent_task_board_history_trim: HashMap::new(),
            computer_human_like: true,
            computer_initial_tier: default_computer_initial_tier(),
            computer_annotated_screen_view_enabled: default_computer_annotated_screen_view_enabled(
            ),
            dati_api_url: default_dati_api_url(),
            dati_authcode: default_dati_authcode(),
            dati_typeno: default_dati_typeno(),
            dati_author: default_dati_author(),
            captcha_slider_offset_px: default_captcha_slider_offset_px(),
            computer_show_monitor_picker: default_computer_show_monitor_picker(),
            computer_auto_switch_monitor: default_computer_auto_switch_monitor(),
            memory_enabled: default_memory_enabled(),
            user_profile_enabled: default_user_profile_enabled(),
            user_coding_rules: String::new(),
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
            computer_tier_llm: default_computer_tier_llm(),
            computer_pipeline_llm: ComputerPipelineLlmSettings::default(),
            agent_mode_llm: default_agent_mode_llm(),
            media_mode_llm: default_media_mode_llm(),
            media_oss: MediaOssConfig::default(),
            max_parallel_tool_calls: None,
            max_parallel_sub_agents: None,
            max_parallel_media_jobs: None,
            max_concurrent_runs: default_max_concurrent_runs(),
            parallel_tool_execution_enabled: true,
            round_enable_thinking: None,
            round_thinking_budget: None,
            round_reasoning_effort: None,
            round_thinking_intensity: None,
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

fn default_play_sound_on_finish() -> bool {
    true
}

/// Default: collapse only the last completed turn; intermediate process stays visible.
fn default_collapse_process_by_default() -> bool {
    false
}

fn default_media_oss_key_prefix() -> String {
    "pointer-media-attachments/".to_string()
}

fn default_media_oss_presign_expires_sec() -> u32 {
    604_800 // 7 days (Aliyun OSS V4 presigned URL max)
}

fn default_media_oss_delete_after_use() -> bool {
    true
}

/// Aliyun OSS settings for large `media_understand` video uploads (HTTP `video_url` to DashScope).
/// API: PutObject + V4 presigned GET — see Aliyun OSS developer reference.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaOssConfig {
    #[serde(default, rename = "enabled")]
    pub enabled: bool,
    #[serde(default, rename = "bucket")]
    pub bucket: String,
    /// Region id, e.g. `cn-hangzhou` (not `oss-cn-hangzhou`).
    #[serde(default, rename = "region")]
    pub region: String,
    /// Optional custom endpoint, e.g. `https://oss-cn-hangzhou.aliyuncs.com`.
    #[serde(default, rename = "endpoint")]
    pub endpoint: String,
    #[serde(default, rename = "accessKeyId")]
    pub access_key_id: String,
    #[serde(default, rename = "accessKeySecret")]
    pub access_key_secret: String,
    #[serde(default = "default_media_oss_key_prefix", rename = "keyPrefix")]
    pub key_prefix: String,
    #[serde(
        default = "default_media_oss_presign_expires_sec",
        rename = "presignExpiresSec"
    )]
    pub presign_expires_sec: u32,
    #[serde(
        default = "default_media_oss_delete_after_use",
        rename = "deleteAfterUse"
    )]
    pub delete_after_use: bool,
}

impl Default for MediaOssConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bucket: String::new(),
            region: String::new(),
            endpoint: String::new(),
            access_key_id: String::new(),
            access_key_secret: String::new(),
            key_prefix: default_media_oss_key_prefix(),
            presign_expires_sec: default_media_oss_presign_expires_sec(),
            delete_after_use: default_media_oss_delete_after_use(),
        }
    }
}

impl MediaOssConfig {
    /// Whether OSS upload is enabled and minimally configured (credentials may come from env).
    pub fn wants_upload(&self) -> bool {
        self.enabled && !self.bucket.trim().is_empty() && !self.region.trim().is_empty()
    }
}

/// Persisted user preferences (theme, optional UI cache).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserSettings {
    #[serde(default = "default_theme", rename = "theme")]
    pub theme: String,
    #[serde(default, rename = "userNickname")]
    pub user_nickname: Option<String>,
    /// Legacy globally enabled skill ids. Not used for runtime resolve; kept for
    /// migration into `agent_skill_overrides["general"]` on client startup.
    #[serde(default = "default_enabled_skill_ids", rename = "enabledSkillIds")]
    pub enabled_skill_ids: Vec<String>,
    /// Per-agent enabled skill ids. Runtime resolve: override ?? agent defaultSkillIds.
    #[serde(default, rename = "agentSkillOverrides")]
    pub agent_skill_overrides: HashMap<String, Vec<String>>,
    #[serde(default = "default_memory_enabled", rename = "memoryEnabled")]
    pub memory_enabled: bool,
    #[serde(
        default = "default_user_profile_enabled",
        rename = "userProfileEnabled"
    )]
    pub user_profile_enabled: bool,
    /// Global coding preferences injected as `[USER RULES]` (see `user_rules` module).
    #[serde(default, rename = "userCodingRules")]
    pub user_coding_rules: String,
    #[serde(default = "default_memory_char_limit", rename = "memoryCharLimit")]
    pub memory_char_limit: u32,
    #[serde(default = "default_user_char_limit", rename = "userCharLimit")]
    pub user_char_limit: u32,
    #[serde(
        default = "default_memory_nudge_interval",
        rename = "memoryNudgeInterval"
    )]
    pub memory_nudge_interval: u32,
    #[serde(
        default = "default_background_review_enabled",
        rename = "backgroundReviewEnabled"
    )]
    pub background_review_enabled: bool,
    #[serde(
        default = "default_skill_creation_nudge_interval",
        rename = "skillCreationNudgeInterval"
    )]
    pub skill_creation_nudge_interval: u32,
    #[serde(default = "default_curator_enabled", rename = "curatorEnabled")]
    pub curator_enabled: bool,
    #[serde(default = "default_curator_idle_hours", rename = "curatorIdleHours")]
    pub curator_idle_hours: u32,
    #[serde(
        default = "default_curator_interval_days",
        rename = "curatorIntervalDays"
    )]
    pub curator_interval_days: u32,
    #[serde(
        default = "default_computer_auto_compact",
        rename = "computerAutoCompact"
    )]
    pub computer_auto_compact: bool,
    /// Play a short chime when a chat turn finishes (UI preference).
    #[serde(default = "default_play_sound_on_finish", rename = "playSoundOnFinish")]
    pub play_sound_on_finish: bool,
    /// Collapse intermediate process entries by default; only show final output
    /// for completed turns (UI preference).
    #[serde(
        default = "default_collapse_process_by_default",
        rename = "collapseProcessByDefault"
    )]
    pub collapse_process_by_default: bool,
    #[serde(default, rename = "mediaOss")]
    pub media_oss: MediaOssConfig,
    // --- Model-service config (user-configurable; persisted in user_settings.json) ---
    #[serde(default = "default_providers_empty", rename = "providers")]
    pub providers: Vec<ProviderConfig>,
    #[serde(default = "default_active_provider_id", rename = "activeProviderId")]
    pub active_provider_id: String,
    #[serde(default = "default_model_name", rename = "model")]
    pub model: String,
    #[serde(default = "default_model_temperature", rename = "temperature")]
    pub temperature: f32,
    #[serde(default = "default_model_max_tokens", rename = "maxTokens")]
    pub max_tokens: u32,
    #[serde(default = "default_tool_approval_mode", rename = "toolApprovalMode")]
    pub tool_approval_mode: String,
    #[serde(default = "default_agent_mode", rename = "agentMode")]
    pub agent_mode: String,
    #[serde(default, rename = "workspaceRoot")]
    pub workspace_root: String,
    #[serde(default = "default_lead_agent_id", rename = "leadAgentId")]
    pub lead_agent_id: String,
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
    #[serde(
        default = "platform_default_max_tool_rounds",
        rename = "maxSubAgentToolRounds"
    )]
    pub max_sub_agent_tool_rounds: u32,
    #[serde(
        default = "platform_default_max_sub_agent_spawn_depth",
        rename = "maxSubAgentSpawnDepth"
    )]
    pub max_sub_agent_spawn_depth: u32,
    #[serde(
        default = "platform_default_raw_content_view_enabled",
        rename = "rawContentViewEnabled"
    )]
    pub raw_content_view_enabled: bool,
    #[serde(
        default = "default_debug_dump_llm_prompts",
        rename = "debugDumpLlmPrompts"
    )]
    pub debug_dump_llm_prompts: bool,
    #[serde(default, rename = "terminalEnvOverrides")]
    pub terminal_env_overrides: HashMap<String, String>,
    #[serde(default = "default_debug_menus_enabled", rename = "debugMenusEnabled")]
    pub debug_menus_enabled: bool,
    #[serde(
        default = "default_task_board_show_child_boards",
        rename = "taskBoardShowChildBoards"
    )]
    pub task_board_show_child_boards: bool,
    #[serde(
        default = "default_user_dynamic_inject_enabled",
        rename = "userDynamicInjectEnabled"
    )]
    pub user_dynamic_inject_enabled: bool,
    #[serde(
        default,
        rename = "agentDefaultModels",
        deserialize_with = "deserialize_agent_default_models",
        serialize_with = "serialize_agent_default_models"
    )]
    pub agent_default_models: HashMap<String, AgentModelRef>,
    #[serde(default, rename = "agentTaskBoardHistoryTrim")]
    pub agent_task_board_history_trim: HashMap<String, bool>,
    #[serde(default = "default_computer_human_like", rename = "computerHumanLike")]
    pub computer_human_like: bool,
    #[serde(
        default = "default_computer_initial_tier",
        rename = "computerInitialTier"
    )]
    pub computer_initial_tier: String,
    #[serde(
        default = "default_computer_annotated_screen_view_enabled",
        rename = "computerAnnotatedScreenViewEnabled"
    )]
    pub computer_annotated_screen_view_enabled: bool,
    #[serde(
        default = "default_captcha_slider_offset_px",
        rename = "captchaSliderOffsetPx"
    )]
    pub captcha_slider_offset_px: i32,
    #[serde(
        default = "default_computer_show_monitor_picker",
        rename = "computerShowMonitorPicker"
    )]
    pub computer_show_monitor_picker: bool,
    #[serde(
        default = "default_computer_auto_switch_monitor",
        rename = "computerAutoSwitchMonitor"
    )]
    pub computer_auto_switch_monitor: bool,
    #[serde(default, rename = "agentUiOverrides")]
    pub agent_ui_overrides: HashMap<String, crate::agents::AgentUiConfig>,
    #[serde(
        default = "default_web_search_model_setting",
        rename = "webSearchModel"
    )]
    pub web_search_model: String,
    #[serde(default, rename = "mediaModelOverrides")]
    pub media_model_overrides: MediaModelOverrides,
    #[serde(default, rename = "agentPerformanceModes")]
    pub agent_performance_modes: HashMap<String, String>,
    #[serde(default, rename = "mediaUnderstandingModes")]
    pub media_understanding_modes: MediaUnderstandingModes,
    #[serde(default = "default_computer_tier_llm", rename = "computerTierLlm")]
    pub computer_tier_llm: HashMap<String, ComputerTierLlmConfig>,
    #[serde(default, rename = "computerPipelineLlm")]
    pub computer_pipeline_llm: ComputerPipelineLlmSettings,
    #[serde(default = "default_agent_mode_llm", rename = "agentModeLlm")]
    pub agent_mode_llm: HashMap<String, HashMap<String, ComputerTierLlmConfig>>,
    #[serde(default = "default_media_mode_llm", rename = "mediaModeLlm")]
    pub media_mode_llm: HashMap<String, HashMap<String, ComputerTierLlmConfig>>,
    #[serde(
        default,
        rename = "maxParallelToolCalls",
        skip_serializing_if = "Option::is_none"
    )]
    pub max_parallel_tool_calls: Option<u32>,
    #[serde(
        default,
        rename = "maxParallelSubAgents",
        skip_serializing_if = "Option::is_none"
    )]
    pub max_parallel_sub_agents: Option<u32>,
    #[serde(
        default,
        rename = "maxParallelMediaJobs",
        skip_serializing_if = "Option::is_none"
    )]
    pub max_parallel_media_jobs: Option<u32>,
    #[serde(default = "default_max_concurrent_runs", rename = "maxConcurrentRuns")]
    pub max_concurrent_runs: u32,
    #[serde(
        default = "default_parallel_tool_execution_enabled",
        rename = "parallelToolExecutionEnabled"
    )]
    pub parallel_tool_execution_enabled: bool,
}

impl Default for UserSettings {
    fn default() -> Self {
        Self {
            theme: default_theme(),
            user_nickname: None,
            enabled_skill_ids: default_enabled_skill_ids(),
            agent_skill_overrides: HashMap::new(),
            memory_enabled: default_memory_enabled(),
            user_profile_enabled: default_user_profile_enabled(),
            user_coding_rules: String::new(),
            memory_char_limit: default_memory_char_limit(),
            user_char_limit: default_user_char_limit(),
            memory_nudge_interval: default_memory_nudge_interval(),
            background_review_enabled: default_background_review_enabled(),
            skill_creation_nudge_interval: default_skill_creation_nudge_interval(),
            curator_enabled: default_curator_enabled(),
            curator_idle_hours: default_curator_idle_hours(),
            curator_interval_days: default_curator_interval_days(),
            computer_auto_compact: default_computer_auto_compact(),
            play_sound_on_finish: default_play_sound_on_finish(),
            collapse_process_by_default: default_collapse_process_by_default(),
            media_oss: MediaOssConfig::default(),
            providers: default_providers_empty(),
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
            max_sub_agent_spawn_depth: platform_default_max_sub_agent_spawn_depth(),
            raw_content_view_enabled: platform_default_raw_content_view_enabled(),
            debug_dump_llm_prompts: default_debug_dump_llm_prompts(),
            terminal_env_overrides: HashMap::new(),
            debug_menus_enabled: default_debug_menus_enabled(),
            task_board_show_child_boards: default_task_board_show_child_boards(),
            user_dynamic_inject_enabled: default_user_dynamic_inject_enabled(),
            agent_default_models: default_platform_agent_models(),
            agent_task_board_history_trim: HashMap::new(),
            computer_human_like: true,
            computer_initial_tier: default_computer_initial_tier(),
            computer_annotated_screen_view_enabled: default_computer_annotated_screen_view_enabled(
            ),
            captcha_slider_offset_px: default_captcha_slider_offset_px(),
            computer_show_monitor_picker: default_computer_show_monitor_picker(),
            computer_auto_switch_monitor: default_computer_auto_switch_monitor(),
            agent_ui_overrides: HashMap::new(),
            web_search_model: default_web_search_model_setting(),
            media_model_overrides: default_media_generation_overrides(),
            agent_performance_modes: HashMap::new(),
            media_understanding_modes: MediaUnderstandingModes::default(),
            computer_tier_llm: default_computer_tier_llm(),
            computer_pipeline_llm: ComputerPipelineLlmSettings::default(),
            agent_mode_llm: default_agent_mode_llm(),
            media_mode_llm: default_media_mode_llm(),
            max_parallel_tool_calls: None,
            max_parallel_sub_agents: None,
            max_parallel_media_jobs: None,
            max_concurrent_runs: default_max_concurrent_runs(),
            parallel_tool_execution_enabled: true,
        }
    }
}

/// Per-tier LLM overrides for Computer Use Agent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComputerTierLlmConfig {
    #[serde(rename = "providerId")]
    pub provider_id: String,
    pub model: String,
    #[serde(default = "default_tier_enable_thinking", rename = "enableThinking")]
    pub enable_thinking: bool,
    #[serde(default, rename = "thinkingBudget")]
    pub thinking_budget: Option<u32>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "reasoningEffort"
    )]
    pub reasoning_effort: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "thinkingIntensity"
    )]
    pub thinking_intensity: Option<String>,
}

fn default_tier_enable_thinking() -> bool {
    true
}

/// Per-phase model ids and thinking budgets for host verify pipeline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComputerPipelineLlmSettings {
    #[serde(default = "default_pipeline_model_decision")]
    pub decision: String,
    #[serde(default = "default_pipeline_model_position")]
    pub position: String,
    #[serde(default = "default_pipeline_model_verify")]
    pub verify: String,
    #[serde(
        default = "default_computer_llm_provider",
        rename = "decisionProviderId"
    )]
    pub decision_provider_id: String,
    #[serde(
        default = "default_computer_llm_provider",
        rename = "positionProviderId"
    )]
    pub position_provider_id: String,
    #[serde(default = "default_computer_llm_provider", rename = "verifyProviderId")]
    pub verify_provider_id: String,
    #[serde(
        default = "default_pipeline_thinking_budget_position",
        rename = "positionThinkingBudget"
    )]
    pub position_thinking_budget: u32,
    #[serde(
        default = "default_pipeline_thinking_budget_verify",
        rename = "verifyThinkingBudget"
    )]
    pub verify_thinking_budget: u32,
}

impl Default for ComputerPipelineLlmSettings {
    fn default() -> Self {
        Self {
            decision: default_pipeline_model_decision(),
            position: default_pipeline_model_position(),
            verify: default_pipeline_model_verify(),
            decision_provider_id: default_computer_llm_provider(),
            position_provider_id: default_computer_llm_provider(),
            verify_provider_id: default_computer_llm_provider(),
            position_thinking_budget: default_pipeline_thinking_budget_position(),
            verify_thinking_budget: default_pipeline_thinking_budget_verify(),
        }
    }
}

fn default_pipeline_model_decision() -> String {
    String::new()
}

fn default_pipeline_model_position() -> String {
    String::new()
}

fn default_pipeline_model_verify() -> String {
    String::new()
}

fn default_computer_llm_provider() -> String {
    String::new()
}

fn default_pipeline_thinking_budget_position() -> u32 {
    DEFAULT_PIPELINE_POSITION_THINKING_BUDGET
}

fn default_pipeline_thinking_budget_verify() -> u32 {
    DEFAULT_PIPELINE_VERIFY_THINKING_BUDGET
}

/// In-memory platform configuration. Only session-scoped / sensitive fields
/// live here (never persisted): runtime provider list (with injected keys),
/// OAuth media OSS credentials, and server-side DaTi CAPTCHA config.
/// All user-editable preferences (incl. debug) live in [`UserSettings`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformSettings {
    pub providers: Vec<ProviderConfig>,
    /// Wire-compat name list from official APIs (`modelCatalog`). This client
    /// does not use it to build providers; `platformProviders` is the directory.
    #[serde(
        default,
        rename = "modelCatalog",
        skip_serializing_if = "HashMap::is_empty"
    )]
    pub model_catalog: HashMap<String, Vec<String>>,
    /// 平台下发的场景档位默认（agent/media/computer 快速/标准/高级默认模型映射）；
    /// merge 时补齐用户缺失档位，不持久化到用户设置。
    #[serde(default, rename = "tierDefaults", alias = "tier_defaults")]
    pub tier_defaults: serde_json::Value,
    #[serde(default, rename = "mediaOss")]
    pub media_oss: MediaOssConfig,
    #[serde(default = "default_dati_api_url", rename = "datiApiUrl")]
    pub dati_api_url: String,
    #[serde(default = "default_dati_authcode", rename = "datiAuthcode")]
    pub dati_authcode: String,
    #[serde(default = "default_dati_typeno", rename = "datiTypeno")]
    pub dati_typeno: String,
    #[serde(default = "default_dati_author", rename = "datiAuthor")]
    pub dati_author: String,
}

/// Process-local debug model configuration. This DTO must never be persisted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebugSessionSettings {
    pub providers: Vec<ProviderConfig>,
    #[serde(rename = "activeProviderId")]
    pub active_provider_id: String,
    pub model: String,
    pub temperature: f32,
    #[serde(rename = "maxTokens")]
    pub max_tokens: u32,
    #[serde(rename = "computerTierLlm")]
    pub computer_tier_llm: HashMap<String, ComputerTierLlmConfig>,
    #[serde(rename = "computerPipelineLlm")]
    pub computer_pipeline_llm: ComputerPipelineLlmSettings,
    #[serde(rename = "agentModeLlm")]
    pub agent_mode_llm: HashMap<String, HashMap<String, ComputerTierLlmConfig>>,
    #[serde(rename = "mediaModeLlm")]
    pub media_mode_llm: HashMap<String, HashMap<String, ComputerTierLlmConfig>>,
}

impl From<&ModelSettings> for DebugSessionSettings {
    fn from(merged: &ModelSettings) -> Self {
        Self {
            providers: merged.providers.clone(),
            active_provider_id: merged.active_provider_id.clone(),
            model: merged.model.clone(),
            temperature: merged.temperature,
            max_tokens: merged.max_tokens,
            computer_tier_llm: merged.computer_tier_llm.clone(),
            computer_pipeline_llm: merged.computer_pipeline_llm.clone(),
            agent_mode_llm: merged.agent_mode_llm.clone(),
            media_mode_llm: merged.media_mode_llm.clone(),
        }
    }
}

/// Redact provider credentials while retaining all process-local debug fields.
///
/// The dedicated WEB endpoint returns this DTO instead of
/// [`WebEffectiveSettingsView`], whose serializer intentionally strips debug
/// fields. A masked key is accepted by the update boundary and resolved back
/// to the current in-memory credential.
pub fn redact_debug_session_settings_for_web(settings: &mut DebugSessionSettings) {
    for provider in &mut settings.providers {
        provider.api_key = if provider.api_key.trim().is_empty() {
            String::new()
        } else {
            "****".into()
        };
    }
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

/// 测试用：带 qwen + deepseek 服务商的运行时设置（本地默认不再内置平台服务商）。
#[cfg(test)]
pub(crate) fn sample_settings() -> ModelSettings {
    let mut s = ModelSettings::default();
    s.active_provider_id = "qwen".into();
    s.model = "qwen3.5-plus".into();
    s.providers.push(ProviderConfig {
        id: "qwen".into(),
        name: "千问".into(),
        base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1".into(),
        api_key: String::new(),
        models: vec!["qwen3.5-plus".into(), "qwen3.5-flash".into()],
        reasoning_in_messages: Some(false),
        temperature: None,
        max_tokens: None,
        model_configs: HashMap::new(),
        enable_thinking: Some(true),
        thinking_budget: Some(2048),
        reasoning_effort: None,
        thinking_protocol: None,
        thinking_intensity: None,
        extra_body: None,
        source: Some("platform".into()),
    });
    s.providers.push(ProviderConfig {
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
        thinking_protocol: None,
        thinking_intensity: None,
        extra_body: None,
        source: Some("platform".into()),
    });
    s
}

fn default_computer_tier_llm() -> HashMap<String, ComputerTierLlmConfig> {
    // 档位默认由平台目录下发；本地不再内置 computer tier 默认模型。
    HashMap::new()
}

fn default_agent_mode_llm() -> HashMap<String, HashMap<String, ComputerTierLlmConfig>> {
    HashMap::new()
}

fn default_media_mode_llm() -> HashMap<String, HashMap<String, ComputerTierLlmConfig>> {
    HashMap::new()
}

fn default_platform_agent_models() -> HashMap<String, AgentModelRef> {
    HashMap::new()
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
    2048
}

fn platform_default_max_tool_rounds() -> u32 {
    200
}

fn platform_default_max_sub_agent_spawn_depth() -> u32 {
    2
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

fn default_providers_empty() -> Vec<ProviderConfig> {
    // 平台模型配置全部由平台下发；本地默认无任何平台服务商。
    Vec::new()
}

impl Default for PlatformSettings {
    fn default() -> Self {
        Self {
            providers: default_providers_empty(),
            model_catalog: HashMap::new(),
            tier_defaults: serde_json::Value::Null,
            dati_api_url: default_dati_api_url(),
            dati_authcode: default_dati_authcode(),
            dati_typeno: default_dati_typeno(),
            dati_author: default_dati_author(),
            media_oss: MediaOssConfig::default(),
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

const DATI_SETTINGS_JSON_KEYS: &[&str] =
    &["datiApiUrl", "datiAuthcode", "datiTypeno", "datiAuthor"];

/// Debug-only settings (visible when debug menus are enabled).
/// Omitted from pointer-server Web API responses for **non-admin** users so
/// casual clients do not round-trip session-only debug state. Platform admins
/// (including standalone local admin) receive these fields so settings UI save
/// → reopen keeps providers / mode LLM maps / debug toggles.
const DEBUG_WEB_SETTINGS_JSON_KEYS: &[&str] = &[
    "rawContentViewEnabled",
    "debugDumpLlmPrompts",
    "terminalEnvOverrides",
    "debugMenusEnabled",
    "taskBoardShowChildBoards",
    "computerAnnotatedScreenViewEnabled",
    "agentUiOverrides",
    "computerTierLlm",
    "computerPipelineLlm",
    "agentModeLlm",
    "mediaModeLlm",
    "agentTaskBoardHistoryTrim",
    "maxSubAgentToolRounds",
    "maxSubAgentSpawnDepth",
];

/// Remove DaTi CAPTCHA fields from a settings JSON object (`platform` / `merged` slices).
pub fn strip_dati_keys_from_settings_json(value: &mut serde_json::Value) {
    let serde_json::Value::Object(obj) = value else {
        return;
    };
    for key in DATI_SETTINGS_JSON_KEYS {
        obj.remove(*key);
    }
    if let Some(platform) = obj.get_mut("platform") {
        strip_dati_keys_from_settings_object(platform);
    }
    if let Some(merged) = obj.get_mut("merged") {
        strip_dati_keys_from_settings_object(merged);
    }
}

fn strip_dati_keys_from_settings_object(value: &mut serde_json::Value) {
    let serde_json::Value::Object(obj) = value else {
        return;
    };
    for key in DATI_SETTINGS_JSON_KEYS {
        obj.remove(*key);
    }
}

fn strip_debug_keys_from_settings_object(value: &mut serde_json::Value) {
    let serde_json::Value::Object(obj) = value else {
        return;
    };
    for key in DEBUG_WEB_SETTINGS_JSON_KEYS {
        obj.remove(*key);
    }
}

fn redact_provider_api_keys_in_array(providers: &mut serde_json::Value) {
    let serde_json::Value::Array(items) = providers else {
        return;
    };
    for item in items {
        let Some(obj) = item.as_object_mut() else {
            continue;
        };
        let masked = obj
            .get("apiKey")
            .and_then(|v| v.as_str())
            .is_some_and(|k| !k.is_empty());
        obj.insert(
            "apiKey".into(),
            serde_json::Value::String(if masked { "****".into() } else { String::new() }),
        );
    }
}

fn redact_media_oss_secrets(oss: &mut serde_json::Value) {
    let Some(obj) = oss.as_object_mut() else {
        return;
    };
    obj.remove("accessKeySecret");
}

fn redact_settings_object_secrets(
    obj: &mut serde_json::Map<String, serde_json::Value>,
    strip_debug: bool,
) {
    // Debug / mode-LLM fields now live in the user slice; non-admins must not
    // see them there either (admins keep them for save→reopen round-trips).
    if let Some(user) = obj.get_mut("user") {
        if strip_debug {
            strip_debug_keys_from_settings_object(user);
        }
        if let Some(user_obj) = user.as_object_mut() {
            if let Some(providers) = user_obj.get_mut("providers") {
                redact_provider_api_keys_in_array(providers);
            }
            if let Some(oss) = user_obj.get_mut("mediaOss") {
                redact_media_oss_secrets(oss);
            }
        }
    }
    if let Some(platform) = obj.get_mut("platform") {
        strip_dati_keys_from_settings_object(platform);
        if strip_debug {
            strip_debug_keys_from_settings_object(platform);
        }
        if let Some(platform_obj) = platform.as_object_mut() {
            if let Some(providers) = platform_obj.get_mut("providers") {
                redact_provider_api_keys_in_array(providers);
            }
            if let Some(oss) = platform_obj.get_mut("mediaOss") {
                redact_media_oss_secrets(oss);
            }
        }
    }
    if let Some(merged) = obj.get_mut("merged") {
        strip_dati_keys_from_settings_object(merged);
        if strip_debug {
            strip_debug_keys_from_settings_object(merged);
        }
        if let Some(merged_obj) = merged.as_object_mut() {
            merged_obj.insert("apiKey".into(), serde_json::Value::String(String::new()));
            if let Some(providers) = merged_obj.get_mut("providers") {
                redact_provider_api_keys_in_array(providers);
            }
            if let Some(oss) = merged_obj.get_mut("mediaOss") {
                redact_media_oss_secrets(oss);
            }
        }
    }
}

/// Web PUT from non-admin clients must not overwrite server debug toggles with
/// Web PUT from non-admin clients must not overwrite server debug toggles with
/// serde defaults (those fields are omitted on non-admin GET). Admins receive
/// and may update the fields; callers should skip this preserve for admins.
/// Debug fields now live in user settings, so the server values come from `user`.
pub fn preserve_platform_debug_settings_in_model(
    incoming: &mut ModelSettings,
    user: &UserSettings,
) {
    incoming.raw_content_view_enabled = user.raw_content_view_enabled;
    incoming.debug_dump_llm_prompts = user.debug_dump_llm_prompts;
    incoming.terminal_env_overrides = user.terminal_env_overrides.clone();
    incoming.debug_menus_enabled = user.debug_menus_enabled;
    incoming.task_board_show_child_boards = user.task_board_show_child_boards;
    incoming.computer_annotated_screen_view_enabled = user.computer_annotated_screen_view_enabled;
    incoming.agent_ui_overrides = user.agent_ui_overrides.clone();
    incoming.agent_task_board_history_trim = user.agent_task_board_history_trim.clone();
    incoming.max_sub_agent_tool_rounds = user.max_sub_agent_tool_rounds;
    incoming.max_sub_agent_spawn_depth = user.max_sub_agent_spawn_depth;
    // Mode LLM maps are also omitted for non-admins; keep server values.
    incoming.agent_mode_llm = user.agent_mode_llm.clone();
    incoming.media_mode_llm = user.media_mode_llm.clone();
    incoming.computer_tier_llm = user.computer_tier_llm.clone();
    incoming.computer_pipeline_llm = user.computer_pipeline_llm.clone();
}

/// UserSettings 版脱敏保护：WEB 非 admin GET 时 user 切片的调试字段被剥掉
/// （DEBUG_WEB_SETTINGS_JSON_KEYS），PUT 回来 serde 默认值会清掉服务端调试配置；
/// 保存前用现有 user 值强改回。admin 走 round-trip 可正常更新，调用方应跳过。
pub fn preserve_platform_debug_settings_in_user(
    incoming: &mut UserSettings,
    existing: &UserSettings,
) {
    incoming.raw_content_view_enabled = existing.raw_content_view_enabled;
    incoming.debug_dump_llm_prompts = existing.debug_dump_llm_prompts;
    incoming.terminal_env_overrides = existing.terminal_env_overrides.clone();
    incoming.debug_menus_enabled = existing.debug_menus_enabled;
    incoming.task_board_show_child_boards = existing.task_board_show_child_boards;
    incoming.computer_annotated_screen_view_enabled =
        existing.computer_annotated_screen_view_enabled;
    incoming.agent_ui_overrides = existing.agent_ui_overrides.clone();
    incoming.agent_task_board_history_trim = existing.agent_task_board_history_trim.clone();
    incoming.max_sub_agent_tool_rounds = existing.max_sub_agent_tool_rounds;
    incoming.max_sub_agent_spawn_depth = existing.max_sub_agent_spawn_depth;
    // Mode LLM maps are also omitted for non-admins; keep server values.
    incoming.agent_mode_llm = existing.agent_mode_llm.clone();
    incoming.media_mode_llm = existing.media_mode_llm.clone();
    incoming.computer_tier_llm = existing.computer_tier_llm.clone();
    incoming.computer_pipeline_llm = existing.computer_pipeline_llm.clone();
}

/// Strip DaTi fields and redact secrets for pointer-server Web API responses.
/// When `strip_debug` is true (non-admin), also omit session-only debug fields.
pub fn redact_settings_json_for_web_api(value: &mut serde_json::Value, strip_debug: bool) {
    let serde_json::Value::Object(obj) = value else {
        return;
    };
    redact_settings_object_secrets(obj, strip_debug);
}

/// Web API response wrapper: omits DaTi fields (server-side only; desktop Tauri unchanged).
/// Platform admins keep debug / mode-LLM fields so settings save → reopen round-trips.
pub struct WebEffectiveSettingsView(pub EffectiveSettingsView);

impl serde::Serialize for WebEffectiveSettingsView {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut value = serde_json::to_value(&self.0).map_err(serde::ser::Error::custom)?;
        let strip_debug = !self.0.is_platform_admin;
        redact_settings_json_for_web_api(&mut value, strip_debug);
        value.serialize(serializer)
    }
}

/// Merge persisted user settings with in-memory platform config.
pub fn merge_user_platform(user: &UserSettings, platform: &PlatformSettings) -> ModelSettings {
    // Providers are user-owned (persisted in user_settings.json). Runtime keys
    // (OAuth / server.toml injection) live in platform.providers and are overlaid
    // by provider id so user edits never wipe injected credentials.
    let mut providers = user.providers.clone();    let platform_keys: HashMap<String, String> = platform
        .providers
        .iter()
        .map(|p| (p.id.clone(), p.api_key.clone()))
        .collect();
    for provider in &mut providers {
        provider.source = Some("user".into());
        if provider.api_key.trim().is_empty() {
            if let Some(key) = platform_keys.get(&provider.id) {
                provider.api_key = key.clone();
            }
        }
    }
    // Platform-only providers (e.g. custom providers from `pointer-server.toml`
    // `[llm]` in standalone deployments) are appended to the merged list so the
    // UI and runtime can use them even before the user saves a copy. User-owned
    // providers keep precedence; duplicates by id never appear.
    let known: std::collections::HashSet<String> = providers.iter().map(|p| p.id.clone()).collect();
    for platform_provider in &platform.providers {
        if !known.contains(&platform_provider.id) {
            let mut p = platform_provider.clone();
            p.source = Some("platform".into());
            providers.push(p);
        }
    }
    let mut settings = ModelSettings {
        providers,
        active_provider_id: user.active_provider_id.clone(),
        model: user.model.clone(),
        api_key: String::new(),
        temperature: user.temperature,
        max_tokens: user.max_tokens,
        has_key: platform.providers.iter().any(|p| !p.api_key.is_empty()),
        tool_approval_mode: user.tool_approval_mode.clone(),
        agent_mode: user.agent_mode.clone(),
        workspace_root: user.workspace_root.clone(),
        lead_agent_id: user.lead_agent_id.clone(),
        context_compression_enabled: user.context_compression_enabled,
        context_budget_tokens: user.context_budget_tokens,
        context_keep_recent_user_turns: user.context_keep_recent_user_turns,
        context_summary_max_tokens: user.context_summary_max_tokens,
        max_tool_rounds: user.max_tool_rounds,
        max_sub_agent_tool_rounds: user.max_sub_agent_tool_rounds,
        max_sub_agent_spawn_depth: user.max_sub_agent_spawn_depth,
        raw_content_view_enabled: user.raw_content_view_enabled,
        debug_dump_llm_prompts: user.debug_dump_llm_prompts,
        terminal_env_overrides: user.terminal_env_overrides.clone(),
        debug_menus_enabled: user.debug_menus_enabled,
        task_board_show_child_boards: user.task_board_show_child_boards,
        user_dynamic_inject_enabled: user.user_dynamic_inject_enabled,
        agent_default_models: user.agent_default_models.clone(),
        agent_task_board_history_trim: user.agent_task_board_history_trim.clone(),
        computer_human_like: user.computer_human_like,
        computer_initial_tier: user.computer_initial_tier.clone(),
        computer_annotated_screen_view_enabled: user.computer_annotated_screen_view_enabled,
        dati_api_url: platform.dati_api_url.clone(),
        dati_authcode: platform.dati_authcode.clone(),
        dati_typeno: platform.dati_typeno.clone(),
        dati_author: platform.dati_author.clone(),
        captcha_slider_offset_px: user.captcha_slider_offset_px,
        computer_show_monitor_picker: user.computer_show_monitor_picker,
        computer_auto_switch_monitor: user.computer_auto_switch_monitor,
        memory_enabled: user.memory_enabled,
        user_profile_enabled: user.user_profile_enabled,
        user_coding_rules: user.user_coding_rules.clone(),
        memory_char_limit: user.memory_char_limit,
        user_char_limit: user.user_char_limit,
        memory_nudge_interval: user.memory_nudge_interval,
        background_review_enabled: user.background_review_enabled,
        skill_creation_nudge_interval: user.skill_creation_nudge_interval,
        curator_enabled: user.curator_enabled,
        curator_idle_hours: user.curator_idle_hours,
        curator_interval_days: user.curator_interval_days,
        theme: user.theme.clone(),
        agent_ui_overrides: user.agent_ui_overrides.clone(),
        web_search_model: user.web_search_model.clone(),
        media_model_overrides: user.media_model_overrides.clone(),
        agent_performance_modes: user.agent_performance_modes.clone(),
        media_understanding_modes: user.media_understanding_modes.clone(),
        agent_mode_llm: user.agent_mode_llm.clone(),
        media_mode_llm: user.media_mode_llm.clone(),
        computer_tier_llm: user.computer_tier_llm.clone(),
        computer_pipeline_llm: user.computer_pipeline_llm.clone(),
        media_oss: platform.media_oss.clone(),
        max_parallel_tool_calls: user.max_parallel_tool_calls,
        max_parallel_sub_agents: user.max_parallel_sub_agents,
        max_parallel_media_jobs: user.max_parallel_media_jobs,
        max_concurrent_runs: user.max_concurrent_runs,
        parallel_tool_execution_enabled: user.parallel_tool_execution_enabled,
        round_enable_thinking: None,
        round_thinking_budget: None,
        round_reasoning_effort: None,
        round_thinking_intensity: None,
    };
    apply_platform_tier_defaults(&mut settings, &platform.tier_defaults, &platform.providers);
    settings
}

/// 将平台下发的档位默认（tierDefaults）补齐到运行时设置：用户已显式配置的
/// 档位保持不变，仅补缺失项。JSON 按已知形状容错读取。
/// tierDefaults 条目只填 providerId+model；maxTokens/enableThinking/thinkingBudget
/// 从 providers 的模型级参数（model_configs，优先）或服务商级默认继承。
pub fn apply_platform_tier_defaults(
    settings: &mut ModelSettings,
    tier_defaults: &serde_json::Value,
    providers: &[ProviderConfig],
) {
    let Some(obj) = tier_defaults.as_object() else {
        return;
    };
    if let Some(agent_defaults) = obj.get("agentDefaultModels").and_then(|v| v.as_object()) {
        for (agent, ref_val) in agent_defaults {
            if settings.agent_default_models.contains_key(agent) {
                continue;
            }
            if let Some(r) = agent_ref_from_json(ref_val) {
                settings.agent_default_models.insert(agent.clone(), r);
            }
        }
    }
    for (key, target) in [
        ("agentModeLlm", &mut settings.agent_mode_llm),
        ("mediaModeLlm", &mut settings.media_mode_llm),
    ] {
        let Some(outer) = obj.get(key).and_then(|v| v.as_object()) else {
            continue;
        };
        for (outer_key, inner) in outer {
            let inner_map = target.entry(outer_key.clone()).or_default();
            let Some(inner_obj) = inner.as_object() else {
                continue;
            };
            for (mode, cfg_val) in inner_obj {
                if inner_map.contains_key(mode) {
                    continue;
                }
                if let Some(cfg) = tier_cfg_from_json(cfg_val, providers) {
                    inner_map.insert(mode.clone(), cfg);
                }
            }
        }
    }
    if let Some(computer_tier) = obj.get("computerTierLlm").and_then(|v| v.as_object()) {
        for (tier, cfg_val) in computer_tier {
            if settings.computer_tier_llm.contains_key(tier) {
                continue;
            }
            if let Some(cfg) = tier_cfg_from_json(cfg_val, providers) {
                settings.computer_tier_llm.insert(tier.clone(), cfg);
            }
        }
    }
    if let Some(pipeline) = obj.get("computerPipelineLlm").and_then(|v| v.as_object()) {
        for (phase, cfg_val) in pipeline {
            let pid = cfg_val
                .get("providerId")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();
            let model = cfg_val
                .get("model")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();
            if pid.is_empty() || model.is_empty() {
                continue;
            }
            let p = &mut settings.computer_pipeline_llm;
            match phase.as_str() {
                "decision" => {
                    if p.decision.trim().is_empty() {
                        p.decision = model.into();
                        p.decision_provider_id = pid.into();
                    }
                }
                "position" => {
                    if p.position.trim().is_empty() {
                        p.position = model.into();
                        p.position_provider_id = pid.into();
                    }
                }
                "verify" => {
                    if p.verify.trim().is_empty() {
                        p.verify = model.into();
                        p.verify_provider_id = pid.into();
                    }
                }
                _ => {}
            }
        }
    }
    if let Some(media_gen) = obj.get("mediaGeneration").and_then(|v| v.as_object()) {
        let overrides = &mut settings.media_model_overrides;
        if overrides.image_generation.is_none() {
            if let Some(img) = media_gen.get("image").and_then(agent_ref_from_json) {
                overrides.image_generation = Some(img);
            }
        }
        if overrides.video_generation.is_none() {
            if let Some(vid) = media_gen.get("video").and_then(agent_ref_from_json) {
                overrides.video_generation = Some(vid);
            }
        }
    }
}

fn agent_ref_from_json(v: &serde_json::Value) -> Option<AgentModelRef> {
    let pid = v.get("providerId").and_then(|v| v.as_str()).unwrap_or("").trim();
    let model = v.get("model").and_then(|v| v.as_str()).unwrap_or("").trim();
    if pid.is_empty() || model.is_empty() {
        None
    } else {
        Some(AgentModelRef {
            provider_id: pid.to_string(),
            model: model.to_string(),
        })
    }
}

/// 解析档位条目（只含 providerId+model）；enableThinking/thinkingBudget 从
/// providers 的模型级参数（model_configs，优先）或服务商级默认继承，缺省 true/None。
fn tier_cfg_from_json(
    v: &serde_json::Value,
    providers: &[ProviderConfig],
) -> Option<ComputerTierLlmConfig> {
    let ref_val = agent_ref_from_json(v)?;
    let mut enable_thinking: Option<bool> = None;
    let mut thinking_budget: Option<u32> = None;
    let mut reasoning_effort: Option<String> = None;
    let mut thinking_intensity: Option<String> = None;
    if let Some(p) = providers.iter().find(|p| p.id == ref_val.provider_id) {
        if let Some(m) = p.model_configs.get(&ref_val.model) {
            enable_thinking = m.enable_thinking.or(enable_thinking);
            thinking_budget = m.thinking_budget.or(thinking_budget);
            reasoning_effort = m.reasoning_effort.clone().or(reasoning_effort);
            thinking_intensity = m.thinking_intensity.clone().or(thinking_intensity);
        }
        enable_thinking = enable_thinking.or(p.enable_thinking);
        thinking_budget = thinking_budget.or(p.thinking_budget);
        reasoning_effort = reasoning_effort.or(p.reasoning_effort.clone());
        thinking_intensity = thinking_intensity.or(p.thinking_intensity.clone());
    }
    Some(ComputerTierLlmConfig {
        provider_id: ref_val.provider_id,
        model: ref_val.model,
        enable_thinking: enable_thinking.unwrap_or(true),
        thinking_budget,
        reasoning_effort,
        thinking_intensity,
    })
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillDef {
    pub id: String,
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    #[serde(
        default,
        rename = "systemPrompt",
        skip_serializing_if = "String::is_empty"
    )]
    pub system_prompt: String,
    #[serde(rename = "toolNames")]
    pub tool_names: Vec<String>,
    pub scenario: String,
    pub builtin: bool,
    /// Legacy field; catalog load no longer indexes skill-tree files (use `skill_read`).
    #[serde(
        default,
        rename = "resourceFiles",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub resource_files: Vec<String>,
    #[serde(default)]
    pub source: Option<String>,
    /// `system` (bundled), `user` (`~/.pointer/skills`), or `external` (`~/.agents/skills`).
    #[serde(default = "default_skill_provenance", rename = "provenance")]
    pub provenance: String,
    /// Whether the skill is user-managed (not pinned system copy).
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

#[cfg(test)]
mod qwen_explicit_cache_tests {
    use super::*;

    #[test]
    fn enabled_for_default_qwen_provider() {
        let s = sample_settings();
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
        let mut s = sample_settings();
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
        let mut s = sample_settings();
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
mod user_settings_defaults_tests {
    use super::*;

    #[test]
    fn strips_legacy_platform_providers_and_tier_maps() {
        // 旧 user_settings.json 中的平台服务商与指向它们的档位映射应被清理。
        let mut user = UserSettings::default();
        user.providers.push(ProviderConfig {
            id: "qwen".into(),
            name: "千问".into(),
            base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1".into(),
            api_key: String::new(),
            models: vec!["qwen3.5-plus".into()],
            reasoning_in_messages: None,
            temperature: None,
            max_tokens: None,
            model_configs: HashMap::new(),
            enable_thinking: None,
            thinking_budget: None,
            reasoning_effort: None,
            thinking_protocol: None,
            thinking_intensity: None,
            extra_body: None,
            source: Some("user".into()),
        });
        user.providers.push(ProviderConfig {
            id: "custom-llm".into(),
            name: "Custom".into(),
            base_url: "https://custom.example/v1".into(),
            api_key: String::new(),
            models: vec!["custom-model".into()],
            reasoning_in_messages: None,
            temperature: None,
            max_tokens: None,
            model_configs: HashMap::new(),
            enable_thinking: None,
            thinking_budget: None,
            reasoning_effort: None,
            thinking_protocol: None,
            thinking_intensity: None,
            extra_body: None,
            source: Some("user".into()),
        });
        user.agent_default_models.insert(
            "general".into(),
            AgentModelRef {
                provider_id: "deepseek".into(),
                model: "deepseek-v4-flash".into(),
            },
        );
        user.agent_default_models.insert(
            "coder".into(),
            AgentModelRef {
                provider_id: "custom-llm".into(),
                model: "custom-model".into(),
            },
        );
        user.agent_mode_llm.insert(
            "general".into(),
            [(
                "fast".into(),
                ComputerTierLlmConfig {
                    provider_id: "qwen".into(),
                    model: "qwen3.5-flash".into(),
                    enable_thinking: true,
                    thinking_budget: None,
                    reasoning_effort: None,
                    thinking_intensity: None,
                },
            )]
            .into_iter()
            .collect(),
        );

        ensure_user_settings_defaults(&mut user);

        // 平台服务商全部清理；自定义服务商保留。
        assert!(
            !user
                .providers
                .iter()
                .any(|p| ["qwen", "deepseek", "doubao"].contains(&p.id.as_str())),
            "legacy platform providers must be stripped"
        );
        assert!(
            user.providers.iter().any(|p| p.id == "custom-llm"),
            "custom provider should be preserved"
        );
        // 指向平台 provider 的档位/默认清理；自定义保留。
        assert!(!user.agent_default_models.contains_key("general"));
        assert_eq!(
            user.agent_default_models.get("coder").map(|r| r.model.as_str()),
            Some("custom-model")
        );
        let general = user.agent_mode_llm.get("general").cloned().unwrap_or_default();
        assert!(general.is_empty(), "qwen tier map should be stripped");
    }

    #[test]
    fn preserves_user_customizations() {
        let mut user = UserSettings::default();
        // User-added custom provider must survive.
        user.providers.push(ProviderConfig {
            id: "openrouter".into(),
            name: "OpenRouter".into(),
            base_url: "https://openrouter.ai/api/v1".into(),
            api_key: String::new(),
            models: vec!["openai/gpt-5.4-nano".into()],
            reasoning_in_messages: None,
            temperature: None,
            max_tokens: None,
            model_configs: HashMap::new(),
            enable_thinking: None,
            thinking_budget: None,
            reasoning_effort: None,
            thinking_protocol: None,
            thinking_intensity: None,
            extra_body: None,
            source: Some("user".into()),
        });
        // User-chosen agent default for `coder` must survive backfill.
        user.agent_default_models.insert(
            "coder".into(),
            AgentModelRef {
                provider_id: "openrouter".into(),
                model: "openai/gpt-5.4-nano".into(),
            },
        );

        ensure_user_settings_defaults(&mut user);

        assert!(
            user.providers.iter().any(|p| p.id == "openrouter"),
            "custom provider should be preserved"
        );
        assert_eq!(
            user.agent_default_models
                .get("coder")
                .map(|r| r.model.as_str()),
            Some("openai/gpt-5.4-nano"),
            "user agent default for coder should not be overwritten"
        );
        // No platform defaults are backfilled anymore.
        assert!(user.agent_default_models.is_empty() || user.agent_default_models.len() == 1);
    }

    #[test]
    fn idempotent_when_defaults_already_present() {
        let mut user = UserSettings::default();
        let before = user.clone();
        ensure_user_settings_defaults(&mut user);
        assert_eq!(user.providers.len(), before.providers.len());
        assert_eq!(
            user.agent_default_models.len(),
            before.agent_default_models.len()
        );
    }

    #[test]
    fn strips_session_placeholder_model_configs() {
        let mut user = UserSettings::default();
        // Legacy debug-session pollution: per-mode maps + pipeline all session-*.
        user.agent_mode_llm.insert(
            "general".into(),
            [(
                "standard".into(),
                ComputerTierLlmConfig {
                    provider_id: "session-provider".into(),
                    model: "session-worker".into(),
                    enable_thinking: true,
                    thinking_budget: Some(2048),
                    reasoning_effort: None,
                    thinking_intensity: None,
                },
            )]
            .into_iter()
            .collect(),
        );
        user.media_mode_llm.insert(
            "image".into(),
            [(
                "fast".into(),
                ComputerTierLlmConfig {
                    provider_id: "session-provider".into(),
                    model: "session-worker".into(),
                    enable_thinking: true,
                    thinking_budget: Some(2048),
                    reasoning_effort: None,
                    thinking_intensity: None,
                },
            )]
            .into_iter()
            .collect(),
        );
        user.computer_tier_llm.insert(
            "primary".into(),
            ComputerTierLlmConfig {
                provider_id: "session-provider".into(),
                model: "session-worker".into(),
                enable_thinking: true,
                thinking_budget: Some(2048),
                reasoning_effort: None,
                thinking_intensity: None,
            },
        );
        user.computer_pipeline_llm = ComputerPipelineLlmSettings {
            decision: "session-worker".into(),
            position: "session-worker".into(),
            verify: "session-worker".into(),
            decision_provider_id: "session-provider".into(),
            position_provider_id: "session-provider".into(),
            verify_provider_id: "session-provider".into(),
            position_thinking_budget: 1024,
            verify_thinking_budget: 256,
        };

        ensure_user_settings_defaults(&mut user);

        assert!(
            user.agent_mode_llm
                .get("general")
                .map(|m| m.is_empty())
                .unwrap_or(true),
            "agent_mode_llm session placeholder should be removed"
        );
        assert!(
            user.media_mode_llm
                .get("image")
                .map(|m| m.is_empty())
                .unwrap_or(true),
            "media_mode_llm session placeholder should be removed"
        );
        assert!(
            !user.computer_tier_llm.contains_key("primary"),
            "computer_tier_llm session placeholder should be removed"
        );
        assert_ne!(user.computer_pipeline_llm.decision, "session-worker");
        assert_ne!(
            user.computer_pipeline_llm.position_provider_id,
            "session-provider"
        );
    }

    #[test]
    fn keeps_user_customized_mode_models_not_session_placeholders() {
        // 用户主动改过档位模型（合法值），清理 session 占位时不能误删。
        let mut user = UserSettings::default();
        user.agent_mode_llm.insert(
            "general".into(),
            [
                (
                    "fast".into(),
                    ComputerTierLlmConfig {
                        provider_id: "openrouter".into(),
                        model: "inclusionai/ling-3.0-flash".into(),
                        enable_thinking: true,
                        thinking_budget: Some(2048),
                        reasoning_effort: None,
                        thinking_intensity: None,
                    },
                ),
                (
                    "standard".into(),
                    ComputerTierLlmConfig {
                        provider_id: "session-provider".into(),
                        model: "session-worker".into(),
                        enable_thinking: true,
                        thinking_budget: Some(2048),
                        reasoning_effort: None,
                        thinking_intensity: None,
                    },
                ),
            ]
            .into_iter()
            .collect(),
        );
        user.media_mode_llm.insert(
            "video".into(),
            [(
                "expert".into(),
                ComputerTierLlmConfig {
                    provider_id: "qwen".into(),
                    model: "qwen3.6-plus".into(),
                    enable_thinking: true,
                    thinking_budget: Some(8192),
                    reasoning_effort: None,
                    thinking_intensity: None,
                },
            )]
            .into_iter()
            .collect(),
        );
        user.computer_tier_llm.insert(
            "advanced".into(),
            ComputerTierLlmConfig {
                provider_id: "qwen".into(),
                model: "qwen3.7-max".into(),
                enable_thinking: true,
                thinking_budget: Some(8192),
                reasoning_effort: None,
                thinking_intensity: None,
            },
        );

        ensure_user_settings_defaults(&mut user);

        let general = user.agent_mode_llm.get("general").expect("general map");
        // 用户自定义 fast 保留
        assert_eq!(
            general
                .get("fast")
                .map(|c| (c.provider_id.as_str(), c.model.as_str())),
            Some(("openrouter", "inclusionai/ling-3.0-flash"))
        );
        // session 占位 standard 被删
        assert!(
            general.get("standard").is_none(),
            "session placeholder removed"
        );
        // media video expert 指向平台 provider（qwen）→ 被清理
        assert!(
            user.media_mode_llm
                .get("video")
                .map(|m| m.is_empty())
                .unwrap_or(true),
            "qwen tier map should be stripped"
        );
        // computer advanced 指向平台 provider（qwen）→ 被清理
        assert!(
            user.computer_tier_llm
                .get("advanced")
                .map(|c| c.provider_id.as_str())
                .is_none_or(|pid| pid != "qwen"),
            "qwen computer tier should be stripped"
        );
    }

    #[test]
    fn merges_platform_only_providers_from_server_config() {
        // server.toml [llm] 配置的自定义 provider 只进 platform 层；
        // merged 视图必须包含它（带 key），否则界面和运行时都不可用。
        let mut platform = PlatformSettings::default();
        platform.providers.push(ProviderConfig {
            id: "vllm-local".into(),
            name: "本地 vLLM".into(),
            base_url: "http://127.0.0.1:8000/v1".into(),
            api_key: "sk-local".into(),
            models: vec!["qwen3.6-27b".into()],
            reasoning_in_messages: None,
            temperature: None,
            max_tokens: None,
            model_configs: Default::default(),
            enable_thinking: None,
            thinking_budget: None,
            reasoning_effort: None,
            thinking_protocol: None,
            thinking_intensity: None,
            extra_body: None,
            source: Some("platform".into()),
        });
        let mut user = UserSettings::default();
        user.active_provider_id = "vllm-local".into();
        user.model = "qwen3.6-27b".into();

        let merged = merge_user_platform(&user, &platform);

        let vllm = merged
            .providers
            .iter()
            .find(|p| p.id == "vllm-local")
            .expect("server custom provider should appear in merged view");
        assert_eq!(vllm.api_key, "sk-local");
        assert_eq!(merged.active_provider_id, "vllm-local");
        assert!(
            merged.providers.iter().any(|p| !p.api_key.is_empty()),
            "merged has_key should be true"
        );
        // 本地不再内置平台服务商；platform-only provider 直接进入 merged 视图。
        assert_eq!(merged.providers.len(), 1);
        assert_eq!(merged.providers[0].id, "vllm-local");
    }

    #[test]
    fn platform_only_provider_does_not_duplicate_existing_user_provider() {
        let mut platform = PlatformSettings::default();
        platform.providers.push(sample_settings().providers.remove(0));
        platform.providers[0].api_key = "platform-qwen-key".into();
        let mut user = UserSettings::default();
        user.providers.push(sample_settings().providers.remove(0));
        let merged = merge_user_platform(&user, &platform);
        assert_eq!(
            merged.providers.len(),
            1,
            "no duplicate for user-owned provider"
        );
        assert_eq!(
            merged.providers[0].api_key, "platform-qwen-key",
            "platform key overlaid onto user-owned provider"
        );
    }
}

#[cfg(test)]
mod effective_reasoning_tests {
    use super::*;

    #[test]
    fn effective_reasoning_defaults_false() {
        let s = ModelSettings::default();
        assert!(!effective_reasoning_in_messages(&s));
    }

    #[test]
    fn effective_reasoning_provider_off() {
        let mut s = sample_settings();
        s.providers[0].reasoning_in_messages = Some(false);
        assert!(!effective_reasoning_in_messages(&s));
    }

    #[test]
    fn effective_reasoning_model_overrides_provider() {
        let mut s = sample_settings();
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
        let mut s = sample_settings();
        s.model = "qwen3.5-plus".into();
        s.temperature = 0.2;
        s.providers[0].temperature = Some(0.9);
        assert!((effective_temperature(&s) - 0.9).abs() < f32::EPSILON);
    }

    #[test]
    fn effective_max_tokens_provider_default() {
        let mut s = sample_settings();
        s.model = "qwen3.5-plus".into();
        s.max_tokens = 512;
        s.providers[0].max_tokens = Some(8192);
        assert_eq!(effective_max_tokens(&s), 8192);
    }

    #[test]
    fn effective_temperature_model_override() {
        let mut s = sample_settings();
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
        let mut s = sample_settings();
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
        let mut s = sample_settings();
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
        let mut s = sample_settings();
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
        assert_eq!(o.get("thinking_budget"), Some(&Value::Number(500.into())));
    }

    #[test]
    fn deepseek_reasoning_effort_on_wire() {
        let mut s = sample_settings();
        s.active_provider_id = "deepseek".into();
        s.model = "deepseek-v4-flash".into();
        s.providers[1].reasoning_effort = Some("max".into());
        let v = effective_chat_extra_body(&s).expect("effort");
        assert_eq!(
            v.get("reasoning_effort"),
            Some(&Value::String("max".into()))
        );
        assert!(v.get("thinking").is_none());
        assert!(v.get("output_config").is_none());
    }

    #[test]
    fn deepseek_thinking_disabled_omits_effort() {
        let mut s = sample_settings();
        s.active_provider_id = "deepseek".into();
        s.model = "deepseek-v4-flash".into();
        s.providers[1].enable_thinking = Some(false);
        s.providers[1].reasoning_effort = Some("high".into());
        assert!(effective_chat_extra_body(&s).is_none());
    }

    #[test]
    fn apply_thinking_disabled_budget_forces_enable_false() {
        let mut s = sample_settings();
        s.model = s.providers[0].models[0].clone();
        s.providers[0].enable_thinking = Some(true);
        s.providers[0].thinking_budget = Some(2048);
        let mut extra = effective_chat_extra_body(&s);
        apply_thinking_disabled_to_extra_body(&s, &mut extra);
        let o = extra.as_ref().and_then(|v| v.as_object()).expect("obj");
        assert_eq!(o.get("enable_thinking"), Some(&Value::Bool(false)));
        assert!(o.get("thinking_budget").is_none());
    }

    #[test]
    fn apply_thinking_disabled_effort_sets_thinking_disabled() {
        let mut s = sample_settings();
        s.active_provider_id = "deepseek".into();
        s.model = "deepseek-v4-flash".into();
        s.providers[1].reasoning_effort = Some("max".into());
        let mut extra = effective_chat_extra_body(&s);
        assert_eq!(
            extra.as_ref().and_then(|v| v.get("reasoning_effort")),
            Some(&Value::String("max".into()))
        );
        apply_thinking_disabled_to_extra_body(&s, &mut extra);
        let o = extra.as_ref();
        assert!(o.and_then(|v| v.get("reasoning_effort")).is_none());
        assert!(o.and_then(|v| v.get("thinking")).is_none());
        assert!(!thinking_enabled_in_extra_body(o));
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
    fn web_effective_settings_view_omits_dati_fields() {
        let mut platform = PlatformSettings::default();
        platform.providers.push(sample_settings().providers.remove(0));
        platform.dati_api_url = "https://dati.example".into();
        platform.dati_authcode = "secret-auth".into();
        platform.dati_typeno = "501057".into();
        platform.dati_author = "author".into();
        platform.providers[0].api_key = "sk-live-secret".into();
        platform.media_oss.access_key_secret = "oss-secret".into();
        let mut user = UserSettings::default();
        user.raw_content_view_enabled = true;
        user.debug_dump_llm_prompts = true;
        user.debug_menus_enabled = true;
        user.computer_annotated_screen_view_enabled = true;
        let merged = merge_user_platform(&user, &platform);
        let view = EffectiveSettingsView {
            user,
            platform,
            merged,
            can_edit_platform: false,
            is_platform_admin: false,
        };
        let json = serde_json::to_string(&WebEffectiveSettingsView(view)).unwrap();
        assert!(!json.contains("datiApiUrl"));
        assert!(!json.contains("datiAuthcode"));
        assert!(!json.contains("datiTypeno"));
        assert!(!json.contains("datiAuthor"));
        assert!(!json.contains("secret-auth"));
        assert!(!json.contains("sk-live-secret"));
        assert!(!json.contains("oss-secret"));
        assert!(!json.contains("rawContentViewEnabled"));
        assert!(!json.contains("debugDumpLlmPrompts"));
        assert!(!json.contains("debugMenusEnabled"));
        assert!(!json.contains("computerAnnotatedScreenViewEnabled"));
        assert!(json.contains("\"apiKey\":\"****\""));
    }

    #[test]
    fn web_effective_settings_view_keeps_debug_fields_for_admin() {
        let mut platform = PlatformSettings::default();
        platform
            .providers
            .push(sample_settings().providers.remove(0));
        platform.providers[0].api_key = "sk-live-secret".into();
        let mut user = UserSettings::default();
        user.debug_menus_enabled = true;
        user.raw_content_view_enabled = true;
        user.agent_mode_llm = default_agent_mode_llm();
        let merged = merge_user_platform(&user, &platform);
        let view = EffectiveSettingsView {
            user,
            platform,
            merged,
            can_edit_platform: true,
            is_platform_admin: true,
        };
        let json = serde_json::to_string(&WebEffectiveSettingsView(view)).unwrap();
        assert!(json.contains("debugMenusEnabled"));
        assert!(json.contains("rawContentViewEnabled"));
        assert!(json.contains("agentModeLlm"));
        assert!(!json.contains("sk-live-secret"));
        assert!(json.contains("\"apiKey\":\"****\""));
    }

    #[test]
    fn preserve_platform_debug_settings_in_model_keeps_server_values() {
        let mut user = UserSettings::default();
        user.raw_content_view_enabled = true;
        user.debug_menus_enabled = true;
        user.max_sub_agent_tool_rounds = 42;
        let mut incoming = ModelSettings::default();
        incoming.raw_content_view_enabled = false;
        incoming.debug_menus_enabled = false;
        incoming.max_sub_agent_tool_rounds = 1;
        preserve_platform_debug_settings_in_model(&mut incoming, &user);
        assert!(incoming.raw_content_view_enabled);
        assert!(incoming.debug_menus_enabled);
        assert_eq!(incoming.max_sub_agent_tool_rounds, 42);
    }

    #[test]
    fn agent_skill_overrides_round_trip_and_default_empty() {
        let default_user = UserSettings::default();
        assert!(default_user.agent_skill_overrides.is_empty());

        let user: UserSettings = serde_json::from_value(serde_json::json!({
            "agentSkillOverrides": {
                "coder": ["skill-manager"]
            }
        }))
        .unwrap();
        assert_eq!(
            user.agent_skill_overrides.get("coder"),
            Some(&vec!["skill-manager".to_string()])
        );
        let json = serde_json::to_value(user).unwrap();
        assert_eq!(
            json["agentSkillOverrides"]["coder"],
            serde_json::json!(["skill-manager"])
        );
    }

    #[test]
    fn collapse_process_by_default_round_trip_and_default_false() {
        let default_user = UserSettings::default();
        assert!(!default_user.collapse_process_by_default);

        let user: UserSettings = serde_json::from_value(serde_json::json!({
            "collapseProcessByDefault": true
        }))
        .unwrap();
        assert!(user.collapse_process_by_default);
        let json = serde_json::to_value(user).unwrap();
        assert_eq!(json["collapseProcessByDefault"], serde_json::json!(true));

        // Missing field falls back to the default (false) instead of erroring.
        let absent: UserSettings = serde_json::from_value(serde_json::json!({})).unwrap();
        assert!(!absent.collapse_process_by_default);
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
        assert_eq!(o.get("thinking_budget"), Some(&Value::Number(100.into())));
    }

    #[test]
    fn hermes_style_extra_body_merges_and_flattens_for_openai_compatible() {
        let mut s = sample_settings();
        s.providers.push(ProviderConfig {
            id: "doubao".into(),
            name: "豆包".into(),
            base_url: "https://ark.cn-beijing.volces.com/api/v3".into(),
            api_key: String::new(),
            models: vec!["ep-demo".into()],
            reasoning_in_messages: None,
            temperature: None,
            max_tokens: None,
            model_configs: HashMap::new(),
            enable_thinking: None,
            thinking_budget: None,
            reasoning_effort: None,
            thinking_protocol: None,
            thinking_intensity: None,
            extra_body: Some(serde_json::json!({
                "repetition_penalty": 1.1,
                "top_p": 0.8
            })),
            source: Some("platform".into()),
        });
        s.active_provider_id = "doubao".into();
        s.model = "ep-demo".into();
        s.providers[2].model_configs.insert(
            "ep-demo".into(),
            ModelRuntimeOverrides {
                extra_body: Some(serde_json::json!({ "top_p": 0.9 })),
                ..Default::default()
            },
        );
        let v = effective_chat_extra_body(&s).expect("extra");
        assert_eq!(v.get("repetition_penalty"), Some(&serde_json::json!(1.1)));
        assert_eq!(v.get("top_p"), Some(&serde_json::json!(0.9)));
        assert!(chat_request_flattens_extra_body(&s));
        let body = serde_json::json!({
            "model": "ep-demo",
            "temperature": 0.7,
            "extra_body": v
        });
        let out = flatten_chat_extra_body_on_wire(body, &s);
        let o = out.as_object().unwrap();
        assert!(!o.contains_key("extra_body"));
        assert_eq!(o.get("temperature"), Some(&serde_json::json!(0.7)));
        assert_eq!(o.get("repetition_penalty"), Some(&serde_json::json!(1.1)));
        assert_eq!(o.get("top_p"), Some(&serde_json::json!(0.9)));
    }

    #[test]
    fn debug_session_web_redaction_keeps_mappings_and_masks_keys() {
        let mut debug = DebugSessionSettings::from(&sample_settings());
        debug.providers[0].api_key = "sk-secret".into();
        debug.agent_mode_llm.insert(
            "general".into(),
            [(
                "fast".into(),
                ComputerTierLlmConfig {
                    provider_id: "qwen".into(),
                    model: "session-model".into(),
                    enable_thinking: true,
                    thinking_budget: None,
                    reasoning_effort: None,
                    thinking_intensity: None,
                },
            )]
            .into_iter()
            .collect(),
        );

        redact_debug_session_settings_for_web(&mut debug);

        assert_eq!(debug.providers[0].api_key, "****");
        assert_eq!(
            debug.agent_mode_llm["general"]["fast"].model,
            "session-model"
        );
    }
}
