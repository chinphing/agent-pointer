//! Provider registry: the single lookup point for configured providers.
//!
//! The registry is also where deployment mode decides what exists at all:
//! `standalone` gets `local_password`, `managed` gets `platform`, and every
//! other provider only appears when it is written in `pointer-server.toml`.

use anyhow::{anyhow, bail, Result};
use serde::Serialize;
use std::collections::BTreeMap;

use super::config::{AuthProviderConfig, AuthProviderKind};
use super::identity::AuthProviderDriver;
use super::local_password::LocalPasswordDriver;
use super::oauth2_userinfo::Oauth2UserinfoDriver;
use super::oidc::OidcDriver;
use super::token_lookup::TokenLookupDriver;

/// Built-in provider id for cloud control-plane login.
pub const PLATFORM_PROVIDER_ID: &str = "platform";
/// Built-in provider id for the local username/password form.
pub const LOCAL_PASSWORD_PROVIDER_ID: &str = "local_password";

/// All `[auth.providers.<id>]` entries, keyed by provider id (sorted, so
/// login-page rendering is stable).
///
/// Disabled entries are kept (they are still addressable) but skipped by
/// [`AuthProviderRegistry::enabled`] and rejected by
/// [`AuthProviderRegistry::driver`]. They are also not validated, so a
/// half-written provider can be switched off instead of breaking startup.
#[derive(Debug, Clone, Default)]
pub struct AuthProviderRegistry {
    providers: BTreeMap<String, AuthProviderConfig>,
    /// Provider used when a login start names none.
    default_id: Option<String>,
}

