//! Image rendering for the B5 locator: full-screen frame, centered crop, and
//! the blue candidate box drawn on both.
//!
//! All rendering works on the **logical-pixel** screenshot; the normalized
//! geometry lives in [`super::geometry`].

use anyhow::{anyhow, Context, Result};
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{ExtendedColorType, ImageFormat, Rgb, RgbImage};
use imageproc::drawing::draw_hollow_rect_mut;
use imageproc::rect::Rect;
use std::io::Cursor;

use super::geometry::{fit_scale, CropBounds, NormBox, PxBox, FULL_MAX_PX};

/// Candidate-box inner ring: magenta, on the candidate edge itself.
///
/// Not black/white. Those contrast with the *background* but not with the
/// interface: a white-on-black document, a black-on-white toolbar, grey chrome —
/// a monochrome outline reads as another piece of UI, and the verifier has to
/// tell 「the box」 from 「a border that was already there」. Magenta does not
/// occur in any normal desktop palette, so the ring can only be the annotation.
const BOX_COLOR: Rgb<u8> = Rgb([255, 0, 200]);
/// Candidate-box outer ring: cyan, the complement of the inner ring.
///
/// The pair is chosen so that at least one ring contrasts with any background:
/// magenta stands out on white and light grey, cyan on black and dark chrome.
/// They also contrast with each other, so the boundary is legible even where the
/// background matches one of them.
const BOX_OUTER_COLOR: Rgb<u8> = Rgb([0, 225, 255]);
/// Candidate-box line width, per ring.
///
/// 4 rather than 3 because JPEG chroma subsampling smears saturated colour on
/// thin lines: at 3 px the magenta/cyan pair survives but reads muddy, and the
/// whole point of the pair is being unmistakable.
const BOX_WIDTH: i32 = 4;
/// JPEG quality for images handed to the locator / verifier.
const LOCATE_JPEG_QUALITY: u8 = 85;

/// One encoded image plus its pixel size (tests assert which frame was sent).
#[derive(Debug, Clone, PartialEq, Eq)]pub struct LocateImage {
    pub jpeg: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// The screenshot a locate run works against, in **logical pixels**.
#[derive(Debug, Clone)]
pub struct LocateFrame {
    image: RgbImage,
}

impl LocateFrame {
    pub fn new(image: RgbImage) -> Self {
        Self { image }
    }

    /// Decode a JPEG screenshot (already resampled to logical resolution).
    pub fn from_jpeg(jpeg: &[u8]) -> Result<Self> {
        let img = image::load_from_memory_with_format(jpeg, ImageFormat::Jpeg)
            .context("decode screenshot jpeg")?
            .to_rgb8();
        Ok(Self::new(img))
    }

    pub fn width(&self) -> u32 {
        self.image.width()
    }

    pub fn height(&self) -> u32 {
        self.image.height()
    }

    pub fn image(&self) -> &RgbImage {
        &self.image
    }

    /// Full screen, downscaled when above [`FULL_MAX_PX`] (B5 `_fit`).
    fn fitted(&self) -> RgbImage {
        let (w, h) = (self.width().max(1), self.height().max(1));
        let scale = fit_scale(w, h, FULL_MAX_PX);
        if scale >= 1.0 {
            return self.image.clone();
        }
        let nw = ((w as f64 * scale).round() as u32).max(1);
        let nh = ((h as f64 * scale).round() as u32).max(1);
        image::imageops::resize(&self.image, nw, nh, FilterType::Lanczos3)
    }
}

/// Render the whole screen, optionally with the previous candidate box drawn.
pub fn render_full(frame: &LocateFrame, candidate: Option<NormBox>) -> Result<LocateImage> {
    let mut img = frame.fitted();
    if let Some(b) = candidate {
        draw_norm_box(&mut img, b);
    }
    encode(&img)
}

/// Whole screen with the crosshair (and optionally the tolerance ring) drawn at
/// the candidate's centre.
///
/// 图二 用整屏而不是裁片时走这条路。裁片版（`render_crop_marked`）输出正方形，
/// 整屏是 1920×1080 —— 只能用 `fitted()` 的原始宽高比，所以单独一个函数。
///
/// 十字臂长仍按框算（`0.35 × box`），所以目标多小，十字就多小，不会因为画面
/// 变大而盖住目标。
pub fn render_full_marked(
    frame: &LocateFrame,
    candidate: NormBox,
    mark: Mark,
) -> Result<LocateImage> {
    let mut img = frame.fitted();
    let (sw, sh) = (frame.width().max(1) as f64, frame.height().max(1) as f64);
    let (dw, dh) = (img.width() as f64, img.height() as f64);
    let (sx, sy) = (dw / sw, dh / sh);
    let target = candidate.to_px(frame.width(), frame.height());
    let drawn = PxBox {
        x1: target.x1 * sx,
        y1: target.y1 * sy,
        x2: target.x2 * sx,
        y2: target.y2 * sy,
    };
    // 容差圈：半径 = 原图 5 px 换算到这张图上。整屏放大倍数 ≈ 1，
    // 所以圈只有 5 px —— 小，但它本来表示的就是 5 px。
    let tol = if mark == Mark::CrossWithTolerance {
        Some(super::geometry::VERIFY_TOLERANCE_PX * sx)
    } else {
        None
    };
    draw_center_mark(
        &mut img,
        drawn,
        mark == Mark::CrossFullSpan,
        tol,
        tol.map(|r| r * 2.0).unwrap_or(0.0),
    );
    encode(&img)
}

