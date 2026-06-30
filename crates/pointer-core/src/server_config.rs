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

#[derive(Debug, Default, Deserialize)]
struct ServerSection {
    #[serde(default)]
    addr: String,
    #[serde(default)]
    static_dir: String,
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
struct OpenpointerSection {
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
    server: ServerSection,
    #[serde(default)]
    openpointer: OpenpointerSection,
    #[serde(default)]
    webhooks: WebhooksSection,
    #[serde(default)]
    env: HashMap<String, String>,
}

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
    eprintln!("pointer-server: config file {}", path.display());
    if applied.is_empty() && skipped_env.is_empty() {
        eprintln!("pointer-server: config file has no recognized keys");
    }
    for (key, value) in &applied {
        if key == "OPENPOINTER_OAUTH_CLIENT_SECRET" || key == "POINTER_WEBHOOK_BEARER_TOKEN" {
            eprintln!("pointer-server: applied {key}=<redacted>");
        } else {
            eprintln!("pointer-server: applied {key}={value}");
        }
    }
    for key in &skipped_env {
        let current = std::env::var(key).unwrap_or_default();
        if key == "OPENPOINTER_OAUTH_CLIENT_SECRET" || key == "POINTER_WEBHOOK_BEARER_TOKEN" {
            eprintln!("pointer-server: skipped {key} (environment already set, value redacted)");
        } else {
            eprintln!("pointer-server: skipped {key} (environment already set to {current})");
        }
    }
    let effective_addr = std::env::var("POINTER_SERVER_ADDR").unwrap_or_else(|_| "127.0.0.1:8787".into());
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
    let bytes = std::fs::read(path)
        .with_context(|| format!("read config file {}", path.display()))?;
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
        "OPENPOINTER_API_BASE",
        &parsed.openpointer.api_base,
        base_dir,
        false,
    );
    push_mapped(
        &mut pairs,
        "OPENPOINTER_OAUTH_CLIENT_SECRET",
        &parsed.openpointer.oauth_client_secret,
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
            map.get("POINTER_SERVER_ALLOWED_USER_IDS").map(String::as_str),
            Some("user-a,user-b")
        );
        assert_eq!(
            map.get("POINTER_SERVER_REQUIRE_ALLOWED_USERS").map(String::as_str),
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

[openpointer]
api_base = "https://api.example.com"
oauth_client_secret = "secret"

[env]
POINTER_WEB_SEARCH_MODEL = "gpt-4o-mini"
"#,
        )
        .unwrap();
        let pairs = parse_toml_file(&cfg, dir.path()).unwrap();
        let map: HashMap<_, _> = pairs.into_iter().collect();
        assert_eq!(map.get("POINTER_SERVER_ADDR").map(String::as_str), Some("0.0.0.0:9999"));
        assert_eq!(
            map.get("POINTER_SERVER_STATIC_DIR").map(String::as_str),
            Some(dir.path().join("dist").to_str().unwrap())
        );
        assert_eq!(
            map.get("OPENPOINTER_API_BASE").map(String::as_str),
            Some("https://api.example.com")
        );
        assert_eq!(map.get("POINTER_WEB_SEARCH_MODEL").map(String::as_str), Some("gpt-4o-mini"));
    }

    #[test]
    fn apply_skips_existing_env() {
        let _guard = env_guard();
        let key = "POINTER_SERVER_CONFIG_TEST_ONLY";
        std::env::set_var(key, "from_env");
        let (applied, skipped) =
            apply_config_pairs(&[(key.to_string(), "from_file".to_string())]);
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
        let (applied, skipped) =
            apply_config_pairs(&[(key.to_string(), "from_file".to_string())]);
        assert_eq!(applied.len(), 1);
        assert!(skipped.is_empty());
        assert_eq!(std::env::var(key).unwrap(), "from_file");
        std::env::remove_var(key);
    }
}
