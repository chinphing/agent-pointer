//! The B5 loop: locate → crop → verify → refine (design §6.1).
//!
//! Ported from `b5_decoupled.py::run_one`. The controller is pure Rust and
//! deterministic; all model judgement sits behind [`LocateVision`], which makes
//! every branch unit-testable without a network or a real screen.

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use super::geometry::{crop_side_px, nearly_same, rel_to_norm, CropBounds, NormBox};
use super::prompt::{locate_prompt, refine_hint, verify_prompt};
use super::render::{render_crop, render_full, LocateFrame, LocateImage};
use super::types::{
    Confidence, FailureReason, LocateConfig, LocateOutcome, LocateRequest, MatchVerdict,
    UnclearKind, VerifyVerdict,
};

/// The two model calls the controller is allowed to make.
#[async_trait]
pub trait LocateVision: Send + Sync {
    /// Return the target box in **image-local 0–1000** coordinates.
    async fn locate(&self, image: &LocateImage, prompt: &str) -> Result<Option<NormBox>>;
    /// Judge the candidate box shown on the crop.
    async fn verify(&self, image: &LocateImage, prompt: &str) -> Result<VerifyVerdict>;
}

/// Why the loop stopped. Mirrors B5 `stop_reason`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stop {
    Verified,
    LocateFailed,
    TooBlurry,
    RoundsExhausted,
}

