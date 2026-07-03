//! Skill library curator: auto maintenance of ~/.pointer/skills (stale markers / archive).

use super::external::{pointer_home_dir, pointer_skills_dir};
use crate::chat_service::AppState;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const CURATOR_META_FILE: &str = "curator_meta.json";
const STALE_DAYS: u64 = 30;
const ARCHIVE_DAYS: u64 = 90;
const DAY_MS: i64 = 86_400_000;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct CuratorMeta {
    #[serde(default, rename = "lastLlmRunMs")]
    last_llm_run_ms: i64,
    #[serde(default, rename = "lastMaintenanceMs")]
    last_maintenance_ms: i64,
    #[serde(default, rename = "skillUsageMs")]
    skill_usage_ms: std::collections::HashMap<String, i64>,
}

#[derive(Debug, Default)]
pub struct MaintenanceReport {
    pub stale_marked: Vec<String>,
    pub archived: Vec<String>,
}

pub fn record_skill_usage(skill_id: &str) {
    let Ok(mut meta) = load_meta() else {
        return;
    };
    meta.skill_usage_ms
        .insert(skill_id.trim().to_string(), now_ms());
    if let Err(e) = save_meta(&meta) {
        log::warn!("curator: record_skill_usage save failed: {e:#}");
    }
}

pub fn run_auto_maintenance() -> Result<MaintenanceReport> {
    let root = pointer_skills_dir()?;
    let archive_root = root.join(".archive");
    fs::create_dir_all(&archive_root)?;
    let mut meta = load_meta()?;
    let now = now_ms();
    let mut report = MaintenanceReport::default();

    for entry in fs::read_dir(&root)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.starts_with('.') {
            continue;
        }
        if !is_managed_skill_dir(&path) {
            continue;
        }
        let last_used = meta
            .skill_usage_ms
            .get(name)
            .copied()
            .unwrap_or_else(|| dir_mtime_ms(&path).unwrap_or(now));
        let age_days = ((now - last_used).max(0) as u64) / (DAY_MS as u64);
        if age_days >= ARCHIVE_DAYS {
            let dest = archive_root.join(name);
            if dest.exists() {
                fs::remove_dir_all(&dest)?;
            }
            fs::rename(&path, &dest)?;
            meta.skill_usage_ms.remove(name);
            report.archived.push(name.to_string());
            log::info!("curator: archived skill {name} (unused {age_days}d)");
        } else if age_days >= STALE_DAYS {
            let marker = path.join(".pointer-stale");
            if !marker.exists() {
                fs::write(&marker, format!("marked_at_ms={now}\n"))?;
                report.stale_marked.push(name.to_string());
                log::info!("curator: marked stale skill {name} (unused {age_days}d)");
            }
        }
    }

    meta.last_maintenance_ms = now;
    save_meta(&meta)?;
    if !report.stale_marked.is_empty() || !report.archived.is_empty() {
        log::info!(
            "curator: maintenance stale={} archived={}",
            report.stale_marked.len(),
            report.archived.len()
        );
    }
    Ok(report)
}

fn is_managed_skill_dir(dir: &Path) -> bool {
    super::external::is_skill_package_dir(dir)
}

fn meta_path() -> Result<PathBuf> {
    Ok(pointer_home_dir()?.join(CURATOR_META_FILE))
}

fn load_meta() -> Result<CuratorMeta> {
    let path = meta_path()?;
    if !path.exists() {
        return Ok(CuratorMeta::default());
    }
    let raw = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    Ok(serde_json::from_str(&raw).unwrap_or_default())
}

fn save_meta(meta: &CuratorMeta) -> Result<()> {
    let path = meta_path()?;
    let tmp = path.with_extension("json.tmp");
    let raw = serde_json::to_string_pretty(meta)?;
    fs::write(&tmp, raw)?;
    fs::rename(&tmp, &path)?;
    Ok(())
}

fn dir_mtime_ms(path: &Path) -> Option<i64> {
    let modified = fs::metadata(path).ok()?.modified().ok()?;
    modified
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_millis() as i64)
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub fn start_background_loop(state: Arc<AppState>) {
    let _ = state;
    tokio::spawn(async move {
        if let Err(e) = run_auto_maintenance() {
            log::warn!("curator: startup maintenance failed: {e:#}");
        }
        let mut interval = tokio::time::interval(Duration::from_secs(1800));
        interval.tick().await;
        loop {
            interval.tick().await;
            if let Err(e) = run_auto_maintenance() {
                log::warn!("curator: periodic maintenance failed: {e:#}");
            }
        }
    });
}
