//! Per-provider thinking-intensity strategies (wire translation).
//!
//! Product intensity is unified: `off | low | medium | high | max`.
//! Each strategy maps that ladder onto vendor-specific chat/completions fields.

use serde_json::{Map, Value};

use crate::models::{
    provider_uses_dashscope_compatible_api, provider_uses_deepseek_api, ComputerTierLlmConfig,
    ModelRuntimeOverrides, ModelSettings, ProviderConfig, DEFAULT_THINKING_BUDGET,
};

/// Unified product ladder for 「思考强度」.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThinkingIntensity {
    Off,
    Low,
    Medium,
    High,
    Max,
}

impl ThinkingIntensity {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "off" | "none" | "disabled" => Some(Self::Off),
            "low" | "minimal" => Some(Self::Low),
            "medium" | "med" | "standard" => Some(Self::Medium),
            "high" => Some(Self::High),
            "max" | "xhigh" | "highest" => Some(Self::Max),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Max => "max",
        }
    }
}

/// Wire strategy id (stored as `thinkingProtocol`, plus auto inference).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThinkingStrategyId {
    /// Infer from provider template / URL.
    Auto,
    /// Qwen / DashScope: `enable_thinking` + `thinking_budget`.
    Budget,
    /// DeepSeek V4+: `reasoning_effort` = `low` | `high` | `max` only.
    Effort,
    /// OpenRouter: root `reasoning: { enabled, effort }`.
    OpenRouter,
    /// Kimi / Moonshot: `thinking` xor `reasoning_effort`.
    Kimi,
    /// OpenAI-style top-level `reasoning_effort` (Zhipu / GPT-o / custom).
    OpenaiEffort,
    /// Do not emit structured thinking fields.
    Off,
    /// Only free-form `extraBody` (no structured overlay).
    Custom,
}

impl ThinkingStrategyId {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "budget" | "qwen" => Some(Self::Budget),
            "effort" | "deepseek" => Some(Self::Effort),
            "openrouter" | "or" => Some(Self::OpenRouter),
            "kimi" | "moonshot" => Some(Self::Kimi),
            "openai" | "openai_effort" | "openaieffort" | "zhipu" | "glm" => {
                Some(Self::OpenaiEffort)
            }
            "off" => Some(Self::Off),
            "custom" => Some(Self::Custom),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Budget => "budget",
            Self::Effort => "effort",
            Self::OpenRouter => "openrouter",
            Self::Kimi => "kimi",
            Self::OpenaiEffort => "openai_effort",
            Self::Off => "off",
            Self::Custom => "custom",
        }
    }
}

pub fn provider_looks_openrouter(provider: &ProviderConfig) -> bool {
    if provider.id.eq_ignore_ascii_case("openrouter") {
        return true;
    }
    let name = provider.name.to_ascii_lowercase();
    if name.contains("openrouter") || name.contains("open router") {
        return true;
    }
    provider
        .base_url
        .to_ascii_lowercase()
        .contains("openrouter.ai")
}

pub fn provider_looks_kimi(provider: &ProviderConfig) -> bool {
    if provider.id.eq_ignore_ascii_case("kimi") {
        return true;
    }
    let url = provider.base_url.to_ascii_lowercase();
    url.contains("moonshot.cn") || url.contains("moonshot.ai") || url.contains("api.kimi.com")
}

pub fn provider_looks_zhipu(provider: &ProviderConfig) -> bool {
    if provider.id.eq_ignore_ascii_case("zhipu") || provider.id.eq_ignore_ascii_case("glm") {
        return true;
    }
    let url = provider.base_url.to_ascii_lowercase();
    url.contains("bigmodel.cn") || url.contains("open.bigmodel.cn") || url.contains("api.z.ai")
}

fn deepseek_model_supports_thinking(model: &str) -> bool {
    let m = model.trim().to_ascii_lowercase();
    if m.is_empty() {
        return false;
    }
    if m == "deepseek-reasoner" {
        return true;
    }
    if m.starts_with("deepseek-v") && !m.starts_with("deepseek-v3") {
        return true;
    }
    m.contains("deepseek-v4") || m.contains("deepseek-r1")
}

