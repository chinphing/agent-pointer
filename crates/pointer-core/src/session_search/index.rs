//! SQLite FTS5 index over `conversations.json` messages.

use anyhow::{Context, Result};
use chrono::{DateTime, Local, TimeZone, Utc};
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use std::path::PathBuf;

use crate::models::{ChatMessage, Conversation, Role};
use crate::storage::{self, app_data_dir};

const DB_FILE: &str = "sessions.db";
const DEFAULT_WINDOW: i64 = 5;
const MAX_WINDOW: i64 = 20;
const DEFAULT_LIMIT: i64 = 3;
const MAX_LIMIT: i64 = 10;
const READ_HEAD: i64 = 20;
const READ_TAIL: i64 = 10;
const BOOKEND_COUNT: i64 = 3;

pub struct SessionIndex {
    conn: Mutex<Connection>,
}

impl SessionIndex {
    pub fn open_default() -> Result<Self> {
        let path = app_data_dir()?.join(DB_FILE);
        Self::open(path)
    }

    pub fn open(path: PathBuf) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create sessions db dir {}", parent.display()))?;
        }
        let conn = Connection::open(&path)
            .with_context(|| format!("open sessions db {}", path.display()))?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;",
        )?;
        migrate_base_schema(&conn)?;
        super::cjk_fts::ensure_loaded(&conn)?;
        ensure_fts_schema(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Sync index from disk (`conversations.json`). Incremental per conversation.
    pub fn sync_from_disk(&self) -> Result<()> {
        let convs = storage::load_conversations()?;
        self.sync_conversations(&convs)
    }

    pub fn sync_conversations(&self, convs: &[Conversation]) -> Result<()> {
        let conn = self.conn.lock();
        for conv in convs {
            let stored_updated: Option<i64> = conn
                .query_row(
                    "SELECT updated_at_ms FROM conversations_meta WHERE id = ?1",
                    params![conv.id],
                    |row| row.get(0),
                )
                .optional()?;
            let msg_count = conv.messages.len() as i64;
            if stored_updated == Some(conv.updated_at) && {
                let stored_count: i64 = conn
                    .query_row(
                        "SELECT message_count FROM conversations_meta WHERE id = ?1",
                        params![conv.id],
                        |row| row.get(0),
                    )
                    .unwrap_or(-1);
                stored_count == msg_count
            } {
                continue;
            }
            reindex_conversation(&conn, conv)?;
        }
        Ok(())
    }

    pub fn dispatch_tool(&self, args: &Value) -> Result<String> {
        self.sync_from_disk()?;
        self.dispatch_tool_inner(args)
    }

    fn dispatch_tool_inner(&self, args: &Value) -> Result<String> {
        let current_conversation_id = args
            .get("_conversation_id")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty());

        let conversation_id = args
            .get("conversation_id")
            .or_else(|| args.get("session_id"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty());

        let around_message_id = args
            .get("around_message_id")
            .and_then(|v| {
                v.as_str()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string())
                    .or_else(|| v.as_i64().map(|n| n.to_string()))
            });

        if conversation_id.is_some() && around_message_id.is_some() {
            let cid = conversation_id.unwrap();
            let anchor = around_message_id.unwrap();
            let window = parse_window(args.get("window"))?;
            return self.scroll(cid, &anchor, window, current_conversation_id);
        }

        if let Some(cid) = conversation_id {
            return self.read_session(cid);
        }

        let query = args
            .get("query")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty());

        if query.is_none() {
            let limit = parse_limit(args.get("limit"))?;
            return self.browse(limit, current_conversation_id);
        }

        let limit = parse_limit(args.get("limit"))?;
        let role_filter = parse_role_filter(args.get("role_filter"));
        let sort = parse_sort(args.get("sort"));
        self.discover(query.unwrap(), limit, role_filter.as_deref(), sort, current_conversation_id)
    }

    fn browse(&self, limit: i64, current_conversation_id: Option<&str>) -> Result<String> {
        let conn = self.conn.lock();
        let fetch = limit + 5;
        let mut stmt = conn.prepare(
            "SELECT id, title, created_at_ms, updated_at_ms, message_count, preview
             FROM conversations_meta
             ORDER BY updated_at_ms DESC
             LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![fetch], |row| {
            Ok(ConversationMetaRow {
                id: row.get(0)?,
                title: row.get(1)?,
                created_at_ms: row.get(2)?,
                updated_at_ms: row.get(3)?,
                message_count: row.get(4)?,
                preview: row.get(5)?,
            })
        })?;

        let mut results = Vec::new();
        for row in rows {
            let r = row?;
            if current_conversation_id == Some(r.id.as_str()) {
                continue;
            }
            results.push(json!({
                "conversation_id": r.id,
                "title": nullable_str(&r.title),
                "started_at": format_timestamp_ms(r.created_at_ms),
                "last_active": format_timestamp_ms(r.updated_at_ms),
                "message_count": r.message_count,
                "preview": nullable_str(&r.preview),
            }));
            if results.len() as i64 >= limit {
                break;
            }
        }

        Ok(json!({
            "success": true,
            "mode": "browse",
            "results": results,
            "count": results.len(),
            "message": format!(
                "Showing {} most recent conversations. Pass query= to search, or conversation_id+around_message_id to scroll.",
                results.len()
            ),
        })
        .to_string())
    }

    fn discover(
        &self,
        query: &str,
        limit: i64,
        role_filter: Option<&[String]>,
        sort: Option<&str>,
        current_conversation_id: Option<&str>,
    ) -> Result<String> {
        let fts_query = build_fts_query(query);
        let conn = self.conn.lock();

        let mut hits: Vec<FtsHit> = Vec::new();
        {
            let sql = "SELECT conversation_id, message_id, role,
                              snippet(messages_fts, 0, '<b>', '</b>', '...', 48) AS snip,
                              bm25(messages_fts) AS rank
                       FROM messages_fts
                       WHERE messages_fts MATCH ?1
                       ORDER BY rank
                       LIMIT ?2";
            let mut stmt = conn.prepare(sql)?;
            let cap = limit * 8;
            let mapped = stmt.query_map(params![fts_query, cap], |row| {
                Ok(FtsHit {
                    conversation_id: row.get(0)?,
                    message_id: row.get(1)?,
                    role: row.get(2)?,
                    snippet: row.get(3)?,
                    rank: row.get(4)?,
                })
            })?;
            for hit in mapped {
                hits.push(hit?);
            }
        }

        if let Some(roles) = role_filter {
            hits.retain(|h| roles.iter().any(|r| r.eq_ignore_ascii_case(&h.role)));
        }

        let mut seen = std::collections::HashSet::new();
        let mut deduped = Vec::new();
        for hit in hits {
            if !seen.insert(hit.conversation_id.clone()) {
                continue;
            }
            if current_conversation_id == Some(hit.conversation_id.as_str()) {
                continue;
            }
            deduped.push(hit);
            if deduped.len() as i64 >= limit {
                break;
            }
        }

        if sort == Some("oldest") {
            deduped.sort_by_key(|h| {
                meta_updated_at(&conn, &h.conversation_id).unwrap_or(0)
            });
        } else {
            deduped.sort_by_key(|h| {
                std::cmp::Reverse(meta_updated_at(&conn, &h.conversation_id).unwrap_or(0))
            });
        }

        let mut results = Vec::new();
        for hit in deduped {
            let meta = load_meta(&conn, &hit.conversation_id)?;
            let Some(meta) = meta else {
                continue;
            };
            let anchor_pos = message_position(&conn, &hit.conversation_id, &hit.message_id)?;
            let window = if anchor_pos >= 0 {
                load_window(&conn, &hit.conversation_id, anchor_pos, DEFAULT_WINDOW)?
            } else {
                WindowView {
                    messages: vec![],
                    messages_before: 0,
                    messages_after: 0,
                }
            };
            let bookend_start = load_bookends(&conn, &hit.conversation_id, true)?;
            let bookend_end = load_bookends(&conn, &hit.conversation_id, false)?;

            results.push(json!({
                "conversation_id": hit.conversation_id,
                "title": nullable_str(&meta.title),
                "when": format_timestamp_ms(meta.updated_at_ms),
                "snippet": hit.snippet,
                "match_message_id": hit.message_id,
                "messages_before": window.messages_before,
                "messages_after": window.messages_after,
                "bookend_start": bookend_start,
                "messages": window.messages,
                "bookend_end": bookend_end,
            }));
        }

        Ok(json!({
            "success": true,
            "mode": "discovery",
            "query": query,
            "results": results,
            "count": results.len(),
        })
        .to_string())
    }

    fn scroll(
        &self,
        conversation_id: &str,
        around_message_id: &str,
        window: i64,
        current_conversation_id: Option<&str>,
    ) -> Result<String> {
        if current_conversation_id == Some(conversation_id) {
            return Ok(error_json(
                "scroll rejected: target is the current conversation (already in your active context)",
            ));
        }

        let conn = self.conn.lock();
        let meta = load_meta(&conn, conversation_id)?;
        if meta.is_none() {
            return Ok(error_json(&format!(
                "conversation_id not found: {conversation_id}"
            )));
        }

        let anchor_pos = message_position(&conn, conversation_id, around_message_id)?;
        if anchor_pos < 0 {
            return Ok(error_json(&format!(
                "around_message_id not found in conversation: {around_message_id}"
            )));
        }

        let view = load_window(&conn, conversation_id, anchor_pos, window)?;
        Ok(json!({
            "success": true,
            "mode": "scroll",
            "conversation_id": conversation_id,
            "around_message_id": around_message_id,
            "window": window,
            "messages_before": view.messages_before,
            "messages_after": view.messages_after,
            "messages": view.messages,
        })
        .to_string())
    }

    fn read_session(&self, conversation_id: &str) -> Result<String> {
        let conn = self.conn.lock();
        let meta = load_meta(&conn, conversation_id)?;
        let Some(meta) = meta else {
            return Ok(error_json(&format!(
                "conversation_id not found: {conversation_id}"
            )));
        };

        let all = load_all_messages(&conn, conversation_id)?;
        let total = all.len() as i64;
        let truncated = total > READ_HEAD + READ_TAIL;
        let window: Vec<Value> = if truncated {
            let head = all.iter().take(READ_HEAD as usize).cloned().collect::<Vec<_>>();
            let tail = all
                .iter()
                .skip(all.len().saturating_sub(READ_TAIL as usize))
                .cloned()
                .collect::<Vec<_>>();
            head.into_iter().chain(tail).collect()
        } else {
            all
        };

        let mut response = json!({
            "success": true,
            "mode": "read",
            "conversation_id": conversation_id,
            "conversation_meta": {
                "title": nullable_str(&meta.title),
                "when": format_timestamp_ms(meta.updated_at_ms),
                "started_at": format_timestamp_ms(meta.created_at_ms),
            },
            "message_count": total,
            "truncated": truncated,
            "messages": window,
        });
        if truncated {
            response["message"] = json!(format!(
                "Conversation has {total} messages; showing first {READ_HEAD} + last {READ_TAIL}. Pass around_message_id to scroll the middle."
            ));
        }
        Ok(response.to_string())
    }
}

