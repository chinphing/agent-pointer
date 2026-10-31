// 用 `verify_position` 的 148 条平移样本（37 case × up/down/left/right）量
// **refine 循环** 能不能把偏掉的框纠回来。
//
// 走生产的 `locate_one` 全流程：有 hint → 跳过整屏找 → crop → verify →
// 判否 → refine（`crop_r{n}` 或 `full_widen_r{n}`）→ 回 crop，≤ `max_locate` 轮。
//
// 判分不是「verify 判得对不对」，而是「最终交付点有没有落回真值框」——
// 这是 refine 唯一有意义的成功标准：判对了但没纠回来，对用户毫无价值。
//
// ```text
// POINTER_REFINE_EVAL_CONCURRENCY=8 cargo test -p pointer-core --test refine_loop_eval -- --nocapture
// ```
//
// 可选环境变量：
// - `POINTER_REFINE_EVAL_VARIANTS=up,down` 只跑部分变体
// - `POINTER_REFINE_EVAL_PER_VARIANT=5` 每个变体只取 N 条（快速冒烟）
// - `POINTER_REFINE_EVAL_LIMIT=10` 总条数上限
// - `POINTER_REFINE_EVAL_ALL_OUT=<path>` 逐条明细落盘（jsonl）
// - `POINTER_REFINE_EVAL_FAILS_OUT=<path>` 失败明细落盘（jsonl，不受「前 8 条」限制）
//
// 数据集在仓库之外：`<repo>/../pointer-eval-data/verify_position/`。
// 为什么复用 `verify_position` 的样本：那 148 条的 `expect.center == "no"`，
// 正是「交点没落在目标上」的输入，与 refine 的触发条件完全一致。`pos`（37 条）
// 和 `big`/`small`（74 条）的 `center == "yes"`，不该触发 refine，所以不在本评测里。

use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use pointer_core::agents::computer::locate::{
    locate_one, ChatLocateVision, LocateConfig, LocateFrame, LocateRequest, LocateVision, PxBox,
};
use pointer_core::models::ModelSettings;
use pointer_core::provider::OpenAIProvider;

/// `verify_position` 里 `expect.center == "no"` 的四个变体。
const SHIFT_VARIANTS: [&str; 4] = ["up", "down", "left", "right"];

#[derive(Deserialize, Debug, Clone)]
struct Row {
    id: String,
    variant: String,
    description: String,
    /// 整屏图（相对 `base`）。
    screen: String,
    /// 整屏图的实际像素尺寸。
    screen_px: [u32; 2],
    /// 故意挪偏的框，作为 hint 交给定位层。
    box_px: [f64; 4],
    /// 真值框。
    gt_px: [f64; 4],
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
    let settings: ModelSettings = serde_json::from_value(serde_json::json!({
        "providers": [{
            "id": "refineeval",
            "name": "refineeval",
            "baseUrl": base_url,
            "apiKey": "",
            "models": [model.clone()],
            // 必须显式声明：不写的话会按 URL 猜策略，认不出 dashscope 就落到
            // `OpenaiEffort`，而这个模型只认 `enable_thinking` —— 关不掉，
            // 每次调用跑一轮 reasoning，6–23 秒而不是 2–3 秒。
            "thinkingProtocol": "budget"
        }],
        "activeProviderId": "refineeval",
        "model": model,
        "temperature": 0.0,
        "maxTokens": 64000,
        "hasKey": true
    }))
    .context("build ModelSettings")?;
    let mut settings = settings;
    // 生产里 locate 是关 thinking 的（与 verify 一致）。必须走
    // `apply_thinking_intensity_to_round`：只设 round_enable_thinking 不够，
    // 少了 round_thinking_locked 会被后面覆盖回去。
    let intensity = std::env::var("POINTER_REFINE_EVAL_THINKING").unwrap_or_else(|_| "off".into());
    pointer_core::thinking_strategy::apply_thinking_intensity_to_round(&mut settings, Some(&intensity));
    Ok(OpenAIProvider::new(settings, api_key))
}

fn read_image(path: &Path) -> Result<image::RgbImage> {
    let img = image::open(path).with_context(|| format!("decode {}", path.display()))?;
    Ok(img.to_rgb8())
}

fn eval_data_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../pointer-eval-data/verify_position")
        .canonicalize()
        .unwrap_or_else(|_| {
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../pointer-eval-data/verify_position")
        })
}

/// 点是否落在框内（含边界）。
fn point_in_box(p: (f64, f64), b: [f64; 4]) -> bool {
    p.0 >= b[0] && p.0 <= b[2] && p.1 >= b[1] && p.1 <= b[3]
}

