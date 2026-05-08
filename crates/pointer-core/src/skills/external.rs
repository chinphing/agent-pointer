use crate::models::{SkillDef, SkillImportResult};
use crate::storage;
use anyhow::{anyhow, Context, Result};
use serde::{de, Deserialize, Deserializer};
use std::collections::HashSet;
use std::env;
use std::fs;
use std::io::{Cursor, Read};
use std::path::{Component, Path, PathBuf};
use zip::ZipArchive;

const SKILLS_DIR: &str = "skills";
const MAX_ZIP_SIZE: usize = 20 * 1024 * 1024;
const MAX_ENTRY_SIZE: u64 = 5 * 1024 * 1024;

#[derive(Debug, Deserialize)]
struct SkillManifest {
    name: String,
    description: String,
    #[serde(default)]
    license: Option<String>,
    #[serde(default)]
    compatibility: Option<String>,
    #[serde(default, rename = "allowed-tools", deserialize_with = "deserialize_allowed_tools")]
    allowed_tools: Vec<String>,
    #[serde(default)]
    metadata: serde_json::Value,
    #[serde(skip)]
    body: String,
}

pub fn skills_dir() -> Result<PathBuf> {
    let dir = storage::app_data_dir()?.join(SKILLS_DIR);
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn load_external_skills() -> Result<Vec<SkillDef>> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();

    for root in skill_roots()? {
        if !root.exists() {
            continue;
        }
        for entry in fs::read_dir(&root)? {
            let entry = entry?;
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            if let Ok(skill) = load_skill_from_dir(&path) {
                if seen.insert(skill.id.clone()) {
                    out.push(skill);
                }
            }
        }
    }

    Ok(out)
}

pub fn import_skill_zip(bytes: &[u8]) -> Result<SkillImportResult> {
    if bytes.len() > MAX_ZIP_SIZE {
        return Err(anyhow!("Skills zip 文件过大，最大支持 20MB"));
    }

    let root = skills_dir()?;
    let mut archive = ZipArchive::new(Cursor::new(bytes)).context("无法读取 Skills zip 文件")?;
    let mut manifests = Vec::new();

    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        if file.is_dir() || file.size() > MAX_ENTRY_SIZE {
            continue;
        }
        let name = file.name().replace('\\', "/");
        if is_manifest_path(&name) {
            let mut raw = String::new();
            file.read_to_string(&mut raw)?;
            let manifest = parse_skill_md(&raw).with_context(|| format!("解析 {} 失败", name))?;
            let base = Path::new(&name).parent().unwrap_or_else(|| Path::new(""));
            manifests.push((manifest, base.to_path_buf()));
        }
    }

    if manifests.is_empty() {
        return Err(anyhow!("zip 中未找到官方规范要求的 SKILL.md"));
    }

    let mut imported = Vec::new();
    let mut skipped = Vec::new();

    for (manifest, base) in manifests {
        if let Err(err) = validate_manifest(&manifest) {
            skipped.push(format!("{}: {err}", manifest.name));
            continue;
        }

        let Some(dir_name) = base.file_name().and_then(|name| name.to_str()) else {
            skipped.push(format!("{}: SKILL.md 必须位于 kebab-case Skill 目录中", manifest.name));
            continue;
        };
        if dir_name != manifest.name {
            skipped.push(format!(
                "{}: Skill 目录名必须与 frontmatter name 一致",
                manifest.name
            ));
            continue;
        }

        let target = root.join(&manifest.name);
        if target.exists() {
            fs::remove_dir_all(&target)?;
        }
        fs::create_dir_all(&target)?;

        extract_skill_dir(&mut archive, &base, &target)?;
        match manifest_to_skill(manifest, &target) {
            Ok(skill) => imported.push(skill),
            Err(err) => {
                let _ = fs::remove_dir_all(&target);
                skipped.push(err.to_string());
            }
        }
    }

    Ok(SkillImportResult { imported, skipped })
}

