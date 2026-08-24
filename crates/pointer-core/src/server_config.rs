//! pointer-server runtime configuration loaded from a file before other startup logic.
//!
//! Search order (first existing file wins):
//! 1. `POINTER_SERVER_CONFIG` — explicit path
//! 2. `{exe_dir}/pointer-server.toml` or `{exe_dir}/pointer-server.env`
//! 3. `{cwd}/pointer-server.toml` or `{cwd}/pointer-server.env`
//!
//! Existing OS environment variables always override file values.

use crate::dotenv::parse_dotenv_bytes;
use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

#[derive(Debug, Default, Deserialize)]
struct DeploymentSection {
    /// `platform` (default) or `standalone`.
    #[serde(default)]
    mode: String,
}

#[derive(Debug, Default, Deserialize)]
struct AuthLocalSsoSection {
    /// When false, SSO is off even if secret/audience are set.
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    secret: String,
    /// Previous secret for rotation window.
    #[serde(default)]
    secret_prev: String,
    /// Must match ticket `aud` (usually this instance public URL).
    #[serde(default)]
    audience: String,
    #[serde(default)]
    max_skew_secs: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
struct AuthLocalSection {
    #[serde(default)]
    username: String,
    #[serde(default)]
    password_hmac: String,
    #[serde(default)]
    hmac_secret: String,
    /// Deprecated: ignored when present (use username + password_hmac).
    #[serde(default)]
    admin_token: String,
    #[serde(default)]
    sso: AuthLocalSsoSection,
}

#[derive(Debug, Default, Deserialize)]
struct LicenseSection {
    #[serde(default)]
    key: String,
    #[serde(default)]
    license_file: String,
}

#[derive(Debug, Default, Deserialize)]
struct UsageSection {
    /// When false, token usage is recorded locally but not reported to the platform API.
    #[serde(default)]
    report_enabled: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
struct ServerSection {
    #[serde(default)]
    addr: String,
    #[serde(default)]
    static_dir: String,
    /// Bundled skills source directory (maps to `POINTER_SERVER_SKILLS_DIR`).
    /// Zip: `skills` beside the binary; deb: `/usr/share/pointer-server/skills`.
    #[serde(default)]
    skills_dir: String,
    /// Optional override for `POINTER_APP_DATA_DIR`. When empty, pointer-core uses
    /// the same default as the desktop client (`{data_dir}/PointerApp` or `PointerAppDev`).
    #[serde(default)]
    app_data_dir: String,
    /// Public base URL (no trailing slash) used to build OAuth `redirect_uri`
    /// for the server-side PKCE login flow. Maps to env `POINTER_SERVER_PUBLIC_URL`.
    /// Example: `https://pointer.example.com`. When empty, the server falls back
    /// to `http://{host}:{port}` derived from `POINTER_SERVER_ADDR`, including
    /// loopback binds (so local development works without configuration). Set
    /// explicitly for production behind a public domain or reverse proxy.
    #[serde(default)]
    public_url: String,
    /// Browser tab title for the served Web UI (`<title>` in `index.html`).
    /// Maps to `POINTER_SERVER_PAGE_TITLE`. Empty → default `Pointer · AI 工作台`.
    #[serde(default)]
    page_title: String,
    /// Default composer input placeholder (when logged in and API key present).
    /// Maps to `POINTER_SERVER_COMPOSER_PLACEHOLDER`. Empty → `告诉我你想做什么`.
    #[serde(default)]
    composer_placeholder: String,
    /// Platform user ids allowed to log in (maps to `POINTER_SERVER_ALLOWED_USER_IDS`).
    #[serde(default)]
    allowed_user_ids: Vec<String>,
    /// When true, startup fails if `allowed_user_ids` is empty
    /// (`POINTER_SERVER_REQUIRE_ALLOWED_USERS`).
    #[serde(default)]
    require_allowed_users: bool,
    /// SSE initial padding comment (flush proxy buffers before first event).
    /// Maps to `POINTER_SERVER_SSE_PADDING_ENABLED`. Default false (disabled).
    #[serde(default)]
    sse_padding_enabled: Option<bool>,
    /// SSE padding comment size in bytes. Maps to `POINTER_SERVER_SSE_PADDING_BYTES`.
    /// Default 10_240 (10 KB). Ignored when padding is disabled or zero.
    #[serde(default)]
    sse_padding_bytes: Option<usize>,
    /// When true, Agent `terminal` rejects `command` / `stdin` containing the
    /// literal `SESSION_USER_ID` (blocks common env overrides). Desktop client
    /// ignores this — server-only. Maps to
    /// `POINTER_SERVER_FORBID_SESSION_USER_ID_IN_TERMINAL`. Default false.
    #[serde(default)]
    forbid_session_user_id_in_terminal: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
struct PointerSection {
    #[serde(default)]
    api_base: String,
    #[serde(default)]
    oauth_client_secret: String,
}

/// Optional webhook ingress configuration. Maps to env
/// `POINTER_WEBHOOK_BEARER_TOKEN`. When unset, the generic webhook endpoint
/// (`POST /api/webhooks/:src`) rejects all requests with 401.
#[derive(Debug, Default, Deserialize)]
struct WebhooksSection {
    /// Shared bearer token expected in `Authorization: Bearer <token>`.
    #[serde(default)]
    bearer_token: String,
}

#[derive(Debug, Default, Deserialize)]
struct ServerConfigToml {
    #[serde(default)]
    deployment: DeploymentSection,
    #[serde(default)]
    server: ServerSection,
    #[serde(default)]
    auth: AuthToml,
    #[serde(default)]
    license: LicenseSection,
    #[serde(default)]
    usage: UsageSection,
    #[serde(default, alias = "openpointer")]
    pointer: PointerSection,
    #[serde(default)]
    webhooks: WebhooksSection,
    /// 任意环境变量注入（仅当该 env 未设置时生效，OS 环境优先）。
    /// 可用于配置 OTLP 导出，例如：
    /// ```toml
    /// [env]
    /// OTEL_EXPORTER_OTLP_ENDPOINT = "http://localhost:4318"
    /// OTEL_SERVICE_NAME = "pointer-server"
    /// ```
    #[serde(default)]
    env: HashMap<String, String>,
    /// P2b 全局 MCP（非插件）：`[[mcp_servers.server]]`，结构复用插件 manifest。
    #[serde(default)]
    mcp_servers: crate::plugins::manifest::McpServersDecl,
}

#[derive(Debug, Default, Deserialize)]
struct AuthToml {
    #[serde(default)]
    local: AuthLocalSection,
}

/// P2b：全局 MCP server 声明 + 配置文件所在目录（相对 command 的解析基准）。
static PARSED_MCP: OnceLock<Option<(crate::plugins::manifest::McpServersDecl, PathBuf)>> =
    OnceLock::new();

/// Result of loading server config from disk.
#[derive(Debug, Clone)]
pub struct ServerConfigLoadResult {
    pub path: PathBuf,
    pub applied: Vec<(String, String)>,
    pub skipped_env: Vec<String>,
}

/// Load server config from disk and apply unset environment variables.
/// Returns details when a file was loaded.
pub fn load_server_config() -> Result<Option<ServerConfigLoadResult>> {
    let Some(path) = resolve_config_path()? else {
        eprintln!("pointer-server: no config file found (checked exe dir and cwd for pointer-server.toml/.env)");
        crate::deployment_mode::init_from_env();
        return Ok(None);
    };
    let base_dir = path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let pairs = if ext == "env" {
        parse_env_file(&path)?
    } else {
        parse_toml_file(&path, &base_dir)?
    };
    let (applied, skipped_env) = apply_config_pairs(&pairs);
    if ext == "toml" {
        if let Ok(text) = std::fs::read_to_string(&path) {
            if let Ok(parsed) = toml::from_str::<ServerConfigToml>(&text) {
                let _ = PARSED_MCP.set(Some((parsed.mcp_servers, base_dir.clone())));
            }
        }
    }
    crate::deployment_mode::init_from_env();
    eprintln!("pointer-server: config file {}", path.display());
    if applied.is_empty() && skipped_env.is_empty() {
        eprintln!("pointer-server: config file has no recognized keys");
    }
    for (key, value) in &applied {
        if key == "POINTER_OAUTH_CLIENT_SECRET"
            || key == "POINTER_WEBHOOK_BEARER_TOKEN"
            || key == "POINTER_SERVER_ADMIN_TOKEN"
            || key == "POINTER_SERVER_ADMIN_PASSWORD_HMAC"
            || key == "POINTER_SERVER_AUTH_HMAC_SECRET"
            || key == "POINTER_LICENSE_KEY"
        {
            eprintln!("pointer-server: applied {key}=<redacted>");
        } else {
            eprintln!("pointer-server: applied {key}={value}");
        }
    }
    for key in &skipped_env {
        let current = std::env::var(key).unwrap_or_default();
        if key == "POINTER_OAUTH_CLIENT_SECRET"
            || key == "POINTER_WEBHOOK_BEARER_TOKEN"
            || key == "POINTER_SERVER_ADMIN_TOKEN"
            || key == "POINTER_SERVER_ADMIN_PASSWORD_HMAC"
            || key == "POINTER_SERVER_AUTH_HMAC_SECRET"
            || key == "POINTER_LICENSE_KEY"
        {
            eprintln!("pointer-server: skipped {key} (environment already set, value redacted)");
        } else {
            eprintln!("pointer-server: skipped {key} (environment already set to {current})");
        }
    }
    let effective_addr =
        std::env::var("POINTER_SERVER_ADDR").unwrap_or_else(|_| "127.0.0.1:8787".into());
    eprintln!("pointer-server: effective POINTER_SERVER_ADDR={effective_addr}");
    Ok(Some(ServerConfigLoadResult {
        path,
        applied,
        skipped_env,
    }))
}

fn resolve_config_path() -> Result<Option<PathBuf>> {
    if let Ok(raw) = std::env::var("POINTER_SERVER_CONFIG") {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            anyhow::bail!("POINTER_SERVER_CONFIG is empty");
        }
        let path = PathBuf::from(trimmed);
        if !path.is_file() {
            anyhow::bail!(
                "POINTER_SERVER_CONFIG={} is not a readable file",
                path.display()
            );
        }
        return Ok(Some(path));
    }

    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("pointer-server.toml"));
            candidates.push(dir.join("pointer-server.env"));
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("pointer-server.toml"));
        candidates.push(cwd.join("pointer-server.env"));
    }

    for path in candidates {
        if path.is_file() {
            return Ok(Some(path));
        }
    }
    Ok(None)
}

