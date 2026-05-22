//! **Pointer position** and **Overlay reference bboxes** (all indices) for `[CUR_SCREEN]`.
//! Keep inject prose descriptive only; analysis logic lives in COMMUNICATION.

use super::annotate::BoxInfo;
use super::coord::CoordinateSystem;
use super::screen::MonitorInfo;

const INJECT_RULES_TAIL_ADVANCED: &str = "Image-grounded analysis: cite **On [slot name]:**. **All tool (x,y) must be looked up in Overlay reference bboxes below** — find row R, copy (left,top,right,bottom), derive anchor, compute (x,y); **forbidden** pixel-guess or digit position as click; **`*_index` forbidden** — use **`*_at`**. Follow **communication** rules.";

const INJECT_RULES_TAIL_INDEX_TIER: &str = "Image-grounded analysis: cite **On [slot name]:**. **Nearby** in thoughts must copy a bullet below character-for-character — if **`- R:`** is missing, **Inject match: NOT FOUND** and **hover_index** only; **forbidden** inventing **(left, top, right, bottom)**. **Verify:** judge **Expected vs Actual UI change** — pointer on target is **not** pass for click/copy goals. Overlay digits label bboxes only — **forbidden** treating digit position as the click point. Follow **communication** rules.";

/// Max overlay rows injected near the pointer for Primary / Intermediate tiers.
pub const MOUSE_NEARBY_REFERENCE_LIMIT: usize = 10;

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

/// Session coordinates in inject are non-negative integers (rounded), per tier communication md.
fn session_xy_int(x: f32, y: f32, cw: f32, ch: f32, coord: CoordinateSystem) -> (i32, i32) {
    let (sx, sy) = to_session_xy(x, y, cw, ch, coord);
    ((sx.round().max(0.0)) as i32, (sy.round().max(0.0)) as i32)
}

fn format_bbox_reference_row(b: &BoxInfo, cw: f32, ch: f32, coord: CoordinateSystem) -> String {
    let (left, top) = session_xy_int(b.x, b.y, cw, ch, coord);
    let (right, _) = session_xy_int(b.x + b.width, b.y, cw, ch, coord);
    let (_, bottom) = session_xy_int(b.x, b.y + b.height, cw, ch, coord);
    format!(
        "- {idx}: ({left}, {top}, {right}, {bottom})",
        idx = b.index,
        left = left,
        top = top,
        right = right,
        bottom = bottom,
    )
}

fn session_label(coord: CoordinateSystem) -> &'static str {
    match coord {
        CoordinateSystem::Qwen | CoordinateSystem::Kimi => "session 0-1000",
        CoordinateSystem::Pixel => "session pixels",
    }
}

fn center_distance_sq(b: &BoxInfo, mouse_bx: f32, mouse_by: f32) -> f32 {
    let (cx, cy) = b.center();
    let dx = cx - mouse_bx;
    let dy = cy - mouse_by;
    dx * dx + dy * dy
}

/// Pick up to `limit` boxes whose centers are closest to the pointer (capture pixel space).
pub fn select_boxes_near_pointer<'a>(
    boxes: &'a [BoxInfo],
    mouse_bx: f32,
    mouse_by: f32,
    limit: usize,
) -> Vec<&'a BoxInfo> {
    let mut ranked: Vec<_> = boxes
        .iter()
        .map(|b| (center_distance_sq(b, mouse_bx, mouse_by), b))
        .collect();
    ranked.sort_by(|a, b| {
        a.0.partial_cmp(&b.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.1.index.cmp(&b.1.index))
    });
    ranked.into_iter().take(limit).map(|(_, b)| b).collect()
}

fn format_overlay_reference_rows(
    header: &str,
    boxes: &[&BoxInfo],
    cw: f32,
    ch: f32,
    coord: CoordinateSystem,
) -> String {
    let mut lines = vec![header.to_string()];
    if boxes.is_empty() {
        lines.push("- none detected on this capture.".to_string());
        return lines.join("\n");
    }
    for b in boxes {
        lines.push(format_bbox_reference_row(b, cw, ch, coord));
    }
    lines.join("\n")
}

