//! Flat computer tool ids — single source of truth for Rust code (prompts mirror these names).

use serde_json::Value;

/// Sidecar flat tool: report verify / repetition signal for tier runtime.
pub const ACTION_VERIFY: &str = "action_verify";

/// Legacy registry / call aliases (history + model drift).
pub const ACTION_VERIFY_LEGACY_BASE: &str = "verify";
pub const ACTION_VERIFY_LEGACY_QUALIFIED: &str = "verify:report";
pub const ACTION_VERIFY_LEGACY_UNDERSCORE: &str = "verify_report";
pub const ACTION_VERIFY_LEGACY_METHOD_REPORT: &str = "report";

/// Whether a tool-call name refers to the action-verify sidecar (any generation).
pub fn is_action_verify_tool_name(name: &str) -> bool {
    let t = name.trim();
    t == ACTION_VERIFY
        || t == ACTION_VERIFY_LEGACY_QUALIFIED
        || t == ACTION_VERIFY_LEGACY_UNDERSCORE
        || t == ACTION_VERIFY_LEGACY_BASE
}

/// Legacy ids accepted on the wire before normalization to [`ACTION_VERIFY`].
pub const ACTION_VERIFY_LEGACY_TOOL_IDS: &[&str] = &[
    ACTION_VERIFY_LEGACY_UNDERSCORE,
    ACTION_VERIFY_LEGACY_QUALIFIED,
    ACTION_VERIFY_LEGACY_BASE,
];

/// Map a tool-call name (and optional legacy `method` arg) to [`ACTION_VERIFY`] when applicable.
pub fn normalize_action_verify_invocation(raw_name: &str, args: &Value) -> Option<&'static str> {
    let raw = raw_name.trim();
    if raw == ACTION_VERIFY
        || raw == ACTION_VERIFY_LEGACY_UNDERSCORE
        || raw == ACTION_VERIFY_LEGACY_QUALIFIED
    {
        return Some(ACTION_VERIFY);
    }
    if raw == ACTION_VERIFY_LEGACY_BASE
        && args
            .get("method")
            .and_then(|v| v.as_str())
            .is_some_and(|m| m.eq_ignore_ascii_case(ACTION_VERIFY_LEGACY_METHOD_REPORT))
    {
        return Some(ACTION_VERIFY);
    }
    None
}
