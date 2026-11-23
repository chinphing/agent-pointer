//! `pointer-server auth doctor` — read-only preflight for browser login.
//!
//! Every check prints 项 → 结论 → 修复建议. Nothing here writes config, touches
//! the session store, or makes a network call unless `--network` is passed.

use std::collections::BTreeMap;
use std::net::{TcpStream, ToSocketAddrs};
use std::path::PathBuf;
use std::time::Duration;

/// Registry construction can fail with a config reason; keep it explicit so the
/// checks can report the reason instead of `?`-bailing.
type RegistryResult = anyhow::Result<AuthProviderRegistry>;

use pointer_core::auth_provider::{
    default_callback_path, AuthCodeRequest, AuthProviderConfig, AuthProviderKind,
    AuthProviderRegistry, LOCAL_PASSWORD_PROVIDER_ID, PLATFORM_CALLBACK_PATH, PLATFORM_PROVIDER_ID,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Ok,
    Warn,
    Fail,
}

impl Status {
    fn tag(self) -> &'static str {
        match self {
            Status::Ok => "OK  ",
            Status::Warn => "WARN",
            Status::Fail => "FAIL",
        }
    }
}

pub struct Check {
    pub name: &'static str,
    pub status: Status,
    pub detail: String,
    pub hint: Option<String>,
}

impl Check {
    fn ok(name: &'static str, detail: impl Into<String>) -> Self {
        Self {
            name,
            status: Status::Ok,
            detail: detail.into(),
            hint: None,
        }
    }

    fn warn(name: &'static str, detail: impl Into<String>, hint: impl Into<String>) -> Self {
        Self {
            name,
            status: Status::Warn,
            detail: detail.into(),
            hint: Some(hint.into()),
        }
    }

    fn fail(name: &'static str, detail: impl Into<String>, hint: impl Into<String>) -> Self {
        Self {
            name,
            status: Status::Fail,
            detail: detail.into(),
            hint: Some(hint.into()),
        }
    }
}

pub struct Report {
    pub config_path: Option<PathBuf>,
    pub config_failed: bool,
    pub standalone: bool,
    pub checks: Vec<Check>,
}

impl Report {
    pub fn failed(&self) -> bool {
        self.checks.iter().any(|c| c.status == Status::Fail)
    }

    fn count(&self, status: Status) -> usize {
        self.checks.iter().filter(|c| c.status == status).count()
    }

    /// Three-part rendering: 项 → 结论 → 修复建议.
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str("pointer-server auth doctor（只读检查，不修改任何状态）\n");
        let config_note = if self.config_failed {
            "（解析失败，见下方「配置文件」项）".to_string()
        } else {
            self.config_path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "（未找到）".to_string())
        };
        out.push_str(&format!("config: {config_note}\n"));
        out.push_str(&format!(
            "mode:   {}\n\n",
            if self.standalone {
                "standalone"
            } else {
                "managed"
            }
        ));
        for check in &self.checks {
            out.push_str(&format!("[{}] {}\n", check.status.tag(), check.name));
            for line in check.detail.lines() {
                out.push_str(&format!("       {line}\n"));
            }
            if let Some(hint) = &check.hint {
                for line in hint.lines() {
                    out.push_str(&format!("       修复：{line}\n"));
                }
            }
        }
        out.push_str(&format!(
            "\n汇总：OK {} · WARN {} · FAIL {}\n",
            self.count(Status::Ok),
            self.count(Status::Warn),
            self.count(Status::Fail)
        ));
        out.push_str(if self.failed() {
            "结论：存在需要修的问题，登录链路按上面的建议修正后重跑。\n"
        } else {
            "结论：未发现阻塞登录的问题。\n"
        });
        out
    }
}

/// Everything `diagnose` needs, so the checks stay pure and testable.
pub struct Inputs {
    pub config_path: Option<PathBuf>,
    pub config_error: Option<String>,
    pub providers: BTreeMap<String, AuthProviderConfig>,
    pub standalone: bool,
    pub public_url: String,
    pub network: bool,
    /// `--exchange`: send one fake-code token request per redirect provider.
    pub exchange: bool,
}

