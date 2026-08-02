//! Task board init-hint policy (legacy).
//!
//! `[TASK_BOARD_HINT]` used to be appended every empty-board LLM round via
//! ephemeral `injected_tail`. Cadence checked `base_messages`, so the hint
//! never stuck in history and was re-injected every tool turn — models then
//! parroted "skip board" in thoughts. Init is left to static tool / agent
//! prompts; only live `[TASK_BOARD]` snapshots are still injected.

use crate::agents::AgentProfile;
use crate::models::ChatMessage;

pub const TASK_BOARD_HINT_TAG: &str = "[TASK_BOARD_HINT";

/// Formerly built the empty-board nudge. Always `None` — do not reintroduce
/// per-turn inject without persisting a history marker first.
pub fn build_init_hint(_profile: &AgentProfile, _is_sub_agent: bool) -> Option<String> {
    None
}

pub fn last_task_board_hint_index(messages: &[ChatMessage]) -> Option<usize> {
    messages
        .iter()
        .rposition(|m| m.content.contains(TASK_BOARD_HINT_TAG))
}

/// Empty-board `[TASK_BOARD_HINT]` inject is disabled (see module docs).
pub fn should_inject_init_hint(_profile: &AgentProfile, _messages: &[ChatMessage]) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::AgentProfile;

    #[test]
    fn init_hint_inject_disabled() {
        assert!(build_init_hint(&AgentProfile::Coder, false).is_none());
        assert!(build_init_hint(&AgentProfile::Computer, true).is_none());
        assert!(!should_inject_init_hint(&AgentProfile::Coder, &[]));
        assert!(!should_inject_init_hint(&AgentProfile::Computer, &[]));
    }
}
