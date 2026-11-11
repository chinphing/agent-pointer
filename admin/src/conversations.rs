//! 会话内容：客户端上报的对话，按「会话 + 轮次」两级存。
//!
//! 客户端**每轮**上报一次，zip 里只有本轮的消息
//! （`conversation_snapshot.json`）：
//!
//! ```json
//! { "conversation_id": "...", "agent_instance_id": "...", "agent_role_id": "general",
//!   "agent_name": "通用助手", "lead_agent_instance_id": "...", "run_id": "...",
//!   "turn_at": 1794417217000, "messages": [ ... ] }
//! ```
//!
//! 两张表：
//! - `conversation_threads`：一个 agent 实例一条（主会话一条，每个子 agent 各一条）
//! - `conversation_turns`：一轮一条，zip 落 `{data_dir}/conversations/yyyy-mm/{id}.zip`
//!
//! 聚合字段（`turn_count` / `message_count` / `first_at` / `last_at`）在**写入时**
//! 维护，所以列表页不用去 GROUP BY 轮次表。
//!
//! 主会话判定看 `agent_instance_id == lead_agent_instance_id`：lead 是**会话**的
//! 属性，每次上报都带同一个值，所以上报乱序、重试、主会话漏报都不会判错——这也是
//! 不存 `is_main` 布尔列的原因。
//!
//! 大块内容落盘、库里只存索引和预览，这点和 `resource_versions` 一致：列表页只扫
//! 小字段，抽屉打开时不读任何 zip，只有展开某一轮才读那一个文件。

use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use sqlx::any::Any;
use sqlx::Row;

use crate::config::AdminConfig;
use crate::db::{now_ts, random_id, AdminStore};

/// 预览（本轮首条用户消息）保留的字数。
const PREVIEW_CHARS: usize = 120;

/// 列表页「共 N 条」的计数上限。
///
/// 精确 COUNT 要扫完整张表：100 万行 ~190ms、500 万行 ~940ms，而列表本身
/// 只要 0.04ms——每次翻页都被这个计数拖住。封顶到 1 万行后代价恒定（~2ms），
/// 超过就显示「10000+」，对「有没有更多」这个用途足够。
const COUNT_CAP: i64 = 10_000;

/// 一个会话（一个 agent 实例一条）。
#[derive(Debug, Clone)]
pub struct Thread {
    pub id: String,
    pub conversation_id: String,
    pub agent_instance_id: String,
    pub agent_role_id: Option<String>,
    pub agent_name: Option<String>,
    /// 是不是主会话（`agent_instance_id == lead_agent_instance_id`）。
    pub is_main: bool,
    pub turn_count: i64,
    pub message_count: i64,
    pub first_at: i64,
    pub last_at: i64,
}

/// 列表里的一行：一个会话（可能由主会话 + 若干子会话组成）。
#[derive(Debug, Clone)]
pub struct ConversationSummary {
    pub conversation_id: String,
    pub user_id: String,
    pub user_name: String,
    /// 会话标题 = 主会话最早一轮的首条用户消息。
    pub title: String,
    pub model: String,
    pub thread_count: i64,
    pub turn_count: i64,
    pub message_count: i64,
    pub first_at: i64,
    pub last_at: i64,
}

/// 一轮。
#[derive(Debug, Clone)]
pub struct Turn {
    pub id: String,
    pub thread_id: String,
    pub seq: i64,
    pub turn_at: i64,
    pub preview: String,
    pub message_count: i64,
    pub size_bytes: i64,
}

/// 上报里带的消息元数据（来自 `UsageReport`）。
pub struct ReportMeta<'a> {
    pub request_id: Option<&'a str>,
    /// 下面三个 id 优先用 zip 里的，zip 缺了才用上报字段兜底。
    pub conversation_id: Option<&'a str>,
    pub agent_instance_id: Option<&'a str>,
    pub agent_role_id: Option<&'a str>,
    pub model: &'a str,
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

