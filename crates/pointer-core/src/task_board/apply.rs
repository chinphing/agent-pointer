//! Apply task_board methods to a [`BoardDocument`].

use super::args::{board_rows_from_args, expected_total_from_args, goal_from_args, prune_ids_from_args};
use super::coordination::parent_child::{assert_child_may_mutate, parent_store_key_from_child};
use super::model::{
    BoardDocument, BoardItem, BoardScope, GlobalContext, ItemStatus, MetaStatus,
};
use super::state_machine::{
    bump_step_count, count_incomplete, dependencies_satisfied, mark_ready_pending_rows,
    validate_item_transition,
};
use anyhow::{anyhow, Result};
use serde_json::{json, Value};

const MAX_FINDING_LEN: usize = 500;
const MAX_FINDINGS: usize = 32;
const DONE_OUTPUT_SUMMARY_MAX_CHARS: usize = 800;
const INTERIM_DRAFTS_CHAR_BUDGET: usize = 80_000;
const INTERIM_DRAFT_ITEM_MAX_CHARS: usize = 2_000;

pub struct ApplyOutcome {
    pub body: Value,
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
    let (body, reflection_required) = match method.as_str() {
        "init" => {
            apply_init(store_key, doc, args)?;
            let mut body = json!({
                "ok": true,
                "method": "init",
                "board_len": doc.board.len(),
            });
            if !doc.meta.goal.is_empty() {
                body["goal"] = json!(doc.meta.goal);
            }
            (body, false)
        }
        "replace" => {
            apply_replace(doc, args)?;
            (
                json!({
                    "ok": true,
                    "method": "replace",
                    "board_len": doc.board.len(),
                }),
                false,
            )
        }
        "patch" | "" => {
            if doc.board_is_empty() {
                let body = json!({
                    "ok": true,
                    "method": "patch",
                    "board_len": 0,
                    "patched": [],
                    "skipped": true,
                    "reason": "board_not_initialized",
                });
                return Ok(ApplyOutcome {
                    body,
                    reflection_required: false,
                });
            }
            let (refl, warnings, patched) = apply_patch(doc, args)?;
            let mut body = json!({
                "ok": true,
                "method": "patch",
                "board_len": doc.board.len(),
                "patched": patched,
                "reflection_required": refl,
            });
            if !warnings.is_empty() {
                body["warnings"] = json!(warnings);
            }
            (body, refl)
        }
        "prune" => {
            let cancelled = apply_prune(doc, args)?;
            let mut body = json!({
                "ok": true,
                "method": "prune",
                "board_len": doc.board.len(),
            });
            if !cancelled.is_empty() {
                body["cancelled"] = json!(cancelled);
            }
            (body, false)
        }
        "finalize" => {
            apply_finalize(doc)?;
            (
                json!({
                    "ok": true,
                    "method": "finalize",
                    "board_len": doc.board.len(),
                    "meta_status": doc.meta.status.as_str(),
                }),
                false,
            )
        }
        "sync_finding" => {
            let body = apply_sync_finding(store_key, doc, args)?;
            (body, false)
        }
        "check_deps" => {
            let body = apply_check_deps(doc, args)?;
            (body, false)
        }
        other => return Err(anyhow!("task_board: unknown method {other}")),
    };
    mark_ready_pending_rows(doc);
    Ok(ApplyOutcome {
        body,
        reflection_required,
    })
}

fn row_status_entry(item: &BoardItem) -> Value {
    json!({
        "id": item.id,
        "status": item.status.as_str(),
    })
}

fn apply_init(store_key: &str, doc: &mut BoardDocument, args: &Value) -> Result<()> {
    if let Some(goal) = goal_from_args(args) {
        doc.meta.goal = goal;
    }
    if let Some(expected_total) = expected_total_from_args(args) {
        doc.meta.expected_total = Some(expected_total);
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
        validate_expected_total_row_count(doc.meta.expected_total, doc.board.len(), "init")?;
    }
    Ok(())
}

fn apply_replace(doc: &mut BoardDocument, args: &Value) -> Result<()> {
    let rows = board_rows_from_args(args);
    doc.board.clear();
    for v in &rows {
        if let Some(item) = BoardItem::from_value(v) {
            doc.board.push(item);
        }
    }
    if !doc.board.is_empty() {
        validate_expected_total_row_count(doc.meta.expected_total, doc.board.len(), "replace")?;
    }
    Ok(())
}

fn validate_expected_total_row_count(
    expected_total: Option<u32>,
    actual_rows: usize,
    method: &str,
) -> Result<()> {
    let Some(expected_total) = expected_total else {
        return Ok(());
    };
    let expected_total = expected_total as usize;
    if actual_rows == expected_total {
        return Ok(());
    }
    Err(anyhow!(
        "task_board:{method} expected exactly {expected_total} item(s), got {actual_rows}"
    ))
}

