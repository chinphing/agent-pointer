//! Compact and full prompt snapshots.

use super::model::{BoardDocument, BoardItem, ItemStatus};
use super::state_machine::dependencies_satisfied;

const RESULT_SNIPPET_INJECT_MAX: usize = 120;
const READY_HINT_COUNT: usize = 2;
const PLAN_INJECT_MAX: usize = 2000;
const REQUIREMENT_INJECT_MAX: usize = 600;
const RESULTS_TAIL_COUNT: usize = 6;

pub fn snapshot_for_prompt(store_key: &str, doc: &BoardDocument, compact: bool) -> Option<String> {
    if doc.board_is_empty() && doc.meta.goal.is_empty() {
        return None;
    }
    let body = if compact {
        compact_json(doc)
    } else {
        serde_json::to_string_pretty(doc).ok()?
    };
    Some(format!("[TASK_BOARD]\nstore_key: {store_key}\n{body}"))
}

pub fn format_parent_tunnel_block(parent: &BoardDocument, sub_task_id: &str) -> String {
    let mut lines = vec!["[TASK_BOARD_PARENT]".to_string(), "read_only: true".to_string()];
    if !parent.meta.goal.is_empty() {
        lines.push(format!("goal: {}", parent.meta.goal));
    }
    for f in parent.global_context.key_findings.iter().rev().take(8) {
        lines.push(format!("finding: {f}"));
    }
    if let Some(row) = parent.board.iter().find(|i| i.id == sub_task_id) {
        lines.push(format!(
            "current_milestone: id={} status={} title={}",
            row.id,
            row.status.as_str(),
            row.title
        ));
    }
    lines.join("\n")
}

pub fn markdown_runtime_block_for_inject(doc: &BoardDocument) -> String {
    let mut lines: Vec<String> = Vec::new();
    lines.push("[TASK_BOARD]".to_string());
    lines.push(String::new());
    lines.push("## Global goals".to_string());
    let goal = doc.meta.goal.trim();
    if goal.is_empty() {
        lines.push("- n/a".to_string());
    } else {
        lines.push(format!("- {goal}"));
    }

    lines.push(String::new());
    lines.push("## All tasks (with status)".to_string());
    if doc.board.is_empty() {
        lines.push("- none".to_string());
    } else {
        for item in &doc.board {
            lines.push(format_all_tasks_line(item));
        }
    }

    lines.push(String::new());
    lines.push("## Current task".to_string());
    if let Some(item) = current_task_item(doc) {
        lines.push(format!("- id: {}", item.id));
        lines.push(format!("- title: {}", item.title.trim()));
        lines.push(format!(
            "- validate_requirement: {}",
            truncate_field(item.validate_requirement.as_deref(), REQUIREMENT_INJECT_MAX)
        ));
        lines.push(format!("- status: {}", item.status.as_str()));
    } else {
        lines.push("- id: n/a".to_string());
        lines.push("- title: n/a".to_string());
        lines.push("- validate_requirement: n/a".to_string());
        lines.push("- status: n/a".to_string());
    }

    lines.push(String::new());
    lines.push("## Current task plan".to_string());
    lines.push(truncate_field(
        current_task_item(doc).and_then(|i| i.plan.as_deref()),
        PLAN_INJECT_MAX,
    ));

    lines.push(String::new());
    lines.push("## Current task checkpoint".to_string());
    let checkpoint = current_task_item(doc)
        .and_then(|i| i.checkpoint.as_ref())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or("n/a");
    lines.push(checkpoint.to_string());

    lines.push(String::new());
    lines.push("## Current task validate_results (recent)".to_string());
    let validate_tail = current_task_item(doc)
        .map(|i| i.validate_results.as_slice())
        .unwrap_or(&[] as &[String]);
    lines.push(format_results_tail(validate_tail));

    if let Some(item) = current_task_item(doc) {
        if !item.extract_results.is_empty() || item.extract_requirement.is_some() {
            lines.push(String::new());
            lines.push("## Current task extract".to_string());
            if let Some(req) = item.extract_requirement.as_deref() {
                lines.push("requirement:".to_string());
                lines.push(truncate_field(Some(req), REQUIREMENT_INJECT_MAX));
            }
            lines.push("results (recent):".to_string());
            lines.push(format_results_tail(&item.extract_results));
        }
    }

    lines.join("\n")
}

