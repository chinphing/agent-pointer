#[cfg(test)]
mod tests {
    use crate::conversation_store::persist::sample_conv;
    use crate::conversation_store::ConversationStore;
    use serde_json::{json, Value};
    use tempfile::TempDir;

    #[test]
    fn create_project_reuses_normalized_workspace_root() {
        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();

        let created = store
            .create_project("Original", "/workspace/example/")
            .unwrap();
        let reused = store
            .create_project("Duplicate name", "  /workspace/example  ")
            .unwrap();

        assert!(!created.reused_existing);
        assert!(reused.reused_existing);
        assert_eq!(reused.project.id, created.project.id);
        assert_eq!(reused.project.name, "Original");
        assert_eq!(reused.project.workspace_root, "/workspace/example");
        let matching: Vec<_> = store
            .load_sidebar_projects()
            .unwrap()
            .into_iter()
            .filter(|project| project.workspace_root == "/workspace/example")
            .collect();
        assert_eq!(matching.len(), 1);
    }

    #[test]
    fn create_project_requires_non_empty_workspace_root() {
        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        let error = store.create_project("Invalid", "  /  ").unwrap_err();
        assert!(error.to_string().contains("workspace root is required"));
    }

    #[test]
    fn sidebar_projects_are_capped_at_five_total_including_pinned() {
        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        let projects = (0..8)
            .map(|index| {
                store
                    .create_project(
                        &format!("Project {index}"),
                        &format!("/workspace/project-{index}"),
                    )
                    .unwrap()
                    .project
            })
            .collect::<Vec<_>>();
        for project in projects.iter().take(6) {
            store
                .update_project(&project.id, None, None, Some(true), None)
                .unwrap();
        }

        let sidebar = store.load_sidebar_projects().unwrap();
        let cursor = sidebar.last().map(|project| crate::models::ProjectCursor {
            last_activity_at: project.last_activity_at,
            id: project.id.clone(),
        });
        let next_page = store.load_projects(cursor, 5).unwrap();

        assert_eq!(sidebar.len(), 5);
        assert!(sidebar.iter().all(|project| project.is_pinned));
        assert_eq!(next_page.items.len(), 4);
        assert!(next_page.items[0].is_pinned);
        assert!(next_page.items[1..]
            .iter()
            .all(|project| !project.is_pinned));
    }

    #[test]
    fn projects_sort_by_pin_then_latest_conversation_activity() {
        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        let older = store
            .create_project("Older", "/workspace/older")
            .unwrap()
            .project;
        let newer = store
            .create_project("Newer", "/workspace/newer")
            .unwrap()
            .project;
        let mut older_conversation = sample_conv("older-conv", "Older", "older");
        older_conversation.project_id = Some(older.id.clone());
        older_conversation.workspace_root = older.workspace_root.clone();
        older_conversation.updated_at = 100;
        let mut newer_conversation = sample_conv("newer-conv", "Newer", "newer");
        newer_conversation.project_id = Some(newer.id.clone());
        newer_conversation.workspace_root = newer.workspace_root.clone();
        newer_conversation.updated_at = 200;
        store
            .sync_conversations(&[older_conversation, newer_conversation])
            .unwrap();

        let projects = store.load_projects(None, 20).unwrap().items;
        let older_index = projects.iter().position(|p| p.id == older.id).unwrap();
        let newer_index = projects.iter().position(|p| p.id == newer.id).unwrap();
        assert!(newer_index < older_index);
        assert_eq!(
            store
                .load_project(&newer.id)
                .unwrap()
                .unwrap()
                .last_activity_at,
            200
        );

        store
            .update_project(&older.id, None, None, Some(true), None)
            .unwrap();
        let projects = store.load_projects(None, 20).unwrap().items;
        let older_index = projects.iter().position(|p| p.id == older.id).unwrap();
        let newer_index = projects.iter().position(|p| p.id == newer.id).unwrap();
        assert!(older_index < newer_index);
    }