fn format_all_overlay_reference_bboxes(
    boxes: &[BoxInfo],
    cw: f32,
    ch: f32,
    coord: CoordinateSystem,
) -> String {
    let mut sorted: Vec<&BoxInfo> = boxes.iter().collect();
    sorted.sort_by_key(|b| b.index);
    let header = format!(
        "**Overlay reference bboxes** (lookup here for all coordinates — {}; sorted by index; each row **R: (left, top, right, bottom)**; Location line 3 must copy row R from this list; indices are **anchors only**, not click targets):",
        session_label(coord)
    );
    format_overlay_reference_rows(&header, &sorted, cw, ch, coord)
}

fn format_nearby_overlay_reference_bboxes(
    boxes: &[&BoxInfo],
    cw: f32,
    ch: f32,
    coord: CoordinateSystem,
    limit: usize,
) -> String {
    let header = format!(
        "**Nearby overlay reference bboxes** ({} indices nearest the **pointer** on this capture — {}; each row **R: (left, top, right, bottom)**; copy row **R** for **W/H** and **dx/dy** on index tools; sorted nearest-first; digits are **anchors only**, not click targets):",
        boxes.len().min(limit),
        session_label(coord)
    );
    format_overlay_reference_rows(&header, boxes, cw, ch, coord)
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
            let nx_i = nx.round().max(0.0) as i32;
            let ny_i = ny.round().max(0.0) as i32;
            format!(
                "**Pointer position** (same **full capture** as **`[Screen after action]`** / **`[Annotated after action]`**, origin top-left): **capture pixels** (x, y) ≈ ({:.0}, {:.0}); **normalized (x, y)** ≈ ({nx_i}, {ny_i}) on **0–1000** (non-negative integers; same space as coordinate tools this session). **`[Zoom pointer after action]`** is a **{out}×{out} px** patch ({factor}× magnified from {crop}×{crop} px) centered on this point.",
                mouse_bx, mouse_by
            )
        }
        CoordinateSystem::Pixel => format!(
            "**Pointer position** (capture pixels, origin top-left, same as **`[Screen after action]`**): (x, y) ≈ ({}, {}). **`[Zoom pointer after action]`** is a **{out}×{out} px** patch ({factor}× magnified from {crop}×{crop} px) centered on this point.",
            mouse_bx.round().max(0.0) as i32,
            mouse_by.round().max(0.0) as i32
        ),
    }
}

/// Prose block: **Pointer position** + coordinate guidance (no bbox rows).
pub fn format_pointer_coordinate_anchor(
    monitor: &MonitorInfo,
    capture_px: (u32, u32),
    global_pointer: (i32, i32),
    coord: CoordinateSystem,
) -> Option<String> {
    let (mouse_bx, mouse_by, cw, ch) = pointer_capture_position(monitor, capture_px, global_pointer)?;
    let pointer_line = format_pointer_position_line(mouse_bx, mouse_by, cw, ch, coord);
    Some(format!("{pointer_line}\n\n{INJECT_RULES_TAIL_INDEX_TIER}"))
}

/// **Pointer position** + up to **10** nearest **Overlay reference bboxes** (Primary / Intermediate).
pub fn format_mouse_nearby_reference_bboxes(
    boxes: &[BoxInfo],
    monitor: &MonitorInfo,
    capture_px: (u32, u32),
    global_pointer: (i32, i32),
    coord: CoordinateSystem,
) -> Option<String> {
    let (mouse_bx, mouse_by, cw, ch) = pointer_capture_position(monitor, capture_px, global_pointer)?;
    let pointer_line = format_pointer_position_line(mouse_bx, mouse_by, cw, ch, coord);
    let nearby = select_boxes_near_pointer(boxes, mouse_bx, mouse_by, MOUSE_NEARBY_REFERENCE_LIMIT);
    let bbox_block =
        format_nearby_overlay_reference_bboxes(&nearby, cw, ch, coord, MOUSE_NEARBY_REFERENCE_LIMIT);
    Some(format!(
        "{pointer_line}\n\n{INJECT_RULES_TAIL_INDEX_TIER}\n\n{bbox_block}"
    ))
}

