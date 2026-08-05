//! Turn-based message windows for UI hydration (not LLM history).
//!
//! A "turn" starts at a real user task message (non-scoped) and runs until the
//! next such user message. Pagination returns complete turns so tool chains and
//! sub-agent rows stay intact.

use anyhow::{anyhow, Result};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::models::{is_scoped_sub_message, ChatMessage};
use crate::task_board::history_trim::is_real_user_task_message;

use super::persist;

/// Default user-turn window for first paint / load-more / around.
pub const DEFAULT_MESSAGE_PAGE_TURNS: u32 = 8;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessagePage {
    pub messages: Vec<ChatMessage>,
    pub has_more_older: bool,
    pub has_more_newer: bool,
    pub oldest_position: Option<i64>,
    pub newest_position: Option<i64>,
    pub message_count: u32,
}

#[derive(Debug, Clone, Default)]
pub struct LoadMessagesPageOpts {
    /// When set (and no around/before-only edge cases), limit to this many user turns.
    /// `None` or `0` means return the full transcript (legacy full hydrate).
    pub limit_turns: Option<u32>,
    /// Load complete turns strictly before this SQLite `position` (exclusive).
    pub before_position: Option<i64>,
    /// Center the window on the turn that contains this message id.
    pub around_message_id: Option<String>,
}

fn is_user_anchor(message: &ChatMessage) -> bool {
    is_real_user_task_message(message) && !is_scoped_sub_message(message)
}

/// Indices where a UI user turn starts (aligned with frontend `splitMessageTurnSegments`).
fn user_anchor_indices(rows: &[(i64, ChatMessage)]) -> Vec<usize> {
    rows.iter()
        .enumerate()
        .filter(|(_, (_, msg))| is_user_anchor(msg))
        .map(|(idx, _)| idx)
        .collect()
}

fn turn_end_exclusive(anchors: &[usize], turn_idx: usize, row_len: usize) -> usize {
    anchors
        .get(turn_idx + 1)
        .copied()
        .unwrap_or(row_len)
}

fn slice_page(
    rows: &[(i64, ChatMessage)],
    start: usize,
    end: usize,
    has_more_older: bool,
    has_more_newer: bool,
) -> MessagePage {
    let end = end.min(rows.len());
    let start = start.min(end);
    let window = &rows[start..end];
    MessagePage {
        messages: window.iter().map(|(_, m)| m.clone()).collect(),
        has_more_older,
        has_more_newer,
        oldest_position: window.first().map(|(p, _)| *p),
        newest_position: window.last().map(|(p, _)| *p),
        message_count: rows.len() as u32,
    }
}

/// Pure paging over `(position, message)` rows ordered by position ascending.
pub fn page_from_positioned(
    rows: &[(i64, ChatMessage)],
    opts: &LoadMessagesPageOpts,
) -> Result<MessagePage> {
    let _message_count = rows.len() as u32;
    if rows.is_empty() {
        return Ok(MessagePage {
            messages: Vec::new(),
            has_more_older: false,
            has_more_newer: false,
            oldest_position: None,
            newest_position: None,
            message_count: 0,
        });
    }

    let limit = opts.limit_turns.unwrap_or(0);
    let around = opts
        .around_message_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());

    if let Some(message_id) = around {
        return page_around(rows, message_id, if limit == 0 { DEFAULT_MESSAGE_PAGE_TURNS } else { limit });
    }

    if let Some(before) = opts.before_position {
        let turns = if limit == 0 { DEFAULT_MESSAGE_PAGE_TURNS } else { limit };
        return Ok(page_before(rows, before, turns));
    }

    if limit == 0 {
        return Ok(slice_page(rows, 0, rows.len(), false, false));
    }

    Ok(page_tail(rows, limit))
}

fn page_tail(rows: &[(i64, ChatMessage)], limit_turns: u32) -> MessagePage {
    let anchors = user_anchor_indices(rows);
    if anchors.is_empty() {
        // No user turns — return everything (prelude / notices only).
        return slice_page(rows, 0, rows.len(), false, false);
    }

    let n = limit_turns.max(1) as usize;
    let first_turn = anchors.len().saturating_sub(n);
    let start = anchors[first_turn];
    // When the window includes the first user turn, also keep any prelude rows.
    let start = if first_turn == 0 { 0 } else { start };
    let has_more_older = first_turn > 0;
    slice_page(rows, start, rows.len(), has_more_older, false)
}

