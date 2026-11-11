//! 成员账号。
//!
//! 密码只以 argon2id 哈希入库；`User` 结构体刻意不含 `password_hash`，避免误传给接口层。

use anyhow::{anyhow, Result};
use sqlx::any::AnyRow;
use sqlx::{Any, Row};

use crate::db::{now_ts, random_id, AdminStore};
use crate::password;

pub const STATUS_ACTIVE: &str = "active";
pub const STATUS_DISABLED: &str = "disabled";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct User {
    pub id: String,
    pub tenant_id: String,
    pub username: String,
    pub display_name: String,
    pub status: String,
    pub created_at: i64,
    pub last_login_at: Option<i64>,
    /// 手机号：开源仓只负责存与维护；私有仓的短信登录复用这一列。
    pub phone: Option<String>,
}

impl User {
    pub fn is_active(&self) -> bool {
        self.status == STATUS_ACTIVE
    }
}

const COLS: &str = "id, tenant_id, username, display_name, status, created_at, last_login_at, phone";

fn map_user(row: &AnyRow) -> Result<User> {
    Ok(User {
        id: row.try_get(0)?,
        tenant_id: row.try_get(1)?,
        username: row.try_get(2)?,
        display_name: row.try_get(3)?,
        status: row.try_get(4)?,
        created_at: row.try_get(5)?,
        last_login_at: row.try_get(6)?,
        phone: row.try_get(7)?,
    })
}

fn normalize_username(raw: &str) -> Result<String> {
    let name = raw.trim();
    if name.is_empty() {
        return Err(anyhow!("username must not be empty"));
    }
    if name.chars().count() > 64 {
        return Err(anyhow!("username too long (max 64)"));
    }
    if name.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(anyhow!("username must not contain whitespace"));
    }
    Ok(name.to_string())
}

pub async fn create(
    store: &AdminStore,
    tenant_id: &str,
    username: &str,
    display_name: &str,
    plain_password: &str,
) -> Result<User> {
    let username = normalize_username(username)?;
    let display_name = {
        let d = display_name.trim();
        if d.is_empty() {
            username.clone()
        } else {
            d.to_string()
        }
    };
    if find_by_username(store, tenant_id, &username).await?.is_some() {
        return Err(anyhow!("username already exists: {username}"));
    }
    let password_hash = password::hash_password(plain_password)?;
    let id = random_id();
    let sql = store.sql(
        "INSERT INTO users (id, tenant_id, username, display_name, password_hash, status, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    );
    sqlx::query::<Any>(&sql)
        .bind(&id)
        .bind(tenant_id)
        .bind(&username)
        .bind(&display_name)
        .bind(password_hash)
        .bind(STATUS_ACTIVE)
        .bind(now_ts())
        .execute(store.pool())
        .await?;

    // 新用户默认给「成员」角色。它没有任何权限（`builtin_permissions` 返回空），
    // 但用户在界面上有个身份，不会显示成「无角色」；要提权由管理员在成员页分配。
    //
    // 先 `ensure_role` 再分配：内置角色通常是 `seed_builtin_roles` 播的，但建号
    // 不该假设它已经跑过——缺角色时把用户创建整体搞失败，比没有角色更糟。
    crate::rbac::ensure_role(store, tenant_id, crate::rbac::ROLE_MEMBER, "成员", true).await?;
    crate::rbac::assign_role(store, tenant_id, &id, crate::rbac::ROLE_MEMBER).await?;

    get(store, &id)
        .await?
        .ok_or_else(|| anyhow!("user vanished right after insert"))
}

pub async fn find_by_username(
    store: &AdminStore,
    tenant_id: &str,
    username: &str,
) -> Result<Option<User>> {
    let sql = store.sql(&format!(
        "SELECT {COLS} FROM users WHERE tenant_id = ? AND username = ?"
    ));
    let row = sqlx::query::<Any>(&sql)
        .bind(tenant_id)
        .bind(username.trim())
        .fetch_optional(store.pool())
        .await?;
    match row {
        Some(row) => Ok(Some(map_user(&row)?)),
        None => Ok(None),
    }
}

pub async fn get(store: &AdminStore, user_id: &str) -> Result<Option<User>> {
    let sql = store.sql(&format!("SELECT {COLS} FROM users WHERE id = ?"));
    let row = sqlx::query::<Any>(&sql)
        .bind(user_id)
        .fetch_optional(store.pool())
        .await?;
    match row {
        Some(row) => Ok(Some(map_user(&row)?)),
        None => Ok(None),
    }
}

pub async fn list(store: &AdminStore, tenant_id: &str) -> Result<Vec<User>> {
    let sql = store.sql(&format!(
        "SELECT {COLS} FROM users WHERE tenant_id = ? ORDER BY username"
    ));
    let rows = sqlx::query::<Any>(&sql)
        .bind(tenant_id)
        .fetch_all(store.pool())
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        out.push(map_user(row)?);
    }
    Ok(out)
}

