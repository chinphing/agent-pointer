//! The contract between a collected [`LocateTrace`] and the fixture runner.
//!
//! A trace with `truth_bbox` filled in is a regression case, but the collector
//! (`examples/locate_trace_to_truth.rs`) and the consumer
//! (`tests/locate_fixture`) have to agree on the file's shape down to the field
//! names and the coordinate space. One definition lives here: the collector
//! serialises it, the runner deserialises it, and a change to either side breaks
//! the same struct instead of two copies drifting apart silently.
//!
//! # Coordinate space
//!
//! Every box in a truth file is in **fixture pixels** — the pixels of the image
//! the runner loads. For the frozen datasets that is the original screenshot; for
//! an incident it is the dumped `frame.jpg`, which [`super::render`] may have
//! downscaled. [`case_from_trace`] does the conversion, so a truth file never
//! contains a box in a space nobody can measure.

use anyhow::{anyhow, ensure, Result};
use serde::{Deserialize, Serialize};

use super::trace::LocateTrace;

/// One dataset: a fixture (or one fixture per case) plus its cases.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TruthFile {
    /// Fixture shared by every case, relative to `tests/locate_fixture/`.
    /// Empty when each case names its own — an incidents dataset, where every
    /// case is a different screenshot.
    #[serde(default)]
    pub fixture: String,
    /// Expected `[width, height]` of the shared fixture, in its own pixels.
    /// A per-case fixture carries its own size instead.
    #[serde(default)]
    pub size: Option<[u32; 2]>,
    #[serde(default)]
    pub cases: Vec<TruthCase>,
}

impl TruthFile {
    /// `true` when every case points at the same fixture (the frozen datasets).
    pub fn has_shared_fixture(&self) -> bool {
        !self.fixture.trim().is_empty()
    }

    /// Fixture name for one case: the case's own, else the file's shared one.
    ///
    /// An empty answer is an error rather than a path that resolves to the
    /// directory itself — `dir().join("")` exists, so the mistake would surface
    /// later as a decode failure on a directory.
    pub fn case_fixture_name<'a>(&'a self, case: &'a TruthCase) -> Result<&'a str> {
        let name = case.fixture.as_deref().unwrap_or(self.fixture.as_str());
        ensure!(
            !name.trim().is_empty(),
            "case {:?} has no fixture and the file declares none",
            case.description
        );
        Ok(name)
    }

    /// Expected `[w, h]` for that fixture.
    ///
    /// The file-level size describes the file-level fixture only: a case that
    /// brings its own screenshot must not be checked against another image's
    /// dimensions, which is exactly how a downscaled JPEG would slip through.
    pub fn case_size(&self, case: &TruthCase) -> Option<[u32; 2]> {
        if case.fixture.is_some() {
            case.size
        } else {
            case.size.or(self.size)
        }
    }
}

/// One case: a description, the right answer, and the hint production had.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TruthCase {
    /// Short label, printed next to the result. Free-form.
    #[serde(default)]
    pub scene: String,
    pub description: String,
    /// What the caller intends to do with the element. Mirrors production, where
    /// `resolve_point` passes the model's own goal and the grounder uses it to
    /// choose between look-alike targets.
    #[serde(default)]
    pub goal: Option<String>,
    /// `[x1, y1, x2, y2]` in fixture pixels; `null` = not measured yet.
    #[serde(default)]
    pub bbox: Option<[f64; 4]>,
    /// `plain` (default), `partial_hint`, `hint_disambig` — which half of the
    /// hint behaviour a test is meant to exercise.
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub hint_box: Option<[f64; 4]>,
    /// Fixture for this case, relative to `tests/locate_fixture/`. Overrides
    /// [`TruthFile::fixture`].
    #[serde(default)]
    pub fixture: Option<String>,
    /// `[width, height]` of this case's fixture, in its own pixels.
    #[serde(default)]
    pub size: Option<[u32; 2]>,
    #[serde(default)]
    pub note: String,
}

/// The scene label every converted incident carries.
pub const INCIDENT_SCENE: &str = "incident";