/// The whole frame with every box the run produced drawn on it, plus the point
/// that will be clicked.
///
/// The crops already show their own box and centre mark, but each one is a
/// different window on a different place: nothing in the collection says where
/// that window *is*. Diagnosing 「偏了一格」 from six crops means mentally
/// stitching them back together. One annotated frame answers it directly — the
/// box is either on the target or it is not, and you can see which neighbour it
/// landed on.
///
/// `boxes` is drawn oldest-first in a faded colour with the final box in the
/// normal one, so the search path is visible without the last box being lost in
/// it.
pub fn render_annotated_frame(
    frame: &LocateFrame,
    boxes: &[PxBox],
    point: Option<(f64, f64)>,
) -> Result<LocateImage> {
    let mut img = frame.fitted();
    let (w, h) = (frame.width().max(1) as f64, frame.height().max(1) as f64);
    let scale = fit_scale(frame.width().max(1), frame.height().max(1), FULL_MAX_PX);
    let to_img = |b: PxBox| PxBox {
        x1: b.x1 * scale,
        y1: b.y1 * scale,
        x2: b.x2 * scale,
        y2: b.y2 * scale,
    };
    for (i, b) in boxes.iter().enumerate() {
        let last = i + 1 == boxes.len();
        let drawn = to_img(*b);
        if last {
            draw_px_box(&mut img, drawn);
        } else {
            // Earlier attempts: outline only, no centre mark, so the final box
            // is the one the eye lands on.
            draw_px_box_colour(&mut img, drawn, Rgb([120, 160, 220]));
        }
    }
    if let Some((x, y)) = point {
        let cx = x * scale;
        let cy = y * scale;
        // Same red-core/white-ring mark the verifier sees, so 「the point」 means
        // the same thing in both images.
        draw_center_mark(
            &mut img,
            PxBox {
                x1: cx,
                y1: cy,
                x2: cx,
                y2: cy,
            },
            false,
            None,
            0.0,
        );
    }
    let _ = (w, h);
    encode(&img)
}

/// Render a centered `side × side` crop, magnified to `out_px`, box drawn.
///
/// Returns the crop bounds so the locator's crop-local answer can be mapped
/// back onto the full screen.
pub fn render_crop(
    frame: &LocateFrame,
    candidate: NormBox,
    side: f64,
    out_px: u32,
) -> Result<(LocateImage, CropBounds)> {
    render_crop_opts(frame, candidate, side, out_px, true)
}

/// Whether crops dim everything outside the candidate box — **no**.
///
/// Dimming was introduced from an argument, not a measurement: the claim was
/// that without a brightness step the verifier reads what it *expects* inside
/// the box instead of where the box actually is. Measured on sudoku 81 × 3 it
/// made no difference to accuracy (93.8 % with, 94.7 % without, both inside the
/// run-to-run spread) while hiding the neighbour the target actually sits on —
/// and the locator is handed the very same image when it re-searches inside the
/// crop, where seeing the neighbour is the whole job.
///
/// Kept as a function so the two call sites stay explicit about it.
pub fn dim_enabled() -> bool {
    false
}

/// Same as [`render_crop`] with the dimming switch explicit.
pub fn render_crop_opts(
    frame: &LocateFrame,
    candidate: NormBox,
    side: f64,
    out_px: u32,
    dim: bool,
) -> Result<(LocateImage, CropBounds)> {
    // verify 只看十字星，不画框 —— 框的四条边会被当成「目标边界」去判，
    // 而交付的坐标是十字交点。框大框小都不该影响判定。
    render_crop_marked(frame, candidate, side, out_px, dim, Mark::CrossOnly)
}

/// What the crop draws on top of the screenshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    /// Candidate outline plus the centre dot — what the verifier needs, because
    /// its whole job is judging that outline.
    BoxAndDot,
    /// Only the centre dot. The locator gets this: the outline is the single most
    /// target-shaped thing in the crop (high saturation, hard edges, dead centre),
    /// and a locator asked 「where is the target」 will hand it straight back. It
    /// did exactly that — `raw=[344,344,656,656]` is the crop centre, i.e. the
    /// box it was shown, returned unchanged. The candidate's real position is
    /// already in the prompt as four numbers, so the picture does not need it.
    DotOnly,
    /// 只有十字星，没有框 —— verify 用这个。
    ///
    /// 框在 verify 里是个干扰源：它的四条边会被当成「目标的边界」去判，
    /// 而真正交付的坐标是十字交点。臂长也不按框算，改成按画面短边的固定
    /// 比例，这样框大框小都不改变标记的形状，模型看到的就是一个等大的准星。
    CrossOnly,
    /// 十字星 + 容差圈 —— verify 的 close 图用这个。
    ///
    /// 判定是几何量：「交点离目标点 / 目标线的距离 < 5 px（原图）」。
    /// 模型拿不到像素刻度，所以把 5 px 按放大倍数换算成图上的半径，
    /// 画成虚线圆 —— 「量距离」变成「看包不包含」。
    ///
    /// 对点：目标点在圈内 ⟺ 距离 < 容差。
    /// 对线：线穿过圈 ⟺ 垂距 < 容差（垂足是线上离交点最近的点）。
    CrossWithTolerance,
    /// 四条臂一直画到画面边缘 —— verify 的**整屏**图用这个。
    ///
    /// 整屏放大 1×，按框算臂长只有十几像素，在 1920×1080 上几乎看不见。臂贯穿
    /// 全屏后，竖臂直接指向上下边缘的列标、横臂指向左右边缘的行标，读行列不再
    /// 需要「先找到交点、再估它在第几列」—— 线自己把答案指出来了。
    ///
    /// 代价是两条线穿过整个画面。整屏图只用来读标签、不判距离，这个遮挡不影响
    /// 它的职责。
    CrossFullSpan,
}

