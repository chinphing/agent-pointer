//! The B5 loop: locate → crop → verify → refine (design §6.1).
//!
//! Ported from `b5_decoupled.py::run_one`. The controller is pure Rust and
//! deterministic; all model judgement sits behind [`LocateVision`], which makes
//! every branch unit-testable without a network or a real screen.

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use super::geometry::{crop_side_px, nearly_same, px_to_rel, rel_to_norm, CropBounds, NormBox, PxBox};
use super::prompt::{locate_prompt, refine_hint, verify_prompt_views, LocateAnswer};
use super::render::{render_crop_opts, render_full, LocateFrame, LocateImage};
use super::trace::{
    FrameTrace, LocateStep, LocateTrace, OutcomeTrace, RequestTrace, RoundTrace, VerifyStep,
};
use super::types::{
    CenterOnTarget, Confidence, FailureReason, LocateConfig, LocateOutcome, LocateRequest,
    VerifyVerdict,
};

/// One switch for everything diagnostic — **the one in the settings UI**.
///
/// 设置 → 调试 → 「保存截图到本地」 (`debugSaveComputerCaptures`) already means
/// 「write what the computer agent saw to disk」. Making locate honour a second,
/// private switch would give two places to look when nothing appears, and the
/// person debugging a miss has no reason to know that locate has its own.
///
/// So: that checkbox turns on the trace log **and** the locate crops, written
/// next to the other captures under `<app data>/computer-captures/locate/`.
///
/// `POINTER_LOCATE_DEBUG=1` / `POINTER_LOCATE_DEBUG_DIR` stay as a back door for
/// tests and CLIs, which have no settings UI to tick. They are not the documented
/// path.
fn debug_on() -> bool {
    if crate::agents::computer::capture_debug::computer_capture_dump_enabled() {
        return true;
    }
    matches!(
        std::env::var("POINTER_LOCATE_DEBUG").as_deref().map(str::trim),
        Ok("1") | Ok("on") | Ok("true")
    )
}

/// Where images and traces are written.
///
/// An explicit `POINTER_LOCATE_DEBUG_DIR` wins — a test that writes to a temp
/// directory must not scribble in the user's app data. Otherwise the captures
/// live beside the other computer captures, which is where someone looking for
/// 「what did it see」 already goes.
fn debug_dir() -> Option<std::path::PathBuf> {
    if let Ok(d) = std::env::var("POINTER_LOCATE_DEBUG_DIR") {
        if !d.trim().is_empty() {
            return Some(std::path::PathBuf::from(d));
        }
    }
    if !debug_on() {
        return None;
    }
    crate::storage::app_data_dir()
        .ok()
        .map(|base| base.join("computer-captures").join("locate"))
}

/// `debug_log!("round={n} box={:?}", b)` — printed only while debugging is on.
///
/// Same content as the `log::info!` lines it replaces, but gated by the switch
/// instead of `RUST_LOG`, and written straight to `stderr` so it lands in the
/// same capture as the test's own `println!`.
macro_rules! debug_log {
    ($($arg:tt)*) => {
        if debug_on() {
            eprintln!("[locate] {}", format!($($arg)*));
        }
    };
}

/// A short id that ties one `locate_one` run's log lines and dumped images
/// together.
///
/// Diagnosing a miss from the log alone is hopeless once two runs interleave:
/// `round=2` appears dozens of times per screen and there is no way to tell
/// whose `verify` verdict belongs to whose `locate` box. Every line and every
/// dumped file carries `run=<id>`, so a failure can be replayed from disk.
fn new_run_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    // 6 hex chars of time + 2 of sequence: unique within a session, short enough
    // to read in a log line.
    format!("{:06x}{:02x}", ms & 0xff_ffff, n & 0xff)
}

/// How much of a target description a **log line** keeps.
///
/// Only the log truncates: the trace stores `target_desc` in full, and so does
/// every prompt. 40 was short enough that two different elements of the same
/// list — 「…里的『第三行』」 vs 「…里的『第四行』」 — cut to the same prefix, which
/// makes a log-only diagnosis actively misleading. The flag is printed next to
/// the text so a reader never has to guess whether they saw all of it.
const LOG_DESC_MAX: usize = 120;

/// `(text, truncated)` for a log line.
fn short_desc(desc: &str) -> (String, bool) {
    let mut chars = desc.chars();
    let head: String = chars.by_ref().take(LOG_DESC_MAX).collect();
    (head, chars.next().is_some())
}

/// Write `image` to `POINTER_LOCATE_DEBUG_DIR/<name>.jpg` when debugging is on.
///
/// Returns the **id** that was logged, so the log line and the file on disk are
/// the same string. Without a directory it still returns an id — the log then
/// says what *would* have been saved, which is enough to ask for a re-run with
/// dumping enabled.
fn dump_image(run: &str, name: &str, image: &LocateImage) -> String {
    let id = format!("{run}_{name}");
    let Some(dir) = debug_dir() else {
        return id;
    };
    if let Err(e) = std::fs::create_dir_all(&dir) {
        log::warn!("locate: debug dir {} unusable: {e}", dir.display());
        return id;
    }
    let file = dir.join(format!("{id}.jpg"));
    match std::fs::write(&file, &image.jpeg) {
        Ok(()) => debug_log!("dumped {} ({} bytes)", file.display(), image.jpeg.len()),
        Err(e) => log::warn!("locate: dump {} failed: {e}", file.display()),
    }
    id
}

/// Write `<run_id>.json` next to the JPEGs.
///
/// This is the artifact a person collects: one file per run, every image
/// referenced by relative name, `truth_bbox` empty and waiting. Copy the
/// directory off a user's machine, open the JSON, fill in the right box, and the
/// case is ready to be turned into a fixture — no re-typing the description, the
/// hint, or which round failed.
fn write_trace(trace: &LocateTrace) {
    let Some(dir) = debug_dir() else {
        return;
    };
    let path = dir.join(format!("{}.json", trace.run_id));
    match serde_json::to_string_pretty(trace) {
        Ok(json) => match std::fs::write(&path, json) {
            Ok(()) => debug_log!("trace -> {}", path.display()),
            Err(e) => log::warn!("locate: trace {} failed: {e}", path.display()),
        },
        Err(e) => log::warn!("locate: trace serialise failed: {e}"),
    }
}

