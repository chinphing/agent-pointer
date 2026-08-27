//! `session_search` tool queries against the canonical conversation store.

use std::time::Instant;

use anyhow::Result;
use chrono::{DateTime, Local, TimeZone, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};

use crate::media::manifest::attachment_summaries_json;
use crate::models::{ChatMessage, ConversationSearchHit, ConversationSearchMatch};
use crate::text_util::{match_centered_excerpt, match_centered_snippet, text_contains_query};

use super::db::DbHandle;
use super::persist::{
    is_session_search_tool_body, is_session_search_tool_name, SESSION_SEARCH_INDEX_STUB,
};
use super::ListScope;

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
/// Extra hits listed per conversation in the **session_search tool** (`matches[]`).
/// Sidebar UI loads every contiguous FTS hit in that conversation (no page).
const MATCHES_RETURN_MAX: usize = 5;
/// Prefix of `messages.content` copied into Rust for sidebar match snippets.
/// Full blobs (legacy `session_search` JSON) must not be loaded on each keystroke.
const UI_MATCH_CONTENT_PREFIX: usize = 16_384;
/// Cap sidebar per-conversation snippet rows (count can still be higher).
const UI_MATCHES_SNIPPET_MAX: usize = 200;
/// Unnamed tool rows larger than this are treated as legacy search dumps / huge
/// payloads and skipped in SQL (`length()` is cheap; `substr`+`instr` is not).
const UNNAMED_TOOL_SKIP_BYTES: i64 = 4_096;
/// Hit-centered `content` budget by role (tool outbound only; SQLite keeps full text).
const HIT_CONTENT_CHARS_USER: usize = 4_000;
const HIT_CONTENT_CHARS_ASSISTANT: usize = 2_500;
const HIT_CONTENT_CHARS_TOOL: usize = 1_500;
const HIT_CONTENT_CHARS_OTHER: usize = 1_500;

/// Sidebar / UI search: FTS over **full message bodies** (`messages_fts`), with
/// title/preview substring match as a supplement (same scope as the sidebar list).
pub fn search_conversations_for_ui(
    db: &DbHandle,
    scope: &ListScope,
    query: &str,
    limit: i64,
) -> Result<Vec<ConversationSearchHit>> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(Vec::new());
    }
    let started = Instant::now();
    let limit = limit.clamp(1, UI_SEARCH_MAX_LIMIT);
    let filter_uid = scope.filter_uid();
    let conn = db.conn.lock();
    let mut hits: std::collections::HashMap<String, ConversationSearchHit> =
        std::collections::HashMap::new();

    let fts_query = build_ui_fts_query(query);
    let mut fts_conv_count = 0usize;
    let rank_started = Instant::now();
    if !fts_query.is_empty() {
        match collect_fts_hits(&conn, &fts_query, filter_uid) {
            Ok(fts_hits) => {
                fts_conv_count = fts_hits.len();
                for hit in fts_hits {
                    hits.insert(hit.id.clone(), hit);
                }
            }
            Err(err) => {
                log::warn!("ui search FTS failed query={query:?}: {err:#}");
            }
        }
    }
    let rank_ms = rank_started.elapsed().as_millis();

    let like = format!("%{}%", query.to_lowercase());
    let mut stmt = conn.prepare(
        "SELECT id, title, updated_at_ms, message_count, preview
         FROM conversations
         WHERE id NOT LIKE 'cron:%'
           AND id NOT LIKE 'webhook:%'
           AND (?3 IS NULL OR session_user_id = ?3)
           AND (lower(title) LIKE ?1 OR lower(preview) LIKE ?1)
         ORDER BY updated_at_ms DESC
         LIMIT ?2",
    )?;
    let title_cap = limit * 2;
    let mapped = stmt.query_map(params![like, title_cap, filter_uid], |row| {
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
                matches: Vec::new(),
                match_count: 0,
            }
        });
    }

    let mut out: Vec<ConversationSearchHit> = hits.into_values().collect();
    out.sort_by_key(|h| std::cmp::Reverse(h.updated_at));
    out.truncate(limit as usize);
    if !fts_query.is_empty() {
        fill_ui_primary_message_ids(&conn, &mut out, &fts_query)?;
        reconcile_ui_match_counts(&conn, &mut out, query, &fts_query)?;
    }
    let matches_started = Instant::now();
    if !fts_query.is_empty() {
        for hit in &mut out {
            if hit.matches.is_empty() && !hit.message_id.is_empty() {
                if let Some((role, content, _)) = load_message_body_prefix(
                    &conn,
                    &hit.id,
                    &hit.message_id,
                    UI_MATCH_CONTENT_PREFIX,
                )? {
                    hit.snippet =
                        match_centered_snippet(&content, query, UI_SNIPPET_RADIUS, "", "");
                    if !hit.snippet.is_empty() {
                        hit.matches.push(ConversationSearchMatch {
                            message_id: hit.message_id.clone(),
                            role,
                            snippet: hit.snippet.clone(),
                        });
                        if hit.match_count == 0 {
                            hit.match_count = 1;
                        }
                    }
                }
            }
            if hit.snippet.is_empty() {
                hit.snippet = title_or_preview_snippet(query, &hit.title, &hit.preview);
            }
        }
    }
    let matches_ms = matches_started.elapsed().as_millis();
    log::info!(
        "ui search: query={query:?} fts_convs={fts_conv_count} returned={} rank_ms={rank_ms} matches_ms={matches_ms} elapsed_ms={}",
        out.len(),
        started.elapsed().as_millis()
    );
    Ok(out)
}