/// Fixed routes that a provider `callback_path` must not shadow.
const FIXED_ROUTES: &[&str] = &[
    "/api/auth/mode",
    "/api/auth/providers",
    "/api/auth/login/start",
    "/api/auth/login/finish",
    "/api/auth/logout",
    "/api/auth/refresh",
    "/api/license/status",
    "/api/license/reload",
];

pub fn diagnose(inputs: &Inputs) -> Report {
    let mut checks = Vec::new();
    checks.push(check_config_file(inputs));

    let builtin_id = if inputs.standalone {
        LOCAL_PASSWORD_PROVIDER_ID
    } else {
        PLATFORM_PROVIDER_ID
    };
    checks.push(Check::ok(
        "部署模式",
        format!(
            "{}；未显式配置时默认注入 provider `{builtin_id}`",
            if inputs.standalone {
                "standalone"
            } else {
                "managed"
            }
        ),
    ));

    let registry =
        AuthProviderRegistry::for_deployment(inputs.providers.clone(), inputs.standalone);
    match &registry {
        Ok(registry) => checks.push(Check::ok("provider 注册表", describe_providers(registry))),
        Err(e) => checks.push(Check::fail(
            "provider 注册表",
            format!("{e:#}"),
            provider_fix_hint(&e.to_string()),
        )),
    }
    checks.push(check_default_provider(&registry, builtin_id));
    checks.push(check_public_url(inputs, &registry));
    checks.push(check_callback_paths(&registry));
    checks.push(Check::ok(
        "会话存储",
        "会话与登录中状态保存在进程内存（web_sessions / auth_pending，TTL 600s）：\
         只支持单实例部署，重启会让在线会话失效。",
    ));
    if inputs.network {
        checks.push(check_network(&registry));
    }
    checks.push(Check::ok(
        "令牌换发",
        if inputs.exchange {
            "由调用方追加（--exchange：对每个 token 端点发一次假 code）"
        } else {
            "未检查（默认离线）；加 --exchange 才会向 token 端点发一次假 code 请求"
        },
    ));

    Report {
        config_path: inputs.config_path.clone(),
        config_failed: inputs.config_error.is_some(),
        standalone: inputs.standalone,
        checks,
    }
}

fn check_config_file(inputs: &Inputs) -> Check {
    if let Some(err) = &inputs.config_error {
        return Check::fail(
            "配置文件",
            format!("无法解析：{err}"),
            "先修好 TOML 语法（`pointer-server auth doctor --config <path>` 可指定文件）；\
             未配置时也可以不建文件，改用环境变量。",
        );
    }
    match &inputs.config_path {
        Some(path) => Check::ok("配置文件", format!("已加载 {}", path.display())),
        None => Check::warn(
            "配置文件",
            "未找到 pointer-server.toml，只使用部署模式注入的默认 provider",
            "把配置放在 `{exe_dir}/pointer-server.toml`，或用 `POINTER_SERVER_CONFIG` / \
             `--config <path>` 指定；确认无误可忽略本项。",
        ),
    }
}

fn describe_providers(registry: &AuthProviderRegistry) -> String {
    let mut lines = vec![format!("{} 个已启用 provider：", registry.len())];
    for (id, config) in registry.enabled() {
        lines.push(format!("· {id}（{}）", config.kind.as_str()));
    }
    lines.join("\n")
}

fn provider_fix_hint(message: &str) -> &'static str {
    if message.contains("allow_in_managed") {
        "managed 部署要保留账号密码登录：在 [auth.providers.local_password] 设 \
         `allow_in_managed = true`；不需要就设 `enabled = false`。"
    } else if message.contains("callback path") {
        "两个 provider 用了同一个 callback_path：给其中一个改成别的路径。"
    } else if message.contains("kind") {
        "kind 只能是 platform / local_password / token_lookup / oauth2_userinfo / oidc。"
    } else {
        "按上面的原因修改 [auth.providers.<id>] 后重跑 doctor。"
    }
}

