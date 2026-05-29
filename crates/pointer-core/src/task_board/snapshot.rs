//! Compact and full prompt snapshots.

use super::model::{BoardDocument, BoardItem, ItemStatus};
use super::state_machine::dependencies_satisfied;

const DONE_OUTPUT_MAX: usize = 120;
const READY_HINT_COUNT: usize = 2;
const DETAILED_PLAN_MAX: usize = 280;
const DETAILED_PLAN_INJECT_MAX: usize = 2000;

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
            let title = item.title.trim();
            let title = if title.is_empty() { "(untitled)" } else { title };
            lines.push(format!(
                "- {}: {} | {}",
                item.id,
                title,
                item.status.as_str()
            ));
        }
    }

    lines.push(String::new());
    lines.push("## Current task".to_string());
    if let Some(item) = current_task_item(doc) {
        lines.push(format!("- id: {}", item.id));
        lines.push(format!("- title: {}", item.title.trim()));
        lines.push(format!("- status: {}", item.status.as_str()));
        let key_verification = item
            .verification
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .or_else(|| {
                item.output
                    .as_ref()
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
            })
            .unwrap_or("n/a");
        lines.push(format!("- key_verification: {key_verification}"));
    } else {
        lines.push("- id: n/a".to_string());
        lines.push("- title: n/a".to_string());
        lines.push("- status: n/a".to_string());
        lines.push("- key_verification: n/a".to_string());
    }

    lines.push(String::new());
    lines.push("## Current task detailed plan".to_string());
    let detail = current_task_item(doc)
        .and_then(|item| item.detailed_plan.as_ref())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| {
            if s.chars().count() > DETAILED_PLAN_INJECT_MAX {
                let compact: String = s.chars().take(DETAILED_PLAN_INJECT_MAX).collect();
                format!("{compact}…")
            } else {
                s.to_string()
            }
        })
        .unwrap_or_else(|| "n/a".to_string());
    lines.push(detail);
    lines.join("\n")
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
    if let Some(v) = item.verification.as_ref().filter(|s| !s.trim().is_empty()) {
        out.insert("verification".into(), v.clone().into());
    }
    if let Some(v) = item.output.as_ref().filter(|s| !s.trim().is_empty()) {
        let compact = v.chars().take(DONE_OUTPUT_MAX).collect::<String>();
        out.insert("output".into(), compact.into());
    }
    if let Some(v) = item
        .detailed_plan
        .as_ref()
        .filter(|s| !s.trim().is_empty())
    {
        let compact = v.chars().take(DETAILED_PLAN_MAX).collect::<String>();
        out.insert("detailed_plan".into(), compact.into());
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
