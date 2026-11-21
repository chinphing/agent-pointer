//! 执行环境选择（设计文档 §3.4「账号与执行环境」）。
//!
//! 三层解析，从具体到默认：
//!
//! 1. **账号级显式配置** `[environment.accounts] alice = "light"`——永远最高优先级
//! 2. **admin 默认**：命中 admin 名单 → `local`（技能维护需要写权限，见 §5.4.1）；
//!    `[environment] admin_tier = "inherit"` 关掉这一层，让名单账号跟普通用户一样
//!    落到部署默认
//! 3. **部署默认** `[environment] provider`；缺省按 `deployment.mode` 派生
//!
//! 硬规则：解析结果是 `light` 却没有 `sandbox_url` → **报错**，不静默退回 `local`
//! （否则"以为隔离了其实没有"）。

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{bail, Result};

/// 执行环境**档位**：决定语义与能力集（能不能桌面、能不能快照、读写范围）。
///
/// 档位与实现分离：同一个档位可以由不同后端实现（见 [`Backend`]），
/// 这样"开发用本地 Docker、生产用 Cube"跑的是同一套语义。
///
/// `Ord` 只为让档位能当 `[environment.backends]` 解析结果的 map key。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    /// 宿主直跑（桌面端 / admin / standalone）。
    Local,
    /// 隔离环境，无 GUI、无快照。
    Light,
    /// 隔离环境 + GUI（Xvfb + WM）+ 快照。
    Container,
}

impl Tier {
    pub fn parse(raw: &str) -> Result<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "local" => Ok(Self::Local),
            "light" => Ok(Self::Light),
            "container" => Ok(Self::Container),
            other => bail!("未知的 environment.provider: `{other}`（只支持 local | light | container）"),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Light => "light",
            Self::Container => "container",
        }
    }
}

/// 执行环境的**实现后端**：决定 IO 怎么落地。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// 宿主进程与文件系统。
    Host,
    /// envd 协议（e2b-local / CubeSandbox）。
    Envd,
    /// 本地 Docker（`docker exec` + bind 挂载）。
    Docker,
}

impl Backend {
    pub fn parse(raw: &str) -> Result<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "host" => Ok(Self::Host),
            "envd" => Ok(Self::Envd),
            "docker" => Ok(Self::Docker),
            other => bail!("未知的 environment.backend: `{other}`（只支持 host | envd | docker）"),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Host => "host",
            Self::Envd => "envd",
            Self::Docker => "docker",
        }
    }
}

/// 档位 + 后端的组合结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedEnv {
    pub tier: Tier,
    pub backend: Backend,
}

impl ResolvedEnv {
    /// 后端缺省推导：保持旧配置（只写 provider）行为不变。
    fn default_backend(tier: Tier) -> Backend {
        match tier {
            Tier::Local => Backend::Host,
            Tier::Light => Backend::Envd,
            Tier::Container => Backend::Docker,
        }
    }
}

/// 命中 admin 名单时给什么档位（`[environment] admin_tier`）。
///
/// 默认 [`AdminTier::Local`] = 历史行为（名单账号宿主直跑，不重写配置即不变）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AdminTier {
    /// 名单账号固定 `local`。
    #[default]
    Local,
    /// 名单账号跳过 admin 层，跟随部署默认档位。
    Inherit,
}

impl AdminTier {
    pub fn parse(raw: &str) -> Result<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "local" => Ok(Self::Local),
            "inherit" => Ok(Self::Inherit),
            other => bail!(
                "未知的 environment.admin_tier: `{other}`（只支持 local | inherit）"
            ),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Inherit => "inherit",
        }
    }
}

/// provider 选择所需的全部配置。默认值 = 桌面端（local，无名单）。
#[derive(Debug, Clone, Default)]
pub struct ProviderConfig {
    /// 部署默认档位；`None` = 按 `deployment.mode` 派生。
    pub tier: Option<Tier>,
    /// 命中 admin 名单时的档位策略；`Local` = 历史行为。
    pub admin_tier: AdminTier,
    /// 部署默认后端；`None` = 按档位推导（local→host / light→envd / container→docker）。
    /// **不作用于 `local` 档位**（local 只能是 host）。
    pub backend: Option<Backend>,
    /// 按档位的后端覆盖（`[environment.backends]`）；优先级高于全局 `backend`。
    pub tier_backends: BTreeMap<Tier, Backend>,
    /// light 档位必填的沙箱控制面地址。
    pub sandbox_url: Option<String>,
    /// e2b-local 的卷宿主目录；有值 → `volumeMounts`，无值 → Cube 的 `metadata["host-mount"]`。
    pub volume_host_path: Option<String>,
    /// 沙箱模板镜像；`None` 用 `DEFAULT_TEMPLATE`。
    pub template: Option<String>,
    /// admin 账号名单（`session_user_id`）。
    pub admins: Vec<String>,
    /// 账号级覆盖。
    pub accounts: BTreeMap<String, Tier>,
    /// 是否平台部署（绑定控制面）。standalone 时默认 local。
    pub platform: bool,
}

