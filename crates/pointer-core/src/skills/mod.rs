pub mod builtin;
pub mod external;

/// Bundled skills enabled for new users when lead agent is `general`.
pub const DEFAULT_ENABLED_SKILL_IDS: &[&str] =
    &["find-skills", "dev-env-setup", "skill-creator", "pointer-config"];

use crate::models::{SkillDef, SkillImportResult};
use anyhow::{anyhow, Result};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

#[derive(Default)]
pub struct SkillRegistry {
    inner: RwLock<HashMap<String, SkillDef>>,
}

impl SkillRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&self, def: SkillDef) {
        self.inner.write().insert(def.id.clone(), def);
    }

    /// Rescan skill directories and refresh external skill metadata in the registry.
    pub fn reload_meta(&self) -> anyhow::Result<usize> {
        let external = external::load_external_skills()?;
        let count = external.len();
        let mut g = self.inner.write();
        g.retain(|_, s| s.builtin);
        for skill in external {
            g.insert(skill.id.clone(), skill);
        }
        log::info!("skill_registry: reload_meta loaded {count} external skill(s)");
        Ok(count)
    }

    pub fn reload_external(&self) -> anyhow::Result<()> {
        self.reload_meta().map(|_| ())
    }

    pub fn import_zip(&self, bytes: &[u8]) -> anyhow::Result<SkillImportResult> {
        let result = external::import_skill_zip(bytes)?;
        self.reload_meta()?;
        Ok(result)
    }

    pub fn import_path(&self, source: &Path) -> anyhow::Result<SkillImportResult> {
        let result = external::import_skill_path(source)?;
        self.reload_meta()?;
        Ok(result)
    }

    pub fn resolve_import_source(path: &str) -> anyhow::Result<PathBuf> {
        let trimmed = path.trim();
        if trimmed.is_empty() {
            return Err(anyhow!("path 不能为空"));
        }
        let p = Path::new(trimmed);
        if p.is_absolute() {
            return Ok(p.to_path_buf());
        }
        let root = crate::tools::file::resolve_tool_workspace_root()?;
        Ok(root.join(p))
    }

    pub fn list(&self) -> Vec<SkillDef> {
        let g = self.inner.read();
        let mut v: Vec<SkillDef> = g.values().cloned().collect();
        v.sort_by(|a, b| a.name.cmp(&b.name));
        v
    }

    pub fn get(&self, id: &str) -> Option<SkillDef> {
        self.inner.read().get(id).cloned()
    }

    pub fn progressive_context(&self, ids: &[String]) -> (Vec<String>, Vec<String>) {
        let g = self.inner.read();
        let selected: Vec<_> = ids.iter().filter_map(|id| g.get(id)).collect();
        let mut prompts = Vec::new();
        if !ids.is_empty() {
            let mut index = String::from(
                "可用 Skills（第一层：frontmatter 索引）。根据用户任务判断是否需要使用某个 Skill；需要时调用 **`skill_load_instructions`** 读取该 Skill 的完整 SKILL.md 正文说明。不要在未读取正文前假设详细步骤。\n",
            );
            for id in ids {
                if let Some(s) = g.get(id) {
                    index.push_str(&format!(
                        "- id: {}\n  name: {}\n  description: {}\n",
                        s.id, s.name, s.description
                    ));
                    if !s.tags.is_empty() {
                        index.push_str(&format!("  tags: {}\n", s.tags.join(", ")));
                    }
                    if !s.resource_files.is_empty() {
                        index.push_str(&format!(
                            "  resources: {} 个，可按需通过 **skill:read_resource** 读取\n",
                            s.resource_files.len()
                        ));
                    }
                } else {
                    index.push_str(&format!(
                        "- id: {id}\n  status: 未安装（`skill_load_instructions` 会失败；请重启应用同步内置技能，或用 skill_import 安装）\n"
                    ));
                }
            }
            prompts.push(index);
        }

        let mut tools = vec![
            "skill_import".to_string(),
            "skill_load_instructions".to_string(),
            "skill_read_resource".to_string(),
        ];
        for s in selected {
            for t in &s.tool_names {
                if !tools.contains(t) {
                    tools.push(t.clone());
                }
            }
        }
        (prompts, tools)
    }

    pub fn load_instructions(&self, id: &str) -> Result<String> {
        let g = self.inner.read();
        let skill = g.get(id).ok_or_else(|| anyhow!("未找到 Skill: {id}"))?;
        Ok(format!(
            "【Skill：{}】\n{}",
            skill.name, skill.system_prompt
        ))
    }

    pub fn read_resource(&self, id: &str, path: &str) -> Result<String> {
        let g = self.inner.read();
        let skill = g.get(id).ok_or_else(|| anyhow!("未找到 Skill: {id}"))?;
        if !skill.resource_files.iter().any(|p| p == path) {
            return Err(anyhow!("资源不在该 Skill 的可读取列表中: {path}"));
        }
        let Some(root) = &skill.source else {
            return Err(anyhow!("内置 Skill 没有关联资源目录"));
        };
        let full_path = safe_resource_join(Path::new(root), Path::new(path))?;
        Ok(fs::read_to_string(full_path)?)
    }
}

fn safe_resource_join(root: &Path, rel: &Path) -> Result<std::path::PathBuf> {
    let mut out = root.to_path_buf();
    for c in rel.components() {
        match c {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            _ => return Err(anyhow!("非法资源路径: {}", rel.display())),
        }
    }
    Ok(out)
}