fn parse_env_file(path: &Path) -> Result<Vec<(String, String)>> {
    let bytes =
        std::fs::read(path).with_context(|| format!("read config file {}", path.display()))?;
    Ok(parse_dotenv_bytes(&bytes).into_iter().collect())
}

fn parse_toml_file(path: &Path, base_dir: &Path) -> Result<Vec<(String, String)>> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("read config file {}", path.display()))?;
    let parsed: ServerConfigToml =
        toml::from_str(&text).with_context(|| format!("parse TOML {}", path.display()))?;
    let mut pairs = Vec::new();

    push_mapped(
        &mut pairs,
        "POINTER_DEPLOYMENT_MODE",
        &parsed.deployment.mode,
        base_dir,
        false,
    );
    push_mapped(
        &mut pairs,
        "POINTER_SERVER_ADMIN_USERNAME",
        &parsed.auth.local.username,
        base_dir,
        false,
    );
    push_mapped(
        &mut pairs,
        "POINTER_SERVER_ADMIN_PASSWORD_HMAC",
        &parsed.auth.local.password_hmac,
        base_dir,
        false,
    );
    push_mapped(
        &mut pairs,
        "POINTER_SERVER_AUTH_HMAC_SECRET",
        &parsed.auth.local.hmac_secret,
        base_dir,
        false,
    );
    // Deprecated: still map so warn_if_deprecated_admin_token_configured can see it.
    push_mapped(
        &mut pairs,
        "POINTER_SERVER_ADMIN_TOKEN",
        &parsed.auth.local.admin_token,
        base_dir,
        false,
    );
    if let Some(enabled) = parsed.auth.local.sso.enabled {
        pairs.push((
            "POINTER_SERVER_SSO_ENABLED".to_string(),
            if enabled { "true" } else { "false" }.to_string(),
        ));
    }
    push_mapped(
        &mut pairs,
        "POINTER_SERVER_SSO_SECRET",
        &parsed.auth.local.sso.secret,
        base_dir,
        false,
    );
    push_mapped(
        &mut pairs,
        "POINTER_SERVER_SSO_SECRET_PREV",
        &parsed.auth.local.sso.secret_prev,
        base_dir,
        false,
    );
    push_mapped(
        &mut pairs,
        "POINTER_SERVER_SSO_AUDIENCE",
        &parsed.auth.local.sso.audience,
        base_dir,
        false,
    );
    if let Some(skew) = parsed.auth.local.sso.max_skew_secs {
        pairs.push((
            "POINTER_SERVER_SSO_MAX_SKEW_SECS".to_string(),
            skew.to_string(),
        ));
    }
    if let Some(key) = resolve_license_key(&parsed.license, base_dir) {
        pairs.push(("POINTER_LICENSE_KEY".to_string(), key));
    }
    if let Some(enabled) = parsed.usage.report_enabled {
        pairs.push((
            "POINTER_USAGE_REPORT_ENABLED".to_string(),
            if enabled { "true" } else { "false" }.to_string(),
        ));
    }

    push_mapped(
        &mut pairs,
        "POINTER_SERVER_ADDR",
        &parsed.server.addr,
        base_dir,
        false,
    );
    push_mapped(
        &mut pairs,
        "POINTER_SERVER_STATIC_DIR",
        &parsed.server.static_dir,
        base_dir,
        true,
    );
    push_mapped(
        &mut pairs,
        "POINTER_SERVER_SKILLS_DIR",
        &parsed.server.skills_dir,
        base_dir,
        true,
    );
    push_mapped(
        &mut pairs,
        "POINTER_APP_DATA_DIR",
        &parsed.server.app_data_dir,
        base_dir,
        true,
    );
    push_mapped(
        &mut pairs,
        "POINTER_SERVER_PUBLIC_URL",
        &parsed.server.public_url,
        base_dir,
        false,
    );
    push_mapped(
        &mut pairs,
        "POINTER_SERVER_PAGE_TITLE",
        &parsed.server.page_title,
        base_dir,
        false,
    );
    push_mapped(
        &mut pairs,
        "POINTER_SERVER_COMPOSER_PLACEHOLDER",
        &parsed.server.composer_placeholder,
        base_dir,
        false,
    );
    if !parsed.server.allowed_user_ids.is_empty() {
        let joined = parsed
            .server
            .allowed_user_ids
            .iter()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(",");
        if !joined.is_empty() {
            pairs.push(("POINTER_SERVER_ALLOWED_USER_IDS".to_string(), joined));
        }
    }
    if parsed.server.require_allowed_users {
        pairs.push((
            "POINTER_SERVER_REQUIRE_ALLOWED_USERS".to_string(),
            "true".to_string(),
        ));
    }
    if let Some(enabled) = parsed.server.sse_padding_enabled {
        pairs.push((
            "POINTER_SERVER_SSE_PADDING_ENABLED".to_string(),
            if enabled { "true" } else { "false" }.to_string(),
        ));
    }
    if let Some(bytes) = parsed.server.sse_padding_bytes {
        pairs.push((
            "POINTER_SERVER_SSE_PADDING_BYTES".to_string(),
            bytes.to_string(),
        ));
    }
    if let Some(enabled) = parsed.server.forbid_session_user_id_in_terminal {
        pairs.push((
            "POINTER_SERVER_FORBID_SESSION_USER_ID_IN_TERMINAL".to_string(),
            if enabled { "true" } else { "false" }.to_string(),
        ));
    }
    push_mapped(
        &mut pairs,
        "POINTER_API_BASE",
        &parsed.pointer.api_base,
        base_dir,
        false,
    );
    push_mapped(
        &mut pairs,
        "POINTER_OAUTH_CLIENT_SECRET",
        &parsed.pointer.oauth_client_secret,
        base_dir,
        false,
    );
    push_mapped(
        &mut pairs,
        "POINTER_WEBHOOK_BEARER_TOKEN",
        &parsed.webhooks.bearer_token,
        base_dir,
        false,
    );

    for (key, value) in parsed.env {
        let key = key.trim();
        if key.is_empty() {
            continue;
        }
        let resolved = resolve_config_path_value(&value, base_dir, is_path_like_env_key(key));
        pairs.push((key.to_string(), resolved));
    }

    Ok(pairs)
}

