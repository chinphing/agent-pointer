//! Pointer-style screen overlays (Python `screen_overlay.py` analogue): system-style pointer bitmap
//! (`assets/cursor-pointer.svg` path, same as Python) and I-beam caret (`draw_focus_caret_overlay`),
//! then zoom crops from the **marked** annotated image.
//!
//! Vision slot labels match the bracket captions in Python `extensions/message_loop_prompts_after/_10_computer_screen_inject.py`
//! (`[Screen before action]`, `[Screen after action]`, `[Annotated after action]`, `[Zoom top after action]`, …).

use super::screen::{self, MonitorInfo};
use crate::agents::computer::tier::ComputerTier;
use anyhow::{anyhow, Result};
use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use imageproc::drawing::{draw_line_segment_mut, draw_polygon_mut};
use imageproc::point::Point;
use std::io::Cursor;
use std::sync::OnceLock;

/// Full screenshot from the **prior** observation: **unmarked** capture from the previous turn with the **current** synthetic pointer drawn on it (pre-action desktop layout; pointer shows where the cursor is **now**).
pub const SLOT_SCREEN_BEFORE_ACTION: &str = "[Screen before action]";
/// Full screenshot from **this** observation (unmarked capture; no synthetic pointer overlay).
pub const SLOT_SCREEN_AFTER_ACTION: &str = "[Screen after action]";
/// Same moment as `[Screen after action]` with synthetic pointer (and caret if focused) drawn.
pub const SLOT_SCREEN_MARKED_AFTER_ACTION: &str = "[Marked screen after action]";
/// Numbered overlay on the **after action** desktop (same moment as `[Screen after action]`).
pub const SLOT_SCREEN_ANNOTATED: &str = "[Annotated after action]";
/// Top bar / chrome strip (after-action annotated frame).
pub const SLOT_SCREEN_ZOOMED_TOP: &str = "[Zoom top after action]";
/// Bottom bar / dock strip (after-action annotated frame).
pub const SLOT_SCREEN_ZOOMED_BOTTOM: &str = "[Zoom bottom after action]";
/// Pointer vicinity patch (after-action annotated frame).
pub const SLOT_SCREEN_ZOOMED_POINTER: &str = "[Zoom pointer after action]";
/// **4×** magnified **200×200 px** crop (±**100 px** radius) around the pointer on **`[Screen before action]`** — **authoritative** for **`Pointer:`** hotspot geometry.
pub const SLOT_SCREEN_ZOOMED_POINTER_BEFORE: &str = "[Zoom pointer before action]";

/// When false, skip drawing the synthetic pointer bitmap (for A/B against OS cursor in capture).
pub const DRAW_SYNTHETIC_POINTER_OVERLAY: bool = true;

const ZOOM_MENU_H: u32 = 100;
const ZOOM_TASK_H: u32 = 100;

/// Pointer vicinity crop side (before 4× magnification) for after- and before-action zoom slots.
pub const ZOOM_POINTER_CROP_SIDE: u32 = 200;
/// Nearest-neighbor upscale for `[Zoom pointer after action]` and `[Zoom pointer before action]`.
pub const ZOOM_POINTER_MAGNIFY_FACTOR: u32 = 4;

/// Half-width of the before-action pointer zoom crop in **capture pixels** (full crop = **2 × radius**).
pub const BEFORE_POINTER_ZOOM_RADIUS_PX: u32 = ZOOM_POINTER_CROP_SIDE / 2;
/// Crop side length on **`[Screen before action]`** before magnification.
pub const BEFORE_POINTER_ZOOM_CROP_SIDE: u32 = ZOOM_POINTER_CROP_SIDE;
/// Nearest-neighbor upscale applied to the before-action pointer crop.
pub const BEFORE_POINTER_ZOOM_FACTOR: u32 = ZOOM_POINTER_MAGNIFY_FACTOR;

const ACCENT_POINTER_BEFORE: Rgba<u8> = Rgba([160, 80, 220, 255]);

/// Raster size / hotspot — matches Python `screen_overlay.py` (`_CURSOR_SIZE`, `_CURSOR_HOTSPOT`).
const POINTER_CURSOR_SIZE: u32 = 32;
const POINTER_HOTSPOT: (i32, i32) = (3, 2);

const ACCENT_MENU: Rgba<u8> = Rgba([220, 60, 60, 255]);
const ACCENT_TASK: Rgba<u8> = Rgba([60, 200, 80, 255]);
const ACCENT_POINTER: Rgba<u8> = Rgba([60, 120, 220, 255]);

