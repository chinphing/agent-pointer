//! Compact and full prompt snapshots.

use super::model::{BoardDocument, BoardItem, ItemStatus};
use super::state_machine::dependencies_satisfied;

const DONE_OUTPUT_MAX: usize = 120;
const READY_HINT_COUNT: usize = 2;
const DETAILS_MAX: usize = 280;
const DETAILS_INJECT_MAX: usize = 2000;

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
        lines.push(format!("- validate: {}", validate_display(item)));
        lines.push(format!("- status: {}", item.status.as_str()));
    } else {
        lines.push("- id: n/a".to_string());
        lines.push("- title: n/a".to_string());
        lines.push("- validate: n/a".to_string());
        lines.push("- status: n/a".to_string());
    }

    lines.push(String::new());
    lines.push("## Current task details".to_string());
    let details = current_task_item(doc)
        .and_then(|item| item.details.as_ref())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| {
            if s.chars().count() > DETAILS_INJECT_MAX {
                let compact: String = s.chars().take(DETAILS_INJECT_MAX).collect();
                format!("{compact}…")
            } else {
                s.to_string()
            }
        })
        .unwrap_or_else(|| "n/a".to_string());
    lines.push(details);

    lines.push(String::new());
    lines.push("## Current task progress".to_string());
    let progress = current_task_item(doc)
        .and_then(|item| item.progress.as_ref())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or("n/a");
    lines.push(progress.to_string());
    lines.join("\n")
}

fn validate_display(item: &BoardItem) -> &str {
    item.validate
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
            "- {}: {} | validate: {} | {}",
            item.id,
            title,
            validate_display(item),
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
    if let Some(v) = item.validate.as_ref().filter(|s| !s.trim().is_empty()) {
        out.insert("validate".into(), v.clone().into());
    }
    if let Some(v) = item.progress.as_ref().filter(|s| !s.trim().is_empty()) {
        out.insert("progress".into(), v.clone().into());
    }
    if let Some(v) = item.output.as_ref().filter(|s| !s.trim().is_empty()) {
        let compact = v.chars().take(DONE_OUTPUT_MAX).collect::<String>();
        out.insert("output".into(), compact.into());
    }
    if let Some(v) = item.details.as_ref().filter(|s| !s.trim().is_empty()) {
        let compact = v.chars().take(DETAILS_MAX).collect::<String>();
        out.insert("details".into(), compact.into());
    }
    if let Some(v) = item.blocked_by.as_ref().filter(|s| !s.trim().is_empty()) {
        out.insert("blockedBy".into(), v.clone().into());
    }
    serde_json::Value::Object(out)
}

fn item_compact(item: &BoardItem) -> serde_json::Value {
    let out = item
        .output
        .as_deref()
        .unwrap_or("")
        .chars()
        .take(DONE_OUTPUT_MAX)
        .collect::<String>();
    serde_json::json!({
        "id": item.id,
        "status": item.status.as_str(),
        "summary": out,
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
                    validate: Some("grep done".into()),
                    ..BoardItem::default()
                },
                BoardItem {
                    id: "m2".into(),
                    title: "Implement".into(),
                    status: ItemStatus::InProgress,
                    validate: Some("cargo test -p foo".into()),
                    details: Some("patch handler".into()),
                    progress: Some("started".into()),
                    ..BoardItem::default()
                },
                BoardItem {
                    id: "m3".into(),
                    title: "Audit".into(),
                    status: ItemStatus::Pending,
                    validate: Some("cargo clippy".into()),
                    ..BoardItem::default()
                },
            ];
        doc
    }

    #[test]
    fn inject_omits_key_validate_and_validate_section() {
        let block = markdown_runtime_block_for_inject(&sample_doc());
        assert!(!block.contains("key_validate"));
        assert!(!block.contains("## Current task validate"));
    }

    #[test]
    fn inject_current_task_puts_validate_before_status() {
        let block = markdown_runtime_block_for_inject(&sample_doc());
        let section = block
            .split("## Current task details")
            .next()
            .expect("current task section");
        assert!(section.contains("- title: Implement"));
        assert!(section.contains("- validate: cargo test -p foo"));
        assert!(section.contains("- status: in_progress"));
        let title_pos = section.find("- title:").expect("title");
        let validate_pos = section.find("- validate:").expect("validate");
        let status_pos = section.find("- status:").expect("status");
        assert!(title_pos < validate_pos);
        assert!(validate_pos < status_pos);
    }

    #[test]
    fn inject_all_tasks_includes_validate_for_pending_and_in_progress() {
        let block = markdown_runtime_block_for_inject(&sample_doc());
        assert!(block.contains("- m1: Explore | done"));
        assert!(block.contains("- m2: Implement | validate: cargo test -p foo | in_progress"));
        assert!(block.contains("- m3: Audit | validate: cargo clippy | pending"));
    }
}
