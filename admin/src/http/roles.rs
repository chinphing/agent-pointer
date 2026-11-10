//! 角色与权限。
//!
//! 内置角色是代码常量，不可改（权限随版本升级一起变）；
//! 自定义角色存在 `roles` / `role_permissions` 两张表里，可增删改。
//! 权限判定只认 `role_permissions`，所以自定义角色天然生效。

use axum::extract::{Path, State};
use serde::Deserialize;
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
        "SELECT r.id, r.role_key, COUNT(ur.user_id) FROM roles r
         LEFT JOIN user_roles ur ON ur.role_id = r.id
         WHERE r.tenant_id = ? GROUP BY r.id, r.role_key",
    );
    let rows = sqlx::query::<Any>(&sql)
        .bind(&state.config.tenant_id)
        .fetch_all(state.store.pool())
        .await?;
    let mut counts = std::collections::HashMap::new();
    let mut ids = std::collections::HashMap::new();
    for row in &rows {
        let id: String = row.try_get(0)?;
        let key: String = row.try_get(1)?;
        let count: i64 = row.try_get(2)?;
        counts.insert(key.clone(), count);
        ids.insert(key, id);
    }

    let mut items: Vec<Value> = rbac::BUILTIN_ROLES
        .iter()
        .map(|role| {
            let permissions: Vec<String> = if role.key == rbac::SUPER_ADMIN {
                rbac::all_permissions()
            } else {
                rbac::builtin_permissions(role.key)
            };
            json!({
                "id": ids.get(role.key).cloned().unwrap_or_default(),
                "key": role.key,
                "name": role.name,
                "permissions": permissions,
                "memberCount": counts.get(role.key).copied().unwrap_or(0),
            })
        })
        .collect();

    // 自定义角色：从库里读，权限取 role_permissions。
    let sql = state.store.sql(
        "SELECT id, role_key, name FROM roles WHERE tenant_id = ? AND builtin = 0 ORDER BY created_at",
    );
    let custom = sqlx::query::<Any>(&sql)
        .bind(&state.config.tenant_id)
        .fetch_all(state.store.pool())
        .await?;
    for row in &custom {
        let id: String = row.try_get(0)?;
        let key: String = row.try_get(1)?;
        let name: String = row.try_get(2)?;
        let perms = rbac::permissions_for_role(&state.store, &id).await?;
        items.push(json!({
            "id": id,
            "key": key,
            "name": name,
            "builtin": false,
            "permissions": perms,
            "memberCount": counts.get(&key).copied().unwrap_or(0),
        }));
    }

    Ok(Json(json!({ "items": items, "total": items.len() })))
}

pub async fn list_permissions(
    State(state): State<Shared>,
    headers: HeaderMap,
) -> ApiResult<Json<Value>> {
    let auth = current_auth(&state, &headers).await?;
    auth.require("role:read")?;
    let items = rbac::all_permissions();
    Ok(Json(json!({
        "total": items.len(),
        "items": items,
    })))
}

#[derive(Debug, Deserialize)]
pub struct RoleBody {
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub permissions: Vec<String>,
}

fn validate_permissions(perms: &[String]) -> ApiResult<()> {
    let known = rbac::all_permissions();
    for p in perms {
        if !known.contains(p) {
            return Err(crate::http::error::ApiError::BadRequest(format!(
                "未知权限：{p}"
            )));
        }
    }
    Ok(())
}

/// 自定义角色的 key 只允许小写字母、数字、下划线、短横线。
fn normalize_role_key(raw: &str) -> ApiResult<String> {
    let key = raw.trim().to_lowercase();
    if key.len() < 2 || key.len() > 32 {
        return Err(crate::http::error::ApiError::BadRequest(
            "角色标识长度需在 2–32 之间".into(),
        ));
    }
    if !key
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
    {
        return Err(crate::http::error::ApiError::BadRequest(
            "角色标识只能包含小写字母、数字、下划线和短横线".into(),
        ));
    }
    if rbac::BUILTIN_ROLES.iter().any(|r| r.key == key) {
        return Err(crate::http::error::ApiError::BadRequest(
            "内置角色标识不可占用".into(),
        ));
    }
    Ok(key)
}

pub async fn create_role(
    State(state): State<Shared>,
    headers: HeaderMap,
    Json(body): Json<RoleBody>,
) -> ApiResult<Json<Value>> {
    let auth = current_auth(&state, &headers).await?;
    auth.require("role:write")?;

    let key = normalize_role_key(&body.key)?;
    let name = body.name.trim();
    if name.is_empty() {
        return Err(crate::http::error::ApiError::BadRequest(
            "角色名称不能为空".into(),
        ));
    }
    validate_permissions(&body.permissions)?;

    let id = crate::db::random_id();
    let now = crate::db::now_ts();
    let sql = state.store.sql(
        "INSERT INTO roles (id, tenant_id, role_key, name, builtin, created_at) \
         VALUES (?, ?, ?, ?, 0, ?)",
    );
    if sqlx::query::<Any>(&sql)
        .bind(&id)
        .bind(&state.config.tenant_id)
        .bind(&key)
        .bind(name)
        .bind(now)
        .execute(state.store.pool())
        .await
        .is_err()
    {
        return Err(crate::http::error::ApiError::BadRequest(
            "角色标识已存在".into(),
        ));
    }
    rbac::replace_role_permissions(&state.store, &id, &body.permissions).await?;

    crate::audit::record(
        &state.store,
        &state.config.tenant_id,
        crate::audit::AuditInput {
            actor_user_id: Some(&auth.user.id),
            actor_name: &auth.user.display_name,
            action: "role.create",
            target_type: "role",
            target_id: &id,
            before_json: None,
            after_json: Some(&json!({"key": key, "permissions": body.permissions}).to_string()),
        },
    )
    .await?;

    Ok(Json(json!({ "id": id, "key": key, "name": name })))
}