fn truncate_field(text: Option<&str>, max: usize) -> String {
    let t = text.unwrap_or("n/a").trim();
    if t.is_empty() {
        return "n/a".to_string();
    }
    if t.chars().count() <= max {
        return t.to_string();
    }
    let compact: String = t.chars().take(max).collect();
    format!("{compact}…")
}

fn normalize_result_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Inject view: unique lines in order, then tail (hides cumulative duplicate blocks in storage).
fn format_results_tail(results: &[String]) -> String {
    if results.is_empty() {
        return "n/a".to_string();
    }
    let mut seen = std::collections::HashSet::new();
    let mut unique_lines: Vec<String> = Vec::new();
    for entry in results {
        for line in entry.lines() {
            let t = line.trim();
            if t.is_empty() {
                continue;
            }
            let key = normalize_result_line(t);
            if seen.insert(key) {
                unique_lines.push(t.to_string());
            }
        }
    }
    if unique_lines.is_empty() {
        return "n/a".to_string();
    }
    let start = unique_lines.len().saturating_sub(RESULTS_TAIL_COUNT);
    unique_lines[start..]
        .iter()
        .map(|s| format!("- {s}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn requirement_one_line(item: &BoardItem) -> &str {
    item.validate_requirement
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or("n/a")
}

fn format_all_tasks_line(item: &BoardItem) -> String {
    let title = item.title.trim();
    let title = if title.is_empty() { "(untitled)" } else { title };
    if matches!(
        item.status,
        ItemStatus::InProgress | ItemStatus::Pending
    ) {
        format!(
            "- {}: {} | validate_requirement: {} | {}",
            item.id,
            title,
            requirement_one_line(item),
            item.status.as_str()
        )
    } else {
        format!(
            "- {}: {} | {}",
            item.id,
            title,
            item.status.as_str()
        )
    }
}

fn current_task_item(doc: &BoardDocument) -> Option<&BoardItem> {
    doc.board
        .iter()
        .find(|i| i.status == ItemStatus::InProgress)
        .or_else(|| doc.board.iter().find(|i| i.status == ItemStatus::Ready))
        .or_else(|| doc.board.iter().find(|i| i.status == ItemStatus::Pending))
        .or_else(|| doc.board.iter().find(|i| i.status == ItemStatus::Failed))
        .or_else(|| doc.board.iter().find(|i| i.status == ItemStatus::Done))
        .or_else(|| doc.board.first())
}

fn compact_json(doc: &BoardDocument) -> String {
    let mut out = serde_json::Map::new();
    out.insert("version".into(), doc.version.into());
    out.insert(
        "meta".into(),
        serde_json::json!({
            "goal": doc.meta.goal,
            "status": doc.meta.status.as_str(),
            "step_count": doc.meta.step_count,
            "max_steps": doc.meta.max_steps,
            "scope": doc.meta.scope,
        }),
    );
    if !doc.global_context.key_findings.is_empty() {
        let n = doc.global_context.key_findings.len();
        let start = n.saturating_sub(6);
        out.insert(
            "global_context".into(),
            serde_json::json!({
                "key_findings": &doc.global_context.key_findings[start..],
            }),
        );
    }
    let mut board_out = Vec::new();
    for item in &doc.board {
        if matches!(
            item.status,
            ItemStatus::InProgress | ItemStatus::Failed
        ) {
            board_out.push(item_full(item));
        }
    }
    let mut ready_added = 0usize;
    for item in &doc.board {
        if ready_added >= READY_HINT_COUNT {
            break;
        }
        if item.status == ItemStatus::Pending || item.status == ItemStatus::Ready {
            if dependencies_satisfied(doc, item) {
                board_out.push(item_full(item));
                ready_added += 1;
            }
        }
    }
    for item in &doc.board {
        if matches!(item.status, ItemStatus::Done | ItemStatus::Cancelled) {
            board_out.push(item_compact(item));
        }
    }
    out.insert("board".into(), board_out.into());
    serde_json::to_string_pretty(&out).unwrap_or_else(|_| "{}".into())
}

fn item_full(item: &BoardItem) -> serde_json::Value {
    let mut out = serde_json::Map::new();
    out.insert("id".into(), item.id.clone().into());
    out.insert("title".into(), item.title.clone().into());
    out.insert("status".into(), item.status.as_str().into());
    if !item.depends_on.is_empty() {
        out.insert("depends_on".into(), serde_json::json!(item.depends_on));
    }
    if item.retry_count > 0 {
        out.insert("retry_count".into(), item.retry_count.into());
    }
    if let Some(v) = item
        .validate_requirement
        .as_ref()
        .filter(|s| !s.trim().is_empty())
    {
        let compact = v.chars().take(REQUIREMENT_INJECT_MAX).collect::<String>();
        out.insert("validate_requirement".into(), compact.into());
    }
    if let Some(v) = item.checkpoint.as_ref().filter(|s| !s.trim().is_empty()) {
        out.insert("checkpoint".into(), v.clone().into());
    }
    if !item.validate_results.is_empty() {
        out.insert(
            "validate_results".into(),
            serde_json::json!(tail_compact_strings(&item.validate_results, 3)),
        );
    }
    if !item.extract_results.is_empty() {
        out.insert(
            "extract_results".into(),
            serde_json::json!(tail_compact_strings(&item.extract_results, 2)),
        );
    }
    if let Some(v) = item.plan.as_ref().filter(|s| !s.trim().is_empty()) {
        let compact = v.chars().take(280).collect::<String>();
        out.insert("plan".into(), compact.into());
    }
    if let Some(v) = item.blocked_by.as_ref().filter(|s| !s.trim().is_empty()) {
        out.insert("blocked_by".into(), v.clone().into());
    }
    serde_json::Value::Object(out)
}

fn tail_compact_strings(list: &[String], n: usize) -> Vec<String> {
    let start = list.len().saturating_sub(n);
    list[start..]
        .iter()
        .map(|s| {
            let t = s.trim();
            if t.chars().count() <= RESULT_SNIPPET_INJECT_MAX {
                t.to_string()
            } else {
                let c: String = t.chars().take(RESULT_SNIPPET_INJECT_MAX).collect();
                format!("{c}…")
            }
        })
        .collect()
}

fn item_compact(item: &BoardItem) -> serde_json::Value {
    let summary = item
        .last_validate_result_snippet()
        .unwrap_or("")
        .chars()
        .take(RESULT_SNIPPET_INJECT_MAX)
        .collect::<String>();
    serde_json::json!({
        "id": item.id,
        "status": item.status.as_str(),
        "summary": summary,
    })
}

#[cfg(test)]
mod inject_format_tests {
    use super::*;
    use crate::task_board::model::BoardItem;

    fn sample_doc() -> BoardDocument {
        let mut doc = BoardDocument::empty_for_store_key("conv-test");
        doc.meta.goal = "Ship feature".into();
        doc.board = vec![
            BoardItem {
                id: "m1".into(),
                title: "Explore".into(),
                status: ItemStatus::Done,
                validate_results: vec!["grep done".into()],
                ..BoardItem::default()
            },
            BoardItem {
                id: "m2".into(),
                title: "Implement".into(),
                status: ItemStatus::InProgress,
                validate_requirement: Some("cargo test -p foo".into()),
                plan: Some("patch handler".into()),
                checkpoint: Some("cycle=1/3".into()),
                ..BoardItem::default()
            },
            BoardItem {
                id: "m3".into(),
                title: "Audit".into(),
                status: ItemStatus::Pending,
                validate_requirement: Some("cargo clippy".into()),
                ..BoardItem::default()
            },
        ];
        doc
    }

    #[test]
    fn inject_has_plan_and_validate_results_sections() {
        let block = markdown_runtime_block_for_inject(&sample_doc());
        assert!(block.contains("## Current task plan"));
        assert!(block.contains("## Current task validate_results"));
        assert!(!block.contains("## Current task details"));
    }

    #[test]
    fn inject_current_task_shows_requirement() {
        let block = markdown_runtime_block_for_inject(&sample_doc());
        let section = block
            .split("## Current task plan")
            .next()
            .expect("current task section");
        assert!(section.contains("- validate_requirement: cargo test -p foo"));
        assert!(section.contains("- status: in_progress"));
    }

    #[test]
    fn inject_all_tasks_includes_requirement_for_pending_and_in_progress() {
        let block = markdown_runtime_block_for_inject(&sample_doc());
        assert!(block.contains("- m1: Explore | done"));
        assert!(block.contains(
            "- m2: Implement | validate_requirement: cargo test -p foo | in_progress"
        ));
        assert!(block.contains(
            "- m3: Audit | validate_requirement: cargo clippy | pending"
        ));
    }
}