/// 客户端给的是毫秒；库里统一存秒，和 `now_ts()` 对齐。
fn to_seconds(raw: i64) -> i64 {
    if raw > 1_000_000_000_000 {
        raw / 1000
    } else {
        raw
    }
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

/// 找到（或创建）会话行，返回 `(thread_id, 是否新建)`。
async fn ensure_thread(
    store: &AdminStore,
    tenant_id: &str,
    user_id: &str,
    conversation_id: &str,
    agent_instance_id: &str,
    agent_role_id: Option<&str>,
    agent_name: Option<&str>,
    lead_agent_instance_id: &str,
    model: &str,
    title: &str,
    at: i64,
) -> Result<(String, bool)> {
    let find = store.sql(
        "SELECT id FROM conversation_threads
          WHERE tenant_id = ? AND conversation_id = ? AND agent_instance_id = ?",
    );
    if let Some(row) = sqlx::query::<Any>(&find)
        .bind(tenant_id)
        .bind(conversation_id)
        .bind(agent_instance_id)
        .fetch_optional(store.pool())
        .await?
    {
        let id: String = row.try_get(0)?;
        return Ok((id, false));
    }

    let id = random_id();
    let insert = store.sql(
        "INSERT INTO conversation_threads
         (id, tenant_id, user_id, conversation_id, agent_instance_id, agent_role_id,
          agent_name, lead_agent_instance_id, title, model, turn_count, message_count,
          first_at, last_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0, 0, ?, ?)",
    );
    let result = sqlx::query::<Any>(&insert)
        .bind(&id)
        .bind(tenant_id)
        .bind(user_id)
        .bind(conversation_id)
        .bind(agent_instance_id)
        .bind(agent_role_id)
        .bind(agent_name)
        .bind(lead_agent_instance_id)
        .bind(title)
        .bind(model)
        .bind(at)
        .bind(at)
        .execute(store.pool())
        .await;
    if let Err(e) = result {
        // 并发上报同一个会话时唯一索引会挡住后来的插入，这不是错误——
        // 重新读一次拿到先插入的那条。
        if let Some(row) = sqlx::query::<Any>(&find)
            .bind(tenant_id)
            .bind(conversation_id)
            .bind(agent_instance_id)
            .fetch_optional(store.pool())
            .await?
        {
            let id: String = row.try_get(0)?;
            return Ok((id, false));
        }
        return Err(e).context("创建会话行失败");
    }
    Ok((id, true))
}

/// 落盘一轮。返回 `false` 表示这一轮已经存过（同一 `run_id` 重试）。
pub async fn record(
    store: &AdminStore,
    config: &AdminConfig,
    tenant_id: &str,
    user_id: &str,
    meta: &ReportMeta<'_>,
    archive: &[u8],
) -> Result<bool> {
    let snapshot = read_snapshot_from_zip(archive)?;
    let conversation_id = snapshot_str(&snapshot, "conversation_id")
        .or_else(|| meta.conversation_id.map(|s| s.to_string()))
        .unwrap_or_default();
    let agent_instance_id = snapshot_str(&snapshot, "agent_instance_id")
        .or_else(|| meta.agent_instance_id.map(|s| s.to_string()))
        .unwrap_or_default();
    if conversation_id.is_empty() || agent_instance_id.is_empty() {
        bail!("快照缺少 conversation_id 或 agent_instance_id");
    }
    let agent_role_id = snapshot_str(&snapshot, "agent_role_id")
        .or_else(|| meta.agent_role_id.map(|s| s.to_string()));
    let agent_name = snapshot_str(&snapshot, "agent_name");
    let lead_agent_instance_id =
        snapshot_str(&snapshot, "lead_agent_instance_id").unwrap_or_default();
    let run_id = snapshot_str(&snapshot, "run_id")
        .or_else(|| meta.request_id.map(str::to_string))
        .unwrap_or_else(|| {
            log::warn!(
                "会话上报缺少 run_id，用随机值兜底 conversation_id={conversation_id}"
            );
            random_id()
        });
    let turn_at = snapshot
        .get("turn_at")
        .and_then(|v| v.as_i64())
        .map(to_seconds)
        .unwrap_or_else(now_ts);
    let message_count = snapshot
        .get("messages")
        .and_then(|m| m.as_array())
        .map(|m| m.len() as i64)
        .unwrap_or(0);
    if message_count == 0 {
        // 空轮（比如用户发了消息但没等到回复就中断）不占一行。
        return Ok(false);
    }
    let preview = preview_of(&snapshot);

    let (thread_id, created) = ensure_thread(
        store,
        tenant_id,
        user_id,
        &conversation_id,
        &agent_instance_id,
        agent_role_id.as_deref(),
        agent_name.as_deref(),
        &lead_agent_instance_id,
        meta.model,
        &preview,
        turn_at,
    )
    .await?;
    // 老会话可能还没有 lead（早期上报没带），补上。
    if !created && !lead_agent_instance_id.is_empty() {
        let fill = store.sql(
            "UPDATE conversation_threads SET lead_agent_instance_id = ?
              WHERE id = ? AND lead_agent_instance_id = ''",
        );
        sqlx::query::<Any>(&fill)
            .bind(&lead_agent_instance_id)
            .bind(&thread_id)
            .execute(store.pool())
            .await?;
    }

    // 幂等：同一轮重试不再存第二份。
    let exists = store.sql(
        "SELECT id FROM conversation_turns WHERE thread_id = ? AND run_id = ? LIMIT 1",
    );
    if sqlx::query::<Any>(&exists)
        .bind(&thread_id)
        .bind(&run_id)
        .fetch_optional(store.pool())
        .await?
        .is_some()
    {
        return Ok(false);
    }

    let id = random_id();
    let created_at = now_ts();
    let month = crate::oauth::token::month_prefix(created_at);
    let dir = blob_dir(config).join(&month);
    std::fs::create_dir_all(&dir).with_context(|| format!("创建快照目录 {}", dir.display()))?;
    let path = dir.join(format!("{id}.zip"));
    std::fs::write(&path, archive).with_context(|| format!("写快照 {}", path.display()))?;

    // seq 和计数必须在同一个事务里：两个上报同时到达时，seq 不能撞。
    let mut tx = store.pool().begin().await?;
    let seq_sql =
        store.sql("SELECT COALESCE(MAX(seq), 0) + 1 FROM conversation_turns WHERE thread_id = ?");
    let seq: i64 = sqlx::query::<Any>(&seq_sql)
        .bind(&thread_id)
        .fetch_one(&mut *tx)
        .await?
        .try_get(0)?;

    let insert = store.sql(
        "INSERT INTO conversation_turns
         (id, thread_id, tenant_id, run_id, seq, turn_at, preview, message_count,
          blob_path, size_bytes, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    );
    if let Err(e) = sqlx::query::<Any>(&insert)
        .bind(&id)
        .bind(&thread_id)
        .bind(tenant_id)
        .bind(&run_id)
        .bind(seq)
        .bind(turn_at)
        .bind(&preview)
        .bind(message_count)
        .bind(path.to_string_lossy().to_string())
        .bind(archive.len() as i64)
        .bind(created_at)
        .execute(&mut *tx)
        .await
    {
        // 并发重试撞上唯一索引：丢掉刚写的 zip，按幂等处理。
        let _ = tx.rollback().await;
        let _ = std::fs::remove_file(&path);
        log::warn!("会话轮次重复写入 thread={thread_id} run_id={run_id}: {e}");
        return Ok(false);
    }

    let bump = store.sql(
        "UPDATE conversation_threads SET
           turn_count    = turn_count + 1,
           message_count = message_count + ?,
           last_at       = ?,
           first_at      = CASE WHEN ? < first_at THEN ? ELSE first_at END,
           title         = CASE WHEN ? < first_at THEN ? ELSE title END,
           model         = CASE WHEN ? <> '' THEN ? ELSE model END
         WHERE id = ?",
    );
    sqlx::query::<Any>(&bump)
        .bind(message_count)
        .bind(turn_at)
        .bind(turn_at)
        .bind(turn_at)
        .bind(turn_at)
        .bind(&preview)
        .bind(meta.model)
        .bind(meta.model)
        .bind(&thread_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(true)
}

/// 列表筛选条件。
#[derive(Debug, Default, Clone, Copy)]
pub struct ListFilter<'a> {
    /// 只看某个成员。
    pub user_id: Option<&'a str>,
    /// 起始时间（含），unix 秒。
    pub from_ts: Option<i64>,
    /// 结束时间（不含），unix 秒。
    pub to_ts: Option<i64>,
}

/// 列表：一个会话一行，按最近活跃倒序。
///
/// 返回 `(本页数据, 总数, 总数是否被 COUNT_CAP 截断)`。
pub async fn list(
    store: &AdminStore,
    tenant_id: &str,
    filter: ListFilter<'_>,
    limit: i64,
    offset: i64,
) -> Result<(Vec<ConversationSummary>, i64, bool)> {
    let limit = limit.clamp(1, 200);
    let offset = offset.max(0);

    // 动态拼 WHERE，绑定顺序必须和这里一致。
    let mut where_sql = String::from("tenant_id = ?");
    if filter.user_id.is_some() {
        where_sql.push_str(" AND user_id = ?");
    }
    if filter.from_ts.is_some() {
        where_sql.push_str(" AND last_at >= ?");
    }
    if filter.to_ts.is_some() {
        where_sql.push_str(" AND last_at < ?");
    }

    let count_sql = store.sql(&format!(
        "SELECT COUNT(*) FROM (SELECT 1 FROM conversation_threads
           WHERE {where_sql} GROUP BY conversation_id LIMIT {COUNT_CAP}) t"
    ));
    let page_sql = store.sql(&format!(
        "SELECT conversation_id,
                MAX(last_at)       AS last_at,
                MIN(first_at)      AS first_at,
                SUM(turn_count)    AS turn_count,
                SUM(message_count) AS message_count,
                COUNT(*)           AS thread_count,
                MAX(user_id)       AS user_id
           FROM conversation_threads
          WHERE {where_sql}
          GROUP BY conversation_id
          ORDER BY last_at DESC
          LIMIT ? OFFSET ?"
    ));

    let mut count_query = sqlx::query::<Any>(&count_sql).bind(tenant_id);
    let mut page_query = sqlx::query::<Any>(&page_sql).bind(tenant_id);
    if let Some(uid) = filter.user_id {
        count_query = count_query.bind(uid);
        page_query = page_query.bind(uid);
    }
    if let Some(from) = filter.from_ts {
        count_query = count_query.bind(from);
        page_query = page_query.bind(from);
    }
    if let Some(to) = filter.to_ts {
        count_query = count_query.bind(to);
        page_query = page_query.bind(to);
    }
    let total: i64 = count_query.fetch_one(store.pool()).await?.try_get(0)?;
    let rows = page_query
        .bind(limit)
        .bind(offset)
        .fetch_all(store.pool())
        .await?;

    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        out.push(ConversationSummary {
            conversation_id: row.try_get(0)?,
            last_at: row.try_get(1)?,
            first_at: row.try_get(2)?,
            turn_count: row.try_get(3)?,
            message_count: row.try_get(4)?,
            thread_count: row.try_get(5)?,
            user_id: row.try_get(6)?,
            user_name: String::new(),
            title: String::new(),
            model: String::new(),
        });
    }
    enrich(store, tenant_id, &mut out).await?;
    Ok((out, total, total >= COUNT_CAP))
}

