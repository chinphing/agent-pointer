//! 服务商种子数据。
//!
//! 目录在 `admin/seed/providers.json`，编译期读进来 —— 前端新建服务商时的
//! 快速选择模板也读同一份，避免两处清单各写各的。
//!
//! 只把 `seeded: true` 的装成企业供应商（qwen / deepseek / 豆包），其余是
//! 「新建服务商时的模板」：选中自动带出地址和模型，管理员只填 key。

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::json;

use crate::db::AdminStore;
use crate::grants::{self, Principal};
use crate::resources::{self, KIND_MODEL, KIND_PROVIDER, KIND_TIER_PROFILE};

const CATALOG: &str = include_str!("../seed/providers.json");

#[derive(Debug, Deserialize)]
pub struct Catalog {
    pub providers: Vec<ProviderPreset>,
    #[serde(default, rename = "tierProfile")]
    pub tier_profile: Option<TierProfileSeed>,
}

#[derive(Debug, Deserialize)]
pub struct ProviderPreset {
    pub slug: String,
    pub name: String,
    #[serde(default)]
    pub seeded: bool,
    #[serde(rename = "baseUrl")]
    pub base_url: String,
    #[serde(default)]
    pub models: Vec<ModelPreset>,
}

#[derive(Debug, Deserialize)]
pub struct ModelPreset {
    pub id: String,
    #[serde(default, rename = "contextBudgetTokens")]
    pub context_budget_tokens: Option<u32>,
    #[serde(default, rename = "maxTokens")]
    pub max_tokens: Option<u32>,
    #[serde(default, rename = "supportsVision")]
    pub supports_vision: bool,
    #[serde(default, rename = "supportsAudio")]
    pub supports_audio: bool,
    #[serde(default, rename = "canGenerateImage")]
    pub can_generate_image: bool,
    #[serde(default, rename = "canGenerateVideo")]
    pub can_generate_video: bool,
}

#[derive(Debug, Deserialize)]
pub struct TierProfileSeed {
    pub slug: String,
    pub name: String,
    pub payload: serde_json::Value,
}

impl ModelPreset {
    /// 只把真的设了的字段写进 payload —— 留空表示「用客户端默认」。
    fn payload(&self) -> serde_json::Value {
        let mut map = serde_json::Map::new();
        if let Some(n) = self.context_budget_tokens {
            map.insert("contextBudgetTokens".into(), json!(n));
        }
        if let Some(n) = self.max_tokens {
            map.insert("maxTokens".into(), json!(n));
        }
        for (key, on) in [
            ("supportsVision", self.supports_vision),
            ("supportsAudio", self.supports_audio),
            ("canGenerateImage", self.can_generate_image),
            ("canGenerateVideo", self.can_generate_video),
        ] {
            if on {
                map.insert(key.into(), json!(true));
            }
        }
        serde_json::Value::Object(map)
    }
}

pub fn load_catalog() -> Result<Catalog> {
    serde_json::from_str(CATALOG).context("解析 seed/providers.json 失败")
}

#[derive(Debug, Default, PartialEq)]
pub struct SeedReport {
    pub providers_created: usize,
    pub providers_skipped: usize,
    pub models_created: usize,
    pub tier_created: bool,
}

