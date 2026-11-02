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

use pointer_core::agents::computer::pipeline::operation::OperationFamily;
use pointer_core::agents::computer::pipeline::wire_format::{
    build_verify_wire_messages, VerifyWireInput,
};
use pointer_core::agents::computer::state::ScreenCaptureResult;
use pointer_core::agents::computer::vision::screen::MonitorInfo;
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
    #[serde(skip)]
    base: PathBuf,
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

fn build_provider() -> Result<OpenAIProvider> {
    load_dotenv();
    let base_url = std::env::var("POINTER_LOCATE_BASE_URL")
        .context("POINTER_LOCATE_BASE_URL not set (tests/locate_fixture/.env)")?;
    let api_key = std::env::var("POINTER_LOCATE_API_KEY").unwrap_or_default();
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
    let scenes: Vec<String> = match std::env::var("POINTER_VERIFY_ACTION_SCENES") {
        Ok(s) => s
            .split(',')
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .collect(),
        Err(_) => vec!["sudoku".to_string(), "general".to_string()],
    };
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
            out.push(row);
        }
    }
    Ok(out)
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

fn parse_verdict(text: &str) -> Option<String> {
    let last = text
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())?
        .trim()
        .to_ascii_lowercase();
    if last == "pass" || last == "fail" {
        Some(last)
    } else {
        None
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
                family: OperationFamily::PointerClick,
                before: &before,
                after: &after,
                operation_text: &operation,
                tool_result: row.tool_result.as_deref(),
                // 图已经带好标记了，再重绘一次会把指针画两遍。
                current_pointer: None,
            });
            // 在最后一条 user 消息后面追加文本模式指令。
            if let Some(msg) = wire.last_mut() {
                if let Some(content) = msg.get_mut("content").and_then(|c| c.as_array_mut()) {
                    content.push(serde_json::json!({"type": "text", "text": TEXT_INSTRUCTION}));
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
            Ok::<_, anyhow::Error>((row.id, row.verdict.clone(), got, want))
        }));
    }

    let mut stat: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut failures = Vec::new();
    let mut total_ok = 0usize;
    let mut total = 0usize;
    for h in handles {
        let (id, want_raw, got, want) = h.await.context("join")??;
        if want.is_empty() || want == "unknown" {
            // 没标签的样本只记录，不进分母（同 verify_position 的 `unknown`）。
            continue;
        }
        total += 1;
        let e = stat.entry("action_verify".to_string()).or_insert((0, 0));
        if got == want {
            total_ok += 1;
            e.0 += 1;
        } else {
            e.1 += 1;
            if failures.len() < 14 {
                failures.push(format!("{id}: got={got} want={want} (原值 {want_raw})"));
            }
        }
    }

    println!("\n=== 判对率 ===");
    for (k, (ok, bad)) in &stat {
        let n = ok + bad;
        println!("{k:14} {ok:>4}/{n:<4} {:>6.2}%", *ok as f64 / n as f64 * 100.0);
    }
    if total > 0 {
        println!(
            "\n总体 {total_ok}/{total} = {:.2}%",
            total_ok as f64 / total as f64 * 100.0
        );
    }
    if !failures.is_empty() {
        println!("\n=== 前 {} 条失败 ===", failures.len());
        for f in failures {
            println!("  {f}");
        }
    }
    Ok(())
}
