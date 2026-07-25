//! macOS on-screen window detection for launch verification (CGWindowList).

use core_foundation::base::{CFType, TCFType};
use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
use core_foundation::number::{CFNumber, CFNumberRef};
use core_foundation::string::CFString;
use core_graphics::display::CGDisplay;
use core_graphics::window::{
    copy_window_info, kCGNullWindowID, kCGWindowListExcludeDesktopElements,
    kCGWindowListOptionOnScreenOnly,
};

use crate::agents::computer::screen::XCAP_MONITOR_ID_PREFIX;

/// Minimum width/height for a window to count as user-visible (filters 1px helpers).
const MIN_VISIBLE_DIMENSION: f64 = 50.0;

type WindowDict = CFDictionary<CFString, CFType>;

/// Returns true when `pid` owns at least one normal on-screen window with usable size.
pub fn has_onscreen_window_for_pid(pid: i32) -> bool {
    largest_window_quartz_bounds(pid).is_some()
}

/// Capture monitor id (`xcap:{CGDirectDisplayID}`) for the largest on-screen window of `pid`.
pub fn monitor_id_for_pid(pid: i32) -> Option<String> {
    let (x, y, width, height) = largest_window_quartz_bounds(pid)?;
    let qcx = x + width / 2.0;
    let qcy = y + height / 2.0;
    monitor_id_for_quartz_point(qcx, qcy)
}

/// Center of the largest on-screen user window for `pid` (top-left global coordinates for xcap).
pub fn primary_window_center_top_left_for_pid(pid: i32) -> Option<(i32, i32)> {
    let (x, y, width, height) = largest_window_quartz_bounds(pid)?;
    let qcx = x + width / 2.0;
    let qcy = y + height / 2.0;
    quartz_point_to_top_left_global(qcx, qcy)
}

/// Center of the frontmost app's largest visible window.
pub fn frontmost_window_center_top_left() -> Option<(i32, i32)> {
    use objc2_app_kit::NSWorkspace;

    let workspace = NSWorkspace::sharedWorkspace();
    let pid = workspace
        .frontmostApplication()
        .map(|app| app.processIdentifier())?;
    primary_window_center_top_left_for_pid(pid)
}

/// Map a CGWindow / CGDisplay quartz point to a capture monitor id.
fn monitor_id_for_quartz_point(qx: f64, qy: f64) -> Option<String> {
    let display_ids = CGDisplay::active_displays().ok()?;
    for display_id in display_ids {
        let bounds = CGDisplay::new(display_id).bounds();
        let bx = bounds.origin.x;
        let by = bounds.origin.y;
        let bw = bounds.size.width;
        let bh = bounds.size.height;
        if qx < bx || qx >= bx + bw || qy < by || qy >= by + bh {
            continue;
        }
        let monitor_id = format!("{XCAP_MONITOR_ID_PREFIX}{display_id}");
        log::info!(
            "launch_app macOS: window center quartz=({qx:.0},{qy:.0}) -> monitor={monitor_id} \
             (display origin=({bx:.0},{by:.0}) size={bw:.0}x{bh:.0})"
        );
        return Some(monitor_id);
    }
    log::warn!("launch_app macOS: no CGDisplay contains quartz point ({qx:.0},{qy:.0})");
    if let Ok(ids) = CGDisplay::active_displays() {
        for display_id in ids {
            let b = CGDisplay::new(display_id).bounds();
            log::warn!(
                "launch_app macOS: display {display_id} bounds origin=({}, {}) size={}x{}",
                b.origin.x,
                b.origin.y,
                b.size.width,
                b.size.height
            );
        }
    }
    None
}

fn largest_window_quartz_bounds(pid: i32) -> Option<(f64, f64, f64, f64)> {
    let windows = copy_window_info(
        kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements,
        kCGNullWindowID,
    )?;
    let mut best: Option<(f64, f64, f64, f64)> = None;
    let mut best_area = 0.0_f64;

    for item in windows.iter() {
        let dict: WindowDict =
            unsafe { CFDictionary::wrap_under_get_rule(*item as CFDictionaryRef) };
        if !window_dict_matches_pid(&dict, pid) || !window_dict_is_user_visible(&dict) {
            continue;
        }
        let Some((x, y, width, height)) = window_bounds_rect(&dict) else {
            continue;
        };
        let area = width * height;
        if area > best_area {
            best_area = area;
            best = Some((x, y, width, height));
        }
    }

    if best.is_none() {
        log::warn!("launch_app macOS: no on-screen window bounds for pid={pid}");
    }
    best
}