fn skill_roots() -> Result<Vec<PathBuf>> {
    let mut roots = Vec::new();
    if let Ok(cwd) = env::current_dir() {
        roots.push(cwd.join("skills"));
        roots.push(cwd.join(".agents").join("skills"));
    }
    if let Some(home) = dirs::home_dir() {
        roots.push(home.join(".agents").join("skills"));
    }
    roots.push(skills_dir()?);
    Ok(roots)
}

fn load_skill_from_dir(dir: &Path) -> Result<SkillDef> {
    if !is_kebab_case_dir(dir) {
        return Err(anyhow!("Skill 目录名必须使用 kebab-case: {}", dir.display()));
    }

    let manifest_path = dir.join("SKILL.md");
    if !manifest_path.exists() {
        return Err(anyhow!("未找到官方规范要求的 SKILL.md"));
    }

    if dir.join("README.md").exists() {
        return Err(anyhow!("Skill 目录不应包含 README.md，请将说明写入 SKILL.md"));
    }

    let raw = fs::read_to_string(&manifest_path)?;
    let manifest = parse_skill_md(&raw)?;
    let dir_name = dir
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow!("Skill 目录名无效"))?;
    if manifest.name != dir_name {
        return Err(anyhow!("Skill 目录名必须与 frontmatter name 一致"));
    }
    manifest_to_skill(manifest, dir)
}

fn manifest_to_skill(manifest: SkillManifest, dir: &Path) -> Result<SkillDef> {
    validate_manifest(&manifest)?;

    let id = manifest.name.clone();
    Ok(SkillDef {
        id,
        name: manifest.name,
        description: manifest.description.clone(),
        tags: metadata_tags(&manifest.metadata),
        system_prompt: manifest.body,
        tool_names: manifest.allowed_tools,
        scenario: manifest.description,
        builtin: false,
        resource_files: collect_resource_files(dir)?,
        source: Some(dir.to_string_lossy().to_string()),
    })
}

fn extract_skill_dir<R: Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
    base: &Path,
    target: &Path,
) -> Result<()> {
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let name = file.name().replace('\\', "/");
        let path = Path::new(&name);
        if !base.as_os_str().is_empty() && !path.starts_with(base) {
            continue;
        }

        let rel = if base.as_os_str().is_empty() {
            path
        } else {
            path.strip_prefix(base)?
        };
        if rel.as_os_str().is_empty() {
            continue;
        }
        let out_path = safe_join(target, rel)?;

        if file.is_dir() {
            fs::create_dir_all(&out_path)?;
        } else {
            if file.size() > MAX_ENTRY_SIZE {
                return Err(anyhow!("zip 条目过大: {}", name));
            }
            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut out = fs::File::create(&out_path)?;
            std::io::copy(&mut file, &mut out)?;
        }
    }
    Ok(())
}

fn parse_skill_md(raw: &str) -> Result<SkillManifest> {
    let text = raw.trim_start_matches('\u{feff}');
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some("---") {
        return Err(anyhow!("SKILL.md 缺少 YAML frontmatter"));
    }

    let mut yaml = Vec::new();
    let mut body = Vec::new();
    let mut in_body = false;
    for line in lines {
        if !in_body && line.trim() == "---" {
            in_body = true;
            continue;
        }
        if in_body {
            body.push(line);
        } else {
            yaml.push(line);
        }
    }
    if !in_body {
        return Err(anyhow!("SKILL.md frontmatter 未闭合"));
    }

    let yaml_text = yaml.join("\n");
    if contains_angle_brackets(&yaml_text) {
        return Err(anyhow!("SKILL.md frontmatter 不允许包含 < 或 >"));
    }

    let mut manifest: SkillManifest = serde_yaml::from_str(&yaml_text)?;
    manifest.body = body.join("\n").trim().to_string();
    Ok(manifest)
}

