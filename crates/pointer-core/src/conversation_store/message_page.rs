//! Turn-based message windows for UI hydration (not LLM history).
//!
//! A "turn" starts at a real user task message (non-scoped) and runs until the
//! next such user message. Pagination returns complete turns so tool chains and
//! sub-agent rows stay intact.

use std::collections::HashMap;

use anyhow::{anyhow, Result};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::models::{is_scoped_sub_message, ChatMessage};

use super::persist::{self, AnchorProbeRow};

/// Default user-turn window for first paint / load-more / around.
pub const DEFAULT_MESSAGE_PAGE_TURNS: u32 = 8;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessagePage {
    pub messages: Vec<ChatMessage>,
    /// SQLite `position` of each message in `messages` (parallel array).
    /// Wire-only: lets the frontend advance its paging cursor when trimming
    /// old in-memory history, without persisting position into payloads.
    #[serde(default)]
    pub positions: Vec<i64>,
    pub has_more_older: bool,
    pub has_more_newer: bool,
    pub oldest_position: Option<i64>,
    pub newest_position: Option<i64>,
    pub message_count: u32,
    /// Scoped rows keyed by SpawnId (`agentInstanceId`, else `trace:{traceId}`).
    /// Only filled when `include_scoped_sub_messages` is true; default hydrate is empty.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub scoped: HashMap<String, Vec<ChatMessage>>,
}

#[derive(Debug, Clone)]
pub struct LoadMessagesPageOpts {
    /// When set (and no around/before-only edge cases), limit to this many user turns.
    /// `None` or `0` means return the full transcript (legacy full hydrate).
    pub limit_turns: Option<u32>,
    /// Load complete turns strictly before this SQLite `position` (exclusive).
    pub before_position: Option<i64>,
    /// Load complete turns strictly after this SQLite `position` (exclusive).
    /// Used to page toward the tail after an around-window search jump.
    pub after_position: Option<i64>,
    /// Center the window on the turn that contains this message id.
    pub around_message_id: Option<String>,
    /// When `false` (UI default), omit scoped sub-agent rows (`context_included = 0`).
    pub include_scoped_sub_messages: bool,
}

impl Default for LoadMessagesPageOpts {
    fn default() -> Self {
        Self {
            limit_turns: None,
            before_position: None,
            after_position: None,
            around_message_id: None,
            include_scoped_sub_messages: false,
        }
    }
}

fn scoped_page_bucket_key(msg: &ChatMessage) -> String {
    if let Some(instance) = msg
        .agent_instance_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return instance.to_string();
    }
    if let Some(trace) = msg
        .trace_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return format!("trace:{trace}");
    }
    format!(
        "anchor:{}",
        msg.anchor_message_id.as_deref().unwrap_or_default()
    )
}

fn split_page_scoped(
    messages: Vec<ChatMessage>,
    positions: Vec<i64>,
) -> (Vec<ChatMessage>, Vec<i64>, HashMap<String, Vec<ChatMessage>>) {
    let mut lead = Vec::new();
    let mut lead_pos = Vec::new();
    let mut scoped: HashMap<String, Vec<ChatMessage>> = HashMap::new();
    for (msg, pos) in messages.into_iter().zip(positions) {
        if is_scoped_sub_message(&msg) {
            scoped.entry(scoped_page_bucket_key(&msg)).or_default().push(msg);
        } else {
            lead.push(msg);
            lead_pos.push(pos);
        }
    }
    (lead, lead_pos, scoped)
}

/// Window boundaries over an anchor probe (exclusive position range to load).
/// `probe` only contains real user-turn anchors, so assistant/tool rows between
/// anchors are captured by loading `[start_position, end_position)`.
struct ProbeWindow {
    start_position: i64,
    end_position: i64, // exclusive; i64::MAX = through the transcript tail
    has_more_older: bool,
    has_more_newer: bool,
}

