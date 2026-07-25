//! Compact and full prompt snapshots (v4 inject).

use super::model::{BoardDocument, BoardItem, ItemStatus};
use super::state_machine::dependencies_satisfied;
const PLAN_INJECT_MAX: usize = 2000;
const RULES_INJECT_MAX: usize = 2000;
const DONE_WHEN_INJECT_MAX: usize = 600;
const LIST_DONE_WHEN_INJECT_MAX: usize = 120;
const READY_HINT_COUNT: usize = 2;
const RESULT_SNIPPET_INJECT_MAX: usize = 120;
/// Visible rows in `## All tasks` around the current task (3 before + current + 5 after).
const ALL_TASKS_WINDOW_BEFORE: usize = 3;
const ALL_TASKS_WINDOW_AFTER: usize = 5;

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
    format_parent_tunnel_block_full(parent, sub_task_id)
}

pub fn format_parent_tunnel_block_full(parent: &BoardDocument, sub_task_id: &str) -> String {
    let mut lines = vec![
        "[TASK_BOARD_PARENT]".to_string(),
        "read_only: true".to_string(),
    ];
    if !parent.meta.goal.is_empty() {
        lines.push(format!("goal: {}", parent.meta.goal));
    }
    if !parent.meta.context.trim().is_empty() {
        lines.push(format!("context: {}", parent.meta.context.trim()));
    }
    if let Some(c) = parent
        .meta
        .constraints
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        append_constraints_block(&mut lines, "constraints", c);
    }
    if let Some(dw) = parent
        .meta
        .done_when
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
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
    if let Some(item) = current_task_in_slice(&parent.global_milestones) {
        lines.push(format!(
            "current_task: {} · {} · {}",
            item.id,
            item.title,
            item.status.as_str()
        ));
    }
    for f in parent.global_context.key_findings.iter().rev().take(8) {
        lines.push(format!("finding: {f}"));
    }
    if let Some(row) = parent
        .global_milestones
        .iter()
        .find(|i| i.id == sub_task_id)
    {
        lines.push(format!(
            "current_milestone: id={} status={} title={}",
            row.id,
            row.status.as_str(),
            row.title
        ));
    }
    lines.join("\n")
}

