//! Computer-use end-to-end grounding benchmark.
//!
//! Dataset: `benchmarks/computer_e2e/data/e2e_truth.json` — one case per real
//! production `locate` run, each carrying the full-screen frame the locator saw
//! and the box the pipeline actually clicked.
//!
//! Metrics follow `benchmarks/screenspot_grid/bench/metrics.py`:
//!   · hit      — final click point falls inside the ground-truth box
//!   · bucket   — ground-truth long edge: <40px / 40–120px / >120px
//!   · cover    — macro (pred box fully wraps gt) / micro (∩ / gt area)
//!
//! Three hint modes, so a single run attributes error to the right layer:
//!   `none`   — whole-screen locate, no hint. Baseline: what the locator alone can do.
//!   `oracle` — ground-truth box as the hint. Upper bound for this locate build.
//!   `agent`  — the real main LLM produces the hint from (frame, goal). This is
//!              the production path, and the number this benchmark exists for.
//!
//! Run:
//!   cargo test -p pointer-core --test computer_e2e -- --ignored --nocapture
//!   POINTER_E2E_SCOPE=smoke cargo test ... (10 runs, ~5 min)
//!   POINTER_E2E_SCOPE=sample cargo test ... (20% of runs)
//!   POINTER_E2E_LIMIT=10 cargo test ... (raw row count, overrides scope)

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use pointer_core::agents::computer::locate::{
    locate_elements, ChatLocateVision, LocateConfig, LocateFrame, LocateRequest, LocateVision,
    PxBox,
};
use pointer_core::agents::computer::locate::prompt::verify_prompt_views;
use pointer_core::agents::computer::locate::render::LocateImage;
use pointer_core::agents::computer::locate::types::VerifyVerdict;
use pointer_core::models::ModelSettings;
use pointer_core::provider::OpenAIProvider;
use futures_util::StreamExt;
use serde::Deserialize;
use tokio_util::sync::CancellationToken;

mod harness;

// ---------------------------------------------------------------------------
// dataset scope
// ---------------------------------------------------------------------------

/// How much of a dataset to run.
///
/// A full locate run is ~831 model calls: the right size for the number that goes
/// into a baseline file, the wrong size for "did my edit break anything".
///
/// Sampling is **by run, not by row.** `locate.jsonl` has one row per round, so
/// `LIMIT 10` can return ten rounds of a single run and say nothing about whether
/// other targets still resolve. Keeping a run's rows together also keeps the
/// multi-round story intact: round 2 only means "corrected after a retry" when
/// round 1 is beside it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Scope {
    /// ~10 runs. Seconds to a minute; use while editing.
    Smoke,
    /// ~20% of runs. A regression shows up without paying for the full set.
    Sample,
    /// Everything. This is the number a baseline records.
    Full,
}

impl Scope {
    fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "" | "full" | "all" => Some(Self::Full),
            "smoke" => Some(Self::Smoke),
            "sample" | "partial" => Some(Self::Sample),
            _ => None,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Smoke => "smoke",
            Self::Sample => "sample",
            Self::Full => "full",
        }
    }

    /// How many distinct runs to keep, given how many the dataset has.
    fn runs(self, total: usize) -> usize {
        match self {
            Self::Smoke => 10.min(total),
            // A fifth, but never fewer than the smoke set — on a small dataset a
            // fifth can be smaller than smoke, which would make "sample" a
            // weaker check than "smoke".
            Self::Sample => (total / 5).max(10).min(total),
            Self::Full => total,
        }
    }
}

/// `POINTER_E2E_SCOPE=smoke|sample|full`, default `full`.
///
/// An unknown value panics instead of falling back: silently running the full
/// set because of a typo costs an hour, and silently running smoke looks like a
/// pass.
fn scope_from_env() -> Scope {
    let raw = std::env::var("POINTER_E2E_SCOPE").unwrap_or_default();
    Scope::parse(&raw).unwrap_or_else(|| {
        panic!("POINTER_E2E_SCOPE={raw:?} is not smoke|sample|full")
    })
}

/// Keep every row whose run id is among the first `n` distinct ones.
fn take_runs<T>(rows: Vec<T>, n: usize, run_of: impl Fn(&T) -> &str) -> Vec<T> {
    let mut kept: Vec<String> = Vec::new();
    for r in &rows {
        let id = run_of(r);
        if kept.iter().any(|k| k == id) {
            continue;
        }
        if kept.len() == n {
            break;
        }
        kept.push(id.to_string());
    }
    rows.into_iter()
        .filter(|r| kept.iter().any(|k| k == run_of(r)))
        .collect()
}

