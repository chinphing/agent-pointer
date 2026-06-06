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
    /// Codex / Claude Code / Agent Zero often use `allowed_tools`; Cursor uses `allowed-tools`.
    #[serde(
        default,
        rename = "allowed-tools",
        alias = "allowed_tools",
        deserialize_with = "deserialize_allowed_tools"
    )]
    allowed_tools: Vec<String>,
    /// Top-level `tags` (Agent Zero / Claude); also accepts `metadata.tags`.
    #[serde(default)]
    tags: Vec<String>,
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
            if !path.is_dir() || is_ignored_skill_dir(path.file_name()) {
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

/// Import from a `.zip` file or a skill directory (single skill dir or parent of many).
pub fn import_skill_path(source: &Path) -> Result<SkillImportResult> {
    if !source.exists() {
        return Err(anyhow!("路径不存在: {}", source.display()));
    }
    if source.is_file() {
        return import_skill_zip_file(source);
    }
    import_skill_dir(source)
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
        return Err(anyhow!("zip 中未找到 SKILL.md 或 skill.md"));
    }

    let mut imported = Vec::new();
    let mut skipped = Vec::new();

    for (manifest, base) in manifests {
        if let Err(err) = validate_manifest(&manifest) {
            skipped.push(format!("{}: {err}", manifest.name));
            continue;
        }

        if base.file_name().and_then(|name| name.to_str()).is_none() {
            skipped.push(format!(
                "{}: SKILL.md 必须位于 Skill 目录中",
                manifest.name
            ));
            continue;
        }
        let target = root.join(manifest.name.trim());
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

fn import_skill_zip_file(path: &Path) -> Result<SkillImportResult> {
    if !is_zip_file(path) {
        return Err(anyhow!(
            "不支持的文件类型: {}（请提供 .zip 或技能目录）",
            path.display()
        ));
    }
    let bytes = fs::read(path).with_context(|| format!("无法读取文件: {}", path.display()))?;
    import_skill_zip(&bytes)
}

fn import_skill_dir(source: &Path) -> Result<SkillImportResult> {
    let mut imported = Vec::new();
    let mut skipped = Vec::new();
    let mut seen = HashSet::new();

    for skill_dir in discover_skill_dirs(source) {
        let key = skill_dir
            .canonicalize()
            .unwrap_or_else(|_| skill_dir.clone())
            .to_string_lossy()
            .to_string();
        if !seen.insert(key) {
            continue;
        }
        match install_skill_dir(&skill_dir) {
            Ok(skill) => imported.push(skill),
            Err(err) => skipped.push(format!("{}: {err}", skill_dir.display())),
        }
    }

    if imported.is_empty() && skipped.is_empty() {
        return Err(anyhow!("目录中未找到符合规范的 Skill（需含 SKILL.md 或 skill.md）"));
    }
    Ok(SkillImportResult { imported, skipped })
}

fn discover_skill_dirs(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if manifest_path_in_dir(root).is_some() {
        out.push(root.to_path_buf());
        return out;
    }
    for entry in walkdir::WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .flatten()
    {
        let path = entry.path();
        if !path.is_file() || !is_manifest_file(path) {
            continue;
        }
        if let Some(parent) = path.parent() {
            out.push(parent.to_path_buf());
        }
    }
    out.sort_by_key(|p| p.components().count());
    out.dedup();
    out
}

fn install_skill_dir(source: &Path) -> Result<SkillDef> {
    let preview = load_skill_from_dir(source)?;
    let target = skills_dir()?.join(&preview.id);
    let source_canon = source.canonicalize().unwrap_or_else(|_| source.to_path_buf());
    if target.exists() {
        let target_canon = target.canonicalize().unwrap_or_else(|_| target.clone());
        if source_canon == target_canon {
            log::info!(
                "skill_import: path already installed at {}",
                target.display()
            );
            return load_skill_from_dir(&target);
        }
        fs::remove_dir_all(&target)?;
    }
    fs::create_dir_all(&target.parent().unwrap_or(&target))?;
    copy_dir_all(source, &target)?;
    load_skill_from_dir(&target)
}

fn copy_dir_all(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)?;
    for entry in walkdir::WalkDir::new(src) {
        let entry = entry?;
        let path = entry.path();
        let rel = path.strip_prefix(src)?;
        if rel.as_os_str().is_empty() {
            continue;
        }
        let target = dst.join(rel);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target)?;
        } else {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(path, &target)?;
        }
    }
    Ok(())
}

fn is_zip_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("zip"))
}

/// Discovery roots for Codex / Claude Code / Cursor compatible skills (first match wins per id).
fn skill_roots() -> Result<Vec<PathBuf>> {
    let mut roots = Vec::new();

    // App imports (Pointer UI / skill_import) — highest priority.
    roots.push(skills_dir()?);

    if let Ok(cwd) = env::current_dir() {
        roots.push(cwd.join(".cursor").join("skills"));
        roots.push(cwd.join(".claude").join("skills"));
        roots.push(cwd.join(".agents").join("skills"));
        roots.push(cwd.join("skills"));
    }

    if let Some(home) = dirs::home_dir() {
        roots.push(home.join(".cursor").join("skills"));
        roots.push(home.join(".claude").join("skills"));
        roots.push(home.join(".agents").join("skills"));

        let codex_home = env::var("CODEX_HOME")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".codex"));
        roots.push(codex_home.join("skills"));
    }

    Ok(roots)
}

