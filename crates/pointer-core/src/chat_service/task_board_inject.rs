use serde_json::Value;

/// Host-only binding for `task_board` and computer tools so models cannot spoof another session id.
pub(crate) fn inject_host_task_board_conversation_id(
    tool_id: &str,
    args: Value,
    conversation_id: &str,
) -> Value {
    let requires_injection = tool_id == "task_board"
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

/// Key for [`crate::tools::task_board::TaskBoardStore`] during Supervisor **sub-agent** runs.
///
/// Isolated from the main chat `conversation_id` board: sub-agents do not read or write the
/// lead session’s task board unless the Supervisor copies state into instructions.
pub(crate) fn sub_agent_task_board_store_key(main_conversation_id: &str, supervisor_task_id: &str) -> String {
    format!(
        "{main}\x1fptr_sub_agent\x1f{task}",
        main = main_conversation_id.trim(),
        task = supervisor_task_id.trim()
    )
}

#[cfg(test)]
mod sub_agent_task_board_key_tests {
    use super::sub_agent_task_board_store_key;

    #[test]
    fn key_is_not_raw_conversation_id() {
        let main = "conv-1";
        let k = sub_agent_task_board_store_key(main, "task_a");
        assert_ne!(k, main);
        assert!(k.contains("ptr_sub_agent"), "{k:?}");
        assert!(k.ends_with("task_a"), "{k:?}");
    }

    #[test]
    fn distinct_supervisor_task_ids_differ() {
        assert_ne!(
            sub_agent_task_board_store_key("c", "t1"),
            sub_agent_task_board_store_key("c", "t2")
        );
    }
}
