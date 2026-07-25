//! Skill provenance and mutation policy (Hermes-aligned).

use super::external::{manifest_path_in_dir, pointer_skills_dir, system_skills_dir};
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

const BUNDLED_MANIFEST: &str = ".bundled_manifest";
const USAGE_JSON: &str = ".usage.json";
const EXTERNAL_PROBE_MARKER: &str = "external_skills_probe_done";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SkillProvenanceKind {
    System,
    User,
    /// Codex / Agent `~/.agents/skills` compatibility path (read-only in Pointer).
    External,
}

impl SkillProvenanceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::User => "user",
            Self::External => "external",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct UsageRecord {
    #[serde(default)]
    pinned: bool,
    #[serde(default, rename = "agentCreated")]
    agent_created: bool,
    #[serde(default, rename = "importedFrom")]
    imported_from: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct UsageStore {
    #[serde(default)]
    skills: HashMap<String, UsageRecord>,
}

pub fn skill_dir_hash(skill_dir: &Path) -> Result<String> {
    let manifest = manifest_path_in_dir(skill_dir)
        .ok_or_else(|| anyhow!("missing SKILL.md in {}", skill_dir.display()))?;
    let bytes = fs::read(&manifest)?;
    let digest = Sha256::digest(bytes);
    Ok(format!("{:x}", digest))
}

pub fn read_bundled_manifest_names(root: &Path) -> HashSet<String> {
    let path = root.join(BUNDLED_MANIFEST);
    if !path.exists() {
        return HashSet::new();
    }
    let Ok(raw) = fs::read_to_string(&path) else {
        return HashSet::new();
    };
    raw.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            Some(line.split(':').next()?.trim().to_string())
        })
        .collect()
}

pub fn read_bundled_origin_hash(root: &Path, skill_id: &str) -> Option<String> {
    let path = root.join(BUNDLED_MANIFEST);
    let raw = fs::read_to_string(path).ok()?;
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.splitn(2, ':');
        let name = parts.next()?.trim();
        if name == skill_id {
            return parts.next().map(str::trim).map(str::to_string);
        }
    }
    None
}

pub fn upsert_bundled_manifest_entry(root: &Path, skill_id: &str, hash: &str) -> Result<()> {
    let path = root.join(BUNDLED_MANIFEST);
    let mut lines: Vec<String> = if path.exists() {
        fs::read_to_string(&path)?
            .lines()
            .map(str::to_string)
            .collect()
    } else {
        Vec::new()
    };
    let entry = format!("{skill_id}:{hash}");
    let mut replaced = false;
    for line in &mut lines {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if trimmed
            .split(':')
            .next()
            .is_some_and(|n| n.trim() == skill_id)
        {
            *line = entry.clone();
            replaced = true;
            break;
        }
    }
    if !replaced {
        lines.push(entry);
    }
    let mut out = lines.join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    atomic_write(&path, out.as_bytes())?;
    Ok(())
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("invalid path: {}", path.display()))?;
    fs::create_dir_all(parent)?;
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

fn usage_path() -> Result<PathBuf> {
    Ok(pointer_skills_dir()?.join(USAGE_JSON))
}

fn load_usage() -> UsageStore {
    let Ok(path) = usage_path() else {
        return UsageStore::default();
    };
    if !path.exists() {
        return UsageStore::default();
    }
    fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save_usage(store: &UsageStore) -> Result<()> {
    let path = usage_path()?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_string_pretty(store)?)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

pub fn mark_agent_created(skill_id: &str, imported_from: Option<&str>) {
    let Ok(mut store) = (|| -> Result<UsageStore> { Ok(load_usage()) })() else {
        return;
    };
    let rec = store.skills.entry(skill_id.trim().to_string()).or_default();
    rec.agent_created = true;
    if let Some(src) = imported_from.filter(|s| !s.is_empty()) {
        rec.imported_from = Some(src.to_string());
    }
    if let Err(e) = save_usage(&store) {
        log::warn!("provenance: save usage failed for {skill_id}: {e:#}");
    }
}

pub fn is_pinned(skill_id: &str) -> bool {
    load_usage().skills.get(skill_id).is_some_and(|r| r.pinned)
}

pub fn is_system_bundled(skill_id: &str) -> bool {
    system_skills_dir()
        .ok()
        .map(|root| read_bundled_manifest_names(&root).contains(skill_id))
        .unwrap_or(false)
}

pub fn is_agent_created(skill_id: &str) -> bool {
    if is_system_bundled(skill_id) {
        return false;
    }
    let usage = load_usage();
    if usage.skills.get(skill_id).is_some_and(|r| r.agent_created) {
        return true;
    }
    // Under user dir and not in bundled manifest => treat as user/agent skill.
    pointer_skills_dir()
        .ok()
        .map(|root| root.join(skill_id).is_dir())
        .unwrap_or(false)
}

pub fn is_curation_eligible(skill_id: &str) -> bool {
    is_agent_created(skill_id) && !is_pinned(skill_id)
}

pub fn resolve_provenance_for_path(skill_id: &str, skill_dir: &Path) -> SkillProvenanceKind {
    if super::external::is_agents_skills_path(skill_dir) {
        return SkillProvenanceKind::External;
    }
    if system_skills_dir()
        .ok()
        .is_some_and(|root| skill_dir.starts_with(&root))
        && is_system_bundled(skill_id)
    {
        return SkillProvenanceKind::System;
    }
    SkillProvenanceKind::User
}

pub fn is_mutable(skill_id: &str, skill_dir: &Path) -> bool {
    match resolve_provenance_for_path(skill_id, skill_dir) {
        SkillProvenanceKind::System | SkillProvenanceKind::External => false,
        SkillProvenanceKind::User => !is_pinned(skill_id),
    }
}

pub fn external_probe_marker_path() -> Result<PathBuf> {
    Ok(super::external::pointer_home_dir()?.join(EXTERNAL_PROBE_MARKER))
}

pub fn should_prompt_external_import() -> bool {
    external_probe_marker_path()
        .ok()
        .is_some_and(|p| !p.exists())
}

pub fn mark_external_import_prompt_done() -> Result<()> {
    let path = external_probe_marker_path()?;
    atomic_write(&path, b"")?;
    Ok(())
}

pub fn sync_bundled_entry_if_unchanged(
    root: &Path,
    skill_id: &str,
    skill_dir: &Path,
) -> Result<bool> {
    let hash = skill_dir_hash(skill_dir)?;
    if let Some(origin) = read_bundled_origin_hash(root, skill_id) {
        if origin != hash {
            log::info!("bundled skill user-modified, skip sync: {skill_id}");
            return Ok(false);
        }
    }
    upsert_bundled_manifest_entry(root, skill_id, &hash)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn bundled_manifest_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        upsert_bundled_manifest_entry(root, "alpha", "hash1").unwrap();
        upsert_bundled_manifest_entry(root, "beta", "hash2").unwrap();
        let names = read_bundled_manifest_names(root);
        assert!(names.contains("alpha"));
        assert!(names.contains("beta"));
        assert_eq!(
            read_bundled_origin_hash(root, "alpha").as_deref(),
            Some("hash1")
        );
    }
}
