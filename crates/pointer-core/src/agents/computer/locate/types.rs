//! Public data types for `locate_element` (design §6.2 / §6.2.1).
//!
//! Inputs and outputs are **logical pixels**. The 0–1000 normalized space is
//! internal to the locator and never leaves this module tree.

use super::geometry::{
    NormBox, PxBox, CROP_MAX, CROP_MIN, CROP_MULT, CROP_OUT_PX, VERIFY_MULT, VERIFY_OUT_PX,
};

/// Hard cap on one `locate_element` call (§6.2: no silent truncation).
pub const MAX_ELEMENTS: usize = 3;
/// B5 locate/verify rounds per element (`max_locate`).
pub const MAX_LOCATE_ROUNDS: usize = 3;
/// B5 verify retries for `unclear` verdicts (`max_verify_retry`).
pub const MAX_VERIFY_RETRY: usize = 2;

/// One element the caller wants located.
#[derive(Debug, Clone, PartialEq)]
pub struct LocateRequest {
    /// Natural-language description of the target element.
    pub target_desc: String,
    /// What the caller intends to do with it. Required: the grounder picks the
    /// element that can serve this purpose when several look alike, and the
    /// verifier treats "can this fulfil the purpose" as the primary judgement —
    /// a box that only covers part of the element still passes when clicking it
    /// would achieve the purpose.
    pub goal: String,
    /// Optional pre-given first-round box, in logical pixels.
    pub hint_px: Option<PxBox>,
}

impl LocateRequest {
    pub fn new(target_desc: impl Into<String>, goal: impl Into<String>) -> Self {
        Self {
            target_desc: target_desc.into(),
            goal: goal.into(),
            hint_px: None,
        }
    }

    pub fn with_hint_px(mut self, hint: PxBox) -> Self {
        self.hint_px = Some(hint);
        self
    }

    /// Centre of the caller's hint box, for the "prefer the nearest candidate"
    /// rule. Same-named elements on different rows are common, and a full-screen
    /// re-locate otherwise has no reason to stay near where the caller pointed.
    pub fn hint_center(&self) -> Option<(f64, f64)> {
        self.hint_px.map(|b| ((b.x1 + b.x2) / 2.0, (b.y1 + b.y2) / 2.0))
    }

    /// Normalized hint, or `None` when absent / zero-area (§6.3.1).
    pub fn hint_norm(&self, w: u32, h: u32) -> Option<NormBox> {
        self.hint_px
            .map(|b| b.to_norm(w, h))
            .filter(|b| !b.is_degenerate())
    }
}

/// Verifier verdict (`match` in B5 `PROMPT_VERIFY`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchVerdict {
    Yes,
    No,
    Unclear,
}

/// Direction the target was seen relative to the candidate box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElsewhereDir {
    Left,
    Right,
    Up,
    Down,
    Inside,
}

impl ElsewhereDir {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
            Self::Up => "up",
            Self::Down => "down",
            Self::Inside => "inside",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "left" => Some(Self::Left),
            "right" => Some(Self::Right),
            "up" => Some(Self::Up),
            "down" => Some(Self::Down),
            "inside" => Some(Self::Inside),
            _ => None,
        }
    }
}

/// Structured verifier output. Defaults mirror B5's "verifier produced nothing"
/// fallback: `unclear` (i.e. never silently pass).
#[derive(Debug, Clone, PartialEq)]
pub struct VerifyVerdict {
    /// 判定 —— 十字交点是否**落在目标上**。
    ///
    /// **这是唯一的判定**，模型只在一处表态，不存在两处打架。
    pub is_same: MatchVerdict,
    /// 备注 —— 观测记录（图一读数 / 图二读数 / 描述要什么 / 结论）。
    ///
    /// Semi-structured on purpose: `is_same` says *whether* it passed, this says
    /// *what was seen*. It is passed through to the locator verbatim as its hint,
    /// so the model that has to fix the box reads the verifier's own words
    /// instead of a translation of them.
    pub notes: String,
    /// The model's thinking channel, when the provider returned one. Empty when
    /// thinking is off. Kept for diagnosis: a wrong verdict is much easier to
    /// explain when you can read what the model reasoned before answering.
    pub reasoning: String,
}

