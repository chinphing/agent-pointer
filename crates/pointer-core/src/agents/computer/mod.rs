//! Computer agent: desktop input, vision pipeline, tier runtime, session state, and tools.

pub mod capture_debug;
pub mod extension_hooks;
pub mod input;
pub mod state;
pub mod tier;
pub mod tool_names;
pub mod tools;
pub mod verify;
pub mod vision;

// --- Stable paths for the rest of the crate (`crate::agents::computer::…`) ---
pub use input::actions;
pub use input::enigo as action_enigo;
pub use input::{mouse_move, mouse_path, timing};
pub use state::{
    ComputerSession, ComputerState, DesktopToolEntry, ScreenCaptureResult, SessionStatus,
};
pub use tier::{
    current_computer_tier, tier_allows_index_tools, ComputerRoundLlmOverrides, ComputerTier,
    ComputerTierConfig, ComputerTierGuard, ComputerTierRuntime, CONFIG_KEY_AUTO_UPGRADE,
    CONFIG_KEY_INITIAL_TIER, CONFIG_KEY_MODEL_ADVANCED, CONFIG_KEY_MODEL_PRIMARY,
};
pub use tool_names::{
    is_action_verify_tool_name, normalize_action_verify_invocation, ACTION_VERIFY,
    ACTION_VERIFY_LEGACY_BASE, ACTION_VERIFY_LEGACY_METHOD_REPORT,
    ACTION_VERIFY_LEGACY_QUALIFIED, ACTION_VERIFY_LEGACY_TOOL_IDS,
    ACTION_VERIFY_LEGACY_UNDERSCORE,
};
pub use timing::{
    is_desktop_post_delay_tool, is_desktop_vision_log_tool,
    post_desktop_action_delay_ms_from_tool_args, COMPOSITE_ACTION_STEP_GAP_MS,
    POST_DESKTOP_ACTION_DELAY_MS, POST_DESKTOP_ACTION_WAIT_SEC_MAX,
    POST_DESKTOP_ACTION_WAIT_SEC_MIN,
};
pub use vision::{annotate, coord, reference_anchors, screen, screen_overlay, vision_state};