/// 补齐标题、模型、成员名：标题取**主会话最早一轮**的预览（会话的"标题"要稳定，
/// 不能每聊一轮就变），模型取主会话最后一次上报的。
async fn enrich(
    store: &AdminStore,
    tenant_id: &str,
    rows: &mut [ConversationSummary],
) -> Result<()> {
    if rows.is_empty() {
        return Ok(());
    }
    let placeholders = vec!["?"; rows.len()].join(",");
    let sql = store.sql(&format!(
        "SELECT conversation_id, agent_instance_id, lead_agent_instance_id, title, model
           FROM conversation_threads
          WHERE tenant_id = ? AND conversation_id IN ({placeholders})
          ORDER BY first_at ASC"
    ));
    let mut query = sqlx::query::<Any>(&sql).bind(tenant_id);
    for row in rows.iter() {
        query = query.bind(&row.conversation_id);
    }
    let threads = query.fetch_all(store.pool()).await?;

    // 每个会话挑主会话；没有主会话（lead 漏报）就退回最早的那条。
    let mut picked: std::collections::HashMap<String, (String, String)> =
        std::collections::HashMap::new();
    for t in &threads {
        let conversation_id: String = t.try_get(0)?;
        let agent_instance_id: String = t.try_get(1)?;
        let lead: String = t.try_get(2)?;
        let title: String = t.try_get(3)?;
        let model: String = t.try_get(4)?;
        let entry = picked.entry(conversation_id);
        match entry {
            std::collections::hash_map::Entry::Vacant(v) => {
                v.insert((title, model));
            }
            std::collections::hash_map::Entry::Occupied(mut o) => {
                // 主会话优先覆盖掉先放进去的"最早那条"。
                if !lead.is_empty() && agent_instance_id == lead {
                    o.insert((title, model));
                }
            }
        }
    }
    for row in rows.iter_mut() {
        if let Some((title, model)) = picked.get(&row.conversation_id) {
            row.title = title.clone();
            row.model = model.clone();
        }
    }

    // 成员名。
    let mut user_ids: Vec<String> = rows.iter().map(|r| r.user_id.clone()).collect();
    user_ids.sort();
    user_ids.dedup();
    if user_ids.is_empty() {
        return Ok(());
    }
    let placeholders = vec!["?"; user_ids.len()].join(",");
    let sql = store.sql(&format!(
        "SELECT id, COALESCE(display_name, username, id) FROM users WHERE id IN ({placeholders})"
    ));
    let mut query = sqlx::query::<Any>(&sql);
    for id in &user_ids {
        query = query.bind(id);
    }
    let mut names: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for row in query.fetch_all(store.pool()).await? {
        let id: String = row.try_get(0)?;
        let name: String = row.try_get(1)?;
        names.insert(id, name);
    }
    for row in rows.iter_mut() {
        if let Some(name) = names.get(&row.user_id) {
            row.user_name = name.clone();
        } else {
            row.user_name = row.user_id.clone();
        }
    }
    Ok(())
}

