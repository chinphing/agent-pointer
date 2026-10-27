//! B5 geometry: **0–1000 normalized** boxes, **logical-pixel** crops, and the
//! conversions between the two coordinate spaces (design §6.2.1).
//!
//! The locator LLM only ever sees an image and answers in 0–1000 coordinates
//! *of that image* (B5 `coord-mode=rel`). Everything outside this module works
//! in logical pixels. `rel_to_norm` is the single bridge between the two.

/// Crop side = target box long edge × this multiplier (B5 `CROP_MULT`).
pub const CROP_MULT: f64 = 3.0;
/// Smallest crop side in logical pixels (B5 `CROP_MIN`).
pub const CROP_MIN: f64 = 240.0;
/// Largest crop side in logical pixels (B5 `CROP_MAX`).
pub const CROP_MAX: f64 = 1600.0;
/// Side length the crop is resized to before the verifier sees it (B5 `OUT_PX`).
pub const CROP_OUT_PX: u32 = 1600;

/// How much larger the verifier's crop is than the locator's, on both axes.
///
/// 2x covers four times the area and renders at 3200x3200 (10.2 Mpx, ~10k prompt
/// tokens). The verifier gets the neighbouring cells it needs to say whether the
/// box is on the right one; the locator keeps its 1600 px crop and its
/// `~1.5 % × side` error budget.
/// How much larger the verifier's crop is than the locator's, on both axes.
///
/// 4.0, measured rather than guessed. The locator's side bottoms out at
/// `CROP_MIN` (240 px), so 2.0 gave the verifier only 480 px — and the sudoku
/// fixture showed exactly what that costs: the board spans 370 px, the row
/// labels another ~30, so a 480 px window that follows a slightly-off box crops
/// the labels away entirely. The verifier then correctly reports 「no row labels
/// in view」 and the position check can never be answered. 240 × 4 = 960 px
/// covers the board plus its labels with room for the box to drift a cell.
pub const VERIFY_MULT: f64 = 4.0;
/// Render size for the verifier's wider crop, independent of `CROP_OUT_PX`.
///
/// The wider crop has to carry two jobs at once: show enough context to read the
/// labels around the target, and still resolve the target finely enough to say
/// whether the centre mark is on it. Area scales with `VERIFY_MULT²`, so at the
/// locator's 1600 px the target lands at 68 px — coarse. 1920 brings it to 82 px
/// for ~44 % more prompt tokens (area, not side, is what the provider bills).
pub const VERIFY_OUT_PX: u32 = 1920;
/// Full-screen images above this pixel count are downscaled (B5 `FULL_MAX_PX`).
pub const FULL_MAX_PX: f64 = 2_500_000.0;

const NORM_MAX: f64 = 1000.0;

/// A box in **0–1000 normalized** screen space (`x1 ≤ x2`, `y1 ≤ y2`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NormBox {
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
}

impl NormBox {
    /// Clamp to `[0, 1000]` and normalize corner order (B5 `_clamp_box`).
    pub fn new(x1: f64, y1: f64, x2: f64, y2: f64) -> Self {
        Self {
            x1,
            y1,
            x2,
            y2,
        }
        .normalized()
    }

    /// Clamp every coordinate into `[0, 1000]`, then order corners.
    pub fn normalized(self) -> Self {
        let clamp = |v: f64| {
            if v.is_finite() {
                v.clamp(0.0, NORM_MAX)
            } else {
                0.0
            }
        };
        let (x1, y1, x2, y2) = (
            clamp(self.x1),
            clamp(self.y1),
            clamp(self.x2),
            clamp(self.y2),
        );
        Self {
            x1: x1.min(x2),
            y1: y1.min(y2),
            x2: x1.max(x2),
            y2: y1.max(y2),
        }
    }

    /// Box center in normalized space.
    pub fn center(self) -> (f64, f64) {
        ((self.x1 + self.x2) / 2.0, (self.y1 + self.y2) / 2.0)
    }

    /// A zero-area box is not usable as a hint (§6.3.1) and has no center.
    pub fn is_degenerate(self) -> bool {
        (self.x2 - self.x1) < 0.5 || (self.y2 - self.y1) < 0.5
    }

    /// Normalized → logical pixels for an image of `w × h`.
    pub fn to_px(self, w: u32, h: u32) -> PxBox {
        let (w, h) = (w as f64, h as f64);
        PxBox {
            x1: self.x1 / NORM_MAX * w,
            y1: self.y1 / NORM_MAX * h,
            x2: self.x2 / NORM_MAX * w,
            y2: self.y2 / NORM_MAX * h,
        }
    }
}

/// A box in **logical pixels** of one image.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PxBox {
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
}

