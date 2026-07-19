//! Conflict-aware batch planning for parallel tool execution.

use crate::models::ToolCall;
use crate::tools::parallel::{conflict_keys_for_invocation, keys_overlap, ToolConflictClass};
use crate::tools::ToolRegistry;
use serde_json::Value;
use std::collections::HashSet;
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatchMode {
    Parallel,
    Mixed,
    Serial,
}

#[derive(Debug, Clone)]
pub enum ToolWave {
    /// Run tools one after another (indices into the prepared batch).
    Serial(Vec<usize>),
    /// Run tools concurrently up to semaphore limits (indices into the prepared batch).
    Parallel(Vec<usize>),
    /// Run only `run_subagent(agentId="self")` calls concurrently.
    ParallelSelfFork(Vec<usize>),
}

#[derive(Debug, Clone)]
pub struct ToolBatchPlan {
    pub mode: BatchMode,
    pub waves: Vec<ToolWave>,
    pub degrade_reason: Option<String>,
}

pub struct PlanToolBatchInput<'a> {
    pub registry: &'a ToolRegistry,
    pub batch: &'a [ToolCall],
    pub parsed_args: &'a [Value],
    pub tool_ids: &'a [String],
    pub workspace_root: &'a str,
    pub conversation_id: &'a str,
    pub force_serial: bool,
    pub max_parallel_tools: usize,
}

/// Build execution waves preserving assistant `tool_calls` order for outcome merge.
pub fn plan_tool_batch(input: PlanToolBatchInput<'_>) -> ToolBatchPlan {
    let started = Instant::now();
    let n = input.batch.len();
    if n == 0 {
        return ToolBatchPlan {
            mode: BatchMode::Serial,
            waves: vec![],
            degrade_reason: None,
        };
    }

    if input.force_serial {
        let waves: Vec<ToolWave> = (0..n).map(|i| ToolWave::Serial(vec![i])).collect();
        log_plan(BatchMode::Serial, &waves, Some("force_serial"), started);
        return ToolBatchPlan {
            mode: BatchMode::Serial,
            waves,
            degrade_reason: Some("force_serial".into()),
        };
    }

    let mut all_eligible = true;
    let mut any_parallel = false;
    for i in 0..n {
        let eligible = input.registry.is_parallel_eligible(&input.tool_ids[i]);
        if !eligible {
            all_eligible = false;
        } else {
            any_parallel = true;
        }
    }

    if !any_parallel {
        let waves: Vec<ToolWave> = (0..n).map(|i| ToolWave::Serial(vec![i])).collect();
        log_plan(BatchMode::Serial, &waves, Some("no_parallel_eligible"), started);
        return ToolBatchPlan {
            mode: BatchMode::Serial,
            waves,
            degrade_reason: Some("no_parallel_eligible".into()),
        };
    }

    let mut waves: Vec<ToolWave> = Vec::new();
    let mut current: Vec<usize> = Vec::new();
    let mut current_keys: HashSet<String> = HashSet::new();
    let mut current_self_forks: Vec<usize> = Vec::new();

    let flush = |waves: &mut Vec<ToolWave>, current: &mut Vec<usize>, current_keys: &mut HashSet<String>| {
        if current.is_empty() {
            return;
        }
        if current.len() == 1 {
            waves.push(ToolWave::Serial(vec![current[0]]));
        } else {
            waves.push(ToolWave::Parallel(current.clone()));
        }
        current.clear();
        current_keys.clear();
    };
    let flush_self_forks = |waves: &mut Vec<ToolWave>, current: &mut Vec<usize>| {
        if current.is_empty() {
            return;
        }
        if current.len() == 1 {
            waves.push(ToolWave::Serial(vec![current[0]]));
        } else {
            waves.push(ToolWave::ParallelSelfFork(current.clone()));
        }
        current.clear();
    };

    for i in 0..n {
        let tool_id = &input.tool_ids[i];
        let class = input.registry.tool_conflict_class(tool_id);
        let eligible = input.registry.is_parallel_eligible(tool_id);
        let is_parallel_subagent = tool_id == "run_subagent"
            && crate::tools::run_subagent::parse_run_subagent_args(&input.parsed_args[i])
                .is_ok_and(|args| args.is_parallel_wave_target());

        if is_parallel_subagent {
            flush(&mut waves, &mut current, &mut current_keys);
            current_self_forks.push(i);
            continue;
        }

        flush_self_forks(&mut waves, &mut current_self_forks);

        if !eligible || class == ToolConflictClass::Sidecar || class == ToolConflictClass::Computer {
            flush(&mut waves, &mut current, &mut current_keys);
            waves.push(ToolWave::Serial(vec![i]));
            continue;
        }

        // Subagent mutates shared trace/history during execution — one per wave.
        if class == ToolConflictClass::SubAgent {
            flush(&mut waves, &mut current, &mut current_keys);
            waves.push(ToolWave::Serial(vec![i]));
            continue;
        }

        let keys = conflict_keys_for_invocation(
            tool_id,
            &input.parsed_args[i],
            input.workspace_root,
            input.conversation_id,
        );

        let conflicts = current
            .iter()
            .any(|&j| keys_overlap(&keys, &wave_keys(&input, j)));
        let at_cap = current.len() >= input.max_parallel_tools.max(1);

        if !current.is_empty() && (conflicts || at_cap) {
            flush(&mut waves, &mut current, &mut current_keys);
        }

        current.push(i);
        current_keys.extend(keys);
    }
    flush_self_forks(&mut waves, &mut current_self_forks);
    flush(&mut waves, &mut current, &mut current_keys);

    let mode = if all_eligible
        && waves
            .iter()
            .all(|w| matches!(w, ToolWave::Parallel(_) | ToolWave::ParallelSelfFork(_)))
    {
        BatchMode::Parallel
    } else if waves
        .iter()
        .any(|w| matches!(w, ToolWave::Parallel(_) | ToolWave::ParallelSelfFork(_)))
    {
        BatchMode::Mixed
    } else {
        BatchMode::Serial
    };

    let degrade = match mode {
        BatchMode::Parallel => None,
        BatchMode::Mixed => Some("conflict_or_cap".into()),
        BatchMode::Serial => Some("serial_waves".into()),
    };
    log_plan(mode, &waves, degrade.as_deref(), started);

    ToolBatchPlan {
        mode,
        waves,
        degrade_reason: degrade,
    }
}

