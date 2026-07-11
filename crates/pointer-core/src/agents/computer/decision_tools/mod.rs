//! Decision-phase tool names (Advanced pipeline). Verify-only host path still uses
//! `is_decision_tool` when sanitizing or routing position helpers on legacy args.

const ALL_TOOLS: &[&str] = &[
    "click",
    "double_click",
    "right_click",
    "hover",
    "scroll",
    "drag",
    "input",
    "modified_click",
    "mouse_move",
];

/// Check whether a tool name belongs to the decision-phase tool set.
pub fn is_decision_tool(name: &str) -> bool {
    let n = name.trim().to_ascii_lowercase();
    ALL_TOOLS.contains(&n.as_str())
}