/// Turn one labelled trace into the case it becomes.
///
/// `fixture` is the case's fixture path relative to `tests/locate_fixture/`, and
/// `jpeg` is that JPEG's **real** pixel size — read from the file by the caller,
/// not taken on trust from the trace, because the whole point of the conversion
/// is to put every box in the space of the image a human can open.
///
/// Errors instead of guessing:
///
/// · no `truth_bbox` — the trace was never labelled, so it is not a case (and
///   writing `null` would make the runner score it as unscored, silently);
/// · a box outside the JPEG — it was measured in the wrong coordinate space, the
///   one mistake that produces a fixture where every case fails for a reason
///   that looks like a model regression.
pub fn case_from_trace(trace: &LocateTrace, fixture: &str, jpeg: [u32; 2]) -> Result<TruthCase> {
    let truth = trace.truth_bbox.ok_or_else(|| {
        anyhow!(
            "{}: truth_bbox is empty — label the run first",
            trace.run_id
        )
    })?;
    let (jw, jh) = (jpeg[0].max(1), jpeg[1].max(1));
    if let Some(dumped) = trace.frame.dumped {
        ensure!(
            dumped == [jw, jh],
            "{}: frame.dumped={dumped:?} but {} is {jw}x{jh} on disk; \
             the trace and the JPEG disagree about the coordinate space",
            trace.run_id,
            trace.frame.path
        );
    }
    let bbox =
        check_box(truth, jw, jh).map_err(|e| anyhow!("{}: truth_bbox: {e}", trace.run_id))?;

    // `hint_box` is in **frame** pixels (the engine's space); the case needs it
    // in fixture pixels, like every other box in the file.
    //
    // Clamped rather than rejected: a hint is a caller's estimate, and one that
    // pokes past the screen edge is normal (`hint_norm` clamps it the same way).
    // Refusing to convert an otherwise good case over it would throw away the
    // thing the collection exists to capture.
    let sx = jw as f64 / trace.frame.width.max(1) as f64;
    let sy = jh as f64 / trace.frame.height.max(1) as f64;
    let hint_box = trace
        .request
        .hint_box
        .map(|b| -> Result<[f64; 4]> {
            ensure!(
                b[0] < b[2] && b[1] < b[3],
                "hint_box {b:?} is not a box (x1<x2 and y1<y2 required)"
            );
            Ok(clamp_box(
                [b[0] * sx, b[1] * sy, b[2] * sx, b[3] * sy],
                jw,
                jh,
            ))
        })
        .transpose()
        .map_err(|e| anyhow!("{}: {e}", trace.run_id))?;

    let mut note = format!(
        "run {} @ {}; stop={} ok={} rounds={}",
        trace.run_id, trace.started_at, trace.outcome.stop, trace.outcome.ok, trace.outcome.rounds
    );
    if !trace.note.trim().is_empty() {
        note.push_str("; ");
        note.push_str(trace.note.trim());
    }

    Ok(TruthCase {
        scene: INCIDENT_SCENE.to_string(),
        description: trace.request.target_desc.clone(),
        goal: Some(trace.request.goal.clone()).filter(|g| !g.trim().is_empty()),
        bbox: Some(bbox),
        kind: "plain".to_string(),
        hint_box,
        fixture: Some(fixture.to_string()),
        size: Some([jw, jh]),
        note,
    })
}

/// A box must be well-ordered and inside the image it is measured on.
fn check_box(b: [f64; 4], w: u32, h: u32) -> Result<[f64; 4]> {
    let [x1, y1, x2, y2] = b;
    ensure!(
        x1.is_finite() && y1.is_finite() && x2.is_finite() && y2.is_finite(),
        "{b:?} is not four finite numbers"
    );
    ensure!(
        x1 < x2 && y1 < y2,
        "{b:?} is not a box (x1<x2 and y1<y2 required)"
    );
    // Half a pixel of slack: a box drawn on a JPEG boundary can round to `w`.
    ensure!(
        x1 >= 0.0 && y1 >= 0.0 && x2 <= w as f64 + 0.5 && y2 <= h as f64 + 0.5,
        "{b:?} is outside the {w}x{h} image it is measured on"
    );
    Ok(b)
}