/// **Pointer position** + **Overlay reference bboxes** (all indices on this capture; Advanced).
pub fn format_mouse_neighbor_reference_bboxes(
    boxes: &[BoxInfo],
    monitor: &MonitorInfo,
    capture_px: (u32, u32),
    global_pointer: (i32, i32),
    coord: CoordinateSystem,
) -> Option<String> {
    let (mouse_bx, mouse_by, cw, ch) = pointer_capture_position(monitor, capture_px, global_pointer)?;
    let pointer_line = format_pointer_position_line(mouse_bx, mouse_by, cw, ch, coord);
    let bbox_block = format_all_overlay_reference_bboxes(boxes, cw, ch, coord);
    Some(format!(
        "{pointer_line}\n\n{INJECT_RULES_TAIL_ADVANCED}\n\n{bbox_block}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn injects_pointer_position_and_rules_tail() {
        let monitor = MonitorInfo::new(0, 0, 1000, 1000);
        let s = format_pointer_coordinate_anchor(
            &monitor,
            (1000, 1000),
            (500, 500),
            CoordinateSystem::Qwen,
        )
        .expect("line");
        assert!(s.contains("**Pointer position**"));
        assert!(s.contains("Nearby overlay reference bboxes"));
        assert!(s.contains("[Zoom pointer after action]"));
        assert!(
            !s.contains("- 4: ("),
            "pointer-only helper should not include bbox rows: {s}"
        );
    }

    #[test]
    fn nearby_list_limits_to_ten_closest_to_pointer() {
        let monitor = MonitorInfo::new(0, 0, 1000, 1000);
        let mut boxes = Vec::new();
        for i in 0..20u32 {
            boxes.push(BoxInfo {
                index: i + 1,
                x: (i as f32) * 40.0,
                y: 500.0,
                width: 30.0,
                height: 30.0,
                confidence: 0.9,
            });
        }
        // Pointer near box 10 (center ~395,515)
        let s = format_mouse_nearby_reference_bboxes(
            &boxes,
            &monitor,
            (1000, 1000),
            (400, 520),
            CoordinateSystem::Qwen,
        )
        .expect("line");
        assert!(s.contains("**Nearby overlay reference bboxes**"));
        assert!(s.contains("10 indices nearest"));
        let row_count = s.lines().filter(|l| l.starts_with("- ")).count();
        assert_eq!(row_count, 10, "{s}");
        assert!(s.contains("- 10: ("));
        assert!(!s.contains("- 1: ("));
    }

    #[test]
    fn select_boxes_near_pointer_orders_by_distance() {
        let boxes = vec![
            BoxInfo {
                index: 1,
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
                confidence: 1.0,
            },
            BoxInfo {
                index: 2,
                x: 100.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
                confidence: 1.0,
            },
        ];
        let near = select_boxes_near_pointer(&boxes, 102.0, 5.0, 10);
        assert_eq!(near.len(), 2);
        assert_eq!(near[0].index, 2);
        assert_eq!(near[1].index, 1);
    }

    #[test]
    fn injects_all_overlay_reference_bbox_coordinates_sorted_by_index() {
        let monitor = MonitorInfo::new(0, 0, 1000, 1000);
        let boxes = vec![
            BoxInfo {
                index: 12,
                x: 100.0,
                y: 100.0,
                width: 50.0,
                height: 50.0,
                confidence: 0.9,
            },
            BoxInfo {
                index: 4,
                x: 450.0,
                y: 480.0,
                width: 120.0,
                height: 80.0,
                confidence: 0.9,
            },
        ];
        let s = format_mouse_neighbor_reference_bboxes(
            &boxes,
            &monitor,
            (1000, 1000),
            (500, 500),
            CoordinateSystem::Qwen,
        )
        .expect("line");
        assert!(s.contains("**Overlay reference bboxes**"));
        assert!(s.contains("- 4: ("));
        assert!(s.contains("- 12: ("));
        assert!(s.contains("450, 480, 570, 560"));
        assert!(!s.contains("top-right"));
        let pos_four = s.find("- 4: (").expect("index 4");
        let pos_twelve = s.find("- 12: (").expect("index 12");
        assert!(pos_four < pos_twelve, "must sort by index: {s}");
    }
}
