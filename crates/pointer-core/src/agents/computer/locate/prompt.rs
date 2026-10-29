//! Prompts, virtual tool schemas, and response parsing for the B5 loop.
//!
//! Ported from `benchmarks/screenspot_grid/baselines/b5_decoupled.py`
//! (`TOOL_LOCATE` / `TOOL_REPORT` / `prompt_locate` / `PROMPT_VERIFY` /
//! `_parse_box` / `_box_from_text`). The only structural change is that the
//! target description arrives as one natural-language string instead of the
//! benchmark's `{text, feat, ctrl, pos, env}` dict.

use serde_json::{json, Value};

use super::geometry::NormBox;
use super::types::{CenterOnTarget, Confidence, MatchVerdict, PositionMatches, VerifyVerdict};

/// Max characters kept from each verifier free-text field (B5 truncation).
/// Hard cap on any free-text field from the report tool.
///
/// 800: the band-mode `why` is a three-part answer and the model routinely runs
/// past the 150 characters it is asked for. At 200 the comparison sentence was
/// cut off entirely — the log could not show whether it had been made — and at
/// 400 it still was. The cap exists to keep a runaway answer out of the
/// transcript, not to police length; the prompt already does that.
const FIELD_CHARS: usize = 800;

// ---- Prompt text lives in `prompts/*.md`, not here -------------------------
//
// Prompts are read by a model, reviewed by a human, and revised far more often
// than the surrounding Rust. Keeping them in `.md` means a wording change is a
// one-line diff in a file that renders as prose — not a change inside a Rust
// string literal where every `\n` and quote is escaped, and where a stray
// backslash silently changes the prompt the model sees.
//
// `{name}` placeholders are substituted at call time. Braces that must reach the
// model literally do not appear in these files, so a plain `str::replace` is
// enough and no template engine is needed.
const LOCATE_PROMPT: &str = include_str!("prompts/locate.md");
const LOCATE_NEAR: &str = include_str!("prompts/locate_near.md");
const LOCATE_HINT: &str = include_str!("prompts/locate_hint.md");
const LOCATE_PREV_BOX: &str = include_str!("prompts/locate_prev_box.md");
const VERIFY_INTRO_WIDE: &str = include_str!("prompts/verify_intro_wide.md");
const VERIFY_INTRO_CLOSE: &str = include_str!("prompts/verify_intro_close.md");
const VERIFY_BODY: &str = include_str!("prompts/verify_body.md");
const REPORT_SCHEMA: &str = include_str!("prompts/report_schema.json");
const LOCATE_SCHEMA: &str = include_str!("prompts/locate_schema.json");

/// How much larger than the target a box may be before `size.ok` fails.
///
/// One number, one place. It appears in the locator's prompt (how tight to draw),
/// the verifier's prompt (when to reject), and the report schema (the field's
/// meaning) — three surfaces that must never disagree, because a locator told
/// 1.5× and a verifier told 2.0× produces a loop that can never converge. The
/// `.md` and `.json` files carry `{size_max}`; the substitution happens at load.
const BOX_SIZE_MAX: &str = "1.5";

/// B5 `TOOL_LOCATE`, made explicit that coordinates are image-local.
pub fn locate_tool() -> Value {
    serde_json::from_str(LOCATE_SCHEMA).expect("locate_schema.json is embedded and valid")
}

/// B5 `TOOL_REPORT`.
pub fn report_tool() -> Value {
    report_tool_views(false)
}

/// `wide` = the call carries a second image, so `why` must open with the 图二
/// verdict.
///
/// Structured rather than prose: 「先看图二再回图一」as an instruction gets
/// skipped when the model answers without reasoning — measured, the same prompt
/// scored 1/3 without thinking and 2/2 with it. A fixed opening makes the step
/// visible in the output itself, so a skipped check reads as `无需看` rather than
/// silently disappearing.
pub fn report_tool_views(_wide: bool) -> Value {
    let raw = REPORT_SCHEMA.replace("{size_max}", BOX_SIZE_MAX);
    serde_json::from_str(&raw).expect("report_schema.json is embedded and valid")
}