/// 一个会话下的所有 thread（抽屉里的 tab）。主会话排第一。
pub async fn threads(
    store: &AdminStore,
    tenant_id: &str,
    conversation_id: &str,
) -> Result<Vec<Thread>> {
    let sql = store.sql(
        "SELECT id, conversation_id, agent_instance_id, agent_role_id, agent_name,
                lead_agent_instance_id, turn_count, message_count, first_at, last_at
           FROM conversation_threads
          WHERE tenant_id = ? AND conversation_id = ?
          ORDER BY first_at ASC",
    );
    let rows = sqlx::query::<Any>(&sql)
        .bind(tenant_id)
        .bind(conversation_id)
        .fetch_all(store.pool())
        .await?;

    let mut out: Vec<Thread> = Vec::with_capacity(rows.len());
    for row in &rows {
        let agent_instance_id: String = row.try_get(2)?;
        let lead: String = row.try_get(5)?;
        out.push(Thread {
            id: row.try_get(0)?,
            conversation_id: row.try_get(1)?,
            is_main: !lead.is_empty() && agent_instance_id == lead,
            agent_instance_id,
            agent_role_id: row.try_get(3)?,
            agent_name: row.try_get(4)?,
            turn_count: row.try_get(6)?,
            message_count: row.try_get(7)?,
            first_at: row.try_get(8)?,
            last_at: row.try_get(9)?,
        });
    }

    // 没有主会话（lead 漏报）时退回最早那条，并留一条日志——
    // 这是数据缺失的降级，不是"猜"。
    if !out.is_empty() && !out.iter().any(|t| t.is_main) {
        log::warn!(
            "会话没有主会话（lead 上报缺失）conversation_id={conversation_id} threads={:?}",
            out.iter()
                .map(|t| t.agent_instance_id.as_str())
                .collect::<Vec<_>>()
        );
        out[0].is_main = true;
    }
    out.sort_by_key(|t| !t.is_main);
    Ok(out)
}