/// Map a Quartz bottom-left global point to xcap top-left global coordinates via CGDisplayBounds.
fn quartz_point_to_top_left_global(qx: f64, qy: f64) -> Option<(i32, i32)> {
    let monitors = crate::agents::computer::screen::list_monitors().ok()?;
    if monitors.is_empty() {
        return None;
    }
    let display_ids = CGDisplay::active_displays().ok()?;
    for display_id in display_ids {
        let bounds = CGDisplay::new(display_id).bounds();
        let bx = bounds.origin.x;
        let by = bounds.origin.y;
        let bw = bounds.size.width;
        let bh = bounds.size.height;
        if qx < bx || qx >= bx + bw || qy < by || qy >= by + bh {
            continue;
        }
        let monitor = monitors.iter().find(|m| {
            m.id.strip_prefix(XCAP_MONITOR_ID_PREFIX)
                .and_then(|s| s.parse::<u32>().ok())
                .is_some_and(|xid| xid == display_id)
        })?;
        let local_x = qx - bx;
        let local_y_top = bh - (qy - by);
        let global_x = monitor.left + local_x.round() as i32;
        let global_y = monitor.top + local_y_top.round() as i32;
        return Some((global_x, global_y));
    }
    None
}

fn window_dict_matches_pid(dict: &WindowDict, pid: i32) -> bool {
    cf_dict_i64(dict, "kCGWindowOwnerPID") == Some(pid as i64)
}

fn window_dict_is_user_visible(dict: &WindowDict) -> bool {
    let layer = cf_dict_i64(dict, "kCGWindowLayer").unwrap_or(-1);
    if layer != 0 {
        return false;
    }
    let Some((width, height)) = window_bounds_size(dict) else {
        return false;
    };
    width >= MIN_VISIBLE_DIMENSION && height >= MIN_VISIBLE_DIMENSION
}

fn window_bounds_size(dict: &WindowDict) -> Option<(f64, f64)> {
    let (_, _, width, height) = window_bounds_rect(dict)?;
    Some((width, height))
}

fn window_bounds_rect(dict: &WindowDict) -> Option<(f64, f64, f64, f64)> {
    let key = CFString::new("kCGWindowBounds");
    let bounds_ref = dict.find(&key)?;
    let bounds: WindowDict =
        unsafe { CFDictionary::wrap_under_get_rule(bounds_ref.as_CFTypeRef() as CFDictionaryRef) };
    Some((
        cf_dict_num(&bounds, "X")?,
        cf_dict_num(&bounds, "Y")?,
        cf_dict_num(&bounds, "Width")?,
        cf_dict_num(&bounds, "Height")?,
    ))
}

fn cf_dict_i64(dict: &WindowDict, key: &str) -> Option<i64> {
    let key = CFString::new(key);
    let val = dict.find(&key)?;
    let num: CFNumber = unsafe { CFNumber::wrap_under_get_rule(val.as_CFTypeRef() as CFNumberRef) };
    num.to_i64()
}

fn cf_dict_f64(dict: &WindowDict, key: &str) -> Option<f64> {
    let key = CFString::new(key);
    let val = dict.find(&key)?;
    let num: CFNumber = unsafe { CFNumber::wrap_under_get_rule(val.as_CFTypeRef() as CFNumberRef) };
    num.to_f64()
}

fn cf_dict_num(dict: &WindowDict, key: &str) -> Option<f64> {
    cf_dict_f64(dict, key).or_else(|| cf_dict_i64(dict, key).map(|v| v as f64))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn min_visible_dimension_is_reasonable() {
        assert!(MIN_VISIBLE_DIMENSION >= 32.0);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn wechat_secondary_display_quartz_center_maps_to_xcap_3() {
        let id = monitor_id_for_quartz_point(1674.0, -643.0);
        assert_eq!(id.as_deref(), Some("xcap:3"), "got {id:?}");
    }
}