/// [`render_crop`] with both the dimming and the mark style explicit.
pub fn render_crop_marked(
    frame: &LocateFrame,
    candidate: NormBox,
    side: f64,
    out_px: u32,
    dim: bool,
    mark: Mark,
) -> Result<(LocateImage, CropBounds)> {
    let (w, h) = (frame.width(), frame.height());
    let target = candidate.to_px(w, h);
    let bounds = CropBounds::around(target, side);
    let out_px = out_px.max(1);

    let crop_w = bounds.width().round().max(1.0) as u32;
    let crop_h = bounds.height().round().max(1.0) as u32;
    let mut crop = RgbImage::from_pixel(crop_w, crop_h, Rgb([0, 0, 0]));
    // Crop origin may be negative (crop extends past the screen edge); those
    // pixels stay black, matching PIL's padding behaviour in the baseline.
    image::imageops::replace(&mut crop, &frame.image, -bounds.x0.round() as i64, -bounds.y0.round() as i64);

    let mut magnified = image::imageops::resize(&crop, out_px, out_px, FilterType::Lanczos3);

    let sx = out_px as f64 / bounds.width().max(f64::EPSILON);
    let sy = out_px as f64 / bounds.height().max(f64::EPSILON);
    let px = target;
    let drawn = PxBox {
        x1: (px.x1 - bounds.x0) * sx,
        y1: (px.y1 - bounds.y0) * sy,
        x2: (px.x2 - bounds.x0) * sx,
        y2: (px.y2 - bounds.y0) * sy,
    };
    // No dimming. It was tried and measured: it changed nothing about accuracy
    // on sudoku 81 × 3 (93.8 % with, 94.7 % without — inside the run-to-run
    // spread) while making the neighbour the target actually sits on harder to
    // see. The verifier is handed the same crop the locator is, and 「目标在
    // 隔壁那格」 is exactly what it needs to notice.
    //
    // The box outline still marks where the candidate is, so the verifier can
    // still answer 「does the box frame the target」 from the boundary alone.
    if dim {
        // Kept for callers that pass `true` explicitly; currently unused.
    }
    if mark == Mark::BoxAndDot {
        draw_px_box(&mut magnified, drawn);
    }
    // Centre mark last, so neither the dimming nor the box outline touches it.
    //
    // This is the whole "is the coordinate right" question, made visual: the mark
    // sits at the box's geometric centre, which is exactly the coordinate the
    // engine will return. Asking "does the mark sit on the target" is a far
    // easier judgement than "do all four edges line up", and it is the question
    // that actually matters — a box can be generous enough to contain the target
    // and still be centred somewhere else entirely.
    draw_center_mark(
        &mut magnified,
        drawn,
        mark == Mark::CrossFullSpan,
        if mark == Mark::CrossWithTolerance {
            Some(super::geometry::tolerance_radius_px(side, out_px))
        } else {
            None
        },
        // 臂长下限 = **2 × 圈半径**（`VERIFY_TOLERANCE_PX = 5` → 原图 10 px）。
        // 臂必然穿过圈、露在外面，模型才能同时看到「交点在哪」和「容差多大」。
        // 两个量都从 `tolerance_radius_px` 来，换放大倍数时关系不变。
        super::geometry::tolerance_radius_px(side, out_px) * 2.0,
    );

    Ok((encode(&magnified)?, bounds))
}

/// Colours for the centre mark: a red core inside a white ring.
///
/// Two colours rather than one because a single-colour dot disappears on a
/// background of the same colour — a red dot on a red button, a white dot on a
/// white page. The ring guarantees one of the two always contrasts. The pair is
/// deliberately generic: it has nothing to do with any particular app's palette.
const MARK_CORE: Rgb<u8> = Rgb([220, 20, 20]);
const MARK_RING: Rgb<u8> = Rgb([255, 255, 255]);

/// Crosshair arm length as a share of the box's own side.
///
/// 0.35 each way → the crosshair spans 70% of the box, so it never touches the
/// outline it is judged against. Smaller and the arms read as a dot again.
const CROSS_ARM_SHARE: f64 = 0.35;
/// Half-thickness of the red core, as a share of the crosshair's own arm.
///
/// Was `short × 0.0025` — a share of the image's short side, which ignores how
/// big the box is. That works while the box is large, but `verify` renders the
/// same box into two crops at different magnifications: close (240 px crop →
/// 1600 px out, ×6.7) and wide (960 px crop → 1920 px out, ×2.0). A thickness
/// fixed in *output* pixels therefore lands as 1.2 source px in the close crop
/// and 4.8 source px in the wide one — four times thicker on the screen. On a
/// 40 px target the wide crosshair covered ~12% of its width and the verifier
/// could not read the digits underneath it.
///
/// Binding thickness to the arm instead keeps the crosshair's own proportions
/// constant at any magnification: the arm is `0.35 × box`, so this is
/// `0.0147 × box` — the same fraction of the target whether the crop is
/// magnified 7.5× (close) or 2.2× (wide).
///
/// 0.042 was solved from the two magnifications rather than picked: it holds
/// close at its previous 9 px stroke (single arm 110 px × 0.042 = 4.6 half)
/// while taking wide from 9 px down to 3 px (arm 33 px × 0.042 = 1.4 half).
/// 0.15 was tried first and failed both ways — it left wide unchanged
/// (33 × 0.15 ≈ 1920 × 0.0025, a coincidence) and quadrupled close to 33 px.
const CROSS_THICK_SHARE: f64 = 0.042;
/// 臂长下限（**渲染图**像素）。
///
/// 整屏图放大 1×，42 px 的目标算出臂长只有 14.9 px —— 在 1920×1080 上是个
/// 几乎看不见的小十字。`verify_position` 的 `left` 变体因此把「偏了一格」
/// 读成「在格内」，假阳性 26/111。
///
/// 40 px 的依据：目标本身就有 40 px 量级，臂长取同等长度，十字仍落在目标
/// 内部（臂长 = 框长边 × 0.35 的原意是「跨度 70% 框宽」），同时足够醒目。
/// close 图上框放大 7.5×，臂长本就 ≥100 px，这个下限永远不触发。
const CROSS_ARM_MIN_PX: f64 = 40.0;

