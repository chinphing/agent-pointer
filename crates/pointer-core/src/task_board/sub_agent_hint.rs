//! Task board init hints for dynamic user inject (delegates to [`super::init_policy`]).

use super::coordination::is_child_store_key;
use super::init_policy;
use super::store::TaskBoardStore;
use crate::agents::AgentProfile;
use crate::models::ChatMessage;

pub use init_policy::TASK_BOARD_HINT_TAG;

/// System slice for lead or sub agent when `task_board` is allowed and the board is empty.
pub fn task_board_init_hint(
    store: &TaskBoardStore,
    store_key: &str,
    profile: &AgentProfile,
) -> Option<String> {
    if !matches!(profile, AgentProfile::Computer | AgentProfile::Coder) {
        return None;
    }
    let doc = store.document(store_key);
    if !doc.global_milestones.is_empty() || !doc.meta.goal.is_empty() {
        return None;
    }
    init_policy::build_init_hint(profile, is_child_store_key(store_key))
}

/// Whether to append `[TASK_BOARD_HINT]` on this turn.
pub fn should_inject_task_board_init_hint(
    profile: &AgentProfile,
    messages: &[ChatMessage],
) -> bool {
    init_policy::should_inject_init_hint(profile, messages)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::AgentProfile;

    #[test]
    fn hint_skips_when_board_has_goal() {
        let store = TaskBoardStore::new();
        store
            .apply(
                "conv-1",
                "init",
                &serde_json::json!({ "goal": "g", "items": [] }),
            )
            .expect("init");
        assert!(task_board_init_hint(&store, "conv-1", &AgentProfile::Coder).is_none());
    }

    #[test]
    fn coder_hint_when_board_empty() {
        let store = TaskBoardStore::new();
        let hint = task_board_init_hint(&store, "conv-1", &AgentProfile::Coder);
        let text = hint.expect("coder hint");
        assert!(text.contains("TASK_BOARD_HINT"));
        assert!(text.contains("task_board_init"));
    }
}
