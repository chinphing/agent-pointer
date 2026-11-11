//! 权限清单、内置角色与判定。
//!
//! 权限串形如 `<domain>:<action>`；domain 与资源 `kind` 对应。
//! 清单写在代码里，库里只存「角色 → 权限」的授权关系，加权限不用迁移。

use anyhow::{anyhow, Result};
use sqlx::{Any, Row};
use std::collections::HashSet;

use crate::db::{now_ts, random_id, AdminStore};

pub const ALL_PERMISSIONS: &[&str] = &[
    "user:read",
    "user:write",
    "role:read",
    "role:write",
    "provider:read",
    "provider:write",
    "provider:delete",
    "provider:reveal",
    "model:read",
    "model:write",
    "model:delete",
    "skill:read",
    "skill:write",
    "skill:publish",
    "skill:delete",
    "plugin:read",
    "plugin:write",
    "plugin:publish",
    "plugin:delete",
    "mcp:read",
    "mcp:write",
    "usage:read",
    "usage:write",
    "release:read",
    "release:write",
    "audit:read",
    "system:read",
];

/// 全部权限 = 内置清单 + 私有仓扩展。
///
/// 私有仓通过 `ext::extra_permissions()` 追加自己的键（如 `payment:read`），
/// 内置的 `admin` / `viewer` 会自动继承。
pub fn all_permissions() -> Vec<String> {
    let mut out: Vec<String> = ALL_PERMISSIONS.iter().map(|p| (*p).to_string()).collect();
    out.extend(crate::ext::extra_permissions());
    out
}

pub const SUPER_ADMIN: &str = "super_admin";
pub const ROLE_ADMIN: &str = "admin";
pub const ROLE_VIEWER: &str = "viewer";
pub const ROLE_MEMBER: &str = "member";

pub struct BuiltinRole {
    pub key: &'static str,
    pub name: &'static str,
}

/// 内置角色不可删除、权限固定；需要差异请复制成自定义角色。
pub const BUILTIN_ROLES: &[BuiltinRole] = &[
    BuiltinRole {
        key: SUPER_ADMIN,
        name: "超级管理员",
    },
    BuiltinRole {
        key: "admin",
        name: "管理员",
    },
    BuiltinRole {
        key: "viewer",
        name: "运营（只读）",
    },
    BuiltinRole {
        key: "member",
        name: "成员",
    },
];

/// 内置角色的默认权限。`super_admin` 隐式拥有全部权限，这里返回空。
pub fn builtin_permissions(key: &str) -> Vec<String> {
    match key {
        "admin" => all_permissions()
            .into_iter()
            .filter(|p| p != "role:write" && p != "provider:reveal")
            .collect(),
        "viewer" => all_permissions()
            .into_iter()
            .filter(|p| p.ends_with(":read"))
            .collect(),
        _ => Vec::new(),
    }
}

pub fn is_builtin(key: &str) -> bool {
    BUILTIN_ROLES.iter().any(|r| r.key == key)
}

pub async fn seed_builtin_roles(store: &AdminStore, tenant_id: &str) -> Result<()> {
    for role in BUILTIN_ROLES {
        let role_id = ensure_role(store, tenant_id, role.key, role.name, true).await?;
        for perm in builtin_permissions(role.key) {
            if permission_exists(store, &role_id, &perm).await? {
                continue;
            }
            let sql = store.sql("INSERT INTO role_permissions (role_id, permission) VALUES (?, ?)");
            sqlx::query::<Any>(&sql)
                .bind(&role_id)
                .bind(perm)
                .execute(store.pool())
                .await?;
        }
    }
    Ok(())
}

async fn permission_exists(store: &AdminStore, role_id: &str, perm: &str) -> Result<bool> {
    let sql = store.sql("SELECT role_id FROM role_permissions WHERE role_id = ? AND permission = ?");
    let row = sqlx::query::<Any>(&sql)
        .bind(role_id)
        .bind(perm)
        .fetch_optional(store.pool())
        .await?;
    Ok(row.is_some())
}

