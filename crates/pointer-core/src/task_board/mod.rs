//! Session task board (v3): working memory, persistence, multi-agent coordination.

pub mod apply;
pub mod args;
pub mod checkpoint;
pub mod coordination;
pub mod evidence;
pub mod finalize;
pub mod gateway;
pub mod history_trim;
pub mod inject;
pub mod planner;
pub mod observability;
pub mod migrate;
pub mod model;
pub mod results_append;
pub mod row_patch;
pub mod persistence;
pub mod snapshot;
pub mod state_machine;
pub mod store;
pub mod sub_agent_hint;
pub mod tool;
pub mod work_item;
pub mod work_items_apply;

pub use checkpoint::{is_task_board_tool_name, task_board_call_is_checkpoint};
pub use coordination::{
    anchor_message_id_from_main_turn_key, conversation_id_from_main_turn_key,
    is_child_store_key, parent_store_key_from_child, sub_agent_task_board_store_key,
    is_main_turn_store_key, looks_like_resume_intent, main_turn_task_board_store_key,
};
pub use gateway::{
    check_dependencies, dispatch_to_child, report_child_status, sync_global_finding,
    sync_parent_board_from_supervisor_plan, DependencyCheck, DispatchContext,
    SupervisorPlanSyncStats,
};
pub use history_trim::{
    default_agent_task_board_history_trim_table, is_task_board_history_trim_enabled,
    maybe_trim_after_tool_pass, trim_history_after_task_board, TaskBoardTrimHook,
    TaskBoardTrimStats, TRIM_PLACEHOLDER_PREFIX,
};
pub use evidence::history_has_recent_action_tools;
pub use finalize::maybe_auto_finalize_if_complete;
pub use inject::{inject_host_task_board_conversation_id, inject_work_items_tool_host};
pub use model::{BoardDocument, BoardItem, DeliveryFormat, ItemStatus, MetaStatus, WorkItemMode};
pub use persistence::TaskBoardSqlite;
pub use planner::{PlannerRunOutcome, PlannedMethod};
pub use store::TaskBoardStore;
pub use tool::register as register_task_board_tool;
pub use work_item::WorkItemStore;

#[cfg(test)]
mod tests;

/// Open SQLite persistence under app data dir when available.
pub fn open_default_persistence() -> Option<std::sync::Arc<TaskBoardSqlite>> {
    let dir = crate::storage::app_data_dir().ok()?;
    let path = dir.join("task_boards.db");
    TaskBoardSqlite::open(path).ok()
}

/// Open SQLite persistence for work items when app data dir is available.
pub fn open_default_work_item_persistence() -> Option<std::sync::Arc<work_item::WorkItemSqlite>> {
    work_item::open_default_persistence()
}
