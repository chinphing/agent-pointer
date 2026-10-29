//! ScreenSpot-Pro, run directly against the Rust locate engine.
//!
//! This is the real benchmark: 1581 GUI-grounding cases across 26 professional
//! applications, each on its own 4K screenshot. The Python harness under
//! `benchmarks/screenspot_grid/` established the splits, the structured
//! descriptions and the scoring rule; this file reuses all three but calls the
//! production engine instead of a Python re-implementation.
//!
//! # Scoring
//!
//! `hit` = the returned **point** falls inside the ground-truth box. Not the
//! box, the point — the box is only a candidate region, and what gets delivered
//! to the caller is one coordinate. This matches `bench/runner.py`:
//!
//! ```python
//! r["hit"] = bool(fp and gb[0] <= fp[0] <= gb[2] and gb[1] <= fp[1] <= gb[3])
//! ```
//!
//! # Descriptions
//!
//! ScreenSpot-Pro's raw `instruction` is one English sentence. The harness ships
//! `descriptions/v2/<split>.json` instead: four fields per id — 外观特征 / 控件
//! 类型 / 屏幕位置 / 周边锚点. Those four are concatenated and passed as
//! `target_desc`, because that is the shape `locate.md` expects and it is what
//! the Python baselines were scored on. Comparing against their numbers without
//! it would be comparing two different questions.
//!
//! # Running
//!
//! ```bash
//! # 20 cases, the split the Python baselines were tuned on
//! POINTER_SP_SPLIT=M1 cargo test --release -p pointer-core \
//!   --test screenspot_pro screenspot_pro -- --ignored --nocapture
//!
//! # a specific case, to debug one failure
//! POINTER_SP_IDS=linux_common_linux_42 cargo test --release -p pointer-core \
//!   --test screenspot_pro screenspot_pro -- --ignored --nocapture
//! ```
//!
//! Env: `POINTER_SP_SPLIT` (M1 | M2 | M3, default M1), `POINTER_SP_IDS`
//! (comma-separated ids, overrides the split), `POINTER_SP_LIMIT`,
//! `POINTER_LOCATE_CONCURRENCY` (default 8), `POINTER_LOCATE_TIMEOUT_SECS`
//! (default 400). The API key comes from `tests/locate_fixture/.env`, the same
//! file the other model-backed tests use.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use futures_util::stream::{FuturesUnordered, StreamExt};
use serde::Deserialize;
use tokio_util::sync::CancellationToken;

use pointer_core::agents::computer::locate::{
    locate_one, ChatLocateVision, LocateConfig, LocateFrame, LocateRequest, PxBox,
};
use pointer_core::models::ModelSettings;
use pointer_core::provider::OpenAIProvider;

/// The production-shaped main-LLM call, reused from the computer e2e harness so
/// the "agent" arm here measures the same thing it measures there: `AGENT.md`
/// plus the `[CUR_SCREEN]` preamble, not a test-only prompt.
#[path = "computer_e2e/harness.rs"]
mod harness;

/// What text the pipeline is asked to ground.
///
/// - `block` — `descriptions/v2` structured blocks (five labelled fields). This
///   is what the Python baselines were scored on, and what `locate.md` expects.
/// - `raw`   — ScreenSpot-Pro's own `instruction`, one short English fragment
///   ("Und", "Bold"), wrapped in the same task declaration the official
///   implementations use ("find the bounding box … don't output analysis").
///   The instruction itself is unchanged; only the task framing is added,
///   because without it a general-purpose agent prompt has no reason to think
///   this is a grounding request at all.
#[derive(Clone, Copy, PartialEq, Eq)]
enum DescMode {
    Block,
    Raw,
}

impl DescMode {
    fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "raw" => DescMode::Raw,
            _ => DescMode::Block,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            DescMode::Block => "block",
            DescMode::Raw => "raw",
        }
    }
}