/// Which overlay assets to build for a tier (skip unused work).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisionOverlayWork {
    pub raw_marked_jpeg: bool,
    pub zoom_crops: bool,
}

impl VisionOverlayWork {
    pub const FULL: Self = Self {
        raw_marked_jpeg: true,
        zoom_crops: true,
    };

    pub fn for_tier(tier: ComputerTier) -> Self {
        match tier {
            ComputerTier::Primary => Self {
                // Primary 3.1 needs `[Screen after action]` (raw marked) + annotated.
                raw_marked_jpeg: true,
                zoom_crops: false,
            },
            ComputerTier::Intermediate => Self {
                raw_marked_jpeg: true,
                zoom_crops: false,
            },
            ComputerTier::Advanced => Self::FULL,
        }
    }
}

/// All extra vision assets for one `[CUR_SCREEN]` turn.
#[derive(Debug, Clone)]
pub struct VisionOverlayPack {
    pub raw_marked_jpeg: Vec<u8>,
    pub annotated_marked_jpeg: Vec<u8>,
    pub zoom_menu_bar_png: Vec<u8>,
    pub zoom_task_bar_png: Vec<u8>,
    pub zoom_pointer_png: Vec<u8>,
}

/// Global screen coordinates → bitmap-local (same space as annotate / vision_state).
pub fn global_pointer_to_local(global_x: i32, global_y: i32, monitor: &MonitorInfo) -> (i32, i32) {
    (
        global_x - monitor.left,
        global_y - monitor.top,
    )
}

fn local_on_monitor(global_x: i32, global_y: i32, monitor: &MonitorInfo) -> (i32, i32) {
    (global_x - monitor.left, global_y - monitor.top)
}

/// `Some` only when the point lies inside the capture bitmap (Python inject uses the same bounds checks).
fn overlay_position_if_inside(lx: i32, ly: i32, iw: u32, ih: u32) -> Option<(i32, i32)> {
    let w = iw as i32;
    let h = ih as i32;
    if lx >= 0 && ly >= 0 && lx < w && ly < h {
        Some((lx, ly))
    } else {
        None
    }
}

fn set_px(img: &mut RgbaImage, x: i32, y: i32, c: Rgba<u8>) {
    let w = img.width() as i32;
    let h = img.height() as i32;
    if x >= 0 && y >= 0 && x < w && y < h {
        img.put_pixel(x as u32, y as u32, c);
    }
}

/// Vertices match Python fallback polygon / `cursor-pointer.svg` path.
fn pointer_cursor_rgba() -> &'static RgbaImage {
    static CUR: OnceLock<RgbaImage> = OnceLock::new();
    CUR.get_or_init(|| {
        let mut buf = RgbaImage::from_pixel(POINTER_CURSOR_SIZE, POINTER_CURSOR_SIZE, Rgba([0, 0, 0, 0]));
        let pts: Vec<Point<i32>> = vec![
            Point::new(3, 2),
            Point::new(3, 21),
            Point::new(8, 16),
            Point::new(11, 23),
            Point::new(14, 22),
            Point::new(11, 15),
            Point::new(18, 15),
        ];
        let white = Rgba([255, 255, 255, 255]);
        let black = Rgba([0, 0, 0, 255]);
        draw_polygon_mut(&mut buf, &pts, white);
        for i in 0..pts.len() {
            let a = pts[i];
            let b = pts[(i + 1) % pts.len()];
            draw_line_segment_mut(
                &mut buf,
                (a.x as f32, a.y as f32),
                (b.x as f32, b.y as f32),
                black,
            );
        }
        buf
    })
}

fn blend_paste(dest: &mut RgbaImage, src: &RgbaImage, origin_x: i32, origin_y: i32) {
    let dw = dest.width() as i32;
    let dh = dest.height() as i32;
    let sw = src.width() as i32;
    let sh = src.height() as i32;
    for sy in 0..sh {
        for sx in 0..sw {
            let dx = origin_x + sx;
            let dy = origin_y + sy;
            if dx < 0 || dy < 0 || dx >= dw || dy >= dh {
                continue;
            }
            let sp = *src.get_pixel(sx as u32, sy as u32);
            let a = sp[3] as u32;
            if a == 0 {
                continue;
            }
            let dp = *dest.get_pixel(dx as u32, dy as u32);
            let inv = 255 - a;
            let blend = |s: u8, d: u8| -> u8 {
                ((s as u32 * a + d as u32 * inv) / 255).min(255) as u8
            };
            dest.put_pixel(
                dx as u32,
                dy as u32,
                Rgba([
                    blend(sp[0], dp[0]),
                    blend(sp[1], dp[1]),
                    blend(sp[2], dp[2]),
                    (a + (dp[3] as u32 * inv) / 255).min(255) as u8,
                ]),
            );
        }
    }
}

