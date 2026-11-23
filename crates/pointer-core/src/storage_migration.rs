//! User data layout migration (dry-run + journal + hash verify).
//!
//! Moves legacy conversation-media / generated-media / computer-captures into
//! the per-user data volume (`v2` layout). Safe to re-run: completed journal
//! entries are skipped.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::user_key::user_key;

const JOURNAL_FILE: &str = "layout-migration-journal.json";
pub const LAYOUT_VERSION: u32 = 2;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MigrationItem {
    pub kind: String,
    pub source: String,
    pub destination: String,
    pub bytes: u64,
    pub sha256: String,
    pub status: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MigrationJournal {
    pub layout_version: u32,
    pub user_key: String,
    pub started_at_ms: u64,
    pub updated_at_ms: u64,
    pub dry_run: bool,
    pub items: Vec<MigrationItem>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationReport {
    pub dry_run: bool,
    pub scanned: usize,
    pub copied: usize,
    pub skipped: usize,
    pub failed: usize,
    pub journal_path: String,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn journal_path(session_user_id: &str) -> Result<PathBuf> {
    let home = crate::user_data_paths::user_home_host_root(session_user_id, "")?;
    let dir = home.join(".pointer");
    fs::create_dir_all(&dir)?;
    Ok(dir.join(JOURNAL_FILE))
}

fn load_journal(path: &Path) -> MigrationJournal {
    fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save_journal(path: &Path, journal: &MigrationJournal) -> Result<()> {
    let tmp = path.with_extension("tmp");
    let json = serde_json::to_string_pretty(journal)?;
    fs::write(&tmp, json)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

fn file_sha256(path: &Path) -> Result<(u64, String)> {
    let mut file = fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        total += n as u64;
        hasher.update(&buf[..n]);
    }
    Ok((total, format!("{:x}", hasher.finalize())))
}

fn copy_verified(src: &Path, dst: &Path) -> Result<(u64, String)> {
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)?;
    }
    let (bytes, hash) = file_sha256(src)?;
    {
        let mut in_f = fs::File::open(src)?;
        let mut out_f = fs::File::create(dst)?;
        std::io::copy(&mut in_f, &mut out_f)?;
        out_f.flush()?;
    }
    let (bytes2, hash2) = file_sha256(dst)?;
    if bytes != bytes2 || hash != hash2 {
        let _ = fs::remove_file(dst);
        anyhow::bail!(
            "hash mismatch after copy {} -> {}",
            src.display(),
            dst.display()
        );
    }
    Ok((bytes, hash))
}

fn path_owned_by_user(rel: &Path, user_segment: &str, user_key_seg: &str) -> bool {
    rel.components().any(|component| {
        let text = component.as_os_str().to_string_lossy();
        text == user_segment || text == user_key_seg
    })
}

fn strip_user_prefix(rel: &Path, user_segment: &str, user_key_seg: &str) -> PathBuf {
    let mut parts = rel.components();
    if let Some(first) = parts.next() {
        let text = first.as_os_str().to_string_lossy();
        if text == user_segment || text == user_key_seg {
            return parts.as_path().to_path_buf();
        }
    }
    rel.to_path_buf()
}

fn walk_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    if !root.is_dir() {
        return Ok(out);
    }
    for entry in walkdir::WalkDir::new(root)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_file() {
            out.push(entry.path().to_path_buf());
        }
    }
    Ok(out)
}

