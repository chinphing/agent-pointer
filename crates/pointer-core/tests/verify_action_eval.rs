// 用运行时采集的 `action_verify` 样本量动作后校验的判对率。
//
// 与 `verify_position_eval.rs` 的区别：
//
// - **输入**：那边是 locate 的 `verify_pair(close, wide)`；这边是 pipeline 的
//   `build_verify_wire_messages(before, after, operation, tool_result)` —— 两条
//   完全不同的 wire 构建路径，prompt 也不同，不能共用一个评测器。
// - **图片**：直接读采集到的 `action_verify_in1/in2.jpg`，不再重绘。
//   这两张就是模型当时收到的字节（`save_verify_inputs` 从 wire 里提取），
//   所以 `current_pointer` 传 `None` —— 传了会触发 `redraw_with_current_pointer`，
//   把已经画好的图再画一遍。
//
// 数据来源：`pointer-eval/scripts/build_action_set.py` 的产物，或任何
// `POINTER_VERIFY_ACTION_ROOT` 指向的目录。
//
// ```text
// POINTER_VERIFY_ACTION_ROOT=... POINTER_VERIFY_ACTION_CONCURRENCY=4 \
//   cargo test --release -p pointer-core --test verify_action_eval -- --nocapture
// ```

use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use pointer_core::agents::computer::pipeline::operation::{
    operation_family_for_tool, OperationFamily,
};
use pointer_core::agents::computer::pipeline::types::{
    ActionResult, FailureCause, ScrollAxis, ScrollMeasurement, VerifyConclusion,
};
use pointer_core::agents::computer::pipeline::wire_format::{
    build_verify_wire_messages, VerifyWireInput,
};
use pointer_core::agents::computer::state::ScreenCaptureResult;
use pointer_core::agents::computer::vision::screen::MonitorInfo;
use pointer_core::chat_service::{
    scroll_scale_line, SCROLL_BAND_MAX, SCROLL_BAND_MIN, SCROLL_TARGET_FRACTION,
};
use pointer_core::models::ModelSettings;
use pointer_core::provider::OpenAIProvider;

#[derive(Deserialize, Debug, Clone)]
struct Row {
    id: String,
    /// 采集时记下的操作文本，例如 `Clicked 'Save' button`。
    #[serde(default)]
    operation: String,
    /// 工具返回。pipeline 里可空。
    #[serde(default)]
    tool_result: Option<String>,
    /// 生产当时给出的结论：`Pass` / `Fail` / `Pending` / `Na`。
    #[serde(default)]
    verdict: String,
    /// 生产当时给的失败原因（仅 Fail 时可能有）。
    #[serde(default)]
    failure_cause: Option<String>,
    /// 两张输入图，相对 `base`。
    #[serde(default)]
    image_in1: Option<String>,
    #[serde(default)]
    image_in2: Option<String>,
    /// 旧格式（`verify_action/{sudoku,general}/truth.jsonl`）的字段名。
    #[serde(default)]
    shots: Vec<String>,
    #[serde(default)]
    executed: String,
    /// 滚动族真值：人工核对过的正确处置。
    /// `no_line` / `keep` / `rescale` / `halve` / `no_false_claim`
    #[serde(default)]
    expect: Option<String>,
    /// 滚动族真值：应该建议的下一个刻度，`None` = 不该给建议。
    #[serde(default)]
    expect_notches_y: Option<i64>,
    /// 滚动族：这一次请求的刻度，喂给生产换算函数。
    #[serde(default)]
    scroll: Option<ScrollArgs>,
    /// 这条样本属于哪个操作族 —— 由工具名推出来，不再一律当点击。
    #[serde(skip, default = "default_family")]
    family: OperationFamily,
    #[serde(skip)]
    base: PathBuf,
}

/// 一条样本跑完的结果。
struct Outcome {
    id: String,
    /// 操作族名（`scroll` / `pointer_click` …），用来分组统计。
    family: String,
    want_raw: String,
    got: String,
    want: String,
    /// 滚动族真值；其他族为 `None`。
    expect: Option<String>,
    expect_notches: Option<i64>,
    /// 生产 `scroll_scale_line` 的输出（`None` = 不该给建议）。
    scale_line: Option<String>,
    scale_notches: Option<i64>,
    /// 模型这一次读出来的位移与越屏标志 —— 刻度规则要拿它们做不变量检查，
    /// 不能用采集时那次的数字（两次读数本来就会不一样）。
    moved: Option<f32>,
    past_panel: bool,
    /// 这一次请求的刻度（带符号）。
    used_notches: Option<i64>,
}

