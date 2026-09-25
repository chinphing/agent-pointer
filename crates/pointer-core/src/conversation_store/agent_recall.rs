//! Agent `session_search` / `session_read` against the canonical store.

use std::time::Instant;

use anyhow::Result;
use chrono::{DateTime, Local, TimeZone, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};

use crate::media::manifest::attachment_summaries_json;
use crate::models::{ChatMessage, ConversationSearchMatch};
use crate::text_util::{match_centered_excerpt, match_centered_snippet};

use super::db::DbHandle;
use super::persist::{
    is_session_search_tool_body, is_session_search_tool_name, SESSION_READ_INDEX_STUB,
    SESSION_SEARCH_INDEX_STUB,
};
use super::search::{
    build_fts_query, promote_primary_match, push_search_match, sort_matches_assistant_first,
};

const DEFAULT_WINDOW: i64 = 5;
const MAX_WINDOW: i64 = 20;
const DEFAULT_LIMIT: i64 = 3;
const MAX_LIMIT: i64 = 10;
const DEFAULT_READ_LIMIT: i64 = 40;
const MAX_READ_LIMIT: i64 = 80;
const TOOL_SNIPPET_RADIUS: usize = 48;
const MATCHES_RETURN_MAX: usize = 5;
const UNNAMED_TOOL_SKIP_BYTES: i64 = 4_096;
const HIT_CONTENT_CHARS_USER: usize = 4_000;
const HIT_CONTENT_CHARS_ASSISTANT: usize = 2_500;
const HIT_CONTENT_CHARS_TOOL: usize = 1_500;
const HIT_CONTENT_CHARS_OTHER: usize = 1_500;

fn sql_and_skip_session_search_dumps() -> String {
    format!(
        r#" AND COALESCE(m.tool_name, '') NOT IN ('session_search', 'session_read')
            AND m.content != '{search_stub}'
            AND m.content != '{read_stub}'
            AND NOT (
              lower(m.role) = 'tool'
              AND COALESCE(m.tool_name, '') = ''
              AND length(m.content) > {max_unnamed}
            )"#,
        search_stub = SESSION_SEARCH_INDEX_STUB.replace('\'', "''"),
        read_stub = SESSION_READ_INDEX_STUB.replace('\'', "''"),
        max_unnamed = UNNAMED_TOOL_SKIP_BYTES
    )
}

pub fn dispatch_tool(db: &DbHandle, args: &Value) -> Result<String> {
    dispatch_tool_inner(db, args)
}

pub fn dispatch_read_tool(db: &DbHandle, args: &Value) -> Result<String> {
    dispatch_read_inner(db, args)
}

fn dispatch_tool_inner(db: &DbHandle, args: &Value) -> Result<String> {
    let current_conversation_id = args
        .get("_conversation_id")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let session_user_filter = parse_session_user_filter(args);

    if args.get("around_message_id").is_some() || args.get("offset").is_some() {
        return Ok(error_json(
            "session_search does not take around_message_id or offset; use session_read",
        ));
    }

    let query = args
        .get("query")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let Some(query) = query else {
        return Ok(error_json(
            "session_search requires query; use session_read for a message window",
        ));
    };

    let conversation_id = parse_conversation_locator(args);
    let instance_id = parse_agent_instance_id(args);
    let conversation_id = match (conversation_id, instance_id, current_conversation_id) {
        (None, Some(_), Some(cur)) => Some(cur),
        (cid, _, _) => cid,
    };
    if conversation_id.is_some()
        && instance_id.is_none()
        && current_conversation_id == conversation_id
    {
        log::info!(
            "session_search: current conversation without agentInstanceId searches lead only conversation_id={}",
            conversation_id.unwrap_or("")
        );
    }

    let limit = parse_limit(args.get("limit"))?;
    let window = parse_window(args.get("window"))?;
    let role_filter = parse_role_filter(args.get("role_filter"));
    let tool_filter = parse_role_filter(args.get("tool_name"));
    discover(
        db,
        query,
        limit,
        window,
        role_filter.as_deref(),
        tool_filter.as_deref(),
        conversation_id,
        instance_id,
        current_conversation_id,
        session_user_filter,
    )
}