/// Python `draw_cursor_pointer_overlay`: paste 32×32 cursor with hotspot `(3,2)`.
fn composite_pointer_cursor(img: &mut RgbaImage, mouse_ix: i32, mouse_iy: i32) {
    let cur = pointer_cursor_rgba();
    let (hx, hy) = POINTER_HOTSPOT;
    let ox = mouse_ix - hx;
    let oy = mouse_iy - hy;
    blend_paste(img, cur, ox, oy);
}

fn sample_luminance(img: &RgbaImage, cx: i32, cy: i32, radius: i32) -> f32 {
    let w = img.width() as i32;
    let h = img.height() as i32;
    let mut total = 0.0_f32;
    let mut count = 0_u32;
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            let x = cx + dx;
            let y = cy + dy;
            if x >= 0 && y >= 0 && x < w && y < h {
                let p = img.get_pixel(x as u32, y as u32);
                total += 0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32;
                count += 1;
            }
        }
    }
    if count > 0 {
        total / count as f32
    } else {
        128.0
    }
}

/// Python `draw_focus_caret_overlay`: I-beam with contrast halo.
fn draw_focus_caret_overlay(img: &mut RgbaImage, focus_ix: i32, focus_iy: i32) {
    let w_caret: i32 = 3;
    let h_caret: i32 = 18;
    let x0 = focus_ix - w_caret / 2;
    let y0 = focus_iy - h_caret;
    let x1 = focus_ix + w_caret - w_caret / 2;
    let y1 = focus_iy;
    let lum = sample_luminance(img, focus_ix, focus_iy, 2);
    let (caret_color, outline_color) = if lum > 140.0 {
        (Rgba([0, 0, 0, 255]), Rgba([255, 255, 255, 255]))
    } else {
        (Rgba([255, 255, 255, 255]), Rgba([0, 0, 0, 255]))
    };
    const OFFS: [(i32, i32); 8] = [
        (-1, -1),
        (-1, 0),
        (-1, 1),
        (0, -1),
        (0, 1),
        (1, -1),
        (1, 0),
        (1, 1),
    ];
    for (dx, dy) in OFFS {
        for py in y0 + dy..=y1 + dy {
            for px in x0 + dx..=x1 + dx {
                set_px(img, px, py, outline_color);
            }
        }
    }
    for py in y0..=y1 {
        for px in x0..=x1 {
            set_px(img, px, py, caret_color);
        }
    }
}

/// Caret first, then pointer — matches Python `_apply_focus_mouse_overlays`.
fn apply_pointer_and_caret_overlays(
    img: &mut RgbaImage,
    caret: Option<(i32, i32)>,
    mouse: Option<(i32, i32)>,
) {
    if let Some((fx, fy)) = caret {
        draw_focus_caret_overlay(img, fx, fy);
    }
    if DRAW_SYNTHETIC_POINTER_OVERLAY {
        if let Some((mx, my)) = mouse {
            composite_pointer_cursor(img, mx, my);
        }
    }
}

fn rgba_to_png_bytes(img: &RgbaImage) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    let mut w = Cursor::new(&mut buf);
    DynamicImage::ImageRgba8(img.clone())
        .write_to(&mut w, ImageFormat::Png)
        .map_err(|e| anyhow!("PNG encode: {}", e))?;
    Ok(buf)
}

fn copy_subimage(src: &RgbaImage, x0: u32, y0: u32, cw: u32, ch: u32) -> RgbaImage {
    let mut out = RgbaImage::new(cw, ch);
    for yy in 0..ch {
        for xx in 0..cw {
            let p = src.get_pixel(x0 + xx, y0 + yy);
            out.put_pixel(xx, yy, *p);
        }
    }
    out
}

/// Top `take_h` rows (or full height if shorter).
fn crop_top_strip(src: &RgbaImage, take_h: u32) -> RgbaImage {
    let w = src.width();
    let h = src.height();
    let ch = take_h.min(h);
    copy_subimage(src, 0, 0, w, ch)
}

