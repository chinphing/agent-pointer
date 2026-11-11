//! 桌面 OAuth 授权页：控制面已经登录时必须直接发码回调，不再问一遍账号密码。
//!
//! 曾经的 bug：`/oauth/authorize` 无条件渲染登录表单，从不读会话 Cookie。于是
//! 员工在网页端登录着（私有仓还能用短信登录），桌面点「登录」照样弹账号密码。
//!
//! 两条契约：
//! - 带有效会话 Cookie → 303 跳回 `redirect_uri?code=…&state=…`
//! - 不带会话 → 200 渲染表单（原有行为不变）

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use pointer_admin::app;
use pointer_admin::config::AdminConfig;
use pointer_admin::db::AdminStore;
use pointer_admin::oauth::clients;
use pointer_admin::schema::DEFAULT_TENANT;
use pointer_admin::sessions;
use pointer_admin::state::{AppState, Shared};
use pointer_admin::users;
use tower::ServiceExt;

const REDIRECT: &str = "http://127.0.0.1:19427/callback";
const REDIRECT_ENCODED: &str = "http%3A%2F%2F127.0.0.1%3A19427%2Fcallback";
const CHALLENGE: &str = "abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJ";
const PASSWORD: &str = "old-password-123";

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
         &code_challenge={CHALLENGE}&code_challenge_method=S256&state=desktop-state"
    )
}

async fn get(app: &Router, uri: &str, cookie: Option<&str>) -> (StatusCode, String, Option<String>) {
    let mut req = Request::builder().method("GET").uri(uri);
    if let Some(cookie) = cookie {
        req = req.header("cookie", cookie);
    }
    let resp = app
        .clone()
        .oneshot(req.body(Body::empty()).expect("request"))
        .await
        .expect("response");
    let status = resp.status();
    let location = resp
        .headers()
        .get("location")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let bytes = axum::body::to_bytes(resp.into_body(), 256 * 1024)
        .await
        .expect("body");
    (status, String::from_utf8_lossy(&bytes).to_string(), location)
}

async fn session_cookie(state: &Shared) -> String {
    let user = users::create(&state.store, DEFAULT_TENANT, "someone", "员工", PASSWORD)
        .await
        .expect("create user");
    let token = sessions::create(&state.store, &user.id, sessions::KIND_ADMIN, 3600)
        .await
        .expect("create session");
    format!("{}={}", state.config.cookie_name, token)
}

#[tokio::test]
async fn logged_in_session_skips_the_password_form() {
    let state = state().await;
    let router = app::build_app(state.clone());
    let cookie = session_cookie(&state).await;

    let (status, body, location) = get(&router, &authorize_uri(), Some(&cookie)).await;

    assert_eq!(status, StatusCode::SEE_OTHER, "body={body}");
    let location = location.expect("redirect location");
    assert!(
        location.starts_with(&format!("{REDIRECT}?code=")),
        "location={location}"
    );
    assert!(
        location.ends_with("&state=desktop-state"),
        "state must round-trip: {location}"
    );
}

#[tokio::test]
async fn anonymous_visitor_still_gets_the_form() {
    let state = state().await;
    let router = app::build_app(state);

    let (status, body, location) = get(&router, &authorize_uri(), None).await;

    assert_eq!(status, StatusCode::OK);
    assert!(location.is_none(), "must not redirect: {location:?}");
    assert!(body.contains("name=\"password\""), "form expected");
    assert!(body.contains("name=\"username\""), "form expected");
}

#[tokio::test]
async fn expired_session_falls_back_to_the_form() {
    let state = state().await;
    let router = app::build_app(state.clone());

    let (status, body, _) = get(
        &router,
        &authorize_uri(),
        Some(&format!("{}=not-a-real-session", state.config.cookie_name)),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "body={body}");
    assert!(body.contains("name=\"password\""), "form expected");
}