fn load_rows() -> Result<Vec<Row>> {
    let root = eval_data_root();
    let mut rows = Vec::new();
    let entries = std::fs::read_dir(&root).with_context(|| format!("read dir {}", root.display()))?;
    let mut dirs: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    for dir in dirs {
        let path = dir.join("truth.jsonl");
        let Ok(raw) = std::fs::read_to_string(&path) else {
            continue;
        };
        for line in raw.lines().filter(|l| !l.trim().is_empty()) {
            let mut row: Row = serde_json::from_str(line)
                .with_context(|| format!("parse {}", path.display()))?;
            if !SHIFT_VARIANTS.contains(&row.variant.as_str()) {
                continue;
            }
            row.base = dir.clone();
            rows.push(row);
        }
    }
    if rows.is_empty() {
        anyhow::bail!("no shift-variant rows under {}", root.display());
    }
    Ok(rows)
}

// worker_threads 必须够：`locate_one` 每轮都有同步的图片渲染
// （Lanczos3 → 1600×1600），那是 CPU 密集，压住 worker 线程就等于压住并发。
// `verify_position_eval` 同样用 8；渲染不行的线程数会让 148 条跑成串行。
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[ignore = "需要真实模型；POINTER_REFINE_EVAL_CONCURRENCY 控制并发"]
async fn refine_loop_eval() -> Result<()> {
    let mut rows = load_rows()?;

    // 变体过滤
    if let Ok(vs) = std::env::var("POINTER_REFINE_EVAL_VARIANTS") {
        let want: Vec<String> = vs.split(',').map(|s| s.trim().to_string()).collect();
        rows.retain(|r| want.iter().any(|w| w == &r.variant));
    }

    // 每变体取 N 条：每个变体样本数一致，否则「取前 N 条」只会取到排前面的变体
    if let Ok(n) = std::env::var("POINTER_REFINE_EVAL_PER_VARIANT") {
        let n: usize = n.parse().context("POINTER_REFINE_EVAL_PER_VARIANT")?;
        let mut seen: BTreeMap<String, usize> = BTreeMap::new();
        rows.retain(|r| {
            let e = seen.entry(r.variant.clone()).or_insert(0);
            *e += 1;
            *e <= n
        });
    }

    if let Ok(n) = std::env::var("POINTER_REFINE_EVAL_LIMIT") {
        let n: usize = n.parse().context("POINTER_REFINE_EVAL_LIMIT")?;
        rows.truncate(n);
    }

    if rows.is_empty() {
        anyhow::bail!("筛选后没有样本");
    }

    let conc: usize = std::env::var("POINTER_REFINE_EVAL_CONCURRENCY")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(8);

    let provider = Arc::new(build_provider()?);
    let sem = Arc::new(tokio::sync::Semaphore::new(conc));
    println!("refine_loop 评测 {} 条，并发 {conc}", rows.len());

    let mut handles = Vec::new();
    for row in rows {
        let provider = provider.clone();
        let sem = sem.clone();
        handles.push(tokio::spawn(async move {
            let _permit = sem.acquire().await.expect("semaphore");
            let img = read_image(&row.base.join(&row.screen))?;
            let frame = LocateFrame::new(img);
            if (frame.width(), frame.height()) != (row.screen_px[0], row.screen_px[1]) {
                anyhow::bail!(
                    "{} 尺寸不符：{}x{} vs 数据集 {}x{}",
                    row.id,
                    frame.width(),
                    frame.height(),
                    row.screen_px[0],
                    row.screen_px[1]
                );
            }

            let hint = PxBox {
                x1: row.box_px[0],
                y1: row.box_px[1],
                x2: row.box_px[2],
                y2: row.box_px[3],
            };
            let req = LocateRequest::new(row.description.clone(), row.description.clone())
                .with_hint_px(hint);

            let t0 = std::time::Instant::now();
            let cancel = tokio_util::sync::CancellationToken::new();
            let vision = ChatLocateVision::new(&provider, cancel.clone());
            let out = locate_one(&vision, &frame, &req, &LocateConfig::default(), &cancel).await?;
            let elapsed = t0.elapsed().as_secs_f64();

            let hint_center = ((hint.x1 + hint.x2) / 2.0, (hint.y1 + hint.y2) / 2.0);
            let before_hit = point_in_box(hint_center, row.gt_px);
            let after_hit = out
                .point_px
                .map(|p| point_in_box(p, row.gt_px))
                .unwrap_or(false);
            Ok::<_, anyhow::Error>((row, before_hit, after_hit, out, elapsed))
        }));
    }

    let mut by_variant: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut total = 0usize;
    let mut fixed = 0usize;
    let mut already_ok = 0usize;
    let mut still_bad = 0usize;
    let mut broke = 0usize;
    let mut round_sum = 0usize;
    let mut time_sum = 0.0f64;
    let mut failures = Vec::new();
    // 逐条明细：`failures` 只留前 8 条给 stdout，落盘要全量。
    let mut all_rows: Vec<serde_json::Value> = Vec::new();
    let mut all_fails: Vec<serde_json::Value> = Vec::new();
    // rounds 分布：1 轮就命中 = verify 首轮判通过，refine 根本没被触发。
    let mut rounds_hist: BTreeMap<usize, usize> = BTreeMap::new();
    let mut first_round_ok = 0usize;

    for h in handles {
        let (row, before_hit, after_hit, out, elapsed) = h.await??;
        total += 1;
        round_sum += out.rounds;
        time_sum += elapsed;
        let e = by_variant.entry(row.variant.clone()).or_insert((0, 0));
        e.1 += 1;
        if after_hit {
            e.0 += 1;
        }
        *rounds_hist.entry(out.rounds).or_insert(0) += 1;
        if out.rounds == 1 && after_hit {
            first_round_ok += 1;
        }
        all_rows.push(serde_json::json!({
            "id": row.id,
            "variant": row.variant,
            "before_hit": before_hit,
            "after_hit": after_hit,
            "rounds": out.rounds,
            "ok": out.ok,
            "point_px": out.point_px,
            "why": out.why,
            "elapsed_s": (elapsed * 100.0).round() / 100.0,
        }));
        match (before_hit, after_hit) {
            (true, true) => already_ok += 1,
            (false, true) => fixed += 1,
            (false, false) => {
                still_bad += 1;
                all_fails.push(serde_json::json!({
                    "id": row.id,
                    "variant": row.variant,
                    "kind": "still_bad",
                    "before_hit": before_hit,
                    "after_hit": after_hit,
                    "rounds": out.rounds,
                    "ok": out.ok,
                    "point_px": out.point_px,
                    "why": out.why,
                }));
                if failures.len() < 8 {
                    failures.push(format!(
                        "{} [{}] 未纠正 rounds={} ok={} point={:?} why={:?}",
                        row.id, row.variant, out.rounds, out.ok, out.point_px, out.why
                    ));
                }
            }
            (true, false) => {
                broke += 1;
                all_fails.push(serde_json::json!({
                    "id": row.id,
                    "variant": row.variant,
                    "kind": "broke",
                    "before_hit": before_hit,
                    "after_hit": after_hit,
                    "rounds": out.rounds,
                    "ok": out.ok,
                    "point_px": out.point_px,
                    "why": out.why,
                }));
                if failures.len() < 8 {
                    failures.push(format!(
                        "{} [{}] 改坏了：hint 本来就在框内，refine 后跑出去 rounds={} point={:?}",
                        row.id, row.variant, out.rounds, out.point_px
                    ));
                }
            }
        }
    }

    println!("\n-- 按变体（纠正率 = 最终点落回真值框）--");
    for (v, (hit, n)) in &by_variant {
        let pct = *hit as f64 / *n as f64 * 100.0;
        println!("  {v:6} {hit:3}/{n:3} = {pct:5.1}%");
    }
    println!("\n-- 汇总 --");
    println!(
        "  总计          {total:3} 条，最终命中 {}/{total} = {:.1}%",
        fixed + already_ok,
        (fixed + already_ok) as f64 / total as f64 * 100.0
    );
    println!("  纠正成功      {fixed:3} 条（hint 偏 → refine 后命中）");
    println!("  本来就对      {already_ok:3} 条（hint 已在框内，不该出现在本数据集）");
    println!("  仍未纠正      {still_bad:3} 条");
    println!("  改坏了        {broke:3} 条（hint 对 → refine 后跑出框）");
    println!(
        "  rounds_avg    {:.2}，耗时 {:.1}s/例",
        round_sum as f64 / total as f64,
        time_sum / total as f64
    );

    println!("\n-- rounds 分布（refine 实际跑了几轮）--");
    for (r, n) in &rounds_hist {
        let pct = *n as f64 / total as f64 * 100.0;
        println!("  rounds={r}  {n:3} 条  {pct:5.1}%");
    }
    println!(
        "\n  ★ 第一轮就命中（rounds=1 且落回真值框）: {first_round_ok}/{total} = {:.1}%",
        first_round_ok as f64 / total as f64 * 100.0
    );
    println!(
        "    （= verify 首轮判通过，refine 从未被触发；这些样本没检验到纠正能力）"
    );

    if let Ok(out) = std::env::var("POINTER_REFINE_EVAL_ALL_OUT") {
        let body: String = all_rows.iter().map(|v| v.to_string()).collect::<Vec<_>>().join("\n");
        if let Some(parent) = std::path::Path::new(&out).parent() {
            std::fs::create_dir_all(parent).ok();
        }
        match std::fs::write(&out, &body) {
            Ok(()) => println!("\n全部 {} 条 → {out}", all_rows.len()),
            Err(e) => println!("\n写 {out} 失败: {e}"),
        }
    }
    if let Ok(out) = std::env::var("POINTER_REFINE_EVAL_FAILS_OUT") {
        let body: String = all_fails.iter().map(|v| v.to_string()).collect::<Vec<_>>().join("\n");
        if let Some(parent) = std::path::Path::new(&out).parent() {
            std::fs::create_dir_all(parent).ok();
        }
        match std::fs::write(&out, &body) {
            Ok(()) => println!("失败 {} 条 → {out}", all_fails.len()),
            Err(e) => println!("失败 {} 条，写 {out} 失败: {e}", all_fails.len()),
        }
    }

    if !failures.is_empty() {
        println!("\n-- 前 {} 条失败 --", failures.len());
        for f in &failures {
            println!("  {f}");
        }
    }

    Ok(())
}
