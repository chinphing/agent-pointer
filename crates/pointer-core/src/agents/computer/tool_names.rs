//! Flat computer tool ids — single source of truth for Rust code (prompts mirror these names).

/// Sidecar flat tool: report verify / repetition signal for tier runtime.
pub const ACTION_VERIFY: &str = "action_verify";

/// Retired allow-list id remapped to [`ACTION_VERIFY`] in [`crate::tools::remap_split_computer_tool_allow_names`].
pub const ACTION_VERIFY_LEGACY_UNDERSCORE: &str = "verify_report";

/// Whether a tool-call name refers to the action-verify sidecar.
pub fn is_action_verify_tool_name(name: &str) -> bool {
    name.trim() == ACTION_VERIFY
}