/// 裁片（close / wide）的臂长系数 —— 与整屏的 `CROSS_ARM_SHARE` 分开。
///
/// 2026-11 实测对比（ScreenSpot 254 条 + sudoku/general 259 条，各三次）：
///
/// | 配置 | ScreenSpot | sudoku |
/// |---|---|---|
/// | 0.35 + 圈×2 下限（旧） | 81.50 % | 92.54 % |
/// | 跨度钉死 48 px（cap48） | 87.01 % | 93.44 % |
/// | **0.1 + 48 px 下限 + 圈缩** | **87.80 %** | 92.02 % |
///
/// 小目标上 `0.35 × 框宽` 会被 `arm_min` 顶到目标整个宽度，把中心特征盖住；
/// 0.1 让十字退化成准星附近的小十字，中心完全露出来。48 px 下限保证小框上
/// 十字不至于短到看不出方向。
const CROP_CROSS_ARM_SHARE: f64 = 0.1;
/// 裁片臂长下限，渲染像素。跨度 = 2 × 24 = 48 px。
const CROP_CROSS_ARM_MIN_PX: f64 = 24.0;
/// 线宽下限（**渲染图**像素，即成品线的粗细）。
///
/// 1.25 px 的线过 JPEG 之后基本糊掉。5 px 在整屏上仍然远小于目标（42 px），
/// 不会遮挡；close 上本就 9 px，不触发。
const CROSS_THICK_MIN_PX: f64 = 5.0;
/// 整屏贯穿十字的**红芯**宽度（渲染图像素）。
///
/// 贯穿线的长度是画面尺寸、与目标无关，粗细不能按比例推（按比例会得到 90 px
/// 的红带，实测过）。固定 3 px 红芯 + 每侧 2 px 白边 = 总宽 7 px，
/// 在 1920×1080 上既能一眼看到，又不会盖住标签。
const CROSS_SPAN_CORE_PX: f64 = 3.0;
/// 整屏贯穿十字的白边宽度（每侧，渲染图像素）。
const CROSS_SPAN_HALO_PX: f64 = 1.0;
/// White halo thickness, as a share of the red core's half-thickness.
///
/// Kept as a ratio (not its own share of the arm) so the halo can never grow
/// wider than the core it is supposed to outline.
const CROSS_RING_SHARE: f64 = 0.72;