fn page_before(rows: &[(i64, ChatMessage)], before_position: i64, limit_turns: u32) -> MessagePage {
    let end = rows
        .iter()
        .position(|(pos, _)| *pos >= before_position)
        .unwrap_or(rows.len());
    if end == 0 {
        return slice_page(rows, 0, 0, false, true);
    }

    let older = &rows[..end];
    let anchors = user_anchor_indices(older);
    if anchors.is_empty() {
        // Only prelude-like rows before the cursor — return them once.
        return slice_page(rows, 0, end, false, true);
    }

    let n = limit_turns.max(1) as usize;
    let first_turn = anchors.len().saturating_sub(n);
    let start = if first_turn == 0 { 0 } else { anchors[first_turn] };
    let has_more_older = first_turn > 0;
    slice_page(rows, start, end, has_more_older, true)
}

fn page_around(
    rows: &[(i64, ChatMessage)],
    message_id: &str,
    limit_turns: u32,
) -> Result<MessagePage> {
    let anchor_row = rows
        .iter()
        .position(|(_, msg)| msg.id == message_id)
        .ok_or_else(|| anyhow!("around_message_id not found: {message_id}"))?;

    let anchors = user_anchor_indices(rows);
    if anchors.is_empty() {
        return Ok(slice_page(rows, 0, rows.len(), false, false));
    }

    let turn_idx = match anchors.iter().rposition(|&start| start <= anchor_row) {
        Some(idx) => idx,
        None => 0, // message sits in prelude before first user turn
    };

    let n = limit_turns.max(1) as usize;
    let before = n / 2;
    let after = n.saturating_sub(before).saturating_sub(1); // remaining after the hit turn
    let first_turn = turn_idx.saturating_sub(before);
    let last_turn = (turn_idx + after).min(anchors.len().saturating_sub(1));

    let start = if first_turn == 0 { 0 } else { anchors[first_turn] };
    let end = turn_end_exclusive(&anchors, last_turn, rows.len());
    let has_more_older = first_turn > 0;
    let has_more_newer = last_turn + 1 < anchors.len() || end < rows.len();

    Ok(slice_page(rows, start, end, has_more_older, has_more_newer))
}

