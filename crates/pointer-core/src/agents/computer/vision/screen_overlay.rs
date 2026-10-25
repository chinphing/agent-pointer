//! Pointer-style screen overlays (Python `screen_overlay.py` analogue): system-style pointer bitmap
//! (`assets/cursor-pointer.svg` path, same as Python) and I-beam caret (`draw_focus_caret_overlay`),
//! then zoom crops from the **marked** annotated image.
//!
//! Vision slot labels match the bracket captions in Python `extensions/message_loop_prompts_after/_10_computer_screen_inject.py`
//! (`[Screen before action]`, `[Screen after action]`, `[Annotated after action]`, `[Zoom top after action]`, …).

use super::screen::{self, MonitorInfo};
use anyhow::{anyhow, Result};
use image::{Rgba, RgbaImage};
use imageproc::drawing::{draw_line_segment_mut, draw_polygon_mut};
use imageproc::point::Point;
use std::sync::OnceLock;

/// Position-phase: full desktop at capture time (pipeline verify-host before-action slot).
pub const SLOT_SCREEN_CURRENT: &str = "[Current screen]";
/// Position-phase: numbered overlay on the current desktop (pipeline).
/// Prior capture's marked JPEG, reused as-is for `[Screen before action]`. The pointer is not redrawn.
pub const SLOT_SCREEN_BEFORE_ACTION: &str = "[Screen before action]";
/// Full screenshot from **this** observation (unmarked capture; no synthetic pointer overlay).
pub const SLOT_SCREEN_AFTER_ACTION: &str = "[Screen after action]";
/// Same moment as `[Screen after action]` with synthetic pointer (and caret if focused) drawn.
pub const SLOT_SCREEN_MARKED_AFTER_ACTION: &str = "[Marked screen after action]";
/// Numbered overlay on the **after action** desktop (same moment as `[Screen after action]`).
/// Top bar / chrome strip (after-action annotated frame).
/// Bottom bar / dock strip (after-action annotated frame).
/// Pointer vicinity patch (after-action annotated frame).
/// **4×** magnified **200×200 px** crop (±**100 px** radius) around the pointer on **`[Screen before action]`** — **authoritative** for **`Pointer:`** hotspot geometry.

/// When false, skip drawing the synthetic pointer bitmap (for A/B against OS cursor in capture).
pub const DRAW_SYNTHETIC_POINTER_OVERLAY: bool = true;


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


/// Raster size / hotspot — matches Python `screen_overlay.py` (`_CURSOR_SIZE`, `_CURSOR_HOTSPOT`).
const POINTER_CURSOR_SIZE: u32 = 32;
const POINTER_HOTSPOT: (i32, i32) = (3, 2);


/// The one screenshot the model sees for this turn.
#[derive(Debug, Clone)]
pub struct VisionOverlayPack {
    pub screenshot_jpeg: Vec<u8>,
}

/// Global screen coordinates → bitmap-local (same space as annotate / vision_state).
pub fn global_pointer_to_local(global_x: i32, global_y: i32, monitor: &MonitorInfo) -> (i32, i32) {
    (global_x - monitor.left, global_y - monitor.top)
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
        let mut buf =
            RgbaImage::from_pixel(POINTER_CURSOR_SIZE, POINTER_CURSOR_SIZE, Rgba([0, 0, 0, 0]));
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
            let blend =
                |s: u8, d: u8| -> u8 { ((s as u32 * a + d as u32 * inv) / 255).min(255) as u8 };
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

/// Redraw a capture with the pointer at its **current** position.
///
/// The before-action capture was taken before the pointer moved, so its overlay
/// shows where the pointer used to be — a spot that has nothing to do with this
/// click. Verify needs the click point on the frame where the target is still
/// intact, so it can tell "hit the target" from "hit the neighbour".
pub fn redraw_with_current_pointer(
    raw_jpeg_unmarked: &[u8],
    monitor: &MonitorInfo,
    global_pointer: (i32, i32),
) -> Result<Vec<u8>> {
    let (mx, my) = local_on_monitor(global_pointer.0, global_pointer.1, monitor);
    let mut rgba = decode_jpeg_to_rgba(raw_jpeg_unmarked)?;
    let (w, h) = (rgba.width(), rgba.height());
    if let Some((px, py)) = overlay_position_if_inside(mx, my, w, h) {
        composite_pointer_cursor(&mut rgba, px, py);
    }
    screen::rgba_to_jpeg_bytes(rgba, screen::SCREENSHOT_JPEG_QUALITY)
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


fn decode_jpeg_to_rgba(bytes: &[u8]) -> Result<RgbaImage> {
    let img = image::load_from_memory(bytes).map_err(|e| anyhow!("decode jpeg: {}", e))?;
    Ok(img.to_rgba8())
}

/// Pipeline: unmarked annotated PNG (+ optional raw JPEG) → tier-selected assets.
///
/// `global_caret` is **global screen** coordinates when known (Python `focus_position.get_focus_position()`), else `None`.
pub fn build_vision_overlay_pack(
    raw_jpeg_unmarked: &[u8],
    monitor: &MonitorInfo,
    global_pointer: (i32, i32),
    global_caret: Option<(i32, i32)>,
) -> Result<VisionOverlayPack> {
    let (mx, my) = local_on_monitor(global_pointer.0, global_pointer.1, monitor);
    let mut rgba = decode_jpeg_to_rgba(raw_jpeg_unmarked)?;
    let (w, h) = (rgba.width(), rgba.height());
    let mouse = overlay_position_if_inside(mx, my, w, h);
    let caret = global_caret.and_then(|(gx, gy)| {
        let (lx, ly) = local_on_monitor(gx, gy, monitor);
        overlay_position_if_inside(lx, ly, w, h)
    });
    apply_pointer_and_caret_overlays(&mut rgba, caret, mouse);
    Ok(VisionOverlayPack {
        screenshot_jpeg: screen::rgba_to_jpeg_bytes(rgba, screen::SCREENSHOT_JPEG_QUALITY)?,
    })
}

/// Build before-action inject: prior turn’s **unmarked** JPEG + **current** pointer, plus a **±50 px** crop magnified **4×** for geometry.
///
/// `prior_monitor` must be the monitor bounds from when `prior_raw_jpeg_unmarked` was captured.
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

#[cfg(test)]
mod tests {
    use super::*;

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
        let jpeg_prior =
            screen::rgba_to_jpeg_bytes(prior.clone(), screen::SCREENSHOT_JPEG_QUALITY).unwrap();
        let out =
            mark_raw_jpeg_with_pointer_and_caret(&jpeg_prior, &monitor, (32, 32), Some((20, 40)))
                .unwrap();
        let marked = decode_jpeg_to_rgba(&out).unwrap();
        let unmarked = decode_jpeg_to_rgba(&jpeg_prior).unwrap();
        assert_ne!(marked.get_pixel(32, 32), unmarked.get_pixel(32, 32));
        assert_ne!(marked.get_pixel(20, 40), unmarked.get_pixel(20, 40));
    }

}