/// Bottom `take_h` rows (or full height if shorter).
fn crop_bottom_strip(src: &RgbaImage, take_h: u32) -> RgbaImage {
    let w = src.width();
    let h = src.height();
    let ch = take_h.min(h);
    let y0 = h.saturating_sub(ch);
    copy_subimage(src, 0, y0, w, ch)
}

/// `side`×`side` centered on `(lx, ly)`, clamped so the window stays inside the image.
fn crop_square_around(src: &RgbaImage, lx: i32, ly: i32, side: u32) -> RgbaImage {
    let w = src.width();
    let h = src.height();
    if w == 0 || h == 0 {
        return RgbaImage::new(1, 1);
    }
    let sw = side.min(w);
    let sh = side.min(h);
    let half_x = (sw / 2) as i32;
    let half_y = (sh / 2) as i32;
    let mut x0 = lx - half_x;
    let mut y0 = ly - half_y;
    let wm = w as i32;
    let hm = h as i32;
    if x0 < 0 {
        x0 = 0;
    }
    if y0 < 0 {
        y0 = 0;
    }
    if x0 + sw as i32 > wm {
        x0 = wm - sw as i32;
    }
    if y0 + sh as i32 > hm {
        y0 = hm - sh as i32;
    }
    if x0 < 0 {
        x0 = 0;
    }
    if y0 < 0 {
        y0 = 0;
    }
    let x0 = x0 as u32;
    let y0 = y0 as u32;
    copy_subimage(src, x0, y0, sw, sh)
}

fn tint_zoom_border(img: &mut RgbaImage, accent: Rgba<u8>, mode: ZoomBorderMode) {
    let w = img.width();
    let h = img.height();
    if w == 0 || h == 0 {
        return;
    }
    let t = 2_u32;
    match mode {
        ZoomBorderMode::TopAccent => {
            for yy in 0..t.min(h) {
                for xx in 0..w {
                    img.put_pixel(xx, yy, accent);
                }
            }
        }
        ZoomBorderMode::BottomAccent => {
            for yy in 0..t.min(h) {
                let row = h.saturating_sub(1 + yy);
                for xx in 0..w {
                    img.put_pixel(xx, row, accent);
                }
            }
        }
        ZoomBorderMode::LeftAccent => {
            for xx in 0..t.min(w) {
                for yy in 0..h {
                    img.put_pixel(xx, yy, accent);
                }
            }
        }
    }
}

#[derive(Clone, Copy)]
enum ZoomBorderMode {
    TopAccent,
    BottomAccent,
    LeftAccent,
}

fn decode_jpeg_to_rgba(bytes: &[u8]) -> Result<RgbaImage> {
    let img = image::load_from_memory(bytes).map_err(|e| anyhow!("decode jpeg: {}", e))?;
    Ok(img.to_rgba8())
}

fn decode_png_to_rgba(bytes: &[u8]) -> Result<RgbaImage> {
    let img = image::load_from_memory(bytes).map_err(|e| anyhow!("decode png: {}", e))?;
    Ok(img.to_rgba8())
}