/// Locate prompt. Coordinates are always **image-local** (B5 `coord-mode=rel`),
/// so there is no screen-size arithmetic for the model to do.
///
/// `goal` is deliberately not passed in: it is caller-facing metadata used for
/// the tool card title. Feeding it to the grounder made it pick a same-named
/// element on a different row, because "enter the news page" fits both.
///
/// `prev_box` is the **previous round's box, already converted to this image's
/// 0–1000 scale**. Without it the locator only ever received the box centre as a
/// bare point, so when the verifier said 「收小到只覆盖 I7 一格」 it had to guess
/// where inside the reference box that cell was — and on r8c6 it guessed the
/// neighbouring cell, producing a box exactly one column off while the verifier
/// then read that box back as the right column.
pub fn locate_prompt(
    description: &str,
    hint_px: Option<(f64, f64)>,
    previous_hint: Option<&str>,
    prev_box: Option<(f64, f64, f64, f64)>,
) -> String {
    let near = match hint_px {
        Some((x, y)) => LOCATE_NEAR
            .replace("{x}", &(x.round() as i64).to_string())
            .replace("{y}", &(y.round() as i64).to_string()),
        None => String::new(),
    };
    let prev_box_block = match prev_box {
        Some((x1, y1, x2, y2)) => LOCATE_PREV_BOX
            .replace("{x1}", &(x1.round() as i64).to_string())
            .replace("{y1}", &(y1.round() as i64).to_string())
            .replace("{x2}", &(x2.round() as i64).to_string())
            .replace("{y2}", &(y2.round() as i64).to_string()),
        None => String::new(),
    };
    // `【上一轮的情况】` goes after everything else on purpose: it is the
    // correction for this round, and a correction placed mid-prompt loses to
    // the generic box rule that follows it.
    let hint_block = match previous_hint.map(str::trim).filter(|s| !s.is_empty()) {
        Some(hint) => LOCATE_HINT.replace("{hint}", hint),
        None => String::new(),
    };
    LOCATE_PROMPT
        .replace("{size_max}", BOX_SIZE_MAX)
        .replace("{desc}", description.trim())
        .replace("{near}", &near)
        .replace("{prev_box}", &prev_box_block)
        .replace("{hint_block}", &hint_block)
}

/// Verifier prompt. It sees only the crop and the description — **never** the
/// hint box (§6.3.1), otherwise it would rubber-stamp a wrong candidate.
pub fn verify_prompt(description: &str) -> String {
    verify_prompt_views(description, false)
}

/// `wide` = a second, wider image is attached as 【图二】.
///
/// The two images have different jobs, and the split is stated up front because
/// getting it wrong is silent: a model that judges the box from the wide image
/// will call a 137 px cell's borders aligned to within a few pixels — which is
/// how 「正好压住」 got said about a box 26 px off.
pub fn verify_prompt_views(description: &str, wide: bool) -> String {
    // `{view_ref}` is the one word that differs between the two layouts: with a
    // wide second image the labels live in 图二, without it the model only has
    // 图一. Everything else in `VERIFY_BODY` is shared verbatim, so a run with
    // `verify_mult = 1.0` and a run with `verify_mult = 4.0` differ in exactly
    // one word of prompt plus the second image.
    let (intro, view_ref) = if wide {
        (VERIFY_INTRO_WIDE, "图二")
    } else {
        (VERIFY_INTRO_CLOSE, "这张图")
    };
    VERIFY_BODY
        .replace("{size_max}", BOX_SIZE_MAX)
        .replace("{intro}", intro)
        .replace("{view_ref}", view_ref)
        .replace("{desc}", &format!("· 目标：{}\n", description.trim()))
}

/// Fallback instruction when the model returns no `locate` tool call (B5).
pub const LOCATE_TEXT_FALLBACK: &str = "请只输出一个 JSON 数组 [x1, y1, x2, y2]，不要其他内容。";

fn num(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    }
}

fn box_from_json(v: &Value) -> Option<NormBox> {
    match v {
        Value::String(s) => parse_box_from_text(s),
        Value::Array(items) if items.len() == 4 => {
            let vals: Vec<f64> = items.iter().filter_map(num).collect();
            (vals.len() == 4).then(|| NormBox::new(vals[0], vals[1], vals[2], vals[3]))
        }
        Value::Object(map) => {
            let x1 = map.get("x1").or_else(|| map.get("x0")).and_then(num)?;
            let y1 = map.get("y1").or_else(|| map.get("y0")).and_then(num)?;
            let x2 = map.get("x2").and_then(num)?;
            let y2 = map.get("y2").and_then(num)?;
            Some(NormBox::new(x1, y1, x2, y2))
        }
        _ => None,
    }
}

/// What the `locate` tool answered.
///
/// `NotFound` exists because the schema used to require `bbox`, so a locator
/// looking at a crop that does not contain the target had only two options:
/// invent a box, or fail to parse. On the far-hint fixture the first produced
/// confident wrong boxes and the second aborted the run — both worse than saying
/// 「not in this image」, which the engine can act on by re-rendering wider.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LocateAnswer {
    Found(NormBox),
    NotFound,
}

