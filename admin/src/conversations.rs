//! 会话快照：客户端上报用量时顺带上传的对话内容，供管理员按需查看。
//!
//! 数据来源是客户端 `conversation_snapshot` 生成的 zip（内含
//! `conversation_snapshot.json`）：每个 agent 实例一份，包含该实例的助手发言、
//! 触发它的用户消息、以及工具调用结果（内容已截断）。
//!
//! 存储策略跟 `resource_versions` 一致——**大块内容落盘，库里只存索引和预览**：
//! - zip 原件存 `{data_dir}/conversations/yyyy-mm/{id}.zip`
//! - 库里存 conversation_id / 用户 / 模型 / 消息数 / 首条用户消息预览 / 大小
//!
//! 这样既不用给 MySQL 换 MEDIUMTEXT，也让「列表页」只扫小字段。

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde_json::{json, Value};
use sqlx::any::Any;
use sqlx::Row;

use crate::config::AdminConfig;
use crate::db::{now_ts, random_id, AdminStore};

/// 列表里展示的预览长度（首条用户消息）。
const PREVIEW_CHARS: usize = 120;

/// 列表页「共 N 条」的计数上限。
///
/// 精确 COUNT 要扫完整张表：100 万行 ~190ms、500 万行 ~940ms，而列表本身
/// 只要 0.04ms——每次翻页都被这个计数拖住。封顶到 1 万行后代价恒定（~2ms），
/// 超过就显示「10000+」，对「有没有更多」这个用途足够。
const COUNT_CAP: i64 = 10_000;

/// 一条快照的索引信息（不含消息正文）。
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub id: String,
    pub user_id: String,
    pub user_name: String,
    pub conversation_id: String,
    pub agent_instance_id: String,
    pub agent_role_id: Option<String>,
    pub model: String,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub message_count: i64,
    pub preview: String,
    pub size_bytes: i64,
    pub created_at: i64,
}

/// 上报里带的消息元数据（来自 `UsageReport`）。
pub struct ReportMeta<'a> {
    pub request_id: Option<&'a str>,
    /// 下面三个 id 优先用 zip 里的，zip 缺了才用上报字段兜底。
    pub conversation_id: Option<&'a str>,
    pub agent_instance_id: Option<&'a str>,
    pub agent_role_id: Option<&'a str>,
    pub model: &'a str,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
}

fn snapshot_str(snapshot: &Value, key: &str) -> Option<String> {
    snapshot
        .get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
}

fn blob_dir(config: &AdminConfig) -> PathBuf {
    config.data_dir.join("conversations")
}

/// 从 zip 里取出 `conversation_snapshot.json`。
fn read_snapshot_from_zip(bytes: &[u8]) -> Result<Value> {
    let reader = std::io::Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(reader).context("快照 zip 打不开")?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let name = file.name().map(|n| n.to_string()).unwrap_or_default();
        if name.ends_with("conversation_snapshot.json") {
            let mut buf = String::new();
            use std::io::Read;
            file.read_to_string(&mut buf)?;
            return serde_json::from_str(&buf).context("快照 JSON 解析失败");
        }
    }
    Err(anyhow::anyhow!("zip 里没有 conversation_snapshot.json"))
}

fn preview_of(snapshot: &Value) -> String {
    let Some(messages) = snapshot.get("messages").and_then(|m| m.as_array()) else {
        return String::new();
    };
    let first_user = messages
        .iter()
        .find(|m| m.get("role").and_then(|r| r.as_str()) == Some("user"))
        .and_then(|m| m.get("content").and_then(|c| c.as_str()))
        .unwrap_or("");
    let text = first_user.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() <= PREVIEW_CHARS {
        return text;
    }
    format!("{}…", text.chars().take(PREVIEW_CHARS).collect::<String>())
}