fn deserialize_allowed_tools<'de, D>(deserializer: D) -> std::result::Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<serde_yaml::Value>::deserialize(deserializer)?;
    let Some(value) = value else {
        return Ok(Vec::new());
    };

    match value {
        serde_yaml::Value::String(s) => Ok(s.split_whitespace().map(str::to_string).collect()),
        serde_yaml::Value::Sequence(items) => items
            .into_iter()
            .map(|item| match item {
                serde_yaml::Value::String(s) => Ok(s),
                _ => Err(de::Error::custom("allowed-tools 只能是字符串或字符串数组")),
            })
            .collect(),
        _ => Err(de::Error::custom("allowed-tools 只能是字符串或字符串数组")),
    }
}

fn validate_manifest(manifest: &SkillManifest) -> Result<()> {
    if !is_kebab_case(&manifest.name) {
        return Err(anyhow!("name 必须是 kebab-case"));
    }
    if manifest.name.contains("claude") || manifest.name.contains("anthropic") {
        return Err(anyhow!("name 不允许包含 claude 或 anthropic"));
    }
    if manifest.description.trim().is_empty() {
        return Err(anyhow!("description 不能为空"));
    }
    if manifest.description.chars().count() > 1024 {
        return Err(anyhow!("description 不能超过 1024 个字符"));
    }
    if contains_angle_brackets(&manifest.name)
        || contains_angle_brackets(&manifest.description)
        || manifest
            .license
            .as_deref()
            .is_some_and(contains_angle_brackets)
        || manifest
            .compatibility
            .as_deref()
            .is_some_and(contains_angle_brackets)
    {
        return Err(anyhow!("frontmatter 字段不允许包含 < 或 >"));
    }
    if let Some(compatibility) = &manifest.compatibility {
        let len = compatibility.chars().count();
        if len == 0 || len > 500 {
            return Err(anyhow!("compatibility 长度必须在 1 到 500 个字符之间"));
        }
    }
    if manifest.allowed_tools.iter().any(|tool| tool.trim().is_empty()) {
        return Err(anyhow!("allowed-tools 不允许包含空项"));
    }
    Ok(())
}

fn metadata_tags(metadata: &serde_json::Value) -> Vec<String> {
    metadata
        .get("tags")
        .and_then(|tags| tags.as_array())
        .map(|tags| {
            tags.iter()
                .filter_map(|tag| tag.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn contains_angle_brackets(value: &str) -> bool {
    value.contains('<') || value.contains('>')
}

fn is_kebab_case_dir(dir: &Path) -> bool {
    dir.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(is_kebab_case)
}

fn is_kebab_case(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes[0] == b'-' || bytes[bytes.len() - 1] == b'-' {
        return false;
    }

    let mut prev_dash = false;
    for &byte in bytes {
        let valid = byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-';
        if !valid {
            return false;
        }
        if byte == b'-' {
            if prev_dash {
                return false;
            }
            prev_dash = true;
        } else {
            prev_dash = false;
        }
    }
    true
}

fn collect_resource_files(dir: &Path) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for root_name in ["references", "assets", "scripts"] {
        let root = dir.join(root_name);
        if root.exists() {
            collect_resource_files_inner(dir, &root, &mut out)?;
        }
    }
    out.sort();
    Ok(out)
}

fn collect_resource_files_inner(base: &Path, dir: &Path, out: &mut Vec<String>) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_resource_files_inner(base, &path, out)?;
        } else if path.is_file() {
            let rel = path
                .strip_prefix(base)?
                .to_string_lossy()
                .replace('\\', "/");
            out.push(rel);
        }
    }
    Ok(())
}

fn is_manifest_path(name: &str) -> bool {
    Path::new(name)
        .file_name()
        .and_then(|file_name| file_name.to_str())
        == Some("SKILL.md")
}

fn safe_join(root: &Path, rel: &Path) -> Result<PathBuf> {
    let mut out = root.to_path_buf();
    for c in rel.components() {
        match c {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            _ => return Err(anyhow!("非法路径: {}", rel.display())),
        }
    }
    Ok(out)
}
