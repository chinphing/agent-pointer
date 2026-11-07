//! 权限清单、内置角色与判定。
//!
//! 权限串形如 `<domain>:<action>`；domain 与资源 `kind` 对应。
//! 清单写在代码里，DB 只存「角色 → 权限」的授权关系，加权限不用迁移。

use anyhow::{anyhow, Result};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashSet;

use crate::db::{now_ts, random_id};

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
    "audit:read",
    "system:read",
];

pub const SUPER_ADMIN: &str = "super_admin";

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
pub fn builtin_permissions(key: &str) -> Vec<&'static str> {
    match key {
        "admin" => ALL_PERMISSIONS
            .iter()
            .copied()
            .filter(|p| *p != "role:write" && *p != "provider:reveal")
            .collect(),
        "viewer" => ALL_PERMISSIONS
            .iter()
            .copied()
            .filter(|p| p.ends_with(":read"))
            .collect(),
        _ => Vec::new(),
    }
}

pub fn is_builtin(key: &str) -> bool {
    BUILTIN_ROLES.iter().any(|r| r.key == key)
}

pub fn seed_builtin_roles(conn: &Connection, tenant_id: &str) -> Result<()> {
    for role in BUILTIN_ROLES {
        let role_id = ensure_role(conn, tenant_id, role.key, role.name, true)?;
        for perm in builtin_permissions(role.key) {
            conn.execute(
                "INSERT OR IGNORE INTO role_permissions (role_id, permission) VALUES (?1, ?2)",
                params![role_id, perm],
            )?;
        }
    }
    Ok(())
}

pub fn ensure_role(
    conn: &Connection,
    tenant_id: &str,
    key: &str,
    name: &str,
    builtin: bool,
) -> Result<String> {
    if let Some(id) = find_role_id(conn, tenant_id, key)? {
        return Ok(id);
    }
    let id = random_id();
    conn.execute(
        "INSERT INTO roles (id, tenant_id, key, name, builtin, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, tenant_id, key, name, if builtin { 1 } else { 0 }, now_ts()],
    )?;
    Ok(id)
}

pub fn find_role_id(conn: &Connection, tenant_id: &str, key: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT id FROM roles WHERE tenant_id = ?1 AND key = ?2",
            params![tenant_id, key],
            |r| r.get(0),
        )
        .optional()?)
}

pub fn role_keys_for_user(conn: &Connection, user_id: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT r.key FROM user_roles ur
         JOIN roles r ON r.id = ur.role_id
         WHERE ur.user_id = ?1
         ORDER BY r.key",
    )?;
    let rows = stmt.query_map(params![user_id], |r| r.get::<_, String>(0))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// 用户的有效权限集。命中 `super_admin` 直接返回全部。
pub fn permissions_for_user(conn: &Connection, user_id: &str) -> Result<HashSet<String>> {
    let keys = role_keys_for_user(conn, user_id)?;
    if keys.iter().any(|k| k == SUPER_ADMIN) {
        return Ok(ALL_PERMISSIONS.iter().map(|p| (*p).to_string()).collect());
    }
    let mut stmt = conn.prepare(
        "SELECT DISTINCT rp.permission FROM user_roles ur
         JOIN role_permissions rp ON rp.role_id = ur.role_id
         WHERE ur.user_id = ?1",
    )?;
    let rows = stmt.query_map(params![user_id], |r| r.get::<_, String>(0))?;
    let mut out = HashSet::new();
    for row in rows {
        out.insert(row?);
    }
    Ok(out)
}

pub fn has_permission(conn: &Connection, user_id: &str, permission: &str) -> Result<bool> {
    Ok(permissions_for_user(conn, user_id)?.contains(permission))
}

pub fn assign_role(conn: &Connection, tenant_id: &str, user_id: &str, role_key: &str) -> Result<()> {
    let role_id = find_role_id(conn, tenant_id, role_key)?
        .ok_or_else(|| anyhow!("unknown role: {role_key}"))?;
    conn.execute(
        "INSERT OR IGNORE INTO user_roles (user_id, role_id) VALUES (?1, ?2)",
        params![user_id, role_id],
    )?;
    Ok(())
}

