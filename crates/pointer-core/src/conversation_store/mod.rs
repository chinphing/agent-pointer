//! Canonical SQLite conversation store (Hermes-style) with embedded FTS search.

mod cjk_fts;
mod db;
pub mod im_session;
mod migrate;
mod persist;
mod search;
mod write;
#[cfg(test)]
mod tests;

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use crate::models::{ChatMessage, Conversation, ConversationMeta};
use crate::storage::app_data_dir;

const DB_FILE: &str = "conversations.db";
const SCHEMA_VERSION: i32 = 4;

static GLOBAL: OnceLock<Arc<ConversationStore>> = OnceLock::new();

pub fn conversation_preview(messages: &[ChatMessage]) -> String {
    persist::conversation_preview(messages)
}

pub struct ConversationStore {
    db: db::DbHandle,
}

impl ConversationStore {
    pub fn open_default() -> Result<Arc<Self>> {
        let path = app_data_dir()?.join(DB_FILE);
        Self::open(path).map(Arc::new)
    }

    pub fn open(path: PathBuf) -> Result<Self> {
        let db = db::DbHandle::open(&path)?;
        {
            let conn = db.conn.lock();
            init_schema(&conn)?;
            cjk_fts::ensure_loaded(&conn)?;
            ensure_fts_schema(&conn)?;
        }
        let store = Self { db };
        let canonical = app_data_dir()?.join(DB_FILE);
        if path == canonical {
            let conn = store.db.conn.lock();
            migrate::migrate_json_if_needed(&conn, &migrate::default_json_path()?)?;
            migrate::migrate_channel_histories_if_needed(&conn)?;
        }
        log::info!("conversation_store: opened {}", path.display());
        Ok(store)
    }

    pub fn load_all(&self) -> Result<Vec<Conversation>> {
        let conn = self.db.conn.lock();
        persist::load_all_from_conn(&conn)
    }

    pub fn load_messages(&self, conversation_id: &str) -> Result<Vec<ChatMessage>> {
        let conn = self.db.conn.lock();
        persist::load_messages(&conn, conversation_id)
    }

    pub fn save_all(&self, list: &[Conversation]) -> Result<()> {
        // Snapshot old IDs before the write so we can detect deletions.
        let old_ids: Vec<String> = self
            .load_all()
            .unwrap_or_default()
            .into_iter()
            .map(|c| c.id)
            .collect();
        let new_ids: Vec<String> = list.iter().map(|c| c.id.clone()).collect();

        self.db.execute_write(|conn| {
            persist::delete_conversations_not_in(conn, &new_ids)?;
            let mut written = 0u32;
            for conv in list {
                if persist::upsert_conversation(conn, conv, true)? {
                    written += 1;
                }
            }
            if written > 0 {
                log::debug!(
                    "conversation_store: upserted {written}/{} conversations",
                    list.len()
                );
            }
            Ok(())
        })?;

        // Clean up sandbox directories for deleted conversations.
        let deleted_ids: Vec<&str> = old_ids
            .iter()
            .filter(|id| !new_ids.contains(id))
            .map(|s| s.as_str())
            .collect();
        for id in &deleted_ids {
            if let Err(e) = crate::session_sandbox::SessionSandbox::cleanup(id) {
                log::warn!("session_sandbox cleanup failed for {id}: {e}");
            }
        }

        Ok(())
    }

    /// P1: sync conversation shell fields only; messages are untouched.
    pub fn save_meta_all(&self, metas: &[ConversationMeta]) -> Result<()> {
        let old_ids: Vec<String> = self
            .load_all()
            .unwrap_or_default()
            .into_iter()
            .map(|c| c.id)
            .collect();
        let new_ids: Vec<String> = metas.iter().map(|m| m.id.clone()).collect();

        self.db
            .execute_write(|conn| write::save_meta_all_in_conn(conn, metas))?;

        // Clean up sandbox directories for deleted conversations.
        for id in &old_ids {
            if !new_ids.contains(id) {
                if let Err(e) = crate::session_sandbox::SessionSandbox::cleanup(id) {
                    log::warn!("session_sandbox cleanup failed for {id}: {e}");
                }
            }
        }

        Ok(())
    }

    /// P0: append messages not yet present in the DB.
    pub fn append_missing_messages(
        &self,
        conversation_id: &str,
        messages: &[ChatMessage],
    ) -> Result<u32> {
        self.db.execute_write(|conn| {
            write::append_missing_messages_in_conn(conn, conversation_id, messages)
        })
    }

    /// P0: insert or update one message.
    pub fn upsert_message(&self, conversation_id: &str, msg: &ChatMessage) -> Result<()> {
        self.db
            .execute_write(|conn| write::upsert_message_in_conn(conn, conversation_id, msg))
    }