/// Resolve concrete strategy (never returns [`ThinkingStrategyId::Auto`]).
pub fn resolve_thinking_strategy_id(
    provider: &ProviderConfig,
    model_over: Option<&ModelRuntimeOverrides>,
) -> ThinkingStrategyId {
    let explicit = model_over
        .and_then(|o| o.thinking_protocol.as_deref())
        .or(provider.thinking_protocol.as_deref())
        .and_then(ThinkingStrategyId::parse);
    match explicit {
        Some(ThinkingStrategyId::Auto) | None => {
            if provider_uses_dashscope_compatible_api(provider) {
                ThinkingStrategyId::Budget
            } else if provider_uses_deepseek_api(provider) {
                ThinkingStrategyId::Effort
            } else if provider_looks_openrouter(provider) {
                ThinkingStrategyId::OpenRouter
            } else if provider_looks_kimi(provider) {
                ThinkingStrategyId::Kimi
            } else if provider_looks_zhipu(provider) {
                ThinkingStrategyId::OpenaiEffort
            } else {
                // Custom OpenAI-compatible endpoints: intensity maps to
                // root `reasoning_effort` (none/low/medium/high/xhigh).
                ThinkingStrategyId::OpenaiEffort
            }
        }
        Some(other) => other,
    }
}

fn budget_for_intensity(intensity: ThinkingIntensity) -> u32 {
    match intensity {
        ThinkingIntensity::Off => 0,
        ThinkingIntensity::Low => 1024,
        ThinkingIntensity::Medium => DEFAULT_THINKING_BUDGET,
        ThinkingIntensity::High => 4096,
        ThinkingIntensity::Max => 8192,
    }
}

fn intensity_from_budget(budget: u32) -> ThinkingIntensity {
    if budget == 0 {
        ThinkingIntensity::Off
    } else if budget < 1536 {
        ThinkingIntensity::Low
    } else if budget < 3072 {
        ThinkingIntensity::Medium
    } else if budget < 6144 {
        ThinkingIntensity::High
    } else {
        ThinkingIntensity::Max
    }
}

/// Copy scene-tier / computer mapping thinking onto this round.
/// Model catalog defaults are not consulted here — they are stamped at select time.
pub fn apply_tier_thinking_to_round(settings: &mut ModelSettings, cfg: &ComputerTierLlmConfig) {
    settings.round_thinking_locked = true;
    match cfg
        .thinking_intensity
        .as_deref()
        .and_then(ThinkingIntensity::parse)
    {
        Some(ThinkingIntensity::Off) => {
            settings.round_thinking_intensity = Some(ThinkingIntensity::Off.as_str().into());
            settings.round_enable_thinking = Some(false);
            settings.round_thinking_budget = None;
            settings.round_reasoning_effort = None;
        }
        Some(level) => {
            settings.round_thinking_intensity = Some(level.as_str().into());
            settings.round_enable_thinking = Some(true);
            settings.round_thinking_budget = cfg.thinking_budget;
            settings.round_reasoning_effort = cfg.reasoning_effort.clone();
        }
        None => {
            settings.round_thinking_intensity = None;
            settings.round_enable_thinking = None;
            settings.round_thinking_budget = None;
            settings.round_reasoning_effort = None;
        }
    }
}

