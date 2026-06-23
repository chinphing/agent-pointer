//! Apply task_board methods to a [`BoardDocument`].

use super::args::{
    constraint_from_args, context_from_args, done_when_from_args,
    expected_total_from_args, global_patch_rows_from_args, global_rows_from_args, goal_from_args,
    item_milestones_from_args, milestone_patch_rows_from_args, prune_ids_from_args,
    replace_has_forbidden_scope,
};
use super::coordination::parent_child::{assert_child_may_mutate, parent_store_key_from_child};
use super::model::{
    BoardDocument, BoardItem, BoardScope, GlobalContext, ItemStatus, MetaStatus,
};
use super::row_patch::{compact_row_after_done, merge_row_patch_with_warnings};
use super::state_machine::{
    count_incomplete, dependencies_satisfied, dependencies_satisfied_rows, mark_ready_pending_rows,
    validate_item_transition,
};
use super::work_item::WorkItemStore;
use super::work_items_apply::{
    apply_meta_work_item_mode, apply_work_item_patch_fields,
    maybe_auto_complete_g_exec, milestone_done_has_work_item_evidence, patch_rejects_g_deliver_when_blocked,
    patch_rejects_g_exec_done_when_not_met, patch_rejects_v3_delta_fields, reset_item_milestones,
    seed_work_items_on_init, validate_board_row_count, validate_expected_total_after_seed,
    validate_work_item_init, work_items_enabled_from_args, workspace_root_from_args,
};
use anyhow::{anyhow, Result};
use serde_json::{json, Value};

