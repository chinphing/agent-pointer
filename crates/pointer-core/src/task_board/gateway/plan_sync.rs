//! Sync parent task board milestones from Supervisor plan output.

use crate::agents::AgentTask;
use super::super::model::{BoardItem, BoardScope, ItemStatus};
use super::super::store::TaskBoardStore;

#[derive(Debug, Clone, Default)]
pub struct SupervisorPlanSyncStats {
    pub goal_set: bool,
    pub milestones_created: usize,
    pub milestones_updated: usize,
    pub orphan_milestone_ids: Vec<String>,
}

/// Upsert Supervisor plan tasks onto the parent board (`conversation_id` key).
pub fn sync_parent_board_from_supervisor_plan(
    store: &TaskBoardStore,
    parent_key: &str,
    tasks: &[AgentTask],
    plan_goal: &str,
) -> SupervisorPlanSyncStats {
    let mut stats = SupervisorPlanSyncStats::default();
    let mut doc = store.document(parent_key);
    let plan_ids: std::collections::HashSet<String> = tasks
        .iter()
        .map(|t| t.id.trim().to_string())
        .filter(|id| !id.is_empty())
        .collect();

    if doc.meta.goal.trim().is_empty() {
        let g = plan_goal.trim();
        if !g.is_empty() {
            doc.meta.goal = g.to_string();
            stats.goal_set = true;
        }
    }
    doc.meta.scope = Some(BoardScope::Parent);

    for task in tasks {
        let id = task.id.trim();
        if id.is_empty() {
            continue;
        }
        let title = if task.title.trim().is_empty() {
            format!("Sub-task {}", id)
        } else {
            task.title.trim().to_string()
        };
        let verification = verification_hint_from_instruction(&task.instruction);
        if let Some(idx) = doc.board.iter().position(|i| i.id == id) {
            let row = &mut doc.board[idx];
            row.title = title;
            row.depends_on = task.depends_on.clone();
            if row.verification.is_none() && verification.is_some() {
                row.verification = verification;
            }
            stats.milestones_updated += 1;
        } else {
            doc.board.push(BoardItem {
                id: id.to_string(),
                title,
                status: ItemStatus::Pending,
                depends_on: task.depends_on.clone(),
                verification,
                ..BoardItem::default()
            });
            stats.milestones_created += 1;
        }
    }

    for row in &doc.board {
        if !plan_ids.contains(&row.id) {
            stats.orphan_milestone_ids.push(row.id.clone());
        }
    }

    store.save_document(parent_key, doc);
    stats
}

fn verification_hint_from_instruction(instruction: &str) -> Option<String> {
    let line = instruction
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())?;
    let mut s: String = line.chars().take(160).collect();
    if line.chars().count() > 160 {
        s.push('…');
    }
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::AgentTask;
    use crate::task_board::store::TaskBoardStore;

    #[test]
    fn sync_creates_milestones_from_plan() {
        let store = TaskBoardStore::new();
        let key = "conv-plan";
        let tasks = vec![
            AgentTask {
                id: "task_1".into(),
                agent_id: "coder".into(),
                title: "Implement".into(),
                instruction: "Run cargo test".into(),
                depends_on: vec![],
            },
            AgentTask {
                id: "task_2".into(),
                agent_id: "coder".into(),
                title: "Review".into(),
                instruction: "Check diff".into(),
                depends_on: vec!["task_1".into()],
            },
        ];
        let stats =
            sync_parent_board_from_supervisor_plan(&store, key, &tasks, "User goal");
        assert!(stats.goal_set);
        assert_eq!(stats.milestones_created, 2);
        let doc = store.document(key);
        assert_eq!(doc.board.len(), 2);
        assert_eq!(doc.board[1].depends_on, vec!["task_1"]);
    }
}