fn default_family() -> OperationFamily {
    OperationFamily::PointerClick
}

#[derive(Deserialize, Debug, Clone, Default)]
struct ScrollArgs {
    #[serde(default)]
    notches_x: Option<i64>,
    #[serde(default)]
    notches_y: Option<i64>,
}

fn load_dotenv() {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/locate_fixture/.env");
    if let Ok(text) = std::fs::read_to_string(&p) {
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((k, v)) = line.split_once('=') {
                let v = v.trim().trim_matches('"').trim_matches('\'');
                if std::env::var(k.trim()).is_err() {
                    std::env::set_var(k.trim(), v);
                }
            }
        }
    }
}

/// `.env` 里 DeepSeek 的 key 是 `enc:v1:` 密文（本机 HKDF 派生）。走生产同一
/// 条解密路径，省得为了跑对照实验去要明文；换机器解不开时会打印原因。
fn resolve_api_key(raw: &str) -> String {
    let Some(rest) = raw.strip_prefix("enc:v1:") else {
        return raw.to_string();
    };
    use base64::Engine as _;
    let Ok(blob) = base64::engine::general_purpose::STANDARD.decode(rest) else {
        println!("key 解密失败：base64 解不开");
        return String::new();
    };
    match pointer_core::local_secret::decrypt_local_secret_with_info(&blob, b"provider-api-keys-v1") {
        Ok(k) => k,
        Err(e) => {
            println!("key 解密失败（换机器会这样）：{e}");
            String::new()
        }
    }
}

fn build_provider() -> Result<OpenAIProvider> {
    load_dotenv();
    let base_url = std::env::var("POINTER_LOCATE_BASE_URL")
        .context("POINTER_LOCATE_BASE_URL not set (tests/locate_fixture/.env)")?;
    let api_key = resolve_api_key(&std::env::var("POINTER_LOCATE_API_KEY").unwrap_or_default());
    let model =
        std::env::var("POINTER_LOCATE_MODEL").unwrap_or_else(|_| "qwen3.8-flash".to_string());
    // 字段名是 camelCase（`baseUrl` / `apiKey` / `activeProviderId`），
    // 写成 snake_case 会得到 `missing field baseUrl`。
    let settings: ModelSettings = serde_json::from_value(serde_json::json!({
        "providers": [{
            "id": "verifyeval",
            "name": "verifyeval",
            "baseUrl": base_url,
            "apiKey": "",
            "models": [model.clone()],
            // 必须显式声明：不写的话 `resolve_thinking_strategy_id` 会按 URL 猜，
            // 认不出 dashscope 就落到 `OpenaiEffort`，而这个模型只认
            // `enable_thinking` —— 结果关不掉，每次 verify 多跑一轮 reasoning。
            "thinkingProtocol": "budget"
        }],
        "activeProviderId": "verifyeval",
        "model": model,
        "temperature": 0.0,
        "maxTokens": 64000,
        "hasKey": true
    }))
    .context("build ModelSettings")?;
    let mut settings = settings;
    // 生产 verify 阶段关 thinking（`PipelineLlmPhase::Verify` 默认 off）。
    // 只设 `round_enable_thinking` 不够，少了 `round_thinking_locked` 会被覆盖回去。
    let intensity =
        std::env::var("POINTER_VERIFY_ACTION_THINKING").unwrap_or_else(|_| "off".into());
    pointer_core::thinking_strategy::apply_thinking_intensity_to_round(&mut settings, Some(&intensity));
    Ok(OpenAIProvider::new(settings, api_key))
}

fn root() -> PathBuf {
    if let Ok(p) = std::env::var("POINTER_VERIFY_ACTION_ROOT") {
        if !p.trim().is_empty() {
            return PathBuf::from(p);
        }
    }
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../pointer-eval-data/verify_action")
}

fn load_rows() -> Result<Vec<Row>> {
    let root = root();
    let scenes = scene_dirs(&root);
    let mut out = Vec::new();
    for scene in scenes {
        let dir = root.join(&scene);
        let truth = dir.join("truth.jsonl");
        if !truth.exists() {
            println!("跳过 {scene}：没有 {}", truth.display());
            continue;
        }
        for (i, line) in std::fs::read_to_string(&truth)?.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let mut row: Row = serde_json::from_str(line)
                .with_context(|| format!("{}:{} 解析失败", truth.display(), i + 1))?;
            row.base = dir.clone();
            row.family = row_family(&row);
            out.push(row);
        }
    }
    Ok(out)
}

