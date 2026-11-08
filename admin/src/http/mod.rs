//! 管理后台 HTTP API。

pub mod auth;
pub mod error;
pub mod insight;
pub mod resources;
pub mod roles;
pub mod users;

use axum::extract::DefaultBodyLimit;
use axum::routing::{delete, get, post};
use axum::Router;

use crate::state::Shared;

pub fn router() -> Router<Shared> {
    Router::new()
        .route("/api/admin/auth/captcha", get(auth::issue_captcha))
        .route("/api/admin/auth/login", post(auth::login))
        .route("/api/admin/auth/logout", post(auth::logout))
        .route("/api/admin/me", get(auth::me))
        .route(
            "/api/admin/users",
            get(users::list_users).post(users::create_user),
        )
        .route("/api/admin/users/import", post(users::import_users))
        .route("/api/admin/users/:id/status", post(users::set_user_status))
        .route(
            "/api/admin/users/:id/password",
            post(users::reset_user_password),
        )
        .route("/api/admin/users/:id/roles", post(users::set_user_roles))
        .route(
            "/api/admin/roles",
            get(roles::list_roles).post(roles::create_role),
        )
        .route(
            "/api/admin/roles/:id",
            axum::routing::put(roles::update_role).delete(roles::delete_role),
        )
        .route("/api/admin/permissions", get(roles::list_permissions))
        .route(
            "/api/admin/resources",
            get(resources::list_resources).post(resources::create_resource),
        )
        .route(
            "/api/admin/resources/:id",
            get(resources::get_resource)
                .put(resources::update_resource)
                .delete(resources::delete_resource),
        )
        .route(
            "/api/admin/resources/:id/grants",
            get(resources::get_grants).put(resources::replace_grants),
        )
        .route(
            "/api/admin/providers/test",
            post(resources::test_provider),
        )
        .route(
            "/api/admin/resources/:id/reveal",
            post(resources::reveal_secret),
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
        .route("/api/admin/usage", get(insight::usage))
        .route(
            "/api/admin/usage/quota/:id",
            get(insight::get_quota).put(insight::set_quota),
        )
        .route("/api/admin/audit", get(insight::audit_log))
        .route("/api/admin/overview", get(insight::overview))
}
