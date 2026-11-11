//! Pointer 企业控制面进程。
//!
//! 启动顺序收在 [`pointer_admin::boot`] 里，开源版与私有版共用同一份 ——
//! 这里只负责解析命令行和配置。

use anyhow::{Context, Result};

use pointer_admin::boot::Boot;
use pointer_admin::config::AdminConfig;
use pointer_admin::db::AdminStore;
use pointer_admin::schema::DEFAULT_TENANT;
use pointer_admin::seed;

#[tokio::main]
async fn main() -> Result<()> {
    pointer_admin::boot::init_logging();

    let config = AdminConfig::from_env();

    // `pointer-admin seed` —— 装一遍内置服务商（qwen / deepseek / 豆包）与默认档位。
    // 幂等：已存在的 slug 跳过，不覆盖管理员改过的配置。
    if std::env::args().any(|a| a == "seed") {
        let store = AdminStore::connect(&config.db_url())
            .await
            .context("连接控制面数据库失败")?;
        let report = seed::seed(&store, DEFAULT_TENANT).await?;
        println!(
            "服务商：新建 {} 个，跳过 {} 个；模型新建 {} 个；档位映射：{}",
            report.providers_created,
            report.providers_skipped,
            report.models_created,
            if report.tier_created { "已建" } else { "已存在" }
        );
        return Ok(());
    }

    let boot = Boot::new(config);
    let store = boot.prepare().await?;
    boot.serve(store).await
}