/// The two model calls the controller is allowed to make.
#[async_trait]
pub trait LocateVision: Send + Sync {
    /// Return the target box in **image-local 0–1000** coordinates, or `NotFound`
    /// when the target is not in this image at all.
    async fn locate(&self, image: &LocateImage, prompt: &str) -> Result<Option<LocateAnswer>>;
    /// Judge the candidate box shown on the crop.
    async fn verify(&self, image: &LocateImage, prompt: &str) -> Result<VerifyVerdict>;
    /// Judge the candidate box, with an optional **wider** second image.
    ///
    /// The two images answer different questions, and the prompt says so:
    ///
    /// · the **close** crop is the original one — everything precise (does the
    ///   box sit on the cell's four borders, what is inside it) is judged from
    ///   it, because at 240 px a 41 px cell lands at 273 px in the image;
    /// · the **wide** crop covers four times the area at the same output size, so
    ///   the cell is only 137 px — too small to judge an edge, but big enough to
    ///   read the row/column labels around it.
    ///
    /// The wide image exists for one job only: confirming 「this is row 8, column
    /// 8」 from the labels. When a scene has no labels in reach the model is told
    /// to ignore it, which is why the close image stays authoritative.
    ///
    /// Default implementation ignores the wide image — strategies that have
    /// nothing extra to show keep working unchanged.
    async fn verify_pair(
        &self,
        close: &LocateImage,
        _wide: Option<&LocateImage>,
        prompt: &str,
    ) -> Result<VerifyVerdict> {
        self.verify(close, prompt).await
    }
}

/// Why the loop stopped. Mirrors B5 `stop_reason`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stop {
    Verified,
    LocateFailed,
    RoundsExhausted,
}

impl Stop {
    fn failure_reason(self) -> FailureReason {
        match self {
            Self::Verified => FailureReason::NotFound, // unreachable; kept total
            Self::LocateFailed => FailureReason::NotFound,
            Self::RoundsExhausted => FailureReason::NotVerified,
        }
    }
}

const NO_OBSERVATION: &str = "当前截图中未能可靠确认目标";

/// Locate every requested element, in input order. One element failing never
/// cancels the others (§6.2).
pub async fn locate_elements<V: LocateVision>(
    vision: &V,
    frame: &LocateFrame,
    requests: &[LocateRequest],
    cfg: &LocateConfig,
    cancel: &CancellationToken,
) -> Result<Vec<LocateOutcome>> {
    let mut out = Vec::with_capacity(requests.len());
    // One batch id per call, so the runs of a single `locate_element` can be
    // grouped after the fact. Without it, three elements located in one call are
    // indistinguishable from three separate calls, and a failure that only shows
    // up when several targets share a screen is invisible.
    let batch = if requests.len() > 1 {
        Some(new_run_id())
    } else {
        None
    };
    for (index, req) in requests.iter().enumerate() {
        let mut cfg = cfg.clone();
        if cfg.trace_batch.is_none() {
            cfg.trace_batch = batch.clone();
            cfg.trace_index = index;
        }
        out.push(locate_one(vision, frame, req, &cfg, cancel).await?);
    }
    Ok(out)
}

/// Run the B5 loop for one element.
pub async fn locate_one<V: LocateVision>(
    vision: &V,
    frame: &LocateFrame,
    req: &LocateRequest,
    cfg: &LocateConfig,
    cancel: &CancellationToken,
) -> Result<LocateOutcome> {
    let (outcome, trace) = locate_one_traced(vision, frame, req, cfg, cancel).await?;
    dump_annotated_frame(frame, &trace);
    write_trace(&trace);
    Ok(outcome)
}

/// Draw every box the run produced onto one copy of the frame, plus the point
/// that will be clicked.
///
/// The crops each show their own box in their own window; nothing says where
/// that window is on the screen. 「偏了一格」 is a question about the whole
/// screen — this image answers it at a glance.
fn dump_annotated_frame(frame: &LocateFrame, trace: &LocateTrace) {
    if debug_dir().is_none() {
        return;
    }
    let mut boxes: Vec<PxBox> = Vec::new();
    for round in &trace.rounds {
        if let Some(step) = round.locate.as_ref() {
            if let Some(b) = step.box_px {
                boxes.push(PxBox {
                    x1: b[0],
                    y1: b[1],
                    x2: b[2],
                    y2: b[3],
                });
            }
        }
        if let Some(v) = round.verify.as_ref() {
            let b = v.box_px;
            let last = boxes.last().copied();
            let same = last.is_some_and(|l| {
                (l.x1 - b[0]).abs() < 1.0
                    && (l.y1 - b[1]).abs() < 1.0
                    && (l.x2 - b[2]).abs() < 1.0
                    && (l.y2 - b[3]).abs() < 1.0
            });
            if !same {
                boxes.push(PxBox {
                    x1: b[0],
                    y1: b[1],
                    x2: b[2],
                    y2: b[3],
                });
            }
        }
    }
    let point = trace.outcome.point.map(|p| (p[0], p[1]));
    match super::render::render_annotated_frame(frame, &boxes, point) {
        Ok(img) => {
            dump_image(&trace.run_id, "annotated", &img);
        }
        Err(e) => log::warn!("locate: annotated frame failed: {e}"),
    }
}

