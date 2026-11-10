//! Pointer 企业控制面进程。
//!
//! 启动顺序：读配置 → 连库迁移 → 种子数据（首个管理员 / OAuth 客户端）→ 挂路由 → 监听。
//! 单独一个进程，server 与桌面客户端都是它的 OAuth 客户端。

use std::net::SocketAddr;

use anyhow::{Context, Result};

use pointer_admin::config::AdminConfig;
use pointer_admin::db::AdminStore;
use pointer_admin::oauth;
use pointer_admin::seed;
use pointer_admin::rbac;
use pointer_admin::state::AppState;
use pointer_admin::users;

async fn bootstrap(store: &AdminStore, cfg: &AdminConfig) -> Result<()> {
    let existing = users::list(store, &cfg.tenant_id).await?;
    if existing.is_empty() {
        match (
            cfg.bootstrap_user.as_deref(),
            cfg.bootstrap_password.as_deref(),
        ) {
            (Some(username), Some(password)) => {
                let user = users::create(store, &cfg.tenant_id, username, username, password)
                    .await
                    .context("创建首个管理员失败")?;
                rbac::assign_role(store, &cfg.tenant_id, &user.id, rbac::SUPER_ADMIN)
                    .await
                    .context("授予 super_admin 失败")?;
                log::info!("已创建首个管理员账号：{username}");
            }
            _ => log::warn!(
                "库里还没有任何账号。请设置 POINTER_ADMIN_BOOTSTRAP_USER 与 \
                 POINTER_ADMIN_BOOTSTRAP_PASSWORD 后重启，否则没人能登录。"
            ),
        }
    }

    match cfg.oauth_secret.as_deref() {
        Some(secret) => {
            oauth::clients::upsert(
                store,
                oauth::clients::CLIENT_SERVER,
                secret,
                oauth::clients::KIND_SERVER,
                &[cfg.server_redirect_uri.as_str()],
            )
            .await?;
            oauth::clients::upsert(
                store,
                oauth::clients::CLIENT_DESKTOP,
                secret,
                oauth::clients::KIND_DESKTOP,
                &[],
            )
            .await?;
        }
        None => log::warn!(
            "未配置 POINTER_ADMIN_OAUTH_SECRET：/oauth/* 与 /auth/app/token 会返回 503"
        ),
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let config = AdminConfig::from_env();
    let db_url = config.db_url();
    log::info!("控制面数据库：{db_url}");
    // 密钥模式决定密文能不能跨机器解，部署时最容易搞错，启动就说清楚。
    if std::env::var(pointer_admin::crypto::MASTER_KEY_ENV)
        .ok()
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false)
    {
        log::info!("企业密钥加密：使用 {}（可跨实例解密）", pointer_admin::crypto::MASTER_KEY_ENV);
    } else {
        log::warn!(
            "企业密钥加密：未配置 {}，按本机标识派生——换机/多实例部署后旧密钥解不开",
            pointer_admin::crypto::MASTER_KEY_ENV
        );
    }

    let store = AdminStore::connect(&db_url)
        .await
        .context("连接控制面数据库失败")?;
    bootstrap(&store, &config).await?;

    // `pointer-admin seed` —— 装一遍内置服务商（qwen / deepseek / 豆包）与默认档位。
    // 幂等：已存在的 slug 跳过，不覆盖管理员改过的配置。
    if std::env::args().any(|a| a == "seed") {
        let report = seed::seed(&store, pointer_admin::schema::DEFAULT_TENANT).await?;
        println!(
            "服务商：新建 {} 个，跳过 {} 个；模型新建 {} 个；档位映射：{}",
            report.providers_created,
            report.providers_skipped,
            report.models_created,
            if report.tier_created { "已建" } else { "已存在" }
        );
        return Ok(());
    }

    // 全新库：把内置的三家服务商（百炼 / DeepSeek / 豆包）和默认档位种上，
    // 否则管理员登录后是一片空白。库里已经有服务商就不动 —— 管理员删掉的
    // 不该在重启后又冒出来。
    if pointer_admin::resources::list(&store, &config.tenant_id, Some(pointer_admin::resources::KIND_PROVIDER))
        .await?
        .is_empty()
    {
        match seed::seed(&store, &config.tenant_id).await {
            Ok(r) => log::info!(
                "初始化服务商：新建 {} 个，模型 {} 个",
                r.providers_created,
                r.models_created
            ),
            Err(e) => log::warn!("初始化服务商失败（不影响启动）：{e}"),
        }
    }

    // 单价每次启动都补一次：库可能是早就建好的，服务商播种不会再跑。
    match seed::seed_prices(&store, &config.tenant_id).await {
        Ok(0) => {}
        Ok(n) => log::info!("初始化模型单价：补齐 {n} 个（照抄官网费率）"),
        Err(e) => log::warn!("初始化模型单价失败（不影响启动）：{e}"),
    }

    let state = AppState::new(store, config.clone());
    let app = pointer_admin::app::build_app(state);

    if let Some(dir) = &config.static_dir {
        log::info!("静态资源目录：{}", dir.display());
    }

    let addr: SocketAddr = config
        .addr
        .parse()
        .with_context(|| format!("地址格式不正确：{}", config.addr))?;
    if !config.captcha_required {
        log::warn!(
            "登录验证码已关闭（POINTER_ADMIN_CAPTCHA=off）——只应在自动化测试或内网联调时这么配"
        );
    }

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("监听 {addr} 失败"))?;

    log::info!("控制面已启动：http://{addr}");
    axum::serve(listener, app).await?;
    Ok(())
}
