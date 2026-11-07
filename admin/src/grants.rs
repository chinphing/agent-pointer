//! 资源可见范围（grants）。
//!
//! 只有并集，没有 deny：
//! ```text
//! 有效可见 = 存在 all 行 ∨ 用户的任一角色命中 role 行 ∨ 用户 id 命中 user 行
//! ```
//! 与 RBAC 的区别：`skill:write` 是「能不能改这个资源的授权」，grants 是「员工能不能用它」。

use anyhow::{anyhow, Result};
use sqlx::{Any, Row};

use crate::db::{random_id, AdminStore};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Principal {
    /// 全员可见。
    All,
    /// 该角色的所有成员可见。
    Role(String),
    /// 仅该成员可见（单人授权）。
    User(String),
}

impl Principal {
    pub fn parse(principal_type: &str, principal_id: &str) -> Result<Self> {
        let id = principal_id.trim();
        match principal_type.trim() {
            "all" => Ok(Self::All),
            "role" if !id.is_empty() => Ok(Self::Role(id.to_string())),
            "user" if !id.is_empty() => Ok(Self::User(id.to_string())),
            other => Err(anyhow!("invalid principal type: {other}")),
        }
    }

    fn parts(&self) -> (&'static str, &str) {
        match self {
            Self::All => ("all", ""),
            Self::Role(id) => ("role", id.as_str()),
            Self::User(id) => ("user", id.as_str()),
        }
    }
}

/// 整体替换某资源的可见范围（幂等）。
///
/// `All` 与明细行互斥：选「全员」就不要再带角色/成员，否则会出现「全员 + 财务组」这种冗余叠加。
pub async fn replace_grants(
    store: &AdminStore,
    resource_id: &str,
    principals: &[Principal],
) -> Result<()> {
    if principals.iter().any(|p| matches!(p, Principal::All)) && principals.len() > 1 {
        return Err(anyhow!(
            "'all' must be the only principal; drop the role/user rows"
        ));
    }

    // 去重（同一 role 重复出现只留一条）
    let mut unique: Vec<&Principal> = Vec::with_capacity(principals.len());
    for p in principals {
        if !unique.contains(&p) {
            unique.push(p);
        }
    }

    // 事务外先确认资源存在：SQLite 只有一条连接，事务内再查池会自锁。
    let exists_sql = store.sql("SELECT id FROM resources WHERE id = ?");
    if sqlx::query::<Any>(&exists_sql)
        .bind(resource_id)
        .fetch_optional(store.pool())
        .await?
        .is_none()
    {
        return Err(anyhow!("resource not found: {resource_id}"));
    }

    let mut tx = store.pool().begin().await?;
    let delete_sql = store.sql("DELETE FROM resource_grants WHERE resource_id = ?");
    sqlx::query::<Any>(&delete_sql)
        .bind(resource_id)
        .execute(&mut *tx)
        .await?;

    let insert_sql = store.sql(
        "INSERT INTO resource_grants (id, resource_id, principal_type, principal_id)
         VALUES (?, ?, ?, ?)",
    );
    for principal in unique {
        let (kind, id) = principal.parts();
        sqlx::query::<Any>(&insert_sql)
            .bind(random_id())
            .bind(resource_id)
            .bind(kind)
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}

pub async fn grants(store: &AdminStore, resource_id: &str) -> Result<Vec<Principal>> {
    let sql = store.sql(
        "SELECT principal_type, principal_id FROM resource_grants
         WHERE resource_id = ? ORDER BY principal_type, principal_id",
    );
    let rows = sqlx::query::<Any>(&sql)
        .bind(resource_id)
        .fetch_all(store.pool())
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        let kind: String = row.try_get(0)?;
        let id: String = row.try_get(1)?;
        out.push(Principal::parse(&kind, &id)?);
    }
    Ok(out)
}

pub async fn clear_grants(store: &AdminStore, resource_id: &str) -> Result<u64> {
    let sql = store.sql("DELETE FROM resource_grants WHERE resource_id = ?");
    Ok(sqlx::query::<Any>(&sql)
        .bind(resource_id)
        .execute(store.pool())
        .await?
        .rows_affected())
}

