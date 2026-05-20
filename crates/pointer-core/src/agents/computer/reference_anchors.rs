//! **Pointer position** numeric line and **`[Zoom pointer after action]`** anchor prose for `[CUR_SCREEN]`.
//! Keep this inject prose descriptive only; analysis logic lives in COMMUNICATION.

use super::annotate::BoxInfo;
use super::coord::CoordinateSystem;
use super::screen::MonitorInfo;

/// Same crop side as `[Zoom pointer after action]` before magnification (`screen_overlay::ZOOM_POINTER_CROP_SIDE`).
pub const POINTER_VICINITY_SIDE_PX: f32 =
    crate::agents::computer::screen_overlay::ZOOM_POINTER_CROP_SIDE as f32;

const INJECT_RULES_TAIL: &str = "Image-grounded analysis: every visual claim must cite **On [slot name]:**. Stage steps and routing tables: runtime **COMMUNICATION.md**.";

fn pointer_capture_position(
    monitor: &MonitorInfo,
    capture_px: (u32, u32),
    global_pointer: (i32, i32),
) -> Option<(f32, f32, f32, f32)> {
    let cw = capture_px.0.max(1) as f32;
    let ch = capture_px.1.max(1) as f32;
    let mw = monitor.width.max(1) as f32;
    let mh = monitor.height.max(1) as f32;
    let scale_up_x = cw / mw;
    let scale_up_y = ch / mh;
    let lx = (global_pointer.0 - monitor.left) as f32;
    let ly = (global_pointer.1 - monitor.top) as f32;
    if !(0.0..=mw).contains(&lx) || !(0.0..=mh).contains(&ly) {
        return None;
    }
    let mouse_bx = lx * scale_up_x;
    let mouse_by = ly * scale_up_y;
    if mouse_bx < 0.0 || mouse_by < 0.0 || mouse_bx > cw || mouse_by > ch {
        return None;
    }
    Some((mouse_bx, mouse_by, cw, ch))
}

fn to_session_xy(x: f32, y: f32, cw: f32, ch: f32, coord: CoordinateSystem) -> (f32, f32) {
    match coord {
        CoordinateSystem::Qwen | CoordinateSystem::Kimi => ((x / cw) * 1000.0, (y / ch) * 1000.0),
        CoordinateSystem::Pixel => (x, y),
    }
}

fn intersects_pointer_vicinity(b: &BoxInfo, mouse_bx: f32, mouse_by: f32) -> bool {
    let half = POINTER_VICINITY_SIDE_PX / 2.0;
    let vx1 = mouse_bx - half;
    let vy1 = mouse_by - half;
    let vx2 = mouse_bx + half;
    let vy2 = mouse_by + half;
    let bx1 = b.x;
    let by1 = b.y;
    let bx2 = b.x + b.width;
    let by2 = b.y + b.height;
    bx1 <= vx2 && bx2 >= vx1 && by1 <= vy2 && by2 >= vy1
}

fn format_pointer_neighbor_bbox_block(
    boxes: &[BoxInfo],
    mouse_bx: f32,
    mouse_by: f32,
    cw: f32,
    ch: f32,
    coord: CoordinateSystem,
) -> String {
    let mut candidates: Vec<(f32, &BoxInfo)> = boxes
        .iter()
        .filter(|b| intersects_pointer_vicinity(b, mouse_bx, mouse_by))
        .map(|b| {
            let (cx, cy) = b.center();
            let dx = cx - mouse_bx;
            let dy = cy - mouse_by;
            (((dx * dx + dy * dy).sqrt()), b)
        })
        .collect();
    candidates.sort_by(|a, b| a.0.total_cmp(&b.0));

    let session_label = match coord {
        CoordinateSystem::Qwen | CoordinateSystem::Kimi => "session 0-1000",
        CoordinateSystem::Pixel => "session pixels",
    };
    let crop = super::screen_overlay::ZOOM_POINTER_CROP_SIDE;

    let mut lines = vec![format!(
        "**Pointer neighbor reference bboxes** (corner/center coordinates for overlay indices near the pointer; session {session_label}; nearest-first in {crop}×{crop} px vicinity):",
        session_label = session_label,
        crop = crop,
    )];

    if candidates.is_empty() {
        lines.push("- none in pointer vicinity; use the selected overlay index bbox corners from the chosen frame.".to_string());
        return lines.join("\n");
    }

    for (_, b) in candidates.into_iter().take(6) {
        let tl = to_session_xy(b.x, b.y, cw, ch, coord);
        let tr = to_session_xy(b.x + b.width, b.y, cw, ch, coord);
        let br = to_session_xy(b.x + b.width, b.y + b.height, cw, ch, coord);
        let bl = to_session_xy(b.x, b.y + b.height, cw, ch, coord);
        let (ccx, ccy) = to_session_xy(b.x + b.width / 2.0, b.y + b.height / 2.0, cw, ch, coord);
        lines.push(format!(
            "- index {idx}: top-left ({tlx:.1}, {tly:.1}); top-right ({trx:.1}, {try_:.1}); bottom-right ({brx:.1}, {bry:.1}); bottom-left ({blx:.1}, {bly:.1}); center ({cx:.1}, {cy:.1}).",
            idx = b.index,
            tlx = tl.0,
            tly = tl.1,
            trx = tr.0,
            try_ = tr.1,
            brx = br.0,
            bry = br.1,
            blx = bl.0,
            bly = bl.1,
            cx = ccx,
            cy = ccy,
        ));
    }
    lines.join("\n")
}

