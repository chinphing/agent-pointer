//! 技能包版本：zip 校验、落盘、发布、下发。
//!
//! 校验口径与客户端保持一致（`crates/pointer-core/src/skills/external.rs`）：
//! 包 ≤ 20 MiB、单条目 ≤ 5 MiB、禁止绝对路径与 `..`、必须含 `SKILL.md`。
//! admin 不依赖 pointer-core，所以常量在这里独立定义——**改一处要同步另一处**，
//! 已在 `docs/zh-CN/design/enterprise-admin-console.md` 标注。

use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use sqlx::{Any, Row};
use zip::ZipArchive;

use crate::db::{now_ts, random_id, sha256_hex, AdminStore};
use crate::resources::{self, KIND_SKILL};

pub const MAX_ZIP_BYTES: usize = 20 * 1024 * 1024;
pub const MAX_ENTRY_BYTES: u64 = 5 * 1024 * 1024;
pub const MAX_ENTRIES: usize = 2_000;
pub const MANIFEST_NAME: &str = "SKILL.md";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillVersion {
    pub id: String,
    pub resource_id: String,
    pub version: String,
    pub dir_hash: String,
    pub size_bytes: i64,
    pub uploaded_by: Option<String>,
    pub created_at: i64,
    pub is_current: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZipSummary {
    /// 整个 zip 的 sha256（客户端用它判断是否需要重新下载）。
    pub dir_hash: String,
    pub size_bytes: i64,
    pub entries: usize,
}

fn normalize_entry(name: &str) -> String {
    let mut s = name.replace('\\', "/");
    while s.starts_with("./") {
        s = s[2..].to_string();
    }
    s
}

/// 校验 zip 并算出内容哈希。任何可疑条目都直接拒绝，不做「尽力而为」。
pub fn inspect_zip(bytes: &[u8]) -> Result<ZipSummary> {
    if bytes.is_empty() {
        return Err(anyhow!("zip 为空"));
    }
    if bytes.len() > MAX_ZIP_BYTES {
        return Err(anyhow!(
            "zip 超过 {} MiB 上限",
            MAX_ZIP_BYTES / 1024 / 1024
        ));
    }

    let mut archive = ZipArchive::new(Cursor::new(bytes)).map_err(|e| anyhow!("不是合法的 zip：{e}"))?;
    if archive.len() > MAX_ENTRIES {
        return Err(anyhow!("条目数超过 {MAX_ENTRIES} 上限"));
    }

    let mut entries = 0usize;
    let mut has_manifest = false;

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|e| anyhow!("读取 zip 条目失败：{e}"))?;

        // enclosed_name() 对绝对路径与 `..` 返回 None —— 路径穿越的第一道闸。
        let safe_path = entry
            .enclosed_name()
            .ok_or_else(|| anyhow!("zip 内含非法路径：{}", entry.name()))?;
        let normalized = normalize_entry(&safe_path.to_string_lossy());
        if normalized.split('/').any(|part| part == "..") {
            return Err(anyhow!("zip 内含非法路径：{}", entry.name()));
        }

        if entry.is_dir() {
            continue;
        }
        if entry.size() > MAX_ENTRY_BYTES {
            return Err(anyhow!(
                "条目 {} 超过 {} MiB 上限",
                entry.name(),
                MAX_ENTRY_BYTES / 1024 / 1024
            ));
        }
        // 真正读一遍：防「声明 1 KB、实际解出 1 GB」的 zip 炸弹。
        let mut sink = Vec::new();
        let mut limited = (&mut entry).take(MAX_ENTRY_BYTES + 1);
        limited
            .read_to_end(&mut sink)
            .map_err(|e| anyhow!("读取条目 {} 失败：{e}", entry.name()))?;
        if sink.len() as u64 > MAX_ENTRY_BYTES {
            return Err(anyhow!("条目 {} 解压后超限", entry.name()));
        }

        if safe_path
            .file_name()
            .map(|n| n.to_string_lossy() == MANIFEST_NAME)
            .unwrap_or(false)
        {
            has_manifest = true;
        }
        entries += 1;
    }

    if entries == 0 {
        return Err(anyhow!("zip 里没有文件"));
    }
    if !has_manifest {
        return Err(anyhow!("技能包必须包含 {MANIFEST_NAME}"));
    }

    Ok(ZipSummary {
        dir_hash: sha256_hex(bytes),
        size_bytes: bytes.len() as i64,
        entries,
    })
}