/// The loop itself, returning the trace **without** writing it.
///
/// Split out so the record can be asserted in a unit test: `write_trace` is a
/// no-op unless `POINTER_LOCATE_DEBUG_DIR` is set, and a test that has to create
/// a directory and guess the run id is not worth writing. Everything a collector
/// later relies on — the hint box, the dumped JPEG's size, the full prompts — is
/// visible here.
async fn locate_one_traced<V: LocateVision>(
    vision: &V,
    frame: &LocateFrame,
    req: &LocateRequest,
    cfg: &LocateConfig,
    cancel: &CancellationToken,
) -> Result<(LocateOutcome, LocateTrace)> {
    let (w, h) = (frame.width().max(1), frame.height().max(1));
    let full = CropBounds::full(w, h);
    // One id per run: every log line and every dumped image carries it, so a
    // failure can be traced from `round=2` back to the exact crop the model saw.
    let run = new_run_id();
    let started_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let (desc_log, desc_cut) = short_desc(&req.target_desc);
    debug_log!(
        "locate: start run={run} frame={}x{} desc={:?} desc_truncated={desc_cut} hint={:?} max_locate={}",
        w,
        h,
        desc_log,
        req.hint_center().map(|(x, y)| [x.round(), y.round()]),
        cfg.max_locate,
    );
    // The full frame is dumped once per run, not once per round: it is what the
    // whole trace is measured against, and a labelled `truth_bbox` is only
    // meaningful in its coordinate space.
    //
    // Its **encoded** size is recorded, not `w`/`h`: `render_full` downscales
    // anything above `FULL_MAX_PX`, so on a 2560×1440 screen the JPEG on disk is
    // smaller than the frame. A box measured on that JPEG (the only image a
    // person can open) would otherwise be off by the scale factor, and nothing in
    // the trace would say so.
    let full_image = render_full(frame, None)?;
    let frame_path = dump_image(&run, "frame", &full_image);
    let mut trace = LocateTrace {
        run_id: run.clone(),
        started_at,
        session: cfg.trace_session.clone(),
        batch: cfg.trace_batch.clone(),
        index: cfg.trace_index,
        frame: FrameTrace {
            path: format!("{frame_path}.jpg"),
            width: w,
            height: h,
            dumped: Some([full_image.width, full_image.height]),
        },
        request: RequestTrace {
            target_desc: req.target_desc.clone(),
            goal: req.goal.clone(),
            hint: req.hint_center().map(|(x, y)| [x, y]),
            hint_box: req.hint_px.map(|b| [b.x1, b.y1, b.x2, b.y2]),
        },
        rounds: Vec::new(),
        outcome: OutcomeTrace {
            ok: false,
            point: None,
            box_px: None,
            rounds: 0,
            stop: String::new(),
        },
        truth_bbox: None,
        note: String::new(),
    };
    // The prompt has to know whether a second image is coming: 图二 exists only
    // to read row/column labels, and a prompt that does not say so gets the box
    // judged from the coarse image.
    let verify_text = verify_prompt_views(&req.target_desc, cfg.verify_mult > 1.0);

    let mut box_norm = req.hint_norm(w, h);
    let mut last_saw = String::new();
    let mut last_why = String::new();
    let mut last_reasoning = String::new();
    let mut final_confidence = Confidence::High;
    let mut rounds = 0usize;
    let mut stop = Stop::RoundsExhausted;

    // ① First locate — skipped entirely when the caller supplied a hint box.
    if box_norm.is_none() {
        // Same encoded image the frame JPEG was written from: re-rendering it
        // would run Lanczos3 over the whole screen a second time and could only
        // produce the same bytes.
        let (attempt, step) = locate_in(vision, &run, "full_init", &full_image, full, w, h, &req.target_desc, req.hint_center(), None, None).await;
        trace.rounds.push(RoundTrace {
            round: 0,
            locate: Some(step),
            verify: None,
        });
        box_norm = match attempt {
            LocateAttempt::Found(b) => Some(b),
            // On the full screen there is nowhere wider to look.
            LocateAttempt::NotFound | LocateAttempt::Failed => None,
        };
        if box_norm.is_none() {
            stop = Stop::LocateFailed;
        }
    }

    // ② verify → ③ decide, at most `max_locate` rounds.
    loop {
        if rounds >= cfg.max_locate {
            break;
        }
        let Some(current) = box_norm else {
            break;
        };
        if cancel.is_cancelled() {
            return Err(anyhow!("locate_element cancelled"));
        }
        rounds += 1;

        let side = crop_side_px(
            current.to_px(w, h),
            cfg.crop_mult,
            cfg.crop_min,
            cfg.crop_max,
        );
        // The verifier no longer offers `unclear`, so the crop is rendered once
        // per round. `retry` survives only as a field on the outcome.
        let verify_retries = 0usize;
        let _ = verify_retries;

        // Inner loop: one crop, retried with a different `side` while the
        // verifier reports `unclear` for a fixable reason.
        // `loop` with only `break`s: clippy is right that it never loops, but it
        // cannot be a plain block because the body uses `break` to leave the
        // outer round loop (`if matches!(stop, Stop::Verified) { break; }`), and
        // `break` is only legal inside a loop. Rewriting it to a block would mean
        // turning that into a labelled break out of the round loop, which is a
        // larger change than this file's job.
        #[allow(clippy::never_loop)]
        let (verdict, last_crop) = loop {
            let t_render = std::time::Instant::now();
            let (image, bounds) =
                render_crop_opts(frame, current, side, cfg.out_px, super::render::dim_enabled())?;
            // 图一 — the original close crop. Everything precise is judged from
            // it: at 240 px a 41 px cell lands at 273 px in the image, which is
            // enough to see whether the box sits on the four borders.
            //
            // 图二 — same centre, `verify_mult` times the area, **same output
            // size**. One job only: reading the row/column labels around the
            // target to confirm 「this is row 8, column 8」. At 480 px the same
            // cell lands at 137 px — too coarse to judge an edge, which is exactly
            // why the prompt tells the model to use 图一 for that and to ignore
            // 图二 when no labels are in reach.
            let wide_image = if cfg.verify_mult > 1.0 {
                let wide_px = if cfg.verify_out_px == 0 {
                    cfg.out_px
                } else {
                    cfg.verify_out_px
                };
                let img = render_crop_opts(
                    frame,
                    current,
                    side * cfg.verify_mult,
                    wide_px,
                    super::render::dim_enabled(),
                )?
                .0;
                Some(img)
            } else {
                None
            };
            // Both crops get an id and, when `POINTER_LOCATE_DEBUG_DIR` is set,
            // a file. A verdict saying 「no row labels」 is otherwise
            // unfalsifiable — you cannot tell whether the labels were outside
            // the crop or the model simply did not look.
            let close_id = dump_image(&run, &format!("verify_r{rounds}_close"), &image);
            let wide_id = wide_image
                .as_ref()
                .map(|w| dump_image(&run, &format!("verify_r{rounds}_wide"), w));
            debug_log!(
                "locate: verify_input run={run} round={rounds} side={side:.0} box=[{:.0},{:.0},{:.0},{:.0}] close_id={close_id} close={}x{} wide_id={:?} wide={:?}",
                current.to_px(w, h).x1,
                current.to_px(w, h).y1,
                current.to_px(w, h).x2,
                current.to_px(w, h).y2,
                image.width,
                image.height,
                wide_id,
                wide_image.as_ref().map(|w| format!("{}x{}", w.width, w.height)),
            );
            let t_call = std::time::Instant::now();
            let v = match vision
                .verify_pair(&image, wide_image.as_ref(), &verify_text)
                .await
            {
                Ok(v) => v,
                Err(e) => {
                    log::warn!("locate_element: verify call failed: {e}");
                    VerifyVerdict::default()
                }
            };
            let call_ms = t_call.elapsed().as_secs_f64() * 1000.0;
            // Split the round's wall clock into its two halves. Without this the
            // only visible number is the whole case, and 「verify got slower」 is
            // indistinguishable from 「the model got slower」 — rendering two
            // crops at 1600² and 1920² is a real cost that never shows up in a
            // per-case timing.
            debug_log!(
                "locate: timing run={run} round={rounds} render={:.0}ms verify_call={:.0}ms close={}x{} wide={}",
                (t_call - t_render).as_secs_f64() * 1000.0,
                call_ms,
                image.width,
                image.height,
                wide_image
                    .as_ref()
                    .map(|w| format!("{}x{}", w.width, w.height))
                    .unwrap_or_else(|| "-".to_string()),
            );
            // Every verdict, not just failures: without this line a locate run
            // is a black box — you cannot tell whether the box was too tight,
            // the crop too small, or the target genuinely absent.
            debug_log!(
                "locate: verify_output run={run} round={rounds} center={:?} size={:?} position={} close_id={close_id} notes={}",
                v.center_on_target,
                v.size_ok,
                v.position_matches.as_str(),
                v.notes.trim()
            );
            trace.rounds.push(RoundTrace {
                round: rounds,
                locate: None,
                verify: Some(VerifyStep {
                    close_image: format!("{close_id}.jpg"),
                    wide_image: wide_id.as_ref().map(|id| format!("{id}.jpg")),
                    box_px: [
                        current.to_px(w, h).x1,
                        current.to_px(w, h).y1,
                        current.to_px(w, h).x2,
                        current.to_px(w, h).y2,
                    ],
                    center: format!("{:?}", v.center_on_target),
                    size: format!("{:?}", v.size_ok),
                    position: v.position_matches.as_str().to_string(),
                    notes: v.notes.trim().to_string(),
                    render_ms: (t_call - t_render).as_millis() as u64,
                    call_ms: call_ms as u64,
                    // The same prompt every round — recorded per step anyway so
                    // that one `rounds[]` entry can be read (and replayed) on its
                    // own, without cross-referencing another step.
                    prompt: Some(verify_text.clone()),
                }),
            });
            // `notes` opens with what is actually in the box, so it doubles as
            // the "what I saw" line shown to the user on failure.
            if !v.notes.trim().is_empty() {
                last_saw = v.notes.clone();
                last_why = v.notes.clone();
                last_reasoning = v.reasoning.clone();
            }
            let crop = Some((image, bounds));

            // Three independent checks, all made by the verifier:
            //   ① `center_on_target` — is the mark on the target? The mark sits at
            //      the box centre, so this decides the returned coordinate.
            //   ② `size_ok` — does the box frame the target properly?
            //   ③ `position_matches` — is this the cell the description names?
            // Code reads the three verdicts; it never re-derives them from
            // numbers or prose.
            if v.is_hit() {
                final_confidence = v.confidence;
                stop = Stop::Verified;
                break (v, crop);
            }
            // Anything else is a retry: `refine_hint` turns the verifier's own
            // `notes` into the next round's hint, so the locator is told exactly
            // which dimension failed and what was seen.
            break (v, crop);
        };

        if matches!(stop, Stop::Verified) {
            break;
        }

        // —— Not verified: ask the locator again ——
        //
        // `BoxTooTight` lands here too, on purpose. It used to grow the box in
        // code (`grown(1.5)`) and re-verify around the same centre, which fixed
        // nothing: a box holding one letter of a 75px label needs six times the
        // width, not 1.5, and that grow happened on the round that was already
        // the last one, so the widened box was never verified at all. A fixed
        // factor can only cover one size of miss, and this miss has no fixed
        // size. `refine_hint` says what the verifier actually saw and asks for
        // the full outline in the same place, so the locator — which knows the
        // label from `target_desc` — can hand back a box that covers all of it.
        let hint = refine_hint(&verdict);

        // Re-locate inside the current crop whenever the target is known to be
        // near where we already looked:
        //  · `elsewhere_dir` — the verifier saw it just outside the box;
        //  · `BoxTooLoose` — the verifier saw it *inside* the box and only
        //    complained about slack;
        //  · `BoxTooTight` — the verifier saw part of it inside the box.
        // Falling through to a full-screen search was wrong for all three: the
        // target never moved, so re-scanning 1920x1080 re-derived the same answer
        // from a harder image (and for `BoxTooLoose` it also blew the timeout).
        // A `BoxTooTight` case did exactly that — the retry jumped from the
        // button at x≈934 to x≈532, then needed a third round to come back.
        // Stay inside the previous crop unless the verifier failed to produce a
        // usable verdict at all (`Unclear` is the parse-failure fallback, never a
        // value the model is offered). A `center = no` means the mark is off the
        // target — the target is right there, the box is just misplaced — so the
        // crop is exactly where it should be re-located: at 8x the cell's borders
        // are legible, on the full screen they are not.
        let stay_local = verdict.center_on_target != CenterOnTarget::Unclear;

        let mut from_crop = false;
        let mut next_box = None;
        // Set when the crop-local locator answered 「not in this image」. The
        // crop is 240–294 px around a candidate that may be several hundred px
        // off, so this is the normal answer when the hint is far away — the fix
        // is a wider view, not a retry on the same crop.
        let mut widen_needed = false;
        if stay_local {
            if let Some((image, bounds)) = last_crop.as_ref() {
                from_crop = true;
                let (attempt, step) = locate_in(
                    vision,
                    &run,
                    &format!("crop_r{rounds}"),
                    image,
                    *bounds,
                    w,
                    h,
                    &req.target_desc,
                    req.hint_center(),
                    Some(&hint),
                    Some(current.to_px(w, h)),
                )
                .await;
                trace.rounds.push(RoundTrace {
                    round: rounds,
                    locate: Some(step),
                    verify: None,
                });
                next_box = match attempt {
                    LocateAttempt::Found(b) => Some(b),
                    LocateAttempt::NotFound => {
                        widen_needed = true;
                        None
                    }
                    LocateAttempt::Failed => None,
                };
            }
        }
        if !from_crop || widen_needed {
            let image = render_full(frame, Some(current))?;
            let (attempt, step) = locate_in(vision, &run, &format!("full_widen_r{rounds}"), &image, full, w, h, &req.target_desc, req.hint_center(), Some(&hint), Some(current.to_px(w, h))).await;
            trace.rounds.push(RoundTrace {
                round: rounds,
                locate: Some(step),
                verify: None,
            });
            next_box = match attempt {
                LocateAttempt::Found(b) => Some(b),
                LocateAttempt::NotFound | LocateAttempt::Failed => None,
            };
        }

        // No-progress guard: a crop-local re-locate that barely moved is
        // escalated to the full screen (B5).
        let mut next_box = match next_box {
            Some(b) => b,
            None => {
                stop = Stop::LocateFailed;
                break;
            }
        };
        if from_crop && !widen_needed && nearly_same(next_box, current) {
            let image = render_full(frame, Some(current))?;
            let (attempt, step) = locate_in(vision, &run, &format!("full_escalate_r{rounds}"), &image, full, w, h, &req.target_desc, req.hint_center(), Some(&hint), Some(current.to_px(w, h))).await;
            trace.rounds.push(RoundTrace {
                round: rounds,
                locate: Some(step),
                verify: None,
            });
            next_box = match attempt {
                LocateAttempt::Found(b) => b,
                LocateAttempt::NotFound | LocateAttempt::Failed => {
                    stop = Stop::LocateFailed;
                    break;
                }
            };
        }
        box_norm = Some(next_box);
    }

    let what_i_saw = if last_saw.trim().is_empty() {
        NO_OBSERVATION.to_string()
    } else {
        last_saw
    };

    trace.outcome.rounds = rounds;
    trace.outcome.stop = format!("{stop:?}");
    trace.outcome.ok = stop == Stop::Verified;
    if let Some(b) = box_norm {
        let px = b.to_px(w, h);
        trace.outcome.box_px = Some([px.x1, px.y1, px.x2, px.y2]);
        let (cx, cy) = b.center();
        trace.outcome.point = Some([cx / 1000.0 * w as f64, cy / 1000.0 * h as f64]);
    }

    if stop != Stop::Verified {
        let mut out = LocateOutcome::failure(
            req.target_desc.clone(),
            stop.failure_reason(),
            what_i_saw,
            rounds,
        );
        out.why = last_why;
        return Ok((out, trace));
    }

    let Some(b) = box_norm else {
        return Ok((
            LocateOutcome::failure(
                req.target_desc.clone(),
                FailureReason::NotFound,
                what_i_saw,
                rounds,
            ),
            trace,
        ));
    };
    let (cx, cy) = b.center();
    let point = (cx / 1000.0 * w as f64, cy / 1000.0 * h as f64);
    Ok((
        LocateOutcome {
            target_desc: req.target_desc.clone(),
            ok: true,
            point_px: Some(point),
            box_px: Some(b.to_px(w, h)),
            confidence: Some(final_confidence),
            what_i_saw,
            why: last_why,
            reasoning: last_reasoning,
            rounds,
            reason: None,
        },
        trace,
    ))
}

