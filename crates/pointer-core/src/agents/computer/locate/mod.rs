//! `locate_element` — L2 定位层：B5 逻辑的 Rust 实现（设计 §6）。
//!
//! ```text
//! ① box = locate(整屏截图, 描述)            ← 有 hint_box 时跳过这一步
//! ② 循环（≤3 轮）crop → verify → refine
//! ③ 输出 box 中心（逻辑像素）
//! ```
//!
//! 坐标空间（§6.2.1）：调用方与返回值都是**逻辑像素**；B5 内部的 0–1000
//! 归一化坐标只在 [`geometry`] / [`engine`] 内部出现，不外泄。

pub mod engine;
pub mod geometry;
pub mod prompt;
pub mod render;
pub mod types;
mod vision;

use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

use crate::agents::computer::state::ComputerState;
use crate::provider::OpenAIProvider;

pub use engine::{locate_elements, locate_one, LocateVision};
pub use geometry::{NormBox, PxBox};
pub use types::{
    Confidence, FailureReason, LocateConfig, LocateOutcome, LocateRequest, MAX_ELEMENTS,
};

pub use render::LocateFrame;
pub use vision::ChatLocateVision;

/// Execute one `locate_element` tool call and return the JSON payload for the model.
///
/// Argument errors (more than [`MAX_ELEMENTS`], missing description, malformed
/// `hint_box`) are returned as `Err` — never silently truncated or defaulted.
pub async fn run_locate_element(
    computer_state: &ComputerState,
    conversation_id: &str,
    provider: &OpenAIProvider,
    args: &Value,
    cancel: &CancellationToken,
) -> Result<String> {
    let requests = parse_requests(args)?;
    let frame = capture_frame(computer_state, conversation_id).await?;
    // B5 runs with `reasoning_effort=none`. Grounding is a visual task, and
    // leaving thinking on lets the verifier spiral into self-doubt when a retry
    // hint says the previous box was wrong (observed: 20-40s per call, and
    // thousands of tokens of "maybe the user is wrong" reasoning).
    let mut locate_settings = provider.settings.clone();
    locate_settings.round_enable_thinking = Some(false);
    let locate_provider = OpenAIProvider::new(locate_settings, provider.api_key.clone());
    let vision = ChatLocateVision::new(&locate_provider, cancel.clone());
    let outcomes =
        locate_elements(&vision, &frame, &requests, &LocateConfig::default(), cancel).await?;
    Ok(render_results(&outcomes))
}

/// The frame the model is already looking at.
///
/// Reuses the pipeline capture rather than taking a new OS screenshot: the main
/// LLM decided to call `locate_element` based on the `[Screen current]` image,
/// so grounding has to run on that same frame. A second capture costs time and,
/// worse, can land on a different moment (animation, loading) and return
/// coordinates for a screen the caller never saw.
///
/// The unmarked JPEG is used on purpose — the synthetic pointer overlay can sit
/// on top of the very target being located.
async fn capture_frame(state: &ComputerState, conversation_id: &str) -> Result<LocateFrame> {
    let cap = state.ensure_pipeline_capture(conversation_id).await?;
    LocateFrame::from_jpeg(&cap.raw_unmarked_jpeg)
}

fn parse_requests(args: &Value) -> Result<Vec<LocateRequest>> {
    let elements = args
        .get("elements")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("缺少 elements 数组"))?;
    if elements.is_empty() {
        return Err(anyhow!("elements 至少需要 1 项"));
    }
    if elements.len() > MAX_ELEMENTS {
        return Err(anyhow!(
            "elements 最多 {MAX_ELEMENTS} 项，收到 {} 项；请拆成多次调用（不做静默截断）",
            elements.len()
        ));
    }

    let mut out = Vec::with_capacity(elements.len());
    for (idx, el) in elements.iter().enumerate() {
        let description = el
            .get("description")
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or("");
        if description.is_empty() {
            return Err(anyhow!("elements[{idx}].description 不能为空"));
        }
        out.push(LocateRequest {
            description: description.to_string(),
            hint_px: parse_hint(el.get("hint_box"), idx)?,
        });
    }
    Ok(out)
}

