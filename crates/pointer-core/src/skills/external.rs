use crate::models::{SkillDef, SkillImportResult};
use crate::storage;
use anyhow::{anyhow, Context, Result};
use serde::{de, Deserialize, Deserializer};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Cursor, Read};
use std::path::{Component, Path, PathBuf};
use zip::ZipArchive;

const MAX_ZIP_SIZE: usize = 20 * 1024 * 1024;
const MAX_ENTRY_SIZE: u64 = 5 * 1024 * 1024;
const LEGACY_SKILLS_DIR: &str = "skills";
const AGENTS_SKILLS_DIR: &str = ".agents/skills";

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

/// Pointer home (`~/.pointer`), shared across app installs.
pub fn pointer_home_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| anyhow!("无法解析用户主目录"))?;
    let dir = home.join(".pointer");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Curator-managed skill library (`~/.pointer/skills`).
pub fn pointer_skills_dir() -> Result<PathBuf> {
    let dir = pointer_home_dir()?.join("skills");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// System bundled skills (`{data_dir}/PointerApp/skills/`), immutable via manifest.
pub fn system_skills_dir() -> Result<PathBuf> {
    let dir = storage::app_data_dir()?.join(LEGACY_SKILLS_DIR);
    fs::create_dir_all(&dir)?;
    migrate_system_skills_layout_if_needed()?;
    Ok(dir)
}

/// User-managed imports (`~/.pointer/skills`).
pub fn skills_dir() -> Result<PathBuf> {
    pointer_skills_dir()
}

/// Inject `SKILL_DIR` into a terminal child env map (user skill library root).
/// Same directory as `~/.pointer/skills`; shared across app installs.
pub fn apply_skill_dir_env(env: &mut HashMap<String, String>) {
    match pointer_skills_dir() {
        Ok(dir) => {
            let path = dir.to_string_lossy();
            if !path.is_empty() {
                env.insert("SKILL_DIR".into(), path.into_owned());
            }
        }
        Err(e) => {
            log::warn!("SKILL_DIR: skills dir unavailable for terminal child: {e:#}");
        }
    }
}

/// Codex / Agent standard user skill library (`~/.agents/skills`).
pub fn home_agents_skills_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(AGENTS_SKILLS_DIR))
}

/// True when `path` is under `~/.agents/skills`.
pub fn is_agents_skills_path(path: &Path) -> bool {
    let Some(root) = home_agents_skills_dir() else {
        return false;
    };
    let root = root.canonicalize().unwrap_or(root);
    let canon = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    canon.starts_with(&root)
}

fn migrate_system_skills_layout_if_needed() -> Result<()> {
    let marker = storage::app_data_dir()?.join(".skills_system_layout_v2");
    if marker.exists() {
        return Ok(());
    }
    let system = storage::app_data_dir()?.join(LEGACY_SKILLS_DIR);
    if system.is_dir() {
        for entry in fs::read_dir(&system)? {
            let entry = entry?;
            let path = entry.path();
            if !path.is_dir() || is_ignored_skill_dir(path.file_name()) {
                continue;
            }
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if let Ok(hash) = super::provenance::skill_dir_hash(&path) {
                let _ = super::provenance::upsert_bundled_manifest_entry(&system, name, &hash);
            }
        }
    }
    fs::write(&marker, b"")?;
    Ok(())
}

pub fn is_skill_manifest_path(path: &str) -> bool {
    is_manifest_path(path)
}

fn is_manifest_basename(name: &str) -> bool {
    name.eq_ignore_ascii_case("SKILL.md") || name.eq_ignore_ascii_case("skill.md")
}

