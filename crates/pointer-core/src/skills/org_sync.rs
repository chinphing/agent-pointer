//! 企业技能下发（org-skills）。
//!
//! 流程：拉目录 → 与本地比对 → 只下载有变化的包 → 解到 `{data_dir}/org-skills/`
//! → 在 `.org_manifest` 记下「远端哈希 + 本地目录哈希」。
//!
//! 为什么要记两个哈希：
//! - **远端哈希**（zip 的 sha256）决定「要不要重新下载」；
//! - **本地目录哈希**（SKILL.md 的 sha256）决定「员工是不是改过」，改过就不覆盖。
//!
//! 只记一个会把两种情况混在一起：要么永远重下，要么永远以为本地被改过。
//!
//! 目录独立于 `~/.pointer/skills`，否则会被 curator 90 天归档。

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use serde::Deserialize;

use super::{external, provenance};

/// 下发台账：`slug:远端哈希:本地目录哈希`，一行一条。
const ORG_MANIFEST: &str = ".org_manifest";

/// 下发对象类型：技能与插件的落盘位置、目录命名、哈希口径都不同。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageKind {
    Skill,
    Plugin,
}

impl PackageKind {
    fn segment(self) -> &'static str {
        match self {
            Self::Skill => "skills",
            Self::Plugin => "plugins",
        }
    }

    fn root(self) -> Result<PathBuf> {
        match self {
            Self::Skill => external::org_skills_dir(),
            Self::Plugin => plugins_root(),
        }
    }
}

