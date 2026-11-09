//! 端到端：控制面真实下发 → 客户端应用 → 档位解析。
//!
//! 夹具 `fixtures/admin_llm_credentials.json` 是 `pointer-admin` 在种子库上
//! `GET /auth/partner/llm-credentials` 的**原样响应**（三家服务商配了假 key），
//! 不是手写样例。控制面改了下发形状、客户端没跟上时，这里会红。

use pointer_core::mode_llm::{resolve_agent_mode_llm_config_for_mode, resolve_media_understanding_model};
use pointer_core::models::ModelSettings;
use pointer_core::platform_auth::PlatformLoginCredentials;
use pointer_core::platform_config::apply_login_credentials_to_model_settings;

const FIXTURE: &str = include_str!("fixtures/admin_llm_credentials.json");

fn apply_fixture() -> ModelSettings {
    let creds: PlatformLoginCredentials =
        serde_json::from_str(FIXTURE).expect("夹具必须能被客户端反序列化");
    let mut settings = ModelSettings::default();
    apply_login_credentials_to_model_settings(&mut settings, &creds);
    settings
}

#[test]
fn payload_creates_the_three_platform_providers() {
    let s = apply_fixture();
    let mut ids: Vec<&str> = s.providers.iter().map(|p| p.id.as_str()).collect();
    ids.sort_unstable();
    assert_eq!(ids, ["deepseek", "doubao", "qwen"], "三家服务商应全部建出来");
}

#[test]
fn media_understanding_resolves_one_model_per_kind() {
    let s = apply_fixture();
    for (kind, provider, model) in [
        ("image", "qwen", "qwen3.5-flash"),
        ("audio", "qwen", "qwen3-asr-flash"),
        ("video", "qwen", "qwen3.5-flash"),
    ] {
        let r = resolve_media_understanding_model(&s, kind);
        assert_eq!(
            (r.provider_id.as_str(), r.model.as_str()),
            (provider, model),
            "{kind} 应解析到 {provider}/{model}"
        );
    }
}

#[test]
fn media_understanding_has_no_tier_layer_left() {
    let s = apply_fixture();
    // 旧形状是 HashMap<类型, HashMap<档位, cfg>>；收敛后外层值的类型不再有档位键。
    // 这里用行为断言：任何档位名都不该被当成媒体类型解析出模型。
    for tier in ["fast", "standard", "expert"] {
        let r = resolve_media_understanding_model(&s, tier);
        assert!(
            r.model.is_empty(),
            "{tier} 不是媒体类型，不该解析出模型（得到 {}）",
            r.model
        );
    }
}

#[test]
fn agent_tiers_carry_thinking_intensity() {
    let s = apply_fixture();

    let general_fast = resolve_agent_mode_llm_config_for_mode(&s, "general", "fast")
        .expect("general/fast 应该有档位配置");
    assert_eq!(general_fast.provider_id, "deepseek");
    assert_eq!(general_fast.model, "deepseek-flash");

    let computer_standard = resolve_agent_mode_llm_config_for_mode(&s, "computer", "standard")
        .expect("computer/standard 应该有档位配置");
    assert_eq!(computer_standard.model, "deepseek-flash");
    assert_eq!(
        computer_standard.thinking_intensity.as_deref(),
        Some("high"),
        "电脑操控标准档的思考强度应下发下来"
    );

    let computer_expert = resolve_agent_mode_llm_config_for_mode(&s, "computer", "expert")
        .expect("computer/expert 应该有档位配置");
    assert_eq!(computer_expert.model, "qwen3.8-flash");
    assert_eq!(computer_expert.thinking_intensity.as_deref(), Some("low"));
}

#[test]
fn computer_pipeline_by_tier_lands_in_settings() {
    let s = apply_fixture();
    let by_tier = &s.computer_pipeline_llm.by_tier;
    let expert = by_tier.get("expert").expect("expert 档流水线应存在");
    assert_eq!(expert.locate, "qwen3.8-flash");
    assert_eq!(expert.verify, "qwen3.8-flash");
    assert_eq!(expert.locate_verify, "qwen3.8-flash");
    assert_eq!(expert.locate_verify_thinking_intensity.as_deref(), Some("low"));

    let standard = by_tier.get("standard").expect("standard 档流水线应存在");
    assert_eq!(standard.locate, "qwen3.8-flash");
    assert_eq!(standard.verify, "deepseek-flash", "标准档的动作校验仍是 deepseek");
}