/// Parse the `locate` tool arguments, tolerating the malformed shapes B5 accepts.
///
/// Returns `None` only when the arguments are unusable — a `found: false` answer
/// is a valid answer, not a parse failure.
pub fn parse_locate_arguments(raw: &str) -> Option<LocateAnswer> {
    let v: Value = serde_json::from_str(raw.trim()).ok()?;
    // An explicit `found: false` wins even if a stray bbox is also present:
    // a model that says 「not here」 and still emits numbers is answering the
    // question, not contradicting itself.
    if matches!(v.get("found").and_then(Value::as_bool), Some(false)) {
        return Some(LocateAnswer::NotFound);
    }
    let bbox = v.get("bbox").or_else(|| v.get("box"))?;
    box_from_json(bbox).map(LocateAnswer::Found)
}

/// Pull the first 4-number bracketed list out of free text (B5 `_box_from_text`).
pub fn parse_box_from_text(text: &str) -> Option<NormBox> {
    for (idx, ch) in text.char_indices() {
        if ch != '[' {
            continue;
        }
        let rest = &text[idx + 1..];
        let Some(end) = rest.find(']') else {
            continue;
        };
        let nums: Vec<f64> = rest[..end]
            .split(',')
            .filter_map(|p| p.trim().parse::<f64>().ok())
            .collect();
        if nums.len() >= 4 {
            return Some(NormBox::new(nums[0], nums[1], nums[2], nums[3]));
        }
    }
    None
}

fn text_field(v: &Value, key: &str) -> String {
    v.get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .chars()
        .take(FIELD_CHARS)
        .collect()
}

/// Parse the `report` tool arguments. Returns `None` when the answer is not
/// trustworthy (unknown `match`, or no `in_box` observation) so the caller
/// retries — exactly like B5's `verifier`.
pub fn parse_verify_arguments(raw: &str) -> Option<VerifyVerdict> {
    let v: Value = serde_json::from_str(raw.trim()).ok()?;

    // Raw arguments, every call: the nested shape is new, and a silent mismatch
    // between schema and parser looks exactly like a model that cannot answer.
    log::info!("verify raw args: {raw}");

    // Three nested objects. Each carries its own verdict and, when the verdict is
    // not a pass, the fact behind it — the two travel together so a `no` cannot
    // arrive without saying what it saw.
    let center_on_target = CenterOnTarget::parse(&text_field(&nested(&v, "center"), "ok"))
        .unwrap_or(CenterOnTarget::Unclear);
    let size_ok = match text_field(&nested(&v, "size"), "ok").to_ascii_lowercase().as_str() {
        "yes" => MatchVerdict::Yes,
        "no" => MatchVerdict::No,
        _ => MatchVerdict::Unclear,
    };
    let position_matches = PositionMatches::parse(&text_field(&nested(&v, "position"), "ok"))
        .unwrap_or(PositionMatches::Unknown);

    let center_remark = text_field(&nested(&v, "center"), "remark");
    let size_remark = text_field(&nested(&v, "size"), "remark");
    let position_remark = text_field(&nested(&v, "position"), "remark");

    // `size = no` without a direction is a dead end: the hint becomes 「框太大」
    // with nothing to act on, and the locator returns the same box next round.
    // Size is loose now (only 「absurdly large」 / 「cut the target」 fail), so a
    // number is no longer required — but *something* has to say which way it is
    // off. Returning `None` retries the verify call; it does not burn a locate
    // round.
    if size_ok == MatchVerdict::No && size_remark.trim().is_empty() {
        log::warn!("verify: size=no but empty remark — retrying");
        return None;
    }

    // A `no` with no fact behind it is equally useless to the next round.
    for (label, verdict, remark) in [
        ("center", center_on_target != CenterOnTarget::Yes, &center_remark),
        (
            "position",
            matches!(position_matches, PositionMatches::No | PositionMatches::Unknown),
            &position_remark,
        ),
    ] {
        if verdict && remark.trim().is_empty() {
            log::warn!("verify: {label} not a pass but remark is empty — retrying");
            return None;
        }
    }

    // The four-section template, enforced on **every** verdict including `yes`.
    //
    // A pass is where the expensive mistake lives: the verifier says yes, the box
    // is one cell off, and nothing downstream can tell. The template makes it read
    // the two images separately and report what it saw in each *before* concluding
    // — 「从图二看到：列标 6」 can be compared against the description, 「位置一致」
    // cannot. Missing sections mean it skipped a step, so the answer is not
    // trustworthy either way and the call is retried.
    if position_matches != PositionMatches::Ignore
        && !position_remark_covers_template(&position_remark)
    {
        log::warn!(
            "verify: position remark is missing template sections — retrying; remark={:?}",
            truncate_for_log(&position_remark)
        );
        return None;
    }

    // Reassembled for the log and for `refine_hint`, which still speaks the
    // four-section language. The fields are the source of truth; this is a view.
    let notes = format!(
        "中心：{} / 大小：{} / 位置：{}",
        or_dash(&center_remark),
        or_dash(&size_remark),
        or_dash(&position_remark)
    );

    log::info!(
        "verify center={center_on_target:?} size={size_ok:?} position={} notes={}",
        position_matches.as_str(),
        notes
    );

    Some(VerifyVerdict {
        center_on_target,
        size_ok,
        position_matches,
        notes,
        confidence: Confidence::parse(&text_field(&v, "confidence")),
        reasoning: String::new(),
        failure_reason: text_field(&v, "failure_reason"),
    })
}

