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
        store
            .apply(
                key,
                "init",
                &json!({
                    "goal": "g",
                    "items": []
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
        assert_eq!(doc.global_milestones[0].remark.as_deref(), Some("tests passed"));
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
        assert!(body["warnings"].as_array().unwrap().iter().any(|w| {
            w.get("code").and_then(|c| c.as_str()) == Some("v3_field_rejected")
        }));
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
            .apply(
                key,
                "patch",
                &json!({"item_id": "1", "remark": "step ok"}),
            )
            .expect("patch");
        assert!(body["warnings"].as_array().unwrap().iter().any(|w| {
            w.get("code").and_then(|c| c.as_str()) == Some("patch_status_required")
        }));
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
        assert!(body["warnings"].as_array().unwrap().iter().any(|w| {
            w.get("code").and_then(|c| c.as_str()) == Some("v3_field_rejected")
        }));
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
        assert!(err.to_string().contains("expected exactly 21 item(s), got 8"));
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
                    "items": []
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

#[cfg(test)]
mod work_items_tests {
    use crate::task_board::store::TaskBoardStore;
    use serde_json::json;

    fn wi_args(extra: serde_json::Value) -> serde_json::Value {
        let mut base = json!({
            "_task_board_work_items_enabled": true,
            "_task_board_b42_enforced": true,
        });
        if let Some(obj) = extra.as_object() {
            for (k, v) in obj {
                base[k] = v.clone();
            }
        }
        base
    }

    #[test]
    fn init_seeds_work_items_and_milestone_patch_closes_row() {
        let store = TaskBoardStore::new();
        let key = "conv-wi-p1";
        store
            .apply(
                key,
                "init",
                &wi_args(json!({
                    "goal": "Open apps",
                    "work_item_mode": "enumerated",
                    "expected_total": 3,
                    "global_milestones": [
                        {"id": "g_plan", "title": "Plan", "status": "pending"},
                        {"id": "g_exec", "title": "Exec", "status": "pending"},
                        {"id": "g_deliver", "title": "Deliver", "status": "pending"}
                    ],
                    "item_milestones": [
                        {"id": "m1", "title": "Open app", "status": "pending", "done_when": "app opened"}
                    ],
                    "work_items": [
                        {"title": "App1"},
                        {"title": "App2"},
                        {"title": "App3"}
                    ]
                })),
            )
            .expect("init");
        assert_eq!(store.work_items.count_campaign(key), 3);
        assert_eq!(store.work_items.store_stats(key).in_progress, 1);

        store
            .apply(
                key,
                "patch",
                &wi_args(json!({
                    "work_item_id": "1",
                    "milestones": [{
                        "id": "m1",
                        "status": "done",
                        "remark": "opened"
                    }]
                })),
            )
            .expect("patch");
        assert_eq!(store.work_items.store_stats(key).done, 1);
        assert_eq!(store.work_items.store_stats(key).in_progress, 1);
    }

    #[test]
    fn patch_rejects_work_item_delta() {
        let store = TaskBoardStore::new();
        let key = "conv-no-delta";
        store
            .apply(
                key,
                "init",
                &wi_args(json!({
                    "work_item_mode": "enumerated",
                    "global_milestones": [{"id": "g_exec", "title": "Exec", "status": "in_progress"}],
                    "item_milestones": [{"id": "m1", "title": "Step", "status": "in_progress"}],
                    "work_items": [{"title": "A"}]
                })),
            )
            .expect("init");
        let err = store
            .apply(
                key,
                "patch",
                &wi_args(json!({
                    "work_item_delta": {"id": "1", "status": "done", "result_summary": "x"}
                })),
            )
            .unwrap_err();
        assert!(err.to_string().contains("work_item_delta removed"));
    }

    #[test]
    fn v3_field_rejected_on_work_item_board_patch() {
        let store = TaskBoardStore::new();
        let key = "conv-b42";
        store
            .apply(
                key,
                "init",
                &wi_args(json!({
                    "work_item_mode": "enumerated",
                    "global_milestones": [{
                        "id": "g_exec",
                        "title": "Batch",
                        "status": "in_progress"
                    }],
                    "work_items": [{"title": "A"}]
                })),
            )
            .expect("init");
        let (body, _) = store
            .apply(
                key,
                "patch",
                &wi_args(json!({
                    "work_item_id": "1",
                    "global_milestones": [{
                        "id": "g_exec",
                        "status": "in_progress",
                        "validate_result_delta": "bad"
                    }]
                })),
            )
            .expect("patch");
        assert!(body["warnings"].as_array().unwrap().iter().any(|w| {
            w.get("code").and_then(|c| c.as_str()) == Some("v3_field_rejected")
        }));
    }

    #[test]
    fn inject_window_stable_for_large_seed() {
        let store = TaskBoardStore::new();
        let key = "conv-big-inject";
        let items: Vec<serde_json::Value> = (1..=50)
            .map(|i| json!({"title": format!("item {i}")}))
            .collect();
        store
            .apply(
                key,
                "init",
                &wi_args(json!({
                    "work_item_mode": "enumerated",
                    "expected_total": 50,
                    "global_milestones": [{
                        "id": "g_exec",
                        "title": "Batch",
                        "status": "in_progress"
                    }],
                    "work_items": items
                })),
            )
            .expect("init");
        let block = crate::task_board::snapshot::markdown_runtime_block_for_inject(
            &store.document(key),
            key,
            Some(store.work_items.as_ref()),
        );
        assert!(block.contains("## Work items"));
        assert!(block.len() < 8000);
    }

    fn wi_init_minimal(store: &TaskBoardStore, key: &str) {
        store
            .apply(
                key,
                "init",
                &wi_args(json!({
                    "goal": "Batch",
                    "work_item_mode": "enumerated",
                    "expected_total": 2,
                    "global_milestones": [
                        {"id": "g_plan", "title": "Plan", "status": "pending"},
                        {"id": "g_exec", "title": "Exec", "status": "pending"},
                        {"id": "g_deliver", "title": "Deliver", "status": "pending"}
                    ],
                    "item_milestones": [
                        {"id": "m1", "title": "Step", "status": "pending", "done_when": "done"}
                    ],
                    "work_items": [{"title": "A"}, {"title": "B"}]
                })),
            )
            .expect("init");
    }

    #[test]
    fn patch_requires_work_item_id_when_in_progress() {
        let store = TaskBoardStore::new();
        let key = "conv-wi-id-req";
        wi_init_minimal(&store, key);
        let err = store
            .apply(
                key,
                "patch",
                &wi_args(json!({
                    "milestones": [{"id": "m1", "status": "done", "remark": "ok"}]
                })),
            )
            .unwrap_err();
        assert!(err.to_string().contains("work_item_id"));
    }

    #[test]
    fn patch_rejects_stale_work_item_id() {
        let store = TaskBoardStore::new();
        let key = "conv-wi-stale";
        wi_init_minimal(&store, key);
        store
            .apply(
                key,
                "patch",
                &wi_args(json!({
                    "work_item_id": "1",
                    "milestones": [{"id": "m1", "status": "done", "remark": "first"}]
                })),
            )
            .expect("close first");
        assert_eq!(store.work_items.store_stats(key).in_progress, 1);
        let err = store
            .apply(
                key,
                "patch",
                &wi_args(json!({
                    "work_item_id": "1",
                    "milestones": [{"id": "m1", "status": "done", "remark": "stale"}]
                })),
            )
            .unwrap_err();
        assert!(err.to_string().contains("does not match in_progress"));
    }

    #[test]
    fn deliver_patch_ok_without_work_item_id_when_queue_finished() {
        let store = TaskBoardStore::new();
        let key = "conv-wi-deliver";
        wi_init_minimal(&store, key);
        store
            .apply(
                key,
                "patch",
                &wi_args(json!({
                    "work_item_id": "1",
                    "milestones": [{"id": "m1", "status": "done", "remark": "a"}]
                })),
            )
            .expect("item 1");
        store
            .apply(
                key,
                "patch",
                &wi_args(json!({
                    "work_item_id": "2",
                    "milestones": [{"id": "m1", "status": "done", "remark": "b"}]
                })),
            )
            .expect("item 2");
        assert_eq!(store.work_items.store_stats(key).in_progress, 0);
        store
            .apply(
                key,
                "patch",
                &wi_args(json!({
                    "global_milestones": [{"id": "g_deliver", "status": "in_progress"}]
                })),
            )
            .expect("deliver without work_item_id");
    }
}
