//! `session_search` tool queries against the canonical conversation store.

use anyhow::Result;
use chrono::{DateTime, Local, TimeZone, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};

use crate::media::manifest::attachment_summaries_json;
use crate::models::{ChatMessage, ConversationSearchHit};
use crate::text_util::{match_centered_snippet, text_contains_query};

use super::db::DbHandle;

const DEFAULT_WINDOW: i64 = 5;
const MAX_WINDOW: i64 = 20;
const DEFAULT_LIMIT: i64 = 3;
const MAX_LIMIT: i64 = 10;
const READ_HEAD: i64 = 20;
const READ_TAIL: i64 = 10;
const BOOKEND_COUNT: i64 = 3;
const UI_SEARCH_MAX_LIMIT: i64 = 100;
/// Unicode scalars kept on each side of the matched query in sidebar snippets.
const UI_SNIPPET_RADIUS: usize = 20;
const TOOL_SNIPPET_RADIUS: usize = 48;

/// Sidebar / UI search: FTS over **full message bodies** (`messages_fts`), with
/// title/preview substring match as a supplement (same scope as the sidebar list).
pub fn search_conversations_for_ui(
    db: &DbHandle,
    query: &str,
    limit: i64,
) -> Result<Vec<ConversationSearchHit>> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(Vec::new());
    }
    let limit = limit.clamp(1, UI_SEARCH_MAX_LIMIT);
    let conn = db.conn.lock();
    let mut hits: std::collections::HashMap<String, ConversationSearchHit> =
        std::collections::HashMap::new();

    let fts_query = build_fts_query(query);
    if !fts_query.is_empty() {
        match collect_fts_hits(&conn, query, &fts_query, limit) {
            Ok(fts_hits) => {
                for hit in fts_hits {
                    hits.insert(hit.id.clone(), hit);
                }
            }
            Err(err) => {
                log::warn!("ui search FTS failed query={query:?}: {err:#}");
            }
        }
    }

    let like = format!("%{}%", query.to_lowercase());
    let mut stmt = conn.prepare(
        "SELECT id, title, updated_at_ms, message_count, preview
         FROM conversations
         WHERE id NOT LIKE 'cron:%'
           AND id NOT LIKE 'webhook:%'
           AND (lower(title) LIKE ?1 OR lower(preview) LIKE ?1)
         ORDER BY updated_at_ms DESC
         LIMIT ?2",
    )?;
    let title_cap = limit * 2;
    let mapped = stmt.query_map(params![like, title_cap], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, i64>(3)?,
            row.get::<_, String>(4)?,
        ))
    })?;
    for row in mapped {
        let (id, title, updated_at_ms, message_count, preview) = row?;
        hits.entry(id.clone()).or_insert_with(|| {
            let snippet = title_or_preview_snippet(query, &title, &preview);
            ConversationSearchHit {
                id,
                title,
                updated_at: updated_at_ms,
                snippet,
                message_id: String::new(),
                message_count: message_count.max(0) as u32,
                preview,
            }
        });
    }

    let mut out: Vec<ConversationSearchHit> = hits.into_values().collect();
    out.sort_by_key(|h| std::cmp::Reverse(h.updated_at));
    out.truncate(limit as usize);
    Ok(out)
}

