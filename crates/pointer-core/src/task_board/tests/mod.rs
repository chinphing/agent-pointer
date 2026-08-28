#[cfg(test)]
mod apply_tests {
    use crate::task_board::args::items_array_from_args;
    use crate::task_board::migrate::normalize_stored_value;
    use crate::task_board::model::{ItemStatus, MetaStatus};
    use crate::task_board::store::TaskBoardStore;
    use serde_json::json;

    #[test]
    fn type1_init_promotes_first_ready_to_in_progress() {
        let store = TaskBoardStore::new();
        let key = "conv-type1-bootstrap";
        store
            .apply(
                key,
                "init",
                &json!({
                    "goal": "g",
                    "items": [
                        {"id": "m1", "title": "One", "status": "pending"},
                        {"id": "m2", "title": "Two", "status": "pending"}
                    ]
                }),
            )
            .expect("init");
        let doc = store.document(key);
        assert_eq!(doc.global_milestones[0].status, ItemStatus::InProgress);
        assert_eq!(doc.global_milestones[1].status, ItemStatus::Ready);
    }

    #[test]
    fn abandon_marks_board_failed_and_cancels_open_rows() {
        let store = TaskBoardStore::new();
        let key = "conv-abandon";
        store
            .apply(
                key,
                "init",
                &json!({
                    "goal": "batch",
                    "items": [
                        {"id": "m1", "title": "One", "status": "done"},
                        {"id": "m2", "title": "Two", "status": "in_progress"},
                        {"id": "m3", "title": "Three", "status": "pending"}
                    ]
                }),
            )
            .expect("init");
        store.apply(key, "abandon", &json!({})).expect("abandon");
        let doc = store.document(key);
        assert_eq!(doc.meta.status, MetaStatus::Failed);
        assert_eq!(doc.global_milestones[0].status, ItemStatus::Done);
        assert_eq!(doc.global_milestones[1].status, ItemStatus::Cancelled);
        assert_eq!(doc.global_milestones[2].status, ItemStatus::Cancelled);
    }

    #[test]
    fn type1_done_patch_advances_next_ready_to_in_progress() {
        let store = TaskBoardStore::new();
        let key = "conv-type1-advance";
        store
            .apply(
                key,
                "init",
                &json!({
                    "goal": "g",
                    "items": [
                        {"id": "m1", "title": "One", "status": "in_progress"},
                        {"id": "m2", "title": "Two", "status": "pending"},
                        {"id": "m3", "title": "Three", "status": "pending"}
                    ]
                }),
            )
            .expect("init");
        store
            .apply(
                key,
                "patch",
                &json!({
                    "milestones": [{
                        "id": "m1",
                        "status": "done",
                        "remark": "finished step one"
                    }]
                }),
            )
            .expect("patch done");
        let doc = store.document(key);
        assert_eq!(doc.global_milestones[0].status, ItemStatus::Done);
        assert_eq!(doc.global_milestones[1].status, ItemStatus::InProgress);
        assert_eq!(doc.global_milestones[2].status, ItemStatus::Ready);
    }

    #[test]
    fn string_items_patch_applies() {
        let store = TaskBoardStore::new();
        let key = "conv-test";
        store
            .apply(
                key,
                "init",
                &json!({
                    "goal": "g",
                    "items": [{"id": "a", "title": "seed", "status": "pending"}]
                }),
            )
            .expect("init");
        let args = json!({
            "items": "[{\"id\":\"a\",\"title\":\"Step A\",\"status\":\"pending\",\"validate_requirement\":\"ok\"}]"
        });
        store.apply(key, "patch", &args).expect("patch");
        let doc = store.document(key);
        assert_eq!(doc.global_milestones.len(), 1);
        assert_eq!(doc.global_milestones[0].id, "a");
        assert_eq!(doc.global_milestones[0].title, "Step A");
    }

    #[test]
    fn init_accepts_milestones_native_array_alias() {
        let store = TaskBoardStore::new();
        let key = "conv-init-milestones-array";
        store
            .apply(
                key,
                "init",
                &json!({
                    "goal": "P8",
                    "milestones": [
                        {"id": "p8-1", "title": "摸底", "status": "pending"},
                        {"id": "p8-2", "title": "方案", "status": "pending"}
                    ]
                }),
            )
            .expect("init");
        let doc = store.document(key);
        assert_eq!(doc.global_milestones.len(), 2);
        assert_eq!(doc.global_milestones[0].id, "p8-1");
    }