/// 落盘一份快照。返回 `false` 表示同一个 `request_id` 已经存过（幂等）。
pub async fn record(
    store: &AdminStore,
    config: &AdminConfig,
    tenant_id: &str,
    user_id: &str,
    meta: &ReportMeta<'_>,
    archive: &[u8],
) -> Result<bool> {
    // 幂等：同一条上报重放不再存第二份。
    if let Some(request_id) = meta.request_id {
        let sql = store.sql(
            "SELECT id FROM conversation_snapshots WHERE tenant_id = ? AND request_id = ? LIMIT 1",
        );
        let existing = sqlx::query::<Any>(&sql)
            .bind(tenant_id)
            .bind(request_id)
            .fetch_optional(store.pool())
            .await?;
        if existing.is_some() {
            return Ok(false);
        }
    }

    let snapshot = read_snapshot_from_zip(archive)?;
    let conversation_id = snapshot_str(&snapshot, "conversation_id")
        .or_else(|| meta.conversation_id.map(|s| s.to_string()))
        .unwrap_or_default();
    let agent_instance_id = snapshot_str(&snapshot, "agent_instance_id")
        .or_else(|| meta.agent_instance_id.map(|s| s.to_string()))
        .unwrap_or_default();
    let agent_role_id = snapshot_str(&snapshot, "agent_role_id")
        .or_else(|| meta.agent_role_id.map(|s| s.to_string()));
    let message_count = snapshot
        .get("messages")
        .and_then(|m| m.as_array())
        .map(|m| m.len() as i64)
        .unwrap_or(0);
    let preview = preview_of(&snapshot);

    let id = random_id();
    let created = now_ts();
    let month = crate::oauth::token::month_prefix(created);
    let dir = blob_dir(config).join(&month);
    std::fs::create_dir_all(&dir).with_context(|| format!("创建快照目录 {}", dir.display()))?;
    let path = dir.join(format!("{id}.zip"));
    std::fs::write(&path, archive).with_context(|| format!("写快照 {}", path.display()))?;

    let sql = store.sql(
        "INSERT INTO conversation_snapshots
         (id, tenant_id, user_id, conversation_id, agent_instance_id, agent_role_id,
          request_id, model, prompt_tokens, completion_tokens, message_count,
          preview, blob_path, size_bytes, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    );
    sqlx::query::<Any>(&sql)
        .bind(&id)
        .bind(tenant_id)
        .bind(user_id)
        .bind(&conversation_id)
        .bind(&agent_instance_id)
        .bind(&agent_role_id)
        .bind(meta.request_id)
        .bind(meta.model)
        .bind(meta.prompt_tokens)
        .bind(meta.completion_tokens)
        .bind(message_count)
        .bind(&preview)
        .bind(path.to_string_lossy().to_string())
        .bind(archive.len() as i64)
        .bind(created)
        .execute(store.pool())
        .await?;
    Ok(true)
}

/// 列表：按时间倒序，可按用户过滤。
///
/// 返回 `(本页数据, 总数, 总数是否被 COUNT_CAP 截断)`。
pub async fn list(
    store: &AdminStore,
    tenant_id: &str,
    user_id: Option<&str>,
    limit: i64,
    offset: i64,
) -> Result<(Vec<Snapshot>, i64, bool)> {
    let limit = limit.clamp(1, 200);
    let offset = offset.max(0);
    // 分两段：先在子查询里用索引取「这一页的 id」，再回表取字段、关联成员名。
    //
    // 直接写 `LEFT JOIN users ... ORDER BY created_at DESC LIMIT 50` 会让
    // MySQL 选 `uq_snapshot_request(tenant_id)` 去扫全部行再排序——百万行实测
    // 3.5 秒。改成子查询后走 idx_snapshot_tenant_time 反向扫 50 行，0.35 毫秒。
    const PAGE_COLUMNS: &str = "s.id, s.user_id, COALESCE(u.display_name, u.username, s.user_id) AS user_name,
                s.conversation_id, s.agent_instance_id, s.agent_role_id, s.model,
                s.prompt_tokens, s.completion_tokens, s.message_count, s.preview,
                s.size_bytes, s.created_at";
    let (count_sql, page_sql) = match user_id {
        Some(_) => (
            store.sql(&format!(
                "SELECT COUNT(*) FROM (SELECT 1 FROM conversation_snapshots
                   WHERE tenant_id = ? AND user_id = ? LIMIT {COUNT_CAP}) t"
            )),
            store.sql(&format!(
                "SELECT {PAGE_COLUMNS}
                   FROM (SELECT id FROM conversation_snapshots
                          WHERE tenant_id = ? AND user_id = ?
                          ORDER BY created_at DESC, id DESC LIMIT ? OFFSET ?) page
                   JOIN conversation_snapshots s ON s.id = page.id
                   LEFT JOIN users u ON u.id = s.user_id
                  ORDER BY s.created_at DESC, s.id DESC"
            )),
        ),
        None => (
            store.sql(&format!(
                "SELECT COUNT(*) FROM (SELECT 1 FROM conversation_snapshots
                   WHERE tenant_id = ? LIMIT {COUNT_CAP}) t"
            )),
            store.sql(&format!(
                "SELECT {PAGE_COLUMNS}
                   FROM (SELECT id FROM conversation_snapshots
                          WHERE tenant_id = ?
                          ORDER BY created_at DESC, id DESC LIMIT ? OFFSET ?) page
                   JOIN conversation_snapshots s ON s.id = page.id
                   LEFT JOIN users u ON u.id = s.user_id
                  ORDER BY s.created_at DESC, s.id DESC"
            )),
        ),
    };

    let mut count_query = sqlx::query::<Any>(&count_sql).bind(tenant_id);
    if let Some(uid) = user_id {
        count_query = count_query.bind(uid);
    }
    let total: i64 = count_query.fetch_one(store.pool()).await?.try_get(0)?;

    let mut page_query = sqlx::query::<Any>(&page_sql).bind(tenant_id);
    if let Some(uid) = user_id {
        page_query = page_query.bind(uid);
    }
    let rows = page_query
        .bind(limit)
        .bind(offset)
        .fetch_all(store.pool())
        .await?;

    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        out.push(Snapshot {
            id: row.try_get(0)?,
            user_id: row.try_get(1)?,
            user_name: row.try_get(2)?,
            conversation_id: row.try_get(3)?,
            agent_instance_id: row.try_get(4)?,
            agent_role_id: row.try_get(5)?,
            model: row.try_get(6)?,
            prompt_tokens: row.try_get(7)?,
            completion_tokens: row.try_get(8)?,
            message_count: row.try_get(9)?,
            preview: row.try_get(10)?,
            size_bytes: row.try_get(11)?,
            created_at: row.try_get(12)?,
        });
    }
    Ok((out, total, total >= COUNT_CAP))
}