/// 只取「id → 展示名」，供用量报表给成员列贴名字。
///
/// 走 `WHERE id IN (...)`：报表只关心出现过的那几个成员，
/// 没必要像 [`list`] 那样把整张用户表（含密码哈希）读出来。
pub async fn display_names_by_id(
    store: &AdminStore,
    tenant_id: &str,
    ids: &[String],
) -> Result<std::collections::HashMap<String, String>> {
    let mut out = std::collections::HashMap::new();
    if ids.is_empty() {
        return Ok(out);
    }
    let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
    let sql = store.sql(&format!(
        "SELECT id, username, display_name FROM users
          WHERE tenant_id = ? AND id IN ({placeholders})"
    ));
    let mut query = sqlx::query::<Any>(&sql).bind(tenant_id);
    for id in ids {
        query = query.bind(id);
    }
    for row in query.fetch_all(store.pool()).await? {
        let id: String = row.try_get(0)?;
        let username: String = row.try_get(1)?;
        let display_name: String = row.try_get(2)?;
        let name = if display_name.trim().is_empty() {
            username
        } else {
            display_name
        };
        out.insert(id, name);
    }
    Ok(out)
}

pub async fn set_status(store: &AdminStore, user_id: &str, status: &str) -> Result<()> {
    if status != STATUS_ACTIVE && status != STATUS_DISABLED {
        return Err(anyhow!("unknown user status: {status}"));
    }
    let sql = store.sql("UPDATE users SET status = ? WHERE id = ?");
    let changed = sqlx::query::<Any>(&sql)
        .bind(status)
        .bind(user_id)
        .execute(store.pool())
        .await?
        .rows_affected();
    if changed == 0 {
        return Err(anyhow!("user not found: {user_id}"));
    }
    Ok(())
}

/// 设置/清空手机号。空串视为清空（NULL），唯一索引按租户。
pub async fn set_phone(
    store: &AdminStore,
    tenant_id: &str,
    user_id: &str,
    phone: Option<&str>,
) -> Result<()> {
    let normalized = phone
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(|p| p.to_string());
    if let Some(p) = &normalized {
        let sql = store.sql("SELECT id FROM users WHERE tenant_id = ? AND phone = ? AND id <> ?");
        let taken = sqlx::query::<Any>(&sql)
            .bind(tenant_id)
            .bind(p)
            .bind(user_id)
            .fetch_optional(store.pool())
            .await?;
        if taken.is_some() {
            return Err(anyhow!("手机号已被其他成员占用：{p}"));
        }
    }
    let sql = store.sql("UPDATE users SET phone = ? WHERE id = ?");
    sqlx::query::<Any>(&sql)
        .bind(normalized)
        .bind(user_id)
        .execute(store.pool())
        .await?;
    Ok(())
}

pub async fn reset_password(
    store: &AdminStore,
    user_id: &str,
    plain_password: &str,
) -> Result<()> {
    let password_hash = password::hash_password(plain_password)?;
    let sql = store.sql("UPDATE users SET password_hash = ? WHERE id = ?");
    let changed = sqlx::query::<Any>(&sql)
        .bind(password_hash)
        .bind(user_id)
        .execute(store.pool())
        .await?
        .rows_affected();
    if changed == 0 {
        return Err(anyhow!("user not found: {user_id}"));
    }
    Ok(())
}

