// 用 `pointer-eval/scripts/verify_position_gen.py` 生成的样本量 verify 的判对率。
//
// 走生产的 `verify_pair(close, wide)` 路径，prompt 用 `verify_prompt_views`，
// 与 `engine.rs:474` 的调用完全一致。
//
// 数据集在仓库之外：`<repo>/../pointer-eval-data/verify_position/`。
// 生成器是 Python（渲染裁片、写 truth.jsonl）；本文件只负责跑和判分。
//
// ```text
// POINTER_VERIFY_EVAL_CONCURRENCY=8 cargo test -p pointer-core --test verify_position_eval -- --nocapture
// ```

use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use pointer_core::agents::computer::locate::prompt::verify_prompt_views;
use pointer_core::agents::computer::locate::render::LocateImage;
use pointer_core::agents::computer::locate::types::{CenterOnTarget, MatchVerdict, VerifyVerdict};
use pointer_core::agents::computer::locate::{ChatLocateVision, LocateVision};
use pointer_core::models::ModelSettings;
use pointer_core::provider::OpenAIProvider;

#[derive(Deserialize, Debug, Clone)]
struct Expect {
    center: String,
    size: String,
}

#[derive(Deserialize, Debug, Clone)]
struct Row {
    id: String,
    // `scene` / `sudoku` 写在 truth.jsonl 里，但判分只按 `variant` 分组。
    // serde 默认忽略未知字段，所以这里不声明 —— 声明了也没人读。
    variant: String,
    description: String,
    close: String,
    wide: String,
    expect: Expect,
    /// 这条样本所属场景目录（`<root>/sudoku` 或 `<root>/general`）。
    /// 图片路径是相对它写的，不进 truth.jsonl。
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


/// 临时验证用：不走 `report` 工具，让模型直接输出
/// 第一行 remark、第二行 yes/no。
fn text_mode_messages(close: &LocateImage, wide: Option<&LocateImage>, prompt: &str) -> Vec<serde_json::Value> {
    use serde_json::json;
    let mut content = vec![json!({"type": "text", "text": "【图一】"})];
    content.push(json!({"type": "image_url", "image_url": {"url": format!(
        "data:image/jpeg;base64,{}", base64_of(&close.jpeg))}}));
    if let Some(w) = wide {
        content.push(json!({"type": "text", "text": "【图二】"}));
        content.push(json!({"type": "image_url", "image_url": {"url": format!(
            "data:image/jpeg;base64,{}", base64_of(&w.jpeg))}}));
    }
    content.push(json!({"type": "text", "text": prompt}));
    vec![json!({"role": "user", "content": content})]
}

/// 从纯文本里取判定：最后一行 `yes` / `no`，其余是 remark。
fn parse_text_mode(text: &str) -> Option<(String, String)> {
    let trimmed = text.trim();
    let lines: Vec<&str> = trimmed.lines().collect();
    let last = lines.iter().rev().find(|l| !l.trim().is_empty())?.trim().to_string();
    let verdict = last.to_ascii_lowercase();
    if verdict != "yes" && verdict != "no" {
        return None;
    }
    let remark = lines[..lines.len().saturating_sub(1)].join("\n");
    Some((verdict, remark))
}

/// base64 编码（生产里在 `vision::screen`，这里就地实现，避免测试依赖模块可见性）。
fn base64_of(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(bytes)
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
            "id": "verifyeval",
            "name": "verifyeval",
            "baseUrl": base_url,
            "apiKey": "",
            "models": [model.clone()],
            // 必须显式声明策略：不写的话 `resolve_thinking_strategy_id` 会按 URL
            // 猜，认不出 dashscope 就落到 `OpenaiEffort`（改 reasoning_effort），
            // 而这个模型只认 `enable_thinking` —— 结果关不掉，每次 verify 都跑
            // 一轮 reasoning，6–23 秒而不是 2–3 秒。
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
    // verify 在生产里是关 thinking 的（`PipelineLlmPhase::Verify` 默认 off）。
    // 必须走 `apply_thinking_intensity_to_round`：只设 `round_enable_thinking =
    // Some(false)` 不够——少了 `round_thinking_locked = true`，后面会被覆盖回去，
    // 结果每次 verify 都跑一轮 reasoning，6–23 秒而不是 2–3 秒。
    // 默认 off（生产行为）；`POINTER_VERIFY_EVAL_THINKING=low` 打开思考，
    // 用来读模型在「usable 和结论打架」时到底怎么想的。
    let intensity = std::env::var("POINTER_VERIFY_EVAL_THINKING").unwrap_or_else(|_| "off".into());
    pointer_core::thinking_strategy::apply_thinking_intensity_to_round(&mut settings, Some(&intensity));
    Ok(OpenAIProvider::new(settings, api_key))
}

/// 评测数据在代码仓库之外：`<repo>/../pointer-eval-data/verify_position/`。
/// 按场景分子目录，每个目录内是 `truth.jsonl` + `images/{close,wide,screen}/`。
fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../pointer-eval-data/verify_position")
}

