//! Runtime load of the vendored sqlite-cjk-fts extension (`cjk_bigram` tokenizer).

use anyhow::{Context, Result};
use rusqlite::Connection;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

#[cfg(target_os = "macos")]
const EXT_FILE: &str = "libcjkfts.dylib";
#[cfg(target_os = "linux")]
const EXT_FILE: &str = "libcjkfts.so";
#[cfg(target_os = "windows")]
const EXT_FILE: &str = "libcjkfts.dll";

#[cfg(target_os = "macos")]
const EXT_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/libcjkfts.dylib"));
#[cfg(target_os = "linux")]
const EXT_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/libcjkfts.so"));
#[cfg(target_os = "windows")]
const EXT_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/libcjkfts.dll"));

static EXTRACTED_PATH: OnceLock<PathBuf> = OnceLock::new();

/// Load `sqlite3_cjkfts_init` and register the `cjk_bigram` FTS5 tokenizer.
pub fn ensure_loaded(conn: &Connection) -> Result<()> {
    let path = extracted_extension_path()?;
    unsafe {
        let _guard = rusqlite::LoadExtensionGuard::new(conn)
            .context("enable SQLite load_extension")?;
        conn.load_extension(&path, None::<&str>)
            .with_context(|| format!("load cjk fts extension {}", path.display()))?;
    }
    log::info!("session_search: loaded cjk_bigram tokenizer from {}", path.display());
    Ok(())
}

fn extracted_extension_path() -> Result<PathBuf> {
    if let Some(path) = EXTRACTED_PATH.get() {
        return Ok(path.clone());
    }
    let path = extract_extension_bytes()?;
    let _ = EXTRACTED_PATH.set(path.clone());
    Ok(path)
}

fn extract_extension_bytes() -> Result<PathBuf> {
    if EXT_BYTES.is_empty() {
        anyhow::bail!("cjk fts extension was not built (empty embedded library)");
    }
    let dir = crate::storage::app_data_dir()
        .unwrap_or_else(|_| std::env::temp_dir())
        .join("native");
    fs::create_dir_all(&dir)
        .with_context(|| format!("create native extension dir {}", dir.display()))?;
    let path = dir.join(EXT_FILE);
    write_if_changed(&path, EXT_BYTES)?;
    set_executable(&path)?;
    Ok(path)
}

fn write_if_changed(path: &Path, bytes: &[u8]) -> Result<()> {
    if path.exists() {
        if fs::read(path).ok().as_deref() == Some(bytes) {
            return Ok(());
        }
    }
    fs::write(path, bytes).with_context(|| format!("write cjk fts extension {}", path.display()))
}

#[cfg(unix)]
fn set_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(path)
        .with_context(|| format!("stat cjk fts extension {}", path.display()))?
        .permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms)
        .with_context(|| format!("chmod cjk fts extension {}", path.display()))
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) -> Result<()> {
    Ok(())
}
