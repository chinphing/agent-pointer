//! 控制面启动序列 —— 开源版与私有版共用这一份。
//!
//! 私有版只提供两样东西：一个扩展安装回调（[`Boot::with_ext_installer`]），
//! 以及它自己独有的、准备完之后要跑的东西（补角色、后台关单）。其余步骤
//! ——连库、迁移、首个管理员、OAuth 客户端、服务商种子、单价种子——全在这里。
//!
//! 为什么要有这一层：私有版曾经把这段启动顺序**抄了一份**到自己的 `main` 里，
//! 结果开源版后来加的两步（`seed::seed` / `seed::seed_prices`）没有跟过去。
//! 控制面初始化完「模型服务」一条都没有，客户端也拿不到企业供应商，而播种
//! 失败只是 `warn`，日志里看不出来。抄一份启动顺序，迟早会漏；共用一份不会。

use std::net::SocketAddr;

use anyhow::{Context, Result};

use crate::config::AdminConfig;
use crate::db::AdminStore;
use crate::resources::{self, KIND_PROVIDER};
use crate::state::AppState;
use crate::{oauth, rbac, seed, users};

/// 装扩展的回调。私有版在里面调 [`crate::ext::install`]。
type Installer = Box<dyn Fn() -> Result<()> + Send + Sync>;

/// 初始化日志，默认 `info` 级。
///
/// 两个 `main` 都调这一个函数。私有版曾经直接 `env_logger::init()`——没有
/// `RUST_LOG` 时默认只打 `error`，于是「初始化服务商：新建 3 个」这种启动事实
/// 一条都不打，服务商没种上也就没人看得出来。
pub fn init_logging() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .try_init();
}

/// 启动器：`Boot::new(config)` → 可选 `.with_ext_installer(..)` → `.prepare()` → `.serve(store)`。
pub struct Boot {
    config: AdminConfig,
    /// 覆盖 `config.db_url()`（测试用：指到临时库，不污染环境变量）。
    db_url: Option<String>,
    installer: Option<Installer>,
}

impl Boot {
    pub fn new(config: AdminConfig) -> Self {
        Self {
            config,
            db_url: None,
            installer: None,
        }
    }

    /// 指定数据库地址。不调用时用 [`AdminConfig::db_url`]（读 `POINTER_ADMIN_DB_URL`）。
    pub fn with_db_url(mut self, url: impl Into<String>) -> Self {
        self.db_url = Some(url.into());
        self
    }

    /// 装扩展。回调内部调 [`crate::ext::install`]。
    ///
    /// 时机固定在**连库之前**：`AdminStore::connect` 内部就会跑
    /// [`crate::schema::migrate`]，而迁移会带上扩展注册的 DDL 段（版本号 9000 起）。
    /// 装晚了私有表建不出来 —— 所以这个顺序不能交给调用方记。
    pub fn with_ext_installer(
        mut self,
        install: impl Fn() -> Result<()> + Send + Sync + 'static,
    ) -> Self {
        self.installer = Some(Box::new(install));
        self
    }

    /// 连库 → 迁移 → 首个管理员 → OAuth 客户端 → 服务商种子 → 单价种子。
    ///
    /// 返回可以直接拿去 [`Self::serve`] 的库句柄。
    pub async fn prepare(&self) -> Result<AdminStore> {
        let db_url = self
            .db_url
            .clone()
            .unwrap_or_else(|| self.config.db_url());
        log::info!("控制面数据库：{db_url}");
        self.log_crypto_mode();

        if let Some(install) = &self.installer {
            install().context("安装扩展失败")?;
        }

        let store = AdminStore::connect(&db_url)
            .await
            .context("连接控制面数据库失败")?;

        // 后台保活（等价 Druid keepAlive）：SQLite 自动跳过。
        let every = keepalive_every();
        if every.is_zero() {
            log::warn!("连接池保活已关闭（POINTER_ADMIN_KEEPALIVE_SECS=0）");
        } else {
            log::info!("连接池保活：每 {} 秒一次", every.as_secs());
        }
        store.spawn_keepalive(every);

        bootstrap(&store, &self.config).await?;
        seed_providers(&store, &self.config).await;
        seed_prices(&store, &self.config).await;

        Ok(store)
    }

