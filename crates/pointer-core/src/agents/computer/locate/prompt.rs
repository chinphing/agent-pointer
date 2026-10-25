//! Prompts, virtual tool schemas, and response parsing for the B5 loop.
//!
//! Ported from `benchmarks/screenspot_grid/baselines/b5_decoupled.py`
//! (`TOOL_LOCATE` / `TOOL_REPORT` / `prompt_locate` / `PROMPT_VERIFY` /
//! `_parse_box` / `_box_from_text`). The only structural change is that the
//! target description arrives as one natural-language string instead of the
//! benchmark's `{text, feat, ctrl, pos, env}` dict.

use serde_json::{json, Value};

use super::geometry::NormBox;
use super::types::{Confidence, ElsewhereDir, MatchVerdict, UnclearKind, VerifyVerdict};

/// Max characters kept from each verifier free-text field (B5 truncation).
const FIELD_CHARS: usize = 200;

/// B5 `TOOL_LOCATE`, made explicit that coordinates are image-local.
pub fn locate_tool() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "locate",
            "description": "给出目标在这张图中的位置，bbox 为 4 个 0-1000 的整数（相对这张图，不是整屏）。",
            "parameters": {
                "type": "object",
                "properties": {
                    "bbox": {
                        "type": "array",
                        "items": {"type": "integer"},
                        "minItems": 4,
                        "maxItems": 4,
                        "description": "目标框 [x1, y1, x2, y2]"
                    }
                },
                "required": ["bbox"]
            }
        }
    })
}

/// B5 `TOOL_REPORT`.
pub fn report_tool() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "report",
            "description": "报告校验结论。先描述蓝框里是什么，再判断它是不是目标。",
            "parameters": {
                "type": "object",
                "properties": {
                    "match": {
                        "type": "string",
                        "enum": ["yes", "no", "unclear"],
                        "description": "蓝框里的东西是不是目标描述的元素"
                    },
                    "confidence": {
                        "type": "string",
                        "enum": ["high", "medium"],
                        "description": "match=yes 时填。文字+方位+图标+颜色四项全中填 high；文字和方位对上、图标或颜色不符填 medium（这仍然算找到了，颜色在 hover/disabled 态会变）"
                    },
                    "why": {
                        "type": "string",
                        "description": "一句话，先说蓝框里实际是什么（文字/图标/控件），再说判定依据。**不超过 40 字**。判 no 时要说清框里缺了目标的哪一部分。例：「框里是空白，目标在框左上约 60 像素」"
                    },
                    "unclear_kind": {
                        "type": "string",
                        "enum": ["", "anchor_out_of_view", "too_small", "too_blurry", "ambiguous", "box_too_tight", "box_too_loose"],
                        "description": "match=unclear 时必填。anchor_out_of_view=锚点不在图内无法核对；too_small=目标太小看不清；too_blurry=图像模糊/被遮挡；ambiguous=描述自相矛盾；box_too_tight=蓝框只框住了目标的一部分，另一部分在框外；box_too_loose=目标完整在框内但框比目标大太多（超过 1.5 倍，紧邻编号格子超过 1.2 倍）"
                    },
                    "target_px": {
                        "type": "array",
                        "items": {"type": "integer"},
                        "minItems": 2,
                        "maxItems": 2,
                        "description": "目标在【这张图】里的实际宽和高，单位像素。先看目标自己的轮廓再填，不要用蓝框的尺寸代替。目标本身是一片空白区域（点空白处关闭、遮罩、画布背景），没有轮廓可量时填 [0, 0]。"
                    },
                    "box_px": {
                        "type": "array",
                        "items": {"type": "integer"},
                        "minItems": 2,
                        "maxItems": 2,
                        "description": "蓝框在【这张图】里的宽和高，单位像素。看框的四条边量出来，不要照抄目标尺寸。"
                    },
                    "elsewhere_dir": {
                        "type": "string",
                        "enum": ["", "left", "right", "up", "down", "inside"],
                        "description": "match=no 且目标在这张图内别处时，填相对蓝框的方向"
                    },
                    "elsewhere_desc": {"type": "string", "description": "那个位置看起来是什么"}
                },
                "required": ["match", "why", "target_px", "box_px"]
            }
        }
    })
}