/// How a login should be started for a provider id.
#[derive(Debug, Clone, Copy)]
pub enum LoginEntry<'a> {
    /// Browser redirect to the provider's authorize endpoint.
    Redirect {
        id: &'a str,
        config: &'a AuthProviderConfig,
    },
    /// Cloud control-plane OAuth, handled by the server layer.
    Platform { id: &'a str },
    /// Username + password form.
    LocalPassword { id: &'a str },
    /// Inbound ticket: the portal starts it, we only finish it.
    Ticket {
        id: &'a str,
        config: &'a AuthProviderConfig,
    },
    /// Not declared at all.
    Unknown,
    /// Declared but switched off.
    Disabled { id: &'a str },
}

impl AuthProviderRegistry {
    /// Validate every enabled provider and build the registry.
    pub fn new(providers: BTreeMap<String, AuthProviderConfig>) -> Result<Self> {
        for (id, config) in &providers {
            if config.is_enabled() {
                config
                    .validate(id)
                    .map_err(|e| anyhow!("invalid auth provider `{id}`: {e}"))?;
            }
        }
        Ok(Self {
            providers,
            default_id: None,
        })
    }

    /// Build the registry for a deployment mode, injecting the built-in default
    /// and enforcing the managed-mode guard.
    ///
    /// * `standalone` → `local_password` unless the config says otherwise.
    /// * `managed` → `platform` unless the config says otherwise.
    ///
    /// A configured entry always wins over the injected default; `enabled = false`
    /// removes the default login method without tripping any validation.
    pub fn for_deployment(
        providers: BTreeMap<String, AuthProviderConfig>,
        standalone: bool,
    ) -> Result<Self> {
        let mut providers = providers;
        let (builtin_id, builtin_kind, builtin_label) = if standalone {
            (
                LOCAL_PASSWORD_PROVIDER_ID,
                AuthProviderKind::LocalPassword,
                "本地密码",
            )
        } else {
            (PLATFORM_PROVIDER_ID, AuthProviderKind::Platform, "平台账号")
        };
        if !providers.contains_key(builtin_id) {
            providers.insert(
                builtin_id.to_string(),
                AuthProviderConfig::builtin(builtin_kind, builtin_label),
            );
        }
        if !standalone {
            // The control plane is registered against the legacy callback path;
            // keep it unless the operator configured something else.
            if let Some(config) = providers.get_mut(PLATFORM_PROVIDER_ID) {
                if config.callback_path_opt().is_none() {
                    config.callback_path = super::config::PLATFORM_CALLBACK_PATH.to_string();
                }
            }
            // `local_password` bypasses the access allowlist, so a managed
            // deployment has to say out loud that it wants that break-glass.
            if let Some(config) = providers.get(LOCAL_PASSWORD_PROVIDER_ID) {
                if config.is_enabled() && !config.allow_in_managed {
                    bail!(
                        "auth provider `local_password` is enabled in managed mode but it bypasses \
                         the access allowlist; set `allow_in_managed = true` under \
                         [auth.providers.local_password] to accept that, or `enabled = false` to \
                         disable local password login"
                    );
                }
            }
        }
        let mut registry = Self::new(providers)?;
        registry.ensure_unique_callback_paths()?;
        registry.default_id = Some(builtin_id.to_string());
        Ok(registry)
    }

    /// Two providers on one callback path would make the router panic at
    /// startup, so reject it here with an actionable message.
    fn ensure_unique_callback_paths(&self) -> Result<()> {
        let mut seen: BTreeMap<String, &str> = BTreeMap::new();
        for (id, config) in self.enabled() {
            let path = config.callback_path(id);
            if let Some(other) = seen.insert(path.clone(), id) {
                bail!(
                    "auth providers `{other}` and `{id}` both use the callback path `{path}`; \
                     give one of them a different `callback_path`"
                );
            }
        }
        Ok(())
    }

    pub fn empty() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.providers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.providers.is_empty()
    }

    /// Every configured id, enabled or not.
    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.providers.keys().map(String::as_str)
    }

    /// Enabled providers, in id order.
    pub fn enabled(&self) -> impl Iterator<Item = (&str, &AuthProviderConfig)> {
        self.providers
            .iter()
            .filter(|(_, config)| config.is_enabled())
            .map(|(id, config)| (id.as_str(), config))
    }

    pub fn get(&self, id: &str) -> Option<&AuthProviderConfig> {
        self.providers.get(id)
    }

    /// Provider a login start should use when the caller names none: the
    /// deployment default when it is enabled, else the first enabled provider.
    pub fn default_provider_id(&self) -> Option<&str> {
        if let Some(id) = self.default_id.as_deref() {
            if self.get(id).is_some_and(AuthProviderConfig::is_enabled) {
                return Some(id);
            }
        }
        self.enabled().next().map(|(id, _)| id)
    }

    /// Resolve how a login for `requested` should be started.
    pub fn login_entry<'a>(&'a self, requested: Option<&'a str>) -> LoginEntry<'a> {
        let id = match requested.map(str::trim).filter(|id| !id.is_empty()) {
            Some(id) => id,
            None => match self.default_provider_id() {
                Some(id) => id,
                None => return LoginEntry::Unknown,
            },
        };
        match self.providers.get(id) {
            None => LoginEntry::Unknown,
            Some(config) if !config.is_enabled() => LoginEntry::Disabled { id },
            Some(config) => match config.kind {
                AuthProviderKind::Platform => LoginEntry::Platform { id },
                AuthProviderKind::LocalPassword => LoginEntry::LocalPassword { id },
                AuthProviderKind::TokenLookup => LoginEntry::Ticket { id, config },
                AuthProviderKind::Oauth2Userinfo | AuthProviderKind::Oidc => {
                    LoginEntry::Redirect { id, config }
                }
            },
        }
    }

    /// Build a driver for one provider id.
    ///
    /// `platform` is deliberately not driver-backed: it produces platform
    /// tokens and credentials rather than a bare identity.
    pub fn driver(&self, id: &str) -> Result<Box<dyn AuthProviderDriver>> {
        let config = self
            .get(id)
            .ok_or_else(|| anyhow!("unknown auth provider `{id}`"))?;
        if !config.is_enabled() {
            bail!("auth provider `{id}` is disabled");
        }
        match config.kind {
            AuthProviderKind::Platform => bail!(
                "auth provider `{id}`: kind `platform` is finished by the server layer, not a driver"
            ),
            AuthProviderKind::LocalPassword => Ok(Box::new(LocalPasswordDriver::new(
                id,
                config.clone(),
            )?)),
            AuthProviderKind::TokenLookup => {
                Ok(Box::new(TokenLookupDriver::new(id, config.clone())?))
            }
            AuthProviderKind::Oauth2Userinfo => Ok(Box::new(Oauth2UserinfoDriver::new(
                id,
                config.clone(),
            )?)),
            AuthProviderKind::Oidc => Ok(Box::new(OidcDriver::new(id, config.clone())?)),
        }
    }

    /// Callback paths that need a dedicated route: everything that does not use
    /// the unified `/api/auth/callback/:provider` path.
    pub fn callback_routes(&self) -> Vec<(&str, String)> {
        self.enabled()
            .map(|(id, config)| (id, config.callback_path(id)))
            .filter(|(id, path)| *path != super::config::default_callback_path(id))
            .collect()
    }

    /// Login-page view of the enabled providers, in id order.
    pub fn enabled_views(&self) -> Vec<ProviderView> {
        let primary_id = self.primary_provider_id();
        self.enabled()
            .map(|(id, config)| ProviderView {
                id: id.to_string(),
                display_name: if config.display_name.trim().is_empty() {
                    id.to_string()
                } else {
                    config.display_name.trim().to_string()
                },
                kind: config.kind.as_str(),
                icon: config.icon_name().to_string(),
                primary: primary_id.as_deref() == Some(id),
            })
            .collect()
    }

    /// Provider that owns the login page's primary entry.
    ///
    /// Explicit `primary = true` wins (lowest id on a tie); otherwise the kind
    /// priority decides. `token_lookup` is never eligible: that entry is
    /// reached from the portal, not from the login page.
    pub fn primary_provider_id(&self) -> Option<String> {
        let eligible = |config: &AuthProviderConfig| config.kind != AuthProviderKind::TokenLookup;
        if let Some((id, _)) = self
            .enabled()
            .find(|(_, config)| eligible(config) && config.primary)
        {
            return Some(id.to_string());
        }
        self.enabled()
            .filter(|(_, config)| eligible(config))
            .min_by_key(|(id, config)| (primary_rank(config.kind), id.to_string()))
            .map(|(id, _)| id.to_string())
    }
}