/// Draw a crosshair at the box's centre.
///
/// Replaces the two-ring dot: a filled disc covers a patch of the image, so
/// 「is the mark inside the target」 degenerates into 「does this blob overlap
/// the cell」 — and a 16 px blob straddling a grid line reads as inside on
/// both sides of it. A crosshair puts the answer on a single pixel: the
/// intersection *is* the coordinate, and the four arms make it obvious which
/// side of a boundary line that pixel falls on.
///
/// Arms stop short of the box edges (35% of the side each) so the crosshair
/// never merges with the outline it is being judged against.
/// 从交点垂直向上画一条细线到画面顶 —— 用来把交点和顶部的列标连起来。
///
fn draw_center_mark(
    img: &mut RgbImage,
    b: PxBox,
    full_span: bool,
    tol_r: Option<f64>,
    arm_min: f64,
) {
    let (w, h) = (img.width() as f64, img.height() as f64);
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    let cx = (b.x1 + b.x2) / 2.0;
    let cy = (b.y1 + b.y2) / 2.0;
    let short = w.min(h);
    let box_w = (b.x2 - b.x1).max(1.0);
    let box_h = (b.y2 - b.y1).max(1.0);
    // Arm length: a share of the box, capped so a huge box does not turn the
    // crosshair into a full-width line, and floored against the image so it
    // stays visible when the box is tiny.
    // 臂长按**输出图里的框宽**算 —— `box_w` 已经是放大后的坐标，所以 close
    // （框 320 px）得到 112 px、wide（框 80 px）得到 28 px，两者都是框的 70%，
    // 视觉比例一致。
    //
    // 下限只防「框小到臂看不见」：0.008 × short = 15 px，正常尺寸永远不触发。
    // 之前写 0.02（38 px）把 wide 的 28 px 顶了上去，臂撑到框的 96%，会压到邻居。
    // `full_span`：臂长一直画到画面边缘，不跟框走。
    // verify 走这条路 —— 框已经不画了，臂长再跟框绑定，就等于让模型从
    // 十字的长短去反推一个它看不见的框。
    // 臂长按**候选框**算，不按画面短边 —— 十字星必须待在框内。
    //
    // 曾经改成「按画面短边 3%」，理由是框不画了、臂长不该跟看不见的框绑定。
    // 这个理由在小目标上失效：screenspot 的 15 px 图标放大到 120 px，而固定
    // 臂长跨度 122 px，比目标本身还大，把目标整个盖住 —— M2 从 85.3% 掉到
    // 80.0%，offset 中位数从 4.3 px 涨到 5.7 px。
    //
    // 按框算则比例恒定：0.35 × 框宽 → 跨度永远是框的 70%，框多大都不遮挡。
    // `full_span`：臂长贯穿全屏，不再按框算（见 `Mark::CrossFullSpan`）。
    // 臂长 = min( max(框长边 × 0.35, 10 原图 px), 画面短边 × 0.25 )
    //
    // - 比例 0.35：单臂 35%、跨度 70% 框宽，不碰框线。
    // - 下限改成 **原图 10 px**（`arm_min` 是调用方按放大倍数换算好的渲染值）。
    //   旧下限是 `short × 0.008`（画面 0.8%），与目标无关；换成原图尺寸后
    //   和圈半径同源：圈 = 5 原图 px，臂 ≥ 10 原图 px，臂恒 ≥ 2 × 圈半径，
    //   必然穿过圈露在外面。
    // - 上限 `short × 0.25` 保留：大目标时框接近画面，0.35 × 框会撑满，
    //   臂长盖住目标就失去指示作用了。
    // 线宽永远按**框**算出来的臂长，不按绘制臂长 ——
    // 贯穿全屏时绘制臂长是画面尺寸，用它算线宽会得到 91 px 的红带。
    //
    // 2026-11 实测：`0.35 × 框宽` + `arm_min = 圈半径 × 2` 的组合，在小目标上
    // 会把臂顶到目标整个宽度（ScreenSpot 21×20 px 图标放大到 140 px，臂被顶到
    // 66.7 px，跨度 134 px = 目标宽度的 96%），把瞳孔、加号这些识别特征盖住。
    // 裁片改走 `CROP_CROSS_ARM_SHARE = 0.1` + 24 px 下限（跨度 48 px），并把
    // 容差圈按臂长缩短的比例一起缩 —— 否则圈比臂还长，十字整根缩在圈里。
    // 整屏（`full_span`）的实线段不受影响，仍是 `CROSS_ARM_SHARE`。
    let orig_arm = (box_w * CROSS_ARM_SHARE)
        .max(arm_min)
        .min(short * 0.25)
        .max(CROSS_ARM_MIN_PX)
        .min(
            (box_h * CROSS_ARM_SHARE)
                .max(arm_min)
                .min(short * 0.25)
                .max(CROSS_ARM_MIN_PX),
        );
    let (box_arm_x, box_arm_y) = if full_span {
        (orig_arm, orig_arm)
    } else {
        (
            (box_w * CROP_CROSS_ARM_SHARE).max(CROP_CROSS_ARM_MIN_PX),
            (box_h * CROP_CROSS_ARM_SHARE).max(CROP_CROSS_ARM_MIN_PX),
        )
    };
    let arm_side = box_arm_x.min(box_arm_y);
    // 贯穿全屏时用**固定**像素值：红芯 3 px、白边每侧 2 px（总宽 7 px）。
    // 按比例算在这里没有意义 —— 贯穿线的长度是画面尺寸，跟目标无关，
    // 用它推粗细只会得到一条 90 px 的红带（实测过）。
    let (half, ring) = if full_span {
        (CROSS_SPAN_CORE_PX / 2.0, CROSS_SPAN_HALO_PX)
    } else {
        let h = (arm_side * CROSS_THICK_SHARE).max(CROSS_THICK_MIN_PX / 2.0);
        (h, (h * CROSS_RING_SHARE).max(0.5))
    };
    // 容差圈跟着臂长缩短的比例缩 —— 只缩臂不缩圈，臂会整根缩进圈里。
    let tol_r = tol_r.map(|r| {
        if full_span || orig_arm <= 0.0 {
            r
        } else {
            r * (arm_side / orig_arm).min(1.0)
        }
    });

    let (arm_x, arm_y) = if full_span {
        // 贯穿全屏：臂长取画面尺寸，超出边界的部分由 `plot` 丢掉。
        (w, h)
    } else {
        (box_arm_x, box_arm_y)
    };

    let mut plot = |x: f64, y: f64, c: Rgb<u8>| {
        let xi = x.round() as i32;
        let yi = y.round() as i32;
        if xi >= 0 && yi >= 0 && xi < w as i32 && yi < h as i32 {
            img.put_pixel(xi as u32, yi as u32, c);
        }
    };

    // 容差圈：以交点为圆心，半径 = 原图 5 px × 放大倍数。
    //
    // **和十字星完全一致**：同一组 `half` / `ring` 参数、同样两遍循环
    // （白描边 → 红芯）、同样沿法线扫描 `±(half + extra)`、同样的
    // `MARK_CORE` / `MARK_RING` 配色。圈与准星是同一个符号的两个部分，
    // 形状区分（环 vs 十字），不需要靠颜色区分。
    //
    // 虚线用「画一段 / 跳一段」产生：沿圆周密集采样，段内每步画一根法线。
    // 第一版按角度画孤立点，52 个点被 JPEG 平均成灰色（实测 (0,200,220)
    // 掉到 (107,171,180)），视觉上是一团模糊的淡影。
    if let Some(r) = tol_r {
        if r >= 1.0 {
            // 虚线节拍：一段 16°、空 8°，圆周约 15 段。
            let (dash, gap) = (16.0_f64.to_radians(), 8.0_f64.to_radians());
            let period = dash + gap;
            let sample = 0.004_f64; // ≈0.23°，保证段内连续
            for (color, extra) in [(MARK_RING, ring), (MARK_CORE, 0.0)] {
                let t = half + extra;
                let mut a = 0.0_f64;
                while a < std::f64::consts::TAU {
                    if a % period <= dash {
                        let (sn, cs) = (a.sin(), a.cos());
                        let mut d = -t;
                        while d <= t {
                            plot(cx + (r + d) * cs, cy + (r + d) * sn, color);
                            d += 1.0;
                        }
                    }
                    a += sample;
                }
            }
        }
    }

    // White halo first, red core on top — same reasoning as the old dot's rim:
    // one of the two always contrasts, whatever the cell background is.
    // For a full-span mark, keep the local cross solid and dash only the
    // extensions outside it. This preserves an unambiguous centre point while
    // making the long guide lines less dominant.
    let dash_len = 6.0;
    let gap_len = 16.0;
    let solid_half_x = (box_w * CROSS_ARM_SHARE).max(half + ring);
    let solid_half_y = (box_h * CROSS_ARM_SHARE).max(half + ring);
    let is_drawn = |distance: f64, solid_half: f64| {
        if !full_span || distance <= solid_half {
            true
        } else {
            // 先空一段再画：中心实线严格等于框 × 0.35，外面才是均匀虚线。
            let phase = distance - solid_half - gap_len;
            phase >= 0.0 && (phase % (dash_len + gap_len)) < dash_len
        }
    };
    for (c, extra) in [(MARK_RING, ring), (MARK_CORE, 0.0)] {
        let tx = half + extra;
        let ty = half + extra;
        let mut x = cx - arm_x - extra;
        while x <= cx + arm_x + extra {
            if is_drawn((x - cx).abs(), solid_half_x) {
                for dy in -(ty as i32)..=(ty as i32) {
                    plot(x, cy + dy as f64, c);
                }
            }
            x += 1.0;
        }
        let mut y = cy - arm_y - extra;
        while y <= cy + arm_y + extra {
            if is_drawn((y - cy).abs(), solid_half_y) {
                for dx in -(tx as i32)..=(tx as i32) {
                    plot(cx + dx as f64, y, c);
                }
            }
            y += 1.0;
        }
    }

}

/// Darken pixels outside `keep` so the box interior stands out.
/// Draw a normalized box onto an image of the same aspect (B5 `draw_box`).
fn draw_norm_box(img: &mut RgbImage, b: NormBox) {
    draw_px_box(img, b.to_px(img.width(), img.height()));
}