/// Pipeline: unmarked annotated PNG (+ optional raw JPEG) → tier-selected assets.
///
/// `global_caret` is **global screen** coordinates when known (Python `focus_position.get_focus_position()`), else `None`.
pub fn build_vision_overlay_pack(
    raw_jpeg_unmarked: &[u8],
    annotated_png_unmarked: &[u8],
    monitor: &MonitorInfo,
    global_pointer: (i32, i32),
    global_caret: Option<(i32, i32)>,
    work: VisionOverlayWork,
) -> Result<VisionOverlayPack> {
    let empty = Vec::new();
    let (mx, my) = local_on_monitor(global_pointer.0, global_pointer.1, monitor);

    // Lightweight path for tiers that only need annotated with no raw/zoom assets.
    if !work.raw_marked_jpeg && !work.zoom_crops {
        let mut ann_rgba = decode_png_to_rgba(annotated_png_unmarked)?;
        let (gw, gh) = (ann_rgba.width(), ann_rgba.height());
        let mouse_ann = overlay_position_if_inside(mx, my, gw, gh);
        let caret_ann = global_caret.and_then(|(gx, gy)| {
            let (lx, ly) = local_on_monitor(gx, gy, monitor);
            overlay_position_if_inside(lx, ly, gw, gh)
        });
        apply_pointer_and_caret_overlays(&mut ann_rgba, caret_ann, mouse_ann);
        return Ok(VisionOverlayPack {
            raw_marked_jpeg: Vec::new(),
            annotated_marked_jpeg: screen::rgba_to_jpeg_bytes(ann_rgba, screen::SCREENSHOT_JPEG_QUALITY)?,
            zoom_menu_bar_png: Vec::new(),
            zoom_task_bar_png: Vec::new(),
            zoom_pointer_png: Vec::new(),
        });
    }

    let mut raw_rgba = decode_jpeg_to_rgba(raw_jpeg_unmarked)?;
    let mut ann_rgba = decode_png_to_rgba(annotated_png_unmarked)?;

    let (w, h) = (raw_rgba.width(), raw_rgba.height());
    let (gw, gh) = (ann_rgba.width(), ann_rgba.height());
    if (w, h) != (gw, gh) {
        log::warn!(
            "screen_overlay: raw {}x{} vs annotated {}x{}; overlay bounds use each image’s size",
            w,
            h,
            gw,
            gh
        );
    }

    let mouse_raw = overlay_position_if_inside(mx, my, w, h);
    let mouse_ann = overlay_position_if_inside(mx, my, gw, gh);

    let caret_raw = global_caret.and_then(|(gx, gy)| {
        let (lx, ly) = local_on_monitor(gx, gy, monitor);
        overlay_position_if_inside(lx, ly, w, h)
    });
    let caret_ann = global_caret.and_then(|(gx, gy)| {
        let (lx, ly) = local_on_monitor(gx, gy, monitor);
        overlay_position_if_inside(lx, ly, gw, gh)
    });

    if work.raw_marked_jpeg {
        apply_pointer_and_caret_overlays(&mut raw_rgba, caret_raw, mouse_raw);
    }
    apply_pointer_and_caret_overlays(&mut ann_rgba, caret_ann, mouse_ann);

    let raw_marked_jpeg = if work.raw_marked_jpeg {
        screen::rgba_to_jpeg_bytes(raw_rgba, screen::SCREENSHOT_JPEG_QUALITY)?
    } else {
        empty.clone()
    };

    let (zoom_menu_bar_png, zoom_task_bar_png, zoom_pointer_png) = if work.zoom_crops {
        let zmx = mx.clamp(0, gw.saturating_sub(1) as i32);
        let zmy = my.clamp(0, gh.saturating_sub(1) as i32);

        let mut zoom_menu = crop_top_strip(&ann_rgba, ZOOM_MENU_H);
        tint_zoom_border(&mut zoom_menu, ACCENT_MENU, ZoomBorderMode::TopAccent);

        let mut zoom_task = crop_bottom_strip(&ann_rgba, ZOOM_TASK_H);
        tint_zoom_border(&mut zoom_task, ACCENT_TASK, ZoomBorderMode::BottomAccent);

        let mut zoom_ptr = crop_square_around(&ann_rgba, zmx, zmy, ZOOM_POINTER_CROP_SIDE);
        zoom_ptr = magnify_nearest(&zoom_ptr, ZOOM_POINTER_MAGNIFY_FACTOR);
        tint_zoom_border(&mut zoom_ptr, ACCENT_POINTER, ZoomBorderMode::LeftAccent);

        (
            rgba_to_png_bytes(&zoom_menu)?,
            rgba_to_png_bytes(&zoom_task)?,
            rgba_to_png_bytes(&zoom_ptr)?,
        )
    } else {
        (empty.clone(), empty.clone(), empty)
    };

    let annotated_marked_jpeg =
        screen::rgba_to_jpeg_bytes(ann_rgba, screen::SCREENSHOT_JPEG_QUALITY)?;

    Ok(VisionOverlayPack {
        raw_marked_jpeg,
        annotated_marked_jpeg,
        zoom_menu_bar_png,
        zoom_task_bar_png,
        zoom_pointer_png,
    })
}

/// Assets for **`[Screen before action]`** + **`[Zoom pointer before action]`** inject slots.
#[derive(Debug, Clone)]
pub struct BeforeActionInject {
    /// Full-frame marked JPEG (`[Screen before action]`).
    pub screen_jpeg: Vec<u8>,
    /// **4×** magnified pointer crop PNG (`[Zoom pointer before action]`).
    pub zoom_pointer_png: Vec<u8>,
}

