//! Manual-only grounding test against a **frozen** screenshot.
//!
//! Everything this test needs lives next to it in `tests/locate_fixture/`:
//!
//! ```text
//! tests/locate_fixture/
//! ├── main.rs                    this test
//! ├── env.example                POINTER_LOCATE_* variables
//! ├── fixtures/
//! │   ├── truth_desktop.png      frozen capture for truth_desktop.json
//! │   ├── truth_eastmoney.png    frozen capture for the eastmoney truth files
//! │   └── sudoku.png             frozen capture for truth_sudoku.json
//! └── truth/
//!     ├── truth_desktop.json     desktop, 20 cases
//!     ├── truth_eastmoney.json   eastmoney page, 29 cases
//!     ├── truth_fourpart.json    same page, four-part descriptions vs old ones
//!     └── truth_sudoku.json      81 cells, one per test point
//! ```
//!
//! A fixture is named after the truth file that drives it, so a screenshot and
//! its ground truth stay paired. `truth_fourpart.json` reuses
//! `fixtures/truth_eastmoney.png` — it is the same page described two ways, and
//! duplicating a 1 MB capture to honour the naming rule would buy nothing.
//!
//! `truth_sudoku.json` is different from the others: its cases are not
//! descriptions to locate but cell boxes, each `bbox` being one sudoku cell. The
//! pointer-overlay tests use them as test points for where the runtime draws the
//! cursor — a check on drawing, not on model grounding.
//!
//! Unlike the P0 harness (`scripts/p0_locate_eval.mjs`), this never takes a live
//! screenshot, so results do not depend on which display the cursor sits on or
//! what windows happen to be open.
//!
//! Run it by hand:
//!
//! ```bash
//! cd crates/pointer-core
//! set -a; . tests/locate_fixture/env.example; set +a
//! cargo test --test locate_fixture -- --ignored --nocapture
//! ```
//!
//! It is `#[ignore]`d because it spends real model tokens and takes ~2–4 minutes.
//!
//! **Capture mode** — when adding cases, run with `POINTER_LOCATE_CAPTURE=1` to
//! print raw boxes instead of asserting, then copy them into the truth file after
//! confirming each crop really contains the target.

use std::path::PathBuf;

use anyhow::{Context, Result};
use pointer_core::agents::computer::locate::{
    locate_elements, ChatLocateVision, LocateConfig, LocateFrame, LocateOutcome, LocateRequest,
    PxBox,
};
use futures_util::StreamExt;
use pointer_core::models::ModelSettings;
use pointer_core::provider::OpenAIProvider;
use tokio_util::sync::CancellationToken;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/locate_fixture")
}

/// Load `tests/locate_fixture/.env` if present, so a real API key survives
/// between runs. `env.example` is the template and is committed; `.env` is
/// git-ignored. Real environment variables always win over the file.
fn load_dotenv() {
    let path = dir().join(".env");
    let Ok(raw) = std::fs::read_to_string(&path) else {
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
        let k = k.trim();
        let v = v.trim().trim_matches('"').trim_matches('\'');
        if k.is_empty() || std::env::var_os(k).is_some() {
            continue;
        }
        std::env::set_var(k, v);
    }
}

fn truth_path() -> PathBuf {
    dir().join("truth").join("truth_desktop.json")
}

fn truth_path_named(name: &str) -> PathBuf {
    dir().join("truth").join(name)
}

fn load_truth_file(path: &PathBuf) -> Result<TruthFile> {
    let raw = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let file: TruthFile =
        serde_json::from_str(&raw).with_context(|| format!("parse {}", path.display()))?;
    anyhow::ensure!(!file.cases.is_empty(), "{} has no cases", path.display());
    Ok(file)
}

#[derive(serde::Deserialize)]
struct TruthFile {
    /// Fixture file name, relative to this directory.
    fixture: String,
    /// Expected `[width, height]` of the fixture, in logical pixels.
    #[serde(default)]
    size: Option<[u32; 2]>,
    #[serde(default)]
    cases: Vec<TruthCase>,
}

#[derive(serde::Deserialize)]
struct TruthCase {
    #[serde(default)]
    scene: String,
    description: String,
    /// What the caller intends to do with the element. Mirrors production, where
    /// `resolve_point` passes the model's own goal and the grounder uses it to
    /// choose between look-alike targets. Falls back to the fill-in wording.
    #[serde(default)]
    goal: Option<String>,
    /// `[x1, y1, x2, y2]` in logical pixels; `null` = not measured yet.
    #[serde(default)]
    bbox: Option<[f64; 4]>,
    /// `plain` (default) or `partial_hint` — the latter supplies a `hint_box`
    /// that overlaps the target but whose centre falls outside it.
    #[serde(default)]
    kind: String,
    #[serde(default)]
    hint_box: Option<[f64; 4]>,
    #[serde(default)]
    note: String,
}

/// The truth file is the single source of truth: fixture name, size, and cases.
fn load_truth() -> Result<TruthFile> {
    // `POINTER_LOCATE_TRUTH` switches datasets the same way it does for the
    // eastmoney tests, so `truth_sudoku.json` can drive `frozen_desktop_grounding`
    // without a second copy of the runner.
    let path = match std::env::var("POINTER_LOCATE_TRUTH") {
        Ok(name) if !name.trim().is_empty() => truth_path_named(name.trim()),
        _ => truth_path(),
    };
    let raw =
        std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let file: TruthFile =
        serde_json::from_str(&raw).with_context(|| format!("parse {}", path.display()))?;
    anyhow::ensure!(
        !file.cases.is_empty(),
        "{} has no cases",
        path.display()
    );
    Ok(file)
}

fn user_settings_path() -> PathBuf {
    if let Ok(p) = std::env::var("POINTER_USER_SETTINGS") {
        return PathBuf::from(p);
    }
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home).join("Library/Application Support/PointerApp/user_settings.json")
}

/// B5 runs with thinking off. `POINTER_LOCATE_THINKING=1` flips it back on so
/// the two configurations can be compared on the same fixture.
fn thinking_enabled() -> bool {
    matches!(
        std::env::var("POINTER_LOCATE_THINKING").as_deref(),
        Ok("1") | Ok("on") | Ok("true")
    )
}

fn thinking_setting() -> Option<bool> {
    if thinking_enabled() {
        None // leave the provider default alone, which is "on" for qwen3.8-flash
    } else {
        Some(false)
    }
}