fn load_rows() -> Result<Vec<Row>> {
    let root = fixture_root();
    let mut out = Vec::new();
    for scene in ["sudoku", "general"] {
        let p = root.join(scene).join("truth.jsonl");
        let Ok(text) = std::fs::read_to_string(&p) else {
            continue;
        };
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            let mut row: Row = serde_json::from_str(line)
                .with_context(|| format!("parse {}: {}", p.display(), &line[..line.len().min(120)]))?;
            row.base = root.join(scene);
            out.push(row);
        }
    }
    Ok(out)
}

fn read_image(path: &Path) -> Result<LocateImage> {
    let jpeg = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let img = image::load_from_memory(&jpeg).with_context(|| format!("decode {}", path.display()))?;
    Ok(LocateImage {
        jpeg,
        width: img.width(),
        height: img.height(),
    })
}

fn verdict_str(c: CenterOnTarget, s: MatchVerdict) -> (String, String) {
    let cs = match c {
        CenterOnTarget::Yes => "yes",
        CenterOnTarget::No => "no",
        CenterOnTarget::Unclear => "unclear",
    };
    let ss = match s {
        MatchVerdict::Yes => "yes",
        MatchVerdict::No => "no",
        MatchVerdict::Unclear => "unclear",
    };
    (cs.to_string(), ss.to_string())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn eval_verify_fixture() -> Result<()> {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .try_init();
    let rows: Vec<Row> = load_rows()?;

    let limit: usize = std::env::var("POINTER_VERIFY_EVAL_N")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(usize::MAX);
    let only_variants: Option<Vec<String>> = std::env::var("POINTER_VERIFY_EVAL_VARIANTS")
        .ok()
        .map(|v| v.split(',').map(|s| s.trim().to_string()).collect());
    let only_ids: Option<Vec<String>> = std::env::var("POINTER_VERIFY_EVAL_IDS")
        .ok()
        .map(|v| v.split(',').map(|s| s.trim().to_string()).collect());
    let per_variant: Option<usize> = std::env::var("POINTER_VERIFY_EVAL_PER_VARIANT")
        .ok()
        .and_then(|v| v.parse().ok());
    // 每个 variant 各取 N 条，保证 7 个变体样本数一致（否则取前 N 条只会
    // 覆盖前几个 case，方差更大）。
    let rows: Vec<Row> = if let Some(vs) = &only_variants {
        rows.into_iter().filter(|r| vs.contains(&r.variant)).collect()
    } else if let Some(ids) = &only_ids {
        rows.into_iter().filter(|r| ids.contains(&r.id)).collect()
    } else { match per_variant {
        Some(k) => {
            let mut seen: BTreeMap<String, usize> = BTreeMap::new();
            rows.into_iter()
                .filter(|r| {
                    let e = seen.entry(r.variant.clone()).or_insert(0);
                    if *e < k {
                        *e += 1;
                        true
                    } else {
                        false
                    }
                })
                .collect()
        }
        None => rows.into_iter().take(limit).collect(),
    }};
    let conc: usize = std::env::var("POINTER_VERIFY_EVAL_CONCURRENCY")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(8);

    let provider = Arc::new(build_provider()?);
    let sem = Arc::new(tokio::sync::Semaphore::new(conc));
    println!("评测 {} 条，并发 {conc}", rows.len());

    let mut handles = Vec::new();
    for row in rows {
        let provider = provider.clone();
        let sem = sem.clone();
        handles.push(tokio::spawn(async move {
            let t0 = std::time::Instant::now();
            let _permit = sem.acquire().await.expect("semaphore");
            let t_perm = t0.elapsed();
            let close = read_image(&row.base.join(&row.close))?;
            let single = std::env::var("POINTER_VERIFY_EVAL_SINGLE").is_ok();
            let wide_img = if single {
                None
            } else {
                Some(read_image(&row.base.join(&row.wide))?)
            };
            let t_img = t0.elapsed();
            let mut prompt = verify_prompt_views(&row.description, wide_img.is_some());
            // 临时验证：纯文本模式要在提示词末尾追加输出格式（生产用的是
            // `report` 工具 + JSON schema，那条路径不追加任何东西）。
            if std::env::var("POINTER_VERIFY_EVAL_TEXT_MODE").is_ok() {
                prompt.push_str(
                    "\n\n## 输出格式\n\n**不要调用工具，直接用文字回答。**\n\n                     先写五项对比的总结（每项一行：甲是什么、乙是什么、一致还是不一致），\n                     然后在**最后单独一行**只写 `yes` 或 `no`，不要写别的。\n",
                );
            }
            let vision = ChatLocateVision::new(&provider, tokio_util::sync::CancellationToken::new());
            // 临时验证：POINTER_VERIFY_EVAL_TEXT_MODE=1 时走纯文本 ——
            // 不让模型填 JSON、不调工具，只让它写一段 remark 再在最后一行写 yes/no。
            // 目的：验证「结构化输出本身」是不是判定失准的来源。
            let v = if std::env::var("POINTER_VERIFY_EVAL_TEXT_MODE").is_ok() {
                let messages = text_mode_messages(&close, wide_img.as_ref(), &prompt);
                let out = provider
                    .stream_pipeline_module_with_tools(
                        messages,
                        None,
                        vec![],
                        tokio_util::sync::CancellationToken::new(),
                        None,
                        Some("locate_element"),
                    )
                    .await?;
                let (verdict, remark) =
                    parse_text_mode(&out.text).with_context(|| {
                        format!("纯文本模式解析失败: {:?}", out.text.chars().take(200).collect::<String>())
                    })?;
                let is_same = if verdict == "yes" {
                    pointer_core::agents::computer::locate::types::MatchVerdict::Yes
                } else {
                    pointer_core::agents::computer::locate::types::MatchVerdict::No
                };
                let mut vv = VerifyVerdict::default();
                vv.is_same = is_same;
                vv.notes = remark;
                vv
            } else {
                vision.verify_pair(&close, wide_img.as_ref(), &prompt).await?
            };
            let t_call = t0.elapsed();
            let (c, s) = verdict_str(v.center_on_target, v.size_ok);
            let u = match v.is_same {
                MatchVerdict::Yes => "yes",
                MatchVerdict::No => "no",
                MatchVerdict::Unclear => "unclear",
            }
            .to_string();
            if std::env::var("POINTER_VERIFY_EVAL_SHOW_REASONING").is_ok() && !v.reasoning.is_empty() {
                println!("\n----- {} reasoning ({} chars) -----\n{}", row.id, v.reasoning.chars().count(), v.reasoning);
            }
            if std::env::var("POINTER_VERIFY_EVAL_TIMING").is_ok() {
                println!(
                    "  {} 等许可={}ms 读图={}ms 调用={}ms",
                    row.id,
                    t_perm.as_millis(),
                    (t_img - t_perm).as_millis(),
                    (t_call - t_img).as_millis()
                );
            }
            Ok::<_, anyhow::Error>((row, c, s, u, v.notes, v.reasoning))
        }));
    }

    // 逐维度统计：命中 / 总
    let mut stat: BTreeMap<(String, String), (usize, usize)> = BTreeMap::new();
    let mut by_variant: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut failures: Vec<String> = Vec::new();
    let mut all_fails: Vec<serde_json::Value> = Vec::new();
    // 全量记录（含判对的）—— 校验「规则会不会误伤正确样本」需要正样本，
    // 只落失败样本时对照集永远是空的。
    let mut all_rows: Vec<serde_json::Value> = Vec::new();

    for h in handles {
        let (row, c, s, u, notes, _reasoning) = h.await.context("join")??;
        // 现在只判位置，不判 size：usable 的期望就是 center 的期望。
        let want_u = row.expect.center.clone();
        // 只报一个维度：position 是否落在目标上。size 不再判 ——
        // 框大框小不影响交付坐标，留着它只会让汇总表多一栏恒 0% 的噪声。
        let _ = &s;
        let got = [("position", u.clone(), want_u.to_string())];
        // 只有一个维度，所以 `usable_ok` 就是这一行是否全对 —— 单独再维护一个
        // `all_ok` 累加器只会是它的副本，且没有任何地方读。
        let usable_ok = u == want_u;
        for (dim, actual, want) in got {
            let key = (dim.to_string(), row.variant.clone());
            let e = stat.entry(key).or_insert((0, 0));
            if actual == want {
                e.0 += 1;
            } else {
                e.1 += 1;
            }
        }
        all_rows.push(serde_json::json!({
            "id": row.id,
            "variant": row.variant,
            "got_usable": u,
            "got_center": c,
            "got_size": s,
            "want_usable": want_u,
            "want_center": row.expect.center,
            "want_size": row.expect.size,
            "notes": notes,
        }));
        let e = by_variant.entry(row.variant.clone()).or_insert((0, 0));
        if usable_ok {
            e.0 += 1;
        } else {
            e.1 += 1;
            if failures.len() < 14 {
                let flat = notes.replace('\n', " | ");
                let brief: String = flat.chars().take(300).collect();
                let desc: String = row.description.chars().take(90).collect();
                failures.push(format!(
                    "{} [{}] u={} c={} s={} (want u={} c={} s={})\n     {}\n     desc: {}",
                    row.id, row.variant, u, c, s, want_u, row.expect.center, row.expect.size,
                    brief, desc
                ));
            }
            // 全量落盘：两次跑取交集，才分得清「固定失败」和「偶发」。
            all_fails.push(serde_json::json!({
                "id": row.id,
                "variant": row.variant,
                "got_usable": u,
                "got_center": c,
                "got_size": s,
                "want_usable": want_u,
                "want_center": row.expect.center,
                "want_size": row.expect.size,
                "notes": notes,
            }));
        }
    }

    println!("\n=== 逐项判对率（dim × variant）===");
    for ((dim, variant), (ok, bad)) in &stat {
        let tot = ok + bad;
        println!("{dim:9} {variant:6} {ok:3}/{tot:3}  {:5.1}%", *ok as f64 / tot as f64 * 100.0);
    }

    println!("\n=== 三项全对率（按 variant）===");
    for (variant, (ok, bad)) in &by_variant {
        let tot = ok + bad;
        println!("{variant:6} {ok:3}/{tot:3}  {:5.1}%", *ok as f64 / tot as f64 * 100.0);
    }

    if let Ok(out) = std::env::var("POINTER_VERIFY_EVAL_ALL_OUT") {
        let body: String = all_rows.iter().map(|v| v.to_string()).collect::<Vec<_>>().join("\n");
        if let Some(parent) = std::path::Path::new(&out).parent() {
            std::fs::create_dir_all(parent).ok();
        }
        match std::fs::write(&out, &body) {
            Ok(()) => println!("\n全部 {} 条 → {out}", all_rows.len()),
            Err(e) => println!("\n写 {out} 失败: {e}"),
        }
    }
    if let Ok(out) = std::env::var("POINTER_VERIFY_EVAL_FAILS_OUT") {
        let body: String = all_fails.iter().map(|v| v.to_string()).collect::<Vec<_>>().join("\n");
        if let Some(parent) = std::path::Path::new(&out).parent() {
            std::fs::create_dir_all(parent).ok();
        }
        match std::fs::write(&out, &body) {
            Ok(()) => println!("\n失败 {} 条 → {out}", all_fails.len()),
            Err(e) => println!("\n失败 {} 条，写 {out} 失败: {e}", all_fails.len()),
        }
    }
    if !failures.is_empty() {
        println!("\n=== 前 12 条失败 ===");
        for f in &failures {
            println!("{f}");
        }
    }
    Ok(())
}

#[test]
fn debug_thinking_settings() {
    let p = build_provider().expect("provider");
    let s = &p.settings;
    println!("round_enable_thinking={:?}", s.round_enable_thinking);
    println!("round_thinking_locked={:?}", s.round_thinking_locked);
    println!("round_thinking_intensity={:?}", s.round_thinking_intensity);
    println!("round_thinking_budget={:?}", s.round_thinking_budget);
    println!("round_reasoning_effort={:?}", s.round_reasoning_effort);
    let eb = pointer_core::models::effective_chat_extra_body(s);
    println!("extra_body={:?}", eb);
}
