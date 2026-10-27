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
//!   "frame": { "path": "e45e4900_frame.jpg", "width": 1920, "height": 1080 },
//!   "request": { "target_desc": "...", "goal": "...", "hint": [384, 578] },
//!   "rounds": [
//!     {
//!       "round": 1,
//!       "locate": { "where": "crop_r1", "image": "...jpg", "bounds": [...],
//!                   "prev_box": [...], "raw": [...], "box": [...] },
//!       "verify": { "close_image": "...", "wide_image": "...",
//!                   "center": "No", "size": "No", "position": "yes",
//!                   "notes": "...", "render_ms": 186, "call_ms": 6283 }
//!     }
//!   ],
//!   "outcome": { "ok": true, "point": [408, 587], "box": [...], "rounds": 2 },
//!   "truth_bbox": null,
//!   "note": ""
//! }
//! ```

use serde::{Deserialize, Serialize};

/// One locate run. Serialised to `<run_id>.json` in the debug directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocateTrace {
    pub run_id: String,
    /// UTC, RFC 3339. Set at `locate_one` entry.
    pub started_at: String,
    pub frame: FrameTrace,
    pub request: RequestTrace,
    #[serde(default)]
    pub rounds: Vec<RoundTrace>,
    pub outcome: OutcomeTrace,
    /// **Filled in by a human**, not by the engine. The correct box in frame
    /// pixels. `None` until someone marks it; a trace with a truth is a
    /// regression case, one without is a failure report.
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
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestTrace {
    pub target_desc: String,
    pub goal: String,
    /// Caller's hint in frame pixels, when it supplied one.
    #[serde(default)]
    pub hint: Option<[f64; 2]>,
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
