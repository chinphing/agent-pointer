//! Prompts, virtual tool schemas, and response parsing for the B5 loop.
//!
//! Ported from `benchmarks/screenspot_grid/baselines/b5_decoupled.py`
//! (`TOOL_LOCATE` / `TOOL_REPORT` / `prompt_locate` / `PROMPT_VERIFY` /
//! `_parse_box` / `_box_from_text`). The only structural change is that the
//! target description arrives as one natural-language string instead of the
//! benchmark's `{text, feat, ctrl, pos, env}` dict.

use serde_json::{json, Value};

use super::geometry::NormBox;
use super::types::{
    CenterOnTarget, Confidence, ElsewhereDir, MatchVerdict, PositionMatches, UnclearKind,
    VerifyVerdict,
};

/// Max characters kept from each verifier free-text field (B5 truncation).
/// Hard cap on any free-text field from the report tool.
///
/// 800: the band-mode `why` is a three-part answer and the model routinely runs
/// past the 150 characters it is asked for. At 200 the comparison sentence was
/// cut off entirely — the log could not show whether it had been made — and at
/// 400 it still was. The cap exists to keep a runaway answer out of the
/// transcript, not to police length; the prompt already does that.
const FIELD_CHARS: usize = 800;

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
pub fn report_tool_views(wide: bool) -> Value {
    let notes = if wide {
        "备注。四段固定格式，每段一行，前缀不能省。\n\
         \n\
         图像：清晰 / 模糊 / 目标太小 / 被遮挡\n\
         中心：标记落在目标的哪个位置（正中 / 偏左半格 / 落在格子外）——这一项从不写「不涉及」\n\
         大小：框大约是目标的几倍（约 1.0 倍 / 约 1.8 倍，框进左右两个邻格）——目标没有轮廓可量时写「不涉及」\n\
         位置：描述说的位置 vs 图里读到的位置（描述第四行第八列，图里 C 行即第三行，差一行）——描述没提位置时写「不涉及」\n\
         \n\
         判断通过时可以写简述；判断没通过时必须写具体，不能只写「不对」「不符」——这段话会原样交给重新定位的那一步。\n\
         例：图像：清晰 / 中心：标记落在空格正中 / 大小：框与格子几乎等大 / 位置：描述第四行第八列，图里 C 行第三行，差一行"
    } else {
        "一句话，先说蓝框里实际是什么（文字/图标/控件），再说判定依据。**不超过 80 字**。\
         目标位置要说绝对坐标（「第 3 列横跨 x≈455 到 625」），不要只写「在 X 右边」。\
         判 no 时要说清框里缺了目标的哪一部分。例：「框里是空白，目标在框左上约 60 像素」"
    };
    json!({
        "type": "function",
        "function": {
            "name": "report",
            "description": "报告校验结论。先描述蓝框里是什么，再判断它是不是目标。",
            "parameters": {
                "type": "object",
                "properties": {
                    "center_on_target": {
                        "type": "string",
                        "enum": ["yes", "no", "unclear"],
                        "description": "【中心定位】图里蓝框中心画了一个双环标记（红色实心圆+白色外环）。这个标记是不是落在目标上？标记的位置就是最终交付的坐标，所以这一项决定坐标对不对。只看标记本身，不要因为「框里能看到目标」就说 yes——框很大、目标在框的角落里时，标记仍然可能落在目标外面。看不清填 unclear。"
                    },
                    "size_ok": {
                        "type": "string",
                        "enum": ["yes", "no", "unclear"],
                        "description": "【大小合适】框的大小合格吗？这一项和中心标记无关，独立回答。yes=目标整个落在框里、框的宽和高都不超过目标的 1.5 倍；no=框只框住目标的一部分，或框明显大于目标；unclear=看不清。"
                    },
                    "position_matches": {
                        "type": "string",
                        "enum": ["yes", "no", "unknown", "ignore"],
                        "description": "【位置匹配】描述里说的位置，和图里读到的一致吗？描述里带行号、列号、序号、序数（「第四行第八列」「C7」「第 3 项」「最后一个」）时必须回答：一致填 yes，对不上填 no，图里读不出绝对位置填 unknown。描述里完全没提位置（「提交按钮」「关闭图标」）填 ignore。"
                    },
                    "notes": {
                        "type": "string",
                        "description": notes
                    },
                    "confidence": {
                        "type": "string",
                        "enum": ["high", "medium"],
                        "description": "三项都通过时填。文字+方位+图标+颜色四项全中填 high；文字和方位对上、图标或颜色不符填 medium"
                    }
                },
                "required": ["center_on_target", "size_ok", "position_matches", "notes"]
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
    verify_prompt_views(description, false)
}

/// `wide` = a second, wider image is attached as 【图二】.
///
/// The two images have different jobs, and the split is stated up front because
/// getting it wrong is silent: a model that judges the box from the wide image
/// will call a 137 px cell's borders aligned to within a few pixels — which is
/// how 「正好压住」 got said about a box 26 px off.
pub fn verify_prompt_views(description: &str, wide: bool) -> String {
    let desc = format!("· 目标：{}\n", description.trim());
    let intro = if wide {
        "给你两张图，都是**以蓝框为中心**从屏幕上裁下来的（不是整屏），蓝框在两张图的正中央。\n\
         · **图一（小图）**：裁剪范围小，目标在图里显得**大**、看得**清**。\n\
         · **图二（大图）**：裁剪范围是图一的 4 倍，输出尺寸一样，所以目标在图里显得**小**，\n\
           但能看到更远的周围。\n\
         \n\
         **【固定顺序：先图二定行列，再图一判框。不要跳步。】**\n\
         \n\
         **第一步 —— 看图二，确认目标在哪一行、哪一列。**\n\
         图二能看到完整的行标/列标、盘面或表格的行列起始位置、更远的锚点（「数字 8 的右边」\n\
         里的那个 8 可能只在图二里看得见）。\n\
         · 先在图二里找到行标/列标（字母、数字、序号），用它确认目标的行号和列号；\n\
         · 如果锚点在图一里看不到，去图二里找；\n\
         · **把结论写进 why 的最前面**，例如「图二：列标 7/8/9 可见，目标在第 8 列；行标 G/H 可见，第 8 行」。\n\
         · 如果图二里确实没有任何行列标、也没有锚点，就明确写「图二无行列标」，然后跳过这一步。\n\
           **不要因为看不到就编一个行列号。**\n\
         \n\
         **第二步 —— 回到图一，判断框对不对。**\n\
         框里是什么、目标的轮廓/边线在哪、框有没有包住四条边线、框比目标大多少——\n\
         全部只看图一。图一里目标最大最清楚，这些判断只有它靠得住。\n\
         用第一步确认的行列号，核对「图一里框住的这一格」和「图二里确认的那一格」是不是同一格。\n\
         **如果两者不一致，说明框错了，`position_matches = no`。**\n\
         \n\
         **图二绝不能用来判「框压没压住边线」「框大了多少」**——图二里目标太小，看这些必错。\n\
         \n\
         【why 要写清两件事】① 你在图二里看到的行列标或锚点（看不到就直说「图二无行标」）；② 框在图一里框住的是哪一格。\n\
         · 图二看不到行标时，直接写「图二无行标」，不要改用盘面内容反推行号——反推出来的行号不可信。\n\
         · 图二能看到行标/列标时，把看到的具体值写出来（「列标7/8/9可见，行标G/H可见」）。\n\
         · 图二里的行列标只看到一部分、读不出绝对位置时，`position_matches` 填 unknown，不要猜。\n\
         · 描述里给了周边锚点（「在 X 右边」）时，说清图二里那个锚点在不在、指向哪一格。\n\
         例：`列标7/8/9可见、行标G/H可见；框在图一里是白底空格子，倍数约1.1`。\n\
         （例结束）\n"
    } else {
        "这张图是从屏幕上裁下来的一块（不是整屏），蓝框是候选位置。\n\
         **蓝框始终在这张图的正中央**——裁剪时就是以它为中心截的，四周留了余量。\n"
    };
    verify_prompt_body(&desc, intro)
}

fn verify_prompt_body(desc: &str, intro: &str) -> String {
    format!(
        "{intro}\
直接看框里和框外各有什么。\n\n\
【目标元素描述】\n{desc}\
【第一件事：中心标记在目标上吗？】\n\
图里蓝框的中心画了一个**双环标记**（红色实心圆 + 白色外环），它的位置就是最终要交付的坐标。先只回答这一件事：这个标记是不是落在目标上？标记落在目标上（目标的轮廓内部、文字笔画上、控件内部）→ `center_on_target = yes`；落在目标外面 → `center_on_target = no` 并在 `center_offset` 里填标记在目标的哪一侧；看不清 → `unclear`。**只看标记本身，不要因为「框里能看到目标」就说 yes**——框很大、目标缩在框的某个角落时，标记完全可能落在目标外面，这种情况必须判 no。\n\
【第二件事：框的大小合格吗？】这一项和上面的标记判断**完全无关**，独立回答。目标整个落在框里、框的宽和高都不超过目标的 1.5 倍 → `match = yes`；框只框住目标的一部分、或框明显大于目标 → `match = no`，并在 `size_ratio` 里填框的宽大约是目标的几倍（1.0 = 一样大，0.6 = 只有六成，1.8 = 大八成）；看不清 → `match = unclear` 并在 `unclear_kind` 里说明原因。目标本身是一片空白区域（点空白处关闭弹窗、遮罩、画布背景），没有轮廓可量时 `target_px` 填 [0, 0]。\\n\\
【第三件事：描述里说的位置，和图里读到的一致吗？】\\n\
描述里如果带了**位置标识**——行号、列号、单元格编号（「第四行第八列」「C7」「第 3 项」「周三」）、序号（「第 5 个」）、序数（「最后一个」）——就必须核对：\\n\
· 图里读到的位置和描述一致 → `position_matches = yes`\\n\
· 对不上 → `position_matches = no`（框在别的地方，不是 unclear）\\n\
· 图里读不出绝对位置（行标被裁掉、没有参照物）→ `position_matches = unknown`\\n\
描述里**完全没提位置**（「提交按钮」「关闭图标」「搜索框」）→ `position_matches = ignore`，这一项不用管。\\n\
\\n\
**不要自己数「从上往下第几行」**——图的上边缘不一定是第一行，数出来的行号没有意义。数不出就填 unknown。\\n\
\\n\
【位置标识和周边锚点不是一回事】\n\
描述里的线索分两类，对不上时的处理完全相反：**位置标识**——描述直接说出了目标所在的位置：行号、列号、单元格编号（「第四行第八列」「C7」「第 3 项」「周三」）、序号（「第 5 个」）、序数（「最后一个」）。这类线索和图里读到的**对不上，必须判 no**——不是 unclear，是 no：框在别的地方。理由：位置标识就是位置本身，不是辅助线索，差一行就是差一行，没有「口径差异」这种说法。行标只看到一部分、数不出绝对行号时，`position_matches` 填 unknown，**不要自己从上往下数「第几行」**——图的上边缘不一定是第一行，数出来的行号没有意义。**周边锚点**——描述说的是目标旁边有什么：「在 X 的右边」「在 Y 上方」「旁边有个 Z」「隔一格」。这类是辅助线索，不是硬约束：\n\
- 锚点和图中有出入，**不单独构成判 no 的理由**。\n\
- 只有当「外观完全符合、但锚点指向另一个明显不同的元素」时才判 no。\n\
- 判断顺序：先看框里是不是目标 → 再核对位置标识 → 最后才考虑周边锚点。\n\n\
【三件事都要单独回答，缺一项都不算完成】\n\
1. `center_on_target` —— 中心标记在目标上吗（yes / no + center_offset / unclear）。\n\
2. `match` —— 框的大小合格吗（yes / no + size_ratio / unclear）。\n\
3. `position_matches` —— 描述里说的位置和图里读到的一致吗（yes / no / unknown / ignore）。\n\
三项都过才算找到：`center_on_target=yes`、`match=yes`、`position_matches` 不是 no 也不是 unknown。\\n\\
   \\n\\
   \\n\\
   \\n\\
   \n\
4. 周边锚点（「在 X 右边」「旁边有个 Z」）**不参与**上面三项的判定：它有出入但三项都过 → 仍然算找到，把出入写进 why。\n\n\
【外观怎么比】按这个顺序，**不要跳级**：\n\
1. **文字** —— 文字内容是否一致（外观维度里最重要的一项）\n\
2. **图标** —— 图标形状/类型是否一致\n\
3. **颜色** —— 颜色是否一致（**最弱**，按钮在 hover / disabled / 选中态下\n\
   颜色本来就会变，**不要因为颜色判 no**）\n\n\
**外观对得上不等于找到。**外观只管「框里这个东西像不像目标」，位置由第三件事单独判。外观全对、但位置标识对不上 → 仍然是 `position_matches = no`，不算找到。\\n\\n\
【注意：蓝框在这张图里居中，是裁剪方式决定的，不能作为位置证据。】\n\
位置关系只能看「目标相对蓝框在画面里的位置」，不能靠蓝框在图中居中这一点推断。\n\n\
【看不清的情况】\n\
- 目标太小看不清 → unclear + too_small\n\
- 图像模糊或被遮挡 → unclear + too_blurry\n\n\
【如果判 unclear】\\n\\
【match=no 时】size_ratio 里填「框的宽大约是目标的几倍」——0.6 表示框只有六成，1.8 表示大八成。写不出这个数，说明你没量，重新量一遍再答。\\n\\
【center_on_target=no 时】center_offset 里填标记在目标的哪一侧（left/right/up/down）。\\n\\
【position_matches=no 时】why 里写清描述说的位置和图里读到的是什么，两者差多少。\\n\\n\\
【如果判 no】\n\
【如果这张图里根本看不到目标】why 里直接说，不要编。\n\
【为什么至少写两句】第一句：图二里看到了什么行列标或锚点（看不到就说看不到）。第二句：框在图一里框住的是哪一格。\n\n\
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

    // `notes` is the one field that must be present: it carries what the verifier
    // actually saw. Without it a wrong verdict is just a wrong enum with no
    // explanation, and the retry hint has nothing to say.
    let notes = text_field(&v, "notes");
    if notes.is_empty() {
        return None;
    }

    // An omitted enum is `unclear` / `unknown`, never `yes`: a verifier that did
    // not answer a question has not answered it in the affirmative.
    let center_on_target = CenterOnTarget::parse(&text_field(&v, "center_on_target"))
        .unwrap_or(CenterOnTarget::Unclear);
    let size_ok = match text_field(&v, "size_ok").to_ascii_lowercase().as_str() {
        "yes" => MatchVerdict::Yes,
        "no" => MatchVerdict::No,
        _ => MatchVerdict::Unclear,
    };
    let position_matches = PositionMatches::parse(&text_field(&v, "position_matches"))
        .unwrap_or(PositionMatches::Unknown);

    log::info!(
        "verify center={center_on_target:?} size={size_ok:?} position={} notes={}",
        position_matches.as_str(),
        notes.trim()
    );

    Some(VerifyVerdict {
        center_on_target,
        size_ok,
        position_matches,
        notes,
        confidence: Confidence::parse(&text_field(&v, "confidence")),
        reasoning: String::new(),
    })
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
        assert!(!p.contains("大得离谱"), "{p}");
        assert!(!p.contains("没有大太多"), "{p}");
    }

    #[test]
    fn verify_prompt_asks_for_a_ratio_on_both_sides_of_the_slack_axis() {
        let p = verify_prompt("数独盘面C3的空格子");
        // Too tight used to ask for 「大概还差多少」 with an example in pixels —
        // the crop is rescaled, so a pixel count from the model does not survive
        // the trip back to screen coordinates. The size answer is now a single
        // multiple, and `refine_hint` passes it through in both directions.
        assert!(p.contains("size_ratio"), "{p}");
        assert!(p.contains("几倍"), "{p}");
        assert!(!p.contains("差 30 像素"), "{p}");
        assert!(!p.contains("大概还差多少"), "{p}");
    }

    #[test]
    fn verify_prompt_handles_a_target_with_no_outline() {
        let p = verify_prompt("弹窗外的空白区域");
        // 「点击空白处关闭」 has no measurable target size. Without an explicit
        // escape the verifier has to invent one, or the schema rejects the call
        // for a missing field.
        assert!(p.contains("没有轮廓可量时 `target_px` 填 [0, 0]"), "{p}");
        assert!(p.contains("空白区域"), "{p}");
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
    fn verify_prompt_defines_the_centre_mark_as_the_answer() {
        let p = verify_prompt("数独盘面第三行第二列的空格子");
        // The coordinate question is now "is the mark on the target", not "do the
        // four edges line up". The latter is what a generous box always passes.
        assert!(p.contains("双环标记"), "{p}");
        assert!(p.contains("center_on_target"), "{p}");
        assert!(p.contains("只看标记本身"), "{p}");
    }

    #[test]
    fn verify_prompt_separates_position_from_size() {
        let p = verify_prompt("数独盘面第三行第二列的空格子");
        assert!(p.contains("第一件事"), "{p}");
        assert!(p.contains("第二件事"), "{p}");
        // Size is judged by the verifier, not measured-and-passed-through: it is
        // the one that can tell a slack box from a tight one.
        assert!(p.contains("1.5 倍"), "{p}");
        assert!(p.contains("size_ratio"), "{p}");
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
        // 判 no 时要说明标记落在目标的哪一侧，代码据此生成方向提示
        assert!(p.contains("center_offset"), "{p}");
        assert!(p.contains("哪一侧"), "{p}");
        assert!(p.contains("落在目标外面"), "{p}");
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
            r#"{"center_on_target":"YES","size_ok":"yes","position_matches":"ignore","notes":"图像：清晰\n中心：标记落在图标正中\n大小：框与图标几乎等大\n位置：不涉及"}"#,
        )
        .unwrap();
        assert_eq!(v.center_on_target, CenterOnTarget::Yes);
        assert_eq!(v.size_ok, MatchVerdict::Yes);
        assert_eq!(v.position_matches, PositionMatches::Ignore);
        assert!(v.notes.contains("标记落在图标正中"));
        assert!(v.is_hit());
    }

    #[test]
    fn rejects_verify_without_notes() {
        assert!(parse_verify_arguments(r#"{"center_on_target":"yes","notes":""}"#).is_none());
    }

    #[test]
    fn an_omitted_verdict_is_never_yes() {
        // A verifier that did not answer has not answered in the affirmative.
        let v = parse_verify_arguments(r#"{"notes":"图像：模糊"}"#).unwrap();
        assert_eq!(v.center_on_target, CenterOnTarget::Unclear);
        assert_eq!(v.size_ok, MatchVerdict::Unclear);
        assert_eq!(v.position_matches, PositionMatches::Unknown);
        assert!(!v.is_hit());
    }

    #[test]
    fn an_off_centre_mark_is_not_a_hit_and_the_hint_names_the_dimension() {
        let v = parse_verify_arguments(
            r#"{"center_on_target":"no","size_ok":"yes","position_matches":"ignore","notes":"图像：清晰\n中心：标记在地址栏上\n大小：不涉及\n位置：不涉及"}"#,
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
            r#"{"center_on_target":"yes","size_ok":"yes","position_matches":"no","notes":"图像：清晰\n中心：正中\n大小：约1.0倍\n位置：描述第四行，图里第三行，差一行"}"#,
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
            r#"{"center_on_target":"yes","size_ok":"yes","position_matches":"unknown","notes":"图像：清晰\n中心：正中\n大小：约1.0倍\n位置：图里读不出绝对行号"}"#,
        )
        .unwrap();
        assert!(!v.is_hit());
        assert_eq!(v.failed_checks(), vec!["位置匹配"]);
    }

    #[test]
    fn a_box_of_the_wrong_size_blocks_the_hit_without_moving_the_centre() {
        let v = parse_verify_arguments(
            r#"{"center_on_target":"yes","size_ok":"no","position_matches":"ignore","notes":"图像：清晰\n中心：正中\n大小：约1.8倍，框进左右两个邻格\n位置：不涉及"}"#,
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
            r#"{"center_on_target":"no","size_ok":"no","position_matches":"ignore","notes":"图像：清晰\n中心：偏左半格\n大小：约2倍\n位置：不涉及"}"#,
        )
        .unwrap();
        assert_eq!(v.failed_checks(), vec!["中心定位", "大小合适"]);
        let hint = refine_hint(&v);
        assert!(hint.contains("中心定位、大小合适"), "{hint}");
    }

    #[test]
    fn a_slack_box_is_named_as_a_size_problem() {
        let v = parse_verify_arguments(
            r#"{"center_on_target":"yes","size_ok":"no","position_matches":"ignore","notes":"图像：清晰\n中心：正中\n大小：约 2.4 倍，含 B1 和 B2 两格\n位置：不涉及"}"#,
        )
        .unwrap();
        assert_eq!(v.failed_checks(), vec!["大小合适"]);
        let hint = refine_hint(&v);
        assert!(hint.contains("大小合适"), "{hint}");
        assert!(hint.contains("2.4 倍"), "{hint}");
    }

}
