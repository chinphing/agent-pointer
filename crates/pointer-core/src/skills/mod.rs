pub mod builtin;
pub mod curator;
pub mod external;
pub mod external_probe;
pub mod provenance;

/// Bundled skills shipped under repository `skills/`. Keep in sync with frontend `DEFAULT_ENABLED_SKILL_IDS`.
pub const BUNDLED_SKILL_IDS: &[&str] = &[
    "find-skills",
    "dev-env-setup",
    "skill-creator",
    "pointer-manager",
    "docx",
    "xlsx",
    "pptx",
    "pdf",
    "agent-browser",
];

/// Default enabled set for new users — all bundled system skills.
pub const DEFAULT_ENABLED_SKILL_IDS: &[&str] = BUNDLED_SKILL_IDS;

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
            prompts.push(build_available_skills_prompt(&g, ids));
        }

        let mut tools = vec!["skill_import".to_string(), "skill_read".to_string()];
        for s in selected {
            for t in &s.tool_names {
                if !tools.contains(t) {
                    tools.push(t.clone());
                }
            }
        }
        (prompts, tools)
    }

    /// Read skill content.
    ///
    /// `path` is required: `SKILL.md` (or equivalent manifest name) for instructions;
    /// otherwise a skill-relative bundled resource path.
    pub fn read(&self, id: &str, path: &str) -> Result<String> {
        let path = path.trim();
        if path.is_empty() {
            return Err(anyhow!(
                "缺少 path（读说明传 SKILL.md；读资源传 skill 相对路径）"
            ));
        }
        if external::is_skill_manifest_path(path) {
            self.load_instructions(id)
        } else {
            self.read_resource(id, path)
        }
    }

    fn load_instructions(&self, id: &str) -> Result<String> {
        let (fallback_name, skill_dir, track_usage, cached_body) = {
            let g = self.inner.read();
            let skill = g.get(id).ok_or_else(|| anyhow!("未找到 Skill: {id}"))?;
            let track = !skill.builtin
                && skill.source.as_deref().is_some_and(|s| {
                    s.contains(".pointer/skills") || s.contains(".pointer\\skills")
                });
            (
                skill.name.clone(),
                skill.source.clone(),
                track,
                skill.system_prompt.clone(),
            )
        };
        if track_usage {
            curator::record_skill_usage(id);
        }

        // Hermes-aligned: re-read SKILL.md from disk each call so edits apply immediately.
        // Registry only keeps catalog metadata; `system_prompt` is intentionally empty.
        let (name, body) = match skill_dir
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            Some(dir) => external::read_skill_instructions_from_dir(Path::new(dir))
                .map_err(|e| anyhow!("读取 Skill 失败 ({id}): {e:#}"))?,
            None => (fallback_name, cached_body),
        };

        let body = skill_dir
            .as_deref()
            .map(|dir| substitute_base_dir_in_skill_body(&body, Path::new(dir)))
            .unwrap_or(body);
        Ok(format!("【Skill：{}】\n{}", name, body))
    }

    fn read_resource(&self, id: &str, path: &str) -> Result<String> {
        let g = self.inner.read();
        let skill = g.get(id).ok_or_else(|| anyhow!("未找到 Skill: {id}"))?;
        let Some(root) = skill.source.as_deref() else {
            return Err(anyhow!("内置 Skill 没有关联资源目录"));
        };
        let root = Path::new(root);
        let full_path = safe_resource_join(root, Path::new(path))?;
        ensure_within_skill_dir(&full_path, root)?;
        if !full_path.is_file() {
            return Err(anyhow!("Skill 资源不存在: {path}"));
        }
        fs::read_to_string(&full_path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::InvalidData {
                anyhow!("Skill 资源不是文本文件: {path}")
            } else {
                e.into()
            }
        })
    }
}

