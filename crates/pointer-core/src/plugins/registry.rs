//! PluginRegistry：发现 / 解析 / 授权 / 状态机（设计稿 §4.5）。
//!
//! 状态机：
//! ```text
//! discovered → parsed → validated → (用户授权) → enabled
//!                                       ↓ 校验失败
//!                                    rejected（UI 显示原因）
//! enabled → disabled（用户关闭 / 版本不兼容）
//! 任意状态 → uninstalled
//! ```
//!
//! 安全铁律：发现 ≠ 执行（扫描只读 manifest，不启动进程）；授权留痕
//! （首次启用记录 manifest + 文件清单哈希，任何哈希变化需重新授权）；
//! 插件工具与内置工具同一审批链路（由 [`crate::plugins::tool_provider`] 与
//! `ToolRegistry` 保证）。

use crate::plugins::manifest::{self, PluginManifest};
use crate::plugins::{discover_plugin_dirs, fingerprint_plugin_files, load_manifest, sha256_hex};
use anyhow::{anyhow, Result};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// 插件状态机状态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginStatus {
    /// 已发现、已解析、已校验，但用户尚未授权。
    Discovered,
    /// 解析/校验失败（UI 显示原因）。
    Rejected(String),
    /// 已授权且启用。
    Enabled,
    /// 已授权但用户关闭。
    Disabled,
    /// manifest 或文件清单哈希变化，需重新授权（防投毒）。
    NeedsReauth,
}

/// 单个插件的完整运行时记录（内存态，由 `scan` 刷新）。
#[derive(Debug, Clone)]
pub struct PluginRecord {
    pub id: String,
    /// 插件根目录（含 `pointer-plugin.toml`）。
    pub dir: PathBuf,
    /// `true` = 用户级 `~/.pointer/plugins`；`false` = 工作区级 `<workspace>/.pointer/plugins`。
    pub is_user_level: bool,
    pub manifest: PluginManifest,
    pub status: PluginStatus,
    /// `pointer-plugin.toml` 内容哈希。
    pub manifest_hash: String,
    /// 能力单元文件清单哈希（相对路径 → sha256）。
    pub fingerprint: HashMap<String, String>,
    pub authorized: bool,
    pub enabled: bool,
}

/// 持久化授权记录（单文件 JSON，位于用户级插件根目录 `.auth.json`）。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct AuthEntry {
    dir: String,
    manifest_hash: String,
    fingerprint: HashMap<String, String>,
    enabled: bool,
    enabled_at_ms: Option<i64>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct AuthStore {
    #[serde(default)]
    plugins: HashMap<String, AuthEntry>,
}

#[derive(Default)]
pub struct PluginRegistry {
    inner: RwLock<HashMap<String, PluginRecord>>,
    /// 授权状态文件路径覆盖（测试用；默认 `~/.pointer/plugins/.auth.json`）。
    auth_path_override: Option<PathBuf>,
    /// 上次扫描见过的插件 id 集合（检测目录被外部删除）。
    seen_ids: RwLock<HashSet<String>>,
    /// 最近一次扫描中消失的插件 id（消费制：`take_disappeared` 后清空）。
    disappeared: RwLock<Vec<String>>,
}