/// Resolve product intensity from round overrides + provider/model fields + legacy fields.
pub fn resolve_thinking_intensity(
    settings: &ModelSettings,
    provider: &ProviderConfig,
    model_over: Option<&ModelRuntimeOverrides>,
) -> Option<ThinkingIntensity> {
    if settings.round_enable_thinking == Some(false) {
        return Some(ThinkingIntensity::Off);
    }
    if settings.round_thinking_locked {
        if let Some(raw) = settings.round_thinking_intensity.as_deref() {
            if let Some(i) = ThinkingIntensity::parse(raw) {
                return Some(i);
            }
        }
        if let Some(effort) = settings.round_reasoning_effort.as_deref() {
            if let Some(i) = ThinkingIntensity::parse(effort) {
                return Some(i);
            }
            match effort.trim().to_ascii_lowercase().as_str() {
                "high" => return Some(ThinkingIntensity::High),
                "max" => return Some(ThinkingIntensity::Max),
                _ => {}
            }
        }
        if let Some(budget) = settings.round_thinking_budget.filter(|&n| n > 0) {
            return Some(intensity_from_budget(budget));
        }
        if settings.round_enable_thinking == Some(true) {
            return Some(ThinkingIntensity::Medium);
        }
        return None;
    }
    let stored_enable = settings
        .round_enable_thinking
        .or_else(|| model_over.and_then(|o| o.enable_thinking))
        .or(provider.enable_thinking);
    if stored_enable == Some(false) {
        return Some(ThinkingIntensity::Off);
    }
    if let Some(raw) = settings.round_thinking_intensity.as_deref() {
        if let Some(i) = ThinkingIntensity::parse(raw) {
            return Some(i);
        }
    }
    if let Some(raw) = model_over
        .and_then(|o| o.thinking_intensity.as_deref())
        .or(provider.thinking_intensity.as_deref())
    {
        if let Some(i) = ThinkingIntensity::parse(raw) {
            return Some(i);
        }
    }
    if let Some(effort) = settings
        .round_reasoning_effort
        .as_deref()
        .or_else(|| model_over.and_then(|o| o.reasoning_effort.as_deref()))
        .or(provider.reasoning_effort.as_deref())
    {
        if let Some(i) = ThinkingIntensity::parse(effort) {
            return Some(i);
        }
        match effort.trim().to_ascii_lowercase().as_str() {
            "high" => return Some(ThinkingIntensity::High),
            "max" => return Some(ThinkingIntensity::Max),
            _ => {}
        }
    }
    if let Some(budget) = settings
        .round_thinking_budget
        .or_else(|| model_over.and_then(|o| o.thinking_budget))
        .or(provider.thinking_budget)
        .filter(|&n| n > 0)
    {
        return Some(intensity_from_budget(budget));
    }
    if stored_enable == Some(true) {
        return Some(ThinkingIntensity::Medium);
    }
    None
}

fn insert_thinking_type(m: &mut Map<String, Value>, enabled: bool) {
    let mut thinking = Map::new();
    thinking.insert(
        "type".into(),
        Value::String(if enabled {
            "enabled".into()
        } else {
            "disabled".into()
        }),
    );
    m.insert("thinking".into(), Value::Object(thinking));
}

fn clear_budget_keys(m: &mut Map<String, Value>) {
    m.remove("enable_thinking");
    m.remove("thinking_budget");
}

fn clear_effort_keys(m: &mut Map<String, Value>) {
    m.remove("reasoning_effort");
    m.remove("thinking");
    m.remove("reasoning");
}

trait ThinkingStrategy {
    fn apply(
        &self,
        intensity: Option<ThinkingIntensity>,
        settings: &ModelSettings,
        provider: &ProviderConfig,
        model: &str,
        model_over: Option<&ModelRuntimeOverrides>,
        m: &mut Map<String, Value>,
    );
    fn apply_disabled(&self, m: &mut Map<String, Value>);
}

struct BudgetStrategy;
struct EffortStrategy;
struct OpenRouterStrategy;
struct KimiStrategy;
struct OpenaiEffortStrategy;
struct OffStrategy;
struct CustomStrategy;

impl ThinkingStrategy for BudgetStrategy {
    fn apply(
        &self,
        intensity: Option<ThinkingIntensity>,
        settings: &ModelSettings,
        provider: &ProviderConfig,
        _model: &str,
        model_over: Option<&ModelRuntimeOverrides>,
        m: &mut Map<String, Value>,
    ) {
        clear_effort_keys(m);
        let (enable, budget) = match intensity {
            Some(ThinkingIntensity::Off) => (false, None),
            Some(level) => (true, Some(budget_for_intensity(level))),
            None => {
                if settings.round_thinking_locked {
                    return;
                }
                let enable = settings
                    .round_enable_thinking
                    .or_else(|| model_over.and_then(|o| o.enable_thinking))
                    .or(provider.enable_thinking);
                let Some(enable) = enable else {
                    return;
                };
                if !enable {
                    (false, None)
                } else {
                    let budget = settings
                        .round_thinking_budget
                        .or_else(|| model_over.and_then(|o| o.thinking_budget))
                        .or(provider.thinking_budget)
                        .filter(|&n| n > 0)
                        .unwrap_or(DEFAULT_THINKING_BUDGET);
                    (true, Some(budget))
                }
            }
        };
        m.insert("enable_thinking".into(), Value::Bool(enable));
        if enable {
            if let Some(budget) = budget {
                m.insert("thinking_budget".into(), Value::Number(budget.into()));
            }
        } else {
            m.remove("thinking_budget");
        }
    }

