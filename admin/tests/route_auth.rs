//! 路由鉴权覆盖：`/api/admin` 下任何路由都必须要求登录，且除少数「登录即可」
//! 的接口外都必须要求权限。
//!
//! 这里不维护手写路由清单——直接从 `http/mod.rs` 的 `.route("…")` 字面量里扫，
//! 所以**新增路由忘记加鉴权会自动红灯**。releases 那 6 个端点就是这样漏掉的。

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use axum::Router;
use pointer_admin::config::AdminConfig;
use pointer_admin::db::AdminStore;
use pointer_admin::http;
use pointer_admin::rbac;
use pointer_admin::schema::DEFAULT_TENANT;
use pointer_admin::sessions;
use pointer_admin::state::{AppState, Shared};
use pointer_admin::users;
use tower::ServiceExt;

/// 完全不需要登录的路由（验证码、登录、登出）。
const PUBLIC: &[&str] = &[
    "/api/admin/auth/captcha",
    "/api/admin/auth/login",
    "/api/admin/auth/logout",
];

/// 登录即可、不需要额外权限的路由（个人中心）。
const LOGIN_ONLY: &[&str] = &[
    "/api/admin/me",
    "/api/admin/my-wallet/ledger",
    // 自己的用量：普通成员没有任何权限，但看得见自己的账
    "/api/admin/my-usage",
];

/// 走 Bearer access token 的客户端接口：不接受 Cookie 会话，
/// 未登录时仍然 401，所以只从「登录即可」里排除，不排除鉴权断言。
const BEARER_ONLY: &[&str] = &["/api/admin/my-wallet"];

/// 自带权限过滤的列表：不带 `kind` 时只返回调用者有权限看的资源类型，
/// 零权限用户得到空列表（200 + `items: []`），不带 `kind` 就不越权。
const SELF_FILTERED: &[&str] = &["/api/admin/resources"];

/// 零权限用户必须被 403 挡住的代表性读接口（都走 GET，能真正进到 handler）。
const MUST_FORBID: &[&str] = &[
    "/api/admin/users",
    "/api/admin/roles",
    "/api/admin/permissions",
    "/api/admin/model-prices",
    "/api/admin/releases",
    "/api/admin/usage",
    "/api/admin/audit",
    "/api/admin/overview",
];

fn methods() -> [Method; 4] {
    [Method::GET, Method::POST, Method::PUT, Method::DELETE]
}

/// 从 `http/mod.rs` 里扫出所有 `/api/admin` 路由字面量。
fn admin_route_paths() -> Vec<String> {
    let src = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/http/mod.rs"),
    )
    .expect("读不到 http/mod.rs");

    let mut paths = Vec::new();
    let mut rest = src.as_str();
    while let Some(pos) = rest.find(".route(") {
        rest = &rest[pos + ".route(".len()..];
        let Some(open) = rest.find('"') else { break };
        let after = &rest[open + 1..];
        let Some(close) = after.find('"') else { break };
        let path = &after[..close];
        if path.starts_with("/api/admin") {
            paths.push(path.to_string());
        }
    }
    paths
}

/// `/api/admin/users/:id/roles` → `/api/admin/users/x/roles`
fn concrete(path: &str) -> String {
    path.split('/')
        .map(|seg| if seg.starts_with(':') { "x" } else { seg })
        .collect::<Vec<_>>()
        .join("/")
}

fn app(state: Shared) -> Router {
    http::router(state.clone()).with_state(state)
}

async fn state() -> Shared {
    let store = AdminStore::connect_in_memory().await.expect("store");
    AppState::new(store, AdminConfig::default())
}

/// 发请求，返回状态码与响应体文本。
async fn send(app: &Router, method: &Method, uri: &str, cookie: Option<&str>) -> (StatusCode, String) {
    let mut req = Request::builder().method(method.clone()).uri(uri);
    if let Some(value) = cookie {
        req = req.header("cookie", value);
    }
    let resp = app
        .clone()
        .oneshot(req.body(Body::empty()).expect("request"))
        .await
        .expect("response");
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 64 * 1024)
        .await
        .expect("body");
    (status, String::from_utf8_lossy(&bytes).to_string())
}


/// 建一个超管账号并返回它的 Cookie 头。
async fn super_admin_cookie(state: &Shared) -> String {
    rbac::seed_builtin_roles(&state.store, DEFAULT_TENANT)
        .await
        .expect("seed roles");
    let user = users::create(&state.store, DEFAULT_TENANT, "root", "超管", "pw-123456")
        .await
        .expect("create user");
    rbac::replace_roles(&state.store, DEFAULT_TENANT, &user.id, &[rbac::SUPER_ADMIN])
        .await
        .expect("assign super admin");
    let token = sessions::create(&state.store, &user.id, sessions::KIND_ADMIN, 3600)
        .await
        .expect("session");
    format!("{}={}", state.config.cookie_name, token)
}