fn probe_window_tail(probe: &[AnchorProbeRow], limit_turns: u32) -> ProbeWindow {
    let n = limit_turns.max(1) as usize;
    if probe.is_empty() {
        // No user turns — return everything (prelude / notices only).
        return ProbeWindow {
            start_position: 0,
            end_position: i64::MAX,
            has_more_older: false,
            has_more_newer: false,
        };
    }
    let first_turn = probe.len().saturating_sub(n);
    let start_position = if first_turn == 0 {
        0
    } else {
        probe[first_turn].position
    };
    let has_more_older = first_turn > 0;
    ProbeWindow {
        start_position,
        end_position: i64::MAX,
        has_more_older,
        has_more_newer: false,
    }
}

/// `end_position` is the first row (any role) at/after `before_position`,
/// resolved by SQL so assistant/tool rows after the cursor are excluded too.
fn probe_window_before(
    probe: &[AnchorProbeRow],
    end_position: i64,
    limit_turns: u32,
) -> ProbeWindow {
    if end_position <= 0 {
        // Cursor sits before the very first row — nothing older to load.
        return ProbeWindow {
            start_position: 0,
            end_position: 0,
            has_more_older: false,
            has_more_newer: true,
        };
    }
    let end_idx = probe
        .iter()
        .position(|row| row.position >= end_position)
        .unwrap_or(probe.len());
    if end_idx == 0 {
        // Only prelude-like rows before the cursor — return them once.
        return ProbeWindow {
            start_position: 0,
            end_position,
            has_more_older: false,
            has_more_newer: true,
        };
    }
    let n = limit_turns.max(1) as usize;
    let first_turn = end_idx.saturating_sub(n);
    let start_position = if first_turn == 0 {
        0
    } else {
        probe[first_turn].position
    };
    let has_more_older = first_turn > 0;
    ProbeWindow {
        start_position,
        end_position,
        has_more_older,
        has_more_newer: true,
    }
}

/// `start_position` is the first row (any role) strictly after `after_position`,
/// resolved by SQL so leftover assistant/tool rows after the cursor are included.
fn probe_window_after(
    probe: &[AnchorProbeRow],
    start_position: i64,
    limit_turns: u32,
) -> ProbeWindow {
    let n = limit_turns.max(1) as usize;
    let start_idx = probe
        .iter()
        .position(|row| row.position >= start_position)
        .unwrap_or(probe.len());
    if start_idx >= probe.len() {
        return ProbeWindow {
            start_position,
            end_position: i64::MAX,
            has_more_older: start_position > 0,
            has_more_newer: false,
        };
    }
    let last_turn = (start_idx + n - 1).min(probe.len().saturating_sub(1));
    let end_position = probe
        .get(last_turn + 1)
        .map(|row| row.position)
        .unwrap_or(i64::MAX);
    ProbeWindow {
        start_position,
        end_position,
        has_more_older: start_position > 0,
        has_more_newer: last_turn + 1 < probe.len() || end_position < i64::MAX,
    }
}

/// `target_position` is the position of the around target message (any role),
/// resolved by SQL (the anchor probe only knows user rows).
fn probe_window_around(
    probe: &[AnchorProbeRow],
    target_position: i64,
    limit_turns: u32,
) -> ProbeWindow {
    if probe.is_empty() {
        return ProbeWindow {
            start_position: 0,
            end_position: i64::MAX,
            has_more_older: false,
            has_more_newer: false,
        };
    }
    let turn_idx = probe
        .iter()
        .rposition(|row| row.position <= target_position)
        .unwrap_or(0); // 0: message sits in prelude before first user turn

    let n = limit_turns.max(1) as usize;
    let before = n / 2;
    let after = n.saturating_sub(before).saturating_sub(1); // remaining after the hit turn
    let first_turn = turn_idx.saturating_sub(before);
    let last_turn = (turn_idx + after).min(probe.len().saturating_sub(1));

    let start_position = if first_turn == 0 {
        0
    } else {
        probe[first_turn].position
    };
    let end_position = probe
        .get(last_turn + 1)
        .map(|row| row.position)
        .unwrap_or(i64::MAX);
    let has_more_older = first_turn > 0;
    let has_more_newer = last_turn + 1 < probe.len() || end_position < i64::MAX;

    ProbeWindow {
        start_position,
        end_position,
        has_more_older,
        has_more_newer,
    }
}

