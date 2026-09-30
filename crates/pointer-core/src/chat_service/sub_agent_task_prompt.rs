//! Host-assembled sub-agent task blocks (`goal` / `context` in system; stub user turn).

use crate::task_board::TaskBoardStore;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubAgentSpawnCapability {
    Registered,
    SelfOnly,
    None,
}

/// System dynamic blocks for the delegated task (not repeated in the first user message).
pub fn build_subagent_task_system_blocks(
    goal: &str,
    context: &str,
    workspace_root: &str,
) -> Vec<String> {
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
    capability: SubAgentSpawnCapability,
) -> String {
    match capability {
        SubAgentSpawnCapability::Registered => format!(
            "## Sub-agent spawning (orchestrator)\n\n\
             You are at depth {spawn_depth}/{max_spawn_depth}. \
             You may call `run_subagent` for listed workers \
             or with `agentId: \"self\"`.\n\n\
             Delegate when a subtask needs isolated context; \
             do not pass through your entire goal unchanged.\n\
             Child results return to you as tool output — synthesize before your final handoff.\n\
             Every spawn, `self` fork included, consumes one real depth level."
        ),
        SubAgentSpawnCapability::SelfOnly => format!(
            "## Sub-agent spawning (self fork only)\n\n\
             You are at depth {spawn_depth}/{max_spawn_depth}.\n\
             You may call `run_subagent` only with `agentId: \"self\"`.\n\
             A self fork inherits your `allowAgents` and may delegate again \
             while it still has depth budget.\n\
             You cannot delegate to registered agents: your `allowAgents` is empty."
        ),
        SubAgentSpawnCapability::None => format!(
            "## Sub-agent spawning (leaf)\n\n\
             You are at depth {spawn_depth}/{max_spawn_depth}. \
             You cannot call `run_subagent`. Complete your assigned task directly."
        ),
    }
}

pub fn build_subagent_initial_user_message() -> String {
    "Begin. Your assigned task is in the system prompt under **Assigned task**. \
     Execute to completion; hand off in final assistant Markdown."
        .to_string()
}

/// Task-related system **dynamic** slices shared by sub-agent execution and pre-run planner.
pub(crate) fn push_sub_agent_task_system_dynamic(
    dynamic: &mut Vec<String>,
    task_dynamic_blocks: &[String],
    store: &TaskBoardStore,
    child_store_key: &str,
    sub_task_id: &str,
) {
    dynamic.extend(task_dynamic_blocks.iter().cloned());
    if let Some(parent_block) = store.parent_tunnel_for_child(child_store_key, sub_task_id) {
        dynamic.push(parent_block);
    }
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
        let b = build_subagent_spawn_depth_block(1, 2, SubAgentSpawnCapability::Registered);
        assert!(b.contains("run_subagent"));
        assert!(b.contains("1/2"));
    }

    #[test]
    fn leaf_block_denies_spawn() {
        let b = build_subagent_spawn_depth_block(2, 2, SubAgentSpawnCapability::None);
        assert!(b.contains("cannot call"));
    }

    #[test]
    fn self_only_block_allows_self_fork_without_registered_catalog() {
        let b = build_subagent_spawn_depth_block(1, 2, SubAgentSpawnCapability::SelfOnly);
        assert!(b.contains(r#"agentId: "self""#));
        assert!(b.contains("inherits your `allowAgents`"));
        assert!(b.contains("cannot delegate to registered agents"));
    }

    #[test]
    fn push_task_system_dynamic_includes_blocks_and_parent_tunnel() {
        use crate::task_board::{sub_agent_task_board_store_key, TaskBoardStore};

        let store = TaskBoardStore::new();
        store
            .apply(
                "parent",
                "init",
                &serde_json::json!({
                    "goal": "parent goal",
                    "items": [{"id": "m1", "title": "Batch", "status": "pending"}]
                }),
            )
            .expect("parent init");
        let child_key = sub_agent_task_board_store_key("parent", "sub1");
        let blocks = build_subagent_task_system_blocks("open ten apps", "ctx", "/ws");
        let mut dynamic = Vec::new();
        push_sub_agent_task_system_dynamic(&mut dynamic, &blocks, &store, &child_key, "sub1");
        assert!(dynamic.iter().any(|b| b.contains("Assigned task")));
        assert!(dynamic.iter().any(|b| b.contains("open ten apps")));
        assert!(dynamic.iter().any(|b| b.contains("[TASK_BOARD_PARENT]")));
    }
}
