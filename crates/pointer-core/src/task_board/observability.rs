//! Structured `task_board_obs` logs for adoption and coordination debugging.

use super::gateway::plan_sync::SupervisorPlanSyncStats;

pub fn log_supervisor_plan_sync(conversation_id: &str, stats: &SupervisorPlanSyncStats, task_count: usize) {
    let orphans = if stats.orphan_milestone_ids.is_empty() {
        String::new()
    } else {
        format!(" orphan_ids={}", stats.orphan_milestone_ids.join(","))
    };
    log::info!(
        "task_board_obs: supervisor_plan_sync conversation_id={conversation_id} task_count={task_count} \
         goal_set={} milestones_created={} milestones_updated={}{orphans}",
        stats.goal_set,
        stats.milestones_created,
        stats.milestones_updated,
    );
}

pub fn log_dispatch_child(
    conversation_id: &str,
    task_id: &str,
    child_seeded: bool,
) {
    log::info!(
        "task_board_obs: dispatch_child conversation_id={conversation_id} task_id={task_id} child_seeded={child_seeded}",
    );
}

pub fn log_store_apply(store_key: &str, method: &str, board_len: usize, reflection_required: bool) {
    log::info!(
        "task_board_obs: store_apply store_key={store_key} method={method} board_len={board_len} reflection_required={reflection_required}",
    );
}

pub fn log_snapshot_injected(store_key: &str, board_len: usize, has_goal: bool) {
    log::info!(
        "task_board_obs: snapshot_injected store_key={store_key} board_len={board_len} has_goal={has_goal}",
    );
}

pub fn log_snapshot_skipped_empty(store_key: &str) {
    log::debug!(
        "task_board_obs: snapshot_skipped_empty store_key={store_key}",
    );
}

pub fn log_sub_agent_init_hint(conversation_id: &str, task_id: &str, agent_id: &str) {
    log::info!(
        "task_board_obs: sub_agent_init_hint conversation_id={conversation_id} task_id={task_id} agent_id={agent_id}",
    );
}

pub fn log_done_soft_validation(store_key: &str, item_id: &str, reason: &str) {
    log::warn!(
        "task_board_obs: done_soft_validation store_key={store_key} item_id={item_id} reason={reason}",
    );
}