    fn apply_disabled(&self, m: &mut Map<String, Value>) {
        m.insert("enable_thinking".into(), Value::Bool(false));
        m.remove("thinking_budget");
        clear_effort_keys(m);
    }
}

/// DeepSeek accepts only `low` / `high` / `max` (no `medium`, no `thinking.type`).
fn deepseek_reasoning_effort_wire(raw: &str) -> Option<&'static str> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "low" | "minimal" => Some("low"),
        "medium" | "med" | "standard" | "high" => Some("high"),
        "max" | "xhigh" | "highest" => Some("max"),
        _ => None,
    }
}

fn deepseek_effort_for_intensity(intensity: ThinkingIntensity) -> Option<&'static str> {
    match intensity {
        ThinkingIntensity::Off => None,
        ThinkingIntensity::Low => Some("low"),
        ThinkingIntensity::Medium | ThinkingIntensity::High => Some("high"),
        ThinkingIntensity::Max => Some("max"),
    }
}

impl ThinkingStrategy for EffortStrategy {
    fn apply(
        &self,
        intensity: Option<ThinkingIntensity>,
        settings: &ModelSettings,
        provider: &ProviderConfig,
        model: &str,
        model_over: Option<&ModelRuntimeOverrides>,
        m: &mut Map<String, Value>,
    ) {
        if !deepseek_model_supports_thinking(model) && provider_uses_deepseek_api(provider) {
            return;
        }
        clear_budget_keys(m);
        m.remove("thinking");
        m.remove("output_config");
        let enable = match intensity {
            Some(ThinkingIntensity::Off) => false,
            Some(_) => true,
            None => {
                if settings.round_thinking_locked {
                    false
                } else {
                    settings
                        .round_enable_thinking
                        .or_else(|| model_over.and_then(|o| o.enable_thinking))
                        .or(provider.enable_thinking)
                        .unwrap_or(true)
                }
            }
        };
        if !enable {
            m.remove("reasoning_effort");
            return;
        }
        let effort = match intensity {
            Some(level) => deepseek_effort_for_intensity(level),
            None => settings
                .round_reasoning_effort
                .as_deref()
                .or_else(|| model_over.and_then(|o| o.reasoning_effort.as_deref()))
                .or(provider.reasoning_effort.as_deref())
                .and_then(deepseek_reasoning_effort_wire),
        };
        if let Some(effort) = effort {
            m.insert(
                "reasoning_effort".into(),
                Value::String(effort.to_string()),
            );
        } else {
            m.remove("reasoning_effort");
        }
    }

    fn apply_disabled(&self, m: &mut Map<String, Value>) {
        clear_budget_keys(m);
        m.remove("thinking");
        m.remove("output_config");
        m.remove("reasoning_effort");
    }
}

impl ThinkingStrategy for OpenRouterStrategy {
    fn apply(
        &self,
        intensity: Option<ThinkingIntensity>,
        _settings: &ModelSettings,
        _provider: &ProviderConfig,
        _model: &str,
        _model_over: Option<&ModelRuntimeOverrides>,
        m: &mut Map<String, Value>,
    ) {
        clear_budget_keys(m);
        m.remove("thinking");
        m.remove("reasoning_effort");
        let Some(intensity) = intensity else {
            return;
        };
        let mut reasoning = Map::new();
        match intensity {
            ThinkingIntensity::Off => {
                reasoning.insert("enabled".into(), Value::Bool(false));
            }
            other => {
                reasoning.insert("enabled".into(), Value::Bool(true));
                let effort = match other {
                    ThinkingIntensity::Low => "low",
                    ThinkingIntensity::Medium => "medium",
                    ThinkingIntensity::High => "high",
                    ThinkingIntensity::Max => "max",
                    ThinkingIntensity::Off => unreachable!(),
                };
                reasoning.insert("effort".into(), Value::String(effort.into()));
            }
        }
        m.insert("reasoning".into(), Value::Object(reasoning));
    }