fn wave_keys(input: &PlanToolBatchInput<'_>, index: usize) -> HashSet<String> {
    conflict_keys_for_invocation(
        &input.tool_ids[index],
        &input.parsed_args[index],
        input.workspace_root,
        input.conversation_id,
    )
}

fn log_plan(mode: BatchMode, waves: &[ToolWave], degrade: Option<&str>, started: Instant) {
    let parallel_waves = waves
        .iter()
        .filter(|w| matches!(w, ToolWave::Parallel(_) | ToolWave::ParallelSelfFork(_)))
        .count();
    let max_fanout = waves
        .iter()
        .map(|w| match w {
            ToolWave::Serial(v) => v.len(),
            ToolWave::Parallel(v) => v.len(),
            ToolWave::ParallelSelfFork(v) => v.len(),
        })
        .max()
        .unwrap_or(0);
    log::info!(
        "tool_batch_plan: mode={mode:?} waves={} parallel_waves={} max_fanout={} degrade={} wall_ms={}",
        waves.len(),
        parallel_waves,
        max_fanout,
        degrade.unwrap_or("none"),
        started.elapsed().as_millis()
    );
}

pub fn batch_needs_serial_for_approval(
    registry: &ToolRegistry,
    tool_approval_mode: &str,
    tool_ids: &[String],
    parsed_args: &[Value],
) -> bool {
    if tool_approval_mode != "manual" {
        return false;
    }
    tool_ids.iter().zip(parsed_args).any(|(id, args)| {
        registry.tool_invocation_needs_approval(id, args)
            || (id == "terminal" && crate::tools::terminal::terminal_requests_elevation(args))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ToolCall;
    use crate::tools::ToolRegistry;
    use std::sync::Arc;
    use tempfile::tempdir;

    fn tc(id: &str, name: &str) -> ToolCall {
        ToolCall {
            id: id.into(),
            name: name.into(),
            arguments: "{}".into(),
            status: "pending".into(),
            result: None,
            error: None,
            duration_ms: None,
            risk_level: None,
            display_label: None,
            display_summary: None,
        }
    }

    fn reg_with_file_tools() -> ToolRegistry {
        let r = ToolRegistry::new();
        let store = Arc::new(crate::task_board::TaskBoardStore::new());
        crate::tools::builtin::register_all(&r, store);
        r
    }

    #[test]
    fn plan_parallel_different_files() {
        let dir = tempdir().unwrap();
        let root = dir.path().to_string_lossy().into_owned();
        std::fs::write(dir.path().join("a.txt"), "a").unwrap();
        std::fs::write(dir.path().join("b.txt"), "b").unwrap();
        let reg = reg_with_file_tools();
        let batch = vec![
            tc("1", "file_read"),
            tc("2", "file_write"),
        ];
        let parsed = vec![
            serde_json::json!({"path": "a.txt"}),
            serde_json::json!({"path": "b.txt", "content": "z"}),
        ];
        let ids = vec!["file_read".into(), "file_write".into()];
        let plan = plan_tool_batch(PlanToolBatchInput {
            registry: &reg,
            batch: &batch,
            parsed_args: &parsed,
            tool_ids: &ids,
            workspace_root: &root,
            conversation_id: "c1",
            force_serial: false,
            max_parallel_tools: 8,
        });
        assert!(matches!(plan.mode, BatchMode::Parallel | BatchMode::Mixed));
        assert!(plan.waves.iter().any(|w| matches!(w, ToolWave::Parallel(_))));
    }

    #[test]
    fn plan_serial_same_file_write_edit() {
        let dir = tempdir().unwrap();
        let root = dir.path().to_string_lossy().into_owned();
        std::fs::write(dir.path().join("a.txt"), "a").unwrap();
        let reg = reg_with_file_tools();
        let batch = vec![tc("1", "file_write"), tc("2", "file_edit")];
        let parsed = vec![
            serde_json::json!({"path": "a.txt", "content": "x"}),
            serde_json::json!({
                "path": "a.txt",
                "oldString": "a",
                "newString": "b"
            }),
        ];
        let ids = vec!["file_write".into(), "file_edit".into()];
        let plan = plan_tool_batch(PlanToolBatchInput {
            registry: &reg,
            batch: &batch,
            parsed_args: &parsed,
            tool_ids: &ids,
            workspace_root: &root,
            conversation_id: "c1",
            force_serial: false,
            max_parallel_tools: 8,
        });
        assert!(plan.waves.len() >= 2);
        assert!(plan
            .waves
            .iter()
            .all(|w| matches!(w, ToolWave::Serial(_))));
    }

    #[test]
    fn plan_force_serial_computer() {
        let reg = reg_with_file_tools();
        let batch = vec![tc("1", "file_read"), tc("2", "file_read")];
        let parsed = vec![
            serde_json::json!({"path": "a.txt"}),
            serde_json::json!({"path": "b.txt"}),
        ];
        let ids = vec!["file_read".into(), "file_read".into()];
        let plan = plan_tool_batch(PlanToolBatchInput {
            registry: &reg,
            batch: &batch,
            parsed_args: &parsed,
            tool_ids: &ids,
            workspace_root: ".",
            conversation_id: "c1",
            force_serial: true,
            max_parallel_tools: 8,
        });
        assert_eq!(plan.mode, BatchMode::Serial);
    }

    #[test]
    fn plan_parallel_multiple_media_understand() {
        let reg = reg_with_file_tools();
        let batch = vec![tc("1", "media_understand"), tc("2", "media_understand")];
        let parsed = vec![
            serde_json::json!({"mode": "image", "refs": ["a.png"], "goal": "a"}),
            serde_json::json!({"mode": "image", "refs": ["b.png"], "goal": "b"}),
        ];
        let ids = vec!["media_understand".into(), "media_understand".into()];
        let plan = plan_tool_batch(PlanToolBatchInput {
            registry: &reg,
            batch: &batch,
            parsed_args: &parsed,
            tool_ids: &ids,
            workspace_root: ".",
            conversation_id: "c1",
            force_serial: false,
            max_parallel_tools: 8,
        });
        assert!(matches!(plan.mode, BatchMode::Parallel));
        assert_eq!(plan.waves.len(), 1);
        assert!(matches!(plan.waves[0], ToolWave::Parallel(ref idx) if idx.len() == 2));
    }

    fn plan_subagents(
        targets: &[&str],
        force_serial: bool,
        _runtime_limit: usize,
    ) -> ToolBatchPlan {
        let reg = reg_with_file_tools();
        let batch: Vec<_> = targets
            .iter()
            .enumerate()
            .map(|(index, _)| tc(&format!("call-{index}"), "run_subagent"))
            .collect();
        let parsed: Vec<_> = targets
            .iter()
            .map(|agent_id| serde_json::json!({"agentId": agent_id, "goal": "work"}))
            .collect();
        let ids = vec!["run_subagent".to_string(); targets.len()];
        plan_tool_batch(PlanToolBatchInput {
            registry: &reg,
            batch: &batch,
            parsed_args: &parsed,
            tool_ids: &ids,
            workspace_root: ".",
            conversation_id: "c1",
            force_serial,
            max_parallel_tools: 8,
        })
    }

    #[test]
    fn plan_groups_consecutive_self_forks_in_dedicated_wave() {
        let plan = plan_subagents(&["self", "self"], false, 8);

        assert!(matches!(
            plan.waves.as_slice(),
            [ToolWave::ParallelSelfFork(indices)] if indices == &[0, 1]
        ));
    }

    #[test]
    fn plan_keeps_all_consecutive_self_forks_in_one_wave_above_runtime_limit() {
        let plan = plan_subagents(&["self", "self", "self"], false, 2);

        assert!(matches!(
            plan.waves.as_slice(),
            [ToolWave::ParallelSelfFork(indices)] if indices == &[0, 1, 2]
        ));
    }

    #[test]
    fn plan_groups_self_and_explore_in_one_parallel_wave() {
        let plan = plan_subagents(&["self", "explore"], false, 8);

        assert!(matches!(
            plan.waves.as_slice(),
            [ToolWave::ParallelSelfFork(indices)] if indices == &[0, 1]
        ));
    }

    #[test]
    fn plan_groups_consecutive_explore_in_parallel_wave() {
        let plan = plan_subagents(&["explore", "explore"], false, 8);

        assert!(matches!(
            plan.waves.as_slice(),
            [ToolWave::ParallelSelfFork(indices)] if indices == &[0, 1]
        ));
    }

    #[test]
    fn plan_keeps_explore_and_writer_subagents_on_serial_boundaries() {
        let plan = plan_subagents(&["explore", "coder"], false, 8);

        assert!(matches!(
            plan.waves.as_slice(),
            [ToolWave::Serial(first), ToolWave::Serial(second)]
                if first == &[0] && second == &[1]
        ));
    }

    #[test]
    fn plan_keeps_writer_registered_subagents_serial() {
        let plan = plan_subagents(&["coder", "coder"], false, 8);

        assert!(plan
            .waves
            .iter()
            .all(|wave| matches!(wave, ToolWave::Serial(_))));
    }

    #[test]
    fn plan_keeps_self_and_computer_on_serial_boundaries() {
        let plan = plan_subagents(&["self", "computer"], false, 8);

        assert!(matches!(
            plan.waves.as_slice(),
            [ToolWave::Serial(first), ToolWave::Serial(second)]
                if first == &[0] && second == &[1]
        ));
    }

    #[test]
    fn plan_force_serial_keeps_self_forks_serial() {
        let plan = plan_subagents(&["self", "self"], true, 8);

        assert!(plan
            .waves
            .iter()
            .all(|wave| matches!(wave, ToolWave::Serial(_))));
    }
}