/// Env-first, then fall back to the local Pointer settings file.
///
/// The settings file is what the running app uses, so the fallback keeps this
/// test honest: same provider, same model, same thinking configuration.
fn build_provider() -> Result<OpenAIProvider> {
    load_dotenv();
    if let Ok(base_url) = std::env::var("POINTER_LOCATE_BASE_URL") {
        let api_key = std::env::var("POINTER_LOCATE_API_KEY").unwrap_or_default();
        let model = std::env::var("POINTER_LOCATE_MODEL")
            .unwrap_or_else(|_| "qwen3.8-flash".to_string());
        let settings: ModelSettings = serde_json::from_value(serde_json::json!({
            "providers": [{
                "id": "locate-test",
                "name": "locate-test",
                "baseUrl": base_url,
                "apiKey": "",
                "models": [model.clone()]
            }],
            "activeProviderId": "locate-test",
            "model": model,
            "temperature": 0.0,
            "maxTokens": 64000,
            "hasKey": true
        }))
        .context("build ModelSettings from env")?;
        let mut settings = settings;
        settings.round_enable_thinking = thinking_setting();
        return Ok(OpenAIProvider::new(settings, api_key));
    }

    // The settings file is `UserSettings`, a superset of `ModelSettings` with a
    // few nullable fields (`hasKey`), so deserializing it straight into
    // `ModelSettings` fails. Pull the fields we need out of the raw JSON instead.
    let path = user_settings_path();
    let raw = std::fs::read_to_string(&path).with_context(|| {
        format!(
            "read {} (or set POINTER_LOCATE_BASE_URL / POINTER_LOCATE_API_KEY)",
            path.display()
        )
    })?;
    let v: serde_json::Value =
        serde_json::from_str(&raw).with_context(|| format!("parse {}", path.display()))?;
    let active = v
        .get("activeProviderId")
        .and_then(|x| x.as_str())
        .unwrap_or("");
    let model = v
        .get("model")
        .and_then(|x| x.as_str())
        .unwrap_or("qwen3.8-flash")
        .to_string();
    let max_tokens = v.get("maxTokens").and_then(|x| x.as_u64()).unwrap_or(64000);
    let providers = v
        .get("providers")
        .and_then(|x| x.as_array())
        .cloned()
        .unwrap_or_default();
    let prov = providers
        .iter()
        .find(|p| p.get("id").and_then(|x| x.as_str()) == Some(active))
        .with_context(|| format!("provider `{active}` not found in {}", path.display()))?;
    let base_url = prov
        .get("baseUrl")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let mut api_key = prov
        .get("apiKey")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    // user_settings.json stores provider keys as `enc:v1:<base64>`, encrypted
    // with a key held in the macOS keychain. Decrypt the same way storage.rs
    // does, otherwise the fallback path would hand an opaque blob to the
    // provider and every call would come back empty.
    if let Some(rest) = api_key.strip_prefix("enc:v1:") {
        use base64::Engine as _;
        let blob = base64::engine::general_purpose::STANDARD
            .decode(rest)
            .context("decode enc:v1 provider key")?;
        api_key = pointer_core::local_secret::decrypt_local_secret_with_info(
            &blob,
            b"provider-api-keys-v1",
        )
        .context("decrypt provider key from user_settings.json")?;
    }
    anyhow::ensure!(
        !api_key.is_empty(),
        "provider `{active}` has no apiKey in {}",
        path.display()
    );

    let settings: ModelSettings = serde_json::from_value(serde_json::json!({
        "providers": [{
            "id": "locate-test",
            "name": "locate-test",
            "baseUrl": base_url,
            "apiKey": "",
            "models": [model.clone()]
        }],
        "activeProviderId": "locate-test",
        "model": model,
        "temperature": 0.0,
        "maxTokens": max_tokens,
        "hasKey": true
    }))
    .context("build ModelSettings from user settings")?;
    let mut settings = settings;
    settings.round_enable_thinking = thinking_setting();
    Ok(OpenAIProvider::new(settings, api_key))
}

