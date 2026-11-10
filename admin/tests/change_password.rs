//! 自助改密码：`POST /api/admin/me/password`。
//!
//! 三条契约：旧密码不对不给改、新密码太短不给改、改成功后旧会话全部作废
//! 且当前会话立刻换到新 token（用户不用重新登录）。

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use axum::Router;
use pointer_admin::config::AdminConfig;
use pointer_admin::db::AdminStore;
use pointer_admin::http;
use pointer_admin::schema::DEFAULT_TENANT;
use pointer_admin::sessions;
use pointer_admin::state::{AppState, Shared};
use pointer_admin::users;
use tower::ServiceExt;

const OLD: &str = "old-password-123";

fn app(state: Shared) -> Router {
    http::router(state.clone()).with_state(state)
}

async fn state() -> Shared {
    let store = AdminStore::connect_in_memory().await.expect("store");
    AppState::new(store, AdminConfig::default())
}

/// 发 JSON 请求，返回 (状态码, 响应体, Set-Cookie)。
async fn send(
    app: &Router,
    method: Method,
    uri: &str,
    cookie: Option<&str>,
    body: Option<serde_json::Value>,
) -> (StatusCode, String, Option<String>) {
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json");
    if let Some(cookie) = cookie {
        req = req.header("cookie", cookie);
    }
    let body = match body {
        Some(v) => Body::from(v.to_string()),
        None => Body::empty(),
    };
    let resp = app
        .clone()
        .oneshot(req.body(body).expect("request"))
        .await
        .expect("response");
    let status = resp.status();
    let set_cookie = resp
        .headers()
        .get("set-cookie")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let bytes = axum::body::to_bytes(resp.into_body(), 64 * 1024)
        .await
        .expect("body");
    (status, String::from_utf8_lossy(&bytes).to_string(), set_cookie)
}

/// 建一个普通账号，返回它的 Cookie 头。
async fn account_cookie(state: &Shared) -> String {
    let user = users::create(&state.store, DEFAULT_TENANT, "someone", "普通用户", OLD)
        .await
        .expect("create user");
    let token = sessions::create(&state.store, &user.id, sessions::KIND_ADMIN, 3600)
        .await
        .expect("session");
    format!("{}={}", state.config.cookie_name, token)
}

fn change_body(old: &str, new: &str) -> serde_json::Value {
    serde_json::json!({ "old_password": old, "new_password": new })
}

#[tokio::test]
async fn rejects_new_password_that_is_too_short() {
    let state = state().await;
    let cookie = account_cookie(&state).await;
    let app = app(state);

    let (status, body, _) = send(
        &app,
        Method::POST,
        "/api/admin/me/password",
        Some(&cookie),
        Some(change_body(OLD, "short")),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.contains("至少 8 位"), "{body}");
}

#[tokio::test]
async fn rejects_wrong_old_password() {
    let state = state().await;
    let cookie = account_cookie(&state).await;
    let app = app(state);

    let (status, body, _) = send(
        &app,
        Method::POST,
        "/api/admin/me/password",
        Some(&cookie),
        Some(change_body("not-the-old-one", "brand-new-password")),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    assert!(body.contains("invalid_credentials"), "{body}");
}

#[tokio::test]
async fn rejects_reusing_the_same_password() {
    let state = state().await;
    let cookie = account_cookie(&state).await;
    let app = app(state);

    let (status, body, _) = send(
        &app,
        Method::POST,
        "/api/admin/me/password",
        Some(&cookie),
        Some(change_body(OLD, OLD)),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn rotates_every_session_and_keeps_the_caller_signed_in() {
    let state = state().await;
    let old_cookie = account_cookie(&state).await;
    let app = app(state.clone());

    // 改之前旧 cookie 是好的
    let (status, _, _) = send(&app, Method::GET, "/api/admin/me", Some(&old_cookie), None).await;
    assert_eq!(status, StatusCode::OK);

    let new_password = "brand-new-password";
    let (status, body, set_cookie) = send(
        &app,
        Method::POST,
        "/api/admin/me/password",
        Some(&old_cookie),
        Some(change_body(OLD, new_password)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let set_cookie = set_cookie.expect("改密码后必须回写新 Cookie");

    // 旧会话作废
    let (status, _, _) = send(&app, Method::GET, "/api/admin/me", Some(&old_cookie), None).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "改密码后旧会话必须失效"
    );

    // 新会话可用
    let new_cookie = set_cookie.split(';').next().unwrap_or_default().to_string();
    assert_ne!(new_cookie, old_cookie, "必须换一个新 token");
    let (status, _, _) = send(&app, Method::GET, "/api/admin/me", Some(&new_cookie), None).await;
    assert_eq!(status, StatusCode::OK, "当前会话应无缝续上");

    // 密码真的换了
    assert!(
        users::authenticate(&state.store, DEFAULT_TENANT, "someone", OLD)
            .await
            .expect("auth")
            .is_none(),
        "旧密码必须失效"
    );
    assert!(
        users::authenticate(&state.store, DEFAULT_TENANT, "someone", new_password)
            .await
            .expect("auth")
            .is_some(),
        "新密码必须能登录"
    );
}