/// Skip prior `session_search` dumps in SQL without loading the full blob.
/// Named rows use `tool_name`; unnamed legacy dumps are huge tool rows.
fn sql_and_skip_session_search_dumps() -> String {
    format!(
        r#" AND COALESCE(m.tool_name, '') != 'session_search'
            AND m.content != '{stub}'
            AND NOT (
              lower(m.role) = 'tool'
              AND COALESCE(m.tool_name, '') = ''
              AND length(m.content) > {max_unnamed}
            )"#,
        stub = SESSION_SEARCH_INDEX_STUB.replace('\'', "''"),
        max_unnamed = UNNAMED_TOOL_SKIP_BYTES
    )
}

/// Sidebar search (keystroke + expand): people do not see tool bodies in the
/// transcript, so never rank or list `role = tool` rows. This also avoids
/// reading `messages.content` overflow for dumps. Agent `session_search` still
/// uses [`sql_and_skip_session_search_dumps`].
fn sql_and_skip_ui_tool_rows() -> &'static str {
    " AND lower(m.role) != 'tool'"
}

fn collect_fts_hits(
    conn: &Connection,
    fts_query: &str,
    filter_uid: Option<&str>,
) -> Result<Vec<ConversationSearchHit>> {
    // Full FTS recall for non-tool rows. Do not `ORDER BY bm25` (scores the
    // whole posting list) and do not JOIN `messages.content`. GROUP BY keeps
    // one row per conversation; primary `message_id` is filled after the UI
    // truncates to `limit`.
    let sql = format!(
        "SELECT m.conversation_id,
                COUNT(*) AS match_count,
                c.title, c.updated_at_ms, c.message_count, c.preview
         FROM messages_fts AS mf
         INNER JOIN messages AS m ON m.id = mf.rowid
         INNER JOIN conversations AS c ON c.id = m.conversation_id
         WHERE messages_fts MATCH ?1
           AND c.id NOT LIKE 'cron:%'
           AND c.id NOT LIKE 'webhook:%'
           AND (?2 IS NULL OR c.session_user_id = ?2)
           {skip}
         GROUP BY m.conversation_id",
        skip = sql_and_skip_ui_tool_rows()
    );
    let mut stmt = conn.prepare(&sql)?;
    let mapped = stmt.query_map(params![fts_query, filter_uid], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, i64>(3)?,
            row.get::<_, i64>(4)?,
            row.get::<_, String>(5)?,
        ))
    })?;

    let mut out = Vec::new();
    for row in mapped {
        let (id, match_count, title, updated_at, message_count, preview) = row?;
        out.push(ConversationSearchHit {
            id,
            snippet: String::new(),
            message_id: String::new(),
            title,
            updated_at,
            message_count: message_count.max(0) as u32,
            preview,
            matches: Vec::new(),
            match_count: match_count.max(0) as u32,
        });
    }
    out.sort_by_key(|h| std::cmp::Reverse(h.updated_at));
    Ok(out)
}

