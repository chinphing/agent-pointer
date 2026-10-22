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
                    "in_box": {
                        "type": "string",
                        "description": "蓝框里实际是什么？只描述看到的文字/图标/控件"
                    },
                    "anchor_seen": {
                        "type": "string",
                        "description": "目标描述里提到的「周边锚点」，在这张图里能看到吗？看到就写它是什么；看不到就写「看不到」。必填"
                    },
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
                        "description": "判定依据，一句话。判 no 时**必须用距离描述**，不要提「框」——下游拿不到框，只拿得到坐标。例：「预估点和目标差了大约 80 像素，目标在预估点右侧偏下，点下去会点空」"
                    },
                    "unclear_kind": {
                        "type": "string",
                        "enum": ["", "anchor_out_of_view", "too_small", "too_blurry", "ambiguous"],
                        "description": "match=unclear 时必填。anchor_out_of_view=锚点不在图内无法核对；too_small=目标太小看不清；too_blurry=图像模糊/被遮挡；ambiguous=描述自相矛盾"
                    },
                    "elsewhere_dir": {
                        "type": "string",
                        "enum": ["", "left", "right", "up", "down", "inside"],
                        "description": "match=no 且目标在这张图内别处时，填相对蓝框的方向"
                    },
                    "elsewhere_desc": {"type": "string", "description": "那个位置看起来是什么"}
                },
                "required": ["in_box", "anchor_seen", "match", "why"]
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
    let tail = "框要刚好框住该元素本身（不要外扩、不要缩小）。\n不需要像素级精确：框的中心落在元素上、四条边没有明显超出轮廓，就算合格。\n\n请调用 locate(bbox=[x1, y1, x2, y2])。";
    let tail = match previous_hint.map(str::trim).filter(|s| !s.is_empty()) {
        Some(hint) => format!("【上一轮的情况】{hint}\n\n{tail}"),
        None => tail.to_string(),
    };
    format!("{head}{near}\n{coord}\n{tail}")
}

/// Verifier prompt. It sees only the crop and the description — **never** the
/// hint box (§6.3.1), otherwise it would rubber-stamp a wrong candidate.
pub fn verify_prompt(description: &str) -> String {
    let desc = format!("· 目标：{}\n", description.trim());
    format!(
        "这张图是从屏幕上裁下来的一块（不是整屏），蓝框是候选位置。\n\n\
【目标元素描述】\n{desc}\
【任务】\n\
① 先看蓝框里实际是什么（文字/图标/控件），填 in_box。\n\
② 再判断蓝框里的这个东西，是不是上面描述的目标元素。\n\n\
【必须单独核对的一件事：预估点和目标的距离】\n\
「蓝框里有目标」和「蓝框中心就在目标上」是两件事，**都要满足才算找到**。\n\
- 蓝框中心落在目标主体上 → 才算真正找到。\n\
- 目标在蓝框里，但**蓝框中心落在目标外面**（只压到一角、目标大部分在框外）\n\
  → 判 **no**，并在 why 里**用距离描述**，不要提框：\n\
    例如「预估点和目标差了大约 80 像素，目标在预估点右侧偏下，点下去会点空」。\n\
  **写清方向和大致距离**（十几像素 / 几十像素 / 上百像素），\n\
  下游拿这句话决定是重新找还是微调。\n\n\
【周边锚点怎么用】\n\
锚点是用来**区分同名/相似干扰元素**的辅助线索，不是硬约束。\n\
- 锚点和图中有出入，**不单独构成判 no 的理由**。\n\
- 只有当「外观完全符合、但锚点指向另一个明显不同的元素」时才判 no。\n\
- 判断顺序：先看框里是不是目标 → 再看中心点落没落在目标上 → 最后才考虑锚点矛盾。\n\n\
【判定规则】\n\
1. 文字内容对不上 → 判 **no**，不管其他项多像。\n\
2. 文字对得上，但中心点落在目标外 → 判 **no**，why 用距离描述（见上）。\n\
3. 文字对得上、中心点也在目标上 → 判 **yes**。\n\
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
    let in_box = text_field(&v, "in_box");
    if in_box.is_empty() {
        return None;
    }
    let unclear_kind = match text_field(&v, "unclear_kind").to_ascii_lowercase().as_str() {
        "anchor_out_of_view" => UnclearKind::AnchorOutOfView,
        "too_small" => UnclearKind::TooSmall,
        "too_blurry" => UnclearKind::TooBlurry,
        "ambiguous" => UnclearKind::Ambiguous,
        _ => UnclearKind::None,
    };
    Some(VerifyVerdict {
        match_,
        in_box,
        anchor_seen: text_field(&v, "anchor_seen"),
        why: text_field(&v, "why"),
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
    })
}

/// Retry hint fed into the next locate round (B5 `hint` construction).
pub fn refine_hint(verdict: &VerifyVerdict) -> String {
    let mut hint = format!("你上次给的框里实际是【{}】，不是目标。", verdict.in_box.trim());
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
        // 判 no 时要用距离描述，因为下游只有坐标、没有框
        assert!(p.contains("用距离描述"), "{p}");
        assert!(p.contains("不要提框"), "{p}");
        assert!(p.contains("点下去会点空"), "{p}");
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
        assert!(p.contains("框要刚好框住该元素本身"), "{p}");
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
        assert_eq!(v.in_box, "蓝色软盘图标");
        assert_eq!(v.unclear_kind, UnclearKind::None);
        assert!(v.elsewhere_dir.is_none());
    }

    #[test]
    fn rejects_verify_without_observation_or_unknown_match() {
        assert!(parse_verify_arguments(r#"{"in_box":"","match":"yes","why":"x","anchor_seen":"y"}"#).is_none());
        assert!(parse_verify_arguments(r#"{"in_box":"a","match":"maybe","why":"x","anchor_seen":"y"}"#).is_none());
    }

    #[test]
    fn parses_direction_and_unclear_kind() {
        let v = parse_verify_arguments(
            r#"{"in_box":"取消","anchor_seen":"看不到","match":"no","why":"不是","elsewhere_dir":"left","elsewhere_desc":"左边有个地址栏"}"#,
        )
        .unwrap();
        assert_eq!(v.elsewhere_dir, Some(ElsewhereDir::Left));
        let hint = refine_hint(&v);
        assert!(hint.contains("【取消】"));
        assert!(hint.contains("left"));
        assert!(hint.contains("左边有个地址栏"));
    }

    #[test]
    fn refine_hint_without_direction_says_not_seen() {
        let v = parse_verify_arguments(
            r#"{"in_box":"空白","anchor_seen":"看不到","match":"no","why":"不是"}"#,
        )
        .unwrap();
        assert!(refine_hint(&v).contains("那一带也看不到目标"));
    }
}