    #[test]
    fn sync_discover_and_scroll() {
        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        let convs = vec![
            sample_conv("c1", "Auth work", "We need to refactor auth middleware"),
            sample_conv("c2", "Other", "Unrelated topic about cooking"),
        ];
        store.sync_conversations(&convs).unwrap();

        let discover = store
            .dispatch_tool_for_test(&json!({ "query": "auth refactor", "limit": 3 }))
            .unwrap();
        let parsed: Value = serde_json::from_str(&discover).unwrap();
        assert_eq!(parsed["success"], true);
        assert_eq!(parsed["mode"], "discovery");
        assert_eq!(parsed["results"].as_array().unwrap().len(), 1);

        let scroll = store
            .dispatch_tool_for_test(&json!({
                "conversation_id": "c1",
                "around_message_id": "msg_u1",
                "window": 2,
                "_conversation_id": "c2"
            }))
            .unwrap();
        let scroll_p: Value = serde_json::from_str(&scroll).unwrap();
        assert_eq!(scroll_p["success"], true);
        assert_eq!(scroll_p["mode"], "scroll");
    }

    #[test]
    fn session_search_filters_by_session_user_id() {
        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        let mut user_a = sample_conv("c_a", "User A topic", "shared keyword alpha");
        user_a.session_user_id = "user-a".into();
        let mut user_b = sample_conv("c_b", "User B topic", "shared keyword beta");
        user_b.session_user_id = "user-b".into();
        store.sync_conversations(&[user_a, user_b]).unwrap();

        let for_a = store
            .dispatch_tool_for_test(&json!({
                "query": "keyword",
                "limit": 5,
                "_session_user_id": "user-a"
            }))
            .unwrap();
        let parsed_a: Value = serde_json::from_str(&for_a).unwrap();
        assert_eq!(parsed_a["success"], true);
        let ids_a: Vec<_> = parsed_a["results"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["conversation_id"].as_str().unwrap())
            .collect();
        assert_eq!(ids_a, vec!["c_a"]);

        let cross_read = store
            .dispatch_tool_for_test(&json!({
                "conversation_id": "c_b",
                "_session_user_id": "user-a"
            }))
            .unwrap();
        let cross_p: Value = serde_json::from_str(&cross_read).unwrap();
        assert_eq!(cross_p["success"], false);
    }

    #[test]
    fn ui_search_uses_fts_and_title() {
        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        let convs = vec![
            sample_conv("c1", "Auth work", "We need to refactor auth middleware"),
            sample_conv("c2", "Cooking notes", "Unrelated topic about cooking"),
        ];
        store.sync_conversations(&convs).unwrap();

        let hits = store.search_conversations("auth refactor", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, "c1");
        assert!(!hits[0].snippet.is_empty());
        assert_eq!(
            hits[0].message_id, "msg_u1",
            "body hit should return the matched message id"
        );

        let title_hits = store.search_conversations("Cooking", 10).unwrap();
        assert!(title_hits.iter().any(|h| h.id == "c2"));
    }

