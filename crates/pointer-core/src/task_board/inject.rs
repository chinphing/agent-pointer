//! Host-injected session binding for `task_board` and computer tools.

use crate::chat_service::AppState;
use crate::models::ChatMessage;
use crate::task_board::checkpoint::is_task_board_tool_name;
use crate::task_board::evidence::{
    history_has_recent_action_tools, history_has_recent_verify_pass,
    history_has_recent_verify_report,
};
use serde_json::Value;

pub use crate::task_board::coordination::parent_child::{
    sub_agent_task_board_store_key, sub_agent_task_board_store_key_for_instance,
};

/// Host-only binding for `task_board` and computer tools so models cannot spoof another session id.
pub fn inject_host_task_board_conversation_id(
    tool_id: &str,
    args: Value,
    conversation_id: &str,
    task_board_store_key: &str,
    session_user_id: &str,
    history: &[ChatMessage],
    app_state: Option<&AppState>,
    _work_items_enabled: bool,
    b42_enforced: bool,
) -> Value {
    let is_task_board = is_task_board_tool_name(tool_id);
    let requires_injection = is_task_board
        || tool_id == "session_search"
        || tool_id == "session_read"
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
    if tool_id == "session_search" || tool_id == "session_read" {
        map.insert(
            "_session_user_id".to_string(),
            Value::String(session_user_id.trim().to_string()),
        );
    }
    if is_task_board {
        map.insert(
            "_recent_action_tools".to_string(),
            Value::Bool(history_has_recent_action_tools(history)),
        );
        map.insert(
            "_recent_verify_pass".to_string(),
            Value::Bool(
                app_state
                    .map(|s| history_has_recent_verify_pass(s, conversation_id))
                    .unwrap_or(false),
            ),
        );
        map.insert(
            "_recent_verify_report".to_string(),
            Value::Bool(
                app_state
                    .map(|s| history_has_recent_verify_report(s, conversation_id))
                    .unwrap_or(false),
            ),
        );
        map.insert(
            "_task_board_b42_enforced".to_string(),
            Value::Bool(b42_enforced),
        );
    }
    Value::Object(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_mouse_click_index_gets_conversation_binding() {
        let out = inject_host_task_board_conversation_id(
            "mouse_click_index",
            serde_json::json!({"goal": "打开微信应用", "index": 141}),
            "conv-abc",
            "conv-abc::tb",
            "user-1",
            &[],
            None,
            false,
            false,
        );
        assert_eq!(
            out.get("_conversation_id").and_then(|v| v.as_str()),
            Some("conv-abc")
        );
    }

    #[test]
    fn session_search_gets_conversation_binding() {
        let out = inject_host_task_board_conversation_id(
            "session_search",
            serde_json::json!({"query": "auth"}),
            "conv-abc",
            "conv-abc::tb",
            "im-user-a",
            &[],
            None,
            false,
            false,
        );
        assert_eq!(
            out.get("_conversation_id").and_then(|v| v.as_str()),
            Some("conv-abc")
        );
        assert_eq!(
            out.get("_session_user_id").and_then(|v| v.as_str()),
            Some("im-user-a")
        );
    }

    #[test]
    fn session_read_gets_conversation_binding() {
        let out = inject_host_task_board_conversation_id(
            "session_read",
            serde_json::json!({"agentInstanceId": "inst-1"}),
            "conv-abc",
            "conv-abc::tb",
            "im-user-a",
            &[],
            None,
            false,
            false,
        );
        assert_eq!(
            out.get("_conversation_id").and_then(|v| v.as_str()),
            Some("conv-abc")
        );
        assert_eq!(
            out.get("_session_user_id").and_then(|v| v.as_str()),
            Some("im-user-a")
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
            "user-1",
            &[],
            None,
            false,
            false,
        );
        assert_eq!(out, args);
    }

    #[test]
    fn inherited_task_board_call_is_bound_only_to_its_child_store() {
        let first_store =
            sub_agent_task_board_store_key_for_instance("parent", "shared-task", "fork-a");
        let second_store =
            sub_agent_task_board_store_key_for_instance("parent", "shared-task", "fork-b");
        let supplied = serde_json::json!({
            "_conversation_id": second_store,
            "items": [{"id": "local_01", "status": "done"}]
        });

        let out = inject_host_task_board_conversation_id(
            "task_board_patch",
            supplied,
            "conversation",
            &first_store,
            "user-1",
            &[],
            None,
            false,
            false,
        );

        assert_eq!(
            out.get("_conversation_id").and_then(Value::as_str),
            Some(first_store.as_str())
        );
    }
}