    /// 绑定端口开始服务。启动前的配置告警也在这里。
    pub async fn serve(&self, store: AdminStore) -> Result<()> {
        if let Some(dir) = &self.config.static_dir {
            log::info!("静态资源目录：{}", dir.display());
        }
        if !self.config.captcha_required {
            log::warn!(
                "登录验证码已关闭（POINTER_ADMIN_CAPTCHA=off）——只应在自动化测试或内网联调时这么配"
            );
        }

        let addr: SocketAddr = self
            .config
            .addr
            .parse()
            .with_context(|| format!("地址格式不正确：{}", self.config.addr))?;

        let app = crate::app::build_app(AppState::new(store, self.config.clone()));
        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .with_context(|| format!("监听 {addr} 失败"))?;

        log::info!("控制面已启动：http://{addr}");
        axum::serve(listener, app).await?;
        Ok(())
    }

    /// 密钥模式决定密文能不能跨机器解，部署时最容易搞错，启动就说清楚。
    fn log_crypto_mode(&self) {
        let has_master_key = std::env::var(crate::crypto::MASTER_KEY_ENV)
            .ok()
            .map(|v| !v.trim().is_empty())
            .unwrap_or(false);
        if has_master_key {
            log::info!(
                "企业密钥加密：使用 {}（可跨实例解密）",
                crate::crypto::MASTER_KEY_ENV
            );
        } else {
            log::warn!(
                "企业密钥加密：未配置 {}，按本机标识派生——换机/多实例部署后旧密钥解不开",
                crate::crypto::MASTER_KEY_ENV
            );
        }
    }
}

/// 首个管理员 + 两个 OAuth 客户端。两者都幂等。
async fn bootstrap(store: &AdminStore, cfg: &AdminConfig) -> Result<()> {
    if users::list(store, &cfg.tenant_id).await?.is_empty() {
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

/// 全新库：把内置的三家服务商（百炼 / DeepSeek / 豆包）和默认档位种上，
/// 否则管理员登录后是一片空白。
///
/// 库里已经有服务商就不动 —— 管理员删掉的不该在重启后又冒出来。
async fn seed_providers(store: &AdminStore, cfg: &AdminConfig) {
    let existing = resources::list(store, &cfg.tenant_id, Some(KIND_PROVIDER)).await;
    match existing {
        Ok(rows) if !rows.is_empty() => return,
        Ok(_) => {}
        Err(e) => {
            log::warn!("查询服务商失败，跳过播种：{e}");
            return;
        }
    }
    match seed::seed(store, &cfg.tenant_id).await {
        Ok(r) => log::info!(
            "初始化服务商：新建 {} 个，模型 {} 个",
            r.providers_created,
            r.models_created
        ),
        Err(e) => log::warn!("初始化服务商失败（不影响启动）：{e}"),
    }
}

/// 单价每次启动都补一次：库可能是早就建好的，服务商播种不会再跑。
async fn seed_prices(store: &AdminStore, cfg: &AdminConfig) {
    match seed::seed_prices(store, &cfg.tenant_id).await {
        Ok(0) => {}
        Ok(n) => log::info!("初始化模型单价：补齐 {n} 个（照抄官网费率）"),
        Err(e) => log::warn!("初始化模型单价失败（不影响启动）：{e}"),
    }
}

/// 保活间隔：默认 [`crate::db::POOL_KEEPALIVE_EVERY_SECS`]，可用
/// `POINTER_ADMIN_KEEPALIVE_SECS` 覆盖，`0` 关闭。
fn keepalive_every() -> std::time::Duration {
    let secs = std::env::var("POINTER_ADMIN_KEEPALIVE_SECS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(crate::db::POOL_KEEPALIVE_EVERY_SECS);
    std::time::Duration::from_secs(secs)
}