    pub fn upsert_message_no_refresh(
        &self,
        conversation_id: &str,
        msg: &ChatMessage,
    ) -> Result<()> {
        self.db.execute_write(|conn| {
            write::upsert_message_no_refresh_in_conn(conn, conversation_id, msg)
        })
    }

    pub fn flush_conversation_meta(
        &self,
        conversation_id: &str,
        message_count: u32,
        preview: &str,
    ) -> Result<()> {
        self.db.execute_write(|conn| {
            write::flush_conversation_meta_in_conn(conn, conversation_id, message_count, preview)
        })
    }

    pub fn message_count(&self, conversation_id: &str) -> Result<u32> {
        self.db
            .execute_write(|conn| write::message_count_in_conn(conn, conversation_id))
    }

    pub fn stored_conversation_preview(&self, conversation_id: &str) -> Result<String> {
        self.db
            .execute_write(|conn| write::stored_preview_in_conn(conn, conversation_id))
    }

    /// P2a: ordered upsert without deleting orphan rows (compression / trim).
    pub fn sync_messages_ordered_with_meta(
        &self,
        conversation_id: &str,
        messages: &[ChatMessage],
        message_count: u32,
        preview: &str,
    ) -> Result<()> {
        self.db.execute_write(|conn| {
            write::sync_messages_ordered_with_meta_in_conn(
                conn,
                conversation_id,
                messages,
                message_count,
                preview,
            )
        })
    }

    /// P2b: replace full transcript from client-held messages.
    pub fn replace_messages(
        &self,
        conversation_id: &str,
        messages: &[ChatMessage],
    ) -> Result<()> {
        self.db
            .execute_write(|conn| write::replace_messages_in_conn(conn, conversation_id, messages))
    }

    pub fn upsert_meta(&self, meta: &ConversationMeta) -> Result<()> {
        self.db
            .execute_write(|conn| write::upsert_conversation_meta(conn, meta))
    }

    pub fn workspace_root(&self, conversation_id: &str) -> Result<String> {
        let conn = self.db.conn.lock();
        let root: Option<String> = conn
            .query_row(
                "SELECT workspace_root FROM conversations WHERE id = ?1",
                rusqlite::params![conversation_id],
                |row| row.get(0),
            )
            .optional()?;
        Ok(root.unwrap_or_default())
    }

    pub fn workspace_user_set(&self, conversation_id: &str) -> Result<bool> {
        let conn = self.db.conn.lock();
        let flag: Option<i64> = conn
            .query_row(
                "SELECT workspace_user_set FROM conversations WHERE id = ?1",
                rusqlite::params![conversation_id],
                |row| row.get(0),
            )
            .optional()?;
        Ok(flag.unwrap_or(0) != 0)
    }

    pub fn patch_session_agent(
        &self,
        conversation_id: &str,
        lead_agent_id: &str,
        agent_mode: &str,
    ) -> Result<()> {
        self.db.execute_write(|conn| {
            write::patch_session_agent_in_conn(conn, conversation_id, lead_agent_id, agent_mode)
        })
    }

    /// Set IM sidebar title when the row is new or still uses the default placeholder.
    pub fn load_im_session(&self, base_conv_id: &str) -> Result<im_session::ImSessionState> {
        self.db
            .execute_write(|conn| im_session::load_im_session_in_conn(conn, base_conv_id))
    }

    pub fn save_im_session(
        &self,
        base_conv_id: &str,
        state: &im_session::ImSessionState,
    ) -> Result<()> {
        self.db
            .execute_write(|conn| im_session::save_im_session_in_conn(conn, base_conv_id, state))
    }

    pub fn touch_im_interaction(&self, base_conv_id: &str) -> Result<()> {
        self.db
            .execute_write(|conn| im_session::touch_im_interaction_in_conn(conn, base_conv_id))
    }

    pub fn ensure_im_title(
        &self,
        conversation_id: &str,
        sender_name: Option<&str>,
        first_user_text: Option<&str>,
    ) -> Result<()> {
        let Some(title) =
            crate::channel_outbound::im_conversation_title(conversation_id, sender_name, first_user_text)
        else {
            return Ok(());
        };
        self.db.execute_write(|conn| {
            write::patch_title_if_default_in_conn(conn, conversation_id, &title)
        })
    }

    pub fn dispatch_search_tool(&self, args: &serde_json::Value) -> Result<String> {
        search::dispatch_tool(&self.db, args)
    }

    /// Alias for tool registration / tests.
    pub fn dispatch_tool(&self, args: &serde_json::Value) -> Result<String> {
        self.dispatch_search_tool(args)
    }

    #[cfg(test)]
    pub fn sync_conversations(&self, convs: &[Conversation]) -> Result<()> {
        self.save_all(convs)
    }