/// 整体替换用户角色（幂等）。
pub fn replace_roles(
    conn: &Connection,
    tenant_id: &str,
    user_id: &str,
    role_keys: &[&str],
) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM user_roles WHERE user_id = ?1", params![user_id])?;
    for key in role_keys {
        assign_role(&tx, tenant_id, user_id, key)?;
    }
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_conn;
    use crate::schema::DEFAULT_TENANT;
    use crate::users;

    fn seed_user(conn: &Connection, name: &str) -> String {
        users::create(conn, DEFAULT_TENANT, name, name, "pw-123456")
            .expect("create user")
            .id
    }

    #[test]
    fn builtin_roles_are_seeded_once() {
        let conn = test_conn();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM roles WHERE builtin = 1", [], |r| r.get(0))
            .expect("count");
        assert_eq!(count, BUILTIN_ROLES.len() as i64);
    }

    #[test]
    fn super_admin_gets_everything() {
        let conn = test_conn();
        let uid = seed_user(&conn, "root");
        assign_role(&conn, DEFAULT_TENANT, &uid, SUPER_ADMIN).expect("assign");
        let perms = permissions_for_user(&conn, &uid).expect("perms");
        assert_eq!(perms.len(), ALL_PERMISSIONS.len());
        assert!(perms.contains("role:write"));
        assert!(perms.contains("provider:reveal"));
    }

    #[test]
    fn admin_lacks_role_write_and_reveal() {
        let conn = test_conn();
        let uid = seed_user(&conn, "ops");
        assign_role(&conn, DEFAULT_TENANT, &uid, "admin").expect("assign");
        let perms = permissions_for_user(&conn, &uid).expect("perms");
        assert!(perms.contains("skill:write"));
        assert!(!perms.contains("role:write"));
        assert!(!perms.contains("provider:reveal"));
    }

    #[test]
    fn viewer_only_reads() {
        let conn = test_conn();
        let uid = seed_user(&conn, "readonly");
        assign_role(&conn, DEFAULT_TENANT, &uid, "viewer").expect("assign");
        let perms = permissions_for_user(&conn, &uid).expect("perms");
        assert!(!perms.is_empty());
        assert!(perms.iter().all(|p| p.ends_with(":read")));
        assert!(perms.contains("audit:read"));
    }

    #[test]
    fn member_has_no_admin_permissions() {
        let conn = test_conn();
        let uid = seed_user(&conn, "staff");
        assign_role(&conn, DEFAULT_TENANT, &uid, "member").expect("assign");
        assert!(permissions_for_user(&conn, &uid).expect("perms").is_empty());
    }

    #[test]
    fn roles_union_across_multiple_assignments() {
        let conn = test_conn();
        let uid = seed_user(&conn, "both");
        assign_role(&conn, DEFAULT_TENANT, &uid, "viewer").expect("viewer");
        assign_role(&conn, DEFAULT_TENANT, &uid, "admin").expect("admin");
        let perms = permissions_for_user(&conn, &uid).expect("perms");
        // admin 是 viewer 的超集
        assert_eq!(perms.len(), builtin_permissions("admin").len());
    }

    #[test]
    fn assign_role_is_idempotent_and_replace_roles_resets() {
        let conn = test_conn();
        let uid = seed_user(&conn, "multi");
        assign_role(&conn, DEFAULT_TENANT, &uid, "admin").expect("assign");
        assign_role(&conn, DEFAULT_TENANT, &uid, "admin").expect("assign again");
        assert_eq!(role_keys_for_user(&conn, &uid).expect("keys").len(), 1);

        replace_roles(&conn, DEFAULT_TENANT, &uid, &["viewer", "member"]).expect("replace");
        assert_eq!(
            role_keys_for_user(&conn, &uid).expect("keys"),
            vec!["member".to_string(), "viewer".to_string()]
        );
    }

    #[test]
    fn unknown_role_is_rejected() {
        let conn = test_conn();
        let uid = seed_user(&conn, "x");
        assert!(assign_role(&conn, DEFAULT_TENANT, &uid, "nope").is_err());
    }

    #[test]
    fn custom_role_can_be_created_and_granted() {
        let conn = test_conn();
        let uid = seed_user(&conn, "finance");
        let role_id = ensure_role(&conn, DEFAULT_TENANT, "finance", "财务审核", false).expect("role");
        conn.execute(
            "INSERT INTO role_permissions (role_id, permission) VALUES (?1, 'skill:read')",
            params![role_id],
        )
        .expect("perm");
        assign_role(&conn, DEFAULT_TENANT, &uid, "finance").expect("assign");
        let perms = permissions_for_user(&conn, &uid).expect("perms");
        assert_eq!(perms.len(), 1);
        assert!(has_permission(&conn, &uid, "skill:read").expect("check"));
        assert!(!has_permission(&conn, &uid, "skill:write").expect("check"));
    }
}