/// 场景目录 = `root` 下带 `truth.jsonl` 的子目录。默认全都要，不再写死两个名字。
fn scene_dirs(root: &Path) -> Vec<String> {
    if let Ok(s) = std::env::var("POINTER_VERIFY_ACTION_SCENES") {
        let list: Vec<String> = s
            .split(',')
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .collect();
        if !list.is_empty() {
            return list;
        }
    }
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(root) {
        for e in rd.flatten() {
            if e.path().join("truth.jsonl").exists() {
                if let Some(n) = e.file_name().to_str() {
                    out.push(n.to_string());
                }
            }
        }
    }
    out.sort();
    out
}

/// 工具名：新格式 `tool=mouse_scroll_current goal=…`，旧格式 `Executed mouse_click_at: …`。
fn row_tool(row: &Row) -> Option<String> {
    if let Some(i) = row.operation.find("tool=") {
        let name: String = row.operation[i + 5..]
            .chars()
            .take_while(|c| !c.is_whitespace())
            .collect();
        if !name.is_empty() {
            return Some(name);
        }
    }
    let i = row.executed.find("Executed ")?;
    let name: String = row.executed[i + 9..]
        .chars()
        .take_while(|c| *c != ':')
        .collect();
    let name = name.trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// 每个样本走自己那个操作族的提示词。
///
/// 之前这里写死 `PointerClick`：滚动样本会拿点击的题面判，`scroll.md` 里
/// 「锚点 / 位移 / 越过一屏」整段不生效 —— 评测测的就不是生产行为。
fn row_family(row: &Row) -> OperationFamily {
    row_tool(row)
        .map(|t| operation_family_for_tool(&t))
        .unwrap_or(OperationFamily::PointerClick)
}

/// 从模型回答里取 `scroll:` 那一行：`moved=1.2 unit=target-heights past_panel=false`。
fn parse_scroll_fields(text: &str) -> (Option<f32>, Option<String>, bool) {
    let Some(line) = text
        .lines()
        .rev()
        .find(|l| l.trim_start().starts_with("scroll:"))
    else {
        return (None, None, false);
    };
    let mut moved = None;
    let mut unit = None;
    let mut past = false;
    for tok in line
        .trim_start()
        .trim_start_matches("scroll:")
        .split_whitespace()
    {
        let Some((k, v)) = tok.split_once('=') else {
            continue;
        };
        let v = v.trim_end_matches(',');
        match k {
            "moved" => moved = v.parse::<f32>().ok(),
            "unit" => unit = Some(v.to_string()),
            "past_panel" => past = v.eq_ignore_ascii_case("true"),
            _ => {}
        }
    }
    (moved, unit, past)
}

/// 从生产输出里取建议刻度：`notches_y=-100` / `notches_x=60`。
fn parse_suggested_notches(line: &str, key: &str) -> Option<i64> {
    let pat = format!("{key}=");
    let i = line.find(&pat)?;
    let num: String = line[i + pat.len()..]
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '-')
        .collect();
    num.parse().ok()
}

fn read_image(path: &Path) -> Result<Vec<u8>> {
    std::fs::read(path).with_context(|| format!("读图失败 {}", path.display()))
}

/// 构造一个只用来喂 wire 的捕获结果。
///
/// `raw_unmarked_jpeg` 留空：`current_pointer = None` 时 wire 不会读它，
/// 传空能让「误传指针导致重绘」变成编译期之外的一个明确错误。
fn fake_capture(jpeg: Vec<u8>) -> ScreenCaptureResult {
    ScreenCaptureResult {
        raw_unmarked_jpeg: Vec::new(),
        screenshot_jpeg: jpeg,
        monitor: MonitorInfo::new(0, 0, 1920, 1080),
    }
}

/// 让模型用纯文本回答，不调工具。
///
/// 生产走 tool call（`submit_*`），但评测只需要判定本身；用文本能少一层
/// 结构化输出的失败模式，`verify_position_eval` 的 `TEXT_MODE` 是同一个理由。
const TEXT_INSTRUCTION: &str = "\n\n## 输出格式\n\n**不要调用工具，直接用文字回答。**\n\n先写一段简短对比（甲=动作前、乙=动作后），然后在**最后单独一行**只写 `pass` 或 `fail`。\n";