pub async fn ensure_role(
    store: &AdminStore,
    tenant_id: &str,
    key: &str,
    name: &str,
    builtin: bool,
) -> Result<String> {
    if let Some(id) = find_role_id(store, tenant_id, key).await? {
        return Ok(id);
    }
    let id = random_id();
    let sql = store.sql(
        "INSERT INTO roles (id, tenant_id, role_key, name, builtin, created_at)
         VALUES (?, ?, ?, ?, ?, ?)",
    );
    sqlx::query::<Any>(&sql)
        .bind(&id)
        .bind(tenant_id)
        .bind(key)
        .bind(name)
        .bind(if builtin { 1i64 } else { 0 })
        .bind(now_ts())
        .execute(store.pool())
        .await?;
    Ok(id)
}

pub async fn find_role_id(
    store: &AdminStore,
    tenant_id: &str,
    key: &str,
) -> Result<Option<String>> {
    let sql = store.sql("SELECT id FROM roles WHERE tenant_id = ? AND role_key = ?");
    let row = sqlx::query::<Any>(&sql)
        .bind(tenant_id)
        .bind(key)
        .fetch_optional(store.pool())
        .await?;
    match row {
        Some(row) => Ok(Some(row.try_get(0)?)),
        None => Ok(None),
    }
}

pub async fn role_keys_for_user(store: &AdminStore, user_id: &str) -> Result<Vec<String>> {
    let sql = store.sql(
        "SELECT r.role_key FROM user_roles ur
         JOIN roles r ON r.id = ur.role_id
         WHERE ur.user_id = ?
         ORDER BY r.role_key",
    );
    let rows = sqlx::query::<Any>(&sql)
        .bind(user_id)
        .fetch_all(store.pool())
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(row.try_get::<String, _>(0)?);
    }
    Ok(out)
}

/// 用户的有效权限集。命中 `super_admin` 直接返回全部。
/// 某个角色的权限（自定义角色用）。
pub async fn permissions_for_role(store: &AdminStore, role_id: &str) -> Result<Vec<String>> {
    let sql = store.sql("SELECT permission FROM role_permissions WHERE role_id = ?");
    let rows = sqlx::query::<Any>(&sql)
        .bind(role_id)
        .fetch_all(store.pool())
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(row.try_get::<String, _>(0)?);
    }
    Ok(out)
}

/// 整体替换某角色的权限集合（先删后插，语义是「这就是全集」）。
pub async fn replace_role_permissions(
    store: &AdminStore,
    role_id: &str,
    permissions: &[String],
) -> Result<()> {
    let sql = store.sql("DELETE FROM role_permissions WHERE role_id = ?");
    sqlx::query::<Any>(&sql)
        .bind(role_id)
        .execute(store.pool())
        .await?;
    for perm in permissions {
        let sql = store.sql("INSERT INTO role_permissions (role_id, permission) VALUES (?, ?)");
        sqlx::query::<Any>(&sql)
            .bind(role_id)
            .bind(perm)
            .execute(store.pool())
            .await?;
    }
    Ok(())
}

pub async fn permissions_for_user(
    store: &AdminStore,
    user_id: &str,
) -> Result<HashSet<String>> {
    let keys = role_keys_for_user(store, user_id).await?;
    if keys.iter().any(|k| k == SUPER_ADMIN) {
        return Ok(all_permissions().into_iter().collect());
    }
    let sql = store.sql(
        "SELECT DISTINCT rp.permission FROM user_roles ur
         JOIN role_permissions rp ON rp.role_id = ur.role_id
         WHERE ur.user_id = ?",
    );
    let rows = sqlx::query::<Any>(&sql)
        .bind(user_id)
        .fetch_all(store.pool())
        .await?;
    let mut out = HashSet::with_capacity(rows.len());
    for row in rows {
        out.insert(row.try_get::<String, _>(0)?);
    }
    Ok(out)
}