/// Apply `ONLY` → `SCOPE` → `LIMIT`, in that order.
///
/// `ONLY` narrows to one run/scene; `SCOPE` picks how many runs; `LIMIT` is the
/// escape hatch that overrides both and keeps the old `LIMIT=10` invocation
/// working.
fn select<T>(
    rows: Vec<T>,
    only: Option<&str>,
    only_matches: impl Fn(&T, &str) -> bool,
    run_of: impl Fn(&T) -> &str,
    limit: Option<usize>,
) -> Vec<T> {
    let mut rows = rows;
    if let Some(o) = only {
        rows.retain(|r| only_matches(r, o));
    }
    if let Some(n) = limit {
        rows.truncate(n);
        return rows;
    }
    let scope = scope_from_env();
    let total = {
        let mut ids: Vec<&str> = rows.iter().map(&run_of).collect();
        ids.sort_unstable();
        ids.dedup();
        ids.len()
    };
    let want = scope.runs(total);
    if want < total {
        rows = take_runs(rows, want, run_of);
    }
    rows
}


// ---------------------------------------------------------------------------
// dataset
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct TruthCase {
    #[serde(default)]
    scene: String,
    description: String,
    #[serde(default)]
    goal: Option<String>,
    bbox: [f64; 4],
    fixture: String,
    #[serde(default)]
    size: Option<[u32; 2]>,
}

#[derive(Debug, Deserialize)]
struct TruthFile {
    #[serde(default)]
    cases: Vec<TruthCase>,
}

/// Repository root (`.../agent-pointer`), resolved from the crate manifest so
/// the benchmark works no matter which directory cargo is invoked from.
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crate lives at <repo>/crates/pointer-core")
        .to_path_buf()
}

/// `pointer-eval-data/` sits **next to** this repo, not inside it: the fixtures
/// are screenshots and captured JSONL, not source. Keeping them out of the
/// checkout is what lets the repo stay small and the data be replaced wholesale.
///
/// Override with `POINTER_EVAL_DATA` when the data lives elsewhere (CI, another
/// machine, a read-only mount).
fn eval_data_root() -> PathBuf {
    if let Ok(p) = std::env::var("POINTER_EVAL_DATA") {
        return PathBuf::from(p);
    }
    repo_root()
        .parent()
        .map(|p| p.join("pointer-eval-data"))
        .unwrap_or_else(|| repo_root().join("pointer-eval-data"))
}

fn data_dir() -> PathBuf {
    eval_data_root().join("computer_e2e")
}

/// Where the screenshots live. They sit beside `e2e_truth.json` in the data
/// root, so `POINTER_E2E_IMAGE_DIR` is only needed when a run should use a
/// different image set than the one the truth file was built from.
fn image_dir() -> PathBuf {
    if let Ok(p) = std::env::var("POINTER_E2E_IMAGE_DIR") {
        return PathBuf::from(p);
    }
    data_dir()
}

fn load_truth() -> Result<TruthFile> {
    let path = data_dir().join("e2e_truth.json");
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("read {}", path.display()))?;
    serde_json::from_str(&raw).with_context(|| format!("parse {}", path.display()))
}

// ---------------------------------------------------------------------------
// provider
// ---------------------------------------------------------------------------

fn load_dotenv() {
    let path = repo_root().join("crates/pointer-core/tests/locate_fixture/.env");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return;
    };
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            let k = k.trim();
            if std::env::var_os(k).is_none() {
                std::env::set_var(k, v.trim().trim_matches('"'));
            }
        }
    }
}

/// Thinking switch for one phase, read from `key`.
///
/// Three phases have independent switches because they want opposite things:
///   · main LLM — wants to reason; a wrong hint poisons everything downstream.
///   · locate    — answers "where is it", and thinking mostly buys latency.
///   · verify    — production keeps this **off** (`locate/mod.rs`: with thinking
///                 on the verifier spirals into self-doubt after a retry hint,
///                 20–40 s and thousands of tokens per call).
///
/// Defaults: main on, locate off, verify off — i.e. what production runs.
fn thinking_from_env(key: &str, default: bool) -> bool {
    match std::env::var(key).as_deref() {
        Ok("1") | Ok("true") => true,
        Ok("0") | Ok("false") => false,
        _ => default,
    }
}