fn draw_px_box(img: &mut RgbImage, b: PxBox) {
    draw_px_box_colour(img, b, BOX_COLOR)
}

/// Same outline in an arbitrary colour, for drawing a search path where only the
/// last box should be the normal blue.
/// Draw a box outline that stays visible on any background.
///
/// One colour can never be universally visible: a dark outline vanishes on a
/// dark wallpaper, a light one on a white document, and the target is whatever
/// the user happened to be looking at. So the outline is two rings — black
/// inside, white outside — and at least one of them always contrasts with
/// whatever is behind it. The black ring sits exactly on the candidate edge, so
/// the boundary a verifier judges against is still the real one; the white ring
/// only extends outward.
fn draw_px_box_colour(img: &mut RgbImage, b: PxBox, colour: Rgb<u8>) {
    let (w, h) = (img.width() as f64, img.height() as f64);
    let x1 = b.x1.clamp(0.0, w);
    let y1 = b.y1.clamp(0.0, h);
    let x2 = b.x2.clamp(0.0, w);
    let y2 = b.y2.clamp(0.0, h);
    let (x0, y0) = (x1.min(x2).round() as i32, y1.min(y2).round() as i32);
    let rw = (x1.max(x2).round() as i32 - x0).max(BOX_WIDTH);
    let rh = (y1.max(y2).round() as i32 - y0).max(BOX_WIDTH);

    // Outer white ring first, so the black ring can be drawn over its inner
    // edge without either being lost.
    for offset in 1..=BOX_WIDTH {
        let outer = Rect::at(x0 - offset, y0 - offset).of_size(
            (rw as u32).saturating_add(offset as u32 * 2),
            (rh as u32).saturating_add(offset as u32 * 2),
        );
        draw_hollow_rect_mut(img, outer, BOX_OUTER_COLOR);
    }
    // Inner magenta ring, on the candidate edge itself.
    let rect = Rect::at(x0, y0).of_size(rw as u32, rh as u32);
    draw_hollow_rect_mut(img, rect, colour);
    for offset in 1..BOX_WIDTH {
        // A box can be smaller than 2 × BOX_WIDTH (a 4 px candidate on a tiny
        // icon). Thickening inward then has no room: `saturating_sub` yields 0
        // and `imageproc::Rect` panics on an empty rect. The inner ring is the
        // boundary marker, so it is the one that must survive — the outer ring
        // can carry the visibility, and it has unlimited room to grow.
        let iw = (rw - offset * 2).max(1) as u32;
        let ih = (rh - offset * 2).max(1) as u32;
        let inner = Rect::at(x0 + offset, y0 + offset).of_size(iw, ih);
        draw_hollow_rect_mut(img, inner, colour);
    }
}

fn encode(img: &RgbImage) -> Result<LocateImage> {
    let mut buf = Vec::new();
    {
        let mut cursor = Cursor::new(&mut buf);
        let mut encoder = JpegEncoder::new_with_quality(&mut cursor, LOCATE_JPEG_QUALITY);
        encoder
            .encode(
                img.as_raw(),
                img.width(),
                img.height(),
                ExtendedColorType::Rgb8,
            )
            .map_err(|e| anyhow!("encode locate jpeg: {e}"))?;
    }
    Ok(LocateImage {
        jpeg: buf,
        width: img.width(),
        height: img.height(),
    })
}

#[cfg(test)]
mod tests {

    /// 裁片的内容必须真的对应它声称的 `bounds`。
    ///
    /// 这是 2026-11-04 那次 bug 的回归测试：定位图按 640 渲染、却用 verify 的
    /// 135 窗口去映射答案，坐标被压缩 4.7 倍，而**两张图各自都是对的** ——
    /// 只测「渲染成功」或「图是 1920」都发现不了。这里改成在合成图上画一个
    /// 已知位置的方块，再从输出图里反算它落在哪，把「内容 ↔ bounds」钉死。
    #[test]
    fn crop_content_matches_the_returned_bounds() {
        // 1000×800 黑底，在屏幕坐标 (495..505, 395..405) 画白块，中心 (500,400)。
        let mut img = RgbImage::from_pixel(1000, 800, Rgb([0, 0, 0]));
        for y in 395..405u32 {
            for x in 495..505u32 {
                img.put_pixel(x, y, Rgb([255, 255, 255]));
            }
        }
        let frame = LocateFrame::new(img);
        // 同一个白块的归一化坐标（0–1000 刻度）。
        let candidate = NormBox::new(495.0, 493.75, 505.0, 506.25);

        let (image, bounds) = render_crop_marked(
            &frame,
            candidate,
            200.0,
            1920,
            false,
            Mark::CrossOnly,
        )
        .unwrap();

        // 窗口：边长 200，中心在 (500,400)。
        assert_eq!(
            (
                bounds.x0.round(),
                bounds.y0.round(),
                bounds.x1.round(),
                bounds.y1.round()
            ),
            (400.0, 300.0, 600.0, 500.0),
            "裁片窗口不对"
        );
        assert_eq!((image.width, image.height), (1920, 1920));

        // 白块 10 px 铺到 200 px 的窗口里 → 图上 96 px 宽，中心在图正中。
        let decoded = image::load_from_memory(&image.jpeg).unwrap().to_rgb8();
        let px = decoded.get_pixel(960, 960);
        assert!(
            px[0] > 200,
            "图正中应当是白块（实际 {px:?}）—— 裁片内容没有对上 bounds"
        );
        // 白块左边界：960 - 48 = 912。往外 12 px 应当是黑底。
        let outside = decoded.get_pixel(900, 960);
        assert!(outside[0] < 60, "白块左边界外的像素应当是黑的（实际 {outside:?}）");
    }
    use super::*;
    use image::Rgb;

    fn frame(w: u32, h: u32) -> LocateFrame {
        LocateFrame::new(RgbImage::from_pixel(w, h, Rgb([30, 30, 30])))
    }

