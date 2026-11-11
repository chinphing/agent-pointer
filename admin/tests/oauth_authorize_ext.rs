//! 授权页的私有登录块注入（`Ext::authorize_login_html`）。
//!
//! 单独一个测试文件：`ext::install` 是进程级 `OnceLock`，装一次就全局生效，
//! 和别的用例放在一起会互相污染。

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use pointer_admin::app;
use pointer_admin::config::AdminConfig;
use pointer_admin::db::AdminStore;
use pointer_admin::ext::{self, Ext, LoginMethod};
use pointer_admin::oauth::clients;
use pointer_admin::state::{AppState, Shared};
use tower::ServiceExt;

const REDIRECT: &str = "http://127.0.0.1:19427/callback";
const REDIRECT_ENCODED: &str = "http%3A%2F%2F127.0.0.1%3A19427%2Fcallback";
const CHALLENGE: &str = "abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJ";
const BLOCK: &str = "<div id=\"sms-login\">短信登录</div>";

struct SmsExt;

impl Ext for SmsExt {
    fn login_methods(&self) -> Vec<LoginMethod> {
        vec![
            LoginMethod {
                key: "sms".to_string(),
                label: "短信登录".to_string(),
                html: BLOCK.to_string(),
            },
            LoginMethod {
                key: "sso".to_string(),
                label: "企业微信".to_string(),
                html: "<div id=\"sso-login\">企业微信</div>".to_string(),
            },
        ]
    }
}

async fn state() -> Shared {
    let store = AdminStore::connect_in_memory().await.expect("store");
    let mut config = AdminConfig::default();
    config.oauth_secret = Some("test-oauth-secret".to_string());
    let state = AppState::new(store, config);
    clients::upsert(
        &state.store,
        "pointer-desktop",
        "client-secret",
        "public",
        &[REDIRECT],
    )
    .await
    .expect("register oauth client");
    state
}

fn authorize_uri() -> String {
    format!(
        "/oauth/authorize?client_id=pointer-desktop&redirect_uri={REDIRECT_ENCODED}\
         &code_challenge={CHALLENGE}&code_challenge_method=S256&state=x"
    )
}

async fn get(app: &Router, uri: &str) -> (StatusCode, String) {
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(uri)
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 256 * 1024)
        .await
        .expect("body");
    (status, String::from_utf8_lossy(&bytes).to_string())
}

#[tokio::test]
async fn ext_methods_become_tabs_with_the_first_one_active() {
    ext::install(Box::new(SmsExt)).expect("install ext once");
    let state = state().await;
    let router = app::build_app(state);

    let (status, body) = get(&router, &authorize_uri()).await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains(BLOCK), "sms form missing");
    // 扩展声明的两种 + 内置账号密码 = 三个标签
    assert!(body.contains("data-tab=\"sms\">短信登录"), "sms tab missing");
    assert!(body.contains("data-tab=\"sso\">企业微信"), "second method tab missing");
    assert!(body.contains("data-tab=\"password\">账号密码"), "password tab missing");
    // 只有第一个是 active，其余面板默认隐藏
    assert!(body.contains("class=\"tab active\" data-tab=\"sms\""), "first method must be active");
    assert!(body.contains("<section id=\"pane-sms\">"), "first pane visible");
    assert!(body.contains("<section id=\"pane-sso\" hidden>"), "second pane hidden");
    assert!(body.contains("id=\"sso-login\""), "second method html must render");
    // 同一时刻只显示一种：密码面板默认 hidden，但内容仍在（切过去就能用）
    assert!(body.contains("<div id=\"pane-password\" hidden>"), "password pane must start hidden");
    assert!(body.contains("name=\"password\""), "password fallback must stay available");
}
