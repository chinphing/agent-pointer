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
        matches!(mark, Mark::CrossOnly),
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
/// Half-thickness of the red core, as a share of the image's short side.
///
/// 0.0015 → 2.9 px in a 1920 px crop, 0.36 px on screen. Thin on purpose: the
/// crosshair has to be able to sit between two grid lines one pixel apart
/// without covering either.
const CROSS_THICK_RATIO: f64 = 0.0025;
/// Extra thickness added by the white halo on each side.
const CROSS_RING_RATIO: f64 = 0.0018;

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
fn draw_center_mark(img: &mut RgbImage, b: PxBox, fixed_arms: bool) {
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
    // `fixed_arms`：臂长不跟框走，按画面短边的固定比例。
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
    let _ = fixed_arms;
    let arm_floor = short * 0.008;
    let (arm_x, arm_y) = (
        (box_w * CROSS_ARM_SHARE).max(arm_floor).min(short * 0.25).max(4.0),
        (box_h * CROSS_ARM_SHARE).max(arm_floor).min(short * 0.25).max(4.0),
    );
    let half = (short * CROSS_THICK_RATIO).max(1.0);
    let ring = (short * CROSS_RING_RATIO).max(0.5);

    let mut plot = |x: f64, y: f64, c: Rgb<u8>| {
        let xi = x.round() as i32;
        let yi = y.round() as i32;
        if xi >= 0 && yi >= 0 && xi < w as i32 && yi < h as i32 {
            img.put_pixel(xi as u32, yi as u32, c);
        }
    };

    // White halo first, red core on top — same reasoning as the old dot's rim:
    // one of the two always contrasts, whatever the cell background is.
    for (c, extra) in [(MARK_RING, ring), (MARK_CORE, 0.0)] {
        let tx = half + extra;
        let ty = half + extra;
        let mut x = cx - arm_x - extra;
        while x <= cx + arm_x + extra {
            for dy in -(ty as i32)..=(ty as i32) {
                plot(x, cy + dy as f64, c);
            }
            x += 1.0;
        }
        let mut y = cy - arm_y - extra;
        while y <= cy + arm_y + extra {
            for dx in -(tx as i32)..=(tx as i32) {
                plot(cx + dx as f64, y, c);
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
    use super::*;
    use image::Rgb;

    fn frame(w: u32, h: u32) -> LocateFrame {
        LocateFrame::new(RgbImage::from_pixel(w, h, Rgb([30, 30, 30])))
    }

    #[test]
    fn full_render_keeps_logical_size() {
        let img = render_full(&frame(1000, 800), None).unwrap();
        assert_eq!((img.width, img.height), (1000, 800));
        assert!(!img.jpeg.is_empty());
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