pub fn load_messages_page(
    conn: &Connection,
    conversation_id: &str,
    opts: &LoadMessagesPageOpts,
) -> Result<MessagePage> {
    let rows = persist::load_messages_with_positions(conn, conversation_id)?;
    let page = page_from_positioned(&rows, opts)?;
    log::info!(
        "conversation_store: message page conversation_id={conversation_id} returned={} total={} has_more_older={} has_more_newer={} limit_turns={:?} before={:?} around={:?}",
        page.messages.len(),
        page.message_count,
        page.has_more_older,
        page.has_more_newer,
        opts.limit_turns,
        opts.before_position,
        opts.around_message_id.as_deref()
    );
    Ok(page)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Role, ChatMessage};

    fn msg(id: &str, role: Role, content: &str) -> ChatMessage {
        let mut m = ChatMessage::user_text(content);
        m.id = id.into();
        m.role = role;
        if !matches!(m.role, Role::User) {
            m.content = content.into();
        }
        m
    }

    fn rows(items: Vec<(&str, Role, &str)>) -> Vec<(i64, ChatMessage)> {
        items
            .into_iter()
            .enumerate()
            .map(|(i, (id, role, content))| (i as i64, msg(id, role, content)))
            .collect()
    }

    #[test]
    fn empty_transcript() {
        let page = page_from_positioned(&[], &LoadMessagesPageOpts::default()).unwrap();
        assert!(page.messages.is_empty());
        assert_eq!(page.message_count, 0);
    }

    #[test]
    fn full_load_when_limit_zero() {
        let data = rows(vec![
            ("u1", Role::User, "one"),
            ("a1", Role::Assistant, "ok"),
            ("u2", Role::User, "two"),
            ("a2", Role::Assistant, "ok"),
        ]);
        let page = page_from_positioned(
            &data,
            &LoadMessagesPageOpts {
                limit_turns: Some(0),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(page.messages.len(), 4);
        assert!(!page.has_more_older);
    }

    #[test]
    fn tail_keeps_last_n_turns_and_prelude_when_included() {
        let data = rows(vec![
            ("sys", Role::System, "hi"),
            ("u1", Role::User, "one"),
            ("a1", Role::Assistant, "ok"),
            ("u2", Role::User, "two"),
            ("a2", Role::Assistant, "ok"),
            ("u3", Role::User, "three"),
            ("a3", Role::Assistant, "ok"),
        ]);
        let page = page_from_positioned(
            &data,
            &LoadMessagesPageOpts {
                limit_turns: Some(2),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            page.messages.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            vec!["u2", "a2", "u3", "a3"]
        );
        assert!(page.has_more_older);
        assert!(!page.has_more_newer);
        assert_eq!(page.oldest_position, Some(3));
        assert_eq!(page.newest_position, Some(6));

        let small = page_from_positioned(
            &data,
            &LoadMessagesPageOpts {
                limit_turns: Some(8),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(small.messages.len(), 7); // includes system prelude
        assert!(!small.has_more_older);
        assert_eq!(small.messages[0].id, "sys");
    }

    #[test]
    fn before_loads_older_complete_turns() {
        let data = rows(vec![
            ("u1", Role::User, "one"),
            ("a1", Role::Assistant, "ok"),
            ("u2", Role::User, "two"),
            ("a2", Role::Assistant, "ok"),
            ("u3", Role::User, "three"),
            ("a3", Role::Assistant, "ok"),
        ]);
        let page = page_from_positioned(
            &data,
            &LoadMessagesPageOpts {
                limit_turns: Some(1),
                before_position: Some(4), // u3's position
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            page.messages.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            vec!["u2", "a2"]
        );
        assert!(page.has_more_older);
        assert!(page.has_more_newer);
    }

    #[test]
    fn around_centers_on_hit_turn() {
        let data = rows(vec![
            ("u1", Role::User, "one"),
            ("a1", Role::Assistant, "ok"),
            ("u2", Role::User, "two"),
            ("a2", Role::Assistant, "hit"),
            ("u3", Role::User, "three"),
            ("a3", Role::Assistant, "ok"),
            ("u4", Role::User, "four"),
            ("a4", Role::Assistant, "ok"),
        ]);
        let page = page_from_positioned(
            &data,
            &LoadMessagesPageOpts {
                limit_turns: Some(3),
                around_message_id: Some("a2".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let ids: Vec<_> = page.messages.iter().map(|m| m.id.as_str()).collect();
        assert!(ids.contains(&"u2"));
        assert!(ids.contains(&"a2"));
        assert!(ids.contains(&"u1") || ids.contains(&"u3"));
        assert!(page.messages.iter().any(|m| m.id == "a2"));
        assert!(page.has_more_older || page.has_more_newer);
    }

    #[test]
    fn around_missing_id_errors() {
        let data = rows(vec![("u1", Role::User, "one")]);
        let err = page_from_positioned(
            &data,
            &LoadMessagesPageOpts {
                limit_turns: Some(2),
                around_message_id: Some("missing".into()),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("around_message_id not found"));
    }

    #[test]
    fn skips_synthetic_user_as_turn_anchor() {
        let mut data = rows(vec![
            ("u1", Role::User, "real"),
            ("a1", Role::Assistant, "ok"),
        ]);
        let mut inject = ChatMessage::user_text("[CUR_SCREEN] shot");
        inject.id = "inj".into();
        data.insert(2, (2, inject));
        for (i, row) in data.iter_mut().enumerate() {
            row.0 = i as i64;
        }
        data.push((3, msg("u2", Role::User, "two")));
        data.push((4, msg("a2", Role::Assistant, "ok")));

        let page = page_from_positioned(
            &data,
            &LoadMessagesPageOpts {
                limit_turns: Some(1),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(page.messages.first().map(|m| m.id.as_str()), Some("u2"));
        assert!(page.has_more_older);
    }
}