/// Skip vendor/system skill buckets (e.g. Codex `.system`, Cursor `skills-cursor`).
fn is_ignored_skill_dir(name: Option<&std::ffi::OsStr>) -> bool {
    let Some(name) = name.and_then(|n| n.to_str()) else {
        return true;
    };
    if name.starts_with('.') {
        return true;
    }
    name.eq_ignore_ascii_case("skills-cursor")
}

fn load_skill_from_dir(dir: &Path) -> Result<SkillDef> {
    let manifest_path = manifest_path_in_dir(dir)
        .ok_or_else(|| anyhow!("未找到 SKILL.md 或 skill.md"))?;

    let raw = fs::read_to_string(&manifest_path)?;
    let manifest = parse_skill_md(&raw)?;
    manifest_to_skill(manifest, dir)
}

fn manifest_to_skill(manifest: SkillManifest, dir: &Path) -> Result<SkillDef> {
    validate_manifest(&manifest)?;

    let id = manifest.name.clone();
    Ok(SkillDef {
        id,
        name: manifest.name,
        description: manifest.description.clone(),
        tags: merge_tags(&manifest.tags, &manifest.metadata),
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
    validate_skill_id(&manifest.name)?;
    if manifest.description.trim().is_empty() {
        return Err(anyhow!("description 不能为空"));
    }
    if manifest.description.chars().count() > 1024 {
        return Err(anyhow!("description 不能超过 1024 个字符"));
    }

    if let Some(compatibility) = &manifest.compatibility {
        let len = compatibility.chars().count();
        if len == 0 || len > 500 {
            return Err(anyhow!("compatibility 长度必须在 1 到 500 个字符之间"));
        }
    }
    if let Some(license) = &manifest.license {
        let len = license.trim().chars().count();
        if len == 0 || len > 200 {
            return Err(anyhow!("license 长度必须在 1 到 200 个字符之间"));
        }
    }
    if manifest
        .allowed_tools
        .iter()
        .any(|tool| tool.trim().is_empty())
    {
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

fn merge_tags(top_level: &[String], metadata: &serde_json::Value) -> Vec<String> {
    let mut out: Vec<String> = top_level
        .iter()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect();
    for tag in metadata_tags(metadata) {
        if !out.iter().any(|existing| existing == &tag) {
            out.push(tag);
        }
    }
    out
}

fn validate_skill_id(id: &str) -> Result<()> {
    let id = id.trim();
    if id.is_empty() {
        return Err(anyhow!("name 不能为空"));
    }
    if id.chars().count() > 128 {
        return Err(anyhow!("name 不能超过 128 个字符"));
    }
    if id.contains('/') || id.contains('\\') {
        return Err(anyhow!("name 不能包含路径分隔符"));
    }
    Ok(())
}

fn manifest_path_in_dir(dir: &Path) -> Option<PathBuf> {
    for name in ["SKILL.md", "skill.md"] {
        let path = dir.join(name);
        if path.is_file() {
            return Some(path);
        }
    }
    fs::read_dir(dir).ok().and_then(|entries| {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() && is_manifest_file(&path) {
                return Some(path);
            }
        }
        None
    })
}

fn is_manifest_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|file_name| file_name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("SKILL.md"))
}

fn collect_resource_files(dir: &Path) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for entry in walkdir::WalkDir::new(dir).follow_links(false) {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            continue;
        }
        let rel = path
            .strip_prefix(dir)?
            .to_string_lossy()
            .replace('\\', "/");
        if is_manifest_file(path) {
            continue;
        }
        out.push(rel);
    }
    out.sort();
    Ok(out)
}

