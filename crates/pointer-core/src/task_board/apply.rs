//! Apply task_board methods to a [`BoardDocument`].

use super::args::{board_rows_from_args, goal_from_args, prune_ids_from_args};
use super::coordination::parent_child::{assert_child_may_mutate, parent_store_key_from_child};
use super::model::{
    BoardDocument, BoardItem, BoardScope, GlobalContext, ItemStatus, MetaStatus,
};
use super::state_machine::{
    bump_step_count, count_incomplete, dependencies_satisfied, mark_ready_pending_rows,
    validate_item_transition,
};
use anyhow::{anyhow, Result};
use serde_json::Value;

const MAX_FINDING_LEN: usize = 500;
const MAX_FINDINGS: usize = 32;

pub struct ApplyOutcome {
    pub summary: Value,
    pub reflection_required: bool,
}

pub fn apply_method(
    store_key: &str,
    doc: &mut BoardDocument,
    method: &str,
    args: &Value,
) -> Result<ApplyOutcome> {
    assert_child_may_mutate(store_key, doc, method)?;
    let method = method.trim().to_ascii_lowercase();
    let mut reflection_required = false;
    let summary = match method.as_str() {
        "init" => apply_init(store_key, doc, args)?,
        "replace" => apply_replace(doc, args)?,
        "patch" | "" => {
            reflection_required = apply_patch(doc, args)?;
            json_summary("patch", doc.board.len())
        }
        "prune" => {
            apply_prune(doc, args)?;
            json_summary("prune", doc.board.len())
        }
        "finalize" => apply_finalize(doc)?,
        "sync_finding" => apply_sync_finding(store_key, doc, args)?,
        "check_deps" => apply_check_deps(doc, args)?,
        other => return Err(anyhow!("task_board: unknown method {other}")),
    };
    mark_ready_pending_rows(doc);
    Ok(ApplyOutcome {
        summary,
        reflection_required,
    })
}

fn json_summary(method: &str, count: usize) -> Value {
    serde_json::json!({ "ok": true, "method": method, "count": count })
}

fn apply_init(store_key: &str, doc: &mut BoardDocument, args: &Value) -> Result<Value> {
    if let Some(goal) = goal_from_args(args) {
        doc.meta.goal = goal;
    }
    if let Some(scope) = args.get("scope").and_then(|v| v.as_str()) {
        doc.meta.scope = match scope.trim().to_ascii_lowercase().as_str() {
            "parent" => Some(BoardScope::Parent),
            "child" => Some(BoardScope::Child),
            _ => doc.meta.scope,
        };
    }
    if let Some(root) = args
        .get("root_target")
        .or_else(|| args.get("parent_sub_task_id"))
        .and_then(|v| v.as_str())
    {
        let t = root.trim().to_string();
        doc.meta.root_target = Some(t.clone());
        doc.meta.parent_sub_task_id = Some(t);
    }
    if parent_store_key_from_child(store_key).is_some() {
        doc.meta.scope = Some(BoardScope::Child);
        doc.meta.parent_store_key = parent_store_key_from_child(store_key);
    } else if doc.meta.scope.is_none() {
        doc.meta.scope = Some(BoardScope::Parent);
    }
    if let Some(gc) = args.get("global_context") {
        merge_global_context(&mut doc.global_context, gc);
    }
    let rows = board_rows_from_args(args);
    if !rows.is_empty() {
        doc.board.clear();
        for v in &rows {
            if let Some(item) = BoardItem::from_value(v) {
                doc.board.push(item);
            }
        }
    }
    Ok(json_summary("init", doc.board.len()))
}

fn apply_replace(doc: &mut BoardDocument, args: &Value) -> Result<Value> {
    let rows = board_rows_from_args(args);
    doc.board.clear();
    for v in &rows {
        if let Some(item) = BoardItem::from_value(v) {
            doc.board.push(item);
        }
    }
    Ok(json_summary("replace", doc.board.len()))
}

