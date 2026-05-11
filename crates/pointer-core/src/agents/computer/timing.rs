//! Tunable delays and desktop tool classification for the computer agent.

/// Milliseconds to wait after a **successful** desktop input tool call before continuing the chat loop.
pub const POST_DESKTOP_ACTION_DELAY_MS: u64 = 200;

/// Tools recorded under `[CUR_SCREEN]` as recent desktop rows (goal/action repetition hints). Includes `wait`
/// so the model sees explicit pauses even though `wait` does not move the pointer.
pub const DESKTOP_VISION_LOG_TOOL_IDS: &[&str] = &[
    "mouse",
    "hotkey",
    "composite_action",
    "modified_click",
    "wait",
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