/// 滚动族还要位移：这一族是唯一「校验结论反推下一次动作参数」的，只判
/// pass/fail 等于没测到刻度换算那一段。
const TEXT_INSTRUCTION_SCROLL: &str = "\n\n## 输出格式\n\n**不要调用工具，直接用文字回答。**\n\n先写一段简短对比（甲=动作前、乙=动作后），然后依次**单独两行**：\n\n```\nscroll: moved=<数字> unit=<target-heights|target-widths|screen|screen-widths> past_panel=<true|false>\npass|fail\n```\n\n`moved` 是内容位移占目标区域的比例（0.05 = 5%）。**锚点已从后帧消失时不要给 moved**，改写 `scroll: past_panel=true unit=…`。\n";

/// 采集落的是 `AtEdge` / `WrongOperation`，`FailureCause::parse` 只认
/// `at_edge` / `wrong_operation` —— 直接传会把 AtEdge 吞成 None。
fn parse_failure_cause(raw: &str) -> Option<FailureCause> {
    let mut snake = String::new();
    for (i, c) in raw.trim().chars().enumerate() {
        if c.is_ascii_uppercase() && i > 0 {
            snake.push('_');
        }
        snake.push(c.to_ascii_lowercase());
    }
    FailureCause::parse(&snake)
}

fn parse_verdict(text: &str) -> Option<String> {
    // 模型常把整段答案包进 ``` 代码块，结尾那行是围栏不是判定 —— 逐行往回
    // 找第一个「只剩 pass / fail」的行，顺手去掉 markdown 装饰。
    text.lines()
        .rev()
        .map(|l| {
            l.trim()
                .trim_matches(|c: char| c == '`' || c == '*' || c == '_' || c.is_whitespace())
                .to_ascii_lowercase()
        })
        .find(|l| l == "pass" || l == "fail")
}

