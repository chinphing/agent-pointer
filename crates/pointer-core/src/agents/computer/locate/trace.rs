//! A machine-readable record of one `locate_one` run, written to
//! `POINTER_LOCATE_DEBUG_DIR` when debugging is on.
//!
//! # Why a file and not just log lines
//!
//! Log lines answer 「what happened」 for one run while you are reading them.
//! They do not accumulate into a dataset: a run that went wrong on the user's
//! machine is gone once the log rotates, and re-reading 400 lines to work out
//! which `round=2` belongs to which crop is exactly the problem this solves.
//!
//! Every run writes one `<run_id>.json` next to the JPEGs it references, with
//! relative paths so the whole directory can be moved or zipped and still
//! resolve. `truth_bbox` and `note` are left empty on purpose — they are where a
//! human fills in the right answer afterwards, which turns a production failure
//! into a regression fixture without re-typing the description or the hint.
//!
//! # Shape
//!
//! ```json
//! {
//!   "run_id": "e45e4900",
//!   "started_at": "2026-10-27T10:25:03.561Z",
//!   "frame": { "path": "e45e4900_frame.jpg", "width": 2560, "height": 1440,
//!              "dumped": [2108, 1186] },
//!   "request": { "target_desc": "...", "goal": "...",
//!                "hint": [384, 578], "hint_box": [370, 560, 398, 596] },
//!   "rounds": [
//!     {
//!       "round": 1,
//!       "locate": { "where": "crop_r1", "image": "...jpg", "bounds": [...],
//!                   "prev_box": [...], "raw": [...], "box": [...],
//!                   "prompt": "…full prompt text…" },
//!       "verify": { "close_image": "...", "wide_image": "...",
//!                   "center": "No", "size": "No", "position": "yes",
//!                   "notes": "...", "render_ms": 186, "call_ms": 6283,
//!                   "prompt": "…full prompt text…" }
//!     }
//!   ],
//!   "outcome": { "ok": true, "point": [408, 587], "box": [...], "rounds": 2 },
//!   "truth_bbox": null,
//!   "note": ""
//! }
//! ```
//!
//! # Two coordinate spaces, and which one `truth_bbox` lives in
//!
//! Everything the engine writes (`bounds`, `prev_box`, `box`, `outcome.box`,
//! `request.hint_box`) is in **frame pixels** — the space of `frame.width` ×
//! `frame.height`. That is the screen the engine reasoned about.
//!
//! `frame.jpg` is not necessarily that size: `render_full` downscales a frame
//! above `FULL_MAX_PX` (2.5 MP), so a 2560×1440 screen is dumped as 2108×1186 (scale 0.823).
//! `frame.dumped` records the JPEG's real pixel size, which is what a person
//! sees when they open the file.
//!
//! `truth_bbox` is in **dumped JPEG pixels**, i.e. what you measure on
//! `frame.jpg` with an image viewer. Labelling has to be free of arithmetic:
//! the only image a human can open is the JPEG, so that is the space the answer
//! is written in. `locate_trace_to_truth` converts it (and `request.hint_box`)
//! into the fixture's own space when it builds a truth file.

use serde::{Deserialize, Serialize};

/// One locate run. Serialised to `<run_id>.json` in the debug directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocateTrace {
    pub run_id: String,
    /// UTC, RFC 3339. Set at `locate_one` entry.
    pub started_at: String,
    /// Conversation this run came from, when the caller supplied one.
    #[serde(default)]
    pub session: Option<String>,
    /// Groups the elements of one `locate_element` call.
    #[serde(default)]
    pub batch: Option<String>,
    /// 0-based position inside `batch`.
    #[serde(default)]
    pub index: usize,
    pub frame: FrameTrace,
    pub request: RequestTrace,
    #[serde(default)]
    pub rounds: Vec<RoundTrace>,
    pub outcome: OutcomeTrace,
    /// **Filled in by a human**, not by the engine. The correct box in **dumped
    /// JPEG pixels** — the space of `frame.dumped`, which is what you measure by
    /// opening `frame.jpg`. (Equal to frame pixels whenever `render_full` did not
    /// downscale, which is the case for every screen up to 2.5 MP.) `None` until
    /// someone marks it; a trace with a truth is a regression case, one without
    /// is a failure report.
    #[serde(default)]
    pub truth_bbox: Option<[f64; 4]>,
    /// Free text for the person labelling: what went wrong, in their words.
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameTrace {
    /// File name relative to the debug directory.
    pub path: String,
    /// The frame the engine worked on, in logical pixels. Every box the engine
    /// wrote in this trace is in this space.
    pub width: u32,
    pub height: u32,
    /// Pixel size of the **dumped JPEG**. Differs from `width`/`height` when
    /// `render_full` downscaled a frame above `FULL_MAX_PX`, and that difference
    /// is exactly what makes a hand-measured box land in the wrong space.
    /// Absent in traces written before this field existed — treat it as
    /// `[width, height]`.
    #[serde(default)]
    pub dumped: Option<[u32; 2]>,
}