/// Which arm to run.
///
/// - `none`  — locate alone, no hint. This is what this file used to do.
/// - `agent` — main LLM produces a bbox, then locate verifies/refines it.
///   The main-LLM call reuses `computer_e2e/harness.rs::agent_hint`, so the
///   prompt assembly (`AGENT.md` + `[CUR_SCREEN]` preamble + slot labels +
///   `mouse.schema.yaml`) is the same one the computer e2e benchmark measures.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    None,
    Agent,
}

impl Mode {
    fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "agent" => Mode::Agent,
            _ => Mode::None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Mode::None => "none",
            Mode::Agent => "agent",
        }
    }
}

/// ScreenSpot 数据根。代码在 `pointer-eval/screenspot_grid/`，图片和标注在
/// `pointer-eval-data/screenspot_grid/`——两者与 `agent-pointer/` 同级。
///
/// `POINTER_SP_DATA` 可覆盖（CI、只读挂载、数据在另一块盘上）。
fn bench_dir() -> PathBuf {
    if let Ok(p) = std::env::var("POINTER_SP_DATA") {
        return PathBuf::from(p);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../pointer-eval-data/screenspot_grid")
}

/// `tests/locate_fixture/.env` — shared with the other model-backed tests so the
/// key is configured once.
fn dotenv_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/locate_fixture/.env")
}

fn load_dotenv() {
    let Ok(raw) = std::fs::read_to_string(dotenv_path()) else {
        return;
    };
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let (k, v) = (k.trim(), v.trim().trim_matches('"').trim_matches('\''));
        if k.is_empty() || std::env::var_os(k).is_some() {
            continue;
        }
        std::env::set_var(k, v);
    }
}

/// One row of `data/annotations/<app>.json`.
#[derive(Debug, Clone, Deserialize)]
struct ScreenSpotCase {
    id: String,
    img_filename: String,
    /// Ground truth in **original screenshot pixels**.
    bbox: [f64; 4],
    img_size: [u32; 2],
    #[serde(default)]
    application: String,
    #[serde(default)]
    platform: String,
    #[serde(default)]
    ui_type: String,
    #[serde(default)]
    group: String,
    /// ScreenSpot-Pro's own instruction — one short English sentence, often a
    /// fragment ("Und", "Bold"). This is what the benchmark actually asks; the
    /// structured `descriptions/v2` blocks were written later to make the task
    /// solvable by a grounder.
    #[serde(default)]
    instruction: String,
    #[serde(default)]
    instruction_cn: String,
}

/// One row of `descriptions/v2/<split>.json`.
#[derive(Debug, Clone, Default, Deserialize)]
struct Description {
    #[serde(default)]
    text: String,
    #[serde(default)]
    feat: String,
    #[serde(default)]
    ctrl: String,
    #[serde(default)]
    pos: String,
    #[serde(default)]
    env: String,
}

impl Description {
    /// Same four fields, same labels, same order as `b5_decoupled._desc_block`.
    /// `text` is prepended when present — some ids describe the label only.
    fn block(&self) -> String {
        let mut out = String::new();
        if !self.text.trim().is_empty() {
            out.push_str(&format!("· 文字内容：{}\n", self.text.trim()));
        }
        for (value, label) in [
            (&self.feat, "外观特征"),
            (&self.ctrl, "控件类型"),
            (&self.pos, "屏幕位置"),
            (&self.env, "周边锚点"),
        ] {
            if !value.trim().is_empty() {
                out.push_str(&format!("· {label}：{}\n", value.trim()));
            }
        }
        out
    }
}

fn load_cases() -> Result<Vec<ScreenSpotCase>> {
    let anno_dir = bench_dir().join("data/annotations");
    let mut all = Vec::new();
    for entry in std::fs::read_dir(&anno_dir)
        .with_context(|| format!("read {}", anno_dir.display()))?
    {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("read {}", path.display()))?;
        let mut rows: Vec<ScreenSpotCase> = serde_json::from_str(&raw)
            .with_context(|| format!("parse {}", path.display()))?;
        all.append(&mut rows);
    }
    anyhow::ensure!(!all.is_empty(), "no annotations under {}", anno_dir.display());
    all.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(all)
}