fn check_default_provider(registry: &RegistryResult, builtin_id: &str) -> Check {
    let Ok(registry) = registry else {
        return Check::warn(
            "默认 provider",
            "provider 注册表未通过校验，无法确认默认项",
            "先修好 provider 注册表一项。",
        );
    };
    match registry.get(builtin_id) {
        Some(config) if config.is_enabled() => Check::ok(
            "默认 provider",
            format!("`{builtin_id}` 可用；未指定 provider 的登录请求走它"),
        ),
        _ => Check::warn(
            "默认 provider",
            format!("部署模式默认的 `{builtin_id}` 已被移除或停用"),
            "不带 `?provider=` 的 `POST /api/auth/login/start` 会返回 unknown_provider；\
             登录页按 /api/auth/providers 渲染，仍可用其它方式登录。",
        ),
    }
}

fn check_public_url(inputs: &Inputs, registry: &RegistryResult) -> Check {
    let raw = inputs.public_url.trim();
    let needs_public_url = registry
        .as_ref()
        .map(|registry| {
            registry
                .enabled()
                .any(|(_, config)| config.kind.requires_allowlist() || config.kind.is_redirect())
        })
        .unwrap_or(!inputs.standalone);

    if raw.is_empty() {
        return if needs_public_url {
            Check::fail(
                "public_url",
                "未配置，但当前启用了需要回跳的登录方式",
                "在 [server] public_url 写入外部可达地址，例如 https://pointer.example.com。",
            )
        } else {
            Check::warn(
                "public_url",
                "未配置；账号密码登录可用，但回调与媒体下载链接无法生成",
                "需要门户跳转或对外下载链接时，在 [server] public_url 写入 https 地址。",
            )
        };
    }
    if raw.ends_with('/') {
        return Check::warn(
            "public_url",
            format!("`{raw}` 末尾多了 `/`，拼接后会得到 `//api/...`"),
            "去掉末尾斜杠，写成 https://host[:port]。",
        );
    }
    let lower = raw.to_ascii_lowercase();
    if lower.starts_with("https://") {
        return Check::ok("public_url", raw);
    }
    if lower.starts_with("http://") {
        let host = raw
            .trim_start_matches("http://")
            .split(['/', ':'])
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        let loopback = matches!(host.as_str(), "localhost" | "127.0.0.1" | "::1" | "[::1]");
        return if loopback {
            Check::warn(
                "public_url",
                format!("`{raw}` 是明文 http 的本机地址，只适合本地调试"),
                "对外部署改成 https，否则回调 Cookie 与票据会走明文。",
            )
        } else {
            Check::fail(
                "public_url",
                format!("`{raw}` 不是 https"),
                "控制面与浏览器都要求 https 回调：把 public_url 改成 https://…。",
            )
        };
    }
    Check::fail(
        "public_url",
        format!("`{raw}` 不是完整 URL"),
        "写成带 scheme 的地址，例如 https://pointer.example.com。",
    )
}

fn check_callback_paths(registry: &RegistryResult) -> Check {
    let Ok(registry) = registry else {
        return Check::warn(
            "回调路径",
            "provider 注册表未通过校验，跳过回调路径检查",
            "先修好 provider 注册表一项。",
        );
    };
    let mut lines = Vec::new();
    let mut problems = Vec::new();
    for (id, config) in registry.enabled() {
        let path = config.callback_path(id);
        if FIXED_ROUTES.contains(&path.as_str()) {
            problems.push(format!("`{id}` 的 callback_path `{path}` 与固定路由同名"));
        }
        if path.starts_with("/api/auth/callback/") && path != default_callback_path(id) {
            problems.push(format!(
                "`{id}` 的 callback_path `{path}` 会顶掉 `/api/auth/callback/:provider` 的参数路由"
            ));
        }
        if id == PLATFORM_PROVIDER_ID && path == PLATFORM_CALLBACK_PATH {
            lines.push(format!("· {id} → {path}（控制面旧路径，白名单无需改动）"));
        } else {
            lines.push(format!("· {id} → {path}"));
        }
    }
    if !problems.is_empty() {
        return Check::fail(
            "回调路径",
            problems.join("\n"),
            "给相关 provider 换一个不冲突的 callback_path，或删掉该字段改用默认 \
             /api/auth/callback/<id>。",
        );
    }
    Check::ok("回调路径", lines.join("\n"))
}

