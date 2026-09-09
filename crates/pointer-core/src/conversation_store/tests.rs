#[cfg(test)]
mod tests {
    use crate::conversation_store::persist::{msg, sample_conv};
    use crate::conversation_store::{ConversationStore, ListScope, LoadMessagesPageOpts};
    use crate::message_context::mark_excluded;
    use crate::models::{ExcludedReason, Role};
    use rusqlite::{params, Connection};
    use serde_json::{json, Value};
    use tempfile::TempDir;

    #[test]
    fn create_project_reuses_normalized_workspace_root() {
        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();

        let created = store
            .create_project("Original", "/workspace/example/", "")
            .unwrap();
        let reused = store
            .create_project("Duplicate name", "  /workspace/example  ", "")
            .unwrap();

        assert!(!created.reused_existing);
        assert!(reused.reused_existing);
        assert_eq!(reused.project.id, created.project.id);
        assert_eq!(reused.project.name, "Original");
        assert_eq!(reused.project.workspace_root, "/workspace/example");
        let matching: Vec<_> = store
            .load_sidebar_projects(&ListScope::User("".into()))
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
        let error = store.create_project("Invalid", "  /  ", "").unwrap_err();
        assert!(error.to_string().contains("workspace root is required"));
    }

    #[test]
    fn create_project_does_not_reuse_across_users() {
        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        let a = store
            .create_project("Shared path A", "/workspace/shared", "user-a")
            .unwrap();
        let b = store
            .create_project("Shared path B", "/workspace/shared", "user-b")
            .unwrap();
        assert!(!a.reused_existing);
        assert!(!b.reused_existing);
        assert_ne!(a.project.id, b.project.id);
        assert_eq!(a.project.session_user_id, "user-a");
        assert_eq!(b.project.session_user_id, "user-b");
        assert_eq!(
            store
                .load_sidebar_projects(&ListScope::User("user-a".into()))
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            store
                .load_sidebar_projects(&ListScope::User("user-b".into()))
                .unwrap()
                .len(),
            1
        );
        assert!(store
            .load_project(&a.project.id, &ListScope::User("user-b".into()))
            .unwrap()
            .is_none());
        // Platform admin list scope sees every user's projects.
        let admin_sidebar = store.load_sidebar_projects(&ListScope::All).unwrap();
        assert!(admin_sidebar.iter().any(|p| p.id == a.project.id));
        assert!(admin_sidebar.iter().any(|p| p.id == b.project.id));
        assert!(store
            .load_project(&a.project.id, &ListScope::All)
            .unwrap()
            .is_some());
    }

