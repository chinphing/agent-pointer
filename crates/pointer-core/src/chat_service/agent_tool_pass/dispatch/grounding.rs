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
/// Build the message the main model sees when grounding fails.
///
/// The three failure shapes need three different responses from the model:
///
/// * `not_found` — the locator never produced a box. Retrying with a different
///   hint is reasonable.
/// * `not_verified` — a box was found and the verifier rejected it. The box or
///   the description is wrong; retrying the same hint will fail again.
/// * `unclear` — the verifier never answered. Nothing was judged, so the model
///   should not "fix" its description over this.
///
/// On top of that, the verifier's own `结论：` line states whether `target_desc`
/// disagrees with the screen. That is the one sentence that tells the model to
/// The verifier's own conclusion sentence, if it wrote one.
fn conclusion_line(summary: &str) -> Option<&str> {
    summary
        .lines()
        .rev()
        .map(str::trim)
        .find(|l| l.starts_with("结论"))
        .map(|l| {
            l.trim_start_matches("结论：")
                .trim_start_matches("结论:")
                .trim()
        })
        .filter(|s| !s.is_empty())
}

/// What `resolve_point` produces: a point, or a structured reason it could not
/// find one.
///
/// Grounding failure used to be `Err(anyhow!("定位失败（…）"))`, which flattened
/// everything the locate pass knew into one sentence and left the caller — and
/// the verify stage after it — matching on a Chinese string prefix. `terminal`
/// already returns `{"success": false, …}`; this brings grounding to the same
/// shape so both can be read as data.
pub(super) enum GroundOutcome {
    /// A confirmed point, plus the box the locator settled on — the box is what
    /// the model needs to judge "did it land where I meant", and it was being
    /// thrown away after the centre was computed.
    Point {
        x: f64,
        y: f64,
        bbox: Value,
    },
    Failed(GroundFailure),
}

/// The result of a successful pointer action, in the same shape as a failure.
///
/// `terminal` reports `{"success": true, …}`; pointer tools used to answer with
/// an English sentence, so a model reading a tool result had one shape on
/// success and another on failure. Both now carry `success`, and the box travels
/// with the point — a bare coordinate says where the click went, not what it hit.
pub(super) fn success_payload(
    action: &str,
    points: &[(f64, f64)],
    boxes: &[Value],
    note: &str,
) -> String {
    let mut payload = serde_json::json!({
        "success": true,
        "action": action,
        "note": note,
    });
    if let Some(obj) = payload.as_object_mut() {
        if points.is_empty() {
            // Scroll-at-current-pointer grounds nothing; there is no point or box
            // to report and inventing an empty one would read as "located at []".
        } else if points.len() == 1 {
            obj.insert("point".into(), serde_json::json!([points[0].0, points[0].1]));
            obj.insert("box".into(), boxes.first().cloned().unwrap_or(Value::Null));
        } else {
            obj.insert("points".into(), serde_json::json!(points));
            obj.insert("boxes".into(), serde_json::json!(boxes));
        }
    }
    payload.to_string()
}

/// Why grounding failed, as the payload the model sees.
#[derive(Debug, Clone)]
pub(super) struct GroundFailure {
    pub status: String,
    pub target_desc: String,
    pub rounds: u64,
    pub bbox: Value,
    pub failure_reason: String,
    pub summary: String,
}

impl GroundFailure {
    /// `success: false` mirrors `terminal`'s result shape, so a caller that
    /// already branches on that field does not need a second convention.
    pub fn to_payload(&self) -> String {
        // The verifier's own sentence is the whole point of the field; fall back
        // to its conclusion line only when it never answered.
        let reason = if self.failure_reason.trim().is_empty() {
            conclusion_line(&self.summary).unwrap_or(self.summary.trim())
        } else {
            self.failure_reason.trim()
        };
        serde_json::json!({
            "success": false,
            "status": self.status,
            "target_desc": self.target_desc,
            "your_bbox": self.bbox,
            "rounds": self.rounds,
            "failure_reason": reason,
        })
        .to_string()
    }
}