fn apply_patch(doc: &mut BoardDocument, args: &Value) -> Result<bool> {
    let rows = board_rows_from_args(args);
    let mut reflection = false;
    if let Some(gc) = args.get("global_context") {
        merge_global_context(&mut doc.global_context, gc);
    }
    for v in &rows {
        let Some(mut incoming) = BoardItem::from_value(v) else {
            continue;
        };
        if let Some(idx) = doc.board.iter().position(|e| e.id == incoming.id) {
            let prev = &doc.board[idx];
            validate_item_transition(prev.status, incoming.status)?;
            if matches!(incoming.status, ItemStatus::InProgress | ItemStatus::Done) {
                if !dependencies_satisfied(doc, &incoming) {
                    return Err(anyhow!(
                        "task_board: dependencies not satisfied for id {}",
                        incoming.id
                    ));
                }
                bump_step_count(&mut doc.meta)?;
            }
            if incoming.status == ItemStatus::InProgress && prev.status != ItemStatus::InProgress
            {
                bump_step_count(&mut doc.meta)?;
            }
            if incoming.retry_count >= 2
                && matches!(
                    incoming.status,
                    ItemStatus::InProgress | ItemStatus::Failed
                )
            {
                reflection = true;
            }
            if incoming.title.is_empty() {
                incoming.title = prev.title.clone();
            }
            doc.board[idx] = incoming;
        } else {
            if matches!(incoming.status, ItemStatus::InProgress | ItemStatus::Done) {
                if !dependencies_satisfied(doc, &incoming) {
                    return Err(anyhow!(
                        "task_board: dependencies not satisfied for new id {}",
                        incoming.id
                    ));
                }
                bump_step_count(&mut doc.meta)?;
            }
            doc.board.push(incoming);
        }
    }
    Ok(reflection)
}

fn apply_prune(doc: &mut BoardDocument, args: &Value) -> Result<()> {
    let ids = prune_ids_from_args(args);
    if ids.is_empty() {
        doc.board.retain(|i| i.status != ItemStatus::Pending);
    } else {
        for item in doc.board.iter_mut() {
            if ids.contains(&item.id) && item.status == ItemStatus::Pending {
                item.status = ItemStatus::Cancelled;
            }
        }
    }
    Ok(())
}

fn apply_finalize(doc: &mut BoardDocument) -> Result<Value> {
    let n = count_incomplete(doc);
    if n > 0 {
        return Err(anyhow!(
            "task_board: finalize blocked — {n} item(s) still incomplete"
        ));
    }
    doc.meta.status = MetaStatus::Completed;
    Ok(json_summary("finalize", doc.board.len()))
}

fn apply_sync_finding(_store_key: &str, _doc: &mut BoardDocument, _args: &Value) -> Result<Value> {
    Err(anyhow!(
        "task_board: sync_finding is handled by TaskBoardStore (routes to parent global_context)"
    ))
}

fn apply_check_deps(doc: &mut BoardDocument, args: &Value) -> Result<Value> {
    let id = super::args::check_item_id_from_args(args)
        .ok_or_else(|| anyhow!("task_board: check_deps requires item_id"))?;
    let item = doc
        .board
        .iter()
        .find(|i| i.id == id)
        .ok_or_else(|| anyhow!("task_board: unknown item_id {id}"))?;
    if dependencies_satisfied(doc, item) {
        Ok(serde_json::json!({
            "ok": true,
            "method": "check_deps",
            "item_id": id,
            "status": "ready"
        }))
    } else {
        let blocking: Vec<&str> = item
            .depends_on
            .iter()
            .filter(|dep| {
                !doc.board.iter().any(|row| {
                    row.id == **dep
                        && matches!(
                            row.status,
                            ItemStatus::Done | ItemStatus::Cancelled
                        )
                })
            })
            .map(|s| s.as_str())
            .collect();
        Ok(serde_json::json!({
            "ok": true,
            "method": "check_deps",
            "item_id": id,
            "status": "blocked",
            "reason": format!("dependencies not satisfied: {}", blocking.join(", "))
        }))
    }
}

pub fn apply_sync_finding_to_doc(doc: &mut BoardDocument, finding: &str) -> Result<Value> {
    let t = finding.trim();
    if t.is_empty() {
        return Err(anyhow!("task_board: empty finding"));
    }
    let entry = if t.len() > MAX_FINDING_LEN {
        format!("{}…", &t[..MAX_FINDING_LEN])
    } else {
        t.to_string()
    };
    if !doc.global_context.key_findings.iter().any(|f| f == &entry) {
        if doc.global_context.key_findings.len() >= MAX_FINDINGS {
            doc.global_context.key_findings.remove(0);
        }
        doc.global_context.key_findings.push(entry);
    }
    Ok(serde_json::json!({
        "ok": true,
        "method": "sync_finding",
        "findings_count": doc.global_context.key_findings.len()
    }))
}

fn merge_global_context(gc: &mut GlobalContext, patch: &Value) {
    if let Some(arr) = patch.get("key_findings").and_then(|v| v.as_array()) {
        for e in arr {
            if let Some(s) = e.as_str() {
                let mut tmp = BoardDocument::empty_for_store_key("_merge");
                tmp.global_context = gc.clone();
                if apply_sync_finding_to_doc(&mut tmp, s).is_ok() {
                    gc.key_findings = tmp.global_context.key_findings;
                }
            }
        }
    }
    if let Some(art) = patch.get("artifacts") {
        if art.is_object() {
            gc.artifacts = art.clone();
        }
    }
}