impl FrameTrace {
    /// The dumped JPEG's pixel size, falling back to the frame's own size.
    pub fn dumped_size(&self) -> (u32, u32) {
        match self.dumped {
            Some([w, h]) => (w.max(1), h.max(1)),
            None => (self.width.max(1), self.height.max(1)),
        }
    }

    /// Factor from frame pixels to dumped pixels (≤ 1.0 on both axes).
    pub fn dump_scale(&self) -> (f64, f64) {
        let (dw, dh) = self.dumped_size();
        (
            dw as f64 / self.width.max(1) as f64,
            dh as f64 / self.height.max(1) as f64,
        )
    }

    /// `true` when the JPEG is the frame, i.e. the two spaces coincide.
    pub fn dump_is_identity(&self) -> bool {
        let (dw, dh) = self.dumped_size();
        dw == self.width && dh == self.height
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestTrace {
    pub target_desc: String,
    pub goal: String,
    /// Caller's hint in frame pixels, when it supplied one.
    #[serde(default)]
    pub hint: Option<[f64; 2]>,
    /// The caller's whole hint **box** in frame pixels, `[x1, y1, x2, y2]`.
    ///
    /// `hint` above keeps only the centre, which was enough to log but not to
    /// replay: `LocateRequest::hint_norm` feeds the box into the first verify
    /// round, and a fixture built from a centre point silently verifies a
    /// different box than the one production used.
    #[serde(default)]
    pub hint_box: Option<[f64; 4]>,
}

/// One locate → verify round.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RoundTrace {
    pub round: usize,
    /// Absent when the round reused the caller's hint box (no locate call).
    #[serde(default)]
    pub locate: Option<LocateStep>,
    #[serde(default)]
    pub verify: Option<VerifyStep>,
}

/// One `locate` model call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocateStep {
    /// `full_init` / `crop_rN` / `full_widen_rN` / `full_escalate_rN`.
    #[serde(rename = "where")]
    pub where_: String,
    /// Image file name relative to the debug directory.
    pub image: String,
    /// Crop the model saw, in frame pixels.
    pub bounds: [f64; 4],
    /// Box the model was asked to correct.
    #[serde(default)]
    pub prev_box: Option<[f64; 4]>,
    /// The model's answer, in the image's own 0–1000 coordinates.
    #[serde(default)]
    pub raw: Option<[f64; 4]>,
    /// `raw` mapped back to frame pixels — what the next round verifies.
    #[serde(default, rename = "box")]
    pub box_px: Option<[f64; 4]>,
    /// `true` when the model answered 「not in this image」.
    #[serde(default)]
    pub not_found: bool,
    /// The **full** prompt this call was given.
    ///
    /// A box that is wrong because the prompt was wrong looks identical to one
    /// that is wrong because the model misread it. `target_desc` and the hint
    /// numbers are recorded elsewhere, but the rendered prompt is the only thing
    /// that shows how they were phrased and what the model was actually told —
    /// and it is what has to be replayed to reproduce a failure months later.
    #[serde(default)]
    pub prompt: Option<String>,
}

