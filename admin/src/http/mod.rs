//! 管理后台 HTTP API。

pub mod auth;
pub mod billing;
pub mod conversations;
pub mod error;
pub mod guard;
pub mod insight;
pub mod resources;
pub mod roles;
pub mod users;

use axum::extract::DefaultBodyLimit;
use axum::routing::{delete, get, post};
use axum::Router;

use crate::http::guard::GuardRoute;
use crate::state::Shared;

/// 路由表。鉴权在路由声明上完成（`login_only` / `require`），
/// 所以需要拿到 `Shared` 才能挂中间件。
pub fn router(state: Shared) -> Router<Shared> {
    Router::new()
        // 私有仓扩展：默认是空 Router，开源版行为不变。
        .merge(crate::ext::router(&state))
        .route("/api/admin/auth/captcha", get(auth::issue_captcha))
        .route("/api/admin/auth/login", post(auth::login))
        .route("/api/admin/auth/logout", post(auth::logout))
        .route("/api/admin/me", get(auth::me).login_only(&state))
        .route(
            "/api/admin/me/password",
            post(auth::change_my_password).login_only(&state),
        )
        .route(
            "/api/admin/users",
            get(users::list_users)
                .require(&state, "user:read")
                .post(users::create_user)
                .require(&state, "user:write"),
        )
        .route(
            "/api/admin/users/import",
            post(users::import_users).require(&state, "user:write"),
        )
        .route(
            "/api/admin/users/:id/status",
            post(users::set_user_status).require(&state, "user:write"),
        )
        .route(
            "/api/admin/users/:id/phone",
            post(users::set_user_phone).require(&state, "user:write"),
        )
        .route(
            "/api/admin/users/:id/password",
            post(users::reset_user_password).require(&state, "user:write"),
        )
        .route(
            "/api/admin/users/:id/roles",
            post(users::set_user_roles).require(&state, "user:write"),
        )
        .route(
            "/api/admin/users/:id/wallet",
            get(billing::get_wallet).require(&state, "usage:read"),
        )
        .route(
            "/api/admin/users/:id/wallet/grant",
            post(billing::grant_wallet).require(&state, "usage:write"),
        )
        .route(
            "/api/admin/model-prices",
            get(billing::list_prices)
                .require(&state, "usage:read")
                .post(billing::set_price)
                .require(&state, "usage:write"),
        )
        .route(
            "/api/admin/model-prices/base",
            axum::routing::put(billing::set_base_price).require(&state, "usage:write"),
        )
        .route(
            "/api/admin/model-prices/:model",
            axum::routing::delete(billing::clear_price).require(&state, "usage:write"),
        )
        .route("/api/admin/my-wallet", get(billing::my_wallet).login_only(&state))
        .route(
            "/api/admin/my-wallet/ledger",
            get(billing::my_ledger).login_only(&state),
        )
        .route("/api/admin/my-usage", get(insight::my_usage).login_only(&state))
        .route(
            "/api/admin/roles",
            get(roles::list_roles)
                .require(&state, "role:read")
                .post(roles::create_role)
                .require(&state, "role:write"),
        )
        .route(
            "/api/admin/roles/:id",
            axum::routing::put(roles::update_role)
                .require(&state, "role:write")
                .delete(roles::delete_role)
                .require(&state, "role:write"),
        )
        .route(
            "/api/admin/permissions",
            get(roles::list_permissions).require(&state, "role:read"),
        )
        .route(
            "/api/admin/releases",
            get(crate::releases::admin_list)
                .require(&state, "release:read")
                .post(crate::releases::admin_create).require(&state, "release:write"),
        )
        .route(
            "/api/admin/releases/:id",
            delete(crate::releases::admin_delete).require(&state, "release:write"),
        )
        .route(
            "/api/admin/releases/:id/publish",
            post(crate::releases::admin_publish).require(&state, "release:write"),
        )
        .route(
            "/api/admin/releases/:id/artifacts/reuse",
            post(crate::releases::admin_reuse_artifacts).require(&state, "release:write"),
        )
        .route(
            "/api/admin/releases/:id/artifacts",
            // 安装包动辄上百 MB，默认 2 MiB 的 body 限制会让 multipart 解析失败
            post(crate::releases::admin_upload_artifact).require(&state, "release:write")
                .layer(DefaultBodyLimit::max(crate::releases::MAX_ARTIFACT_BYTES)),
        )
        .route(
            "/api/admin/releases/:id/artifacts/:kind",
            delete(crate::releases::admin_delete_artifact).require(&state, "release:write"),
        )
        .route(
            "/api/admin/resources",
            get(resources::list_resources)
                .login_only(&state)
                .post(resources::create_resource)
                .login_only(&state),
        )
        .route(
            "/api/admin/resources/:id",
            get(resources::get_resource)
                .put(resources::update_resource)
                .delete(resources::delete_resource)
                .login_only(&state),
        )
        .route(
            "/api/admin/resources/:id/grants",
            get(resources::get_grants)
                .put(resources::replace_grants)
                .login_only(&state),
        )
        .route(
            "/api/admin/providers/test",
            post(resources::test_provider).require(&state, "provider:write"),
        )
        .route(
            "/api/admin/resources/:id/reveal",
            post(resources::reveal_secret).require(&state, "provider:reveal"),
        )
        .route(
            "/api/admin/resources/:id/versions",
            get(resources::list_versions)
                .post(resources::upload_version)
                // 技能包上限 20 MiB，默认 2 MiB 的 body 限制会直接 413
                .layer(DefaultBodyLimit::max(crate::versions::MAX_ZIP_BYTES)),
        )
        .route(
            "/api/admin/resources/:id/versions/:vid/publish",
            post(resources::publish_version),
        )
        .route(
            "/api/admin/resources/:id/versions/:vid",
            delete(resources::delete_version),
        )
        .route("/api/admin/usage", get(insight::usage).require(&state, "usage:read"))
        .route(
            "/api/admin/usage/quota/:id",
            get(insight::get_quota)
                .require(&state, "usage:read")
                .put(insight::set_quota)
                .require(&state, "usage:write"),
        )
        .route(
            "/api/admin/conversations",
            get(conversations::list).require(&state, "conversation:read"),
        )
        .route(
            "/api/admin/conversations/:conversation_id/tabs",
            get(conversations::tabs).require(&state, "conversation:read"),
        )
        .route(
            "/api/admin/threads/:thread_id/turns",
            get(conversations::turns).require(&state, "conversation:read"),
        )
        .route(
            "/api/admin/turns/:turn_id",
            get(conversations::turn).require(&state, "conversation:read"),
        )
        .route("/api/admin/audit", get(insight::audit_log).require(&state, "audit:read"))
        .route(
            "/api/admin/overview",
            get(insight::overview).require(&state, "system:read"),
        )
}