fn dispatch_read_inner(db: &DbHandle, args: &Value) -> Result<String> {
    let current_conversation_id = args
        .get("_conversation_id")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let session_user_filter = parse_session_user_filter(args);

    if args
        .get("query")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .is_some_and(|s| !s.is_empty())
    {
        return Ok(error_json(
            "session_read does not take query; use session_search",
        ));
    }

    let conversation_id = parse_conversation_locator(args);
    let instance_id = parse_agent_instance_id(args);
    if conversation_id.is_none() && instance_id.is_none() {
        return Ok(error_json(
            "session_read requires agentInstanceId and/or conversation_id",
        ));
    }
    if instance_id.is_none() && current_conversation_id == conversation_id {
        return Ok(error_json(
            "session_read of the current conversation requires agentInstanceId",
        ));
    }

    let around_message_id = args.get("around_message_id").and_then(|v| {
        v.as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .or_else(|| v.as_i64().map(|n| n.to_string()))
    });
    let offset_arg = args.get("offset");
    if around_message_id.is_some() && offset_arg.is_some() {
        return Ok(error_json(
            "session_read: pass offset or around_message_id, not both",
        ));
    }

    let limit = parse_read_limit(args.get("limit"))?;
    read_window(
        db,
        conversation_id,
        instance_id,
        around_message_id.as_deref(),
        offset_arg,
        limit,
        current_conversation_id,
        session_user_filter,
    )
}