impl PxBox {
    pub fn width(self) -> f64 {
        self.x2 - self.x1
    }

    pub fn height(self) -> f64 {
        self.y2 - self.y1
    }

    pub fn center(self) -> (f64, f64) {
        ((self.x1 + self.x2) / 2.0, (self.y1 + self.y2) / 2.0)
    }

    /// Logical pixels → normalized for an image of `w × h`.
    pub fn to_norm(self, w: u32, h: u32) -> NormBox {
        let (w, h) = (w.max(1) as f64, h.max(1) as f64);
        NormBox::new(
            self.x1 / w * NORM_MAX,
            self.y1 / h * NORM_MAX,
            self.x2 / w * NORM_MAX,
            self.y2 / h * NORM_MAX,
        )
    }
}

/// A square window on the full screenshot, in logical pixels. May extend past
/// the image bounds; rendering pads those areas black (PIL `crop` behaviour).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CropBounds {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

impl CropBounds {
    /// Whole-image bounds.
    pub fn full(w: u32, h: u32) -> Self {
        Self {
            x0: 0.0,
            y0: 0.0,
            x1: w as f64,
            y1: h as f64,
        }
    }

    /// Centered `side × side` window around `target` (B5 `render_crop`).
    pub fn around(target: PxBox, side: f64) -> Self {
        let (cx, cy) = target.center();
        let half = side / 2.0;
        Self {
            x0: cx - half,
            y0: cy - half,
            x1: cx + half,
            y1: cy + half,
        }
    }

    pub fn width(self) -> f64 {
        self.x1 - self.x0
    }

    pub fn height(self) -> f64 {
        self.y1 - self.y0
    }
}

/// Crop side for a target box: `clamp(long edge × mult, min, max)` (B5 `crop_side_px`).
pub fn crop_side_px(target: PxBox, mult: f64, min: f64, max: f64) -> f64 {
    let side = target.width().max(target.height()) * mult;
    side.clamp(min, max)
}

/// Crop-image 0–1000 → full-screen 0–1000 (B5 `rel_to_norm`, pure math).
pub fn rel_to_norm(rel: NormBox, bounds: CropBounds, w: u32, h: u32) -> NormBox {
    let bw = bounds.width();
    let bh = bounds.height();
    let (w, h) = (w.max(1) as f64, h.max(1) as f64);
    NormBox::new(
        (bounds.x0 + rel.x1 / NORM_MAX * bw) / w * NORM_MAX,
        (bounds.y0 + rel.y1 / NORM_MAX * bh) / h * NORM_MAX,
        (bounds.x0 + rel.x2 / NORM_MAX * bw) / w * NORM_MAX,
        (bounds.y0 + rel.y2 / NORM_MAX * bh) / h * NORM_MAX,
    )
}

/// Screen pixels → this crop's 0–1000 scale. The inverse of `rel_to_norm`'s
/// first half, needed when a prompt has to quote a box position back to the
/// model in the same units the model answers in.
pub fn px_to_rel(b: PxBox, bounds: CropBounds) -> (f64, f64, f64, f64) {
    let bw = bounds.width().max(f64::EPSILON);
    let bh = bounds.height().max(f64::EPSILON);
    (
        (b.x1 - bounds.x0) / bw * NORM_MAX,
        (b.y1 - bounds.y0) / bh * NORM_MAX,
        (b.x2 - bounds.x0) / bw * NORM_MAX,
        (b.y2 - bounds.y0) / bh * NORM_MAX,
    )
}

/// Downscale factor for the full-screen image (`1.0` = no resize).
pub fn fit_scale(w: u32, h: u32, max_px: f64) -> f64 {
    let area = (w.max(1) as f64) * (h.max(1) as f64);
    if area <= max_px {
        return 1.0;
    }
    (max_px / area).sqrt().min(1.0)
}

/// Every coordinate moved by at most this much (normalized) means "no progress".
pub const NO_PROGRESS_EPSILON: f64 = 2.0;