/// Window computation over a scalar anchor probe — no full payloads needed.
/// Mirrors `page_from_positioned` (which is kept for tests / callers that already
/// have full rows in memory).
///
/// `before_end_position`: SQL-resolved first row (any role) at/after
/// `before_position`, used as the exclusive end of a `before` window.
/// `after_start_position`: SQL-resolved first row (any role) strictly after
/// `after_position`, used as the inclusive start of an `after` window.
/// `around_target_position`: SQL-resolved position of the around target message
/// (any role), used to locate its containing turn.
fn probe_window_from_rows(
    probe: &[AnchorProbeRow],
    opts: &LoadMessagesPageOpts,
    before_end_position: Option<i64>,
    after_start_position: Option<i64>,
    around_target_position: Option<i64>,
) -> Result<ProbeWindow> {
    let limit = opts.limit_turns.unwrap_or(0);
    let around = opts
        .around_message_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());

    if let Some(message_id) = around {
        let turns = if limit == 0 {
            DEFAULT_MESSAGE_PAGE_TURNS
        } else {
            limit
        };
        let target_position = around_target_position
            .ok_or_else(|| anyhow!("around_message_id not found: {message_id}"))?;
        return Ok(probe_window_around(probe, target_position, turns));
    }
    if let Some(_before) = opts.before_position {
        let turns = if limit == 0 {
            DEFAULT_MESSAGE_PAGE_TURNS
        } else {
            limit
        };
        let end_position = before_end_position.unwrap_or(i64::MAX);
        return Ok(probe_window_before(probe, end_position, turns));
    }
    if opts.after_position.is_some() {
        let turns = if limit == 0 {
            DEFAULT_MESSAGE_PAGE_TURNS
        } else {
            limit
        };
        let Some(start_position) = after_start_position else {
            // Cursor is at or past the last row — empty page, keep the exclusive
            // bound past `after` so oldest/newest stay None (not position 0).
            let after = opts.after_position.unwrap_or(0);
            let start = after.saturating_add(1);
            return Ok(ProbeWindow {
                start_position: start,
                end_position: start,
                has_more_older: after >= 0,
                has_more_newer: false,
            });
        };
        return Ok(probe_window_after(probe, start_position, turns));
    }
    if limit == 0 {
        return Ok(ProbeWindow {
            start_position: 0,
            end_position: i64::MAX,
            has_more_older: false,
            has_more_newer: false,
        });
    }
    Ok(probe_window_tail(probe, limit))
}

