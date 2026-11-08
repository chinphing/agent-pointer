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
    /// `optional`（默认）/ `default_on`：下发后客户端是否默认启用。
    pub activation: String,
    pub created_at: i64,
    pub updated_at: i64,
}

const COLS: &str = "id, tenant_id, kind, slug, name, description, parent_id, enabled, \
                    payload_json, secret_enc, current_version_id, created_at, updated_at, activation";

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
        activation: row.try_get(13)?,
    })
}

/// 启用策略取值：`optional`（默认）/ `default_on`。
pub const ACTIVATION_OPTIONAL: &str = "optional";
pub const ACTIVATION_DEFAULT_ON: &str = "default_on";

pub fn normalize_activation(raw: &str) -> Result<String> {
    match raw.trim() {
        "" | ACTIVATION_OPTIONAL => Ok(ACTIVATION_OPTIONAL.to_string()),
        ACTIVATION_DEFAULT_ON => Ok(ACTIVATION_DEFAULT_ON.to_string()),
        other => Err(anyhow!(
            "activation 只能是 {ACTIVATION_OPTIONAL} 或 {ACTIVATION_DEFAULT_ON}，收到 {other}"
        )),
    }
}

pub async fn set_activation(
    store: &AdminStore,
    id: &str,
    activation: &str,
) -> Result<()> {
    let value = normalize_activation(activation)?;
    let sql = store.sql("UPDATE resources SET activation = ?, updated_at = ? WHERE id = ?");
    sqlx::query::<Any>(&sql)
        .bind(&value)
        .bind(now_ts())
        .bind(id)
        .execute(store.pool())
        .await?;
    Ok(())
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

/// 删除资源。服务商连带删除它下面的模型，并把授权一起清掉。
///
/// 删除前查一遍档位映射：**任何被引用的模型都不允许删**，否则客户端会拿到
/// 指向不存在模型的档位，运行时才炸。删单个模型和删整个服务商走同一套检查。
pub async fn delete(store: &AdminStore, id: &str) -> Result<bool> {
    let children = children_of(store, id).await?;
    let mut doomed: Vec<String> = children.iter().map(|c| c.slug.clone()).collect();
    if let Some(own) = slug_of(store, id).await? {
        doomed.push(own);
    }
    let used = referenced_by_tier_profiles(store, &doomed).await?;
    if !used.is_empty() {
        anyhow::bail!(
            "这些模型还被档位映射引用，先改档位再删：{}",
            used.join("、")
        );
    }

    // 先删子资源与授权，再删自己：老库没有外键，顺序反了会留孤儿
    for child in &children {
        let sql = store.sql("DELETE FROM resource_grants WHERE resource_id = ?");
        sqlx::query::<Any>(&sql).bind(&child.id).execute(store.pool()).await?;
        let sql = store.sql("DELETE FROM resources WHERE id = ?");
        sqlx::query::<Any>(&sql).bind(&child.id).execute(store.pool()).await?;
    }
    let sql = store.sql("DELETE FROM resource_grants WHERE resource_id = ?");
    sqlx::query::<Any>(&sql).bind(id).execute(store.pool()).await?;

    let sql = store.sql("DELETE FROM resources WHERE id = ?");
    Ok(sqlx::query::<Any>(&sql)
        .bind(id)
        .execute(store.pool())
        .await?
        .rows_affected()
        > 0)
}

async fn slug_of(store: &AdminStore, id: &str) -> Result<Option<String>> {
    let sql = store.sql("SELECT slug FROM resources WHERE id = ?");
    let row: Option<(String,)> = sqlx::query_as::<Any, (String,)>(&sql)
        .bind(id)
        .fetch_optional(store.pool())
        .await?;
    Ok(row.map(|r| r.0))
}

async fn children_of(store: &AdminStore, id: &str) -> Result<Vec<ChildResource>> {
    let sql = store.sql("SELECT id, slug FROM resources WHERE parent_id = ?");
    let rows: Vec<(String, String)> = sqlx::query_as::<Any, (String, String)>(&sql)
        .bind(id)
        .fetch_all(store.pool())
        .await?;
    Ok(rows
        .into_iter()
        .map(|(id, slug)| ChildResource { id, slug })
        .collect())
}

struct ChildResource {
    id: String,
    slug: String,
}

/// 档位映射里引用了哪些 slug。payload 是 JSON，按 `"model":"<slug>"` 精确匹配，
/// 不做前缀或子串猜测。
async fn referenced_by_tier_profiles(
    store: &AdminStore,
    slugs: &[String],
) -> Result<Vec<String>> {
    if slugs.is_empty() {
        return Ok(Vec::new());
    }
    let sql = store.sql("SELECT payload_json FROM resources WHERE kind = 'tier_profile'");
    let payloads: Vec<(String,)> = sqlx::query_as::<Any, (String,)>(&sql)
        .fetch_all(store.pool())
        .await?;
    let mut hits: Vec<String> = Vec::new();
    for (payload,) in payloads {
        for slug in slugs {
            let needle = format!("\"model\":\"{slug}\"");
            let spaced = format!("\"model\": \"{slug}\"");
            if (payload.contains(&needle) || payload.contains(&spaced)) && !hits.contains(slug) {
                hits.push(slug.clone());
            }
        }
    }
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::{decrypt_secret, encrypt_secret};
    use crate::schema::DEFAULT_TENANT;

    async fn store() -> AdminStore {
        AdminStore::connect_in_memory().await.expect("store")
    }

    async fn make_provider(store: &AdminStore, slug: &str) -> Resource {
        create(
            store,
            DEFAULT_TENANT,
            KIND_PROVIDER,
            slug,
            "阿里云百炼",
            "",
            None,
            "{}",
            None,
        )
        .await
        .expect("provider")
    }

    async fn make_model(store: &AdminStore, provider: &Resource, slug: &str) -> Resource {
        create(
            store,
            DEFAULT_TENANT,
            KIND_MODEL,
            slug,
            slug,
            "",
            Some(&provider.id),
            "{}",
            None,
        )
        .await
        .expect("model")
    }

    /// 删服务商要连带删模型，否则模型变成 parent_id 指向空资源的孤儿，
    /// 还会继续出现在档位矩阵的模型下拉里。
    #[tokio::test]
    async fn delete_provider_cascades_to_models() {
        let store = store().await;
        let p = make_provider(&store, "bailian").await;
        let m1 = make_model(&store, &p, "qwen-plus").await;
        let m2 = make_model(&store, &p, "qwen-max").await;

        assert!(delete(&store, &p.id).await.expect("delete provider"));
        assert!(get(&store, &m1.id).await.expect("query").is_none(), "模型应被一起删掉");
        assert!(get(&store, &m2.id).await.expect("query").is_none(), "模型应被一起删掉");
    }

    /// 被档位映射引用的模型不能删，删服务商也不行 —— 否则客户端拿到死档位。
    #[tokio::test]
    async fn delete_refuses_models_referenced_by_tier_profile() {
        let store = store().await;
        let p = make_provider(&store, "bailian").await;
        let m = make_model(&store, &p, "qwen-plus").await;
        create(
            &store,
            DEFAULT_TENANT,
            KIND_TIER_PROFILE,
            "default",
            "默认档位映射",
            "",
            None,
            r#"{"agentModeLlm":{"general":{"fast":{"providerId":"bailian","model":"qwen-plus"}}}}"#,
            None,
        )
        .await
        .expect("tier profile");

        let err = delete(&store, &m.id).await.expect_err("应拒绝");
        assert!(err.to_string().contains("qwen-plus"), "报错要点名是哪个模型");

        let err = delete(&store, &p.id).await.expect_err("删服务商也要拒绝");
        assert!(err.to_string().contains("qwen-plus"));

        // 资源都还在
        assert!(get(&store, &m.id).await.expect("query").is_some());
        assert!(get(&store, &p.id).await.expect("query").is_some());
    }

    #[tokio::test]
    async fn activation_defaults_optional_and_is_validated() {
        let store = store().await;
        let skill = create_skill(&store, "travel").await;
        assert_eq!(skill.activation, ACTIVATION_OPTIONAL);

        set_activation(&store, &skill.id, ACTIVATION_DEFAULT_ON)
            .await
            .expect("set");
        let reloaded = get(&store, &skill.id).await.expect("get").expect("some");
        assert_eq!(reloaded.activation, ACTIVATION_DEFAULT_ON);

        // 未实现 forced，写入必须被拒（不是静默接受）。
        let err = set_activation(&store, &skill.id, "forced")
            .await
            .expect_err("forced 不支持");
        assert!(err.to_string().contains("activation"));
        assert_eq!(normalize_activation("").expect("empty"), ACTIVATION_OPTIONAL);
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