fn fill_ui_primary_message_ids(
    conn: &Connection,
    hits: &mut [ConversationSearchHit],
    fts_query: &str,
) -> Result<()> {
    let ids: Vec<String> = hits
        .iter()
        .filter(|h| h.message_id.is_empty() && h.match_count > 0)
        .map(|h| h.id.clone())
        .collect();
    if ids.is_empty() {
        return Ok(());
    }
    let in_list = sql_quoted_id_list(&ids);
    let sql = format!(
        "SELECT r.conversation_id, r.message_id
         FROM (
           SELECT m.conversation_id, m.message_id,
                  ROW_NUMBER() OVER (
                    PARTITION BY m.conversation_id
                    ORDER BY CASE lower(m.role)
                      WHEN 'assistant' THEN 0
                      WHEN 'user' THEN 1
                      ELSE 2
                    END, m.position ASC
                  ) AS rn
           FROM messages_fts AS mf
           INNER JOIN messages AS m ON m.id = mf.rowid
           WHERE messages_fts MATCH ?1
             AND m.conversation_id IN ({in_list})
             {skip}
         ) AS r
         WHERE r.rn = 1",
        skip = sql_and_skip_ui_tool_rows()
    );
    let mut stmt = conn.prepare(&sql)?;
    let mapped = stmt.query_map(params![fts_query], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut by_id = std::collections::HashMap::new();
    for row in mapped {
        let (id, message_id) = row?;
        by_id.insert(id, message_id);
    }
    for hit in hits.iter_mut() {
        if hit.message_id.is_empty() {
            if let Some(message_id) = by_id.remove(&hit.id) {
                hit.message_id = message_id;
            }
        }
    }
    Ok(())
}

/// Align sidebar `match_count` with expand: same FTS rows, prefix, and
/// `text_contains_query` filters — raw `COUNT(*)` alone can over-count.
fn reconcile_ui_match_counts(
    conn: &Connection,
    hits: &mut [ConversationSearchHit],
    raw_query: &str,
    fts_query: &str,
) -> Result<()> {
    let ids: Vec<String> = hits
        .iter()
        .filter(|h| h.match_count > 0)
        .map(|h| h.id.clone())
        .collect();
    if ids.is_empty() {
        return Ok(());
    }
    let by_conv = collect_ui_matches_via_fts(conn, &ids, raw_query, fts_query)?;
    for hit in hits.iter_mut() {
        if let Some((_matches, count)) = by_conv.get(&hit.id) {
            hit.match_count = *count;
        }
    }
    Ok(())
}

fn search_hit_role_rank(role: &str) -> u8 {
    match role.trim().to_ascii_lowercase().as_str() {
        "assistant" => 0,
        "user" => 1,
        "tool" => 3,
        _ => 2,
    }
}

fn sort_matches_assistant_first(matches: &mut [ConversationSearchMatch]) {
    matches.sort_by_key(|m| search_hit_role_rank(&m.role));
}

/// Contiguous hits via FTS (not `content LIKE`). One query for all listed conversations.
/// Only a content prefix is copied into Rust so huge tool blobs stay on disk.
fn collect_ui_matches_via_fts(
    conn: &Connection,
    conversation_ids: &[String],
    raw_query: &str,
    fts_query: &str,
) -> Result<std::collections::HashMap<String, (Vec<ConversationSearchMatch>, u32)>> {
    let q = raw_query.trim();
    let mut by_conv: std::collections::HashMap<String, (Vec<ConversationSearchMatch>, u32)> =
        std::collections::HashMap::new();
    if q.is_empty() || fts_query.is_empty() || conversation_ids.is_empty() {
        return Ok(by_conv);
    }
    let in_list = sql_quoted_id_list(conversation_ids);
    let sql = format!(
        "SELECT m.conversation_id, m.message_id, m.role, substr(m.content, 1, {prefix})
         FROM messages_fts AS mf
         INNER JOIN messages AS m ON m.id = mf.rowid
         WHERE messages_fts MATCH ?1
           AND m.conversation_id IN ({in_list})
           {skip}
         ORDER BY m.position ASC",
        prefix = UI_MATCH_CONTENT_PREFIX,
        skip = sql_and_skip_ui_tool_rows()
    );
    let mut stmt = conn.prepare(&sql)?;
    let mapped = stmt.query_map(params![fts_query], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
        ))
    })?;
    let mut seen_by_conv: std::collections::HashMap<String, std::collections::HashSet<String>> =
        std::collections::HashMap::new();
    for row in mapped {
        let (conversation_id, message_id, role, content) = row?;
        if is_omitted_session_search_row(&role, &content, None) {
            continue;
        }
        if !text_contains_query(&content, q) {
            continue;
        }
        let snippet = match_centered_snippet(&content, q, UI_SNIPPET_RADIUS, "", "");
        let entry = by_conv
            .entry(conversation_id.clone())
            .or_insert_with(|| (Vec::new(), 0));
        let seen = seen_by_conv.entry(conversation_id).or_default();
        push_search_match(
            &mut entry.0,
            seen,
            &mut entry.1,
            message_id,
            role,
            snippet,
            UI_MATCHES_SNIPPET_MAX,
        );
    }
    for (matches, _) in by_conv.values_mut() {
        sort_matches_assistant_first(matches);
    }
    Ok(by_conv)
}