/// 幂等：已存在的 slug 一律跳过，不覆盖管理员改过的配置。
pub async fn seed(store: &AdminStore, tenant_id: &str) -> Result<SeedReport> {
    let catalog = load_catalog()?;
    let mut report = SeedReport::default();

    for preset in catalog.providers.iter().filter(|p| p.seeded) {
        let existing =
            resources::find_by_slug(store, tenant_id, KIND_PROVIDER, &preset.slug).await?;
        if existing.is_some() {
            report.providers_skipped += 1;
            continue;
        }

        let payload = json!({ "baseUrl": preset.base_url });
        let provider = resources::create(
            store,
            tenant_id,
            KIND_PROVIDER,
            &preset.slug,
            &preset.name,
            "",
            None,
            &payload.to_string(),
            None,
        )
        .await?;
        // create 里 enabled 写死为 0，不显式启用的话客户端拿不到，种子数据等于白建
        resources::set_enabled(store, &provider.id, true).await?;
        // 不给授权同样拿不到
        grants::replace_grants(store, &provider.id, &[Principal::All]).await?;
        report.providers_created += 1;

        for model in &preset.models {
            let created = resources::create(
                store,
                tenant_id,
                KIND_MODEL,
                &model.id,
                &model.id,
                "",
                Some(&provider.id),
                &model.payload().to_string(),
                None,
            )
            .await;
            match created {
                Ok(m) => {
                    resources::set_enabled(store, &m.id, true).await?;
                    grants::replace_grants(store, &m.id, &[Principal::All]).await?;
                    report.models_created += 1;
                }
                Err(e) => {
                    // 模型 slug 撞车（多个服务商有同名模型）不该中断整批
                    log::warn!("seed: 跳过模型 {}：{e}", model.id);
                }
            }
        }
    }

    if let Some(tier) = &catalog.tier_profile {
        let existing =
            resources::find_by_slug(store, tenant_id, KIND_TIER_PROFILE, &tier.slug).await?;
        if existing.is_none() {
            let created = resources::create(
                store,
                tenant_id,
                KIND_TIER_PROFILE,
                &tier.slug,
                &tier.name,
                "",
                None,
                &tier.payload.to_string(),
                None,
            )
            .await?;
            resources::set_enabled(store, &created.id, true).await?;
            grants::replace_grants(store, &created.id, &[Principal::All]).await?;
            report.tier_created = true;
        }
    }

    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_parses_and_has_three_seeded_providers() {
        let c = load_catalog().expect("catalog");
        let seeded: Vec<&str> = c
            .providers
            .iter()
            .filter(|p| p.seeded)
            .map(|p| p.slug.as_str())
            .collect();
        assert_eq!(seeded, vec!["qwen", "deepseek", "doubao"]);
        assert!(c.tier_profile.is_some());
    }

    #[test]
    fn every_provider_has_base_url_and_models() {
        let c = load_catalog().expect("catalog");
        for p in &c.providers {
            assert!(p.base_url.starts_with("http"), "{} 缺 baseUrl", p.slug);
            assert!(!p.models.is_empty(), "{} 没有模型", p.slug);
        }
    }

    #[test]
    fn model_payload_omits_unset_fields() {
        // 留空表示用客户端默认，不能写 0 或 false
        let m = ModelPreset {
            id: "x".into(),
            context_budget_tokens: None,
            max_tokens: None,
            supports_vision: false,
            supports_audio: false,
            can_generate_image: false,
            can_generate_video: false,
        };
        assert_eq!(m.payload(), json!({}));
    }

    #[test]
    fn model_payload_keeps_set_fields() {
        let m = ModelPreset {
            id: "x".into(),
            context_budget_tokens: Some(1_000_000),
            max_tokens: Some(131_072),
            supports_vision: true,
            supports_audio: false,
            can_generate_image: false,
            can_generate_video: false,
        };
        assert_eq!(
            m.payload(),
            json!({"contextBudgetTokens": 1000000, "maxTokens": 131072, "supportsVision": true})
        );
    }

    /// 档位里引用的每个 `providerId` / `model` 都必须真的在目录里 —— 否则
    /// 客户端拿到一个指向不存在模型的档位，界面显示「已配置」，实际调不通。
    #[test]
    fn tier_profile_references_exist_in_the_catalog() {
        let c = load_catalog().expect("catalog");
        let tier = c.tier_profile.expect("tier");

        let mut refs: Vec<(String, String)> = Vec::new();
        collect_refs(&tier.payload, &mut refs);
        assert!(!refs.is_empty(), "档位里一个模型引用都没有");

        for (provider_id, model_id) in refs {
            let provider = c
                .providers
                .iter()
                .find(|p| p.slug == provider_id)
                .unwrap_or_else(|| panic!("档位引用了目录里没有的服务商：{provider_id}"));
            assert!(
                provider.seeded,
                "档位引用了未种子的服务商 {provider_id} —— 客户端不会下发它"
            );
            assert!(
                provider.models.iter().any(|m| m.id == model_id),
                "档位引用了 {provider_id} 下不存在的模型：{model_id}"
            );
        }
    }

    /// 思考强度只能落在客户端那套阶梯上（`off | low | medium | high | max`）。
    /// 写错一个字母，客户端 `parse` 会静默丢弃，档位看起来配了强度、实际没生效。
    #[test]
    fn tier_profile_thinking_intensity_is_in_the_ladder() {
        let c = load_catalog().expect("catalog");
        let tier = c.tier_profile.expect("tier");
        let allowed = ["off", "low", "medium", "high", "max"];

        let mut values: Vec<String> = Vec::new();
        collect_thinking_intensity(&tier.payload, &mut values);
        assert!(!values.is_empty(), "档位里一个思考强度都没有 —— 截图里明明配了");

        for v in values {
            assert!(allowed.contains(&v.as_str()), "思考强度不在阶梯上：{v}");
        }
    }

    /// 电脑操控流水线按档位下发时，三档 × 三步必须齐全；缺一格客户端就退回
    /// 内置默认，界面上却仍显示平台配置。
    #[test]
    fn computer_pipeline_covers_every_tier_and_step() {
        let c = load_catalog().expect("catalog");
        let tier = c.tier_profile.expect("tier");
        let by_tier = tier
            .payload
            .get("computerPipelineLlm")
            .and_then(|v| v.get("byTier"))
            .and_then(|v| v.as_object())
            .expect("computerPipelineLlm.byTier 缺失");

        for mode in ["fast", "standard", "expert"] {
            let slot = by_tier
                .get(mode)
                .and_then(|v| v.as_object())
                .unwrap_or_else(|| panic!("流水线缺档位：{mode}"));
            for step in ["locate", "verify", "locateVerify"] {
                let entry = slot
                    .get(step)
                    .and_then(|v| v.as_object())
                    .unwrap_or_else(|| panic!("{mode} 档缺步骤：{step}"));
                assert!(
                    entry.get("providerId").and_then(|v| v.as_str()).is_some()
                        && entry.get("model").and_then(|v| v.as_str()).is_some(),
                    "{mode}/{step} 没写全 providerId+model"
                );
            }
        }
    }

    fn collect_thinking_intensity(node: &serde_json::Value, out: &mut Vec<String>) {
        match node {
            serde_json::Value::Object(map) => {
                if let Some(serde_json::Value::String(v)) = map.get("thinkingIntensity") {
                    out.push(v.clone());
                }
                for v in map.values() {
                    collect_thinking_intensity(v, out);
                }
            }
            serde_json::Value::Array(items) => {
                for v in items {
                    collect_thinking_intensity(v, out);
                }
            }
            _ => {}
        }
    }

    fn collect_refs(node: &serde_json::Value, out: &mut Vec<(String, String)>) {
        match node {
            serde_json::Value::Object(map) => {
                if let (Some(serde_json::Value::String(p)), Some(serde_json::Value::String(m))) =
                    (map.get("providerId"), map.get("model"))
                {
                    out.push((p.clone(), m.clone()));
                }
                for v in map.values() {
                    collect_refs(v, out);
                }
            }
            serde_json::Value::Array(items) => {
                for v in items {
                    collect_refs(v, out);
                }
            }
            _ => {}
        }
    }
}
