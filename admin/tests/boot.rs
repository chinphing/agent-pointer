//! 启动序列：种子数据必须真的种进去。
//!
//! 私有版曾经自己抄了一份启动顺序、漏掉 `seed::seed`，结果控制面初始化完
//! 「模型服务」一条都没有。这组测试钉住「prepare 之后就一定有内置服务商」。

use pointer_admin::boot::Boot;
use pointer_admin::config::AdminConfig;
use pointer_admin::resources::{self, KIND_MODEL, KIND_PROVIDER};

fn temp_db(tag: &str) -> String {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "pointer-admin-boot-{tag}-{}-{}.db",
        std::process::id(),
        pointer_admin::db::random_id()
    ));
    format!("sqlite:{}", path.display())
}

fn config() -> AdminConfig {
    AdminConfig {
        bootstrap_user: Some("admin".to_string()),
        bootstrap_password: Some("pw-123456".to_string()),
        ..Default::default()
    }
}

async fn provider_slugs(url: &str) -> Vec<String> {
    let store = pointer_admin::db::AdminStore::connect(url)
        .await
        .expect("connect");
    let mut slugs: Vec<String> = resources::list(&store, "default", Some(KIND_PROVIDER))
        .await
        .expect("list providers")
        .into_iter()
        .map(|r| r.slug)
        .collect();
    slugs.sort();
    slugs
}

#[tokio::test]
async fn prepare_seeds_builtin_providers_models_and_prices() {
    let url = temp_db("seed");
    let store = Boot::new(config())
        .with_db_url(&url)
        .prepare()
        .await
        .expect("prepare");

    let mut slugs: Vec<String> = resources::list(&store, "default", Some(KIND_PROVIDER))
        .await
        .expect("list providers")
        .into_iter()
        .map(|r| r.slug)
        .collect();
    slugs.sort();
    assert_eq!(
        slugs,
        vec!["deepseek", "doubao", "qwen"],
        "内置三家服务商应全部种上（seeded: true 的那三个）"
    );

    let models = resources::list(&store, "default", Some(KIND_MODEL))
        .await
        .expect("list models");
    assert!(
        models.len() >= 25,
        "qwen(18) + deepseek(3) + 豆包(9) 至少 25 个模型，实际 {}",
        models.len()
    );
    assert!(
        models.iter().any(|m| m.slug.starts_with("qwen")),
        "模型清单里应有 qwen 系列"
    );
    assert!(
        models.iter().any(|m| m.slug.starts_with("deepseek")),
        "模型清单里应有 deepseek 系列"
    );

    // 单价也要补上，否则计费按默认倍率算，账是错的。
    let price = pointer_admin::billing::price_for(&store, "default", "deepseek-v4-flash")
        .await
        .expect("price query");
    assert!(price.is_some(), "deepseek-v4-flash 应有单价");
}

#[tokio::test]
async fn prepare_is_idempotent() {
    let url = temp_db("idem");
    Boot::new(config())
        .with_db_url(&url)
        .prepare()
        .await
        .expect("first prepare");
    let first = provider_slugs(&url).await;

    // 第二次启动：服务商表非空，播种应整体跳过，不会重复建。
    Boot::new(config())
        .with_db_url(&url)
        .prepare()
        .await
        .expect("second prepare");
    let second = provider_slugs(&url).await;

    assert_eq!(first, second, "重启不应重复播种");
    assert_eq!(first.len(), 3, "仍应只有三家");
}

#[tokio::test]
async fn prepare_creates_first_admin_with_super_admin() {
    let url = temp_db("admin");
    let store = Boot::new(config())
        .with_db_url(&url)
        .prepare()
        .await
        .expect("prepare");

    let users = pointer_admin::users::list(&store, "default")
        .await
        .expect("list users");
    assert_eq!(users.len(), 1, "应建出一个管理员");
    assert_eq!(users[0].username, "admin");
    let perms = pointer_admin::rbac::permissions_for_user(&store, &users[0].id)
        .await
        .expect("permissions");
    assert!(
        perms.contains("system:read"),
        "首个管理员必须是 super_admin，实际权限：{perms:?}"
    );
}
