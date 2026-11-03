//! Prompts, virtual tool schemas, and response parsing for the B5 loop.
//!
//! Ported from `benchmarks/screenspot_grid/baselines/b5_decoupled.py`
//! (`TOOL_LOCATE` / `TOOL_REPORT` / `prompt_locate` / `PROMPT_VERIFY` /
//! `_parse_box` / `_box_from_text`). The only structural change is that the
//! target description arrives as one natural-language string instead of the
//! benchmark's `{text, feat, ctrl, pos, env}` dict.

use serde_json::Value;

use super::geometry::NormBox;
use super::types::{MatchVerdict, VerifyVerdict};

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

    // Raw arguments, every call: a silent mismatch between schema and parser
    // looks exactly like a model that cannot answer.
    log::info!("verify raw args: {raw}");

    // 只有一个判定：十字交点是否落在目标位置上。框的大小不再参与判断 ——
    // 实测里模型把「框」和「目标」当成同一个东西，size 永远报 1.0 倍
    // （small 变体 0.51 倍、big 变体 2.0 倍，它都说 1.0），这一项从来没判对过。
    let is_same = match text_field(&v, "is_same").to_ascii_lowercase().as_str() {
        "yes" => MatchVerdict::Yes,
        "no" => MatchVerdict::No,
        _ => MatchVerdict::Unclear,
    };
    let remark = text_field(&v, "remark");

    // `no` 必须说清差在哪 —— 下一个环节要靠这句话决定往哪挪。
    if is_same != MatchVerdict::Yes && remark.trim().is_empty() {
        log::warn!("verify: is_same={is_same:?} but remark is empty — retrying");
        return None;
    }

    // 两段模板（图一 / 图二），`yes` 也要检查。判 yes 是代价最高的错误：
    // 说对了没人能验，说错了直接交付一个错误点击。模板逼它把两张图的读数
    // 分别写出来 —— 「列标读到 6」能和描述比，「位置一致」不能。
    //
    // remark **不再要求下结论**：结论只有一个出口 `position`，写两遍就会
    // 打架（实测 3.9% 的样本 usable 与 remark 结论不一致，且模型在 reasoning
    // 里从未提到 usable —— 它把那个字段当成填表，不是判断）。
    if !remark_covers_template(&remark) {
        log::warn!(
            "verify: remark is missing template sections — retrying; remark={:?}",
            truncate_for_log(&remark)
        );
        return None;
    }

    log::info!("verify is_same={is_same:?} notes={remark}");

    Some(VerifyVerdict {
        is_same,
        notes: remark.clone(),
        reasoning: String::new(),
    })
}

