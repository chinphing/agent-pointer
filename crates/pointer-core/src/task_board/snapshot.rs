//! Compact and full prompt snapshots.

use super::model::{BoardDocument, BoardItem, ItemStatus};
use super::state_machine::dependencies_satisfied;
use super::work_item::model::{WorkItemStatus, RESULT_SUMMARY_INJECT_MAX};
use super::work_item::WorkItemStore;

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

pub fn markdown_runtime_block_for_inject(
    doc: &BoardDocument,
    store_key: &str,
    work_items: Option<&WorkItemStore>,
) -> String {
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
    lines.push("## Current task progress".to_string());
    let progress = current_task_item(doc)
        .and_then(|i| i.progress.as_ref())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or("n/a");
    lines.push(progress.to_string());

    if let Some(item) = current_task_item(doc) {
        if item.has_work_items() {
            if let Some(store) = work_items {
                append_work_items_section(&mut lines, store_key, store, item);
            }
        } else if matches!(
            item.status,
            ItemStatus::InProgress | ItemStatus::Ready | ItemStatus::Pending
        ) {
            lines.push(String::new());
            lines.push("## Current task validate_result_delta".to_string());
            lines.push(format_results_tail(&item.validate_results));
            if !item.extract_results.is_empty() || item.extract_requirement.is_some() {
                lines.push(String::new());
                lines.push("## Current task extract".to_string());
                if let Some(req) = item.extract_requirement.as_deref() {
                    lines.push("requirement:".to_string());
                    lines.push(truncate_field(Some(req), REQUIREMENT_INJECT_MAX));
                }
                lines.push("extract_result_delta (recent):".to_string());
                lines.push(format_results_tail(&item.extract_results));
            }
        } else if matches!(item.status, ItemStatus::Done) {
            lines.push(String::new());
            lines.push("## Current task validate_results".to_string());
            lines.push(format_results_all(&item.validate_results));
            if !item.extract_results.is_empty() {
                lines.push(String::new());
                lines.push("## Current task extract_results".to_string());
                lines.push(format_results_all(&item.extract_results));
            }
        }
    }

    lines.join("\n")
}