fn magnify_nearest(img: &RgbaImage, factor: u32) -> RgbaImage {
    if factor <= 1 {
        return img.clone();
    }
    let nw = img.width().saturating_mul(factor);
    let nh = img.height().saturating_mul(factor);
    image::imageops::resize(img, nw, nh, image::imageops::FilterType::Nearest)
}

/// Build before-action inject: prior turn’s **unmarked** JPEG + **current** pointer, plus a **±50 px** crop magnified **4×** for geometry.
///
/// `prior_monitor` must be the monitor bounds from when `prior_raw_jpeg_unmarked` was captured.
pub fn build_before_action_inject(
    prior_raw_jpeg_unmarked: &[u8],
    prior_monitor: &MonitorInfo,
    current_global_pointer: (i32, i32),
) -> Result<BeforeActionInject> {
    let mut raw_rgba = decode_jpeg_to_rgba(prior_raw_jpeg_unmarked)?;
    let (w, h) = (raw_rgba.width(), raw_rgba.height());
    let (mx, my) = local_on_monitor(current_global_pointer.0, current_global_pointer.1, prior_monitor);
    let mouse = overlay_position_if_inside(mx, my, w, h);
    apply_pointer_and_caret_overlays(&mut raw_rgba, None, mouse);

    let zmx = mx.clamp(0, w.saturating_sub(1) as i32);
    let zmy = my.clamp(0, h.saturating_sub(1) as i32);
    let mut zoom_crop =
        crop_square_around(&raw_rgba, zmx, zmy, BEFORE_POINTER_ZOOM_CROP_SIDE);
    zoom_crop = magnify_nearest(&zoom_crop, BEFORE_POINTER_ZOOM_FACTOR);
    tint_zoom_border(&mut zoom_crop, ACCENT_POINTER_BEFORE, ZoomBorderMode::LeftAccent);

    Ok(BeforeActionInject {
        screen_jpeg: screen::rgba_to_jpeg_bytes(raw_rgba, screen::SCREENSHOT_JPEG_QUALITY)?,
        zoom_pointer_png: rgba_to_png_bytes(&zoom_crop)?,
    })
}

/// Mark an unmarked capture JPEG with synthetic pointer and focus caret (no SOM annotation).
pub fn mark_raw_jpeg_with_pointer_and_caret(
    raw_jpeg_unmarked: &[u8],
    monitor: &MonitorInfo,
    global_pointer: (i32, i32),
    global_caret: Option<(i32, i32)>,
) -> Result<Vec<u8>> {
    let mut raw_rgba = decode_jpeg_to_rgba(raw_jpeg_unmarked)?;
    let (w, h) = (raw_rgba.width(), raw_rgba.height());
    let (mx, my) = local_on_monitor(global_pointer.0, global_pointer.1, monitor);
    let mouse = overlay_position_if_inside(mx, my, w, h);
    let caret = global_caret.and_then(|(gx, gy)| {
        let (lx, ly) = local_on_monitor(gx, gy, monitor);
        overlay_position_if_inside(lx, ly, w, h)
    });
    apply_pointer_and_caret_overlays(&mut raw_rgba, caret, mouse);
    screen::rgba_to_jpeg_bytes(raw_rgba, screen::SCREENSHOT_JPEG_QUALITY)
}

