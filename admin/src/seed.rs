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

    #[test]
    fn tier_profile_models_belong_to_seeded_providers() {
        let c = load_catalog().expect("catalog");
        let seeded: Vec<&str> = c.providers.iter().filter(|p| p.seeded).map(|p| p.slug.as_str()).collect();
        let tier = c.tier_profile.expect("tier");
        let text = tier.payload.to_string();
        for pid in ["qwen", "deepseek", "doubao"] {
            if text.contains(&format!("\"providerId\":\"{pid}\"")) {
                assert!(seeded.contains(&pid), "档位引用了未种子的服务商 {pid}");
            }
        }
        // 档位里引用的模型必须在种子模型里存在，否则客户端拿到空档位
        let all_models: Vec<&str> = c
            .providers
            .iter()
            .flat_map(|p| p.models.iter().map(|m| m.id.as_str()))
            .collect();
        for model in ["qwen3.8-max", "qwen3.8-flash", "qwen3.8-27b"] {
            assert!(all_models.contains(&model), "档位引用了不存在的模型 {model}");
        }
    }
}