/// Copy bundled skill directories into `{data_dir}/skills/` with `.bundled_manifest`.
pub fn sync_bundled_skill_dirs(sources: &[PathBuf]) -> Result<Vec<String>> {
    let target_root = system_skills_dir()?;
    let mut installed = Vec::new();
    let mut seen = HashSet::new();

    for source_root in sources {
        if !source_root.exists() {
            continue;
        }
        for entry in fs::read_dir(source_root)? {
            let entry = entry?;
            let source = entry.path();
            if !source.is_dir() || is_ignored_skill_dir(source.file_name()) {
                continue;
            }
            let Some(name) = source.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if !seen.insert(name.to_string()) {
                continue;
            }
            let target = target_root.join(entry.file_name());
            if target.exists() {
                if let Ok(local_hash) = super::provenance::skill_dir_hash(&target) {
                    if let Some(origin) =
                        super::provenance::read_bundled_origin_hash(&target_root, name)
                    {
                        if origin != local_hash {
                            log::info!("bundled skill skipped (modified): {name}");
                            continue;
                        }
                        fs::remove_dir_all(&target)?;
                    } else {
                        continue;
                    }
                } else {
                    continue;
                }
            }
            copy_dir_recursive(&source, &target)?;
            if let Ok(hash) = super::provenance::skill_dir_hash(&target) {
                super::provenance::upsert_bundled_manifest_entry(&target_root, name, &hash)?;
            }
            installed.push(name.to_string());
            log::info!("bundled skill installed: {name}");
        }
    }
    Ok(installed)
}

/// Deb / FHS install path for bundled skills (see `scripts/build-server-deb.mjs`).
pub const DEB_SHARE_SKILLS_DIR: &str = "/usr/share/pointer-server/skills";

/// Bundled skill source directories for pointer-server deploy.
///
/// Search order:
/// 1. `POINTER_SERVER_SKILLS_DIR`
/// 2. `{exe_dir}/skills` (zip layout)
/// 3. `{exe_dir}/../../skills` (cargo `target/*/pointer-server` → repo `skills/`)
/// 4. `{cwd}/skills`
/// 5. `/usr/share/pointer-server/skills` (Linux `.deb` layout)
pub fn resolve_deploy_bundled_skill_sources() -> Vec<PathBuf> {
    let mut sources = Vec::new();
    let mut seen = HashSet::new();

    let mut push_dir = |path: PathBuf| {
        if !path.is_dir() {
            return;
        }
        let key = path.canonicalize().unwrap_or(path.clone());
        if seen.insert(key) {
            sources.push(path);
        }
    };

    if let Ok(raw) = std::env::var("POINTER_SERVER_SKILLS_DIR") {
        push_dir(PathBuf::from(raw.trim()));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            push_dir(parent.join("skills"));
            push_dir(parent.join("../../skills"));
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        push_dir(cwd.join("skills"));
    }
    // Linux deb: binary is `/usr/bin/pointer-server`, skills live under share/.
    push_dir(PathBuf::from(DEB_SHARE_SKILLS_DIR));

    sources
}

/// Sync default bundled skills into `{data_dir}/PointerApp/skills/` before registry load.
pub fn install_deploy_bundled_skills() -> Result<()> {
    let sources = resolve_deploy_bundled_skill_sources();
    if sources.is_empty() {
        log::warn!(
            "bundled skills: no source directory found (expected skills/ beside pointer-server)"
        );
        return Ok(());
    }
    let installed = sync_bundled_skill_dirs(&sources)?;
    if installed.is_empty() {
        log::info!(
            "bundled skills: all present under {}",
            system_skills_dir()?.display()
        );
    } else {
        log::info!("bundled skills: installed {:?}", installed);
    }
    Ok(())
}

fn copy_dir_recursive(source: &Path, target: &Path) -> Result<()> {
    fs::create_dir_all(target)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());
        if source_path.is_dir() {
            copy_dir_recursive(&source_path, &target_path)?;
        } else if source_path.is_file() {
            fs::copy(&source_path, &target_path)?;
        }
    }
    Ok(())
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

fn normalize_zip_entry_path(name: &str) -> String {
    let mut s = name.replace('\\', "/");
    while s.starts_with("./") {
        s = s[2..].to_string();
    }
    while s.starts_with('/') {
        s = s[1..].to_string();
    }
    if s.ends_with('/') && s.len() > 1 {
        s.pop();
    }
    s
}

fn zip_manifest_base(name: &str) -> PathBuf {
    let normalized = normalize_zip_entry_path(name);
    Path::new(&normalized)
        .parent()
        .map(|parent| PathBuf::from(normalize_zip_entry_path(&parent.to_string_lossy())))
        .unwrap_or_default()
}