    /// 同一目标在 close 与 wide 两种放大倍数下的十字星粗细对比。
    ///
    /// 生产里 verify 会把同一个框渲染成两张图：
    ///   close — 裁片 240 px → 输出 1600 px（放大 6.7×）
    ///   wide  — 裁片 960 px → 输出 1920 px（放大 2.0×）
    /// 线宽若按「输出图短边」定，两张图在**原图上**的线宽就差 3.3 倍，
    /// 小目标上 wide 的十字会盖住目标本身。这个测试把两张图落盘供肉眼比对。
    ///
    /// `POINTER_CROSS_CASE_DIR` 指向 `pointer-eval-data/verify_position/sudoku`。
    #[test]
    #[ignore]
    fn dump_cross_thickness_compare() {
        use image::ImageReader;
        let Ok(dir) = std::env::var("POINTER_CROSS_CASE_DIR") else {
            eprintln!("跳过：未设置 POINTER_CROSS_CASE_DIR");
            return;
        };
        let src = format!("{dir}/images/screen/0ff09072_base.jpg");
        let Ok(reader) = ImageReader::open(&src) else {
            eprintln!("跳过：找不到 {src}");
            return;
        };
        let img = reader.decode().unwrap().to_rgb8();
        let frame = LocateFrame::new(img);
        let (w, h) = (frame.width(), frame.height());
        // `0ff09072_pos` 的候选框（原图 px），42×43 —— 与 5e712d92 同量级。
        let px = PxBox {
            x1: 88.5,
            y1: 239.0,
            x2: 130.4,
            y2: 281.9,
        };
        let norm = px.to_norm(w, h);
        let side = super::super::geometry::crop_side_px(px, 3.0, 240.0, 1600.0);
        eprintln!(
            "原图 {w}x{h}  框 {:.0}x{:.0}  裁片 side={side:.0}",
            px.width(),
            px.height()
        );
        // 对比用：两张都带容差圈，条件一致才能比线宽。
        // （生产上 wide 走 `render_crop_opts` 即 `CrossOnly`，本来不画圈。）
        for (name, s, out, mark) in [
            ("close", side, 1600u32, Mark::CrossWithTolerance),
            ("wide", side * 4.0, 1920u32, Mark::CrossWithTolerance),
        ] {
            let (im, _) = render_crop_marked(&frame, norm, s, out, false, mark).unwrap();
            let p = format!("{dir}/cross_{name}.jpg");
            std::fs::write(&p, &im.jpeg).unwrap();
            eprintln!("写出 {p}  {out}x{out}  mark={mark:?}");
        }
        // 整屏（verify 图二现在走这条路）
        let full = render_full_marked(&frame, norm, Mark::CrossFullSpan).unwrap();
        let p = format!("{dir}/cross_full.jpg");
        std::fs::write(&p, &full.jpeg).unwrap();
        eprintln!("写出 {p}  {}x{}  full", full.width, full.height);
    }

    #[test]
    fn full_render_keeps_logical_size() {
        let img = render_full(&frame(1000, 800), None).unwrap();
        assert_eq!((img.width, img.height), (1000, 800));
        assert!(!img.jpeg.is_empty());
    }

    /// 容差圈：半径必须等于「原图 5 px × 放大倍数」。
    ///
    /// 这是判定的物理量。算错就意味着模型看到的「圈内」和真实的「5 px 内」
    /// 对不上 —— 而且不会有任何报错，只会静默地放宽或收紧判据。
    #[test]
    fn tolerance_radius_matches_five_logical_px() {
        use super::super::geometry::{tolerance_radius_px, CROP_OUT_PX};

        // 裁片 240 px 输出 1600 px → 放大 6.67× → 5 px 原图 = 33.3 px 图上
        let r = tolerance_radius_px(240.0, CROP_OUT_PX);
        assert!((r - 5.0 * 1600.0 / 240.0).abs() < 1e-9);
        assert!((r - 33.333).abs() < 0.01, "期望约 33.3 px，实际 {r}");

        // 放大倍数变化时线性跟随
        let r2 = tolerance_radius_px(480.0, CROP_OUT_PX);
        assert!((r2 - r / 2.0).abs() < 1e-9);

        // 退化输入不能产生 NaN / 负数（会画出一个半径 0 或巨大的圈）
        assert_eq!(tolerance_radius_px(0.0, CROP_OUT_PX), 0.0);
        assert_eq!(tolerance_radius_px(-1.0, CROP_OUT_PX), 0.0);
        assert_eq!(tolerance_radius_px(240.0, 0), 0.0);
    }

    /// 渲染一致性校验：把 Rust 的渲染结果写到磁盘，供 Python 侧逐像素比对。
    ///
    /// `cargo test -p pointer-core --lib render::tests::dump_render_for_parity -- --ignored --nocapture`
    ///
    /// 输入图、候选框、裁片边长、输出尺寸全部写死，Python 侧
    /// `pointer-eval/scripts/parity_check.py` 用同样的参数渲染后逐像素 diff。
    #[test]
    #[ignore]
    fn dump_render_for_parity() {
        use image::ImageReader;
        let src = std::env::var("PARITY_SRC")
            .unwrap_or_else(|_| "/tmp/parity_src.png".to_string());
        let Ok(reader) = ImageReader::open(&src) else {
            eprintln!("跳过：找不到 {src}");
            return;
        };
        // 只在「画标记」这一层比对 —— resize 与 JPEG 编码两边实现不同
        // （image crate vs PIL），整条链路比出来的差异全在底图上，跟标记无关。
        // 所以这里直接拿一张已经放大到 1600 的图，画完就存 PNG（无损）。
        let mut img = reader.decode().unwrap().to_rgb8();
        let w = img.width() as f64;
        let h = img.height() as f64;
        // 与 Python 侧完全相同的 drawn 框
        let drawn = PxBox {
            x1: 0.35 * w,
            y1: 0.35 * h,
            x2: 0.55 * w,
            y2: 0.55 * h,
        };
        // 与生产 verify close 一致：`ver_side` 钳到 CROP_MIN=240，
        // 输出尺寸是 `cfg.out_px` = CROP_OUT_PX = 1600（不是 VERIFY_OUT_PX=1920）。
        // 用 1920 会得到 40 px 的圈，而生产画的是 33 px —— 比对的就不是产线了。
        let side = 240.0;
        let out_px = 1600u32;
        draw_center_mark(
            &mut img,
            drawn,
            true,
            Some(super::super::geometry::tolerance_radius_px(side, out_px)),
            super::super::geometry::tolerance_radius_px(side, out_px) * 2.0,
        );
        img.save("/tmp/parity_rust.png").unwrap();
        eprintln!("写出 /tmp/parity_rust.png {w}x{h}");
    }