fn apply_patch(doc: &mut BoardDocument, args: &Value) -> Result<(bool, Vec<Value>, Vec<Value>)> {
    let recent_action = args
        .get("_recent_action_tools")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let recent_verify_pass = args
        .get("_recent_verify_pass")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let recent_verify_report = args
        .get("_recent_verify_report")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let rows = board_rows_from_args(args);
    let mut reflection = false;
    let mut warnings: Vec<Value> = Vec::new();
    let mut patched: Vec<Value> = Vec::new();
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
            maybe_warn_done_without_evidence(
                prev,
                &incoming,
                recent_action,
                &mut reflection,
                &mut warnings,
            );
            maybe_warn_done_without_verify_pass(
                prev,
                &incoming,
                recent_verify_report,
                recent_verify_pass,
                &mut reflection,
                &mut warnings,
            );
            if incoming.title.is_empty() {
                incoming.title = prev.title.clone();
            }
            if incoming.details.is_none() {
                incoming.details = prev.details.clone();
            }
            if incoming.progress.is_none() {
                incoming.progress = prev.progress.clone();
            }
            if incoming.validate.is_none() {
                incoming.validate = prev.validate.clone();
            }
            maybe_warn_in_progress_without_details(doc, prev, &incoming, &mut warnings);
            compact_item_after_success(prev, &mut incoming);
            doc.board[idx] = incoming;
            patched.push(row_status_entry(&doc.board[idx]));
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
            maybe_warn_done_without_evidence(
                &BoardItem {
                    id: incoming.id.clone(),
                    title: incoming.title.clone(),
                    status: ItemStatus::Pending,
                    ..BoardItem::default()
                },
                &incoming,
                recent_action,
                &mut reflection,
                &mut warnings,
            );
            maybe_warn_done_without_verify_pass(
                &BoardItem {
                    id: incoming.id.clone(),
                    title: incoming.title.clone(),
                    status: ItemStatus::Pending,
                    ..BoardItem::default()
                },
                &incoming,
                recent_verify_report,
                recent_verify_pass,
                &mut reflection,
                &mut warnings,
            );
            maybe_warn_in_progress_without_details(
                doc,
                &BoardItem {
                    id: incoming.id.clone(),
                    title: incoming.title.clone(),
                    status: ItemStatus::Pending,
                    ..BoardItem::default()
                },
                &incoming,
                &mut warnings,
            );
            compact_item_after_success(
                &BoardItem {
                    id: incoming.id.clone(),
                    title: incoming.title.clone(),
                    status: ItemStatus::Pending,
                    ..BoardItem::default()
                },
                &mut incoming,
            );
            doc.board.push(incoming);
            patched.push(row_status_entry(doc.board.last().expect("just pushed")));
        }
    }
    enforce_interim_drafts_budget(doc, &mut reflection, &mut warnings);
    Ok((reflection, warnings, patched))
}

fn maybe_warn_done_without_evidence(
    prev: &BoardItem,
    incoming: &BoardItem,
    recent_action: bool,
    reflection: &mut bool,
    warnings: &mut Vec<Value>,
) {
    if incoming.status != ItemStatus::Done || prev.status == ItemStatus::Done {
        return;
    }
    let has_output = incoming
        .output
        .as_ref()
        .filter(|s| !s.trim().is_empty())
        .or(prev.output.as_ref())
        .filter(|s| !s.trim().is_empty())
        .is_some();
    if has_output || recent_action {
        return;
    }
    let has_verification = incoming
        .validate
        .as_ref()
        .filter(|s| !s.trim().is_empty())
        .or(prev.validate.as_ref())
        .filter(|s| !s.trim().is_empty())
        .is_some();
    if has_verification {
        return;
    }
    *reflection = true;
    let reason = "done_without_evidence: add output, set validate, or run action tools before marking done";
    warnings.push(serde_json::json!({
        "code": "done_without_evidence",
        "requires_evidence": true,
        "message": reason
    }));
    log::warn!(
        "task_board_obs: done_soft_validation item_id={} reason={reason}",
        incoming.id
    );
}

fn maybe_warn_done_without_verify_pass(
    prev: &BoardItem,
    incoming: &BoardItem,
    recent_verify_report: bool,
    recent_verify_pass: bool,
    reflection: &mut bool,
    warnings: &mut Vec<Value>,
) {
    if incoming.status != ItemStatus::Done || prev.status == ItemStatus::Done {
        return;
    }
    if !recent_verify_report || recent_verify_pass {
        return;
    }
    *reflection = true;
    let reason = "done_without_verify_pass: verify:report should be pass before marking done";
    warnings.push(serde_json::json!({
        "code": "done_without_verify_pass",
        "requires_verify_pass": true,
        "message": reason
    }));
    log::warn!(
        "task_board_obs: done_soft_validation item_id={} reason={reason}",
        incoming.id
    );
}

