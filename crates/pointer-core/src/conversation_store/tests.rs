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
}
