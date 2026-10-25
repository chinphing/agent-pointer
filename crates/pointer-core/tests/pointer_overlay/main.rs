//! Pointer-overlay accuracy, measured on a real sudoku board.
//!
//! Verify draws the pointer onto the before frame so it can tell "clicked this
//! cell" from "clicked the neighbour". That judgement only works if the drawn
//! pointer lands exactly where the caller asked — and on Retina a logical-vs-
//! physical mix-up shifts every drawn point by half, which looks almost right
//! to the eye while verify stably picks the wrong cell.
//!
//! The fixture is a 1920×1080 capture of the secondary display with a 9×9 sudoku
//! board on it. Each of the 81 cells is a test point.
//!
//! These tests do not call a model, so they run by default:
//!
//! ```bash
//! cargo test -p pointer-core --test pointer_overlay
//! ```

use std::path::PathBuf;

use anyhow::Result;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/pointer_overlay")
}

/// Board bounds in `fixtures/sudoku.png`, measured by scanning for dark line
/// pixels (`examples/grid_probe.rs`). The heavy 3×3 rules sit at
/// x = 99 / 222 / 346 / 469 and y = 237 / 360 / 484 / 608, so the board is
/// 370×371 with a 41.1 px cell.
const SUDOKU_GRID: (i32, i32, i32, i32) = (99, 237, 469, 608);
/// Hotspot of the pointer bitmap — the pixel that means "here".
const POINTER_HOTSPOT: (i32, i32) = (3, 2);
const POINTER_BITMAP: u32 = 32;

fn sudoku_cell_center(col: usize, row: usize) -> (i32, i32) {
    let (left, top, right, bottom) = SUDOKU_GRID;
    let cell_w = (right - left) as f64 / 9.0;
    let cell_h = (bottom - top) as f64 / 9.0;
    (
        left + ((col as f64 + 0.5) * cell_w) as i32,
        top + ((row as f64 + 0.5) * cell_h) as i32,
    )
}

/// Top-left corner of the cell at grid position `(col, row)`, in fixture pixels.
/// `col` / `row` may be 9, which yields the board's right / bottom edge.
fn cell_bounds(col: usize, row: usize) -> (i32, i32) {
    let (left, top, right, bottom) = SUDOKU_GRID;
    let cell_w = (right - left) as f64 / 9.0;
    let cell_h = (bottom - top) as f64 / 9.0;
    (
        left + (col as f64 * cell_w).round() as i32,
        top + (row as f64 * cell_h).round() as i32,
    )
}

/// Bounding box of every pixel that differs between two frames, inside `window`.
///
/// Scanning the whole 1920×1080 frame 81 times takes ~40 s; the pointer only
/// ever touches a 32×32 patch, so the search is bounded.
fn diff_bbox(
    a: &image::RgbaImage,
    b: &image::RgbaImage,
    window: (u32, u32, u32, u32),
) -> Option<(u32, u32, u32, u32)> {
    use image::GenericImageView;
    let (w, h) = a.dimensions();
    let (wx0, wy0, wx1, wy1) = window;
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0u32, 0u32);
    for y in wy0..wy1.min(h) {
        for x in wx0..wx1.min(w) {
            if a.get_pixel(x, y) != b.get_pixel(x, y) {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x);
                y1 = y1.max(y);
            }
        }
    }
    (x0 != u32::MAX).then_some((x0, y0, x1, y1))
}