fn resolve_license_key(license: &LicenseSection, base_dir: &Path) -> Option<String> {
    let inline = license.key.trim();
    if !inline.is_empty() {
        return Some(inline.to_string());
    }
    let file = license.license_file.trim();
    if file.is_empty() {
        return None;
    }
    let path = if Path::new(file).is_absolute() {
        PathBuf::from(file)
    } else {
        base_dir.join(file)
    };
    match std::fs::read_to_string(&path) {
        Ok(text) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                log::warn!("server_config: license file {} is empty", path.display());
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        Err(e) => {
            log::warn!(
                "server_config: failed to read license file {}: {e}",
                path.display()
            );
            None
        }
    }
}

/// P2b：读取已解析的全局 MCP server 声明 + 配置文件目录（相对 command 解析基准）。
/// 仅当 `load_server_config` 已执行（server 启动路径）时返回 Some。
pub fn mcp_servers_from_config() -> Option<(Vec<crate::plugins::manifest::McpServerDecl>, PathBuf)>
{
    PARSED_MCP
        .get()
        .and_then(|v| v.as_ref())
        .map(|(decls, base)| (decls.server.clone(), base.clone()))
}

/// P2b：重新解析配置文件中的全局 MCP 段（热重载 / 桌面端未走 `load_server_config`
/// 时兜底）。只读文件并更新缓存，不应用 env、不改部署模式（无副作用）。
pub fn reload_mcp_servers_config(
) -> Result<Option<(Vec<crate::plugins::manifest::McpServerDecl>, PathBuf)>> {
    let Some(path) = resolve_config_path()? else {
        return Ok(None);
    };
    let base_dir = path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ext != "toml" {
        return Ok(None); // .env 不支持 mcp_servers 段
    }
    let text = std::fs::read_to_string(&path)?;
    let parsed: ServerConfigToml = toml::from_str(&text)?;
    let server_decls = parsed.mcp_servers.server.clone();
    let _ = PARSED_MCP.set(Some((parsed.mcp_servers, base_dir.clone())));
    Ok(Some((server_decls, base_dir)))
}