fn check_network(registry: &RegistryResult) -> Check {
    let Ok(registry) = registry else {
        return Check::warn(
            "出网连通性",
            "provider 注册表未通过校验，跳过联网检查",
            "先修好 provider 注册表一项。",
        );
    };
    let mut lines = Vec::new();
    let mut failures = Vec::new();
    for (id, config) in registry.enabled() {
        let urls = provider_urls(config);
        if urls.is_empty() {
            continue;
        }
        for url in urls {
            match tcp_reachable(&url) {
                Ok(target) => lines.push(format!("· {id} → {target} 可连接")),
                Err(err) => failures.push(format!("· {id} → {url} 不可连接：{err}")),
            }
        }
    }
    if lines.is_empty() && failures.is_empty() {
        return Check::ok("出网连通性", "当前 provider 没有需要访问的外部地址");
    }
    if !failures.is_empty() {
        return Check::warn(
            "出网连通性",
            failures.join("\n"),
            "确认 DNS / 代理 / 防火墙放通这些地址，否则对应登录方式会超时。",
        );
    }
    Check::ok("出网连通性", lines.join("\n"))
}

fn provider_urls(config: &AuthProviderConfig) -> Vec<String> {
    let mut urls = Vec::new();
    let mut push = |url: &str| {
        let url = url.trim();
        if !url.is_empty() {
            urls.push(url.to_string());
        }
    };
    match config.kind {
        AuthProviderKind::Oauth2Userinfo | AuthProviderKind::Oidc => {
            push(&config.authorize.url);
            push(&config.token.url);
            if matches!(config.kind, AuthProviderKind::Oauth2Userinfo) {
                push(&config.userinfo.url);
            }
            push(&config.oidc.discovery_url);
        }
        AuthProviderKind::TokenLookup => push(&config.token_lookup.lookup.url),
        AuthProviderKind::Platform | AuthProviderKind::LocalPassword => {}
    }
    urls
}

fn tcp_reachable(url: &str) -> Result<String, String> {
    let (host, port) = split_host_port(url)?;
    let addrs = (host.as_str(), port)
        .to_socket_addrs()
        .map_err(|e| format!("DNS 解析失败：{e}"))?;
    let mut last = String::from("无可用地址");
    for addr in addrs {
        match TcpStream::connect_timeout(&addr, Duration::from_secs(5)) {
            Ok(_) => return Ok(format!("{host}:{port}")),
            Err(e) => last = e.to_string(),
        }
    }
    Err(last)
}

fn split_host_port(url: &str) -> Result<(String, u16), String> {
    let (scheme, rest) = url
        .split_once("://")
        .ok_or_else(|| format!("`{url}` 缺少 scheme"))?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty() {
        return Err(format!("`{url}` 缺少主机名"));
    }
    let default_port = if scheme.eq_ignore_ascii_case("https") {
        443
    } else {
        80
    };
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) if port.chars().all(|c| c.is_ascii_digit()) && !port.is_empty() => (
            host.to_string(),
            port.parse::<u16>().map_err(|e| e.to_string())?,
        ),
        _ => (authority.to_string(), default_port),
    };
    Ok((host.trim_matches(['[', ']']).to_string(), port))
}

/// Code / verifier sent to the token endpoint. Deliberately invalid: a healthy
/// endpoint must answer with an OAuth error, not a token.
const EXCHANGE_PROBE_CODE: &str = "pointer-doctor-probe-code";
const EXCHANGE_PROBE_VERIFIER: &str = "pointer-doctor-probe-verifier-0123456789abcdefghij";

/// How a token-endpoint probe ended.
#[derive(Debug, PartialEq, Eq)]
enum ExchangeOutcome {
    /// The endpoint answered with a standard OAuth error — the config works.
    OAuthError,
    /// HTTP response, but no OAuth error code: wrong endpoint or wrong client.
    HttpStatusOnly,
    /// Network, TLS, timeout, or body-parse failure.
    Transport,
}