/// 刻度规则不变量。`None` = 这条不适用（无标签 / 没判定）。
fn scale_rule_violation(o: &Outcome) -> Option<String> {
    let expect = o.expect.as_deref()?;
    if expect == "unreviewed" || expect == "skip" {
        return None;
    }
    let line = o.scale_line.as_deref();
    let bad = |why: &str| {
        Some(format!(
            "{} [{}]: expect={} {} 实得刻度={:?} 行={:?}",
            o.id, o.family, expect, why, o.scale_notches, o.scale_line
        ))
    };

    // 上下文决定的结论：到边 / 未判定 → 不该给建议。
    if expect == "no_line" {
        if line.is_some() {
            return bad("期望不给刻度行");
        }
        return None;
    }
    // 已知缺陷：位移 0 时宿主断言了没被证实的原因。
    if expect == "no_false_claim" {
        let false_claim = line.is_some_and(|l| l.contains("did not reach a scrollable area"));
        if false_claim {
            return bad("不该断言「没落到可滚动区域」");
        }
        return None;
    }

    // 其余按规则检查：给定模型这一次读的 (moved, past_panel)，该走哪一支。
    let Some(used) = o.used_notches else {
        return Some(format!("{}: 缺 notches，无法检查刻度规则", o.id));
    };
    let sign = if used < 0 { -1i64 } else { 1 };
    let used_abs = used.abs();
    let half = sign * (used_abs / 2).max(1);
    let moved = o.moved.unwrap_or(0.0);

    if o.past_panel {
        let l = line?;
        if !l.contains("jumped past a full panel") {
            return bad("置了 past_panel 却没说是越过整屏");
        }
        if l.contains("moved ~") {
            return bad("越屏时不该报位移数值");
        }
        if o.scale_notches != Some(half) {
            return bad("越屏应减半");
        }
        return None;
    }
    if moved > 1.0 {
        let l = line?;
        if l.contains("anchor is gone") {
            return bad("没置 past_panel 却断言锚点消失");
        }
        if !l.contains("not usable") {
            return bad("报了一屏以上却没说是不可用的数字");
        }
        if o.scale_notches != Some(half) {
            return bad("过冲应减半");
        }
        return None;
    }
    if moved <= 0.0 {
        if line.is_none() {
            return bad("位移为 0 应给出提示");
        }
        return None;
    }
    // 落在目标带宽内：宿主应当说「保持这个刻度」，不再给数字。
    if (SCROLL_BAND_MIN..=SCROLL_BAND_MAX).contains(&moved) {
        let l = line?;
        if !l.contains("keep this count") {
            return bad("带宽内应保持刻度");
        }
        return None;
    }
    let want =
        sign * ((used_abs as f32 * SCROLL_TARGET_FRACTION / moved).round().max(1.0) as i64);
    match o.scale_notches {
        Some(got) if (got - want).abs() <= 1 => None,
        other => bad(&format!("按实测应给 {want}，实得 {other:?}")),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn eval_action_verify() -> Result<()> {
    let rows = load_rows()?;
    if rows.is_empty() {
        println!("没有样本 —— 用 build_action_set.py 从采集目录生成，或设置 POINTER_VERIFY_ACTION_ROOT");
        return Ok(());
    }
    let provider = build_provider()?;
    let conc: usize = std::env::var("POINTER_VERIFY_ACTION_CONCURRENCY")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(4);
    let sem = Arc::new(tokio::sync::Semaphore::new(conc));
    println!("评测 {} 条，并发 {conc}", rows.len());

    let mut handles = Vec::new();
    for row in rows {
        let provider = provider.clone();
        let sem = sem.clone();
        handles.push(tokio::spawn(async move {
            let _permit = sem.acquire().await.expect("semaphore");

            // 新旧两种字段名都要认：采集产物用 image_in1/in2，旧数据集用 shots[]。
            let (in1_path, in2_path) = match (&row.image_in1, &row.image_in2) {
                (Some(a), Some(b)) => (row.base.join(a), row.base.join(b)),
                _ if row.shots.len() >= 2 => (row.base.join(&row.shots[0]), row.base.join(&row.shots[1])),
                _ => anyhow::bail!("{}: 缺 image_in1/in2 或 shots[]", row.id),
            };
            let before = fake_capture(read_image(&in1_path)?);
            let after = fake_capture(read_image(&in2_path)?);
            let operation = if row.operation.is_empty() {
                row.executed.clone()
            } else {
                row.operation.clone()
            };

            let mut wire = build_verify_wire_messages(&VerifyWireInput {
                family: row.family,
                before: &before,
                after: &after,
                operation_text: &operation,
                tool_result: row.tool_result.as_deref(),
                // 图已经带好标记了，再重绘一次会把指针画两遍。
                current_pointer: None,
            });
            // 在最后一条 user 消息后面追加文本模式指令。
            let instruction = if row.family == OperationFamily::Scroll {
                TEXT_INSTRUCTION_SCROLL
            } else {
                TEXT_INSTRUCTION
            };
            if let Some(msg) = wire.last_mut() {
                if let Some(content) = msg.get_mut("content").and_then(|c| c.as_array_mut()) {
                    content.push(serde_json::json!({"type": "text", "text": instruction}));
                }
            }

            let out = provider
                .stream_pipeline_module_with_tools(
                    wire,
                    None,
                    vec![],
                    tokio_util::sync::CancellationToken::new(),
                    None,
                    Some("verify_action_eval"),
                )
                .await?;
            let got = parse_verdict(&out.text).with_context(|| {
                format!(
                    "{}: 无法解析判定: {:?}",
                    row.id,
                    out.text.chars().take(200).collect::<String>()
                )
            })?;
            // 生产 verdict 是大写开头（`Pass` / `Fail`）。
            let want = row.verdict.to_ascii_lowercase();

            // 滚动族：把模型读出来的位移喂进**生产**换算函数，量「该给多少
            // notches」。`failure_cause` 用采集时记录的那个 —— 文本模式下
            // 模型不产出它，而这里要测的是换算规则，不是原因分类。
            let (scale_line, scale_notches) = if row.family == OperationFamily::Scroll {
                let (moved, unit, past) = parse_scroll_fields(&out.text);
                let axis = match unit.as_deref().map(|u| u.trim().to_ascii_lowercase()) {
                    Some(u) if u == "target-widths" || u == "screen-widths" => {
                        ScrollAxis::Horizontal
                    }
                    _ => ScrollAxis::Vertical,
                };
                let conclusion = VerifyConclusion {
                    action_result: ActionResult::Pass,
                    failure_cause: row
                        .failure_cause
                        .as_deref()
                        .and_then(parse_failure_cause),
                    step_summary: None,
                    scroll: Some(ScrollMeasurement {
                        fraction: moved.unwrap_or(0.0),
                        axis,
                        past_panel: past,
                    }),
                };
                let sa = row.scroll.clone().unwrap_or_default();
                let args = serde_json::json!({
                    "notches_x": sa.notches_x,
                    "notches_y": sa.notches_y,
                });
                let line = scroll_scale_line(&args, &conclusion);
                let key = if axis == ScrollAxis::Vertical {
                    "notches_y"
                } else {
                    "notches_x"
                };
                let n = line
                    .as_deref()
                    .and_then(|l| parse_suggested_notches(l, key));
                (line, n)
            } else {
                (None, None)
            };

            let (moved_used, past_used) = if row.family == OperationFamily::Scroll {
                let (m, _, p) = parse_scroll_fields(&out.text);
                (m, p)
            } else {
                (None, false)
            };
            if row.family == OperationFamily::Scroll {
                println!(
                    "  {}: moved={:?} past_panel={} notches={:?} → 建议={:?}",
                    row.id,
                    moved_used,
                    past_used,
                    row.scroll.as_ref().and_then(|s| s.notches_y),
                    scale_notches,
                );
            }
            let used_notches = row.scroll.as_ref().and_then(|s| {
                if row.family != OperationFamily::Scroll {
                    return None;
                }
                s.notches_y.or(s.notches_x)
            });

            Ok::<_, anyhow::Error>(Outcome {
                id: row.id.clone(),
                family: row.family.prompt_key().to_string(),
                want_raw: row.verdict.clone(),
                got,
                want,
                expect: row.expect.clone(),
                expect_notches: row.expect_notches_y,
                scale_line,
                scale_notches,
                moved: moved_used,
                past_panel: past_used,
                used_notches,
            })
        }));
    }

    // 按操作族分开统计 —— 不同族的提示词不同，混在一起算一个总分没有意义。
    let mut stat: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut scale_stat: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut failures = Vec::new();
    let mut scale_failures = Vec::new();
    for h in handles {
        let o = h.await.context("join")??;
        if o.want.is_empty() || o.want == "unknown" || o.want == "pending" {
            // 没标签的样本只记录，不进分母（同 verify_position 的 `unknown`）。
            continue;
        }
        let e = stat.entry(o.family.clone()).or_insert((0, 0));
        if o.got == o.want {
            e.0 += 1;
        } else {
            e.1 += 1;
            if failures.len() < 14 {
                failures.push(format!(
                    "{} [{}]: got={} want={} (原值 {})",
                    o.id, o.family, o.got, o.want, o.want_raw
                ));
            }
        }

        // 滚动刻度：不比对「采集那次算出来的数字」—— 模型两次读数本来就不同。
        // 检查的是**规则不变量**：给定模型这一次报的位移，生产换算该走哪一支。
        if o.family == "scroll" {
            if let Some(err) = scale_rule_violation(&o) {
                let e = scale_stat.entry(o.family.clone()).or_insert((0, 0));
                e.1 += 1;
                if scale_failures.len() < 14 {
                    scale_failures.push(err);
                }
            } else if scale_stat.get(&o.family).is_some() || o.expect.is_some() {
                let e = scale_stat.entry(o.family.clone()).or_insert((0, 0));
                e.0 += 1;
            }
        }
    }

    println!("\n=== 判对率（按操作族）===");
    for (k, (ok, bad)) in &stat {
        let n = ok + bad;
        println!("{k:14} {ok:>4}/{n:<4} {:>6.2}%", *ok as f64 / n as f64 * 100.0);
    }
    let (tok, tbad) = stat.values().fold((0, 0), |a, b| (a.0 + b.0, a.1 + b.1));
    if tok + tbad > 0 {
        println!(
            "\n总体 {tok}/{} = {:.2}%",
            tok + tbad,
            tok as f64 / (tok + tbad) as f64 * 100.0
        );
    }

    if !scale_stat.is_empty() {
        println!("\n=== 滚动刻度建议（生产 scroll_scale_line 对人工核对标签）===");
        for (k, (ok, bad)) in &scale_stat {
            let n = ok + bad;
            println!("{k:14} {ok:>4}/{n:<4} {:>6.2}%", *ok as f64 / n as f64 * 100.0);
        }
    }

    if !failures.is_empty() {
        println!("\n=== 前 {} 条判定失败 ===", failures.len());
        for f in failures {
            println!("  {f}");
        }
    }
    if !scale_failures.is_empty() {
        println!("\n=== 前 {} 条刻度失败 ===", scale_failures.len());
        for f in scale_failures {
            println!("  {f}");
        }
    }
    Ok(())
}
