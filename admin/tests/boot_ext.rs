//! 扩展安装必须在共享启动序列的**迁移之前**完成。
//!
//! `AdminStore::connect` 内部就会跑 `schema::migrate`，而迁移会带上扩展注册的
//! DDL 段（版本号 9000 起）。私有版早期是在 connect 之后才 install，再补一次
//! migrate 才把私有表建出来 —— 顺序靠人记，迟早出错。现在顺序由 `Boot` 保证：
//! installer 先跑，connect 后跑。
//!
//! 这个测试用一个只建一张表的假扩展来验证：表建出来了，说明 install 确实发生在
//! 迁移之前。（独立测试文件 = 独立进程，进程级 `ext::install` 不会互相干扰。）

use anyhow::Result;
use pointer_admin::boot::Boot;
use pointer_admin::config::AdminConfig;
use pointer_admin::ext::{self, Ext};
use pointer_admin::resources::{self, KIND_PROVIDER};
use sqlx::Any;

struct MarkerExt;

impl Ext for MarkerExt {
    fn migrations(&self) -> &'static [(i64, &'static str)] {
        &[(9001, "CREATE TABLE boot_ext_marker (id INTEGER)")]
    }
}

fn temp_db(tag: &str) -> String {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "pointer-admin-bootext-{tag}-{}-{}.db",
        std::process::id(),
        pointer_admin::db::random_id()
    ));
    format!("sqlite:{}", path.display())
}

#[tokio::test]
async fn installer_runs_before_migrations_and_seed_still_happens() -> Result<()> {
    let url = temp_db("order");
    let store = Boot::new(AdminConfig::default())
        .with_db_url(&url)
        .with_ext_installer(|| {
            ext::install(Box::new(MarkerExt)).map_err(|e| anyhow::anyhow!(e))
        })
        .prepare()
        .await
        .expect("prepare");

    // 扩展的迁移段跑过了：表存在。
    let sql = "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'boot_ext_marker'";
    let rows = sqlx::query::<Any>(sql)
        .fetch_all(store.pool())
        .await
        .expect("query sqlite_master");
    assert_eq!(
        rows.len(),
        1,
        "扩展的 DDL 段没被执行 —— install 发生在迁移之后"
    );

    // 共享步骤没被扩展顶掉：服务商照样种上了。
    let providers = resources::list(&store, "default", Some(KIND_PROVIDER))
        .await
        .expect("list providers");
    assert_eq!(providers.len(), 3, "内置三家服务商仍应被播种");

    Ok(())
}