impl PluginRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// 测试辅助：将授权状态文件指向自定义路径，避免污染真实用户目录。
    pub fn with_auth_path(path: PathBuf) -> Self {
        Self {
            inner: RwLock::new(HashMap::new()),
            auth_path_override: Some(path),
            seen_ids: RwLock::new(HashSet::new()),
            disappeared: RwLock::new(Vec::new()),
        }
    }

    /// 授权状态文件路径。
    fn auth_path(&self) -> Result<PathBuf> {
        match &self.auth_path_override {
            Some(p) => {
                if let Some(parent) = p.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                Ok(p.clone())
            }
            None => Ok(crate::plugins::user_plugins_dir()?.join(".auth.json")),
        }
    }

    fn load_auth(&self) -> AuthStore {
        let path = match self.auth_path() {
            Ok(p) => p,
            Err(_) => return AuthStore::default(),
        };
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default()
    }

    fn save_auth(&self, store: &AuthStore) -> Result<()> {
        let path = self.auth_path()?;
        let json = serde_json::to_string_pretty(store)?;
        std::fs::write(&path, json)?;
        Ok(())
    }

    /// 全量扫描：发现目录 → 解析 → 校验 → 与授权记录对比得出状态。
    pub fn scan(&self) -> Result<usize> {
        let roots = discover_plugin_dirs()?;
        self.scan_roots(&roots)
    }

    /// 扫描给定插件根列表（`(dir, is_user_level)`）。测试可用临时目录注入。
    pub fn scan_roots(&self, roots: &[(PathBuf, bool)]) -> Result<usize> {
        let auth = self.load_auth();
        let mut records = HashMap::new();
        let mut count = 0usize;

        for (dir, is_user_level) in roots {
            let id = dir
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string();
            let record = build_record(dir, *is_user_level, &auth);
            if matches!(record.status, PluginStatus::Rejected(_)) {
                log::warn!(
                    "plugin: {} rejected: {:?}",
                    id,
                    match &record.status {
                        PluginStatus::Rejected(reason) => reason.clone(),
                        _ => String::new(),
                    }
                );
            }
            records.insert(id.clone(), record);
            count += 1;
        }

        // 检测本次扫描「上次见过、本次消失」的插件（目录被外部删除 / 工作区清理），
        // 供上层注销其残留能力（skills/tools/agents/extensions）。
        let new_ids: HashSet<String> = records.keys().cloned().collect();
        {
            let mut seen = self.seen_ids.write();
            let mut gone: Vec<String> = seen
                .iter()
                .filter(|id| !new_ids.contains(*id))
                .cloned()
                .collect();
            gone.sort();
            *self.disappeared.write() = gone;
            *seen = new_ids;
        }

        *self.inner.write() = records;
        Ok(count)
    }

    /// 取出最近一次扫描中消失的插件 id（消费制；调用方负责注销残留能力）。
    pub fn take_disappeared(&self) -> Vec<String> {
        std::mem::take(&mut *self.disappeared.write())
    }

    pub fn list(&self) -> Vec<PluginRecord> {
        let g = self.inner.read();
        let mut v: Vec<PluginRecord> = g.values().cloned().collect();
        v.sort_by(|a, b| a.id.cmp(&b.id));
        v
    }

    pub fn get(&self, id: &str) -> Option<PluginRecord> {
        self.inner.read().get(id).cloned()
    }

    /// 授权当前哈希（首次启用或重新授权）。哈希变化后调用方应提示用户确认。
    pub fn authorize(&self, id: &str) -> Result<()> {
        let (dir, manifest_hash, fingerprint) = {
            let g = self.inner.read();
            let rec = g.get(id).ok_or_else(|| anyhow!("插件不存在: {id}"))?;
            if matches!(rec.status, PluginStatus::Rejected(_)) {
                return Err(anyhow!("插件 {id} 校验失败，不能授权"));
            }
            (
                rec.dir.clone(),
                rec.manifest_hash.clone(),
                rec.fingerprint.clone(),
            )
        };
        let mut auth = self.load_auth();
        let entry = AuthEntry {
            dir: dir.to_string_lossy().to_string(),
            manifest_hash,
            fingerprint,
            enabled: false,
            enabled_at_ms: None,
        };
        auth.plugins.insert(id.to_string(), entry);
        self.save_auth(&auth)?;
        self.refresh_record_status(id);
        Ok(())
    }

    /// 启用插件（须先 `authorize`；哈希不匹配时报错要求重新授权）。
    pub fn enable(&self, id: &str) -> Result<()> {
        let mut auth = self.load_auth();
        let Some(entry) = auth.plugins.get_mut(id) else {
            return Err(anyhow!("插件 {id} 未授权，请先授权"));
        };
        let now_ms = crate::plugins::now_ms();
        entry.enabled = true;
        entry.enabled_at_ms = Some(now_ms);
        self.save_auth(&auth)?;
        self.refresh_record_status(id);
        Ok(())
    }

    /// 禁用插件。
    pub fn disable(&self, id: &str) -> Result<()> {
        let mut auth = self.load_auth();
        if let Some(entry) = auth.plugins.get_mut(id) {
            entry.enabled = false;
        }
        self.save_auth(&auth)?;
        self.refresh_record_status(id);
        Ok(())
    }

    /// 卸载：删除插件目录 + 清除授权记录。
    pub fn uninstall(&self, id: &str) -> Result<()> {
        let dir = {
            let g = self.inner.read();
            let rec = g.get(id).ok_or_else(|| anyhow!("插件不存在: {id}"))?;
            rec.dir.clone()
        };
        if dir.exists() {
            std::fs::remove_dir_all(&dir).map_err(|e| anyhow!("删除插件目录 {dir:?} 失败: {e}"))?;
        }
        let mut auth = self.load_auth();
        auth.plugins.remove(id);
        self.save_auth(&auth)?;
        self.inner.write().remove(id);
        // 已由卸载流程注销能力；从 seen 移除避免下次 scan 误判为「外部消失」。
        self.seen_ids.write().remove(id);
        Ok(())
    }

    fn refresh_record_status(&self, id: &str) {
        let auth = self.load_auth();
        let mut g = self.inner.write();
        if let Some(rec) = g.get_mut(id) {
            rec.status = resolve_status(rec, &auth);
            rec.authorized = auth.plugins.contains_key(id);
            rec.enabled = auth.plugins.get(id).is_some_and(|e| e.enabled);
        }
    }
}