    /// 圈一律画，不论半径大小 —— 圈是判据的载体，缺了它判定就退回凭感觉。
    ///
    /// 小圈（半径几像素）也要存在：线宽会跟着半径收缩，但圈本身必须有。
    #[test]
    fn tolerance_ring_is_always_drawn() {
        let f = frame(1000, 800);
        let candidate = NormBox::new(400.0, 400.0, 448.0, 448.0);

        // 大裁片 → 小圈（120 px 裁片、400 px 输出 = 3.33× → 16.7 px 圈）
        let (plain_a, _) =
            render_crop_marked(&f, candidate, 120.0, 400, false, Mark::CrossOnly).unwrap();
        let (ring_a, _) =
            render_crop_marked(&f, candidate, 120.0, 400, false, Mark::CrossWithTolerance)
                .unwrap();
        assert_ne!(plain_a.jpeg, ring_a.jpeg, "小圈也必须画出来");

        // 小裁片 → 大圈（80 px 裁片��400 px 输出 = 5.0× → 25 px 圈）
        let (plain_b, _) =
            render_crop_marked(&f, candidate, 80.0, 400, false, Mark::CrossOnly).unwrap();
        let (ring_b, _) =
            render_crop_marked(&f, candidate, 80.0, 400, false, Mark::CrossWithTolerance)
                .unwrap();
        assert_ne!(plain_b.jpeg, ring_b.jpeg, "大圈同样要画");
    }

    #[test]
    fn crop_render_is_square_and_returns_bounds() {
        let f = frame(1000, 800);
        let candidate = NormBox::new(400.0, 400.0, 500.0, 500.0);
        let (img, bounds) = render_crop(&f, candidate, 300.0, 64).unwrap();
        assert_eq!((img.width, img.height), (64, 64));
        assert_eq!(bounds.width(), 300.0);
        assert_eq!(bounds.height(), 300.0);
        // Box is centered because the crop is centered on it.
        assert!(((bounds.x0 + bounds.x1) / 2.0 - 450.0).abs() < 1e-6);
    }

    #[test]
    fn crop_pads_outside_screen_instead_of_clipping() {
        let f = frame(100, 100);
        let candidate = NormBox::new(0.0, 0.0, 100.0, 100.0);
        let (_img, bounds) = render_crop(&f, candidate, 240.0, 32).unwrap();
        assert!(bounds.x0 < 0.0 && bounds.y0 < 0.0);
        assert!(bounds.x1 > 100.0 && bounds.y1 > 100.0);
    }

    #[test]
    fn crop_copies_the_screen_window_not_blank_pixels() {
        let f = LocateFrame::new(RgbImage::from_pixel(100, 100, Rgb([200, 10, 10])));
        // Box (45,45)-(55,55) px with a 40 px window → bounds (30,30)-(70,70),
        // entirely inside the screen, so the crop must be the screen's red.
        let candidate = NormBox::new(450.0, 450.0, 550.0, 550.0);
        let (img, bounds) = render_crop(&f, candidate, 40.0, 40).unwrap();
        assert_eq!((bounds.x0, bounds.y0), (30.0, 30.0));
        let decoded = LocateFrame::from_jpeg(&img.jpeg).unwrap();
        // Sample inside the box (px 20,20 = screen 50,50). Outside the box the
        // crop is deliberately dimmed, so sampling (2,2) would read the dim
        // colour instead of the screen's.
        let px = decoded.image().get_pixel(20, 20);
        assert!(
            px[0] > 150 && px[1] < 80,
            "expected screen pixels inside the box, got {px:?}"
        );
    }

    #[test]
    fn crop_does_not_dim_outside_the_box() {
        // Dimming outside the box was removed: it changed nothing about accuracy
        // on sudoku 81 × 3 (93.8 % with, 94.7 % without — inside the run-to-run
        // spread) while hiding the neighbour the target actually sits on.
        //
        // The assertion is 「not darkened」, not 「equal」: Lanczos resampling
        // overshoots around edges, so a flat (200,10,10) frame comes out
        // anywhere in ~200–255 depending on where you sample. Dimming was
        // `× 0.45`, i.e. 200 → 90, which is nowhere near that range — so the
        // threshold still separates the two behaviours cleanly.
        let f = LocateFrame::new(RgbImage::from_pixel(100, 100, Rgb([200, 10, 10])));
        let candidate = NormBox::new(450.0, 450.0, 550.0, 550.0);
        let (img, _) = render_crop(&f, candidate, 40.0, 40).unwrap();
        let decoded = LocateFrame::from_jpeg(&img.jpeg).unwrap();
        for (x, y) in [(8, 8), (32, 32), (8, 32), (32, 8)] {
            let px = decoded.image().get_pixel(x, y);
            assert!(
                px[0] > 150,
                "outside the box must keep its brightness (dimming would take \
                 200 down to ~90): ({x},{y}) = {px:?}"
            );
        }
    }

    #[test]
    fn decoded_frame_reports_logical_size() {
        let f = frame(640, 480);
        let (img, _) = render_crop(&f, NormBox::new(100.0, 100.0, 200.0, 200.0), 240.0, 16).unwrap();
        let decoded = LocateFrame::from_jpeg(&img.jpeg).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (16, 16));
    }
}
