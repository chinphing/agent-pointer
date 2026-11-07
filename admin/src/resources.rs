//! 统一资源表。
//!
//! 技能、模型、插件、模型服务、MCP、档位映射都走同一套存储与授权，
//! 差异只在 `kind` 与 `payload_json`。新增一类资源 = 加一个 `kind` 常量 + 一组权限。

use anyhow::{anyhow, Result};
use sqlx::any::AnyRow;
use sqlx::{Any, Row};

use crate::crypto::EncryptedSecret;
use crate::db::{now_ts, random_id, AdminStore};

pub const KIND_PROVIDER: &str = "provider";
pub const KIND_MODEL: &str = "model";
pub const KIND_SKILL: &str = "skill";
pub const KIND_PLUGIN: &str = "plugin";
pub const KIND_MCP: &str = "mcp";
pub const KIND_TIER_PROFILE: &str = "tier_profile";

pub const KNOWN_KINDS: &[&str] = &[
    KIND_PROVIDER,
    KIND_MODEL,
    KIND_SKILL,
    KIND_PLUGIN,
    KIND_MCP,
    KIND_TIER_PROFILE,
];

pub fn is_known_kind(kind: &str) -> bool {
    KNOWN_KINDS.contains(&kind)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resource {
    pub id: String,
    pub tenant_id: String,
    pub kind: String,
    pub slug: String,
    pub name: String,
    pub description: String,
    pub parent_id: Option<String>,
    pub enabled: bool,
    pub payload_json: String,
    /// 是否有加密密钥（不返回密文本身）。
    pub has_secret: bool,
    pub current_version_id: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

const COLS: &str = "id, tenant_id, kind, slug, name, description, parent_id, enabled, \
                    payload_json, secret_enc, current_version_id, created_at, updated_at";

fn map_resource(row: &AnyRow) -> Result<Resource> {
    let enabled: i64 = row.try_get(7)?;
    let secret: Option<String> = row.try_get(9)?;
    Ok(Resource {
        id: row.try_get(0)?,
        tenant_id: row.try_get(1)?,
        kind: row.try_get(2)?,
        slug: row.try_get(3)?,
        name: row.try_get(4)?,
        description: row.try_get(5)?,
        parent_id: row.try_get(6)?,
        enabled: enabled != 0,
        payload_json: row.try_get(8)?,
        has_secret: secret.is_some(),
        current_version_id: row.try_get(10)?,
        created_at: row.try_get(11)?,
        updated_at: row.try_get(12)?,
    })
}

fn normalize_slug(raw: &str) -> Result<String> {
    let slug = raw.trim();
    if slug.is_empty() {
        return Err(anyhow!("slug must not be empty"));
    }
    if slug.chars().count() > 128 {
        return Err(anyhow!("slug too long (max 128)"));
    }
    if slug.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(anyhow!("slug must not contain whitespace"));
    }
    Ok(slug.to_string())
}

#[allow(clippy::too_many_arguments)]
pub async fn create(
    store: &AdminStore,
    tenant_id: &str,
    kind: &str,
    slug: &str,
    name: &str,
    description: &str,
    parent_id: Option<&str>,
    payload_json: &str,
    secret: Option<&EncryptedSecret>,
) -> Result<Resource> {
    if !is_known_kind(kind) {
        return Err(anyhow!("unknown resource kind: {kind}"));
    }
    let slug = normalize_slug(slug)?;
    let name = {
        let n = name.trim();
        if n.is_empty() {
            slug.clone()
        } else {
            n.to_string()
        }
    };
    if find_by_slug(store, tenant_id, kind, &slug).await?.is_some() {
        return Err(anyhow!("resource already exists: {kind}/{slug}"));
    }

    let id = random_id();
    let now = now_ts();
    let sql = store.sql(
        "INSERT INTO resources (id, tenant_id, kind, slug, name, description, parent_id,
                                enabled, payload_json, secret_enc, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, 0, ?, ?, ?, ?)",
    );
    sqlx::query::<Any>(&sql)
        .bind(&id)
        .bind(tenant_id)
        .bind(kind)
        .bind(&slug)
        .bind(&name)
        .bind(description.trim())
        .bind(parent_id)
        .bind(payload_json)
        .bind(secret.map(|s| s.as_str().to_string()))
        .bind(now)
        .bind(now)
        .execute(store.pool())
        .await?;

    get(store, &id)
        .await?
        .ok_or_else(|| anyhow!("resource vanished right after insert"))
}

pub async fn get(store: &AdminStore, id: &str) -> Result<Option<Resource>> {
    let sql = store.sql(&format!("SELECT {COLS} FROM resources WHERE id = ?"));
    let row = sqlx::query::<Any>(&sql)
        .bind(id)
        .fetch_optional(store.pool())
        .await?;
    match row {
        Some(row) => Ok(Some(map_resource(&row)?)),
        None => Ok(None),
    }
}