impl VerifyVerdict {
    /// Did all three independent checks pass?
    ///
    /// `unknown` for the position blocks: it means the description demanded a
    /// position the image could not confirm, and returning a box on that basis is
    /// a guess. `ignore` passes — there was nothing to check.
    pub fn is_hit(&self) -> bool {
        // 只看判定。`size_ok` 不再参与 —— 框大小不影响交付坐标，而实测里
        // 模型把「框」和「目标」当成同一个东西，size 从来没判对过。
        self.is_same == MatchVerdict::Yes
    }

}

impl Default for VerifyVerdict {
    fn default() -> Self {
        Self {
            // 没拿到判定：`Unclear` 让 `stay_local` 判否重找，不直接当成通过。
            is_same: MatchVerdict::Unclear,
            notes: "校验器无有效输出".to_string(),
            reasoning: String::new(),
        }
    }
}

/// Tunables; defaults are the B5 baseline constants.
///
/// Not `Copy`: it carries the trace context (session / batch), which are strings
/// handed in by the caller once per run. Cloning it per element is the only
/// place that cost shows up.
#[derive(Debug, Clone, PartialEq)]
pub struct LocateConfig {
    pub max_locate: usize,
    pub max_verify_retry: usize,
    pub crop_mult: f64,
    pub crop_min: f64,
    pub crop_max: f64,
    pub out_px: u32,
    /// How much larger the verifier's crop is than the locator's, on both axes.
    ///
    /// The two want opposite things from a crop. Locating wants it small: the
    /// model answers in percent of the crop, so the crop's own size is the error
    /// budget (`~1.5 % × side`). Verifying wants context: to say 「is this the
    /// right cell」 it has to see the neighbours, and a crop tight enough to read
    /// an edge has none.
    ///
    /// Both the covered area and the rendered size scale by this factor, so the
    /// verifier sees more *and* sees it at the same or higher resolution. 1.0
    /// reuses the locator's crop.
    pub verify_mult: f64,
    /// Render size for the wider verify crop. `0` reuses `out_px`.
    ///
    /// Separate from `out_px` because the two crops are asked different
    /// questions: the locator's crop needs resolution on the target alone, the
    /// verifier's needs context *and* resolution. Sharing one number forced a
    /// trade that satisfied neither.
    pub verify_out_px: u32,
    /// Which conversation this run belongs to. Recorded in the trace so a
    /// collected sample says where it came from — a box that is wrong on a
    /// sudoku board and one that is wrong on a spreadsheet are different bugs.
    pub trace_session: Option<String>,
    /// Pipeline round this run belongs to (`1`, `2`, …). Together with
    /// `trace_session` and `trace_msg_seq` it forms the sample prefix
    /// `{conv8}_{round:03d}_{msg:03d}`, so a collected run can be joined back
    /// to the round it came from without parsing timestamps.
    pub trace_round_seq: Option<u32>,
    /// `locate_element` call number inside that round (1-based).
    pub trace_msg_seq: Option<u32>,
    /// Groups the elements of one `locate_element` call. Three elements in one
    /// call produce three runs; without this they cannot be told apart from
    /// three separate calls.
    pub trace_batch: Option<String>,
    /// Position of this run inside its batch, 0-based.
    pub trace_index: usize,
}

impl Default for LocateConfig {
    fn default() -> Self {
        Self {
            max_locate: MAX_LOCATE_ROUNDS,
            max_verify_retry: MAX_VERIFY_RETRY,
            crop_mult: CROP_MULT,
            crop_min: CROP_MIN,
            crop_max: CROP_MAX,
            out_px: CROP_OUT_PX,
            verify_mult: env_f64("POINTER_LOCATE_VERIFY_MULT", VERIFY_MULT),
            verify_out_px: env_u32("POINTER_LOCATE_VERIFY_OUT_PX", VERIFY_OUT_PX),
            trace_session: None,
            trace_round_seq: None,
            trace_msg_seq: None,
            trace_batch: None,
            trace_index: 0,
        }
    }
}