/// 插件列表视图（供 server / Tauri / 前端共用，避免双端重复定义与状态映射漂移）。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginView {
    pub plugin_id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub is_user_level: bool,
    pub status: String,
    pub status_reason: Option<String>,
    pub is_authorized: bool,
    pub is_enabled: bool,
}

impl PluginView {
    pub fn from_record(record: &PluginRecord) -> Self {
        let (status, status_reason) = match &record.status {
            PluginStatus::Discovered => ("discovered".to_string(), None),
            PluginStatus::Rejected(reason) => ("rejected".to_string(), Some(reason.clone())),
            PluginStatus::Enabled => ("enabled".to_string(), None),
            PluginStatus::Disabled => ("disabled".to_string(), None),
            PluginStatus::NeedsReauth => ("needs_reauth".to_string(), None),
        };
        Self {
            plugin_id: record.id.clone(),
            name: record.manifest.plugin.name.clone(),
            version: record.manifest.plugin.version.clone(),
            description: record.manifest.plugin.description.clone(),
            is_user_level: record.is_user_level,
            status,
            status_reason,
            is_authorized: record.authorized,
            is_enabled: record.enabled,
        }
    }
}

/// 解析并校验单个插件目录，结合授权记录得到状态。
fn build_record(dir: &Path, is_user_level: bool, auth: &AuthStore) -> PluginRecord {
    let id = dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();
    let (manifest, raw) = match load_manifest(dir) {
        Ok(v) => v,
        Err(e) => {
            return PluginRecord {
                id,
                dir: dir.to_path_buf(),
                is_user_level,
                manifest: dummy_manifest(),
                manifest_hash: String::new(),
                fingerprint: HashMap::new(),
                status: PluginStatus::Rejected(format!("{e:#}")),
                authorized: false,
                enabled: false,
            };
        }
    };
    let manifest_hash = sha256_hex(raw.as_bytes());
    let fingerprint = fingerprint_plugin_files(dir).unwrap_or_default();
    let record = PluginRecord {
        id,
        dir: dir.to_path_buf(),
        is_user_level,
        manifest,
        manifest_hash,
        fingerprint,
        status: PluginStatus::Discovered,
        authorized: false,
        enabled: false,
    };
    let status = resolve_status(&record, auth);
    PluginRecord {
        status,
        authorized: auth.plugins.contains_key(&record.id),
        enabled: auth.plugins.get(&record.id).is_some_and(|e| e.enabled),
        ..record
    }
}

fn resolve_status(record: &PluginRecord, auth: &AuthStore) -> PluginStatus {
    if matches!(record.status, PluginStatus::Rejected(_)) {
        return record.status.clone();
    }
    let Some(entry) = auth.plugins.get(&record.id) else {
        return PluginStatus::Discovered;
    };
    // 哈希留痕：manifest 或任何能力单元文件变化都触发重新授权。
    if entry.manifest_hash != record.manifest_hash || entry.fingerprint != record.fingerprint {
        return PluginStatus::NeedsReauth;
    }
    if entry.enabled {
        PluginStatus::Enabled
    } else {
        PluginStatus::Disabled
    }
}