/// 该用户能看到的资源 id（只含已启用的）。
pub async fn visible_resource_ids(
    store: &AdminStore,
    tenant_id: &str,
    user_id: &str,
) -> Result<Vec<String>> {
    let sql = store.sql(
        "SELECT DISTINCT r.id FROM resources r
         JOIN resource_grants g ON g.resource_id = r.id
         WHERE r.tenant_id = ?
           AND r.enabled = 1
           AND (
             g.principal_type = 'all'
             OR (g.principal_type = 'role'
                 AND g.principal_id IN (SELECT role_id FROM user_roles WHERE user_id = ?))
             OR (g.principal_type = 'user' AND g.principal_id = ?)
           )
         ORDER BY r.id",
    );
    let rows = sqlx::query::<Any>(&sql)
        .bind(tenant_id)
        .bind(user_id)
        .bind(user_id)
        .fetch_all(store.pool())
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        out.push(row.try_get::<String, _>(0)?);
    }
    Ok(out)
}

pub async fn can_see(store: &AdminStore, resource_id: &str, user_id: &str) -> Result<bool> {
    let sql = store.sql(
        "SELECT COUNT(*) FROM resource_grants g
         WHERE g.resource_id = ?
           AND (
             g.principal_type = 'all'
             OR (g.principal_type = 'role'
                 AND g.principal_id IN (SELECT role_id FROM user_roles WHERE user_id = ?))
             OR (g.principal_type = 'user' AND g.principal_id = ?)
           )",
    );
    let row = sqlx::query::<Any>(&sql)
        .bind(resource_id)
        .bind(user_id)
        .bind(user_id)
        .fetch_one(store.pool())
        .await?;
    let count: i64 = row.try_get(0)?;
    Ok(count > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::AdminStore;
    use crate::rbac;
    use crate::resources::{self, KIND_SKILL};
    use crate::schema::DEFAULT_TENANT;
    use crate::users;

    async fn setup() -> (AdminStore, String, String, String) {
        let store = AdminStore::connect_in_memory().await.expect("store");
        let alice = users::create(&store, DEFAULT_TENANT, "alice", "", "pw-123456")
            .await
            .expect("alice")
            .id;
        let bob = users::create(&store, DEFAULT_TENANT, "bob", "", "pw-123456")
            .await
            .expect("bob")
            .id;
        let carol = users::create(&store, DEFAULT_TENANT, "carol", "", "pw-123456")
            .await
            .expect("carol")
            .id;
        // alice 属于 admin 角色；bob 属于 member 角色；carol 无角色。
        rbac::assign_role(&store, DEFAULT_TENANT, &alice, "admin")
            .await
            .expect("alice role");
        rbac::assign_role(&store, DEFAULT_TENANT, &bob, "member")
            .await
            .expect("bob role");
        (store, alice, bob, carol)
    }

    async fn skill(store: &AdminStore, slug: &str) -> String {
        resources::create(store, DEFAULT_TENANT, KIND_SKILL, slug, slug, "", None, "{}", None)
            .await
            .expect("skill")
            .id
    }

    #[tokio::test]
    async fn no_grants_means_nobody_sees_it() {
        let (store, alice, bob, carol) = setup().await;
        let rid = skill(&store, "orphan").await;
        resources::set_enabled(&store, &rid, true).await.expect("enable");
        for uid in [&alice, &bob, &carol] {
            assert!(!can_see(&store, &rid, uid).await.expect("can_see"));
        }
        assert!(visible_resource_ids(&store, DEFAULT_TENANT, &alice)
            .await
            .expect("visible")
            .is_empty());
    }

    #[tokio::test]
    async fn all_grants_everyone() {
        let (store, alice, bob, carol) = setup().await;
        let rid = skill(&store, "everyone").await;
        resources::set_enabled(&store, &rid, true).await.expect("enable");
        replace_grants(&store, &rid, &[Principal::All])
            .await
            .expect("grant all");
        for uid in [&alice, &bob, &carol] {
            assert!(can_see(&store, &rid, uid).await.expect("can_see"));
        }
    }

    #[tokio::test]
    async fn role_grant_covers_members_only() {
        let (store, alice, bob, carol) = setup().await;
        let rid = skill(&store, "admin-only").await;
        resources::set_enabled(&store, &rid, true).await.expect("enable");
        let role_id = rbac::find_role_id(&store, DEFAULT_TENANT, "admin")
            .await
            .expect("role lookup")
            .expect("role exists");
        replace_grants(&store, &rid, &[Principal::Role(role_id)])
            .await
            .expect("grant role");

        assert!(can_see(&store, &rid, &alice).await.expect("alice"));
        assert!(!can_see(&store, &rid, &bob).await.expect("bob"));
        assert!(!can_see(&store, &rid, &carol).await.expect("carol"));
    }

    #[tokio::test]
    async fn user_grant_is_single_person() {
        let (store, alice, bob, _carol) = setup().await;
        let rid = skill(&store, "personal").await;
        resources::set_enabled(&store, &rid, true).await.expect("enable");
        replace_grants(&store, &rid, &[Principal::User(bob.clone())])
            .await
            .expect("grant user");

        assert!(can_see(&store, &rid, &bob).await.expect("bob"));
        assert!(!can_see(&store, &rid, &alice).await.expect("alice"));
    }

    #[tokio::test]
    async fn role_and_user_grants_union() {
        let (store, alice, bob, carol) = setup().await;
        let rid = skill(&store, "mixed").await;
        resources::set_enabled(&store, &rid, true).await.expect("enable");
        let role_id = rbac::find_role_id(&store, DEFAULT_TENANT, "admin")
            .await
            .expect("role lookup")
            .expect("role exists");
        replace_grants(
            &store,
            &rid,
            &[Principal::Role(role_id), Principal::User(carol.clone())],
        )
        .await
        .expect("mixed grants");

        assert!(can_see(&store, &rid, &alice).await.expect("alice via role"));
        assert!(can_see(&store, &rid, &carol).await.expect("carol via user"));
        assert!(!can_see(&store, &rid, &bob).await.expect("bob"));
    }

    #[tokio::test]
    async fn all_with_details_is_rejected() {
        let (store, _alice, bob, _carol) = setup().await;
        let rid = skill(&store, "conflict").await;
        let err = replace_grants(&store, &rid, &[Principal::All, Principal::User(bob)])
            .await
            .expect_err("all + detail");
        assert!(err.to_string().contains("only principal"), "got: {err}");
    }

    #[tokio::test]
    async fn replace_is_idempotent_and_dedupes() {
        let (store, alice, bob, _carol) = setup().await;
        let rid = skill(&store, "replace").await;
        replace_grants(&store, &rid, &[Principal::User(bob.clone()), Principal::User(bob.clone())])
            .await
            .expect("grant");
        assert_eq!(grants(&store, &rid).await.expect("grants").len(), 1);

        replace_grants(&store, &rid, &[Principal::User(alice.clone())])
            .await
            .expect("replace");
        assert_eq!(
            grants(&store, &rid).await.expect("grants"),
            vec![Principal::User(alice)]
        );
    }

    #[tokio::test]
    async fn disabled_resource_is_invisible() {
        let (store, alice, _bob, _carol) = setup().await;
        let rid = skill(&store, "disabled").await;
        replace_grants(&store, &rid, &[Principal::All])
            .await
            .expect("grant");
        // 默认 enabled = 0
        assert!(visible_resource_ids(&store, DEFAULT_TENANT, &alice)
            .await
            .expect("visible")
            .is_empty());

        resources::set_enabled(&store, &rid, true).await.expect("enable");
        assert_eq!(
            visible_resource_ids(&store, DEFAULT_TENANT, &alice)
                .await
                .expect("visible"),
            vec![rid]
        );
    }

    #[tokio::test]
    async fn deleting_resource_cascades_grants() {
        let (store, _alice, _bob, _carol) = setup().await;
        let rid = skill(&store, "cascade").await;
        replace_grants(&store, &rid, &[Principal::All])
            .await
            .expect("grant");
        resources::delete(&store, &rid).await.expect("delete");

        let row = store
            .fetch_optional("SELECT COUNT(*) FROM resource_grants")
            .await
            .expect("count")
            .expect("row");
        let count: i64 = row.try_get(0).expect("count");
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn grants_for_missing_resource_are_rejected() {
        let (store, _alice, _bob, _carol) = setup().await;
        assert!(replace_grants(&store, "missing", &[Principal::All])
            .await
            .is_err());
    }

    #[tokio::test]
    async fn principal_parse_rejects_bad_input() {
        assert!(Principal::parse("team", "x").is_err());
        assert!(Principal::parse("role", "").is_err());
        assert!(Principal::parse("user", "  ").is_err());
        assert_eq!(Principal::parse("all", "").expect("all"), Principal::All);
    }
}