fn parse_hint(raw: Option<&Value>, idx: usize) -> Result<Option<PxBox>> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    if raw.is_null() {
        return Ok(None);
    }
    let arr = raw.as_array().ok_or_else(|| {
        anyhow!("elements[{idx}].hint_box 必须是 4 个数字的数组 [x1, y1, x2, y2]")
    })?;
    if arr.len() != 4 {
        return Err(anyhow!(
            "elements[{idx}].hint_box 需要 4 个数字 [x1, y1, x2, y2]，收到 {} 个",
            arr.len()
        ));
    }
    let mut v = [0f64; 4];
    for (j, item) in arr.iter().enumerate() {
        v[j] = item
            .as_f64()
            .ok_or_else(|| anyhow!("elements[{idx}].hint_box[{j}] 不是数字"))?;
    }
    Ok(Some(PxBox {
        x1: v[0],
        y1: v[1],
        x2: v[2],
        y2: v[3],
    }))
}

fn render_results(outcomes: &[LocateOutcome]) -> String {
    let results: Vec<Value> = outcomes
        .iter()
        .map(|o| {
            if o.ok {
                let (x, y) = o.point_px.unwrap_or((0.0, 0.0));
                json!({
                    "status": "located",
                    "target_desc": o.description,
                    "summary": format!(
                        "坐标 ({}, {}) 是元素中心，可直接点击。框内是：{}",
                        x.round() as i64,
                        y.round() as i64,
                        o.what_i_saw.trim()
                    ),
                    "x": x.round() as i64,
                    "y": y.round() as i64,
                    "confidence": o.confidence.map(|c| c.as_str()),
                })
            } else {
                let status = o.reason.map(|r| r.as_str()).unwrap_or("failed");
                json!({
                    "status": status,
                    "target_desc": o.description,
                    "summary": format!(
                        "{} 画面里看到：{}",
                        failure_summary(o.reason),
                        o.what_i_saw.trim()
                    ),
                })
            }
        })
        .collect();
    json!({ "results": results }).to_string()
}