pub async fn find_by_slug(
    store: &AdminStore,
    tenant_id: &str,
    kind: &str,
    slug: &str,
) -> Result<Option<Resource>> {
    let sql = store.sql(&format!(
        "SELECT {COLS} FROM resources WHERE tenant_id = ? AND kind = ? AND slug = ?"
    ));
    let row = sqlx::query::<Any>(&sql)
        .bind(tenant_id)
        .bind(kind)
        .bind(slug.trim())
        .fetch_optional(store.pool())
        .await?;
    match row {
        Some(row) => Ok(Some(map_resource(&row)?)),
        None => Ok(None),
    }
}

pub async fn list(
    store: &AdminStore,
    tenant_id: &str,
    kind: Option<&str>,
) -> Result<Vec<Resource>> {
    let (sql, kind_filter) = match kind {
        Some(k) => (
            store.sql(&format!(
                "SELECT {COLS} FROM resources WHERE tenant_id = ? AND kind = ? ORDER BY kind, slug"
            )),
            Some(k),
        ),
        None => (
            store.sql(&format!(
                "SELECT {COLS} FROM resources WHERE tenant_id = ? ORDER BY kind, slug"
            )),
            None,
        ),
    };
    let mut query = sqlx::query::<Any>(&sql).bind(tenant_id);
    if let Some(k) = kind_filter {
        query = query.bind(k);
    }
    let rows = query.fetch_all(store.pool()).await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        out.push(map_resource(row)?);
    }
    Ok(out)
}

pub async fn update_meta(
    store: &AdminStore,
    id: &str,
    name: &str,
    description: &str,
    payload_json: &str,
) -> Result<()> {
    let sql = store.sql(
        "UPDATE resources SET name = ?, description = ?, payload_json = ?, updated_at = ?
         WHERE id = ?",
    );
    let changed = sqlx::query::<Any>(&sql)
        .bind(name.trim())
        .bind(description.trim())
        .bind(payload_json)
        .bind(now_ts())
        .bind(id)
        .execute(store.pool())
        .await?
        .rows_affected();
    if changed == 0 {
        return Err(anyhow!("resource not found: {id}"));
    }
    Ok(())
}

pub async fn set_enabled(store: &AdminStore, id: &str, enabled: bool) -> Result<()> {
    let sql = store.sql("UPDATE resources SET enabled = ?, updated_at = ? WHERE id = ?");
    let changed = sqlx::query::<Any>(&sql)
        .bind(if enabled { 1i64 } else { 0 })
        .bind(now_ts())
        .bind(id)
        .execute(store.pool())
        .await?
        .rows_affected();
    if changed == 0 {
        return Err(anyhow!("resource not found: {id}"));
    }
    Ok(())
}

pub async fn set_secret(
    store: &AdminStore,
    id: &str,
    secret: Option<&EncryptedSecret>,
) -> Result<()> {
    let sql = store.sql("UPDATE resources SET secret_enc = ?, updated_at = ? WHERE id = ?");
    let changed = sqlx::query::<Any>(&sql)
        .bind(secret.map(|s| s.as_str().to_string()))
        .bind(now_ts())
        .bind(id)
        .execute(store.pool())
        .await?
        .rows_affected();
    if changed == 0 {
        return Err(anyhow!("resource not found: {id}"));
    }
    Ok(())
}

/// 取密文（不解密）。解密由 [`crate::crypto::decrypt_secret`] 完成。
pub async fn secret(store: &AdminStore, id: &str) -> Result<Option<EncryptedSecret>> {
    let sql = store.sql("SELECT secret_enc FROM resources WHERE id = ?");
    let row = sqlx::query::<Any>(&sql)
        .bind(id)
        .fetch_optional(store.pool())
        .await?;
    let Some(row) = row else { return Ok(None) };
    let raw: Option<String> = row.try_get(0)?;
    match raw {
        Some(raw) => Ok(Some(EncryptedSecret::from_stored(&raw)?)),
        None => Ok(None),
    }
}