fn classify_exchange_error(message: &str) -> ExchangeOutcome {
    const OAUTH_ERRORS: [&str; 6] = [
        "invalid_grant",
        "invalid_client",
        "invalid_request",
        "unsupported_grant_type",
        "invalid_scope",
        "unauthorized_client",
    ];
    let lower = message.to_ascii_lowercase();
    if OAUTH_ERRORS.iter().any(|code| lower.contains(code)) {
        return ExchangeOutcome::OAuthError;
    }
    if lower.contains("http_status=") {
        return ExchangeOutcome::HttpStatusOnly;
    }
    ExchangeOutcome::Transport
}

/// One fake-code request per provider that has a token endpoint.
async fn check_token_exchange(registry: &RegistryResult, inputs: &Inputs) -> Check {
    let Ok(registry) = registry else {
        return Check::warn(
            "令牌换发",
            "provider 注册表未通过校验，跳过换发检查",
            "先修好 provider 注册表一项。",
        );
    };
    let base = inputs.public_url.trim().trim_end_matches('/');
    let mut ok = Vec::new();
    let mut warnings = Vec::new();
    let mut failures = Vec::new();
    for (id, config) in registry.enabled() {
        if !uses_token_endpoint(config) {
            continue;
        }
        if base.is_empty() {
            warnings.push(format!("· {id} → 缺少 public_url，无法构造 redirect_uri"));
            continue;
        }
        let redirect_uri = format!("{base}{}", config.callback_path(id));
        let Ok(driver) = registry.driver(id) else {
            failures.push(format!("· {id} → 无法构造 driver"));
            continue;
        };
        // `authenticate` is the trait's code-exchange entry point: with a fake
        // code a healthy endpoint answers with an OAuth error before any user
        // info is fetched.
        let request = AuthCodeRequest {
            code: EXCHANGE_PROBE_CODE,
            redirect_uri: &redirect_uri,
            code_verifier: Some(EXCHANGE_PROBE_VERIFIER),
            nonce: None,
        };
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            driver.authenticate(request),
        )
        .await;
        match result {
            Err(_) => failures.push(format!("· {id} → token 端点 10s 超时")),
            Ok(Ok(_)) => warnings.push(format!(
                "· {id} → token 端点接受了假 code（不应发生，确认端点是否真的是 OAuth token 端点）"
            )),
            Ok(Err(e)) => match classify_exchange_error(&format!("{e:#}")) {
                ExchangeOutcome::OAuthError => {
                    ok.push(format!("· {id} → 返回标准 OAuth 错误，端点与凭据可用"))
                }
                ExchangeOutcome::HttpStatusOnly => warnings.push(format!(
                    "· {id} → 有 HTTP 响应但不是标准 OAuth 错误，检查 token.url / client_id / client_secret"
                )),
                ExchangeOutcome::Transport => {
                    failures.push(format!("· {id} → token 端点不可达：{e:#}"))
                }
            },
        }
    }
    if ok.is_empty() && warnings.is_empty() && failures.is_empty() {
        return Check::ok("令牌换发", "没有配置 token 端点的 provider，跳过");
    }
    if !failures.is_empty() {
        return Check::fail(
            "令牌换发",
            failures.join("\n"),
            "确认出网/DNS/代理可用，且 token.url 指向 OAuth token 端点后重跑。",
        );
    }
    if !warnings.is_empty() {
        let mut detail = warnings;
        detail.extend(ok);
        return Check::warn(
            "令牌换发",
            detail.join("\n"),
            "端点有响应但不像标准 OAuth 错误：核对 client_id / client_secret / token.url。",
        );
    }
    Check::ok("令牌换发", ok.join("\n"))
}

/// Whether a provider can be probed for a token exchange.
///
/// `oidc` gets its token endpoint from the discovery document, so an empty
/// `token.url` is normal there — checking only `token.url` skipped every OIDC
/// provider (the exact bug this predicate documents).
fn uses_token_endpoint(config: &AuthProviderConfig) -> bool {
    match config.kind {
        AuthProviderKind::Oauth2Userinfo => !config.token.url.trim().is_empty(),
        AuthProviderKind::Oidc => {
            !config.token.url.trim().is_empty()
                || !config.oidc.discovery_url.trim().is_empty()
                || !config.issuer.trim().is_empty()
        }
        AuthProviderKind::Platform
        | AuthProviderKind::LocalPassword
        | AuthProviderKind::TokenLookup => false,
    }
}

