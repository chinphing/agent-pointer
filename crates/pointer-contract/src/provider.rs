//! 控制面下发的服务商模板与模型条目。

use serde::{Deserialize, Serialize};

/// 平台服务商完整模板（由平台目录下发；客户端据此创建服务商，不再本地内置）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlatformProviderTemplate {
    #[serde(default)]
    pub id: String,
    #[serde(default, rename = "sortOrder", alias = "sort_order")]
    pub sort_order: Option<u32>,
    #[serde(default)]
    pub name: String,
    #[serde(default, rename = "baseUrl", alias = "base_url")]
    pub base_url: String,
    #[serde(default, deserialize_with = "deserialize_platform_models")]
    pub models: Vec<PlatformProviderModel>,
    #[serde(
        default,
        rename = "reasoningInMessages",
        alias = "reasoning_in_messages"
    )]
    pub reasoning_in_messages: Option<bool>,
    #[serde(default, rename = "enableThinking", alias = "enable_thinking")]
    pub enable_thinking: Option<bool>,
    #[serde(default, rename = "thinkingBudget", alias = "thinking_budget")]
    pub thinking_budget: Option<u32>,
    #[serde(default, rename = "reasoningEffort", alias = "reasoning_effort")]
    pub reasoning_effort: Option<String>,
    #[serde(default, rename = "thinkingProtocol", alias = "thinking_protocol")]
    pub thinking_protocol: Option<String>,
    #[serde(default, rename = "thinkingIntensity", alias = "thinking_intensity")]
    pub thinking_intensity: Option<String>,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default, rename = "topP", alias = "top_p")]
    pub top_p: Option<f32>,
    #[serde(default, rename = "maxTokens", alias = "max_tokens")]
    pub max_tokens: Option<u32>,
    #[serde(
        default,
        rename = "contextBudgetTokens",
        alias = "context_budget_tokens"
    )]
    pub context_budget_tokens: Option<u32>,
}

/// 平台模型条目（name + 模型级参数；未设置的字段继承服务商级默认）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlatformProviderModel {
    #[serde(default)]
    pub name: String,
    #[serde(
        default,
        rename = "reasoningInMessages",
        alias = "reasoning_in_messages"
    )]
    pub reasoning_in_messages: Option<bool>,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default, rename = "topP", alias = "top_p")]
    pub top_p: Option<f32>,
    #[serde(default, rename = "maxTokens", alias = "max_tokens")]
    pub max_tokens: Option<u32>,
    #[serde(
        default,
        rename = "contextBudgetTokens",
        alias = "context_budget_tokens"
    )]
    pub context_budget_tokens: Option<u32>,
    #[serde(default, rename = "enableThinking", alias = "enable_thinking")]
    pub enable_thinking: Option<bool>,
    #[serde(default, rename = "thinkingBudget", alias = "thinking_budget")]
    pub thinking_budget: Option<u32>,
    #[serde(default, rename = "reasoningEffort", alias = "reasoning_effort")]
    pub reasoning_effort: Option<String>,
    #[serde(default, rename = "thinkingProtocol", alias = "thinking_protocol")]
    pub thinking_protocol: Option<String>,
    #[serde(default, rename = "thinkingIntensity", alias = "thinking_intensity")]
    pub thinking_intensity: Option<String>,
    #[serde(default, rename = "supportsVision", alias = "supports_vision")]
    pub supports_vision: Option<bool>,
    #[serde(default, rename = "supportsAudio", alias = "supports_audio")]
    pub supports_audio: Option<bool>,
    #[serde(default, rename = "canGenerateImage", alias = "can_generate_image")]
    pub can_generate_image: Option<bool>,
    #[serde(default, rename = "canGenerateVideo", alias = "can_generate_video")]
    pub can_generate_video: Option<bool>,
}

/// models 数组元素兼容纯字符串模型名与 `{name, ...模型级参数}` 对象。
fn deserialize_platform_models<'de, D>(
    deserializer: D,
) -> Result<Vec<PlatformProviderModel>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw: Vec<serde_json::Value> = serde::Deserialize::deserialize(deserializer)?;
    let mut out = Vec::with_capacity(raw.len());
    for value in raw {
        if let Some(name) = value.as_str() {
            let name = name.trim();
            if !name.is_empty() {
                out.push(PlatformProviderModel {
                    name: name.to_string(),
                    ..Default::default()
                });
            }
        } else if let Ok(model) = serde_json::from_value::<PlatformProviderModel>(value) {
            if !model.name.trim().is_empty() {
                out.push(model);
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn models_accept_plain_strings_and_objects() {
        let raw = r#"{
            "id": "qwen",
            "name": "阿里云百炼",
            "baseUrl": "https://example.invalid/v1",
            "models": ["qwen3.5-plus", {"name": "qwen3.5-max", "maxTokens": 8192}]
        }"#;
        let tpl: PlatformProviderTemplate = serde_json::from_str(raw).expect("parse");
        assert_eq!(tpl.models.len(), 2);
        assert_eq!(tpl.models[0].name, "qwen3.5-plus");
        assert_eq!(tpl.models[1].name, "qwen3.5-max");
        assert_eq!(tpl.models[1].max_tokens, Some(8192));
    }

    #[test]
    fn empty_model_names_are_dropped() {
        let raw = r#"{"models": ["", "  ", {"name": ""}, "ok"]}"#;
        let tpl: PlatformProviderTemplate = serde_json::from_str(raw).expect("parse");
        assert_eq!(tpl.models.len(), 1);
        assert_eq!(tpl.models[0].name, "ok");
    }

    #[test]
    fn serializes_camel_case() {
        let tpl = PlatformProviderTemplate {
            id: "qwen".into(),
            base_url: "https://x".into(),
            ..Default::default()
        };
        let json = serde_json::to_string(&tpl).expect("serialize");
        assert!(json.contains("\"baseUrl\":\"https://x\""));
    }
}