const MAX_FINDING_LEN: usize = 500;
const MAX_FINDINGS: usize = 32;
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
    work_items: &WorkItemStore,
) -> Result<ApplyOutcome> {
    assert_child_may_mutate(store_key, doc, method)?;
    let method = method.trim().to_ascii_lowercase();
    let work_items_enabled = work_items_enabled_from_args(args);
    let (body, reflection_required) = match method.as_str() {
        "init" => {
            let seeded = apply_init(store_key, doc, args, work_items, work_items_enabled)?;
            let mut body = json!({
                "ok": true,
                "method": "init",
                "board_len": doc.global_milestones.len(),
            });
            if !doc.meta.goal.is_empty() {
                body["goal"] = json!(doc.meta.goal);
            }
            if seeded > 0 {
                body["work_items_seeded"] = json!(seeded);
            }
            (body, false)
        }
        "replace" => {
            let seeded = apply_replace(store_key, doc, args, work_items, work_items_enabled)?;
            let mut body = json!({
                "ok": true,
                "method": "replace",
                "board_len": doc.global_milestones.len(),
            });
            if seeded > 0 {
                body["work_items_seeded"] = json!(seeded);
            }
            (body, false)
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
            let (refl, warnings, patched) =
                apply_patch(store_key, doc, args, work_items, work_items_enabled)?;
            let mut body = json!({
                "ok": true,
                "method": "patch",
                "board_len": doc.global_milestones.len(),
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
                "board_len": doc.global_milestones.len(),
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
                    "board_len": doc.global_milestones.len(),
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

fn apply_meta_fields(doc: &mut BoardDocument, args: &Value) {
    if let Some(goal) = goal_from_args(args) {
        doc.meta.goal = goal;
    }
    if let Some(ctx) = context_from_args(args) {
        doc.meta.context = ctx;
    }
    if let Some(c) = constraint_from_args(args) {
        doc.meta.constraint = Some(c);
    }
    if let Some(dw) = done_when_from_args(args) {
        doc.meta.done_when = Some(dw);
    }
    if let Some(expected_total) = expected_total_from_args(args) {
        doc.meta.expected_total = Some(expected_total);
    }
    apply_meta_work_item_mode(doc, args);
}

fn apply_init(
    store_key: &str,
    doc: &mut BoardDocument,
    args: &Value,
    work_items: &WorkItemStore,
    work_items_enabled: bool,
) -> Result<u32> {
    apply_meta_fields(doc, args);
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
    let global_rows = global_rows_from_args(args);
    if !global_rows.is_empty() {
        doc.global_milestones.clear();
        for v in &global_rows {
            if let Some(item) = BoardItem::from_value(v) {
                doc.global_milestones.push(item);
            }
        }
        validate_board_row_count(doc.global_milestones.len(), "init")?;
    }
    let item_rows = item_milestones_from_args(args);
    if !item_rows.is_empty() {
        doc.item_milestones.clear();
        for v in &item_rows {
            if let Some(item) = BoardItem::from_value(v) {
                doc.item_milestones.push(item);
            }
        }
        validate_board_row_count(doc.item_milestones.len(), "init")?;
    }
    validate_work_item_init(args, doc, work_items_enabled)?;
    let seeded = seed_work_items_on_init(
        store_key,
        doc,
        args,
        work_items,
        work_items_enabled,
        workspace_root_from_args(args),
    )?;
    if work_items_enabled && seeded > 0 && doc.meta.expected_total.is_none() {
        doc.meta.expected_total = Some(seeded);
    }
    validate_expected_total_after_seed(doc, work_items, store_key, "init")?;
    Ok(seeded)
}

fn apply_replace(
    _store_key: &str,
    doc: &mut BoardDocument,
    args: &Value,
    _work_items: &WorkItemStore,
    _work_items_enabled: bool,
) -> Result<u32> {
    if replace_has_forbidden_scope(args) {
        return Err(anyhow!("replace_scope_forbidden"));
    }
    let rows = item_milestones_from_args(args);
    if rows.is_empty() {
        return Err(anyhow!("task_board: replace requires item_milestones[]"));
    }
    doc.item_milestones.clear();
    for v in &rows {
        if let Some(item) = BoardItem::from_value(v) {
            doc.item_milestones.push(item);
        }
    }
    validate_board_row_count(doc.item_milestones.len(), "replace")?;
    Ok(0)
}

fn apply_patch(
    store_key: &str,
    doc: &mut BoardDocument,
    args: &Value,
    work_items: &WorkItemStore,
    work_items_enabled: bool,
) -> Result<(bool, Vec<Value>, Vec<Value>)> {
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

    let item_rows = milestone_patch_rows_from_args(args);
    let global_rows = global_patch_rows_from_args(args);
    if item_rows.is_some() && global_rows.is_some() {
        return Err(anyhow!(
            "task_board: patch cannot send both global_milestones and milestones"
        ));
    }

    let mut reflection = false;
    let mut warnings: Vec<Value> = Vec::new();
    let mut patched: Vec<Value> = Vec::new();
    if let Some(gc) = args.get("global_context") {
        merge_global_context(&mut doc.global_context, gc);
    }

    let wi_note = apply_work_item_patch_fields(
        store_key,
        doc,
        args,
        work_items,
        work_items_enabled,
    )?;
    if let Some(note) = wi_note {
        log::info!("task_board: work_items {note}");
    }

    let global_snapshot = doc.global_milestones.clone();
    let has_work_items = doc.has_work_items();
    let global_len = doc.global_milestones.len();
    let rows = if let Some(item_rows) = item_rows {
        patch_rows_on_slice(
            &mut doc.item_milestones,
            &item_rows,
            &global_snapshot,
            has_work_items,
            global_len,
            store_key,
            work_items,
            recent_action,
            recent_verify_report,
            recent_verify_pass,
            &mut reflection,
            &mut warnings,
            &mut patched,
        )?
    } else if let Some(global_rows) = global_rows {
        patch_rows_on_slice(
            &mut doc.global_milestones,
            &global_rows,
            &global_snapshot,
            has_work_items,
            global_len,
            store_key,
            work_items,
            recent_action,
            recent_verify_report,
            recent_verify_pass,
            &mut reflection,
            &mut warnings,
            &mut patched,
        )?
    } else {
        Vec::new()
    };
    let _ = rows;

    if args.get("work_item_delta").is_some() {
        if let Some(delta_v) = args.get("work_item_delta") {
            if let Some(status) = delta_v.get("status").and_then(|s| s.as_str()) {
                if status == "done" || status == "failed" {
                    reset_item_milestones(doc);
                }
            }
        }
        maybe_auto_complete_g_exec(doc, work_items, store_key);
    }

    enforce_interim_drafts_budget(doc, &mut reflection, &mut warnings);
    Ok((reflection, warnings, patched))
}

fn patch_rows_on_slice(
    target: &mut Vec<BoardItem>,
    rows: &[Value],
    global_rows: &[BoardItem],
    has_work_items: bool,
    global_len: usize,
    store_key: &str,
    work_items: &WorkItemStore,
    recent_action: bool,
    recent_verify_report: bool,
    recent_verify_pass: bool,
    reflection: &mut bool,
    warnings: &mut Vec<Value>,
    patched: &mut Vec<Value>,
) -> Result<Vec<Value>> {
    for v in rows {
        let id = v
            .get("id")
            .and_then(|x| x.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let Some(id) = id else {
            continue;
        };
        patch_rejects_v3_delta_fields(v, &BoardItem::default(), false)?;
        if let Some(idx) = target.iter().position(|e| e.id == id) {
            let prev = target[idx].clone();
            let merged = merge_row_patch_with_warnings(&prev, v);
            let mut incoming = merged.row;
            warnings.extend(merged.warnings);
            patch_rejects_g_exec_done_when_not_met(
                global_rows,
                &id,
                incoming.status,
                has_work_items,
                work_items,
                store_key,
            )?;
            patch_rejects_g_deliver_when_blocked(
                global_rows,
                &id,
                incoming.status,
                has_work_items,
                work_items,
                store_key,
            )?;
            validate_item_transition(prev.status, incoming.status)?;
            if matches!(incoming.status, ItemStatus::InProgress | ItemStatus::Done) {
                if !dependencies_satisfied_rows(global_rows, &incoming) {
                    return Err(anyhow!(
                        "task_board: dependencies not satisfied for id {}",
                        incoming.id
                    ));
                }
            }
            if incoming.retry_count >= 2
                && matches!(
                    incoming.status,
                    ItemStatus::InProgress | ItemStatus::Failed
                )
            {
                *reflection = true;
            }
            maybe_warn_done_without_evidence(
                store_key,
                &prev,
                &incoming,
                recent_action,
                work_items,
                reflection,
                warnings,
            );
            maybe_warn_done_without_verify_pass(
                &prev,
                &incoming,
                recent_verify_report,
                recent_verify_pass,
                reflection,
                warnings,
            );
            maybe_warn_in_progress_without_plan(global_len, &prev, &incoming, warnings);
            compact_row_after_done(&prev, &mut incoming);
            target[idx] = incoming;
            patched.push(row_status_entry(&target[idx]));
        } else {
            let Some(mut incoming) = BoardItem::from_value(v) else {
                continue;
            };
            let empty_prev = BoardItem {
                id: incoming.id.clone(),
                title: incoming.title.clone(),
                status: ItemStatus::Pending,
                ..BoardItem::default()
            };
            let merged = merge_row_patch_with_warnings(&empty_prev, v);
            incoming = merged.row;
            warnings.extend(merged.warnings);
            if matches!(incoming.status, ItemStatus::InProgress | ItemStatus::Done) {
                if !dependencies_satisfied_rows(global_rows, &incoming) {
                    return Err(anyhow!(
                        "task_board: dependencies not satisfied for new id {}",
                        incoming.id
                    ));
                }
            }
            maybe_warn_done_without_evidence(
                store_key,
                &empty_prev,
                &incoming,
                recent_action,
                work_items,
                reflection,
                warnings,
            );
            maybe_warn_done_without_verify_pass(
                &empty_prev,
                &incoming,
                recent_verify_report,
                recent_verify_pass,
                reflection,
                warnings,
            );
            maybe_warn_in_progress_without_plan(global_len, &empty_prev, &incoming, warnings);
            compact_row_after_done(&empty_prev, &mut incoming);
            target.push(incoming);
            patched.push(row_status_entry(target.last().expect("just pushed")));
        }
    }
    Ok(Vec::new())
}

fn maybe_warn_done_without_evidence(
    store_key: &str,
    prev: &BoardItem,
    incoming: &BoardItem,
    recent_action: bool,
    work_items: &WorkItemStore,
    reflection: &mut bool,
    warnings: &mut Vec<Value>,
) {
    if incoming.status != ItemStatus::Done || prev.status == ItemStatus::Done {
        return;
    }
    if incoming.has_validate_evidence()
        || prev.has_validate_evidence()
        || recent_action
        || milestone_done_has_work_item_evidence(store_key, incoming, work_items)
    {
        return;
    }
    *reflection = true;
    let reason = "done_without_evidence: add remark or run action tools before marking done";
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
    let reason = format!(
        "done_without_verify_pass: {} should be pass before marking done",
        crate::agents::computer::tool_names::ACTION_VERIFY
    );
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

fn maybe_warn_in_progress_without_plan(
    global_len: usize,
    prev: &BoardItem,
    incoming: &BoardItem,
    warnings: &mut Vec<Value>,
) {
    if incoming.status != ItemStatus::InProgress || prev.status == ItemStatus::InProgress {
        return;
    }
    if global_len <= 1 {
        return;
    }
    let has_plan = incoming
        .plan
        .as_ref()
        .filter(|s| !s.trim().is_empty())
        .or(prev.plan.as_ref())
        .filter(|s| !s.trim().is_empty())
        .is_some();
    let has_done_when = incoming
        .done_when
        .as_ref()
        .filter(|s| !s.trim().is_empty())
        .or(prev.done_when.as_ref())
        .filter(|s| !s.trim().is_empty())
        .is_some();
    if has_plan || has_done_when {
        return;
    }
    let reason = "in_progress_without_plan: add plan or done_when before or when marking in_progress";
    warnings.push(serde_json::json!({
        "code": "in_progress_without_plan",
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
        for item in doc.global_milestones.iter_mut() {
            if item.status == ItemStatus::Pending {
                item.status = ItemStatus::Cancelled;
                cancelled.push(row_status_entry(item));
            }
        }
    } else {
        for item in doc.global_milestones.iter_mut() {
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
        .global_milestones
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
                !doc.global_milestones.iter().any(|row| {
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