fn load_split(name: &str) -> Result<Vec<String>> {
    let path = bench_dir().join("splits").join(format!("{name}.json"));
    let raw = std::fs::read_to_string(&path).with_context(|| {
        format!(
            "read {} — set POINTER_SP_SPLIT to M1/M2/M3 or POINTER_SP_IDS",
            path.display()
        )
    })?;
    serde_json::from_str(&raw).with_context(|| format!("parse {}", path.display()))
}

/// The ids actually scored for a split.
///
/// `splits/<split>.json` is **not** authoritative: `M2.json` holds 100 ids that
/// do not intersect `descriptions/v2/M2.json` at all, while `META.json` records
/// M2 as 300 cases and `descriptions/v2/M2.json` has exactly 300 keys — all of
/// which are in the annotations. The Python harness only ever scores
/// `ids ∩ descriptions`, so its M2 is those 300. Using the split file alone
/// silently yields zero runnable cases.
///
/// So: **the description file defines the split.** The split file is only a
/// fallback for a split that has no descriptions.
fn load_split_ids(name: &str) -> Result<Vec<String>> {
    let desc_path = bench_dir()
        .join("descriptions/v2")
        .join(format!("{name}.json"));
    if let Ok(raw) = std::fs::read_to_string(&desc_path) {
        let map: serde_json::Map<String, serde_json::Value> =
            serde_json::from_str(&raw).with_context(|| format!("parse {}", desc_path.display()))?;
        let mut ids: Vec<String> = map.keys().cloned().collect();
        ids.sort();
        anyhow::ensure!(
            !ids.is_empty(),
            "{} has no descriptions for split {name}",
            desc_path.display()
        );
        return Ok(ids);
    }
    load_split(name)
}

fn load_descriptions(split: &str) -> Result<HashMap<String, Description>> {
    let path = bench_dir()
        .join("descriptions/v2")
        .join(format!("{split}.json"));
    let raw = std::fs::read_to_string(&path).with_context(|| {
        format!(
            "read {} — the descriptions are what the baselines were scored on",
            path.display()
        )
    })?;
    serde_json::from_str(&raw).with_context(|| format!("parse {}", path.display()))
}

/// Every description across M1/M2/M3, keyed by id.
///
/// `POINTER_SP_IDS` may name an id from any split, but a single split's file
/// only holds its own ids — asking for `vscode_macos_19` (M2) while
/// `POINTER_SP_SPLIT` defaults to M1 finds nothing and aborts with "no runnable
/// cases". Merging is safe: an id belongs to one split, and the content is the
/// same object.
fn load_all_descriptions() -> Result<HashMap<String, Description>> {
    let mut out = HashMap::new();
    let dir = bench_dir().join("descriptions/v2");
    let mut found = 0;
    for split in ["M1", "M2", "M3"] {
        let path = dir.join(format!("{split}.json"));
        let Ok(raw) = std::fs::read_to_string(&path) else {
            continue;
        };
        let map: HashMap<String, Description> =
            serde_json::from_str(&raw).with_context(|| format!("parse {}", path.display()))?;
        out.extend(map);
        found += 1;
    }
    anyhow::ensure!(
        found > 0,
        "no description files under {} — the benchmark cannot run without them",
        dir.display()
    );
    Ok(out)
}

fn env_usize(key: &str, default: usize) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(default)
}

fn thinking_enabled() -> bool {
    matches!(
        std::env::var("POINTER_SP_THINKING").as_deref(),
        Ok("1") | Ok("on") | Ok("true")
    )
}