    #[test]
    fn init_rejects_stringified_milestones() {
        let store = TaskBoardStore::new();
        let err = store
            .apply(
                "conv-init-milestones-string",
                "init",
                &json!({
                    "goal": "P8",
                    "milestones": "[{\"id\":\"p8-1\",\"title\":\"摸底\",\"status\":\"pending\"}]"
                }),
            )
            .expect_err("quoted array must fail");
        let msg = err.to_string();
        assert!(msg.contains("JSON array"), "{msg}");
        assert!(msg.contains("string"), "{msg}");
    }

    #[test]
    fn init_rejects_goal_only_empty_board() {
        let store = TaskBoardStore::new();
        let err = store
            .apply("conv-empty-init", "init", &json!({ "goal": "P7" }))
            .expect_err("goal-only init must fail");
        assert!(err.to_string().contains("global_milestones"), "{}", err);
    }

    #[test]
    fn init_rejects_empty_global_milestones_array() {
        let store = TaskBoardStore::new();
        let err = store
            .apply(
                "conv-empty-array",
                "init",
                &json!({ "goal": "P7", "global_milestones": [] }),
            )
            .expect_err("empty array must fail");
        assert!(err.to_string().contains("global_milestones"), "{}", err);
    }

    #[test]
    fn init_rejects_rows_missing_id() {
        let store = TaskBoardStore::new();
        let err = store
            .apply(
                "conv-missing-id",
                "init",
                &json!({
                    "goal": "P7",
                    "global_milestones": [{"title": "摸底", "status": "pending"}]
                }),
            )
            .expect_err("missing id must fail");
        assert!(err.to_string().contains("missing id"), "{}", err);
    }

    #[test]
    fn patch_rejects_stringified_milestones() {
        let store = TaskBoardStore::new();
        let key = "conv-patch-string";
        store
            .apply(
                key,
                "init",
                &json!({
                    "goal": "g",
                    "global_milestones": [
                        {"id": "m1", "title": "One", "status": "pending"}
                    ]
                }),
            )
            .expect("init");
        let err = store
            .apply(
                key,
                "patch",
                &json!({
                    "milestones": "[{\"id\":\"m1\",\"status\":\"done\",\"remark\":\"ok\"}]"
                }),
            )
            .expect_err("quoted patch array must fail");
        let msg = err.to_string();
        assert!(msg.contains("JSON array"), "{msg}");
        assert!(msg.contains("string"), "{msg}");
    }

    #[test]
    fn legacy_array_migrates_to_global_milestones() {
        let raw = json!([
            {"id": "1", "title": "t", "status": "done"}
        ]);
        let doc = normalize_stored_value("k", raw);
        assert_eq!(doc.version, 4);
        assert_eq!(doc.global_milestones.len(), 1);
    }

    #[test]
    fn flat_item_id_patch_applies() {
        let store = TaskBoardStore::new();
        let key = "conv-flat";
        store
            .apply(
                key,
                "init",
                &json!({
                    "goal": "g",
                    "items": [
                        {"id": "1", "title": "Step 1", "status": "in_progress"},
                        {"id": "2", "title": "Step 2", "status": "pending"},
                        {"id": "3", "title": "Step 3", "status": "pending"}
                    ]
                }),
            )
            .expect("init");
        store
            .apply(
                key,
                "patch",
                &json!({
                    "item_id": "1",
                    "status": "done",
                    "validate_results": "微信应用已打开"
                }),
            )
            .expect("patch 1");
        store
            .apply(
                key,
                "patch",
                &json!({
                    "item_id": "2",
                    "status": "done",
                    "validate_results": "老婆聊天窗口已打开，输入框可见"
                }),
            )
            .expect("patch 2");
        store
            .apply(
                key,
                "patch",
                &json!({"item_id": "3", "status": "in_progress"}),
            )
            .expect("patch 3");
        let doc = store.document(key);
        assert_eq!(doc.global_milestones[0].status, ItemStatus::Done);
        assert_eq!(doc.global_milestones[1].status, ItemStatus::Done);
        assert_eq!(doc.global_milestones[2].status, ItemStatus::InProgress);
    }

    #[test]
    fn items_array_from_string() {
        let args = json!({"items": "[{\"id\":\"x\"}]"});
        assert_eq!(items_array_from_args(&args).map(|a| a.len()), Some(1));
    }

    #[test]
    fn done_without_evidence_sets_reflection() {
        let store = TaskBoardStore::new();
        let key = "conv-done";
        store
            .apply(
                key,
                "init",
                &json!({
                    "goal": "g",
                    "items": [{"id": "s1", "title": "Step", "status": "in_progress"}]
                }),
            )
            .expect("init");
        let (body, reflection) = store
            .apply(
                key,
                "patch",
                &json!({
                    "_recent_action_tools": false,
                    "items": [{"id": "s1", "status": "done"}]
                }),
            )
            .expect("done patch");
        assert!(reflection);
        assert!(body["warnings"].as_array().is_some());
    }

