//! **Pointer position** numeric line and **`[Zoom pointer after action]`** anchor prose for `[CUR_SCREEN]`.
//! Coordinate-based tools use the pointer zoom crop as the visual anchor (not a separate bbox text list).

use super::coord::CoordinateSystem;
use super::screen::MonitorInfo;

/// Same side length as `[Zoom pointer after action]` (`screen_overlay::ZOOM_POINTER_SIDE`).
pub const POINTER_VICINITY_SIDE_PX: f32 = 300.0;

const ZOOM_POINTER_ANCHOR: &str = "**Pointer coordinate anchor:** Use the attached **`[Zoom pointer after action]`** image — the **300×300 px** annotated crop centered on **Pointer position** above (overlay digits and **bbox** border colors match **`[Annotated after action]`**). For **coordinate-based** `*_at` calls, read the **line 1** sub-target’s placement **on that crop** relative to the synthetic pointer hotspot, then map to session **(x, y)** using the **Pointer position** line (full-capture space). Do **not** treat overlay **`index`** on this crop as a click target when the **bbox** wraps multiple controls. If the sub-target is **not visible** inside **`[Zoom pointer after action]`**, anchor from **`[Screen after action]`** or the **`Location:`** line 1 overlay frame instead — still derive **(x, y)** from visible layout, not from memory.";

fn format_pointer_position_line(
    mouse_bx: f32,
    mouse_by: f32,
    cw: f32,
    ch: f32,
    coord: CoordinateSystem,
) -> String {
    match coord {
        CoordinateSystem::Qwen | CoordinateSystem::Kimi => {
            let nx = (mouse_bx / cw) * 1000.0;
            let ny = (mouse_by / ch) * 1000.0;
            format!(
                "**Pointer position** (same **full capture** as **`[Screen after action]`** / **`[Annotated after action]`**, origin top-left): **capture pixels** (x, y) ≈ ({:.0}, {:.0}); **normalized (x, y)** ≈ ({:.1}, {:.1}) on **0–1000** (same numeric space as coordinate-based `mouse` / `composite_action` / `modified_click` this session). **`[Zoom pointer after action]`** is the **300×300 px** crop centered on this point.",
                mouse_bx, mouse_by, nx, ny
            )
        }
        CoordinateSystem::Pixel => format!(
            "**Pointer position** (capture pixels, origin top-left, same as **`[Screen after action]`**): (x, y) ≈ ({:.0}, {:.0}). **`[Zoom pointer after action]`** is the **300×300 px** crop centered on this point.",
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

    let pointer_line = format_pointer_position_line(mouse_bx, mouse_by, cw, ch, coord);
    Some(format!("{pointer_line}\n\n{ZOOM_POINTER_ANCHOR}"))
}

/// Back-compat alias (call sites may still use the old name).
pub fn format_mouse_neighbor_reference_bboxes(
    _boxes: &[super::annotate::BoxInfo],
    monitor: &MonitorInfo,
    capture_px: (u32, u32),
    global_pointer: (i32, i32),
    coord: CoordinateSystem,
) -> Option<String> {
    format_pointer_coordinate_anchor(monitor, capture_px, global_pointer, coord)
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
        assert!(s.contains("**Pointer coordinate anchor:**"));
        assert!(s.contains("[Zoom pointer after action]"));
        assert!(
            !s.contains("Pointer neighbor reference bboxes"),
            "bbox list removed: {s}"
        );
        assert!(!s.contains("overlay index 1 (left"), "no bbox rows: {s}");
    }
}