/// Every cell of a real sudoku board is a test point: draw the pointer at its
/// centre and assert the changed pixels start at the hotspot offset. A regression
/// in the logical-vs-physical coordinate conversion shows up here as a box at
/// half the offset — on Retina that is exactly the bug this catches.
#[test]
fn pointer_overlay_lands_on_every_sudoku_cell() -> Result<()> {
    use pointer_core::agents::computer::vision::screen::MonitorInfo;
    use pointer_core::agents::computer::vision::screen_overlay::redraw_with_current_pointer;

    let path = dir().join("fixtures/sudoku.png");
    assert!(path.exists(), "missing fixture: {}", path.display());
    let raw = std::fs::read(&path)?;
    let full = image::load_from_memory(&raw)?.to_rgba8();

    // Work on the board plus a margin: `redraw_with_current_pointer` decodes and
    // re-encodes the whole frame, so 81 points against a full 1920×1080 capture
    // costs ~37 s. Cropping first keeps the fixture realistic and the test fast.
    const MARGIN: u32 = 60;
    let (gl, gt, gr, gb) = SUDOKU_GRID;
    let crop_x = (gl as u32).saturating_sub(MARGIN);
    let crop_y = (gt as u32).saturating_sub(MARGIN);
    let crop_w = (gr as u32 - crop_x) + MARGIN;
    let crop_h = (gb as u32 - crop_y) + MARGIN;
    let original = image::imageops::crop_imm(&full, crop_x, crop_y, crop_w, crop_h).to_image();
    let (w, h) = original.dimensions();
    let monitor = MonitorInfo::new(crop_x as i32, crop_y as i32, w as i32, h as i32);

    // Encode once; `redraw_with_current_pointer` decodes from bytes, so both
    // sides of the diff come from the same source.
    let mut png = Vec::new();
    original.write_to(
        &mut std::io::Cursor::new(&mut png),
        image::ImageFormat::Png,
    )?;
    // A point outside the frame draws nothing — that is the baseline.
    let baseline = image::load_from_memory(&redraw_with_current_pointer(
        &png,
        &monitor,
        (-100, -100),
    )?)?;
    let baseline = baseline.to_rgba8();

    let mut failures = Vec::new();
    for row in 0..9 {
        for col in 0..9 {
            let (cx, cy) = sudoku_cell_center(col, row);
            let drawn = image::load_from_memory(&redraw_with_current_pointer(
                &png,
                &monitor,
                (cx, cy),
            )?)?;
            let drawn = drawn.to_rgba8();
            // `redraw_with_current_pointer` takes global coordinates and maps them
            // through the monitor origin; the diff window lives in crop pixels.
            let (lx, ly) = (cx - crop_x as i32, cy - crop_y as i32);
            let window = (
                (lx - 16).max(0) as u32,
                (ly - 16).max(0) as u32,
                (lx as u32 + 48).min(w),
                (ly as u32 + 48).min(h),
            );
            let Some((x0, y0, x1, y1)) = diff_bbox(&baseline, &drawn, window) else {
                failures.push(format!("r{row}c{col} ({cx},{cy}): pointer drew nothing"));
                continue;
            };
            // The bitmap is pasted at `(x - hotspot)`; its black outline reaches a
            // few pixels past that corner, and JPEG softens the rest. Anchor the
            // assertion on the diff corner with that slack — a coordinate
            // conversion bug (logical vs physical) lands tens of pixels away, so
            // this still catches it while tolerating the outline.
            let (want_x0, want_y0) = (lx - POINTER_HOTSPOT.0, ly - POINTER_HOTSPOT.1);
            let slack = 6i64;
            let off = |got: u32, want: i32| (got as i64 - want as i64).abs();
            if off(x0, want_x0) > slack || off(y0, want_y0) > slack {
                failures.push(format!(
                    "r{row}c{col} ({cx},{cy}): diff starts at ({x0},{y0}), want ({want_x0},{want_y0}) ±{slack}"
                ));
                continue;
            }
            let (box_w, box_h) = (x1 - x0 + 1, y1 - y0 + 1);
            let cap = POINTER_BITMAP + 12;
            if box_w > cap || box_h > cap {
                failures.push(format!(
                    "r{row}c{col} ({cx},{cy}): diff box {box_w}×{box_h} exceeds the pointer footprint"
                ));
                continue;
            }
            // A real pointer covers most of its 32×32 footprint; a stray
            // JPEG ripple covers a handful of pixels.
            if box_w < 12 || box_h < 12 {
                failures.push(format!(
                    "r{row}c{col} ({cx},{cy}): diff box {box_w}×{box_h} is too small for the pointer"
                ));
                continue;
            }
            // The arrow tip — the corner the bitmap anchors on — must sit inside
            // the cell it was aimed at. The body extends down-right past that
            // corner and may cross a rule; the tip is what marks the click.
            let (cell_x0, cell_y0) = cell_bounds(col, row);
            let (cell_x1, cell_y1) = cell_bounds(col + 1, row + 1);
            let tip_x = x0 as i32 + crop_x as i32;
            let tip_y = y0 as i32 + crop_y as i32;
            if tip_x < cell_x0 || tip_x >= cell_x1 || tip_y < cell_y0 || tip_y >= cell_y1 {
                failures.push(format!(
                    "r{row}c{col} ({cx},{cy}): pointer tip at ({tip_x},{tip_y}) is outside cell ({cell_x0},{cell_y0})-({cell_x1},{cell_y1})"
                ));
            }
        }
    }

    assert!(
        failures.is_empty(),
        "{} of 81 cells mis-placed:\n{}",
        failures.len(),
        failures.join("\n")
    );
    Ok(())
}

/// Cell centres must sit inside the measured board — a typo in `SUDOKU_GRID`
/// fails loudly instead of silently shifting all 81 test points.
#[test]
fn sudoku_cell_centres_stay_inside_the_board() {
    let (left, top, right, bottom) = SUDOKU_GRID;
    for row in 0..9 {
        for col in 0..9 {
            let (cx, cy) = sudoku_cell_center(col, row);
            assert!(
                cx > left && cx < right && cy > top && cy < bottom,
                "r{row}c{col} centre ({cx},{cy}) is outside the board"
            );
        }
    }
}