/// 建一个零权限账号并返回它的 Cookie 头。
async fn zero_permission_cookie(state: &Shared) -> String {
    let user = users::create(&state.store, DEFAULT_TENANT, "nobody", "零权限", "pw-123456")
        .await
        .expect("create user");
    let token = sessions::create(&state.store, &user.id, sessions::KIND_ADMIN, 3600)
        .await
        .expect("session");
    format!("{}={}", state.config.cookie_name, token)
}

#[tokio::test]
async fn scanner_finds_the_route_table() {
    let paths = admin_route_paths();
    // 解析器坏掉时这里会先炸，而不是让下面几个测试空转通过。
    assert!(
        paths.len() >= 25,
        "只扫到 {} 条 /api/admin 路由，解析可能失效",
        paths.len()
    );
    assert!(paths.iter().any(|p| p == "/api/admin/releases"));
}

#[tokio::test]
async fn no_admin_route_answers_anonymous() {
    let state = state().await;
    let app = app(state);
    for path in admin_route_paths() {
        if PUBLIC.contains(&path.as_str()) {
            continue;
        }
        let uri = concrete(&path);
        for method in methods() {
            let (status, _) = send(&app, &method, &uri, None).await;
            assert!(
                !status.is_success(),
                "{method} {uri} 未登录竟然成功：{status}"
            );
        }
    }
}

#[tokio::test]
async fn zero_permission_user_is_denied_everywhere() {
    let state = state().await;
    let cookie = zero_permission_cookie(&state).await;
    let app = app(state);

    for path in admin_route_paths() {
        if PUBLIC.contains(&path.as_str())
            || LOGIN_ONLY.contains(&path.as_str())
            || BEARER_ONLY.contains(&path.as_str())
            || SELF_FILTERED.contains(&path.as_str())
        {
            continue;
        }
        let uri = concrete(&path);
        for method in methods() {
            let (status, _) = send(&app, &method, &uri, Some(&cookie)).await;
            assert!(
                !status.is_success(),
                "{method} {uri} 零权限用户竟然成功：{status}"
            );
        }
    }
}

#[tokio::test]
async fn zero_permission_user_gets_403_on_read_endpoints() {
    let state = state().await;
    let cookie = zero_permission_cookie(&state).await;
    let app = app(state);

    for uri in MUST_FORBID {
        let (status, _) = send(&app, &Method::GET, uri, Some(&cookie)).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{uri} 应该 403");
    }
}

#[tokio::test]
async fn login_only_endpoints_work_for_any_signed_in_user() {
    let state = state().await;
    let cookie = zero_permission_cookie(&state).await;
    let app = app(state);

    for uri in LOGIN_ONLY {
        let (status, _) = send(&app, &Method::GET, uri, Some(&cookie)).await;
        assert_eq!(status, StatusCode::OK, "{uri} 登录即可访问");
    }
}

/// 资源列表不带 `kind` 时按权限过滤：零权限用户拿到空列表，不是 403 也不是全量。
#[tokio::test]
async fn resources_list_filters_by_permission() {
    let state = state().await;
    let cookie = zero_permission_cookie(&state).await;
    let app = app(state);

    let (status, body) = send(&app, &Method::GET, "/api/admin/resources", Some(&cookie)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body.contains(r#""items":[]"#),
        "零权限用户不应看到任何资源：{body}"
    );

    // 指定 kind 时要走权限判定。
    let (status, _) = send(
        &app,
        &Method::GET,
        "/api/admin/resources?kind=model",
        Some(&cookie),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "带 kind 必须校验权限");
}

/// Bearer 接口不吃 Cookie 会话，但仍然必须挡住匿名访问。
#[tokio::test]
async fn bearer_endpoints_still_reject_anonymous() {
    let state = state().await;
    let app = app(state);
    for uri in BEARER_ONLY {
        let (status, _) = send(&app, &Method::GET, uri, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{uri} 匿名应 401");
    }
}

/// 超管不该被任何权限检查挡住。
///
/// 零权限测试只能证明「不该进的进不去」：权限键拼错（写成 `release:reed`）时，
/// 零权限用户照样 403、测试照样绿，而真正该进的人被挡在门外。这条补上那个盲区。
#[tokio::test]
async fn super_admin_is_never_blocked_by_permission_checks() {
    let state = state().await;
    let cookie = super_admin_cookie(&state).await;
    let app = app(state);

    for path in admin_route_paths() {
        // 走 Bearer 的接口不收 Cookie，单独在 bearer_endpoints_still_reject_anonymous 里管
        if PUBLIC.contains(&path.as_str()) || BEARER_ONLY.contains(&path.as_str()) {
            continue;
        }
        let uri = concrete(&path);
        let (status, body) = send(&app, &Method::GET, &uri, Some(&cookie)).await;
        assert!(
            !matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN),
            "GET {uri} 超管被拒：{status} {body}"
        );
    }
}
