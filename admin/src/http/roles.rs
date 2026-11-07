//! 角色与权限清单（只读）。
//!
//! 内置角色是代码常量，M0 不提供「自定义角色」的写接口——
//! 新增角色会牵出权限继承与降级语义，留到有真实需求时再做。

use axum::extract::State;
use axum::http::HeaderMap;
use axum::Json;
use serde_json::{json, Value};
use sqlx::{Any, Row};

use crate::http::auth::current_auth;
use crate::http::error::ApiResult;
use crate::rbac;
use crate::state::Shared;

pub async fn list_roles(
    State(state): State<Shared>,
    headers: HeaderMap,
) -> ApiResult<Json<Value>> {
    let auth = current_auth(&state, &headers).await?;
    auth.require("role:read")?;

    let sql = state.store.sql(
        "SELECT r.role_key, COUNT(ur.user_id) FROM roles r
         LEFT JOIN user_roles ur ON ur.role_id = r.id
         WHERE r.tenant_id = ? GROUP BY r.role_key",
    );
    let rows = sqlx::query::<Any>(&sql)
        .bind(&state.config.tenant_id)
        .fetch_all(state.store.pool())
        .await?;
    let mut counts = std::collections::HashMap::new();
    for row in &rows {
        let key: String = row.try_get(0)?;
        let count: i64 = row.try_get(1)?;
        counts.insert(key, count);
    }

    let items: Vec<Value> = rbac::BUILTIN_ROLES
        .iter()
        .map(|role| {
            let permissions: Vec<&str> = if role.key == rbac::SUPER_ADMIN {
                rbac::ALL_PERMISSIONS.to_vec()
            } else {
                rbac::builtin_permissions(role.key)
            };
            json!({
                "key": role.key,
                "name": role.name,
                "permissions": permissions,
                "memberCount": counts.get(role.key).copied().unwrap_or(0),
            })
        })
        .collect();

    Ok(Json(json!({ "items": items, "total": items.len() })))
}

pub async fn list_permissions(
    State(state): State<Shared>,
    headers: HeaderMap,
) -> ApiResult<Json<Value>> {
    let auth = current_auth(&state, &headers).await?;
    auth.require("role:read")?;
    Ok(Json(json!({
        "items": rbac::ALL_PERMISSIONS,
        "total": rbac::ALL_PERMISSIONS.len(),
    })))
}