/// Same default as `locate_fixture`: **off**.
///
/// Leaving the provider default (on for qwen3.8-flash) makes every call emit
/// ~3000 reasoning tokens first: measured on ScreenSpot-Pro, 25–53 s per call
/// instead of 3–5 s. The sudoku benchmark runs with thinking off, so scoring
/// this one with it on would be measuring two different systems.
fn thinking_setting() -> Option<bool> {
    if thinking_enabled() {
        None // leave the provider default alone
    } else {
        Some(false)
    }
}

fn build_provider() -> Result<OpenAIProvider> {
    load_dotenv();
    let base_url = std::env::var("POINTER_LOCATE_BASE_URL")
        .context("POINTER_LOCATE_BASE_URL is not set (see tests/locate_fixture/.env)")?;
    let api_key = std::env::var("POINTER_LOCATE_API_KEY").unwrap_or_default();
    let model =
        std::env::var("POINTER_LOCATE_MODEL").unwrap_or_else(|_| "qwen3.8-flash".to_string());
    let settings: ModelSettings = serde_json::from_value(serde_json::json!({
        "providers": [{
            "id": "screenspot",
            "name": "screenspot",
            "baseUrl": base_url,
            "apiKey": "",
            "models": [model.clone()]
        }],
        "activeProviderId": "screenspot",
        "model": model,
        "temperature": 0.0,
        "maxTokens": 64000,
        "hasKey": true
    }))
    .context("build ModelSettings from env")?;
    let mut settings = settings;
    settings.round_enable_thinking = thinking_setting();
    Ok(OpenAIProvider::new(settings, api_key))
}

/// Provider for the main-LLM arm.
///
/// Must carry `supportsVision` — without it `make_openai_messages` replaces the
/// screenshot with the text "(image not inlined: primary model does not support
/// vision)" and the model answers blind. Must also carry `enable_thinking` in
/// `extraBody` — `round_enable_thinking` alone does not put the flag on the
/// wire, and without it `reasoning_content` comes back empty. Both are what
/// `computer_e2e`'s provider sets, for the same reasons.
fn build_agent_provider() -> Result<OpenAIProvider> {
    load_dotenv();
    let base_url = std::env::var("POINTER_LOCATE_BASE_URL")
        .context("POINTER_LOCATE_BASE_URL is not set (see tests/locate_fixture/.env)")?;
    let api_key = std::env::var("POINTER_LOCATE_API_KEY").unwrap_or_default();
    let model =
        std::env::var("POINTER_LOCATE_MODEL").unwrap_or_else(|_| "qwen3.8-flash".to_string());
    let thinking = thinking_enabled();
    let settings: ModelSettings = serde_json::from_value(serde_json::json!({
        "providers": [{
            "id": "screenspot-agent",
            "name": "screenspot-agent",
            "baseUrl": base_url,
            "apiKey": "",
            "models": [model.clone()],
            "modelConfigs": { model.clone(): { "supportsVision": true } },
            "extraBody": { "enable_thinking": thinking }
        }],
        "activeProviderId": "screenspot-agent",
        "model": model,
        // Production runs 0.7 on the main LLM (see the llm_prompts dumps).
        "temperature": 0.7,
        "maxTokens": 64000,
        "hasKey": true
    }))
    .context("build agent ModelSettings from env")?;
    let mut settings = settings;
    settings.round_enable_thinking = Some(thinking);
    // Computer tier default (`session_model.rs`): budget 1024, i.e. "low".
    settings.round_thinking_budget = Some(1024);
    Ok(OpenAIProvider::new(settings, api_key))
}

struct CaseOutcome {
    id: String,
    /// Returned point in original screenshot pixels, when the engine produced one.
    point: Option<(f64, f64)>,
    /// Returned box in original screenshot pixels. Recorded so failures can be
    /// split into 「found the right element, aimed badly」 and 「found the wrong
    /// element」 — the point alone cannot tell those apart.
    box_px: Option<[f64; 4]>,
    ok: bool,
    rounds: usize,
    secs: f64,
    app: String,
    platform: String,
    ui_type: String,
    group: String,
    target_len: f64,
}