impl Stop {
    fn failure_reason(self) -> FailureReason {
        match self {
            Self::Verified => FailureReason::NotFound, // unreachable; kept total
            Self::LocateFailed => FailureReason::NotFound,
            Self::TooBlurry => FailureReason::Unclear,
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
    let verify_text = verify_prompt(&req.description, &req.goal);

    let mut box_norm = req.hint_norm(w, h);
    let mut last_saw = String::new();
    let mut rounds = 0usize;
    let mut stop = Stop::RoundsExhausted;

    // ① First locate — skipped entirely when the caller supplied a hint box.
    if box_norm.is_none() {
        let image = render_full(frame, None)?;
        box_norm = locate_in(vision, &image, full, w, h, &req.description, &req.goal, None).await;
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

        let mut side = crop_side_px(
            current.to_px(w, h),
            cfg.crop_mult,
            cfg.crop_min,
            cfg.crop_max,
        );
        let mut verify_retries = 0usize;

        // Inner loop: one crop, retried with a different `side` while the
        // verifier reports `unclear` for a fixable reason.
        let (verdict, last_crop) = loop {
            let (image, bounds) = render_crop(frame, current, side, cfg.out_px)?;
            let v = match vision.verify(&image, &verify_text).await {
                Ok(v) => v,
                Err(e) => {
                    log::warn!("locate_element: verify call failed: {e}");
                    VerifyVerdict::default()
                }
            };
            if !v.in_box.trim().is_empty() {
                last_saw = v.in_box.clone();
            }
            let crop = Some((image, bounds));

            match v.match_ {
                MatchVerdict::Yes => {
                    stop = Stop::Verified;
                    break (v, crop);
                }
                MatchVerdict::Unclear
                    if v.unclear_kind == UnclearKind::AnchorOutOfView
                        && verify_retries < cfg.max_verify_retry
                        && side < cfg.crop_max * 2.0 =>
                {
                    side = (side * 2.0).min(w.max(h) as f64 * 1.5);
                    verify_retries += 1;
                    continue;
                }
                MatchVerdict::Unclear
                    if v.unclear_kind == UnclearKind::TooSmall
                        && verify_retries < cfg.max_verify_retry
                        && side > cfg.crop_min / 2.0 =>
                {
                    side = (side / 2.0).max(120.0);
                    verify_retries += 1;
                    continue;
                }
                MatchVerdict::Unclear if v.unclear_kind == UnclearKind::TooBlurry => {
                    stop = Stop::TooBlurry;
                    break (v, crop);
                }
                // `ambiguous` or retries exhausted → treated as `no`.
                _ => break (v, crop),
            }
        };

        if matches!(stop, Stop::Verified | Stop::TooBlurry) {
            break;
        }

        // —— Judged `no`: decide where to look next ——
        let hint = refine_hint(&verdict);

        let mut from_crop = false;
        let mut next_box = None;
        if verdict.elsewhere_dir.is_some() {
            if let Some((image, bounds)) = last_crop.as_ref() {
                from_crop = true;
                next_box = locate_in(
                    vision,
                    image,
                    *bounds,
                    w,
                    h,
                    &req.description,
                    &req.goal,
                    Some(&hint),
                )
                .await;
            }
        }
        if !from_crop {
            let image = render_full(frame, Some(current))?;
            next_box = locate_in(vision, &image, full, w, h, &req.description, &req.goal, Some(&hint)).await;
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
        if from_crop && nearly_same(next_box, current) {
            let image = render_full(frame, Some(current))?;
            next_box = match locate_in(vision, &image, full, w, h, &req.description, &req.goal, Some(&hint)).await
            {
                Some(b) => b,
                None => {
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
        return Ok(LocateOutcome::failure(
            req.description.clone(),
            stop.failure_reason(),
            what_i_saw,
            rounds,
        ));
    }

    let Some(b) = box_norm else {
        return Ok(LocateOutcome::failure(
            req.description.clone(),
            FailureReason::NotFound,
            what_i_saw,
            rounds,
        ));
    };
    let (cx, cy) = b.center();
    let point = (cx / 1000.0 * w as f64, cy / 1000.0 * h as f64);
    Ok(LocateOutcome {
        description: req.description.clone(),
        ok: true,
        point_px: Some(point),
        box_px: Some(b.to_px(w, h)),
        confidence: Some(Confidence::High),
        what_i_saw,
        rounds,
        reason: None,
    })
}

/// One locate call, mapped from image-local 0–1000 back to screen 0–1000.
async fn locate_in<V: LocateVision>(
    vision: &V,
    image: &LocateImage,
    bounds: CropBounds,
    w: u32,
    h: u32,
    description: &str,
    goal: &str,
    previous_hint: Option<&str>,
) -> Option<NormBox> {
    let prompt = locate_prompt(description, goal, previous_hint);
    match vision.locate(image, &prompt).await {
        Ok(Some(raw)) => Some(rel_to_norm(raw, bounds, w, h)),
        Ok(None) => None,
        Err(e) => {
            log::warn!("locate_element: locate call failed: {e}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::computer::locate::geometry::{NormBox, PxBox};
    use crate::agents::computer::locate::types::{ElsewhereDir, MatchVerdict};
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
        async fn locate(&self, image: &LocateImage, _prompt: &str) -> Result<Option<NormBox>> {
            self.calls.lock().unwrap().push(Call::Locate {
                width: image.width,
                height: image.height,
            });
            Ok(self.locate_script.lock().unwrap().pop_front().flatten())
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

    fn verdict(match_: MatchVerdict, in_box: &str) -> VerifyVerdict {
        VerifyVerdict {
            match_,
            in_box: in_box.to_string(),
            anchor_seen: "看不到".into(),
            why: "test".into(),
            ..VerifyVerdict::default()
        }
    }

    fn yes(in_box: &str) -> VerifyVerdict {
        verdict(MatchVerdict::Yes, in_box)
    }

    fn no_left(in_box: &str) -> VerifyVerdict {
        VerifyVerdict {
            elsewhere_dir: Some(ElsewhereDir::Left),
            ..verdict(MatchVerdict::No, in_box)
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
    async fn direction_verdict_relocates_inside_the_crop() {
        let hint = PxBox {
            x1: 400.0,
            y1: 300.0,
            x2: 500.0,
            y2: 400.0,
        };
        let vision = FakeVision::new()
            .with_verify(vec![no_left("取消按钮"), yes("保存按钮")])
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

        assert!(out.ok);
        assert_eq!(out.rounds, 2);
        assert_eq!(
            vision.calls(),
            vec![
                Call::Verify {
                    width: 64,
                    height: 64
                },
                Call::Locate {
                    width: 64,
                    height: 64
                },
                Call::Verify {
                    width: 64,
                    height: 64
                }
            ]
        );
    }

    #[tokio::test]
    async fn no_progress_in_crop_escalates_to_full_screen() {
        let hint = PxBox {
            x1: 400.0,
            y1: 300.0,
            x2: 500.0,
            y2: 400.0,
        };
        // Crop-local 0–1000 that maps back onto `hint` exactly: the engine's
        // crop is side 300 centred on (450, 350), i.e. bounds (300,200)-(600,500).
        let same = NormBox::new(333.333, 333.333, 666.667, 666.667);
        let vision = FakeVision::new()
            .with_verify(vec![no_left("取消按钮"), yes("保存按钮")])
            .with_locate(vec![
                Some(same),
                Some(NormBox::new(700.0, 100.0, 800.0, 200.0)),
            ]);
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
        assert_eq!(
            vision.calls(),
            vec![
                Call::Verify {
                    width: 64,
                    height: 64
                },
                Call::Locate {
                    width: 64,
                    height: 64
                },
                // Escalated because the crop-local box did not move.
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
                verdict(MatchVerdict::No, "空白"),
                verdict(MatchVerdict::No, "空白"),
                verdict(MatchVerdict::No, "空白"),
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
    async fn blurry_crop_stops_early_with_unclear() {
        let vision = FakeVision::new().with_verify(vec![VerifyVerdict {
            unclear_kind: UnclearKind::TooBlurry,
            ..verdict(MatchVerdict::Unclear, "一片模糊")
        }]);
        let hint = PxBox {
            x1: 400.0,
            y1: 300.0,
            x2: 500.0,
            y2: 400.0,
        };
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
        assert_eq!(out.reason, Some(FailureReason::Unclear));
        assert_eq!(out.rounds, 1);
        assert!(out.point_px.is_none());
    }

    #[tokio::test]
    async fn too_small_zooms_in_then_gives_up() {
        let hint = PxBox {
            x1: 400.0,
            y1: 300.0,
            x2: 500.0,
            y2: 400.0,
        };
        let vision = FakeVision::new().with_verify(vec![
            VerifyVerdict {
                unclear_kind: UnclearKind::TooSmall,
                ..verdict(MatchVerdict::Unclear, "看不清")
            },
            yes("保存按钮"),
        ]);
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
        assert_eq!(out.rounds, 1, "zoom retries stay inside the same round");
        assert_eq!(
            vision.calls(),
            vec![
                Call::Verify {
                    width: 64,
                    height: 64
                },
                Call::Verify {
                    width: 64,
                    height: 64
                }
            ]
        );
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
        assert_eq!(out[0].description, "地址栏");
        assert!(out[1].ok);
        assert_eq!(out[1].description, "保存按钮");
    }
}