fn sql_quoted_id_list(ids: &[String]) -> String {
    ids.iter()
        .map(|id| format!("'{}'", id.replace('\'', "''")))
        .collect::<Vec<_>>()
        .join(",")
}

/// Sidebar expand: every contiguous FTS hit in one conversation (prefix only).
pub fn list_conversation_search_matches(
    db: &DbHandle,
    scope: &ListScope,
    conversation_id: &str,
    query: &str,
) -> Result<Vec<ConversationSearchMatch>> {
    let query = query.trim();
    let conversation_id = conversation_id.trim();
    if query.is_empty() || conversation_id.is_empty() {
        return Ok(Vec::new());
    }
    let fts_query = build_ui_fts_query(query);
    if fts_query.is_empty() {
        return Ok(Vec::new());
    }
    let conn = db.conn.lock();
    if !conversation_in_scope(&conn, conversation_id, scope.filter_uid())? {
        return Err(anyhow::anyhow!("conversation not found"));
    }
    let mut by_conv =
        collect_ui_matches_via_fts(&conn, &[conversation_id.to_string()], query, &fts_query)?;
    Ok(by_conv
        .remove(conversation_id)
        .map(|(matches, _)| matches)
        .unwrap_or_default())
}

fn conversation_in_scope(
    conn: &Connection,
    conversation_id: &str,
    filter_uid: Option<&str>,
) -> Result<bool> {
    let n: i64 = match filter_uid {
        Some(uid) => conn.query_row(
            "SELECT COUNT(*) FROM conversations WHERE id = ?1 AND session_user_id = ?2",
            params![conversation_id, uid],
            |row| row.get(0),
        )?,
        None => conn.query_row(
            "SELECT COUNT(*) FROM conversations WHERE id = ?1",
            params![conversation_id],
            |row| row.get(0),
        )?,
    };
    Ok(n > 0)
}

fn push_search_match(
    matches: &mut Vec<ConversationSearchMatch>,
    seen: &mut std::collections::HashSet<String>,
    match_count: &mut u32,
    message_id: String,
    role: String,
    snippet: String,
    max: usize,
) {
    if message_id.trim().is_empty() || !seen.insert(message_id.clone()) {
        return;
    }
    *match_count = match_count.saturating_add(1);
    if matches.len() < max {
        matches.push(ConversationSearchMatch {
            message_id,
            role,
            snippet,
        });
    }
}