#[tokio::test]
#[ignore = "calls a real vision model; run with --ignored"]
async fn screenspot_pro() -> Result<()> {
    let _ = env_logger::builder()
        .format_timestamp_millis()
        .is_test(false)
        .try_init();

    let split = std::env::var("POINTER_SP_SPLIT").unwrap_or_else(|_| "M1".to_string());
    let explicit_ids = std::env::var("POINTER_SP_IDS")
        .ok()
        .filter(|raw| !raw.trim().is_empty());
    let ids: Vec<String> = match &explicit_ids {
        Some(raw) => raw
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect(),
        None => load_split_ids(&split)?,
    };
    // Explicit ids may come from any split, so look across all three.
    let descriptions = if explicit_ids.is_some() {
        load_all_descriptions()?
    } else {
        load_descriptions(&split)?
    };
    let by_id: HashMap<String, ScreenSpotCase> =
        load_cases()?.into_iter().map(|c| (c.id.clone(), c)).collect();

    let mut selected = Vec::new();
    let mut missing = Vec::new();
    let mut no_desc = Vec::new();
    let desc_mode =
        DescMode::parse(&std::env::var("POINTER_SP_DESC").unwrap_or_else(|_| "block".into()));
    for id in &ids {
        let Some(case) = by_id.get(id) else {
            missing.push(id.clone());
            continue;
        };
        // Only the `block` arm needs a descriptions/v2 row. The `raw` arm reads
        // the dataset's own instruction, so a missing block is not a skip.
        if desc_mode == DescMode::Block && !descriptions.contains_key(id) {
            no_desc.push(id.clone());
            continue;
        }
        selected.push(case.clone());
    }
    if !missing.is_empty() {
        println!(
            "  {} id(s) not in the dataset, skipped: {:?}",
            missing.len(),
            &missing[..missing.len().min(3)]
        );
    }
    if !no_desc.is_empty() {
        println!(
            "  {} id(s) have no description, skipped: {:?}",
            no_desc.len(),
            &no_desc[..no_desc.len().min(3)]
        );
    }
    let limit = env_usize("POINTER_SP_LIMIT", 0);
    if limit > 0 && selected.len() > limit {
        selected.truncate(limit);
    }
    anyhow::ensure!(!selected.is_empty(), "no runnable cases for split {split}");

    let concurrency = env_usize("POINTER_LOCATE_CONCURRENCY", 8).clamp(1, 16);
    let timeout = Duration::from_secs(env_usize("POINTER_LOCATE_TIMEOUT_SECS", 400) as u64);
    let provider = Arc::new(build_provider()?);
    let mode = Mode::parse(&std::env::var("POINTER_SP_MODE").unwrap_or_else(|_| "none".into()));
    // Only the main-LLM arm needs this provider; building it unconditionally
    // would fail the `none` arm on a machine without the agent model configured.
    let agent_provider = match mode {
        Mode::None => None,
        Mode::Agent => Some(Arc::new(build_agent_provider()?)),
    };

    println!(
        "screenspot-pro: split={split} mode={} desc={} cases={} concurrency={concurrency} timeout={}s thinking={}",
        mode.as_str(),
        desc_mode.as_str(),
        selected.len(),
        timeout.as_secs(),
        if thinking_enabled() { "on" } else { "off" }
    );

    let mut outcomes: Vec<CaseOutcome> = Vec::new();
    let mut running = FuturesUnordered::new();
    let mut queue = selected.into_iter();

    loop {
        while running.len() < concurrency {
            let Some(case) = queue.next() else { break };
            let desc = match desc_mode {
                DescMode::Block => descriptions
                    .get(&case.id)
                    .map(|d| d.block())
                    .unwrap_or_default(),
                // The benchmark's own instruction, verbatim — but with the task
                // declaration both official implementations carry. ScreenSpot-Pro's
                // own gpt4x.py prefixes "You are asked to find the bounding box of
                // an UI element ... Don't output any analysis"; qwen2_5vl.py goes
                // further and pre-fills the tool-call prefix. Without a declaration
                // the model sees only a word ("Redo") plus a general-purpose agent
                // prompt and reasonably asks what the user wants.
                DescMode::Raw => format!(
                    "Find the UI element in this screenshot that the instruction below \
                     refers to, and call the pointing tool with its bounding box. \
                     Don't output analysis — the tool call is the answer.\n\n\
                     The instruction is:\n{}",
                    case.instruction
                ),
            };
            let provider = Arc::clone(&provider);
            let agent_provider = agent_provider.clone();
            running.push(tokio::spawn(async move {
                let t0 = std::time::Instant::now();
                let img_path = bench_dir().join("data/images").join(&case.img_filename);
                let outcome = async {
                    let img = image::open(&img_path).with_context(|| {
                        format!("open {} (run scripts/download_data.py?)", img_path.display())
                    })?;
                    // The main-LLM arm needs the raw JPEG bytes: `agent_hint`
                    // rebuilds the production `[CUR_SCREEN]` payload from them.
                    let hint = match (&mode, &agent_provider) {
                        (Mode::Agent, Some(ap)) => {
                            let mut jpeg: Vec<u8> = Vec::new();
                            let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(
                                std::io::Cursor::new(&mut jpeg),
                                90,
                            );
                            enc.encode_image(&img)
                                .context("encode frame as jpeg for agent hint")?;
                            let a = harness::agent_hint(
                                ap,
                                &jpeg,
                                (img.width(), img.height()),
                                &desc,
                            )
                            .await?;
                            eprintln!(
                                "[screenspot] hint {} norm=[{:.0},{:.0},{:.0},{:.0}] tool={}",
                                case.id, a.box_norm[0], a.box_norm[1], a.box_norm[2], a.box_norm[3],
                                a.tool
                            );
                            Some(PxBox {
                                x1: a.box_px[0],
                                y1: a.box_px[1],
                                x2: a.box_px[2],
                                y2: a.box_px[3],
                            })
                        }
                        _ => None,
                    };
                    let frame = LocateFrame::new(img.to_rgb8());
                    let cancel = CancellationToken::new();
                    let vision = ChatLocateVision::new(&provider, cancel.clone());
                    // `goal` is caller-facing metadata; the grounder never sees it
                    // (see `locate_prompt`). Any non-empty string will do.
                    let mut req = LocateRequest::new(desc, "点击它");
                    if let Some(h) = hint {
                        req = req.with_hint_px(h);
                    }
                    let out = locate_one(&vision, &frame, &req, &LocateConfig::default(), &cancel)
                        .await?;
                    Ok::<_, anyhow::Error>(out)
                };
                match tokio::time::timeout(timeout, outcome).await {
                    Ok(Ok(out)) => {
                        let tb = case.bbox;
                        let target_len = (tb[2] - tb[0]).max(tb[3] - tb[1]);
                        CaseOutcome {
                            id: case.id.clone(),
                            point: out.point_px,
                            box_px: out.box_px.map(|b| [b.x1, b.y1, b.x2, b.y2]),
                            ok: out.ok,
                            rounds: out.rounds,
                            secs: t0.elapsed().as_secs_f64(),
                            app: case.application.clone(),
                            platform: case.platform.clone(),
                            ui_type: case.ui_type.clone(),
                            group: case.group.clone(),
                            target_len,
                        }
                    }
                    Ok(Err(e)) => {
                        // A swallowed error here reads as "the model was wrong",
                        // which is exactly the confusion this benchmark exists to
                        // avoid. Say what actually happened.
                        println!("  ERROR {:<28} {e:#}", case.id);
                        CaseOutcome {
                            id: case.id.clone(),
                            point: None,
                            box_px: None,
                            ok: false,
                            rounds: 0,
                            secs: t0.elapsed().as_secs_f64(),
                            app: case.application.clone(),
                            platform: case.platform.clone(),
                            ui_type: case.ui_type.clone(),
                            group: case.group.clone(),
                            target_len: (case.bbox[2] - case.bbox[0])
                                .max(case.bbox[3] - case.bbox[1]),
                        }
                    }
                    Err(_elapsed) => {
                        println!("  TIMEOUT {:<26} after {}s", case.id, timeout.as_secs());
                        CaseOutcome {
                            id: case.id.clone(),
                            point: None,
                            box_px: None,
                            ok: false,
                            rounds: 0,
                            secs: t0.elapsed().as_secs_f64(),
                            app: case.application.clone(),
                            platform: case.platform.clone(),
                            ui_type: case.ui_type.clone(),
                            group: case.group.clone(),
                            target_len: (case.bbox[2] - case.bbox[0])
                                .max(case.bbox[3] - case.bbox[1]),
                        }
                    }
                }
            }));
        }
        let Some(joined) = running.next().await else { break };
        match joined {
            Ok(o) => outcomes.push(o),
            Err(e) => println!("  task panicked: {e}"),
        }
    }

    outcomes.sort_by(|a, b| a.id.cmp(&b.id));

    let mut hits = 0usize;
    let mut claimed = 0usize;
    let mut claimed_but_missed = 0usize;
    let mut offsets = Vec::new();
    for o in &outcomes {
        let Some(case) = by_id.get(&o.id) else { continue };
        let tb = case.bbox;
        let landed = o.point.is_some_and(|(x, y)| {
            x >= tb[0] && x <= tb[2] && y >= tb[1] && y <= tb[3]
        });
        if landed {
            hits += 1;
        }
        if o.ok {
            claimed += 1;
            if !landed {
                claimed_but_missed += 1;
            }
        }
        if let Some((x, y)) = o.point {
            let (gcx, gcy) = ((tb[0] + tb[2]) / 2.0, (tb[1] + tb[3]) / 2.0);
            offsets.push(((x - gcx).powi(2) + (y - gcy).powi(2)).sqrt());
        }
        println!(
            "{} {:<28} ok={:<5} pt={:?} rounds={} {:.1}s  {}/{}",
            if landed { "PASS" } else { "FAIL" },
            o.id,
            o.ok,
            o.point.map(|(x, y)| (x.round() as i64, y.round() as i64)),
            o.rounds,
            o.secs,
            o.app,
            o.ui_type
        );
    }

    let n = outcomes.len();
    offsets.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median = offsets.get(offsets.len() / 2).copied().unwrap_or(f64::NAN);
    let mean = if offsets.is_empty() {
        f64::NAN
    } else {
        offsets.iter().sum::<f64>() / offsets.len() as f64
    };
    let mean_secs = if n == 0 {
        0.0
    } else {
        outcomes.iter().map(|o| o.secs).sum::<f64>() / n as f64
    };
    let mean_rounds = if n == 0 {
        0.0
    } else {
        outcomes.iter().map(|o| o.rounds as f64).sum::<f64>() / n as f64
    };

    println!("\n=== screenspot-pro {split} ===");
    println!("  cases            : {n}");
    println!(
        "  hit (point in gt): {hits}/{n} = {:.1}%",
        hits as f64 * 100.0 / n.max(1) as f64
    );
    println!("  claimed ok       : {claimed}  (claimed_but_missed={claimed_but_missed})");
    println!("  offset median    : {median:.1}px   mean: {mean:.1}px");
    println!("  mean rounds      : {mean_rounds:.2}");
    println!("  mean per case    : {mean_secs:.1}s");

    Ok(())
}