/// 某个 thread 下的轮次，新的在前，分页。返回 `(本页, 总数)`。
pub async fn turns(
    store: &AdminStore,
    tenant_id: &str,
    thread_id: &str,
    limit: i64,
    offset: i64,
) -> Result<(Vec<Turn>, i64)> {
    let limit = limit.clamp(1, 200);
    let offset = offset.max(0);

    let count_sql = store.sql(
        "SELECT COUNT(*) FROM conversation_turns WHERE tenant_id = ? AND thread_id = ?",
    );
    let total: i64 = sqlx::query::<Any>(&count_sql)
        .bind(tenant_id)
        .bind(thread_id)
        .fetch_one(store.pool())
        .await?
        .try_get(0)?;

    let page_sql = store.sql(
        "SELECT id, thread_id, seq, turn_at, preview, message_count, size_bytes
           FROM conversation_turns
          WHERE tenant_id = ? AND thread_id = ?
          ORDER BY seq DESC
          LIMIT ? OFFSET ?",
    );
    let rows = sqlx::query::<Any>(&page_sql)
        .bind(tenant_id)
        .bind(thread_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(store.pool())
        .await?;

    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        out.push(Turn {
            id: row.try_get(0)?,
            thread_id: row.try_get(1)?,
            seq: row.try_get(2)?,
            turn_at: row.try_get(3)?,
            preview: row.try_get(4)?,
            message_count: row.try_get(5)?,
            size_bytes: row.try_get(6)?,
        });
    }
    Ok((out, total))
}