/// Whether SSE initial padding is enabled (flush proxy buffers).
/// Reads `POINTER_SERVER_SSE_PADDING_ENABLED`; defaults to `false`.
pub fn sse_padding_enabled() -> bool {
    std::env::var("POINTER_SERVER_SSE_PADDING_ENABLED")
        .map(|v| v != "0" && v.to_ascii_lowercase() != "false")
        .unwrap_or(false)
}

/// SSE initial padding size in bytes.
/// Reads `POINTER_SERVER_SSE_PADDING_BYTES`; defaults to `10_240`.
pub fn sse_padding_bytes() -> usize {
    std::env::var("POINTER_SERVER_SSE_PADDING_BYTES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10_240)
}

/// Whether Agent `terminal` rejects `command` / `stdin` containing `SESSION_USER_ID`.
/// Server-only (`pointer-server.toml` / env). Defaults to `false` (desktop never sets this).
pub fn forbid_session_user_id_in_terminal() -> bool {
    std::env::var("POINTER_SERVER_FORBID_SESSION_USER_ID_IN_TERMINAL")
        .map(|v| v != "0" && v.to_ascii_lowercase() != "false")
        .unwrap_or(false)
}

fn push_mapped(
    pairs: &mut Vec<(String, String)>,
    env_key: &str,
    value: &str,
    base_dir: &Path,
    resolve_relative: bool,
) {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return;
    }
    let resolved = resolve_config_path_value(trimmed, base_dir, resolve_relative);
    pairs.push((env_key.to_string(), resolved));
}