pub async fn has_permission(
    store: &AdminStore,
    user_id: &str,
    permission: &str,
) -> Result<bool> {
    Ok(permissions_for_user(store, user_id).await?.contains(permission))
}

pub async fn assign_role(
    store: &AdminStore,
    tenant_id: &str,
    user_id: &str,
    role_key: &str,
) -> Result<()> {
    let role_id = find_role_id(store, tenant_id, role_key)
        .await?
        .ok_or_else(|| anyhow!("unknown role: {role_key}"))?;
    let exists_sql = store.sql("SELECT user_id FROM user_roles WHERE user_id = ? AND role_id = ?");
    if sqlx::query::<Any>(&exists_sql)
        .bind(user_id)
        .bind(&role_id)
        .fetch_optional(store.pool())
        .await?
        .is_some()
    {
        return Ok(());
    }
    let sql = store.sql("INSERT INTO user_roles (user_id, role_id) VALUES (?, ?)");
    sqlx::query::<Any>(&sql)
        .bind(user_id)
        .bind(&role_id)
        .execute(store.pool())
        .await?;
    Ok(())
}

/// 整体替换用户角色（幂等）。
///
/// 先把角色 id 全部解析完再开事务：SQLite 只有一条连接，事务内再查池会自锁。
pub async fn replace_roles(
    store: &AdminStore,
    tenant_id: &str,
    user_id: &str,
    role_keys: &[&str],
) -> Result<()> {
    let mut role_ids = Vec::with_capacity(role_keys.len());
    for key in role_keys {
        let id = find_role_id(store, tenant_id, key)
            .await?
            .ok_or_else(|| anyhow!("unknown role: {key}"))?;
        role_ids.push(id);
    }

    let mut tx = store.pool().begin().await?;
    let delete_sql = store.sql("DELETE FROM user_roles WHERE user_id = ?");
    sqlx::query::<Any>(&delete_sql)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    let insert_sql = store.sql("INSERT INTO user_roles (user_id, role_id) VALUES (?, ?)");
    for role_id in &role_ids {
        sqlx::query::<Any>(&insert_sql)
            .bind(user_id)
            .bind(role_id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::AdminStore;
    use crate::schema::DEFAULT_TENANT;
    use crate::users;

    async fn seed_user(store: &AdminStore, name: &str) -> String {
        users::create(store, DEFAULT_TENANT, name, name, "pw-123456")
            .await
            .expect("create user")
            .id
    }

    #[tokio::test]
    async fn builtin_roles_are_seeded_once() {
        let store = AdminStore::connect_in_memory().await.expect("store");
        let row = store
            .fetch_optional("SELECT COUNT(*) FROM roles WHERE builtin = 1")
            .await
            .expect("count")
            .expect("row");
        let count: i64 = row.try_get(0).expect("count");
        assert_eq!(count, BUILTIN_ROLES.len() as i64);
    }

    #[tokio::test]
    async fn super_admin_gets_everything() {
        let store = AdminStore::connect_in_memory().await.expect("store");
        let uid = seed_user(&store, "root").await;
        assign_role(&store, DEFAULT_TENANT, &uid, SUPER_ADMIN)
            .await
            .expect("assign");
        let perms = permissions_for_user(&store, &uid).await.expect("perms");
        assert_eq!(perms.len(), all_permissions().len());
        assert!(perms.contains("role:write"));
        assert!(perms.contains("provider:reveal"));
    }

    #[tokio::test]
    async fn admin_lacks_role_write_and_reveal() {
        let store = AdminStore::connect_in_memory().await.expect("store");
        let uid = seed_user(&store, "ops").await;
        assign_role(&store, DEFAULT_TENANT, &uid, "admin")
            .await
            .expect("assign");
        let perms = permissions_for_user(&store, &uid).await.expect("perms");
        assert!(perms.contains("skill:write"));
        assert!(!perms.contains("role:write"));
        assert!(!perms.contains("provider:reveal"));
    }

    #[tokio::test]
    async fn viewer_only_reads() {
        let store = AdminStore::connect_in_memory().await.expect("store");
        let uid = seed_user(&store, "readonly").await;
        assign_role(&store, DEFAULT_TENANT, &uid, "viewer")
            .await
            .expect("assign");
        let perms = permissions_for_user(&store, &uid).await.expect("perms");
        assert!(!perms.is_empty());
        assert!(perms.iter().all(|p| p.ends_with(":read")));
        assert!(perms.contains("audit:read"));
    }

    #[tokio::test]
    async fn member_has_no_admin_permissions() {
        let store = AdminStore::connect_in_memory().await.expect("store");
        let uid = seed_user(&store, "staff").await;
        assign_role(&store, DEFAULT_TENANT, &uid, "member")
            .await
            .expect("assign");
        assert!(permissions_for_user(&store, &uid)
            .await
            .expect("perms")
            .is_empty());
    }

    #[tokio::test]
    async fn roles_union_across_multiple_assignments() {
        let store = AdminStore::connect_in_memory().await.expect("store");
        let uid = seed_user(&store, "both").await;
        assign_role(&store, DEFAULT_TENANT, &uid, "viewer")
            .await
            .expect("viewer");
        assign_role(&store, DEFAULT_TENANT, &uid, "admin")
            .await
            .expect("admin");
        let perms = permissions_for_user(&store, &uid).await.expect("perms");
        assert_eq!(perms.len(), builtin_permissions("admin").len());
    }

    #[tokio::test]
    async fn assign_role_is_idempotent_and_replace_roles_resets() {
        let store = AdminStore::connect_in_memory().await.expect("store");
        let uid = seed_user(&store, "multi").await;
        // 新用户默认带「成员」角色：没有任何权限，但界面上有个身份
        assert_eq!(
            role_keys_for_user(&store, &uid).await.expect("keys"),
            vec!["member".to_string()]
        );

        assign_role(&store, DEFAULT_TENANT, &uid, "admin")
            .await
            .expect("assign");
        assign_role(&store, DEFAULT_TENANT, &uid, "admin")
            .await
            .expect("assign again");
        // 重复分配同一角色不该多出一行：member + admin
        assert_eq!(
            role_keys_for_user(&store, &uid).await.expect("keys").len(),
            2
        );

        replace_roles(&store, DEFAULT_TENANT, &uid, &["viewer", "member"])
            .await
            .expect("replace");
        assert_eq!(
            role_keys_for_user(&store, &uid).await.expect("keys"),
            vec!["member".to_string(), "viewer".to_string()]
        );
    }

    #[tokio::test]
    async fn unknown_role_is_rejected() {
        let store = AdminStore::connect_in_memory().await.expect("store");
        let uid = seed_user(&store, "x").await;
        assert!(assign_role(&store, DEFAULT_TENANT, &uid, "nope")
            .await
            .is_err());
    }

    #[tokio::test]
    async fn custom_role_can_be_created_and_granted() {
        let store = AdminStore::connect_in_memory().await.expect("store");
        let uid = seed_user(&store, "finance").await;
        let role_id = ensure_role(&store, DEFAULT_TENANT, "finance", "财务审核", false)
            .await
            .expect("role");
        let sql = store.sql("INSERT INTO role_permissions (role_id, permission) VALUES (?, 'skill:read')");
        sqlx::query::<Any>(&sql)
            .bind(&role_id)
            .execute(store.pool())
            .await
            .expect("perm");
        assign_role(&store, DEFAULT_TENANT, &uid, "finance")
            .await
            .expect("assign");
        let perms = permissions_for_user(&store, &uid).await.expect("perms");
        assert_eq!(perms.len(), 1);
        assert!(has_permission(&store, &uid, "skill:read")
            .await
            .expect("check"));
        assert!(!has_permission(&store, &uid, "skill:write")
            .await
            .expect("check"));
    }
}