/// 失败时只给「下一步」，状态和原因已经在 `status` 里了。
fn failure_summary(reason: Option<FailureReason>) -> String {
    match reason {
        Some(FailureReason::NotFound) => {
            "先滚动、切换页面或等待加载，再重新定位；不要用猜测的坐标点击。".to_string()
        }
        Some(FailureReason::NotVerified) => {
            "换一个更有区分度的描述（加上周边锚点或可见文字）再试。".to_string()
        }
        Some(FailureReason::Unclear) => "先让界面稳定（等加载、关闭弹窗）再试。".to_string(),
        None => "不要用猜测的坐标点击。".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn located_result_carries_an_explicit_verdict() {
        let out = LocateOutcome {
            ok: true,
            description: "地址栏右侧的刷新按钮".into(),
            point_px: Some((820.0, 45.0)),
            box_px: None,
            confidence: Some(Confidence::High),
            what_i_saw: "圆形箭头图标".into(),
            rounds: 1,
            reason: None,
        };
        let json: Value = serde_json::from_str(&render_results(&[out])).unwrap();
        let r = &json["results"][0];
        assert_eq!(r["status"], "located");
        assert_eq!(r["target_desc"], "地址栏右侧的刷新按钮");
        let summary = r["summary"].as_str().unwrap();
        assert!(summary.contains("(820, 45)"), "{summary}");
        assert!(summary.contains("可直接点击"), "{summary}");
        // what_i_saw 并进 summary，不再单独成字段
        assert!(summary.contains("圆形箭头图标"), "{summary}");
        assert!(r.get("what_i_saw").is_none(), "{r}");
        // 冗余字段已经删掉
        assert!(r.get("ok").is_none(), "{r}");
        assert!(r.get("box").is_none(), "{r}");
        assert!(r.get("rounds").is_none(), "{r}");
        assert!(r.get("description").is_none(), "{r}");
    }

    #[test]
    fn failed_result_explains_what_to_do_next() {
        let out = LocateOutcome::failure(
            "『文件』菜单里的『另存为』",
            FailureReason::NotFound,
            "当前截图中未能可靠确认目标".into(),
            1,
        );
        let json: Value = serde_json::from_str(&render_results(&[out])).unwrap();
        let r = &json["results"][0];
        // reason 已经合进 status，不再单独有 reason 字段
        assert_eq!(r["status"], "not_found");
        assert_eq!(r["target_desc"], "『文件』菜单里的『另存为』");
        assert!(r.get("reason").is_none(), "{r}");
        assert!(r.get("ok").is_none(), "{r}");
        assert!(r.get("description").is_none(), "{r}");
        assert!(r.get("what_i_saw").is_none(), "{r}");
        let summary = r["summary"].as_str().unwrap();
        assert!(summary.contains("不要用猜测的坐标点击"), "{summary}");
        assert!(summary.contains("当前截图中未能可靠确认目标"), "{summary}");
    }

    fn el(desc: &str) -> Value {
        json!({ "description": desc })
    }

    #[test]
    fn parses_one_to_three_elements() {
        let reqs = parse_requests(&json!({"elements":[el("保存按钮"), el("地址栏")]})).unwrap();
        assert_eq!(reqs.len(), 2);
        assert_eq!(reqs[0].description, "保存按钮");
        assert!(reqs[0].hint_px.is_none());
    }

    #[test]
    fn rejects_more_than_three_elements_without_truncating() {
        let err = parse_requests(&json!({"elements":[el("a"), el("b"), el("c"), el("d")]}))
            .expect_err("four elements must fail");
        let msg = err.to_string();
        assert!(msg.contains("最多 3 项"), "{msg}");
        assert!(msg.contains("收到 4 项"), "{msg}");
    }

    #[test]
    fn rejects_empty_and_missing_elements() {
        assert!(parse_requests(&json!({"elements":[]})).is_err());
        assert!(parse_requests(&json!({})).is_err());
    }

    #[test]
    fn rejects_blank_description() {
        assert!(parse_requests(&json!({"elements":[{"description":"   "}]})).is_err());
    }

    #[test]
    fn parses_hint_box_pixels() {
        let reqs = parse_requests(&json!({
            "elements":[{"description":"保存按钮","hint_box":[800,35,840,55]}]
        }))
        .unwrap();
        let hint = reqs[0].hint_px.unwrap();
        assert_eq!(
            hint,
            PxBox {
                x1: 800.0,
                y1: 35.0,
                x2: 840.0,
                y2: 55.0
            }
        );
    }

    #[test]
    fn rejects_malformed_hint_box() {
        assert!(parse_requests(&json!({
            "elements":[{"description":"a","hint_box":[1,2,3]}]
        }))
        .is_err());
        assert!(parse_requests(&json!({
            "elements":[{"description":"a","hint_box":"800,35,840,55"}]
        }))
        .is_err());
        // Explicit null means "no hint" rather than an error.
        assert!(parse_requests(&json!({
            "elements":[{"description":"a","hint_box":null}]
        }))
        .unwrap()[0]
            .hint_px
            .is_none());
    }

    #[test]
    fn serializes_success_and_failure_shapes() {
        let ok = LocateOutcome {
            description: "保存按钮".into(),
            ok: true,
            point_px: Some((820.4, 45.2)),
            box_px: Some(PxBox {
                x1: 800.0,
                y1: 35.0,
                x2: 840.0,
                y2: 55.0,
            }),
            confidence: Some(Confidence::High),
            what_i_saw: "蓝色软盘图标".into(),
            rounds: 2,
            reason: None,
        };
        let bad = LocateOutcome::failure(
            "地址栏",
            FailureReason::NotFound,
            "当前截图中未能可靠确认目标".into(),
            1,
        );
        let v: Value = serde_json::from_str(&render_results(&[ok, bad])).unwrap();
        let results = v["results"].as_array().unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0]["status"], "located");
        assert_eq!(results[0]["target_desc"], "保存按钮");
        assert_eq!(results[0]["x"], 820);
        assert_eq!(results[0]["y"], 45);
        assert_eq!(results[0]["confidence"], "high");
        assert_eq!(results[1]["status"], "not_found");
        assert_eq!(results[1]["target_desc"], "地址栏");
        // Failure items must not carry coordinates at all.
        assert!(results[1].get("x").is_none());
        assert!(results[1].get("box").is_none());
        // 冗余字段已删：ok / box / rounds / reason / what_i_saw / description
        for i in 0..2 {
            for gone in ["ok", "box", "rounds", "reason", "what_i_saw", "description"] {
                assert!(results[i].get(gone).is_none(), "{gone} in {}", results[i]);
            }
        }
    }
}
