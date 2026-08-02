//! Task board init hints for dynamic user inject (delegates to [`super::init_policy`]).
//! Empty-board `[TASK_BOARD_HINT]` inject is disabled; helpers remain for callers/tests.

use super::coordination::is_child_store_key;
use super::init_policy;
use super::store::TaskBoardStore;
use crate::agents::AgentProfile;
use crate::models::ChatMessage;

pub use init_policy::TASK_BOARD_HINT_TAG;

/// Formerly returned empty-board init nudge. Always `None` while inject is disabled.
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

/// Whether to append `[TASK_BOARD_HINT]` on this turn (always false while disabled).
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
    fn hint_disabled_even_when_board_empty() {
        let store = TaskBoardStore::new();
        assert!(task_board_init_hint(&store, "conv-1", &AgentProfile::Coder).is_none());
        assert!(task_board_init_hint(&store, "conv-1", &AgentProfile::Computer).is_none());
        assert!(!should_inject_task_board_init_hint(
            &AgentProfile::Coder,
            &[]
        ));
    }

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
}