fn is_path_like_env_key(key: &str) -> bool {
    let upper = key.trim().to_ascii_uppercase();
    upper.ends_with("_DIR")
        || upper.ends_with("_PATH")
        || upper == "PATH"
        || upper.contains("STATIC")
}

fn resolve_config_path_value(value: &str, base_dir: &Path, resolve_relative: bool) -> String {
    if !resolve_relative {
        return value.to_string();
    }
    let path = Path::new(value);
    if path.is_absolute() {
        return value.to_string();
    }
    base_dir.join(path).to_string_lossy().into_owned()
}

fn apply_config_pairs(pairs: &[(String, String)]) -> (Vec<(String, String)>, Vec<String>) {
    let mut applied = Vec::new();
    let mut skipped_env = Vec::new();
    for (key, value) in pairs {
        if std::env::var(key).is_ok() {
            skipped_env.push(key.clone());
            continue;
        }
        if value.is_empty() {
            continue;
        }
        // SAFETY: called once at process startup before other threads read these vars.
        unsafe { std::env::set_var(key, value) };
        applied.push((key.clone(), value.clone()));
    }
    (applied, skipped_env)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn env_guard() -> MutexGuard<'static, ()> {
        ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn toml_maps_allowed_user_ids_to_env() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("pointer-server.toml");
        std::fs::write(
            &cfg,
            r#"
[server]
allowed_user_ids = ["user-a", "user-b"]
require_allowed_users = true
"#,
        )
        .unwrap();
        let pairs = parse_toml_file(&cfg, dir.path()).unwrap();
        let map: HashMap<_, _> = pairs.into_iter().collect();
        assert_eq!(
            map.get("POINTER_SERVER_ALLOWED_USER_IDS")
                .map(String::as_str),
            Some("user-a,user-b")
        );
        assert_eq!(
            map.get("POINTER_SERVER_REQUIRE_ALLOWED_USERS")
                .map(String::as_str),
            Some("true")
        );
    }

    #[test]
    fn toml_maps_public_url_to_env() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("pointer-server.toml");
        std::fs::write(
            &cfg,
            r#"
[server]
public_url = "https://pointer.example.com"
"#,
        )
        .unwrap();
        let pairs = parse_toml_file(&cfg, dir.path()).unwrap();
        let map: HashMap<_, _> = pairs.into_iter().collect();
        assert_eq!(
            map.get("POINTER_SERVER_PUBLIC_URL").map(String::as_str),
            Some("https://pointer.example.com")
        );
    }

    #[test]
    fn toml_env_section_maps_otel_keys() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("pointer-server.toml");
        std::fs::write(
            &cfg,
            r#"
[env]
OTEL_EXPORTER_OTLP_ENDPOINT = "http://localhost:4318"
OTEL_SERVICE_NAME = "pointer-server"
"#,
        )
        .unwrap();
        let pairs = parse_toml_file(&cfg, dir.path()).unwrap();
        let map: HashMap<_, _> = pairs.into_iter().collect();
        assert_eq!(
            map.get("OTEL_EXPORTER_OTLP_ENDPOINT").map(String::as_str),
            Some("http://localhost:4318")
        );
        assert_eq!(
            map.get("OTEL_SERVICE_NAME").map(String::as_str),
            Some("pointer-server")
        );
    }

    #[test]
    fn parses_global_mcp_servers_section() {
        // P2b：`[[mcp_servers.server]]` 段结构复用插件 manifest 的 McpServerDecl。
        let parsed: ServerConfigToml = toml::from_str(
            r#"
[mcp_servers]
[[mcp_servers.server]]
name = "demo"
transport = "stdio"
command = "bin/demo-mcp"
args = ["serve"]
env = { TOKEN = "x" }
"#,
        )
        .unwrap();
        assert_eq!(parsed.mcp_servers.server.len(), 1);
        let decl = &parsed.mcp_servers.server[0];
        assert_eq!(decl.name, "demo");
        assert_eq!(decl.transport, "stdio");
        assert_eq!(decl.command, "bin/demo-mcp");
        assert_eq!(decl.args, vec!["serve".to_string()]);
        assert_eq!(decl.env.get("TOKEN").map(|s| s.as_str()), Some("x"));

        // 缺省 transport 默认 stdio；未声明段时为空
        let parsed2: ServerConfigToml = toml::from_str(
            r#"
[mcp_servers]
[[mcp_servers.server]]
name = "min"
command = "bin/min"
"#,
        )
        .unwrap();
        assert_eq!(parsed2.mcp_servers.server[0].transport, "stdio");
        let parsed3: ServerConfigToml = toml::from_str(r#"[server]"#).unwrap();
        assert!(parsed3.mcp_servers.server.is_empty());
    }

    #[test]
    fn toml_maps_page_title_to_env() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("pointer-server.toml");
        std::fs::write(
            &cfg,
            r#"
[server]
page_title = "Acme · AI 助手"
composer_placeholder = "有什么可以帮你？"
"#,
        )
        .unwrap();
        let pairs = parse_toml_file(&cfg, dir.path()).unwrap();
        let map: HashMap<_, _> = pairs.into_iter().collect();
        assert_eq!(
            map.get("POINTER_SERVER_PAGE_TITLE").map(String::as_str),
            Some("Acme · AI 助手")
        );
        assert_eq!(
            map.get("POINTER_SERVER_COMPOSER_PLACEHOLDER")
                .map(String::as_str),
            Some("有什么可以帮你？")
        );
    }

    #[test]
    fn toml_maps_forbid_session_user_id_in_terminal() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("pointer-server.toml");
        std::fs::write(
            &cfg,
            r#"
[server]
forbid_session_user_id_in_terminal = true
"#,
        )
        .unwrap();
        let pairs = parse_toml_file(&cfg, dir.path()).unwrap();
        let map: HashMap<_, _> = pairs.into_iter().collect();
        assert_eq!(
            map.get("POINTER_SERVER_FORBID_SESSION_USER_ID_IN_TERMINAL")
                .map(String::as_str),
            Some("true")
        );
    }

    #[test]
    fn forbid_session_user_id_in_terminal_defaults_off() {
        let _guard = env_guard();
        std::env::remove_var("POINTER_SERVER_FORBID_SESSION_USER_ID_IN_TERMINAL");
        assert!(!forbid_session_user_id_in_terminal());
        std::env::set_var("POINTER_SERVER_FORBID_SESSION_USER_ID_IN_TERMINAL", "true");
        assert!(forbid_session_user_id_in_terminal());
        std::env::set_var("POINTER_SERVER_FORBID_SESSION_USER_ID_IN_TERMINAL", "false");
        assert!(!forbid_session_user_id_in_terminal());
        std::env::remove_var("POINTER_SERVER_FORBID_SESSION_USER_ID_IN_TERMINAL");
    }

    #[test]
    fn toml_maps_sections_to_env_keys() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("pointer-server.toml");
        std::fs::write(
            &cfg,
            r#"
[server]
addr = "0.0.0.0:9999"
static_dir = "dist"
skills_dir = "skills"

[pointer]
api_base = "https://api.example.com"
oauth_client_secret = "secret"

[env]
POINTER_WEB_SEARCH_MODEL = "gpt-4o-mini"
"#,
        )
        .unwrap();
        let pairs = parse_toml_file(&cfg, dir.path()).unwrap();
        let map: HashMap<_, _> = pairs.into_iter().collect();
        assert_eq!(
            map.get("POINTER_SERVER_ADDR").map(String::as_str),
            Some("0.0.0.0:9999")
        );
        assert_eq!(
            map.get("POINTER_SERVER_STATIC_DIR").map(String::as_str),
            Some(dir.path().join("dist").to_str().unwrap())
        );
        assert_eq!(
            map.get("POINTER_SERVER_SKILLS_DIR").map(String::as_str),
            Some(dir.path().join("skills").to_str().unwrap())
        );
        assert_eq!(
            map.get("POINTER_API_BASE").map(String::as_str),
            Some("https://api.example.com")
        );
        assert_eq!(
            map.get("POINTER_WEB_SEARCH_MODEL").map(String::as_str),
            Some("gpt-4o-mini")
        );
    }

    #[test]
    fn toml_accepts_legacy_openpointer_section() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("pointer-server.toml");
        std::fs::write(
            &cfg,
            r#"
[openpointer]
api_base = "https://legacy.example.com"
"#,
        )
        .unwrap();
        let pairs = parse_toml_file(&cfg, dir.path()).unwrap();
        let map: HashMap<_, _> = pairs.into_iter().collect();
        assert_eq!(
            map.get("POINTER_API_BASE").map(String::as_str),
            Some("https://legacy.example.com")
        );
    }

    #[test]
    fn apply_skips_existing_env() {
        let _guard = env_guard();
        let key = "POINTER_SERVER_CONFIG_TEST_ONLY";
        std::env::set_var(key, "from_env");
        let (applied, skipped) = apply_config_pairs(&[(key.to_string(), "from_file".to_string())]);
        assert!(applied.is_empty());
        assert_eq!(skipped, vec![key.to_string()]);
        assert_eq!(std::env::var(key).unwrap(), "from_env");
        std::env::remove_var(key);
    }

    #[test]
    fn apply_sets_missing_env() {
        let _guard = env_guard();
        let key = "POINTER_SERVER_CONFIG_TEST_ONLY";
        std::env::remove_var(key);
        let (applied, skipped) = apply_config_pairs(&[(key.to_string(), "from_file".to_string())]);
        assert_eq!(applied.len(), 1);
        assert!(skipped.is_empty());
        assert_eq!(std::env::var(key).unwrap(), "from_file");
        std::env::remove_var(key);
    }

    #[test]
    fn toml_ignores_legacy_llm_section() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("pointer-server.toml");
        std::fs::write(
            &cfg,
            r#"
[deployment]
mode = "standalone"

[llm]
active_provider = "qwen"

[llm.providers.qwen]
api_key = "sk-should-not-apply"
base_url = "https://example.invalid/v1"
name = "ignored"
models = ["ignored-model"]
"#,
        )
        .unwrap();
        let pairs = parse_toml_file(&cfg, dir.path()).unwrap();
        let map: HashMap<_, _> = pairs.into_iter().collect();
        assert_eq!(
            map.get("POINTER_DEPLOYMENT_MODE").map(String::as_str),
            Some("standalone")
        );
        assert!(!map.contains_key("POINTER_LLM_ACTIVE_PROVIDER"));
        assert!(!map.values().any(|v| v.contains("sk-should-not-apply")));
    }
}