/// Some models only accept a fixed temperature (kimi-k3.1 rejects anything but
/// 1). Override with `POINTER_E2E_TEMPERATURE` when comparing across providers.
fn temperature() -> f64 {
    std::env::var("POINTER_E2E_TEMPERATURE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.0)
}

fn provider_from_settings_file(thinking: bool) -> Result<OpenAIProvider> {
    let home = std::env::var("HOME").unwrap_or_default();
    let path = PathBuf::from(home)
        .join("Library/Application Support/PointerApp/user_settings.json");
    let raw = std::fs::read_to_string(&path).with_context(|| {
        format!(
            "read {} (or set POINTER_LOCATE_BASE_URL / POINTER_LOCATE_API_KEY)",
            path.display()
        )
    })?;
    let v: serde_json::Value = serde_json::from_str(&raw)?;
    let active = v.get("activeProviderId").and_then(|x| x.as_str()).unwrap_or("");
    let model = v
        .get("model")
        .and_then(|x| x.as_str())
        .unwrap_or("qwen3.8-flash")
        .to_string();
    let max_tokens = v.get("maxTokens").and_then(|x| x.as_u64()).unwrap_or(64000);
    let providers = v.get("providers").and_then(|x| x.as_array()).cloned().unwrap_or_default();
    let prov = providers
        .iter()
        .find(|p| p.get("id").and_then(|x| x.as_str()) == Some(active))
        .with_context(|| format!("provider `{active}` not found in {}", path.display()))?;
    let base_url = prov.get("baseUrl").and_then(|x| x.as_str()).unwrap_or("").to_string();
    let mut api_key = prov.get("apiKey").and_then(|x| x.as_str()).unwrap_or("").to_string();
    if let Some(rest) = api_key.strip_prefix("enc:v1:") {
        use base64::Engine as _;
        let blob = base64::engine::general_purpose::STANDARD.decode(rest)?;
        api_key = pointer_core::local_secret::decrypt_local_secret_with_info(
            &blob,
            b"provider-api-keys-v1",
        )
        .context("decrypt provider key")?;
    }
    anyhow::ensure!(!api_key.is_empty(), "provider `{active}` has no apiKey");
    eprintln!("[provider] active={active} base_url={base_url} model={model} key_len={}", api_key.len());

    let mut settings: ModelSettings = serde_json::from_value(serde_json::json!({
        "providers": [{
            "id": "e2e",
            "name": "e2e",
            "baseUrl": base_url,
            "apiKey": "",
            "models": [model.clone()]
        }],
        "activeProviderId": "e2e",
        "model": model,
        "temperature": temperature(),
        "maxTokens": max_tokens,
        "hasKey": true
    }))?;
    settings.round_enable_thinking = Some(thinking);
    Ok(OpenAIProvider::new(settings, api_key))
}

fn build_provider(thinking: bool) -> Result<OpenAIProvider> {
    load_dotenv();
    if let Ok(base_url) = std::env::var("POINTER_LOCATE_BASE_URL") {
        let api_key = std::env::var("POINTER_LOCATE_API_KEY").unwrap_or_default();
        let model = std::env::var("POINTER_LOCATE_MODEL")
            .unwrap_or_else(|_| "qwen3.8-flash".to_string());
        let mut settings: ModelSettings = serde_json::from_value(serde_json::json!({
            "providers": [{
                "id": "e2e",
                "name": "e2e",
                "baseUrl": base_url,
                "apiKey": "",
                "models": [model.clone()],
                // Without this the image is stripped before the request:
                // `model_capability_flags` defaults `supports_vision` to false,
                // and `make_openai_messages` then replaces the screenshot with
                // the text "(image not inlined: primary model does not support
                // vision)". The model was answering blind — that single flag
                // explained every bad hint measured so far.
                "modelConfigs": { model.clone(): { "supportsVision": true } },
                // Production sends `enable_thinking: true` on every main-LLM
                // round (verified in `logs/llm_prompts/0a8eeb49/*.json`).
                // Without it qwen defaults to no thinking: `reasoning_content`
                // comes back empty and the hint is a blind guess.
                // `round_enable_thinking` alone does not put the flag on the
                // wire — it has to be in the provider's `extraBody`, which
                // `effective_chat_extra_body` merges into the request root.
                "extraBody": { "enable_thinking": thinking }
            }],
            "activeProviderId": "e2e",
            "model": model,
            // Production runs 0.7 (see the dumps). Greedy decoding is not the
            // production configuration, and on DashScope it also suppressed the
            // reasoning channel entirely in our runs.
            "temperature": temperature(),
            "maxTokens": 64000,
            "hasKey": true
        }))?;
        settings.round_enable_thinking = Some(thinking);
        // Match the computer tier defaults (`session_model.rs`: budget 1024,
        // i.e. "low"). Leaving it unset makes `resolve_thinking_intensity`
        // return Medium (2048), which is not what production sends.
        settings.round_thinking_budget = Some(1024);
        // Diagnostic: `enable_thinking` only reaches the wire through
        // `extraBody`, so print what the request will actually carry. A silent
        // "thinking is on" that never lands in the body would make every hint
        // measurement unrepresentative of production.
        eprintln!(
            "[provider] thinking={thinking} round_enable={:?} extra_body={}",
            settings.round_enable_thinking,
            pointer_core::models::effective_chat_extra_body(&settings)
                .map(|v| v.to_string())
                .unwrap_or_else(|| "<none>".into())
        );
        return Ok(OpenAIProvider::new(settings, api_key));
    }
    provider_from_settings_file(thinking)
}

// ---------------------------------------------------------------------------
// metrics (mirrors benchmarks/screenspot_grid/bench/metrics.py)
// ---------------------------------------------------------------------------

fn inside(pt: (f64, f64), b: [f64; 4]) -> bool {
    b[0] <= pt.0 && pt.0 <= b[2] && b[1] <= pt.1 && pt.1 <= b[3]
}

fn size_bucket(b: [f64; 4]) -> &'static str {
    let long = (b[2] - b[0]).max(b[3] - b[1]);
    if long < 40.0 {
        "小(<40px)"
    } else if long <= 120.0 {
        "中(40-120px)"
    } else {
        "大(>120px)"
    }
}