#[cfg(test)]
impl SessionIndex {
    pub fn open_in_dir(dir: &std::path::Path) -> Result<Self> {
        Self::open(dir.join(DB_FILE))
    }

    fn dispatch_tool_for_test(&self, args: &Value) -> Result<String> {
        self.dispatch_tool_inner(args)
    }
}

fn migrate_base_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS conversations_meta (
           id TEXT PRIMARY KEY,
           title TEXT NOT NULL,
           created_at_ms INTEGER NOT NULL,
           updated_at_ms INTEGER NOT NULL,
           message_count INTEGER NOT NULL,
           preview TEXT NOT NULL DEFAULT ''
         );
         CREATE TABLE IF NOT EXISTS messages (
           id INTEGER PRIMARY KEY,
           conversation_id TEXT NOT NULL,
           message_id TEXT NOT NULL,
           role TEXT NOT NULL,
           content TEXT NOT NULL,
           created_at_ms INTEGER NOT NULL,
           position INTEGER NOT NULL,
           UNIQUE(conversation_id, message_id)
         );
         CREATE INDEX IF NOT EXISTS idx_messages_conv_pos
           ON messages(conversation_id, position);
         CREATE TABLE IF NOT EXISTS index_meta (
           key TEXT PRIMARY KEY,
           value TEXT NOT NULL
         );",
    )?;
    Ok(())
}

