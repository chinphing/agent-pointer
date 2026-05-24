//! Integration tests for DashScope web search (Generation API for tool, Responses for research).

use pointer_core::models::{ModelSettings, ProviderConfig};
use pointer_core::tools::web_search::{
    execute_responses_web_search, execute_web_search, execute_web_search_stream, WebSearchRequest,
};
use serde_json::json;
use tokio_util::sync::CancellationToken;
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

fn dashscope_settings(api_key: &str, base_url: &str) -> ModelSettings {
    ModelSettings {
        providers: vec![ProviderConfig {
            id: "qwen".into(),
            name: "Qwen".into(),
            base_url: base_url.into(),
            api_key: api_key.into(),
            models: vec!["qwen3-max".into(), "qwen3-max-2026-01-23".into()],
            reasoning_in_messages: None,
            temperature: None,
            max_tokens: None,
            model_configs: Default::default(),
            enable_thinking: None,
            thinking_budget: None,
            reasoning_effort: None,
        }],
        ..Default::default()
    }
}

fn generation_sse_body(answer: &str) -> String {
    let sources_chunk = json!({
        "output": {
            "choices": [{ "message": { "content": "" }, "finish_reason": "null" }],
            "search_info": {
                "search_results": [{
                    "index": 1,
                    "title": "Rust Edition Guide",
                    "url": "https://doc.rust-lang.org/edition-guide/"
                }]
            }
        }
    });
    let answer_chunk = json!({
        "output": {
            "choices": [{ "message": { "content": answer }, "finish_reason": "stop" }]
        },
        "usage": {
            "input_tokens": 50,
            "output_tokens": 10,
            "total_tokens": 60,
            "plugins": { "search": { "count": 1 } }
        },
        "request_id": "req-gen-sse"
    });
    format!(
        "id:1\nevent:result\ndata:{sources_chunk}\n\nid:2\nevent:result\ndata:{answer_chunk}\n\n"
    )
}

fn generation_fixture(answer: &str) -> serde_json::Value {
    json!({
        "output": {
            "choices": [{
                "message": { "content": answer }
            }],
            "search_info": {
                "search_results": [{
                    "index": 1,
                    "title": "Rust Edition Guide",
                    "url": "https://doc.rust-lang.org/edition-guide/"
                }]
            }
        },
        "usage": {
            "input_tokens": 50,
            "output_tokens": 10,
            "total_tokens": 60,
            "plugins": { "search": { "count": 1 } }
        },
        "request_id": "req-gen-test"
    })
}

fn responses_fixture(answer: &str) -> serde_json::Value {
    json!({
        "id": "resp_test",
        "output": [
            {
                "type": "web_search_call",
                "action": {
                    "sources": [
                        { "type": "url", "url": "https://doc.rust-lang.org/edition-guide/" }
                    ]
                }
            },
            {
                "type": "message",
                "content": [{ "type": "output_text", "text": answer }]
            }
        ],
        "usage": {
            "input_tokens": 50,
            "output_tokens": 10,
            "total_tokens": 60,
            "x_tools": { "web_search": { "count": 1 } }
        }
    })
}

#[tokio::test]
async fn web_search_happy_path_parses_sources() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(generation_fixture("Rust 2024 edition is stable.")),
        )
        .mount(&server)
        .await;

    let base = format!("{}/compatible-mode/v1", server.uri());
    let settings = dashscope_settings("sk-test", &base);
    let req = WebSearchRequest {
        query: "Rust 2024 edition release".into(),
        search_strategy: "pro_max".into(),
        forced_search: false,
        enable_vertical_search: false,
        enable_thinking: false,
        messages: vec![],
    };
    let result = execute_web_search(&settings, None, req).await.unwrap();
    assert!(result.ok);
    assert_eq!(result.sources.len(), 1);
    assert_eq!(result.search_count, 1);
    assert!(result.answer.contains("2024"));
}

#[tokio::test]
async fn web_search_missing_key_returns_error() {
    let settings = dashscope_settings("", "https://dashscope.aliyuncs.com/compatible-mode/v1");
    let req = WebSearchRequest {
        query: "test".into(),
        search_strategy: "pro_max".into(),
        forced_search: false,
        enable_vertical_search: false,
        enable_thinking: false,
        messages: vec![],
    };
    assert!(execute_web_search(&settings, None, req).await.is_err());
}

#[tokio::test]
async fn web_search_http_error_surfaces_status() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(401).set_body_string("Unauthorized"))
        .mount(&server)
        .await;

    let base = format!("{}/compatible-mode/v1", server.uri());
    let settings = dashscope_settings("sk-test", &base);
    let req = WebSearchRequest {
        query: "test".into(),
        search_strategy: "pro_max".into(),
        forced_search: false,
        enable_vertical_search: false,
        enable_thinking: false,
        messages: vec![],
    };
    let err = execute_web_search(&settings, None, req).await.unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("401"));
}

#[tokio::test]
async fn web_search_generation_sse_streams_sources_then_answer() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(generation_sse_body("Rust is great.")),
        )
        .mount(&server)
        .await;

    let base = format!("{}/compatible-mode/v1", server.uri());
    let settings = dashscope_settings("sk-test", &base);
    let req = WebSearchRequest {
        query: "What is Rust?".into(),
        search_strategy: "pro_max".into(),
        forced_search: false,
        enable_vertical_search: false,
        enable_thinking: false,
        messages: vec![],
    };
    let cancel = CancellationToken::new();
    let result = execute_web_search_stream(&settings, None, req, cancel, None)
        .await
        .unwrap();
    assert!(result.ok);
    assert_eq!(result.answer, "Rust is great.");
    assert_eq!(result.sources.len(), 1);
    assert_eq!(result.request_id.as_deref(), Some("req-gen-sse"));
}

#[tokio::test]
async fn web_search_generation_stream_entry_parses_answer() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(generation_fixture("Rust is great.")))
        .mount(&server)
        .await;

    let base = format!("{}/compatible-mode/v1", server.uri());
    let settings = dashscope_settings("sk-test", &base);
    let req = WebSearchRequest {
        query: "What is Rust?".into(),
        search_strategy: "pro_max".into(),
        forced_search: false,
        enable_vertical_search: false,
        enable_thinking: false,
        messages: vec![],
    };
    let cancel = CancellationToken::new();
    let result = execute_web_search_stream(&settings, None, req, cancel, None)
        .await
        .unwrap();
    assert!(result.ok);
    assert_eq!(result.answer, "Rust is great.");
    assert_eq!(result.sources.len(), 1);
    assert_eq!(result.request_id.as_deref(), Some("req-gen-test"));
}

#[tokio::test]
async fn research_responses_path_parses_sources() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(responses_fixture("Deep research answer.")),
        )
        .mount(&server)
        .await;

    let base = format!("{}/compatible-mode/v1", server.uri());
    let settings = dashscope_settings("sk-test", &base);
    let req = WebSearchRequest {
        query: "Rust 2024 edition release".into(),
        search_strategy: "max".into(),
        forced_search: false,
        enable_vertical_search: false,
        enable_thinking: true,
        messages: vec![],
    };
    let result = execute_responses_web_search(
        &settings,
        Some("research"),
        req,
        CancellationToken::new(),
        None,
    )
    .await
    .unwrap();
    assert!(result.ok);
    assert_eq!(result.sources.len(), 1);
    assert_eq!(result.search_count, 1);
    assert!(result.answer.contains("Deep research"));
}
