//! Apply task_board methods to a [`BoardDocument`].

use super::args::{
    constraints_from_args, context_from_args, done_when_from_args, expected_total_from_args,
    global_rows_from_args_typed, goal_from_args, normalize_patch_args, prune_ids_from_args,
    reject_init_removed_fields, reject_patch_foreign_work_item_fields, replace_has_forbidden_scope,
    unified_patch_rows_from_args,
};
use super::coordination::parent_child::{assert_child_may_mutate, parent_store_key_from_child};
use super::loop_milestones::{self, validate_board_row_count};
use super::model::{BoardDocument, BoardItem, BoardScope, GlobalContext, ItemStatus, MetaStatus};
use super::row_patch::{compact_row_after_done, merge_row_patch_with_warnings};
use super::snapshot::{unified_patch_target, UnifiedPatchTarget};
use super::state_machine::{
    count_incomplete, dependencies_satisfied, dependencies_satisfied_rows,
    ensure_single_milestone_in_progress, mark_ready_pending_rows, validate_item_transition,
};
use anyhow::{anyhow, Result};
use serde_json::{json, Value};

const MAX_FINDING_BYTES: usize = 500;
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
) -> Result<ApplyOutcome> {
    assert_child_may_mutate(store_key, doc, method)?;
    let method = method.trim().to_ascii_lowercase();
    let (body, reflection_required) = match method.as_str() {
        "init" => {
            apply_init(store_key, doc, args)?;
            let mut body = json!({
                "ok": true,
                "method": "init",
                "board_len": doc.global_milestones.len(),
            });
            if !doc.meta.goal.is_empty() {
                body["goal"] = json!(doc.meta.goal);
            }
            if let Some(wi) = loop_milestones::loop_progress_json(doc) {
                body["loop_progress"] = wi;
            }
            (body, false)
        }
        "replace" => {
            apply_replace(doc, args)?;
            let mut body = json!({
                "ok": true,
                "method": "replace",
                "board_len": doc.global_milestones.len(),
            });
            if let Some(wi) = loop_milestones::loop_progress_json(doc) {
                body["loop_progress"] = wi;
            }
            (body, false)
        }
        "patch" | "" | "patch_milestones" => apply_patch(store_key, doc, args, &method)?,
        "patch_items" => {
            return Err(anyhow!(
                "task_board: patch_items removed; use task_board_patch with milestones[]"
            ));
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
        "abandon" => {
            apply_abandon(doc)?;
            (
                json!({
                    "ok": true,
                    "method": "abandon",
                    "board_len": doc.global_milestones.len(),
                    "meta_status": doc.meta.status.as_str(),
                }),
                false,
            )
        }
        "check_deps" => {
            let body = apply_check_deps(doc, args)?;
            (body, false)
        }
        other => return Err(anyhow!("task_board: unknown method {other}")),
    };
    mark_ready_pending_rows(doc);
    ensure_single_milestone_in_progress(doc);
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
    if let Some(constraints) = constraints_from_args(args) {
        doc.meta.constraints = Some(constraints);
    }
    if let Some(dw) = done_when_from_args(args) {
        doc.meta.done_when = Some(dw);
    }
    if let Some(expected_total) = expected_total_from_args(args) {
        doc.meta.expected_total = Some(expected_total);
    }
    if let Some(quota) = args.get("dynamic_quota").and_then(|v| v.as_u64()) {
        if quota > 0 {
            doc.meta.dynamic_quota = Some(quota as u32);
        }
    }
}

fn apply_init(store_key: &str, doc: &mut BoardDocument, args: &Value) -> Result<()> {
    reject_init_removed_fields(args)?;
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
    let global_rows = global_rows_from_args_typed(args)?;
    if !global_rows.is_empty() {
        doc.global_milestones.clear();
        let mut dropped = 0usize;
        for v in &global_rows {
            if let Some(item) = BoardItem::from_value(v) {
                doc.global_milestones.push(item);
            } else {
                dropped += 1;
            }
        }
        if dropped > 0 {
            return Err(anyhow!(
                "task_board: init dropped {dropped}/{} row(s) missing id; each global_milestones[] object needs id, title, status",
                global_rows.len()
            ));
        }
        validate_board_row_count(doc.global_milestones.len(), "init")?;
    }
    loop_milestones::expand_loop_milestones_on_init(doc, args)?;
    validate_board_row_count(doc.global_milestones.len(), "init")?;
    if doc.global_milestones.is_empty() {
        return Err(anyhow!(
            "task_board: init requires global_milestones[] as a JSON array with at least one row (id, title, status)"
        ));
    }
    if loop_milestones::is_loop_milestone_board(doc) {
        loop_milestones::validate_loop_milestone_init(doc, args)?;
        loop_milestones::bootstrap_loop_milestone_board(doc);
    }
    validate_expected_total_on_init(doc, "init")?;
    Ok(())
}

fn validate_expected_total_on_init(doc: &BoardDocument, method: &str) -> Result<()> {
    let Some(expected_total) = doc.meta.expected_total else {
        return Ok(());
    };
    if loop_milestones::is_loop_milestone_board(doc) {
        return Ok(());
    }
    let rows = doc.global_milestones.len();
    if rows as u32 != expected_total {
        return Err(anyhow!(
            "task_board:{method} expected exactly {expected_total} item(s), got {rows}"
        ));
    }
    Ok(())
}

fn apply_replace(doc: &mut BoardDocument, args: &Value) -> Result<()> {
    if replace_has_forbidden_scope(args) {
        return Err(anyhow!("replace_scope_forbidden"));
    }
    if args.get("item_milestones").is_some() {
        return Err(anyhow!(
            "task_board: replace uses global_milestones[] only (not item_milestones)"
        ));
    }
    let rows = global_rows_from_args_typed(args)?;
    if rows.is_empty() {
        return Err(anyhow!(
            "task_board: replace requires global_milestones[] as a JSON array"
        ));
    }
    doc.global_milestones.clear();
    for v in &rows {
        if let Some(item) = BoardItem::from_value(v) {
            doc.global_milestones.push(item);
        }
    }
    validate_board_row_count(doc.global_milestones.len(), "replace")?;
    Ok(())
}

fn apply_patch(
    _store_key: &str,
    doc: &mut BoardDocument,
    args: &Value,
    method: &str,
) -> Result<(Value, bool)> {
    if doc.board_is_empty() {
        return Ok((
            json!({
                "ok": true,
                "method": method,
                "board_len": 0,
                "patched": [],
                "skipped": true,
                "reason": "board_not_initialized",
            }),
            false,
        ));
    }
    let patch_args = normalize_patch_args(args.clone());
    reject_patch_foreign_work_item_fields(&patch_args)?;
    if args.get("work_item_delta").is_some() {
        return Err(anyhow!(
            "task_board: work_item_delta removed; patch milestones (done/failed + remark)"
        ));
    }

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

    let patch_rows = unified_patch_rows_from_args(&patch_args)?;
    let has_milestone_patch = patch_rows.as_ref().is_some_and(|r| !r.is_empty());
    if !has_milestone_patch && patch_args.get("global_context").is_none() {
        return Err(anyhow!("task_board: patch requires milestones[]"));
    }

    let mut reflection = false;
    let mut warnings: Vec<Value> = Vec::new();
    let mut patched: Vec<Value> = Vec::new();
    if let Some(gc) = patch_args.get("global_context") {
        merge_global_context(&mut doc.global_context, gc);
    }

    let global_snapshot = doc.global_milestones.clone();
    let global_len = doc.global_milestones.len();

    let transitions = if let Some(rows) = patch_rows.filter(|r| !r.is_empty()) {
        let row_id = rows
            .first()
            .and_then(|v| v.get("id"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow!("task_board: patch requires one milestone row with id"))?;
        let target = unified_patch_target(doc, row_id)?;
        if let Some(status_s) = rows
            .first()
            .and_then(|v| v.get("status"))
            .and_then(|v| v.as_str())
        {
            if let Some(status) = ItemStatus::from_str_loose(status_s) {
                loop_milestones::patch_rejects_g_deliver_when_loop_blocked(doc, row_id, status)?;
            }
        }
        match target {
            UnifiedPatchTarget::GlobalMilestones => patch_rows_on_slice(
                &mut doc.global_milestones,
                &rows,
                &global_snapshot,
                global_len,
                recent_action,
                recent_verify_report,
                recent_verify_pass,
                &mut reflection,
                &mut warnings,
                &mut patched,
            )?,
        }
    } else {
        Vec::new()
    };

    if loop_milestones::is_loop_milestone_board(doc) {
        for (prev, incoming) in &transitions {
            loop_milestones::handle_loop_milestone_transition(doc, prev, incoming);
        }
        loop_milestones::sync_loop_meta(doc);
    }

    enforce_interim_drafts_budget(doc, &mut reflection, &mut warnings);

    let mut body = json!({
        "ok": true,
        "method": method,
        "board_len": doc.global_milestones.len(),
        "patched": patched,
        "reflection_required": reflection,
    });
    if !warnings.is_empty() {
        body["warnings"] = json!(warnings);
    }
    if let Some(wi) = loop_milestones::loop_progress_json(doc) {
        body["loop_progress"] = wi;
    }
    Ok((body, reflection))
}

fn patch_rows_on_slice(
    target: &mut Vec<BoardItem>,
    rows: &[Value],
    global_rows: &[BoardItem],
    global_len: usize,
    recent_action: bool,
    recent_verify_report: bool,
    recent_verify_pass: bool,
    reflection: &mut bool,
    warnings: &mut Vec<Value>,
    patched: &mut Vec<Value>,
) -> Result<Vec<(BoardItem, BoardItem)>> {
    let mut transitions = Vec::new();
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
        if let Some(idx) = target.iter().position(|e| e.id == id) {
            let prev = target[idx].clone();
            let merged = merge_row_patch_with_warnings(&prev, v);
            let mut incoming = merged.row;
            warnings.extend(merged.warnings);
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
                && matches!(incoming.status, ItemStatus::InProgress | ItemStatus::Failed)
            {
                *reflection = true;
            }
            maybe_warn_done_without_evidence(&prev, &incoming, recent_action, reflection, warnings);
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
            target[idx] = incoming.clone();
            patched.push(row_status_entry(&target[idx]));
            transitions.push((prev, incoming));
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
                &empty_prev,
                &incoming,
                recent_action,
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
            target.push(incoming.clone());
            patched.push(row_status_entry(target.last().expect("just pushed")));
            transitions.push((empty_prev, incoming));
        }
    }
    Ok(transitions)
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
    if incoming.has_validate_evidence() || prev.has_validate_evidence() || recent_action {
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
    let reason = "done_without_verify_pass: host verify should be pass before marking done";
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
    let reason =
        "in_progress_without_plan: add plan or done_when before or when marking in_progress";
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
        *v = Value::String(format!(
            "{compact}\n\n[trimmed_by_engine_for_context_budget]"
        ));
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

fn apply_abandon(doc: &mut BoardDocument) -> Result<()> {
    if doc.board_is_empty() && doc.meta.goal.trim().is_empty() {
        return Err(anyhow!("task_board: nothing to abandon"));
    }
    if matches!(doc.meta.status, MetaStatus::Completed | MetaStatus::Failed) {
        return Ok(());
    }
    doc.meta.status = MetaStatus::Failed;
    Ok(())
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
                        && matches!(row.status, ItemStatus::Done | ItemStatus::Cancelled)
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

/// Append one `key_findings` entry: deduped, oldest dropped at capacity.
///
/// Findings are free-form model text, so the byte budget is applied through
/// the UTF-8-safe helper — a raw byte slice would split a multibyte character.
fn push_key_finding(gc: &mut GlobalContext, finding: &str) {
    let entry = finding.trim();
    if entry.is_empty() {
        return;
    }
    let entry = crate::text_util::truncate_bytes(entry, MAX_FINDING_BYTES);
    if gc.key_findings.iter().any(|f| f == &entry) {
        return;
    }
    if gc.key_findings.len() >= MAX_FINDINGS {
        gc.key_findings.remove(0);
    }
    gc.key_findings.push(entry);
}

fn merge_global_context(gc: &mut GlobalContext, patch: &Value) {
    if let Some(arr) = patch.get("key_findings").and_then(|v| v.as_array()) {
        for e in arr {
            if let Some(s) = e.as_str() {
                push_key_finding(gc, s);
            }
        }
    }
    if let Some(artifacts) = patch.get("artifacts") {
        if artifacts.is_object() {
            if gc.artifacts.is_object() {
                if let (Some(dst), Some(src)) =
                    (gc.artifacts.as_object_mut(), artifacts.as_object())
                {
                    for (k, v) in src {
                        dst.insert(k.clone(), v.clone());
                    }
                }
            } else {
                gc.artifacts = artifacts.clone();
            }
        }
    }
}
