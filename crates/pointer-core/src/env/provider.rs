//! 执行环境选择（设计文档 §3.4「账号与执行环境」）。
//!
//! 三层解析，从具体到默认：
//!
//! 1. **账号级显式配置** `[environment.accounts] alice = "light"`
//! 2. **admin 默认**：命中 admin 名单 → `local`（技能维护需要写权限，见 §5.4.1）
//! 3. **部署默认** `[environment] provider`；缺省按 `deployment.mode` 派生
//!
//! 硬规则：解析结果是 `light` 却没有 `sandbox_url` → **报错**，不静默退回 `local`
//! （否则"以为隔离了其实没有"）。

use std::collections::BTreeMap;

use anyhow::{bail, Result};

/// 执行环境**档位**：决定语义与能力集（能不能桌面、能不能快照、读写范围）。
///
/// 档位与实现分离：同一个档位可以由不同后端实现（见 [`Backend`]），
/// 这样"开发用本地 Docker、生产用 Cube"跑的是同一套语义。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    fn with_default_backend(tier: Tier, backend: Option<Backend>) -> Self {
        let backend = backend.unwrap_or(match tier {
            Tier::Local => Backend::Host,
            Tier::Light => Backend::Envd,
            Tier::Container => Backend::Docker,
        });
        Self { tier, backend }
    }
}

/// provider 选择所需的全部配置。默认值 = 桌面端（local，无名单）。
#[derive(Debug, Clone, Default)]
pub struct ProviderConfig {
    /// 部署默认档位；`None` = 按 `deployment.mode` 派生。
    pub tier: Option<Tier>,
    /// 部署默认后端；`None` = 按档位推导（local→host / light→envd / container→docker）。
    pub backend: Option<Backend>,
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
        let mut accounts = BTreeMap::new();
        for (user, raw) in crate::server_config::environment_accounts() {
            let user = user.trim().to_string();
            if user.is_empty() {
                continue;
            }
            accounts.insert(user, Tier::parse(&raw)?);
        }
        let cfg = Self {
            tier,
            backend,
            sandbox_url: crate::server_config::environment_sandbox_url(),
            volume_host_path: crate::server_config::environment_volume_host_path(),
            template: crate::server_config::environment_template(),
            admins: crate::server_config::admin_accounts(),
            accounts,
            platform: crate::deployment_mode::is_platform(),
        };
        log::info!(
            "environment: tier={:?} backend={:?} sandbox_url={:?} volume={:?} template={:?} platform={} admins={}",
            cfg.tier,
            cfg.backend,
            cfg.sandbox_url,
            cfg.volume_host_path,
            cfg.template,
            cfg.platform,
            cfg.admins.len()
        );
        Ok(cfg)
    }

    /// 该账号是否在 admin 名单里（空账号永远不是 admin）。
    pub fn is_admin(&self, session_user_id: &str) -> bool {
        let uid = session_user_id.trim();
        !uid.is_empty() && self.admins.iter().any(|a| a.trim() == uid)
    }

    /// 三层解析 + 校验。`session_user_id` 为空（桌面端 / 未登录）时只看部署默认。
    pub fn resolve(&self, session_user_id: Option<&str>) -> Result<ResolvedEnv> {
        let tier = self.resolve_unchecked(session_user_id);
        let resolved = ResolvedEnv::with_default_backend(tier, self.backend);
        self.validate(resolved, session_user_id)?;
        Ok(resolved)
    }

    /// 启动时校验部署默认值：light 必须配 `sandbox_url`。
    pub fn validate_deployment(&self) -> Result<()> {
        let tier = self
            .tier
            .unwrap_or(if self.platform { Tier::Light } else { Tier::Local });
        self.validate(ResolvedEnv::with_default_backend(tier, self.backend), None)
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
            if self.is_admin(uid) {
                return Tier::Local;
            }
        }
        self.tier
            .unwrap_or(if self.platform { Tier::Light } else { Tier::Local })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> ProviderConfig {
        ProviderConfig {
            tier: Some(Tier::Light),
            backend: None,
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

    #[test]
    fn empty_account_uses_deployment_default() {
        let c = cfg();
        assert_eq!(c.resolve(Some("")).unwrap().tier, Tier::Light);
    }
}