/// `(macro, micro)` — macro 1 when the predicted box fully wraps ground truth,
/// micro = min(1, ∩ / gt_area).
fn box_cover(pred: [f64; 4], gt: [f64; 4]) -> (u8, f64) {
    let iw = (pred[2].min(gt[2]) - pred[0].max(gt[0])).max(0.0);
    let ih = (pred[3].min(gt[3]) - pred[1].max(gt[1])).max(0.0);
    let garea = ((gt[2] - gt[0]) * (gt[3] - gt[1])).max(0.0);
    let macro_hit = u8::from(
        pred[0] <= gt[0] && pred[1] <= gt[1] && pred[2] >= gt[2] && pred[3] >= gt[3],
    );
    let micro = if garea > 0.0 { (iw * ih / garea).min(1.0) } else { 0.0 };
    (macro_hit, micro)
}

#[derive(Default)]
struct Tally {
    n: usize,
    hit: usize,
    rounds: Vec<usize>,
    macro_sum: u32,
    micro_sum: f64,
}

impl Tally {
    fn record(&mut self, hit: bool, rounds: usize, cover: Option<(u8, f64)>) {
        self.n += 1;
        if hit {
            self.hit += 1;
        }
        self.rounds.push(rounds);
        if let Some((m, u)) = cover {
            self.macro_sum += m as u32;
            self.micro_sum += u;
        }
    }

    fn line(&self, label: &str) -> String {
        if self.n == 0 {
            return format!("{label:<16} n=0");
        }
        let mut r = self.rounds.clone();
        r.sort_unstable();
        let median_rounds = r[r.len() / 2];
        format!(
            "{label:<16} n={:<4} hit={:>3}/{:<3} = {:>5.1}%  cover(macro/micro)={:.0}%/{:.1}%  rounds_med={}",
            self.n,
            self.hit,
            self.n,
            100.0 * self.hit as f64 / self.n as f64,
            100.0 * self.macro_sum as f64 / self.n as f64,
            100.0 * self.micro_sum / self.n as f64,
            median_rounds,
        )
    }
}

// ---------------------------------------------------------------------------
// hint modes
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    None,
    Oracle,
    Agent,
    /// Main LLM only: take the box it submits and score its centre against the
    /// truth box. No locate, no verify — this measures **hint quality alone**,
    /// which is the thing that decides how many locate rounds follow.
    Hint,
}

impl Mode {
    fn parse(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "none" => Mode::None,
            "oracle" => Mode::Oracle,
            "agent" => Mode::Agent,
            "hint" | "hint_only" => Mode::Hint,
            other => panic!("POINTER_E2E_MODE must be none|oracle|agent|hint, got `{other}`"),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Mode::None => "hint=none",
            Mode::Oracle => "hint=oracle",
            Mode::Agent => "hint=agent",
            Mode::Hint => "hint=main-llm-only",
        }
    }
}

// ---------------------------------------------------------------------------
// per-case work
// ---------------------------------------------------------------------------

struct CaseResult {
    hit: bool,
    rounds: usize,
    cover: Option<(u8, f64)>,
    bucket: usize,
    log: Option<String>,
    failure: Option<String>,
    /// Wall-clock split, seconds: (image load+decode, agent hint, locate).
    timing: (f64, f64, f64),
}

fn bucket_index(bbox: [f64; 4]) -> usize {
    match size_bucket(bbox) {
        "小(<40px)" => 0,
        "中(40-120px)" => 1,
        _ => 2,
    }
}