/// 详情：读出 zip 并解析成消息列表。
pub async fn detail(
    store: &AdminStore,
    tenant_id: &str,
    id: &str,
) -> Result<Option<(Snapshot, Value)>> {
    let sql = store.sql(
        "SELECT s.id, s.user_id, COALESCE(u.display_name, u.username, s.user_id) AS user_name,
                s.conversation_id, s.agent_instance_id, s.agent_role_id, s.model,
                s.prompt_tokens, s.completion_tokens, s.message_count, s.preview,
                s.size_bytes, s.created_at, s.blob_path
           FROM conversation_snapshots s
           LEFT JOIN users u ON u.id = s.user_id
          WHERE s.tenant_id = ? AND s.id = ?",
    );
    let Some(row) = sqlx::query::<Any>(&sql)
        .bind(tenant_id)
        .bind(id)
        .fetch_optional(store.pool())
        .await?
    else {
        return Ok(None);
    };
    let blob_path: Option<String> = row.try_get(13)?;
    let snapshot = Snapshot {
        id: row.try_get(0)?,
        user_id: row.try_get(1)?,
        user_name: row.try_get(2)?,
        conversation_id: row.try_get(3)?,
        agent_instance_id: row.try_get(4)?,
        agent_role_id: row.try_get(5)?,
        model: row.try_get(6)?,
        prompt_tokens: row.try_get(7)?,
        completion_tokens: row.try_get(8)?,
        message_count: row.try_get(9)?,
        preview: row.try_get(10)?,
        size_bytes: row.try_get(11)?,
        created_at: row.try_get(12)?,
    };

    let content = match blob_path {
        Some(path) => match std::fs::read(&path) {
            Ok(bytes) => read_snapshot_from_zip(&bytes).unwrap_or_else(|e| {
                log::warn!("会话快照解析失败 {path}：{e}");
                json!({ "messages": [] })
            }),
            Err(e) => {
                log::warn!("会话快照读不到 {path}：{e}");
                json!({ "messages": [] })
            }
        },
        None => json!({ "messages": [] }),
    };
    Ok(Some((snapshot, content)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zip_with(json: &str) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut out));
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            zip.start_file("conversation_snapshot.json", opts).unwrap();
            use std::io::Write;
            zip.write_all(json.as_bytes()).unwrap();
            zip.finish().unwrap();
        }
        out
    }


    async fn insert_row(store: &AdminStore, id: &str, user_id: &str, created_at: i64) {
        let sql = store.sql(
            "INSERT INTO conversation_snapshots
             (id, tenant_id, user_id, conversation_id, agent_instance_id, agent_role_id,
              request_id, model, prompt_tokens, completion_tokens, message_count,
              preview, blob_path, size_bytes, created_at)
             VALUES (?, 'default', ?, 'c1', 'a1', 'coder', ?, 'm', 0, 0, 1, '预览', NULL, 0, ?)",
        );
        sqlx::query::<Any>(&sql)
            .bind(id)
            .bind(user_id)
            .bind(format!("req-{id}"))
            .bind(created_at)
            .execute(store.pool())
            .await
            .expect("insert");
    }

    /// 列表按时间倒序，且总数不会被 COUNT_CAP 误标成「封顶」。
    #[tokio::test]
    async fn list_returns_newest_first() {
        let store = AdminStore::connect_in_memory().await.expect("store");
        insert_row(&store, "s1", "u1", 100).await;
        insert_row(&store, "s2", "u1", 300).await;
        insert_row(&store, "s3", "u2", 200).await;

        let (rows, total, capped) = list(&store, "default", None, 10, 0)
            .await
            .expect("list");
        let ids: Vec<&str> = rows.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, vec!["s2", "s3", "s1"], "必须按时间倒序");
        assert_eq!(total, 3);
        assert!(!capped);

        // 按成员过滤
        let (rows, total, _) = list(&store, "default", Some("u1"), 10, 0)
            .await
            .expect("list");
        assert_eq!(rows.len(), 2);
        assert_eq!(total, 2);

        // 分页
        let (rows, _, _) = list(&store, "default", None, 1, 1).await.expect("list");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "s3");
    }
    #[test]
    fn preview_uses_first_user_message() {
        let snap = json!({ "messages": [
            { "role": "assistant", "content": "你好" },
            { "role": "user", "content": "帮我查一下报销单\n第二行" }
        ]});
        assert_eq!(preview_of(&snap), "帮我查一下报销单 第二行");
    }

    #[test]
    fn preview_is_truncated() {
        let long = "啊".repeat(500);
        let snap = json!({ "messages": [{ "role": "user", "content": long }] });
        let preview = preview_of(&snap);
        assert!(preview.chars().count() <= PREVIEW_CHARS + 1);
        assert!(preview.ends_with('…'));
    }

    #[test]
    fn reads_snapshot_out_of_zip() {
        let bytes = zip_with(r#"{"messages":[{"role":"user","content":"hi"}]}"#);
        let snap = read_snapshot_from_zip(&bytes).expect("parse");
        assert_eq!(snap["messages"][0]["content"], "hi");
    }
}