fn ensure_within_skill_dir(path: &Path, root: &Path) -> Result<()> {
    let root_canon = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let path_canon = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if !path_canon.starts_with(&root_canon) {
        return Err(anyhow!("非法资源路径: {}", path.display()));
    }
    Ok(())
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

/// Absolute path to a skill's `SKILL.md` manifest (when `source` is the skill root dir).
pub fn skill_manifest_path(skill: &SkillDef) -> Option<PathBuf> {
    let source = skill.source.as_deref()?.trim();
    if source.is_empty() {
        return None;
    }
    external::manifest_path_in_dir(Path::new(source))
        .or_else(|| Some(Path::new(source).join("SKILL.md")))
}

fn resolve_compact_home_prefixes() -> Vec<PathBuf> {
    let mut homes = Vec::new();
    if let Some(home) = dirs::home_dir() {
        homes.push(home);
    }
    if let Ok(app) = crate::storage::app_data_dir() {
        homes.push(app);
    }
    homes.sort_by_key(|p| std::cmp::Reverse(p.components().count()));
    homes
}

/// Compact absolute paths for prompt injection (`~/…`), OpenClaw-aligned.
pub fn compact_skill_location(path: &Path) -> String {
    compact_skill_location_with_prefixes(path, &resolve_compact_home_prefixes())
}

fn compact_skill_location_with_prefixes(path: &Path, homes: &[PathBuf]) -> String {
    let normalized = path.to_string_lossy().replace('\\', "/");
    for home in homes {
        let home_str = home.to_string_lossy().replace('\\', "/");
        let prefix = if home_str.ends_with('/') {
            home_str
        } else {
            format!("{home_str}/")
        };
        if normalized.starts_with(&prefix) {
            return format!("~/{}", normalized[prefix.len()..].trim_start_matches('/'));
        }
    }
    normalized
}

fn escape_xml_attr(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn build_available_skills_prompt(registry: &HashMap<String, SkillDef>, ids: &[String]) -> String {
    let mut lines = vec![
        "可用 Skills（第一层：frontmatter 索引）。".to_string(),
        "Before replying: scan <available_skills> entries.".to_string(),
        "If a skill matches, call **`skill_read`** with **`skill_id`** equal to `<name>` and **`path`** `SKILL.md` (do not guess).".to_string(),
        "When a skill references a relative path or `{baseDir}`, resolve it against the skill directory (parent of `<location>`) and use absolute paths in **`terminal`**.".to_string(),
        "For bundled resources (`references/`, `assets/`, `scripts/`), call **`skill_read`** again with the same **`skill_id`** and that relative **`path`**.".to_string(),
        String::new(),
        "The following skills provide specialized instructions for specific tasks.".to_string(),
        "Use **`skill_read`** (`skill_id` + required **`path`**) to load instructions or resources when a task matches `<description>`.".to_string(),
        String::new(),
        "<available_skills>".to_string(),
    ];

    let mut installed: Vec<_> = ids
        .iter()
        .filter_map(|id| registry.get(id).map(|s| (id.as_str(), s)))
        .collect();
    installed.sort_by(|a, b| a.1.name.cmp(&b.1.name));

    for (_id, skill) in installed {
        lines.push("  <skill>".to_string());
        lines.push(format!("    <name>{}</name>", escape_xml_attr(&skill.name)));
        lines.push(format!(
            "    <description>{}</description>",
            escape_xml_attr(&skill.description)
        ));
        if let Some(manifest) = skill_manifest_path(skill) {
            let location = compact_skill_location(&manifest);
            lines.push(format!(
                "    <location>{}</location>",
                escape_xml_attr(&location)
            ));
        } else {
            log::warn!(
                "skill_registry: skill {} missing source; omitting <location> from inject",
                skill.id
            );
        }
        lines.push("  </skill>".to_string());
    }

    for id in ids {
        if !registry.contains_key(id) {
            lines.push("  <skill>".to_string());
            lines.push(format!("    <name>{}</name>", escape_xml_attr(id)));
            lines.push(
                "    <description>未安装 — skill_read 会失败；请 skill_import 或重启同步内置技能</description>".to_string(),
            );
            lines.push("  </skill>".to_string());
        }
    }

    lines.push("</available_skills>".to_string());
    lines.join("\n")
}

fn substitute_base_dir_in_skill_body(body: &str, skill_dir: &Path) -> String {
    let base = skill_dir.to_string_lossy();
    body.replace("{baseDir}", base.as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    #[test]
    fn is_skill_manifest_path_matches_skill_md() {
        assert!(external::is_skill_manifest_path("SKILL.md"));
        assert!(external::is_skill_manifest_path("skill.md"));
        assert!(external::is_skill_manifest_path("./SKILL.md"));
        assert!(!external::is_skill_manifest_path("references/guide.md"));
    }

    #[test]
    fn compact_skill_location_replaces_home_prefix() {
        let home = PathBuf::from("/Users/test");
        let path = PathBuf::from("/Users/test/.pointer/skills/pdf/SKILL.md");
        assert_eq!(
            compact_skill_location_with_prefixes(&path, &[home]),
            "~/.pointer/skills/pdf/SKILL.md"
        );
    }

    #[test]
    fn skill_manifest_path_joins_skill_md() {
        let skill = SkillDef {
            id: "pdf".into(),
            name: "pdf".into(),
            description: "d".into(),
            tags: vec![],
            system_prompt: String::new(),
            tool_names: vec![],
            scenario: String::new(),
            builtin: false,
            resource_files: vec![],
            source: Some("/tmp/skills/pdf".into()),
            provenance: "system".into(),
            mutable: false,
        };
        assert_eq!(
            skill_manifest_path(&skill).map(|p| p.to_string_lossy().into_owned()),
            Some("/tmp/skills/pdf/SKILL.md".into())
        );
    }

    #[test]
    fn progressive_context_includes_openclaw_style_location() {
        let reg = SkillRegistry::new();
        reg.register(SkillDef {
            id: "demo".into(),
            name: "demo".into(),
            description: "Demo skill".into(),
            tags: vec![],
            system_prompt: String::new(),
            tool_names: vec![],
            scenario: String::new(),
            builtin: false,
            resource_files: vec![],
            source: Some("/tmp/demo-skill".into()),
            provenance: "system".into(),
            mutable: false,
        });
        let (prompts, tools) = reg.progressive_context(&["demo".into()]);
        assert_eq!(tools.len(), 2);
        let block = &prompts[0];
        assert!(block.contains("<available_skills>"));
        assert!(block.contains("<name>demo</name>"));
        assert!(block.contains("<location>/tmp/demo-skill/SKILL.md</location>"));
        assert!(block.contains("parent of `<location>`"));
    }

    #[test]
    fn read_resource_reads_from_disk_without_cached_whitelist() {
        let dir = tempfile::tempdir().expect("tempdir");
        let skill_dir = dir.path().join("demo-skill");
        fs::create_dir_all(skill_dir.join("references")).expect("mkdir");
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: demo-skill\ndescription: d\n---\nbody\n",
        )
        .expect("write manifest");
        fs::write(
            skill_dir.join("references/new.md"),
            "fresh reference content",
        )
        .expect("write reference");

        let reg = SkillRegistry::new();
        reg.register(SkillDef {
            id: "demo-skill".into(),
            name: "demo-skill".into(),
            description: "d".into(),
            tags: vec![],
            system_prompt: "body".into(),
            tool_names: vec![],
            scenario: String::new(),
            builtin: false,
            resource_files: vec![],
            source: Some(skill_dir.to_string_lossy().into_owned()),
            provenance: "user".into(),
            mutable: true,
        });

        let out = reg
            .read("demo-skill", "references/new.md")
            .expect("read resource");
        assert!(out.contains("fresh reference content"));
    }

    #[test]
    fn read_resource_rejects_path_outside_skill_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        let skill_dir = dir.path().join("demo-skill");
        fs::create_dir_all(&skill_dir).expect("mkdir");
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: demo-skill\ndescription: d\n---\nbody\n",
        )
        .expect("write manifest");

        let reg = SkillRegistry::new();
        reg.register(SkillDef {
            id: "demo-skill".into(),
            name: "demo-skill".into(),
            description: "d".into(),
            tags: vec![],
            system_prompt: "body".into(),
            tool_names: vec![],
            scenario: String::new(),
            builtin: false,
            resource_files: vec![],
            source: Some(skill_dir.to_string_lossy().into_owned()),
            provenance: "user".into(),
            mutable: true,
        });

        let err = reg
            .read("demo-skill", "../outside.md")
            .expect_err("traversal");
        assert!(err.to_string().contains("非法"));
    }

    #[test]
    fn load_instructions_reads_skill_md_from_disk_not_cache() {
        let dir = tempfile::tempdir().expect("tempdir");
        let skill_dir = dir.path().join("live-skill");
        fs::create_dir_all(&skill_dir).expect("mkdir");
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: live-skill\ndescription: d\n---\nversion one\n",
        )
        .expect("write v1");

        let reg = SkillRegistry::new();
        reg.register(SkillDef {
            id: "live-skill".into(),
            name: "live-skill".into(),
            description: "d".into(),
            tags: vec![],
            system_prompt: "stale cached body".into(),
            tool_names: vec![],
            scenario: String::new(),
            builtin: false,
            resource_files: vec![],
            source: Some(skill_dir.to_string_lossy().into_owned()),
            provenance: "user".into(),
            mutable: true,
        });

        let out1 = reg.read("live-skill", "SKILL.md").expect("read v1");
        assert!(out1.contains("version one"));
        assert!(!out1.contains("stale cached body"));

        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: live-skill\ndescription: d\n---\nversion two\n",
        )
        .expect("write v2");
        let out2 = reg.read("live-skill", "SKILL.md").expect("read v2");
        assert!(out2.contains("version two"));
        assert!(!out2.contains("version one"));
    }

    #[test]
    fn load_instructions_substitutes_base_dir_placeholder() {
        let dir = tempfile::tempdir().expect("tempdir");
        let skill_dir = dir.path().join("vid");
        fs::create_dir_all(skill_dir.join("scripts")).expect("mkdir");
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: vid\ndescription: d\n---\nRun {baseDir}/scripts/frame.sh\n",
        )
        .expect("write manifest");

        let reg = SkillRegistry::new();
        reg.register(SkillDef {
            id: "vid".into(),
            name: "vid".into(),
            description: "d".into(),
            tags: vec![],
            system_prompt: String::new(),
            tool_names: vec![],
            scenario: String::new(),
            builtin: false,
            resource_files: vec![],
            source: Some(skill_dir.to_string_lossy().into_owned()),
            provenance: "system".into(),
            mutable: false,
        });
        let out = reg.read("vid", "SKILL.md").expect("read");
        let expected = format!("{}/scripts/frame.sh", skill_dir.display());
        assert!(out.contains(&expected), "expected {expected} in {out}");
    }

    #[test]
    fn read_requires_non_empty_path() {
        let reg = SkillRegistry::new();
        reg.register(SkillDef {
            id: "vid".into(),
            name: "vid".into(),
            description: "d".into(),
            tags: vec![],
            system_prompt: "body".into(),
            tool_names: vec![],
            scenario: String::new(),
            builtin: false,
            resource_files: vec![],
            source: Some("/opt/skills/vid".into()),
            provenance: "system".into(),
            mutable: false,
        });
        let err = reg.read("vid", "  ").expect_err("empty path");
        assert!(err.to_string().contains("缺少 path"));
    }

    #[test]
    fn default_enabled_includes_all_repo_bundled_skills() {
        let skills_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills");
        let mut dirs: Vec<String> = fs::read_dir(&skills_dir)
            .expect("skills dir")
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .filter_map(|p| p.file_name().and_then(|n| n.to_str()).map(str::to_string))
            .filter(|name| !name.starts_with('.'))
            .collect();
        dirs.sort();
        let mut expected: Vec<String> =
            BUNDLED_SKILL_IDS.iter().map(|s| (*s).to_string()).collect();
        expected.sort();
        assert_eq!(
            dirs, expected,
            "update BUNDLED_SKILL_IDS and frontend DEFAULT_ENABLED_SKILL_IDS when adding skills/"
        );
        assert_eq!(DEFAULT_ENABLED_SKILL_IDS, BUNDLED_SKILL_IDS);
    }
}
