//! macOS on-screen window detection for launch verification (CGWindowList).

use core_foundation::base::{CFType, TCFType};
use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
use core_foundation::number::{CFNumber, CFNumberRef};
use core_foundation::string::CFString;
use core_graphics::window::{
    copy_window_info, kCGNullWindowID, kCGWindowListExcludeDesktopElements,
    kCGWindowListOptionOnScreenOnly,
};

/// Minimum width/height for a window to count as user-visible (filters 1px helpers).
const MIN_VISIBLE_DIMENSION: f64 = 50.0;

type WindowDict = CFDictionary<CFString, CFType>;

/// Returns true when `pid` owns at least one normal on-screen window with usable size.
pub fn has_onscreen_window_for_pid(pid: i32) -> bool {
    let Some(windows) = copy_window_info(
        kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements,
        kCGNullWindowID,
    ) else {
        log::warn!("launch_app macOS: CGWindowListCopyWindowInfo returned null");
        return false;
    };

    for item in windows.iter() {
        let dict: WindowDict = unsafe {
            CFDictionary::wrap_under_get_rule(*item as CFDictionaryRef)
        };
        if !window_dict_matches_pid(&dict, pid) {
            continue;
        }
        if window_dict_is_user_visible(&dict) {
            return true;
        }
    }
    false
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
    let key = CFString::new("kCGWindowBounds");
    let bounds_ref = dict.find(&key)?;
    let bounds: WindowDict = unsafe {
        CFDictionary::wrap_under_get_rule(bounds_ref.as_CFTypeRef() as CFDictionaryRef)
    };
    let width = cf_dict_f64(&bounds, "Width")?;
    let height = cf_dict_f64(&bounds, "Height")?;
    Some((width, height))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn min_visible_dimension_is_reasonable() {
        assert!(MIN_VISIBLE_DIMENSION >= 32.0);
    }
}