/// One `locate` call's outcome.
///
/// `NotFound` is kept apart from `Failed` on purpose: the locator saying 「not in
/// this image」 is actionable (render wider and look again), while a parse
/// failure or a call error is not. Collapsing them into `Option::None` made the
/// far-hint fixture abort instead of escalating.
enum LocateAttempt {
    Found(NormBox),
    NotFound,
    Failed,
}

/// One locate call, mapped from image-local 0–1000 back to screen 0–1000.
///
/// `prev_box` is the previous round's box in **screen pixels**; it is converted
/// to this image's 0–1000 scale here, because that is the only scale the prompt
/// and the answer share. `hint_px` gets the same treatment for the same reason:
/// it used to be passed through as raw screen pixels even when the image was a
/// crop, so the point the prompt named and the picture the model saw disagreed
/// by the crop offset.
async fn locate_in<V: LocateVision>(
    vision: &V,
    run: &str,
    where_: &str,
    image: &LocateImage,
    bounds: CropBounds,
    w: u32,
    h: u32,
    description: &str,
    hint_px: Option<(f64, f64)>,
    previous_hint: Option<&str>,
    prev_box: Option<PxBox>,
) -> (LocateAttempt, LocateStep) {
    let hint_rel = hint_px.map(|(x, y)| {
        let r = px_to_rel(
            PxBox {
                x1: x,
                y1: y,
                x2: x,
                y2: y,
            },
            bounds,
        );
        (r.0, r.1)
    });
    let prev_rel = prev_box.map(|b| px_to_rel(b, bounds));
    let prompt = locate_prompt(description, hint_rel, previous_hint, prev_rel);
    let image_id = dump_image(run, &format!("locate_{where_}"), image);
    let (desc_log, desc_cut) = short_desc(description);
    // The inputs matter as much as the answer: a box that is wrong because the
    // crop was wrong looks identical to a box that is wrong because the model
    // misread it. `bounds` is what the model actually saw, `prev` is what it was
    // told to fix, `hint` is where the caller thought the target was.
    debug_log!(
        "locate: call run={run} where={where_} image_id={image_id} size={}x{} bounds=[{:.0},{:.0},{:.0},{:.0}] prev={:?} hint={:?} desc={:?} desc_truncated={desc_cut}",
        image.width,
        image.height,
        bounds.x0,
        bounds.y0,
        bounds.x1,
        bounds.y1,
        prev_box.map(|b| [b.x1.round(), b.y1.round(), b.x2.round(), b.y2.round()]),
        hint_px.map(|(x, y)| [x.round(), y.round()]),
        desc_log,
    );
    let mut step = LocateStep {
        where_: where_.to_string(),
        image: format!("{image_id}.jpg"),
        bounds: [bounds.x0, bounds.y0, bounds.x1, bounds.y1],
        prev_box: prev_box.map(|b| [b.x1, b.y1, b.x2, b.y2]),
        raw: None,
        box_px: None,
        not_found: false,
        prompt: Some(prompt.clone()),
    };
    match vision.locate(image, &prompt).await {
        Ok(Some(LocateAnswer::Found(raw))) => {
            // The prompt asks for 0-1000 coordinates relative to this image. Log
            // the raw answer next to the mapped one: a model that reasons in
            // pixels and answers in pixels looks identical to a correct one
            // until the two are compared.
            let mapped = rel_to_norm(raw, bounds, w, h);
            debug_log!(
                "locate: answer run={run} where={where_} raw=[{:.0},{:.0},{:.0},{:.0}] -> px=[{:.0},{:.0},{:.0},{:.0}] (rel=[{:.0},{:.0},{:.0},{:.0}])",
                raw.x1,
                raw.y1,
                raw.x2,
                raw.y2,
                mapped.to_px(w, h).x1,
                mapped.to_px(w, h).y1,
                mapped.to_px(w, h).x2,
                mapped.to_px(w, h).y2,
                mapped.x1,
                mapped.y1,
                mapped.x2,
                mapped.y2,
            );
            step.raw = Some([raw.x1, raw.y1, raw.x2, raw.y2]);
            // `mapped` is still in the image's 0–1000 space; `to_px` is what the
            // rest of the trace speaks. Storing the normalised value here made
            // every locate box look ~5x smaller than the verify box it fed.
            let px = mapped.to_px(w, h);
            step.box_px = Some([px.x1, px.y1, px.x2, px.y2]);
            (LocateAttempt::Found(mapped), step)
        }
        Ok(Some(LocateAnswer::NotFound)) => {
            step.not_found = true;
            // Distinct from a parse failure, and distinct from 「found a box but
            // it was wrong」: the locator looked and said the target is not in
            // this image. Recorded because the two have opposite fixes — this
            // one needs a wider view, the other needs a better box — and from
            // the final outcome alone they are indistinguishable (both end as
            // `rounds_exhausted` or a silent miss).
            debug_log!(
                "locate: not_found run={run} where={where_} image_id={image_id} bounds=[{:.0},{:.0},{:.0},{:.0}]",
                bounds.x0,
                bounds.y0,
                bounds.x1,
                bounds.y1,
            );
            (LocateAttempt::NotFound, step)
        }
        Ok(None) | Err(_) => (LocateAttempt::Failed, step),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::computer::locate::geometry::{NormBox, PxBox};
    use crate::agents::computer::locate::types::{MatchVerdict, PositionMatches};
    use image::{Rgb, RgbImage};
    use std::collections::VecDeque;
    use std::sync::Mutex;

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Call {
        Locate { width: u32, height: u32 },
        Verify { width: u32, height: u32 },
    }

    #[derive(Default)]
    struct FakeVision {
        locate_script: Mutex<VecDeque<Option<NormBox>>>,
        verify_script: Mutex<VecDeque<VerifyVerdict>>,
        calls: Mutex<Vec<Call>>,
    }

    impl FakeVision {
        fn new() -> Self {
            Self::default()
        }

        fn with_locate(self, boxes: Vec<Option<NormBox>>) -> Self {
            *self.locate_script.lock().unwrap() = boxes.into();
            self
        }

        fn with_verify(self, verdicts: Vec<VerifyVerdict>) -> Self {
            *self.verify_script.lock().unwrap() = verdicts.into();
            self
        }

        fn calls(&self) -> Vec<Call> {
            self.calls.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl LocateVision for FakeVision {
        async fn locate(&self, image: &LocateImage, _prompt: &str) -> Result<Option<LocateAnswer>> {
            self.calls.lock().unwrap().push(Call::Locate {
                width: image.width,
                height: image.height,
            });
            Ok(self
                .locate_script
                .lock()
                .unwrap()
                .pop_front()
                .flatten()
                .map(LocateAnswer::Found))
        }

        async fn verify(&self, image: &LocateImage, _prompt: &str) -> Result<VerifyVerdict> {
            self.calls.lock().unwrap().push(Call::Verify {
                width: image.width,
                height: image.height,
            });
            Ok(self
                .verify_script
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_default())
        }
    }

    fn frame() -> LocateFrame {
        LocateFrame::new(RgbImage::from_pixel(1000, 800, Rgb([40, 40, 40])))
    }

    fn cfg() -> LocateConfig {
        LocateConfig {
            out_px: 64,
            crop_min: 40.0,
            crop_max: 400.0,
            ..LocateConfig::default()
        }
    }

    /// A verdict whose three checks are all satisfied unless the caller overrides
    /// them — tests then say only what they care about.
    fn verdict(notes: &str) -> VerifyVerdict {
        VerifyVerdict {
            center_on_target: CenterOnTarget::Yes,
            size_ok: MatchVerdict::Yes,
            position_matches: PositionMatches::Ignore,
            notes: notes.to_string(),
            ..VerifyVerdict::default()
        }
    }

    fn yes(in_box: &str) -> VerifyVerdict {
        verdict(in_box)
    }

    fn no_left(in_box: &str) -> VerifyVerdict {
        VerifyVerdict {
            center_on_target: CenterOnTarget::No,
            ..verdict(in_box)
        }
    }

    fn cancel_token() -> CancellationToken {
        CancellationToken::new()
    }

    #[tokio::test]
    async fn without_hint_runs_full_locate_then_verifies() {
        let vision = FakeVision::new()
            .with_locate(vec![Some(NormBox::new(400.0, 300.0, 500.0, 400.0))])
            .with_verify(vec![yes("蓝色软盘图标")]);
        let out = locate_one(
            &vision,
            &frame(),
            &LocateRequest::new("保存按钮", "点击它"),
            &cfg(),
            &cancel_token(),
        )
        .await
        .unwrap();

        assert!(out.ok);
        assert_eq!(out.rounds, 1);
        assert_eq!(out.confidence, Some(Confidence::High));
        assert_eq!(out.what_i_saw, "蓝色软盘图标");
        let (x, y) = out.point_px.unwrap();
        assert!((x - 450.0).abs() < 1e-6);
        assert!((y - 280.0).abs() < 1e-6);
        let b = out.box_px.unwrap();
        assert!((b.x1 - 400.0).abs() < 1e-6 && (b.y2 - 320.0).abs() < 1e-6);
        assert_eq!(
            vision.calls(),
            vec![
                Call::Locate {
                    width: 1000,
                    height: 800
                },
                Call::Verify {
                    width: 64,
                    height: 64
                }
            ]
        );
    }

    #[tokio::test]
    async fn hint_skips_the_full_screen_locate() {
        let hint = PxBox {
            x1: 800.0,
            y1: 35.0,
            x2: 840.0,
            y2: 55.0,
        };
        let vision = FakeVision::new().with_verify(vec![yes("保存按钮")]);
        let out = locate_one(
            &vision,
            &frame(),
            &LocateRequest::new("保存按钮", "点击它").with_hint_px(hint),
            &cfg(),
            &cancel_token(),
        )
        .await
        .unwrap();

        assert!(out.ok);
        assert_eq!(out.rounds, 1);
        // No locate call at all — the hint became round 1's box.
        assert_eq!(
            vision.calls(),
            vec![Call::Verify {
                width: 64,
                height: 64
            }]
        );
        let (x, y) = out.point_px.unwrap();
        assert!((x - 820.0).abs() < 1e-6, "x={x}");
        assert!((y - 45.0).abs() < 1e-6, "y={y}");
        // Same coordinate space as the hint itself (px in, px out).
        let b = out.box_px.unwrap();
        assert!((b.x1 - 800.0).abs() < 1e-6);
        assert!((b.y2 - 55.0).abs() < 1e-6);
    }

    #[tokio::test]
    async fn degenerate_hint_falls_back_to_full_locate() {
        let vision = FakeVision::new()
            .with_locate(vec![Some(NormBox::new(0.0, 0.0, 100.0, 100.0))])
            .with_verify(vec![yes("某元素")]);
        let hint = PxBox {
            x1: 10.0,
            y1: 10.0,
            x2: 10.0,
            y2: 10.0,
        };
        let out = locate_one(
            &vision,
            &frame(),
            &LocateRequest::new("某元素", "点击它").with_hint_px(hint),
            &cfg(),
            &cancel_token(),
        )
        .await
        .unwrap();
        assert!(out.ok);
        assert!(matches!(
            vision.calls().first(),
            Some(Call::Locate {
                width: 1000,
                height: 800
            })
        ));
    }

    #[tokio::test]
    async fn box_too_tight_relocates_inside_the_crop_not_on_the_full_frame() {
        // Same reasoning as the loose case: part of the target is inside the box,
        // so the answer is nearby. A full-screen re-locate once jumped from the
        // button at x≈934 to x≈532 and needed a third round to come back.
        let hint = PxBox {
            x1: 400.0,
            y1: 300.0,
            x2: 500.0,
            y2: 400.0,
        };
        // Centre on the target, but the box only holds part of it — a size
        // problem, not a position one.
        let tight = VerifyVerdict {
            size_ok: MatchVerdict::No,
            ..verdict("保存按钮")
        };
        let vision = FakeVision::new()
            .with_verify(vec![tight, yes("保存按钮")])
            .with_locate(vec![Some(NormBox::new(100.0, 100.0, 300.0, 300.0))]);
        let out = locate_one(
            &vision,
            &frame(),
            &LocateRequest::new("保存按钮", "点击它").with_hint_px(hint),
            &cfg(),
            &cancel_token(),
        )
        .await
        .unwrap();

        assert!(out.ok, "{out:?}");
        assert_eq!(out.rounds, 2);
        for call in vision.calls() {
            if let Call::Locate { width, height } = call {
                assert_eq!(
                    (width, height),
                    (64, 64),
                    "locator ran on the full frame instead of the crop"
                );
            }
        }
    }

    #[tokio::test]
    async fn box_too_loose_relocates_inside_the_crop_not_on_the_full_frame() {
        // The target is in the box; only the box is slack. Re-scanning the whole
        // screen re-derives the same answer from a harder image, and rendering
        // plus uploading the full frame pushed one real case past its timeout.
        // The re-locate must reuse the crop the verifier just looked at.
        let hint = PxBox {
            x1: 400.0,
            y1: 300.0,
            x2: 500.0,
            y2: 400.0,
        };
        // Centre on the target, box slack around it — again a size problem.
        let loose = VerifyVerdict {
            size_ok: MatchVerdict::No,
            ..verdict("B1 有数字9的格子")
        };
        let vision = FakeVision::new()
            .with_verify(vec![loose, yes("B1 有数字9的格子")])
            .with_locate(vec![Some(NormBox::new(100.0, 100.0, 300.0, 300.0))]);
        let out = locate_one(
            &vision,
            &frame(),
            &LocateRequest::new("B1 有数字9的格子", "选中这一格").with_hint_px(hint),
            &cfg(),
            &cancel_token(),
        )
        .await
        .unwrap();

        assert!(out.ok, "{out:?}");
        assert_eq!(out.rounds, 2);
        // Every image the locator saw is the 64x64 crop, never a full-frame
        // render — a full-screen re-locate would show up as a larger image.
        for call in vision.calls() {
            if let Call::Locate { width, height } = call {
                assert_eq!(
                    (width, height),
                    (64, 64),
                    "locator ran on the full frame instead of the crop"
                );
            }
        }
    }

    #[tokio::test]
    async fn unverified_target_fails_without_coordinates() {
        let hint = PxBox {
            x1: 400.0,
            y1: 300.0,
            x2: 500.0,
            y2: 400.0,
        };
        let box_ = NormBox::new(400.0, 300.0, 500.0, 400.0);
        let vision = FakeVision::new()
            .with_verify(vec![
                no_left("空白"),
                no_left("空白"),
                no_left("空白"),
            ])
            .with_locate(vec![Some(box_), Some(box_), None]);
        let out = locate_one(
            &vision,
            &frame(),
            &LocateRequest::new("地址栏", "点击它").with_hint_px(hint),
            &cfg(),
            &cancel_token(),
        )
        .await
        .unwrap();

        assert!(!out.ok);
        assert!(out.point_px.is_none());
        assert!(out.box_px.is_none());
        assert_eq!(out.reason, Some(FailureReason::NotFound));
        assert_eq!(out.rounds, 3);
        assert_eq!(out.what_i_saw, "空白");
    }

    #[tokio::test]
    async fn batch_isolates_failures() {
        let vision = FakeVision::new()
            .with_locate(vec![
                None,
                Some(NormBox::new(400.0, 300.0, 500.0, 400.0)),
            ])
            .with_verify(vec![yes("保存按钮")]);
        let out = locate_elements(
            &vision,
            &frame(),
            &[LocateRequest::new("地址栏", "点击它"), LocateRequest::new("保存按钮", "点击它")],
            &cfg(),
            &cancel_token(),
        )
        .await
        .unwrap();

        assert_eq!(out.len(), 2);
        assert!(!out[0].ok);
        assert_eq!(out[0].reason, Some(FailureReason::NotFound));
        assert_eq!(out[0].target_desc, "地址栏");
        assert!(out[1].ok);
        assert_eq!(out[1].target_desc, "保存按钮");
    }

    #[tokio::test]
    async fn box_too_tight_asks_the_locator_again_instead_of_growing_in_code() {
        // A box holding one letter of a label is not fixed by scaling it 1.5x —
        // that covers one size of miss, and this miss has no fixed size. The
        // verifier's observation goes back to the locator, which knows the label
        // from `target_desc` and can return a box covering all of it.
        let vision = FakeVision::new()
            .with_locate(vec![
                Some(NormBox::new(696.0, 596.0, 708.0, 615.0)), // "A" only
                Some(NormBox::new(467.0, 596.0, 542.0, 612.0)), // full label
            ])
            .with_verify(vec![
                VerifyVerdict {
                    size_ok: MatchVerdict::No,
                    ..verdict("蓝框内只有首字母 A")
                },
                yes("完整的 About DagsHub 链接"),
            ]);
        let out = locate_one(
            &vision,
            &frame(),
            &LocateRequest::new("About DagsHub 链接", "点击它"),
            &cfg(),
            &cancel_token(),
        )
        .await
        .unwrap();

        assert!(out.ok, "the re-locate result should verify: {out:?}");
        // Two locate calls: the hinted box, then the re-locate after the
        // verifier said the box was too tight. A code-side grow would never
        // reach the vision model, so it would show up as one locate call.
        let locates = vision
            .calls()
            .iter()
            .filter(|c| matches!(c, Call::Locate { .. }))
            .count();
        assert_eq!(locates, 2, "expected a re-locate, got {locates}");
    }

    /// Only the log line is cut; the flag tells a reader whether they saw all of
    /// it, so a diagnosis is never built on a silently abbreviated description.
    #[test]
    fn short_desc_flags_truncation() {
        assert_eq!(short_desc("保存按钮"), ("保存按钮".to_string(), false));
        // Exactly at the cap is complete — the flag means "there is more".
        let exact = "字".repeat(LOG_DESC_MAX);
        assert_eq!(short_desc(&exact), (exact.clone(), false));
        let over = format!("{exact}多");
        let (text, cut) = short_desc(&over);
        assert!(cut);
        assert_eq!(text.chars().count(), LOG_DESC_MAX);
    }

    /// The trace is the artifact a failure is diagnosed from months later, so
    /// what it must contain is worth asserting: the caller's whole hint box (not
    /// just its centre), and the exact prompt each call was given.
    #[tokio::test]
    async fn trace_keeps_the_hint_box_and_the_full_prompts() {
        let hint = PxBox {
            x1: 800.0,
            y1: 35.0,
            x2: 840.0,
            y2: 55.0,
        };
        let vision = FakeVision::new().with_verify(vec![yes("蓝色软盘图标")]);
        let (out, trace) = locate_one_traced(
            &vision,
            &frame(),
            &LocateRequest::new("保存按钮", "点击它").with_hint_px(hint),
            &cfg(),
            &cancel_token(),
        )
        .await
        .unwrap();

        assert!(out.ok);
        // `hint` stays as the centre for old readers; `hint_box` is what the
        // first verify round actually verified, so a fixture can replay it.
        assert_eq!(trace.request.hint, Some([820.0, 45.0]));
        assert_eq!(trace.request.hint_box, Some([800.0, 35.0, 840.0, 55.0]));

        let verify = trace.rounds[0].verify.as_ref().expect("verify step");
        let prompt = verify.prompt.as_deref().expect("verify prompt recorded");
        assert!(prompt.contains("保存按钮"), "prompt lost the description");
        // The hint box is what round 1 verified; it goes through the 0–1000
        // normalisation, so compare with a tolerance rather than bit-exact.
        for (got, want) in verify.box_px.iter().zip([800.0, 35.0, 840.0, 55.0]) {
            assert!((got - want).abs() < 1e-6, "box {got} != {want}");
        }

        // The frame is 1000x800 = 0.8 MP, under `FULL_MAX_PX`, so the dumped JPEG
        // is the frame and a hand-measured box needs no rescaling.
        assert_eq!(trace.frame.dumped, Some([1000, 800]));
        assert!(trace.frame.dump_is_identity());
    }

    #[tokio::test]
    async fn trace_keeps_the_locate_prompt_of_every_call() {
        let vision = FakeVision::new()
            .with_locate(vec![Some(NormBox::new(400.0, 300.0, 500.0, 400.0))])
            .with_verify(vec![yes("蓝色软盘图标")]);
        let (_, trace) = locate_one_traced(
            &vision,
            &frame(),
            &LocateRequest::new("保存按钮", "点击它"),
            &cfg(),
            &cancel_token(),
        )
        .await
        .unwrap();

        let locate = trace.rounds[0].locate.as_ref().expect("locate step");
        assert_eq!(locate.where_, "full_init");
        let prompt = locate.prompt.as_deref().expect("locate prompt recorded");
        assert!(prompt.contains("保存按钮"), "prompt lost the description");
    }

    /// A description long enough that the **log line** cuts it: the trace must
    /// still hold every character, otherwise the artifact a failure is diagnosed
    /// from loses the part that distinguishes two look-alike targets.
    #[tokio::test]
    async fn trace_keeps_a_description_the_log_line_would_cut() {
        // The words do not matter, only that the description is longer than the
        // log line's cap (4 × ~41 chars).
        let long = "「财务分析」标签页右侧的导出按钮，位于下拉箭头左边，仅在报表有数据时出现。".repeat(4);
        assert!(
            long.chars().count() > LOG_DESC_MAX,
            "this test is pointless unless the description exceeds the log cap"
        );
        let vision = FakeVision::new()
            .with_locate(vec![Some(NormBox::new(400.0, 300.0, 500.0, 400.0))])
            .with_verify(vec![yes("导出按钮")]);
        let (_, trace) = locate_one_traced(
            &vision,
            &frame(),
            &LocateRequest::new(long.as_str(), "点击它"),
            &cfg(),
            &cancel_token(),
        )
        .await
        .unwrap();

        assert_eq!(trace.request.target_desc, long);
        let prompt = trace.rounds[0]
            .locate
            .as_ref()
            .unwrap()
            .prompt
            .as_deref()
            .unwrap();
        assert!(prompt.contains(&long), "the trace must keep the whole prompt");
    }
}
