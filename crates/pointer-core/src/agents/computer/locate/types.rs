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

/// Why the verifier could not decide.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnclearKind {
    None,
    AnchorOutOfView,
    TooSmall,
    TooBlurry,
    Ambiguous,
    /// The box holds part of the target — the rest spills outside. Distinct from
    /// `AnchorOutOfView` (target is somewhere else entirely) and from `TooSmall`
    /// (target is inside the box but too tiny to read). The fix here is a wider
    /// box, not a different place to look.
    BoxTooTight,
    /// The box holds the whole target and a lot of its surroundings. Distinct
    /// from `BoxTooTight` in the opposite direction, and from `No`: the target
    /// *is* here, the box is just slack. Reported as `unclear` rather than `no`
    /// so the retry says "shrink around the same place" instead of "look
    /// elsewhere" — a `no` carries the wrong instruction and, with
    /// `elsewhere_dir=inside`, produces the self-contradictory hint
    /// 「不是目标」+「目标在框内部」.
    BoxTooLoose,
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

/// Whether the centre mark landed on the target.
///
/// Split out from `MatchVerdict` on purpose. The old single verdict asked 「is the
/// target in the box」, which a generous box always answers `yes` to — and then
/// the coordinate returned is the box's centre, which can be nowhere near the
/// target. This asks the question that actually decides the answer: **is the
/// point we are about to return sitting on the target**.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CenterOnTarget {
    /// The mark is on the target.
    Yes,
    /// The mark is off the target; `center_offset` says which side.
    No,
    /// Cannot tell (blur, occlusion, too small).
    Unclear,
}

impl CenterOnTarget {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "yes" => Some(Self::Yes),
            "no" => Some(Self::No),
            "unclear" => Some(Self::Unclear),
            _ => None,
        }
    }
}

/// Does the position the description names match the position in the image?
///
/// Separate from `CenterOnTarget` because they fail differently and need
/// different fixes. The mark can sit on *a* cell while that cell is the wrong one
/// — 「第四行第八列」 vs a box on row 3 — and no amount of looking at the mark
/// alone will catch it. This is the check that compares what the description
/// says with what the image shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PositionMatches {
    /// The description named a position and the image agrees.
    Yes,
    /// The description named a position and the image disagrees.
    No,
    /// The description names a position but the image does not show enough
    /// (labels cropped away, no absolute reference) to confirm or deny it.
    ///
    /// Distinct from `Ignore`: here the description *does* demand a position, so
    /// the answer is unverified rather than unnecessary. Treating this as a pass
    /// is what let r3c7 through — the verifier wrote 「绝对行号无法确证」 in `why`
    /// and still returned a hit.
    Unknown,
    /// The description asks for no position at all (「提交按钮」, 「关闭图标」) —
    /// there is nothing to check.
    Ignore,
}

impl PositionMatches {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "yes" => Some(Self::Yes),
            "no" => Some(Self::No),
            "unknown" => Some(Self::Unknown),
            "ignore" => Some(Self::Ignore),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Yes => "yes",
            Self::No => "no",
            Self::Unknown => "unknown",
            Self::Ignore => "ignore",
        }
    }
}

/// Structured verifier output. Defaults mirror B5's "verifier produced nothing"
/// fallback: `unclear` + `too_blurry` (i.e. never silently pass).
#[derive(Debug, Clone, PartialEq)]
pub struct VerifyVerdict {
    /// 中心定位 —— is the centre mark on the target? The mark sits at the box
    /// centre, which is the coordinate the engine returns, so this is the
    /// question that decides whether the answer is right.
    pub center_on_target: CenterOnTarget,
    /// 大小合适 —— does the box frame the target properly (fully containing it,
    /// not more than 1.5x its size)? Judged by the verifier, not by a threshold
    /// in code.
    pub size_ok: MatchVerdict,
    /// 位置匹配 —— does the position the description names match what the image
    /// shows? Orthogonal to the two above: the mark can be on *a* cell while that
    /// cell is the wrong one.
    pub position_matches: PositionMatches,
    /// 备注 —— the diagnosis, four fixed sections (图像 / 中心 / 大小 / 位置).
    ///
    /// Semi-structured on purpose: the three verdicts above say *whether* each
    /// check passed, this says *what was seen*. It is passed through to the
    /// locator verbatim as its hint, so the model that has to fix the box reads
    /// the verifier's own words instead of a translation of them.
    pub notes: String,
    /// How many of the four appearance signals (text / position / icon / colour)
    /// lined up. Diagnostic only; never gates a hit.
    pub confidence: Confidence,
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
        self.center_on_target == CenterOnTarget::Yes
            && self.size_ok == MatchVerdict::Yes
            && !matches!(
                self.position_matches,
                PositionMatches::No | PositionMatches::Unknown
            )
    }

    /// Which checks failed, for the hint. Empty when `is_hit()`.
    pub fn failed_checks(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        if self.center_on_target != CenterOnTarget::Yes {
            out.push("中心定位");
        }
        if self.size_ok != MatchVerdict::Yes {
            out.push("大小合适");
        }
        if matches!(
            self.position_matches,
            PositionMatches::No | PositionMatches::Unknown
        ) {
            out.push("位置匹配");
        }
        out
    }
}

impl Default for VerifyVerdict {
    fn default() -> Self {
        Self {
            center_on_target: CenterOnTarget::Unclear,
            size_ok: MatchVerdict::Unclear,
            // `unknown`, not `ignore`: a verdict that never arrived cannot claim
            // the description asked for no position. This is the "verifier
            // produced nothing" fallback and must never read as a pass.
            position_matches: PositionMatches::Unknown,
            notes: "校验器无有效输出".to_string(),
            confidence: Confidence::High,
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

/// Confidence of a located element. Only verified targets are returned, so the
/// only value that reaches the caller is `High` (§6.3.1: a hint may not lower
/// the bar).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confidence {
    High,
    Medium,
}

impl Confidence {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::High => "high",
            Self::Medium => "medium",
        }
    }

    pub fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "medium" | "mid" | "中" => Self::Medium,
            _ => Self::High,
        }
    }
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
    pub confidence: Option<Confidence>,
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
            confidence: None,
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
        assert_eq!(v.center_on_target, CenterOnTarget::Unclear);
        assert_eq!(v.size_ok, MatchVerdict::Unclear);
        // `unknown`, not `ignore`: a verdict that never arrived cannot claim the
        // description asked for no position.
        assert_eq!(v.position_matches, PositionMatches::Unknown);
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