pub async fn delete(store: &AdminStore, id: &str) -> Result<bool> {
    let sql = store.sql("DELETE FROM resources WHERE id = ?");
    Ok(sqlx::query::<Any>(&sql)
        .bind(id)
        .execute(store.pool())
        .await?
        .rows_affected()
        > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::{decrypt_secret, encrypt_secret};
    use crate::schema::DEFAULT_TENANT;

    async fn store() -> AdminStore {
        AdminStore::connect_in_memory().await.expect("store")
    }

    async fn create_skill(store: &AdminStore, slug: &str) -> Resource {
        create(
            store,
            DEFAULT_TENANT,
            KIND_SKILL,
            slug,
            "差旅报销",
            "报销单预审",
            None,
            "{}",
            None,
        )
        .await
        .expect("create skill")
    }

    #[tokio::test]
    async fn create_get_and_find() {
        let store = store().await;
        let r = create_skill(&store, "travel-reimburse").await;
        assert_eq!(r.kind, KIND_SKILL);
        assert_eq!(r.slug, "travel-reimburse");
        assert_eq!(r.name, "差旅报销");
        assert!(!r.enabled, "新建资源默认停用");
        assert!(!r.has_secret);

        assert_eq!(get(&store, &r.id).await.expect("get"), Some(r.clone()));
        assert_eq!(
            find_by_slug(&store, DEFAULT_TENANT, KIND_SKILL, "travel-reimburse")
                .await
                .expect("find")
                .expect("present")
                .id,
            r.id
        );
    }

    #[tokio::test]
    async fn duplicate_slug_in_same_kind_rejected() {
        let store = store().await;
        create_skill(&store, "dup").await;
        let err = create(
            &store,
            DEFAULT_TENANT,
            KIND_SKILL,
            "dup",
            "另一个",
            "",
            None,
            "{}",
            None,
        )
        .await
        .expect_err("duplicate");
        assert!(err.to_string().contains("already exists"), "got: {err}");
    }

    #[tokio::test]
    async fn same_slug_allowed_across_kinds() {
        let store = store().await;
        create_skill(&store, "shared").await;
        create(
            &store,
            DEFAULT_TENANT,
            KIND_PLUGIN,
            "shared",
            "插件",
            "",
            None,
            "{}",
            None,
        )
        .await
        .expect("plugin with same slug");
    }

    #[tokio::test]
    async fn unknown_kind_and_bad_slug_rejected() {
        let store = store().await;
        assert!(create(&store, DEFAULT_TENANT, "nope", "x", "", "", None, "{}", None)
            .await
            .is_err());
        assert!(create(&store, DEFAULT_TENANT, KIND_SKILL, "", "", "", None, "{}", None)
            .await
            .is_err());
        assert!(
            create(&store, DEFAULT_TENANT, KIND_SKILL, "a b", "", "", None, "{}", None)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn list_filters_by_kind_and_tenant() {
        let store = store().await;
        create_skill(&store, "s1").await;
        create_skill(&store, "s2").await;
        create(&store, DEFAULT_TENANT, KIND_PLUGIN, "p1", "", "", None, "{}", None)
            .await
            .expect("plugin");
        create(&store, "other", KIND_SKILL, "s3", "", "", None, "{}", None)
            .await
            .expect("other tenant");

        assert_eq!(list(&store, DEFAULT_TENANT, None).await.expect("all").len(), 3);
        let skills = list(&store, DEFAULT_TENANT, Some(KIND_SKILL))
            .await
            .expect("skills");
        assert_eq!(
            skills.iter().map(|r| r.slug.as_str()).collect::<Vec<_>>(),
            vec!["s1", "s2"]
        );
    }

    #[tokio::test]
    async fn enable_and_update_meta() {
        let store = store().await;
        let r = create_skill(&store, "meta").await;
        set_enabled(&store, &r.id, true).await.expect("enable");
        update_meta(&store, &r.id, "新名字", "新描述", r#"{"a":1}"#)
            .await
            .expect("update");

        let got = get(&store, &r.id).await.expect("get").expect("present");
        assert!(got.enabled);
        assert_eq!(got.name, "新名字");
        assert_eq!(got.description, "新描述");
        assert_eq!(got.payload_json, r#"{"a":1}"#);
        assert!(got.updated_at >= r.updated_at);
    }

    #[tokio::test]
    async fn secret_roundtrip_through_store() {
        let store = store().await;
        let encrypted = encrypt_secret("sk-live-123").expect("encrypt");
        let r = create(
            &store,
            DEFAULT_TENANT,
            KIND_PROVIDER,
            "aliyun",
            "阿里云百炼",
            "",
            None,
            r#"{"baseUrl":"https://example.invalid/v1"}"#,
            Some(&encrypted),
        )
        .await
        .expect("create provider");

        assert!(r.has_secret);
        let stored = secret(&store, &r.id).await.expect("read").expect("present");
        assert_eq!(stored.as_str(), encrypted.as_str());
        assert_eq!(decrypt_secret(&stored).expect("decrypt"), "sk-live-123");

        set_secret(&store, &r.id, None).await.expect("clear");
        assert!(secret(&store, &r.id).await.expect("read").is_none());
    }

    #[tokio::test]
    async fn delete_removes_resource() {
        let store = store().await;
        let r = create_skill(&store, "gone").await;
        assert!(delete(&store, &r.id).await.expect("delete"));
        assert!(get(&store, &r.id).await.expect("get").is_none());
        assert!(!delete(&store, &r.id).await.expect("delete again"));
    }

    #[tokio::test]
    async fn missing_resource_updates_are_errors() {
        let store = store().await;
        assert!(set_enabled(&store, "missing", true).await.is_err());
        assert!(update_meta(&store, "missing", "", "", "{}").await.is_err());
        assert!(set_secret(&store, "missing", None).await.is_err());
    }
}