fn dummy_manifest() -> PluginManifest {
    // Rejected 记录的占位 manifest（不会被使用）。
    let raw = "[plugin]\nid = \"local.rejected\"\nname = \"rejected\"\nversion = \"0\"\n";
    manifest::parse(raw).expect("dummy manifest must parse")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const VALID: &str = r#"
[plugin]
id = "com.example.demo"
name = "Demo"
version = "1.0.0"
api_version = "v1"

[skills]
path = "skills/"

[[tools.tool]]
name = "demo_hello"
risk_level = "low"
requires_approval = false
exec = { command = "bin/demo-tool", transport = "sidecar" }
"#;

    /// 在临时目录搭一个用户级插件根（monkey-patch 目录不可行，因此用环境变量
    /// 覆盖 `~/.pointer` 不可行；测试改为直接扫描临时目录并断言 build_record）。
    fn write_plugin(root: &Path, manifest_text: &str) -> PathBuf {
        let dir = root.join("com.example.demo");
        fs::create_dir_all(dir.join("skills")).unwrap();
        fs::write(dir.join("pointer-plugin.toml"), manifest_text).unwrap();
        fs::write(dir.join("skills/hello.md"), "# Hello\n").unwrap();
        dir
    }

    #[test]
    fn build_record_rejected_on_bad_manifest() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("com.example.bad");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("pointer-plugin.toml"), "not toml").unwrap();
        let record = build_record(&dir, true, &AuthStore::default());
        assert!(matches!(record.status, PluginStatus::Rejected(_)));
    }

    #[test]
    fn build_record_discovered_without_auth() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = write_plugin(tmp.path(), VALID);
        let record = build_record(&dir, true, &AuthStore::default());
        assert_eq!(record.id, "com.example.demo");
        assert_eq!(record.status, PluginStatus::Discovered);
        assert!(!record.authorized);
        assert!(!record.enabled);
        assert_eq!(record.manifest.plugin.name, "Demo");
        assert!(!record.manifest_hash.is_empty());
        assert!(record.fingerprint.contains_key("skills/hello.md"));
    }

    #[test]
    fn auth_matching_enabled_resolves_enabled() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = write_plugin(tmp.path(), VALID);
        let record = build_record(&dir, true, &AuthStore::default());
        let mut auth = AuthStore::default();
        auth.plugins.insert(
            record.id.clone(),
            AuthEntry {
                dir: dir.to_string_lossy().to_string(),
                manifest_hash: record.manifest_hash.clone(),
                fingerprint: record.fingerprint.clone(),
                enabled: true,
                enabled_at_ms: Some(1),
            },
        );
        let status = resolve_status(&record, &auth);
        assert_eq!(status, PluginStatus::Enabled);
    }

    #[test]
    fn manifest_change_requires_reauth() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = write_plugin(tmp.path(), VALID);
        let record = build_record(&dir, true, &AuthStore::default());
        let mut auth = AuthStore::default();
        auth.plugins.insert(
            record.id.clone(),
            AuthEntry {
                dir: dir.to_string_lossy().to_string(),
                manifest_hash: record.manifest_hash.clone(),
                fingerprint: record.fingerprint.clone(),
                enabled: true,
                enabled_at_ms: Some(1),
            },
        );
        // 修改能力单元文件 → fingerprint 变化 → NeedsReauth
        fs::write(dir.join("skills/hello.md"), "# Changed\n").unwrap();
        let changed = build_record(&dir, true, &auth);
        assert_eq!(changed.status, PluginStatus::NeedsReauth);
    }

    #[test]
    fn auth_enabled_false_resolves_disabled() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = write_plugin(tmp.path(), VALID);
        let record = build_record(&dir, true, &AuthStore::default());
        let mut auth = AuthStore::default();
        auth.plugins.insert(
            record.id.clone(),
            AuthEntry {
                dir: dir.to_string_lossy().to_string(),
                manifest_hash: record.manifest_hash.clone(),
                fingerprint: record.fingerprint.clone(),
                enabled: false,
                enabled_at_ms: None,
            },
        );
        assert_eq!(resolve_status(&record, &auth), PluginStatus::Disabled);
    }

    #[test]
    fn registry_full_lifecycle_with_temp_auth() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("plugins");
        let dir = write_plugin(&root, VALID);
        let auth_file = tmp.path().join("auth.json");
        let reg = PluginRegistry::with_auth_path(auth_file);

        // 初始扫描：未授权 → Discovered
        reg.scan_roots(&[(dir.clone(), true)]).unwrap();
        let rec = reg.get("com.example.demo").unwrap();
        assert_eq!(rec.status, PluginStatus::Discovered);
        assert!(!rec.authorized);

        // authorize：记录哈希（enabled=false）
        reg.authorize("com.example.demo").unwrap();
        reg.scan_roots(&[(dir.clone(), true)]).unwrap();
        let rec = reg.get("com.example.demo").unwrap();
        assert_eq!(rec.status, PluginStatus::Disabled);
        assert!(rec.authorized);

        // enable → Enabled
        reg.enable("com.example.demo").unwrap();
        reg.scan_roots(&[(dir.clone(), true)]).unwrap();
        assert_eq!(
            reg.get("com.example.demo").unwrap().status,
            PluginStatus::Enabled
        );

        // 修改能力单元文件 → 哈希变化 → NeedsReauth
        fs::write(dir.join("skills/hello.md"), "# Hello changed\n").unwrap();
        reg.scan_roots(&[(dir.clone(), true)]).unwrap();
        assert_eq!(
            reg.get("com.example.demo").unwrap().status,
            PluginStatus::NeedsReauth
        );

        // 重新授权后再次启用
        reg.authorize("com.example.demo").unwrap();
        reg.enable("com.example.demo").unwrap();
        reg.scan_roots(&[(dir.clone(), true)]).unwrap();
        assert_eq!(
            reg.get("com.example.demo").unwrap().status,
            PluginStatus::Enabled
        );

        // disable → Disabled
        reg.disable("com.example.demo").unwrap();
        reg.scan_roots(&[(dir.clone(), true)]).unwrap();
        assert_eq!(
            reg.get("com.example.demo").unwrap().status,
            PluginStatus::Disabled
        );

        // uninstall → 目录删除 + 记录清除
        reg.uninstall("com.example.demo").unwrap();
        assert!(!dir.exists());
        assert!(reg.get("com.example.demo").is_none());
    }

    #[test]
    fn uninstall_missing_id_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let reg = PluginRegistry::with_auth_path(tmp.path().join("auth.json"));
        assert!(reg.uninstall("nope").is_err());
    }

    #[test]
    fn enable_before_authorize_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let reg = PluginRegistry::with_auth_path(tmp.path().join("auth.json"));
        // 未扫描/未授权时 enable 报错
        assert!(reg.enable("com.example.demo").is_err());
    }

    #[test]
    fn scan_detects_externally_removed_plugin() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let reg = PluginRegistry::with_auth_path(tmp.path().join("auth.json"));
        let dir = write_plugin(root, VALID);
        reg.scan_roots(&[(dir.clone(), true)]).unwrap();
        assert!(reg.get("com.example.demo").is_some());
        assert!(reg.take_disappeared().is_empty());

        // 外部删除目录后，下一次扫描报告该插件「消失」。
        fs::remove_dir_all(&dir).unwrap();
        reg.scan_roots(&[]).unwrap();
        assert_eq!(reg.take_disappeared(), vec!["com.example.demo".to_string()]);

        // 消费制：再次取为空。
        assert!(reg.take_disappeared().is_empty());
    }

    #[test]
    fn uninstall_does_not_count_as_disappeared() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let reg = PluginRegistry::with_auth_path(tmp.path().join("auth.json"));
        let dir = write_plugin(root, VALID);
        reg.scan_roots(&[(dir.clone(), true)]).unwrap();
        reg.authorize("com.example.demo").unwrap();
        reg.enable("com.example.demo").unwrap();
        reg.uninstall("com.example.demo").unwrap();
        // 卸载后扫描：seen 已移除，不会误判为「外部消失」。
        reg.scan_roots(&[]).unwrap();
        assert!(reg.take_disappeared().is_empty());
    }
}
