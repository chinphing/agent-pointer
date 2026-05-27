//! Host-injected session binding for `task_board` and computer tools.

use crate::models::ChatMessage;
use crate::task_board::checkpoint::is_task_board_tool_name;
use crate::task_board::evidence::{
    history_has_recent_action_tools, history_has_recent_verify_pass, history_has_recent_verify_report,
};
use serde_json::Value;

pub use crate::task_board::coordination::parent_child::sub_agent_task_board_store_key;

/// Host-only binding for `task_board` and computer tools so models cannot spoof another session id.
pub fn inject_host_task_board_conversation_id(
    tool_id: &str,
    args: Value,
    conversation_id: &str,
    history: &[ChatMessage],
) -> Value {
    let is_task_board = is_task_board_tool_name(tool_id);
    let requires_injection = is_task_board
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
    if is_task_board {
        map.insert(
            "_recent_action_tools".to_string(),
            Value::Bool(history_has_recent_action_tools(history)),
        );
        map.insert(
            "_recent_verify_pass".to_string(),
            Value::Bool(history_has_recent_verify_pass(history)),
        );
        map.insert(
            "_recent_verify_report".to_string(),
            Value::Bool(history_has_recent_verify_report(history)),
        );
    }
    Value::Object(map)
}
