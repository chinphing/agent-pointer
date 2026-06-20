//! Host-assembled sub-agent task blocks (`goal` / `context` in system; stub user turn).

/// System dynamic blocks for the delegated task (not repeated in the first user message).
pub fn build_subagent_task_system_blocks(goal: &str, context: &str, workspace_root: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let goal = goal.trim();
    if !goal.is_empty() {
        blocks.push(format!(
            "## Assigned task\n\n\
             Delegated from the lead agent — not end-user chat. \
             Complete per your worker policy.\n\n\
             ```\n{goal}\n```"
        ));
    }
    let ctx = context.trim();
    if !ctx.is_empty() {
        blocks.push(format!(
            "## Lead context (trusted — verify with tools)\n\n```\n{ctx}\n```"
        ));
    }
    let ws = workspace_root.trim();
    if !ws.is_empty() {
        blocks.push(format!("## Workspace\n\n`{ws}`"));
    }
    blocks
}

pub fn build_subagent_spawn_depth_block(
    spawn_depth: u32,
    max_spawn_depth: u32,
    can_spawn: bool,
) -> String {
    if can_spawn {
        format!(
            "## Sub-agent spawning (orchestrator)\n\n\
             You are at depth {spawn_depth}/{max_spawn_depth}. \
             You may call `run_subagent` for workers listed in delegatable sub-agents metadata.\n\n\
             Delegate when a subtask needs isolated context; \
             do not pass through your entire goal unchanged.\n\
             Child results return to you as tool output — synthesize before your final handoff."
        )
    } else {
        format!(
            "## Sub-agent spawning (leaf)\n\n\
             You are at depth {spawn_depth}/{max_spawn_depth}. \
             You cannot call `run_subagent`. Complete your assigned task directly."
        )
    }
}

pub fn build_subagent_initial_user_message() -> String {
    "Begin. Your assigned task is in the system prompt under **Assigned task**. \
     Execute to completion; hand off in final assistant Markdown."
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_blocks_include_goal_and_context() {
        let blocks = build_subagent_task_system_blocks(
            "Scenario: spec_map\nMap auth flow.",
            "Already checked: src/auth/",
            "/tmp/repo",
        );
        assert_eq!(blocks.len(), 3);
        assert!(blocks[0].contains("Assigned task"));
        assert!(blocks[0].contains("Scenario: spec_map"));
        assert!(blocks[1].contains("Lead context"));
        assert!(blocks[2].contains("/tmp/repo"));
    }

    #[test]
    fn initial_user_is_stub_without_goal() {
        let msg = build_subagent_initial_user_message();
        assert!(msg.contains("Assigned task"));
        assert!(!msg.contains("Scenario:"));
    }

    #[test]
    fn orchestrator_block_mentions_run_subagent() {
        let b = build_subagent_spawn_depth_block(1, 2, true);
        assert!(b.contains("run_subagent"));
        assert!(b.contains("1/2"));
    }

    #[test]
    fn leaf_block_denies_spawn() {
        let b = build_subagent_spawn_depth_block(2, 2, false);
        assert!(b.contains("cannot call"));
    }
}
