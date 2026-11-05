//! Pointer-style screen overlays (Python `screen_overlay.py` analogue): system-style pointer bitmap
//! (`assets/cursor-pointer.svg` path, same as Python) and I-beam caret (`draw_focus_caret_overlay`),
//! then zoom crops from the **marked** annotated image.
//!
//! Vision slot labels match the bracket captions in Python `extensions/message_loop_prompts_after/_10_computer_screen_inject.py`
//! (`[Screen before action]`, `[Screen after action]`, `[Annotated after action]`, `[Zoom top after action]`, …).

use super::screen::{self, MonitorInfo};
use anyhow::{anyhow, Result};
use image::{Rgba, RgbaImage};

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

/// 指针十字星：与 locate 整屏图（`Mark::CrossOnly`）同一形态。
///
/// 换掉箭头是因为**箭头的热点在尖端**：模型按视觉中心估位置，箭头按尖端画，
/// 两者天然差几个像素。十字星的交点是中心，没有这个歧义 —— 而且整屏定位图
/// 早就在用十字星，模型只需要认一种形状。
///
/// 尺寸不另定，直接取 locate 整屏十字的几何（`full_frame_crosshair_geometry`）：
/// 臂 24 px、芯宽 5 px、白边 1.8 px。两处各写一份常量迟早会漂成两个形状。
fn pointer_crosshair_geometry() -> (i32, i32, i32) {
    let (arm, half_core, halo) = crate::agents::computer::locate::render::full_frame_crosshair_geometry();
    (
        arm.round() as i32,
        ((2.0 * half_core).round() as i32) / 2,   // 5 px 宽 → 半宽 2
        halo.round() as i32,
    )
}
/// 虚线圆环半径。臂长 22 px，环 10 px —— 臂伸出环外 12 px，读起来是准星
/// 而不是一个圆圈。
const POINTER_CROSS_RING_PX: f64 = 10.0;

/// 越界自动裁剪 —— 指针贴边时不该 panic，也不该画到图外。
fn fill_rgba_rect(img: &mut RgbaImage, x0: i32, y0: i32, x1: i32, y1: i32, color: Rgba<u8>) {
    let (w, h) = (img.width() as i32, img.height() as i32);
    for y in y0.max(0)..=y1.min(h - 1) {
        for x in x0.max(0)..=x1.min(w - 1) {
            img.put_pixel(x as u32, y as u32, color);
        }
    }
}

/// 白边先画、红芯后画：任何背景上都看得见，中心永远是红的。
///
/// 形状 = 十字星 + 虚线圆环，和 locate 的容差圈（`Mark::CrossWithTolerance`）
/// 同一套节拍：一段 16°、空 8°，白描边在下、红芯在上。十字负责指出交点，
/// 圆环给出一个「大概这么大」的尺度 —— 判断「指针是否落在这个控件/面板里」
/// 时，眼睛要比的是圆环和控件边界，而不是一个没有大小的点。
fn composite_pointer_crosshair(img: &mut RgbaImage, cx: i32, cy: i32) {
    let (arm, core, halo_w) = pointer_crosshair_geometry();
    let halo = core + halo_w;
    let ring = POINTER_CROSS_RING_PX;
    let white = Rgba([255, 255, 255, 235]);
    let red = Rgba([220, 38, 38, 255]);

    let mut put = |x: f64, y: f64, c: Rgba<u8>| {
        let (xi, yi) = (x.round() as i32, y.round() as i32);
        if xi >= 0 && yi >= 0 && xi < img.width() as i32 && yi < img.height() as i32 {
            img.put_pixel(xi as u32, yi as u32, c);
        }
    };

    // 圆环：沿圆周密集采样，段内每步画一根法线（和 locate 一样 —— 按角度
    // 画孤立点会被 JPEG 平均成一层灰影）。
    let (dash, gap) = (16.0_f64.to_radians(), 8.0_f64.to_radians());
    let period = dash + gap;
    for (color, extra) in [(white, halo_w as f64), (red, 0.0)] {
        let t = core as f64 + extra;
        let mut a = 0.0_f64;
        while a < std::f64::consts::TAU {
            if a % period <= dash {
                let (sn, cs) = (a.sin(), a.cos());
                let mut d = -t;
                while d <= t {
                    put(cx as f64 + (ring + d) * cs, cy as f64 + (ring + d) * sn, color);
                    d += 1.0;
                }
            }
            a += 0.004;
        }
    }

    // 十字：白边铺底，红芯压上。
    fill_rgba_rect(img, cx - arm - halo, cy - halo, cx + arm + halo, cy + halo, white);
    fill_rgba_rect(img, cx - halo, cy - arm - halo, cx + halo, cy + arm + halo, white);
    fill_rgba_rect(img, cx - arm, cy - core, cx + arm, cy + core, red);
    fill_rgba_rect(img, cx - core, cy - arm, cx + core, cy + arm, red);
}

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
        composite_pointer_crosshair(&mut rgba, px, py);
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
            composite_pointer_crosshair(img, mx, my);
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

    /// 十字星的中心必须落在指针位置上：臂往两侧展开、中心是红的。
    #[test]
    fn pointer_crosshair_centres_on_the_pointer() {
        let mut img = RgbaImage::from_pixel(200, 200, Rgba([40, 80, 120, 255]));
        composite_pointer_crosshair(&mut img, 100, 100);
        let centre = img.get_pixel(100, 100);
        assert_eq!((centre[0], centre[1], centre[2]), (220, 38, 38), "中心应为红芯");
        // 四条臂都画到了
        for (x, y) in [(100 - 20, 100), (100 + 20, 100), (100, 100 - 20), (100, 100 + 20)] {
            assert_eq!(img.get_pixel(x, y)[0], 220, "臂缺失于 ({x}, {y})");
        }
        // 虚线圆环：半径 10 px，0° 处正好在段内
        assert_eq!(img.get_pixel(100 + 10, 100)[0], 220, "圆环缺失");
        // 环外（对角 28 px 处）不画 —— 这是准星，不是方框
        assert_eq!(img.get_pixel(120, 120)[0], 40);
    }

    /// 指针贴边时不能 panic，也不能画到图外。
    #[test]
    fn pointer_crosshair_clips_at_the_edge() {
        let mut img = RgbaImage::from_pixel(60, 60, Rgba([0, 0, 0, 255]));
        composite_pointer_crosshair(&mut img, 1, 58);
        assert_eq!(img.get_pixel(1, 58)[0], 220);
        assert_eq!(img.get_pixel(59, 59)[0], 0, "不该溢出到未触及的角落");
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