    fn apply_disabled(&self, m: &mut Map<String, Value>) {
        clear_budget_keys(m);
        m.remove("thinking");
        m.remove("reasoning_effort");
        let mut reasoning = Map::new();
        reasoning.insert("enabled".into(), Value::Bool(false));
        m.insert("reasoning".into(), Value::Object(reasoning));
    }
}

impl ThinkingStrategy for KimiStrategy {
    fn apply(
        &self,
        intensity: Option<ThinkingIntensity>,
        _settings: &ModelSettings,
        _provider: &ProviderConfig,
        _model: &str,
        _model_over: Option<&ModelRuntimeOverrides>,
        m: &mut Map<String, Value>,
    ) {
        clear_budget_keys(m);
        m.remove("reasoning");
        m.remove("thinking");
        m.remove("reasoning_effort");
        let Some(intensity) = intensity else {
            if !_settings.round_thinking_locked {
                insert_thinking_type(m, true);
            }
            return;
        };
        match intensity {
            ThinkingIntensity::Off => insert_thinking_type(m, false),
            ThinkingIntensity::Low => {
                m.insert("reasoning_effort".into(), Value::String("low".into()));
            }
            ThinkingIntensity::Medium => {
                m.insert("reasoning_effort".into(), Value::String("medium".into()));
            }
            ThinkingIntensity::High | ThinkingIntensity::Max => {
                m.insert("reasoning_effort".into(), Value::String("high".into()));
            }
        }
    }

    fn apply_disabled(&self, m: &mut Map<String, Value>) {
        clear_budget_keys(m);
        m.remove("reasoning");
        m.remove("reasoning_effort");
        insert_thinking_type(m, false);
    }
}

impl ThinkingStrategy for OpenaiEffortStrategy {
    fn apply(
        &self,
        intensity: Option<ThinkingIntensity>,
        _settings: &ModelSettings,
        _provider: &ProviderConfig,
        _model: &str,
        _model_over: Option<&ModelRuntimeOverrides>,
        m: &mut Map<String, Value>,
    ) {
        clear_budget_keys(m);
        m.remove("thinking");
        m.remove("reasoning");
        let Some(intensity) = intensity else {
            return;
        };
        match intensity {
            ThinkingIntensity::Off => {
                m.insert("reasoning_effort".into(), Value::String("none".into()));
            }
            ThinkingIntensity::Low => {
                m.insert("reasoning_effort".into(), Value::String("low".into()));
            }
            ThinkingIntensity::Medium => {
                m.insert("reasoning_effort".into(), Value::String("medium".into()));
            }
            ThinkingIntensity::High => {
                m.insert("reasoning_effort".into(), Value::String("high".into()));
            }
            ThinkingIntensity::Max => {
                m.insert("reasoning_effort".into(), Value::String("xhigh".into()));
            }
        }
    }

    fn apply_disabled(&self, m: &mut Map<String, Value>) {
        clear_budget_keys(m);
        m.remove("thinking");
        m.remove("reasoning");
        m.insert("reasoning_effort".into(), Value::String("none".into()));
    }
}

impl ThinkingStrategy for OffStrategy {
    fn apply(
        &self,
        _intensity: Option<ThinkingIntensity>,
        _settings: &ModelSettings,
        _provider: &ProviderConfig,
        _model: &str,
        _model_over: Option<&ModelRuntimeOverrides>,
        _m: &mut Map<String, Value>,
    ) {
    }

    fn apply_disabled(&self, m: &mut Map<String, Value>) {
        if m.contains_key("enable_thinking") || m.contains_key("thinking_budget") {
            m.insert("enable_thinking".into(), Value::Bool(false));
            m.remove("thinking_budget");
        }
        if m.contains_key("thinking") || m.contains_key("reasoning_effort") {
            insert_thinking_type(m, false);
            m.remove("reasoning_effort");
        }
        if m.contains_key("reasoning") {
            let mut reasoning = Map::new();
            reasoning.insert("enabled".into(), Value::Bool(false));
            m.insert("reasoning".into(), Value::Object(reasoning));
        }
    }
}

