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
pub mod trace;
pub mod truth;
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
    verify_provider: &OpenAIProvider,
    args: &Value,
    cancel: &CancellationToken,
) -> Result<String> {
    let frame = capture_frame(computer_state, conversation_id).await?;
    // `hint_box` is in frame pixels; the size is only used to reject a box that
    // falls entirely outside the capture.
    let requests = parse_requests(args, frame.width(), frame.height())?;
    // Thinking comes from the callers: `locate_providers` applies the locate /
    // locate-verify phase budgets (0 = off, the default). Leaving thinking on
    // lets the verifier spiral into self-doubt when a retry hint says the
    // previous box was wrong (observed: 20-40s per call, thousands of tokens of
    // "maybe the user is wrong" reasoning), so the default stays off.
    let vision = ChatLocateVision::with_verify_provider(provider, verify_provider, cancel.clone());
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

fn parse_requests(args: &Value, w: u32, h: u32) -> Result<Vec<LocateRequest>> {
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
    let goal = args
        .get("goal")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");
    if goal.is_empty() {
        return Err(anyhow!(
            "goal 不能为空。goal 是**顶层**字段，不在 elements[] 里。正确格式：\
             {{\"goal\": \"这次定位要完成什么\", \"elements\": [{{\"target_desc\": \"目标元素\"}}]}}。\
             例如 {{\"goal\": \"查看腾讯控股的财务数据\", \"elements\": [{{\"target_desc\": \"「财务分析」标签\"}}]}}"
        ));
    }

    let mut out = Vec::with_capacity(elements.len());
    for (idx, el) in elements.iter().enumerate() {
        let target_desc = el
            .get("target_desc")
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or("");
        if target_desc.is_empty() {
            return Err(anyhow!("elements[{idx}].target_desc 不能为空"));
        }
        out.push(LocateRequest {
            target_desc: target_desc.to_string(),
            goal: goal.to_string(),
            hint_px: parse_hint(el.get("hint_box"), idx, w, h)?,
        });
    }
    Ok(out)
}

fn parse_hint(raw: Option<&Value>, idx: usize, w: u32, h: u32) -> Result<Option<PxBox>> {
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
    // **Pixels only.** `communication.md` asks the model to locate on a 0–1000 grid and
    // then convert to pixels before answering, so the value arriving here is
    // already in frame pixels.
    //
    // This used to accept both shapes and disambiguate by magnitude ("any
    // coordinate above 1000 must be a pixel"). That heuristic silently corrupted
    // every target in the left/top part of the screen: a keypad button at
    // `[236, 740, 260, 800]` is all under 1000, so it was read as normalized and
    // multiplied by 1.92/1.08 — landing on a different button, which the
    // verifier then correctly rejected. The model's answer was right; the
    // conversion broke it. Unit is a contract question, not a numeric one.
    //
    // History worth keeping in view: an earlier attempt asked for pixels *without*
    // telling the model to reason on a 0–1000 grid first, and that measured
    // 153.5 px median offset / 13.6% hit versus 14.4 px / 64.2% for normalized.
    // The failure was the missing relative frame, not the pixel unit — once the
    // prompt spells out "locate on 1000×1000, then convert", pixels measure
    // 5–8 px median (see `benchmarks/computer_e2e/README.md`).
    let box_px = PxBox {
        x1: v[0],
        y1: v[1],
        x2: v[2],
        y2: v[3],
    };
    if box_px.x2 <= box_px.x1 || box_px.y2 <= box_px.y1 {
        return Err(anyhow!(
            "elements[{idx}].hint_box 宽高必须为正，收到 [{}, {}, {}, {}]",
            v[0],
            v[1],
            v[2],
            v[3]
        ));
    }
    // The hint must be somewhere on the captured frame. Without this, a hint in
    // the wrong *unit* (or in another monitor's space) still parses and becomes
    // a reference box the locator cannot see — the first verify round then
    // rejects a crop that only contains wallpaper, and the retry loop spends its
    // budget fixing a hint that was never about this screen. Rejecting early
    // makes the caller's error visible instead of silent.
    //
    // Intersection, not containment: a target at the very edge is legitimately
    // clipped by the frame, so a box that overlaps but does not fit is fine.
    let frame = (0.0, 0.0, w as f64, h as f64);
    if box_px.x2 <= frame.0 || box_px.x1 >= frame.2 || box_px.y2 <= frame.1 || box_px.y1 >= frame.3
    {
        return Err(anyhow!(
            "elements[{idx}].hint_box 完全在画面外，收到 [{}, {}, {}, {}]，画面 {}×{}",
            v[0],
            v[1],
            v[2],
            v[3],
            w,
            h
        ));
    }
    Ok(Some(box_px))
}

fn render_results(outcomes: &[LocateOutcome]) -> String {
    let results: Vec<Value> = outcomes
        .iter()
        .map(|o| {
            if o.ok {
                // Only the box is exposed. The main model passes it straight to
                // `mouse_click_at` as the grounding hint, so a centre point would
                // throw away the extent it needs.
                let b = o.box_px.unwrap_or(PxBox {
                    x1: 0.0,
                    y1: 0.0,
                    x2: 0.0,
                    y2: 0.0,
                });
                // `what_i_saw` is the verifier's `why`, which already opens with
                // "框内是…". Wrapping it in another "框内是：" produced
                // "框内是：框内是…", so use it verbatim.
                let saw = o.what_i_saw.trim();
                // `rounds > 1` means the first box was rejected and re-located.
                // The main model needs that: a 3-round locate that ended right is
                // less trustworthy than a first-try hit, and it tells the model
                // its description was misleading.
                let summary = if o.rounds > 1 {
                    format!("{saw}（纠正 {} 轮后确认）", o.rounds)
                } else {
                    saw.to_string()
                };
                json!({
                    "status": "located",
                    "target_desc": o.target_desc,
                    "summary": summary,
                    "bbox": [
                        b.x1.round() as i64,
                        b.y1.round() as i64,
                        b.x2.round() as i64,
                        b.y2.round() as i64,
                    ],
                    "confidence": o.confidence.map(|c| c.as_str()),
                    "rounds": o.rounds,
                })
            } else {
                let status = o.reason.map(|r| r.as_str()).unwrap_or("failed");
                // `why` is the verifier's full sentence: what the box actually
                // contained, plus which side the target was on and by how much.
                // `what_i_saw` is the same string (engine.rs assigns both from
                // the verifier's `why`), so appending it again would repeat the
                // sentence verbatim. Only fall back to it when `why` is empty.
                let why = o.why.trim();
                let saw = o.what_i_saw.trim();
                let summary = if !why.is_empty() {
                    why.to_string()
                } else if saw.is_empty() {
                    failure_summary(o.reason)
                } else {
                    format!("{} 画面里看到：{saw}", failure_summary(o.reason))
                };
                json!({
                    "status": status,
                    "target_desc": o.target_desc,
                    "summary": summary,
                    "rounds": o.rounds,
                    // The verifier's own answer to "what should change": move the
                    // box, or rewrite the description. `summary` is four sections
                    // of measurements and buries that; this is the one line the
                    // caller acts on.
                    "failure_reason": o.failure_reason,
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

    /// Test-side shim: `parse_requests` needs the frame size to convert the
    /// 0–1000 `hint_box` into pixels. The assertions below only care about
    /// argument validation, so pin a 1920×1080 frame and keep the call sites
    /// reading like the one-argument version they were written against.
    fn parse_requests(args: &Value) -> Result<Vec<LocateRequest>> {
        super::parse_requests(args, 1920, 1080)
    }

    #[test]
    fn located_result_carries_an_explicit_verdict() {
        let out = LocateOutcome {
            ok: true,
            target_desc: "地址栏右侧的刷新按钮".into(),
            point_px: Some((820.0, 45.0)),
            box_px: Some(PxBox {
                x1: 800.0,
                y1: 35.0,
                x2: 840.0,
                y2: 55.0,
            }),
            confidence: Some(Confidence::High),
            what_i_saw: "圆形箭头图标".into(),
            why: String::new(),
            rounds: 1,
            reason: None,
            reasoning: String::new(),
            failure_reason: String::new(),
        };
        let json: Value = serde_json::from_str(&render_results(&[out])).unwrap();
        let r = &json["results"][0];
        assert_eq!(r["status"], "located");
        assert_eq!(r["target_desc"], "地址栏右侧的刷新按钮");
        // 只给 bbox，不给中心点——主模型把它原样传给 mouse_click_at 当定位种子
        assert_eq!(r["bbox"], serde_json::json!([800, 35, 840, 55]));
        assert!(r.get("x").is_none(), "x 已废弃: {r}");
        assert!(r.get("y").is_none(), "y 已废弃: {r}");
        let summary = r["summary"].as_str().unwrap();
        // what_i_saw 并进 summary，不再单独成字段
        assert!(summary.contains("圆形箭头图标"), "{summary}");
        // 一次就中，不要画蛇添足说「纠正 1 轮」
        assert!(!summary.contains("纠正"), "{summary}");
        assert!(r.get("what_i_saw").is_none(), "{r}");
        // 冗余字段已经删掉
        assert!(r.get("ok").is_none(), "{r}");
        assert!(r.get("box").is_none(), "{r}");
        // rounds 现在是有效信息：主模型据此判断这次定位可不可信
        assert_eq!(r["rounds"], 1, "{r}");
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
        assert!(summary.contains("当前截图中未能可靠确认目标"), "{summary}");
    }

    #[test]
    fn goal_is_required_at_top_level() {
        let args = json!({"elements": [{"target_desc": "搜索框"}]});
        let err = parse_requests(&args).unwrap_err().to_string();
        assert!(err.contains("goal 不能为空"), "{err}");
    }

    #[test]
    fn blank_top_level_goal_is_rejected() {
        let args = json!({"goal": "   ", "elements": [{"target_desc": "搜索框"}]});
        let err = parse_requests(&args).unwrap_err().to_string();
        assert!(err.contains("goal 不能为空"), "{err}");
    }

    #[test]
    fn blank_goal_is_rejected() {
        let args = json!({"elements": [{"target_desc": "搜索框", "goal": "   "}]});
        let err = parse_requests(&args).unwrap_err().to_string();
        assert!(err.contains("goal 不能为空"), "{err}");
    }
    fn goal_is_shared_by_every_element() {
        let args = json!({
            "goal": "在股票页面查询行情",
            "elements": [
                {"target_desc": "搜索框"},
                {"target_desc": "搜索按钮"}
            ]
        });
        let reqs = parse_requests(&args).unwrap();
        assert_eq!(reqs[0].goal, "在股票页面查询行情");
        assert_eq!(reqs[1].goal, "在股票页面查询行情");
    }

    fn el(desc: &str) -> Value {
        json!({ "target_desc": desc, "goal": "点击它" })
    }

    #[test]
    fn parses_one_to_three_elements() {
        let reqs = parse_requests(&json!({"goal":"点击它","elements":[el("保存按钮"), el("地址栏")]})).unwrap();
        assert_eq!(reqs.len(), 2);
        assert_eq!(reqs[0].target_desc, "保存按钮");
        assert!(reqs[0].hint_px.is_none());
    }

    #[test]
    fn rejects_more_than_three_elements_without_truncating() {
        let err = parse_requests(&json!({"goal":"点击它","elements":[el("a"), el("b"), el("c"), el("d")]}))
            .expect_err("four elements must fail");
        let msg = err.to_string();
        assert!(msg.contains("最多 3 项"), "{msg}");
        assert!(msg.contains("收到 4 项"), "{msg}");
    }

    #[test]
    fn rejects_empty_and_missing_elements() {
        assert!(parse_requests(&json!({"goal":"点击它","elements":[]})).is_err());
        assert!(parse_requests(&json!({})).is_err());
    }

    #[test]
    fn rejects_blank_description() {
        assert!(parse_requests(&json!({"goal":"点击它","elements":[{"target_desc":"   "}]})).is_err());
    }

    #[test]
    fn hint_box_is_taken_as_pixels_verbatim() {
        // Regression for the dev-environment failure at 04:21:47 on 2026-10-29.
        //
        // The model reasoned "Button 4 is at x≈247" and answered
        // `bbox: [236, 740, 260, 800]` — correct pixels for a keypad button in
        // the lower-left of the screen. Every value is under 1000, so the old
        // magnitude heuristic read it as normalized and multiplied by 1.92/1.08,
        // landing on `[453, 799, 499, 864]` — the *8th* key. The verifier then
        // truthfully reported "框里是大字 8" and the model spent three rounds
        // chasing a target the code had moved.
        //
        // Pixels are the only accepted unit now, so this must survive untouched.
        let reqs = parse_requests(&json!({
            "goal":"点击它",
            "elements":[{"target_desc":"数字键盘 4","hint_box":[236,740,260,800]}]
        }))
        .expect("pixel hint_box should be accepted");
        let b = reqs[0].hint_px.expect("hint_px present");
        assert_eq!((b.x1, b.y1, b.x2, b.y2), (236.0, 740.0, 260.0, 800.0));
    }

    #[test]
    fn accepts_hint_box_in_screen_pixels() {
        // Pixels are the documented shape (`communication.md` → Locating targets:
        // normalize to decide where, convert to pixels for the answer).
        let reqs = parse_requests(&json!({
            "goal":"点击它",
            "elements":[{"target_desc":"保存按钮","hint_box":[1600,700,1680,740]}]
        }))
        .expect("pixel hint_box should be accepted");
        let b = reqs[0].hint_px.expect("hint_px present");
        assert_eq!((b.x1, b.y1, b.x2, b.y2), (1600.0, 700.0, 1680.0, 740.0));
    }

    #[test]
    fn small_pixel_box_is_not_rescaled() {
        // A box entirely below 1000 is still pixels — a target in the top-left
        // quadrant is legitimately small in both axes. This is the exact shape
        // the old heuristic got wrong.
        let reqs = parse_requests(&json!({
            "goal":"点击它",
            "elements":[{"target_desc":"保存按钮","hint_box":[500,500,520,540]}]
        }))
        .expect("small pixel hint_box should be accepted");
        let b = reqs[0].hint_px.expect("hint_px present");
        assert_eq!((b.x1, b.y1, b.x2, b.y2), (500.0, 500.0, 520.0, 540.0));
    }

    #[test]
    fn rejects_hint_box_off_the_frame() {
        // A box that shares no area with the frame cannot be verified: every
        // crop would contain nothing but wallpaper. Fail loudly instead of
        // burning the retry budget on a hint that was never about this screen.
        let err = parse_requests(&json!({
            "goal":"点击它",
            "elements":[{"target_desc":"保存按钮","hint_box":[2000,1200,2100,1300]}]
        }))
        .expect_err("off-frame hint_box must be rejected");
        assert!(format!("{err}").contains("画面外"), "{err}");
    }

    #[test]
    fn accepts_hint_box_clipped_by_the_frame_edge() {
        // A target at the very edge is legitimately clipped. Intersection is the
        // bar, not containment — `[1900, 1000, 1960, 1100]` starts inside the
        // 1920×1080 frame even though it ends outside.
        let reqs = parse_requests(&json!({
            "goal":"点击它",
            "elements":[{"target_desc":"保存按钮","hint_box":[1900,1000,1960,1100]}]
        }))
        .expect("partially visible hint_box should be accepted");
        let b = reqs[0].hint_px.expect("hint_px present");
        assert_eq!((b.x1, b.y1, b.x2, b.y2), (1900.0, 1000.0, 1960.0, 1100.0));
    }

    #[test]
    fn rejects_malformed_hint_box() {
        assert!(parse_requests(&json!({
            "goal":"点击它",
            "elements":[{"target_desc":"a","hint_box":[1,2,3]}]
        }))
        .is_err());
        assert!(parse_requests(&json!({
            "goal":"点击它",
            "elements":[{"target_desc":"a","hint_box":"800,35,840,55"}]
        }))
        .is_err());
        // Explicit null means "no hint" rather than an error.
        assert!(parse_requests(&json!({
            "goal":"点击它",
            "elements":[{"target_desc":"a","hint_box":null}]
        }))
        .unwrap()[0]
            .hint_px
            .is_none());
    }

    #[test]
    fn serializes_success_and_failure_shapes() {
        let ok = LocateOutcome {
            target_desc: "保存按钮".into(),
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
            why: String::new(),
            rounds: 2,
            reason: None,
            reasoning: String::new(),
            failure_reason: String::new(),
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
        assert_eq!(results[0]["bbox"], serde_json::json!([800, 35, 840, 55]));
        assert_eq!(results[0]["confidence"], "high");
        assert_eq!(results[1]["status"], "not_found");
        assert_eq!(results[1]["target_desc"], "地址栏");
        // Failure items must not carry a box at all.
        assert!(results[1].get("bbox").is_none(), "{}", results[1]);
        assert!(results[1].get("x").is_none());
        assert!(results[1].get("box").is_none());
        // 冗余字段已删：ok / box / reason / what_i_saw / description
        // （rounds 保留，主模型用它判断定位可信度）
        for i in 0..2 {
            for gone in ["ok", "box", "reason", "what_i_saw", "description"] {
                assert!(results[i].get(gone).is_none(), "{gone} in {}", results[i]);
            }
        }
        // rounds 两档都要给：成功 2 轮、失败 1 轮
        assert_eq!(results[0]["rounds"], 2, "{}", results[0]);
        assert_eq!(results[1]["rounds"], 1, "{}", results[1]);
        // 纠正过不止一轮时，summary 要说出来
        let s0 = results[0]["summary"].as_str().unwrap();
        assert!(s0.contains("纠正 2 轮后确认"), "{s0}");
    }
}