    #[cfg(test)]
    pub fn dispatch_tool_for_test(&self, args: &serde_json::Value) -> Result<String> {
        self.dispatch_search_tool(args)
    }
#[cfg(test)]
    pub fn open_in_dir(dir: &std::path::Path) -> Result<Self> {
        Self::open(dir.join(DB_FILE))
    }
}

pub fn global_store() -> Result<Arc<ConversationStore>> {
    if let Some(store) = GLOBAL.get() {
        return Ok(store.clone());
    }
    let store = ConversationStore::open_default()?;
    let _ = GLOBAL.set(store.clone());
    Ok(store)
}

fn init_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_version (
           version INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS store_meta (
           key TEXT PRIMARY KEY,
           value TEXT NOT NULL
         );
         CREATE TABLE IF NOT EXISTS conversations (
           id TEXT PRIMARY KEY,
           title TEXT NOT NULL,
           created_at_ms INTEGER NOT NULL,
           updated_at_ms INTEGER NOT NULL,
           message_count INTEGER NOT NULL DEFAULT 0,
           preview TEXT NOT NULL DEFAULT '',
           skill_ids_json TEXT NOT NULL DEFAULT '[]',
           tool_rounds_used INTEGER NOT NULL DEFAULT 0,
           tool_rounds_used_supervisor INTEGER NOT NULL DEFAULT 0,
           computer_monitor_id TEXT,
           workspace_root TEXT NOT NULL DEFAULT '',
           workspace_user_set INTEGER NOT NULL DEFAULT 0,
           lead_agent_id TEXT NOT NULL DEFAULT 'general',
           agent_mode TEXT NOT NULL DEFAULT 'single',
           im_session_epoch INTEGER NOT NULL DEFAULT 0,
           im_active_conversation_id TEXT
         );
         CREATE TABLE IF NOT EXISTS messages (
           id INTEGER PRIMARY KEY,
           conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
           message_id TEXT NOT NULL,
           role TEXT NOT NULL,
           content TEXT NOT NULL,
           payload TEXT NOT NULL,
           created_at_ms INTEGER NOT NULL,
           position INTEGER NOT NULL,
           UNIQUE(conversation_id, message_id)
         );
         CREATE INDEX IF NOT EXISTS idx_conversations_updated
           ON conversations(updated_at_ms DESC);
         CREATE INDEX IF NOT EXISTS idx_messages_conv_pos
           ON messages(conversation_id, position);",
    )?;
    let version: Option<i32> = conn
        .query_row("SELECT version FROM schema_version LIMIT 1", [], |row| {
            row.get(0)
        })
        .optional()?;
    if version.is_none() {
        conn.execute(
            "INSERT INTO schema_version(version) VALUES (?1)",
            [SCHEMA_VERSION],
        )?;
    }
    migrate_schema_columns(conn)?;
    Ok(())
}

fn migrate_schema_columns(conn: &Connection) -> Result<()> {
    let version: i32 = conn
        .query_row("SELECT version FROM schema_version LIMIT 1", [], |row| {
            row.get(0)
        })
        .unwrap_or(1);
    if version >= SCHEMA_VERSION {
        return Ok(());
    }
    add_column_if_missing(
        conn,
        "conversations",
        "lead_agent_id",
        "TEXT NOT NULL DEFAULT 'general'",
    )?;
    add_column_if_missing(
        conn,
        "conversations",
        "agent_mode",
        "TEXT NOT NULL DEFAULT 'single'",
    )?;
    add_column_if_missing(
        conn,
        "conversations",
        "im_session_epoch",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    add_column_if_missing(conn, "conversations", "im_active_conversation_id", "TEXT")?;
    add_column_if_missing(
        conn,
        "conversations",
        "workspace_user_set",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    conn.execute(
        "UPDATE schema_version SET version = ?1",
        params![SCHEMA_VERSION],
    )?;
    log::info!("conversation_store: migrated schema to v{}", SCHEMA_VERSION);
    Ok(())
}

fn add_column_if_missing(
    conn: &Connection,
    table: &str,
    column: &str,
    column_def: &str,
) -> Result<()> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
    for name in rows {
        if name? == column {
            return Ok(());
        }
    }
    conn.execute(
        &format!("ALTER TABLE {table} ADD COLUMN {column} {column_def}"),
        [],
    )?;
    Ok(())
}

fn ensure_fts_schema(conn: &Connection) -> Result<()> {
    let current: Option<String> = conn
        .query_row(
            "SELECT value FROM store_meta WHERE key = 'fts_tokenizer'",
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
        log::info!("conversation_store: rebuilding FTS with cjk_bigram");
    }

    create_fts_table(conn)?;
    conn.execute(
        "INSERT INTO store_meta(key, value) VALUES ('fts_tokenizer', 'cjk_bigram')
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [],
    )?;
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
