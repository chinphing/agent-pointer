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
use super::render::{render_crop, render_full, LocateFrame, LocateImage};
use super::types::{
    CenterOnTarget, Confidence, FailureReason, LocateConfig, LocateOutcome, LocateRequest,
    VerifyVerdict,
};

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
    for req in requests {
        out.push(locate_one(vision, frame, req, cfg, cancel).await?);
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
    let (w, h) = (frame.width().max(1), frame.height().max(1));
    let full = CropBounds::full(w, h);
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
        let image = render_full(frame, None)?;
        box_norm = match locate_in(vision, &image, full, w, h, &req.target_desc, req.hint_center(), None, None).await {
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
        // per round: `retry` is kept only so the log line stays parseable.
        let verify_retries = 0usize;

        // Inner loop: one crop, retried with a different `side` while the
        // verifier reports `unclear` for a fixable reason.
        let (verdict, last_crop) = loop {
            let (image, bounds) = render_crop(frame, current, side, cfg.out_px)?;
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
                let img = render_crop(frame, current, side * cfg.verify_mult, wide_px)?.0;
                // `POINTER_LOCATE_DUMP_WIDE=/tmp/wide` writes each verifier crop to
                // disk. A verdict that says 「no row labels」 is otherwise
                // unfalsifiable — you cannot tell whether the labels were outside
                // the crop or the model simply did not look.
                if let Ok(prefix) = std::env::var("POINTER_LOCATE_DUMP_WIDE") {
                    if !prefix.is_empty() {
                        let _ = std::fs::write(
                            format!("{prefix}_{}_r{}.jpg", req.target_desc.chars().take(12).collect::<String>().replace(['/', ' '], "_"), rounds),
                            &img.jpeg,
                        );
                    }
                }
                Some(img)
            } else {
                None
            };
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
            // Every verdict, not just failures: without this line a locate run
            // is a black box — you cannot tell whether the box was too tight,
            // the crop too small, or the target genuinely absent.
            log::info!(
                "locate: round={rounds} retry={verify_retries} side={side:.0} box=[{:.0},{:.0},{:.0},{:.0}] center={:?} size={:?} position={} notes={}",
                current.to_px(w, h).x1,
                current.to_px(w, h).y1,
                current.to_px(w, h).x2,
                current.to_px(w, h).y2,
                v.center_on_target,
                v.size_ok,
                v.position_matches.as_str(),
                v.notes.trim()
            );
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
                next_box = match locate_in(
                    vision,
                    image,
                    *bounds,
                    w,
                    h,
                    &req.target_desc,
                    req.hint_center(),
                    Some(&hint),
                    Some(current.to_px(w, h)),
                )
                .await
                {
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
            next_box = match locate_in(vision, &image, full, w, h, &req.target_desc, req.hint_center(), Some(&hint), Some(current.to_px(w, h))).await {
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
            next_box = match locate_in(vision, &image, full, w, h, &req.target_desc, req.hint_center(), Some(&hint), Some(current.to_px(w, h))).await
            {
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

    if stop != Stop::Verified {
        let mut out = LocateOutcome::failure(
            req.target_desc.clone(),
            stop.failure_reason(),
            what_i_saw,
            rounds,
        );
        out.why = last_why;
        return Ok(out);
    }

    let Some(b) = box_norm else {
        return Ok(LocateOutcome::failure(
            req.target_desc.clone(),
            FailureReason::NotFound,
            what_i_saw,
            rounds,
        ));
    };
    let (cx, cy) = b.center();
    let point = (cx / 1000.0 * w as f64, cy / 1000.0 * h as f64);
    Ok(LocateOutcome {
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
    })
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
    image: &LocateImage,
    bounds: CropBounds,
    w: u32,
    h: u32,
    description: &str,
    hint_px: Option<(f64, f64)>,
    previous_hint: Option<&str>,
    prev_box: Option<PxBox>,
) -> LocateAttempt {
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
    match vision.locate(image, &prompt).await {
        Ok(Some(LocateAnswer::Found(raw))) => {
            // The prompt asks for 0-1000 coordinates relative to this image. Log
            // the raw answer next to the mapped one: a model that reasons in
            // pixels and answers in pixels looks identical to a correct one
            // until the two are compared.
            log::info!(
                "locate: raw=[{:.0},{:.0},{:.0},{:.0}] bounds=[{:.0},{:.0},{:.0},{:.0}] image={}x{}",
                raw.x1,
                raw.y1,
                raw.x2,
                raw.y2,
                bounds.x0,
                bounds.y0,
                bounds.x1,
                bounds.y1,
                image.width,
                image.height
            );
            LocateAttempt::Found(rel_to_norm(raw, bounds, w, h))
        }
        Ok(Some(LocateAnswer::NotFound)) => {
            // Distinct from a parse failure: the locator looked and said the
            // target is not in this image. The caller can widen the view; a
            // bare `None` would read the same as 「the model broke」.
            log::info!("locate: not found in this image");
            LocateAttempt::NotFound
        }
        Ok(None) => LocateAttempt::Failed,
        Err(e) => {
            log::warn!("locate_element: locate call failed: {e}");
            LocateAttempt::Failed
        }
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
}
