//! IM thread session state stored on the base conversation row (without `@sN`).

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};

use crate::agents::{AGENT_MODE_SINGLE, DEFAULT_LEAD_AGENT_ID};
use crate::channel_outbound::{im_base_conversation_id, im_desktop_conversation_id};

use super::write;

#[derive(Debug, Clone)]
pub struct ImSessionState {
    pub session_epoch: u32,
    pub active_conversation_id: Option<String>,
    pub last_interaction_at_ms: i64,
    pub lead_agent_id: String,
    pub agent_mode: String,
}

impl Default for ImSessionState {
    fn default() -> Self {
        Self {
            session_epoch: 0,
            active_conversation_id: None,
            last_interaction_at_ms: 0,
            lead_agent_id: DEFAULT_LEAD_AGENT_ID.to_string(),
            agent_mode: AGENT_MODE_SINGLE.to_string(),
        }
    }
}

pub fn resolve_active_desktop_id(base_conv_id: &str, state: &ImSessionState) -> String {
    if let Some(active) = state.active_conversation_id.as_deref().filter(|s| !s.is_empty()) {
        if im_base_conversation_id(active) == base_conv_id {
            return active.to_string();
        }
    }
    im_desktop_conversation_id(base_conv_id, state.session_epoch)
}

pub fn load_im_session_in_conn(conn: &Connection, base_conv_id: &str) -> Result<ImSessionState> {
    let row: Option<(u32, Option<String>, i64, i64, String, String)> = conn
        .query_row(
            "SELECT im_session_epoch, im_active_conversation_id, im_last_interaction_at_ms,
                    updated_at_ms, lead_agent_id, agent_mode
             FROM conversations WHERE id = ?1",
            params![base_conv_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)? as u32,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                ))
            },
        )
        .optional()?;
    Ok(match row {
        Some((epoch, active, im_last, updated_at, lead, mode)) => ImSessionState {
            session_epoch: epoch,
            active_conversation_id: active,
            last_interaction_at_ms: if im_last > 0 {
                im_last
            } else {
                updated_at
            },
            lead_agent_id: lead,
            agent_mode: mode,
        },
        None => ImSessionState::default(),
    })
}

pub fn ensure_im_base_shell_in_conn(conn: &Connection, base_conv_id: &str) -> Result<()> {
    write::ensure_conversation_row(conn, base_conv_id)
}

pub fn save_im_session_in_conn(
    conn: &Connection,
    base_conv_id: &str,
    state: &ImSessionState,
) -> Result<()> {
    ensure_im_base_shell_in_conn(conn, base_conv_id)?;
    let now = write::now_ms();
    conn.execute(
        "UPDATE conversations SET
           im_session_epoch = ?2,
           im_active_conversation_id = ?3,
           lead_agent_id = ?4,
           agent_mode = ?5,
           updated_at_ms = ?6,
           im_last_interaction_at_ms = ?6
         WHERE id = ?1",
        params![
            base_conv_id,
            state.session_epoch as i64,
            state.active_conversation_id,
            state.lead_agent_id,
            state.agent_mode,
            now,
        ],
    )?;
    Ok(())
}

pub fn touch_im_interaction_in_conn(conn: &Connection, base_conv_id: &str) -> Result<()> {
    ensure_im_base_shell_in_conn(conn, base_conv_id)?;
    let now = write::now_ms();
    conn.execute(
        "UPDATE conversations SET im_last_interaction_at_ms = ?2 WHERE id = ?1",
        params![base_conv_id, now],
    )?;
    Ok(())
}