/// Pull a box inside the image, keeping its corner order.
fn clamp_box(b: [f64; 4], w: u32, h: u32) -> [f64; 4] {
    let (w, h) = (w as f64, h as f64);
    [
        b[0].clamp(0.0, w),
        b[1].clamp(0.0, h),
        b[2].clamp(0.0, w),
        b[3].clamp(0.0, h),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::computer::locate::trace::{
        FrameTrace, OutcomeTrace, RequestTrace, RoundTrace,
    };

    fn trace(width: u32, height: u32, dumped: Option<[u32; 2]>) -> LocateTrace {
        LocateTrace {
            run_id: "e45e4900".into(),
            started_at: "2026-10-27T10:25:03.561Z".into(),
            session: None,
            batch: None,
            index: 0,
            frame: FrameTrace {
                path: "e45e4900_frame.jpg".into(),
                width,
                height,
                dumped,
            },
            request: RequestTrace {
                target_desc: "数独盘面I8的空格子".into(),
                goal: "选中这一格准备填数".into(),
                hint: Some([540.0, 780.0]),
                hint_box: Some([520.0, 760.0, 560.0, 800.0]),
            },
            rounds: Vec::<RoundTrace>::new(),
            outcome: OutcomeTrace {
                ok: false,
                point: None,
                box_px: None,
                rounds: 2,
                stop: "RoundsExhausted".into(),
            },
            truth_bbox: Some([346.0, 567.0, 387.0, 608.0]),
            note: "偏右一格，I7 认成 I8".into(),
        }
    }

    #[test]
    fn a_full_size_frame_needs_no_rescaling() {
        let t = trace(1920, 1080, Some([1920, 1080]));
        let case = case_from_trace(&t, "fixtures/incidents/e45e4900.jpg", [1920, 1080]).unwrap();

        assert_eq!(case.bbox, Some([346.0, 567.0, 387.0, 608.0]));
        assert_eq!(case.hint_box, Some([520.0, 760.0, 560.0, 800.0]));
        assert_eq!(case.size, Some([1920, 1080]));
        assert_eq!(
            case.fixture.as_deref(),
            Some("fixtures/incidents/e45e4900.jpg")
        );
        assert_eq!(case.scene, INCIDENT_SCENE);
        assert_eq!(case.kind, "plain");
        assert_eq!(case.description, "数独盘面I8的空格子");
        assert_eq!(case.goal.as_deref(), Some("选中这一格准备填数"));
        // Provenance travels with the case: which run, how it ended, and what the
        // person labelling it thought was wrong.
        assert!(case.note.contains("run e45e4900"), "{}", case.note);
        assert!(case.note.contains("RoundsExhausted"), "{}", case.note);
        assert!(case.note.contains("I7 认成 I8"), "{}", case.note);
    }

    /// A 2560×1440 screen is dumped as a smaller JPEG (`FULL_MAX_PX`). The truth
    /// box was measured on that JPEG, so it needs no rescale; the hint box came
    /// from the engine in frame pixels, so it does.
    #[test]
    fn a_downscaled_jpeg_moves_the_hint_into_fixture_pixels() {
        let t = trace(2560, 1440, Some([2108, 1186]));
        let case = case_from_trace(&t, "fixtures/incidents/e45e4900.jpg", [2108, 1186]).unwrap();

        assert_eq!(case.bbox, Some([346.0, 567.0, 387.0, 608.0]));
        assert_eq!(case.size, Some([2108, 1186]));
        let hint = case.hint_box.unwrap();
        let sx = 2108.0 / 2560.0;
        let sy = 1186.0 / 1440.0;
        assert!((hint[0] - 520.0 * sx).abs() < 1e-9);
        assert!((hint[3] - 800.0 * sy).abs() < 1e-9);
        // The hint stays the deliberately-off box it was: in this run it sits to
        // the right of the truth, and rescaling must not move it onto the target.
        assert!(
            hint[0] > case.bbox.unwrap()[2],
            "{hint:?} vs {:?}",
            case.bbox
        );
    }

    #[test]
    fn an_unlabelled_trace_is_refused() {
        let mut t = trace(1920, 1080, Some([1920, 1080]));
        t.truth_bbox = None;
        let err = case_from_trace(&t, "fixtures/incidents/e45e4900.jpg", [1920, 1080])
            .unwrap_err()
            .to_string();
        assert!(err.contains("truth_bbox is empty"), "{err}");
        assert!(err.contains("e45e4900"), "{err}");
    }

    /// The failure this check exists for: someone measures the box on the JPEG
    /// but types the numbers in frame coordinates (or the other way round). The
    /// box lands outside the image, and a silent fixture would fail forever.
    #[test]
    fn a_box_outside_the_jpeg_is_refused() {
        let mut t = trace(1920, 1080, Some([1920, 1080]));
        t.truth_bbox = Some([2500.0, 100.0, 2540.0, 140.0]);
        let err = case_from_trace(&t, "fixtures/incidents/e45e4900.jpg", [1920, 1080])
            .unwrap_err()
            .to_string();
        assert!(err.contains("outside the 1920x1080 image"), "{err}");
    }

    #[test]
    fn a_trace_and_its_jpeg_must_agree_on_the_size() {
        let t = trace(1920, 1080, Some([1920, 1080]));
        let err = case_from_trace(&t, "fixtures/incidents/e45e4900.jpg", [960, 540])
            .unwrap_err()
            .to_string();
        assert!(err.contains("disagree about the coordinate space"), "{err}");
    }

    /// A hint that pokes past the screen edge is a normal caller estimate. It is
    /// clamped into the fixture, not turned into a failed conversion.
    #[test]
    fn a_hint_off_the_screen_edge_is_clamped_not_refused() {
        let mut t = trace(1920, 1080, Some([1920, 1080]));
        t.request.hint_box = Some([1900.0, 1000.0, 2100.0, 1200.0]);
        let case = case_from_trace(&t, "fixtures/incidents/e45e4900.jpg", [1920, 1080]).unwrap();
        assert_eq!(case.hint_box, Some([1900.0, 1000.0, 1920.0, 1080.0]));
        // The truth box is still the answer and stays untouched.
        assert_eq!(case.bbox, Some([346.0, 567.0, 387.0, 608.0]));
    }

    /// What the converter writes is what the runner reads: same struct, so a
    /// round trip is the whole contract.
    #[test]
    fn a_written_dataset_round_trips_through_the_runner_shape() {
        let case = case_from_trace(
            &trace(1920, 1080, Some([1920, 1080])),
            "fixtures/incidents/e45e4900.jpg",
            [1920, 1080],
        )
        .unwrap();
        let file = TruthFile {
            fixture: String::new(),
            size: None,
            cases: vec![case],
        };
        let json = serde_json::to_string_pretty(&file).unwrap();
        let back: TruthFile = serde_json::from_str(&json).unwrap();
        assert_eq!(back, file);
        // The dataset names no shared fixture, so every case must carry its own.
        assert!(!back.has_shared_fixture());
        assert_eq!(
            back.case_fixture_name(&back.cases[0]).unwrap(),
            "fixtures/incidents/e45e4900.jpg"
        );
        assert_eq!(back.case_size(&back.cases[0]), Some([1920, 1080]));
    }

    /// The frozen datasets name one fixture and may omit the per-case fields;
    /// that shape has to keep working unchanged.
    #[test]
    fn a_frozen_dataset_still_resolves_its_shared_fixture() {
        let file: TruthFile = serde_json::from_str(
            r#"{"fixture": "fixtures/truth_desktop.png", "size": [1512, 982],
                "cases": [{"description": "Apple 菜单图标", "bbox": [17, 10, 39, 27]}]}"#,
        )
        .unwrap();
        assert!(file.has_shared_fixture());
        let case = &file.cases[0];
        assert_eq!(
            file.case_fixture_name(case).unwrap(),
            "fixtures/truth_desktop.png"
        );
        assert_eq!(file.case_size(case), Some([1512, 982]));
    }

    #[test]
    fn a_case_without_any_fixture_is_an_error_not_an_empty_path() {
        let file = TruthFile {
            fixture: String::new(),
            size: None,
            cases: vec![TruthCase {
                scene: String::new(),
                description: "无图可依".into(),
                goal: None,
                bbox: None,
                kind: String::new(),
                hint_box: None,
                fixture: None,
                size: None,
                note: String::new(),
            }],
        };
        let err = file
            .case_fixture_name(&file.cases[0])
            .unwrap_err()
            .to_string();
        assert!(err.contains("no fixture"), "{err}");
    }
}