fn collect_fts_hits(
    conn: &Connection,
    raw_query: &str,
    fts_query: &str,
    limit: i64,
) -> Result<Vec<ConversationSearchHit>> {
    // Join `messages` for body text: FTS5 external-content can leave `mf.content`
    // awkward to rely on; always read the canonical row.
    //
    // Do not keep only the first (best bm25) row per conversation — that message
    // may match via token overlap without a contiguous query substring, which
    // made match_centered_snippet fall back to the document head. Prefer a row
    // that actually contains the query string.
    let sql = "SELECT m.conversation_id,
                      m.message_id,
                      m.content,
                      c.title, c.updated_at_ms, c.message_count, c.preview
               FROM messages_fts AS mf
               INNER JOIN messages AS m ON m.id = mf.rowid
               INNER JOIN conversations AS c ON c.id = m.conversation_id
               WHERE messages_fts MATCH ?1
                 AND c.id NOT LIKE 'cron:%'
                 AND c.id NOT LIKE 'webhook:%'
               ORDER BY bm25(messages_fts)
               LIMIT ?2";
    let cap = limit * 24;
    let mut stmt = conn.prepare(sql)?;
    let mapped = stmt.query_map(params![fts_query, cap], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, i64>(4)?,
            row.get::<_, i64>(5)?,
            row.get::<_, String>(6)?,
        ))
    })?;

    // conversation_id -> (has_contiguous_hit, hit)
    let mut best: std::collections::HashMap<String, (bool, ConversationSearchHit)> =
        std::collections::HashMap::new();
    for row in mapped {
        let (id, message_id, content, title, updated_at, message_count, preview) = row?;
        let contiguous = text_contains_query(&content, raw_query);
        let (snippet, resolved_message_id) = if contiguous {
            (
                match_centered_snippet(&content, raw_query, UI_SNIPPET_RADIUS, "", ""),
                message_id,
            )
        } else if let Some((mid, snip)) =
            find_contiguous_snippet_in_conversation(conn, &id, raw_query)?
        {
            // FTS ranked a weak row first; locate a real substring hit in the same conversation.
            (snip, mid)
        } else {
            // Last resort: still show something from this row (may be head).
            (
                match_centered_snippet(&content, raw_query, UI_SNIPPET_RADIUS, "", ""),
                message_id,
            )
        };
        let hit = ConversationSearchHit {
            id: id.clone(),
            snippet,
            message_id: resolved_message_id,
            title,
            updated_at,
            message_count: message_count.max(0) as u32,
            preview,
        };
        let has_hit = contiguous || text_contains_query(&hit.snippet, raw_query);
        match best.get(&id) {
            Some((true, _)) => {}
            Some((false, _)) if !has_hit => {}
            _ => {
                best.insert(id, (has_hit, hit));
            }
        }
    }

    let mut out: Vec<ConversationSearchHit> = best.into_values().map(|(_, h)| h).collect();
    out.sort_by_key(|h| std::cmp::Reverse(h.updated_at));
    Ok(out)
}

