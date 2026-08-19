//! 导入转换器：Codex / Claude 插件目录 → Pointer 原生格式（设计稿 §4.3）。
//!
//! 输入：Claude 插件目录（`.claude-plugin/plugin.json` + skills/agents/commands/hooks/.mcp.json）
//! 或 Codex 插件/Skills 目录；输出：写入 `~/.pointer/plugins/<id>/` 的完整原生插件
//! （生成 `pointer-plugin.toml`，拷贝/改写能力单元文件）。
//!
//! 原则：
//! - 原生格式是唯一运行时格式：导入是一次性快照，源目录后续变更**不自动同步**；
//! - 导入报告列出每个组件的转换结果（成功/跳过/未映射），用户确认后才落盘；
//! - 无法映射的字段保留在 `[metadata]` 段并在导入报告中标注「未映射」；
//! - 宽容解析：未知字段进 `[metadata]`，不因单字段失败而整体拒绝。

use anyhow::{anyhow, Context, Result};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Claude 插件 manifest 文件名。
const CLAUDE_PLUGIN_MANIFEST: &str = "plugin.json";
/// Claude 插件 manifest 所在子目录（`.claude-plugin/plugin.json`）。
const CLAUDE_PLUGIN_DIR: &str = ".claude-plugin";
/// Codex 插件 manifest 所在子目录（`.codex-plugin/plugin.json`）。
const CODEX_PLUGIN_DIR: &str = ".codex-plugin";
/// MCP 声明文件（`.mcp.json`，Claude 约定）。
const MCP_CONFIG_FILE: &str = ".mcp.json";
/// hooks 声明文件（`hooks/hooks.json`，Claude 约定）。
const HOOKS_FILE: &str = "hooks/hooks.json";

/// 插件平台类型（自动识别结果）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PluginKind {
    /// Pointer 原生（`pointer-plugin.toml`）。
    Pointer,
    /// Codex 插件（`.codex-plugin/plugin.json`）。
    Codex,
    /// Claude 插件（`.claude-plugin/plugin.json`）。
    Claude,
}

impl PluginKind {
    pub fn label(&self) -> &'static str {
        match self {
            PluginKind::Pointer => "Pointer 原生",
            PluginKind::Codex => "Codex",
            PluginKind::Claude => "Claude Code",
        }
    }
}

/// 在目录中发现的可导入插件包（供 UI 列出候选）。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredPlugin {
    pub kind: PluginKind,
    /// 插件根目录（能力单元所在）。
    pub root: PathBuf,
    /// manifest 文件路径。
    pub manifest: PathBuf,
    pub name: String,
    pub version: String,
    pub description: String,
}

/// 检测目录的插件类型与插件根。
/// 优先级（用户指定）：Pointer 原生 → Codex → Claude。
pub fn detect_plugin_package(dir: &Path) -> Result<Option<(PluginKind, PathBuf, PathBuf)>> {
    if !dir.is_dir() {
        return Ok(None);
    }
    // Pointer 原生
    let pointer_manifest = dir.join(crate::plugins::PLUGIN_MANIFEST_FILE);
    if pointer_manifest.is_file() {
        return Ok(Some((
            PluginKind::Pointer,
            dir.to_path_buf(),
            pointer_manifest,
        )));
    }
    // Codex
    let codex_manifest = dir.join(CODEX_PLUGIN_DIR).join(CLAUDE_PLUGIN_MANIFEST);
    if codex_manifest.is_file() {
        return Ok(Some((PluginKind::Codex, dir.to_path_buf(), codex_manifest)));
    }
    // Claude（含用户误选 .claude-plugin 内部的情况）
    let claude_manifest = dir.join(CLAUDE_PLUGIN_DIR).join(CLAUDE_PLUGIN_MANIFEST);
    if claude_manifest.is_file() {
        return Ok(Some((
            PluginKind::Claude,
            dir.to_path_buf(),
            claude_manifest,
        )));
    }
    let root_manifest = dir.join(CLAUDE_PLUGIN_MANIFEST);
    if root_manifest.is_file() {
        let dir_name = dir.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if dir_name == CLAUDE_PLUGIN_DIR {
            let parent = dir.parent().ok_or_else(|| anyhow!("无法解析插件根目录"))?;
            return Ok(Some((
                PluginKind::Claude,
                parent.to_path_buf(),
                root_manifest,
            )));
        }
        return Ok(Some((PluginKind::Claude, dir.to_path_buf(), root_manifest)));
    }
    Ok(None)
}

/// 扫描目录（含一层直接子目录），列出所有可导入插件包候选。
/// 每个候选按 kind 标注；同一目录多平台（如 superpowers 同时有 claude/codex）会各列一条。
pub fn discover_plugin_packages(dir: &Path) -> Result<Vec<DiscoveredPlugin>> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    let mut seen: std::collections::HashSet<(PluginKind, String)> =
        std::collections::HashSet::new();

    // 目录本身
    push_discovered(dir, &mut out, &mut seen)?;
    // 直接子目录（跳过隐藏/常见大目录）
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.starts_with('.') || name == "node_modules" || name == "target" || name == "dist"
            {
                continue;
            }
            push_discovered(&path, &mut out, &mut seen)?;
        }
    }

    // 按优先级排序：Pointer → Codex → Claude
    let rank = |k: PluginKind| match k {
        PluginKind::Pointer => 0,
        PluginKind::Codex => 1,
        PluginKind::Claude => 2,
    };
    out.sort_by_key(|p| (rank(p.kind), p.root.clone()));
    Ok(out)
}

fn push_discovered(
    dir: &Path,
    out: &mut Vec<DiscoveredPlugin>,
    seen: &mut std::collections::HashSet<(PluginKind, String)>,
) -> Result<()> {
    let Some((kind, root, manifest)) = detect_plugin_package(dir)? else {
        return Ok(());
    };
    let key = (kind, root.to_string_lossy().to_string());
    if !seen.insert(key) {
        return Ok(());
    }
    let raw = fs::read_to_string(&manifest).unwrap_or_default();
    let value: Value = serde_json::from_str(&raw).unwrap_or_else(|_| Value::Null);
    let (name, version, description) = match kind {
        PluginKind::Pointer => read_pointer_meta(&raw),
        _ => (
            value
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            value
                .get("version")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            value
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
        ),
    };
    out.push(DiscoveredPlugin {
        kind,
        root,
        manifest,
        name,
        version,
        description,
    });
    Ok(())
}

