//! Shared grounding: provider resolution, `bbox` parsing, `{target_desc, bbox}`
//! → confirmed point.
//!
//! Every pointing tool (`mouse_*`, `input_at`, `modified_click_*`) needs the same
//! two things: a provider pinned to `PipelineLlmPhase::Locate`, and a way to
//! turn "where I think it is + what it is" into a point that has been checked
//! against the screen. Both live here so the call sites stay one line each.
//!
//! Nothing in this module knows about mice or keyboards — it answers "where is
//! this thing", and the caller decides what to do with the answer.

use crate::agents::computer::runtime::PipelineLlmPhase;
use crate::provider::OpenAIProvider;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use super::super::super::app_state::AppState;

/// Providers for one grounding call: the locator and its verifier.
pub(super) struct LocateProviders {
    pub locate: OpenAIProvider,
    /// Equals `locate` unless locate-verify model/provider/thinking is set.
    pub verify: OpenAIProvider,
}

/// Apply locate-verify overrides on top of the locator's settings.
///
/// Empty model / provider and `None` thinking all follow the locator, so an
/// unconfigured verifier is byte-identical to the locator's settings.
fn apply_locate_verify_overrides(
    settings: &mut crate::models::ModelSettings,
    cfg: &crate::agents::computer::runtime::ComputerPipelineLlmConfig,
) {
    let model = cfg.model_locate_verify.trim();
    if !model.is_empty() {
        settings.model = model.to_string();
        let provider_id = cfg.provider_locate_verify.trim();
        if !provider_id.is_empty() {
            settings.active_provider_id = provider_id.to_string();
        }
    }
    // `None` = follow the locator, so an unconfigured verifier stays
    // byte-identical to it; `Some("off")` is an explicit 「no thinking」.
    if let Some(intensity) = cfg.thinking_intensity_locate_verify.as_deref() {
        crate::thinking_strategy::apply_thinking_intensity_to_round(settings, Some(intensity));
    }
}

/// Build the providers used for grounding calls.
///
/// Grounding runs on its own model by default (`pipeline_llm.model_locate`) so
/// the agent's primary model stays free, and the key has to come from whichever
/// provider that resolves to — reusing the agent's key shows up as a 401 from
/// the wrong endpoint.
///
/// The locate verifier can override model / provider / thinking; anything left
/// empty follows the locator, so the default is a single provider.
pub(super) fn locate_providers(
    state: &AppState,
    conversation_id: &str,
    provider: &OpenAIProvider,
) -> anyhow::Result<LocateProviders> {
    let cfg = state.computer_state.effective_pipeline_config();
    let locate_model = cfg.pipeline_llm.model_locate.trim();
    let locate_settings = if locate_model.is_empty() {
        // 模型跟随主模型，但定位的思考强度仍按定位配置（0 = 关）。
        let mut s = provider.settings.clone();
        state
            .computer_state
            .apply_pipeline_thinking(PipelineLlmPhase::Locate, &mut s);
        s
    } else {
        state.computer_state.apply_pipeline_phase_settings(
            conversation_id,
            PipelineLlmPhase::Locate,
            &provider.settings,
        )
    };
    let locate_key = crate::provider::api_key_for_settings(&locate_settings)?;
    let locate = OpenAIProvider::new(locate_settings.clone(), locate_key.clone());

    // 定位校验：模型 / 服务商 / 思考强度都可独立配置，留空跟随定位。
    let mut verify_settings = locate_settings;
    apply_locate_verify_overrides(&mut verify_settings, &cfg.pipeline_llm);
    let verify_key = if verify_settings.active_provider_id.trim()
        == locate.settings.active_provider_id.trim()
    {
        locate_key
    } else {
        crate::provider::api_key_for_settings(&verify_settings)?
    };
    Ok(LocateProviders {
        locate,
        verify: OpenAIProvider::new(verify_settings, verify_key),
    })
}

/// `[x1, y1, x2, y2]` → `PxBox`, rejecting degenerate boxes.
pub(super) fn parse_bbox(v: &Value) -> anyhow::Result<crate::agents::computer::locate::PxBox> {
    let arr = v
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("bbox 必须是 [x1, y1, x2, y2]"))?;
    if arr.len() != 4 {
        anyhow::bail!("bbox 必须是 4 个数，收到 {}", arr.len());
    }
    let mut n = [0f64; 4];
    for (i, item) in arr.iter().enumerate() {
        n[i] = item
            .as_f64()
            .ok_or_else(|| anyhow::anyhow!("bbox[{i}] 不是数字"))?;
    }
    let b = crate::agents::computer::locate::PxBox {
        x1: n[0],
        y1: n[1],
        x2: n[2],
        y2: n[3],
    };
    if b.x2 <= b.x1 || b.y2 <= b.y1 {
        anyhow::bail!(
            "bbox 宽高必须为正：收到 [{}, {}, {}, {}]",
            n[0],
            n[1],
            n[2],
            n[3]
        );
    }
    Ok(b)
}