impl ProviderConfig {
    /// 从 server 配置 / 环境变量读取。值非法直接报错（不猜）。
    pub fn from_env() -> Result<Self> {
        let tier = match crate::server_config::environment_provider() {
            Some(raw) => Some(Tier::parse(&raw)?),
            None => None,
        };
        let backend = match crate::server_config::environment_backend() {
            Some(raw) => Some(Backend::parse(&raw)?),
            None => None,
        };
        let admin_tier = match crate::server_config::environment_admin_tier() {
            Some(raw) => AdminTier::parse(&raw)?,
            None => AdminTier::Local,
        };
        let mut accounts = BTreeMap::new();
        for (user, raw) in crate::server_config::environment_accounts() {
            let user = user.trim().to_string();
            if user.is_empty() {
                continue;
            }
            accounts.insert(user, Tier::parse(&raw)?);
        }
        let mut tier_backends = BTreeMap::new();
        for (raw_tier, raw_backend) in crate::server_config::environment_backends() {
            let tier = Tier::parse(&raw_tier).map_err(|_| {
                anyhow::anyhow!(
                    "未知的 environment.backends 档位: `{raw_tier}`（只支持 local | light | container）"
                )
            })?;
            tier_backends.insert(tier, Backend::parse(&raw_backend)?);
        }
        let cfg = Self {
            tier,
            backend,
            tier_backends,
            admin_tier,
            sandbox_url: crate::server_config::environment_sandbox_url(),
            volume_host_path: crate::server_config::environment_volume_host_path(),
            template: crate::server_config::environment_template(),
            admins: crate::server_config::admin_accounts(),
            accounts,
            platform: crate::deployment_mode::is_platform(),
        };
        log::info!(
            "environment: tier={:?} backend={:?} tier_backends={:?} admin_tier={} sandbox_url={:?} volume={:?} template={:?} platform={} admins={}",
            cfg.tier,
            cfg.backend,
            cfg.tier_backends,
            cfg.admin_tier.as_str(),
            cfg.sandbox_url,
            cfg.volume_host_path,
            cfg.template,
            cfg.platform,
            cfg.admins.len()
        );
        Ok(cfg)
    }

    /// 该账号是否在 admin 名单里（空账号永远不是 admin）。
    ///
    /// **边界（刻意，不是遗漏）**：执行环境档位只认 `[auth] admins` 名单，不认
    /// 平台管理员身份。管理员口径的唯一入口是
    /// [`crate::platform_auth::is_admin_session`]（会话票 / 名单 / standalone 本地
    /// 运维会话），**不要**把它接到这里。平台模式是多租户，管理员身份由控制面
    /// 签发，不等于可以在这台机器上绕过沙箱；把票管理员降级成「宿主直跑」是
    /// 安全回退。
    pub fn is_admin(&self, session_user_id: &str) -> bool {
        let uid = session_user_id.trim();
        !uid.is_empty() && self.admins.iter().any(|a| a.trim() == uid)
    }

    /// 档位 → 后端。优先级从高到低：
    ///
    /// 1. `[environment.backends].<tier>` 显式值
    /// 2. 全局 `backend`——**不作用于 `local`**（local 只能是 host，否则
    ///    「全局 docker + 管理员 local」这类配置会被判死）
    /// 3. 档位推导默认：local→host / light→envd / container→docker
    pub fn backend_for(&self, tier: Tier) -> Backend {
        if let Some(backend) = self.tier_backends.get(&tier) {
            return *backend;
        }
        if tier != Tier::Local {
            if let Some(backend) = self.backend {
                return backend;
            }
        }
        ResolvedEnv::default_backend(tier)
    }

    fn resolved(&self, tier: Tier) -> ResolvedEnv {
        ResolvedEnv {
            tier,
            backend: self.backend_for(tier),
        }
    }