/// Scan legacy trees and migrate into the user data volume.
///
/// When `dry_run` is true, only the journal is filled with planned items.
pub fn migrate_user_layout(session_user_id: &str, dry_run: bool) -> Result<MigrationReport> {
    let uid = session_user_id.trim();
    let ukey = user_key(uid);
    let app_data = crate::storage::app_data_dir()?;
    let data_root = crate::user_data_paths::user_data_host_root(uid, "_migration")?;
    let journal_file = journal_path(uid)?;
    let mut journal = load_journal(&journal_file);
    if journal.user_key.is_empty() {
        journal.user_key = ukey.clone();
        journal.layout_version = LAYOUT_VERSION;
        journal.started_at_ms = now_ms();
    }
    journal.dry_run = dry_run;
    journal.updated_at_ms = now_ms();

    let completed: std::collections::HashSet<String> = journal
        .items
        .iter()
        .filter(|i| i.status == "copied" || i.status == "verified")
        .map(|i| i.source.clone())
        .collect();

    let mut planned: Vec<(String, PathBuf, PathBuf)> = Vec::new();

    let user_segment = crate::user_storage::user_storage_segment(uid);
    let user_key_seg = ukey.clone();

    // Legacy conversation-media/{user}/{conv}/{file} — only this user's files.
    let media_root = app_data.join("conversation-media");
    for file in walk_files(&media_root)? {
        let rel = match file.strip_prefix(&media_root) {
            Ok(r) => r,
            Err(_) => continue,
        };
        if !path_owned_by_user(rel, &user_segment, &user_key_seg) {
            continue;
        }
        let dest_rel = strip_user_prefix(rel, &user_segment, &user_key_seg);
        let dest = data_root.join("media").join("chat").join(dest_rel);
        planned.push(("conversation-media".into(), file, dest));
    }

    // Legacy generated-media/ — only paths that name this user.
    let gen_root = app_data.join("generated-media");
    for file in walk_files(&gen_root)? {
        let rel = match file.strip_prefix(&gen_root) {
            Ok(r) => r,
            Err(_) => continue,
        };
        if !path_owned_by_user(rel, &user_segment, &user_key_seg) {
            continue;
        }
        let dest_rel = strip_user_prefix(rel, &user_segment, &user_key_seg);
        let dest = data_root.join("media").join("generated").join(dest_rel);
        planned.push(("generated-media".into(), file, dest));
    }

    // Legacy computer-captures/ — only paths that name this user.
    let cap_root = app_data.join("computer-captures");
    for file in walk_files(&cap_root)? {
        let rel = match file.strip_prefix(&cap_root) {
            Ok(r) => r,
            Err(_) => continue,
        };
        if !path_owned_by_user(rel, &user_segment, &user_key_seg) {
            continue;
        }
        let dest_rel = strip_user_prefix(rel, &user_segment, &user_key_seg);
        let dest = data_root.join("captures").join(dest_rel);
        planned.push(("computer-captures".into(), file, dest));
    }

    let mut scanned = 0usize;
    let mut copied = 0usize;
    let mut skipped = 0usize;
    let mut failed = 0usize;

    for (kind, src, dst) in planned {
        scanned += 1;
        let src_s = src.display().to_string();
        if completed.contains(&src_s) {
            skipped += 1;
            continue;
        }
        if dry_run {
            let (bytes, hash) = match file_sha256(&src) {
                Ok(v) => v,
                Err(e) => {
                    log::warn!("migration dry-run hash failed {}: {e:#}", src.display());
                    failed += 1;
                    continue;
                }
            };
            journal.items.push(MigrationItem {
                kind: kind.clone(),
                source: src_s,
                destination: dst.display().to_string(),
                bytes,
                sha256: hash,
                status: "planned".into(),
            });
            continue;
        }
        match copy_verified(&src, &dst) {
            Ok((bytes, hash)) => {
                journal.items.push(MigrationItem {
                    kind,
                    source: src_s,
                    destination: dst.display().to_string(),
                    bytes,
                    sha256: hash,
                    status: "copied".into(),
                });
                copied += 1;
            }
            Err(e) => {
                log::warn!(
                    "migration copy failed {} -> {}: {e:#}",
                    src.display(),
                    dst.display()
                );
                journal.items.push(MigrationItem {
                    kind,
                    source: src_s,
                    destination: dst.display().to_string(),
                    bytes: 0,
                    sha256: String::new(),
                    status: format!("failed:{e:#}"),
                });
                failed += 1;
            }
        }
        journal.updated_at_ms = now_ms();
        save_journal(&journal_file, &journal)?;
    }

    journal.updated_at_ms = now_ms();
    save_journal(&journal_file, &journal)?;
    log::info!(
        "storage_migration: user_key={ukey} dry_run={dry_run} scanned={scanned} copied={copied} skipped={skipped} failed={failed}"
    );
    Ok(MigrationReport {
        dry_run,
        scanned,
        copied,
        skipped,
        failed,
        journal_path: journal_file.display().to_string(),
    })
}

/// Read the current journal for a user (empty if none).
pub fn read_migration_journal(session_user_id: &str) -> Result<MigrationJournal> {
    let path = journal_path(session_user_id)?;
    Ok(load_journal(&path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dry_run_plans_legacy_media() {
        let _guard = crate::storage::test_app_data_dir_lock();
        let temp = tempfile::tempdir().unwrap();
        crate::storage::set_test_app_data_dir(temp.path().to_path_buf());
        let media = temp
            .path()
            .join("conversation-media")
            .join("user_a")
            .join("conv1");
        fs::create_dir_all(&media).unwrap();
        fs::write(media.join("a.png"), b"png").unwrap();
        let other = temp
            .path()
            .join("conversation-media")
            .join("other_user")
            .join("conv9");
        fs::create_dir_all(&other).unwrap();
        fs::write(other.join("nope.png"), b"nope").unwrap();

        let report = migrate_user_layout("user_a", true).unwrap();
        assert_eq!(report.scanned, 1);
        assert_eq!(report.copied, 0);
        let journal = read_migration_journal("user_a").unwrap();
        assert_eq!(journal.items.len(), 1);
        assert_eq!(journal.items[0].status, "planned");
    }

    #[test]
    fn copy_is_idempotent_after_success() {
        let _guard = crate::storage::test_app_data_dir_lock();
        let temp = tempfile::tempdir().unwrap();
        crate::storage::set_test_app_data_dir(temp.path().to_path_buf());
        let media = temp
            .path()
            .join("conversation-media")
            .join("user_b")
            .join("conv1");
        fs::create_dir_all(&media).unwrap();
        fs::write(media.join("b.bin"), b"hello").unwrap();

        let first = migrate_user_layout("user_b", false).unwrap();
        assert_eq!(first.copied, 1);
        let second = migrate_user_layout("user_b", false).unwrap();
        assert_eq!(second.skipped, 1);
        assert_eq!(second.copied, 0);
    }
}