fn zip_entry_relative_to_base(entry: &str, base: &Path) -> Option<PathBuf> {
    let entry_norm = normalize_zip_entry_path(entry);
    if entry_norm.is_empty() {
        return None;
    }
    let base_norm = normalize_zip_entry_path(&base.to_string_lossy());
    if base_norm.is_empty() {
        return Some(PathBuf::from(entry_norm));
    }
    if entry_norm == base_norm {
        return None;
    }
    let prefix = format!("{base_norm}/");
    if !entry_norm.starts_with(&prefix) {
        return None;
    }
    Some(PathBuf::from(&entry_norm[prefix.len()..]))
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
        let name = normalize_zip_entry_path(file.name());
        if is_manifest_path(&name) {
            let mut raw = String::new();
            file.read_to_string(&mut raw)?;
            let manifest = parse_skill_md(&raw).with_context(|| format!("解析 {} 失败", name))?;
            let base = zip_manifest_base(&name);
            manifests.push((manifest, base));
        }
    }

    if manifests.is_empty() {
        return Err(anyhow!("zip 中未找到 SKILL.md 或 skill.md"));
    }

    // Re-import replaces the user-library copy (same as directory import).
    // Skipping "already loaded" ids used to swallow auto_enable: the tool
    // returned skipped-only, dispatch never wrote agentSkillOverrides.
    let mut seen_in_zip = HashSet::new();
    let mut imported = Vec::new();
    let mut skipped = Vec::new();

    for (manifest, base) in manifests {
        if let Err(err) = validate_manifest(&manifest) {
            skipped.push(format!("{}: {err}", manifest.name));
            continue;
        }

        if base.file_name().and_then(|name| name.to_str()).is_none() {
            skipped.push(format!("{}: SKILL.md 必须位于 Skill 目录中", manifest.name));
            continue;
        }

        if !seen_in_zip.insert(manifest.name.clone()) {
            skipped.push(format!("{}: zip 中重复", manifest.name));
            continue;
        }

        let target = root.join(manifest.name.trim());
        if target.exists() {
            log::info!(
                "skill_import: replacing existing user-library skill {}",
                target.display()
            );
            fs::remove_dir_all(&target)?;
        }
        fs::create_dir_all(&target)?;

        extract_skill_dir(&mut archive, &base, &target)?;
        match manifest_to_skill(manifest, &target) {
            Ok(skill) => {
                super::provenance::mark_agent_created(&skill.id, Some("zip"));
                imported.push(skill);
            }
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
        return Err(anyhow!(
            "目录中未找到符合规范的 Skill（需含 SKILL.md 或 skill.md）"
        ));
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

pub fn discover_skill_dirs_in(root: &Path) -> Vec<PathBuf> {
    discover_skill_dirs(root)
}

pub fn install_skill_dir(source: &Path) -> Result<SkillDef> {
    install_skill_dir_internal(source, None)
}

pub fn install_skill_dir_with_provenance(source: &Path, imported_from: &str) -> Result<SkillDef> {
    install_skill_dir_internal(source, Some(imported_from))
}

fn install_skill_dir_internal(source: &Path, imported_from: Option<&str>) -> Result<SkillDef> {
    let preview = load_skill_from_dir(source)?;
    let target = skills_dir()?.join(&preview.id);
    let source_canon = source
        .canonicalize()
        .unwrap_or_else(|_| source.to_path_buf());
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
    let skill = load_skill_from_dir(&target)?;
    super::provenance::mark_agent_created(&skill.id, imported_from);
    Ok(skill)
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

/// Runtime discovery roots (first match wins per skill id).
///
/// Order: user library → `~/.agents/skills` → system bundled.
fn skill_roots() -> Result<Vec<PathBuf>> {
    let mut roots = vec![pointer_skills_dir()?];
    if let Some(dir) = home_agents_skills_dir() {
        roots.push(dir);
    }
    roots.push(system_skills_dir()?);
    Ok(roots)
}

/// Skip vendor/system skill buckets (e.g. Codex `.system`, Cursor `skills-cursor`).
pub fn is_ignored_skill_dir(name: Option<&std::ffi::OsStr>) -> bool {
    let Some(name) = name.and_then(|n| n.to_str()) else {
        return true;
    };
    if name.starts_with('.') {
        return true;
    }
    name.eq_ignore_ascii_case("skills-cursor") || name.eq_ignore_ascii_case(".archive")
}

pub fn load_skill_from_dir(dir: &Path) -> Result<SkillDef> {
    let manifest_path =
        manifest_path_in_dir(dir).ok_or_else(|| anyhow!("未找到 SKILL.md 或 skill.md"))?;

    let raw = fs::read_to_string(&manifest_path)?;
    let manifest = parse_skill_md(&raw)?;
    manifest_to_skill(manifest, dir)
}

fn manifest_to_skill(manifest: SkillManifest, dir: &Path) -> Result<SkillDef> {
    validate_manifest(&manifest)?;

    let id = manifest.name.clone();
    let provenance = super::provenance::resolve_provenance_for_path(&id, dir);
    let mutable = super::provenance::is_mutable(&id, dir);
    Ok(SkillDef {
        id,
        name: manifest.name,
        description: manifest.description.clone(),
        tags: merge_tags(&manifest.tags, &manifest.metadata),
        // Catalog metadata only: body and resource paths load on demand via skill_read.
        system_prompt: String::new(),
        tool_names: manifest.allowed_tools,
        scenario: manifest.description,
        builtin: false,
        // Do not walk the skill tree (scripts/, venv/, cases/ can be huge).
        resource_files: Vec::new(),
        source: Some(dir.to_string_lossy().to_string()),
        provenance: provenance.as_str().to_string(),
        mutable,
        plugin_id: None,
    })
}

/// Read skill name + markdown body from disk (no registry cache).
/// Used by `skill_read` so edits to `SKILL.md` take effect immediately.
pub fn read_skill_instructions_from_dir(dir: &Path) -> Result<(String, String)> {
    let manifest_path =
        manifest_path_in_dir(dir).ok_or_else(|| anyhow!("未找到 SKILL.md 或 skill.md"))?;
    let raw = fs::read_to_string(&manifest_path)
        .with_context(|| format!("无法读取 {}", manifest_path.display()))?;
    let manifest = parse_skill_md(&raw)?;
    validate_manifest(&manifest)?;
    Ok((manifest.name, manifest.body))
}

fn extract_skill_dir<R: Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
    base: &Path,
    target: &Path,
) -> Result<()> {
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let name = file.name();
        let Some(rel) = zip_entry_relative_to_base(name, base) else {
            continue;
        };
        if rel.as_os_str().is_empty() {
            continue;
        }
        let out_path = safe_join(target, &rel)?;

        if file.is_dir() {
            fs::create_dir_all(&out_path)?;
        } else {
            if file.size() > MAX_ENTRY_SIZE {
                return Err(anyhow!("zip 条目过大: {}", normalize_zip_entry_path(name)));
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

pub fn is_skill_package_dir(dir: &Path) -> bool {
    manifest_path_in_dir(dir).is_some()
}

pub fn manifest_path_in_dir(dir: &Path) -> Option<PathBuf> {
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
        .is_some_and(is_manifest_basename)
}

fn is_manifest_path(name: &str) -> bool {
    Path::new(name)
        .file_name()
        .and_then(|file_name| file_name.to_str())
        .is_some_and(is_manifest_basename)
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
    use std::fs;
    use std::path::Path;

    fn parse(raw: &str) -> SkillManifest {
        parse_skill_md(raw).unwrap()
    }

    // ── `>` folded block scalar ──

    #[test]
    fn deb_share_skills_dir_constant_matches_packaging() {
        assert_eq!(DEB_SHARE_SKILLS_DIR, "/usr/share/pointer-server/skills");
    }

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
        assert_eq!(m.description.trim(), "Line one.\nLine two.\nLine three.");
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
        let input =
            "---\nname: test-skill\ndescription: Files larger than >500MB are skipped.\n---\nbody";
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
        let m = parse_skill_md("---\nname: bad/name\ndescription: d.\n---\nbody").unwrap();
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
        assert_eq!(merge_tags(&m.tags, &m.metadata), vec!["dev", "api"]);
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
        let m = parse_skill_md("---\nname: my-skill\ndescription: \n---\nbody").unwrap();
        assert!(validate_manifest(&m).is_err());
    }

    #[test]
    fn load_external_skills_reads_home_agents_skills() {
        use std::fs;
        use std::sync::{Mutex, OnceLock};

        static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        let _guard = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();

        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        let skill_dir = home.join(".agents/skills/agents-demo");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: agents-demo\ndescription: From ~/.agents/skills.\n---\nDemo body\n",
        )
        .unwrap();

        let prev_home = std::env::var("HOME").ok();
        std::env::set_var("HOME", home);
        let loaded = load_external_skills().unwrap();
        if let Some(h) = prev_home {
            std::env::set_var("HOME", h);
        } else {
            std::env::remove_var("HOME");
        }

        let skill = loaded
            .iter()
            .find(|s| s.id == "agents-demo")
            .expect("agents-demo from ~/.agents/skills");
        assert_eq!(skill.provenance, "external");
        assert!(!skill.mutable);
    }

    #[test]
    fn normalize_zip_entry_path_strips_leading_dot_slash() {
        assert_eq!(normalize_zip_entry_path("./pdf/SKILL.md"), "pdf/SKILL.md");
        assert_eq!(
            normalize_zip_entry_path("pdf/scripts/run.py"),
            "pdf/scripts/run.py"
        );
    }

    #[test]
    fn zip_entry_relative_handles_mixed_prefix() {
        let base = PathBuf::from("pdf");
        assert_eq!(
            zip_entry_relative_to_base("pdf/scripts/run.py", &base)
                .map(|p| p.to_string_lossy().into_owned()),
            Some("scripts/run.py".to_string())
        );
        assert_eq!(
            zip_entry_relative_to_base("./pdf/scripts/run.py", &base)
                .map(|p| p.to_string_lossy().into_owned()),
            Some("scripts/run.py".to_string())
        );
        assert!(zip_entry_relative_to_base("other/scripts/run.py", &base).is_none());
    }

    #[test]
    fn copy_dir_all_preserves_scripts_tree() {
        use std::fs;

        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src-skill");
        let dst = tmp.path().join("dst-skill");
        fs::create_dir_all(src.join("scripts")).unwrap();
        fs::create_dir_all(src.join("references")).unwrap();
        fs::write(src.join("SKILL.md"), "manifest").unwrap();
        fs::write(src.join("scripts/run.sh"), "#!/bin/sh\n").unwrap();
        fs::write(src.join("references/guide.md"), "guide").unwrap();

        copy_dir_all(&src, &dst).expect("copy_dir_all");

        assert!(dst.join("scripts/run.sh").is_file());
        assert!(dst.join("references/guide.md").is_file());
        assert!(dst.join("SKILL.md").is_file());
    }

    fn with_temp_home<T>(f: impl FnOnce(&Path) -> T) -> T {
        use std::sync::{Mutex, OnceLock};

        static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        let _guard = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        let prev_home = std::env::var("HOME").ok();
        let prev_profile = std::env::var("USERPROFILE").ok();
        std::env::set_var("HOME", home);
        std::env::set_var("USERPROFILE", home);
        let out = f(home);
        if let Some(h) = prev_home {
            std::env::set_var("HOME", h);
        } else {
            std::env::remove_var("HOME");
        }
        if let Some(p) = prev_profile {
            std::env::set_var("USERPROFILE", p);
        } else {
            std::env::remove_var("USERPROFILE");
        }
        out
    }

    fn zip_skill_bytes(name: &str, body: &str, extra: &[(&str, &[u8])]) -> Vec<u8> {
        use std::io::Write;
        use zip::write::{SimpleFileOptions, ZipWriter};
        use zip::CompressionMethod;

        let manifest = format!("---\nname: {name}\ndescription: Zip import demo.\n---\n{body}\n");
        let mut zip_bytes = Vec::new();
        {
            let mut zip = ZipWriter::new(Cursor::new(&mut zip_bytes));
            let options =
                SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
            zip.start_file(format!("{name}/SKILL.md"), options).unwrap();
            zip.write_all(manifest.as_bytes()).unwrap();
            for (rel, content) in extra {
                zip.start_file(format!("{name}/{rel}"), options).unwrap();
                zip.write_all(content).unwrap();
            }
            zip.finish().unwrap();
        }
        zip_bytes
    }

    #[test]
    fn import_skill_zip_extracts_whole_skill_directory() {
        let zip_bytes = zip_skill_bytes(
            "zip-pack",
            "Body",
            &[
                ("scripts/helper.py", b"print('hi')\n"),
                ("assets/icon.svg", b"<svg/>"),
            ],
        );
        with_temp_home(|home| {
            let result = import_skill_zip(&zip_bytes).expect("import zip");
            let target = home.join(".pointer/skills/zip-pack");
            assert_eq!(result.imported.len(), 1);
            assert_eq!(result.imported[0].id, "zip-pack");
            assert!(target.join("scripts/helper.py").is_file());
            assert!(target.join("assets/icon.svg").is_file());
            let script = fs::read_to_string(target.join("scripts/helper.py")).unwrap();
            assert!(script.contains("print('hi')"));
        });
    }

    #[test]
    fn import_skill_zip_reimport_replaces_and_counts_as_imported() {
        let first = zip_skill_bytes("zip-pack", "v1", &[("note.txt", b"old")]);
        let second = zip_skill_bytes("zip-pack", "v2", &[("note.txt", b"new")]);
        with_temp_home(|home| {
            let first_result = import_skill_zip(&first).expect("first import");
            let second_result = import_skill_zip(&second).expect("re-import");
            let note = fs::read_to_string(home.join(".pointer/skills/zip-pack/note.txt"))
                .expect("replaced file");
            assert_eq!(first_result.imported.len(), 1);
            assert!(first_result.skipped.is_empty());
            assert_eq!(second_result.imported.len(), 1);
            assert_eq!(second_result.imported[0].id, "zip-pack");
            assert!(
                second_result.skipped.is_empty(),
                "re-import must not skip already-on-disk ids (auto_enable reads imported): {:?}",
                second_result.skipped
            );
            assert_eq!(note, "new");
        });
    }

    #[test]
    fn import_skill_dir_already_in_user_library_counts_as_imported() {
        with_temp_home(|home| {
            let dir = home.join(".pointer/skills/direct-pack");
            fs::create_dir_all(&dir).unwrap();
            fs::write(
                dir.join("SKILL.md"),
                "---\nname: direct-pack\ndescription: Already on disk.\n---\nBody\n",
            )
            .unwrap();
            let result = import_skill_path(&dir).expect("import existing library dir");
            assert_eq!(result.imported.len(), 1);
            assert_eq!(result.imported[0].id, "direct-pack");
            assert!(
                result.skipped.is_empty(),
                "same-path library dir must count as imported so auto_enable can run: {:?}",
                result.skipped
            );
        });
    }

    #[test]
    fn catalog_load_skips_resource_file_index() {
        let dir = tempfile::tempdir().unwrap();
        let skill_dir = dir.path().join("fat-skill");
        fs::create_dir_all(skill_dir.join("scripts/venv/lib")).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: fat-skill\ndescription: Has many files under scripts.\n---\nBody\n",
        )
        .unwrap();
        for i in 0..200 {
            fs::write(
                skill_dir.join(format!("scripts/venv/lib/mod_{i}.py")),
                "# junk\n",
            )
            .unwrap();
        }
        let skill = load_skill_from_dir(&skill_dir).expect("load fat skill");
        assert_eq!(skill.id, "fat-skill");
        assert!(
            skill.resource_files.is_empty(),
            "catalog must not index skill-tree files"
        );
        assert!(skill.system_prompt.is_empty());
        let json = serde_json::to_string(&skill).unwrap();
        assert!(
            !json.contains("resourceFiles"),
            "empty resourceFiles should be omitted from JSON: {json}"
        );
        assert!(
            !json.contains("systemPrompt"),
            "empty systemPrompt should be omitted from JSON: {json}"
        );
    }
}