/// 按用户 id 校验密码。自助改密码用：只需要确认「旧密码对不对」，
/// 不碰 `last_login_at`（`authenticate` 会写，那属于登录语义）。
pub async fn verify_password_for(
    store: &AdminStore,
    user_id: &str,
    plain_password: &str,
) -> Result<bool> {
    let sql = store.sql("SELECT password_hash FROM users WHERE id = ?");
    let row = sqlx::query::<Any>(&sql)
        .bind(user_id)
        .fetch_optional(store.pool())
        .await?;
    let Some(row) = row else { return Ok(false) };
    let hash: String = row.try_get(0)?;
    Ok(password::verify_password(&hash, plain_password))
}

pub async fn touch_login(store: &AdminStore, user_id: &str) -> Result<()> {
    let sql = store.sql("UPDATE users SET last_login_at = ? WHERE id = ?");
    sqlx::query::<Any>(&sql)
        .bind(now_ts())
        .bind(user_id)
        .execute(store.pool())
        .await?;
    Ok(())
}

/// 校验账密。用户不存在、被禁用、密码错误一律返回 `None`（不泄露是哪一种）。
pub async fn authenticate(
    store: &AdminStore,
    tenant_id: &str,
    username: &str,
    plain_password: &str,
) -> Result<Option<User>> {
    let sql = store.sql(
        "SELECT id, password_hash, status FROM users WHERE tenant_id = ? AND username = ?",
    );
    let row = sqlx::query::<Any>(&sql)
        .bind(tenant_id)
        .bind(username.trim())
        .fetch_optional(store.pool())
        .await?;

    let Some(row) = row else { return Ok(None) };
    let id: String = row.try_get(0)?;
    let hash: String = row.try_get(1)?;
    let status: String = row.try_get(2)?;

    if status != STATUS_ACTIVE {
        log::warn!("admin auth: user {id} is {status}");
        return Ok(None);
    }
    if !password::verify_password(&hash, plain_password) {
        return Ok(None);
    }
    touch_login(store, &id).await?;
    get(store, &id).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::AdminStore;
    use crate::schema::DEFAULT_TENANT;

    async fn store() -> AdminStore {
        AdminStore::connect_in_memory().await.expect("store")
    }

    #[tokio::test]
    async fn create_and_fetch() {
        let store = store().await;
        let u = create(&store, DEFAULT_TENANT, "  alice  ", "", "pw-123456")
            .await
            .expect("create");
        assert_eq!(u.username, "alice");
        assert_eq!(u.display_name, "alice");
        assert_eq!(u.status, STATUS_ACTIVE);
        assert!(u.last_login_at.is_none());

        let found = find_by_username(&store, DEFAULT_TENANT, "alice")
            .await
            .expect("find")
            .expect("present");
        assert_eq!(found.id, u.id);
        assert_eq!(get(&store, &u.id).await.expect("get").expect("present"), u);
    }

    #[tokio::test]
    async fn duplicate_username_rejected() {
        let store = store().await;
        create(&store, DEFAULT_TENANT, "bob", "Bob", "pw-123456")
            .await
            .expect("first");
        let err = create(&store, DEFAULT_TENANT, "bob", "Bob2", "pw-123456")
            .await
            .expect_err("dup");
        assert!(err.to_string().contains("already exists"), "got: {err}");
    }

    #[tokio::test]
    async fn invalid_usernames_rejected() {
        let store = store().await;
        let long = "x".repeat(65);
        for bad in ["", "   ", "a b", long.as_str()] {
            assert!(
                create(&store, DEFAULT_TENANT, bad, "", "pw-123456")
                    .await
                    .is_err(),
                "expected reject for {bad:?}"
            );
        }
    }

    #[tokio::test]
    async fn password_is_not_stored_in_plaintext() {
        let store = store().await;
        let u = create(&store, DEFAULT_TENANT, "carol", "", "pw-123456")
            .await
            .expect("create");
        let sql = store.sql("SELECT password_hash FROM users WHERE id = ?");
        let row = sqlx::query::<Any>(&sql)
            .bind(&u.id)
            .fetch_one(store.pool())
            .await
            .expect("hash row");
        let hash: String = row.try_get(0).expect("hash");
        assert!(hash.starts_with("$argon2id$"), "got: {hash}");
        assert!(!hash.contains("pw-123456"));
    }

    #[tokio::test]
    async fn authenticate_ok_and_updates_last_login() {
        let store = store().await;
        let u = create(&store, DEFAULT_TENANT, "dave", "", "pw-123456")
            .await
            .expect("create");
        let authed = authenticate(&store, DEFAULT_TENANT, "dave", "pw-123456")
            .await
            .expect("auth")
            .expect("ok");
        assert_eq!(authed.id, u.id);
        assert!(authed.last_login_at.is_some());
    }

    #[tokio::test]
    async fn authenticate_rejects_wrong_password_and_disabled_user() {
        let store = store().await;
        let u = create(&store, DEFAULT_TENANT, "erin", "", "pw-123456")
            .await
            .expect("create");
        assert!(authenticate(&store, DEFAULT_TENANT, "erin", "wrong")
            .await
            .expect("auth")
            .is_none());

        set_status(&store, &u.id, STATUS_DISABLED)
            .await
            .expect("disable");
        assert!(
            authenticate(&store, DEFAULT_TENANT, "erin", "pw-123456")
                .await
                .expect("auth")
                .is_none(),
            "disabled user must not authenticate"
        );
    }

    #[tokio::test]
    async fn authenticate_unknown_user_is_none() {
        let store = store().await;
        assert!(authenticate(&store, DEFAULT_TENANT, "ghost", "pw-123456")
            .await
            .expect("auth")
            .is_none());
    }

    #[tokio::test]
    async fn reset_password_takes_effect() {
        let store = store().await;
        let u = create(&store, DEFAULT_TENANT, "frank", "", "old-password")
            .await
            .expect("create");
        reset_password(&store, &u.id, "new-password")
            .await
            .expect("reset");
        assert!(authenticate(&store, DEFAULT_TENANT, "frank", "old-password")
            .await
            .expect("auth")
            .is_none());
        assert!(
            authenticate(&store, DEFAULT_TENANT, "frank", "new-password")
                .await
                .expect("auth")
                .is_some()
        );
    }

    #[tokio::test]
    async fn set_status_rejects_unknown_value_and_missing_user() {
        let store = store().await;
        let u = create(&store, DEFAULT_TENANT, "gina", "", "pw-123456")
            .await
            .expect("create");
        assert!(set_status(&store, &u.id, "archived").await.is_err());
        assert!(set_status(&store, "missing", STATUS_DISABLED).await.is_err());
    }

    #[tokio::test]
    async fn list_is_tenant_scoped_and_sorted() {
        let store = store().await;
        create(&store, DEFAULT_TENANT, "zoe", "", "pw-123456")
            .await
            .expect("zoe");
        create(&store, DEFAULT_TENANT, "amy", "", "pw-123456")
            .await
            .expect("amy");
        create(&store, "other", "anna", "", "pw-123456")
            .await
            .expect("other tenant");
        let names: Vec<String> = list(&store, DEFAULT_TENANT)
            .await
            .expect("list")
            .into_iter()
            .map(|u| u.username)
            .collect();
        assert_eq!(names, vec!["amy".to_string(), "zoe".to_string()]);
    }

    /// 手机号按租户唯一；空串视为清空；不同租户可以重复。
    #[tokio::test]
    async fn phone_is_unique_per_tenant() {
        let store = store().await;
        let amy = create(&store, DEFAULT_TENANT, "amy", "", "pw-123456")
            .await
            .expect("amy");
        let bob = create(&store, DEFAULT_TENANT, "bob", "", "pw-123456")
            .await
            .expect("bob");

        set_phone(&store, DEFAULT_TENANT, &amy.id, Some("13800000001"))
            .await
            .expect("set amy");

        let dup = set_phone(&store, DEFAULT_TENANT, &bob.id, Some("13800000001")).await;
        assert!(dup.is_err(), "同租户手机号不能重复");

        // 换一个租户可以用同一个号（用户的 tenant_id 不变，只是冲突检查按租户）
        let carol = create(&store, "other", "carol", "", "pw-123456")
            .await
            .expect("carol");
        set_phone(&store, "other", &carol.id, Some("13800000001"))
            .await
            .expect("cross tenant");

        // 空串 / 纯空白视为清空
        set_phone(&store, DEFAULT_TENANT, &amy.id, Some("   "))
            .await
            .expect("clear");
        let after = get(&store, &amy.id).await.expect("get").expect("present");
        assert_eq!(after.phone, None);
    }
}
