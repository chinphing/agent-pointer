//! First-launch probe of external agent skill directories (Codex, Claude, OpenClaw, Hermes).

use super::external::{
    discover_skill_dirs_in, install_skill_dir_with_provenance, is_ignored_skill_dir,
    load_external_skills, load_skill_from_dir,
};
use super::provenance::{mark_external_import_prompt_done, should_prompt_external_import};
use crate::models::SkillImportResult;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalSkillSource {
    pub id: String,
    pub label: String,
    pub path: String,
    #[serde(rename = "skillCount")]
    pub skill_count: u32,
    #[serde(rename = "skillIds")]
    pub skill_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalSkillsProbeResult {
    #[serde(rename = "shouldPrompt")]
    pub should_prompt: bool,
    pub sources: Vec<ExternalSkillSource>,
    #[serde(rename = "totalSkills")]
    pub total_skills: u32,
}

fn codex_skills_dir(home: &Path) -> PathBuf {
    if let Ok(codex_home) = std::env::var("CODEX_HOME") {
        let trimmed = codex_home.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed).join("skills");
        }
    }
    home.join(".codex").join("skills")
}

fn external_source_specs(home: &Path) -> Vec<(&'static str, &'static str, PathBuf)> {
    vec![
        ("codex", "OpenAI Codex", codex_skills_dir(home)),
        ("claude", "Claude Code", home.join(".claude").join("skills")),
        (
            "openclaw",
            "OpenClaw",
            home.join(".openclaw").join("skills"),
        ),
        (
            "hermes",
            "Hermes Agent",
            home.join(".hermes").join("skills"),
        ),
    ]
}

fn already_loaded_ids() -> HashSet<String> {
    load_external_skills()
        .unwrap_or_default()
        .into_iter()
        .map(|s| s.id)
        .collect()
}

fn list_importable_in_root(root: &Path, loaded: &HashSet<String>) -> Vec<String> {
    let mut out = Vec::new();
    if !root.is_dir() {
        return out;
    }
    for entry in fs::read_dir(root).into_iter().flatten().flatten() {
        let path = entry.path();
        if !path.is_dir() || is_ignored_skill_dir(path.file_name()) {
            continue;
        }
        if let Ok(skill) = load_skill_from_dir(&path) {
            if !loaded.contains(&skill.id) {
                out.push(skill.id);
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

pub fn probe_external_skill_sources() -> Result<ExternalSkillsProbeResult> {
    let home = dirs::home_dir().ok_or_else(|| anyhow!("无法解析用户主目录"))?;
    let loaded = already_loaded_ids();
    let mut sources = Vec::new();
    let mut total = 0u32;

    for (id, label, path) in external_source_specs(&home) {
        let skill_ids = list_importable_in_root(&path, &loaded);
        let count = skill_ids.len() as u32;
        if count == 0 {
            continue;
        }
        total += count;
        sources.push(ExternalSkillSource {
            id: id.to_string(),
            label: label.to_string(),
            path: path.to_string_lossy().to_string(),
            skill_count: count,
            skill_ids,
        });
    }

    Ok(ExternalSkillsProbeResult {
        should_prompt: should_prompt_external_import() && total > 0,
        sources,
        total_skills: total,
    })
}

pub fn dismiss_external_skills_prompt() -> Result<()> {
    mark_external_import_prompt_done()
}

pub fn import_external_skills(source_ids: &[String]) -> Result<SkillImportResult> {
    let home = dirs::home_dir().ok_or_else(|| anyhow!("无法解析用户主目录"))?;
    let specs: Vec<_> = external_source_specs(&home);
    let wanted: HashSet<_> = source_ids
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    if wanted.is_empty() {
        return Err(anyhow!("请至少选择一个来源"));
    }

    let mut loaded = already_loaded_ids();
    let mut imported = Vec::new();
    let mut skipped = Vec::new();

    for (id, _label, root) in specs {
        if !wanted.contains(id) {
            continue;
        }
        if !root.is_dir() {
            skipped.push(format!("{id}: 目录不存在"));
            continue;
        }
        for skill_dir in discover_skill_dirs_in(&root) {
            let preview = match load_skill_from_dir(&skill_dir) {
                Ok(s) => s,
                Err(err) => {
                    skipped.push(format!("{}: {err}", skill_dir.display()));
                    continue;
                }
            };
            if loaded.contains(&preview.id) {
                skipped.push(format!("{}: 已存在", preview.id));
                continue;
            }
            match install_skill_dir_with_provenance(&skill_dir, id) {
                Ok(skill) => {
                    loaded.insert(skill.id.clone());
                    imported.push(skill);
                }
                Err(err) => skipped.push(format!("{}: {err}", skill_dir.display())),
            }
        }
    }

    mark_external_import_prompt_done()?;
    Ok(SkillImportResult { imported, skipped })
}
