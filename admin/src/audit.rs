//! 审计流水：谁、何时、改了什么、改前改后。

use anyhow::Result;
use sqlx::{Any, Row};

use crate::db::{now_ts, random_id, AdminStore};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditEntry {
    pub id: String,
    pub actor_user_id: Option<String>,
    pub actor_name: String,
    pub action: String,
    pub target_type: String,
    pub target_id: String,
    pub before_json: Option<String>,
    pub after_json: Option<String>,
    pub created_at: i64,
}

pub struct AuditInput<'a> {
    pub actor_user_id: Option<&'a str>,
    pub actor_name: &'a str,
    pub action: &'a str,
    pub target_type: &'a str,
    pub target_id: &'a str,
    pub before_json: Option<&'a str>,
    pub after_json: Option<&'a str>,
}

pub async fn record(store: &AdminStore, tenant_id: &str, input: AuditInput<'_>) -> Result<()> {
    let sql = store.sql(
        "INSERT INTO audit_log
         (id, tenant_id, actor_user_id, actor_name, action, target_type, target_id,
          before_json, after_json, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    );
    sqlx::query::<Any>(&sql)
        .bind(random_id())
        .bind(tenant_id)
        .bind(input.actor_user_id)
        .bind(input.actor_name)
        .bind(input.action)
        .bind(input.target_type)
        .bind(input.target_id)
        .bind(input.before_json)
        .bind(input.after_json)
        .bind(now_ts())
        .execute(store.pool())
        .await?;
    Ok(())
}

pub async fn list(store: &AdminStore, tenant_id: &str, limit: i64) -> Result<Vec<AuditEntry>> {
    let sql = store.sql(
        "SELECT id, actor_user_id, actor_name, action, target_type, target_id,
                before_json, after_json, created_at
         FROM audit_log WHERE tenant_id = ? ORDER BY created_at DESC LIMIT ?",
    );
    let rows = sqlx::query::<Any>(&sql)
        .bind(tenant_id)
        .bind(limit.clamp(1, 500))
        .fetch_all(store.pool())
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        out.push(AuditEntry {
            id: row.try_get(0)?,
            actor_user_id: row.try_get(1)?,
            actor_name: row.try_get(2)?,
            action: row.try_get(3)?,
            target_type: row.try_get(4)?,
            target_id: row.try_get(5)?,
            before_json: row.try_get(6)?,
            after_json: row.try_get(7)?,
            created_at: row.try_get(8)?,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::DEFAULT_TENANT;

    async fn store() -> AdminStore {
        AdminStore::connect_in_memory().await.expect("store")
    }

    fn input<'a>(action: &'a str, before: Option<&'a str>, after: Option<&'a str>) -> AuditInput<'a> {
        AuditInput {
            actor_user_id: Some("u1"),
            actor_name: "王国宏",
            action,
            target_type: "skill",
            target_id: "travel",
            before_json: before,
            after_json: after,
        }
    }

    #[tokio::test]
    async fn record_and_list_newest_first() {
        let store = store().await;
        record(&store, DEFAULT_TENANT, input("create", None, Some(r#"{"v":1}"#)))
            .await
            .expect("first");
        record(&store, DEFAULT_TENANT, input("update", Some(r#"{"v":1}"#), Some(r#"{"v":2}"#)))
            .await
            .expect("second");

        let entries = list(&store, DEFAULT_TENANT, 10).await.expect("list");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].action, "update");
        assert_eq!(entries[0].before_json.as_deref(), Some(r#"{"v":1}"#));
        assert_eq!(entries[1].action, "create");
        assert_eq!(entries[1].actor_name, "王国宏");
    }

    #[tokio::test]
    async fn list_is_tenant_scoped_and_limited() {
        let store = store().await;
        for _ in 0..3 {
            record(&store, DEFAULT_TENANT, input("create", None, None))
                .await
                .expect("record");
        }
        record(&store, "other", input("create", None, None))
            .await
            .expect("other");

        assert_eq!(list(&store, DEFAULT_TENANT, 2).await.expect("list").len(), 2);
        assert_eq!(list(&store, DEFAULT_TENANT, 100).await.expect("list").len(), 3);
    }
}
