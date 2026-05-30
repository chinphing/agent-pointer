#[cfg(test)]
mod apply_tests {
    use crate::task_board::args::items_array_from_args;
    use crate::task_board::migrate::normalize_stored_value;
    use crate::task_board::model::ItemStatus;
    use crate::task_board::store::TaskBoardStore;
    use crate::task_board::{check_dependencies, report_child_status, DependencyCheck};
    use serde_json::json;

    #[test]
    fn string_items_patch_applies() {
        let store = TaskBoardStore::new();
        let key = "conv-test";
        let args = json!({
            "items": "[{\"id\":\"a\",\"title\":\"Step A\",\"status\":\"pending\",\"verification\":\"ok\"}]"
        });
        store.apply(key, "patch", &args).expect("patch");
        let doc = store.document(key);
        assert_eq!(doc.board.len(), 1);
        assert_eq!(doc.board[0].id, "a");
    }

    #[test]
    fn v1_array_migrates() {
        let raw = json!([
            {"id": "1", "title": "t", "status": "done"}
        ]);
        let doc = normalize_stored_value("k", raw);
        assert_eq!(doc.version, 2);
        assert_eq!(doc.board.len(), 1);
    }

    #[test]
    fn dependency_gate_blocks() {
        let store = TaskBoardStore::new();
        let key = "parent";
        store
            .apply(
                key,
                "init",
                &json!({
                    "goal": "g",
                    "items": [
                        {"id": "sub_01", "title": "first", "status": "pending"},
                        {"id": "sub_02", "title": "second", "status": "pending", "depends_on": ["sub_01"]}
                    ]
                }),
            )
            .expect("init");
        let doc = store.document(key);
        assert!(matches!(
            check_dependencies(&doc, "sub_02"),
            DependencyCheck::Blocked { .. }
        ));
        let mut doc = store.document(key);
        report_child_status(&mut doc, "sub_01", ItemStatus::Done, "ok").expect("report");
        store.save_document(key, doc);
        let doc = store.document(key);
        assert!(matches!(
            check_dependencies(&doc, "sub_02"),
            DependencyCheck::Ready
        ));
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
                    "verification": "微信应用已打开"
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
                    "verification": "老婆聊天窗口已打开，输入框可见"
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
        assert_eq!(doc.board[0].status, ItemStatus::Done);
        assert_eq!(doc.board[1].status, ItemStatus::Done);
        assert_eq!(doc.board[2].status, ItemStatus::InProgress);
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
                "patch",
                &json!({
                    "items": [{"id": "s1", "title": "Step", "status": "in_progress"}]
                }),
            )
            .expect("patch");
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
                &json!({"item_id": "1", "status": "done", "verification": "ok"}),
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
        assert!(err.to_string().contains("expected exactly 21 item(s), got 8"));
    }

    #[test]
    fn init_accepts_full_rows_for_explicit_total_goal() {
        let store = TaskBoardStore::new();
        let key = "conv-explicit-total-ok";
        let items: Vec<_> = (1..=21)
            .map(|i| json!({"id": format!("c{i}"), "title": format!("组合{i}"), "status": "pending"}))
            .collect();
        store
            .apply(
                key,
                "init",
                &json!({
                    "goal": "测试7种验证码类型×3种交互形式共21种组合",
                    "expected_total": 21,
                    "items": items
                }),
            )
            .expect("init");
        let doc = store.document(key);
        assert_eq!(doc.board.len(), 21);
        assert_eq!(doc.meta.expected_total, Some(21));
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
        assert_eq!(doc.board.len(), 1);
        assert_eq!(doc.board[0].id, "m1");
    }
}