/// 从 pointer-plugin.toml 文本读取元信息（名称/版本/描述）。
fn read_pointer_meta(raw: &str) -> (String, String, String) {
    let parsed = toml::from_str::<serde_json::Value>(raw).ok();
    let meta = parsed.as_ref().and_then(|v| v.get("plugin"));
    (
        meta.and_then(|m| m.get("name"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        meta.and_then(|m| m.get("version"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        meta.and_then(|m| m.get("description"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
    )
}

/// 导入报告：每个组件的转换结果。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    /// 生成的插件 id（原生格式）。
    pub plugin_id: String,
    /// 插件名称（来自源 manifest）。
    pub plugin_name: String,
    /// 目标插件目录。
    pub target_dir: PathBuf,
    /// 成功转换的组件清单。
    pub converted: Vec<String>,
    /// 跳过（源不存在 / 空目录）的组件。
    pub skipped: Vec<String>,
    /// 无法映射的字段（保留在 `[metadata]`）。
    pub unmapped: Vec<String>,
}

/// 宽容解析 Claude `plugin.json` 的已知字段；未知字段进 `metadata`。
#[derive(Debug, Default)]
struct ClaudePlugin {
    name: String,
    version: String,
    description: String,
    author: String,
    license: String,
    homepage: String,
    repository: String,
    keywords: Vec<String>,
    /// 顶层已知键（用于区分「已消费」与「未映射」）。
    metadata: BTreeMap<String, Value>,
}

impl ClaudePlugin {
    fn from_json(raw: &Value) -> ClaudePlugin {
        let mut plugin = ClaudePlugin::default();
        let Some(obj) = raw.as_object() else {
            return plugin;
        };
        let mut consumed = std::collections::HashSet::new();
        if let Some(v) = obj.get("name").and_then(|v| v.as_str()) {
            plugin.name = v.to_string();
            consumed.insert("name");
        }
        if let Some(v) = obj.get("version").and_then(|v| v.as_str()) {
            plugin.version = v.to_string();
            consumed.insert("version");
        }
        if let Some(v) = obj.get("description").and_then(|v| v.as_str()) {
            plugin.description = v.to_string();
            consumed.insert("description");
        }
        // author 兼容字符串或对象（{name, email}）
        if let Some(v) = obj.get("author") {
            match v {
                Value::String(s) => plugin.author = s.clone(),
                Value::Object(m) => {
                    if let Some(name) = m.get("name").and_then(|n| n.as_str()) {
                        plugin.author = name.to_string();
                    }
                }
                _ => {}
            }
            consumed.insert("author");
        }
        if let Some(v) = obj.get("license").and_then(|v| v.as_str()) {
            plugin.license = v.to_string();
            consumed.insert("license");
        }
        if let Some(v) = obj.get("homepage").and_then(|v| v.as_str()) {
            plugin.homepage = v.to_string();
            consumed.insert("homepage");
        }
        if let Some(v) = obj.get("repository").and_then(|v| v.as_str()) {
            plugin.repository = v.to_string();
            consumed.insert("repository");
        }
        if let Some(v) = obj.get("keywords").and_then(|v| v.as_array()) {
            plugin.keywords = v
                .iter()
                .filter_map(|k| k.as_str())
                .map(|s| s.to_string())
                .collect();
            consumed.insert("keywords");
        }
        // 能力单元键（skills/agents/commands/hooks/mcpServers）由转换逻辑单独处理。
        for key in [
            "skills",
            "agents",
            "commands",
            "hooks",
            "mcpServers",
            "mcp_servers",
            "integrations",
        ] {
            consumed.insert(key);
        }
        for (k, v) in obj {
            if !consumed.contains(k.as_str()) {
                plugin.metadata.insert(k.clone(), v.clone());
            }
        }
        plugin
    }
}

/// 从反向域名 / 名称生成合法插件 id（无点时用 `local.` 前缀补全）。
fn derive_plugin_id(name: &str) -> String {
    let sanitized: String = name
        .trim()
        .to_ascii_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '.' {
                c
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    if sanitized.contains('.') {
        sanitized
    } else if sanitized.is_empty() {
        "local.imported".to_string()
    } else {
        format!("local.{sanitized}")
    }
}

/// 导入 Claude/Codex 格式插件目录 → 目标插件根目录（默认 `~/.pointer/plugins`）。
///
/// 支持两种选择方式：
/// - 选择**插件根目录**（含 `skills/`、`.claude-plugin/plugin.json` 或 `.codex-plugin/plugin.json`）；
/// - 选择 `.claude-plugin` / `.codex-plugin` 目录内部（用户可能在目录选择器里选到了它）——
///   此时能力单元在父目录，自动上溯定位插件根。
pub fn import_claude_plugin(source: &Path, target_root: &Path) -> Result<ImportReport> {
    import_external_plugin_kind(source, target_root, None)
}

/// 统一导入入口：检测目录插件类型并按类型分发。
/// 优先级：Pointer 原生 → Codex → Claude；未知则报错。
///
/// - Pointer：直接复制整个目录到 `target_root/<id>/`（保留 `pointer-plugin.toml` 原样）；
/// - Codex / Claude：转换生成原生 `pointer-plugin.toml` + 拷贝能力单元。
pub fn import_plugin_directory(source: &Path, target_root: &Path) -> Result<ImportReport> {
    if !source.is_dir() {
        return Err(anyhow!("源插件目录不存在: {}", source.display()));
    }
    let Some((kind, root, _manifest)) = detect_plugin_package(source)? else {
        return Err(anyhow!(
            "未在该目录中发现插件元数据：{}（期望 pointer-plugin.toml / .claude-plugin/plugin.json / .codex-plugin/plugin.json）",
            source.display()
        ));
    };
    match kind {
        PluginKind::Pointer => import_pointer_plugin(&root, target_root),
        PluginKind::Codex => import_external_plugin_kind(source, target_root, Some(kind)),
        PluginKind::Claude => import_external_plugin_kind(source, target_root, Some(kind)),
    }
}

/// 批量导入：扫描顶层目录（含一层子目录）中全部插件候选，按优先级
/// （Pointer → Codex → Claude）逐个自动导入，返回全部导入报告。
/// 单个候选失败只记日志不阻断其余候选。
pub fn import_plugin_directory_all(source: &Path, target_root: &Path) -> Result<Vec<ImportReport>> {
    if !source.is_dir() {
        return Err(anyhow!("源插件目录不存在: {}", source.display()));
    }
    let candidates = discover_plugin_packages(source)?;
    if candidates.is_empty() {
        return Err(anyhow!(
            "未在 {} 中发现插件元数据（pointer-plugin.toml / .claude-plugin/plugin.json / .codex-plugin/plugin.json）",
            source.display()
        ));
    }
    let mut reports = Vec::new();
    for candidate in candidates {
        match import_plugin_directory(&candidate.root, target_root) {
            Ok(report) => reports.push(report),
            Err(err) => {
                log::warn!(
                    "plugin: 自动导入失败 root={} kind={:?}: {err:#}",
                    candidate.root.display(),
                    candidate.kind
                );
            }
        }
    }
    Ok(reports)
}

/// 从 zip 字节导入插件：解压到临时目录后复用目录导入逻辑。
/// zip 结构可以是「插件根目录」或「含多个插件子目录的包」；
/// 自动按 Pointer → Codex → Claude 优先级识别，返回全部导入报告。
pub fn import_plugin_zip(bytes: &[u8], target_root: &Path) -> Result<Vec<ImportReport>> {
    use std::io::Cursor;
    const MAX_ZIP_SIZE: usize = 50 * 1024 * 1024;
    const MAX_ENTRY_SIZE: u64 = 20 * 1024 * 1024;

    if bytes.len() > MAX_ZIP_SIZE {
        return Err(anyhow!("插件 zip 文件过大，最大支持 50MB"));
    }
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).context("无法读取插件 zip 文件")?;

    // 安全解压到临时目录（防 zip-slip / 绝对路径 / 上层路径逃逸）
    let tmp = tempfile::tempdir().context("创建临时目录失败")?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let name = file.name().to_string();
        let Some(relative) = normalize_zip_entry_path(&name) else {
            return Err(anyhow!("插件 zip 包含非法路径: {name}"));
        };
        let is_dir = file.is_dir() || name.ends_with('/');
        let dest = tmp.path().join(&relative);
        if is_dir {
            fs::create_dir_all(&dest)?;
            continue;
        }
        if file.size() > MAX_ENTRY_SIZE {
            return Err(anyhow!("插件 zip 单文件过大: {name}"));
        }
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut out = fs::File::create(&dest)?;
        std::io::copy(&mut file, &mut out)?;
    }

    // 优先直接识别（zip 根 = 插件 或 一层子目录 = 插件）；
    // 失败时剥掉一层公共顶层目录重试（常见「包名/插件…」打包惯例）。
    let direct = import_plugin_directory_all(tmp.path(), target_root);
    if let Ok(reports) = &direct {
        if !reports.is_empty() {
            return Ok(reports.clone());
        }
    }
    let inner = strip_common_top_dir(tmp.path());
    if inner != tmp.path() {
        if let Ok(reports) = import_plugin_directory_all(&inner, target_root) {
            if !reports.is_empty() {
                return Ok(reports);
            }
        }
    }
    direct
}

/// 规范化 zip 内路径：拒绝绝对路径、上层路径、空段；其余保留原样。
fn normalize_zip_entry_path(name: &str) -> Option<PathBuf> {
    let raw = name.replace('\\', "/");
    if raw.starts_with('/') || raw.split('/').any(|p| p == "..") {
        return None;
    }
    let trimmed = raw.trim_end_matches('/');
    if trimmed.is_empty() {
        return None;
    }
    Some(PathBuf::from(trimmed))
}

/// 若临时目录根没有插件元数据、且恰好只有一个直接子目录，则返回该子目录
/// （剥掉 zip 打包的公共顶层目录层）；否则原样返回。
fn strip_common_top_dir(dir: &Path) -> PathBuf {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e.flatten().collect::<Vec<_>>(),
        Err(_) => return dir.to_path_buf(),
    };
    let has_manifest = entries.iter().any(|e| {
        e.path().is_file()
            && (e.file_name() == "pointer-plugin.toml" || e.file_name() == "plugin.json")
    });
    if has_manifest {
        return dir.to_path_buf();
    }
    let subdirs: Vec<PathBuf> = entries
        .iter()
        .filter(|e| e.path().is_dir() && !e.file_name().to_string_lossy().starts_with('.'))
        .map(|e| e.path())
        .collect();
    if subdirs.len() == 1 {
        return subdirs[0].clone();
    }
    dir.to_path_buf()
}

/// Pointer 原生插件：复制整个目录到 target_root/<id>/，并校验 manifest。
fn import_pointer_plugin(source: &Path, target_root: &Path) -> Result<ImportReport> {
    let (manifest, _) = crate::plugins::load_manifest(source)?;
    let plugin_id = manifest.plugin.id.clone();
    let target = target_root.join(&plugin_id);
    if target.exists() {
        fs::remove_dir_all(&target)?;
    }
    copy_dir(source, &target)?;
    Ok(ImportReport {
        plugin_id: plugin_id.clone(),
        plugin_name: manifest.plugin.name.clone(),
        target_dir: target,
        converted: vec!["pointer-plugin.toml".to_string(), "目录整体".to_string()],
        skipped: Vec::new(),
        unmapped: Vec::new(),
    })
}

/// 转换导入 Codex / Claude 插件。`kind=None` 时自动检测（兼容 `import_claude_plugin`）。
fn import_external_plugin_kind(
    source: &Path,
    target_root: &Path,
    kind: Option<PluginKind>,
) -> Result<ImportReport> {
    if !source.is_dir() {
        return Err(anyhow!("源插件目录不存在: {}", source.display()));
    }
    let (manifest_path, plugin_root) = find_external_plugin_root(source, kind)?;
    let raw: Value = serde_json::from_str(&fs::read_to_string(&manifest_path)?)
        .with_context(|| format!("解析 {} 失败", manifest_path.display()))?;
    let plugin = ClaudePlugin::from_json(&raw);

    let plugin_id = derive_plugin_id(&plugin.name);
    let target = target_root.join(&plugin_id);
    if target.exists() {
        fs::remove_dir_all(&target)?;
    }
    fs::create_dir_all(&target)?;

    let mut report = ImportReport {
        plugin_id: plugin_id.clone(),
        plugin_name: if plugin.name.is_empty() {
            plugin_id.clone()
        } else {
            plugin.name.clone()
        },
        target_dir: target.clone(),
        converted: Vec::new(),
        skipped: Vec::new(),
        unmapped: Vec::new(),
    };

    // skills/ → 原样拷贝（SKILL.md 开放标准，格式一致）
    if plugin_root.join("skills").is_dir() {
        copy_dir(&plugin_root.join("skills"), &target.join("skills"))?;
        report.converted.push("skills".to_string());
    } else {
        report.skipped.push("skills".to_string());
    }

    // agents/*.md → 拷贝（frontmatter 字段映射由 agents 加载层处理；原样保留）
    if plugin_root.join("agents").is_dir() {
        copy_dir(&plugin_root.join("agents"), &target.join("agents"))?;
        report.converted.push("agents".to_string());
    } else {
        report.skipped.push("agents".to_string());
    }

    // commands/*.md → 转为 Skill（disable-model-invocation 语义）
    if plugin_root.join("commands").is_dir() {
        let n = convert_commands(&plugin_root.join("commands"), &target.join("skills"))?;
        if n > 0 {
            report.converted.push(format!("commands→skills ({n})"));
        } else {
            report.skipped.push("commands".to_string());
        }
    } else {
        report.skipped.push("commands".to_string());
    }

    // hooks/hooks.json → 原样保留路径引用（P3 执行器；P1 仅拷贝）
    let hooks_source = plugin_root.join(HOOKS_FILE);
    if hooks_source.is_file() {
        fs::create_dir_all(target.join("hooks"))?;
        fs::copy(&hooks_source, target.join(HOOKS_FILE))?;
        report.converted.push("hooks".to_string());
    } else {
        report.skipped.push("hooks".to_string());
    }

    // .mcp.json / plugin.json.mcpServers → [[mcp_servers.server]]
    let mut mcp_servers: Vec<Value> = Vec::new();
    let mcp_path = plugin_root.join(MCP_CONFIG_FILE);
    if mcp_path.is_file() {
        let mcp_raw: Value = serde_json::from_str(&fs::read_to_string(&mcp_path)?)
            .with_context(|| format!("解析 {} 失败", mcp_path.display()))?;
        if let Some(servers) = mcp_raw.get("mcpServers").and_then(|v| v.as_object()) {
            for (name, cfg) in servers {
                mcp_servers.push(serde_json::json!({
                    "name": name,
                    "transport": cfg.get("type").and_then(|t| t.as_str()).unwrap_or("stdio"),
                    "command": cfg.get("command").and_then(|c| c.as_str()).unwrap_or(""),
                    "args": cfg.get("args").cloned().unwrap_or_else(|| Value::Array(vec![])),
                    "env": cfg.get("env").cloned().unwrap_or_else(|| Value::Object(Default::default())),
                }));
            }
        }
    }
    if let Some(servers) = raw.get("mcpServers").and_then(|v| v.as_object()) {
        for (name, cfg) in servers {
            mcp_servers.push(serde_json::json!({
                "name": name,
                "transport": cfg.get("type").and_then(|t| t.as_str()).unwrap_or("stdio"),
                "command": cfg.get("command").and_then(|c| c.as_str()).unwrap_or(""),
                "args": cfg.get("args").cloned().unwrap_or_else(|| Value::Array(vec![])),
                "env": cfg.get("env").cloned().unwrap_or_else(|| Value::Object(Default::default())),
            }));
        }
    }
    if !mcp_servers.is_empty() {
        report
            .converted
            .push(format!("mcp_servers ({})", mcp_servers.len()));
    }

    // 未映射字段 → [metadata]
    for (k, v) in &plugin.metadata {
        report.unmapped.push(format!("{k}: {v}"));
    }

    // 生成 pointer-plugin.toml
    let manifest = build_native_manifest(&plugin, &mcp_servers);
    fs::write(target.join("pointer-plugin.toml"), manifest)
        .with_context(|| "写入 pointer-plugin.toml 失败")?;
    report.converted.push("pointer-plugin.toml".to_string());

    log::info!(
        "plugin imported: {} from {} (root={}) converted={:?} skipped={:?}",
        plugin_id,
        source.display(),
        plugin_root.display(),
        report.converted,
        report.skipped
    );
    Ok(report)
}

/// 定位外部插件（Codex / Claude）：返回 `(manifest_path, plugin_root)`。
///
/// 规则：
/// - `source/.claude-plugin/plugin.json` 或 `source/.codex-plugin/plugin.json` → 插件根 = source；
/// - `source/plugin.json` 且目录名是 `.claude-plugin` / `.codex-plugin`（用户选中了内部目录）→ 插件根 = 父目录；
/// - `source/plugin.json` → 插件根 = source。
/// `kind=None` 时按 Claude → Codex 顺序探测（兼容旧入口）。
fn find_external_plugin_root(
    source: &Path,
    kind: Option<PluginKind>,
) -> Result<(PathBuf, PathBuf)> {
    let dirs: Vec<&str> = match kind {
        Some(PluginKind::Codex) => vec![CODEX_PLUGIN_DIR],
        Some(PluginKind::Claude) | None => vec![CLAUDE_PLUGIN_DIR, CODEX_PLUGIN_DIR],
        Some(PluginKind::Pointer) => unreachable!("pointer 不走外部转换"),
    };
    for dir in dirs {
        let nested = source.join(dir).join(CLAUDE_PLUGIN_MANIFEST);
        if nested.is_file() {
            return Ok((nested, source.to_path_buf()));
        }
    }
    let root_manifest = source.join(CLAUDE_PLUGIN_MANIFEST);
    if root_manifest.is_file() {
        let dir_name = source.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if dir_name == CLAUDE_PLUGIN_DIR || dir_name == CODEX_PLUGIN_DIR {
            let parent = source
                .parent()
                .ok_or_else(|| anyhow!("无法解析插件根目录"))?;
            return Ok((root_manifest, parent.to_path_buf()));
        }
        return Ok((root_manifest, source.to_path_buf()));
    }
    Err(anyhow!(
        "未找到插件 manifest（{} / {} 或根目录 plugin.json）",
        source
            .join(CLAUDE_PLUGIN_DIR)
            .join(CLAUDE_PLUGIN_MANIFEST)
            .display(),
        source
            .join(CODEX_PLUGIN_DIR)
            .join(CLAUDE_PLUGIN_MANIFEST)
            .display()
    ))
}

/// commands/*.md → skills/<command-name>/SKILL.md（`disable-model-invocation` 语义）。
fn convert_commands(commands_dir: &Path, skills_target: &Path) -> Result<usize> {
    let mut count = 0usize;
    if !commands_dir.is_dir() {
        return Ok(0);
    }
    fs::create_dir_all(skills_target)?;
    for entry in fs::read_dir(commands_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("md") {
            let name = path
                .file_stem()
                .and_then(|n| n.to_str())
                .unwrap_or("command")
                .to_string();
            let skill_dir = skills_target.join(&name);
            fs::create_dir_all(&skill_dir)?;
            let body = fs::read_to_string(&path)?;
            let skill_md = format!(
                "---\nname: {name}\ndescription: 导入的命令（disable-model-invocation）\ndisable-model-invocation: true\n---\n\n{body}\n"
            );
            fs::write(skill_dir.join("SKILL.md"), skill_md)?;
            count += 1;
        }
    }
    Ok(count)
}

/// 生成 Pointer 原生 `pointer-plugin.toml`。
fn build_native_manifest(plugin: &ClaudePlugin, mcp_servers: &[Value]) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "[plugin]\nid = \"{}\"\nname = \"{}\"\nversion = \"{}\"\napi_version = \"v1\"\n",
        derive_plugin_id(&plugin.name),
        escape_toml(&plugin.name),
        escape_toml(if plugin.version.is_empty() {
            "0.1.0"
        } else {
            &plugin.version
        }),
    ));
    if !plugin.description.is_empty() {
        out.push_str(&format!(
            "description = \"{}\"\n",
            escape_toml(&plugin.description)
        ));
    }
    if !plugin.author.is_empty() {
        out.push_str(&format!("author = \"{}\"\n", escape_toml(&plugin.author)));
    }
    if !plugin.license.is_empty() {
        out.push_str(&format!("license = \"{}\"\n", escape_toml(&plugin.license)));
    }
    if !plugin.homepage.is_empty() {
        out.push_str(&format!(
            "homepage = \"{}\"\n",
            escape_toml(&plugin.homepage)
        ));
    }
    if !plugin.repository.is_empty() {
        out.push_str(&format!(
            "repository = \"{}\"\n",
            escape_toml(&plugin.repository)
        ));
    }
    if !plugin.keywords.is_empty() {
        let joined = plugin
            .keywords
            .iter()
            .map(|k| format!("\"{}\"", escape_toml(k)))
            .collect::<Vec<_>>()
            .join(", ");
        out.push_str(&format!("keywords = [{joined}]\n"));
    }
    out.push('\n');

    // 有 skill/agent/command 目录才写 skills / agents 段（由转换逻辑决定）
    out.push_str("[skills]\npath = \"skills/\"\n\n");
    out.push_str("[agents]\npath = \"agents/\"\n\n");
    out.push_str("[rules]\npath = \"rules/\"\n\n");

    for server in mcp_servers {
        out.push_str("[[mcp_servers.server]]\n");
        out.push_str(&format!(
            "name = \"{}\"\n",
            escape_toml(server["name"].as_str().unwrap_or(""))
        ));
        out.push_str(&format!(
            "transport = \"{}\"\n",
            escape_toml(server["transport"].as_str().unwrap_or("stdio"))
        ));
        if let Some(cmd) = server["command"].as_str() {
            out.push_str(&format!("command = \"{}\"\n", escape_toml(cmd)));
        }
        if let Some(args) = server["args"].as_array() {
            if !args.is_empty() {
                let joined = args
                    .iter()
                    .filter_map(|a| a.as_str())
                    .map(|a| format!("\"{}\"", escape_toml(a)))
                    .collect::<Vec<_>>()
                    .join(", ");
                out.push_str(&format!("args = [{joined}]\n"));
            }
        }
        out.push('\n');
    }

    if !plugin.metadata.is_empty() {
        out.push_str("[metadata]\n");
        for (k, v) in &plugin.metadata {
            let json = serde_json::to_string(v).unwrap_or_else(|_| "\"\"".into());
            out.push_str(&format!("{k} = \"{}\"\n", escape_toml(&json)));
        }
    }

    out
}