fn promote_primary_match(matches: &mut [ConversationSearchMatch], primary_id: &str) {
    let id = primary_id.trim();
    if id.is_empty() {
        return;
    }
    if let Some(i) = matches.iter().position(|m| m.message_id == id) {
        matches.rotate_left(i);
    }
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
    let started = Instant::now();
    let fts_query = build_fts_query(query);
    let conn = db.conn.lock();

    let mut hits: Vec<FtsHit> = Vec::new();
    {
        let sql = format!(
            "SELECT mf.conversation_id, mf.message_id, mf.role,
                    m.tool_name, bm25(messages_fts) AS rank
             FROM messages_fts AS mf
             INNER JOIN messages AS m ON m.id = mf.rowid
             INNER JOIN conversations AS c ON c.id = mf.conversation_id
             WHERE messages_fts MATCH ?1
               AND c.session_user_id = ?2
               {skip}
             ORDER BY rank
             LIMIT ?3",
            skip = sql_and_skip_session_search_dumps()
        );
        let mut stmt = conn.prepare(&sql)?;
        let cap = limit * 24;
        let mapped = stmt.query_map(params![fts_query, session_user_filter, cap], |row| {
            Ok(FtsHit {
                conversation_id: row.get(0)?,
                message_id: row.get(1)?,
                role: row.get(2)?,
                content: String::new(),
                tool_name: row.get(3)?,
                snippet: String::new(),
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

    struct DiscGroup {
        primary: FtsHit,
        matches: Vec<ConversationSearchMatch>,
        seen: std::collections::HashSet<String>,
        match_count: u32,
    }

    let mut groups: std::collections::HashMap<String, DiscGroup> = std::collections::HashMap::new();
    let mut order: Vec<String> = Vec::new();
    for hit in hits {
        if current_conversation_id == Some(hit.conversation_id.as_str()) {
            continue;
        }
        if let Some(group) = groups.get_mut(&hit.conversation_id) {
            push_search_match(
                &mut group.matches,
                &mut group.seen,
                &mut group.match_count,
                hit.message_id,
                hit.role,
                hit.snippet,
                usize::MAX,
            );
            continue;
        }
        if (order.len() as i64) >= limit {
            continue;
        }
        let mut group = DiscGroup {
            primary: hit.clone(),
            matches: Vec::new(),
            seen: std::collections::HashSet::new(),
            match_count: 0,
        };
        push_search_match(
            &mut group.matches,
            &mut group.seen,
            &mut group.match_count,
            hit.message_id,
            hit.role,
            hit.snippet,
            usize::MAX,
        );
        order.push(hit.conversation_id.clone());
        groups.insert(hit.conversation_id, group);
    }

    if sort == Some("oldest") {
        order.sort_by_key(|id| meta_updated_at(&conn, id).unwrap_or(0));
    } else {
        order.sort_by_key(|id| std::cmp::Reverse(meta_updated_at(&conn, id).unwrap_or(0)));
    }

    let mut results = Vec::new();
    for conversation_id in order {
        let Some(mut group) = groups.remove(&conversation_id) else {
            continue;
        };
        sort_matches_assistant_first(&mut group.matches);
        if let Some(first) = group.matches.first() {
            group.primary.message_id = first.message_id.clone();
            group.primary.role = first.role.clone();
        }
        if group.matches.len() > MATCHES_RETURN_MAX {
            group.matches.truncate(MATCHES_RETURN_MAX);
        }
        promote_primary_match(&mut group.matches, &group.primary.message_id);
        hydrate_discover_snippets(
            &conn,
            &conversation_id,
            query,
            &mut group.primary,
            &mut group.matches,
        )?;
        let meta = load_meta(&conn, &conversation_id)?;
        let Some(meta) = meta else {
            continue;
        };
        let anchor_pos = message_position(&conn, &conversation_id, &group.primary.message_id)?;
        let window = if anchor_pos >= 0 {
            load_window(&conn, &conversation_id, anchor_pos, DEFAULT_WINDOW, query)?
        } else {
            WindowView {
                messages: vec![],
                messages_before: 0,
                messages_after: 0,
            }
        };
        let bookend_start = load_bookends(&conn, &conversation_id, true, query)?;
        let bookend_end = load_bookends(&conn, &conversation_id, false, query)?;
        let matches_json: Vec<Value> = group
            .matches
            .iter()
            .map(|m| {
                json!({
                    "id": m.message_id,
                    "role": m.role,
                    "snippet": m.snippet,
                })
            })
            .collect();

        results.push(json!({
            "conversation_id": conversation_id,
            "title": nullable_str(&meta.title),
            "when": format_timestamp_ms(meta.updated_at_ms),
            "snippet": group.primary.snippet,
            "match_message_id": group.primary.message_id,
            "match_count": group.match_count,
            "matches": matches_json,
            "messages_before": window.messages_before,
            "messages_after": window.messages_after,
            "bookend_start": bookend_start,
            "messages": window.messages,
            "bookend_end": bookend_end,
        }));
    }

    log::info!(
        "session_search: discover query={query:?} groups={} elapsed_ms={}",
        results.len(),
        started.elapsed().as_millis()
    );
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

    let view = load_window(&conn, conversation_id, anchor_pos, window, "")?;
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

    let all = load_all_messages(&conn, conversation_id, "")?;
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

#[derive(Clone)]
struct FtsHit {
    conversation_id: String,
    message_id: String,
    role: String,
    content: String,
    #[allow(dead_code)]
    tool_name: Option<String>,
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

fn hydrate_discover_snippets(
    conn: &Connection,
    conversation_id: &str,
    query: &str,
    primary: &mut FtsHit,
    matches: &mut [ConversationSearchMatch],
) -> Result<()> {
    let mut ids = vec![primary.message_id.clone()];
    for m in matches.iter() {
        if m.message_id != primary.message_id {
            ids.push(m.message_id.clone());
        }
    }
    for id in ids {
        let Some((role, content, tool_name)) = load_message_body(conn, conversation_id, &id)?
        else {
            continue;
        };
        if is_omitted_session_search_row(&role, &content, tool_name.as_deref()) {
            log::info!(
                "session_search: skip_prior_tool_hit conversation_id={conversation_id} message_id={id}"
            );
            continue;
        }
        let snippet = match_centered_snippet(&content, query, TOOL_SNIPPET_RADIUS, "<b>", "</b>");
        if id == primary.message_id {
            primary.content = content;
            primary.snippet = snippet.clone();
        }
        if let Some(m) = matches.iter_mut().find(|m| m.message_id == id) {
            m.snippet = snippet;
        }
    }
    Ok(())
}

fn load_message_body(
    conn: &Connection,
    conversation_id: &str,
    message_id: &str,
) -> Result<Option<(String, String, Option<String>)>> {
    load_message_body_prefix(conn, conversation_id, message_id, usize::MAX)
}

fn load_message_body_prefix(
    conn: &Connection,
    conversation_id: &str,
    message_id: &str,
    prefix: usize,
) -> Result<Option<(String, String, Option<String>)>> {
    let sql = if prefix == usize::MAX {
        "SELECT role, content, tool_name FROM messages
         WHERE conversation_id = ?1 AND message_id = ?2"
            .to_string()
    } else {
        format!(
            "SELECT role, substr(content, 1, {prefix}), tool_name FROM messages
             WHERE conversation_id = ?1 AND message_id = ?2"
        )
    };
    conn.query_row(&sql, params![conversation_id, message_id], |row| {
        Ok((row.get(0)?, row.get(1)?, row.get(2)?))
    })
    .optional()
    .map_err(Into::into)
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
    query: &str,
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
        "SELECT message_id, role, content, created_at_ms, payload, tool_name
         FROM messages
         WHERE conversation_id = ?1 AND position BETWEEN ?2 AND ?3
           AND NOT (role = 'tool' AND content = ?4)
           AND COALESCE(tool_name, '') != 'session_search'
         ORDER BY position ASC",
    )?;
    let anchor_message_id: String = conn.query_row(
        "SELECT message_id FROM messages WHERE conversation_id = ?1 AND position = ?2",
        params![conversation_id, anchor_pos],
        |row| row.get(0),
    )?;
    let rows = stmt.query_map(
        params![conversation_id, start, end, SESSION_SEARCH_INDEX_STUB],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, Option<String>>(5)?,
            ))
        },
    )?;

    let mut messages = Vec::new();
    for row in rows {
        let (id, role, content, ts, payload, tool_name) = row?;
        let is_anchor = id == anchor_message_id;
        if let Some(entry) = session_search_message_json(
            id,
            role,
            content,
            ts,
            &payload,
            tool_name.as_deref(),
            is_anchor,
            query,
        ) {
            messages.push(entry);
        }
    }

    Ok(WindowView {
        messages,
        messages_before,
        messages_after,
    })
}

fn load_bookends(
    conn: &Connection,
    conversation_id: &str,
    start: bool,
    query: &str,
) -> Result<Vec<Value>> {
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
        if let Some(entry) =
            session_search_message_json(id, role, content, ts, &payload, None, false, query)
        {
            out.push(entry);
        }
    }
    if !start {
        out.reverse();
    }
    Ok(out)
}

fn load_all_messages(conn: &Connection, conversation_id: &str, query: &str) -> Result<Vec<Value>> {
    let mut stmt = conn.prepare(
        "SELECT message_id, role, content, created_at_ms, payload, tool_name
         FROM messages
         WHERE conversation_id = ?1
           AND NOT (role = 'tool' AND content = ?2)
           AND COALESCE(tool_name, '') != 'session_search'
         ORDER BY position ASC",
    )?;
    let rows = stmt.query_map(params![conversation_id, SESSION_SEARCH_INDEX_STUB], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, i64>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, Option<String>>(5)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, role, content, ts, payload, tool_name) = row?;
        if let Some(entry) = session_search_message_json(
            id,
            role,
            content,
            ts,
            &payload,
            tool_name.as_deref(),
            false,
            query,
        ) {
            out.push(entry);
        }
    }
    Ok(out)
}

fn hit_content_char_limit(role: &str) -> usize {
    match role.trim().to_ascii_lowercase().as_str() {
        "user" => HIT_CONTENT_CHARS_USER,
        "assistant" => HIT_CONTENT_CHARS_ASSISTANT,
        "tool" => HIT_CONTENT_CHARS_TOOL,
        _ => HIT_CONTENT_CHARS_OTHER,
    }
}

fn is_omitted_session_search_row(role: &str, content: &str, tool_name: Option<&str>) -> bool {
    if !role.eq_ignore_ascii_case("tool") {
        return false;
    }
    if is_session_search_tool_name(tool_name) || content.trim() == SESSION_SEARCH_INDEX_STUB {
        return true;
    }
    // Unnamed legacy rows only: inspect the already-fetched envelope head.
    tool_name.map(str::trim).filter(|s| !s.is_empty()).is_none()
        && is_session_search_tool_body(content)
}

fn session_search_message_json(
    id: String,
    role: String,
    content: String,
    created_at_ms: i64,
    payload: &str,
    tool_name: Option<&str>,
    anchor: bool,
    query: &str,
) -> Option<Value> {
    if is_omitted_session_search_row(&role, &content, tool_name) {
        log::info!(
            "session_search: omit_prior_tool_result message_id={} role={}",
            id,
            role
        );
        return None;
    }
    let mut display_content = content;
    let mut attachments = None;
    if let Ok(msg) = serde_json::from_str::<ChatMessage>(payload) {
        if is_omitted_session_search_row(&role, &msg.content, msg.tool_name.as_deref()) {
            log::info!(
                "session_search: omit_prior_tool_result message_id={} role={}",
                id,
                role
            );
            return None;
        }
        display_content = msg.content;
        attachments = msg.attachments.filter(|a| !a.is_empty());
    }
    let limit = hit_content_char_limit(&role);
    let orig_chars = display_content.chars().count();
    let truncated = orig_chars > limit;
    if truncated {
        display_content = match_centered_excerpt(&display_content, query, limit);
        if orig_chars > limit.saturating_mul(4) {
            log::info!(
                "session_search: clip_hit_content message_id={} role={} chars={} limit={}",
                id,
                role,
                orig_chars,
                limit
            );
        }
    }
    let mut entry = json!({
        "id": id,
        "role": role,
        "content": display_content,
        "timestamp": format_timestamp_ms(created_at_ms),
    });
    if let Some(atts) = attachments {
        entry["attachments"] = json!(attachment_summaries_json(&atts));
    }
    if truncated {
        entry["truncated"] = json!(true);
        entry["contentChars"] = json!(orig_chars);
        entry["contentLimit"] = json!(limit);
    }
    if anchor {
        entry["anchor"] = json!(true);
    }
    Some(entry)
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

/// Sidebar MATCH: search `content` only, drop `role = tool` in the FTS index
/// (`role` is a tokenized column). Agent `session_search` uses [`build_fts_query`]
/// without this filter so tool bodies stay searchable.
fn build_ui_fts_query(raw: &str) -> String {
    let inner = build_fts_query(raw);
    if inner.is_empty() {
        return String::new();
    }
    format!("{{content}}: ({inner}) NOT {{role}}: tool")
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
