//! Encrypted secret store for plugin / MCP credentials.
//!
//! Secrets are keyed by `(user_key, scope_id, secret_name)` and encrypted with
//! the existing machine-bound local_secret helpers. Values never belong in
//! plugin code trees, skill directories, or model context.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::local_secret::{decrypt_local_secret_with_info, encrypt_local_secret_with_info};
use crate::user_key::user_key;

const SECRET_INFO: &[u8] = b"pointer-plugin-secret-store-v1";
const STORE_FILE: &str = "plugin-secrets.enc.json";

#[derive(Debug, Default, Serialize, Deserialize)]
struct SecretFile {
    /// Map of logical key → base64 ciphertext blob (version+nonce+ct).
    #[serde(default)]
    entries: HashMap<String, String>,
}

fn store_path(session_user_id: &str) -> Result<PathBuf> {
    let home = crate::user_data_paths::user_home_host_root(session_user_id)?;
    let dir = home.join(".pointer");
    fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    Ok(dir.join(STORE_FILE))
}

fn entry_key(session_user_id: &str, scope_id: &str, secret_name: &str) -> String {
    format!(
        "{}::{}::{}",
        user_key(session_user_id),
        scope_id.trim(),
        secret_name.trim()
    )
}

fn load_file(path: &PathBuf) -> SecretFile {
    fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save_file(path: &PathBuf, file: &SecretFile) -> Result<()> {
    let json = serde_json::to_string_pretty(file).context("serialize secret store")?;
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, json).with_context(|| format!("write {}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("rename into {}", path.display()))?;
    Ok(())
}

/// Persist a secret for `scope_id` (plugin_id or mcp server id).
pub fn put_secret(
    session_user_id: &str,
    scope_id: &str,
    secret_name: &str,
    plaintext: &str,
) -> Result<()> {
    if scope_id.trim().is_empty() || secret_name.trim().is_empty() {
        anyhow::bail!("secret scope_id and name are required");
    }
    let path = store_path(session_user_id)?;
    let mut file = load_file(&path);
    let blob = encrypt_local_secret_with_info(plaintext, SECRET_INFO)?;
    let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &blob);
    let key = entry_key(session_user_id, scope_id, secret_name);
    file.entries.insert(key, b64);
    save_file(&path, &file)?;
    log::info!(
        "secret_store: put user_key={} scope={} name={}",
        user_key(session_user_id),
        scope_id.trim(),
        secret_name.trim()
    );
    Ok(())
}

/// Read a secret; returns `None` when missing.
pub fn get_secret(
    session_user_id: &str,
    scope_id: &str,
    secret_name: &str,
) -> Result<Option<String>> {
    let path = store_path(session_user_id)?;
    let file = load_file(&path);
    let key = entry_key(session_user_id, scope_id, secret_name);
    let Some(b64) = file.entries.get(&key) else {
        return Ok(None);
    };
    let blob = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, b64)
        .context("decode secret blob")?;
    let plain = decrypt_local_secret_with_info(&blob, SECRET_INFO)?;
    Ok(Some(plain))
}

/// Delete one secret entry.
pub fn delete_secret(session_user_id: &str, scope_id: &str, secret_name: &str) -> Result<bool> {
    let path = store_path(session_user_id)?;
    let mut file = load_file(&path);
    let key = entry_key(session_user_id, scope_id, secret_name);
    let removed = file.entries.remove(&key).is_some();
    if removed {
        save_file(&path, &file)?;
        log::info!(
            "secret_store: deleted user_key={} scope={} name={}",
            user_key(session_user_id),
            scope_id.trim(),
            secret_name.trim()
        );
    }
    Ok(removed)
}

/// Resolve `${secrets.NAME}` placeholders using the store for this scope.
pub fn resolve_secret_placeholders(
    session_user_id: &str,
    scope_id: &str,
    raw: &str,
) -> Result<String> {
    // Scan the original text only. Inserted secret values are not scanned again,
    // so a value that itself contains `${secrets.` cannot loop or expand further.
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(start) = rest.find("${secrets.") {
        out.push_str(&rest[..start]);
        let after = start + "${secrets.".len();
        let Some(end_rel) = rest[after..].find('}') else {
            out.push_str(&rest[start..]);
            return Ok(out);
        };
        let name = rest[after..after + end_rel].trim();
        let value = get_secret(session_user_id, scope_id, name)?
            .with_context(|| format!("secret `{name}` not found for scope `{scope_id}`"))?;
        out.push_str(&value);
        rest = &rest[after + end_rel + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

/// Environment variables injected into plugin hook / MCP child processes.
pub fn plugin_process_env(
    session_user_id: &str,
    plugin_id: &str,
    plugin_root: &std::path::Path,
    project_dir: Option<&std::path::Path>,
) -> Result<Vec<(String, String)>> {
    let data = crate::user_data_paths::plugin_data_host_dir(session_user_id, plugin_id)?;
    let mut env = vec![
        (
            "POINTER_PLUGIN_ROOT".into(),
            plugin_root.display().to_string(),
        ),
        (
            "POINTER_PLUGIN_DATA".into(),
            data.display().to_string(),
        ),
    ];
    if let Some(project) = project_dir {
        env.push((
            "POINTER_PROJECT_DIR".into(),
            project.display().to_string(),
        ));
    }
    Ok(env)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_get_delete_roundtrip() {
        let _guard = crate::storage::test_app_data_dir_lock();
        let temp = tempfile::tempdir().unwrap();
        crate::storage::set_test_app_data_dir(temp.path().to_path_buf());
        put_secret("alice", "com.example.p", "TOKEN", "s3cret").unwrap();
        assert_eq!(
            get_secret("alice", "com.example.p", "TOKEN").unwrap().as_deref(),
            Some("s3cret")
        );
        assert!(delete_secret("alice", "com.example.p", "TOKEN").unwrap());
        assert!(get_secret("alice", "com.example.p", "TOKEN")
            .unwrap()
            .is_none());
    }

    #[test]
    fn resolve_placeholders() {
        let _guard = crate::storage::test_app_data_dir_lock();
        let temp = tempfile::tempdir().unwrap();
        crate::storage::set_test_app_data_dir(temp.path().to_path_buf());
        put_secret("bob", "srv", "API_KEY", "abc").unwrap();
        let got = resolve_secret_placeholders("bob", "srv", "Bearer ${secrets.API_KEY}").unwrap();
        assert_eq!(got, "Bearer abc");
    }

    #[test]
    fn secret_values_are_not_expanded_again() {
        let _guard = crate::storage::test_app_data_dir_lock();
        let temp = tempfile::tempdir().unwrap();
        crate::storage::set_test_app_data_dir(temp.path().to_path_buf());
        put_secret("bob", "srv", "TOKEN", "${secrets.TOKEN}").unwrap();
        let got = resolve_secret_placeholders("bob", "srv", "x=${secrets.TOKEN}").unwrap();
        assert_eq!(got, "x=${secrets.TOKEN}");
    }
}