/// Locate prompt. Coordinates are always **image-local** (B5 `coord-mode=rel`),
/// so there is no screen-size arithmetic for the model to do.
///
/// `goal` is deliberately not passed in: it is caller-facing metadata used for
/// the tool card title. Feeding it to the grounder made it pick a same-named
/// element on a different row, because "enter the news page" fits both.
pub fn locate_prompt(
    description: &str,
    hint_px: Option<(f64, f64)>,
    previous_hint: Option<&str>,
) -> String {
    let head = format!(
        "请找出界面中符合以下描述的元素，给出它的位置。\n\n【目标元素描述】\n· 目标：{}\n",
        description.trim()
    );
    let near = match hint_px {
        Some((x, y)) => format!(
            "【位置偏好】画面里如果有多个符合条件的元素，选**离 ({}, {}) 最近的那个**。\n\
             那是调用方估计的位置，通常八九不离十；远处即使长得一模一样，\n\
             也很可能是另一行或另一栏的同名元素。\n",
            x.round() as i64,
            y.round() as i64
        ),
        None => String::new(),
    };
    let coord = "【坐标要求】只按【这张图】给坐标：横向 0 = 这张图左边缘，1000 = 右边缘；纵向 0 = 上边缘，1000 = 下边缘。\n不需要知道整张屏幕有多大。";
    let tail = "先**完整包住**目标：目标的每段文字、每条轮廓边都要落在框内。只框住一部分不合格——这时要把框放大到装得下整个目标，不要怕松。包住之后可以比目标略大一圈，留点余量没关系；关键是**中心点要落在目标上**——框明显偏向一侧、或者中心跑到目标外面，返回的坐标才会偏。\n不需要像素级精确：框的中心落在元素上、四条边没有明显超出轮廓，就算合格。\n\n请调用 locate(bbox=[x1, y1, x2, y2])。";
    // `【上一轮的情况】` goes after everything else on purpose: it is the
    // correction for this round, and a correction placed mid-prompt loses to
    // the generic box rule that follows it.
    let hint_block = match previous_hint.map(str::trim).filter(|s| !s.is_empty()) {
        Some(hint) => format!("\n\n【上一轮的情况】{hint}"),
        None => String::new(),
    };
    format!("{head}{near}\n{coord}\n{tail}{hint_block}")
}