/// Load the real config, then run the same checks the tests exercise.
pub async fn run_from_environment(network: bool, exchange: bool) -> Report {
    let (config_path, config_error) = match pointer_core::server_config::load_server_config() {
        Ok(Some(result)) => (Some(result.path), None),
        Ok(None) => (None, None),
        Err(e) => (None, Some(format!("{e:#}"))),
    };
    let inputs = Inputs {
        config_path,
        config_error,
        providers: pointer_core::server_config::parsed_auth_providers(),
        standalone: pointer_core::deployment_mode::is_standalone(),
        public_url: std::env::var("POINTER_SERVER_PUBLIC_URL").unwrap_or_default(),
        network,
        exchange,
    };
    let mut report = diagnose(&inputs);
    if exchange {
        let registry =
            AuthProviderRegistry::for_deployment(inputs.providers.clone(), inputs.standalone);
        report.checks.retain(|check| check.name != "令牌换发");
        report
            .checks
            .push(check_token_exchange(&registry, &inputs).await);
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local_password() -> AuthProviderConfig {
        AuthProviderConfig::builtin(AuthProviderKind::LocalPassword, "账号密码")
    }

    fn signed_ticket(path: &str) -> AuthProviderConfig {
        let mut config = AuthProviderConfig::builtin(AuthProviderKind::TokenLookup, "门户");
        config.callback_path = path.to_string();
        config.token_lookup.signing.secret = "s".to_string();
        config
    }

    fn standalone_inputs() -> Inputs {
        Inputs {
            config_path: Some(PathBuf::from("/etc/pointer/pointer-server.toml")),
            config_error: None,
            providers: BTreeMap::new(),
            standalone: true,
            public_url: "https://pointer.example.com".to_string(),
            network: false,
            exchange: false,
        }
    }

    #[test]
    fn oidc_providers_are_eligible_for_the_exchange_probe() {
        // Regression: `oidc` leaves `token.url` empty (the endpoint comes from
        // discovery), so a `token.url`-only predicate skipped every OIDC
        // provider and the doctor printed "没有配置 token 端点的 provider，跳过".
        let mut oidc = AuthProviderConfig::builtin(AuthProviderKind::Oidc, "Keycloak");
        oidc.issuer = "https://kc.example.com/realms/main".to_string();
        oidc.token.url = String::new();
        assert!(
            uses_token_endpoint(&oidc),
            "oidc with an issuer must be probed"
        );

        let mut discovery_only = AuthProviderConfig::builtin(AuthProviderKind::Oidc, "KC");
        discovery_only.issuer = String::new();
        discovery_only.oidc.discovery_url =
            "https://kc.example.com/.well-known/openid-configuration".to_string();
        assert!(
            uses_token_endpoint(&discovery_only),
            "oidc with only discovery_url must be probed"
        );

        let mut oauth2 = AuthProviderConfig::builtin(AuthProviderKind::Oauth2Userinfo, "OAuth2");
        oauth2.token.url = String::new();
        assert!(
            !uses_token_endpoint(&oauth2),
            "oauth2 without token.url is skipped"
        );
        oauth2.token.url = "https://idp.example.com/token".to_string();
        assert!(uses_token_endpoint(&oauth2));

        assert!(!uses_token_endpoint(&local_password()));
        assert!(!uses_token_endpoint(&signed_ticket(
            "/api/auth/callback/portal"
        )));
    }

    #[test]
    fn empty_config_is_readable_and_not_fatal() {
        let report = diagnose(&Inputs {
            config_path: None,
            config_error: None,
            providers: BTreeMap::new(),
            standalone: true,
            public_url: String::new(),
            network: false,
            exchange: false,
        });
        assert!(!report.failed(), "{}", report.render());
        let rendered = report.render();
        assert!(
            rendered.contains("未找到 pointer-server.toml"),
            "{rendered}"
        );
        assert!(rendered.contains("local_password"), "{rendered}");
        assert!(rendered.contains("单实例"), "{rendered}");
        assert!(rendered.contains("修复："), "{rendered}");
    }

    #[test]
    fn broken_config_reports_parse_error_with_a_fix() {
        let report = diagnose(&Inputs {
            config_path: None,
            config_error: Some("TOML parse error at line 3: expected `=`".to_string()),
            providers: BTreeMap::new(),
            standalone: true,
            public_url: String::new(),
            network: false,
            exchange: false,
        });
        assert!(report.failed());
        let rendered = report.render();
        assert!(rendered.contains("无法解析"), "{rendered}");
        assert!(rendered.contains("expected `=`"), "{rendered}");
        assert!(rendered.contains("修复："), "{rendered}");
    }

    #[test]
    fn managed_local_password_without_opt_in_fails_with_a_hint() {
        let mut providers = BTreeMap::new();
        providers.insert(LOCAL_PASSWORD_PROVIDER_ID.to_string(), local_password());
        let report = diagnose(&Inputs {
            config_path: None,
            config_error: None,
            providers,
            standalone: false,
            public_url: "https://pointer.example.com".to_string(),
            network: false,
            exchange: false,
        });
        assert!(report.failed());
        let rendered = report.render();
        assert!(rendered.contains("allow_in_managed"), "{rendered}");
        assert!(rendered.contains("修复："), "{rendered}");
    }

    #[test]
    fn healthy_standalone_config_has_no_findings() {
        let mut providers = BTreeMap::new();
        providers.insert(LOCAL_PASSWORD_PROVIDER_ID.to_string(), local_password());
        let report = diagnose(&Inputs {
            providers,
            ..standalone_inputs()
        });
        assert!(!report.failed(), "{}", report.render());
        assert_eq!(report.count(Status::Warn), 0, "{}", report.render());
        assert!(report.render().contains("结论：未发现阻塞登录的问题"));
    }

    #[test]
    fn http_public_url_outside_loopback_is_a_failure() {
        let report = diagnose(&Inputs {
            public_url: "http://pointer.example.com".to_string(),
            ..standalone_inputs()
        });
        assert!(report.failed());
        assert!(report.render().contains("不是 https"));
    }

    #[test]
    fn callback_path_that_shadows_the_param_route_fails() {
        let mut providers = BTreeMap::new();
        providers.insert(
            "corp".to_string(),
            signed_ticket("/api/auth/callback/other"),
        );
        let report = diagnose(&Inputs {
            providers,
            ..standalone_inputs()
        });
        assert!(report.failed());
        assert!(report.render().contains("参数路由"));
    }

    #[test]
    fn platform_keeps_the_legacy_path_without_a_finding() {
        let report = diagnose(&Inputs {
            standalone: false,
            public_url: "https://pointer.example.com".to_string(),
            ..standalone_inputs()
        });
        assert!(!report.failed(), "{}", report.render());
        assert!(report.render().contains("/api/auth/oauth/callback"));
    }

    #[test]
    fn exchange_errors_are_classified_by_what_came_back() {
        assert_eq!(
            classify_exchange_error(
                "auth provider `corp`: token exchange failed http_status=400 (400 Bad Request): {\"error\":\"invalid_grant\"}"
            ),
            ExchangeOutcome::OAuthError
        );
        assert_eq!(
            classify_exchange_error("token exchange failed: invalid_client"),
            ExchangeOutcome::OAuthError
        );
        assert_eq!(
            classify_exchange_error("token exchange failed http_status=502 (502 Bad Gateway)"),
            ExchangeOutcome::HttpStatusOnly
        );
        assert_eq!(
            classify_exchange_error("error sending request for url (https://idp/token): dns error"),
            ExchangeOutcome::Transport
        );
        assert_eq!(
            classify_exchange_error("token response is not valid json: expected value"),
            ExchangeOutcome::Transport
        );
    }

    #[test]
    fn url_splitting_handles_ports_and_defaults() {
        assert_eq!(
            split_host_port("https://example.com/x").unwrap(),
            ("example.com".to_string(), 443)
        );
        assert_eq!(
            split_host_port("http://127.0.0.1:8787/x").unwrap(),
            ("127.0.0.1".to_string(), 8787)
        );
        assert!(split_host_port("example.com").is_err());
    }
}