async fn run_case(
    agent_provider: &OpenAIProvider,
    locate_provider: &OpenAIProvider,
    verify_provider: &OpenAIProvider,
    img_dir: &Path,
    case: &TruthCase,
    mode: Mode,
) -> CaseResult {
    let mut out = CaseResult {
        hit: false,
        rounds: 0,
        cover: None,
        bucket: bucket_index(case.bbox),
        log: None,
        failure: None,
        timing: (0.0, 0.0, 0.0),
    };

    let t_read = std::time::Instant::now();
    let path = img_dir.join(&case.fixture);
    if !path.exists() {
        out.log = Some(format!("  SKIP {} — missing {}", case.scene, path.display()));
        return out;
    }
    let jpeg = match std::fs::read(&path) {
        Ok(v) => v,
        Err(e) => {
            out.failure = Some(format!("{} read failed: {e}", case.scene));
            return out;
        }
    };
    let img = match image::open(&path) {
        Ok(i) => i.to_rgb8(),
        Err(e) => {
            out.failure = Some(format!("{} decode failed: {e}", case.scene));
            return out;
        }
    };
    let frame = LocateFrame::new(img);
    if let Some([w, h]) = case.size {
        if (frame.width(), frame.height()) != (w, h) {
            out.failure = Some(format!(
                "{} is {}x{}, dataset says {w}x{h}",
                case.fixture,
                frame.width(),
                frame.height()
            ));
            return out;
        }
    }

    let goal = case.goal.clone().unwrap_or_else(|| case.description.clone());
    out.timing.0 = t_read.elapsed().as_secs_f64();

    // Hint-only mode: score the submitted box directly and stop. This is the
    // main LLM's own accuracy, with locate/verify removed from the chain, so it
    // answers "how good is the hint" rather than "does the pipeline recover".
    if mode == Mode::Hint {
        let t_hint = std::time::Instant::now();
        match harness::agent_hint(
            agent_provider,
            &jpeg,
            (frame.width(), frame.height()),
            &goal,
        )
        .await
        {
            Ok(a) => {
                let cx = (a.box_px[0] + a.box_px[2]) / 2.0;
                let cy = (a.box_px[1] + a.box_px[3]) / 2.0;
                out.hit = inside((cx, cy), case.bbox);
                out.rounds = 0;
                out.cover = Some(box_cover(a.box_px, case.bbox));
                let dx = cx - (case.bbox[0] + case.bbox[2]) / 2.0;
                let dy = cy - (case.bbox[1] + case.bbox[3]) / 2.0;
                let dist = (dx * dx + dy * dy).sqrt();
                out.log = Some(format!(
                    "  {} hit={} centre=({:.0},{:.0}) truth=({:.0},{:.0}) d=({:+.0},{:+.0}) |{:.0}px| norm=[{:.0},{:.0},{:.0},{:.0}]",
                    case.scene,
                    if out.hit { "Y" } else { "n" },
                    cx,
                    cy,
                    (case.bbox[0] + case.bbox[2]) / 2.0,
                    (case.bbox[1] + case.bbox[3]) / 2.0,
                    dx,
                    dy,
                    dist,
                    a.box_norm[0], a.box_norm[1], a.box_norm[2], a.box_norm[3]
                ));
                // Reasoning is the whole point of the two-arm comparison: two
                // prompts can land on the same wrong box for different reasons.
                if !a.reasoning.trim().is_empty() {
                    let flat: String = a
                        .reasoning
                        .lines()
                        .map(str::trim)
                        .filter(|l| !l.is_empty())
                        .collect::<Vec<_>>()
                        .join(" ⏎ ");
                    let cut: String = flat.chars().take(600).collect();
                    out.log = Some(format!(
                        "{}  ↳ reasoning: {cut}",
                        out.log.unwrap_or_default()
                    ));
                }
            }
            Err(e) => {
                out.log = Some(format!("  SKIP {} — agent hint failed: {e:#}", case.scene));
                out.failure = Some(format!("{} hint failed: {e:#}", case.scene));
            }
        }
        out.timing.1 = t_hint.elapsed().as_secs_f64();
        return out;
    }

    let t_agent = std::time::Instant::now();
    let hint = match mode {
        // Handled above; reaching here would mean the early return was skipped.
        Mode::Hint => unreachable!("hint-only mode returns before locate"),
        Mode::None => None,
        Mode::Oracle => Some(PxBox {
            x1: case.bbox[0],
            y1: case.bbox[1],
            x2: case.bbox[2],
            y2: case.bbox[3],
        }),
        Mode::Agent => match harness::agent_hint(
            agent_provider,
            &jpeg,
            (frame.width(), frame.height()),
            &goal,
        )
        .await
        {
            Ok(a) => {
                out.log = Some(format!(
                    "  {} hint_norm=[{:.0},{:.0},{:.0},{:.0}] -> px=[{:.0},{:.0},{:.0},{:.0}] tool={}",
                    case.scene,
                    a.box_norm[0], a.box_norm[1], a.box_norm[2], a.box_norm[3],
                    a.box_px[0], a.box_px[1], a.box_px[2], a.box_px[3],
                    a.tool
                ));
                Some(PxBox {
                    x1: a.box_px[0],
                    y1: a.box_px[1],
                    x2: a.box_px[2],
                    y2: a.box_px[3],
                })
            }
            Err(e) => {
                out.log = Some(format!("  SKIP {} — agent hint failed: {e:#}", case.scene));
                return out;
            }
        },
    };

    let cancel = CancellationToken::new();
    let vision =
        ChatLocateVision::with_verify_provider(locate_provider, verify_provider, cancel.clone());
    let mut req = LocateRequest::new(case.description.clone(), goal);
    req.hint_px = hint;
    out.timing.1 = t_agent.elapsed().as_secs_f64();

    let t_locate = std::time::Instant::now();
    let res = locate_elements(&vision, &frame, &[req], &LocateConfig::default(), &cancel).await;
    out.timing.2 = t_locate.elapsed().as_secs_f64();
    match res {
        Ok(v) => match v.first() {
            Some(o) if o.ok => {
                let pt = o.point_px.expect("ok outcome carries a point");
                let pred = o
                    .box_px
                    .map(|b| [b.x1, b.y1, b.x2, b.y2])
                    .unwrap_or([pt.0, pt.1, pt.0, pt.1]);
                out.hit = inside(pt, case.bbox);
                out.rounds = o.rounds;
                out.cover = Some(box_cover(pred, case.bbox));
            }
            Some(o) => {
                out.rounds = o.rounds;
                out.failure = Some(format!("{} rounds={} why={}", case.scene, o.rounds, o.why));
            }
            None => {
                out.failure = Some(format!("{} no outcome", case.scene));
            }
        },
        Err(e) => {
            out.failure = Some(format!("{} locate failed: {e:#}", case.scene));
        }
    }
    out
}