/// One `verify` model call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyStep {
    /// Close crop (图一) — authoritative for edges and contents.
    pub close_image: String,
    /// Wide crop (图二) — exists only to read row/column labels.
    #[serde(default)]
    pub wide_image: Option<String>,
    /// Box under judgement, in frame pixels.
    #[serde(rename = "box")]
    pub box_px: [f64; 4],
    pub center: String,
    pub size: String,
    pub position: String,
    pub notes: String,
    pub render_ms: u64,
    pub call_ms: u64,
    /// The **full** prompt this call was given (see `LocateStep::prompt`).
    #[serde(default)]
    pub prompt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutcomeTrace {
    pub ok: bool,
    /// Click point in frame pixels.
    #[serde(default)]
    pub point: Option<[f64; 2]>,
    #[serde(default, rename = "box")]
    pub box_px: Option<[f64; 4]>,
    pub rounds: usize,
    /// `verified` / `locate_failed` / `rounds_exhausted`.
    pub stop: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A trace written before `dumped` / `hint_box` / `prompt` existed must still
    /// load: the collector runs against directories that already hold production
    /// runs, and a rejected file is a lost case.
    #[test]
    fn trace_written_before_the_new_fields_still_loads() {
        let old = r#"{
          "run_id": "e45e4900",
          "started_at": "2026-10-27T10:25:03.561Z",
          "frame": { "path": "e45e4900_frame.jpg", "width": 1920, "height": 1080 },
          "request": { "target_desc": "保存按钮", "goal": "点击它", "hint": [384, 578] },
          "rounds": [
            { "round": 1,
              "locate": { "where": "full_init", "image": "a.jpg",
                          "bounds": [0, 0, 1920, 1080], "raw": [1, 2, 3, 4],
                          "box": [10, 20, 30, 40] },
              "verify": { "close_image": "b.jpg", "box": [10, 20, 30, 40],
                          "center": "Yes", "size": "Yes", "position": "yes",
                          "notes": "", "render_ms": 1, "call_ms": 2 } }
          ],
          "outcome": { "ok": true, "point": [20, 30], "box": [10, 20, 30, 40],
                       "rounds": 1, "stop": "Verified" }
        }"#;
        let trace: LocateTrace = serde_json::from_str(old).unwrap();

        assert_eq!(trace.request.hint, Some([384.0, 578.0]));
        assert_eq!(trace.request.hint_box, None);
        let round = &trace.rounds[0];
        assert_eq!(round.locate.as_ref().unwrap().prompt, None);
        assert_eq!(round.verify.as_ref().unwrap().prompt, None);
        // No `dumped` means the JPEG was the frame — the pre-downscale default.
        assert_eq!(trace.frame.dumped, None);
        assert_eq!(trace.frame.dumped_size(), (1920, 1080));
        assert!(trace.frame.dump_is_identity());
    }

    /// The scale factor is the whole reason `dumped` exists: a box measured on
    /// the JPEG has to be converted into frame pixels (or the other way round)
    /// by exactly this ratio.
    #[test]
    fn dump_scale_reports_the_downscaled_jpeg() {
        let frame = FrameTrace {
            path: "run_frame.jpg".into(),
            width: 2560,
            height: 1440,
            dumped: Some([2108, 1186]),
        };
        assert!(!frame.dump_is_identity());
        assert_eq!(frame.dumped_size(), (2108, 1186));
        let (sx, sy) = frame.dump_scale();
        assert!((sx - 0.8234).abs() < 0.001, "sx={sx}");
        assert!((sy - 0.8236).abs() < 0.001, "sy={sy}");
    }

    #[test]
    fn hint_box_survives_a_round_trip() {
        let json = serde_json::json!({
            "target_desc": "保存按钮",
            "goal": "点击它",
            "hint": [820.0, 45.0],
            "hint_box": [800.0, 35.0, 840.0, 55.0],
        });
        let req: RequestTrace = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(req.hint_box, Some([800.0, 35.0, 840.0, 55.0]));
        // Serialising keeps the key, so a collected file is self-describing.
        let back: serde_json::Value = serde_json::to_value(&req).unwrap();
        assert_eq!(back["hint_box"], json["hint_box"]);
    }
}