/// Scan messages in a conversation for a contiguous query substring.
/// Returns `(message_id, snippet)` for the first match.
fn find_contiguous_snippet_in_conversation(
    conn: &Connection,
    conversation_id: &str,
    raw_query: &str,
) -> Result<Option<(String, String)>> {
    let q = raw_query.trim();
    if q.is_empty() {
        return Ok(None);
    }
    let like = format!("%{q}%");
    let mut stmt = conn.prepare(
        "SELECT message_id, content FROM messages
         WHERE conversation_id = ?1 AND content LIKE ?2
         ORDER BY position ASC
         LIMIT 8",
    )?;
    let mapped = stmt.query_map(params![conversation_id, like], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    for row in mapped {
        let (message_id, content) = row?;
        if text_contains_query(&content, q) {
            return Ok(Some((
                message_id,
                match_centered_snippet(&content, q, UI_SNIPPET_RADIUS, "", ""),
            )));
        }
    }
    Ok(None)
}

fn title_or_preview_snippet(query: &str, title: &str, preview: &str) -> String {
    let title_l = title.to_lowercase();
    let q_l = query.to_lowercase();
    if !q_l.is_empty() && title_l.contains(&q_l) {
        return match_centered_snippet(title, query, UI_SNIPPET_RADIUS, "", "");
    }
    if text_contains_query(preview, query) {
        return match_centered_snippet(preview, query, UI_SNIPPET_RADIUS, "", "");
    }
    if !preview.trim().is_empty() {
        return match_centered_snippet(preview, query, UI_SNIPPET_RADIUS, "", "");
    }
    match_centered_snippet(title, query, UI_SNIPPET_RADIUS, "", "")
}

pub fn dispatch_tool(db: &DbHandle, args: &Value) -> Result<String> {
    dispatch_tool_inner(db, args)
}

fn dispatch_tool_inner(db: &DbHandle, args: &Value) -> Result<String> {
    let current_conversation_id = args
        .get("_conversation_id")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let session_user_filter = parse_session_user_filter(args);

    let conversation_id = args
        .get("conversation_id")
        .or_else(|| args.get("session_id"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let around_message_id = args.get("around_message_id").and_then(|v| {
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
        return scroll(
            db,
            cid,
            &anchor,
            window,
            current_conversation_id,
            session_user_filter,
        );
    }

    if let Some(cid) = conversation_id {
        return read_session(db, cid, session_user_filter);
    }

    let query = args
        .get("query")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());

    if query.is_none() {
        let limit = parse_limit(args.get("limit"))?;
        return browse(db, limit, current_conversation_id, session_user_filter);
    }

    let limit = parse_limit(args.get("limit"))?;
    let role_filter = parse_role_filter(args.get("role_filter"));
    let sort = parse_sort(args.get("sort"));
    discover(
        db,
        query.unwrap(),
        limit,
        role_filter.as_deref(),
        sort,
        current_conversation_id,
        session_user_filter,
    )
}

fn parse_session_user_filter(args: &Value) -> &str {
    args.get("_session_user_id")
        .and_then(|v| v.as_str())
        .map(super::session_user::normalize_session_user_id)
        .unwrap_or("")
}

fn browse(
    db: &DbHandle,
    limit: i64,
    current_conversation_id: Option<&str>,
    session_user_filter: &str,
) -> Result<String> {
    let conn = db.conn.lock();
    let fetch = limit + 5;
    let mut stmt = conn.prepare(
        "SELECT id, title, created_at_ms, updated_at_ms, message_count, preview
         FROM conversations
         WHERE session_user_id = ?2
         ORDER BY updated_at_ms DESC
         LIMIT ?1",
    )?;
    let rows = stmt.query_map(params![fetch, session_user_filter], |row| {
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
    db: &DbHandle,
    query: &str,
    limit: i64,
    role_filter: Option<&[String]>,
    sort: Option<&str>,
    current_conversation_id: Option<&str>,
    session_user_filter: &str,
) -> Result<String> {
    let fts_query = build_fts_query(query);
    let conn = db.conn.lock();

    let mut hits: Vec<FtsHit> = Vec::new();
    {
        let sql = "SELECT mf.conversation_id, mf.message_id, mf.role, m.content,
                          bm25(messages_fts) AS rank
                   FROM messages_fts AS mf
                   INNER JOIN messages AS m ON m.id = mf.rowid
                   INNER JOIN conversations AS c ON c.id = mf.conversation_id
                   WHERE messages_fts MATCH ?1
                     AND c.session_user_id = ?2
                   ORDER BY rank
                   LIMIT ?3";
        let mut stmt = conn.prepare(sql)?;
        let cap = limit * 12;
        let mapped = stmt.query_map(params![fts_query, session_user_filter, cap], |row| {
            let content: String = row.get(3)?;
            Ok(FtsHit {
                conversation_id: row.get(0)?,
                message_id: row.get(1)?,
                role: row.get(2)?,
                snippet: match_centered_snippet(
                    &content,
                    query,
                    TOOL_SNIPPET_RADIUS,
                    "<b>",
                    "</b>",
                ),
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
        deduped.sort_by_key(|h| meta_updated_at(&conn, &h.conversation_id).unwrap_or(0));
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
    db: &DbHandle,
    conversation_id: &str,
    around_message_id: &str,
    window: i64,
    current_conversation_id: Option<&str>,
    session_user_filter: &str,
) -> Result<String> {
    if current_conversation_id == Some(conversation_id) {
        return Ok(error_json(
            "scroll rejected: target is the current conversation (already in your active context)",
        ));
    }

    let conn = db.conn.lock();
    if load_meta_for_session_user(&conn, conversation_id, Some(session_user_filter))?.is_none() {
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

fn read_session(db: &DbHandle, conversation_id: &str, session_user_filter: &str) -> Result<String> {
    let conn = db.conn.lock();
    let meta = load_meta_for_session_user(&conn, conversation_id, Some(session_user_filter))?;
    let Some(meta) = meta else {
        return Ok(error_json(&format!(
            "conversation_id not found: {conversation_id}"
        )));
    };

    let all = load_all_messages(&conn, conversation_id)?;
    let total = all.len() as i64;
    let truncated = total > READ_HEAD + READ_TAIL;
    let window: Vec<Value> = if truncated {
        let head = all
            .iter()
            .take(READ_HEAD as usize)
            .cloned()
            .collect::<Vec<_>>();
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
    load_meta_for_session_user(conn, conversation_id, None)
}

fn load_meta_for_session_user(
    conn: &Connection,
    conversation_id: &str,
    session_user_filter: Option<&str>,
) -> Result<Option<MetaRow>> {
    match session_user_filter {
        Some(uid) => conn
            .query_row(
                "SELECT title, created_at_ms, updated_at_ms FROM conversations
                 WHERE id = ?1 AND session_user_id = ?2",
                params![conversation_id, uid],
                |row| {
                    Ok(MetaRow {
                        title: row.get(0)?,
                        created_at_ms: row.get(1)?,
                        updated_at_ms: row.get(2)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into),
        None => conn
            .query_row(
                "SELECT title, created_at_ms, updated_at_ms FROM conversations WHERE id = ?1",
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
            .map_err(Into::into),
    }
}

fn meta_updated_at(conn: &Connection, conversation_id: &str) -> Result<i64> {
    Ok(conn
        .query_row(
            "SELECT updated_at_ms FROM conversations WHERE id = ?1",
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
        "SELECT message_id, role, content, created_at_ms, payload
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
            row.get::<_, String>(4)?,
        ))
    })?;

    let mut messages = Vec::new();
    for row in rows {
        let (id, role, content, ts, payload) = row?;
        let is_anchor = id == anchor_message_id;
        messages.push(session_search_message_json(
            id, role, content, ts, &payload, is_anchor,
        ));
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
        "SELECT message_id, role, content, created_at_ms, payload
         FROM messages
         WHERE conversation_id = ?1 AND role IN ('user', 'assistant')
         ORDER BY position {order}
         LIMIT ?2"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![conversation_id, BOOKEND_COUNT], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, i64>(3)?,
            row.get::<_, String>(4)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, role, content, ts, payload) = row?;
        out.push(session_search_message_json(
            id, role, content, ts, &payload, false,
        ));
    }
    if !start {
        out.reverse();
    }
    Ok(out)
}

fn load_all_messages(conn: &Connection, conversation_id: &str) -> Result<Vec<Value>> {
    let mut stmt = conn.prepare(
        "SELECT message_id, role, content, created_at_ms, payload
         FROM messages
         WHERE conversation_id = ?1
         ORDER BY position ASC",
    )?;
    let rows = stmt.query_map(params![conversation_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, i64>(3)?,
            row.get::<_, String>(4)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, role, content, ts, payload) = row?;
        out.push(session_search_message_json(
            id, role, content, ts, &payload, false,
        ));
    }
    Ok(out)
}

fn session_search_message_json(
    id: String,
    role: String,
    content: String,
    created_at_ms: i64,
    payload: &str,
    anchor: bool,
) -> Value {
    let mut display_content = content;
    let mut entry = json!({
        "id": id,
        "role": role,
        "content": &display_content,
        "timestamp": format_timestamp_ms(created_at_ms),
    });
    if let Ok(msg) = serde_json::from_str::<ChatMessage>(payload) {
        display_content = msg.content;
        entry["content"] = json!(display_content);
        if let Some(atts) = msg.attachments.filter(|a| !a.is_empty()) {
            entry["attachments"] = json!(attachment_summaries_json(&atts));
        }
    }
    if anchor {
        entry["anchor"] = json!(true);
    }
    entry
}

fn build_fts_query(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.contains('"')
        || trimmed.contains('*')
        || trimmed.to_ascii_uppercase().contains(" OR ")
    {
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
pub fn dispatch_tool_for_test(db: &DbHandle, args: &Value) -> Result<String> {
    dispatch_tool_inner(db, args)
}
