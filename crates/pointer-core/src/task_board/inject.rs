//! Host-injected session binding for `task_board` and computer tools.

use serde_json::Value;

pub use crate::task_board::coordination::parent_child::sub_agent_task_board_store_key;

/// Host-only binding for `task_board` and computer tools so models cannot spoof another session id.
pub fn inject_host_task_board_conversation_id(
    tool_id: &str,
    args: Value,
    conversation_id: &str,
) -> Value {
    let requires_injection = tool_id == "task_board"
        || tool_id.starts_with("task_board:")
        || crate::agents::computer::is_desktop_vision_log_tool(tool_id)
        || crate::agents::computer::is_desktop_post_delay_tool(tool_id);
    if !requires_injection {
        return args;
    }
    let mut map = if let Value::Object(m) = args {
        m
    } else {
        serde_json::Map::new()
    };
    map.insert(
        "_conversation_id".to_string(),
        Value::String(conversation_id.to_string()),
    );
    Value::Object(map)
}