fn ensure_fts_schema(conn: &Connection) -> Result<()> {
    let current: Option<String> = conn
        .query_row(
            "SELECT value FROM index_meta WHERE key = 'fts_tokenizer'",
            [],
            |row| row.get(0),
        )
        .optional()?;

    if current.as_deref() == Some("cjk_bigram") && fts_table_exists(conn)? {
        return Ok(());
    }

    if fts_table_exists(conn)? {
        conn.execute_batch(
            "DROP TRIGGER IF EXISTS messages_ai;
             DROP TRIGGER IF EXISTS messages_ad;
             DROP TRIGGER IF EXISTS messages_au;
             DROP TABLE IF EXISTS messages_fts;",
        )?;
        log::info!("session_search: rebuilding FTS index with cjk_bigram tokenizer");
    }

    create_fts_table(conn)?;
    conn.execute(
        "INSERT INTO index_meta(key, value) VALUES ('fts_tokenizer', 'cjk_bigram')
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [],
    )?;

    if current.as_deref() != Some("cjk_bigram") {
        conn.execute_batch("DELETE FROM messages; DELETE FROM conversations_meta;")?;
        log::info!("session_search: cleared message index for cjk_bigram reindex");
    }
    Ok(())
}

fn fts_table_exists(conn: &Connection) -> Result<bool> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='messages_fts'",
        [],
        |row| row.get(0),
    )?;
    Ok(n > 0)
}

