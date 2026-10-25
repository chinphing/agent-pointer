//! Public data types for `locate_element` (design §6.2 / §6.2.1).
//!
//! Inputs and outputs are **logical pixels**. The 0–1000 normalized space is
//! internal to the locator and never leaves this module tree.

use super::geometry::{NormBox, PxBox, CROP_MAX, CROP_MIN, CROP_MULT, CROP_OUT_PX};

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

/// Structured verifier output. Defaults mirror B5's "verifier produced nothing"
/// fallback: `unclear` + `too_blurry` (i.e. never silently pass).
#[derive(Debug, Clone, PartialEq)]
pub struct VerifyVerdict {
    pub match_: MatchVerdict,
    pub why: String,
    pub unclear_kind: UnclearKind,
    pub elsewhere_dir: Option<ElsewhereDir>,
    pub elsewhere_desc: String,
    /// How many of the four signals (text / position / icon / colour) lined up.
    /// `high` = all four; `medium` = text and position matched but icon or
    /// colour did not. Only meaningful when `match_` is `Yes`.
    pub confidence: Confidence,
    /// The model's thinking channel, when the provider returned one. Empty when
    /// thinking is off. Kept for diagnosis: a wrong verdict is much easier to
    /// explain when you can read what the model reasoned before answering.
    pub reasoning: String,
}

impl Default for VerifyVerdict {
    fn default() -> Self {
        Self {
            match_: MatchVerdict::Unclear,
            why: "校验器无有效输出".to_string(),
            unclear_kind: UnclearKind::TooBlurry,
            elsewhere_dir: None,
            elsewhere_desc: String::new(),
            confidence: Confidence::High,
            reasoning: String::new(),
        }
    }
}

/// Tunables; defaults are the B5 baseline constants.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LocateConfig {
    pub max_locate: usize,
    pub max_verify_retry: usize,
    pub crop_mult: f64,
    pub crop_min: f64,
    pub crop_max: f64,
    pub out_px: u32,
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
        }
    }
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
        assert_eq!(v.match_, MatchVerdict::Unclear);
        assert_eq!(v.unclear_kind, UnclearKind::TooBlurry);
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
