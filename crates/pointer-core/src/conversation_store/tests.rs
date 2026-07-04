#[cfg(test)]
mod tests {
    use crate::conversation_store::ConversationStore;
    use crate::conversation_store::persist::sample_conv;
    use serde_json::{json, Value};
    use tempfile::TempDir;

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
    fn discover_cjk_bigram() {
        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        store
            .sync_conversations(&vec![sample_conv(
                "c1",
                "认证",
                "我们需要重构认证中间件",
            )])
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
                msg(
                    "msg_a1",
                    Role::Assistant,
                    "收到图片了",
                    1_700_000_001_000,
                ),
            ],
            skill_ids: vec![],
            tool_rounds_used: 0,
            tool_rounds_used_supervisor: 0,
            computer_monitor_id: None,
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
        assert_eq!(loaded.active_conversation_id.as_deref(), Some(format!("{base}@s2").as_str()));
        assert_eq!(loaded.lead_agent_id, "coder");
    }

    fn conv_with(id: &str, title: &str, updated_at: i64, workspace_root: &str) -> crate::models::Conversation {
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
}