fn create_fts_table(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(
           content,
           conversation_id UNINDEXED,
           message_id UNINDEXED,
           role UNINDEXED,
           content='messages',
           content_rowid='id',
           tokenize='cjk_bigram'
         );
         CREATE TRIGGER IF NOT EXISTS messages_ai AFTER INSERT ON messages BEGIN
           INSERT INTO messages_fts(rowid, content, conversation_id, message_id, role)
           VALUES (new.id, new.content, new.conversation_id, new.message_id, new.role);
         END;
         CREATE TRIGGER IF NOT EXISTS messages_ad AFTER DELETE ON messages BEGIN
           INSERT INTO messages_fts(messages_fts, rowid, content, conversation_id, message_id, role)
           VALUES ('delete', old.id, old.content, old.conversation_id, old.message_id, old.role);
         END;
         CREATE TRIGGER IF NOT EXISTS messages_au AFTER UPDATE ON messages BEGIN
           INSERT INTO messages_fts(messages_fts, rowid, content, conversation_id, message_id, role)
           VALUES ('delete', old.id, old.content, old.conversation_id, old.message_id, old.role);
           INSERT INTO messages_fts(rowid, content, conversation_id, message_id, role)
           VALUES (new.id, new.content, new.conversation_id, new.message_id, new.role);
         END;",
    )?;
    Ok(())
}

fn reindex_conversation(conn: &Connection, conv: &Conversation) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "DELETE FROM messages WHERE conversation_id = ?1",
        params![conv.id],
    )?;
    for (pos, msg) in conv.messages.iter().enumerate() {
        let content = message_index_content(msg);
        let role = role_str(&msg.role);
        tx.execute(
            "INSERT INTO messages (conversation_id, message_id, role, content, created_at_ms, position)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                conv.id,
                msg.id,
                role,
                content,
                msg.created_at,
                pos as i64
            ],
        )?;
    }
    let preview = conversation_preview(&conv.messages);
    tx.execute(
        "INSERT INTO conversations_meta (id, title, created_at_ms, updated_at_ms, message_count, preview)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(id) DO UPDATE SET
           title = excluded.title,
           updated_at_ms = excluded.updated_at_ms,
           message_count = excluded.message_count,
           preview = excluded.preview",
        params![
            conv.id,
            conv.title,
            conv.created_at,
            conv.updated_at,
            conv.messages.len() as i64,
            preview
        ],
    )?;
    tx.commit()?;
    Ok(())
}

fn message_index_content(msg: &ChatMessage) -> String {
    match msg.role {
        Role::System => String::new(),
        Role::User | Role::Assistant => msg.content.trim().to_string(),
        Role::Tool => {
            let c = msg.content.trim();
            if c.is_empty() {
                String::new()
            } else {
                format!("[tool] {c}")
            }
        }
    }
}

fn role_str(role: &Role) -> String {
    match role {
        Role::System => "system".into(),
        Role::User => "user".into(),
        Role::Assistant => "assistant".into(),
        Role::Tool => "tool".into(),
    }
}