#[tokio::test]
#[ignore = "calls a real vision model; run with --ignored"]
async fn frozen_desktop_grounding() -> Result<()> {
    // `engine.rs` logs every verdict (`locate: round=N ... why=...`). Without a
    // logger those lines vanish and the only visible trace of a round is the
    // final `what_i_saw` — which loses everything the earlier rounds decided.
    // Run with `RUST_LOG=info` to see them.
    let _ = env_logger::builder()
        .format_timestamp_millis()
        .is_test(false)
        .try_init();
    let truth = load_truth()?;
    let path = dir().join(&truth.fixture);
    assert!(path.exists(), "missing fixture: {}", path.display());

    let img = image::open(&path)?.to_rgb8();
    let frame = LocateFrame::new(img);
    if let Some([w, h]) = truth.size {
        assert_eq!(
            (frame.width(), frame.height()),
            (w, h),
            "fixture size does not match the truth file; a Retina 2x capture would \
             shift every returned coordinate by 2x"
        );
    }

    let capture = std::env::var("POINTER_LOCATE_CAPTURE").is_ok();

    let provider = build_provider()?;
    let cancel = CancellationToken::new();
    let vision = ChatLocateVision::new(&provider, cancel.clone());

    let mut scored = 0usize;
    let mut hits = 0usize;
    // Two ways the verifier's self-report and the truth box can disagree. Neither
    // changes `hits` — accuracy is the landing point — but both are worth printing:
    // a rising `claimed_but_missed` means the verifier is passing boxes it should
    // reject, and a rising `refused_but_landed` means it is rejecting good boxes.
    let mut refused_but_landed = 0usize;
    let mut claimed_but_missed = 0usize;
    // Owned strings: the case rows are moved out of the concurrent batch, so a
    // `&str` into them would not outlive the loop.
    let mut unscored: Vec<String> = Vec::new();
    let mut failures: Vec<String> = Vec::new();

    let timeout_secs: u64 = std::env::var("POINTER_LOCATE_TIMEOUT_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(60);
    let per_case = std::time::Duration::from_secs(timeout_secs);
    println!(
        "mode: thinking={} timeout={}s fixture={}",
        if thinking_enabled() { "on" } else { "off" },
        timeout_secs,
        truth.fixture
    );

    // `POINTER_LOCATE_ONLY=18` limits the run to specific 1-based case numbers.
    let only: Vec<usize> = std::env::var("POINTER_LOCATE_ONLY")
        .ok()
        .map(|s| {
            s.split(',')
                .filter_map(|x| x.trim().parse::<usize>().ok())
                .collect()
        })
        .unwrap_or_default();

    // Concurrency: each case is an independent API call, so they can overlap.
    // `buffer_unordered` caps how many are in flight at once — the API rejects
    // bursts, and 81 simultaneous 1920x1080 uploads would also cost more memory
    // than the run is worth. Default 1 keeps the old serial behaviour.
    let concurrency: usize = std::env::var("POINTER_LOCATE_CONCURRENCY")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1)
        .max(1);
    if concurrency > 1 {
        println!("concurrency: {concurrency}");
    }

    struct CaseRun {
        idx: usize,
        scene: String,
        desc: String,
        bbox: Option<[f64; 4]>,
        /// `Err` carries the reason, already formatted for printing.
        result: Result<LocateOutcome, String>,
        elapsed: f64,
    }

    let runs: Vec<CaseRun> = futures_util::stream::iter(
        truth
            .cases
            .iter()
            .enumerate()
            .filter(|(idx, _)| only.is_empty() || only.contains(&(idx + 1)))
            .map(|(idx, case)| {
                let scene = case.scene.clone();
                let desc = case.description.clone();
                let bbox = case.bbox;
                // `goal` and `hint_px` mirror production: `resolve_point` always
                // passes the caller's estimated box, and the grounder uses `goal`
                // to pick between look-alike elements. Running without them
                // measured a different problem — full-screen search with no
                // intent — which is why sudoku cases carry a deliberately off
                // hint box.
                let goal = case
                    .goal
                    .clone()
                    .unwrap_or_else(|| "选中这一格准备填数".to_string());
                let mut request = LocateRequest::new(desc.as_str(), goal);
                if let Some(h) = case.hint_box {
                    request = request.with_hint_px(PxBox {
                        x1: h[0],
                        y1: h[1],
                        x2: h[2],
                        y2: h[3],
                    });
                }
                let requests = vec![request];
                let vision = &vision;
                let frame = &frame;
                let cancel = &cancel;
                async move {
                    let started = std::time::Instant::now();
                    // Per-case timeout: a single stubborn target can retry for
                    // minutes, and one of those must not swallow the whole run.
                    let call = tokio::time::timeout(
                        per_case,
                        locate_elements(vision, frame, &requests, &LocateConfig::default(), cancel),
                    )
                    .await;
                    let elapsed = started.elapsed().as_secs_f64();
                    let result = match call {
                        Ok(Ok(mut outcomes)) => match outcomes.pop() {
                            Some(o) => Ok(o),
                            None => Err("empty outcome".to_string()),
                        },
                        Ok(Err(e)) => Err(format!("{e:#}")),
                        Err(_) => Err(format!(">{timeout_secs}s")),
                    };
                    CaseRun {
                        idx,
                        scene,
                        desc,
                        bbox,
                        result,
                        elapsed,
                    }
                }
            }),
    )
    .buffer_unordered(concurrency)
    .collect()
    .await;

    // Concurrent calls finish out of order; report in file order so a diff
    // between two runs lines up.
    let mut runs = runs;
    runs.sort_by_key(|r| r.idx);

    for run in runs {
        let CaseRun {
            scene,
            desc,
            bbox,
            result,
            elapsed,
            ..
        } = &run;
        let scene = scene.as_str();
        let desc = desc.as_str();

        let outcome = match result {
            Ok(o) => o,
            Err(reason) => {
                let label = if reason.starts_with('>') {
                    "TIMEOUT"
                } else {
                    "ERROR"
                };
                println!("{label} {scene:<6} {desc:<34} {reason}");
                // A case with ground truth that errors is a miss, not a skip.
                if bbox.is_some() {
                    scored += 1;
                    failures.push(desc.to_string());
                }
                continue;
            }
        };
        // 600, not 90: the verdict now carries three things (what the wide image
        // showed, what the mark landed on, and the size measurement) and the
        // interesting part is always the last one. At 90 every line ended
        // mid-sentence, which made a wrong verdict impossible to diagnose.
        let saw: String = outcome.what_i_saw.chars().take(600).collect();

        if capture {
            println!(
                "CAPTURE\t{scene}\t{desc}\tbox={:?}\trounds={}\t{:.1}s\t{}",
                outcome.box_px, outcome.rounds, elapsed, saw
            );
            continue;
        }

        let Some(bbox) = bbox else {
            unscored.push(desc.to_string());
            println!("SKIP  {scene:<6} {desc:<34} (no bbox in the truth file)");
            continue;
        };

        scored += 1;
        let center = outcome
            .box_px
            .map(|b| ((b.x1 + b.x2) / 2.0, (b.y1 + b.y2) / 2.0));
        // Accuracy is decided by **where the box centre lands**, not by what the
        // verifier said about itself. `outcome.ok` is the model's own verdict; a
        // run that reports `ok = true` with the centre outside the truth box is
        // wrong, and a run that refuses while its last box happened to land on
        // the target is right. Counting `ok` here would measure the verifier's
        // self-confidence, not the grounder's accuracy.
        let landed = center
            .is_some_and(|(cx, cy)| cx >= bbox[0] && cx <= bbox[2] && cy >= bbox[1] && cy <= bbox[3]);
        let hit = landed;
        if landed && !outcome.ok {
            refused_but_landed += 1;
        }
        if !landed && outcome.ok {
            claimed_but_missed += 1;
        }
        if hit {
            hits += 1;
        } else {
            failures.push(desc.to_string());
        }
        println!(
            "{} {scene:<6} {desc:<34} ok={:<5} box={:?} rounds={} {:.1}s | {}",
            if hit { "PASS" } else { "FAIL" },
            outcome.ok,
            outcome.box_px,
            outcome.rounds,
            elapsed,
            saw
        );
        // The thinking channel, when the provider returned one. A wrong verdict is
        // otherwise just a number — you cannot tell whether the model misread the
        // boundary, counted the wrong row, or never saw the target at all.
        if !outcome.reasoning.trim().is_empty() {
            println!(
                "  [thinking] {}",
                outcome.reasoning.trim().replace('\n', " ⏎ ")
            );
        }
    }

    if capture {
        println!("\ncapture mode: copy the boxes above into {}", truth_path().display());
        return Ok(());
    }

    println!("\n{hits}/{scored} landed inside the target box");
    if claimed_but_missed > 0 || refused_but_landed > 0 {
        println!(
            "  verifier vs truth: claimed_but_missed={claimed_but_missed}  refused_but_landed={refused_but_landed}"
        );
    }
    if !unscored.is_empty() {
        println!("{} case(s) not scored: {}", unscored.len(), unscored.join(", "));
    }
    if !failures.is_empty() {
        println!("missed: {}", failures.join(", "));
    }

    // The coverage bar only makes sense for a full run; `--only` deliberately
    // scores a subset.
    if only.is_empty() {
        assert!(
            scored >= 15,
            "only {scored} case(s) have a bbox in the truth file; expected at least 15"
        );
    }
    let rate = hits as f64 / scored as f64;
    assert!(
        rate >= 0.7,
        "hit rate {:.1}% ({hits}/{scored}) is below the 70% bar",
        rate * 100.0
    );
    Ok(())
}

/// A hint that is **wrong on purpose**: same size as the target (1.5x), shifted
/// by three target-widths down-right, so the real element is not inside it.
/// This measures the interesting half of `hint_box` — can the verifier notice
/// the caller's guess is off and re-locate, instead of trusting it?
fn offset_hint(b: [f64; 4], max_w: f64, max_h: f64) -> PxBox {
    let cx = (b[0] + b[2]) / 2.0;
    let cy = (b[1] + b[3]) / 2.0;
    let w = (b[2] - b[0]) * 1.5;
    let h = (b[3] - b[1]) * 1.5;
    let dx = (b[2] - b[0]) * 3.0;
    let dy = (b[3] - b[1]) * 3.0;
    PxBox {
        x1: (cx - w / 2.0 + dx).clamp(0.0, max_w),
        y1: (cy - h / 2.0 + dy).clamp(0.0, max_h),
        x2: (cx + w / 2.0 + dx).clamp(0.0, max_w),
        y2: (cy + h / 2.0 + dy).clamp(0.0, max_h),
    }
}

