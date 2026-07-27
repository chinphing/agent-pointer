//! pointer-server runtime configuration loaded from a file before other startup logic.
//!
//! Search order (first existing file wins):
//! 1. `POINTER_SERVER_CONFIG` — explicit path
//! 2. `{exe_dir}/pointer-server.toml` or `{exe_dir}/pointer-server.env`
//! 3. `{cwd}/pointer-server.toml` or `{cwd}/pointer-server.env`
//!
//! Existing OS environment variables always override file values.

use crate::dotenv::parse_dotenv_bytes;
use crate::models::{PlatformSettings, ProviderConfig};
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
struct LlmProviderToml {
    #[serde(default)]
    api_key: String,
    #[serde(default)]
    base_url: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    models: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
struct LlmSection {
    #[serde(default)]
    active_provider: String,
    #[serde(default)]
    providers: HashMap<String, LlmProviderToml>,
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
    /// Platform user ids allowed to log in (maps to `POINTER_SERVER_ALLOWED_USER_IDS`).
    #[serde(default)]
    allowed_user_ids: Vec<String>,
    /// When true, startup fails if `allowed_user_ids` is empty
    /// (`POINTER_SERVER_REQUIRE_ALLOWED_USERS`).
    #[serde(default)]
    require_allowed_users: bool,
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
    llm: LlmSection,
    #[serde(default)]
    license: LicenseSection,
    #[serde(default)]
    usage: UsageSection,
    #[serde(default, alias = "openpointer")]
    pointer: PointerSection,
    #[serde(default)]
    webhooks: WebhooksSection,
    #[serde(default)]
    env: HashMap<String, String>,
}

#[derive(Debug, Default, Deserialize)]
struct AuthToml {
    #[serde(default)]
    local: AuthLocalSection,
}

static PARSED_LLM: OnceLock<Option<LlmSection>> = OnceLock::new();

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
                let _ = PARSED_LLM.set(Some(parsed.llm));
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
    if !parsed.llm.active_provider.trim().is_empty() {
        pairs.push((
            "POINTER_LLM_ACTIVE_PROVIDER".to_string(),
            parsed.llm.active_provider.trim().to_string(),
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

/// If `platform.model` is missing from the active provider's `models` list,
/// switch to the first configured model (standalone TOML often replaces the
/// built-in catalog with a single local/custom id).
fn sync_active_model_to_provider_list(platform: &mut PlatformSettings) {
    let pid = platform.active_provider_id.trim().to_string();
    if pid.is_empty() {
        return;
    }
    let Some(provider) = platform.providers.iter().find(|p| p.id == pid) else {
        return;
    };
    let first = provider
        .models
        .iter()
        .map(|m| m.trim())
        .find(|m| !m.is_empty())
        .map(str::to_string);
    let Some(first) = first else {
        return;
    };
    let current = platform.model.trim();
    if provider.models.iter().any(|m| m.trim() == current) {
        return;
    }
    log::info!(
        "server_config: active model '{current}' not in provider {pid} models; using '{first}'"
    );
    platform.model = first;
}

fn platform_provider_model_usable(
    platform: &PlatformSettings,
    provider_id: &str,
    model: &str,
) -> bool {
    let pid = provider_id.trim();
    let model = model.trim();
    if pid.is_empty() || model.is_empty() {
        return false;
    }
    let Some(provider) = platform.providers.iter().find(|p| p.id == pid) else {
        return false;
    };
    if provider.api_key.trim().is_empty() {
        return false;
    }
    if provider.models.is_empty() {
        return true;
    }
    provider.models.iter().any(|m| m.trim() == model)
}

/// Rewrite `agentModeLlm` / `mediaModeLlm` rows that point at missing keys or
/// catalog models so chat uses the standalone-configured active model.
fn sync_mode_llm_maps_to_active(platform: &mut PlatformSettings) {
    let active_pid = platform.active_provider_id.trim().to_string();
    let active_model = platform.model.trim().to_string();
    if active_pid.is_empty() || active_model.is_empty() {
        return;
    }
    if !platform_provider_model_usable(platform, &active_pid, &active_model) {
        return;
    }

    let mut agent_rewrites: Vec<(String, String)> = Vec::new();
    for (outer, modes) in &platform.agent_mode_llm {
        for (mode, cfg) in modes {
            if !platform_provider_model_usable(platform, &cfg.provider_id, &cfg.model) {
                agent_rewrites.push((outer.clone(), mode.clone()));
            }
        }
    }
    for (outer, mode) in agent_rewrites {
        if let Some(cfg) = platform
            .agent_mode_llm
            .get_mut(&outer)
            .and_then(|m| m.get_mut(&mode))
        {
            log::info!(
                "server_config: agentModeLlm {outer}/{mode} provider={} model={} unusable; \
                 rewriting to active {active_pid}/{active_model}",
                cfg.provider_id.trim(),
                cfg.model.trim()
            );
            cfg.provider_id = active_pid.clone();
            cfg.model = active_model.clone();
        }
    }

    let mut media_rewrites: Vec<(String, String)> = Vec::new();
    for (outer, modes) in &platform.media_mode_llm {
        for (mode, cfg) in modes {
            if !platform_provider_model_usable(platform, &cfg.provider_id, &cfg.model) {
                media_rewrites.push((outer.clone(), mode.clone()));
            }
        }
    }
    for (outer, mode) in media_rewrites {
        if let Some(cfg) = platform
            .media_mode_llm
            .get_mut(&outer)
            .and_then(|m| m.get_mut(&mode))
        {
            log::info!(
                "server_config: mediaModeLlm {outer}/{mode} provider={} model={} unusable; \
                 rewriting to active {active_pid}/{active_model}",
                cfg.provider_id.trim(),
                cfg.model.trim()
            );
            cfg.provider_id = active_pid.clone();
            cfg.model = active_model.clone();
        }
    }
}

/// Apply `[llm]` provider keys from pointer-server.toml into in-memory platform settings.
pub fn apply_llm_providers_from_config(platform: &mut PlatformSettings) {
    let Some(llm) = PARSED_LLM.get().and_then(|o| o.as_ref()) else {
        return;
    };
    apply_llm_section(platform, llm);
}

fn apply_llm_section(platform: &mut PlatformSettings, llm: &LlmSection) {
    if llm.providers.is_empty() {
        return;
    }
    let active = std::env::var("POINTER_LLM_ACTIVE_PROVIDER")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| llm.active_provider.trim().to_string());
    if !active.is_empty() {
        platform.active_provider_id = active;
    }
    for (provider_id, cfg) in &llm.providers {
        let pid = provider_id.trim();
        if pid.is_empty() {
            continue;
        }
        let api_key = cfg.api_key.trim();
        if api_key.is_empty() {
            continue;
        }
        if let Some(existing) = platform.providers.iter_mut().find(|p| p.id == pid) {
            existing.api_key = api_key.to_string();
            if !cfg.base_url.trim().is_empty() {
                existing.base_url = cfg.base_url.trim().to_string();
            }
            if !cfg.name.trim().is_empty() {
                existing.name = cfg.name.trim().to_string();
            }
            if !cfg.models.is_empty() {
                existing.models = cfg.models.clone();
            }
            log::info!("server_config: injected llm api_key for provider {pid}");
        } else {
            platform.providers.push(ProviderConfig {
                id: pid.to_string(),
                name: if cfg.name.trim().is_empty() {
                    pid.to_string()
                } else {
                    cfg.name.trim().to_string()
                },
                base_url: cfg.base_url.trim().to_string(),
                api_key: api_key.to_string(),
                models: cfg.models.clone(),
                reasoning_in_messages: None,
                temperature: None,
                max_tokens: None,
                model_configs: HashMap::new(),
                enable_thinking: None,
                thinking_budget: None,
                reasoning_effort: None,
            });
            log::info!("server_config: added llm provider {pid} from config");
        }
    }
    sync_active_model_to_provider_list(platform);
    sync_mode_llm_maps_to_active(platform);
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
    fn apply_llm_section_syncs_active_model_to_configured_list() {
        let _guard = env_guard();
        std::env::remove_var("POINTER_LLM_ACTIVE_PROVIDER");

        let mut platform = PlatformSettings::default();
        platform.model = "qwen3.5-plus".into();
        platform.active_provider_id = "qwen".into();

        let mut llm = LlmSection::default();
        llm.active_provider = "xiaohe".into();
        llm.providers.insert(
            "xiaohe".into(),
            LlmProviderToml {
                api_key: "sk-local".into(),
                base_url: "http://127.0.0.1:8000/v1".into(),
                name: "xiaohe".into(),
                models: vec!["qwen3.6-27b".into()],
            },
        );

        apply_llm_section(&mut platform, &llm);

        assert_eq!(platform.active_provider_id, "xiaohe");
        assert_eq!(platform.model, "qwen3.6-27b");
        let p = platform
            .providers
            .iter()
            .find(|p| p.id == "xiaohe")
            .expect("xiaohe provider");
        assert_eq!(p.api_key, "sk-local");
        assert_eq!(p.models, vec!["qwen3.6-27b".to_string()]);
    }

    #[test]
    fn apply_llm_section_keeps_active_model_when_still_listed() {
        let _guard = env_guard();
        std::env::remove_var("POINTER_LLM_ACTIVE_PROVIDER");

        let mut platform = PlatformSettings::default();
        platform.model = "qwen3.5-turbo".into();
        platform.active_provider_id = "qwen".into();

        let mut llm = LlmSection::default();
        llm.active_provider = "qwen".into();
        llm.providers.insert(
            "qwen".into(),
            LlmProviderToml {
                api_key: "sk-qwen".into(),
                base_url: String::new(),
                name: String::new(),
                models: vec!["qwen3.5-plus".into(), "qwen3.5-turbo".into()],
            },
        );

        apply_llm_section(&mut platform, &llm);

        assert_eq!(platform.active_provider_id, "qwen");
        assert_eq!(platform.model, "qwen3.5-turbo");
    }
}
