//! Structured `task_board_obs` logs for adoption and coordination debugging.

use crate::agents::AgentProfile;

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
    log::debug!("task_board_obs: snapshot_skipped_empty store_key={store_key}",);
}

pub fn log_sub_agent_init_hint(conversation_id: &str, task_id: &str, agent_id: &str) {
    log::info!(
        "task_board_obs: sub_agent_init_hint conversation_id={conversation_id} task_id={task_id} agent_id={agent_id}",
    );
}

pub fn log_main_agent_init_hint(conversation_id: &str, store_key: &str, profile: &AgentProfile) {
    log::info!(
        "task_board_obs: main_agent_init_hint conversation_id={conversation_id} store_key={store_key} profile={:?}",
        profile
    );
}

pub fn log_done_soft_validation(store_key: &str, item_id: &str, reason: &str) {
    log::warn!(
        "task_board_obs: done_soft_validation store_key={store_key} item_id={item_id} reason={reason}",
    );
}
