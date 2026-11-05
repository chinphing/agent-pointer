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
/// 指针标记的尺寸**按图宽取比例**，不写死像素。
///
/// 参照点：模型看的 `[Current screen]` 实测 1512 px 宽（Retina 逻辑分辨率），
/// 正文一行约 14–18 px。臂长取 10 px → 整个标记跨度 20 px，约一行高。
///
/// 不照抄 locate 的 24 px：那张图 2444 px 宽，像素值搬过来就是 1.6 倍粗，
/// 直接糊住一行文字。像素值不可移植，可移植的只有比例。
const POINTER_CROSS_ARM_SHARE_OF_WIDTH: f64 = 18.0 / 1512.0;
/// 比例算出来的下限。小窗口/小截图（宽 < 800 px）上按比例只剩个位数，
/// 标记会缩到看不见 —— 保底 8 px。
const POINTER_CROSS_ARM_MIN_PX: i32 = 10;
/// 比例照抄 verify 的实测值（拿 `Mark::CrossWithTolerance` 渲染真图量出来的）：
///
/// - 环直径 / 臂跨度 ≈ 0.65  →  环半径 = 臂 × 0.65
/// - 线宽 / 半臂长 ≈ 1/9     →  芯半宽 = 臂 × 0.055
/// - 白边 = 芯半宽 × 0.72
///
/// **只有臂长带下限**，其余全部是臂长的倍数 —— 任何图宽下形状完全一样，
/// 不会出现「十字粗了、环小了」这种比例漂移。
const POINTER_CROSS_CORE_OF_ARM: f64 = 0.055;
const POINTER_CROSS_HALO_OF_CORE: f64 = 0.72;
const POINTER_CROSS_RING_OF_ARM: f64 = 0.65;

/// `(臂长, 芯半宽, 白边宽, 环半径)` —— 后三项都是臂长的倍数，不做整数取整，
/// 取整放在绘制时按像素边界做，避免小尺寸下比例被放大。
fn pointer_crosshair_geometry(width: u32) -> (f64, f64, f64, f64) {
    let arm = (width as f64 * POINTER_CROSS_ARM_SHARE_OF_WIDTH)
        .round()
        .max(POINTER_CROSS_ARM_MIN_PX as f64);
    let core = arm * POINTER_CROSS_CORE_OF_ARM;
    let halo = core * POINTER_CROSS_HALO_OF_CORE;
    let ring = arm * POINTER_CROSS_RING_OF_ARM;
    (arm, core, halo, ring)
}

/// 越界自动裁剪 —— 指针贴边时不该 panic，也不该画到图外。
fn fill_rgba_rect(img: &mut RgbaImage, x0: f64, y0: f64, x1: f64, y1: f64, color: Rgba<u8>) {
    let (w, h) = (img.width() as i32, img.height() as i32);
    let (x0, y0) = (x0.round() as i32, y0.round() as i32);
    let (x1, y1) = (x1.round() as i32, y1.round() as i32);
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
    let (arm, core, halo_w, ring) = pointer_crosshair_geometry(img.width());
    let halo = core + halo_w;
    let (fx, fy) = (cx as f64, cy as f64);
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
    for (color, extra) in [(white, halo_w), (red, 0.0)] {
        let t = core as f64 + extra;
        let mut a = 0.0_f64;
        while a < std::f64::consts::TAU {
            if a % period <= dash {
                let (sn, cs) = (a.sin(), a.cos());
                let mut d = -t;
                while d <= t {
                    put(fx + (ring + d) * cs, fy + (ring + d) * sn, color);
                    d += 1.0;
                }
            }
            a += 0.004;
        }
    }

    // 十字：白边铺底，红芯压上。
    fill_rgba_rect(img, fx - arm - halo, fy - halo, fx + arm + halo, fy + halo, white);
    fill_rgba_rect(img, fx - halo, fy - arm - halo, fx + halo, fy + arm + halo, white);
    fill_rgba_rect(img, fx - arm, fy - core, fx + arm, fy + core, red);
    fill_rgba_rect(img, fx - core, fy - arm, fx + core, fy + arm, red);
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

    /// 十字星的中心必须落在指针位置上：臂往两侧展开、中心是红的、环在臂内。
    #[test]
    fn pointer_crosshair_centres_on_the_pointer() {
        // 参照宽度 2444 px → 臂 24 px，和 locate 整屏图一致
        let (arm, _core, _halo, ring) = pointer_crosshair_geometry(2444);
        let (cx, cy) = (1200i32, 500i32);
        let mut img = RgbaImage::from_pixel(2444, 1023, Rgba([40, 80, 120, 255]));
        composite_pointer_crosshair(&mut img, cx, cy);
        assert_eq!(img.get_pixel(cx as u32, cy as u32)[0], 220, "中心应为红芯");
        // 四条臂都画到了
        let ai = arm as i32;
        for (x, y) in [
            (cx - ai + 2, cy),
            (cx + ai - 2, cy),
            (cx, cy - ai + 2),
            (cx, cy + ai - 2),
        ] {
            assert_eq!(img.get_pixel(x as u32, y as u32)[0], 220, "臂缺失于 ({x}, {y})");
        }
        // 圆环：30° 落在虚线「段」内（16°段 / 8°空），且不在任何一条臂上
        let (a, r) = (30.0_f64.to_radians(), ring);
        let rx = (cx as f64 + r * a.cos()).round() as u32;
        let ry = (cy as f64 + r * a.sin()).round() as u32;
        assert_ne!(img.get_pixel(rx, ry)[0], 40, "圆环缺失于 ({rx}, {ry})");
        // 环外对角不画 —— 这是准星，不是方框
        assert_eq!(img.get_pixel((cx + 40) as u32, (cy + 40) as u32)[0], 40);
    }

    /// 臂长按图宽比例走，并受最小值约束。
    #[test]
    fn pointer_crosshair_scales_with_image_width() {
        // 1512 宽（模型实际看到的 Retina 逻辑分辨率）→ 18 px
        assert_eq!(pointer_crosshair_geometry(1512).0, 18.0);
        // 更大的图按比例放大
        assert_eq!(pointer_crosshair_geometry(3024).0, 36.0);
        // 小窗口：比例算出来只有 6，保底 10
        assert_eq!(pointer_crosshair_geometry(500).0, 10.0);

        // 除臂长外全部是臂长的倍数 —— 任意图宽下形状完全一致
        for w in [500u32, 1512, 3024] {
            let (arm, core, halo, ring) = pointer_crosshair_geometry(w);
            assert!((core / arm - 0.055).abs() < 1e-9, "w={w} 芯臂比漂移");
            assert!((halo / core - 0.72).abs() < 1e-9, "w={w} 白边比漂移");
            assert!((ring / arm - 0.65).abs() < 1e-9, "w={w} 环臂比漂移");
        }
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
