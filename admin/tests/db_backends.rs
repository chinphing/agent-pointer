//! 真实后端冒烟测试。
//!
//! 单元测试跑的是 SQLite 内存库；PostgreSQL / MySQL 需要显式给一个**专用测试库**：
//!
//! ```bash
//! POINTER_ADMIN_TEST_DB_URL=postgres://pointer:pointer@127.0.0.1:5432/pointer_admin_test \
//!   cargo test -p pointer-admin --test db_backends -- --nocapture
//!
//! POINTER_ADMIN_TEST_DB_URL='mysql://pointer:pointer@127.0.0.1:3306/pointer_admin_test' \
//!   cargo test -p pointer-admin --test db_backends -- --nocapture
//! ```
//!
//! 未设置环境变量时直接跳过（打印一行 skipped），不影响默认 CI。
//! 测试会建表、建用户，最后删掉自己建的用户；请勿指向生产库。

use pointer_admin::db::{random_id, AdminStore, Backend};
use pointer_admin::{rbac, schema, sessions, users};
use sqlx::Any;

#[tokio::test]
async fn smoke_against_configured_backend() {
    let Ok(url) = std::env::var("POINTER_ADMIN_TEST_DB_URL") else {
        eprintln!("skipped: POINTER_ADMIN_TEST_DB_URL not set");
        return;
    };
    if url.trim().is_empty() {
        eprintln!("skipped: POINTER_ADMIN_TEST_DB_URL is empty");
        return;
    }

    let store = AdminStore::connect(&url).await.expect("connect + migrate");
    eprintln!("backend = {:?}", store.backend());
    assert!(matches!(
        store.backend(),
        Backend::Sqlite | Backend::Postgres | Backend::MySql
    ));

    // 迁移幂等：再跑一次不应报错。
    schema::migrate(&store).await.expect("migrate twice");

    let tenant = schema::DEFAULT_TENANT;
    let username = format!("smoke_{}", random_id());
    let user = users::create(&store, tenant, &username, "冒烟用户", "pw-123456")
        .await
        .expect("create user");

    let authed = users::authenticate(&store, tenant, &username, "pw-123456")
        .await
        .expect("authenticate")
        .expect("user should authenticate");
    assert_eq!(authed.id, user.id);

    rbac::assign_role(&store, tenant, &user.id, "viewer")
        .await
        .expect("assign role");
    let perms = rbac::permissions_for_user(&store, &user.id)
        .await
        .expect("permissions");
    assert!(perms.contains("audit:read"), "viewer should read audit");
    assert!(!perms.contains("skill:write"), "viewer must not write");

    let token = sessions::create(&store, &user.id, sessions::KIND_ADMIN, 60)
        .await
        .expect("create session");
    let session = sessions::lookup(&store, &token)
        .await
        .expect("lookup")
        .expect("session should exist");
    assert_eq!(session.user_id, user.id);
    assert!(sessions::revoke(&store, &token).await.expect("revoke"));

    // 聚合读回 i64：MySQL 的 SUM() 返回 DECIMAL，sqlx::Any 解不了
    // （`Any driver does not support MySql type NewDecimal`）。
    // 控制面板登录后第一屏就调这两个，漏了这条 MySQL 上直接 500。
    seed_usage_and_ledger(&store, tenant, &user.id).await;
    let agg = pointer_admin::usage::totals(
        &store,
        tenant,
        "2026-01-01",
        "2026-01-01",
        Some(&user.id),
    )
    .await
    .expect("usage totals must decode on this backend");
    assert_eq!(agg.prompt, 120, "prompt tokens");
    assert_eq!(agg.completion, 30, "completion tokens");
    assert_eq!(agg.total(), 150, "total tokens");

    let (spent, calls) = pointer_admin::billing::spend_in_range(&store, &user.id, 0, i64::MAX)
        .await
        .expect("spend must decode on this backend");
    assert_eq!(spent, 700, "spent micro (negated back to positive)");
    assert_eq!(calls, 1, "one charge");

    // 清理本次造的数据（usage_daily / wallet_ledger 没有外键级联，要自己删）。
    for table in ["usage_daily", "wallet_ledger"] {
        let sql = store.sql(&format!("DELETE FROM {table} WHERE user_id = ?"));
        sqlx::query::<Any>(&sql)
            .bind(&user.id)
            .execute(store.pool())
            .await
            .expect("cleanup seeded rows");
    }
    // 清理本次建的用户（外键级联清掉 user_roles / sessions）。
    let sql = store.sql("DELETE FROM users WHERE id = ?");
    sqlx::query::<Any>(&sql)
        .bind(&user.id)
        .execute(store.pool())
        .await
        .expect("cleanup user");
}

/// 造一行当日用量 + 一笔消费流水，供聚合读取断言。
async fn seed_usage_and_ledger(
    store: &AdminStore,
    tenant: &str,
    user_id: &str,
) {
    let sql = store.sql(
        "INSERT INTO usage_daily
            (id, tenant_id, day, user_id, model, prompt_tokens, completion_tokens,
             thinking_tokens, cached_tokens)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    );
    sqlx::query::<Any>(&sql)
        .bind(random_id())
        .bind(tenant)
        .bind("2026-01-01")
        .bind(user_id)
        .bind("test-model")
        .bind(120_i64)
        .bind(30_i64)
        .bind(0_i64)
        .bind(0_i64)
        .execute(store.pool())
        .await
        .expect("seed usage_daily");

    let sql = store.sql(
        "INSERT INTO wallet_ledger
            (id, tenant_id, user_id, kind, amount_micro, balance_after_micro,
             model, note, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    );
    sqlx::query::<Any>(&sql)
        .bind(random_id())
        .bind(tenant)
        .bind(user_id)
        .bind(pointer_admin::billing::KIND_CHARGE)
        // 消费流水在账本里是负数，spend_in_range 取反后返回正数
        .bind(-700_i64)
        .bind(0_i64)
        .bind("")
        .bind("smoke")
        .bind(1000_i64)
        .execute(store.pool())
        .await
        .expect("seed wallet_ledger");
}