/// Hint path: `hint_box` skips the full-screen locate and starts from the hint,
/// so this measures whether a roughly-right hint helps instead of misleading.
#[tokio::test]
#[ignore = "calls a real vision model; run with --ignored"]
async fn frozen_desktop_hint_grounding() -> Result<()> {
    let truth = load_truth()?;
    let path = dir().join(&truth.fixture);
    let img = image::open(&path)?.to_rgb8();
    let frame = LocateFrame::new(img);
    if let Some([w, h]) = truth.size {
        assert_eq!(
            (frame.width(), frame.height()),
            (w, h),
            "fixture size does not match the truth file"
        );
    }

    let provider = build_provider()?;
    let cancel = CancellationToken::new();
    let vision = ChatLocateVision::new(&provider, cancel.clone());

    let timeout_secs: u64 = std::env::var("POINTER_LOCATE_TIMEOUT_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(60);
    let per_case = std::time::Duration::from_secs(timeout_secs);
    println!(
        "mode: thinking={} timeout={}s hint=offset",
        if thinking_enabled() { "on" } else { "off" },
        timeout_secs
    );

    let (fw, fh) = (frame.width() as f64, frame.height() as f64);
    let mut scored = 0usize;
    let mut hits = 0usize;
    let mut misses: Vec<String> = Vec::new();

    for case in truth.cases.iter() {
        let Some(bbox) = case.bbox else { continue };
        let hint = offset_hint(bbox, fw, fh);
        let requests = vec![LocateRequest::new(case.description.as_str(), "点击它").with_hint_px(hint)];
        let started = std::time::Instant::now();
        let call = tokio::time::timeout(
            per_case,
            locate_elements(&vision, &frame, &requests, &LocateConfig::default(), &cancel),
        )
        .await;
        let elapsed = started.elapsed().as_secs_f64();

        scored += 1;
        let outcome = match call {
            Ok(Ok(mut v)) => v.pop(),
            Ok(Err(e)) => {
                println!("ERROR {:<34} {e:#}", case.description);
                misses.push(case.description.clone());
                continue;
            }
            Err(_) => {
                println!("TIMEOUT {:<34} >{timeout_secs}s", case.description);
                misses.push(case.description.clone());
                continue;
            }
        };
        let Some(outcome) = outcome else {
            misses.push(case.description.clone());
            continue;
        };
        let hit = outcome.ok
            && outcome.box_px.is_some_and(|b| {
                let (cx, cy) = ((b.x1 + b.x2) / 2.0, (b.y1 + b.y2) / 2.0);
                cx >= bbox[0] && cx <= bbox[2] && cy >= bbox[1] && cy <= bbox[3]
            });
        if hit {
            hits += 1;
        } else {
            misses.push(case.description.clone());
        }
        println!(
            "{} hint {:<34} ok={:<5} box={:?} rounds={} {:.1}s",
            if hit { "PASS" } else { "FAIL" },
            case.description,
            outcome.ok,
            outcome.box_px,
            outcome.rounds,
            elapsed
        );
    }

    println!("\nhint: {hits}/{scored} hit");
    if !misses.is_empty() {
        println!("missed: {}", misses.join(", "));
    }
    let rate = hits as f64 / scored as f64;
    assert!(
        rate >= 0.7,
        "hint hit rate {:.1}% ({hits}/{scored}) is below the 70% bar",
        rate * 100.0
    );
    Ok(())
}

/// A hint on the far side of the screen: mirrored through the centre, so the
/// distance from the truth is at least half the screen. This tests a safety
/// property, not accuracy: when the caller's guess is nowhere near the target,
/// the locator must say "not found" rather than confidently return a wrong box.
fn far_hint(b: [f64; 4], max_w: f64, max_h: f64) -> PxBox {
    let w = b[2] - b[0];
    let h = b[3] - b[1];
    let cx = max_w - (b[0] + b[2]) / 2.0;
    let cy = max_h - (b[1] + b[3]) / 2.0;
    PxBox {
        x1: (cx - w / 2.0).clamp(0.0, max_w),
        y1: (cy - h / 2.0).clamp(0.0, max_h),
        x2: (cx + w / 2.0).clamp(0.0, max_w),
        y2: (cy + h / 2.0).clamp(0.0, max_h),
    }
}

/// Far-hint safety: every case must end in one of two honest states —
/// re-located onto the truth, or `ok=false`. A confident wrong box is a failure.
#[tokio::test]
#[ignore = "calls a real vision model; run with --ignored"]
async fn frozen_desktop_far_hint_stays_honest() -> Result<()> {
    // Same logger as `frozen_desktop_grounding`. Without it the engine's
    // `locate: not found in this image` line — the whole point of this test —
    // never reaches the log.
    let _ = env_logger::builder()
        .format_timestamp_millis()
        .is_test(false)
        .try_init();
    let truth = load_truth()?;
    let path = dir().join(&truth.fixture);
    let img = image::open(&path)?.to_rgb8();
    let frame = LocateFrame::new(img);
    if let Some([w, h]) = truth.size {
        assert_eq!(
            (frame.width(), frame.height()),
            (w, h),
            "fixture size does not match the truth file"
        );
    }

    let provider = build_provider()?;
    let cancel = CancellationToken::new();
    let vision = ChatLocateVision::new(&provider, cancel.clone());

    let timeout_secs: u64 = std::env::var("POINTER_LOCATE_TIMEOUT_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(60);
    let per_case = std::time::Duration::from_secs(timeout_secs);
    println!(
        "mode: thinking={} timeout={}s hint=far",
        if thinking_enabled() { "on" } else { "off" },
        timeout_secs
    );

    let (fw, fh) = (frame.width() as f64, frame.height() as f64);
    let mut rescued = 0usize;
    let mut honest_fail = 0usize;
    let mut dangerous: Vec<String> = Vec::new();
    let mut scored = 0usize;

    for case in truth.cases.iter() {
        let Some(bbox) = case.bbox else { continue };
        let hint = far_hint(bbox, fw, fh);
        let hx = (hint.x1 + hint.x2) / 2.0;
        let hy = (hint.y1 + hint.y2) / 2.0;
        let tx = (bbox[0] + bbox[2]) / 2.0;
        let ty = (bbox[1] + bbox[3]) / 2.0;
        let dist = ((hx - tx).powi(2) + (hy - ty).powi(2)).sqrt();

        let requests = vec![LocateRequest::new(case.description.as_str(), "点击它").with_hint_px(hint)];
        let started = std::time::Instant::now();
        let call = tokio::time::timeout(
            per_case,
            locate_elements(&vision, &frame, &requests, &LocateConfig::default(), &cancel),
        )
        .await;
        let elapsed = started.elapsed().as_secs_f64();
        scored += 1;

        let outcome = match call {
            Ok(Ok(mut v)) => v.pop(),
            Ok(Err(e)) => {
                println!("ERROR   {:<34} {e:#}", case.description);
                honest_fail += 1;
                continue;
            }
            Err(_) => {
                println!("TIMEOUT {:<34} >{timeout_secs}s", case.description);
                honest_fail += 1;
                continue;
            }
        };
        let Some(outcome) = outcome else {
            honest_fail += 1;
            continue;
        };

        let in_truth = outcome.box_px.is_some_and(|b| {
            let (cx, cy) = ((b.x1 + b.x2) / 2.0, (b.y1 + b.y2) / 2.0);
            cx >= bbox[0] && cx <= bbox[2] && cy >= bbox[1] && cy <= bbox[3]
        });
        let state = if outcome.ok && in_truth {
            rescued += 1;
            "RESCUED"
        } else if !outcome.ok {
            honest_fail += 1;
            "NOT-FOUND"
        } else {
            dangerous.push(case.description.clone());
            "WRONG!"
        };
        println!(
            "{:<9} {:<34} dist={dist:>5.0}px ok={:<5} box={:?} rounds={} {:.1}s",
            state, case.description, outcome.ok, outcome.box_px, outcome.rounds, elapsed
        );
    }

    println!(
        "\nfar hint: {scored} cases — rescued={rescued} honest_fail={honest_fail} dangerous={}",
        dangerous.len()
    );
    if !dangerous.is_empty() {
        println!("CONFIDENTLY WRONG: {}", dangerous.join(", "));
    }
    assert!(
        dangerous.is_empty(),
        "a far hint produced {} confident wrong box(es): {}",
        dangerous.len(),
        dangerous.join(", ")
    );
    Ok(())
}

/// Targets that are **not on this screen**. The locator must answer "not found":
/// `ok=false`, no coordinates. A hallucinated box here would be the worst
/// possible failure — a caller acting on it would click somewhere random.
// Only names that are certainly not on this screen. Two earlier candidates
// (WeChat, VS Code) turned out to really sit in the Dock — the locator was
// right and the test case was wrong, so they are gone.
const ABSENT_CASES: &[(&str, [f64; 4])] = &[
    ("桌面上的 不存在的文件XYZ.pdf 文件图标", [700.0, 400.0, 900.0, 600.0]),
    ("底部 Dock 里的 Adobe Photoshop 图标", [500.0, 900.0, 600.0, 980.0]),
    ("菜单栏右上角的蓝牙图标", [1200.0, 8.0, 1245.0, 32.0]),
    ("桌面上的 火星基地规划书.docx 文件图标", [760.0, 300.0, 860.0, 400.0]),
    ("菜单栏里的「格式」菜单项", [240.0, 8.0, 290.0, 29.0]),
];

/// Absent targets: `ok` must be false and `box_px` must be empty. This is the
/// "rather leave it blank than fill it wrong" property, tested directly.
#[tokio::test]
#[ignore = "calls a real vision model; run with --ignored"]
async fn absent_targets_fail_honestly() -> Result<()> {
    // Same logger as the other model-backed tests; without it the
    // `locate: not found in this image` line is invisible and there is no way
    // to tell 「said not found」 from 「guessed and happened to fail」.
    let _ = env_logger::builder()
        .format_timestamp_millis()
        .is_test(false)
        .try_init();
    let truth = load_truth()?;
    let path = dir().join(&truth.fixture);
    let img = image::open(&path)?.to_rgb8();
    let frame = LocateFrame::new(img);

    let provider = build_provider()?;
    let cancel = CancellationToken::new();
    let vision = ChatLocateVision::new(&provider, cancel.clone());

    let timeout_secs: u64 = std::env::var("POINTER_LOCATE_TIMEOUT_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(60);
    let per_case = std::time::Duration::from_secs(timeout_secs);
    println!(
        "mode: thinking={} timeout={}s targets=absent",
        if thinking_enabled() { "on" } else { "off" },
        timeout_secs
    );

    let mut honest = 0usize;
    let mut hallucinated: Vec<&str> = Vec::new();
    let mut boxed_on_failure: Vec<&str> = Vec::new();

    for (desc, h) in ABSENT_CASES {
        let hint = PxBox { x1: h[0], y1: h[1], x2: h[2], y2: h[3] };
        let requests = vec![LocateRequest::new(*desc, "点击它").with_hint_px(hint)];
        let started = std::time::Instant::now();
        let call = tokio::time::timeout(
            per_case,
            locate_elements(&vision, &frame, &requests, &LocateConfig::default(), &cancel),
        )
        .await;
        let elapsed = started.elapsed().as_secs_f64();

        let outcome = match call {
            Ok(Ok(mut v)) => v.pop(),
            Ok(Err(e)) => {
                println!("ERROR   {desc:<34} {e:#}");
                honest += 1;
                continue;
            }
            Err(_) => {
                println!("TIMEOUT {desc:<34} >{timeout_secs}s");
                honest += 1;
                continue;
            }
        };
        let Some(outcome) = outcome else {
            honest += 1;
            continue;
        };

        if outcome.ok {
            hallucinated.push(desc);
        } else {
            honest += 1;
            if outcome.box_px.is_some() {
                boxed_on_failure.push(desc);
            }
        }
        println!(
            "{} {desc:<34} ok={:<5} box={:?} rounds={} {:.1}s | {}",
            if outcome.ok { "HALLUCINATED" } else { "NOT-FOUND" },
            outcome.ok,
            outcome.box_px,
            outcome.rounds,
            elapsed,
            outcome.what_i_saw.chars().take(70).collect::<String>()
        );
    }

    println!(
        "\nabsent: {} cases — honest_fail={honest} hallucinated={} boxed_on_failure={}",
        ABSENT_CASES.len(),
        hallucinated.len(),
        boxed_on_failure.len()
    );
    assert!(
        hallucinated.is_empty(),
        "locator returned ok=true for absent target(s): {}",
        hallucinated.join(", ")
    );
    assert!(
        boxed_on_failure.is_empty(),
        "failed results carried coordinates for: {}",
        boxed_on_failure.join(", ")
    );
    Ok(())
}

// ---------------------------------------------------------------- 东方财富首页
//
// 与 frozen_desktop_* 分开：那张是通用桌面，这张是真实网页，标签密集、
// 存在同名/相邻元素。truth_eastmoney.json 的 bbox 全部来自 macOS Vision
// OCR 的字符框，不是人工估算。
//
// 两组用例：
//   kind=plain        普通定位，无 hint
//   kind=partial_hint hint 框住了目标的一部分，但框中心落在目标外
//
// partial_hint 组是这次新增的边缘测试：旧行为下 verify 只判「框里有没有
// 目标」，会判 yes 并把偏心的坐标返回，下游点空。期望是判 no。

async fn eastmoney_frame() -> Result<(TruthFile, LocateFrame)> {
    let name = std::env::var("POINTER_LOCATE_TRUTH").unwrap_or_else(|_| "truth_eastmoney.json".into());
    let truth = load_truth_file(&truth_path_named(&name))?;
    let path = dir().join(&truth.fixture);
    anyhow::ensure!(path.exists(), "missing fixture: {}", path.display());
    let img = image::open(&path)?.to_rgb8();
    let frame = LocateFrame::new(img);
    if let Some([w, h]) = truth.size {
        anyhow::ensure!(
            (frame.width(), frame.height()) == (w, h),
            "fixture size {}x{} != truth {}x{}",
            frame.width(),
            frame.height(),
            w,
            h
        );
    }
    Ok((truth, frame))
}

fn inside(pt: (f64, f64), b: [f64; 4]) -> bool {
    pt.0 >= b[0] && pt.0 <= b[2] && pt.1 >= b[1] && pt.1 <= b[3]
}

#[tokio::test]
#[ignore = "calls a real vision model; run with --ignored"]
async fn eastmoney_home_grounding() -> Result<()> {
    let (truth, frame) = eastmoney_frame().await?;
    let provider = build_provider()?;
    let vision = ChatLocateVision::new(&provider, CancellationToken::new());
    let cancel = CancellationToken::new();

    let cases: Vec<&TruthCase> = truth
        .cases
        .iter()
        .filter(|c| c.kind.is_empty() || c.kind == "plain")
        .collect();
    println!("\n=== eastmoney plain: {} cases ===", cases.len());

    let mut hits = 0usize;
    let mut misses: Vec<String> = Vec::new();
    let mut total_secs = 0.0f64;

    for case in &cases {
        let Some(bbox) = case.bbox else { continue };
        let req = LocateRequest::new(case.description.as_str(), "点击它");
        let started = std::time::Instant::now();
        let out = locate_elements(&vision, &frame, &[req], &LocateConfig::default(), &cancel).await?;
        let secs = started.elapsed().as_secs_f64();
        total_secs += secs;

        let Some(o) = out.into_iter().next() else {
            misses.push(case.description.clone());
            continue;
        };
        let hit = o.ok
            && o.point_px
                .map(|p| inside(p, bbox))
                .unwrap_or(false);
        if hit {
            hits += 1;
        } else {
            misses.push(case.description.clone());
        }
        println!(
            "{} {:<40} ok={:<5} pt={:?} conf={:?} reason={:?} rounds={} {:.1}s saw={} why={}",
            if hit { "PASS" } else { "FAIL" },
            case.description,
            o.ok,
            o.point_px.map(|(x, y)| (x.round() as i64, y.round() as i64)),
            o.confidence.map(|c| c.as_str()),
            o.reason,
            o.rounds,
            secs,
            o.what_i_saw,
            o.why
        );
    }

    let rate = hits as f64 / cases.len() as f64;
    println!(
        "\nplain: {}/{} hit ({:.1}%)  avg {:.1}s/case",
        hits,
        cases.len(),
        rate * 100.0,
        total_secs / cases.len() as f64
    );
    if !misses.is_empty() {
        println!("missed: {}", misses.join(" | "));
    }
    assert!(rate >= 0.7, "eastmoney plain hit rate {:.1}% below 70%", rate * 100.0);
    Ok(())
}

#[tokio::test]
#[ignore = "calls a real vision model; run with --ignored"]
async fn eastmoney_partial_hint_is_rejected() -> Result<()> {
    let (truth, frame) = eastmoney_frame().await?;
    let provider = build_provider()?;
    let vision = ChatLocateVision::new(&provider, CancellationToken::new());
    let cancel = CancellationToken::new();

    let cases: Vec<&TruthCase> = truth
        .cases
        .iter()
        .filter(|c| c.kind == "partial_hint")
        .collect();
    println!("\n=== eastmoney partial_hint: {} cases ===", cases.len());
    println!("期望：hint 框住了一部分但中心在目标外 → verify 判 no，不返回点不中的坐标\n");

    let mut rejected = 0usize;
    let mut leaked: Vec<String> = Vec::new();

    for case in &cases {
        let (Some(bbox), Some(hint)) = (case.bbox, case.hint_box) else {
            continue;
        };
        let hint_px = PxBox {
            x1: hint[0],
            y1: hint[1],
            x2: hint[2],
            y2: hint[3],
        };
        let req = LocateRequest::new(case.description.as_str(), "点击它").with_hint_px(hint_px);
        let started = std::time::Instant::now();
        let out = locate_elements(&vision, &frame, &[req], &LocateConfig::default(), &cancel).await?;
        let secs = started.elapsed().as_secs_f64();

        let Some(o) = out.into_iter().next() else {
            continue;
        };
        // 两种可接受结果：
        //   a) ok=false                    → 明确拒绝
        //   b) ok=true 但点落在目标内      → refine 纠回来了，也算对
        let good = match (o.ok, o.point_px) {
            (false, _) => true,
            (true, Some(p)) => inside(p, bbox),
            (true, None) => false,
        };
        if good {
            rejected += 1;
        } else {
            leaked.push(format!(
                "{} -> 返回 {:?}，中心落在目标外",
                case.description, o.point_px
            ));
        }
        let hint_c = ((hint[0] + hint[2]) / 2.0, (hint[1] + hint[3]) / 2.0);
        let hint_c_inside = inside(hint_c, bbox);
        println!(
            "{} {:<40} ok={:<5} pt={:?} rounds={} hint_center_inside={} {:.1}s  why={}",
            if good { "PASS" } else { "LEAK" },
            case.description,
            o.ok,
            o.point_px.map(|(x, y)| (x.round() as i64, y.round() as i64)),
            o.rounds,
            hint_c_inside,
            secs,
            o.why
        );
        if !o.reasoning.trim().is_empty() {
            println!("  [thinking] {}", o.reasoning.trim().replace('\n', " ⏎ "));
        } else {
            println!("  [thinking] <empty — POINTER_LOCATE_THINKING not set?>");
        }
    }

    println!(
        "\npartial_hint: {}/{} 正确处理",
        rejected,
        cases.len()
    );
    if !leaked.is_empty() {
        println!("leaked: {}", leaked.join(" | "));
    }
    assert!(
        leaked.is_empty(),
        "{} 个用例返回了点不中的坐标（框中心在目标外仍判 ok）",
        leaked.len()
    );
    Ok(())
}

// ------------------------------------------------- 同名干扰 + hint 消歧
//
// 页面上同一个名字出现多处（两个「东方财富期货」、两个「立即开户」、
// 两个「研报」）。prompt 里有一条规则：
//   【位置偏好】画面里如果有多个符合条件的元素，选离 (x, y) 最近的那个。
// 这组用例给一个指向正确目标的 hint，看规则是否真的生效。
//
// 对照组：hint 指向 A 就该选 A，指向 B 就该选 B。两次都选同一个位置，
// 说明模型没在看 hint。

#[tokio::test]
#[ignore = "calls a real vision model; run with --ignored"]
async fn eastmoney_hint_disambiguates_same_name() -> Result<()> {
    let (truth, frame) = eastmoney_frame().await?;
    let provider = build_provider()?;
    let vision = ChatLocateVision::new(&provider, CancellationToken::new());
    let cancel = CancellationToken::new();

    let cases: Vec<&TruthCase> = truth
        .cases
        .iter()
        .filter(|c| c.kind == "hint_disambig")
        .collect();
    println!("\n=== hint_disambig: {} cases ===", cases.len());
    println!("每组同名元素测两次：hint 指 A 该选 A，指 B 该选 B\n");

    let mut hits = 0usize;
    let mut misses: Vec<String> = Vec::new();

    for case in &cases {
        let (Some(bbox), Some(hint)) = (case.bbox, case.hint_box) else {
            continue;
        };
        let hint_px = PxBox {
            x1: hint[0],
            y1: hint[1],
            x2: hint[2],
            y2: hint[3],
        };
        let req =
            LocateRequest::new(case.description.as_str(), "点击它").with_hint_px(hint_px);
        let started = std::time::Instant::now();
        let out =
            locate_elements(&vision, &frame, &[req], &LocateConfig::default(), &cancel).await?;
        let secs = started.elapsed().as_secs_f64();

        let Some(o) = out.into_iter().next() else {
            continue;
        };
        let hit = o.ok && o.point_px.map(|p| inside(p, bbox)).unwrap_or(false);
        if hit {
            hits += 1;
        } else {
            misses.push(format!(
                "{} -> {:?}",
                case.description,
                o.point_px.map(|(x, y)| (x.round() as i64, y.round() as i64))
            ));
        }
        println!(
            "{} {:<46} ok={:<5} pt={:?} rounds={} {:.1}s  hint=({},{})",
            if hit { "PASS" } else { "FAIL" },
            case.description,
            o.ok,
            o.point_px.map(|(x, y)| (x.round() as i64, y.round() as i64)),
            o.rounds,
            secs,
            hint[0],
            hint[1]
        );
        if !o.reasoning.trim().is_empty() {
            println!("  [thinking] {}", o.reasoning.trim().replace('\n', " ⏎ "));
        }
    }

    let rate = hits as f64 / cases.len() as f64;
    println!(
        "\nhint_disambig: {}/{} 选对 ({:.1}%)",
        hits,
        cases.len(),
        rate * 100.0
    );
    if !misses.is_empty() {
        println!("missed:\n  {}", misses.join("\n  "));
    }
    assert!(rate >= 0.8, "hint 消歧命中率 {:.1}% 低于 80%", rate * 100.0);
    Ok(())
}

// ── Pointer overlay accuracy ────────────────────────────────────────────────
//
// Verify draws the pointer onto the before frame so it can tell "clicked this
// cell" from "clicked the neighbour". That judgement only works if the drawn
// pointer lands exactly where the caller asked — and on Retina a logical-vs-
// physical mix-up shifts every drawn point by half, which looks almost right to
// the eye while verify stably picks the wrong cell.
//
// `fixtures/sudoku.png` is a 1920×1080 capture of the secondary display with a
// 9×9 board on it; `truth/truth_sudoku.json` lists all 81 cells as cases. These
// tests do not call a model, so they run without `--ignored`.

/// Hotspot of the pointer bitmap — the pixel that means "here".
const POINTER_HOTSPOT: (i32, i32) = (3, 2);
const POINTER_BITMAP: u32 = 32;

fn sudoku_truth() -> Result<(TruthFile, PathBuf)> {
    let path = dir().join("truth").join("truth_sudoku.json");
    let truth = load_truth_file(&path)?;
    anyhow::ensure!(
        truth.cases.len() == 81,
        "truth_sudoku.json must list 81 cells, found {}",
        truth.cases.len()
    );
    Ok((truth, path))
}

/// Centre of a case's box, rounded to whole pixels.
fn case_center(case: &TruthCase) -> (i32, i32) {
    let b = case.bbox.expect("every sudoku case carries a bbox");
    (
        ((b[0] + b[2]) / 2.0).round() as i32,
        ((b[1] + b[3]) / 2.0).round() as i32,
    )
}

fn case_bounds(case: &TruthCase) -> (i32, i32, i32, i32) {
    let b = case.bbox.expect("every sudoku case carries a bbox");
    (
        b[0].round() as i32,
        b[1].round() as i32,
        b[2].round() as i32,
        b[3].round() as i32,
    )
}

/// Bounding box of every pixel that differs between two frames, inside `window`.
///
/// Scanning the whole 1920×1080 frame 81 times takes ~40 s; the pointer only
/// ever touches a 32×32 patch, so the search is bounded.
fn diff_bbox(
    a: &image::RgbaImage,
    b: &image::RgbaImage,
    window: (u32, u32, u32, u32),
) -> Option<(u32, u32, u32, u32)> {
    use image::GenericImageView;
    let (w, h) = a.dimensions();
    let (wx0, wy0, wx1, wy1) = window;
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0u32, 0u32);
    for y in wy0..wy1.min(h) {
        for x in wx0..wx1.min(w) {
            if a.get_pixel(x, y) != b.get_pixel(x, y) {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x);
                y1 = y1.max(y);
            }
        }
    }
    (x0 != u32::MAX).then_some((x0, y0, x1, y1))
}

/// Every cell listed in `truth_sudoku.json` is a test point: draw the pointer at
/// its centre and assert the changed pixels start at the hotspot offset. A
/// regression in the logical-vs-physical coordinate conversion shows up here as
/// a box at half the offset — on Retina that is exactly the bug this catches.
#[test]
fn pointer_overlay_lands_on_every_sudoku_cell() -> Result<()> {
    use pointer_core::agents::computer::vision::screen::MonitorInfo;
    use pointer_core::agents::computer::vision::screen_overlay::redraw_with_current_pointer;

    let (truth, _) = sudoku_truth()?;
    let path = dir().join(&truth.fixture);
    assert!(path.exists(), "missing fixture: {}", path.display());
    let raw = std::fs::read(&path)?;
    let full = image::load_from_memory(&raw)?.to_rgba8();
    if let Some([w, h]) = truth.size {
        assert_eq!(
            (full.width(), full.height()),
            (w, h),
            "fixture size does not match truth_sudoku.json"
        );
    }

    // Work on the board plus a margin: `redraw_with_current_pointer` decodes and
    // re-encodes the whole frame, so 81 points against a full 1920×1080 capture
    // costs ~37 s. Cropping first keeps the fixture realistic and the test fast.
    const MARGIN: u32 = 60;
    let (min_x, min_y, max_x, max_y) = truth
        .cases
        .iter()
        .filter_map(|c| c.bbox)
        .fold((f64::MAX, f64::MAX, 0.0f64, 0.0f64), |acc, b| {
            (acc.0.min(b[0]), acc.1.min(b[1]), acc.2.max(b[2]), acc.3.max(b[3]))
        });
    let crop_x = (min_x as u32).saturating_sub(MARGIN);
    let crop_y = (min_y as u32).saturating_sub(MARGIN);
    let crop_w = (max_x as u32 - crop_x) + MARGIN;
    let crop_h = (max_y as u32 - crop_y) + MARGIN;
    let original = image::imageops::crop_imm(&full, crop_x, crop_y, crop_w, crop_h).to_image();
    let (w, h) = original.dimensions();
    let monitor = MonitorInfo::new(crop_x as i32, crop_y as i32, w as i32, h as i32);

    // Encode once; `redraw_with_current_pointer` decodes from bytes, so both
    // sides of the diff come from the same source.
    let mut png = Vec::new();
    original.write_to(
        &mut std::io::Cursor::new(&mut png),
        image::ImageFormat::Png,
    )?;
    // A point outside the frame draws nothing — that is the baseline.
    let baseline = image::load_from_memory(&redraw_with_current_pointer(
        &png,
        &monitor,
        (-100, -100),
    )?)?;
    let baseline = baseline.to_rgba8();

    let mut failures = Vec::new();
    for case in &truth.cases {
        let (cx, cy) = case_center(case);
        let drawn = image::load_from_memory(&redraw_with_current_pointer(
            &png,
            &monitor,
            (cx, cy),
        )?)?;
        let drawn = drawn.to_rgba8();
        // `redraw_with_current_pointer` takes global coordinates and maps them
        // through the monitor origin; the diff window lives in crop pixels.
        let (lx, ly) = (cx - crop_x as i32, cy - crop_y as i32);
        let window = (
            (lx - 16).max(0) as u32,
            (ly - 16).max(0) as u32,
            (lx as u32 + 48).min(w),
            (ly as u32 + 48).min(h),
        );
        let tag = &case.scene;
        let Some((x0, y0, x1, y1)) = diff_bbox(&baseline, &drawn, window) else {
            failures.push(format!("{tag} ({cx},{cy}): pointer drew nothing"));
            continue;
        };
        // The bitmap is pasted at `(x - hotspot)`; its black outline reaches a
        // few pixels past that corner, and JPEG softens the rest. Anchor the
        // assertion on the diff corner with that slack — a coordinate
        // conversion bug (logical vs physical) lands tens of pixels away, so
        // this still catches it while tolerating the outline.
        let (want_x0, want_y0) = (lx - POINTER_HOTSPOT.0, ly - POINTER_HOTSPOT.1);
        let slack = 6i64;
        let off = |got: u32, want: i32| (got as i64 - want as i64).abs();
        if off(x0, want_x0) > slack || off(y0, want_y0) > slack {
            failures.push(format!(
                "{tag} ({cx},{cy}): diff starts at ({x0},{y0}), want ({want_x0},{want_y0}) ±{slack}"
            ));
            continue;
        }
        let (box_w, box_h) = (x1 - x0 + 1, y1 - y0 + 1);
        let cap = POINTER_BITMAP + 12;
        if box_w > cap || box_h > cap {
            failures.push(format!(
                "{tag} ({cx},{cy}): diff box {box_w}×{box_h} exceeds the pointer footprint"
            ));
            continue;
        }
        // A real pointer covers most of its 32×32 footprint; a stray
        // JPEG ripple covers a handful of pixels.
        if box_w < 12 || box_h < 12 {
            failures.push(format!(
                "{tag} ({cx},{cy}): diff box {box_w}×{box_h} is too small for the pointer"
            ));
            continue;
        }
        // The arrow tip — the corner the bitmap anchors on — must sit inside
        // the cell it was aimed at. The body extends down-right past that
        // corner and may cross a rule; the tip is what marks the click.
        let (cell_x0, cell_y0, cell_x1, cell_y1) = case_bounds(case);
        let tip_x = x0 as i32 + crop_x as i32;
        let tip_y = y0 as i32 + crop_y as i32;
        if tip_x < cell_x0 || tip_x >= cell_x1 || tip_y < cell_y0 || tip_y >= cell_y1 {
            failures.push(format!(
                "{tag} ({cx},{cy}): pointer tip at ({tip_x},{tip_y}) is outside cell ({cell_x0},{cell_y0})-({cell_x1},{cell_y1})"
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "{} of {} cells mis-placed:\n{}",
        failures.len(),
        truth.cases.len(),
        failures.join("\n")
    );
    Ok(())
}

/// Every case centre must sit inside its own box, and the boxes must tile the
/// board without gaps or overlap — a typo in the truth file fails loudly instead
/// of silently shifting a test point.
#[test]
fn sudoku_truth_cells_tile_the_board() -> Result<()> {
    let (truth, path) = sudoku_truth()?;
    let mut seen: Vec<(String, (i32, i32, i32, i32))> = Vec::new();
    for case in &truth.cases {
        let (cx, cy) = case_center(case);
        let (x0, y0, x1, y1) = case_bounds(case);
        assert!(
            cx > x0 && cx < x1 && cy > y0 && cy < y1,
            "{}: centre ({cx},{cy}) is outside its box ({x0},{y0})-({x1},{y1}) in {}",
            case.scene,
            path.display()
        );
        assert!(
            x1 > x0 && y1 > y0,
            "{}: degenerate box ({x0},{y0})-({x1},{y1})",
            case.scene
        );
        seen.push((case.scene.clone(), (x0, y0, x1, y1)));
    }
    // Nine rows, nine columns: every x0 and y0 appears exactly nine times.
    for (label, pick) in [
        ("x0", (|b: &(i32, i32, i32, i32)| b.0) as fn(&(i32, i32, i32, i32)) -> i32),
        ("y0", (|b: &(i32, i32, i32, i32)| b.1) as fn(&(i32, i32, i32, i32)) -> i32),
    ] {
        let mut counts = std::collections::HashMap::new();
        for (_, b) in &seen {
            *counts.entry(pick(b)).or_insert(0) += 1;
        }
        assert_eq!(
            counts.len(),
            9,
            "{label} has {} distinct values, expected 9 columns/rows: {counts:?}",
            counts.len()
        );
        for (value, n) in &counts {
            assert_eq!(*n, 9, "{label}={value} appears {n} times, expected 9");
        }
    }
    Ok(())
}