impl ThinkingStrategy for CustomStrategy {
    fn apply(
        &self,
        _intensity: Option<ThinkingIntensity>,
        _settings: &ModelSettings,
        _provider: &ProviderConfig,
        _model: &str,
        _model_over: Option<&ModelRuntimeOverrides>,
        _m: &mut Map<String, Value>,
    ) {
    }

    fn apply_disabled(&self, m: &mut Map<String, Value>) {
        OffStrategy.apply_disabled(m);
    }
}

fn strategy_for(id: ThinkingStrategyId) -> &'static dyn ThinkingStrategy {
    match id {
        ThinkingStrategyId::Budget => &BudgetStrategy,
        ThinkingStrategyId::Effort => &EffortStrategy,
        ThinkingStrategyId::OpenRouter => &OpenRouterStrategy,
        ThinkingStrategyId::Kimi => &KimiStrategy,
        ThinkingStrategyId::OpenaiEffort => &OpenaiEffortStrategy,
        ThinkingStrategyId::Off | ThinkingStrategyId::Auto => &OffStrategy,
        ThinkingStrategyId::Custom => &CustomStrategy,
    }
}

/// Apply the active provider's thinking strategy onto `m`.
pub fn apply_thinking_strategy(
    settings: &ModelSettings,
    provider: &ProviderConfig,
    model: &str,
    model_over: Option<&ModelRuntimeOverrides>,
    m: &mut Map<String, Value>,
) {
    let id = resolve_thinking_strategy_id(provider, model_over);
    if matches!(id, ThinkingStrategyId::Off | ThinkingStrategyId::Custom) {
        strategy_for(id).apply(None, settings, provider, model, model_over, m);
        return;
    }
    let intensity = resolve_thinking_intensity(settings, provider, model_over);
    strategy_for(id).apply(intensity, settings, provider, model, model_over, m);
}

/// Force thinking off using the active strategy (summary / structured helpers).
pub fn apply_thinking_disabled_strategy(settings: &ModelSettings, extra: &mut Option<Value>) {
    let Some((provider, model)) = crate::models::active_provider_and_model(settings) else {
        return;
    };
    let model_over = provider.model_configs.get(model);
    let id = resolve_thinking_strategy_id(provider, model_over);
    if !matches!(extra, Some(Value::Object(_))) {
        if matches!(
            id,
            ThinkingStrategyId::Budget
                | ThinkingStrategyId::Effort
                | ThinkingStrategyId::OpenRouter
                | ThinkingStrategyId::Kimi
                | ThinkingStrategyId::OpenaiEffort
        ) {
            *extra = Some(Value::Object(Map::new()));
        } else if extra.is_none() {
            return;
        }
    }
    let Some(Value::Object(m)) = extra.as_mut() else {
        return;
    };
    strategy_for(id).apply_disabled(m);
}