/// B5 no-progress guard: the re-located box barely moved.
pub fn nearly_same(a: NormBox, b: NormBox) -> bool {
    (a.x1 - b.x1).abs() <= NO_PROGRESS_EPSILON
        && (a.y1 - b.y1).abs() <= NO_PROGRESS_EPSILON
        && (a.x2 - b.x2).abs() <= NO_PROGRESS_EPSILON
        && (a.y2 - b.y2).abs() <= NO_PROGRESS_EPSILON
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamps_and_reorders_corners() {
        let b = NormBox::new(900.0, 800.0, 100.0, -50.0);
        assert_eq!(
            b,
            NormBox {
                x1: 100.0,
                y1: 0.0,
                x2: 900.0,
                y2: 800.0
            }
        );
    }

    #[test]
    fn zero_area_box_is_degenerate() {
        assert!(NormBox::new(10.0, 10.0, 10.0, 10.0).is_degenerate());
        assert!(NormBox::new(0.0, 0.0, 0.0, 500.0).is_degenerate());
        assert!(!NormBox::new(10.0, 10.0, 11.0, 11.0).is_degenerate());
    }

    #[test]
    fn px_norm_round_trip_is_stable() {
        let (w, h) = (1000u32, 800u32);
        let px = PxBox {
            x1: 800.0,
            y1: 35.0,
            x2: 840.0,
            y2: 55.0,
        };
        let norm = px.to_norm(w, h);
        assert!((norm.x1 - 800.0).abs() < 1e-9);
        assert!((norm.y1 - 43.75).abs() < 1e-9);
        let back = norm.to_px(w, h);
        assert!((back.x1 - px.x1).abs() < 1e-6);
        assert!((back.y2 - px.y2).abs() < 1e-6);
    }

    #[test]
    fn crop_side_uses_long_edge_and_clamps() {
        let small = PxBox {
            x1: 0.0,
            y1: 0.0,
            x2: 10.0,
            y2: 10.0,
        };
        assert_eq!(crop_side_px(small, CROP_MULT, CROP_MIN, CROP_MAX), CROP_MIN);

        let big = PxBox {
            x1: 0.0,
            y1: 0.0,
            x2: 2000.0,
            y2: 100.0,
        };
        assert_eq!(crop_side_px(big, CROP_MULT, CROP_MIN, CROP_MAX), CROP_MAX);

        let mid = PxBox {
            x1: 0.0,
            y1: 0.0,
            x2: 100.0,
            y2: 40.0,
        };
        assert_eq!(crop_side_px(mid, CROP_MULT, CROP_MIN, CROP_MAX), 300.0);
    }

    #[test]
    fn crop_bounds_center_on_target() {
        let target = PxBox {
            x1: 100.0,
            y1: 100.0,
            x2: 200.0,
            y2: 200.0,
        };
        let b = CropBounds::around(target, 300.0);
        assert_eq!(b, CropBounds { x0: 0.0, y0: 0.0, x1: 300.0, y1: 300.0 });
    }

    #[test]
    fn rel_to_norm_maps_crop_local_coordinates_back_to_screen() {
        // A crop covering the whole image is the identity mapping.
        let full = CropBounds::full(1000, 800);
        let same = rel_to_norm(NormBox::new(100.0, 200.0, 300.0, 400.0), full, 1000, 800);
        assert!((same.x1 - 100.0).abs() < 1e-9);
        assert!((same.y2 - 400.0).abs() < 1e-9);

        // Half-size crop starting at (500, 400) of a 1000×800 screen.
        let half = CropBounds {
            x0: 500.0,
            y0: 400.0,
            x1: 1000.0,
            y1: 800.0,
        };
        let mapped = rel_to_norm(NormBox::new(0.0, 0.0, 500.0, 500.0), half, 1000, 800);
        assert!((mapped.x1 - 500.0).abs() < 1e-9);
        // y is normalized against the screen height (800), so 400 px = 500.
        assert!((mapped.y1 - 500.0).abs() < 1e-9);
        assert!((mapped.x2 - 750.0).abs() < 1e-9);
        assert!((mapped.y2 - 750.0).abs() < 1e-9);
    }

    #[test]
    fn rel_to_norm_clamps_when_crop_exceeds_screen() {
        let outside = CropBounds {
            x0: -200.0,
            y0: -100.0,
            x1: 200.0,
            y1: 100.0,
        };
        let mapped = rel_to_norm(NormBox::new(0.0, 0.0, 1000.0, 1000.0), outside, 1000, 800);
        assert_eq!(
            mapped,
            NormBox {
                x1: 0.0,
                y1: 0.0,
                x2: 200.0,
                y2: 125.0
            }
        );
    }

    #[test]
    fn fit_scale_downscales_only_large_captures() {
        assert_eq!(fit_scale(1920, 1080, FULL_MAX_PX), 1.0);
        let s = fit_scale(2560, 1440, FULL_MAX_PX);
        assert!(s < 1.0 && s > 0.8);
    }

    #[test]
    fn nearly_same_uses_b5_two_unit_tolerance() {
        let a = NormBox::new(100.0, 100.0, 200.0, 200.0);
        assert!(nearly_same(a, NormBox::new(102.0, 98.0, 202.0, 198.0)));
        assert!(!nearly_same(a, NormBox::new(103.0, 100.0, 200.0, 200.0)));
    }
}