    #[test]
    fn list_scope_admin_sees_all_conversations_user_only_own() {
        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        let mut a = sample_conv("conv-a", "User A", "hello from a");
        a.session_user_id = "user-a".into();
        a.updated_at = 200;
        let mut b = sample_conv("conv-b", "User B", "hello from b");
        b.session_user_id = "user-b".into();
        b.updated_at = 100;
        store.sync_conversations(&[a, b]).unwrap();

        let admin = store.load_metas(&ListScope::All, None, 50).unwrap();
        assert_eq!(admin.len(), 2);

        let only_a = store
            .load_metas(&ListScope::User("user-a".into()), None, 50)
            .unwrap();
        assert_eq!(only_a.len(), 1);
        assert_eq!(only_a[0].id, "conv-a");

        let hits_all = store
            .search_conversations(&ListScope::All, "hello", 10)
            .unwrap();
        assert_eq!(hits_all.len(), 2);
        let hits_a = store
            .search_conversations(&ListScope::User("user-a".into()), "hello", 10)
            .unwrap();
        assert_eq!(hits_a.len(), 1);
        assert_eq!(hits_a[0].id, "conv-a");
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
                        "user-a",
                    )
                    .unwrap()
                    .project
            })
            .collect::<Vec<_>>();
        for project in projects.iter().take(6) {
            store
                .update_project(&project.id, "user-a", None, None, Some(true), None)
                .unwrap();
        }

        let sidebar = store
            .load_sidebar_projects(&ListScope::User("user-a".into()))
            .unwrap();
        let cursor = sidebar.last().map(|project| crate::models::ProjectCursor {
            last_activity_at: project.last_activity_at,
            id: project.id.clone(),
        });
        let next_page = store
            .load_projects(&ListScope::User("user-a".into()), cursor, 5)
            .unwrap();

        assert_eq!(sidebar.len(), 5);
        assert!(sidebar.iter().all(|project| project.is_pinned));
        assert_eq!(next_page.items.len(), 3);
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
            .create_project("Older", "/workspace/older", "")
            .unwrap()
            .project;
        let newer = store
            .create_project("Newer", "/workspace/newer", "")
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

        let projects = store
            .load_projects(&ListScope::User("".into()), None, 20)
            .unwrap()
            .items;
        let older_index = projects.iter().position(|p| p.id == older.id).unwrap();
        let newer_index = projects.iter().position(|p| p.id == newer.id).unwrap();
        assert!(newer_index < older_index);
        assert_eq!(
            store
                .load_project(&newer.id, &ListScope::User("".into()))
                .unwrap()
                .unwrap()
                .last_activity_at,
            200
        );

        store
            .update_project(&older.id, "", None, None, Some(true), None)
            .unwrap();
        let projects = store
            .load_projects(&ListScope::User("".into()), None, 20)
            .unwrap()
            .items;
        let older_index = projects.iter().position(|p| p.id == older.id).unwrap();
        let newer_index = projects.iter().position(|p| p.id == newer.id).unwrap();
        assert!(older_index < newer_index);
    }

    #[test]
    fn reconcile_default_project_is_per_session_user() {
        let dir = TempDir::new().unwrap();
        std::env::set_var("POINTER_APP_DATA_DIR", dir.path());
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();

        let mut a = sample_conv("conv-a", "A", "a");
        a.session_user_id = "user-a".into();
        a.updated_at = 100;
        let mut b = sample_conv("conv-b", "B", "b");
        b.session_user_id = "user-b".into();
        b.updated_at = 200;
        store
            .save_meta_all_with_platform_user(&[(&a).into()], Some("user-a"))
            .unwrap();
        store
            .save_meta_all_with_platform_user(&[(&b).into()], Some("user-b"))
            .unwrap();

        let projects_a = store
            .load_projects(&ListScope::User("user-a".into()), None, 20)
            .unwrap()
            .items;
        let projects_b = store
            .load_projects(&ListScope::User("user-b".into()), None, 20)
            .unwrap()
            .items;
        assert_eq!(projects_a.len(), 1);
        assert_eq!(projects_b.len(), 1);
        assert!(projects_a[0].is_default);
        assert!(projects_b[0].is_default);
        assert_ne!(projects_a[0].id, projects_b[0].id);
        assert_ne!(projects_a[0].workspace_root, projects_b[0].workspace_root);
        assert!(projects_a[0].workspace_root.contains("user-a"));
        assert!(projects_b[0].workspace_root.contains("user-b"));
        assert!(store
            .load_project(&projects_a[0].id, &ListScope::User("user-b".into()))
            .unwrap()
            .is_none());
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
            .dispatch_read_tool_for_test(&json!({
                "conversation_id": "c1",
                "around_message_id": "msg_u1",
                "limit": 5,
                "_conversation_id": "c2"
            }))
            .unwrap();
        let scroll_p: Value = serde_json::from_str(&scroll).unwrap();
        assert_eq!(scroll_p["success"], true);
        assert_eq!(scroll_p["mode"], "read");
    }

    #[test]
    fn discover_skips_prior_session_search_tool_dump() {
        use crate::conversation_store::persist::msg;
        use crate::models::Role;

        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        let dump = json!({
            "success": true,
            "mode": "discovery",
            "query": "发票附件 unique_ss_token",
            "results": [{"conversation_id": "other", "messages": [
                {"content": format!("{}{}", "PAD", "x".repeat(80_000))}
            ]}],
            "count": 1
        })
        .to_string();
        let mut conv = sample_conv("c_ss", "Invoice work", "follow-up note");
        let mut dump_msg = msg("msg_dump", Role::Tool, &dump, 1_700_000_000_000);
        dump_msg.tool_name = Some("session_search".into());
        conv.messages = vec![
            dump_msg,
            msg(
                "msg_real",
                Role::User,
                "请核对 unique_ss_token 发票附件是否已归档",
                1_700_000_001_000,
            ),
            msg("msg_a", Role::Assistant, "Acknowledged.", 1_700_000_002_000),
        ];
        store.sync_conversations(&[conv]).unwrap();

        let discover = store
            .dispatch_tool_for_test(&json!({
                "query": "unique_ss_token",
                "limit": 3
            }))
            .unwrap();
        let parsed: Value = serde_json::from_str(&discover).unwrap();
        assert_eq!(parsed["success"], true);
        assert_eq!(parsed["count"], 1);
        assert_eq!(parsed["results"][0]["match_message_id"], "msg_real");
        let ids: Vec<_> = parsed["results"][0]["messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["id"].as_str().unwrap().to_string())
            .collect();
        assert!(ids.contains(&"msg_real".to_string()));
        assert!(!ids.contains(&"msg_dump".to_string()));
        assert!(parsed["results"][0]["messages"]
            .as_array()
            .unwrap()
            .iter()
            .all(|m| m["content"].as_str().unwrap().len() < 20_000));
    }

    #[test]
    fn discover_skips_unnamed_legacy_session_search_envelope() {
        use crate::conversation_store::persist::msg;
        use crate::models::Role;

        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        let dump = json!({
            "success": true,
            "mode": "discovery",
            "query": "发票附件 unique_ss_legacy",
            "results": [{"conversation_id": "other", "messages": [
                {"content": format!("{}{}", "PAD", "x".repeat(80_000))}
            ]}],
            "count": 1
        })
        .to_string();
        let mut conv = sample_conv("c_ss_legacy", "Invoice work", "follow-up note");
        conv.messages = vec![
            msg("msg_dump", Role::Tool, &dump, 1_700_000_000_000),
            msg(
                "msg_real",
                Role::User,
                "请核对 unique_ss_legacy 发票附件是否已归档",
                1_700_000_001_000,
            ),
            msg("msg_a", Role::Assistant, "Acknowledged.", 1_700_000_002_000),
        ];
        store.sync_conversations(&[conv]).unwrap();

        let discover = store
            .dispatch_tool_for_test(&json!({
                "query": "unique_ss_legacy",
                "limit": 3
            }))
            .unwrap();
        let parsed: Value = serde_json::from_str(&discover).unwrap();
        assert_eq!(parsed["success"], true);
        assert_eq!(parsed["results"][0]["match_message_id"], "msg_real");

        let ui = store
            .search_conversations(&ListScope::All, "unique_ss_legacy", 10)
            .unwrap();
        assert_eq!(ui.len(), 1);
        assert_eq!(ui[0].message_id, "msg_real");
        assert!(
            !ui[0].snippet.contains("PAD"),
            "sidebar must not snippet the legacy dump, got {:?}",
            ui[0].snippet
        );
    }

    #[test]
    fn discover_skips_named_session_search_without_envelope() {
        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        let token = "unique_ss_col_token";
        let mut named = msg(
            "msg_named_ss",
            Role::Tool,
            &format!("{token} leftover note, not a search envelope"),
            1_700_000_000_000,
        );
        named.tool_name = Some("plugin.session_search".into());
        let mut conv = sample_conv("c_ss_col", "Column filter", "follow-up");
        conv.messages = vec![
            named,
            msg(
                "msg_real",
                Role::User,
                &format!("请核对 {token} 是否已归档"),
                1_700_000_001_000,
            ),
            msg("msg_a", Role::Assistant, "Acknowledged.", 1_700_000_002_000),
        ];
        store.sync_conversations(&[conv]).unwrap();

        let conn = Connection::open(dir.path().join("conversations.db")).unwrap();
        let (col, indexed): (Option<String>, String) = conn
            .query_row(
                "SELECT tool_name, content FROM messages WHERE message_id = ?1",
                params!["msg_named_ss"],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(col.as_deref(), Some("session_search"));
        assert_eq!(
            indexed,
            crate::conversation_store::persist::SESSION_SEARCH_INDEX_STUB
        );

        let discover = store
            .dispatch_tool_for_test(&json!({
                "query": token,
                "limit": 3
            }))
            .unwrap();
        let parsed: Value = serde_json::from_str(&discover).unwrap();
        assert_eq!(parsed["success"], true);
        assert_eq!(parsed["count"], 1);
        assert_eq!(parsed["results"][0]["match_message_id"], "msg_real");
        let ids: Vec<_> = parsed["results"][0]["messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["id"].as_str().unwrap().to_string())
            .collect();
        assert!(ids.contains(&"msg_real".to_string()));
        assert!(!ids.contains(&"msg_named_ss".to_string()));
    }

    #[test]
    fn discover_lists_multiple_matches_in_one_conversation() {
        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        let token = "unique_mm_hit_token";
        let mut conv = sample_conv("c_mm", "Multi hit", "follow-up");
        conv.messages = vec![
            msg(
                "msg_a",
                Role::User,
                &format!("first {token} note"),
                1_700_000_000_000,
            ),
            msg("msg_b", Role::Assistant, "ack", 1_700_000_001_000),
            msg(
                "msg_c",
                Role::User,
                &format!("second {token} note"),
                1_700_000_002_000,
            ),
            msg("msg_d", Role::Assistant, "ack2", 1_700_000_003_000),
        ];
        store.sync_conversations(&[conv]).unwrap();

        let discover = store
            .dispatch_tool_for_test(&json!({ "query": token, "limit": 3 }))
            .unwrap();
        let parsed: Value = serde_json::from_str(&discover).unwrap();
        assert_eq!(parsed["success"], true);
        assert_eq!(parsed["count"], 1);
        let matches = parsed["results"][0]["matches"].as_array().unwrap();
        assert_eq!(matches.len(), 2);
        let ids: Vec<_> = matches.iter().filter_map(|m| m["id"].as_str()).collect();
        assert!(ids.contains(&"msg_a"));
        assert!(ids.contains(&"msg_c"));
        assert_eq!(parsed["results"][0]["match_count"], 2);
        let mid = parsed["results"][0]["match_message_id"].as_str().unwrap();
        assert!(ids.contains(&mid));
        let window_ids: Vec<_> = parsed["results"][0]["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|m| m["id"].as_str())
            .collect();
        assert!(window_ids.contains(&mid));
    }

    #[test]
    fn discover_clips_content_around_query_hit() {
        use crate::conversation_store::persist::msg;
        use crate::models::Role;

        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        let long = format!("{} HITWORD {}", "aaa ".repeat(2_000), "bbb ".repeat(2_000));
        let mut conv = sample_conv("c_clip", "Long body", "short");
        conv.messages = vec![
            msg("msg_long", Role::User, &long, 1_700_000_000_000),
            msg("msg_a", Role::Assistant, "Acknowledged.", 1_700_000_001_000),
        ];
        store.sync_conversations(&[conv]).unwrap();

        let discover = store
            .dispatch_tool_for_test(&json!({
                "query": "HITWORD",
                "limit": 3
            }))
            .unwrap();
        let parsed: Value = serde_json::from_str(&discover).unwrap();
        let msgs = parsed["results"][0]["messages"].as_array().unwrap();
        let long_msg = msgs
            .iter()
            .find(|m| m["id"] == "msg_long")
            .expect("long user row");
        let body = long_msg["content"].as_str().unwrap();
        assert!(body.contains("HITWORD"), "body={body}");
        assert!(
            body.starts_with('…') || body.contains("HITWORD"),
            "expected hit-centered excerpt, body_head={:?}",
            body.chars().take(24).collect::<String>()
        );
        assert!(
            !body.starts_with("aaa aaa aaa"),
            "must not return the document head"
        );
        assert_eq!(long_msg["truncated"], true);
        assert!(long_msg["contentLimit"].as_u64().unwrap() <= 4_000);
        assert!(body.chars().count() <= 4_200);
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
            .dispatch_read_tool_for_test(&json!({
                "conversation_id": "c_b",
                "_session_user_id": "user-a"
            }))
            .unwrap();
        let cross_p: Value = serde_json::from_str(&cross_read).unwrap();
        assert_eq!(cross_p["success"], false);
    }

    #[test]
    fn session_search_requires_query() {
        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        let out = store.dispatch_tool_for_test(&json!({})).unwrap();
        let parsed: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["success"], false);
        assert!(parsed["error"].as_str().unwrap().contains("query"));
    }

    #[test]
    fn session_read_rejects_current_conversation_without_instance() {
        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        store
            .sync_conversations(&[sample_conv("c1", "T", "hello world")])
            .unwrap();
        let out = store
            .dispatch_read_tool_for_test(&json!({
                "conversation_id": "c1",
                "_conversation_id": "c1"
            }))
            .unwrap();
        let parsed: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["success"], false);
        assert!(parsed["error"]
            .as_str()
            .unwrap()
            .contains("agentInstanceId"));
    }

    #[test]
    fn lead_instance_reused_and_stamped_on_new_messages() {
        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        let first = store.ensure_lead_agent_instance("c_lead").unwrap();
        let second = store.ensure_lead_agent_instance("c_lead").unwrap();
        assert_eq!(first, second);
        store
            .sync_conversations(&[sample_conv("c_lead", "Lead", "hello stamped")])
            .unwrap();
        let loaded = store.load_messages("c_lead").unwrap();
        let stamped = loaded
            .iter()
            .filter_map(|m| m.agent_instance_id.as_deref())
            .collect::<Vec<_>>();
        assert!(!stamped.is_empty());
        assert!(stamped.iter().all(|id| *id == first.as_str()));
        let col = store
            .message_agent_instance_id_col("c_lead", "msg_u1")
            .unwrap();
        assert_eq!(col.as_deref(), Some(first.as_str()));
    }

    #[test]
    fn session_search_can_scope_current_conversation_by_instance() {
        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        let conv = sample_conv("c_cur", "Current", "unique_scope_token auth");
        store.sync_conversations(&[conv]).unwrap();
        let instance = store
            .load_messages("c_cur")
            .unwrap()
            .into_iter()
            .find_map(|m| m.agent_instance_id)
            .expect("stamped instance");
        let skipped = store
            .dispatch_tool_for_test(&json!({
                "query": "unique_scope_token",
                "_conversation_id": "c_cur"
            }))
            .unwrap();
        let skipped_p: Value = serde_json::from_str(&skipped).unwrap();
        assert_eq!(skipped_p["count"], 0);
        let scoped = store
            .dispatch_tool_for_test(&json!({
                "query": "unique_scope_token",
                "agentInstanceId": instance,
                "_conversation_id": "c_cur"
            }))
            .unwrap();
        let scoped_p: Value = serde_json::from_str(&scoped).unwrap();
        assert_eq!(scoped_p["count"], 1);
    }

    #[test]
    fn save_meta_rotates_lead_instance_when_lead_agent_changes() {
        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        store
            .sync_conversations(&[sample_conv("c_meta", "T", "hello")])
            .unwrap();
        let before = store.ensure_lead_agent_instance("c_meta").unwrap();
        let mut metas = store.load_metas(&ListScope::All, None, 10).unwrap();
        assert_eq!(metas.len(), 1);
        store.save_meta_all(&metas).unwrap();
        assert_eq!(store.ensure_lead_agent_instance("c_meta").unwrap(), before);
        metas[0].lead_agent_id = "coder".into();
        metas[0].updated_at += 1;
        store.save_meta_all(&metas).unwrap();
        let after = store.ensure_lead_agent_instance("c_meta").unwrap();
        assert_ne!(after, before);
        metas[0].agent_mode = "supervisor".into();
        metas[0].updated_at += 1;
        store.save_meta_all(&metas).unwrap();
        assert_eq!(store.ensure_lead_agent_instance("c_meta").unwrap(), after);
    }

    #[test]
    fn opens_backfills_agent_instance_id_column_from_payload() {
        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        let mut conv = sample_conv("c_bf", "BF", "payload stamp token");
        conv.messages[0].agent_instance_id = Some("inst-from-payload".into());
        conv.messages[1].agent_instance_id = Some("inst-from-payload".into());
        store.sync_conversations(&[conv]).unwrap();
        drop(store);

        let db_path = dir.path().join("conversations.db");
        {
            let conn = Connection::open(&db_path).unwrap();
            conn.execute("UPDATE messages SET agent_instance_id = NULL", [])
                .unwrap();
            conn.execute(
                "DELETE FROM store_meta WHERE key = 'agent_instance_id_col_v25'",
                [],
            )
            .unwrap();
        }
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        assert_eq!(
            store
                .message_agent_instance_id_col("c_bf", "msg_u1")
                .unwrap()
                .as_deref(),
            Some("inst-from-payload")
        );
    }

    #[test]
    fn session_search_instance_window_excludes_other_threads() {
        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        let lead = "lead-aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";
        let child = "child-bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb";
        let mut conv = sample_conv("c_win", "Mixed", "placeholder");
        let mut lead_before = msg("m_lead_before", Role::User, "lead_noise_aaa", 1);
        lead_before.agent_instance_id = Some(lead.into());
        let mut child_hit = msg(
            "m_child_hit",
            Role::Assistant,
            "child_hit_unique_bbb",
            2,
        );
        child_hit.agent_instance_id = Some(child.into());
        let mut lead_after = msg("m_lead_after", Role::User, "lead_noise_ccc", 3);
        lead_after.agent_instance_id = Some(lead.into());
        conv.messages = vec![lead_before, child_hit, lead_after];
        store.sync_conversations(&[conv]).unwrap();
        let out = store
            .dispatch_tool_for_test(&json!({
                "query": "child_hit_unique_bbb",
                "agentInstanceId": child,
                "window": 20
            }))
            .unwrap();
        let parsed: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["count"], 1);
        let window = parsed["results"][0]["messages"].as_array().unwrap();
        let texts: Vec<&str> = window
            .iter()
            .filter_map(|m| m["content"].as_str())
            .collect();
        assert!(texts.iter().any(|t| t.contains("child_hit_unique_bbb")));
        assert!(texts.iter().all(|t| !t.contains("lead_noise")));
    }

    #[test]
    fn session_search_primary_instance_follows_assistant_promote() {
        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        let lead = "lead-cccccccc-cccc-cccc-cccc-cccccccccccc";
        let child = "child-dddddddd-dddd-dddd-dddd-dddddddddddd";
        let mut conv = sample_conv("c_prom", "Promote", "placeholder");
        let mut tool_hit = msg(
            "m_tool",
            Role::Tool,
            "shared_promote_zzz child tool",
            1,
        );
        tool_hit.agent_instance_id = Some(child.into());
        tool_hit.tool_name = Some("terminal".into());
        let mut assistant_hit = msg(
            "m_asst",
            Role::Assistant,
            "shared_promote_zzz assistant",
            2,
        );
        assistant_hit.agent_instance_id = Some(lead.into());
        conv.messages = vec![tool_hit, assistant_hit];
        store.sync_conversations(&[conv]).unwrap();
        let out = store
            .dispatch_tool_for_test(&json!({
                "query": "shared_promote_zzz",
                "conversation_id": "c_prom",
                "limit": 3
            }))
            .unwrap();
        let parsed: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["count"], 1);
        assert_eq!(parsed["results"][0]["match_message_id"], "m_asst");
        assert_eq!(parsed["results"][0]["agentInstanceId"], lead);
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

        let hits = store
            .search_conversations(&ListScope::All, "auth refactor", 10)
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, "c1");
        assert!(!hits[0].snippet.is_empty());
        assert_eq!(
            hits[0].message_id, "msg_u1",
            "body hit should return the matched message id"
        );

        let title_hits = store
            .search_conversations(&ListScope::All, "Cooking", 10)
            .unwrap();
        assert!(title_hits.iter().any(|h| h.id == "c2"));
    }

    #[test]
    fn ui_search_lists_multiple_matches_in_one_conversation() {
        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        let token = "unique_ui_mm_token";
        let mut conv = sample_conv("c_ui_mm", "UI multi", "follow-up");
        conv.messages = (0..16)
            .map(|i| {
                let ts = 1_700_000_000_000 + i * 1_000;
                if i % 2 == 0 {
                    msg(
                        &format!("u_{i}"),
                        Role::User,
                        &format!("hit {i} {token} here"),
                        ts,
                    )
                } else {
                    msg(&format!("u_{i}"), Role::Assistant, "ack", ts)
                }
            })
            .collect();
        store.sync_conversations(&[conv]).unwrap();

        let hits = store
            .search_conversations(&ListScope::All, token, 10)
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, "c_ui_mm");
        // Keystroke path only hydrates the primary snippet; full list is on expand.
        assert_eq!(hits[0].match_count, 8);
        assert_eq!(hits[0].matches.len(), 1);
        let listed = store
            .list_conversation_search_matches(&ListScope::All, "c_ui_mm", token)
            .unwrap();
        assert_eq!(listed.len(), 8);
        let ids: Vec<_> = listed.iter().map(|m| m.message_id.as_str()).collect();
        assert_eq!(
            ids,
            ["u_0", "u_2", "u_4", "u_6", "u_8", "u_10", "u_12", "u_14"]
        );
        assert_eq!(hits[0].match_count as usize, listed.len());
    }

    #[test]
    fn ui_search_match_count_ignores_fts_hits_beyond_content_prefix() {
        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        let token = "unique_ui_prefix_tail_token";
        let padding = "z".repeat(20_000);
        let mut conv = sample_conv("c_ui_prefix", "Prefix clip", "follow-up");
        conv.messages = vec![
            msg(
                "u_near",
                Role::User,
                &format!("near hit {token}"),
                1_700_000_000_000,
            ),
            msg(
                "u_tail",
                Role::User,
                &format!("{padding}{token}"),
                1_700_000_001_000,
            ),
        ];
        store.sync_conversations(&[conv]).unwrap();

        let hits = store
            .search_conversations(&ListScope::All, token, 10)
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].match_count, 1);
        let listed = store
            .list_conversation_search_matches(&ListScope::All, "c_ui_prefix", token)
            .unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].message_id, "u_near");
        assert_eq!(hits[0].match_count as usize, listed.len());
    }

    #[test]
    fn ui_search_orders_assistant_before_tool() {
        use crate::conversation_store::persist::msg;
        use crate::models::Role;

        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        let token = "unique_role_ord_token";
        let mut conv = sample_conv("c_role_ord", "Role order", "follow-up");
        conv.messages = vec![
            msg(
                "t1",
                Role::Tool,
                &format!("tool saw {token} first"),
                1_700_000_000_000,
            ),
            msg(
                "u1",
                Role::User,
                &format!("user mentioned {token}"),
                1_700_000_001_000,
            ),
            msg(
                "a1",
                Role::Assistant,
                &format!("assistant explained {token}"),
                1_700_000_002_000,
            ),
        ];
        store.sync_conversations(&[conv]).unwrap();

        let hits = store
            .search_conversations(&ListScope::All, token, 10)
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].message_id, "a1");
        assert_eq!(hits[0].match_count, 2);
        assert_eq!(hits[0].matches.len(), 1);
        assert_eq!(hits[0].matches[0].message_id, "a1");
        let listed = store
            .list_conversation_search_matches(&ListScope::All, "c_role_ord", token)
            .unwrap();
        let roles: Vec<_> = listed.iter().map(|m| m.role.as_str()).collect();
        assert_eq!(roles, ["assistant", "user"]);
        let ids: Vec<_> = listed.iter().map(|m| m.message_id.as_str()).collect();
        assert_eq!(ids, ["a1", "u1"]);
        assert_eq!(hits[0].match_count as usize, listed.len());

        let discover = store
            .dispatch_tool_for_test(&json!({ "query": token, "limit": 3 }))
            .unwrap();
        let parsed: Value = serde_json::from_str(&discover).unwrap();
        assert_eq!(parsed["results"][0]["match_message_id"], "a1");
        let disc_roles: Vec<_> = parsed["results"][0]["matches"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|m| m["role"].as_str())
            .collect();
        assert_eq!(disc_roles, ["assistant", "user", "tool"]);
    }

    #[test]
    fn ui_search_skips_all_tool_rows() {
        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        let token = "unique_tool_only_token";
        let mut conv = sample_conv("c_tool_only", "No keyword in title", "plain user line");
        let mut tool = msg(
            "t_grep",
            Role::Tool,
            &format!("file_grep saw {token}"),
            1_700_000_002_000,
        );
        tool.tool_name = Some("file_grep".into());
        conv.messages.push(tool);
        store.sync_conversations(&[conv]).unwrap();

        let hits = store
            .search_conversations(&ListScope::All, token, 10)
            .unwrap();
        assert!(
            hits.is_empty(),
            "sidebar search must ignore tool bodies, got {hits:?}"
        );
        let listed = store
            .list_conversation_search_matches(&ListScope::All, "c_tool_only", token)
            .unwrap();
        assert!(listed.is_empty());
    }

    #[test]
    fn ui_search_finds_ascii_acronym_in_hyphenated_token() {
        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        let mut conv = sample_conv("c_cwpt", "报销技能", "帮我找一下报销 skill");
        conv.messages.push(msg(
            "a_cwpt",
            Role::Assistant,
            "找到了 **`cwpt-reimburse-review`** 和 CWPT_TOKEN",
            1_700_000_002_000,
        ));
        store.sync_conversations(&[conv]).unwrap();

        let hits = store
            .search_conversations(&ListScope::All, "CWPT", 10)
            .unwrap();
        assert_eq!(hits.len(), 1, "expected CWPT body hit, got {hits:?}");
        assert!(
            hits[0].snippet.to_lowercase().contains("cwpt"),
            "snippet={:?}",
            hits[0].snippet
        );
    }

    #[test]
    fn ui_search_does_not_match_role_column() {
        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        store
            .sync_conversations(&[sample_conv(
                "c_role_tok",
                "Weather notes",
                "plain user line",
            )])
            .unwrap();
        let hits = store
            .search_conversations(&ListScope::All, "assistant", 10)
            .unwrap();
        assert!(
            hits.is_empty(),
            "indexing role must not make every assistant row match query assistant, got {hits:?}"
        );
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

        let hits = store
            .search_conversations(&ListScope::All, "北京", 10)
            .unwrap();
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
            is_pinned: false,
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
            performance_mode: None,
            session_user_id: String::new(),
        };
        store.sync_conversations(&[conv]).unwrap();

        let hits = store
            .search_conversations(&ListScope::All, "北京", 10)
            .unwrap();
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
            is_pinned: false,
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
            performance_mode: None,
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

        let hits = store
            .search_conversations(&ListScope::All, "推送吧", 10)
            .unwrap();
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

        let hits = store
            .search_conversations(&ListScope::All, "北京", 10)
            .unwrap();
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

        let hits = store
            .search_conversations(&ListScope::All, "secret keyword", 10)
            .unwrap();
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
            is_pinned: false,
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
            performance_mode: None,
            session_user_id: String::new(),
        };
        store.sync_conversations(&[conv]).unwrap();

        let read = store
            .dispatch_read_tool_for_test(&json!({ "conversation_id": "c_img" }))
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
        // (is_pinned DESC, updated_at DESC, id DESC), so expected order is: c3, c2, c1.
        store
            .save_all(&[
                conv_with("c1", "T1", 1_000, ""),
                conv_with("c2", "T2", 2_000, ""),
                conv_with("c3", "T3", 3_000, ""),
            ])
            .unwrap();

        // First page (limit 2): c3, c2.
        let page1 = store.load_metas(&ListScope::All, None, 2).unwrap();
        assert_eq!(page1.len(), 2);
        assert_eq!(page1[0].id, "c3");
        assert_eq!(page1[1].id, "c2");

        // Second page using cursor = last row of page1 (c2).
        let cursor = (page1[1].updated_at, page1[1].id.clone());
        let page2 = store.load_metas(&ListScope::All, Some(cursor), 2).unwrap();
        assert_eq!(page2.len(), 1);
        assert_eq!(page2[0].id, "c1");
    }

    #[test]
    fn load_metas_pins_before_recent_activity() {
        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        let mut older_pinned = conv_with("pinned-old", "Pinned", 1_000, "");
        older_pinned.is_pinned = true;
        store
            .save_all(&[
                older_pinned,
                conv_with("fresh", "Fresh", 9_000, ""),
                conv_with("mid", "Mid", 5_000, ""),
            ])
            .unwrap();

        let page1 = store.load_metas(&ListScope::All, None, 2).unwrap();
        assert_eq!(page1[0].id, "pinned-old");
        assert!(page1[0].is_pinned);
        assert_eq!(page1[1].id, "fresh");
        assert!(!page1[1].is_pinned);

        let cursor = (page1[1].updated_at, page1[1].id.clone());
        let page2 = store.load_metas(&ListScope::All, Some(cursor), 2).unwrap();
        assert_eq!(page2.len(), 1);
        assert_eq!(page2[0].id, "mid");
    }

    #[test]
    fn load_metas_populates_db_message_count_and_preview() {
        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        store.save_all(&[conv_with("c1", "T1", 1_000, "")]).unwrap();

        let metas = store.load_metas(&ListScope::All, None, 50).unwrap();
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
        let metas = store.load_metas(&ListScope::All, None, 2).unwrap();
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
            is_pinned: false,
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
            performance_mode: None,
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
        let metas = store.load_metas(&ListScope::All, None, 50).unwrap();
        assert_eq!(metas.len(), 1);
        assert_eq!(metas[0].id, "legacy-1");

        let conn = Connection::open(&db_path).unwrap();
        let version: i32 = conn
            .query_row("SELECT version FROM schema_version LIMIT 1", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(version, 26);
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

    /// v18 DBs already at SCHEMA_VERSION gate must still pick up later cron
    /// columns when SCHEMA_VERSION is bumped (schedule_kind / schedule_raw).
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
        assert_eq!(version, 26);
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

    #[test]
    fn anchor_probe_filters_system_generated_and_scoped_rows_end_to_end() {
        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        let mut conv = sample_conv("page-anchor", "Anchor", "real one");
        // sample_conv has: u1 (real user), a1 (assistant). Append a synthetic
        // user row, a second real user turn, and a scoped sub-message.
        let mut synthetic = msg("u-syn", Role::User, "[CUR_SCREEN] shot", 2);
        synthetic.id = "u-syn".into();
        let u2 = msg("u2", Role::User, "real two", 3);
        let a2 = msg("a2", Role::Assistant, "ok", 4);
        let mut scoped = msg("u-scoped", Role::User, "sub", 5);
        scoped.anchor_message_id = Some("u2".into());
        conv.messages.push(synthetic);
        conv.messages.push(u2);
        conv.messages.push(a2);
        conv.messages.push(scoped);
        store.sync_conversations(&[conv]).unwrap();

        // Tail of 1 turn: only real user rows are anchors, so the window starts
        // at u2 (u-syn / u-scoped must NOT count as separate turns).
        let page = store
            .load_messages_page(
                "page-anchor",
                &LoadMessagesPageOpts {
                    limit_turns: Some(1),
                    ..Default::default()
                },
            )
            .unwrap();
        let ids: Vec<&str> = page.messages.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["u2", "a2"],
            "tail window omits scoped rows unless include_scoped_sub_messages"
        );
        assert_eq!(page.oldest_position, Some(3), "oldest = u2 position");
        assert_eq!(page.newest_position, Some(4), "newest = last lead row position");
        assert!(page.has_more_older);
        assert!(!page.has_more_newer);
        // message_count counts ALL rows (frontend hydration uses it), not just anchors.
        assert_eq!(page.message_count, 6);

        let with_scoped = store
            .load_messages_page(
                "page-anchor",
                &LoadMessagesPageOpts {
                    limit_turns: Some(1),
                    include_scoped_sub_messages: true,
                    ..Default::default()
                },
            )
            .unwrap();
        let with_ids: Vec<&str> = with_scoped.messages.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(
            with_ids,
            vec!["u2", "a2"],
            "include_scoped splits scoped rows out of messages"
        );
        let scoped_flat: Vec<&str> = with_scoped
            .scoped
            .values()
            .flatten()
            .map(|m| m.id.as_str())
            .collect();
        assert_eq!(scoped_flat, vec!["u-scoped"]);

        // Before u2: end bound = first row at/after u2's position, window = [0, 3).
        let before = store
            .load_messages_page(
                "page-anchor",
                &LoadMessagesPageOpts {
                    limit_turns: Some(1),
                    before_position: Some(3),
                    ..Default::default()
                },
            )
            .unwrap();
        let before_ids: Vec<&str> = before.messages.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(
            before_ids,
            vec!["msg_u1", "msg_a1", "u-syn"],
            "before window ends before u2"
        );
        assert_eq!(before.oldest_position, Some(0));
        assert_eq!(before.newest_position, Some(2));
        assert!(!before.has_more_older);
        assert!(before.has_more_newer);
        assert_eq!(before.message_count, 6);
    }

    #[test]
    fn default_page_keeps_soft_excluded_lead_rows() {
        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        let mut conv = sample_conv("page-excluded", "Excluded", "before compression");
        mark_excluded(
            &mut conv.messages[0],
            ExcludedReason::ContextCompression,
        );
        let u2 = msg("u2", Role::User, "after summary", 3);
        let a2 = msg("a2", Role::Assistant, "ok", 4);
        let mut scoped = msg("u-scoped", Role::User, "sub", 5);
        scoped.anchor_message_id = Some("u2".into());
        conv.messages.push(u2);
        conv.messages.push(a2);
        conv.messages.push(scoped);
        store.sync_conversations(&[conv]).unwrap();

        let conn = Connection::open(dir.path().join("conversations.db")).unwrap();
        let scoped_flag: i64 = conn
            .query_row(
                "SELECT is_scoped FROM messages WHERE conversation_id = ?1 AND message_id = ?2",
                params!["page-excluded", "u-scoped"],
                |r| r.get(0),
            )
            .unwrap();
        let lead_flag: i64 = conn
            .query_row(
                "SELECT is_scoped FROM messages WHERE conversation_id = ?1 AND message_id = ?2",
                params!["page-excluded", "msg_u1"],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(scoped_flag, 1);
        assert_eq!(lead_flag, 0);

        let page = store
            .load_messages_page(
                "page-excluded",
                &LoadMessagesPageOpts {
                    limit_turns: Some(8),
                    ..Default::default()
                },
            )
            .unwrap();
        let ids: Vec<&str> = page.messages.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["msg_u1", "msg_a1", "u2", "a2"],
            "compressed lead rows stay on the UI page; scoped rows stay out"
        );

        let around = store
            .load_messages_page(
                "page-excluded",
                &LoadMessagesPageOpts {
                    limit_turns: Some(8),
                    around_message_id: Some("msg_u1".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(
            around.messages.iter().any(|m| m.id == "msg_u1"),
            "nav jump to a compressed user turn must still hydrate it"
        );
    }

    #[test]
    fn load_scoped_prefers_agent_instance_id() {
        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        let mut conv = sample_conv("scoped-inst", "Scoped", "hi");
        let mut a = msg("sc-a", Role::Assistant, "one", 2);
        a.anchor_message_id = Some("msg_a1".into());
        a.trace_id = Some("task:coder".into());
        a.agent_instance_id = Some("inst-a".into());
        let mut b = msg("sc-b", Role::Assistant, "two", 3);
        b.anchor_message_id = Some("msg_a1".into());
        b.trace_id = Some("task:coder".into());
        b.agent_instance_id = Some("inst-b".into());
        conv.messages.push(a);
        conv.messages.push(b);
        store.sync_conversations(&[conv]).unwrap();

        let by_instance = store
            .load_scoped_sub_messages_for_trace("scoped-inst", "", "", Some("inst-b"))
            .unwrap();
        assert_eq!(
            by_instance.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            vec!["sc-b"]
        );

        let by_instance_and_anchor = store
            .load_scoped_sub_messages_for_trace("scoped-inst", "msg_a1", "", Some("inst-a"))
            .unwrap();
        assert_eq!(
            by_instance_and_anchor
                .iter()
                .map(|m| m.id.as_str())
                .collect::<Vec<_>>(),
            vec!["sc-a"]
        );

        let by_legacy = store
            .load_scoped_sub_messages_for_trace("scoped-inst", "msg_a1", "task:coder", None)
            .unwrap();
        assert_eq!(
            by_legacy.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            vec!["sc-a", "sc-b"]
        );

        let empty = store
            .load_scoped_sub_messages_for_trace("scoped-inst", "", "", None)
            .unwrap();
        assert!(empty.is_empty());
    }

    /// Ops helper: `POINTER_MIGRATE_OPEN=1 POINTER_APP_DATA_DIR=… cargo test -p pointer-core open_app_data_dir_once -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn open_app_data_dir_once() {
        if std::env::var_os("POINTER_MIGRATE_OPEN").as_deref() != Some(std::ffi::OsStr::new("1")) {
            eprintln!("skip: set POINTER_MIGRATE_OPEN=1 to open POINTER_APP_DATA_DIR");
            return;
        }
        let store = ConversationStore::open_default().expect("open_default");
        let n = store
            .load_metas(&ListScope::All, None, 1)
            .expect("load_metas")
            .len();
        eprintln!("open_app_data_dir_once: ok metas_sample={n}");
    }

    #[test]
    fn backfill_system_generated_only_flags_synthetic_users() {
        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        store
            .sync_conversations(&[sample_conv("bf-legacy", "BF", "real one")])
            .unwrap();
        let mut conv = sample_conv("bf-legacy", "BF", "real one");
        let mut synthetic = msg("u-syn", Role::User, "[CUR_SCREEN] shot", 2);
        synthetic.id = "u-syn".into();
        conv.messages.push(synthetic);
        conv.messages.push(msg(
            "u-env",
            Role::User,
            "【环境反馈】本回合模型输出异常（error）。",
            3,
        ));
        store.replace_messages("bf-legacy", &conv.messages).unwrap();

        let db_path = dir.path().join("conversations.db");
        {
            let conn = Connection::open(&db_path).unwrap();
            // Simulate a pre-backfill DB: clear flags and meta so open re-runs it.
            conn.execute_batch(
                "UPDATE messages SET is_system_generated = 0;
                 DELETE FROM store_meta WHERE key = 'is_system_generated_backfilled';
                 DELETE FROM store_meta WHERE key = 'is_system_generated_backfilled_v2';
                 UPDATE schema_version SET version = 20;",
            )
            .unwrap();
        }

        // Re-open triggers v21 migrate + optimized backfill.
        let _store2 = ConversationStore::open_in_dir(dir.path()).unwrap();
        let conn = Connection::open(&db_path).unwrap();
        let flags: Vec<(String, i64)> = conn
            .prepare(
                "SELECT message_id, is_system_generated FROM messages WHERE conversation_id = ?1 ORDER BY position ASC",
            )
            .unwrap()
            .query_map(params!["bf-legacy"], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(
            flags,
            vec![
                ("msg_u1".to_string(), 0),
                ("msg_a1".to_string(), 0),
                ("u-syn".to_string(), 1),
                ("u-env".to_string(), 1)
            ]
        );
        let meta: String = conn
            .query_row(
                "SELECT value FROM store_meta WHERE key = 'is_system_generated_backfilled_v2'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(meta, "1");
        let version: i32 = conn
            .query_row("SELECT version FROM schema_version LIMIT 1", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert!(version >= 21);
    }

    #[test]
    fn backfill_context_included_flags_soft_excluded_and_scoped() {
        use crate::models::{ExcludedReason, MessageContextState};

        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        let mut conv = sample_conv("bf-ctx", "BF", "real one");
        conv.messages[0].context_state = Some(MessageContextState {
            included: false,
            excluded_reason: Some(ExcludedReason::ContextCompression),
        });
        let mut scoped = msg("u-scoped", Role::User, "child", 3);
        scoped.anchor_message_id = Some("msg_a1".into());
        conv.messages.push(scoped);
        store.save_all(&[conv]).unwrap();

        let db_path = dir.path().join("conversations.db");
        {
            let conn = Connection::open(&db_path).unwrap();
            conn.execute_batch(
                "UPDATE messages SET context_included = 1;
                 DELETE FROM store_meta WHERE key = 'context_included_backfilled';
                 UPDATE schema_version SET version = 21;",
            )
            .unwrap();
        }

        let _store2 = ConversationStore::open_in_dir(dir.path()).unwrap();
        let conn = Connection::open(&db_path).unwrap();
        let flags: Vec<(String, i64)> = conn
            .prepare(
                "SELECT message_id, context_included FROM messages
                 WHERE conversation_id = ?1 ORDER BY position ASC",
            )
            .unwrap()
            .query_map(params!["bf-ctx"], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(
            flags,
            vec![
                ("msg_u1".to_string(), 0),
                ("msg_a1".to_string(), 1),
                ("u-scoped".to_string(), 0),
            ]
        );
        let meta: String = conn
            .query_row(
                "SELECT value FROM store_meta WHERE key = 'context_included_backfilled'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(meta, "1");
        let version: i32 = conn
            .query_row("SELECT version FROM schema_version LIMIT 1", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert!(version >= 22);
    }

    #[test]
    fn backfill_context_included_skips_malformed_payload_rows() {
        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        let conv = sample_conv("bf-corrupt", "BF", "real one");
        store.save_all(&[conv]).unwrap();

        let db_path = dir.path().join("conversations.db");
        {
            let conn = Connection::open(&db_path).unwrap();
            conn.execute_batch(
                "INSERT INTO messages(conversation_id, message_id, role, content, payload, created_at_ms, position, is_system_generated, context_included)
                 VALUES ('bf-corrupt', 'corrupt-1', 'assistant', 'x', '{\"not valid json', 1782538761000, 100, 0, 1);
                 UPDATE messages SET context_included = 1;
                 DELETE FROM store_meta WHERE key = 'context_included_backfilled';
                 UPDATE schema_version SET version = 21;",
            )
            .unwrap();
        }

        // Reopen must succeed even though one row's payload is malformed JSON.
        let _store2 = ConversationStore::open_in_dir(dir.path()).unwrap();
        let conn = Connection::open(&db_path).unwrap();
        // Malformed rows are skipped by the backfill (stay at the default 1).
        let corrupt_flag: i64 = conn
            .query_row(
                "SELECT context_included FROM messages WHERE message_id = 'corrupt-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(corrupt_flag, 1);
        let meta: String = conn
            .query_row(
                "SELECT value FROM store_meta WHERE key = 'context_included_backfilled'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(meta, "1");
        let version: i32 = conn
            .query_row("SELECT version FROM schema_version LIMIT 1", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert!(version >= 22);
    }

    #[test]
    fn replace_messages_writes_system_generated_flag() {
        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        // replace_messages does not create the conversation row (FK); seed it first.
        store
            .sync_conversations(&[sample_conv("flag-write", "Flag", "real one")])
            .unwrap();
        let mut conv = sample_conv("flag-write", "Flag", "real one");
        let mut synthetic = msg("u-syn", Role::User, "[CUR_SCREEN] shot", 2);
        synthetic.id = "u-syn".into();
        conv.messages.push(synthetic);
        store
            .replace_messages("flag-write", &conv.messages)
            .unwrap();

        let conn = Connection::open(dir.path().join("conversations.db")).unwrap();
        let flags: Vec<(String, i64)> = conn
            .prepare(
                "SELECT message_id, is_system_generated FROM messages WHERE conversation_id = ?1 ORDER BY position ASC",
            )
            .unwrap()
            .query_map(params!["flag-write"], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(
            flags,
            vec![
                ("msg_u1".to_string(), 0),
                ("msg_a1".to_string(), 0),
                ("u-syn".to_string(), 1)
            ]
        );
    }

    #[test]
    fn conversation_outline_lists_real_user_turns() {
        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        let mut conv = sample_conv("nav-1", "Nav", "first turn");
        conv.messages.push(msg(
            "msg_u2",
            Role::User,
            "second\n\nline with extra words for truncation padding 一二三四五六七八九十",
            1_700_000_002_000,
        ));
        conv.messages
            .push(msg("msg_a2", Role::Assistant, "ok", 1_700_000_003_000));
        conv.messages.push(msg(
            "u-syn",
            Role::User,
            "[CUR_SCREEN] shot",
            1_700_000_004_000,
        ));
        conv.messages.push(msg(
            "u-env",
            Role::User,
            "【环境反馈】本回合模型输出异常（error）。",
            1_700_000_004_500,
        ));
        let mut scoped = msg("u-scoped", Role::User, "subagent prompt", 1_700_000_005_000);
        scoped.anchor_message_id = Some("msg_u1".into());
        conv.messages.push(scoped);
        conv.messages
            .push(msg("msg_u3", Role::User, "   ", 1_700_000_006_000));
        store.sync_conversations(&[conv]).unwrap();

        let items = store
            .list_conversation_outline(&ListScope::All, "nav-1")
            .unwrap();
        assert_eq!(
            items
                .iter()
                .map(|i| i.message_id.as_str())
                .collect::<Vec<_>>(),
            vec!["msg_u1", "msg_u2", "msg_u3"]
        );
        assert_eq!(items[0].preview, "first turn");
        assert!(items[1].preview.starts_with("second line"));
        assert!(items[1].preview.chars().count() <= 36);
        assert!(items[1].preview.ends_with('…'));
        assert_eq!(items[2].preview, "（无文字）");

        let err = store
            .list_conversation_outline(&ListScope::User("other".into()), "nav-1")
            .unwrap_err();
        assert!(err.to_string().contains("not found"));
        assert!(store
            .list_conversation_outline(&ListScope::All, "")
            .unwrap()
            .is_empty());
    }

    #[test]
    fn subsequent_lead_turn_ids_skip_scoped_and_synthetic() {
        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        let mut conv = sample_conv("sub-lead", "Sub lead", "first");
        // sample_conv already has msg_u1 / msg_a1; append more turns.
        let u2 = msg("u2", Role::User, "second", 2);
        let a2 = msg("a2", Role::Assistant, "ok", 3);
        let mut scoped = msg("u-scoped", Role::User, "sub task", 4);
        scoped.anchor_message_id = Some("u2".into());
        let synthetic = msg(
            "u-syn",
            Role::User,
            "[Conversation summary (auto-compression)]\nkeep going",
            5,
        );
        let u3 = msg("u3", Role::User, "third", 6);
        conv.messages.push(u2);
        conv.messages.push(a2);
        conv.messages.push(scoped);
        conv.messages.push(synthetic);
        conv.messages.push(u3);
        store.sync_conversations(&[conv]).unwrap();

        let after_u1 = store
            .load_subsequent_lead_turn_ids("sub-lead", "msg_u1")
            .unwrap();
        assert_eq!(after_u1, vec!["u2".to_string(), "u3".to_string()]);
        let after_u2 = store.load_subsequent_lead_turn_ids("sub-lead", "u2").unwrap();
        assert_eq!(after_u2, vec!["u3".to_string()]);
        let after_u3 = store.load_subsequent_lead_turn_ids("sub-lead", "u3").unwrap();
        assert!(after_u3.is_empty());
        let missing = store
            .load_subsequent_lead_turn_ids("sub-lead", "missing")
            .unwrap();
        assert!(missing.is_empty());
    }

    #[test]
    #[ignore]
    fn profile_ui_search_real_db() {
        let _ = env_logger::builder()
            .is_test(false)
            .filter_level(log::LevelFilter::Info)
            .try_init();
        crate::conversation_store::cjk_fts::ensure_registered().unwrap();
        let path = std::env::var("POINTER_PROFILE_DB").expect("set POINTER_PROFILE_DB");
        let conn = rusqlite::Connection::open_with_flags(
            &path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap_or_else(|err| panic!("open {path}: {err}"));
        let db = crate::conversation_store::db::DbHandle::wrap_connection(conn);
        for q in ["搜索", "hello", "北京", "agent", "文件", "CWPT", "cwpt"] {
            let started = std::time::Instant::now();
            let hits = crate::conversation_store::search::search_conversations_for_ui(
                &db,
                &ListScope::All,
                q,
                50,
            )
            .unwrap();
            let match_rows: usize = hits.iter().map(|h| h.matches.len()).sum();
            eprintln!(
                "profile ui search q={q:?} hits={} match_rows={} elapsed_ms={}",
                hits.len(),
                match_rows,
                started.elapsed().as_millis()
            );
        }
    }
}