/// Whether wire extra body currently enables thinking.
pub fn thinking_enabled_in_extra_body(extra: Option<&Value>) -> bool {
    let Some(eb) = extra else {
        return false;
    };
    if eb.get("enable_thinking").and_then(|v| v.as_bool()) == Some(true) {
        return true;
    }
    if matches!(
        eb.pointer("/thinking/type").and_then(|v| v.as_str()),
        Some("enabled")
    ) {
        return true;
    }
    if eb.pointer("/reasoning/enabled").and_then(|v| v.as_bool()) == Some(true) {
        return true;
    }
    if let Some(effort) = eb.get("reasoning_effort").and_then(|v| v.as_str()) {
        let e = effort.trim().to_ascii_lowercase();
        if !e.is_empty() && e != "none" && e != "off" {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::sample_settings;

    #[test]
    fn deepseek_effort_is_reasoning_effort_only() {
        let mut s = sample_settings();
        s.active_provider_id = "deepseek".into();
        s.model = "deepseek-v4-flash".into();
        s.providers[1].thinking_intensity = Some("medium".into());
        let p = &s.providers[1];
        let mut m = Map::new();
        apply_thinking_strategy(&s, p, "deepseek-v4-flash", None, &mut m);
        assert_eq!(
            m.get("reasoning_effort").and_then(|v| v.as_str()),
            Some("high")
        );
        assert!(m.get("thinking").is_none());
        assert!(m.get("output_config").is_none());
    }

    #[test]
    fn auto_unknown_provider_uses_openai_effort() {
        let mut s = sample_settings();
        s.providers.push(ProviderConfig {
            id: "local".into(),
            name: "Local".into(),
            base_url: "http://127.0.0.1:8000/v1".into(),
            api_key: String::new(),
            models: vec!["my-model".into()],
            reasoning_in_messages: None,
            temperature: None,
            max_tokens: None,
            model_configs: Default::default(),
            enable_thinking: None,
            thinking_budget: None,
            reasoning_effort: None,
            thinking_protocol: None,
            thinking_intensity: Some("high".into()),
            extra_body: None,
            source: None,
        });
        let p = s.providers.last().unwrap();
        assert_eq!(
            resolve_thinking_strategy_id(p, None),
            ThinkingStrategyId::OpenaiEffort
        );
        let mut m = Map::new();
        apply_thinking_strategy(&s, p, "my-model", None, &mut m);
        assert_eq!(
            m.get("reasoning_effort").and_then(|v| v.as_str()),
            Some("high")
        );
    }

    #[test]
    fn locked_mapping_ignores_provider_model_intensity() {
        let mut s = sample_settings();
        s.providers[0].thinking_intensity = Some("max".into());
        s.round_thinking_locked = true;
        s.round_thinking_intensity = Some("low".into());
        s.round_enable_thinking = Some(true);
        let p = &s.providers[0];
        assert_eq!(
            resolve_thinking_intensity(&s, p, None),
            Some(ThinkingIntensity::Low)
        );

        s.round_thinking_intensity = None;
        s.round_enable_thinking = None;
        s.round_thinking_budget = None;
        assert_eq!(resolve_thinking_intensity(&s, p, None), None);
        let mut m = Map::new();
        apply_thinking_strategy(&s, p, "qwen-plus", None, &mut m);
        assert!(m.get("enable_thinking").is_none());
        assert!(m.get("thinking_budget").is_none());
    }

    #[test]
    fn auto_picks_budget_for_qwen() {
        let s = sample_settings();
        let p = &s.providers[0];
        assert_eq!(
            resolve_thinking_strategy_id(p, None),
            ThinkingStrategyId::Budget
        );
    }

    #[test]
    fn apply_tier_thinking_to_round_stamps_selected_intensity() {
        let mut s = sample_settings();
        apply_tier_thinking_to_round(
            &mut s,
            &ComputerTierLlmConfig {
                provider_id: "qwen".into(),
                model: "qwen-plus".into(),
                enable_thinking: true,
                thinking_budget: Some(4096),
                reasoning_effort: None,
                thinking_intensity: Some("high".into()),
            },
        );
        assert!(s.round_thinking_locked);
        assert_eq!(s.round_thinking_intensity.as_deref(), Some("high"));
        assert_eq!(s.round_enable_thinking, Some(true));
    }

    #[test]
    fn openrouter_strategy_emits_reasoning_object() {
        let mut s = sample_settings();
        s.providers.push(ProviderConfig {
            id: "openrouter".into(),
            name: "OpenRouter".into(),
            base_url: "https://openrouter.ai/api/v1".into(),
            api_key: String::new(),
            models: vec!["openai/gpt-5".into()],
            reasoning_in_messages: None,
            temperature: None,
            max_tokens: None,
            model_configs: Default::default(),
            enable_thinking: None,
            thinking_budget: None,
            reasoning_effort: None,
            thinking_protocol: None,
            thinking_intensity: Some("high".into()),
            extra_body: None,
            source: None,
        });
        s.active_provider_id = "openrouter".into();
        s.model = "openai/gpt-5".into();
        let p = s.providers.last().unwrap();
        let mut m = Map::new();
        apply_thinking_strategy(&s, p, "openai/gpt-5", None, &mut m);
        assert_eq!(
            m.get("reasoning")
                .and_then(|v| v.get("effort"))
                .and_then(|v| v.as_str()),
            Some("high")
        );
    }
}
