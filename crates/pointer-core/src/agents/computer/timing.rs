//! Tunable delays and desktop tool classification for the computer agent.

use log::debug;
use serde_json::Value;

/// Milliseconds to wait after a **successful** desktop input tool call (`mouse`, `hotkey`,
/// `composite_action`, `modified_click`) before the chat loop continues — i.e. before the next
/// model round where **screenshot / verify (`[CUR_SCREEN]`)** runs. Gives the OS and target UI time
/// to repaint. Used when **`wait`** is **omitted** from `tool_args` (see
/// [`post_desktop_action_delay_ms_from_tool_args`]).
pub const POST_DESKTOP_ACTION_DELAY_MS: u64 = 1000;

/// Allowed **`wait`** seconds in `tool_args` for post-action screenshot delay (inclusive).
pub const POST_DESKTOP_ACTION_WAIT_SEC_MIN: f64 = 1.0;
pub const POST_DESKTOP_ACTION_WAIT_SEC_MAX: f64 = 5.0;

/// Milliseconds after a successful **`mouse`** / **`hotkey`** / **`composite_action`** / **`modified_click`**
/// before the next **`[CUR_SCREEN]`** capture. Reads optional **`wait`** from `tool_args` (seconds,
/// **1.0–5.0** clamped); if missing, invalid, non-positive, or not an object, returns
/// [`POST_DESKTOP_ACTION_DELAY_MS`].
pub fn post_desktop_action_delay_ms_from_tool_args(args: &Value) -> u64 {
    let Value::Object(map) = args else {
        return POST_DESKTOP_ACTION_DELAY_MS;
    };
    let Some(raw) = map.get("wait") else {
        return POST_DESKTOP_ACTION_DELAY_MS;
    };
    let sec = match raw {
        Value::Number(n) => n.as_f64().unwrap_or(f64::NAN),
        Value::String(s) => s.trim().parse::<f64>().unwrap_or(f64::NAN),
        _ => {
            debug!("post_action wait: ignored non-number wait value");
            return POST_DESKTOP_ACTION_DELAY_MS;
        }
    };
    if !sec.is_finite() || sec <= 0.0 {
        debug!("post_action wait: non-positive or non-finite, using default ms");
        return POST_DESKTOP_ACTION_DELAY_MS;
    }
    let clamped = sec.clamp(POST_DESKTOP_ACTION_WAIT_SEC_MIN, POST_DESKTOP_ACTION_WAIT_SEC_MAX);
    if (clamped - sec).abs() > f64::EPSILON {
        debug!(
            "post_action wait: clamped wait {}s to {}s before screenshot",
            sec, clamped
        );
    }
    ((clamped * 1000.0).round() as u64).max(1)
}

/// After [`super::actions::ActionExecutor`] moves the cursor to an absolute target, wait before
/// click/scroll/drag so the OS and target app can update hit-testing (hover, focus, animations).
pub const SETTLE_AFTER_ABSOLUTE_MOVE_MS: u64 = 100;

/// After any synthetic mouse button gesture (left / right / double click), brief pause before the
/// tool returns so the target app can process the event. Used by all [`super::actions::ActionExecutor`]
/// click entry points (`click_at`, `click_here`, `double_click_*`, `right_click_*`, batch clicks).
pub const POST_MOUSE_BUTTON_SETTLE_MS: u64 = 50;

/// Milliseconds between **sub-steps inside one composite desktop action** in [`super::actions::ActionExecutor`]
/// (e.g. after focus click, before `type_text`; after select-all, before typing; after move+settle,
/// before scroll). Tunable in one place; independent of [`POST_DESKTOP_ACTION_DELAY_MS`].
pub const COMPOSITE_ACTION_STEP_GAP_MS: u64 = 50;

/// Milliseconds between the two physical clicks of a synthetic double-click.
/// Applied by the concrete input backend so the host app can recognize it
/// as one double-click gesture reliably across platforms.
pub const DOUBLE_CLICK_INTERVAL_MS: u64 = 60;

/// Default waypoint count for cursor path planning (aligned with Python `MouseMove` ~10 points).
pub const MOUSE_MOVE_DEFAULT_POINT_COUNT: usize = 10;

/// Default total cursor move duration (seconds) when using eased total-time mode.
/// Matches Python `MouseHelper.move_to_position(..., duration=0.5)`.
pub const MOUSE_MOVE_TOTAL_DURATION_SECS: f64 = 0.5;

/// Fast / non-human-like move duration (Python `pyautogui.moveTo(..., duration=0.05)`).
pub const MOUSE_MOVE_FAST_DURATION_SECS: f64 = 0.05;

/// Tools recorded under `[CUR_SCREEN]` as recent desktop rows (goal/action repetition hints). Includes `wait`
/// so the model sees explicit pauses even though `wait` does not move the pointer. Includes `clipboard` for
/// copy/paste verification chains even though it does not move the pointer.
pub const DESKTOP_VISION_LOG_TOOL_IDS: &[&str] = &[
    "mouse",
    "hotkey",
    "composite_action",
    "modified_click",
    "wait",
    "clipboard",
];

/// Tools that actually drive or schedule desktop interaction; **`wait` excluded** — it already blocks and
/// does not need an extra post-call delay.
pub const DESKTOP_POST_DELAY_TOOL_IDS: &[&str] = &[
    "mouse",
    "hotkey",
    "composite_action",
    "modified_click",
];

#[inline]
pub fn is_desktop_vision_log_tool(tool_id: &str) -> bool {
    DESKTOP_VISION_LOG_TOOL_IDS.contains(&tool_id)
}

#[inline]
pub fn is_desktop_post_delay_tool(tool_id: &str) -> bool {
    DESKTOP_POST_DELAY_TOOL_IDS.contains(&tool_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn post_delay_default_when_wait_missing() {
        assert_eq!(
            post_desktop_action_delay_ms_from_tool_args(&json!({"goal": "x"})),
            POST_DESKTOP_ACTION_DELAY_MS
        );
    }

    #[test]
    fn post_delay_clamps_wait_seconds() {
        assert_eq!(post_desktop_action_delay_ms_from_tool_args(&json!({"wait": 2.5})), 2500);
        assert_eq!(post_desktop_action_delay_ms_from_tool_args(&json!({"wait": 0.2})), 1000);
        assert_eq!(post_desktop_action_delay_ms_from_tool_args(&json!({"wait": 9})), 5000);
        assert_eq!(
            post_desktop_action_delay_ms_from_tool_args(&json!({"wait": 0})),
            POST_DESKTOP_ACTION_DELAY_MS
        );
    }
}