pub fn load_messages_page(
    conn: &Connection,
    conversation_id: &str,
    opts: &LoadMessagesPageOpts,
) -> Result<MessagePage> {
    // Two-phase paging: first a scalar anchor probe (only real user-turn rows,
    // served by the partial index — no content/payload deserialization), then
    // load only the window rows inside the computed position range.
    let probe = persist::load_anchor_probe(conn, conversation_id)?;
    let before_end_position = match opts.before_position {
        Some(before) => persist::first_position_at_or_after(conn, conversation_id, before)?,
        None => None,
    };
    let around_target_position = match opts
        .around_message_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(message_id) => persist::message_position(conn, conversation_id, message_id)?,
        None => None,
    };
    let after_start_position = match opts.after_position {
        Some(after) => persist::first_position_after(conn, conversation_id, after)?,
        None => None,
    };
    let window = probe_window_from_rows(
        &probe,
        opts,
        before_end_position,
        after_start_position,
        around_target_position,
    )?;

    let (messages, positions, loaded_first, loaded_last, scoped) =
        if window.start_position >= window.end_position {
            (Vec::new(), Vec::new(), None, None, HashMap::new())
        } else {
            let rows = persist::load_messages_in_position_range(
                conn,
                conversation_id,
                window.start_position,
                window.end_position,
                opts.include_scoped_sub_messages,
            )?;
            let mut msgs = Vec::with_capacity(rows.len());
            let mut poss = Vec::with_capacity(rows.len());
            for (pos, msg) in rows {
                msgs.push(msg);
                poss.push(pos);
            }
            if opts.include_scoped_sub_messages {
                let (lead, lead_pos, scoped) = split_page_scoped(msgs, poss);
                let first = lead_pos.first().copied();
                let last = lead_pos.last().copied();
                (lead, lead_pos, first, last, scoped)
            } else {
                let first = poss.first().copied();
                let last = poss.last().copied();
                (msgs, poss, first, last, HashMap::new())
            }
        };

    // Total transcript size is a separate scalar (anchors alone don't count
    // assistant/tool rows; frontend uses it for hydration checks).
    let message_count: u32 = conn.query_row(
        "SELECT COUNT(*) FROM messages WHERE conversation_id = ?1",
        params![conversation_id],
        |row| row.get(0),
    )?;

    let page = MessagePage {
        messages,
        positions,
        has_more_older: window.has_more_older,
        has_more_newer: window.has_more_newer,
        oldest_position: loaded_first,
        newest_position: loaded_last,
        message_count,
        scoped,
    };
    log::info!(
        "conversation_store: message page conversation_id={conversation_id} returned={} total={} has_more_older={} has_more_newer={} limit_turns={:?} before={:?} after={:?} around={:?}",
        page.messages.len(),
        page.message_count,
        page.has_more_older,
        page.has_more_newer,
        opts.limit_turns,
        opts.before_position,
        opts.after_position,
        opts.around_message_id.as_deref()
    );
    Ok(page)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{is_scoped_sub_message, ChatMessage, Role};
    use crate::task_board::history_trim::is_real_user_task_message;

    // ── Reference implementation (full-rows in memory) ──
    // Kept as an oracle so probe-based paging can be verified against it.
    fn is_user_anchor(message: &ChatMessage) -> bool {
        is_real_user_task_message(message) && !is_scoped_sub_message(message)
    }

    fn user_anchor_indices(rows: &[(i64, ChatMessage)]) -> Vec<usize> {
        rows.iter()
            .enumerate()
            .filter(|(_, (_, msg))| is_user_anchor(msg))
            .map(|(idx, _)| idx)
            .collect()
    }

    fn turn_end_exclusive(anchors: &[usize], turn_idx: usize, row_len: usize) -> usize {
        anchors.get(turn_idx + 1).copied().unwrap_or(row_len)
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
            positions: window.iter().map(|(p, _)| *p).collect(),
            has_more_older,
            has_more_newer,
            oldest_position: window.first().map(|(p, _)| *p),
            newest_position: window.last().map(|(p, _)| *p),
            message_count: rows.len() as u32,
            scoped: HashMap::new(),
        }
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

    fn page_before(
        rows: &[(i64, ChatMessage)],
        before_position: i64,
        limit_turns: u32,
    ) -> MessagePage {
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
        let start = if first_turn == 0 {
            0
        } else {
            anchors[first_turn]
        };
        let has_more_older = first_turn > 0;
        slice_page(rows, start, end, has_more_older, true)
    }

    fn page_after(
        rows: &[(i64, ChatMessage)],
        after_position: i64,
        limit_turns: u32,
    ) -> MessagePage {
        let start = rows
            .iter()
            .position(|(pos, _)| *pos > after_position)
            .unwrap_or(rows.len());
        if start >= rows.len() {
            return slice_page(rows, rows.len(), rows.len(), start > 0, false);
        }

        let anchors = user_anchor_indices(rows);
        let start_idx = anchors
            .iter()
            .position(|&i| i >= start)
            .unwrap_or(anchors.len());
        if start_idx >= anchors.len() {
            return slice_page(rows, start, rows.len(), start > 0, false);
        }

        let n = limit_turns.max(1) as usize;
        let last_turn = (start_idx + n - 1).min(anchors.len().saturating_sub(1));
        let end = turn_end_exclusive(&anchors, last_turn, rows.len());
        let has_more_newer = last_turn + 1 < anchors.len() || end < rows.len();
        slice_page(rows, start, end, start > 0, has_more_newer)
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

        let start = if first_turn == 0 {
            0
        } else {
            anchors[first_turn]
        };
        let end = turn_end_exclusive(&anchors, last_turn, rows.len());
        let has_more_older = first_turn > 0;
        let has_more_newer = last_turn + 1 < anchors.len() || end < rows.len();

        Ok(slice_page(rows, start, end, has_more_older, has_more_newer))
    }

    /// Pure paging over `(position, message)` rows ordered by position ascending.
    fn page_from_positioned(
        rows: &[(i64, ChatMessage)],
        opts: &LoadMessagesPageOpts,
    ) -> Result<MessagePage> {
        if rows.is_empty() {
            return Ok(MessagePage {
                messages: Vec::new(),
                positions: Vec::new(),
                has_more_older: false,
                has_more_newer: false,
                oldest_position: None,
                newest_position: None,
                message_count: 0,
                scoped: HashMap::new(),
            });
        }

        let limit = opts.limit_turns.unwrap_or(0);
        let around = opts
            .around_message_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());

        if let Some(message_id) = around {
            return page_around(
                rows,
                message_id,
                if limit == 0 {
                    DEFAULT_MESSAGE_PAGE_TURNS
                } else {
                    limit
                },
            );
        }

        if let Some(before) = opts.before_position {
            let turns = if limit == 0 {
                DEFAULT_MESSAGE_PAGE_TURNS
            } else {
                limit
            };
            return Ok(page_before(rows, before, turns));
        }

        if let Some(after) = opts.after_position {
            let turns = if limit == 0 {
                DEFAULT_MESSAGE_PAGE_TURNS
            } else {
                limit
            };
            return Ok(page_after(rows, after, turns));
        }

        if limit == 0 {
            return Ok(slice_page(rows, 0, rows.len(), false, false));
        }

        Ok(page_tail(rows, limit))
    }

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
            page.messages
                .iter()
                .map(|m| m.id.as_str())
                .collect::<Vec<_>>(),
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
            page.messages
                .iter()
                .map(|m| m.id.as_str())
                .collect::<Vec<_>>(),
            vec!["u2", "a2"]
        );
        assert!(page.has_more_older);
        assert!(page.has_more_newer);
    }

    #[test]
    fn after_loads_newer_complete_turns() {
        let data = rows(vec![
            ("u1", Role::User, "one"),
            ("a1", Role::Assistant, "ok"),
            ("u2", Role::User, "two"),
            ("a2", Role::Assistant, "ok"),
            ("u3", Role::User, "three"),
            ("a3", Role::Assistant, "ok"),
            ("u4", Role::User, "four"),
            ("a4", Role::Assistant, "ok"),
        ]);
        // around u2/a2 ends at position 3 (a2). Newer page should start at u3.
        let page = page_from_positioned(
            &data,
            &LoadMessagesPageOpts {
                limit_turns: Some(1),
                after_position: Some(3),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            page.messages
                .iter()
                .map(|m| m.id.as_str())
                .collect::<Vec<_>>(),
            vec!["u3", "a3"]
        );
        assert!(page.has_more_older);
        assert!(page.has_more_newer);

        let rest = page_from_positioned(
            &data,
            &LoadMessagesPageOpts {
                limit_turns: Some(8),
                after_position: Some(5),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            rest.messages
                .iter()
                .map(|m| m.id.as_str())
                .collect::<Vec<_>>(),
            vec!["u4", "a4"]
        );
        assert!(rest.has_more_older);
        assert!(!rest.has_more_newer);
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

    fn probe_from_rows(rows: &[(i64, ChatMessage)]) -> Vec<AnchorProbeRow> {
        rows.iter()
            .filter(|(_, msg)| is_user_anchor(msg))
            .map(|(pos, _)| AnchorProbeRow { position: *pos })
            .collect()
    }

    fn before_end_position_for(rows: &[(i64, ChatMessage)], before: i64) -> Option<i64> {
        rows.iter()
            .find(|(pos, _)| *pos >= before)
            .map(|(pos, _)| *pos)
    }

    fn around_target_position_for(rows: &[(i64, ChatMessage)], message_id: &str) -> Option<i64> {
        rows.iter()
            .find(|(_, m)| m.id == message_id)
            .map(|(pos, _)| *pos)
    }

    /// Probe window computation must match the full-rows reference exactly.
    fn assert_window_matches(rows: &[(i64, ChatMessage)], opts: &LoadMessagesPageOpts) {
        let reference = page_from_positioned(rows, opts).unwrap();
        let probe = probe_from_rows(rows);
        let before_end = opts
            .before_position
            .and_then(|b| before_end_position_for(rows, b));
        let after_start = opts.after_position.and_then(|after| {
            rows.iter()
                .find(|(pos, _)| *pos > after)
                .map(|(pos, _)| *pos)
        });
        let around_target = opts
            .around_message_id
            .as_deref()
            .and_then(|id| around_target_position_for(rows, id));
        let window =
            probe_window_from_rows(&probe, opts, before_end, after_start, around_target).unwrap();
        let ids: Vec<&str> = rows
            .iter()
            .filter(|(pos, _)| *pos >= window.start_position && *pos < window.end_position)
            .map(|(_, m)| m.id.as_str())
            .collect();
        let reference_ids: Vec<&str> = reference.messages.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(
            ids, reference_ids,
            "window mismatch for opts={opts:?} (full={:?})",
            reference_ids
        );
        assert_eq!(
            window.has_more_older, reference.has_more_older,
            "has_more_older opts={opts:?}"
        );
        assert_eq!(
            window.has_more_newer, reference.has_more_newer,
            "has_more_newer opts={opts:?}"
        );
        assert_eq!(
            rows.iter()
                .find(|(p, _)| *p >= window.start_position)
                .map(|(p, _)| *p),
            reference.oldest_position,
            "oldest_position opts={opts:?}"
        );
        assert_eq!(
            rows.iter()
                .filter(|(pos, _)| { *pos >= window.start_position && *pos < window.end_position })
                .last()
                .map(|(p, _)| *p),
            reference.newest_position,
            "newest_position opts={opts:?}"
        );
    }

    #[test]
    fn probe_window_matches_reference() {
        let data = rows(vec![
            ("sys", Role::System, "prelude"),
            ("u1", Role::User, "one"),
            ("a1", Role::Assistant, "ok"),
            ("u2", Role::User, "two"),
            ("a2", Role::Assistant, "ok"),
            ("u3", Role::User, "three"),
            ("a3", Role::Assistant, "ok"),
            ("u4", Role::User, "four"),
            ("a4", Role::Assistant, "ok"),
            ("u5", Role::User, "five"),
            ("a5", Role::Assistant, "ok"),
        ]);
        let cases = [
            LoadMessagesPageOpts::default(),
            LoadMessagesPageOpts {
                limit_turns: Some(0),
                ..Default::default()
            },
            LoadMessagesPageOpts {
                limit_turns: Some(2),
                ..Default::default()
            },
            LoadMessagesPageOpts {
                limit_turns: Some(8),
                ..Default::default()
            },
            LoadMessagesPageOpts {
                limit_turns: Some(2),
                before_position: Some(4),
                ..Default::default()
            },
            LoadMessagesPageOpts {
                limit_turns: Some(1),
                before_position: Some(2),
                ..Default::default()
            },
            LoadMessagesPageOpts {
                limit_turns: Some(3),
                around_message_id: Some("a2".into()),
                ..Default::default()
            },
            LoadMessagesPageOpts {
                limit_turns: Some(3),
                around_message_id: Some("sys".into()),
                ..Default::default()
            },
            LoadMessagesPageOpts {
                limit_turns: Some(2),
                after_position: Some(4),
                ..Default::default()
            },
            LoadMessagesPageOpts {
                limit_turns: Some(8),
                after_position: Some(10),
                ..Default::default()
            },
        ];
        for opts in cases {
            assert_window_matches(&data, &opts);
        }
    }

    #[test]
    fn probe_window_empty_transcript() {
        let window =
            probe_window_from_rows(&[], &LoadMessagesPageOpts::default(), None, None, None)
                .unwrap();
        assert_eq!((window.start_position, window.end_position), (0, i64::MAX));
        assert!(!window.has_more_older);
        assert!(!window.has_more_newer);
    }
}
