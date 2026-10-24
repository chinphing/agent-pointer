//! Computer agent: desktop input, vision pipeline, tier runtime, session state, and tools.

pub mod capture_debug;
pub mod control;
pub mod decision_tools;
pub mod extension_hooks;
pub mod input;
pub mod locate;
pub mod pipeline;
pub mod runtime;
pub mod state;
pub mod tool_names;
pub mod tools;
pub mod verify;
pub mod vision;

// --- Stable paths for the rest of the crate (`crate::agents::computer::…`) ---
pub use input::actions;
#[cfg(feature = "computer")]
pub use input::enigo as action_enigo;
pub use input::{mouse_move, mouse_path, timing};
pub use state::{
    ComputerSession, ComputerState, DesktopToolEntry, ScreenCaptureResult, SessionStatus,
};
pub use runtime::{
    ComputerModelConfig, ComputerRuntime,
};
pub use timing::{
    is_desktop_post_delay_tool, is_desktop_vision_log_tool,
    post_desktop_action_delay_ms_from_tool_args, COMPOSITE_ACTION_STEP_GAP_MS,
    POST_DESKTOP_ACTION_DELAY_MS, POST_DESKTOP_ACTION_WAIT_SEC_MAX,
    POST_DESKTOP_ACTION_WAIT_SEC_MIN,
};
pub use vision::{coord, screen, screen_overlay, vision_state};