fn is_manifest_path(name: &str) -> bool {
    Path::new(name)
        .file_name()
        .and_then(|file_name| file_name.to_str())
        .is_some_and(|file| file.eq_ignore_ascii_case("SKILL.md"))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(raw: &str) -> SkillManifest {
        parse_skill_md(raw).unwrap()
    }

    // ── `>` folded block scalar ──

    #[test]
    fn folded_single_paragraph() {
        let input = "---\nname: test-skill\ndescription: >\n  Analyzes macOS disk storage.\n  Provides cleanup recommendations.\n---\nbody";
        let m = parse(input);
        assert_eq!(
            m.description.trim(),
            "Analyzes macOS disk storage. Provides cleanup recommendations."
        );
    }

    #[test]
    fn folded_multi_paragraph() {
        let input = "---\nname: test-skill\ndescription: >\n  First paragraph line one.\n  First paragraph line two.\n\n  Second paragraph.\n---\nbody";
        let m = parse(input);
        let desc = m.description.trim();
        assert!(desc.starts_with("First paragraph line one. First paragraph line two."));
        assert!(desc.contains("Second paragraph."));
    }

    #[test]
    fn folded_preserves_body() {
        let input = "---\nname: test-skill\ndescription: >\n  Some text.\n---\nline1\nline2";
        let m = parse(input);
        assert_eq!(m.body, "line1\nline2");
    }

    // ── `|` literal block scalar ──

    #[test]
    fn literal_preserves_newlines() {
        let input = "---\nname: test-skill\ndescription: |\n  Line one.\n  Line two.\n  Line three.\n---\nbody";
        let m = parse(input);
        assert_eq!(
            m.description.trim(),
            "Line one.\nLine two.\nLine three."
        );
    }

    #[test]
    fn literal_preserves_body() {
        let input = "---\nname: test-skill\ndescription: |\n  Multi\n  line.\n---\nbody text";
        let m = parse(input);
        assert_eq!(m.body, "body text");
    }

    // ── `<` and `>` in values ──

    #[test]
    fn angle_brackets_in_description_value() {
        let input = "---\nname: test-skill\ndescription: Files larger than >500MB are skipped.\n---\nbody";
        let m = parse(input);
        assert_eq!(
            m.description.trim(),
            "Files larger than >500MB are skipped."
        );
    }

    #[test]
    fn angle_brackets_in_name() {
        let input = "---\nname: test-skill<v2>\ndescription: desc.\n---\nbody";
        let m = parse(input);
        assert_eq!(m.name, "test-skill<v2>");
    }

    // ── 端到端: 真实技能 SKILL.md（`>` 折叠描述）──

    #[test]
    fn real_world_skill_with_folded_description() {
        let input = "---\nname: disk-storage-analyzer\ndescription: >\n  Analyzes macOS disk storage usage by category (Documents, Desktop, Pictures,\n  Movies, Downloads, Music). Identifies large chat-app caches (WeChat, WeCom),\n  Docker volumes, and cloud-sync folders. Provides cleanup recommendations with\n  estimated reclaimable space.\n---\n# Disk Storage Analyzer\n\nScan the user's home directory and categorize disk usage.\n\n## Workflow\n\n1. Run `du -sh ~/Documents ~/Desktop ~/Pictures ~/Movies ~/Downloads ~/Music`\n2. Identify top space consumers (>1 GiB)\n3. Suggest cleanup actions";
        let m = parse(input);
        assert_eq!(m.name, "disk-storage-analyzer");
        assert!(m.description.contains("Analyzes macOS disk storage usage"));
        assert!(m.description.contains("Provides cleanup recommendations"));
        // `>` folds newlines into spaces — not literal \n
        assert!(!m.description.contains("\n    Analyzes"));
        // body intact
        assert!(m.body.contains("# Disk Storage Analyzer"));
        assert!(m.body.contains("du -sh"));
    }

    // ── errors still work ──

    #[test]
    fn missing_frontmatter_is_error() {
        assert!(parse_skill_md("no frontmatter").is_err());
    }

    #[test]
    fn external_skill_id_allows_claude_prefix() {
        let m = parse_skill_md(
            "---\nname: claude-api\ndescription: Claude API integration.\n---\nbody",
        )
        .unwrap();
        assert!(validate_manifest(&m).is_ok());
    }

    #[test]
    fn external_skill_id_rejects_path_separator() {
        let m = parse_skill_md(
            "---\nname: bad/name\ndescription: d.\n---\nbody",
        )
        .unwrap();
        assert!(validate_manifest(&m).is_err());
    }

    #[test]
    fn parses_allowed_tools_underscore_alias() {
        let m = parse(
            "---\nname: codex-skill\ndescription: Demo.\nallowed_tools:\n  - Bash\n  - Read\n---\nbody",
        );
        assert_eq!(m.allowed_tools, vec!["Bash", "Read"]);
    }

    #[test]
    fn merges_top_level_and_metadata_tags() {
        let m = parse(
            "---\nname: claude-skill\ndescription: Demo.\ntags:\n  - dev\nmetadata:\n  tags:\n    - api\n---\nbody",
        );
        assert_eq!(
            merge_tags(&m.tags, &m.metadata),
            vec!["dev", "api"]
        );
    }

    #[test]
    fn ignores_dot_prefixed_skill_dirs() {
        use std::ffi::OsStr;
        assert!(is_ignored_skill_dir(Some(OsStr::new(".system"))));
        assert!(is_ignored_skill_dir(Some(OsStr::new(".hidden"))));
        assert!(!is_ignored_skill_dir(Some(OsStr::new("brainstorming"))));
        assert!(is_ignored_skill_dir(Some(OsStr::new("skills-cursor"))));
    }

    #[test]
    fn empty_description_is_error() {
        let m = parse_skill_md(
            "---\nname: my-skill\ndescription: \n---\nbody",
        )
        .unwrap();
        assert!(validate_manifest(&m).is_err());
    }
}