    #[test]
    fn patch_without_init_is_noop() {
        let store = TaskBoardStore::new();
        let key = "conv-no-init";
        let (body, reflection) = store
            .apply(
                key,
                "patch",
                &json!({
                    "items": [{"id": "s1", "title": "Step", "status": "in_progress"}]
                }),
            )
            .expect("patch noop");
        assert!(!reflection);
        assert_eq!(body["ok"], true);
        assert_eq!(body["method"], "patch");
        assert_eq!(body["skipped"], true);
        assert_eq!(body["reason"], "board_not_initialized");
        assert_eq!(body["patched"].as_array().map(|a| a.len()), Some(0));
        let doc = store.document(key);
        assert!(doc.global_milestones.is_empty());
        assert!(doc.meta.goal.is_empty());
    }

    #[test]
    fn patch_returns_compact_body_without_document() {
        let store = TaskBoardStore::new();
        let key = "conv-compact";
        store
            .apply(
                key,
                "init",
                &json!({
                    "goal": "g",
                    "items": [
                        {"id": "1", "title": "A", "status": "pending"},
                        {"id": "2", "title": "B", "status": "pending"}
                    ]
                }),
            )
            .expect("init");
        let (body, reflection) = store
            .apply(
                key,
                "patch",
                &json!({"item_id": "1", "status": "done", "remark": "ok"}),
            )
            .expect("patch");
        assert!(!reflection);
        assert_eq!(body["ok"], true);
        assert_eq!(body["method"], "patch");
        assert_eq!(body["board_len"], 2);
        assert!(body.get("document").is_none());
        let patched = body["patched"].as_array().expect("patched array");
        assert_eq!(patched.len(), 1);
        assert_eq!(patched[0]["id"], "1");
        assert_eq!(patched[0]["status"], "done");
    }

    #[test]
    fn patch_preserves_done_when_when_omitted() {
        let store = TaskBoardStore::new();
        let key = "conv-preserve";
        store
            .apply(
                key,
                "init",
                &json!({
                    "goal": "g",
                    "items": [
                        {
                            "id": "1",
                            "title": "A",
                            "status": "in_progress",
                            "plan": "detail text",
                            "done_when": "run unit tests"
                        }
                    ]
                }),
            )
            .expect("init");
        store
            .apply(
                key,
                "patch",
                &json!({
                    "item_id": "1",
                    "status": "done",
                    "remark": "tests passed"
                }),
            )
            .expect("patch");
        let doc = store.document(key);
        assert_eq!(doc.global_milestones[0].plan, None);
        assert_eq!(
            doc.global_milestones[0].done_when.as_deref(),
            Some("run unit tests")
        );
        assert_eq!(
            doc.global_milestones[0].remark.as_deref(),
            Some("tests passed")
        );
    }