pub fn markdown_runtime_block_for_inject(doc: &BoardDocument, _store_key: &str) -> String {
    let mut lines: Vec<String> = Vec::new();
    lines.push("[TASK_BOARD]".to_string());
    lines.push(String::new());
    let mode = milestone_inject_mode(doc);
    append_task_section(&mut lines, doc);

    match mode {
        MilestoneInjectMode::Step => {
            append_all_tasks_list(&mut lines, &doc.global_milestones);
            if let Some(current) = current_task_in_slice(&doc.global_milestones) {
                append_current_task_section(&mut lines, current, doc);
            }
        }
        MilestoneInjectMode::QueueExec => {
            append_loop_shared_plan_section(&mut lines, doc);
            let items: Vec<BoardItem> = super::loop_milestones::loop_item_rows(doc)
                .into_iter()
                .cloned()
                .collect();
            append_all_tasks_list(&mut lines, &items);
            if let Some(current) = current_task_in_slice(&items) {
                append_current_task_section(&mut lines, current, doc);
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
                append_current_task_section(&mut lines, &deliver[0], doc);
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

pub fn milestone_inject_mode(doc: &BoardDocument) -> MilestoneInjectMode {
    if super::loop_milestones::is_loop_milestone_board(doc) {
        if super::loop_milestones::loop_exec_met(doc) {
            return MilestoneInjectMode::QueueDeliver;
        }
        return MilestoneInjectMode::QueueExec;
    }
    MilestoneInjectMode::Step
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnifiedPatchTarget {
    GlobalMilestones,
}

/// Route a unified `milestones` patch row to the document slice matching inject projection.
pub fn unified_patch_target(
    doc: &BoardDocument,
    row_id: &str,
) -> anyhow::Result<UnifiedPatchTarget> {
    use anyhow::anyhow;
    let id = row_id.trim();
    if id.is_empty() {
        return Err(anyhow!("task_board: patch row requires id"));
    }
    let mode = milestone_inject_mode(doc);
    match mode {
        MilestoneInjectMode::Step => {
            if doc.global_milestones.iter().any(|r| r.id == id) {
                return Ok(UnifiedPatchTarget::GlobalMilestones);
            }
            if !id.starts_with("g_") {
                return Ok(UnifiedPatchTarget::GlobalMilestones);
            }
            Err(anyhow!(
                "task_board: patch id {id} not in visible step ladder (global_milestones)"
            ))
        }
        MilestoneInjectMode::QueueExec => {
            if super::loop_milestones::is_loop_item_id(doc, id) {
                return Ok(UnifiedPatchTarget::GlobalMilestones);
            }
            if id.starts_with("g_") {
                return Err(anyhow!(
                    "task_board: during loop exec patch item rows (wi_*) only — not {id}"
                ));
            }
            Err(anyhow!(
                "task_board: patch id {id} not in visible loop item ladder"
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
        return;
    }
    let cur = current_task_index(items);
    let start = cur.saturating_sub(ALL_TASKS_WINDOW_BEFORE);
    let end = (cur + 1 + ALL_TASKS_WINDOW_AFTER).min(items.len());
    if start > 0 {
        lines.push(format_omitted_tasks_line(&items[..start], "earlier"));
    }
    for item in &items[start..end] {
        lines.push(format_item_list_line(item));
    }
    if end < items.len() {
        lines.push(format_omitted_tasks_line(&items[end..], "later"));
    }
}

fn current_task_index(items: &[BoardItem]) -> usize {
    items
        .iter()
        .position(|i| i.status == ItemStatus::InProgress)
        .or_else(|| items.iter().position(|i| i.status == ItemStatus::Ready))
        .or_else(|| items.iter().position(|i| i.status == ItemStatus::Pending))
        .unwrap_or(0)
}

fn format_omitted_tasks_line(items: &[BoardItem], position: &str) -> String {
    if items.is_empty() {
        return String::new();
    }
    let first = items[0].id.trim();
    let last = items[items.len() - 1].id.trim();
    let range = if items.len() == 1 || first == last {
        first.to_string()
    } else {
        format!("{first}…{last}")
    };
    let mut done = 0u32;
    let mut failed = 0u32;
    let mut pending = 0u32;
    let mut other = 0u32;
    for item in items {
        match item.status {
            ItemStatus::Done => done += 1,
            ItemStatus::Failed | ItemStatus::Cancelled => failed += 1,
            ItemStatus::Pending | ItemStatus::Ready => pending += 1,
            _ => other += 1,
        }
    }
    let mut parts: Vec<String> = Vec::new();
    if done > 0 {
        parts.push(format!("{done} done"));
    }
    if failed > 0 {
        parts.push(format!("{failed} failed"));
    }
    if pending > 0 {
        parts.push(format!("{pending} pending"));
    }
    if other > 0 {
        parts.push(format!("{other} active"));
    }
    let summary = if parts.is_empty() {
        format!("{} tasks", items.len())
    } else {
        format!("{} tasks: {}", items.len(), parts.join(", "))
    };
    format!("- … {range} omitted ({position}; {summary}) …")
}

fn append_loop_shared_plan_section(lines: &mut Vec<String>, doc: &BoardDocument) {
    let Some(plan) = super::loop_milestones::loop_shared_plan(doc) else {
        return;
    };
    lines.push(String::new());
    lines.push("## Loop procedure (shared)".to_string());
    lines.push(truncate_field(Some(plan.as_str()), PLAN_INJECT_MAX));
}

fn append_current_task_section(lines: &mut Vec<String>, item: &BoardItem, doc: &BoardDocument) {
    lines.push(String::new());
    lines.push("## Current task".to_string());
    append_current_row_bullets(lines, item, doc);
    let plan = item.plan.as_deref().filter(|s| !s.trim().is_empty());
    if plan.is_some() {
        lines.push(String::new());
        lines.push("## Current task plan".to_string());
        lines.push(truncate_field(plan, PLAN_INJECT_MAX));
    }
    let rules = item.rules.as_deref().filter(|s| !s.trim().is_empty());
    if rules.is_some() {
        lines.push(String::new());
        lines.push("## Current task rules".to_string());
        lines.push(truncate_field(rules, RULES_INJECT_MAX));
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
    if doc
        .meta
        .constraints
        .as_deref()
        .map(|s| s.trim().is_empty())
        .unwrap_or(true)
    {
        lines.push("- constraints: n/a".to_string());
    } else {
        append_constraints_block(
            lines,
            "- constraints",
            doc.meta.constraints.as_deref().unwrap_or(""),
        );
    }
    lines.push(format!(
        "- done_when: {}",
        meta_or_na(doc.meta.done_when.as_deref())
    ));
    if let Some(n) = doc.meta.expected_total {
        lines.push(format!("- expected_total: {n}"));
    }
}

fn append_current_row_bullets(lines: &mut Vec<String>, item: &BoardItem, _doc: &BoardDocument) {
    lines.push(format!("- id: {}", item.id));
    lines.push(format!("- title: {}", item.title.trim()));
    lines.push(format!(
        "- done_when: {}",
        truncate_field(item.done_when.as_deref(), DONE_WHEN_INJECT_MAX)
    ));
    lines.push(format!("- status: {}", item.status.as_str()));
    append_milestone_rules_and_constraints(lines, item);
    if !item.depends_on.is_empty() {
        lines.push(format!("- depends_on: {}", item.depends_on.join(", ")));
    }
    if let Some(b) = item.blocked_by.as_deref().filter(|s| !s.trim().is_empty()) {
        lines.push(format!("- blocked_by: {b}"));
    }
}

fn append_constraints_block(lines: &mut Vec<String>, label: &str, constraints: &str) {
    let t = constraints.trim();
    if t.is_empty() {
        return;
    }
    if t.contains('\n') {
        lines.push(format!("{label}:"));
        for line in t.lines() {
            let line = line.trim();
            if !line.is_empty() {
                lines.push(format!("  {line}"));
            }
        }
    } else {
        lines.push(format!("{label}: {t}"));
    }
}

fn append_milestone_rules_and_constraints(lines: &mut Vec<String>, item: &BoardItem) {
    if let Some(c) = item.constraints.as_deref().filter(|s| !s.trim().is_empty()) {
        append_constraints_block(lines, "- constraints", c);
    }
}

pub fn substitute_placeholders(
    template: &str,
    vars: &std::collections::HashMap<String, String>,
) -> String {
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
    let title = if title.is_empty() {
        "(untitled)"
    } else {
        title
    };
    if matches!(
        item.status,
        ItemStatus::InProgress | ItemStatus::Pending | ItemStatus::Ready
    ) {
        return format!(
            "- {}: {} | done_when: {} | {}",
            item.id,
            title,
            done_when_list_line(item),
            item.status.as_str()
        );
    }
    let base = format!("- {}: {} | {}", item.id, title, item.status.as_str());
    if let Some(r) = item.remark.as_deref().filter(|s| !s.trim().is_empty()) {
        let remark = truncate_field(Some(r), LIST_DONE_WHEN_INJECT_MAX);
        return format!("{base}\n  remark: {remark}");
    }
    base
}

fn done_when_list_line(item: &BoardItem) -> String {
    truncate_field(item.done_when.as_deref(), LIST_DONE_WHEN_INJECT_MAX)
}

fn format_item_list_line(item: &BoardItem) -> String {
    format_global_list_line(item)
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
    if let Some(v) = item.rules.as_ref().filter(|s| !s.trim().is_empty()) {
        let compact = v.chars().take(RULES_INJECT_MAX).collect::<String>();
        out.insert("rules".into(), compact.into());
    }
    if let Some(v) = item.constraints.as_ref().filter(|s| !s.trim().is_empty()) {
        out.insert("constraints".into(), v.clone().into());
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
    fn inject_has_task_and_task_sections() {
        let block = markdown_runtime_block_for_inject(&sample_doc(), "conv-test");
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
        let block = markdown_runtime_block_for_inject(&sample_doc(), "conv-test");
        assert!(block.contains("- done_when: cargo test -p foo"));
        assert!(block.contains("- status: in_progress"));
    }

    #[test]
    fn inject_list_windows_around_current_task() {
        let mut doc = BoardDocument::empty_for_store_key("conv-window");
        doc.meta.goal = "Batch".into();
        let mut rows: Vec<BoardItem> = Vec::new();
        for i in 1..=12 {
            rows.push(BoardItem {
                id: format!("wi_{i}"),
                title: format!("Item {i}"),
                status: if i < 6 {
                    ItemStatus::Done
                } else if i == 6 {
                    ItemStatus::InProgress
                } else {
                    ItemStatus::Pending
                },
                done_when: Some(format!("done {i}")),
                ..BoardItem::default()
            });
        }
        doc.global_milestones = rows;
        let block = markdown_runtime_block_for_inject(&doc, "conv-window");
        assert!(block.contains("… wi_1…wi_2 omitted (earlier; 2 tasks: 2 done) …"));
        assert!(block.contains("- wi_3:"));
        assert!(block.contains("- wi_6:"));
        assert!(block.contains("- wi_11:"));
        assert!(block.contains("… wi_12 omitted (later; 1 tasks: 1 pending) …"));
        assert!(!block.contains("- wi_1:"));
        assert!(!block.contains("- wi_12:"));
    }

    #[test]
    fn inject_list_shows_all_when_within_window() {
        let mut doc = BoardDocument::empty_for_store_key("conv-small");
        doc.meta.goal = "Small".into();
        doc.global_milestones = vec![
            BoardItem {
                id: "m1".into(),
                title: "One".into(),
                status: ItemStatus::Done,
                ..BoardItem::default()
            },
            BoardItem {
                id: "m2".into(),
                title: "Two".into(),
                status: ItemStatus::InProgress,
                ..BoardItem::default()
            },
        ];
        let block = markdown_runtime_block_for_inject(&doc, "conv-small");
        assert!(block.contains("- m1:"));
        assert!(block.contains("- m2:"));
        assert!(!block.contains("omitted"));
    }

    #[test]
    fn inject_list_uses_done_when() {
        let block = markdown_runtime_block_for_inject(&sample_doc(), "conv-test");
        assert!(block.contains("- m1: Explore | done"));
        assert!(block.contains("  remark: grep done"));
        assert!(block.contains("- m2: Implement | done_when: cargo test -p foo | in_progress"));
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
        let doc = sample_doc();
        let target = unified_patch_target(&doc, "m1").expect("target");
        assert_eq!(target, UnifiedPatchTarget::GlobalMilestones);
    }
}