// ---------------------------------------------------------------------------
// runner
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "calls a real vision model; run with --ignored"]
async fn computer_e2e_grounding() -> Result<()> {
    let _ = env_logger::builder().is_test(false).try_init();

    let mode = Mode::parse(&std::env::var("POINTER_E2E_MODE").unwrap_or_else(|_| "none".into()));
    let limit: Option<usize> = std::env::var("POINTER_E2E_LIMIT").ok().and_then(|v| v.parse().ok());
    let only: Option<String> = std::env::var("POINTER_E2E_ONLY").ok();

    let truth = load_truth()?;
    let cases: Vec<&TruthCase> = select(
        truth.cases.iter().collect(),
        only.as_deref(),
        |c, o| c.scene == o,
        |c| c.scene.as_str(),
        limit,
    );
    anyhow::ensure!(!cases.is_empty(), "no cases selected");

    let agent_thinking = thinking_from_env("POINTER_E2E_AGENT_THINKING", true);
    let locate_thinking = thinking_from_env("POINTER_E2E_LOCATE_THINKING", false);
    let verify_thinking = thinking_from_env("POINTER_E2E_VERIFY_THINKING", false);

    let agent_provider = build_provider(agent_thinking)?;
    let locate_provider = build_provider(locate_thinking)?;
    let verify_provider = build_provider(verify_thinking)?;
    let img_dir = image_dir();
    let concurrency: usize = std::env::var("POINTER_E2E_CONCURRENCY")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(4)
        .max(1);

    println!();
    println!("== computer_e2e :: {} :: {} cases ==", mode.label(), cases.len());
    println!("   images: {}", img_dir.display());
    println!("   concurrency: {}", concurrency);

    // Cases are independent and the bottleneck is LLM latency, so run them
    // concurrently. Accuracy is unaffected; only wall clock improves.
    let results: Vec<CaseResult> = futures_util::stream::iter(
        cases
            .iter()
            .map(|c| {
                run_case(
                    &agent_provider,
                    &locate_provider,
                    &verify_provider,
                    &img_dir,
                    c,
                    mode,
                )
            }),
    )
    .buffer_unordered(concurrency)
    .collect()
    .await;

    let mut total = Tally::default();
    let mut buckets: [Tally; 3] = Default::default();
    let mut failures: Vec<String> = Vec::new();

    for r in &results {
        if let Some(line) = &r.log {
            println!("{line}");
        }
        if let Some(f) = &r.failure {
            failures.push(f.clone());
        }
        total.record(r.hit, r.rounds, r.cover);
        buckets[r.bucket].record(r.hit, r.rounds, r.cover);
    }

    // Where the wall clock went, averaged per case.
    let n = results.len().max(1) as f64;
    let (s_read, s_agent, s_locate) = results.iter().fold((0.0, 0.0, 0.0), |a, r| {
        (a.0 + r.timing.0, a.1 + r.timing.1, a.2 + r.timing.2)
    });
    let locate_rounds: f64 = results
        .iter()
        .map(|r| r.rounds.max(1) as f64)
        .sum::<f64>()
        / n;
    println!();
    println!("-- timing (mean per case, {} cases) --", results.len());
    println!(
        "  image load   {:6.2}s  {:5.1}%",
        s_read / n,
        100.0 * s_read / (s_read + s_agent + s_locate).max(0.001)
    );
    println!(
        "  agent hint   {:6.2}s  {:5.1}%",
        s_agent / n,
        100.0 * s_agent / (s_read + s_agent + s_locate).max(0.001)
    );
    println!(
        "  locate       {:6.2}s  {:5.1}%   ({:.2} rounds avg, {:.1}s/round)",
        s_locate / n,
        100.0 * s_locate / (s_read + s_agent + s_locate).max(0.001),
        locate_rounds,
        (s_locate / n) / locate_rounds.max(0.01)
    );
    println!(
        "  total        {:6.2}s",
        (s_read + s_agent + s_locate) / n
    );

    println!();
    println!("{}", total.line("ALL"));
    for (i, label) in ["小(<40px)", "中(40-120px)", "大(>120px)"].iter().enumerate() {
        println!("{}", buckets[i].line(label));
    }
    if !failures.is_empty() {
        println!();
        println!("-- first {} failures --", failures.len().min(10));
        for f in failures.iter().take(10) {
            println!("  {f}");
        }
    }
    println!();
    Ok(())
}