    /// 三层解析 + 校验。`session_user_id` 为空（桌面端 / 未登录）时只看部署默认。
    pub fn resolve(&self, session_user_id: Option<&str>) -> Result<ResolvedEnv> {
        let tier = self.resolve_unchecked(session_user_id);
        let resolved = self.resolved(tier);
        self.validate(resolved, session_user_id)?;
        Ok(resolved)
    }

    /// 启动时校验：把**实际会用到**的档位各解析一次并校验，配错在启动就报，
    /// 不留到运行时变成「助手消息异常」。
    ///
    /// 覆盖的档位 = 部署默认 + `[environment.accounts]` 覆盖 + `[environment.backends]`
    /// 显式列出的档位（+ 配了 admin 名单时的 `local`）。
    ///
    /// **刻意不遍历全部三个档位**：`provider = "local"` 的部署从不配 `sandbox_url`，
    /// 而 `light` 档位的推导默认是 envd——按「全部档位」校验会把这种合法配置判死。
    /// 写进配置的档位永远在校验范围内，真正的配错不会漏。
    pub fn validate_deployment(&self) -> Result<()> {
        for tier in self.tiers_in_use() {
            self.validate(self.resolved(tier), None)?;
        }
        Ok(())
    }

    /// 启动校验要覆盖的档位集合（见 [`Self::validate_deployment`]）。
    fn tiers_in_use(&self) -> BTreeSet<Tier> {
        let mut tiers = BTreeSet::new();
        tiers.insert(self.deployment_default_tier());
        tiers.extend(self.accounts.values().copied());
        tiers.extend(self.tier_backends.keys().copied());
        // 名单账号只在 `admin_tier = "local"` 时才会落到 local。
        if !self.admins.is_empty() && self.admin_tier == AdminTier::Local {
            tiers.insert(Tier::Local);
        }
        tiers
    }

    /// 部署默认档位：`provider` 缺省时按部署模式派生（平台 → light，standalone → local）。
    fn deployment_default_tier(&self) -> Tier {
        self.tier
            .unwrap_or(if self.platform { Tier::Light } else { Tier::Local })
    }

    fn validate(&self, resolved: ResolvedEnv, session_user_id: Option<&str>) -> Result<()> {
        let uid = session_user_id.map(str::trim).filter(|s| !s.is_empty());
        // 档位与后端必须自洽：Local 只能用 Host，非 Local 不能用 Host。
        match (resolved.tier, resolved.backend) {
            (Tier::Local, Backend::Host) => {}
            (Tier::Local, other) => bail!(
                "environment: 档位 local 只能用 host 后端（当前 backend={}）",
                other.as_str()
            ),
            (_, Backend::Host) => bail!(
                "environment: 档位 {} 不能配 host 后端（隔离会失效）",
                resolved.tier.as_str()
            ),
            _ => {}
        }
        if resolved.backend != Backend::Envd {
            return Ok(());
        }
        let has_url = self
            .sandbox_url
            .as_deref()
            .map(str::trim)
            .map(|s| !s.is_empty())
            .unwrap_or(false);
        if has_url {
            return Ok(());
        }
        match uid {
            Some(uid) => bail!(
                "账号 `{uid}` 解析为 {} + envd 后端，但未配置 environment.sandbox_url（不静默退回 local）",
                resolved.tier.as_str()
            ),
            None => bail!(
                "environment: 档位 {} 使用 envd 后端，但未配置 environment.sandbox_url",
                resolved.tier.as_str()
            ),
        }
    }