/// Diagnostic override for the verifier's second crop.
///
/// The wide crop costs a second image upload on **every** verify call. Whether
/// that cost is worth it is a question about the model's latency curve, not
/// about the code, so it has to be measurable without editing constants and
/// rebuilding: `POINTER_LOCATE_VERIFY_MULT=1` disables the second image,
/// `POINTER_LOCATE_VERIFY_OUT_PX=960` halves it. Absent → the compiled default.
fn env_f64(key: &str, default: f64) -> f64 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(default)
}

fn env_u32(key: &str, default: u32) -> u32 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(default)
}

/// Why an element could not be located. Failure never carries coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureReason {
    /// The locator returned no box at all.
    NotFound,
    /// Boxes were produced but never verified within the round budget.
    NotVerified,
    /// The crop was too blurry / occluded to judge.
    Unclear,
}

impl FailureReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotFound => "not_found",
            Self::NotVerified => "not_verified",
            Self::Unclear => "unclear",
        }
    }
}

/// Result for one requested element, in input order.
#[derive(Debug, Clone, PartialEq)]
pub struct LocateOutcome {
    pub target_desc: String,
    pub ok: bool,
    /// Logical-pixel click point (`None` unless `ok`).
    pub point_px: Option<(f64, f64)>,
    /// Logical-pixel box (`None` unless `ok`).
    pub box_px: Option<PxBox>,
    /// What the verifier actually saw in the candidate box.
    pub what_i_saw: String,
    /// The verifier's one-line reason. On failure this is the "how far off"
    /// sentence — the main model never sees a box, so it has to be a distance.
    pub why: String,
    /// Number of crop/verify rounds spent (B5 `locate_rounds`).
    pub rounds: usize,
    pub reason: Option<FailureReason>,
    /// The verifier's thinking channel from the final round. Diagnostic only —
    /// never shown to the main model. Empty when thinking is off.
    pub reasoning: String,
}

impl LocateOutcome {
    pub fn failure(target_desc: impl Into<String>, reason: FailureReason, what_i_saw: String, rounds: usize) -> Self {
        Self {
            target_desc: target_desc.into(),
            ok: false,
            point_px: None,
            box_px: None,
            what_i_saw,
            why: String::new(),
            rounds,
            reason: Some(reason),
            reasoning: String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn degenerate_hint_is_ignored() {
        let req = LocateRequest::new("保存按钮", "点击它").with_hint_px(PxBox {
            x1: 10.0,
            y1: 10.0,
            x2: 10.0,
            y2: 10.0,
        });
        assert!(req.hint_norm(1000, 800).is_none());
    }

    #[test]
    fn valid_hint_is_normalized() {
        let req = LocateRequest::new("保存按钮", "点击它").with_hint_px(PxBox {
            x1: 800.0,
            y1: 35.0,
            x2: 840.0,
            y2: 55.0,
        });
        let hint = req.hint_norm(1000, 800).expect("hint");
        assert!((hint.x1 - 800.0).abs() < 1e-9);
        assert!((hint.y2 - 68.75).abs() < 1e-9);
    }

    #[test]
    fn verifier_default_never_passes() {
        let v = VerifyVerdict::default();
        assert!(!v.is_hit());
        assert_eq!(v.is_same, MatchVerdict::Unclear);
    }

    #[test]
    fn failure_outcome_has_no_coordinates() {
        let o = LocateOutcome::failure("地址栏", FailureReason::NotFound, "未确认".into(), 1);
        assert!(!o.ok);
        assert!(o.point_px.is_none());
        assert!(o.box_px.is_none());
        assert_eq!(o.reason, Some(FailureReason::NotFound));
    }
}