/// Back-compat helper: full-frame only (tests).
pub fn build_before_action_raw_jpeg(
    prior_raw_jpeg_unmarked: &[u8],
    prior_monitor: &MonitorInfo,
    current_global_pointer: (i32, i32),
) -> Result<Vec<u8>> {
    Ok(build_before_action_inject(
        prior_raw_jpeg_unmarked,
        prior_monitor,
        current_global_pointer,
    )?
    .screen_jpeg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crop_square_clamps_inside() {
        let mut img = RgbaImage::new(50, 50);
        img.put_pixel(10, 10, Rgba([1, 2, 3, 255]));
        let sub = crop_square_around(&img, 5, 5, ZOOM_POINTER_CROP_SIDE);
        assert_eq!(sub.width(), 50);
        assert_eq!(sub.height(), 50);
    }

    #[test]
    fn global_to_local() {
        let m = MonitorInfo::new(100, 200, 800, 600);
        assert_eq!(global_pointer_to_local(150, 250, &m), (50, 50));
    }

    #[test]
    fn pointer_cursor_has_opaque_arrow_pixels() {
        let c = pointer_cursor_rgba();
        assert_eq!(c.dimensions(), (POINTER_CURSOR_SIZE, POINTER_CURSOR_SIZE));
        assert!(c.get_pixel(3, 2)[3] > 200);
    }

    #[test]
    fn mark_raw_jpeg_draws_pointer_and_caret() {
        let monitor = MonitorInfo::new(0, 0, 64, 64);
        let prior = RgbaImage::from_pixel(64, 64, Rgba([40, 80, 120, 255]));
        let jpeg_prior = screen::rgba_to_jpeg_bytes(prior.clone(), screen::SCREENSHOT_JPEG_QUALITY).unwrap();
        let out = mark_raw_jpeg_with_pointer_and_caret(&jpeg_prior, &monitor, (32, 32), Some((20, 40)))
            .unwrap();
        let marked = decode_jpeg_to_rgba(&out).unwrap();
        let unmarked = decode_jpeg_to_rgba(&jpeg_prior).unwrap();
        assert_ne!(marked.get_pixel(32, 32), unmarked.get_pixel(32, 32));
        assert_ne!(marked.get_pixel(20, 40), unmarked.get_pixel(20, 40));
    }

    #[test]
    fn before_action_synthetic_pointer_toggle() {
        let monitor = MonitorInfo::new(0, 0, 32, 32);
        let prior = RgbaImage::from_pixel(32, 32, Rgba([40, 80, 120, 255]));
        let jpeg_prior = screen::rgba_to_jpeg_bytes(prior.clone(), screen::SCREENSHOT_JPEG_QUALITY).unwrap();
        let out = build_before_action_raw_jpeg(&jpeg_prior, &monitor, (16, 16)).unwrap();
        let marked = decode_jpeg_to_rgba(&out).unwrap();
        let unmarked = decode_jpeg_to_rgba(&jpeg_prior).unwrap();
        if DRAW_SYNTHETIC_POINTER_OVERLAY {
            assert_ne!(marked.get_pixel(16, 16), unmarked.get_pixel(16, 16));
        } else {
            assert_eq!(marked.get_pixel(16, 16), unmarked.get_pixel(16, 16));
        }
    }

    #[test]
    fn after_action_pointer_zoom_is_4x_200_crop() {
        let monitor = MonitorInfo::new(0, 0, 400, 400);
        let ann = RgbaImage::from_pixel(400, 400, Rgba([10, 20, 30, 255]));
        let ann_png = rgba_to_png_bytes(&ann).unwrap();
        let raw_jpeg =
            screen::rgba_to_jpeg_bytes(ann.clone(), screen::SCREENSHOT_JPEG_QUALITY).unwrap();
        let pack = build_vision_overlay_pack(
            &raw_jpeg,
            &ann_png,
            &monitor,
            (200, 200),
            None,
            VisionOverlayWork::FULL,
        )
        .unwrap();
        let zoom = decode_png_to_rgba(&pack.zoom_pointer_png).unwrap();
        let expected = ZOOM_POINTER_CROP_SIDE * ZOOM_POINTER_MAGNIFY_FACTOR;
        assert_eq!(zoom.width(), expected);
        assert_eq!(zoom.height(), expected);
    }

    #[test]
    fn primary_work_keeps_raw_marked_and_skips_zoom() {
        let work = VisionOverlayWork::for_tier(ComputerTier::Primary);
        assert!(work.raw_marked_jpeg);
        assert!(!work.zoom_crops);
    }

    #[test]
    fn before_action_zoom_is_4x_crop_side() {
        let monitor = MonitorInfo::new(0, 0, 200, 200);
        let prior = RgbaImage::from_pixel(200, 200, Rgba([40, 80, 120, 255]));
        let jpeg_prior = screen::rgba_to_jpeg_bytes(prior, screen::SCREENSHOT_JPEG_QUALITY).unwrap();
        let pack = build_before_action_inject(&jpeg_prior, &monitor, (100, 100)).unwrap();
        let zoom = decode_png_to_rgba(&pack.zoom_pointer_png).unwrap();
        assert_eq!(zoom.width(), BEFORE_POINTER_ZOOM_CROP_SIDE * BEFORE_POINTER_ZOOM_FACTOR);
        assert_eq!(zoom.height(), BEFORE_POINTER_ZOOM_CROP_SIDE * BEFORE_POINTER_ZOOM_FACTOR);
    }
}