    #[test]
    fn ui_search_snippet_centers_on_cjk_hit() {
        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        let body = format!(
            "{}工作城市「北京」已填写完成",
            "=== 第1页 === - 1 - ".repeat(40)
        );
        store
            .sync_conversations(&[sample_conv("c1", "案件ID: 778508", &body)])
            .unwrap();

        let hits = store.search_conversations("北京", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert!(
            hits[0].snippet.contains("北京"),
            "snippet should include query, got {:?}",
            hits[0].snippet
        );
        assert!(
            !hits[0].snippet.starts_with("=== 第1页"),
            "snippet should not be document head, got {:?}",
            hits[0].snippet
        );
        let body = hits[0].snippet.trim_start_matches('…');
        let byte_pos = body.find("北京").expect("hit in snippet");
        let char_pos = body[..byte_pos].chars().count();
        assert!(
            char_pos <= 8,
            "hit should stay near start for CSS truncate, char_pos={char_pos} snippet={:?}",
            hits[0].snippet
        );
    }

    #[test]
    fn ui_search_prefers_message_that_contains_query() {
        use crate::conversation_store::persist::msg;
        use crate::models::{Conversation, Role};

        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        // Long OCR-like message without the contiguous query, plus a later hit.
        // FTS may rank either; snippet must still show「北京」.
        let ocr = format!(
            "{}日期 **2026-06-29** 附件路径",
            "=== 第1页 === ".repeat(20)
        );
        let hit_msg = "候选人期望工作城市是北京朝阳区";
        let conv = Conversation {
            id: "c1".into(),
            title: "案件ID: 841648".into(),
            created_at: 1_700_000_000_000,
            updated_at: 1_700_000_100_000,
            messages: vec![
                msg("m1", Role::Assistant, &ocr, 1_700_000_000_000),
                msg("m2", Role::User, hit_msg, 1_700_000_001_000),
            ],
            skill_ids: vec![],
            tool_rounds_used: 0,
            tool_rounds_used_supervisor: 0,
            computer_monitor_id: None,
            project_id: None,
            workspace_root: String::new(),
            workspace_user_set: false,
            workspace_inherit_disabled: false,
            lead_agent_id: crate::agents::DEFAULT_LEAD_AGENT_ID.to_string(),
            agent_mode: crate::agents::AGENT_MODE_SINGLE.to_string(),
            session_user_id: String::new(),
        };
        store.sync_conversations(&[conv]).unwrap();

        let hits = store.search_conversations("北京", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert!(
            hits[0].snippet.contains("北京"),
            "should locate contiguous hit message, got {:?}",
            hits[0].snippet
        );
        assert!(
            !hits[0].snippet.contains("2026-06-29"),
            "should not show OCR head, got {:?}",
            hits[0].snippet
        );
        assert_eq!(
            hits[0].message_id, "m2",
            "should point at the contiguous hit message"
        );
    }

    #[test]
    fn ui_search_uses_canonical_message_id_when_fts_metadata_drifts() {
        use crate::conversation_store::persist::msg;
        use crate::models::{Conversation, Role};
        use rusqlite::params;

        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        let conv = Conversation {
            id: "c1".into(),
            title: "推送记录".into(),
            created_at: 1_700_000_000_000,
            updated_at: 1_700_000_100_000,
            messages: vec![
                msg(
                    "assistant-before",
                    Role::Assistant,
                    "已完成提交。",
                    1_700_000_000_000,
                ),
                msg("user-hit", Role::User, "推送吧", 1_700_000_001_000),
            ],
            skill_ids: vec![],
            tool_rounds_used: 0,
            tool_rounds_used_supervisor: 0,
            computer_monitor_id: None,
            project_id: None,
            workspace_root: String::new(),
            workspace_user_set: false,
            workspace_inherit_disabled: false,
            lead_agent_id: crate::agents::DEFAULT_LEAD_AGENT_ID.to_string(),
            agent_mode: crate::agents::AGENT_MODE_SINGLE.to_string(),
            session_user_id: String::new(),
        };
        store.sync_conversations(&[conv]).unwrap();

        // Simulate stale FTS UNINDEXED metadata: the indexed row still belongs to
        // user-hit, but its redundant message_id points at the preceding assistant.
        {
            let conn = store.db.conn.lock();
            let (rowid, content, conversation_id, message_id, role): (
                i64,
                String,
                String,
                String,
                String,
            ) = conn
                .query_row(
                    "SELECT id, content, conversation_id, message_id, role
                     FROM messages WHERE conversation_id = 'c1' AND message_id = 'user-hit'",
                    [],
                    |row| {
                        Ok((
                            row.get(0)?,
                            row.get(1)?,
                            row.get(2)?,
                            row.get(3)?,
                            row.get(4)?,
                        ))
                    },
                )
                .unwrap();
            conn.execute(
                "INSERT INTO messages_fts(messages_fts, rowid, content, conversation_id, message_id, role)
                 VALUES ('delete', ?1, ?2, ?3, ?4, ?5)",
                params![rowid, content, conversation_id, message_id, role],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO messages_fts(rowid, content, conversation_id, message_id, role)
                 VALUES (?1, ?2, ?3, 'assistant-before', ?4)",
                params![rowid, content, conversation_id, role],
            )
            .unwrap();
        }

        let hits = store.search_conversations("推送吧", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].message_id, "user-hit");
        assert!(hits[0].snippet.contains("推送吧"));
    }

    #[test]
    fn ui_search_title_match_gets_snippet() {
        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        store
            .sync_conversations(&[sample_conv(
                "c1",
                "北京出差计划",
                "completely unrelated body text",
            )])
            .unwrap();

        let hits = store.search_conversations("北京", 10).unwrap();
        assert!(hits.iter().any(|h| h.id == "c1"));
        let hit = hits.iter().find(|h| h.id == "c1").unwrap();
        assert!(
            hit.snippet.contains("北京"),
            "title-only hit needs a snippet, got {:?}",
            hit.snippet
        );
        assert!(
            hit.message_id.is_empty(),
            "title-only hit should not claim a message id, got {:?}",
            hit.message_id
        );
    }

    #[test]
    fn ui_search_finds_messages_regardless_of_session_user_id() {
        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        let mut conv = sample_conv("c1", "Private chat", "secret keyword in body");
        conv.session_user_id = "user-a".into();
        store.sync_conversations(&[conv]).unwrap();

        let hits = store.search_conversations("secret keyword", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, "c1");
    }

    #[test]
    fn discover_cjk_bigram() {
        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        store
            .sync_conversations(&vec![sample_conv("c1", "认证", "我们需要重构认证中间件")])
            .unwrap();

        let discover = store
            .dispatch_tool_for_test(&json!({ "query": "认证", "limit": 3 }))
            .unwrap();
        let parsed: Value = serde_json::from_str(&discover).unwrap();
        assert_eq!(parsed["success"], true);
        assert_eq!(parsed["results"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn load_save_roundtrip() {
        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        let convs = vec![sample_conv("c1", "T", "hello world")];
        store.save_all(&convs).unwrap();
        let loaded = store.load_all().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, "c1");
        assert_eq!(loaded[0].messages.len(), 2);
    }

    #[test]
    fn skip_unchanged_upsert() {
        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        let convs = vec![sample_conv("c1", "T", "hello")];
        store.save_all(&convs).unwrap();
        store.save_all(&convs).unwrap();
        let loaded = store.load_all().unwrap();
        assert_eq!(loaded[0].messages[0].content, "hello");
    }

    #[test]
    fn read_session_includes_attachment_summary() {
        use crate::conversation_store::persist::msg;
        use crate::models::{MediaAttachment, Role};

        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        let mut user = msg("msg_img", Role::User, "", 1_700_000_000_000);
        user.attachments = Some(vec![MediaAttachment {
            id: "att-1".into(),
            kind: "image".into(),
            mime_type: "image/jpeg".into(),
            file_name: "id-card.jpg".into(),
            size_bytes: 1234,
            storage_rel_path: Some("c1/att-1_id-card.jpg".into()),
            content_base64: None,
            derived_text: None,
            local_abs_path: None,
            remote_url: None,
            oss_object_key: None,
        }]);
        let conv = crate::models::Conversation {
            id: "c_img".into(),
            title: "微信 私信".into(),
            created_at: 1_700_000_000_000,
            updated_at: 1_700_000_100_000,
            messages: vec![
                user,
                msg("msg_a1", Role::Assistant, "收到图片了", 1_700_000_001_000),
            ],
            skill_ids: vec![],
            tool_rounds_used: 0,
            tool_rounds_used_supervisor: 0,
            computer_monitor_id: None,
            project_id: None,
            workspace_root: String::new(),
            workspace_user_set: false,
            workspace_inherit_disabled: false,
            lead_agent_id: crate::agents::DEFAULT_LEAD_AGENT_ID.to_string(),
            agent_mode: crate::agents::AGENT_MODE_SINGLE.to_string(),
            session_user_id: String::new(),
        };
        store.sync_conversations(&[conv]).unwrap();

        let read = store
            .dispatch_tool_for_test(&json!({ "conversation_id": "c_img" }))
            .unwrap();
        let parsed: Value = serde_json::from_str(&read).unwrap();
        assert_eq!(parsed["mode"], "read");
        let first = &parsed["messages"][0];
        assert_eq!(first["content"], "");
        let atts = first["attachments"].as_array().unwrap();
        assert_eq!(atts.len(), 1);
        assert_eq!(atts[0]["fileName"], "id-card.jpg");
        assert_eq!(atts[0]["kind"], "image");
        assert!(atts[0]["ref"]
            .as_str()
            .unwrap()
            .starts_with("pointer-media://"));

        let discover = store
            .dispatch_tool_for_test(&json!({ "query": "id-card", "limit": 3 }))
            .unwrap();
        let disc_p: Value = serde_json::from_str(&discover).unwrap();
        assert_eq!(disc_p["results"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn im_session_round_trip_on_base_row() {
        use crate::conversation_store::im_session::ImSessionState;
        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        let base = "feishu:acct:group:g1:user1";
        let state = ImSessionState {
            session_epoch: 2,
            active_conversation_id: Some(format!("{base}@s2")),
            last_interaction_at_ms: 0,
            lead_agent_id: "coder".into(),
            agent_mode: "single".into(),
        };
        store.save_im_session(base, &state).unwrap();
        let loaded = store.load_im_session(base).unwrap();
        assert_eq!(loaded.session_epoch, 2);
        assert_eq!(
            loaded.active_conversation_id.as_deref(),
            Some(format!("{base}@s2").as_str())
        );
        assert_eq!(loaded.lead_agent_id, "coder");
    }

    fn conv_with(
        id: &str,
        title: &str,
        updated_at: i64,
        workspace_root: &str,
    ) -> crate::models::Conversation {
        let mut c = sample_conv(id, title, "hello world");
        c.updated_at = updated_at;
        c.workspace_root = workspace_root.to_string();
        c
    }

    #[test]
    fn load_metas_cursor_pagination_and_order() {
        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        // Insert 3 conversations with distinct (updated_at, id). Sort order is
        // (updated_at DESC, id DESC), so expected order is: c3, c2, c1.
        store
            .save_all(&[
                conv_with("c1", "T1", 1_000, ""),
                conv_with("c2", "T2", 2_000, ""),
                conv_with("c3", "T3", 3_000, ""),
            ])
            .unwrap();

        // First page (limit 2): c3, c2.
        let page1 = store.load_metas(None, 2).unwrap();
        assert_eq!(page1.len(), 2);
        assert_eq!(page1[0].id, "c3");
        assert_eq!(page1[1].id, "c2");

        // Second page using cursor = last row of page1 (c2).
        let cursor = (page1[1].updated_at, page1[1].id.clone());
        let page2 = store.load_metas(Some(cursor), 2).unwrap();
        assert_eq!(page2.len(), 1);
        assert_eq!(page2[0].id, "c1");
    }

    #[test]
    fn load_metas_populates_db_message_count_and_preview() {
        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        store.save_all(&[conv_with("c1", "T1", 1_000, "")]).unwrap();

        let metas = store.load_metas(None, 50).unwrap();
        assert_eq!(metas.len(), 1);
        // sample_conv writes a user "hello world" + assistant "Acknowledged.".
        assert_eq!(metas[0].message_count, 2);
        assert_eq!(metas[0].preview, "hello world");
    }

    #[test]
    fn latest_other_workspace_root_skips_self_and_empty() {
        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        // c1: empty workspace; c2: workspace /ws2 (older); c3: workspace /ws3 (newer).
        store
            .save_all(&[
                conv_with("c1", "T1", 1_000, ""),
                conv_with("c2", "T2", 2_000, "/ws2"),
                conv_with("c3", "T3", 3_000, "/ws3"),
            ])
            .unwrap();

        // Excluding c3 → most recent other with non-empty workspace is c2 (/ws2).
        let ws = store.latest_other_workspace_root("c3").unwrap();
        assert_eq!(ws.as_deref(), Some("/ws2"));

        // Excluding c1 → c3 is most recent → /ws3.
        let ws = store.latest_other_workspace_root("c1").unwrap();
        assert_eq!(ws.as_deref(), Some("/ws3"));

        // Excluding the only conversation with a workspace → None.
        let ws = store.latest_other_workspace_root("c2").unwrap();
        // c3 has /ws3, so excluding c2 still leaves c3.
        assert_eq!(ws.as_deref(), Some("/ws3"));

        // All workspaces empty for the other candidates.
        let dir2 = TempDir::new().unwrap();
        let store2 = ConversationStore::open_in_dir(dir2.path()).unwrap();
        store2
            .save_all(&[
                conv_with("c1", "T1", 1_000, ""),
                conv_with("c2", "T2", 2_000, ""),
            ])
            .unwrap();
        assert_eq!(store2.latest_other_workspace_root("c1").unwrap(), None);
    }

    #[test]
    fn list_all_ids_is_lightweight() {
        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        store
            .save_all(&[
                conv_with("c1", "T1", 1_000, ""),
                conv_with("c2", "T2", 2_000, ""),
            ])
            .unwrap();
        let mut ids = store.list_all_ids().unwrap();
        ids.sort();
        assert_eq!(ids, vec!["c1".to_string(), "c2".to_string()]);
    }

    #[test]
    fn save_meta_all_does_not_delete_unloaded_conversations() {
        // Regression: with cursor-paginated lazy loading, the frontend only
        // holds a subset of conversations. save_meta_all must be a pure upsert
        // and must NOT delete rows whose ids are absent from the passed list.
        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        store
            .save_all(&[
                conv_with("c1", "T1", 1_000, ""),
                conv_with("c2", "T2", 2_000, ""),
                conv_with("c3", "T3", 3_000, ""),
            ])
            .unwrap();
        // Simulate a paginated boot that only loaded the first two metas.
        let metas = store.load_metas(None, 2).unwrap();
        assert_eq!(metas.len(), 2);
        store.save_meta_all(&metas).unwrap();
        // All three conversations must still exist.
        let mut ids = store.list_all_ids().unwrap();
        ids.sort();
        assert_eq!(
            ids,
            vec!["c1".to_string(), "c2".to_string(), "c3".to_string()]
        );
    }

    #[test]
    fn save_meta_all_with_platform_user_binds_empty_session_user_id() {
        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        let meta = crate::models::ConversationMeta {
            id: "new-session".into(),
            title: "新会话".into(),
            created_at: 1_000,
            updated_at: 1_000,
            skill_ids: vec![],
            tool_rounds_used: 0,
            tool_rounds_used_supervisor: 0,
            computer_monitor_id: None,
            project_id: None,
            workspace_root: String::new(),
            workspace_user_set: false,
            workspace_inherit_disabled: false,
            lead_agent_id: crate::agents::DEFAULT_LEAD_AGENT_ID.to_string(),
            agent_mode: crate::agents::AGENT_MODE_SINGLE.to_string(),
            message_count: 0,
            preview: String::new(),
            session_user_id: String::new(),
        };
        store
            .save_meta_all_with_platform_user(&[meta], Some("platform-user-1"))
            .unwrap();
        assert_eq!(
            store.session_user_id("new-session").unwrap(),
            "platform-user-1"
        );
    }

    #[test]
    fn delete_conversation_removes_row_and_messages() {
        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        store
            .save_all(&[
                conv_with("c1", "T1", 1_000, ""),
                conv_with("c2", "T2", 2_000, ""),
            ])
            .unwrap();
        store.delete_conversation("c1").unwrap();
        let ids = store.list_all_ids().unwrap();
        assert_eq!(ids, vec!["c2".to_string()]);
        // Messages for the deleted conversation are gone too (FK cascade).
        let msgs = store.load_messages("c1").unwrap();
        assert!(msgs.is_empty());
    }

    #[test]
    fn load_meta_returns_single_row_without_messages() {
        // Regression: the chat-send persist hooks (patch_tool_rounds /
        // patch_ephemeral_workspace) use load_meta(id) instead of load_all() so
        // they stay O(log n) at scale and never deserialize messages.
        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        store
            .save_all(&[
                conv_with("c1", "T1", 1_000, "/ws1"),
                conv_with("c2", "T2", 2_000, ""),
            ])
            .unwrap();
        let m = store.load_meta("c2").unwrap().unwrap();
        assert_eq!(m.id, "c2");
        assert_eq!(m.title, "T2");
        assert_eq!(m.workspace_root, "");
        // Missing id -> None (no error).
        assert!(store.load_meta("nope").unwrap().is_none());
    }

    /// Regression: v12 DBs have im_* columns but lack session_user_id. init_schema
    /// must not CREATE INDEX on session_user_id before migrate_schema_columns runs.
    #[test]
    fn opens_and_migrates_legacy_v12_schema_without_session_user_id() {
        use rusqlite::Connection;

        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("conversations.db");
        {
            let conn = Connection::open(&db_path).unwrap();
            conn.execute_batch(
                "CREATE TABLE schema_version (version INTEGER NOT NULL);
                 INSERT INTO schema_version(version) VALUES (12);
                 CREATE TABLE store_meta (
                   key TEXT PRIMARY KEY,
                   value TEXT NOT NULL
                 );
                 CREATE TABLE conversations (
                   id TEXT PRIMARY KEY,
                   title TEXT NOT NULL,
                   created_at_ms INTEGER NOT NULL,
                   updated_at_ms INTEGER NOT NULL,
                   message_count INTEGER NOT NULL DEFAULT 0,
                   preview TEXT NOT NULL DEFAULT '',
                   skill_ids_json TEXT NOT NULL DEFAULT '[]',
                   tool_rounds_used INTEGER NOT NULL DEFAULT 0,
                   tool_rounds_used_supervisor INTEGER NOT NULL DEFAULT 0,
                   computer_monitor_id TEXT,
                   workspace_root TEXT NOT NULL DEFAULT '',
                   workspace_user_set INTEGER NOT NULL DEFAULT 0,
                   workspace_inherit_disabled INTEGER NOT NULL DEFAULT 0,
                   lead_agent_id TEXT NOT NULL DEFAULT 'general',
                   agent_mode TEXT NOT NULL DEFAULT 'single',
                   im_session_epoch INTEGER NOT NULL DEFAULT 0,
                   im_active_conversation_id TEXT,
                   im_last_interaction_at_ms INTEGER NOT NULL DEFAULT 0
                 );
                 CREATE TABLE messages (
                   id INTEGER PRIMARY KEY,
                   conversation_id TEXT NOT NULL,
                   message_id TEXT NOT NULL,
                   role TEXT NOT NULL,
                   content TEXT NOT NULL,
                   payload TEXT NOT NULL,
                   created_at_ms INTEGER NOT NULL,
                   position INTEGER NOT NULL,
                   UNIQUE(conversation_id, message_id)
                 );
                 INSERT INTO conversations (id, title, created_at_ms, updated_at_ms)
                 VALUES ('legacy-1', 'Legacy chat', 1, 2);",
            )
            .unwrap();
        }

        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        let metas = store.load_metas(None, 50).unwrap();
        assert_eq!(metas.len(), 1);
        assert_eq!(metas[0].id, "legacy-1");

        let conn = Connection::open(&db_path).unwrap();
        let version: i32 = conn
            .query_row("SELECT version FROM schema_version LIMIT 1", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(version, 19);
        let has_session_user_id: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('conversations') WHERE name = 'session_user_id'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(has_session_user_id, 1);
        let index_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'index' AND name = 'idx_conversations_user_updated'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(index_count, 1);
    }

    /// v18 DBs already at SCHEMA_VERSION gate must still pick up v19 cron columns
    /// when SCHEMA_VERSION is bumped (schedule_kind / schedule_raw).
    #[test]
    fn migrates_v18_cron_jobs_adds_schedule_kind() {
        use rusqlite::Connection;

        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("conversations.db");
        {
            let conn = Connection::open(&db_path).unwrap();
            conn.execute_batch(
                "CREATE TABLE schema_version (version INTEGER NOT NULL);
                 INSERT INTO schema_version(version) VALUES (18);
                 CREATE TABLE store_meta (
                   key TEXT PRIMARY KEY,
                   value TEXT NOT NULL
                 );
                 CREATE TABLE conversations (
                   id TEXT PRIMARY KEY,
                   title TEXT NOT NULL,
                   created_at_ms INTEGER NOT NULL,
                   updated_at_ms INTEGER NOT NULL,
                   message_count INTEGER NOT NULL DEFAULT 0,
                   preview TEXT NOT NULL DEFAULT '',
                   skill_ids_json TEXT NOT NULL DEFAULT '[]',
                   tool_rounds_used INTEGER NOT NULL DEFAULT 0,
                   tool_rounds_used_supervisor INTEGER NOT NULL DEFAULT 0,
                   computer_monitor_id TEXT,
                   workspace_root TEXT NOT NULL DEFAULT '',
                   workspace_user_set INTEGER NOT NULL DEFAULT 0,
                   workspace_inherit_disabled INTEGER NOT NULL DEFAULT 0,
                   lead_agent_id TEXT NOT NULL DEFAULT 'general',
                   agent_mode TEXT NOT NULL DEFAULT 'single',
                   im_session_epoch INTEGER NOT NULL DEFAULT 0,
                   im_active_conversation_id TEXT,
                   im_last_interaction_at_ms INTEGER NOT NULL DEFAULT 0,
                   session_user_id TEXT NOT NULL DEFAULT ''
                 );
                 CREATE TABLE messages (
                   id INTEGER PRIMARY KEY,
                   conversation_id TEXT NOT NULL,
                   message_id TEXT NOT NULL,
                   role TEXT NOT NULL,
                   content TEXT NOT NULL,
                   payload TEXT NOT NULL,
                   created_at_ms INTEGER NOT NULL,
                   position INTEGER NOT NULL,
                   UNIQUE(conversation_id, message_id)
                 );
                 CREATE TABLE cron_jobs (
                   id TEXT PRIMARY KEY,
                   label TEXT NOT NULL,
                   cron_expr TEXT NOT NULL,
                   conversation_id TEXT NOT NULL,
                   current_session_id TEXT,
                   prompt_text TEXT NOT NULL,
                   agent_mode TEXT,
                   lead_agent_id TEXT,
                   enabled INTEGER NOT NULL DEFAULT 1,
                   last_run_at_ms INTEGER,
                   next_run_at_ms INTEGER,
                   created_at_ms INTEGER NOT NULL,
                   deliver TEXT,
                   last_delivery_error TEXT
                 );",
            )
            .unwrap();
        }

        let _store = ConversationStore::open_in_dir(dir.path()).unwrap();
        let conn = Connection::open(&db_path).unwrap();
        let version: i32 = conn
            .query_row("SELECT version FROM schema_version LIMIT 1", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(version, 19);
        let has_kind: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('cron_jobs') WHERE name = 'schedule_kind'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let has_raw: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('cron_jobs') WHERE name = 'schedule_raw'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(has_kind, 1);
        assert_eq!(has_raw, 1);
    }
}