/// 五项对比是否写出来了。
///
/// 旧版检查的是「图一 / 图二」两段 —— 那是 remark 还按图分段的时代。现在的
/// remark 是五项对比（索引 / 文字 / 图标 / 颜色 / 锚点），实测 74 条里只有 12%
/// 还会写「图一」「图二」，其余 88% 被判「模板不全」白白重试两三次。留着旧检查
/// 等于把 88% 的样本拖进重试，测出来的判对率也是重试后的混合分布。
///
/// 至少三项：要求全写太严（「不涉及」的项目模型常省略），只写一两项说明它
/// 根本没做逐项对比。
fn remark_covers_template(remark: &str) -> bool {
    // 结论 + 一致性表态，缺一不可：只有结论没表态，下游看不出判的是哪边；
    // 只有表态没结论，下游不知道该往哪挪。
    let has_verdict = remark.contains("一致");
    let has_conclusion = remark.contains("结论") || remark.contains("交点");
    has_verdict && has_conclusion
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

    // ---- 判定：只有一个 position，只判十字交点是否落在目标位置上 ----

    // 注意：这段会被 `format!` 直接嵌进 JSON 字符串，换行必须写成 JSON 的 `\n`
    // （Rust 源码里是 `\\n`），裸换行会让 JSON 非法、解析直接失败。
    const OK_REMARK: &str = "结论：交点在C行第6列，描述要C行第6列 —— 一致";

    #[test]
    fn parses_a_pass() {
        let raw = format!(r#"{{"is_same":"yes","remark":"{OK_REMARK}"}}"#);
        let v = parse_verify_arguments(&raw).expect("should parse");
        assert_eq!(v.is_same, MatchVerdict::Yes);
        assert!(v.is_hit());
        // 判定只剩 `is_same` 一个出口。
    }

    #[test]
    fn parses_a_fail() {
        let raw = r#"{"is_same":"no","remark":"结论：交点在B行第8列，描述要A行第8列 —— 索引不一致，差一行"}"#;
        let v = parse_verify_arguments(raw).expect("should parse");
        assert_eq!(v.is_same, MatchVerdict::No);
        assert!(!v.is_hit());
    }

    #[test]
    fn an_omitted_verdict_is_never_yes() {
        let raw = r#"{"remark":"结论：标尺不可见，读不出位置 —— 无法判断是否一致"}"#;
        let v = parse_verify_arguments(raw).expect("should parse");
        assert_eq!(v.is_same, MatchVerdict::Unclear);
        assert!(!v.is_hit());
    }

    #[test]
    fn a_no_without_a_remark_is_retried() {
        let raw = r#"{"is_same":"no","remark":""}"#;
        assert!(parse_verify_arguments(raw).is_none(), "empty remark must retry");
    }

    #[test]
    fn a_remark_missing_a_view_is_retried() {
        // 只有表态、没有结论，下游不知道该往哪挪。
        let raw = r#"{"is_same":"yes","remark":"一致"}"#;
        assert!(parse_verify_arguments(raw).is_none(), "partial template must retry");
    }

    #[test]
    fn template_check_needs_conclusion_and_verdict() {
        // 结论 + 一致性表态，缺一不可。
        assert!(remark_covers_template("结论：交点在B8，描述要B9 —— 不一致"));
        assert!(remark_covers_template("结论：交点在B8，描述要B8 —— 一致"));
        // 只有表态，没有结论。
        assert!(!remark_covers_template("一致"));
        assert!(!remark_covers_template("位置一致"));
        // 只有结论，没有表态。
        assert!(!remark_covers_template("结论：交点在B8"));
        // 旧的「图一/图二」和五项逐条都不再是判据。
        assert!(!remark_covers_template("图一：a / 图二：b"));
        assert!(!remark_covers_template("索引：a\n文字：b\n图标：c"));
    }

    #[test]
    fn remark_is_not_required_to_conclude() {
        // 结论只有一个出口 `position`；remark 写不写 yes/no 都不影响解析。
        let raw = r#"{"is_same":"no","remark":"结论：交点在B8，描述要B9 —— 索引不一致，差一列"}"#;
        let v = parse_verify_arguments(raw).expect("should parse");
        assert_eq!(v.is_same, MatchVerdict::No);
    }

    // ---- 提示词：十字星 + 只判位置 ----

    #[test]
    fn prompt_defines_the_crosshair_intersection_as_the_answer() {
        let p = verify_prompt("数独盘面第三行第二列的空格子");
        assert!(p.contains("十字"), "{p}");
        assert!(p.contains("交点"), "{p}");
        assert!(p.contains("交付的坐标"), "{p}");
        assert!(!p.contains("红点"), "{p}");
    }

    #[test]
    fn prompt_drops_size_entirely() {
        let p = verify_prompt("数独盘面C3的空格子");
        assert!(!p.contains("size.ok"), "{p}");
        assert!(!p.contains("大到离谱"), "{p}");
        assert!(!p.contains("框的大小"), "{p}");
    }

    #[test]
    fn prompt_ranks_text_over_colour() {
        let p = verify_prompt("数据按钮");
        assert!(p.contains("| 颜色 |"), "{p}");
        assert!(p.contains("状态色会变"), "{p}");
        assert!(p.contains("| 文字 |"), "{p}");
    }

    #[test]
    fn prompt_forbids_invented_index_offsets() {
        let p = verify_prompt("数独盘面第八列的空格子");
        assert!(p.contains("不做任何偏移换算"), "{p}");
        assert!(p.contains("一律不许自己加"), "{p}");
    }

    #[test]
    fn prompt_forbids_judging_the_box_instead_of_the_crosshair() {
        // 最小化后正文只保留 schema 里没有的信息，这条是核心之一。
        let p = verify_prompt("资讯公告");
        assert!(p.contains("交点就是交付的坐标"), "{p}");
        // 框已经不再画、也不再提 —— 留着这个词只会把注意力拉回「框在哪」。
        assert!(!p.contains("参考框"), "{p}");
        assert!(!p.contains("框的大"), "{p}");
    }

    #[test]
    fn prompt_requires_the_report_tool() {
        // 少了这句，模型会把整份报告写进正文、tool_calls=0，三次重试全废。
        let p = verify_prompt("资讯公告");
        assert!(p.contains("调用 `report`"), "{p}");
        assert!(p.contains("不要把内容写成正文"), "{p}");
    }

    #[test]
    fn prompt_never_mentions_the_hint_box() {
        let p = verify_prompt("资讯公告");
        assert!(!p.contains("上一轮"), "{p}");
        assert!(!p.contains("hint"), "{p}");
    }

    #[test]
    fn prompt_views_differ_only_in_the_intro() {
        let wide = verify_prompt_views("数独盘面C3的空格子", true);
        let close = verify_prompt_views("数独盘面C3的空格子", false);
        assert!(wide.contains("图二"), "wide intro missing 图二");
        assert!(!close.contains("范围是图一的 4 倍"), "close must not describe a second image");
        assert!(!wide.contains("{"), "unsubstituted placeholder in wide");
        assert!(!close.contains("{"), "unsubstituted placeholder in close");
        let body = |s: &str| s[s.find("## 图里有什么").expect("body")..].to_string();
        assert_eq!(body(&wide), body(&close));
    }
}