fn zoom_pointer_output_side() -> u32 {
    super::screen_overlay::ZOOM_POINTER_CROP_SIDE
        * super::screen_overlay::ZOOM_POINTER_MAGNIFY_FACTOR
}

fn format_pointer_position_line(
    mouse_bx: f32,
    mouse_by: f32,
    cw: f32,
    ch: f32,
    coord: CoordinateSystem,
) -> String {
    let crop = super::screen_overlay::ZOOM_POINTER_CROP_SIDE;
    let factor = super::screen_overlay::ZOOM_POINTER_MAGNIFY_FACTOR;
    let out = zoom_pointer_output_side();
    match coord {
        CoordinateSystem::Qwen | CoordinateSystem::Kimi => {
            let nx = (mouse_bx / cw) * 1000.0;
            let ny = (mouse_by / ch) * 1000.0;
            format!(
                "**Pointer position** (same **full capture** as **`[Screen after action]`** / **`[Annotated after action]`**, origin top-left): **capture pixels** (x, y) ≈ ({:.0}, {:.0}); **normalized (x, y)** ≈ ({:.1}, {:.1}) on **0–1000** (same numeric space as coordinate-based `mouse` / `composite_action` / `modified_click` this session). **`[Zoom pointer after action]`** is a **{out}×{out} px** patch ({factor}× magnified from {crop}×{crop} px) centered on this point.",
                mouse_bx, mouse_by, nx, ny
            )
        }
        CoordinateSystem::Pixel => format!(
            "**Pointer position** (capture pixels, origin top-left, same as **`[Screen after action]`**): (x, y) ≈ ({:.0}, {:.0}). **`[Zoom pointer after action]`** is a **{out}×{out} px** patch ({factor}× magnified from {crop}×{crop} px) centered on this point.",
            mouse_bx, mouse_by
        ),
    }
}

/// Prose block appended after `[CUR_SCREEN]` slot order: **Pointer position** + **Zoom pointer** coordinate-anchor guidance.
pub fn format_pointer_coordinate_anchor(
    monitor: &MonitorInfo,
    capture_px: (u32, u32),
    global_pointer: (i32, i32),
    coord: CoordinateSystem,
) -> Option<String> {
    let (mouse_bx, mouse_by, cw, ch) = pointer_capture_position(monitor, capture_px, global_pointer)?;
    let pointer_line = format_pointer_position_line(mouse_bx, mouse_by, cw, ch, coord);
    Some(format!("{pointer_line}\n\n{INJECT_RULES_TAIL}"))
}

/// Back-compat alias (call sites may still use the old name).
pub fn format_mouse_neighbor_reference_bboxes(
    boxes: &[BoxInfo],
    monitor: &MonitorInfo,
    capture_px: (u32, u32),
    global_pointer: (i32, i32),
    coord: CoordinateSystem,
) -> Option<String> {
    let (mouse_bx, mouse_by, cw, ch) = pointer_capture_position(monitor, capture_px, global_pointer)?;
    let pointer_line = format_pointer_position_line(mouse_bx, mouse_by, cw, ch, coord);
    let bbox_block = format_pointer_neighbor_bbox_block(boxes, mouse_bx, mouse_by, cw, ch, coord);
    Some(format!("{pointer_line}\n\n{INJECT_RULES_TAIL}\n\n{bbox_block}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn injects_pointer_position_and_zoom_anchor() {
        let monitor = MonitorInfo::new(0, 0, 1000, 1000);
        let s = format_pointer_coordinate_anchor(
            &monitor,
            (1000, 1000),
            (500, 500),
            CoordinateSystem::Qwen,
        )
        .expect("line");
        assert!(s.contains("**Pointer position**"));
        assert!(s.contains("COMMUNICATION.md"));
        assert!(s.contains("[Zoom pointer after action]"));
        assert!(
            !s.contains("intersecting pointer 200x200 vicinity"),
            "anchor-only helper should not include bbox rows: {s}"
        );
    }

    #[test]
    fn injects_neighbor_reference_bbox_coordinates() {
        let monitor = MonitorInfo::new(0, 0, 1000, 1000);
        let boxes = vec![BoxInfo {
            index: 4,
            x: 450.0,
            y: 480.0,
            width: 120.0,
            height: 80.0,
            confidence: 0.9,
        }];
        let s = format_mouse_neighbor_reference_bboxes(
            &boxes,
            &monitor,
            (1000, 1000),
            (500, 500),
            CoordinateSystem::Qwen,
        )
        .expect("line");
        assert!(s.contains("**Pointer neighbor reference bboxes**"));
        assert!(s.contains("index 4:"));
        assert!(s.contains("top-left"));
        assert!(s.contains("bottom-right"));
    }
}
