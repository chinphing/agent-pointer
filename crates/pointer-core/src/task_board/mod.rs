//! Session task board (v4): working memory, persistence, multi-agent coordination.

pub mod apply;
pub mod args;
pub mod checkpoint;
pub mod coordination;
pub mod evidence;
pub mod finalize;
pub mod history_trim;
pub mod init_policy;
pub mod inject;
pub mod loop_milestones;
pub mod migrate;
pub mod model;
pub mod observability;
pub mod persistence;
pub mod results_append;
pub mod row_patch;
pub mod snapshot;
pub mod state_machine;
pub mod store;
pub mod sub_agent_hint;
pub mod tool;

pub use checkpoint::{is_task_board_tool_name, task_board_call_is_checkpoint};
pub use coordination::{
    abandon_previous_board_for_fresh_init, anchor_message_id_from_main_turn_key,
    conversation_id_from_main_turn_key, fresh_main_turn_store_key_for_init, is_child_store_key,
    is_main_turn_store_key, latest_real_user_message_id, looks_like_resume_intent,
    main_turn_task_board_store_key, parent_store_key_from_child,
    resolve_fresh_main_turn_init_store_key, resolve_store_key_for_read,
    sub_agent_task_board_store_key, sub_agent_task_board_store_key_for_instance,
    supersede_anchor_for_previous_board,
};
pub use evidence::history_has_recent_action_tools;
pub use finalize::maybe_auto_finalize_if_complete;
pub use history_trim::{
    default_agent_task_board_history_trim_table, is_task_board_history_trim_enabled,
    maybe_trim_after_tool_pass, trim_history_after_task_board, TaskBoardTrimHook,
    TaskBoardTrimStats, TRIM_PLACEHOLDER_PREFIX,
};
pub use inject::inject_host_task_board_conversation_id;
pub use model::{BoardDocument, BoardItem, DeliveryFormat, ItemStatus, MetaStatus};
pub use persistence::TaskBoardSqlite;
pub use store::TaskBoardStore;
pub use tool::register as register_task_board_tool;

#[cfg(test)]
mod tests;

/// Open SQLite persistence under app data dir when available.
pub fn open_default_persistence() -> Option<std::sync::Arc<TaskBoardSqlite>> {
    let dir = crate::storage::app_data_dir().ok()?;
    let path = dir.join("task_boards.db");
    TaskBoardSqlite::open(path).ok()
}
