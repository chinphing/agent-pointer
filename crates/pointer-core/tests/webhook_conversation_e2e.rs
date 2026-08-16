//! Webhook → dispatcher → run_chat conversation E2E (mock LLM).

use std::sync::Arc;
use std::time::Duration;

use pointer_core::chat_service::AppState;
use pointer_core::conversation_store::ConversationStore;
use pointer_core::dispatcher::{
    DeliverTarget, RunOutcome, TriggerMeta, TriggerRequest, TriggerSource,
};
use pointer_core::models::{ChatMessage, ProviderConfig, Role};
use pointer_core::webhook_config::WebhookTokenStore;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn chat_completion_sse_body(content: &str) -> String {
    let delta = serde_json::json!({
        "choices": [{
            "index": 0,
            "delta": { "content": content },
            "finish_reason": null
        }]
    });
    let finish = serde_json::json!({
        "choices": [{
            "index": 0,
            "delta": {},
            "finish_reason": "stop"
        }],
        "usage": { "prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15 }
    });
    format!(
        "data: {delta}\n\ndata: {finish}\n\ndata: [DONE]\n\n",
        delta = delta,
        finish = finish
    )
}

#[tokio::test]
async fn webhook_trigger_completes_assistant_reply_with_local_api_key() {
    let dir = std::env::temp_dir().join(format!("pointer-webhook-e2e-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_var("POINTER_APP_DATA_DIR", dir.to_string_lossy().as_ref());
    // Isolate from real user settings: `UserSettings::default()` carries
    // built-in providers with real base URLs/keys, and merged settings prefer
    // user-owned providers over the platform list below (so the real qwen
    // would win and the mock never gets used). An explicit empty
    // user_settings.json keeps the mock platform provider active.
    std::fs::write(
        dir.join("user_settings.json"),
        r#"{"providers":[],"activeProviderId":"qwen"}"#,
    )
    .unwrap();

    let mock = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(chat_completion_sse_body("Webhook E2E reply from mock LLM.")),
        )
        .mount(&mock)
        .await;

    let store = ConversationStore::open(dir.join("conversations.db")).unwrap();
    let token_store = WebhookTokenStore::new(&store);
    assert!(token_store
        .set_source_token("curltest", "dev-curl-test-token", None, None)
        .unwrap());

    let state = Arc::new(AppState::new());
    {
        let mut platform = state.platform_config.write();
        platform.providers = vec![ProviderConfig {
            id: "qwen".into(),
            name: "Qwen".into(),
            base_url: format!("{}/v1", mock.uri()),
            api_key: "mock-key".into(),
            models: vec!["qwen-plus".into()],
            reasoning_in_messages: None,
            temperature: None,
            max_tokens: None,
            model_configs: Default::default(),
            enable_thinking: None,
            thinking_budget: None,
            reasoning_effort: None,
            source: Some("platform".into()),
            extra_body: None,
        }];
    }

    let dispatcher = Arc::new(state.build_dispatcher());
    let conv = state
        .session_index
        .resolve_webhook_ingress_session("curltest", None)
        .unwrap();
    let req = TriggerRequest {
        run_id: None,
        idempotency_key: None,
        conversation_id: Some(conv.clone()),
        trigger_source: TriggerSource::Webhook,
        trigger_meta: TriggerMeta {
            webhook_source: Some("curltest".into()),
            ..TriggerMeta::default()
        },
        lane: None,
        messages: vec![ChatMessage::user_text("hello from webhook e2e")],
        enabled_skill_ids: vec![],
        agent_skill_overrides: std::collections::HashMap::new(),
        agent_mode: None,
        lead_agent_id: None,
        performance_mode: None,
        tool_rounds_used_single_start: 0,
        tool_rounds_used_supervisor_start: 0,
        workspace_root: String::new(),
        workspace_inherit_disabled: None,
        deliver: DeliverTarget::None,
        web_session_auth: None,
    };

    let handle = dispatcher.dispatch(req).await.unwrap();
    let outcome = tokio::time::timeout(Duration::from_secs(30), dispatcher.wait(&handle.run_id))
        .await
        .expect("webhook run timed out")
        .expect("wait failed");

    match outcome {
        RunOutcome::Finished {
            conversation_id, ..
        } => {
            assert_eq!(conversation_id, conv);
        }
        other => panic!("unexpected run outcome: {other:?}"),
    }

    let messages = state.session_index.load_messages(&conv).unwrap();
    assert!(
        messages.iter().any(|m| {
            matches!(m.role, Role::User) && m.content.contains("hello from webhook e2e")
        }),
        "user turn missing: {messages:?}"
    );
    assert!(
        messages.iter().any(|m| {
            matches!(m.role, Role::Assistant)
                && m.content.contains("Webhook E2E reply from mock LLM")
        }),
        "assistant reply missing: {messages:?}"
    );

    std::env::remove_var("POINTER_APP_DATA_DIR");
    let _ = std::fs::remove_dir_all(dir);
}