/// Lower wins when no provider sets `primary = true`.
fn primary_rank(kind: AuthProviderKind) -> u8 {
    match kind {
        AuthProviderKind::Platform => 0,
        AuthProviderKind::Oidc => 1,
        AuthProviderKind::Oauth2Userinfo => 2,
        AuthProviderKind::LocalPassword => 3,
        AuthProviderKind::TokenLookup => u8::MAX,
    }
}

/// What the login page needs to render a provider button. Deliberately free of
/// endpoints, client ids, and secrets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProviderView {
    pub id: String,
    pub display_name: String,
    pub kind: &'static str,
    /// Icon name for the login button; the SPA falls back to an initial.
    pub icon: String,
    /// The page's primary entry (solid button); exactly one when any provider
    /// is enabled and renderable.
    pub primary: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dingtalk() -> AuthProviderConfig {
        toml::from_str(
            r#"
kind = "oauth2_userinfo"
display_name = "钉钉"
issuer = "https://login.dingtalk.com"

[authorize]
url = "https://login.dingtalk.com/oauth2/auth"
scope = "openid"

[token]
url = "https://api.dingtalk.com/v1.0/oauth2/userAccessToken"
client_id = "id"
client_secret = "secret"
access_token_path = "/accessToken"

[userinfo]
url = "https://api.dingtalk.com/v1.0/contact/users/me"
token_auth = "header"
token_name = "x-acs-dingtalk-access-token"
token_prefix = ""
subject_path = "/unionId"
"#,
        )
        .expect("dingtalk provider parses")
    }

    fn local_password_toml(extra: &str) -> AuthProviderConfig {
        toml::from_str(&format!(
            r#"
kind = "local_password"
display_name = "本地密码"
{extra}
"#
        ))
        .expect("local_password provider parses")
    }

    fn map(entries: Vec<(&str, AuthProviderConfig)>) -> BTreeMap<String, AuthProviderConfig> {
        entries
            .into_iter()
            .map(|(id, config)| (id.to_string(), config))
            .collect()
    }

    fn enabled_ids(registry: &AuthProviderRegistry) -> Vec<String> {
        registry
            .enabled_views()
            .into_iter()
            .map(|view| view.id)
            .collect()
    }

    /// `LoginEntry` carries a config reference, so compare a stable label.
    fn entry_key(entry: LoginEntry<'_>) -> String {
        match entry {
            LoginEntry::Redirect { id, .. } => format!("redirect:{id}"),
            LoginEntry::Platform { id } => format!("platform:{id}"),
            LoginEntry::LocalPassword { id } => format!("local_password:{id}"),
            LoginEntry::Ticket { id, .. } => format!("ticket:{id}"),
            LoginEntry::Unknown => "unknown".to_string(),
            LoginEntry::Disabled { id } => format!("disabled:{id}"),
        }
    }

    // ---- 验收用例：部署模式默认注入 -------------------------------------

    #[test]
    fn standalone_with_empty_config_only_has_local_password() {
        let registry = AuthProviderRegistry::for_deployment(map(vec![]), true)
            .expect("standalone registry builds");
        assert_eq!(enabled_ids(&registry), vec!["local_password".to_string()]);
        assert_eq!(
            entry_key(registry.login_entry(None)),
            "local_password:local_password"
        );
        // No platform entry at all: managed login is not offered.
        assert_eq!(entry_key(registry.login_entry(Some("platform"))), "unknown");
    }

    #[test]
    fn managed_with_empty_config_only_has_platform() {
        let registry =
            AuthProviderRegistry::for_deployment(map(vec![]), false).expect("managed registry");
        assert_eq!(enabled_ids(&registry), vec!["platform".to_string()]);
        assert_eq!(entry_key(registry.login_entry(None)), "platform:platform");
        assert_eq!(
            entry_key(registry.login_entry(Some("local_password"))),
            "unknown"
        );
    }

    #[test]
    fn managed_with_a_configured_provider_keeps_platform() {
        let registry = AuthProviderRegistry::for_deployment(map(vec![("dingtalk", dingtalk())]), false)
            .expect("managed registry");
        assert_eq!(
            enabled_ids(&registry),
            vec!["dingtalk".to_string(), "platform".to_string()]
        );
        match registry.login_entry(Some("dingtalk")) {
            LoginEntry::Redirect { id, config } => {
                assert_eq!(id, "dingtalk");
                assert_eq!(config.kind, AuthProviderKind::Oauth2Userinfo);
            }
            other => panic!("expected a redirect entry, got {other:?}"),
        }
        assert_eq!(entry_key(registry.login_entry(None)), "platform:platform");
    }

    // ---- 验收用例：守卫 -------------------------------------------------

    #[test]
    fn managed_rejects_local_password_without_explicit_opt_in() {
        let err = AuthProviderRegistry::for_deployment(
            map(vec![(
                LOCAL_PASSWORD_PROVIDER_ID,
                local_password_toml(""),
            )]),
            false,
        )
        .expect_err("managed mode must reject local_password by default");
        let message = err.to_string();
        assert!(message.contains("local_password"), "{message}");
        assert!(message.contains("allow_in_managed"), "{message}");
        assert!(message.contains("allowlist"), "{message}");

        // Explicit opt-in is accepted, and the built-in default stays platform.
        let registry = AuthProviderRegistry::for_deployment(
            map(vec![(
                LOCAL_PASSWORD_PROVIDER_ID,
                local_password_toml("allow_in_managed = true"),
            )]),
            false,
        )
        .expect("explicit opt-in is accepted");
        assert_eq!(
            enabled_ids(&registry),
            vec!["local_password".to_string(), "platform".to_string()]
        );
        assert_eq!(entry_key(registry.login_entry(None)), "platform:platform");

        // Switching it off also satisfies the guard.
        let registry = AuthProviderRegistry::for_deployment(
            map(vec![(
                LOCAL_PASSWORD_PROVIDER_ID,
                local_password_toml("enabled = false"),
            )]),
            false,
        )
        .expect("disabled local_password is fine");
        assert_eq!(enabled_ids(&registry), vec!["platform".to_string()]);
    }

    #[test]
    fn managed_can_disable_the_platform_provider() {
        let platform: AuthProviderConfig = toml::from_str(
            r#"
kind = "platform"
enabled = false
"#,
        )
        .expect("platform provider parses");
        let registry = AuthProviderRegistry::for_deployment(
            map(vec![(PLATFORM_PROVIDER_ID, platform)]),
            false,
        )
        .expect("registry builds");

        assert!(registry.enabled_views().is_empty());
        assert_eq!(
            entry_key(registry.login_entry(Some(PLATFORM_PROVIDER_ID))),
            format!("disabled:{PLATFORM_PROVIDER_ID}")
        );
        // No enabled provider left → nothing to start by default.
        assert_eq!(entry_key(registry.login_entry(None)), "unknown");

        // The driver is refused too, so a stray callback cannot use it either.
        assert!(registry.driver(PLATFORM_PROVIDER_ID).is_err());
    }

    #[test]
    fn provider_views_carry_an_icon_with_a_per_kind_default() {
        let registry = AuthProviderRegistry::for_deployment(map(vec![]), true)
            .expect("standalone registry");
        let views = registry.enabled_views();
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].id, LOCAL_PASSWORD_PROVIDER_ID);
        assert_eq!(views[0].icon, "key");

        // An explicit `icon` wins over the kind default.
        let mut custom = local_password_toml("icon = \"globe\"");
        custom.icon = "globe".to_string();
        let registry = AuthProviderRegistry::for_deployment(
            map(vec![(LOCAL_PASSWORD_PROVIDER_ID, custom)]),
            true,
        )
        .expect("registry builds");
        assert_eq!(registry.enabled_views()[0].icon, "globe");

        // Redirect providers get their own defaults.
        let registry = AuthProviderRegistry::for_deployment(
            map(vec![("corp", dingtalk())]),
            false,
        )
        .expect("managed registry");
        let view = registry
            .enabled_views()
            .into_iter()
            .find(|view| view.id == "corp")
            .expect("corp present");
        assert_eq!(view.icon, "user");
    }

    #[test]
    fn duplicate_callback_paths_are_rejected() {
        // A platform provider pointed at another provider's path would make the
        // router dispatch the wrong provider, so the registry refuses it.
        let platform: AuthProviderConfig = toml::from_str(
            "kind = \"platform\"\ncallback_path = \"/api/auth/callback/dingtalk\"\n",
        )
        .expect("platform provider parses");
        let err = AuthProviderRegistry::for_deployment(
            map(vec![(PLATFORM_PROVIDER_ID, platform), ("dingtalk", dingtalk())]),
            false,
        )
        .expect_err("duplicate callback paths are rejected");
        assert!(err.to_string().contains("callback path"), "{err}");
    }

    #[test]
    fn platform_keeps_the_legacy_callback_path() {
        let registry = AuthProviderRegistry::for_deployment(map(vec![]), false)
            .expect("managed registry");
        assert_eq!(
            registry
                .get(PLATFORM_PROVIDER_ID)
                .expect("platform present")
                .callback_path(PLATFORM_PROVIDER_ID),
            crate::auth_provider::config::PLATFORM_CALLBACK_PATH
        );
        // Only the platform provider needs a route outside the unified path.
        assert_eq!(
            registry.callback_routes(),
            vec![(
                PLATFORM_PROVIDER_ID,
                crate::auth_provider::config::PLATFORM_CALLBACK_PATH.to_string()
            )]
        );

        // An explicit path wins.
        let platform: AuthProviderConfig =
            toml::from_str("kind = \"platform\"\ncallback_path = \"/sso/platform/cb\"\n")
                .expect("platform provider parses");
        let registry = AuthProviderRegistry::for_deployment(
            map(vec![(PLATFORM_PROVIDER_ID, platform)]),
            false,
        )
        .expect("registry builds");
        assert_eq!(
            registry.callback_routes(),
            vec![(PLATFORM_PROVIDER_ID, "/sso/platform/cb".to_string())]
        );

        // Standalone has no platform entry at all, so no legacy route.
        let registry = AuthProviderRegistry::for_deployment(map(vec![]), true)
            .expect("standalone registry");
        assert!(registry.callback_routes().is_empty());
    }

    #[test]
    fn callback_path_must_be_a_plain_path() {
        for bad in ["api/auth/cb", "/api/auth/cb?x=1", "/api/auth/cb#f", "/api/auth c b"] {
            let config: AuthProviderConfig = toml::from_str(&format!(
                "kind = \"platform\"\ncallback_path = \"{bad}\"\n"
            ))
            .expect("parses");
            let err = config.validate("platform").expect_err("bad path is rejected");
            assert!(err.to_string().contains("callback_path"), "{bad}: {err}");
        }
    }

    #[test]
    fn undeclared_provider_is_unknown() {
        let registry =
            AuthProviderRegistry::for_deployment(map(vec![]), false).expect("managed registry");
        assert_eq!(entry_key(registry.login_entry(Some("dingtalk"))), "unknown");
        assert_eq!(
            entry_key(registry.login_entry(Some("  "))),
            entry_key(registry.login_entry(None))
        );
    }

    // ---- 既有行为 -------------------------------------------------------

    #[test]
    fn looks_up_enabled_providers_and_builds_drivers() {
        let mut disabled = dingtalk();
        disabled.enabled = Some(false);

        let registry =
            AuthProviderRegistry::new(map(vec![("dingtalk", dingtalk()), ("feishu", disabled)]))
                .expect("registry validates");

        assert_eq!(registry.len(), 2);
        assert_eq!(registry.ids().collect::<Vec<_>>(), vec!["dingtalk", "feishu"]);
        assert_eq!(
            registry.enabled().map(|(id, _)| id).collect::<Vec<_>>(),
            vec!["dingtalk"]
        );

        let driver = registry.driver("dingtalk").expect("driver builds");
        assert_eq!(driver.provider_id(), "dingtalk");
        assert_eq!(driver.kind(), AuthProviderKind::Oauth2Userinfo);

        assert!(registry
            .driver("feishu")
            .err()
            .expect("disabled provider has no driver")
            .to_string()
            .contains("disabled"));
        assert!(registry
            .driver("nope")
            .err()
            .expect("unknown provider has no driver")
            .to_string()
            .contains("unknown"));
    }

    #[test]
    fn rejects_invalid_enabled_provider_but_skips_disabled() {
        let mut broken = dingtalk();
        broken.token.url = String::new();

        let err = AuthProviderRegistry::new(map(vec![("dingtalk", broken.clone())]))
            .expect_err("empty url is rejected");
        assert!(err.to_string().contains("dingtalk"), "{err}");
        assert!(err.to_string().contains("url is required"), "{err}");

        // Same broken entry, switched off: startup keeps working.
        broken.enabled = Some(false);
        let registry = AuthProviderRegistry::new(map(vec![("dingtalk", broken)]))
            .expect("disabled providers are not validated");
        assert!(registry.enabled().next().is_none());
    }

    #[test]
    fn empty_registry_is_usable() {
        let registry = AuthProviderRegistry::empty();
        assert!(registry.is_empty());
        assert!(registry.get("dingtalk").is_none());
        assert!(registry.driver("dingtalk").is_err());
        assert_eq!(entry_key(registry.login_entry(None)), "unknown");
    }

    #[test]
    fn platform_is_not_driver_backed() {
        let registry =
            AuthProviderRegistry::for_deployment(map(vec![]), false).expect("managed registry");
        let err = registry
            .driver(PLATFORM_PROVIDER_ID)
            .err()
            .expect("platform has no driver");
        assert!(err.to_string().contains("server layer"), "{err}");
    }

    fn local_cfg() -> AuthProviderConfig {
        AuthProviderConfig::builtin(AuthProviderKind::LocalPassword, "账号密码")
    }

    fn oidc_cfg() -> AuthProviderConfig {
        let mut cfg = AuthProviderConfig::builtin(AuthProviderKind::Oidc, "Keycloak");
        cfg.issuer = "https://kc.example.com/realms/main".to_string();
        cfg.oidc.client_id = "pointer".to_string();
        cfg.oidc.client_secret = "secret".to_string();
        cfg
    }

    fn ticket_cfg() -> AuthProviderConfig {
        let mut cfg = AuthProviderConfig::builtin(AuthProviderKind::TokenLookup, "门户");
        cfg.token_lookup.signing.secret = "s".to_string();
        cfg
    }

    #[test]
    fn primary_follows_kind_priority_when_unset() {
        let registry = AuthProviderRegistry::for_deployment(
            map(vec![
                ("local", local_cfg()),
                ("corp", oidc_cfg()),
                ("dingtalk", dingtalk()),
            ]),
            true,
        )
        .expect("registry");
        let views = registry.enabled_views();
        let primary: Vec<_> = views
            .iter()
            .filter(|view| view.primary)
            .map(|view| view.id.as_str())
            .collect();
        assert_eq!(
            primary,
            vec!["corp"],
            "oidc outranks oauth2_userinfo and local_password"
        );
    }

    #[test]
    fn explicit_primary_wins_and_token_lookup_is_never_primary() {
        let mut local = local_cfg();
        local.primary = true;
        let mut portal = ticket_cfg();
        portal.primary = true;
        let registry = AuthProviderRegistry::for_deployment(
            map(vec![("corp", oidc_cfg()), ("local", local), ("portal", portal)]),
            true,
        )
        .expect("registry");
        assert_eq!(
            registry.primary_provider_id().as_deref(),
            Some("local"),
            "explicit primary must win over kind priority"
        );

        let mut portal_only = ticket_cfg();
        portal_only.primary = true;
        let only_portal =
            AuthProviderRegistry::for_deployment(map(vec![("portal", portal_only)]), true)
                .expect("registry");
        assert_ne!(
            only_portal.primary_provider_id().as_deref(),
            Some("portal"),
            "the ticket entry is reached from the portal, never from the login page"
        );
        assert_eq!(
            only_portal.primary_provider_id().as_deref(),
            Some("local_password"),
            "standalone injects the local entry, which then owns the primary slot"
        );
    }
}