fn compact_item_after_success(prev: &BoardItem, incoming: &mut BoardItem) {
    if incoming.status != ItemStatus::Done || prev.status == ItemStatus::Done {
        return;
    }
    if let Some(out) = incoming.output.as_ref() {
        let trimmed = out.trim();
        if trimmed.chars().count() > DONE_OUTPUT_SUMMARY_MAX_CHARS {
            let compact: String = trimmed.chars().take(DONE_OUTPUT_SUMMARY_MAX_CHARS).collect();
            incoming.output = Some(format!("{compact}…"));
        }
    }
}

fn maybe_warn_in_progress_without_details(
    doc: &BoardDocument,
    prev: &BoardItem,
    incoming: &BoardItem,
    warnings: &mut Vec<Value>,
) {
    if incoming.status != ItemStatus::InProgress || prev.status == ItemStatus::InProgress {
        return;
    }
    if doc.board.len() <= 1 {
        return;
    }
    let has_details = incoming
        .details
        .as_ref()
        .filter(|s| !s.trim().is_empty())
        .or(prev.details.as_ref())
        .filter(|s| !s.trim().is_empty())
        .is_some();
    if has_details {
        return;
    }
    let reason = "in_progress_without_details: add task details before or when marking in_progress";
    warnings.push(serde_json::json!({
        "code": "in_progress_without_details",
        "message": reason
    }));
    log::warn!(
        "task_board_obs: in_progress_soft_validation item_id={} reason={reason}",
        incoming.id
    );
}

fn enforce_interim_drafts_budget(
    doc: &mut BoardDocument,
    reflection: &mut bool,
    warnings: &mut Vec<Value>,
) {
    let Some(artifacts) = doc.global_context.artifacts.as_object_mut() else {
        return;
    };
    let Some(interim) = artifacts.get_mut("interim_drafts") else {
        return;
    };
    let Some(drafts) = interim.as_object_mut() else {
        return;
    };
    let mut total_chars = 0usize;
    for v in drafts.values() {
        if let Some(s) = v.as_str() {
            total_chars = total_chars.saturating_add(s.chars().count());
        }
    }
    if total_chars <= INTERIM_DRAFTS_CHAR_BUDGET {
        return;
    }
    let mut shortened = 0usize;
    for v in drafts.values_mut() {
        let Some(s) = v.as_str() else {
            continue;
        };
        let chars = s.chars().count();
        if chars <= INTERIM_DRAFT_ITEM_MAX_CHARS {
            continue;
        }
        let compact: String = s.chars().take(INTERIM_DRAFT_ITEM_MAX_CHARS).collect();
        *v = Value::String(format!("{compact}\n\n[trimmed_by_engine_for_context_budget]"));
        shortened += 1;
    }
    if shortened == 0 {
        return;
    }
    *reflection = true;
    warnings.push(serde_json::json!({
        "code": "interim_drafts_budget_exceeded",
        "requires_memory_summarization": true,
        "message": format!(
            "interim_drafts exceeded char budget {}; engine compacted {} draft(s)",
            INTERIM_DRAFTS_CHAR_BUDGET,
            shortened
        ),
    }));
}

fn apply_prune(doc: &mut BoardDocument, args: &Value) -> Result<Vec<Value>> {
    let ids = prune_ids_from_args(args);
    let mut cancelled = Vec::new();
    if ids.is_empty() {
        for item in doc.board.iter_mut() {
            if item.status == ItemStatus::Pending {
                item.status = ItemStatus::Cancelled;
                cancelled.push(row_status_entry(item));
            }
        }
    } else {
        for item in doc.board.iter_mut() {
            if ids.contains(&item.id) && item.status == ItemStatus::Pending {
                item.status = ItemStatus::Cancelled;
                cancelled.push(row_status_entry(item));
            }
        }
    }
    Ok(cancelled)
}

fn apply_finalize(doc: &mut BoardDocument) -> Result<()> {
    let n = count_incomplete(doc);
    if n > 0 {
        return Err(anyhow!(
            "task_board: finalize blocked — {n} item(s) still incomplete"
        ));
    }
    doc.meta.status = MetaStatus::Completed;
    Ok(())
}

fn apply_sync_finding(_store_key: &str, doc: &mut BoardDocument, args: &Value) -> Result<Value> {
    // Fast-path route should be handled in TaskBoardStore::apply.
    // Keep this as a non-failing fallback so UI cards do not show failed when a caller
    // reaches apply_method directly.
    if let Some(finding) = super::args::finding_from_args(args) {
        return apply_sync_finding_to_doc(doc, &finding);
    }
    Ok(serde_json::json!({
        "ok": true,
        "method": "sync_finding",
        "message": "sync_finding handled by store route; no finding appended"
    }))
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
