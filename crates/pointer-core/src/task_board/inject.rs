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
    task_board_store_key: &str,
    history: &[ChatMessage],
) -> Value {
    let is_task_board = is_task_board_tool_name(tool_id);
    let requires_injection = is_task_board
        || crate::agents::computer::is_desktop_vision_log_tool(tool_id)
        || crate::agents::computer::is_desktop_post_delay_tool(tool_id);
    if !requires_injection {
        return args;
    }
    let host_binding = if is_task_board {
        task_board_store_key
    } else {
        conversation_id
    };
    let mut map = if let Value::Object(m) = args {
        m
    } else {
        serde_json::Map::new()
    };
    map.insert(
        "_conversation_id".to_string(),
        Value::String(host_binding.to_string()),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captcha_verify_gets_conversation_binding() {
        let out = inject_host_task_board_conversation_id(
            "captcha_verify_drag",
            serde_json::json!({"goal": "x", "index_captcha_area": 1}),
            "conv-abc",
            "conv-abc::tb",
            &[],
        );
        assert_eq!(
            out.get("_conversation_id").and_then(|v| v.as_str()),
            Some("conv-abc")
        );
    }

    #[test]
    fn flat_mouse_click_index_gets_conversation_binding() {
        let out = inject_host_task_board_conversation_id(
            "mouse_click_index",
            serde_json::json!({"goal": "打开微信应用", "index": 141}),
            "conv-abc",
            "conv-abc::tb",
            &[],
        );
        assert_eq!(
            out.get("_conversation_id").and_then(|v| v.as_str()),
            Some("conv-abc")
        );
    }

    #[test]
    fn unrelated_tool_skips_injection() {
        let args = serde_json::json!({"query": "hello"});
        let out = inject_host_task_board_conversation_id(
            "web_search",
            args.clone(),
            "conv-abc",
            "conv-abc::tb",
            &[],
        );
        assert_eq!(out, args);
    }
}