/// Verifier prompt. It sees only the crop and the description — **never** the
/// hint box (§6.3.1), otherwise it would rubber-stamp a wrong candidate.
pub fn verify_prompt(description: &str) -> String {
    let desc = format!("· 目标：{}\n", description.trim());
    format!(
        "这张图是从屏幕上裁下来的一块（不是整屏），蓝框是候选位置。\n\
**蓝框始终在这张图的正中央**——裁剪时就是以它为中心截的，四周留了余量。\n\
不用去估算框的像素坐标，也不要猜，直接看框里和框外各有什么。\n\n\
【目标元素描述】\n{desc}\
【任务】\n\
① 先看蓝框里实际是什么（文字/图标/控件），写进 why 的开头。\n\
② 再判断蓝框里的这个东西，是不是上面描述的目标元素。\\n\\
③ 最后量两个数：目标的实际宽高填 `target_px`，蓝框的宽高填 `box_px`（都按这张图的像素）。why 里写这两者的倍数。目标本身是一片空白区域（点空白处关闭弹窗、遮罩、画布背景），没有轮廓可量时 `target_px` 填 [0, 0]，why 里说明它是空白区域，不用写倍数。\n\n\
【必须单独核对的一件事：蓝框有没有把目标完整框住】\n\
「蓝框里能看到目标」和「蓝框把目标整个框住了」是两件事，**都要满足才算找到**。\n\
- 目标的**整个轮廓**（文字的全部笔画、按钮的全部边缘、图标的完整形状）\n\
  都落在蓝框里面，**而且框没有超过下面的倍数** → 才算真正找到。\n\
- 目标是**空格子**时，它的轮廓就是**分隔它的四条边线**。框必须包住这四条边线；\\n\\
  只框住格子中央的空白区域不算框住——框里空无一物，没有任何东西能证明\\n\\
  这是这一格而不是隔壁那一格。\\n\\
- 目标只有一部分在蓝框里，另一部分伸到蓝框外面（哪怕只露出一半，\n\
  或者蓝框只压住了目标的一个角）→ 判 **unclear** + `unclear_kind=box_too_tight`，并在 why 里写清：\n\
  框住了目标的哪一部分、目标的哪一侧在框外、**目标大约是框的几倍宽几倍高**。\n\
  倍数比像素可靠——这张图是缩放过的，你说的像素数换算回屏幕会差很远。\n\
- 目标本身是**一片空白区域**（点空白处关闭、遮罩、画布背景），没有轮廓可量时，
  「框太大」「框太小」都不适用：只要框整个落在那片空白里、框住的位置就是要的位置，就算框住了。
- 反过来，**框比目标大太多**也算不合格。太大是算出来的：先看目标在框里的实际长宽，再和框的长宽比——通常框的宽和高都不超过目标的 1.5 倍算合格；目标是紧邻排列的编号格子（表格单元格、数独格、棋盘格、日历格）时降到 1.2 倍，相邻格只隔一个格宽，框松一点就框进隔壁，坐标会整个偏到另一格。超过倍数判 unclear + unclear_kind=box_too_loose，why 里写清框大约是目标的几倍宽几倍高、框里除了目标还有什么。\n\
  下游拿这句话决定是重新找还是把框收小。\n\n\
【周边锚点怎么用】\n\
锚点是用来**区分同名/相似干扰元素**的辅助线索，不是硬约束。\n\
- 锚点和图中有出入，**不单独构成判 no 的理由**。\n\
- 只有当「外观完全符合、但锚点指向另一个明显不同的元素」时才判 no。\n\
- 判断顺序：先看框里是不是目标 → 再看框有没有把目标完整框住 → 最后才考虑锚点矛盾。\n\n\
【判定规则】\n\
1. 文字内容对不上 → 判 **no**，不管其他项多像。\n\
2. 文字对得上，但蓝框没把目标完整框住 —— 蓝框里已是目标的一部分（文字/边框只露出一截）→ 判 **unclear** + `unclear_kind=box_too_tight`；蓝框里完全是别的东西 → 判 **no** + `elsewhere_dir`。why 说清框住了哪部分、\n\
   目标哪一侧在框外、目标大约是框的几倍（见上）。\n\
3. 判 yes 前逐条确认，缺一条都不算：\\n\\
   · 文字对得上；\\n\\
   · 框包住了目标的整个轮廓（空格子 = 四条边线）；\\n\\
   · 框的宽高都没超过目标的 1.5 倍（紧邻排列的编号格子 1.2 倍）。\\n\\
   三条都过才判 **yes**。\n\
4. 外观、锚点有出入但文字和位置都对 → 仍然判 **yes**，把出入写进 why。\n\n\
【外观怎么比】按这个顺序，**不要跳级**：\n\
1. **文字** —— 文字内容是否一致（最重要，几乎决定成败）\n\
2. **图标** —— 图标形状/类型是否一致\n\
3. **颜色** —— 颜色是否一致（**最弱**，按钮在 hover / disabled / 选中态下\n\
   颜色本来就会变，**不要因为颜色判 no**）\n\n\
【注意：蓝框在这张图里居中，是裁剪方式决定的，不能作为位置证据。】\n\
位置关系只能看「目标相对蓝框在画面里的位置」，不能靠蓝框在图中居中这一点推断。\n\n\
【看不清的情况】\n\
- 目标太小看不清 → unclear + too_small\n\
- 图像模糊或被遮挡 → unclear + too_blurry\n\n\
【如果判 unclear】\\n\\
- `box_too_tight`：why 里必须写「目标大约是框的几倍宽几倍高」。\\n\\
- `box_too_loose`：why 里必须写「框大约是目标的几倍宽几倍高」。\\n\\
- 写不出这个数，说明你没量——那就重新量一遍再答。\\n\\n\\
【如果判 no】\n\
- 如果目标就在这张图里、只是蓝框没框住它，填 elsewhere_dir（left/right/up/down/inside）和 elsewhere_desc。\n\
- 如果这张图里根本看不到目标，elsewhere_dir 留空。\n\n\
请调用 report。\n"
    )
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

/// Parse the `locate` tool arguments, tolerating the malformed shapes B5 accepts.
pub fn parse_locate_arguments(raw: &str) -> Option<NormBox> {
    let v: Value = serde_json::from_str(raw.trim()).ok()?;
    let bbox = v.get("bbox").or_else(|| v.get("box"))?;
    box_from_json(bbox)
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

/// `[w, h]` from the report arguments. A `[0, 0]` pair is kept on purpose: the
/// target may be a blank region (click-the-backdrop, a canvas background) with
/// no outline to measure, and dropping it would look identical to a missing
/// field. Negative entries are still rejected.
fn pair_field(v: &Value, key: &str) -> Option<(u32, u32)> {
    let arr = v.get(key)?.as_array()?;
    let mut out = [0u32; 2];
    let mut n = 0;
    for x in arr {
        // `as f64` rather than `as_u64` so negatives are visible: casting -5
        // straight to an integer saturates at 0 and would read as a legitimate
        // 「no outline」 pair.
        let f = x.as_f64()?;
        if !f.is_finite() || f < 0.0 {
            return None;
        }
        if n < 2 {
            out[n] = f.round().min(u32::MAX as f64) as u32;
            n += 1;
        }
    }
    (n == 2).then_some((out[0], out[1]))
}

/// Parse the `report` tool arguments. Returns `None` when the answer is not
/// trustworthy (unknown `match`, or no `in_box` observation) so the caller
/// retries — exactly like B5's `verifier`.
pub fn parse_verify_arguments(raw: &str) -> Option<VerifyVerdict> {
    let v: Value = serde_json::from_str(raw.trim()).ok()?;
    let match_raw = v.get("match").and_then(Value::as_str)?.trim().to_ascii_lowercase();
    let match_ = match match_raw.as_str() {
        "yes" => MatchVerdict::Yes,
        "no" => MatchVerdict::No,
        "unclear" => MatchVerdict::Unclear,
        _ => return None,
    };
    // `why` now carries both "what is in the box" and the verdict reason, so a
    // blank one means the model did not actually look — treat it as unusable
    // rather than accepting a verdict with no observation behind it.
    let why = text_field(&v, "why");
    if why.is_empty() {
        return None;
    }
    let unclear_kind = match text_field(&v, "unclear_kind").to_ascii_lowercase().as_str() {
        "anchor_out_of_view" => UnclearKind::AnchorOutOfView,
        "too_small" => UnclearKind::TooSmall,
        "too_blurry" => UnclearKind::TooBlurry,
        "ambiguous" => UnclearKind::Ambiguous,
        "box_too_tight" => UnclearKind::BoxTooTight,
        "box_too_loose" => UnclearKind::BoxTooLoose,
        _ => UnclearKind::None,
    };
    Some(VerifyVerdict {
        match_,
        why,
        unclear_kind,
        elsewhere_dir: ElsewhereDir::parse(&text_field(&v, "elsewhere_dir")),
        elsewhere_desc: v
            .get("elsewhere_desc")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .chars()
            .take(150)
            .collect(),
        confidence: Confidence::parse(&text_field(&v, "confidence")),
        reasoning: String::new(),
        target_px: pair_field(&v, "target_px"),
        box_px: pair_field(&v, "box_px"),
    })
}

/// Retry hint fed into the next locate round (B5 `hint` construction).
pub fn refine_hint(verdict: &VerifyVerdict) -> String {
    // A box that holds only part of the target is not a wrong place — it is a
    // box that is too small. Telling the locator "look elsewhere" sends it
    // hunting for a different element when all it has to do is widen.
    //
    // The measured ratio is passed through for the same reason as the loose
    // case below: 「框里只有目标的一部分」 alone leaves the locator guessing how
    // much to widen, and it tends to widen by a token amount and fail again.
    if verdict.unclear_kind == UnclearKind::BoxTooTight {
        return format!(
            "上次的框太窄：框里只有目标的一部分（{}）。这次按上面的倍数把框放大到装得下整个目标，位置就在原处附近，不要换地方找。",
            verdict.why.trim()
        );
    }
    // Mirror case: the target is right there, the box is slack around it. The
    // verifier already measured how slack (it is what decided the box failed);
    // pass that number through so the locator shrinks by the right amount
    // instead of guessing. Without it the locator re-frames "somewhere around
    // here" and lands at 2x again — which is exactly what r1c0 did.
    if verdict.unclear_kind == UnclearKind::BoxTooLoose {
        return format!(
            "上次的框太松：目标就在框里，但框比目标大太多（{}）。这次只框住目标本身，不要带进周围的格子或标签，中心位置不变。",
            verdict.why.trim()
        );
    }
    let mut hint = format!("你上次给的框里实际是【{}】，不是目标。", verdict.why.trim());
    match verdict.elsewhere_dir {
        Some(dir) => {
            hint.push_str(&format!(
                "目标可能在框的 {} 方向，{}。",
                dir.as_str(),
                verdict.elsewhere_desc.trim()
            ));
        }
        None => hint.push_str("那一带也看不到目标。"),
    }
    hint
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
        );
        assert!(p.contains("只按【这张图】给坐标"));
        assert!(p.contains("保存按钮"));
        assert!(p.contains("上一轮的情况"));
        assert!(!p.contains("整屏左边缘"));
    }

    #[test]
    fn locate_prompt_prefers_the_candidate_near_the_hint() {
        let p = locate_prompt("资讯公告", Some((975.0, 675.0)), None);
        assert!(p.contains("离 (975, 675) 最近的那个"), "{p}");
        assert!(p.contains("另一行或另一栏的同名元素"), "{p}");
    }

    #[test]
    fn locate_prompt_without_hint_has_no_position_preference() {
        let p = locate_prompt("资讯公告", None, None);
        assert!(!p.contains("位置偏好"), "{p}");
    }

    #[test]
    fn verify_prompt_gives_a_number_for_how_much_slack_is_acceptable() {
        let p = verify_prompt("数独盘面C3的空格子");
        // 「大得离谱」「没有大太多」were unmeasurable — the model spent its whole
        // first round arguing with itself about whether a 3x box counted. The
        // threshold has to be a number it can compare against.
        assert!(p.contains("1.5 倍"), "{p}");
        assert!(p.contains("1.2 倍"), "{p}");
        assert!(!p.contains("大得离谱"), "{p}");
        assert!(!p.contains("没有大太多"), "{p}");
        // The tighter ratio exists because neighbouring cells are one cell apart.
        assert!(p.contains("紧邻排列的编号格子"), "{p}");
    }

    #[test]
    fn verify_prompt_asks_for_a_ratio_on_both_sides_of_the_slack_axis() {
        let p = verify_prompt("数独盘面C3的空格子");
        // Too tight used to ask for 「大概还差多少」 with an example in pixels —
        // the crop is rescaled, so a pixel count from the model does not survive
        // the trip back to screen coordinates. Both directions now report a
        // ratio, and both hints pass it through.
        assert!(p.contains("目标大约是框的几倍宽几倍高"), "{p}");
        assert!(p.contains("框大约是目标的几倍宽几倍高"), "{p}");
        assert!(!p.contains("差 30 像素"), "{p}");
        assert!(!p.contains("大概还差多少"), "{p}");
    }

    #[test]
    fn refine_hint_for_box_too_tight_asks_to_widen_by_the_measured_ratio() {
        let v = parse_verify_arguments(
            r#"{"match":"unclear","why":"只框住文件名前半段，目标宽约框的 2 倍","unclear_kind":"box_too_tight"}"#,
        )
        .unwrap();
        let hint = refine_hint(&v);
        assert!(hint.contains("太窄"), "{hint}");
        assert!(hint.contains("按上面的倍数"), "{hint}");
        assert!(hint.contains("2 倍"), "{hint}");
        assert!(hint.contains("不要换地方找"), "{hint}");
        assert!(!hint.contains("不是目标"), "{hint}");
    }

    #[test]
    fn verify_prompt_handles_a_target_with_no_outline() {
        let p = verify_prompt("弹窗外的空白区域");
        // 「点击空白处关闭」 has no measurable target size. Without an explicit
        // escape the verifier has to invent one, or the schema rejects the call
        // for a missing field.
        assert!(p.contains("没有轮廓可量时 `target_px` 填 [0, 0]"), "{p}");
        assert!(p.contains("「框太大」「框太小」都不适用"), "{p}");
    }

    #[test]
    fn pair_field_keeps_a_zero_pair_but_rejects_negatives() {
        // [0, 0] means 「the target has no outline」 — dropping it would look
        // the same as the model omitting the field.
        let v: Value = serde_json::from_str(r#"{"target_px":[0,0],"box_px":[10,20]}"#).unwrap();
        assert_eq!(pair_field(&v, "target_px"), Some((0, 0)));
        assert_eq!(pair_field(&v, "box_px"), Some((10, 20)));

        let neg: Value = serde_json::from_str(r#"{"target_px":[-5,3]}"#).unwrap();
        assert_eq!(pair_field(&neg, "target_px"), None);
    }

    #[test]
    fn verify_prompt_defines_the_outline_of_an_empty_cell() {
        let p = verify_prompt("数独盘面第三行第二列的空格子");
        // Without this the verifier argued with itself over whether an empty
        // cell even has an outline: 「目标是一个空格子，整个轮廓包括边框线…
        // 嗯，但"一部分"对空格子而言意味着什么？」. A box holding only the blank
        // centre was then passed as complete.
        assert!(p.contains("四条边线"), "{p}");
        assert!(p.contains("只框住格子中央的空白区域不算框住"), "{p}");
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
        // 判 no 时要说明框漏掉了哪部分，因为模型判断不了「中心」这种几何量
        assert!(p.contains("完整框住"), "{p}");
        assert!(p.contains("哪一部分在框外") || p.contains("哪一侧在框外"), "{p}");
        assert!(p.contains("伸到蓝框外面"), "{p}");
    }

    #[test]
    fn verify_prompt_treats_anchors_as_a_hint_not_a_hard_rule() {
        let p = verify_prompt("资讯公告");
        assert!(p.contains("不是硬约束"), "{p}");
        assert!(p.contains("不单独构成判 no 的理由"), "{p}");
    }

    #[test]
    fn locate_prompt_does_not_leak_the_goal() {
        // goal 只用于卡片标题，不进定位 prompt——它会让模型在多个同名
        // 元素里挑错行（「进入资讯公告页」两行都成立）。
        let p = locate_prompt("搜索框", None, None);
        assert!(!p.contains("用途"), "{p}");
        assert!(p.contains("完整包住"), "{p}");
        assert!(p.contains("中心点要落在目标上"), "{p}");
    }

    #[test]
    fn previous_hint_comes_after_the_box_rule() {
        // The correction must be the last thing the model reads. Mid-prompt it
        // loses to the generic box rule that follows it.
        let p = locate_prompt("搜索框", None, Some("上次的框太窄，这次要框完整"));
        let hint_at = p.find("上一轮的情况").expect("hint block missing");
        let rule_at = p.find("完整包住").expect("box rule missing");
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
        assert_eq!(b, NormBox::new(10.0, 20.0, 30.0, 40.0));
    }

    #[test]
    fn parses_malformed_bbox_shapes() {
        // String payload with a JSON array inside.
        let b = parse_locate_arguments(r#"{"bbox":"[100, 200, 300, 400]"}"#).unwrap();
        assert_eq!(b.x2, 300.0);
        // Object payload.
        let b = parse_locate_arguments(r#"{"box":{"x1":1,"y1":2,"x2":3,"y2":4}}"#).unwrap();
        assert_eq!(b, NormBox::new(1.0, 2.0, 3.0, 4.0));
        // Out-of-range values are clamped, not rejected.
        let b = parse_locate_arguments(r#"{"bbox":[-50,10,1200,900]}"#).unwrap();
        assert_eq!(b, NormBox::new(0.0, 10.0, 1000.0, 900.0));
        // Garbage.
        assert!(parse_locate_arguments(r#"{"bbox":"nope"}"#).is_none());
        assert!(parse_locate_arguments(r#"{"bbox":[1,2]}"#).is_none());
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
            r#"{"in_box":"蓝色软盘图标","anchor_seen":"看不到","match":"YES","why":"图标一致","unclear_kind":"","elsewhere_dir":""}"#,
        )
        .unwrap();
        assert_eq!(v.match_, MatchVerdict::Yes);
        assert_eq!(v.why, "图标一致");
        assert_eq!(v.unclear_kind, UnclearKind::None);
        assert!(v.elsewhere_dir.is_none());
    }

    #[test]
    fn rejects_verify_without_observation_or_unknown_match() {
        // `why` carries the observation now — blank means the model did not look.
        assert!(parse_verify_arguments(r#"{"match":"yes","why":""}"#).is_none());
        assert!(parse_verify_arguments(r#"{"match":"maybe","why":"x"}"#).is_none());
    }

    #[test]
    fn parses_direction_and_unclear_kind() {
        let v = parse_verify_arguments(
            r#"{"match":"no","why":"框里是取消按钮","elsewhere_dir":"left","elsewhere_desc":"左边有个地址栏"}"#,
        )
        .unwrap();
        assert_eq!(v.elsewhere_dir, Some(ElsewhereDir::Left));
        let hint = refine_hint(&v);
        assert!(hint.contains("【框里是取消按钮】"));
        assert!(hint.contains("left"));
        assert!(hint.contains("左边有个地址栏"));
    }

    #[test]
    fn parses_box_too_tight() {
        let v = parse_verify_arguments(
            r#"{"match":"unclear","why":"框里只有文件名前半段，后半在框外","unclear_kind":"box_too_tight"}"#,
        )
        .unwrap();
        assert_eq!(v.match_, MatchVerdict::Unclear);
        assert_eq!(v.unclear_kind, UnclearKind::BoxTooTight);
    }

    #[test]
    fn refine_hint_for_box_too_tight_says_widen_not_relocate() {
        // The verifier saw part of the target. Telling the locator "you looked
        // in the wrong place" sends it hunting elsewhere; it must be told to
        // widen instead.
        let v = parse_verify_arguments(
            r#"{"match":"unclear","why":"框里只有文件名前半段，后半在框外","unclear_kind":"box_too_tight"}"#,
        )
        .unwrap();
        let hint = refine_hint(&v);
        assert!(hint.contains("太窄"), "{hint}");
        assert!(hint.contains("不要换地方找"), "{hint}");
        assert!(
            !hint.contains("不是目标"),
            "must not read as 'wrong place': {hint}"
        );
    }

    #[test]
    fn report_tool_offers_every_unclear_kind_the_parser_accepts() {
        // The parser and the prompt can both know about `box_too_loose`, but the
        // model can only answer with what the tool schema lists. r1c0 reasoned
        // its way to the right verdict and then wrote: 「unclear_kind 选项里没有
        // box_too_loose，只有 box_too_tight」, and fell back to `yes`.
        let schema = report_tool().to_string();
        for kind in [
            "anchor_out_of_view",
            "too_small",
            "too_blurry",
            "ambiguous",
            "box_too_tight",
            "box_too_loose",
        ] {
            assert!(schema.contains(kind), "schema is missing {kind}: {schema}");
        }
    }

    #[test]
    fn refine_hint_for_box_too_loose_says_shrink_not_relocate() {
        // The mirror of the box_too_tight case: the target is in the box, the
        // box is just slack. The locator must shrink around the same place —
        // 「不是目标」 would send it hunting elsewhere.
        let v = parse_verify_arguments(
            r#"{"match":"unclear","why":"框宽约 2.4 倍格宽、高约 2.4 倍格高，含 B1 和 B2 两格","unclear_kind":"box_too_loose"}"#,
        )
        .unwrap();
        assert_eq!(v.unclear_kind, UnclearKind::BoxTooLoose);
        let hint = refine_hint(&v);
        assert!(hint.contains("太松"), "{hint}");
        assert!(hint.contains("只框住目标本身"), "{hint}");
        assert!(hint.contains("中心位置不变"), "{hint}");
        // The measured ratio has to survive into the hint — that is the whole
        // point of reporting box_too_loose instead of no.
        assert!(hint.contains("2.4 倍"), "{hint}");
        assert!(!hint.contains("不是目标"), "{hint}");
    }

    #[test]
    fn refine_hint_without_direction_says_not_seen() {
        let v = parse_verify_arguments(
            r#"{"match":"no","why":"框里是空白"}"#,
        )
        .unwrap();
        assert!(refine_hint(&v).contains("那一带也看不到目标"));
    }
}