fn parse_conversation_locator(args: &Value) -> Option<&str> {
    args.get("conversation_id")
        .or_else(|| args.get("session_id"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn parse_agent_instance_id(args: &Value) -> Option<&str> {
    args.get("agentInstanceId")
        .or_else(|| args.get("agent_instance_id"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn parse_session_user_filter(args: &Value) -> &str {
    args.get("_session_user_id")
        .and_then(|v| v.as_str())
        .map(super::session_user::normalize_session_user_id)
        .unwrap_or("")
}

fn discover(
    db: &DbHandle,
    query: &str,
    limit: i64,
    window: i64,
    role_filter: Option<&[String]>,
    tool_filter: Option<&[String]>,
    conversation_filter: Option<&str>,
    instance_filter: Option<&str>,
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
                    m.tool_name, bm25(messages_fts) AS rank,
                    m.agent_instance_id
             FROM messages_fts AS mf
             INNER JOIN messages AS m ON m.id = mf.rowid
             INNER JOIN conversations AS c ON c.id = mf.conversation_id
             WHERE messages_fts MATCH ?1
               AND c.session_user_id = ?2
               AND (?4 IS NULL OR m.conversation_id = ?4)
               AND (?5 IS NULL OR m.agent_instance_id = ?5)
               AND (
                 ?6 IS NULL
                 OR m.conversation_id != ?6
                 OR COALESCE(m.is_scoped, 0) = 0
               )
               {skip}
             ORDER BY rank
             LIMIT ?3",
            skip = sql_and_skip_session_search_dumps()
        );
        let mut stmt = conn.prepare(&sql)?;
        let cap = limit * 24;
        let skip_current = if instance_filter.is_none() {
            current_conversation_id
        } else {
            None
        };
        let mapped = stmt.query_map(
            params![
                fts_query,
                session_user_filter,
                cap,
                conversation_filter,
                instance_filter,
                skip_current
            ],
            |row| {
                Ok(FtsHit {
                    conversation_id: row.get(0)?,
                    message_id: row.get(1)?,
                    role: row.get(2)?,
                    content: String::new(),
                    tool_name: row.get(3)?,
                    snippet: String::new(),
                    rank: row.get(4)?,
                    agent_instance_id: row.get(5)?,
                })
            },
        )?;
        for hit in mapped {
            hits.push(hit?);
        }
    }

    if let Some(roles) = role_filter {
        hits.retain(|h| roles.iter().any(|r| r.eq_ignore_ascii_case(&h.role)));
    }
    if let Some(tools) = tool_filter {
        hits.retain(|h| {
            h.tool_name
                .as_deref()
                .is_some_and(|n| tools.iter().any(|t| t.eq_ignore_ascii_case(n)))
        });
    }

    struct DiscGroup {
        primary: FtsHit,
        matches: Vec<ConversationSearchMatch>,
        seen: std::collections::HashSet<String>,
        match_count: u32,
        instance_by_message: std::collections::HashMap<String, Option<String>>,
    }

    let mut groups: std::collections::HashMap<String, DiscGroup> = std::collections::HashMap::new();
    let mut order: Vec<String> = Vec::new();
    for hit in hits {
        if let Some(group) = groups.get_mut(&hit.conversation_id) {
            group
                .instance_by_message
                .insert(hit.message_id.clone(), hit.agent_instance_id.clone());
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
            instance_by_message: std::collections::HashMap::new(),
        };
        group
            .instance_by_message
            .insert(hit.message_id.clone(), hit.agent_instance_id.clone());
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

    order.sort_by_key(|id| std::cmp::Reverse(meta_updated_at(&conn, id).unwrap_or(0)));

    let mut results = Vec::new();
    for conversation_id in order {
        let Some(mut group) = groups.remove(&conversation_id) else {
            continue;
        };
        sort_matches_assistant_first(&mut group.matches);
        if let Some(first) = group.matches.first() {
            group.primary.message_id = first.message_id.clone();
            group.primary.role = first.role.clone();
            if let Some(inst) = group.instance_by_message.get(&first.message_id) {
                group.primary.agent_instance_id = inst.clone();
            }
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
        let lead_only = instance_filter.is_none()
            && current_conversation_id == Some(conversation_id.as_str());
        let window_view = load_window(
            &conn,
            &conversation_id,
            &group.primary.message_id,
            window,
            query,
            instance_filter,
            lead_only,
        )?;
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
            "agentInstanceId": group.primary.agent_instance_id,
            "title": nullable_str(&meta.title),
            "when": format_timestamp_ms(meta.updated_at_ms),
            "snippet": group.primary.snippet,
            "match_message_id": group.primary.message_id,
            "match_count": group.match_count,
            "matches": matches_json,
            "messages_before": window_view.messages_before,
            "messages_after": window_view.messages_after,
            "messages": window_view.messages,
        }));
    }

    log::info!(
        "session_search: discover query={query:?} groups={} include_current_lead={} elapsed_ms={}",
        results.len(),
        instance_filter.is_none(),
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

fn read_window(
    db: &DbHandle,
    conversation_id: Option<&str>,
    instance_id: Option<&str>,
    around_message_id: Option<&str>,
    offset_arg: Option<&Value>,
    limit: i64,
    current_conversation_id: Option<&str>,
    session_user_filter: &str,
) -> Result<String> {
    let conn = db.conn.lock();
    let resolved = match resolve_read_conversation(
        &conn,
        conversation_id,
        instance_id,
        current_conversation_id,
        session_user_filter,
    )? {
        Ok(id) => id,
        Err(msg) => return Ok(error_json(&msg)),
    };

    if load_meta_for_session_user(&conn, &resolved, Some(session_user_filter))?.is_none() {
        return Ok(error_json(&format!(
            "conversation_id not found: {resolved}"
        )));
    }

    let ids = load_filtered_message_ids(&conn, &resolved, instance_id, false)?;
    let message_count = ids.len() as i64;
    let offset = if let Some(anchor) = around_message_id {
        match ids.iter().position(|id| id == anchor) {
            Some(idx) => (idx as i64) + 1,
            None => {
                return Ok(error_json(&format!(
                    "around_message_id not found for locator: {anchor}"
                )));
            }
        }
    } else {
        parse_read_offset(offset_arg)?
    };
    let start = (offset - 1).max(0) as usize;
    let end = (start + limit as usize).min(ids.len());
    let slice = if start >= ids.len() {
        Vec::new()
    } else {
        ids[start..end].to_vec()
    };
    let returned = slice.len() as i64;
    let truncated = (start as i64) + returned < message_count;
    let messages = load_messages_by_ids(&conn, &resolved, &slice, "", None)?;

    log::info!(
        "session_read: conversation_id={resolved} agentInstanceId={:?} offset={offset} returned={returned} message_count={message_count}",
        instance_id
    );
    Ok(json!({
        "success": true,
        "mode": "read",
        "conversation_id": resolved,
        "agentInstanceId": instance_id,
        "message_count": message_count,
        "offset": offset,
        "returned": returned,
        "truncated": truncated,
        "messages": messages,
    })
    .to_string())
}

fn resolve_read_conversation(
    conn: &Connection,
    conversation_id: Option<&str>,
    instance_id: Option<&str>,
    current_conversation_id: Option<&str>,
    session_user_filter: &str,
) -> Result<std::result::Result<String, String>> {
    if let Some(cid) = conversation_id {
        return Ok(Ok(cid.to_string()));
    }
    let Some(instance_id) = instance_id else {
        return Ok(Err(
            "session_read requires agentInstanceId and/or conversation_id".into(),
        ));
    };
    if let Some(current) = current_conversation_id {
        return Ok(Ok(current.to_string()));
    }
    let mut stmt = conn.prepare(
        "SELECT DISTINCT m.conversation_id
         FROM messages AS m
         INNER JOIN conversations AS c ON c.id = m.conversation_id
         WHERE m.agent_instance_id = ?1
           AND c.session_user_id = ?2",
    )?;
    let ids: Vec<String> = stmt
        .query_map(params![instance_id, session_user_filter], |row| row.get(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    match ids.as_slice() {
        [one] => Ok(Ok(one.clone())),
        [] => Ok(Err(format!("agentInstanceId not found: {instance_id}"))),
        _ => Ok(Err(format!(
            "agentInstanceId is ambiguous across conversations: {instance_id}"
        ))),
    }
}

fn load_filtered_message_ids(
    conn: &Connection,
    conversation_id: &str,
    instance_id: Option<&str>,
    lead_only: bool,
) -> Result<Vec<String>> {
    let skip = sql_and_skip_session_search_dumps();
    let lead_sql = if lead_only {
        "AND COALESCE(m.is_scoped, 0) = 0"
    } else {
        ""
    };
    let sql = if instance_id.is_some() {
        format!(
            "SELECT m.message_id FROM messages AS m
             WHERE m.conversation_id = ?1
               AND m.agent_instance_id = ?2
               {skip}
             ORDER BY m.position ASC"
        )
    } else {
        format!(
            "SELECT m.message_id FROM messages AS m
             WHERE m.conversation_id = ?1
               {lead_sql}
               {skip}
             ORDER BY m.position ASC"
        )
    };
    let mut stmt = conn.prepare(&sql)?;
    let rows = if let Some(instance_id) = instance_id {
        stmt.query_map(params![conversation_id, instance_id], |row| row.get(0))?
            .collect::<rusqlite::Result<Vec<String>>>()?
    } else {
        stmt.query_map(params![conversation_id], |row| row.get(0))?
            .collect::<rusqlite::Result<Vec<String>>>()?
    };
    Ok(rows)
}

fn load_messages_by_ids(
    conn: &Connection,
    conversation_id: &str,
    ids: &[String],
    query: &str,
    anchor_message_id: Option<&str>,
) -> Result<Vec<Value>> {
    let mut out = Vec::new();
    for id in ids {
        let row: Option<(String, String, i64, String, Option<String>)> = conn
            .query_row(
                "SELECT role, content, created_at_ms, payload, tool_name
                 FROM messages
                 WHERE conversation_id = ?1 AND message_id = ?2",
                params![conversation_id, id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .optional()?;
        let Some((role, content, ts, payload, tool_name)) = row else {
            continue;
        };
        let is_anchor = anchor_message_id == Some(id.as_str());
        if let Some(entry) = session_search_message_json(
            id.clone(),
            role,
            content,
            ts,
            &payload,
            tool_name.as_deref(),
            is_anchor,
            query,
        ) {
            out.push(entry);
        }
    }
    Ok(out)
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
    agent_instance_id: Option<String>,
}

struct WindowView {
    messages: Vec<Value>,
    messages_before: i64,
    messages_after: i64,
}

struct MetaRow {
    title: String,
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
                "SELECT title, updated_at_ms FROM conversations
                 WHERE id = ?1 AND session_user_id = ?2",
                params![conversation_id, uid],
                |row| {
                    Ok(MetaRow {
                        title: row.get(0)?,
                        updated_at_ms: row.get(1)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into),
        None => conn
            .query_row(
                "SELECT title, updated_at_ms FROM conversations WHERE id = ?1",
                params![conversation_id],
                |row| {
                    Ok(MetaRow {
                        title: row.get(0)?,
                        updated_at_ms: row.get(1)?,
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

fn load_window(
    conn: &Connection,
    conversation_id: &str,
    anchor_message_id: &str,
    window: i64,
    query: &str,
    instance_id: Option<&str>,
    lead_only: bool,
) -> Result<WindowView> {
    let ids = load_filtered_message_ids(conn, conversation_id, instance_id, lead_only)?;
    let Some(anchor_idx) = ids.iter().position(|id| id == anchor_message_id) else {
        return Ok(WindowView {
            messages: vec![],
            messages_before: 0,
            messages_after: 0,
        });
    };
    let last = ids.len().saturating_sub(1) as i64;
    let start = (anchor_idx as i64 - window).max(0) as usize;
    let end = (anchor_idx as i64 + window).min(last).max(0) as usize;
    let slice = ids[start..=end].to_vec();
    let messages = load_messages_by_ids(
        conn,
        conversation_id,
        &slice,
        query,
        Some(anchor_message_id),
    )?;
    Ok(WindowView {
        messages,
        messages_before: (anchor_idx - start) as i64,
        messages_after: (end - anchor_idx) as i64,
    })
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
    if is_session_search_tool_name(tool_name)
        || content.trim() == SESSION_SEARCH_INDEX_STUB
        || content.trim() == SESSION_READ_INDEX_STUB
    {
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
    let mut agent_instance_id = None;
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
        agent_instance_id = msg.agent_instance_id.filter(|s| !s.trim().is_empty());
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
    if let Some(instance_id) = agent_instance_id {
        entry["agentInstanceId"] = json!(instance_id);
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

fn parse_read_limit(v: Option<&Value>) -> Result<i64> {
    let n = match v {
        Some(Value::Number(n)) => n.as_i64().unwrap_or(DEFAULT_READ_LIMIT),
        Some(Value::String(s)) => s.parse().unwrap_or(DEFAULT_READ_LIMIT),
        _ => DEFAULT_READ_LIMIT,
    };
    Ok(n.clamp(1, MAX_READ_LIMIT))
}

fn parse_read_offset(v: Option<&Value>) -> Result<i64> {
    let n = match v {
        Some(Value::Number(n)) => n.as_i64().unwrap_or(1),
        Some(Value::String(s)) => s.parse().unwrap_or(1),
        _ => 1,
    };
    Ok(n.max(1))
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
