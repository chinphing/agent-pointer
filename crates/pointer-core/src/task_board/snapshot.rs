//! Compact and full prompt snapshots (v4 inject).

use super::model::{BoardDocument, BoardItem, ItemStatus, WorkItemMode};
use super::state_machine::dependencies_satisfied;
use super::work_item::model::{WorkItem, WorkItemStatus, RESULT_SUMMARY_INJECT_MAX};
use super::work_item::WorkItemStore;
use super::work_items_apply::exec_met;
use serde_json::Value;

const PLAN_INJECT_MAX: usize = 2000;
const DONE_WHEN_INJECT_MAX: usize = 600;
const READY_HINT_COUNT: usize = 2;
const RESULT_SNIPPET_INJECT_MAX: usize = 120;

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
    format_parent_tunnel_block_full(parent, sub_task_id, None, "")
}

pub fn format_parent_tunnel_block_full(
    parent: &BoardDocument,
    sub_task_id: &str,
    work_items: Option<&WorkItemStore>,
    store_key: &str,
) -> String {
    let mut lines = vec!["[TASK_BOARD_PARENT]".to_string(), "read_only: true".to_string()];
    if !parent.meta.goal.is_empty() {
        lines.push(format!("goal: {}", parent.meta.goal));
    }
    if !parent.meta.context.trim().is_empty() {
        lines.push(format!("context: {}", parent.meta.context.trim()));
    }
    if let Some(c) = parent.meta.constraint.as_deref().filter(|s| !s.trim().is_empty()) {
        lines.push(format!("constraint: {c}"));
    }
    if let Some(dw) = parent.meta.done_when.as_deref().filter(|s| !s.trim().is_empty()) {
        lines.push(format!("done_when: {dw}"));
    }
    lines.push(format!(
        "global: {}",
        parent
            .global_milestones
            .iter()
            .map(|r| format!("{} {}", r.id, r.status.as_str()))
            .collect::<Vec<_>>()
            .join(" · ")
    ));
    if parent.has_work_items() {
        if let Some(store) = work_items {
            let stats = store.store_stats(store_key);
            lines.push(format!(
                "exec_progress: {} done · {} failed · {} in_progress · {} pending",
                stats.done, stats.failed, stats.in_progress, stats.pending
            ));
            lines.push(format!("exec_met: {}", exec_met(parent, store, store_key)));
            if let Some(focus) = store
                .inject_window(store_key)
                .into_iter()
                .find(|wi| wi.status == WorkItemStatus::InProgress)
            {
                lines.push(format!(
                    "work_item_focus: {} · {} · {}",
                    focus.id,
                    focus.title,
                    focus.status.as_str()
                ));
            }
            if let Some(item) = current_task_in_slice(&parent.item_milestones) {
                lines.push(format!(
                    "current_task: {} · {} · {}",
                    item.id,
                    item.title,
                    item.status.as_str()
                ));
            }
        }
    }
    for f in parent.global_context.key_findings.iter().rev().take(8) {
        lines.push(format!("finding: {f}"));
    }
    if let Some(row) = parent.global_milestones.iter().find(|i| i.id == sub_task_id) {
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
    let mode = milestone_inject_mode(doc, store_key, work_items);
    append_task_section(&mut lines, doc);

    match mode {
        MilestoneInjectMode::Step => {
            append_all_tasks_list(&mut lines, &doc.global_milestones);
            if let Some(current) = current_task_in_slice(&doc.global_milestones) {
                append_current_task_section(
                    &mut lines,
                    current,
                    doc,
                    store_key,
                    work_items,
                    false,
                );
            }
        }
        MilestoneInjectMode::QueueExec => {
            append_all_tasks_list(&mut lines, &doc.item_milestones);
            if let Some(current) = current_task_in_slice(&doc.item_milestones) {
                append_current_task_section(
                    &mut lines,
                    current,
                    doc,
                    store_key,
                    work_items,
                    true,
                );
            }
            if doc.has_work_items() {
                if let Some(store) = work_items {
                    append_work_items_mismatch_note(&mut lines, doc, store_key, store);
                    append_work_items_section(&mut lines, store_key, store);
                }
            }
        }
        MilestoneInjectMode::QueueDeliver => {
            let deliver: Vec<BoardItem> = doc
                .global_milestones
                .iter()
                .filter(|r| r.id == "g_deliver")
                .cloned()
                .collect();
            if deliver.is_empty() {
                lines.push(String::new());
                lines.push("## All tasks (with status)".to_string());
                lines.push("- none".to_string());
            } else {
                append_all_tasks_list(&mut lines, &deliver);
                append_current_task_section(
                    &mut lines,
                    &deliver[0],
                    doc,
                    store_key,
                    work_items,
                    false,
                );
            }
            if doc.has_work_items() {
                if let Some(store) = work_items {
                    append_work_items_mismatch_note(&mut lines, doc, store_key, store);
                    append_work_items_section(&mut lines, store_key, store);
                }
            }
        }
    }

    lines.join("\n")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MilestoneInjectMode {
    Step,
    QueueExec,
    QueueDeliver,
}

pub fn milestone_inject_mode(
    doc: &BoardDocument,
    store_key: &str,
    work_items: Option<&WorkItemStore>,
) -> MilestoneInjectMode {
    if !doc.has_work_items() {
        return MilestoneInjectMode::Step;
    }
    if let Some(store) = work_items {
        if exec_met(doc, store, store_key) {
            return MilestoneInjectMode::QueueDeliver;
        }
    }
    MilestoneInjectMode::QueueExec
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnifiedPatchTarget {
    GlobalMilestones,
    ItemMilestones,
}

/// Route a unified `milestones` patch row to the document slice matching inject projection.
pub fn unified_patch_target(
    doc: &BoardDocument,
    store_key: &str,
    work_items: Option<&WorkItemStore>,
    row_id: &str,
) -> anyhow::Result<UnifiedPatchTarget> {
    use anyhow::anyhow;
    let id = row_id.trim();
    if id.is_empty() {
        return Err(anyhow!("task_board: patch row requires id"));
    }
    let mode = milestone_inject_mode(doc, store_key, work_items);
    match mode {
        MilestoneInjectMode::Step => {
            if doc.global_milestones.iter().any(|r| r.id == id) {
                return Ok(UnifiedPatchTarget::GlobalMilestones);
            }
            // Type1: allow appending new step rows via patch (legacy items[] upsert).
            if !id.starts_with("g_") {
                return Ok(UnifiedPatchTarget::GlobalMilestones);
            }
            Err(anyhow!(
                "task_board: patch id {id} not in visible step ladder (global_milestones)"
            ))
        }
        MilestoneInjectMode::QueueExec => {
            if doc.item_milestones.iter().any(|r| r.id == id) {
                return Ok(UnifiedPatchTarget::ItemMilestones);
            }
            if id.starts_with("g_") {
                return Err(anyhow!(
                    "task_board: do not patch {id} during queue exec — patch item SOP rows only"
                ));
            }
            Err(anyhow!(
                "task_board: patch id {id} not in visible queue SOP ladder (item_milestones)"
            ))
        }
        MilestoneInjectMode::QueueDeliver => {
            if id == "g_deliver" && doc.global_milestones.iter().any(|r| r.id == id) {
                return Ok(UnifiedPatchTarget::GlobalMilestones);
            }
            Err(anyhow!(
                "task_board: deliver phase — patch g_deliver only (via milestones)"
            ))
        }
    }
}

fn append_all_tasks_list(lines: &mut Vec<String>, items: &[BoardItem]) {
    lines.push(String::new());
    lines.push("## All tasks (with status)".to_string());
    if items.is_empty() {
        lines.push("- none".to_string());
    } else {
        for item in items {
            lines.push(format_item_list_line(item));
        }
    }
}

fn append_current_task_section(
    lines: &mut Vec<String>,
    item: &BoardItem,
    doc: &BoardDocument,
    store_key: &str,
    work_items: Option<&WorkItemStore>,
    resolve_work_item: bool,
) {
    lines.push(String::new());
    lines.push("## Current task".to_string());
    if resolve_work_item {
        append_item_current(lines, item, work_items, store_key);
    } else {
        append_current_row_bullets(lines, item, doc, work_items, store_key);
    }
    let plan = item.plan.as_deref().filter(|s| !s.trim().is_empty());
    if plan.is_some() {
        lines.push(String::new());
        lines.push("## Current task plan".to_string());
        lines.push(truncate_field(plan, PLAN_INJECT_MAX));
    }
}

fn current_task_in_slice(items: &[BoardItem]) -> Option<&BoardItem> {
    items
        .iter()
        .find(|i| i.status == ItemStatus::InProgress)
        .or_else(|| items.iter().find(|i| i.status == ItemStatus::Ready))
        .or_else(|| items.iter().find(|i| i.status == ItemStatus::Pending))
        .or_else(|| items.first())
}

fn work_items_store_mismatch(
    doc: &BoardDocument,
    store: &WorkItemStore,
    store_key: &str,
) -> bool {
    if !doc.has_work_items() {
        return false;
    }
    let meta_rows = doc.meta.work_items_seeded_rows.unwrap_or(0);
    meta_rows > 0 && store.store_total(store_key) == 0
}

fn append_work_items_mismatch_note(
    lines: &mut Vec<String>,
    doc: &BoardDocument,
    store_key: &str,
    store: &WorkItemStore,
) {
    if !work_items_store_mismatch(doc, store, store_key) {
        return;
    }
    let meta_rows = doc.meta.work_items_seeded_rows.unwrap_or(0);
    lines.push(String::new());
    lines.push("## Work items host note".to_string());
    lines.push(format!(
        "- WARNING: meta reports {meta_rows} seeded row(s) but work_items DB has 0 loaded."
    ));
    lines.push(
        "- Do NOT call task_board_init to re-seed. Host restores rows on read; use task_board_patch.".to_string(),
    );
}

fn append_task_section(lines: &mut Vec<String>, doc: &BoardDocument) {
    lines.push("## Task".to_string());
    let goal = doc.meta.goal.trim();
    lines.push(if goal.is_empty() {
        "- goal: n/a".to_string()
    } else {
        format!("- goal: {goal}")
    });
    let ctx = doc.meta.context.trim();
    if !ctx.is_empty() {
        lines.push(format!("- context: {ctx}"));
    }
    lines.push(format!(
        "- constraint: {}",
        meta_or_na(doc.meta.constraint.as_deref())
    ));
    lines.push(format!(
        "- done_when: {}",
        meta_or_na(doc.meta.done_when.as_deref())
    ));
    if let Some(mode) = doc.meta.work_item_mode {
        lines.push(format!(
            "- work_item_mode: {}",
            match mode {
                WorkItemMode::Enumerated => "enumerated",
                WorkItemMode::Dynamic => "dynamic",
            }
        ));
    }
    if let Some(n) = doc.meta.expected_total {
        lines.push(format!("- expected_total: {n}"));
    }
    if let Some(path) = doc
        .meta
        .work_items_source_path
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let rows = doc
            .meta
            .work_items_seeded_rows
            .map(|n| n.to_string())
            .unwrap_or_else(|| "n/a".into());
        lines.push(format!("- work_items_source: {path} ({rows} rows seeded)"));
    } else if let Some(n) = doc.meta.work_items_seeded_rows {
        lines.push(format!("- work_items_seeded: {n} (inline on init)"));
    }
}

fn append_current_row_bullets(
    lines: &mut Vec<String>,
    item: &BoardItem,
    doc: &BoardDocument,
    work_items: Option<&WorkItemStore>,
    store_key: &str,
) {
    lines.push(format!("- id: {}", item.id));
    lines.push(format!("- title: {}", item.title.trim()));
    lines.push(format!(
        "- done_when: {}",
        truncate_field(item.done_when.as_deref(), DONE_WHEN_INJECT_MAX)
    ));
    lines.push(format!("- status: {}", item.status.as_str()));
    if let Some(c) = item.constraint.as_deref().filter(|s| !s.trim().is_empty()) {
        lines.push(format!("- constraint: {c}"));
    }
    if !item.depends_on.is_empty() {
        lines.push(format!("- depends_on: {}", item.depends_on.join(", ")));
    }
    if let Some(b) = item.blocked_by.as_deref().filter(|s| !s.trim().is_empty()) {
        lines.push(format!("- blocked_by: {b}"));
    }
    if doc.has_work_items() {
        if let Some(store) = work_items {
            let stats = store.store_stats(store_key);
            lines.push(format!(
                "- exec_progress: {} done · {} failed · {} in_progress · {} pending",
                stats.done, stats.failed, stats.in_progress, stats.pending
            ));
            lines.push(format!(
                "- exec_met: {}",
                exec_met(doc, store, store_key)
            ));
        }
    }
}

fn append_item_current(
    lines: &mut Vec<String>,
    item: &BoardItem,
    work_items: Option<&WorkItemStore>,
    store_key: &str,
) {
    lines.push(format!("- id: {}", item.id));
    lines.push(format!("- title: {}", item.title.trim()));
    lines.push(format!("- status: {}", item.status.as_str()));
    if let Some(p) = item.plan.as_deref().filter(|s| !s.trim().is_empty()) {
        lines.push(format!("- plan: {}", truncate_field(Some(p), PLAN_INJECT_MAX)));
    }
    if let Some(dw) = item.done_when.as_deref().filter(|s| !s.trim().is_empty()) {
        lines.push(format!(
            "- done_when: {}",
            truncate_field(Some(dw), DONE_WHEN_INJECT_MAX)
        ));
    }
    if let (Some(store), Some(focus)) = (work_items, focus_work_item(store_key, work_items)) {
        let vars = placeholder_vars(&focus);
        if let Some(p) = item.plan.as_deref() {
            let resolved = substitute_placeholders(p, &vars);
            if resolved != p {
                lines.push(format!("- plan_resolved: {resolved}"));
            }
        }
        if let Some(dw) = item.done_when.as_deref() {
            let resolved = substitute_placeholders(dw, &vars);
            if resolved != dw {
                lines.push(format!("- done_when_resolved: {resolved}"));
            }
        }
        let _ = store;
    }
}

fn focus_work_item<'a>(
    store_key: &str,
    work_items: Option<&'a WorkItemStore>,
) -> Option<WorkItem> {
    let store = work_items?;
    store
        .inject_window(store_key)
        .into_iter()
        .find(|wi| wi.status == WorkItemStatus::InProgress)
}

fn focus_payload_summary(wi: &WorkItem) -> String {
    serde_json::from_str::<Value>(&wi.payload_json)
        .ok()
        .and_then(|v| {
            v.as_object().map(|o| {
                o.iter()
                    .take(4)
                    .map(|(k, v)| format!("{k}={}", value_display(v)))
                    .collect::<Vec<_>>()
                    .join(" ")
            })
        })
        .unwrap_or_default()
}

fn value_display(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn placeholder_vars(wi: &WorkItem) -> std::collections::HashMap<String, String> {
    let mut vars = std::collections::HashMap::new();
    vars.insert("title".into(), wi.title.clone());
    if let Ok(v) = serde_json::from_str::<Value>(&wi.payload_json) {
        if let Some(obj) = v.as_object() {
            for (k, val) in obj {
                vars.insert(k.clone(), value_display(val));
            }
        }
    }
    vars
}

pub fn substitute_placeholders(template: &str, vars: &std::collections::HashMap<String, String>) -> String {
    let mut out = template.to_string();
    for (key, val) in vars {
        let placeholder = format!("{{{key}}}");
        if out.contains(&placeholder) && !val.is_empty() {
            out = out.replace(&placeholder, val);
        }
    }
    // warn on remaining placeholders
    for cap in template.match_indices('{') {
        if let Some(end) = template[cap.0..].find('}') {
            let ph = &template[cap.0..cap.0 + end + 1];
            if out.contains(ph) {
                log::warn!("task_board inject: unknown or empty placeholder {ph}");
            }
        }
    }
    out
}

fn append_work_items_section(
    lines: &mut Vec<String>,
    store_key: &str,
    work_items: &WorkItemStore,
) {
    let stats = work_items.store_stats(store_key);
    let total_in_db = work_items.store_total(store_key);
    lines.push(String::new());
    lines.push("## Work items".to_string());
    lines.push(format!(
        "- exec: {}/{} terminal · {} in_progress · {} pending",
        stats.done + stats.failed,
        stats.total.max(1),
        stats.in_progress,
        stats.pending
    ));
    let window = work_items.inject_window(store_key);
    for wi in &window {
        append_work_item_line(lines, wi);
        if wi.status == WorkItemStatus::InProgress {
            let payload_hint = focus_payload_summary(wi);
            if payload_hint.is_empty() {
                lines.push(format!(
                    "[WORK_ITEM_FOCUS] id={} title={}",
                    wi.id, wi.title
                ));
            } else {
                lines.push(format!(
                    "[WORK_ITEM_FOCUS] id={} title={} {}",
                    wi.id, wi.title, payload_hint
                ));
            }
        }
    }
    let hidden = total_in_db.saturating_sub(window.len() as u32);
    if hidden > 0 {
        lines.push(format!("… {hidden} more in DB, not injected"));
    }
}

fn append_work_item_line(lines: &mut Vec<String>, wi: &WorkItem) {
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

fn format_global_list_line(item: &BoardItem) -> String {
    let title = item.title.trim();
    let title = if title.is_empty() { "(untitled)" } else { title };
    if matches!(item.status, ItemStatus::InProgress | ItemStatus::Pending | ItemStatus::Ready) {
        return format!(
            "- {}: {} | done_when: {} | {}",
            item.id,
            title,
            done_when_one_line(item),
            item.status.as_str()
        );
    }
    let base = format!("- {}: {} | {}", item.id, title, item.status.as_str());
    if let Some(r) = item.remark.as_deref().filter(|s| !s.trim().is_empty()) {
        return format!("{base}\n  remark: {r}");
    }
    base
}

fn format_item_list_line(item: &BoardItem) -> String {
    format_global_list_line(item)
}

fn done_when_one_line(item: &BoardItem) -> &str {
    item.done_when
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or("n/a")
}

fn meta_or_na(s: Option<&str>) -> &str {
    s.map(str::trim).filter(|t| !t.is_empty()).unwrap_or("n/a")
}

fn compact_json(doc: &BoardDocument) -> String {
    let mut out = serde_json::Map::new();
    out.insert("version".into(), doc.version.into());
    out.insert(
        "meta".into(),
        serde_json::json!({
            "goal": doc.meta.goal,
            "context": doc.meta.context,
            "status": doc.meta.status.as_str(),
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
    for item in &doc.global_milestones {
        if matches!(item.status, ItemStatus::InProgress | ItemStatus::Failed) {
            board_out.push(item_full(item));
        }
    }
    let mut ready_added = 0usize;
    for item in &doc.global_milestones {
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
    for item in &doc.global_milestones {
        if matches!(item.status, ItemStatus::Done | ItemStatus::Cancelled) {
            board_out.push(item_compact(item));
        }
    }
    out.insert("global_milestones".into(), board_out.into());
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
    if let Some(v) = item.done_when.as_ref().filter(|s| !s.trim().is_empty()) {
        let compact = v.chars().take(DONE_WHEN_INJECT_MAX).collect::<String>();
        out.insert("done_when".into(), compact.into());
    }
    if let Some(v) = item.remark.as_ref().filter(|s| !s.trim().is_empty()) {
        out.insert("remark".into(), v.clone().into());
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

fn item_compact(item: &BoardItem) -> serde_json::Value {
    let summary = item
        .last_remark_snippet()
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

    fn sample_doc() -> BoardDocument {
        let mut doc = BoardDocument::empty_for_store_key("conv-test");
        doc.meta.goal = "Ship feature".into();
        doc.global_milestones = vec![
            BoardItem {
                id: "m1".into(),
                title: "Explore".into(),
                status: ItemStatus::Done,
                remark: Some("grep done".into()),
                ..BoardItem::default()
            },
            BoardItem {
                id: "m2".into(),
                title: "Implement".into(),
                status: ItemStatus::InProgress,
                done_when: Some("cargo test -p foo".into()),
                plan: Some("patch handler".into()),
                ..BoardItem::default()
            },
            BoardItem {
                id: "m3".into(),
                title: "Audit".into(),
                status: ItemStatus::Pending,
                done_when: Some("cargo clippy".into()),
                ..BoardItem::default()
            },
        ];
        doc
    }

    #[test]
    fn inject_task_shows_work_items_source_meta() {
        let mut doc = sample_doc();
        doc.meta.work_item_mode = Some(WorkItemMode::Enumerated);
        doc.meta.expected_total = Some(10);
        doc.meta.work_items_source_path = Some("/tmp/campaign.xlsx".into());
        doc.meta.work_items_seeded_rows = Some(10);
        let block = markdown_runtime_block_for_inject(&doc, "conv-test", None);
        assert!(block.contains("- work_items_source: /tmp/campaign.xlsx (10 rows seeded)"));
    }

    #[test]
    fn inject_warns_when_meta_seeded_but_db_empty() {
        use crate::task_board::TaskBoardStore;

        let mut doc = sample_doc();
        doc.meta.work_item_mode = Some(WorkItemMode::Enumerated);
        doc.meta.work_items_seeded_rows = Some(127);
        doc.meta.work_items_source_path = Some("/tmp/cities.xlsx".into());
        let store = TaskBoardStore::new();
        let block = markdown_runtime_block_for_inject(
            &doc,
            "conv-mismatch",
            Some(store.work_items.as_ref()),
        );
        assert!(block.contains("## Work items host note"));
        assert!(block.contains("127 seeded row(s) but work_items DB has 0"));
        assert!(block.contains("Do NOT call task_board_init"));
    }

    #[test]
    fn inject_has_task_and_task_sections() {
        let block = markdown_runtime_block_for_inject(&sample_doc(), "conv-test", None);
        assert!(block.contains("## Task"));
        assert!(block.contains("## All tasks (with status)"));
        assert!(block.contains("## Current task"));
        assert!(block.contains("## Current task plan"));
        assert!(!block.contains("## Global milestones"));
        assert!(!block.contains("## Item milestones"));
        assert!(!block.contains("patch_layer"));
        assert!(!block.contains("validate_result_delta"));
    }

    #[test]
    fn inject_current_shows_done_when() {
        let block = markdown_runtime_block_for_inject(&sample_doc(), "conv-test", None);
        assert!(block.contains("- done_when: cargo test -p foo"));
        assert!(block.contains("- status: in_progress"));
    }

    #[test]
    fn inject_list_uses_done_when() {
        let block = markdown_runtime_block_for_inject(&sample_doc(), "conv-test", None);
        assert!(block.contains("- m1: Explore | done"));
        assert!(block.contains("  remark: grep done"));
        assert!(block.contains(
            "- m2: Implement | done_when: cargo test -p foo | in_progress"
        ));
    }

    #[test]
    fn substitute_city_placeholder() {
        let mut vars = std::collections::HashMap::new();
        vars.insert("city".into(), "深圳".into());
        let out = substitute_placeholders("切换至 {city} → 搜 Java", &vars);
        assert_eq!(out, "切换至 深圳 → 搜 Java");
    }

    #[test]
    fn unified_patch_target_step_mode_uses_global() {
        let mut doc = sample_doc();
        doc.meta.work_item_mode = None;
        let target = unified_patch_target(&doc, "conv-test", None, "m1").expect("target");
        assert_eq!(target, UnifiedPatchTarget::GlobalMilestones);
    }

    #[test]
    fn unified_patch_target_queue_exec_uses_item_template() {
        let mut doc = BoardDocument::empty_for_store_key("conv-q");
        doc.meta.work_item_mode = Some(WorkItemMode::Enumerated);
        doc.global_milestones = vec![
            BoardItem {
                id: "g_exec".into(),
                title: "Exec".into(),
                status: ItemStatus::InProgress,
                ..BoardItem::default()
            },
            BoardItem {
                id: "g_deliver".into(),
                title: "Deliver".into(),
                status: ItemStatus::Pending,
                ..BoardItem::default()
            },
        ];
        doc.item_milestones = vec![BoardItem {
            id: "m1".into(),
            title: "Step".into(),
            status: ItemStatus::InProgress,
            ..BoardItem::default()
        }];
        let target = unified_patch_target(&doc, "conv-q", None, "m1").expect("target");
        assert_eq!(target, UnifiedPatchTarget::ItemMilestones);
    }
}