/// 只允许改自定义角色：内置角色的权限随版本走，改了会被下次升级覆盖。
async fn load_custom_role(state: &Shared, id: &str) -> ApiResult<String> {
    let sql = state
        .store
        .sql("SELECT role_key, builtin FROM roles WHERE id = ? AND tenant_id = ?");
    let row = sqlx::query::<Any>(&sql)
        .bind(id)
        .bind(&state.config.tenant_id)
        .fetch_optional(state.store.pool())
        .await?;
    let Some(row) = row else {
        return Err(crate::http::error::ApiError::NotFound("角色不存在".into()));
    };
    let builtin: i64 = row.try_get(1)?;
    if builtin != 0 {
        return Err(crate::http::error::ApiError::BadRequest(
            "内置角色不可修改".into(),
        ));
    }
    Ok(row.try_get(0)?)
}

pub async fn update_role(
    State(state): State<Shared>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<RoleBody>,
) -> ApiResult<Json<Value>> {
    let auth = current_auth(&state, &headers).await?;
    auth.require("role:write")?;
    let key = load_custom_role(&state, &id).await?;
    validate_permissions(&body.permissions)?;

    if !body.name.trim().is_empty() {
        let sql = state.store.sql("UPDATE roles SET name = ? WHERE id = ?");
        sqlx::query::<Any>(&sql)
            .bind(body.name.trim())
            .bind(&id)
            .execute(state.store.pool())
            .await?;
    }
    rbac::replace_role_permissions(&state.store, &id, &body.permissions).await?;

    crate::audit::record(
        &state.store,
        &state.config.tenant_id,
        crate::audit::AuditInput {
            actor_user_id: Some(&auth.user.id),
            actor_name: &auth.user.display_name,
            action: "role.update",
            target_type: "role",
            target_id: &id,
            before_json: None,
            after_json: Some(&json!({"key": key, "permissions": body.permissions}).to_string()),
        },
    )
    .await?;

    Ok(Json(json!({ "ok": true })))
}

pub async fn delete_role(
    State(state): State<Shared>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let auth = current_auth(&state, &headers).await?;
    auth.require("role:write")?;
    let key = load_custom_role(&state, &id).await?;

    let sql = state.store.sql("SELECT COUNT(*) FROM user_roles WHERE role_id = ?");
    let used: i64 = sqlx::query::<Any>(&sql)
        .bind(&id)
        .fetch_one(state.store.pool())
        .await?
        .try_get(0)?;
    if used > 0 {
        return Err(crate::http::error::ApiError::BadRequest(format!(
            "还有 {used} 个成员在用这个角色，先改掉他们的角色再删"
        )));
    }

    let sql = state.store.sql("DELETE FROM roles WHERE id = ?");
    sqlx::query::<Any>(&sql)
        .bind(&id)
        .execute(state.store.pool())
        .await?;

    crate::audit::record(
        &state.store,
        &state.config.tenant_id,
        crate::audit::AuditInput {
            actor_user_id: Some(&auth.user.id),
            actor_name: &auth.user.display_name,
            action: "role.delete",
            target_type: "role",
            target_id: &id,
            before_json: Some(&json!({"key": key}).to_string()),
            after_json: None,
        },
    )
    .await?;

    Ok(Json(json!({ "ok": true })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_key_is_normalized_and_validated() {
        assert_eq!(normalize_role_key(" Auditor ").unwrap(), "auditor");
        assert!(normalize_role_key("a").is_err(), "太短要拒绝");
        assert!(normalize_role_key("财务").is_err(), "非 ASCII 要拒绝");
        assert!(normalize_role_key("aud itor").is_err(), "空格要拒绝");
        assert!(normalize_role_key("auditor!").is_err());
    }

    #[test]
    fn builtin_role_keys_cannot_be_taken() {
        for r in rbac::BUILTIN_ROLES {
            assert!(
                normalize_role_key(r.key).is_err(),
                "内置 key {} 不该被自定义角色占用",
                r.key
            );
        }
    }

    #[test]
    fn unknown_permissions_are_rejected() {
        assert!(validate_permissions(&["audit:read".to_string()]).is_ok());
        assert!(validate_permissions(&["audit:superuser".to_string()]).is_err());
        assert!(validate_permissions(&["nope".to_string()]).is_err());
    }
}