/// 展开某一轮：读它那一个 zip。返回 `(轮次信息, 消息数组)`。
pub async fn turn_messages(
    store: &AdminStore,
    tenant_id: &str,
    turn_id: &str,
) -> Result<Option<(Turn, Value)>> {
    let sql = store.sql(
        "SELECT id, thread_id, seq, turn_at, preview, message_count, size_bytes, blob_path
           FROM conversation_turns WHERE tenant_id = ? AND id = ?",
    );
    let Some(row) = sqlx::query::<Any>(&sql)
        .bind(tenant_id)
        .bind(turn_id)
        .fetch_optional(store.pool())
        .await?
    else {
        return Ok(None);
    };
    let blob_path: Option<String> = row.try_get(7)?;
    let turn = Turn {
        id: row.try_get(0)?,
        thread_id: row.try_get(1)?,
        seq: row.try_get(2)?,
        turn_at: row.try_get(3)?,
        preview: row.try_get(4)?,
        message_count: row.try_get(5)?,
        size_bytes: row.try_get(6)?,
    };

    let messages = match blob_path {
        Some(path) => match std::fs::read(&path) {
            Ok(bytes) => read_snapshot_from_zip(&bytes)
                .map(|snapshot| snapshot.get("messages").cloned().unwrap_or_else(|| json!([])))
                .unwrap_or_else(|e| {
                    log::warn!("会话轮次解析失败 {path}：{e}");
                    json!([])
                }),
            Err(e) => {
                log::warn!("会话轮次读不到 {path}：{e}");
                json!([])
            }
        },
        None => json!([]),
    };
    Ok(Some((turn, messages)))
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

    /// 造一份本轮快照。`turn_at` 按客户端习惯给毫秒。
    fn snapshot(
        conversation_id: &str,
        agent_instance_id: &str,
        lead: &str,
        run_id: &str,
        turn_at_ms: i64,
        user_text: &str,
    ) -> String {
        json!({
            "conversation_id": conversation_id,
            "agent_instance_id": agent_instance_id,
            "agent_role_id": "general",
            "agent_name": "通用助手",
            "lead_agent_instance_id": lead,
            "run_id": run_id,
            "turn_at": turn_at_ms,
            "messages": [
                { "id": "u1", "role": "user", "content": user_text, "created_at": turn_at_ms },
                { "id": "a1", "role": "assistant", "content": "收到", "created_at": turn_at_ms + 1 },
            ],
        })
        .to_string()
    }

    async fn setup() -> (AdminStore, AdminConfig) {
        let store = AdminStore::connect_in_memory().await.expect("store");
        crate::schema::migrate(&store).await.expect("migrate");
        let config = AdminConfig {
            data_dir: std::env::temp_dir().join(format!("conv-test-{}", random_id())),
            ..Default::default()
        };
        std::fs::create_dir_all(&config.data_dir).expect("data dir");
        (store, config)
    }

    fn meta<'a>(model: &'a str) -> ReportMeta<'a> {
        ReportMeta {
            request_id: None,
            conversation_id: None,
            agent_instance_id: None,
            agent_role_id: None,
            model,
        }
    }

    async fn report(
        store: &AdminStore,
        config: &AdminConfig,
        json: &str,
    ) -> Result<bool> {
        record(store, config, "default", "u1", &meta("deepseek"), &zip_with(json)).await
    }

    /// 第一轮：建会话行 + 建轮次，计数从 0 变成 1 轮 2 条。
    #[tokio::test]
    async fn first_turn_creates_thread_and_turn() {
        let (store, config) = setup().await;
        let snap = snapshot("c1", "a1", "a1", "run1", 1_794_417_217_000, "哈喽");
        assert!(report(&store, &config, &snap).await.expect("record"));

        let threads = threads(&store, "default", "c1").await.expect("threads");
        assert_eq!(threads.len(), 1);
        assert!(threads[0].is_main, "agent 实例等于 lead 就是主会话");
        assert_eq!(threads[0].turn_count, 1);
        assert_eq!(threads[0].message_count, 2);
        // 毫秒入库要换成秒，和 now_ts() 对齐
        assert_eq!(threads[0].first_at, 1_794_417_217);
    }

    /// 同一轮重试（run_id 相同）不能存第二份，计数也不能涨。
    #[tokio::test]
    async fn same_run_id_is_idempotent() {
        let (store, config) = setup().await;
        let snap = snapshot("c1", "a1", "a1", "run1", 1_794_417_217_000, "哈喽");
        assert!(report(&store, &config, &snap).await.expect("first"));
        assert!(!report(&store, &config, &snap).await.expect("retry"));

        let threads = threads(&store, "default", "c1").await.expect("threads");
        assert_eq!(threads[0].turn_count, 1);
        assert_eq!(threads[0].message_count, 2);
        let (rows, total) = turns(&store, "default", &threads[0].id, 20, 0)
            .await
            .expect("turns");
        assert_eq!(total, 1);
        assert_eq!(rows.len(), 1);
    }

    /// 多轮：seq 递增、计数累加、按 seq 倒序分页。
    #[tokio::test]
    async fn turns_accumulate_and_page_newest_first() {
        let (store, config) = setup().await;
        for i in 0..3 {
            let snap = snapshot(
                "c1",
                "a1",
                "a1",
                &format!("run{i}"),
                1_794_417_217_000 + i * 60_000,
                &format!("第 {i} 轮"),
            );
            assert!(report(&store, &config, &snap).await.expect("record"));
        }
        let threads = threads(&store, "default", "c1").await.expect("threads");
        assert_eq!(threads[0].turn_count, 3);
        assert_eq!(threads[0].message_count, 6);

        let (rows, total) = turns(&store, "default", &threads[0].id, 20, 0)
            .await
            .expect("turns");
        assert_eq!(total, 3);
        let seqs: Vec<i64> = rows.iter().map(|t| t.seq).collect();
        assert_eq!(seqs, vec![3, 2, 1], "新的在前");

        // 分页
        let (page, _) = turns(&store, "default", &threads[0].id, 1, 1)
            .await
            .expect("page");
        assert_eq!(page.len(), 1);
        assert_eq!(page[0].seq, 2);
    }

    /// 主会话按 lead 判定：子 agent 的上报不会抢走主会话。
    #[tokio::test]
    async fn main_thread_follows_lead_instance() {
        let (store, config) = setup().await;
        // 子会话先到，它带着 lead=a1
        let sub = snapshot("c1", "sub1", "a1", "run-sub", 1_794_417_217_000, "子任务");
        assert!(report(&store, &config, &sub).await.expect("sub"));
        let rows = threads(&store, "default", "c1").await.expect("threads");
        assert_eq!(rows.len(), 1);
        // 此时主会话还没上报，只剩一个 thread。按「数据缺失降级」把它标成主，
        // 否则抽屉里一个 tab 都没有、什么都看不到。
        assert!(rows[0].is_main, "只有一个 thread 时降级成主会话");

        // 主会话后到 —— 判定立刻纠正
        let main = snapshot("c1", "a1", "a1", "run-main", 1_794_417_218_000, "哈喽");
        assert!(report(&store, &config, &main).await.expect("main"));

        let rows = threads(&store, "default", "c1").await.expect("threads");
        assert_eq!(rows.len(), 2);
        assert!(rows[0].is_main, "主会话必须排第一");
        assert_eq!(rows[0].agent_instance_id, "a1");
        assert!(!rows[1].is_main, "主会话到齐后子会话不能再算主");
    }

    /// 列表：主会话 + 子会话合成一行，轮次和消息数是合计。
    #[tokio::test]
    async fn list_merges_threads_into_one_row() {
        let (store, config) = setup().await;
        let main = snapshot("c1", "a1", "a1", "run1", 1_794_417_217_000, "哈喽");
        report(&store, &config, &main).await.expect("main");
        let sub = snapshot("c1", "sub1", "a1", "run2", 1_794_417_218_000, "子任务");
        report(&store, &config, &sub).await.expect("sub");

        let (rows, total, capped) = list(&store, "default", ListFilter::default(), 10, 0)
            .await
            .expect("list");
        assert_eq!(total, 1, "一个会话只占一行");
        assert!(!capped);
        assert_eq!(rows[0].thread_count, 2);
        assert_eq!(rows[0].turn_count, 2);
        assert_eq!(rows[0].message_count, 4);
        assert_eq!(rows[0].title, "哈喽", "标题取主会话最早一轮");
    }

    /// 标题是会话的"名字"，不能因为后来聊了别的就变。
    #[tokio::test]
    async fn title_stays_on_earliest_turn() {
        let (store, config) = setup().await;
        let first = snapshot("c1", "a1", "a1", "run1", 1_794_417_217_000, "第一个问题");
        report(&store, &config, &first).await.expect("first");
        let second = snapshot("c1", "a1", "a1", "run2", 1_794_417_300_000, "第二个问题");
        report(&store, &config, &second).await.expect("second");

        let (rows, _, _) = list(&store, "default", ListFilter::default(), 10, 0)
            .await
            .expect("list");
        assert_eq!(rows[0].title, "第一个问题");
        assert_eq!(rows[0].last_at, 1_794_417_300);
    }

    /// 没有消息的轮次不占一行（用户发了话但没等到回复就中断）。
    #[tokio::test]
    async fn empty_turn_is_skipped() {
        let (store, config) = setup().await;
        let empty = json!({
            "conversation_id": "c1",
            "agent_instance_id": "a1",
            "lead_agent_instance_id": "a1",
            "run_id": "run1",
            "turn_at": 1_794_417_217_000_i64,
            "messages": [],
        })
        .to_string();
        assert!(!report(&store, &config, &empty).await.expect("record"));

        let (rows, total, _) = list(&store, "default", ListFilter::default(), 10, 0)
            .await
            .expect("list");
        assert_eq!(total, 0);
        assert!(rows.is_empty());
    }

    /// 展开某一轮能读回消息正文。
    #[tokio::test]
    async fn turn_messages_returns_content() {
        let (store, config) = setup().await;
        let snap = snapshot("c1", "a1", "a1", "run1", 1_794_417_217_000, "哈喽");
        report(&store, &config, &snap).await.expect("record");
        let rows = threads(&store, "default", "c1").await.expect("threads");
        let (turns_page, _) = turns(&store, "default", &rows[0].id, 20, 0)
            .await
            .expect("turns");
        let (turn, messages) = turn_messages(&store, "default", &turns_page[0].id)
            .await
            .expect("messages")
            .expect("exists");
        assert_eq!(turn.seq, 1);
        assert_eq!(messages.as_array().expect("array").len(), 2);
        assert_eq!(messages[0]["content"], "哈喽");
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