fn normalize_version(raw: &str) -> Result<String> {
    let version = raw.trim();
    if version.is_empty() {
        return Err(anyhow!("版本号不能为空"));
    }
    if version.chars().count() > 32 {
        return Err(anyhow!("版本号过长（最多 32 字符）"));
    }
    if version
        .chars()
        .any(|c| c.is_whitespace() || c.is_control() || matches!(c, '/' | '\\' | '?' | '#'))
    {
        return Err(anyhow!("版本号不能包含空白或 / \\ ? #"));
    }
    Ok(version.to_string())
}

fn blob_rel_path(resource_id: &str, version: &str) -> String {
    format!("skills/{resource_id}/{version}.zip")
}

/// 上传一个版本：先校验，再落盘，最后写库。
pub async fn add_version(
    store: &AdminStore,
    data_dir: &Path,
    resource_id: &str,
    version: &str,
    bytes: &[u8],
    uploaded_by: Option<&str>,
) -> Result<SkillVersion> {
    let version = normalize_version(version)?;

    let resource = resources::get(store, resource_id)
        .await?
        .ok_or_else(|| anyhow!("资源不存在：{resource_id}"))?;
    if resource.kind != KIND_SKILL {
        return Err(anyhow!("只有技能资源可以上传版本"));
    }

    let summary = inspect_zip(bytes)?;

    let existing = store.sql(
        "SELECT id FROM resource_versions WHERE resource_id = ? AND version = ?",
    );
    if sqlx::query::<Any>(&existing)
        .bind(resource_id)
        .bind(&version)
        .fetch_optional(store.pool())
        .await?
        .is_some()
    {
        return Err(anyhow!("版本 {version} 已存在"));
    }

    let rel = blob_rel_path(resource_id, &version);
    let absolute = data_dir.join(&rel);
    if let Some(parent) = absolute.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // 先写临时文件再改名：中途失败不会留下半个包。
    let tmp = absolute.with_extension("zip.part");
    {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    std::fs::rename(&tmp, &absolute)?;

    let id = random_id();
    let insert = store.sql(
        "INSERT INTO resource_versions
         (id, resource_id, version, dir_hash, blob_path, size_bytes, uploaded_by, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    );
    sqlx::query::<Any>(&insert)
        .bind(&id)
        .bind(resource_id)
        .bind(&version)
        .bind(&summary.dir_hash)
        .bind(&rel)
        .bind(summary.size_bytes)
        .bind(uploaded_by)
        .bind(now_ts())
        .execute(store.pool())
        .await?;

    Ok(SkillVersion {
        id,
        resource_id: resource_id.to_string(),
        version,
        dir_hash: summary.dir_hash,
        size_bytes: summary.size_bytes,
        uploaded_by: uploaded_by.map(str::to_string),
        created_at: now_ts(),
        is_current: false,
    })
}

pub async fn list_versions(
    store: &AdminStore,
    resource_id: &str,
) -> Result<Vec<SkillVersion>> {
    let sql = store.sql(
        "SELECT id, resource_id, version, dir_hash, size_bytes, uploaded_by, created_at
         FROM resource_versions WHERE resource_id = ? ORDER BY created_at DESC",
    );
    let rows = sqlx::query::<Any>(&sql)
        .bind(resource_id)
        .fetch_all(store.pool())
        .await?;

    let current: Option<String> = resources::get(store, resource_id)
        .await?
        .and_then(|r| r.current_version_id);

    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        let id: String = row.try_get(0)?;
        out.push(SkillVersion {
            is_current: current.as_deref() == Some(id.as_str()),
            id,
            resource_id: row.try_get(1)?,
            version: row.try_get(2)?,
            dir_hash: row.try_get(3)?,
            size_bytes: row.try_get(4)?,
            uploaded_by: row.try_get(5)?,
            created_at: row.try_get(6)?,
        });
    }
    Ok(out)
}

pub async fn get_version(store: &AdminStore, version_id: &str) -> Result<Option<SkillVersion>> {
    let sql = store.sql(
        "SELECT id, resource_id, version, dir_hash, size_bytes, uploaded_by, created_at
         FROM resource_versions WHERE id = ?",
    );
    let row = sqlx::query::<Any>(&sql)
        .bind(version_id)
        .fetch_optional(store.pool())
        .await?;
    let Some(row) = row else { return Ok(None) };
    let resource_id: String = row.try_get(1)?;
    let current: Option<String> = resources::get(store, &resource_id)
        .await?
        .and_then(|r| r.current_version_id);
    let id: String = row.try_get(0)?;
    Ok(Some(SkillVersion {
        is_current: current.as_deref() == Some(id.as_str()),
        id,
        resource_id,
        version: row.try_get(2)?,
        dir_hash: row.try_get(3)?,
        size_bytes: row.try_get(4)?,
        uploaded_by: row.try_get(5)?,
        created_at: row.try_get(6)?,
    }))
}

/// 发布：把该版本设为 current，并顺带启用资源（未发布 = 员工拿不到）。
pub async fn publish(store: &AdminStore, resource_id: &str, version_id: &str) -> Result<()> {
    let version = get_version(store, version_id)
        .await?
        .ok_or_else(|| anyhow!("版本不存在：{version_id}"))?;
    if version.resource_id != resource_id {
        return Err(anyhow!("版本不属于该资源"));
    }
    let sql = store.sql(
        "UPDATE resources SET current_version_id = ?, enabled = 1, updated_at = ? WHERE id = ?",
    );
    let changed = sqlx::query::<Any>(&sql)
        .bind(version_id)
        .bind(now_ts())
        .bind(resource_id)
        .execute(store.pool())
        .await?
        .rows_affected();
    if changed == 0 {
        return Err(anyhow!("资源不存在：{resource_id}"));
    }
    Ok(())
}

pub async fn delete_version(
    store: &AdminStore,
    data_dir: &Path,
    version_id: &str,
) -> Result<bool> {
    let Some(version) = get_version(store, version_id).await? else {
        return Ok(false);
    };
    if version.is_current {
        return Err(anyhow!("当前发布版本不能删除，请先发布其它版本"));
    }
    let sql = store.sql("SELECT blob_path FROM resource_versions WHERE id = ?");
    let row = sqlx::query::<Any>(&sql)
        .bind(version_id)
        .fetch_optional(store.pool())
        .await?;
    if let Some(row) = row {
        let rel: Option<String> = row.try_get(0)?;
        if let Some(rel) = rel {
            let _ = std::fs::remove_file(data_dir.join(rel));
        }
    }
    let del = store.sql("DELETE FROM resource_versions WHERE id = ?");
    let changed = sqlx::query::<Any>(&del)
        .bind(version_id)
        .execute(store.pool())
        .await?
        .rows_affected();
    Ok(changed > 0)
}

pub async fn read_blob(data_dir: &Path, version_id: &str, store: &AdminStore) -> Result<Vec<u8>> {
    let sql = store.sql("SELECT blob_path FROM resource_versions WHERE id = ?");
    let row = sqlx::query::<Any>(&sql)
        .bind(version_id)
        .fetch_optional(store.pool())
        .await?
        .ok_or_else(|| anyhow!("版本不存在：{version_id}"))?;
    let rel: Option<String> = row.try_get(0)?;
    let rel = rel.ok_or_else(|| anyhow!("版本没有对应的文件"))?;
    let path = data_dir.join(&rel);
    // 兜底：即便库里被写脏，也不允许读到 data_dir 之外。
    let canonical_root = data_dir.canonicalize().unwrap_or_else(|_| data_dir.to_path_buf());
    let canonical = path.canonicalize().unwrap_or(path.clone());
    if !canonical.starts_with(&canonical_root) {
        return Err(anyhow!("非法的存储路径"));
    }
    Ok(std::fs::read(canonical)?)
}

/// 找到某技能资源下、指定版本号的版本记录。
pub async fn find_version(
    store: &AdminStore,
    resource_id: &str,
    version: &str,
) -> Result<Option<SkillVersion>> {
    let sql = store.sql(
        "SELECT id FROM resource_versions WHERE resource_id = ? AND version = ?",
    );
    let row = sqlx::query::<Any>(&sql)
        .bind(resource_id)
        .bind(version)
        .fetch_optional(store.pool())
        .await?;
    let Some(row) = row else { return Ok(None) };
    let id: String = row.try_get(0)?;
    get_version(store, &id).await
}

/// 构造一个「只含 SKILL.md」的最小合法 zip，供测试与冒烟使用。
pub fn build_test_zip(manifest: &str, extra: &[(&str, &str)]) -> Vec<u8> {
    use zip::write::SimpleFileOptions;
    let mut buffer = Vec::new();
    {
        let cursor = Cursor::new(&mut buffer);
        let mut writer = zip::ZipWriter::new(cursor);
        let options = SimpleFileOptions::default();
        writer.start_file(MANIFEST_NAME, options).expect("start manifest");
        writer.write_all(manifest.as_bytes()).expect("write manifest");
        for (name, body) in extra {
            writer.start_file(*name, options).expect("start entry");
            writer.write_all(body.as_bytes()).expect("write entry");
        }
        writer.finish().expect("finish zip");
    }
    buffer
}

pub fn build_zip_with_entries(entries: &[(&str, &str)]) -> Vec<u8> {
    use zip::write::SimpleFileOptions;
    let mut buffer = Vec::new();
    {
        let cursor = Cursor::new(&mut buffer);
        let mut writer = zip::ZipWriter::new(cursor);
        let options = SimpleFileOptions::default();
        for (name, body) in entries {
            writer.start_file(*name, options).expect("start entry");
            writer.write_all(body.as_bytes()).expect("write entry");
        }
        writer.finish().expect("finish zip");
    }
    buffer
}

pub fn temp_data_dir(tag: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!("pointer-admin-test-{tag}-{}", random_id()));
    path
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resources::{self, KIND_PROVIDER};
    use crate::schema::DEFAULT_TENANT;

    async fn store() -> AdminStore {
        AdminStore::connect_in_memory().await.expect("store")
    }

    async fn skill(store: &AdminStore, slug: &str) -> String {
        resources::create(
            store,
            DEFAULT_TENANT,
            KIND_SKILL,
            slug,
            slug,
            "",
            None,
            "{}",
            None,
        )
        .await
        .expect("skill")
        .id
    }

    #[test]
    fn valid_zip_passes_and_hashes_content() {
        let zip = build_test_zip("# demo", &[("references/a.md", "hello")]);
        let summary = inspect_zip(&zip).expect("inspect");
        assert_eq!(summary.entries, 2);
        assert_eq!(summary.size_bytes, zip.len() as i64);
        assert_eq!(summary.dir_hash.len(), 64);
        assert_eq!(summary.dir_hash, sha256_hex(&zip));
    }

    #[test]
    fn zip_without_manifest_is_rejected() {
        let zip = build_zip_with_entries(&[("readme.txt", "hi")]);
        let err = inspect_zip(&zip).expect_err("no manifest");
        assert!(err.to_string().contains("SKILL.md"), "got: {err}");
    }

    #[test]
    fn path_traversal_is_rejected() {
        let zip = build_zip_with_entries(&[("../evil.txt", "x")]);
        assert!(inspect_zip(&zip).is_err());
    }

    #[test]
    fn absolute_path_is_rejected() {
        let zip = build_zip_with_entries(&[("/etc/passwd", "x")]);
        assert!(inspect_zip(&zip).is_err());
    }

    #[test]
    fn empty_bytes_are_rejected() {
        assert!(inspect_zip(&[]).is_err());
    }

    #[test]
    fn garbage_is_not_a_zip() {
        assert!(inspect_zip(b"not a zip at all").is_err());
    }

    #[test]
    fn version_normalization() {
        assert_eq!(normalize_version(" 1.2.0 ").expect("ok"), "1.2.0");
        assert!(normalize_version("").is_err());
        assert!(normalize_version("1 2").is_err());
        assert!(normalize_version("../x").is_err());
        assert!(normalize_version(&"9".repeat(33)).is_err());
    }

    #[tokio::test]
    async fn add_list_publish_flow() {
        let store = store().await;
        let data_dir = temp_data_dir("flow");
        let rid = skill(&store, "travel").await;
        let zip = build_test_zip("# travel", &[]);

        let v1 = add_version(&store, &data_dir, &rid, "1.0.0", &zip, Some("admin"))
            .await
            .expect("add v1");
        assert!(!v1.is_current);
        assert!(data_dir.join(blob_rel_path(&rid, "1.0.0")).exists());

        let err = add_version(&store, &data_dir, &rid, "1.0.0", &zip, None)
            .await
            .expect_err("duplicate");
        assert!(err.to_string().contains("已存在"), "got: {err}");

        publish(&store, &rid, &v1.id).await.expect("publish");
        let listed = list_versions(&store, &rid).await.expect("list");
        assert_eq!(listed.len(), 1);
        assert!(listed[0].is_current);

        let resource = resources::get(&store, &rid).await.expect("get").expect("present");
        assert!(resource.enabled, "发布后应自动启用");
        assert_eq!(resource.current_version_id.as_deref(), Some(v1.id.as_str()));

        let blob = read_blob(&data_dir, &v1.id, &store).await.expect("read");
        assert_eq!(blob, zip);

        let _ = std::fs::remove_dir_all(&data_dir);
    }

    #[tokio::test]
    async fn current_version_cannot_be_deleted() {
        let store = store().await;
        let data_dir = temp_data_dir("del");
        let rid = skill(&store, "travel").await;
        let zip = build_test_zip("# x", &[]);
        let v1 = add_version(&store, &data_dir, &rid, "1.0.0", &zip, None)
            .await
            .expect("v1");
        publish(&store, &rid, &v1.id).await.expect("publish");

        assert!(delete_version(&store, &data_dir, &v1.id).await.is_err());

        let v2 = add_version(&store, &data_dir, &rid, "1.1.0", &zip, None)
            .await
            .expect("v2");
        publish(&store, &rid, &v2.id).await.expect("publish v2");
        assert!(delete_version(&store, &data_dir, &v1.id).await.expect("delete v1"));
        assert!(!data_dir.join(blob_rel_path(&rid, "1.0.0")).exists());

        let _ = std::fs::remove_dir_all(&data_dir);
    }

    #[tokio::test]
    async fn only_skill_resources_accept_versions() {
        let store = store().await;
        let data_dir = temp_data_dir("kind");
        let provider = resources::create(
            &store,
            DEFAULT_TENANT,
            KIND_PROVIDER,
            "qwen",
            "qwen",
            "",
            None,
            "{}",
            None,
        )
        .await
        .expect("provider");
        let zip = build_test_zip("# x", &[]);
        let err = add_version(&store, &data_dir, &provider.id, "1.0.0", &zip, None)
            .await
            .expect_err("not a skill");
        assert!(err.to_string().contains("技能"), "got: {err}");
        let _ = std::fs::remove_dir_all(&data_dir);
    }
}