fn append_work_items_section(
    lines: &mut Vec<String>,
    store_key: &str,
    work_items: &WorkItemStore,
    item: &BoardItem,
) {
    let stats = work_items.batch_stats(store_key, &item.id);
    let total_in_db = work_items.batch_total(store_key, &item.id);
    lines.push(String::new());
    lines.push("## Current task work_items".to_string());
    if item.is_dynamic_work_items() {
        let quota = item.dynamic_quota.unwrap_or(0);
        lines.push(format!(
            "progress: {}/{} claimed ({} done, {} failed)",
            stats.done + stats.in_progress,
            quota,
            stats.done,
            stats.failed
        ));
    } else {
        lines.push(format!("progress: {}", stats.progress_label()));
    }
    if total_in_db > stats.total {
        lines.push(format!("note: {total_in_db} rows in store"));
    }
    let window = work_items.inject_window(store_key, &item.id);
    for wi in &window {
        let summary = wi
            .result_json
            .as_deref()
            .and_then(|j| super::work_item::model::result_summary_from_json(j))
            .unwrap_or_default();
        let summary = if summary.chars().count() > RESULT_SUMMARY_INJECT_MAX {
            let c: String = summary.chars().take(RESULT_SUMMARY_INJECT_MAX).collect();
            format!("{c}…")
        } else {
            summary
        };
        if summary.is_empty() {
            lines.push(format!(
                "- {} · {} · {}",
                wi.id,
                wi.title,
                wi.status.as_str()
            ));
        } else {
            lines.push(format!(
                "- {} · {} · {} · {}",
                wi.id,
                wi.title,
                wi.status.as_str(),
                summary
            ));
        }
        if wi.status == WorkItemStatus::InProgress {
            lines.push(format!("[WORK_ITEM_FOCUS] id={} title={}", wi.id, wi.title));
        }
    }
    let hidden = total_in_db.saturating_sub(window.len() as u32);
    if hidden > 0 {
        lines.push(format!("… {hidden} more items in DB, not injected"));
    }
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

fn dedupe_result_lines(results: &[String]) -> Vec<String> {
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
    unique_lines
}

/// Inject view: all unique lines in order (hides cumulative duplicate blocks in storage).
fn format_results_all(results: &[String]) -> String {
    let unique_lines = dedupe_result_lines(results);
    if unique_lines.is_empty() {
        return "n/a".to_string();
    }
    unique_lines
        .iter()
        .map(|s| format!("- {s}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Inject view: unique lines in order, then tail (hides cumulative duplicate blocks in storage).
fn format_results_tail(results: &[String]) -> String {
    let unique_lines = dedupe_result_lines(results);
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

fn append_done_validate_results(base: &str, item: &BoardItem) -> String {
    let unique_lines = dedupe_result_lines(&item.validate_results);
    if unique_lines.is_empty() {
        return base.to_string();
    }
    let mut out = base.to_string();
    for line in unique_lines {
        out.push('\n');
        out.push_str("  validate_results: ");
        out.push_str(&line);
    }
    out
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
        return format!(
            "- {}: {} | validate_requirement: {} | {}",
            item.id,
            title,
            requirement_one_line(item),
            item.status.as_str()
        );
    }
    let base = format!(
        "- {}: {} | {}",
        item.id,
        title,
        item.status.as_str()
    );
    if !matches!(
        item.status,
        ItemStatus::Done | ItemStatus::Cancelled | ItemStatus::Failed
    ) {
        return base;
    }
    append_done_validate_results(&base, item)
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
    if let Some(v) = item.progress.as_ref().filter(|s| !s.trim().is_empty()) {
        out.insert("progress".into(), v.clone().into());
    }
    if !item.validate_results.is_empty() {
        let key = if matches!(
            item.status,
            ItemStatus::Done | ItemStatus::Cancelled | ItemStatus::Failed
        ) {
            "validate_results"
        } else {
            "validate_result_delta"
        };
        let tail_n = if key == "validate_results" {
            item.validate_results.len().max(3)
        } else {
            3
        };
        out.insert(
            key.into(),
            serde_json::json!(tail_compact_strings(&item.validate_results, tail_n)),
        );
    }
    if !item.extract_results.is_empty() {
        let key = if matches!(
            item.status,
            ItemStatus::Done | ItemStatus::Cancelled | ItemStatus::Failed
        ) {
            "extract_results"
        } else {
            "extract_result_delta"
        };
        let tail_n = if key == "extract_results" {
            item.extract_results.len().max(2)
        } else {
            2
        };
        out.insert(
            key.into(),
            serde_json::json!(tail_compact_strings(&item.extract_results, tail_n)),
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
                progress: Some("3/10".into()),
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
    fn inject_has_plan_and_validate_delta_sections() {
        let block = markdown_runtime_block_for_inject(&sample_doc(), "conv-test", None);
        assert!(block.contains("## Current task plan"));
        assert!(block.contains("## Current task validate_result_delta"));
        assert!(!block.contains("## Current task validate_results"));
        assert!(!block.contains("## Current task details"));
    }

    #[test]
    fn inject_current_task_shows_requirement() {
        let block = markdown_runtime_block_for_inject(&sample_doc(), "conv-test", None);
        let section = block
            .split("## Current task plan")
            .next()
            .expect("current task section");
        assert!(section.contains("- validate_requirement: cargo test -p foo"));
        assert!(section.contains("- status: in_progress"));
    }

    #[test]
    fn inject_all_tasks_includes_requirement_for_pending_and_in_progress() {
        let block = markdown_runtime_block_for_inject(&sample_doc(), "conv-test", None);
        assert!(block.contains("- m1: Explore | done"));
        assert!(block.contains("  validate_results: grep done"));
        assert!(block.contains(
            "- m2: Implement | validate_requirement: cargo test -p foo | in_progress"
        ));
        assert!(block.contains(
            "- m3: Audit | validate_requirement: cargo clippy | pending"
        ));
    }

    #[test]
    fn inject_current_task_shows_delta_tail_not_full_results() {
        let mut doc = BoardDocument::empty_for_store_key("conv-test");
        doc.board = vec![BoardItem {
            id: "m1".into(),
            title: "Batch".into(),
            status: ItemStatus::InProgress,
            validate_results: (1..=8)
                .map(|i| format!("{i}/10: step {i}"))
                .collect(),
            ..BoardItem::default()
        }];
        let block = markdown_runtime_block_for_inject(&doc, "conv-test", None);
        assert!(block.contains("## Current task validate_result_delta"));
        assert!(!block.contains("## Current task validate_results"));
        assert!(block.contains("- 8/10: step 8"));
        assert!(!block.contains("1/10: step 1"));
    }

    #[test]
    fn inject_done_task_shows_all_validate_results_after_status_line() {
        let mut doc = BoardDocument::empty_for_store_key("conv-test");
        doc.board = vec![BoardItem {
            id: "8".into(),
            title: "13289012347".into(),
            status: ItemStatus::Done,
            validate_results: vec![
                "1/3: pending".into(),
                "手机号 13289012347 - 账号存在 (小景家)".into(),
            ],
            ..BoardItem::default()
        }];
        let block = markdown_runtime_block_for_inject(&doc, "conv-test", None);
        assert!(block.contains("- 8: 13289012347 | done"));
        assert!(block.contains("  validate_results: 1/3: pending"));
        assert!(block.contains("  validate_results: 手机号 13289012347 - 账号存在 (小景家)"));
    }
}