// ---------------------------------------------------------------------------
// verify-only mode
// ---------------------------------------------------------------------------
//
// `datasets/pointer-2026-10-28/verify.jsonl` holds every verifier call from a
// production session: the two crops it saw, the box it was asked about, the
// description it was given, and the verdict it returned. Replaying them measures
// the verifier on its own — no hint quality, no locate rounds in the way.
//
// The stored verdict is the **baseline**, not the truth: it is what the model
// said last time. Divergence is the signal, and a flipped case needs a human to
// decide which of the two answers was right.

#[derive(Debug, Clone, Deserialize)]
struct VerifyRecord {
    run_id: String,
    round: usize,
    close_image: String,
    #[serde(default)]
    wide_image: Option<String>,
    target_desc: String,
    #[serde(default)]
    frame_size: Option<[u32; 2]>,
    labels: VerifyLabels,
}

#[derive(Debug, Clone, Deserialize)]
struct VerifyLabels {
    center: String,
    size: String,
    position: String,
    #[serde(default)]
    notes: String,
}

fn verify_dataset_path() -> PathBuf {
    if let Ok(p) = std::env::var("POINTER_E2E_VERIFY_JSONL") {
        return PathBuf::from(p);
    }
    eval_data_root().join("pointer-2026-10-28/verify.jsonl")
}

fn load_verify_records() -> Result<Vec<VerifyRecord>> {
    let path = verify_dataset_path();
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        out.push(
            serde_json::from_str(line)
                .with_context(|| format!("{} line {}", path.display(), i + 1))?,
        );
    }
    Ok(out)
}

/// Read a dumped crop back into the type `verify_pair` takes. The JPEG bytes are
/// passed through untouched — re-encoding would change what the model sees and
/// make a replay disagree with the recording for reasons unrelated to the code.
fn load_locate_image(path: &Path) -> Result<LocateImage> {
    let jpeg = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let img =
        image::load_from_memory(&jpeg).with_context(|| format!("decode {}", path.display()))?;
    Ok(LocateImage {
        jpeg,
        width: img.width(),
        height: img.height(),
    })
}

/// Normalise a stored label to the same lowercase vocabulary the engine uses, so
/// `Yes` / `yes` / `No` / `no` compare equal.
fn norm_label(s: &str) -> String {
    s.trim().to_ascii_lowercase()
}

#[tokio::test]
#[ignore = "calls a real vision model; run with --ignored"]
async fn computer_verify_e2e() -> Result<()> {
    let _ = env_logger::builder().is_test(false).try_init();
    load_dotenv();

    let limit: Option<usize> = std::env::var("POINTER_E2E_LIMIT")
        .ok()
        .and_then(|v| v.parse().ok());
    let only: Option<String> = std::env::var("POINTER_E2E_ONLY").ok();

    let records = select(
        load_verify_records()?,
        only.as_deref(),
        |r, o| r.run_id == o,
        |r| r.run_id.as_str(),
        limit,
    );
    anyhow::ensure!(!records.is_empty(), "no verify records selected");

    let verify_thinking = thinking_from_env("POINTER_E2E_VERIFY_THINKING", false);
    let provider = build_provider(verify_thinking)?;
    // The crops sit beside the JSONL that names them, not in the e2e frame
    // directory — `verify.jsonl` records a verifier call, and its two images are
    // that call's input, not a full-screen fixture.
    let img_dir = std::env::var("POINTER_E2E_IMAGE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            verify_dataset_path()
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| eval_data_root())
        });
    let concurrency: usize = std::env::var("POINTER_E2E_CONCURRENCY")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(4)
        .max(1);

    println!();
    println!(
        "== computer_verify_e2e :: {} records ==",
        records.len()
    );
    println!("   images: {}", img_dir.display());
    println!("   dataset: {}", verify_dataset_path().display());
    println!("   concurrency: {}", concurrency);

    let results: Vec<(String, VerifyRecord, Result<VerifyVerdict>)> =
        futures_util::stream::iter(records.into_iter().map(|rec| {
            let provider = &provider;
            let img_dir = &img_dir;
            async move {
                let key = format!("{} r{}", rec.run_id, rec.round);
                let close_path = img_dir.join(&rec.close_image);
                let wide_path = rec.wide_image.as_ref().map(|w| img_dir.join(w));
                let verdict = async {
                    let close = load_locate_image(&close_path)?;
                    let wide = match wide_path.as_deref() {
                        Some(p) if p.exists() => Some(load_locate_image(p)?),
                        _ => None,
                    };
                    let prompt = verify_prompt_views(&rec.target_desc, wide.is_some());
                    let vision = ChatLocateVision::new(provider, CancellationToken::new());
                    vision.verify_pair(&close, wide.as_ref(), &prompt).await
                }
                .await;
                (key, rec, verdict)
            }
        }))
        .buffer_unordered(concurrency)
        .collect()
        .await;

    let mut agree = 0usize;
    let mut flipped = Vec::new();
    let mut failed = Vec::new();
    for (key, rec, verdict) in &results {
        match verdict {
            Ok(v) => {
                let same = norm_label(&format!("{:?}", v.center_on_target)) == norm_label(&rec.labels.center)
                    && norm_label(&format!("{:?}", v.size_ok)) == norm_label(&rec.labels.size)
                    && norm_label(v.position_matches.as_str()) == norm_label(&rec.labels.position);
                if same {
                    agree += 1;
                } else {
                    flipped.push(format!(
                        "{key} was c={} s={} p={} → now c={:?} s={:?} p={}",
                        rec.labels.center,
                        rec.labels.size,
                        rec.labels.position,
                        v.center_on_target,
                        v.size_ok,
                        v.position_matches.as_str()
                    ));
                }
            }
            Err(e) => failed.push(format!("{key}: {e}")),
        }
    }

    let total = results.len();
    println!();
    println!(
        "verify  n={total}  agree={agree}  flipped={}  failed={}",
        flipped.len(),
        failed.len()
    );
    if !flipped.is_empty() {
        println!("-- flipped (needs a human call) --");
        for f in flipped.iter().take(20) {
            println!("  {f}");
        }
    }
    if !failed.is_empty() {
        println!("-- failed --");
        for f in failed.iter().take(20) {
            println!("  {f}");
        }
    }
    println!();
    Ok(())
}

