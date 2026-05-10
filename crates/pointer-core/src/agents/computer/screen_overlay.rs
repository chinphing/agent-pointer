//! Pointer-style screen overlays (Python `screen_overlay.py` analogue): system-style pointer bitmap
//! (`assets/cursor-pointer.svg` path, same as Python) and I-beam caret (`draw_focus_caret_overlay`),
//! then zoom crops from the **marked** annotated image.
//!
//! Vision slot labels match the bracket captions in Python `extensions/message_loop_prompts_after/_10_computer_screen_inject.py`
//! (`[Previous screen raw]`, `[Current screen raw]`, `[Screen annotated]`, `[Screen zoomed top]`, …).

use crate::agents::computer::screen::{self, MonitorInfo};
use anyhow::{anyhow, Result};
use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use imageproc::drawing::{draw_line_segment_mut, draw_polygon_mut};
use imageproc::point::Point;
use std::io::Cursor;
use std::sync::OnceLock;

/// Previous-turn raw JPEG (overlays already applied when stored).
pub const SLOT_PREVIOUS_SCREEN_RAW: &str = "[Previous screen raw]";
/// This-turn raw JPEG after overlays (indices were computed on the unmarked upload).
pub const SLOT_CURRENT_SCREEN_RAW: &str = "[Current screen raw]";
/// Annotated frame after overlays (matches Python `[Screen annotated]`).
pub const SLOT_SCREEN_ANNOTATED: &str = "[Screen annotated]";
/// Top strip zoom of the marked annotated image (Python `[Screen zoomed top]` / disk `zoom_top_bar`).
pub const SLOT_SCREEN_ZOOMED_TOP: &str = "[Screen zoomed top]";
/// Bottom strip zoom (Python `[Screen zoomed bottom]` / disk `zoom_bottom_bar`).
pub const SLOT_SCREEN_ZOOMED_BOTTOM: &str = "[Screen zoomed bottom]";
/// Pointer-centered zoom (Python `[Screen zoomed pointer]` / disk `zoom_mouse`).
pub const SLOT_SCREEN_ZOOMED_POINTER: &str = "[Screen zoomed pointer]";

const ZOOM_MENU_H: u32 = 100;
const ZOOM_TASK_H: u32 = 100;
const ZOOM_POINTER_SIDE: u32 = 300;

/// Raster size / hotspot — matches Python `screen_overlay.py` (`_CURSOR_SIZE`, `_CURSOR_HOTSPOT`).
const POINTER_CURSOR_SIZE: u32 = 32;
const POINTER_HOTSPOT: (i32, i32) = (3, 2);

const ACCENT_MENU: Rgba<u8> = Rgba([220, 60, 60, 255]);
const ACCENT_TASK: Rgba<u8> = Rgba([60, 200, 80, 255]);
const ACCENT_POINTER: Rgba<u8> = Rgba([60, 120, 220, 255]);

/// All extra vision assets for one `[CUR_SCREEN]` turn.
#[derive(Debug, Clone)]
pub struct VisionOverlayPack {
    pub raw_marked_jpeg: Vec<u8>,
    pub annotated_marked_png: Vec<u8>,
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
    if let Some((mx, my)) = mouse {
        composite_pointer_cursor(img, mx, my);
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

/// Pipeline: unmarked raw JPEG + unmarked annotated PNG → marked JPEG/PNG + three zoom PNGs from **marked annotated**.
///
/// `global_caret` is **global screen** coordinates when known (Python `focus_position.get_focus_position()`), else `None`.
pub fn build_vision_overlay_pack(
    raw_jpeg_unmarked: &[u8],
    annotated_png_unmarked: &[u8],
    monitor: &MonitorInfo,
    global_pointer: (i32, i32),
    global_caret: Option<(i32, i32)>,
) -> Result<VisionOverlayPack> {
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

    let (mx, my) = local_on_monitor(global_pointer.0, global_pointer.1, monitor);
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

    apply_pointer_and_caret_overlays(&mut raw_rgba, caret_raw, mouse_raw);
    apply_pointer_and_caret_overlays(&mut ann_rgba, caret_ann, mouse_ann);

    let raw_marked_jpeg = screen::rgba_to_jpeg_bytes(raw_rgba, screen::SCREENSHOT_JPEG_QUALITY)?;
    let annotated_marked_png = rgba_to_png_bytes(&ann_rgba)?;

    let zmx = mx.clamp(0, gw.saturating_sub(1) as i32);
    let zmy = my.clamp(0, gh.saturating_sub(1) as i32);

    let mut zoom_menu = crop_top_strip(&ann_rgba, ZOOM_MENU_H);
    tint_zoom_border(&mut zoom_menu, ACCENT_MENU, ZoomBorderMode::TopAccent);

    let mut zoom_task = crop_bottom_strip(&ann_rgba, ZOOM_TASK_H);
    tint_zoom_border(&mut zoom_task, ACCENT_TASK, ZoomBorderMode::BottomAccent);

    let mut zoom_ptr = crop_square_around(&ann_rgba, zmx, zmy, ZOOM_POINTER_SIDE);
    tint_zoom_border(&mut zoom_ptr, ACCENT_POINTER, ZoomBorderMode::LeftAccent);

    Ok(VisionOverlayPack {
        raw_marked_jpeg,
        annotated_marked_png,
        zoom_menu_bar_png: rgba_to_png_bytes(&zoom_menu)?,
        zoom_task_bar_png: rgba_to_png_bytes(&zoom_task)?,
        zoom_pointer_png: rgba_to_png_bytes(&zoom_ptr)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crop_square_clamps_inside() {
        let mut img = RgbaImage::new(50, 50);
        img.put_pixel(10, 10, Rgba([1, 2, 3, 255]));
        let sub = crop_square_around(&img, 5, 5, 300);
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
}