/// Read a nested object, or an empty object when it is absent.
///
/// Absent and empty have to behave the same: both mean 「this question was not
/// answered」, and the `unwrap_or` defaults downstream already treat them that
/// way. Distinguishing them here would only move the same decision twice.
fn nested(v: &Value, key: &str) -> Value {
    v.get(key).cloned().unwrap_or_else(|| json!({}))
}

/// Does a `position.remark` follow the four-section template?
///
/// The sections are the point, not the prose: 「从图二看到：列标 6」 can be checked
/// against a description that says 「第 6 列」, while 「位置一致」 cannot be checked
/// against anything. Requiring all four means the verifier has to look at both
/// images and restate the expectation before it is allowed to conclude.
///
/// The single-image case keeps the same shape — `verify_intro_close.md` tells it to
/// write 「从图二看到：本图没有第二张」 — so this check does not need to know which
/// prompt was used.
fn position_remark_covers_template(remark: &str) -> bool {
    const SECTIONS: [&str; 4] = ["从图一看到", "从图二看到", "预期的目标是", "结论"];
    SECTIONS.iter().all(|s| remark.contains(s))
}

/// Enough of a remark to recognise it in a log line.
fn truncate_for_log(s: &str) -> String {
    const MAX: usize = 120;
    let t = s.trim();
    if t.chars().count() <= MAX {
        return t.to_string();
    }
    let head: String = t.chars().take(MAX).collect();
    format!("{head}…")
}

fn or_dash(s: &str) -> &str {    if s.trim().is_empty() {
        "—"
    } else {
        s.trim()
    }
}