    fn resolve_unchecked(&self, session_user_id: Option<&str>) -> Tier {
        let uid = session_user_id.unwrap_or("").trim();
        if !uid.is_empty() {
            if let Some(p) = self.accounts.get(uid) {
                return *p;
            }
            // admin 层：`admin_tier = "inherit"` 时整层跳过，继续落到部署默认。
            if self.admin_tier == AdminTier::Local && self.is_admin(uid) {
                return Tier::Local;
            }
        }
        self.deployment_default_tier()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> ProviderConfig {
        ProviderConfig {
            tier: Some(Tier::Light),
            backend: None,
            tier_backends: BTreeMap::new(),
            admin_tier: AdminTier::Local,
            sandbox_url: Some("http://127.0.0.1:3000".into()),
            volume_host_path: None,
            template: None,
            admins: vec!["admin".into(), "wangguohong".into()],
            accounts: BTreeMap::new(),
            platform: true,
        }
    }

    #[test]
    fn parses_tiers_and_backends() {
        assert_eq!(Tier::parse("Local").unwrap(), Tier::Local);
        assert_eq!(Tier::parse(" light ").unwrap(), Tier::Light);
        assert_eq!(Tier::parse("container").unwrap(), Tier::Container);
        assert!(Tier::parse("").is_err());
        assert_eq!(Backend::parse("envd").unwrap(), Backend::Envd);
        assert_eq!(Backend::parse("Docker").unwrap(), Backend::Docker);
        assert!(Backend::parse("firecracker").is_err());
    }

    #[test]
    fn backend_defaults_follow_tier() {
        let mut c = cfg();
        c.backend = None;
        assert_eq!(c.resolve(Some("alice")).unwrap().backend, Backend::Envd);
        c.tier = Some(Tier::Local);
        assert_eq!(c.resolve(Some("alice")).unwrap().backend, Backend::Host);
        c.tier = Some(Tier::Container);
        assert_eq!(c.resolve(Some("alice")).unwrap().backend, Backend::Docker);
    }

    #[test]
    fn docker_backend_needs_no_sandbox_url() {
        let c = ProviderConfig {
            tier: Some(Tier::Light),
            backend: Some(Backend::Docker),
            sandbox_url: None,
            platform: true,
            ..Default::default()
        };
        assert_eq!(c.resolve(Some("alice")).unwrap().backend, Backend::Docker);
    }

    #[test]
    fn tier_and_backend_must_be_consistent() {
        let c = ProviderConfig {
            tier: Some(Tier::Light),
            backend: Some(Backend::Host),
            sandbox_url: Some("http://x".into()),
            platform: true,
            ..Default::default()
        };
        assert!(c.resolve(Some("alice")).is_err(), "light + host 必须报错");
    }

    #[test]
    fn deployment_default_applies_to_normal_users() {
        let c = cfg();
        assert_eq!(c.resolve(Some("alice")).unwrap().tier, Tier::Light);
        assert_eq!(c.resolve(None).unwrap().tier, Tier::Light);
    }

    #[test]
    fn admin_defaults_to_local() {
        let c = cfg();
        assert_eq!(c.resolve(Some("admin")).unwrap().tier, Tier::Local);
        assert_eq!(c.resolve(Some(" wangguohong ")).unwrap().tier, Tier::Local);
    }

    #[test]
    fn account_override_beats_admin_default() {
        let mut c = cfg();
        c.accounts.insert("admin".into(), Tier::Light);
        assert_eq!(c.resolve(Some("admin")).unwrap().tier, Tier::Light);
    }

    #[test]
    fn parses_admin_tier() {
        assert_eq!(AdminTier::parse("local").unwrap(), AdminTier::Local);
        assert_eq!(AdminTier::parse(" InHerit ").unwrap(), AdminTier::Inherit);
        let err = AdminTier::parse("sandbox").unwrap_err().to_string();
        assert!(err.contains("sandbox"), "{err}");
        assert!(err.contains("local") && err.contains("inherit"), "{err}");
        assert!(AdminTier::parse("").is_err());
        assert_eq!(AdminTier::default(), AdminTier::Local, "缺省必须保持旧行为");
    }

    #[test]
    fn admin_tier_local_keeps_listed_admin_on_host() {
        let c = cfg();
        assert_eq!(c.admin_tier, AdminTier::Local);
        assert_eq!(c.resolve(Some("admin")).unwrap().tier, Tier::Local);
    }

    #[test]
    fn admin_tier_inherit_follows_deployment_default() {
        let mut c = cfg();
        c.admin_tier = AdminTier::Inherit;
        // platform 部署默认 light：名单账号不再被钉在 local
        assert_eq!(c.resolve(Some("admin")).unwrap().tier, Tier::Light);
        // 非名单账号不受影响
        assert_eq!(c.resolve(Some("alice")).unwrap().tier, Tier::Light);
    }

    #[test]
    fn admin_tier_inherit_on_standalone_still_lands_on_local() {
        let c = ProviderConfig {
            tier: None,
            backend: None,
            admin_tier: AdminTier::Inherit,
            platform: false,
            ..Default::default()
        };
        assert_eq!(c.resolve(Some("admin")).unwrap().tier, Tier::Local);
    }

    #[test]
    fn account_override_beats_admin_tier_inherit() {
        let mut c = cfg();
        c.admin_tier = AdminTier::Inherit;
        c.accounts.insert("admin".into(), Tier::Container);
        assert_eq!(c.resolve(Some("admin")).unwrap().tier, Tier::Container);
        c.accounts.insert("admin".into(), Tier::Local);
        assert_eq!(c.resolve(Some("admin")).unwrap().tier, Tier::Local);
    }

    #[test]
    fn account_override_can_force_local_for_normal_user() {
        let mut c = cfg();
        c.accounts.insert("debugger".into(), Tier::Local);
        assert_eq!(c.resolve(Some("debugger")).unwrap().tier, Tier::Local);
        // 没被覆盖的普通用户仍然走部署默认
        assert_eq!(c.resolve(Some("bob")).unwrap().tier, Tier::Light);
    }

    #[test]
    fn standalone_defaults_to_local() {
        let c = ProviderConfig {
            tier: None,
            backend: None,
            platform: false,
            ..Default::default()
        };
        assert_eq!(c.resolve(Some("alice")).unwrap().tier, Tier::Local);
        assert!(c.validate_deployment().is_ok());
    }

    #[test]
    fn platform_defaults_to_light_when_provider_unset() {
        let c = ProviderConfig {
            tier: None,
            backend: None,
            sandbox_url: Some("http://sandbox".into()),
            platform: true,
            ..Default::default()
        };
        assert_eq!(c.resolve(Some("alice")).unwrap().tier, Tier::Light);
    }

    #[test]
    fn light_without_sandbox_url_fails_fast() {
        let c = ProviderConfig {
            tier: Some(Tier::Light),
            backend: None,
            sandbox_url: None,
            platform: true,
            ..Default::default()
        };
        let err = c.resolve(Some("alice")).unwrap_err().to_string();
        assert!(err.contains("sandbox_url"), "{err}");
        assert!(c.validate_deployment().is_err());
    }

    #[test]
    fn blank_sandbox_url_counts_as_missing() {
        let c = ProviderConfig {
            tier: Some(Tier::Light),
            backend: None,
            sandbox_url: Some("   ".into()),
            platform: true,
            ..Default::default()
        };
        assert!(c.resolve(None).is_err());
    }

    #[test]
    fn local_never_requires_sandbox_url() {
        let c = ProviderConfig {
            tier: Some(Tier::Local),
            backend: None,
            sandbox_url: None,
            platform: true,
            ..Default::default()
        };
        assert_eq!(c.resolve(Some("alice")).unwrap().tier, Tier::Local);
        assert!(c.validate_deployment().is_ok());
    }

    #[test]
    fn empty_account_is_never_admin() {
        let c = cfg();
        assert!(!c.is_admin(""));
        assert!(!c.is_admin("   "));
        assert!(c.is_admin("admin"));
    }

    /// 档位边界回归：`is_admin` 只认 `[auth] admins` 名单。
    ///
    /// 名单里的账号仍解析为 `local`；控制面签发的平台管理员（票管理员，不在名单）
    /// 仍走部署默认档位——管理员身份不等于可以绕过沙箱。
    #[test]
    fn platform_admin_ticket_does_not_bypass_sandbox_tier() {
        let c = cfg();
        assert_eq!(c.resolve(Some("admin")).unwrap().tier, Tier::Local);
        assert!(!c.is_admin("ticket-admin"));
        assert_eq!(c.resolve(Some("ticket-admin")).unwrap().tier, Tier::Light);
    }

    #[test]
    fn empty_account_uses_deployment_default() {
        let c = cfg();
        assert_eq!(c.resolve(Some("")).unwrap().tier, Tier::Light);
    }

    /// 回归：全局 `backend = "docker"` + 名单里的管理员 → `local + host`。
    ///
    /// 以前全局 backend 套到所有档位，管理员一发起对话就报「档位 local 只能用 host 后端」。
    #[test]
    fn global_backend_does_not_apply_to_local_tier() {
        let c = ProviderConfig {
            tier: Some(Tier::Light),
            backend: Some(Backend::Docker),
            admins: vec!["admin".into()],
            platform: true,
            ..Default::default()
        };
        let admin = c.resolve(Some("admin")).expect("管理员不该报错");
        assert_eq!(admin, ResolvedEnv { tier: Tier::Local, backend: Backend::Host });
        assert_eq!(
            c.resolve(Some("alice")).unwrap(),
            ResolvedEnv { tier: Tier::Light, backend: Backend::Docker }
        );
        assert!(c.validate_deployment().is_ok(), "启动校验也必须通过");
    }

    #[test]
    fn tier_backend_override_beats_global_backend() {
        let mut c = cfg();
        c.backend = Some(Backend::Docker);
        c.tier_backends.insert(Tier::Light, Backend::Envd);
        assert_eq!(
            c.resolve(Some("alice")).unwrap(),
            ResolvedEnv { tier: Tier::Light, backend: Backend::Envd }
        );
    }

    #[test]
    fn tier_backend_override_can_restore_envd_without_global() {
        let mut c = cfg();
        c.tier = Some(Tier::Container);
        // 全局没配 backend：container 推导默认是 docker
        assert_eq!(c.resolve(Some("alice")).unwrap().backend, Backend::Docker);
        c.tier_backends.insert(Tier::Container, Backend::Envd);
        assert_eq!(c.resolve(Some("alice")).unwrap().backend, Backend::Envd);
    }

    /// 组合矩阵：档位 / 后端 / 是否配 sandbox_url → 解析结果或报错。
    #[test]
    fn tier_backend_combinations() {
        // (tier, tier_backends 覆盖, sandbox_url, 期望)
        let cases: Vec<(Tier, Option<Backend>, Option<&str>, Result<Backend, ()>)> = vec![
            (Tier::Local, None, None, Ok(Backend::Host)),
            (Tier::Light, Some(Backend::Docker), None, Ok(Backend::Docker)),
            (Tier::Container, Some(Backend::Docker), None, Ok(Backend::Docker)),
            (Tier::Light, Some(Backend::Envd), Some("http://cube:3000"), Ok(Backend::Envd)),
            (Tier::Light, Some(Backend::Envd), None, Err(())),
            (Tier::Local, Some(Backend::Docker), None, Err(())),
            (Tier::Light, Some(Backend::Host), Some("http://cube:3000"), Err(())),
        ];
        for (tier, backend, sandbox_url, expect) in cases {
            let mut c = ProviderConfig {
                tier: Some(tier),
                sandbox_url: sandbox_url.map(str::to_string),
                platform: true,
                ..Default::default()
            };
            if let Some(backend) = backend {
                c.tier_backends.insert(tier, backend);
            }
            match (c.resolve(Some("alice")), expect) {
                (Ok(resolved), Ok(backend)) => assert_eq!(
                    resolved.backend, backend,
                    "tier={} 期望 backend={}",
                    tier.as_str(),
                    backend.as_str()
                ),
                (Err(err), Err(())) => {
                    assert!(!err.to_string().is_empty(), "错误信息不能为空");
                }
                (got, _) => panic!(
                    "tier={} 解析结果不符合预期: {:?}",
                    tier.as_str(),
                    got.map(|r| r.backend)
                ),
            }
        }
    }

    /// 启动校验要覆盖账号覆盖出来的档位：`light` 用 envd 却没配 sandbox_url → 启动即报。
    #[test]
    fn validate_deployment_covers_account_override_tiers() {
        let mut c = ProviderConfig {
            tier: Some(Tier::Local),
            platform: true,
            accounts: BTreeMap::from([("bob".to_string(), Tier::Light)]),
            ..Default::default()
        };
        assert!(c.validate_deployment().is_err(), "账号覆盖出来的 light 也必须校验");
        c.sandbox_url = Some("http://cube:3000".into());
        assert!(c.validate_deployment().is_ok());
    }

    /// 显式写进 `[environment.backends]` 的档位即使当前没人用，启动也要校验。
    #[test]
    fn validate_deployment_covers_explicit_tier_backends() {
        let c = ProviderConfig {
            tier: Some(Tier::Local),
            tier_backends: BTreeMap::from([(Tier::Local, Backend::Docker)]),
            platform: true,
            ..Default::default()
        };
        let err = c.validate_deployment().unwrap_err().to_string();
        assert!(err.contains("local"), "{err}");
    }

    /// 只跑 local 的部署从不配 sandbox_url：不能因为「light 的推导默认是 envd」被判死。
    #[test]
    fn validate_deployment_skips_unused_derived_tiers() {
        let c = ProviderConfig {
            tier: Some(Tier::Local),
            platform: true,
            ..Default::default()
        };
        assert!(c.validate_deployment().is_ok());
    }
}
