//! Sidebar conversation search (UI). Agent tools are in the sibling recall module.

use std::time::Instant;

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use crate::models::{ConversationSearchHit, ConversationSearchMatch};
use crate::text_util::{match_centered_snippet, text_contains_query};

use super::db::DbHandle;
use super::ListScope;

const UI_SEARCH_MAX_LIMIT: i64 = 100;
/// Unicode scalars kept on each side of the matched query in sidebar snippets.
const UI_SNIPPET_RADIUS: usize = 20;
/// Prefix of `messages.content` copied into Rust for sidebar match snippets.
const UI_MATCH_CONTENT_PREFIX: usize = 16_384;
/// Cap sidebar per-conversation snippet rows (count can still be higher).
const UI_MATCHES_SNIPPET_MAX: usize = 200;

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

/// Sidebar search (keystroke + expand): people do not see tool bodies in the
/// transcript, so never rank or list `role = tool` rows.
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

pub(super) fn sort_matches_assistant_first(matches: &mut [ConversationSearchMatch]) {
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

pub(super) fn push_search_match(
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

pub(super) fn promote_primary_match(matches: &mut [ConversationSearchMatch], primary_id: &str) {
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

pub(super) fn build_fts_query(raw: &str) -> String {
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