/// Ground `{<prefix>target_desc, <prefix>bbox}` and return the confirmed centre.
///
/// `prefix` lets one function serve both the single-target tools (`""`) and the
/// two-ended `mouse_drag_from_to_at` (`"from_"` / `"to_"`).
///
/// The returned point is the centre of the box **locate confirmed**, not the
/// centre of the box the caller passed — so a loose hint still lands correctly.
pub(super) async fn resolve_point(
    state: &AppState,
    conversation_id: &str,
    provider: &OpenAIProvider,
    args: &Value,
    prefix: &str,
    cancel: &CancellationToken,
) -> anyhow::Result<(f64, f64)> {
    let desc_key = format!("{prefix}target_desc");
    let bbox_key = format!("{prefix}bbox");
    let target_desc = args
        .get(&desc_key)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("缺少 {desc_key}——目标是什么"))?;
    let bbox_value = args
        .get(&bbox_key)
        .ok_or_else(|| anyhow::anyhow!("缺少 {bbox_key}——目标大概在哪个范围"))?;
    // The model answers `bbox` in 0–1000 normalized coordinates, so this value
    // is **not** pixels even though `parse_bbox` returns a `PxBox` — it only
    // validates the shape. `run_locate_element` → `parse_hint` does the
    // conversion against the captured frame size.
    let hint = parse_bbox(bbox_value)?;

    let providers = locate_providers(state, conversation_id, provider)?;

    let locate_args = serde_json::json!({
        // `goal` is context for the locate model, not user-visible text. Keep it
        // language-neutral: `target_desc` already carries the screen's language,
        // and prefixing a hardcoded verb produced mixed strings like
        // 「点击Save button」 for English UIs.
        "goal": target_desc,
        "elements": [{
            "target_desc": target_desc,
            "hint_box": [hint.x1, hint.y1, hint.x2, hint.y2],
        }]
    });
    let payload = crate::agents::computer::locate::run_locate_element(
        state.computer_state.as_ref(),
        conversation_id,
        &providers.locate,
        &providers.verify,
        &locate_args,
        cancel,
    )
    .await?;

    let parsed: Value = serde_json::from_str(&payload)
        .map_err(|e| anyhow::anyhow!("locate 返回的不是合法 JSON: {e}"))?;
    let first = parsed
        .get("results")
        .and_then(|r| r.as_array())
        .and_then(|a| a.first())
        .ok_or_else(|| anyhow::anyhow!("locate 没有返回结果"))?;

    let status = first.get("status").and_then(|v| v.as_str()).unwrap_or("");
    if status != "located" {
        // Surface the locate failure verbatim — its `summary` already explains
        // what went wrong in user terms.
        let summary = first
            .get("summary")
            .and_then(|v| v.as_str())
            .unwrap_or("定位失败");
        anyhow::bail!("没找到「{target_desc}」：{summary}");
    }
    let confirmed = first
        .get("bbox")
        .ok_or_else(|| anyhow::anyhow!("locate 结果缺少 bbox"))
        .and_then(parse_bbox)?;
    let (cx, cy) = confirmed.center();
    Ok((cx, cy))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_bbox_rejects_bad_shapes() {
        assert!(parse_bbox(&json!([10, 20, 30, 40])).is_ok());
        assert!(parse_bbox(&json!([10, 20, 30])).is_err(), "3 个数");
        assert!(parse_bbox(&json!([10, 20, 30, 40, 50])).is_err(), "5 个数");
        assert!(parse_bbox(&json!("10,20,30,40")).is_err(), "字符串");
        assert!(parse_bbox(&json!([30, 20, 10, 40])).is_err(), "x2 < x1");
        assert!(parse_bbox(&json!([10, 40, 30, 20])).is_err(), "y2 < y1");
        assert!(parse_bbox(&json!([10, 20, 10, 40])).is_err(), "零宽");
    }

    #[test]
    fn parse_bbox_keeps_the_centre() {
        let b = parse_bbox(&json!([100, 200, 300, 260])).unwrap();
        assert_eq!(b.center(), (200.0, 230.0));
    }

    #[test]
    fn locate_verify_overrides_follow_locate_when_unset() {
        let mut settings = crate::models::ModelSettings::default();
        settings.model = "locate-model".into();
        settings.active_provider_id = "qwen".into();
        settings.round_enable_thinking = Some(false);
        settings.round_thinking_budget = Some(0);

        let cfg = crate::agents::computer::runtime::ComputerPipelineLlmConfig::default();
        apply_locate_verify_overrides(&mut settings, &cfg);
        assert_eq!(settings.model, "locate-model");
        assert_eq!(settings.active_provider_id, "qwen");
        assert_eq!(settings.round_enable_thinking, Some(false));
        assert_eq!(settings.round_thinking_budget, Some(0));

        let mut cfg = crate::agents::computer::runtime::ComputerPipelineLlmConfig::default();
        cfg.model_locate_verify = "verify-model".into();
        cfg.provider_locate_verify = "deepseek".into();
        cfg.thinking_intensity_locate_verify = Some("high".into());
        apply_locate_verify_overrides(&mut settings, &cfg);
        assert_eq!(settings.model, "verify-model");
        assert_eq!(settings.active_provider_id, "deepseek");
        assert_eq!(settings.round_enable_thinking, Some(true));
        assert_eq!(settings.round_thinking_budget, Some(4096));

        // `off` = 显式关闭。
        let mut cfg = crate::agents::computer::runtime::ComputerPipelineLlmConfig::default();
        cfg.thinking_intensity_locate_verify = Some("off".into());
        apply_locate_verify_overrides(&mut settings, &cfg);
        assert_eq!(settings.round_enable_thinking, Some(false));
        assert_eq!(settings.round_thinking_budget, None);
    }

    #[test]
    fn module_is_wired() {
        // Compilation is the assertion: the signatures must stay callable from
        // locate.rs, mouse.rs, input.rs and modified_click.rs.
        let _ = (locate_providers, resolve_point);
    }
}