/// 企业下发的插件落在 `~/.pointer/plugins`（原生插件目录），**只落盘不激活**。
pub fn plugins_root() -> Result<PathBuf> {
    let dir = external::pointer_home_dir()?.join(crate::plugins::USER_PLUGINS_DIR_NAME);
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// 插件目录的内容哈希：所有文件的 (相对路径, 内容 sha256) 排序后再哈希。
/// 技能用 `SKILL.md` 单文件哈希就够，插件没有统一入口文件，只能整目录算。
pub fn plugin_dir_hash(dir: &Path) -> Result<String> {
    use sha2::{Digest, Sha256};

    let mut files: Vec<(String, String)> = Vec::new();
    for entry in walkdir::WalkDir::new(dir).follow_links(false) {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        let rel = entry
            .path()
            .strip_prefix(dir)
            .unwrap_or(entry.path())
            .to_string_lossy()
            .replace('\\', "/");
        let digest = Sha256::digest(fs::read(entry.path())?);
        files.push((rel, format!("{digest:x}")));
    }
    files.sort();

    let mut hasher = Sha256::new();
    for (rel, digest) in &files {
        hasher.update(rel.as_bytes());
        hasher.update(b"\0");
        hasher.update(digest.as_bytes());
        hasher.update(b"\n");
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// 安装目录名：技能用资源 slug，插件用插件自己的 id（`import_plugin_zip` 就是这么建的目录）。
fn install_key(kind: PackageKind, remote: &RemoteSkill) -> Result<String> {
    match kind {
        PackageKind::Skill => Ok(remote.slug.trim().to_string()),
        PackageKind::Plugin => remote
            .plugin_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .ok_or_else(|| anyhow!("插件资源缺少 pluginId，无法确定安装目录")),
    }
}

fn dir_hash(kind: PackageKind, dir: &Path) -> Result<String> {
    match kind {
        PackageKind::Skill => provenance::skill_dir_hash(dir),
        PackageKind::Plugin => plugin_dir_hash(dir),
    }
}

/// 远端清单里的一条技能。
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct RemoteSkill {
    pub slug: String,
    #[serde(default)]
    pub name: String,
    /// 插件自己的 id（`pointer-plugin.toml` 里的 id），仅插件用。
    #[serde(default, rename = "pluginId")]
    pub plugin_id: Option<String>,
    pub version: String,
    /// zip 的 sha256（与 admin 侧 `dir_hash` 同义）。
    pub hash: String,
    #[serde(default, rename = "sizeBytes")]
    pub size_bytes: i64,
    /// `optional`（默认）/ `default_on`：控制面设置的启用策略。
    #[serde(default)]
    pub activation: String,
    /// 相对路径（admin 返回 `/auth/partner/skills/...`），客户端自行拼 base。
    pub url: String,
}

impl RemoteSkill {
    fn is_default_on(&self) -> bool {
        self.activation.trim() == "default_on"
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Catalog {
    #[serde(default)]
    pub skills: Vec<RemoteSkill>,
    #[serde(default, rename = "catalogHash")]
    pub catalog_hash: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ManifestEntry {
    remote_hash: String,
    local_hash: String,
}

fn read_manifest(root: &Path) -> HashMap<String, ManifestEntry> {
    let mut out = HashMap::new();
    let Ok(raw) = fs::read_to_string(root.join(ORG_MANIFEST)) else {
        return out;
    };
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.splitn(3, ':');
        let (Some(slug), Some(remote), Some(local)) =
            (parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        out.insert(
            slug.trim().to_string(),
            ManifestEntry {
                remote_hash: remote.trim().to_string(),
                local_hash: local.trim().to_string(),
            },
        );
    }
    out
}

fn write_manifest(root: &Path, entries: &HashMap<String, ManifestEntry>) -> Result<()> {
    let path = root.join(ORG_MANIFEST);
    let mut slugs: Vec<&String> = entries.keys().collect();
    slugs.sort();
    let mut body = String::from("# slug:remote_hash:local_hash\n");
    for slug in slugs {
        let entry = &entries[slug];
        body.push_str(&format!(
            "{}:{}:{}\n",
            slug, entry.remote_hash, entry.local_hash
        ));
    }
    let tmp = root.join(format!("{ORG_MANIFEST}.part"));
    fs::write(&tmp, body.as_bytes())?;
    fs::rename(&tmp, &path)?;
    Ok(())
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SyncPlan {
    pub download: Vec<RemoteSkill>,
    pub unchanged: Vec<String>,
    /// (slug, 原因)：本地有改动，保留本地不覆盖。
    pub blocked: Vec<(String, String)>,
    /// 远端已下架、且本地未被改动的目录。
    pub remove: Vec<String>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SyncReport {
    pub installed: Vec<String>,
    pub unchanged: Vec<String>,
    pub removed: Vec<String>,
    pub failed: Vec<(String, String)>,
}

/// 纯计算：对比本地目录与远端清单，决定要下载/删除什么。不碰网络。
pub fn plan(root: &Path, kind: PackageKind, catalog: &Catalog) -> Result<SyncPlan> {
    let mut out = SyncPlan::default();
    let manifest = read_manifest(root);
    let mut remote: HashSet<String> = HashSet::new();

    for skill in &catalog.skills {
        let slug = skill.slug.trim();
        if slug.is_empty() {
            continue;
        }
        let key = match install_key(kind, skill) {
            Ok(key) => key,
            Err(err) => {
                out.blocked.push((slug.to_string(), err.to_string()));
                continue;
            }
        };
        remote.insert(key.clone());

        let dir = root.join(&key);
        let entry = manifest.get(&key);

        if !dir.is_dir() {
            out.download.push(skill.clone());
            continue;
        }

        let Some(entry) = entry else {
            // 目录在、但没有下发记录：不是我们装的，不碰。
            out.blocked
                .push((key, "目录已存在且无下发记录".to_string()));
            continue;
        };

        let current = dir_hash(kind, &dir).ok();
        if current.as_deref() != Some(entry.local_hash.as_str()) {
            out.blocked
                .push((key, "本地已修改，跳过覆盖".to_string()));
            continue;
        }

        if entry.remote_hash == skill.hash {
            out.unchanged.push(key);
        } else {
            out.download.push(skill.clone());
        }
    }

    if root.is_dir() {
        for entry in fs::read_dir(root).with_context(|| format!("读取 {}", root.display()))? {
            let entry = entry?;
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if name.starts_with('.') || remote.contains(name) {
                continue;
            }
            let unmodified = manifest
                .get(name)
                .and_then(|e| dir_hash(kind, &path).ok().map(|c| c == e.local_hash))
                .unwrap_or(false);
            if unmodified {
                out.remove.push(name.to_string());
            } else {
                out.blocked
                    .push((name.to_string(), "远端已下架但本地有改动".to_string()));
            }
        }
    }

    Ok(out)
}

/// 把计划落到磁盘。`packages` 里是要下载的包的字节（key = 安装目录名）。
pub fn apply(
    root: &Path,
    kind: PackageKind,
    plan: &SyncPlan,
    packages: &HashMap<String, Vec<u8>>,
) -> Result<SyncReport> {
    let mut report = SyncReport {
        unchanged: plan.unchanged.clone(),
        ..Default::default()
    };
    report.failed.extend(plan.blocked.iter().cloned());

    let mut manifest = read_manifest(root);
    let mut manifest_dirty = false;

    for skill in &plan.download {
        let key = match install_key(kind, skill) {
            Ok(key) => key,
            Err(err) => {
                report.failed.push((skill.slug.clone(), err.to_string()));
                continue;
            }
        };
        let Some(bytes) = packages.get(&skill.slug) else {
            report
                .failed
                .push((key, "下载失败".to_string()));
            continue;
        };

        let installed: Result<PathBuf> = match kind {
            PackageKind::Skill => external::import_skill_zip_into(root, bytes).and_then(
                |(dir_name, path)| {
                    if dir_name == key {
                        return Ok(path);
                    }
                    // zip 里的技能名与资源 slug 可能不同（中文名 vs 英文 slug），
                    // 目录统一按 slug 命名，避免两套名字并存。
                    let target = root.join(&key);
                    if target.exists() {
                        fs::remove_dir_all(&target)?;
                    }
                    fs::rename(&path, &target).with_context(|| {
                        format!("重命名 {} -> {}", path.display(), target.display())
                    })?;
                    Ok(target)
                },
            ),
            PackageKind::Plugin => crate::plugins::importer::import_plugin_zip(bytes, root)
                .map_err(|e| anyhow!(e.to_string()))
                .and_then(|reports| {
                    if reports.len() != 1 {
                        return Err(anyhow!(
                            "插件包应只含一个插件，实际 {} 个",
                            reports.len()
                        ));
                    }
                    let report = &reports[0];
                    if report.plugin_id != key {
                        return Err(anyhow!(
                            "插件 id 不匹配：包内为 {}，资源声明为 {key}",
                            report.plugin_id
                        ));
                    }
                    Ok(report.target_dir.clone())
                }),
        };

        match installed.and_then(|target| {
            let local_hash = dir_hash(kind, &target)?;
            Ok((target, local_hash))
        }) {
            Ok((_target, local_hash)) => {
                manifest.insert(
                    key.clone(),
                    ManifestEntry {
                        remote_hash: skill.hash.clone(),
                        local_hash,
                    },
                );
                manifest_dirty = true;
                report.installed.push(key);
            }
            Err(err) => report.failed.push((key, err.to_string())),
        }
    }

    for key in &plan.remove {
        let dir = root.join(key);
        if let Err(err) = fs::remove_dir_all(&dir) {
            report
                .failed
                .push((key.clone(), format!("删除失败：{err}")));
            continue;
        }
        manifest.remove(key);
        manifest_dirty = true;
        report.removed.push(key.clone());
    }

    if manifest_dirty {
        write_manifest(root, &manifest)?;
    }

    Ok(report)
}

/// 企业策略文件：`default_on` 的技能 id 列表，由 `sync` 写、`manifest_to_skill` /
/// `resolve_skill_ids` 读。
const POLICY_FILE: &str = ".org_policy.json";

#[derive(Debug, Default, Clone, serde::Serialize, Deserialize)]
struct Policy {
    #[serde(default, rename = "defaultOn")]
    default_on: Vec<String>,
}

fn write_policy(root: &Path, default_on: Vec<String>) -> Result<()> {
    let path = root.join(POLICY_FILE);
    let body = serde_json::to_vec_pretty(&Policy { default_on })?;
    let tmp = root.join(format!("{POLICY_FILE}.part"));
    fs::write(&tmp, &body)?;
    fs::rename(&tmp, &path)?;
    Ok(())
}

fn read_policy(root: &Path) -> Vec<String> {
    let Ok(raw) = fs::read_to_string(root.join(POLICY_FILE)) else {
        return Vec::new();
    };
    serde_json::from_str::<Policy>(&raw)
        .map(|p| p.default_on)
        .unwrap_or_default()
}

/// 缓存：技能目录每次加载都会问一遍，不能每个技能读一次文件。
/// 用 (mtime, 长度) 当 key——文件一变就重读，测试里也不会有陈旧值。
/// 策略缓存：(文件 mtime, 长度, 解析结果)。
type PolicyCache = std::sync::Mutex<Option<(std::time::SystemTime, u64, Vec<String>)>>;

fn cached_default_on() -> Vec<String> {
    use std::sync::OnceLock;

    static CACHE: OnceLock<PolicyCache> = OnceLock::new();    let Ok(root) = external::org_skills_dir() else {
        return Vec::new();
    };
    let path = root.join(POLICY_FILE);
    let Ok(meta) = fs::metadata(&path) else {
        return Vec::new();
    };
    let key = (meta.modified().unwrap_or(std::time::UNIX_EPOCH), meta.len());

    let cell = CACHE.get_or_init(|| std::sync::Mutex::new(None));
    let mut guard = match cell.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    if let Some((mtime, len, ids)) = guard.as_ref() {
        if *mtime == key.0 && *len == key.1 {
            return ids.clone();
        }
    }
    let fresh = read_policy(&root);
    *guard = Some((key.0, key.1, fresh.clone()));
    fresh
}

/// 该技能是否被企业设为「默认启用」。
pub fn is_default_on(skill_id: &str) -> bool {
    cached_default_on().iter().any(|id| id == skill_id)
}

/// 企业设为 `default_on` 的技能 id（供 agent 解析默认技能列表时合并）。
pub fn default_on_skill_ids() -> Vec<String> {
    cached_default_on()
}

fn api_base() -> String {
    crate::platform_endpoints::api_base()
}

fn absolute_url(url: &str) -> String {
    if url.starts_with("http://") || url.starts_with("https://") {
        url.to_string()
    } else {
        format!("{}{}", api_base().trim_end_matches('/'), url)
    }
}

/// `GET {api_base}/auth/partner/{skills|plugins}`
pub async fn fetch_catalog(
    auth: &crate::platform_auth::PlatformAuthManager,
    kind: PackageKind,
) -> Result<Catalog> {
    let token = auth.ensure_access_token().await?;
    let url = format!(
        "{}/auth/partner/{}",
        api_base().trim_end_matches('/'),
        kind.segment()
    );
    let resp = reqwest::Client::new()
        .get(&url)
        .bearer_auth(token)
        .send()
        .await
        .context("拉取下发目录失败")?;
    if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err(anyhow!("platform_token_expired"));
    }
    if !resp.status().is_success() {
        return Err(anyhow!("拉取下发目录失败：HTTP {}", resp.status()));
    }
    resp.json::<Catalog>().await.context("解析下发目录失败")
}

async fn download_package(
    auth: &crate::platform_auth::PlatformAuthManager,
    url: &str,
) -> Result<Vec<u8>> {
    let token = auth.ensure_access_token().await?;
    let resp = reqwest::Client::new()
        .get(absolute_url(url))
        .bearer_auth(token)
        .send()
        .await
        .context("下载技能包失败")?;
    if !resp.status().is_success() {
        return Err(anyhow!("下载技能包失败：HTTP {}", resp.status()));
    }
    Ok(resp.bytes().await?.to_vec())
}

/// 同步一次：拉目录 → 规划 → 下载有变化的包 → 落盘。
pub async fn sync(
    auth: &crate::platform_auth::PlatformAuthManager,
    kind: PackageKind,
) -> Result<SyncReport> {
    let catalog = fetch_catalog(auth, kind).await?;
    let root = kind.root()?;
    let plan = plan(&root, kind, &catalog)?;

    let mut packages: HashMap<String, Vec<u8>> = HashMap::new();
    for skill in &plan.download {
        match download_package(auth, &skill.url).await {
            Ok(bytes) => {
                packages.insert(skill.slug.clone(), bytes);
            }
            Err(err) => log::warn!(
                "org {} {} 下载失败: {err:#}",
                kind.segment(),
                skill.slug
            ),
        }
    }

    let report = apply(&root, kind, &plan, &packages)?;

    // 只有技能会按策略默认启用；插件带工具/hooks/MCP，仍需显式激活。
    if kind == PackageKind::Skill {
        let default_on: Vec<String> = catalog
            .skills
            .iter()
            .filter(|item| item.is_default_on())
            .filter_map(|item| install_key(kind, item).ok())
            .collect();
        write_policy(&root, default_on)?;
    }

    Ok(report)
}

/// 启动后台同步：首次延迟 15s（别和登录/启动抢资源），之后每 6 小时一次。
pub fn start_background_loop(state: std::sync::Arc<crate::chat_service::AppState>) {
    if !crate::platform_endpoints::control_plane_bound() {
        log::debug!("org skills: control plane not bound, skip sync loop");
        return;
    }
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(15)).await;
        loop {
            let mut skills_changed = false;
            let mut plugins_changed = false;
            for kind in [PackageKind::Skill, PackageKind::Plugin] {
                match sync(state.platform_auth.as_ref(), kind).await {
                    Ok(report) => {
                        if !report.installed.is_empty() || !report.removed.is_empty() {
                            match kind {
                                PackageKind::Skill => skills_changed = true,
                                PackageKind::Plugin => plugins_changed = true,
                            }
                            log::info!(
                                "org {} synced: +{} -{} (unchanged {})",
                                kind.segment(),
                                report.installed.len(),
                                report.removed.len(),
                                report.unchanged.len()
                            );
                        }
                        for (key, reason) in &report.failed {
                            log::warn!("org {} {key} 未处理: {reason}", kind.segment());
                        }
                    }
                    Err(err) => log::warn!("org {} sync failed: {err:#}", kind.segment()),
                }
            }
            if skills_changed {
                if let Err(err) = state.skills.reload_meta() {
                    log::warn!("org skills: reload registry failed: {err:#}");
                }
            }
            if plugins_changed {
                // 插件落了盘还要重扫 + 装配，否则界面里看不到、能力也不生效。
                // 注意：只扫不激活——是否启用仍由员工/管理员决定。
                match state.plugins.scan() {
                    Ok(_) => state.apply_plugins(),
                    Err(err) => log::warn!("org plugins: rescan failed: {err:#}"),
                }
            }
            tokio::time::sleep(Duration::from_secs(6 * 3600)).await;
        }
    });
}

#[cfg(test)]
fn temp_root(tag: &str) -> std::path::PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "pointer-org-skills-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    fs::create_dir_all(&path).expect("temp root");
    path
}

#[cfg(test)]
fn build_zip(manifest_name: &str, body: &str) -> Vec<u8> {
    use std::io::Write;
    let mut buffer = Vec::new();
    {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(&mut buffer));
        let options = zip::write::SimpleFileOptions::default();
        writer.start_file("SKILL.md", options).expect("start");
        write!(
            writer,
            "---\nname: {manifest_name}\ndescription: 测试技能\n---\n\n{body}\n"
        )
        .expect("write");
        writer.finish().expect("finish");
    }
    buffer
}

#[cfg(test)]
fn build_plugin_zip(plugin_id: &str) -> Vec<u8> {
    use std::io::Write;
    let mut buffer = Vec::new();
    {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(&mut buffer));
        let options = zip::write::SimpleFileOptions::default();
        writer.start_file("pointer-plugin.toml", options).expect("start");
        write!(
            writer,
            "[plugin]\nid = \"{plugin_id}\"\nname = \"示例插件\"\nversion = \"1.0.0\"\napi_version = \"v1\"\n"
        )
        .expect("write");
        writer.start_file("README.md", options).expect("start readme");
        writer.write_all(b"# demo\n").expect("write readme");
        writer.finish().expect("finish");
    }
    buffer
}

#[cfg(test)]
fn remote(slug: &str, hash: &str) -> RemoteSkill {
    RemoteSkill {
        slug: slug.to_string(),
        name: slug.to_string(),
        plugin_id: None,
        version: "1.0.0".to_string(),
        hash: hash.to_string(),
        size_bytes: 0,
        activation: "optional".to_string(),
        url: format!("/auth/partner/skills/{slug}/1.0.0/download"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog_with(skills: Vec<RemoteSkill>) -> Catalog {
        Catalog {
            skills,
            catalog_hash: "c".into(),
        }
    }

    fn install(root: &Path, slug: &str, zip: Vec<u8>, hash: &str) {
        let catalog = catalog_with(vec![remote(slug, hash)]);
        let plan1 = plan(root, PackageKind::Skill, &catalog).expect("plan");
        let mut packages = HashMap::new();
        packages.insert(slug.to_string(), zip);
        apply(root, PackageKind::Skill, &plan1, &packages).expect("apply");
    }

    #[test]
    fn fresh_install_then_idempotent() {
        let root = temp_root("fresh");
        let zip = build_zip("travel", "# 差旅");
        let catalog = catalog_with(vec![remote("travel", "zip-hash-v1")]);

        let plan1 = plan(&root, PackageKind::Skill, &catalog).expect("plan1");
        assert_eq!(plan1.download.len(), 1);

        let mut packages = HashMap::new();
        packages.insert("travel".to_string(), zip);
        let report = apply(&root, PackageKind::Skill, &plan1, &packages).expect("apply");
        assert_eq!(report.installed, vec!["travel"]);
        assert!(root.join("travel/SKILL.md").is_file());

        let manifest = read_manifest(&root);
        let entry = manifest.get("travel").expect("manifest entry");
        assert_eq!(entry.remote_hash, "zip-hash-v1");
        assert_eq!(
            entry.local_hash,
            provenance::skill_dir_hash(&root.join("travel")).expect("dir hash")
        );

        // 第二次：两个哈希都没变 → 不下载
        let plan2 = plan(&root, PackageKind::Skill, &catalog).expect("plan2");
        assert!(plan2.download.is_empty());
        assert_eq!(plan2.unchanged, vec!["travel"]);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn local_modification_blocks_overwrite() {
        let root = temp_root("modified");
        install(&root, "travel", build_zip("travel", "# 差旅"), "zip-hash-v1");

        // 员工改了 SKILL.md
        fs::write(
            root.join("travel/SKILL.md"),
            "---\nname: travel\ndescription: 改过\n---\n\n本地改动\n",
        )
        .expect("modify");

        let catalog2 = catalog_with(vec![remote("travel", "zip-hash-v2")]);
        let plan2 = plan(&root, PackageKind::Skill, &catalog2).expect("plan2");
        assert!(plan2.download.is_empty(), "本地改过就不该覆盖");
        assert_eq!(plan2.blocked.len(), 1);
        assert!(plan2.blocked[0].1.contains("本地已修改"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn remote_upgrade_reinstalls_when_local_untouched() {
        let root = temp_root("upgrade");
        install(&root, "travel", build_zip("travel", "# v1"), "zip-hash-v1");

        let catalog2 = catalog_with(vec![remote("travel", "zip-hash-v2")]);
        let plan2 = plan(&root, PackageKind::Skill, &catalog2).expect("plan2");
        assert_eq!(plan2.download.len(), 1, "远端换了哈希应重新下载");

        let mut packages = HashMap::new();
        packages.insert("travel".to_string(), build_zip("travel", "# v2"));
        apply(&root, PackageKind::Skill, &plan2, &packages).expect("apply");
        let entry = read_manifest(&root).remove("travel").expect("entry");
        assert_eq!(entry.remote_hash, "zip-hash-v2");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn removed_remote_skill_is_deleted() {
        let root = temp_root("removed");
        install(&root, "travel", build_zip("travel", "# 差旅"), "zip-hash-v1");

        let empty = Catalog::default();
        let plan2 = plan(&root, PackageKind::Skill, &empty).expect("plan2");
        assert_eq!(plan2.remove, vec!["travel"]);
        let report = apply(&root, PackageKind::Skill, &plan2, &HashMap::new()).expect("apply");
        assert_eq!(report.removed, vec!["travel"]);
        assert!(!root.join("travel").exists());
        assert!(read_manifest(&root).get("travel").is_none());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn foreign_directory_is_not_touched() {
        let root = temp_root("foreign");
        fs::create_dir_all(root.join("handmade")).expect("dir");
        fs::write(
            root.join("handmade/SKILL.md"),
            "---\nname: handmade\ndescription: 手放进去的\n---\n",
        )
        .expect("write");

        let catalog = catalog_with(vec![remote("handmade", "zip-hash")]);
        let plan1 = plan(&root, PackageKind::Skill, &catalog).expect("plan");
        assert!(plan1.download.is_empty());
        assert_eq!(plan1.blocked.len(), 1);

        let empty = Catalog::default();
        let plan2 = plan(&root, PackageKind::Skill, &empty).expect("plan2");
        assert!(plan2.remove.is_empty(), "没下发记录的不删");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn multi_skill_zip_is_rejected() {
        use std::io::Write;
        let mut buffer = Vec::new();
        {
            let mut writer = zip::ZipWriter::new(std::io::Cursor::new(&mut buffer));
            let options = zip::write::SimpleFileOptions::default();
            writer.start_file("a/SKILL.md", options).expect("a");
            write!(writer, "---\nname: a\ndescription: x\n---\n").expect("write");
            writer.start_file("b/SKILL.md", options).expect("b");
            write!(writer, "---\nname: b\ndescription: y\n---\n").expect("write");
            writer.finish().expect("finish");
        }
        let root = temp_root("multi");
        assert!(external::import_skill_zip_into(&root, &buffer).is_err());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_package_is_reported_not_panicked() {
        let root = temp_root("missing");
        let catalog = catalog_with(vec![remote("travel", "zip-hash-v1")]);
        let plan1 = plan(&root, PackageKind::Skill, &catalog).expect("plan1");
        let report = apply(&root, PackageKind::Skill, &plan1, &HashMap::new()).expect("apply");
        assert!(report.installed.is_empty());
        assert_eq!(report.failed.len(), 1);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn plugin_install_uses_plugin_id_as_directory() {
        let root = temp_root("plugin");
        let zip = build_plugin_zip("com.example.demo");
        let mut item = remote("demo-plugin", "zip-hash-v1");
        item.plugin_id = Some("com.example.demo".to_string());
        let catalog = catalog_with(vec![item]);

        let plan1 = plan(&root, PackageKind::Plugin, &catalog).expect("plan");
        assert_eq!(plan1.download.len(), 1);

        let mut packages = HashMap::new();
        packages.insert("demo-plugin".to_string(), zip);
        let report = apply(&root, PackageKind::Plugin, &plan1, &packages).expect("apply");
        assert_eq!(report.installed, vec!["com.example.demo"]);
        assert!(root.join("com.example.demo/pointer-plugin.toml").is_file());
        assert!(read_manifest(&root).get("com.example.demo").is_some());

        // 第二次：哈希不变 → 不重装
        let plan2 = plan(&root, PackageKind::Plugin, &catalog).expect("plan2");
        assert!(plan2.download.is_empty());
        assert_eq!(plan2.unchanged, vec!["com.example.demo"]);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn plugin_without_plugin_id_is_blocked() {
        let root = temp_root("plugin-noid");
        let catalog = catalog_with(vec![remote("demo-plugin", "zip-hash-v1")]);
        let plan1 = plan(&root, PackageKind::Plugin, &catalog).expect("plan");
        assert!(plan1.download.is_empty());
        assert_eq!(plan1.blocked.len(), 1);
        assert!(plan1.blocked[0].1.contains("pluginId"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn policy_file_roundtrip() {
        let root = temp_root("policy");
        write_policy(&root, vec!["travel".into(), "expense".into()]).expect("write");
        assert_eq!(read_policy(&root), vec!["travel", "expense"]);
        assert_eq!(
            read_policy(&root),
            vec!["travel".to_string(), "expense".to_string()]
        );

        // 空策略也要能读（客户端据此判定「没有企业强制默认」）。
        write_policy(&root, Vec::new()).expect("write empty");
        assert!(read_policy(&root).is_empty());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn catalog_parses_control_plane_shape() {
        let raw = r#"{
            "catalogHash": "abc123",
            "skills": [{
                "slug": "travel",
                "name": "差旅报销",
                "version": "1.2.0",
                "hash": "deadbeef",
                "sizeBytes": 2048,
                "url": "/auth/partner/skills/travel/1.2.0/download"
            }]
        }"#;
        let catalog: Catalog = serde_json::from_str(raw).expect("parse");
        assert_eq!(catalog.catalog_hash, "abc123");
        assert_eq!(catalog.skills[0].slug, "travel");
        assert_eq!(catalog.skills[0].size_bytes, 2048);
    }
}