#[cfg(test)]
mod scope_tests {
    use super::{take_runs, Scope};

    /// Rows as they appear in `locate.jsonl`: several per run, ordered by round.
    fn rows() -> Vec<(&'static str, u32)> {
        vec![
            ("a", 1),
            ("a", 2),
            ("a", 3),
            ("b", 1),
            ("c", 1),
            ("c", 2),
            ("d", 1),
        ]
    }

    #[test]
    fn smoke_keeps_whole_runs_not_the_first_n_rows() {
        // The bug this guards: `LIMIT 3` would return a/a/a — three rounds of one
        // run, telling you nothing about whether b, c or d still resolve.
        let kept = take_runs(rows(), 2, |r| r.0);
        assert_eq!(kept, vec![("a", 1), ("a", 2), ("a", 3), ("b", 1)]);
        assert_eq!(
            kept.iter().map(|r| r.0).collect::<std::collections::BTreeSet<_>>().len(),
            2,
            "two distinct runs"
        );
    }

    #[test]
    fn take_runs_never_splits_a_run_across_the_cut() {
        let kept = take_runs(rows(), 3, |r| r.0);
        // c has two rounds; both must survive or the second one reads as a
        // correction that never happened.
        assert!(kept.contains(&("c", 1)) && kept.contains(&("c", 2)));
        assert_eq!(kept.len(), 6, "a(3) + b(1) + c(2)");
    }

    #[test]
    fn smoke_is_a_subset_of_sample_is_a_subset_of_full() {
        for total in [5usize, 12, 77, 831] {
            let smoke = Scope::Smoke.runs(total);
            let sample = Scope::Sample.runs(total);
            let full = Scope::Full.runs(total);
            assert!(smoke <= sample, "total={total}: smoke={smoke} sample={sample}");
            assert!(sample <= full, "total={total}: sample={sample} full={full}");
            assert_eq!(full, total);
        }
    }

    #[test]
    fn a_small_dataset_is_never_cut_below_smoke_size() {
        // 20% of 12 is 2, which would make "sample" a weaker check than "smoke".
        assert_eq!(Scope::Sample.runs(12), 10);
        assert_eq!(Scope::Sample.runs(5), 5, "cannot exceed the dataset");
    }

    #[test]
    fn limit_overrides_scope_and_counts_rows() {
        // The old invocation shape has to keep working: LIMIT is a row count, so
        // it can cut a run in half — that is the caller's call to make.
        let kept = super::select(
            rows(),
            None,
            |_r, _o| false,
            |r| r.0,
            Some(3),
        );
        assert_eq!(kept, vec![("a", 1), ("a", 2), ("a", 3)]);
    }

    #[test]
    fn only_narrows_before_scope_runs() {
        // ONLY picks one run; scope must not then drop it for being past the cut.
        let kept = super::select(rows(), Some("d"), |r, o| r.0 == o, |r| r.0, None);
        assert_eq!(kept, vec![("d", 1)]);
    }

    #[test]
    fn an_unknown_scope_is_rejected_instead_of_defaulting() {
        assert!(Scope::parse("smoke").is_some());
        assert!(Scope::parse("SAMPLE").is_some());
        assert!(Scope::parse("").is_some());
        assert!(Scope::parse("all").is_some());
        assert!(Scope::parse("smok").is_none());
        assert!(Scope::parse("20").is_none());
    }
}