fn conversation_preview(messages: &[ChatMessage]) -> String {
    for msg in messages {
        if matches!(msg.role, Role::User) {
            let t = msg.content.trim();
            if !t.is_empty() {
                return truncate_chars(t, 160);
            }
        }
    }
    for msg in messages {
        if matches!(msg.role, Role::Assistant) {
            let t = msg.content.trim();
            if !t.is_empty() {
                return truncate_chars(t, 160);
            }
        }
    }
    String::new()
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let end: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{end}…")
}

struct ConversationMetaRow {
    id: String,
    title: String,
    created_at_ms: i64,
    updated_at_ms: i64,
    message_count: i64,
    preview: String,
}

struct FtsHit {
    conversation_id: String,
    message_id: String,
    role: String,
    snippet: String,
    #[allow(dead_code)]
    rank: f64,
}

struct WindowView {
    messages: Vec<Value>,
    messages_before: i64,
    messages_after: i64,
}

struct MetaRow {
    title: String,
    created_at_ms: i64,
    updated_at_ms: i64,
}

fn load_meta(conn: &Connection, conversation_id: &str) -> Result<Option<MetaRow>> {
    conn.query_row(
        "SELECT title, created_at_ms, updated_at_ms FROM conversations_meta WHERE id = ?1",
        params![conversation_id],
        |row| {
            Ok(MetaRow {
                title: row.get(0)?,
                created_at_ms: row.get(1)?,
                updated_at_ms: row.get(2)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

fn meta_updated_at(conn: &Connection, conversation_id: &str) -> Result<i64> {
    Ok(conn
        .query_row(
            "SELECT updated_at_ms FROM conversations_meta WHERE id = ?1",
            params![conversation_id],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
        .unwrap_or(0))
}

fn message_position(conn: &Connection, conversation_id: &str, message_id: &str) -> Result<i64> {
    Ok(conn
        .query_row(
            "SELECT position FROM messages WHERE conversation_id = ?1 AND message_id = ?2",
            params![conversation_id, message_id],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
        .unwrap_or(-1))
}

fn load_window(
    conn: &Connection,
    conversation_id: &str,
    anchor_pos: i64,
    window: i64,
) -> Result<WindowView> {
    let total: i64 = conn.query_row(
        "SELECT COUNT(*) FROM messages WHERE conversation_id = ?1",
        params![conversation_id],
        |row| row.get(0),
    )?;
    let start = (anchor_pos - window).max(0);
    let end = (anchor_pos + window).min(total.saturating_sub(1));
    let messages_before = anchor_pos - start;
    let messages_after = end - anchor_pos;

    let mut stmt = conn.prepare(
        "SELECT message_id, role, content, created_at_ms, position
         FROM messages
         WHERE conversation_id = ?1 AND position BETWEEN ?2 AND ?3
         ORDER BY position ASC",
    )?;
    let anchor_message_id: String = conn.query_row(
        "SELECT message_id FROM messages WHERE conversation_id = ?1 AND position = ?2",
        params![conversation_id, anchor_pos],
        |row| row.get(0),
    )?;
    let rows = stmt.query_map(params![conversation_id, start, end], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, i64>(3)?,
        ))
    })?;

    let mut messages = Vec::new();
    for row in rows {
        let (id, role, content, ts) = row?;
        let mut entry = json!({
            "id": id,
            "role": role,
            "content": content,
            "timestamp": format_timestamp_ms(ts),
        });
        if id == anchor_message_id {
            entry["anchor"] = json!(true);
        }
        messages.push(entry);
    }

    Ok(WindowView {
        messages,
        messages_before,
        messages_after,
    })
}

fn load_bookends(conn: &Connection, conversation_id: &str, start: bool) -> Result<Vec<Value>> {
    let order = if start { "ASC" } else { "DESC" };
    let sql = format!(
        "SELECT message_id, role, content, created_at_ms
         FROM messages
         WHERE conversation_id = ?1 AND role IN ('user', 'assistant')
         ORDER BY position {order}
         LIMIT ?2"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![conversation_id, BOOKEND_COUNT], |row| {
        Ok(json!({
            "id": row.get::<_, String>(0)?,
            "role": row.get::<_, String>(1)?,
            "content": row.get::<_, String>(2)?,
            "timestamp": format_timestamp_ms(row.get::<_, i64>(3)?),
        }))
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    if !start {
        out.reverse();
    }
    Ok(out)
}

fn load_all_messages(conn: &Connection, conversation_id: &str) -> Result<Vec<Value>> {
    let mut stmt = conn.prepare(
        "SELECT message_id, role, content, created_at_ms
         FROM messages
         WHERE conversation_id = ?1
         ORDER BY position ASC",
    )?;
    let rows = stmt.query_map(params![conversation_id], |row| {
        Ok(json!({
            "id": row.get::<_, String>(0)?,
            "role": row.get::<_, String>(1)?,
            "content": row.get::<_, String>(2)?,
            "timestamp": format_timestamp_ms(row.get::<_, i64>(3)?),
        }))
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

fn build_fts_query(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.contains('"') || trimmed.contains('*') || trimmed.to_ascii_uppercase().contains(" OR ") {
        return trimmed.to_string();
    }
    if contains_cjk(trimmed) && !trimmed.contains(' ') {
        return trimmed.to_string();
    }
    trimmed
        .split_whitespace()
        .map(|term| {
            if term.starts_with('-') || term.ends_with('*') {
                term.to_string()
            } else {
                format!("\"{}\"", term.replace('"', "\"\""))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn contains_cjk(s: &str) -> bool {
    s.chars().any(|ch| {
        let cp = ch as u32;
        (0x4E00..=0x9FFF).contains(&cp)
            || (0x3400..=0x4DBF).contains(&cp)
            || (0x3040..=0x309F).contains(&cp)
            || (0x30A0..=0x30FF).contains(&cp)
            || (0xAC00..=0xD7AF).contains(&cp)
    })
}

fn parse_window(v: Option<&Value>) -> Result<i64> {
    let n = match v {
        Some(Value::Number(n)) => n.as_i64().unwrap_or(DEFAULT_WINDOW),
        Some(Value::String(s)) => s.parse().unwrap_or(DEFAULT_WINDOW),
        _ => DEFAULT_WINDOW,
    };
    Ok(n.clamp(1, MAX_WINDOW))
}

fn parse_limit(v: Option<&Value>) -> Result<i64> {
    let n = match v {
        Some(Value::Number(n)) => n.as_i64().unwrap_or(DEFAULT_LIMIT),
        Some(Value::String(s)) => s.parse().unwrap_or(DEFAULT_LIMIT),
        _ => DEFAULT_LIMIT,
    };
    Ok(n.clamp(1, MAX_LIMIT))
}

fn parse_role_filter(v: Option<&Value>) -> Option<Vec<String>> {
    let s = v.and_then(|x| x.as_str())?;
    let roles: Vec<String> = s
        .split(',')
        .map(str::trim)
        .filter(|r| !r.is_empty())
        .map(str::to_string)
        .collect();
    if roles.is_empty() {
        None
    } else {
        Some(roles)
    }
}

fn parse_sort(v: Option<&Value>) -> Option<&'static str> {
    match v.and_then(|x| x.as_str()).map(str::trim) {
        Some(s) if s.eq_ignore_ascii_case("newest") => Some("newest"),
        Some(s) if s.eq_ignore_ascii_case("oldest") => Some("oldest"),
        _ => None,
    }
}

fn format_timestamp_ms(ms: i64) -> String {
    let secs = ms / 1000;
    if let Some(dt) = Utc.timestamp_opt(secs, 0).single() {
        let local: DateTime<Local> = dt.into();
        return local.format("%B %d, %Y at %I:%M %p").to_string();
    }
    ms.to_string()
}

fn nullable_str(s: &str) -> Value {
    if s.trim().is_empty() {
        Value::Null
    } else {
        Value::String(s.to_string())
    }
}

fn error_json(msg: &str) -> String {
    json!({ "success": false, "error": msg }).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ChatMessage, Conversation, Role};
    use tempfile::TempDir;

    fn msg(id: &str, role: Role, content: &str, created_at: i64) -> ChatMessage {
        ChatMessage {
            id: id.into(),
            role,
            content: content.into(),
            status: "done".into(),
            created_at,
            tool_calls: None,
            tool_call_id: None,
            error_message: None,
            reasoning: None,
            thoughts: None,
            headline: None,
            raw_content: None,
            tool_raw_output: None,
            agent_id: None,
            agent_instance_id: None,
            agent_name: None,
            agent_trace: None,
            images_base64: None,
            image_slot_labels: None,
            computer_round_screen_rel_path: None,
            ui_bindings: None,
            context_state: None,
        }
    }

    fn sample_conv(id: &str, title: &str, user_text: &str) -> Conversation {
        Conversation {
            id: id.to_string(),
            title: title.to_string(),
            created_at: 1_700_000_000_000,
            updated_at: 1_700_000_100_000,
            messages: vec![
                msg("msg_u1", Role::User, user_text, 1_700_000_000_000),
                msg("msg_a1", Role::Assistant, "Acknowledged.", 1_700_000_001_000),
            ],
            skill_ids: vec![],
            tool_rounds_used: 0,
            tool_rounds_used_supervisor: 0,
            computer_monitor_id: None,
            workspace_root: String::new(),
        }
    }

    #[test]
    fn sync_discover_and_scroll() {
        let dir = TempDir::new().unwrap();
        let index = SessionIndex::open_in_dir(dir.path()).unwrap();
        let convs = vec![
            sample_conv("c1", "Auth work", "We need to refactor auth middleware"),
            sample_conv("c2", "Other", "Unrelated topic about cooking"),
        ];
        index.sync_conversations(&convs).unwrap();

        let discover = index
            .dispatch_tool_for_test(&json!({ "query": "auth refactor", "limit": 3 }))
            .unwrap();
        let parsed: Value = serde_json::from_str(&discover).unwrap();
        assert_eq!(parsed["success"], true);
        assert_eq!(parsed["mode"], "discovery");
        let results = parsed["results"].as_array().unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["conversation_id"], "c1");

        let scroll = index
            .dispatch_tool_for_test(&json!({
                "conversation_id": "c1",
                "around_message_id": "msg_u1",
                "window": 2,
                "_conversation_id": "c2"
            }))
            .unwrap();
        let scroll_p: Value = serde_json::from_str(&scroll).unwrap();
        assert_eq!(scroll_p["success"], true);
        assert_eq!(scroll_p["mode"], "scroll");
        assert!(scroll_p["messages"].as_array().unwrap().len() >= 1);

        let reject = index
            .dispatch_tool_for_test(&json!({
                "conversation_id": "c1",
                "around_message_id": "msg_u1",
                "_conversation_id": "c1"
            }))
            .unwrap();
        let reject_p: Value = serde_json::from_str(&reject).unwrap();
        assert_eq!(reject_p["success"], false);
    }

    #[test]
    fn discover_cjk_bigram() {
        let dir = TempDir::new().unwrap();
        let index = SessionIndex::open_in_dir(dir.path()).unwrap();
        index
            .sync_conversations(&vec![sample_conv(
                "c1",
                "认证",
                "我们需要重构认证中间件",
            )])
            .unwrap();

        let discover = index
            .dispatch_tool_for_test(&json!({ "query": "认证", "limit": 3 }))
            .unwrap();
        let parsed: Value = serde_json::from_str(&discover).unwrap();
        assert_eq!(parsed["success"], true);
        assert_eq!(parsed["results"].as_array().unwrap().len(), 1);
        assert_eq!(parsed["results"][0]["conversation_id"], "c1");
    }

    #[test]
    fn browse_lists_recent() {
        let dir = TempDir::new().unwrap();
        let index = SessionIndex::open_in_dir(dir.path()).unwrap();
        index
            .sync_conversations(&vec![
                sample_conv("c1", "Old", "alpha"),
                sample_conv("c2", "New", "beta"),
            ])
            .unwrap();

        let out = index
            .dispatch_tool_for_test(&json!({ "_conversation_id": "c2" }))
            .unwrap();
        let parsed: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["mode"], "browse");
        let ids: Vec<_> = parsed["results"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["conversation_id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, vec!["c1"]);
    }

    #[test]
    fn read_session_dump() {
        let dir = TempDir::new().unwrap();
        let index = SessionIndex::open_in_dir(dir.path()).unwrap();
        index
            .sync_conversations(&vec![sample_conv("c1", "T", "hello")])
            .unwrap();
        let out = index
            .dispatch_tool_for_test(&json!({ "conversation_id": "c1" }))
            .unwrap();
        let parsed: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["mode"], "read");
        assert_eq!(parsed["messages"].as_array().unwrap().len(), 2);
    }
}