    #[test]
    fn v3_validate_delta_emits_rejected_warning() {
        let store = TaskBoardStore::new();
        let key = "conv-append";
        store
            .apply(
                key,
                "init",
                &json!({
                    "goal": "g",
                    "items": [{"id": "1", "title": "A", "status": "in_progress"}]
                }),
            )
            .expect("init");
        let (body, _) = store
            .apply(
                key,
                "patch",
                &json!({"items": [{"id": "1", "status": "in_progress", "validate_result_delta": "first"}]}),
            )
            .expect("p1");
        assert!(body["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| { w.get("code").and_then(|c| c.as_str()) == Some("v3_field_rejected") }));
    }

    #[test]
    fn patch_without_status_emits_warning() {
        let store = TaskBoardStore::new();
        let key = "conv-status-warn";
        store
            .apply(
                key,
                "init",
                &json!({
                    "goal": "g",
                    "items": [{"id": "1", "title": "A", "status": "in_progress"}]
                }),
            )
            .expect("init");
        let (body, _) = store
            .apply(key, "patch", &json!({"item_id": "1", "remark": "step ok"}))
            .expect("patch");
        assert!(body["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| { w.get("code").and_then(|c| c.as_str()) == Some("patch_status_required") }));
    }

    #[test]
    fn v3_progress_field_rejected_on_patch() {
        let store = TaskBoardStore::new();
        let key = "conv-progress";
        store
            .apply(
                key,
                "init",
                &json!({
                    "goal": "g",
                    "items": [{
                        "id": "1",
                        "title": "A",
                        "status": "in_progress"
                    }]
                }),
            )
            .expect("init");
        let (body, _) = store
            .apply(
                key,
                "patch",
                &json!({"item_id": "1", "status": "in_progress", "progress": "7/10"}),
            )
            .expect("patch");
        assert!(body["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| { w.get("code").and_then(|c| c.as_str()) == Some("v3_field_rejected") }));
    }

    #[test]
    fn v3_document_roundtrip() {
        let raw = json!({
            "version": 3,
            "task_id": "tb_k",
            "meta": {"goal": "g"},
            "board": [{
                "id": "1",
                "title": "T",
                "status": "done",
                "plan": "plan here",
                "validate_results": ["evidence"]
            }]
        });
        let doc = normalize_stored_value("k", raw);
        assert_eq!(doc.global_milestones.len(), 1);
        assert_eq!(doc.global_milestones[0].plan.as_deref(), Some("plan here"));
        assert_eq!(doc.global_milestones[0].remark.as_deref(), Some("evidence"));
    }

    #[test]
    fn init_rejects_incomplete_rows_for_explicit_total_goal() {
        let store = TaskBoardStore::new();
        let key = "conv-explicit-total";
        let err = store
            .apply(
                key,
                "init",
                &json!({
                    "goal": "测试7种验证码类型×3种交互形式共21种组合",
                    "expected_total": 21,
                    "items": [
                        {"id": "c1", "title": "组合1", "status": "pending"},
                        {"id": "c2", "title": "组合2", "status": "pending"},
                        {"id": "c3", "title": "组合3", "status": "pending"},
                        {"id": "c4", "title": "组合4", "status": "pending"},
                        {"id": "c5", "title": "组合5", "status": "pending"},
                        {"id": "c6", "title": "组合6", "status": "pending"},
                        {"id": "c7", "title": "组合7", "status": "pending"},
                        {"id": "c8", "title": "组合8", "status": "pending"}
                    ]
                }),
            )
            .expect_err("should reject incomplete explicit total");
        assert!(err
            .to_string()
            .contains("expected exactly 21 item(s), got 8"));
    }

    #[test]
    fn init_accepts_full_rows_for_explicit_total_goal() {
        let store = TaskBoardStore::new();
        let key = "conv-explicit-total-ok";
        let items: Vec<_> = (1..=20)
            .map(|i| json!({"id": format!("c{i}"), "title": format!("组合{i}"), "status": "pending"}))
            .collect();
        store
            .apply(
                key,
                "init",
                &json!({
                    "goal": "Matrix milestone coverage",
                    "expected_total": 20,
                    "items": items
                }),
            )
            .expect("init");
        let doc = store.document(key);
        assert_eq!(doc.global_milestones.len(), 20);
        assert_eq!(doc.meta.expected_total, Some(20));
    }

    #[test]
    fn long_cjk_key_finding_is_truncated_without_panic() {
        let store = TaskBoardStore::new();
        let key = "conv-cjk-finding";
        let finding = "关键发现".repeat(300);
        store
            .apply(
                key,
                "init",
                &json!({
                    "goal": "g",
                    "global_context": { "key_findings": [finding] },
                    "items": [{"id": "seed", "title": "seed", "status": "pending"}]
                }),
            )
            .expect("init");
        let doc = store.document(key);
        assert_eq!(doc.global_context.key_findings.len(), 1);
        let stored = &doc.global_context.key_findings[0];
        assert!(stored.ends_with('…'));
        assert!(stored.len() < finding.len());
    }

    #[test]
    fn sync_finding_method_is_removed() {
        let store = TaskBoardStore::new();
        let child = crate::task_board::sub_agent_task_board_store_key("conv-parent", "task_a");
        let err = store
            .apply(&child, "sync_finding", &json!({ "finding": "突破" }))
            .expect_err("sync_finding is no longer a task_board method");
        assert!(err.to_string().contains("unknown method sync_finding"));
    }
}

#[cfg(test)]
mod sqlite_tests {
    use crate::task_board::persistence::TaskBoardSqlite;
    use crate::task_board::TaskBoardStore;
    use serde_json::json;
    #[test]
    fn sqlite_roundtrip_survives_new_store() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("task_boards.db");
        let db = TaskBoardSqlite::open(path).expect("open");
        let store = TaskBoardStore::with_persistence(db.clone());
        store
            .apply(
                "conv-persist",
                "init",
                &json!({
                    "goal": "g",
                    "items": [{"id": "m1", "title": "Milestone", "status": "pending"}]
                }),
            )
            .expect("init");
        store
            .apply(
                "conv-persist",
                "patch",
                &json!({
                    "items": [{"id": "m1", "title": "Milestone", "status": "in_progress"}]
                }),
            )
            .expect("patch");
        drop(store);
        let store2 = TaskBoardStore::with_persistence(db);
        store2.ensure_loaded("conv-persist");
        let doc = store2.document("conv-persist");
        assert_eq!(doc.global_milestones.len(), 1);
        assert_eq!(doc.global_milestones[0].id, "m1");
    }
}