/// Retry hint fed into the next locate round (B5 `hint` construction).
///
/// The verifier's own words are the bulk of it: `notes` is four labelled lines
/// that already say what was seen and what was wrong. Code adds only the list of
/// failed dimensions, because that is the one thing prose does not state
/// unambiguously — and it tells the locator what to change and what to leave
/// alone.
pub fn refine_hint(verdict: &VerifyVerdict) -> String {
    let failed = verdict.failed_checks();
    if failed.is_empty() {
        return verdict.notes.trim().to_string();
    }
    let list = failed.join("、");
    format!(
        "上一轮不合格的维度：{list}。\n{}\n这次只改{list}，其他维度保持不变。",
        verdict.notes.trim()
    )
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locate_prompt_is_image_local_and_carries_hint() {
        let p = locate_prompt(
            "保存按钮",
            None,
            Some("你上次给的框里实际是【取消】，不是目标。"),
            None,
        );
        assert!(p.contains("只按「这张图」给坐标"));
        assert!(p.contains("保存按钮"));
        assert!(p.contains("上一轮的情况"));
        assert!(!p.contains("整屏左边缘"));
    }

    #[test]
    fn locate_prompt_prefers_the_candidate_near_the_hint() {
        let p = locate_prompt("资讯公告", Some((975.0, 675.0)), None, None);
        assert!(p.contains("离 (975, 675) 最近的那个"), "{p}");
        assert!(p.contains("另一行或另一栏的同名元素"), "{p}");
    }

    #[test]
    fn locate_prompt_without_hint_has_no_position_preference() {
        let p = locate_prompt("资讯公告", None, None, None);
        assert!(!p.contains("位置偏好"), "{p}");
    }

    #[test]
    fn locate_prompt_carries_the_previous_box_edges() {
        // The locator used to receive only the box *centre*. Told to shrink onto
        // one cell of a 2×2 reference box it had to guess which cell, and on r8c6
        // it guessed the neighbour — then the verifier read that box back as the
        // right column. The four edges are what make the instruction checkable.
        let p = locate_prompt("数独 I7", None, Some("收小到只覆盖 I7 一格"), Some((100.0, 100.0, 300.0, 300.0)));
        assert!(p.contains("参考框的边界"), "{p}");
        assert!(p.contains("左 100"), "{p}");
        assert!(p.contains("上 100"), "{p}");
        assert!(p.contains("右 300"), "{p}");
        assert!(p.contains("下 300"), "{p}");
    }

    #[test]
    fn locate_prompt_without_a_previous_box_has_no_edge_block() {
        let p = locate_prompt("资讯公告", None, None, None);
        assert!(!p.contains("参考框的边界"), "{p}");
    }

    #[test]
    fn locate_prompt_puts_the_box_edges_before_the_correction() {
        // The correction is the last thing read, so the geometry it refers to
        // has to be established earlier — otherwise 「收小到某一格」 arrives with
        // no frame of reference at all.
        let p = locate_prompt("数独 I7", None, Some("收小到只覆盖 I7 一格"), Some((100.0, 100.0, 300.0, 300.0)));
        let edges = p.find("# 参考框的边界").expect("edge block");
        let correction = p.find("# 上一轮的情况").expect("correction block");
        assert!(edges < correction, "edge block must precede the correction");
    }

    #[test]
    fn verify_prompt_views_differ_only_in_the_intro() {
        // The two layouts share `VERIFY_BODY` verbatim; only the intro block that
        // describes what the images are may differ. That keeps a single-image run
        // and a wide run comparable.
        let wide = verify_prompt_views("数独盘面C3的空格子", true);
        let close = verify_prompt_views("数独盘面C3的空格子", false);

        assert!(wide.contains("图二"), "wide intro missing 图二");
        // The close intro may only mention 图二 in the template instruction that
        // tells it what to write in that section — it must not describe a second
        // image as if one exists.
        assert!(
            !close.contains("范围更大的那张是"),
            "close intro must not describe a second image"
        );
        assert!(
            close.contains("本图没有第二张"),
            "close intro must say what the 图二 section should contain"
        );
        assert!(!wide.contains("{"), "unsubstituted placeholder in wide");
        assert!(!close.contains("{"), "unsubstituted placeholder in close");
        // Both must carry the same body. Anchor on the body's own first heading —
        // the wide intro has its own `## 1.` section, so a bare `## 1.` would
        // match the intro instead of the body.
        let body = |s: &str| s[s.find("## 1. 目标元素描述").expect("body")..].to_string();
        assert_eq!(body(&wide), body(&close));
    }

    #[test]
    fn verify_prompt_keeps_size_loose() {
        let p = verify_prompt("数独盘面C3的空格子");
        // Size is deliberately loose now: the returned coordinate is the box
        // centre, so a box that is a bit large or a bit small changes nothing as
        // long as the centre sits on the target. Only 「absurdly large」 and
        // 「cut the target」 are failures.
        assert!(p.contains("大到离谱"), "{p}");
        assert!(p.contains("小到切掉目标"), "{p}");
        assert!(!p.contains("大得离谱"), "{p}");
    }

    #[test]
    fn verify_prompt_asks_for_a_direction_when_size_fails() {
        let p = verify_prompt("数独盘面C3的空格子");
        assert!(p.contains("size.remark"), "{p}");
        assert!(p.contains("往哪个方向差多少"), "{p}");
    }

    #[test]
    fn verify_prompt_handles_a_target_with_no_outline() {
        let p = verify_prompt("弹窗外的空白区域");
        // 「点击空白处关闭」 has no measurable target size. Without an explicit
        // escape the verifier has to invent one, or the schema rejects the call
        // for a missing field.
        assert!(p.contains("没有轮廓可量时 `size.ok` 填 `yes`"), "{p}");
        assert!(p.contains("空白区域"), "{p}");
    }

    #[test]
    fn verify_prompt_defines_the_centre_mark_as_the_answer() {
        let p = verify_prompt("数独盘面第三行第二列的空格子");
        // The coordinate question is now "is the mark on the target", not "do the
        // four edges line up". The latter is what a generous box always passes.
        assert!(p.contains("红点"), "{p}");
        assert!(p.contains("center.ok"), "{p}");
        assert!(p.contains("目标元素"), "{p}");
    }

    #[test]
    fn verify_prompt_separates_position_from_size() {
        let p = verify_prompt("数独盘面第三行第二列的空格子");
        assert!(p.contains("2.1"), "{p}");
        assert!(p.contains("2.2"), "{p}");
        assert!(p.contains("2.3"), "{p}");
        // Size is judged by the verifier, not measured-and-passed-through — but
        // loosely: only 「absurdly large」 and 「cut the target」 fail it.
        assert!(p.contains("只判两件事"), "{p}");
        assert!(p.contains("size.remark"), "{p}");
    }

    #[test]
    fn verify_prompt_ranks_text_over_colour() {
        let p = verify_prompt("数据按钮");
        assert!(p.contains("外观怎么比"), "{p}");
        assert!(p.contains("**文字**"), "{p}");
        assert!(p.contains("**颜色**"), "{p}");
        // 颜色弱到不该判 no
        assert!(p.contains("不要因为颜色判 no"), "{p}");
    }

    #[test]
    fn verify_prompt_asks_for_a_distance_not_a_box() {
        let p = verify_prompt("资讯公告");
        // 判 no 时要说明标记实际落在哪，代码据此生成提示
        assert!(p.contains("center.remark"), "{p}");
        assert!(p.contains("越出哪一侧") || p.contains("落在哪"), "{p}");
        assert!(p.contains("红点整个落在目标外面"), "{p}");
    }

    #[test]
    fn anchors_and_position_ids_must_agree() {
        // 锚点从「辅助线索」升为「第二道验证」：只对上一条、另一条对不上
        // 时判 no。读行列标会错（一排长得一样的元素最容易数错），而锚点
        // 往往有独特外观，是独立证据。
        let p = verify_prompt("资讯公告");
        assert!(p.contains("两条都要对上"), "{p}");
        assert!(p.contains("矛盾本身就是「参考框错了」的信号"), "{p}");
        assert!(p.contains("只有锚点时"), "{p}");
    }

    #[test]
    fn locate_prompt_does_not_leak_the_goal() {
        // goal 只用于卡片标题，不进定位 prompt——它会让模型在多个同名
        // 元素里挑错行（「进入资讯公告页」两行都成立）。
        let p = locate_prompt("搜索框", None, None, None);
        assert!(!p.contains("用途"), "{p}");
        assert!(p.contains("四条边"), "{p}");
        assert!(p.contains("目标框的中心点要落在目标元素上"), "{p}");
    }

    #[test]
    fn previous_hint_comes_after_the_box_rule() {
        // The correction must be the last thing the model reads. Mid-prompt it
        // loses to the generic box rule that follows it.
        let p = locate_prompt("搜索框", None, Some("上次的框太窄，这次要框完整"), None);
        // Locate by the hint's own text, not by `【上一轮的情况】`: the standing
        // `【上一轮的情况怎么用】` section quotes that marker, so a `find` on the
        // marker lands in the middle of the prompt instead of at the hint.
        let hint_at = p.find("上次的框太窄").expect("hint block missing");
        let rule_at = p.find("四条边").expect("box rule missing");
        assert!(
            hint_at > rule_at,
            "hint must sit after the box rule, not before it:\n{p}"
        );
        // And nothing may follow it.
        let after = &p[hint_at..];
        assert!(
            !after.contains("请调用 locate"),
            "the call instruction must not come after the hint:\n{p}"
        );
    }

    #[test]
    fn verify_prompt_does_not_leak_the_goal() {
        let p = verify_prompt("搜索框");
        assert!(!p.contains("用途"), "{p}");
        assert!(!p.contains("用途优先于外观"), "{p}");
    }

    #[test]
    fn verify_prompt_never_mentions_the_hint_box() {
        let p = verify_prompt("地址栏");
        assert!(p.contains("地址栏"));
        // 校验器看不到 hint_box，prompt 里也不能出现这个词
        assert!(!p.to_ascii_lowercase().contains("hint_box"), "{p}");
    }

    #[test]
    fn parses_bbox_array() {
        let b = parse_locate_arguments(r#"{"bbox":[10,20,30,40]}"#).unwrap();
        assert_eq!(b, LocateAnswer::Found(NormBox::new(10.0, 20.0, 30.0, 40.0)));
    }

    #[test]
    fn parses_malformed_bbox_shapes() {
        // String payload with a JSON array inside.
        let b = parse_locate_arguments(r#"{"bbox":"[100, 200, 300, 400]"}"#).unwrap();
        match b {
            LocateAnswer::Found(b) => assert_eq!(b.x2, 300.0),
            LocateAnswer::NotFound => panic!("expected a box"),
        }
        // Object payload.
        let b = parse_locate_arguments(r#"{"box":{"x1":1,"y1":2,"x2":3,"y2":4}}"#).unwrap();
        assert_eq!(b, LocateAnswer::Found(NormBox::new(1.0, 2.0, 3.0, 4.0)));
        // Out-of-range values are clamped, not rejected.
        let b = parse_locate_arguments(r#"{"bbox":[-50,10,1200,900]}"#).unwrap();
        assert_eq!(b, LocateAnswer::Found(NormBox::new(0.0, 10.0, 1000.0, 900.0)));
        // Garbage.
        assert!(parse_locate_arguments(r#"{"bbox":"nope"}"#).is_none());
        assert!(parse_locate_arguments(r#"{"bbox":[1,2]}"#).is_none());
    }

    #[test]
    fn an_explicit_not_found_is_an_answer_not_a_parse_failure() {
        // The far-hint fixture needs this: on a crop that does not contain the
        // target, 「not here」 is the correct answer and the engine escalates to
        // a wider render. Treating it as a parse failure aborted the run.
        assert_eq!(
            parse_locate_arguments(r#"{"found":false}"#),
            Some(LocateAnswer::NotFound)
        );
        // `found: false` wins even when stray numbers are present — the model
        // answered the question; it is not contradicting itself.
        assert_eq!(
            parse_locate_arguments(r#"{"found":false,"bbox":[10,20,30,40]}"#),
            Some(LocateAnswer::NotFound)
        );
        // `found: true` still needs a box.
        assert!(parse_locate_arguments(r#"{"found":true}"#).is_none());
    }

    #[test]
    fn parses_box_from_free_text() {
        let b = parse_box_from_text("结果：[120, 240, 360, 480] 完毕").unwrap();
        assert_eq!(b, NormBox::new(120.0, 240.0, 360.0, 480.0));
        assert!(parse_box_from_text("没有坐标").is_none());
    }

    #[test]
    fn parses_verify_report() {
        let v = parse_verify_arguments(
            r#"{"center":{"ok":"YES","remark":"标记落在图标正中"},"size":{"ok":"yes"},"position":{"ok":"ignore"}}"#,
        )
        .unwrap();
        assert_eq!(v.center_on_target, CenterOnTarget::Yes);
        assert_eq!(v.size_ok, MatchVerdict::Yes);
        assert_eq!(v.position_matches, PositionMatches::Ignore);
        assert!(v.notes.contains("标记落在图标正中"));
        assert!(v.is_hit());
    }

    #[test]
    fn rejects_a_size_verdict_without_a_remark() {
        // 「框太大」 with nothing else cannot steer the next round: the locator
        // has nothing to act on. Rejecting the verdict retries the verify call
        // instead of burning a locate round on a blind guess. A number is no
        // longer required — size is loose now — but the remark must say which
        // way the box is off.
        assert!(parse_verify_arguments(
            r#"{"center":{"ok":"yes"},"size":{"ok":"no","remark":""},"position":{"ok":"ignore"}}"#
        )
        .is_none());
        assert!(parse_verify_arguments(
            r#"{"center":{"ok":"yes"},"size":{"ok":"no","remark":"约为目标的 3 倍，把相邻两个元素也框了进来"},"position":{"ok":"ignore"}}"#
        )
        .is_some());
        // A direction without a number is enough now.
        assert!(parse_verify_arguments(
            r#"{"center":{"ok":"yes"},"size":{"ok":"no","remark":"右边切掉了目标的三分之一"},"position":{"ok":"ignore"}}"#
        )
        .is_some());
    }

    #[test]
    fn an_omitted_verdict_is_never_yes() {
        // A verifier that did not answer has not answered in the affirmative —
        // and an unanswered check with no remark is rejected outright, because
        // the retry hint would have nothing to say.
        assert!(parse_verify_arguments(r#"{"center":{},"size":{},"position":{}}"#).is_none());
        // Answering with enums but no remarks is still a verdict: `yes` needs no
        // explanation, and `ignore` means the description named no position.
        let v = parse_verify_arguments(
            r#"{"center":{"ok":"yes"},"size":{"ok":"yes"},"position":{"ok":"ignore"}}"#,
        )
        .unwrap();
        assert_eq!(v.center_on_target, CenterOnTarget::Yes);
        assert_eq!(v.size_ok, MatchVerdict::Yes);
        assert_eq!(v.position_matches, PositionMatches::Ignore);
        assert!(v.is_hit());
    }

    #[test]
    fn an_off_centre_mark_is_not_a_hit_and_the_hint_names_the_dimension() {
        let v = parse_verify_arguments(
            r#"{"center":{"ok":"no","remark":"标记在地址栏上"},"size":{"ok":"yes"},"position":{"ok":"ignore"}}"#,
        )
        .unwrap();
        assert!(!v.is_hit());
        assert_eq!(v.failed_checks(), vec!["中心定位"]);
        let hint = refine_hint(&v);
        assert!(hint.contains("中心定位"), "{hint}");
        assert!(hint.contains("标记在地址栏上"), "{hint}");
        assert!(hint.contains("这次只改中心定位"), "{hint}");
    }

    #[test]
    fn a_wrong_position_blocks_the_hit_even_when_centre_and_size_pass() {
        let v = parse_verify_arguments(
            r#"{"center":{"ok":"yes"},"size":{"ok":"yes"},"position":{"ok":"no","remark":"从图一看到：框住第三行第八列的格子 / 从图二看到：行标读到 C，列标读到 8 / 预期的目标是：第四行第八列 / 结论：差一行，目标在下方"}}"#,
        )
        .unwrap();
        assert!(!v.is_hit());
        assert_eq!(v.failed_checks(), vec!["位置匹配"]);
        let hint = refine_hint(&v);
        assert!(hint.contains("位置匹配"), "{hint}");
        assert!(hint.contains("差一行"), "{hint}");
    }

    #[test]
    fn an_unreadable_position_also_blocks_the_hit() {
        let v = parse_verify_arguments(
            r#"{"center":{"ok":"yes"},"size":{"ok":"yes"},"position":{"ok":"unknown","remark":"从图一看到：框住空白格 / 从图二看到：列标被裁掉，读不出列号 / 预期的目标是：第 6 列 / 结论：读不出绝对位置"}}"#,
        )
        .unwrap();
        assert!(!v.is_hit());
        assert_eq!(v.failed_checks(), vec!["位置匹配"]);
    }

    #[test]
    fn a_hand_wavy_position_remark_is_retried_not_accepted() {
        // The failure this guards: `ok: yes` with a remark that never reports what
        // it read. Nothing downstream can compare 「位置一致」 against 「第 6 列」,
        // so the call is retried rather than counted as a pass.
        let out = parse_verify_arguments(
            r#"{"center":{"ok":"yes"},"size":{"ok":"yes"},"position":{"ok":"yes","remark":"位置一致，锚点成立"}}"#,
        );
        assert!(out.is_none(), "expected a retry, got {out:?}");
    }

    #[test]
    fn a_box_of_the_wrong_size_blocks_the_hit_without_moving_the_centre() {
        let v = parse_verify_arguments(
            r#"{"center":{"ok":"yes"},"size":{"ok":"no","remark":"约1.8倍，框进左右两个邻格"},"position":{"ok":"ignore"}}"#,
        )
        .unwrap();
        assert!(!v.is_hit());
        assert_eq!(v.failed_checks(), vec!["大小合适"]);
        let hint = refine_hint(&v);
        assert!(hint.contains("大小合适"), "{hint}");
        assert!(hint.contains("这次只改大小合适"), "{hint}");
    }

    #[test]
    fn two_failing_dimensions_are_both_named() {
        let v = parse_verify_arguments(
            r#"{"center":{"ok":"no","remark":"偏左半格"},"size":{"ok":"no","remark":"约2倍"},"position":{"ok":"ignore"}}"#,
        )
        .unwrap();
        assert_eq!(v.failed_checks(), vec!["中心定位", "大小合适"]);
        let hint = refine_hint(&v);
        assert!(hint.contains("中心定位、大小合适"), "{hint}");
    }

    #[test]
    fn a_slack_box_is_named_as_a_size_problem() {
        let v = parse_verify_arguments(
            r#"{"center":{"ok":"yes"},"size":{"ok":"no","remark":"约 2.4 倍，含 B1 和 B2 两格"},"position":{"ok":"ignore"}}"#,
        )
        .unwrap();
        assert_eq!(v.failed_checks(), vec!["大小合适"]);
        let hint = refine_hint(&v);
        assert!(hint.contains("大小合适"), "{hint}");
        assert!(hint.contains("2.4 倍"), "{hint}");
    }

    #[test]
    fn position_remark_needs_all_four_sections() {
        let full = "从图一看到：框住的是空白格 / 从图二看到：列标读到 6 / \
                    预期的目标是：第 6 列、在蓝色 5 右边 / 结论：yes";
        assert!(position_remark_covers_template(full));

        // The real failure this catches: a `yes` whose remark never reports what it
        // read, so nothing downstream can compare it against the description.
        let hand_wavy = "从图一看到：框住一个空格 / 预期的目标是：如描述 / 结论：yes";
        assert!(!position_remark_covers_template(hand_wavy));

        // 「位置一致」 style conclusions have no numbers to check.
        assert!(!position_remark_covers_template("位置一致，锚点成立"));
    }

    #[test]
    fn single_image_remark_still_fills_the_wide_section() {
        // `verify_intro_close.md` tells the model to write 「本图没有第二张」 there, so
        // the same four-section check applies without knowing which prompt ran.
        let single = "从图一看到：框住空白格 / 从图二看到：本图没有第二张，行列标不可见 / \
                      预期的目标是：第 6 列 / 结论：no";
        assert!(position_remark_covers_template(single));
    }

}