pub(super) async fn resolve_point(
    state: &AppState,
    conversation_id: &str,
    provider: &OpenAIProvider,
    args: &Value,
    prefix: &str,
    cancel: &CancellationToken,
) -> anyhow::Result<GroundOutcome> {
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
    // `bbox` is **pixels**, full stop. This value is passed through untouched —
    // `parse_bbox` only validates the shape, and `run_locate_element` →
    // `parse_hint` treats it as pixels with no magnitude guessing and no
    // conversion. A model that answers on a 0–1000 grid lands its hint in the
    // top-left corner; that failure has been diagnosed twice already.
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
        let summary = first
            .get("summary")
            .and_then(|v| v.as_str())
            .unwrap_or("定位失败");
        let rounds = first.get("rounds").and_then(|v| v.as_u64()).unwrap_or(0);
        let failure_reason = first
            .get("failure_reason")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        return Ok(GroundOutcome::Failed(GroundFailure {
            status: status.to_string(),
            target_desc: target_desc.to_string(),
            rounds,
            bbox: bbox_value.clone(),
            failure_reason: failure_reason.to_string(),
            summary: summary.to_string(),
        }));
    }
    let confirmed = first
        .get("bbox")
        .ok_or_else(|| anyhow::anyhow!("locate 结果缺少 bbox"))
        .and_then(parse_bbox)?;
    let (cx, cy) = confirmed.center();
    // Ship the box alongside the centre: the model asked for "the cell with a
    // red 9" and gets back "clicked at (274, 341)" — without the box it cannot
    // tell whether that point is inside the cell it named.
    Ok(GroundOutcome::Point {
        x: cx,
        y: cy,
        bbox: serde_json::json!([
            confirmed.x1.round() as i64,
            confirmed.y1.round() as i64,
            confirmed.x2.round() as i64,
            confirmed.y2.round() as i64,
        ]),
    })
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

    // The dev run at 09:0x on 2026-10-29: the model wrote `target_desc` saying
    // E4 held a red 9, but the red 9 was in E5. It retried the same wrong cell
    // until the round budget ran out, because the old message
    // 「没找到「…」：…」 read as "it exists, click again".
    const E4_E5_SUMMARY: &str = "中心：红点完整落在空白单元格内部 / 大小：大小合适 / 位置：从图一看到：\
框住空白格，四边各贴网格线，左邻空白、右邻红色数字9\n\
从图二看到：列标4/5/6可见，行标E可见；框在E行第4列（E4）；右侧紧邻E5为红色9\n\
预期的目标是：数独盘面E行第四列显示红色数字9的单元格\n\
结论：no，描述要求目标单元格内有红色数字9，但图里E4是空白格，红色9在右侧E5";

    fn failure_for(status: &str, reason: &str) -> GroundFailure {
        GroundFailure {
            status: status.to_string(),
            target_desc: "数独盘面E行第四列显示红色数字9的单元格".to_string(),
            rounds: 3,
            bbox: json!([280, 430, 305, 460]),
            failure_reason: reason.to_string(),
            summary: E4_E5_SUMMARY.to_string(),
        }
    }

    #[test]
    fn success_payload_carries_the_point_and_the_box_it_landed_in() {
        let v: serde_json::Value = serde_json::from_str(&success_payload(
            "mouse_click_at",
            &[(274.0, 341.0)],
            &[json!([253, 320, 295, 363])],
            "Attempted click. Do not assume success.",
        ))
        .unwrap();
        assert_eq!(v["success"], serde_json::json!(true));
        assert_eq!(v["action"], serde_json::json!("mouse_click_at"));
        assert_eq!(v["point"], serde_json::json!([274.0, 341.0]));
        // The box is the part the model was missing: a bare coordinate says
        // where the click went, not what it hit.
        assert_eq!(v["box"], serde_json::json!([253, 320, 295, 363]));
    }

    #[test]
    fn success_payload_keeps_both_ends_of_a_drag() {
        let v: serde_json::Value = serde_json::from_str(&success_payload(
            "mouse_drag_from_to_at",
            &[(10.0, 20.0), (300.0, 400.0)],
            &[json!([5, 15, 15, 25]), json!([295, 395, 305, 405])],
            "Attempted drag.",
        ))
        .unwrap();
        assert_eq!(v["points"], serde_json::json!([[10.0, 20.0], [300.0, 400.0]]));
        assert_eq!(
            v["boxes"],
            serde_json::json!([[5, 15, 15, 25], [295, 395, 305, 405]])
        );
        // A drag must not pretend to have a single point.
        assert!(v.get("point").is_none(), "{v}");
    }

    #[test]
    fn success_payload_omits_geometry_when_nothing_was_grounded() {
        // Scroll-at-current-pointer grounds nothing; an empty `point: []` would
        // read as "located at nowhere".
        let v: serde_json::Value =
            serde_json::from_str(&success_payload("mouse_scroll_current", &[], &[], "Scrolled."))
                .unwrap();
        assert_eq!(v["success"], serde_json::json!(true));
        assert!(v.get("point").is_none(), "{v}");
        assert!(v.get("box").is_none(), "{v}");
    }

    #[test]
    fn payload_marks_the_call_unsuccessful_and_carries_every_field() {
        let v: serde_json::Value = serde_json::from_str(
            &failure_for("not_verified", "描述说框内有红色 9，但框里是空白，红色 9 在右侧相邻格")
                .to_payload(),
        )
        .unwrap();
        // Same discriminator `terminal` uses, so the verify stage can branch on
        // one convention instead of matching message text.
        assert_eq!(v["success"], serde_json::json!(false));
        assert_eq!(v["status"], serde_json::json!("not_verified"));
        assert_eq!(v["rounds"], serde_json::json!(3));
        assert_eq!(v["your_bbox"], serde_json::json!([280, 430, 305, 460]));
        assert_eq!(
            v["failure_reason"],
            serde_json::json!("描述说框内有红色 9，但框里是空白，红色 9 在右侧相邻格")
        );
    }

    #[test]
    fn payload_does_not_ship_the_four_section_diagnostics() {
        // `summary` averages 304 characters of centre/size/position readings. The
        // model cannot act on any of it, and carrying it buried the one sentence
        // that says what to change.
        let payload =
            failure_for("not_verified", "描述说框内有红色 9，但框里是空白").to_payload();
        assert!(!payload.contains("中心："), "{payload}");
        assert!(!payload.contains("从图一看到"), "{payload}");
    }

    #[test]
    fn payload_falls_back_to_the_conclusion_line() {
        // Older prompts have no `failure_reason` field; the verifier's `结论：`
        // line carries the same information, so the payload must not lose it.
        let v: serde_json::Value =
            serde_json::from_str(&failure_for("not_verified", "").to_payload()).unwrap();
        assert_eq!(
            v["failure_reason"],
            serde_json::json!("no，描述要求目标单元格内有红色数字9，但图里E4是空白格，红色9在右侧E5")
        );
    }

    #[test]
    fn payload_keeps_the_status_that_says_nothing_was_judged() {
        // `unclear` means the verifier never answered. The model must not read
        // that as "my box is wrong" and start moving coordinates.
        let v: serde_json::Value = serde_json::from_str(
            &failure_for("unclear", "目标看不清，按钮边缘被选中框盖住").to_payload(),
        )
        .unwrap();
        assert_eq!(v["status"], serde_json::json!("unclear"));
        assert_eq!(v["failure_reason"], serde_json::json!("目标看不清，按钮边缘被选中框盖住"));
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
