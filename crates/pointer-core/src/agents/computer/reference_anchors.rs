//! Nearest annotated regions to the pointer as **coordinate-method** anchor text
//! (aligned with PyProjects `_10_computer_screen_inject.py` `reference_bbox_text`).

use super::annotate::BoxInfo;
use super::coord::CoordinateSystem;
use super::screen::MonitorInfo;

/// Same side length as `[Zoom pointer after action]` (`screen_overlay::ZOOM_POINTER_SIDE`): only
/// boxes whose **center** lies in this axis-aligned square (in capture pixels) enter the distance sort.
const POINTER_VICINITY_SIDE_PX: f32 = 300.0;

/// Count of overlay regions nearest the pointer to list as bbox anchors (PyProjects used 4; we use 5).
const NEAREST_MOUSE_REFERENCE_BOXES: usize = 5;

#[inline]
fn half_vicinity() -> f32 {
    POINTER_VICINITY_SIDE_PX / 2.0
}

/// True if the box center lies in the `POINTER_VICINITY_SIDE_PX` square centered on `(mx, my)` (capture space).
fn box_center_in_pointer_vicinity(b: &BoxInfo, mx: f32, my: f32) -> bool {
    let (cx, cy) = b.center();
    let h = half_vicinity();
    (cx - mx).abs() <= h && (cy - my).abs() <= h
}

/// When no overlay center falls in the pointer window, still inject guidance: model may aim
/// directly without bbox anchors (no global fallback to distant boxes).
const NO_VICINITY_ANCHORS: &str = "**Pointer neighbor reference bboxes:** None — no annotated overlay **center** lies in the **300×300 px** capture window around the pointer. Aim **directly** at the visible control with coordinate-based methods; a listed bbox anchor is **not** required. Refine **x**/**y** across turns to move **closer** even without a neighbor anchor row.";

/// One-line prose for `[CUR_SCREEN]` when boxes exist and the pointer lies in the capture.
pub fn format_mouse_neighbor_reference_bboxes(
    boxes: &[BoxInfo],
    monitor: &MonitorInfo,
    capture_px: (u32, u32),
    global_pointer: (i32, i32),
    coord: CoordinateSystem,
) -> Option<String> {
    if boxes.is_empty() {
        return None;
    }
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

    let in_v: Vec<&BoxInfo> = boxes
        .iter()
        .filter(|b| box_center_in_pointer_vicinity(b, mouse_bx, mouse_by))
        .collect();

    if in_v.is_empty() {
        return Some(NO_VICINITY_ANCHORS.to_string());
    }

    let mut scored: Vec<(f32, &BoxInfo)> = in_v
        .iter()
        .copied()
        .map(|b| {
            let (cx, cy) = b.center();
            let dx = cx - mouse_bx;
            let dy = cy - mouse_by;
            (dx * dx + dy * dy, b)
        })
        .collect();
    scored.sort_by(|a, b| a.0.total_cmp(&b.0));

    let mut picked: Vec<&BoxInfo> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for (_, b) in scored {
        if seen.insert(b.index) && picked.len() < NEAREST_MOUSE_REFERENCE_BOXES {
            picked.push(b);
        }
    }
    if picked.is_empty() {
        return None;
    }

    let parts: Vec<String> = picked.iter().map(|b| format_box(b, cw, ch, coord)).collect();

    let range_hint = match coord {
        CoordinateSystem::Qwen | CoordinateSystem::Kimi => {
            "**left**/**top**/**right**/**bottom** are **0–1000** normalized on the **full capture** (same numeric space as `x`/`y` for coordinate-based `mouse` / `composite_action` / `modified_click` methods this session). Use only as nearby anchors for **coordinate** calls — not as overlay-`index` click targets."
        }
        CoordinateSystem::Pixel => {
            "**left**/**top**/**right**/**bottom** are **screenshot pixel** coords (origin top-left), same as `x`/`y` for `*_at` methods. Anchors only — not overlay indices."
        }
    };

    Some(format!(
        "**Pointer neighbor reference bboxes** ({} nearest by pointer–center distance among overlays whose **center** lies in the **{}×{} px** capture window centered on the pointer; same geometry as **`[Annotated after action]`**): {}; {}",
        picked.len(),
        POINTER_VICINITY_SIDE_PX as i32,
        POINTER_VICINITY_SIDE_PX as i32,
        parts.join("; "),
        range_hint
    ))
}

fn format_box(b: &BoxInfo, cw: f32, ch: f32, coord: CoordinateSystem) -> String {
    let x1 = b.x;
    let y1 = b.y;
    let x2 = b.x + b.width;
    let y2 = b.y + b.height;
    match coord {
        CoordinateSystem::Qwen | CoordinateSystem::Kimi => {
            let nx1 = (x1 / cw) * 1000.0;
            let ny1 = (y1 / ch) * 1000.0;
            let nx2 = (x2 / cw) * 1000.0;
            let ny2 = (y2 / ch) * 1000.0;
            format!(
                "overlay index {} (left,top,right,bottom)=({:.1},{:.1},{:.1},{:.1})",
                b.index, nx1, ny1, nx2, ny2
            )
        }
        CoordinateSystem::Pixel => format!(
            "overlay index {} (left,top,right,bottom)=({:.0},{:.0},{:.0},{:.0})",
            b.index, x1, y1, x2, y2
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_nearest_by_center_distance() {
        let monitor = MonitorInfo::new(0, 0, 1000, 1000);
        let capture_px = (1000, 1000);
        let pointer = (500, 500);
        let boxes = vec![
            BoxInfo {
                index: 1,
                x: 390.0,
                y: 390.0,
                width: 20.0,
                height: 20.0,
                confidence: 1.0,
            },
            BoxInfo {
                index: 2,
                x: 490.0,
                y: 490.0,
                width: 20.0,
                height: 20.0,
                confidence: 1.0,
            },
        ];
        let s = format_mouse_neighbor_reference_bboxes(
            &boxes,
            &monitor,
            capture_px,
            pointer,
            CoordinateSystem::Qwen,
        )
        .expect("line");
        assert!(s.contains("index 2"));
        assert!(s.contains("index 1"));
        let pos2 = s.find("index 2").unwrap();
        let pos1 = s.find("index 1").unwrap();
        assert!(pos2 < pos1, "nearest (2) should appear before farther (1)");
    }

    #[test]
    fn no_anchor_list_when_vicinity_empty_direct_aim_ok() {
        let monitor = MonitorInfo::new(0, 0, 1000, 1000);
        let capture_px = (1000, 1000);
        let pointer = (500, 500);
        let boxes = vec![BoxInfo {
            index: 1,
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
            confidence: 1.0,
        }];
        let s = format_mouse_neighbor_reference_bboxes(
            &boxes,
            &monitor,
            capture_px,
            pointer,
            CoordinateSystem::Qwen,
        )
        .expect("line");
        assert!(
            !s.contains("overlay index 1 (left"),
            "must not list distant boxes: {s}"
        );
        assert!(s.contains("None —"), "expected no-anchor guidance: {s}");
        assert!(s.contains("directly"), "expected direct-aim hint: {s}");
    }
}