fn escape_toml(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

fn copy_dir(src: &Path, dst: &Path) -> Result<()> {
    if !src.is_dir() {
        return Ok(());
    }
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        let target = dst.join(entry.file_name());
        if path.is_dir() {
            copy_dir(&path, &target)?;
        } else if path.is_file() {
            fs::copy(&path, &target)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_claude_plugin(root: &Path) -> PathBuf {
        let dir = root.join("my-plugin");
        fs::create_dir_all(dir.join(".claude-plugin")).unwrap();
        fs::create_dir_all(dir.join("skills/hello")).unwrap();
        fs::create_dir_all(dir.join("agents/worker")).unwrap();
        fs::create_dir_all(dir.join("commands")).unwrap();
        fs::create_dir_all(dir.join("hooks")).unwrap();

        fs::write(
            dir.join(".claude-plugin/plugin.json"),
            r#"{
  "name": "My Demo Plugin",
  "version": "1.0.0",
  "description": "测试导入",
  "author": "someone",
  "license": "MIT",
  "customField": { "x": 1 },
  "mcpServers": {
    "demo": { "type": "stdio", "command": "bin/demo-mcp", "args": ["serve"] }
  }
}"#,
        )
        .unwrap();
        fs::write(
            dir.join("skills/hello/SKILL.md"),
            "---\nname: hello\ndescription: 打招呼\n---\n\n你好\n",
        )
        .unwrap();
        fs::write(
            dir.join("agents/worker/AGENT.md"),
            "---\nid: worker\ndescription: 工人\n---\n\nbody\n",
        )
        .unwrap();
        fs::write(dir.join("commands/do-it.md"), "# Do It\n\nRun things.\n").unwrap();
        fs::write(
            dir.join("hooks/hooks.json"),
            r#"{"hooks":{"PreToolUse":[{"matcher":"*","command":"bin/check.sh"}]}}"#,
        )
        .unwrap();
        dir
    }

    #[test]
    fn imports_claude_plugin_to_native() {
        let tmp = tempfile::tempdir().unwrap();
        let source = write_claude_plugin(tmp.path());
        let target_root = tmp.path().join("out");

        let report = import_claude_plugin(&source, &target_root).unwrap();
        assert_eq!(report.plugin_id, "local.my-demo-plugin");
        assert!(report.target_dir.join("pointer-plugin.toml").is_file());
        assert!(report.converted.iter().any(|c| c == "skills"));
        assert!(report
            .converted
            .iter()
            .any(|c| c.starts_with("commands→skills")));
        assert!(report
            .converted
            .iter()
            .any(|c| c.starts_with("mcp_servers")));
        // 未映射字段
        assert!(report.unmapped.iter().any(|u| u.starts_with("customField")));

        // 能力单元文件已拷贝/转换
        assert!(report.target_dir.join("skills/hello/SKILL.md").is_file());
        assert!(report.target_dir.join("skills/do-it/SKILL.md").is_file());
        assert!(report.target_dir.join("agents/worker/AGENT.md").is_file());
        assert!(report.target_dir.join("hooks/hooks.json").is_file());

        // 生成的 manifest 可被原生解析器解析
        let raw = fs::read_to_string(report.target_dir.join("pointer-plugin.toml")).unwrap();
        let parsed = crate::plugins::manifest::parse(&raw).unwrap();
        assert_eq!(parsed.plugin.name, "My Demo Plugin");
        assert_eq!(parsed.mcp_servers.server.len(), 1);
        assert_eq!(parsed.mcp_servers.server[0].name, "demo");
        assert_eq!(parsed.mcp_servers.server[0].command, "bin/demo-mcp");
        // 转换后的 commands → skill 目录
        assert!(parsed.skills.is_some());
    }

    #[test]
    fn imports_plugin_zip_with_top_level_dir() {
        use std::io::Write;
        use zip::write::{SimpleFileOptions, ZipWriter};

        let tmp = tempfile::tempdir().unwrap();
        let source = write_claude_plugin(tmp.path());

        // 打包为 zip，外层带一个顶层目录（zip 打包惯例）
        let mut buf = Vec::new();
        {
            let mut zw = ZipWriter::new(std::io::Cursor::new(&mut buf));
            let opts = SimpleFileOptions::default();
            fn add_dir(zw: &mut ZipWriter<std::io::Cursor<&mut Vec<u8>>>, path: &str) {
                zw.add_directory(format!("{path}/"), SimpleFileOptions::default())
                    .unwrap();
            }
            add_dir(&mut zw, "bundle");
            add_dir(&mut zw, "bundle/my-plugin");
            add_dir(&mut zw, "bundle/my-plugin/.claude-plugin");
            add_dir(&mut zw, "bundle/my-plugin/skills/hello");
            add_dir(&mut zw, "bundle/my-plugin/agents/worker");
            add_dir(&mut zw, "bundle/my-plugin/commands");
            add_dir(&mut zw, "bundle/my-plugin/hooks");
            for rel in [
                ".claude-plugin/plugin.json",
                "skills/hello/SKILL.md",
                "agents/worker/AGENT.md",
                "commands/do-it.md",
                "hooks/hooks.json",
            ] {
                let full = format!("bundle/my-plugin/{rel}");
                let content = fs::read(source.join(rel)).unwrap();
                zw.start_file(full, opts).unwrap();
                zw.write_all(&content).unwrap();
            }
            zw.finish().unwrap();
        }

        let target_root = tmp.path().join("out");
        let reports = import_plugin_zip(&buf, &target_root).unwrap();
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].plugin_id, "local.my-demo-plugin");
        assert!(reports[0].target_dir.join("pointer-plugin.toml").is_file());
        // 能力单元完整落盘
        assert!(reports[0]
            .target_dir
            .join("skills/hello/SKILL.md")
            .is_file());
        assert!(reports[0].target_dir.join("hooks/hooks.json").is_file());
    }

    #[test]
    fn rejects_zip_slip_paths() {
        use std::io::Write;
        use zip::write::{SimpleFileOptions, ZipWriter};

        let tmp = tempfile::tempdir().unwrap();
        let mut buf = Vec::new();
        {
            let mut zw = ZipWriter::new(std::io::Cursor::new(&mut buf));
            zw.start_file("../evil.txt", SimpleFileOptions::default())
                .unwrap();
            zw.write_all(b"x").unwrap();
            zw.finish().unwrap();
        }
        let err = import_plugin_zip(&buf, &tmp.path().join("out"))
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("非法路径") || err.contains("zip"),
            "unexpected: {err}"
        );
    }

    #[test]
    fn derives_plugin_id() {
        assert_eq!(derive_plugin_id("My Demo Plugin"), "local.my-demo-plugin");
        assert_eq!(derive_plugin_id("com.example.foo"), "com.example.foo");
        assert_eq!(derive_plugin_id("   "), "local.imported");
    }

    #[test]
    fn missing_manifest_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let err = import_claude_plugin(tmp.path(), &tmp.path().join("out"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("未找到插件 manifest"), "unexpected: {err}");
    }

    #[test]
    fn imports_superpowers_style_multi_platform_plugin() {
        // 模拟 obra/superpowers 结构：.claude-plugin/plugin.json（author 为对象）+
        // skills 在插件根；同时用户在目录选择器里可能选中 .claude-plugin 内部。
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("superpowers");
        fs::create_dir_all(root.join(".claude-plugin")).unwrap();
        fs::create_dir_all(root.join("skills/brainstorming")).unwrap();
        fs::create_dir_all(root.join("skills/test-driven-development")).unwrap();
        fs::write(
            root.join(".claude-plugin/plugin.json"),
            r#"{
  "name": "superpowers",
  "description": "Core skills library for Claude Code",
  "version": "6.3.0",
  "author": { "name": "Jesse Vincent", "email": "jesse@fsck.com" },
  "homepage": "https://github.com/obra/superpowers",
  "repository": "https://github.com/obra/superpowers",
  "license": "MIT",
  "keywords": ["skills", "tdd", "debugging"]
}"#,
        )
        .unwrap();
        fs::write(
            root.join("skills/brainstorming/SKILL.md"),
            "---\nname: brainstorming\ndescription: 头脑风暴\n---\n\n澄清需求\n",
        )
        .unwrap();
        fs::write(
            root.join("skills/test-driven-development/SKILL.md"),
            "---\nname: test-driven-development\ndescription: TDD\n---\n\n红绿重构\n",
        )
        .unwrap();

        // 场景 A：用户选择插件根目录 → 全部能力被导入
        let out_a = tmp.path().join("out_a");
        let report_a = import_claude_plugin(&root, &out_a).unwrap();
        assert_eq!(report_a.plugin_id, "local.superpowers");
        assert_eq!(report_a.plugin_name, "superpowers");
        assert!(report_a.converted.iter().any(|c| c == "skills"));
        assert!(report_a
            .converted
            .iter()
            .any(|c| c == "pointer-plugin.toml"));
        assert!(!report_a.skipped.contains(&"skills".to_string()));
        assert!(out_a
            .join("local.superpowers/skills/brainstorming/SKILL.md")
            .is_file());
        assert!(out_a
            .join("local.superpowers/skills/test-driven-development/SKILL.md")
            .is_file());
        // author 对象被消费，不再进未映射
        assert!(!report_a.unmapped.iter().any(|u| u.starts_with("author")));
        let raw = fs::read_to_string(out_a.join("local.superpowers/pointer-plugin.toml")).unwrap();
        assert!(raw.contains("author = \"Jesse Vincent\""));
        assert!(raw.contains("homepage = \"https://github.com/obra/superpowers\""));
        assert!(raw.contains("keywords = [\"skills\", \"tdd\", \"debugging\"]"));

        // 场景 B：用户误选 .claude-plugin 内部 → 自动上溯到插件根，能力仍被导入
        let out_b = tmp.path().join("out_b");
        let report_b = import_claude_plugin(&root.join(".claude-plugin"), &out_b).unwrap();
        assert_eq!(report_b.plugin_id, "local.superpowers");
        assert!(report_b.converted.iter().any(|c| c == "skills"));
        assert!(out_b
            .join("local.superpowers/skills/brainstorming/SKILL.md")
            .is_file());
    }

    #[test]
    fn detect_plugin_priority_pointer_then_codex_then_claude() {
        let tmp = tempfile::tempdir().unwrap();

        // Claude only
        let claude_dir = tmp.path().join("claude-only");
        fs::create_dir_all(claude_dir.join(".claude-plugin")).unwrap();
        fs::write(
            claude_dir.join(".claude-plugin/plugin.json"),
            r#"{"name": "c", "version": "1.0.0"}"#,
        )
        .unwrap();
        let detected = detect_plugin_package(&claude_dir).unwrap().unwrap();
        assert_eq!(detected.0, PluginKind::Claude);
        assert_eq!(detected.1, claude_dir);

        // Codex + Claude（superpowers 风格）：Codex 优先
        let multi = tmp.path().join("multi");
        fs::create_dir_all(multi.join(".codex-plugin")).unwrap();
        fs::create_dir_all(multi.join(".claude-plugin")).unwrap();
        fs::write(
            multi.join(".codex-plugin/plugin.json"),
            r#"{"name": "superpowers", "version": "6.3.0", "skills": "./skills/"}"#,
        )
        .unwrap();
        fs::write(
            multi.join(".claude-plugin/plugin.json"),
            r#"{"name": "superpowers", "version": "6.3.0"}"#,
        )
        .unwrap();
        let detected = detect_plugin_package(&multi).unwrap().unwrap();
        assert_eq!(detected.0, PluginKind::Codex);

        // Pointer 原生最优先
        let ptr = tmp.path().join("pointer-native");
        fs::create_dir_all(ptr.join(".codex-plugin")).unwrap();
        fs::write(
            ptr.join("pointer-plugin.toml"),
            "[plugin]\nid = \"com.example.native\"\nname = \"Native\"\nversion = \"1.0.0\"\napi_version = \"v1\"\n",
        )
        .unwrap();
        fs::write(
            ptr.join(".codex-plugin/plugin.json"),
            r#"{"name": "native", "version": "1.0.0"}"#,
        )
        .unwrap();
        let detected = detect_plugin_package(&ptr).unwrap().unwrap();
        assert_eq!(detected.0, PluginKind::Pointer);

        // 空目录 → None
        let empty = tmp.path().join("empty");
        fs::create_dir_all(&empty).unwrap();
        assert!(detect_plugin_package(&empty).unwrap().is_none());
    }

    #[test]
    fn discover_lists_multi_platform_candidates_in_top_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let top = tmp.path().join("plugins");
        // 两个插件：superpowers（claude+codex）、simple-claude
        let sp = top.join("superpowers");
        fs::create_dir_all(sp.join(".claude-plugin")).unwrap();
        fs::create_dir_all(sp.join(".codex-plugin")).unwrap();
        fs::create_dir_all(sp.join("skills/a")).unwrap();
        fs::write(
            sp.join("skills/a/SKILL.md"),
            "---\nname: a\ndescription: A\n---\n",
        )
        .unwrap();
        fs::write(
            sp.join(".claude-plugin/plugin.json"),
            r#"{"name": "superpowers", "version": "6.3.0"}"#,
        )
        .unwrap();
        fs::write(
            sp.join(".codex-plugin/plugin.json"),
            r#"{"name": "superpowers", "version": "6.3.0", "skills": "./skills/"}"#,
        )
        .unwrap();

        let sc = top.join("simple-claude");
        fs::create_dir_all(sc.join(".claude-plugin")).unwrap();
        fs::write(
            sc.join(".claude-plugin/plugin.json"),
            r#"{"name": "simple-claude", "version": "2.0.0"}"#,
        )
        .unwrap();

        let found = discover_plugin_packages(&top).unwrap();
        // 每目录只返回优先级最高的一个候选：superpowers（Codex 优先）+ simple-claude（Claude）
        assert_eq!(
            found.len(),
            2,
            "codex(superpowers) + claude(simple-claude)，got {:?}",
            found.iter().map(|d| (&d.kind, &d.name)).collect::<Vec<_>>()
        );
        assert_eq!(found[0].kind, PluginKind::Codex);
        assert_eq!(found[0].name, "superpowers");
        assert_eq!(found[1].kind, PluginKind::Claude);
        assert_eq!(found[1].name, "simple-claude");
    }

    #[test]
    fn imports_pointer_plugin_by_copy() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("native");
        fs::create_dir_all(src.join("skills/demo")).unwrap();
        fs::write(
            src.join("pointer-plugin.toml"),
            "[plugin]\nid = \"com.example.native\"\nname = \"Native\"\nversion = \"1.0.0\"\napi_version = \"v1\"\n\n[skills]\npath = \"skills/\"\n",
        )
        .unwrap();
        fs::write(
            src.join("skills/demo/SKILL.md"),
            "---\nname: demo\ndescription: Demo\n---\n",
        )
        .unwrap();

        let out = tmp.path().join("out");
        let report = import_plugin_directory(&src, &out).unwrap();
        assert_eq!(report.plugin_id, "com.example.native");
        assert!(report
            .converted
            .contains(&"pointer-plugin.toml".to_string()));
        assert!(out
            .join("com.example.native/skills/demo/SKILL.md")
            .is_file());
        assert!(out.join("com.example.native/pointer-plugin.toml").is_file());
    }

    #[test]
    fn import_plugin_directory_auto_detects_superpowers() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("superpowers");
        fs::create_dir_all(root.join(".claude-plugin")).unwrap();
        fs::create_dir_all(root.join(".codex-plugin")).unwrap();
        fs::create_dir_all(root.join("skills/brainstorming")).unwrap();
        fs::write(
            root.join(".codex-plugin/plugin.json"),
            r#"{"name": "superpowers", "version": "6.3.0", "skills": "./skills/"}"#,
        )
        .unwrap();
        fs::write(
            root.join(".claude-plugin/plugin.json"),
            r#"{"name": "superpowers", "version": "6.3.0"}"#,
        )
        .unwrap();
        fs::write(
            root.join("skills/brainstorming/SKILL.md"),
            "---\nname: brainstorming\ndescription: 头脑风暴\n---\n\n澄清需求\n",
        )
        .unwrap();

        let out = tmp.path().join("out");
        let report = import_plugin_directory(&root, &out).unwrap();
        assert_eq!(report.plugin_id, "local.superpowers");
        assert!(report.converted.iter().any(|c| c == "skills"));
        assert!(out
            .join("local.superpowers/skills/brainstorming/SKILL.md")
            .is_file());
    }

    #[test]
    fn import_plugin_directory_all_imports_all_candidates() {
        let tmp = tempfile::tempdir().unwrap();
        let top = tmp.path().join("plugins");

        // 插件 A：superpowers（codex 优先）
        let sp = top.join("superpowers");
        fs::create_dir_all(sp.join(".codex-plugin")).unwrap();
        fs::create_dir_all(sp.join("skills/a")).unwrap();
        fs::write(
            sp.join("skills/a/SKILL.md"),
            "---\nname: a\ndescription: A\n---\n",
        )
        .unwrap();
        fs::write(
            sp.join(".codex-plugin/plugin.json"),
            r#"{"name": "superpowers", "version": "6.3.0", "skills": "./skills/"}"#,
        )
        .unwrap();

        // 插件 B：simple-claude
        let sc = top.join("simple-claude");
        fs::create_dir_all(sc.join(".claude-plugin")).unwrap();
        fs::create_dir_all(sc.join("skills/b")).unwrap();
        fs::write(
            sc.join("skills/b/SKILL.md"),
            "---\nname: b\ndescription: B\n---\n",
        )
        .unwrap();
        fs::write(
            sc.join(".claude-plugin/plugin.json"),
            r#"{"name": "simple-claude", "version": "2.0.0"}"#,
        )
        .unwrap();

        // 插件 C：pointer 原生
        let native = top.join("native");
        fs::create_dir_all(native.join("skills/c")).unwrap();
        fs::write(
            native.join("skills/c/SKILL.md"),
            "---\nname: c\ndescription: C\n---\n",
        )
        .unwrap();
        fs::write(
            native.join("pointer-plugin.toml"),
            "[plugin]\nid = \"com.example.native\"\nname = \"Native\"\nversion = \"1.0.0\"\napi_version = \"v1\"\n\n[skills]\npath = \"skills/\"\n",
        )
        .unwrap();

        let out = tmp.path().join("out");
        let reports = import_plugin_directory_all(&top, &out).unwrap();
        let ids: Vec<&str> = reports.iter().map(|r| r.plugin_id.as_str()).collect();
        assert_eq!(ids.len(), 3, "got {ids:?}");
        assert!(ids.contains(&"local.superpowers"));
        assert!(ids.contains(&"local.simple-claude"));
        assert!(ids.contains(&"com.example.native"));
        // 能力都被拷贝
        assert!(out.join("local.superpowers/skills/a/SKILL.md").is_file());
        assert!(out.join("local.simple-claude/skills/b/SKILL.md").is_file());
        assert!(out.join("com.example.native/skills/c/SKILL.md").is_file());
    }

    #[test]
    fn import_plugin_directory_all_errors_when_nothing_found() {
        let tmp = tempfile::tempdir().unwrap();
        let empty = tmp.path().join("empty");
        fs::create_dir_all(&empty).unwrap();
        let err = import_plugin_directory_all(&empty, &tmp.path().join("out"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("未在"), "unexpected: {err}");
    }

    #[test]
    fn converts_commands_to_skills() {
        let tmp = tempfile::tempdir().unwrap();
        let commands = tmp.path().join("commands");
        fs::create_dir_all(&commands).unwrap();
        fs::write(commands.join("a.md"), "# A\n").unwrap();
        fs::write(commands.join("b.md"), "# B\n").unwrap();
        let target = tmp.path().join("skills");
        let n = convert_commands(&commands, &target).unwrap();
        assert_eq!(n, 2);
        assert!(target.join("a/SKILL.md").is_file());
        let body = fs::read_to_string(target.join("a/SKILL.md")).unwrap();
        assert!(body.contains("disable-model-invocation: true"));
    }
}
