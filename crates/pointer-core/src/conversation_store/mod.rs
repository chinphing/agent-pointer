//! Canonical SQLite conversation store (Hermes-style) with embedded FTS search.

mod agent_recall;
pub mod app_secrets;
mod background_host_merge;
mod cjk_fts;
pub mod cron_jobs;
mod db;
pub mod im_session;
mod message_page;
mod migrate;
mod persist;
pub mod runs;
mod search;
mod session_user;
#[cfg(test)]
mod tests;
pub mod webhook_sources;
mod write;

pub use message_page::{
    load_messages_page, LoadMessagesPageOpts, MessagePage, DEFAULT_MESSAGE_PAGE_TURNS,
};
pub use write::AppendedMessageRow;

pub use session_user::{normalize_session_user_id, ListScope};

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use crate::models::{
    ChatMessage, Conversation, ConversationMeta, ConversationOutlineItem, ConversationSearchHit,
    Project, ProjectCreationResult, ProjectCursor, ProjectPage,
};
use crate::storage::app_data_dir;

const DB_FILE: &str = "conversations.db";
const SCHEMA_VERSION: i32 = 26;

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
        cjk_fts::ensure_registered()?;
        let db = db::DbHandle::open(&path)?;
        {
            let conn = db.conn.lock();
            init_schema(&conn)?;
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

    /// One transcript row. Prefer this over [`Self::load_messages`] when patching a single id.
    pub fn load_message(
        &self,
        conversation_id: &str,
        message_id: &str,
    ) -> Result<Option<ChatMessage>> {
        let conn = self.db.conn.lock();
        persist::load_message_by_id(&conn, conversation_id, message_id)
    }

    /// Tool result row for `toolCallId`, earliest position first.
    pub fn load_tool_message_by_call_id(
        &self,
        conversation_id: &str,
        tool_call_id: &str,
    ) -> Result<Option<ChatMessage>> {
        let conn = self.db.conn.lock();
        persist::load_tool_message_by_call_id(&conn, conversation_id, tool_call_id)
    }

    /// First tool row whose stored `content` contains `needle`.
    pub fn load_first_tool_message_containing(
        &self,
        conversation_id: &str,
        needle: &str,
    ) -> Result<Option<ChatMessage>> {
        let conn = self.db.conn.lock();
        persist::load_first_tool_message_containing(&conn, conversation_id, needle)
    }

    /// First assistant row that issued `tool_name` with `tool_call_id`.
    pub fn load_assistant_message_with_tool_call(
        &self,
        conversation_id: &str,
        tool_call_id: &str,
        tool_name: &str,
    ) -> Result<Option<ChatMessage>> {
        let conn = self.db.conn.lock();
        persist::load_assistant_message_with_tool_call(
            &conn,
            conversation_id,
            tool_call_id,
            tool_name,
        )
    }

    /// Lead user turn ids after `turn_id` (no full payload deserialize).
    pub fn load_subsequent_lead_turn_ids(
        &self,
        conversation_id: &str,
        turn_id: &str,
    ) -> Result<Vec<String>> {
        let conn = self.db.conn.lock();
        persist::load_subsequent_lead_turn_ids(&conn, conversation_id, turn_id)
    }

    /// Latest real lead user message id. Skips scoped and synthetic user rows.
    pub fn load_latest_real_lead_user_message_id(
        &self,
        conversation_id: &str,
    ) -> Result<Option<String>> {
        let conn = self.db.conn.lock();
        persist::load_latest_real_lead_user_message_id(&conn, conversation_id)
    }

    /// Last non-empty assistant `content` (not `rawContent`).
    pub fn load_last_assistant_content(&self, conversation_id: &str) -> Result<Option<String>> {
        let conn = self.db.conn.lock();
        persist::load_last_assistant_content(&conn, conversation_id)
    }

    /// Last assistant body for outbound delivery: `rawContent`, then `content`.
    pub fn load_last_assistant_outbound_text(
        &self,
        conversation_id: &str,
    ) -> Result<Option<String>> {
        let conn = self.db.conn.lock();
        persist::load_last_assistant_outbound_text(&conn, conversation_id)
    }

    /// Newest user attachment whose payload contains `needle` or `alt_needle`.
    pub fn find_user_attachment(
        &self,
        conversation_id: &str,
        needle: &str,
        alt_needle: &str,
        matches: impl Fn(&crate::models::MediaAttachment) -> bool,
    ) -> Result<Option<crate::models::MediaAttachment>> {
        let conn = self.db.conn.lock();
        persist::find_user_attachment(&conn, conversation_id, needle, alt_needle, matches)
    }

    /// Recent user attachments, newest first, until `min_count`.
    pub fn load_recent_user_attachments(
        &self,
        conversation_id: &str,
        min_count: usize,
    ) -> Result<Vec<crate::models::MediaAttachment>> {
        let conn = self.db.conn.lock();
        persist::load_recent_user_attachments(&conn, conversation_id, min_count)
    }

    /// Full DB row count plus lead LLM working-set messages (`included`, non-scoped).
    pub fn load_lead_working_messages(
        &self,
        conversation_id: &str,
    ) -> Result<(Vec<ChatMessage>, u32)> {
        let conn = self.db.conn.lock();
        persist::load_lead_working_messages(&conn, conversation_id)
    }

    /// Turn-windowed messages for UI hydration (`limit_turns` / `before` / `around`).
    pub fn load_messages_page(
        &self,
        conversation_id: &str,
        opts: &LoadMessagesPageOpts,
    ) -> Result<MessagePage> {
        let conn = self.db.conn.lock();
        load_messages_page(&conn, conversation_id, opts)
    }

    /// Scoped sub-agent transcript rows for one trace (on-demand UI expand).
    pub fn load_scoped_sub_messages_for_trace(
        &self,
        conversation_id: &str,
        anchor_message_id: &str,
        trace_id: &str,
        agent_instance_id: Option<&str>,
    ) -> Result<Vec<ChatMessage>> {
        let conn = self.db.conn.lock();
        persist::load_scoped_sub_messages_for_trace(
            &conn,
            conversation_id,
            anchor_message_id,
            trace_id,
            agent_instance_id,
        )
    }

    /// Replace `payload.agentTrace` on one message without rewriting the rest of the row.
    pub fn patch_message_agent_trace(
        &self,
        conversation_id: &str,
        message_id: &str,
        agent_trace_json: &str,
    ) -> Result<usize> {
        let conn = self.db.conn.lock();
        persist::patch_agent_trace_json(&conn, conversation_id, message_id, agent_trace_json)
    }

    /// Cursor-paginated meta-only list (no messages). Sort order is
    /// `(is_pinned DESC, updated_at_ms DESC, id DESC)`. Pass `None` for the first page.
    pub fn load_metas(
        &self,
        scope: &ListScope,
        cursor: Option<persist::MetaCursor>,
        limit: i64,
    ) -> Result<Vec<ConversationMeta>> {
        let conn = self.db.conn.lock();
        persist::load_metas_from_conn(&conn, scope, cursor, limit)
    }

    /// Load one conversation's meta by id (no messages). O(log n) via PK.
    pub fn load_meta(&self, id: &str) -> Result<Option<ConversationMeta>> {
        let conn = self.db.conn.lock();
        persist::load_meta_from_conn(&conn, id)
    }

    pub fn load_projects(
        &self,
        scope: &ListScope,
        cursor: Option<ProjectCursor>,
        limit: i64,
    ) -> Result<ProjectPage> {
        let conn = self.db.conn.lock();
        persist::load_project_page_from_conn(&conn, scope, cursor, limit)
    }

    pub fn load_sidebar_projects(&self, scope: &ListScope) -> Result<Vec<Project>> {
        let conn = self.db.conn.lock();
        persist::load_sidebar_projects_from_conn(&conn, scope)
    }

    pub fn load_project(&self, id: &str, scope: &ListScope) -> Result<Option<Project>> {
        let conn = self.db.conn.lock();
        persist::load_project_from_conn(&conn, id, scope)
    }

    pub fn load_project_metas(
        &self,
        project_id: &str,
        scope: &ListScope,
        cursor: Option<persist::MetaCursor>,
        limit: i64,
    ) -> Result<Vec<ConversationMeta>> {
        let conn = self.db.conn.lock();
        if persist::load_project_from_conn(&conn, project_id, scope)?.is_none() {
            anyhow::bail!("project not found");
        }
        // Project conversations: admin sees all under the project; user scope
        // still filters by session_user_id so mixed-owner legacy rows stay hidden.
        persist::load_project_metas_from_conn(&conn, project_id, scope, cursor, limit)
    }

    pub fn create_project(
        &self,
        name: &str,
        workspace_root: &str,
        session_user_id: &str,
    ) -> Result<ProjectCreationResult> {
        let name = name.trim();
        if name.is_empty() {
            anyhow::bail!("project name is required");
        }
        let workspace_root = normalize_workspace_root(workspace_root);
        if workspace_root.is_empty() {
            anyhow::bail!("project workspace root is required");
        }
        let uid = session_user::normalize_session_user_id(session_user_id).to_string();
        let now = chrono::Utc::now().timestamp_millis();
        let project = Project {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.to_string(),
            workspace_root: workspace_root.clone(),
            is_default: false,
            is_pinned: false,
            is_archived: false,
            created_at: now,
            updated_at: now,
            last_activity_at: now,
            session_user_id: uid.clone(),
        };
        let mut result = self.db.execute_write(|conn| {
            let existing = conn
                .query_row(
                    "SELECT id, name, workspace_root, is_default, is_pinned, is_archived,
                            created_at_ms, updated_at_ms, session_user_id
                     FROM projects
                     WHERE session_user_id = ?1
                       AND rtrim(trim(workspace_root), '/\\') = ?2
                     ORDER BY is_archived ASC, updated_at_ms DESC, id DESC
                     LIMIT 1",
                    params![uid, workspace_root],
                    project_from_row,
                )
                .optional()?;
            if let Some(existing) = existing {
                return Ok(ProjectCreationResult {
                    project: existing,
                    reused_existing: true,
                });
            }
            conn.execute(
                "INSERT INTO projects (id, name, workspace_root, is_default, is_pinned, is_archived,
                                      created_at_ms, updated_at_ms, session_user_id)
                 VALUES (?1, ?2, ?3, 0, 0, 0, ?4, ?4, ?5)",
                params![
                    project.id,
                    project.name,
                    project.workspace_root,
                    now,
                    uid
                ],
            )?;
            Ok(ProjectCreationResult {
                project: project.clone(),
                reused_existing: false,
            })
        })?;
        if result.reused_existing {
            let conn = self.db.conn.lock();
            let scope = ListScope::User(uid.clone());
            if let Some(project) =
                persist::load_project_from_conn(&conn, &result.project.id, &scope)?
            {
                result.project = project;
            }
            log::info!(
                "conversation_store: reused project id={} session_user_id={uid}",
                result.project.id
            );
        } else {
            log::info!(
                "conversation_store: created project id={} session_user_id={uid}",
                result.project.id
            );
        }
        Ok(result)
    }

    pub fn update_project(
        &self,
        id: &str,
        session_user_id: &str,
        name: Option<&str>,
        workspace_root: Option<&str>,
        is_pinned: Option<bool>,
        is_archived: Option<bool>,
    ) -> Result<Project> {
        let uid = session_user::normalize_session_user_id(session_user_id).to_string();
        if is_archived == Some(true) {
            let conn = self.db.conn.lock();
            let is_default: Option<i64> = conn
                .query_row(
                    "SELECT is_default FROM projects WHERE id = ?1 AND session_user_id = ?2",
                    params![id, uid],
                    |row| row.get(0),
                )
                .optional()?;
            if is_default == Some(1) {
                anyhow::bail!("default project cannot be archived");
            }
            if is_default.is_none() {
                anyhow::bail!("project not found");
            }
        }
        let now = chrono::Utc::now().timestamp_millis();
        self.db.execute_write(|conn| {
            let changed = conn.execute(
                "UPDATE projects SET
                   name = COALESCE(?2, name), workspace_root = COALESCE(?3, workspace_root),
                   is_pinned = COALESCE(?4, is_pinned), is_archived = COALESCE(?5, is_archived),
                   updated_at_ms = ?6
                 WHERE id = ?1 AND session_user_id = ?7",
                params![
                    id,
                    name.map(str::trim),
                    workspace_root.map(str::trim),
                    is_pinned.map(i64::from),
                    is_archived.map(i64::from),
                    now,
                    uid
                ],
            )?;
            if changed == 0 {
                anyhow::bail!("project not found");
            }
            Ok(())
        })?;
        let conn = self.db.conn.lock();
        let scope = ListScope::User(uid);
        persist::load_project_from_conn(&conn, id, &scope)?
            .ok_or_else(|| anyhow::anyhow!("project not found"))
    }

    pub fn delete_project(&self, id: &str, session_user_id: &str) -> Result<()> {
        let uid = session_user::normalize_session_user_id(session_user_id).to_string();
        self.db.execute_write(|conn| {
            let default: Option<i64> = conn
                .query_row(
                    "SELECT is_default FROM projects WHERE id = ?1 AND session_user_id = ?2",
                    params![id, uid],
                    |r| r.get(0),
                )
                .optional()?;
            match default {
                None => anyhow::bail!("project not found"),
                Some(v) if v != 0 => anyhow::bail!("default project cannot be deleted"),
                _ => {}
            }
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM conversations WHERE project_id = ?1",
                params![id],
                |r| r.get(0),
            )?;
            // Project deletion is confirmed by the UI and intentionally removes
            // its conversations only. The workspace directory is never touched.
            conn.execute(
                "DELETE FROM conversations WHERE project_id = ?1",
                params![id],
            )?;
            conn.execute(
                "DELETE FROM projects WHERE id = ?1 AND session_user_id = ?2",
                params![id, uid],
            )?;
            log::info!(
                "conversation_store: deleted project id={id} session_user_id={uid} conversations={count}"
            );
            Ok(())
        })?;
        Ok(())
    }

    /// Cheap list of all conversation ids (no message deserialization).
    pub fn list_all_ids(&self) -> Result<Vec<String>> {
        let conn = self.db.conn.lock();
        persist::list_all_ids_from_conn(&conn)
    }

    /// Most-recently-updated `workspace_root` among conversations other than
    /// `exclude_id` with a non-empty workspace. `None` when no candidate.
    pub fn latest_other_workspace_root(&self, exclude_id: &str) -> Result<Option<String>> {
        let conn = self.db.conn.lock();
        persist::latest_other_workspace_root_from_conn(&conn, exclude_id)
    }

    /// Last lead-agent LLM round `prompt_tokens` (API-reported, includes system/tools).
    pub fn get_last_lead_prompt_tokens(&self, conversation_id: &str) -> Result<Option<u32>> {
        let conn = self.db.conn.lock();
        persist::get_last_lead_prompt_tokens_from_conn(&conn, conversation_id)
    }

    pub fn set_last_lead_prompt_tokens(
        &self,
        conversation_id: &str,
        prompt_tokens: Option<u32>,
    ) -> Result<()> {
        self.db.execute_write(|conn| {
            persist::set_last_lead_prompt_tokens_in_conn(conn, conversation_id, prompt_tokens)
        })
    }

    pub fn session_user_id(&self, conversation_id: &str) -> Result<String> {
        let conn = self.db.conn.lock();
        session_user::session_user_id_in_conn(&conn, conversation_id)
    }

    pub fn set_session_user_id(&self, conversation_id: &str, user_id: &str) -> Result<()> {
        self.db.execute_write(|conn| {
            session_user::set_session_user_id_in_conn(conn, conversation_id, user_id)
        })
    }

    /// Find the most recently active IM desktop conversation for a channel peer
    /// (`session_user_id` = peer). Used to mirror cron/IM deliveries into the
    /// same transcript the user will reply into (Feishu chat_id ≠ open_id).
    pub fn find_im_desktop_for_channel_peer(
        &self,
        channel: &str,
        account_id: &str,
        peer_user_id: &str,
    ) -> Result<Option<(String, String)>> {
        let conn = self.db.conn.lock();
        session_user::find_im_desktop_for_channel_peer_in_conn(
            &conn,
            channel,
            account_id,
            peer_user_id,
        )
    }

    pub fn ensure_session_user_id(&self, conversation_id: &str, candidate: &str) -> Result<String> {
        self.db.execute_write(|conn| {
            session_user::ensure_session_user_id_in_conn(conn, conversation_id, candidate)
        })
    }

    /// Full-conversation import/upsert. **Test-only since 2026-08**: the legacy
    /// FE/server `save_conversations` API was removed (production logs never
    /// showed `save_all_legacy_import` after the append-migration). Unit tests
    /// still use this as a convenience write path.
    #[cfg(test)]
    pub fn save_all(&self, list: &[Conversation]) -> Result<()> {
        // Snapshot old IDs before the write so we can detect deletions.
        let old_ids: Vec<String> = self.list_all_ids().unwrap_or_default();
        let new_ids: Vec<String> = list.iter().map(|c| c.id.clone()).collect();
        let total_messages: usize = list.iter().map(|c| c.messages.len()).sum();
        log::info!(
            "conversation_store: save_all_test_only conversations={} total_messages={} deleted_absent={}",
            list.len(),
            total_messages,
            old_ids
                .iter()
                .filter(|id| !new_ids.contains(id))
                .count()
        );

        self.db.execute_write(|conn| {
            persist::delete_conversations_not_in(conn, &new_ids)?;
            let mut written = 0u32;
            for conv in list {
                if persist::upsert_conversation(conn, conv, true)? {
                    written += 1;
                }
            }
            if written > 0 {
                log::info!(
                    "conversation_store: save_all_test_only upserted {written}/{} conversations",
                    list.len()
                );
            }
            Ok(())
        })?;
        for id in &new_ids {
            crate::conversation_session::note_transcript_mutated(id);
        }
        for id in &old_ids {
            if !new_ids.contains(id) {
                crate::conversation_session::drop_conversation(id);
            }
        }

        // Clean up sandbox directories for deleted conversations.
        let deleted_ids: Vec<&str> = old_ids
            .iter()
            .filter(|id| !new_ids.contains(id))
            .map(|s| s.as_str())
            .collect();
        for id in &deleted_ids {
            if let Err(e) = crate::session_sandbox::SessionSandbox::cleanup_for_conversation(id) {
                log::warn!("session_sandbox cleanup failed for {id}: {e}");
            }
        }

        Ok(())
    }

    /// P1: sync conversation shell fields only; messages are untouched.
    /// Pure upsert — does NOT delete conversations absent from `metas`.
    /// Deletion is handled explicitly via [`ConversationStore::delete_conversation`].
    pub fn save_meta_all(&self, metas: &[ConversationMeta]) -> Result<()> {
        self.save_meta_all_with_platform_user(metas, None)
    }

    /// Like [`Self::save_meta_all`], but binds empty `session_user_id` rows to
    /// `platform_user_id` when the desktop/web user is logged in.
    pub fn save_meta_all_with_platform_user(
        &self,
        metas: &[ConversationMeta],
        platform_user_id: Option<&str>,
    ) -> Result<()> {
        self.db.execute_write(|conn| {
            if let Some(uid) = platform_user_id.filter(|s| !s.trim().is_empty()) {
                for meta in metas {
                    if let Err(e) =
                        session_user::ensure_session_user_id_in_conn(conn, &meta.id, uid)
                    {
                        log::warn!(
                            "session_user_id ensure on meta save failed conv={}: {e:#}",
                            meta.id
                        );
                    }
                }
            }
            write::save_meta_all_in_conn(conn, metas)?;
            // A first conversation learns the platform user id before its
            // workspace event arrives. Reconcile again on every meta save so
            // that first resolved sandbox becomes that user's default project
            // without requiring an application restart.
            if let Some(uid) = platform_user_id.filter(|s| !s.trim().is_empty()) {
                reconcile_default_project_for_user(conn, uid)?;
            }
            Ok(())
        })?;
        Ok(())
    }

    /// Explicitly delete one conversation and its messages, and clean up its
    /// sandbox directory. Use this instead of relying on `save_meta_all` to
    /// diff against a (now paginated, incomplete) in-memory list.
    pub fn delete_conversation(&self, id: &str) -> Result<()> {
        self.db
            .execute_write(|conn| persist::delete_conversation_from_conn(conn, id))?;
        crate::conversation_session::drop_conversation(id);
        log::info!("conversation_store: deleted conversation id={id}");
        if let Err(e) = crate::session_sandbox::SessionSandbox::cleanup_for_conversation(id) {
            log::warn!("session_sandbox cleanup failed for {id}: {e}");
        }
        Ok(())
    }

    /// P0: append messages not yet present in the DB.
    ///
    /// Crate-private: external crates must use [`crate::conversation_session`].
    pub(crate) fn append_missing_messages(
        &self,
        conversation_id: &str,
        messages: &[ChatMessage],
    ) -> Result<Vec<AppendedMessageRow>> {
        let appended = self.db.execute_write(|conn| {
            write::append_missing_messages_in_conn(conn, conversation_id, messages)
        })?;
        if !appended.is_empty() {
            crate::conversation_session::note_transcript_mutated(conversation_id);
        }
        Ok(appended)
    }

    /// Ensure a dedicated cron session row exists for a cron job. Creates the
    /// `cron:{job_id}` conversation meta with the given title on first fire.
    /// Cron sessions are isolated from the user's interactive sessions: they are
    /// excluded from the sidebar/pagination queries and are only reachable via
    /// the cron job management UI.
    pub fn ensure_cron_session(&self, conversation_id: &str, title: &str) -> Result<()> {
        self.db.execute_write(|conn| {
            write::ensure_conversation_row_with_title(conn, conversation_id, Some(title))
        })
    }

    /// Resolve the active webhook session for `:src` (daily rollover or per-delivery).
    pub fn resolve_webhook_ingress_session(
        &self,
        src: &str,
        delivery_id: Option<&str>,
    ) -> Result<String> {
        self.db.execute_write(|conn| {
            webhook_sources::resolve_ingress_session_id(
                conn,
                src,
                &chrono::Local::now(),
                delivery_id,
            )
        })
    }

    /// Record explicit webhook session id from multipart upload (keeps trigger aligned).
    pub fn adopt_webhook_upload_session(&self, src: &str, conversation_id: &str) -> Result<()> {
        self.db.execute_write(|conn| {
            webhook_sources::adopt_upload_session(conn, src, conversation_id, &chrono::Local::now())
        })
    }

    pub fn webhook_sources_get(
        &self,
        src: &str,
    ) -> Result<Option<webhook_sources::WebhookSourceRecord>> {
        let conn = self.db.conn.lock();
        webhook_sources::get(&conn, src)
    }

    pub fn webhook_sources_ensure_row(&self, src: &str) -> Result<()> {
        let now_ms = chrono::Local::now().timestamp_millis();
        let conn = self.db.conn.lock();
        webhook_sources::ensure_row(&conn, src, now_ms)
    }

    pub fn webhook_sources_delete(&self, src: &str) -> Result<bool> {
        let conn = self.db.conn.lock();
        webhook_sources::delete(&conn, src)
    }

    pub fn webhook_sources_set_auth_header_name(
        &self,
        src: &str,
        auth_header_name: Option<&str>,
    ) -> Result<()> {
        let conn = self.db.conn.lock();
        webhook_sources::set_auth_header_name(&conn, src, auth_header_name)
    }

    pub fn webhook_sources_auth_header_name(&self, src: &str) -> Result<Option<String>> {
        let conn = self.db.conn.lock();
        webhook_sources::auth_header_name(&conn, src)
    }

    pub fn webhook_sources_set_session_mode(
        &self,
        src: &str,
        mode: webhook_sources::WebhookSessionMode,
    ) -> Result<()> {
        let conn = self.db.conn.lock();
        webhook_sources::set_session_mode(&conn, src, mode)
    }

    pub fn webhook_sources_session_mode(
        &self,
        src: &str,
    ) -> Result<webhook_sources::WebhookSessionMode> {
        let conn = self.db.conn.lock();
        webhook_sources::session_mode(&conn, src)
    }

    /// P0: insert or update one message.
    ///
    /// Crate-private: external crates must use [`crate::conversation_session`].
    /// Retained for store unit tests (`upsert` with refresh); production uses
    /// [`Self::upsert_message_no_refresh`] via the session facade.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn upsert_message(&self, conversation_id: &str, msg: &ChatMessage) -> Result<()> {
        self.db
            .execute_write(|conn| write::upsert_message_in_conn(conn, conversation_id, msg))?;
        crate::conversation_session::note_transcript_mutated(conversation_id);
        Ok(())
    }

    pub(crate) fn upsert_message_no_refresh(
        &self,
        conversation_id: &str,
        msg: &ChatMessage,
    ) -> Result<()> {
        self.db.execute_write(|conn| {
            write::upsert_message_no_refresh_in_conn(conn, conversation_id, msg)
        })?;
        crate::conversation_session::note_transcript_mutated(conversation_id);
        Ok(())
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

    pub fn count_duplicate_positions(&self, conversation_id: &str) -> Result<u32> {
        self.db
            .execute_write(|conn| write::count_duplicate_positions_in_conn(conn, conversation_id))
    }

    pub fn stored_conversation_preview(&self, conversation_id: &str) -> Result<String> {
        self.db
            .execute_write(|conn| write::stored_preview_in_conn(conn, conversation_id))
    }

    /// P2a: ordered upsert without deleting orphan rows (compression / trim).
    /// When DB has rows absent from `messages` (soft-exclude), positions of existing
    /// ids are preserved and new ids are inserted near neighbors (or appended).
    ///
    /// Crate-private: external crates must use [`crate::conversation_session`].
    pub(crate) fn sync_messages_ordered_with_meta(
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
        })?;
        crate::conversation_session::note_transcript_mutated(conversation_id);
        Ok(())
    }

    /// Soft-exclude payloads + shift suffix + insert summary at the cut point.
    ///
    /// Crate-private: external crates must use [`crate::conversation_session`].
    pub(crate) fn persist_context_compression(
        &self,
        conversation_id: &str,
        excluded_messages: &[ChatMessage],
        summary: &ChatMessage,
        insert_before_message_id: &str,
        preview: &str,
    ) -> Result<()> {
        self.db.execute_write(|conn| {
            write::persist_context_compression_in_conn(
                conn,
                conversation_id,
                excluded_messages,
                summary,
                insert_before_message_id,
                preview,
            )
        })?;
        crate::conversation_session::note_transcript_mutated(conversation_id);
        Ok(())
    }

    /// Soft-exclude sub-agent rows in place and insert a scoped summary.
    /// Does not publish the lead working set.
    pub(crate) fn persist_sub_agent_compression(
        &self,
        conversation_id: &str,
        excluded_messages: &[ChatMessage],
        summary: &ChatMessage,
        insert_before_message_id: &str,
        anchor_message_id: &str,
        agent_instance_id: &str,
    ) -> Result<()> {
        self.db.execute_write(|conn| {
            write::persist_sub_agent_compression_in_conn(
                conn,
                conversation_id,
                excluded_messages,
                summary,
                insert_before_message_id,
                anchor_message_id,
                agent_instance_id,
            )
        })?;
        crate::conversation_session::note_transcript_mutated(conversation_id);
        Ok(())
    }

    /// P2b: replace full transcript from client-held messages.
    ///
    /// Crate-private: used by store unit tests and legacy in-crate paths.
    /// External crates use `save_all` for full-conversation import.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn replace_messages(
        &self,
        conversation_id: &str,
        messages: &[ChatMessage],
    ) -> Result<()> {
        self.db.execute_write(|conn| {
            write::replace_messages_in_conn(conn, conversation_id, messages)
        })?;
        crate::conversation_session::note_transcript_mutated(conversation_id);
        Ok(())
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

    pub fn workspace_inherit_disabled(&self, conversation_id: &str) -> Result<bool> {
        let conn = self.db.conn.lock();
        let flag: Option<i64> = conn
            .query_row(
                "SELECT workspace_inherit_disabled FROM conversations WHERE id = ?1",
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
    ) -> Result<()> {
        self.db.execute_write(|conn| {
            write::patch_session_agent_in_conn(conn, conversation_id, lead_agent_id)
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
        let Some(title) = crate::channel_outbound::im_conversation_title(
            conversation_id,
            sender_name,
            first_user_text,
        ) else {
            return Ok(());
        };
        self.db.execute_write(|conn| {
            write::patch_title_if_default_in_conn(conn, conversation_id, &title)
        })
    }

    pub fn dispatch_search_tool(&self, args: &serde_json::Value) -> Result<String> {
        agent_recall::dispatch_tool(&self.db, args)
    }

    pub fn dispatch_read_tool(&self, args: &serde_json::Value) -> Result<String> {
        agent_recall::dispatch_read_tool(&self.db, args)
    }

    pub fn ensure_lead_agent_instance(&self, conversation_id: &str) -> Result<String> {
        self.db
            .execute_write(|conn| write::ensure_lead_agent_instance_in_conn(conn, conversation_id))
    }

    /// Sidebar search: FTS over full message bodies (+ title/preview supplement).
    pub fn search_conversations(
        &self,
        scope: &ListScope,
        query: &str,
        limit: i64,
    ) -> Result<Vec<ConversationSearchHit>> {
        search::search_conversations_for_ui(&self.db, scope, query, limit)
    }

    pub fn list_conversation_search_matches(
        &self,
        scope: &ListScope,
        conversation_id: &str,
        query: &str,
    ) -> Result<Vec<crate::models::ConversationSearchMatch>> {
        search::list_conversation_search_matches(&self.db, scope, conversation_id, query)
    }

    /// Full-session real user turns for in-chat 导航 (preview only).
    pub fn list_conversation_outline(
        &self,
        scope: &ListScope,
        conversation_id: &str,
    ) -> Result<Vec<ConversationOutlineItem>> {
        let conversation_id = conversation_id.trim();
        if conversation_id.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.db.conn.lock();
        if !persist::conversation_in_scope(&conn, conversation_id, scope.filter_uid())? {
            anyhow::bail!("conversation not found");
        }
        persist::load_conversation_outline(&conn, conversation_id)
    }

    /// Mark or clear a real lead user turn as a nav milestone.
    pub fn set_message_milestone(
        &self,
        scope: &ListScope,
        conversation_id: &str,
        message_id: &str,
        milestone: bool,
    ) -> Result<()> {
        let conversation_id = conversation_id.trim();
        if conversation_id.is_empty() {
            log::warn!("conversation_store: set_message_milestone skipped; empty conversation_id");
            anyhow::bail!("conversation not found");
        }
        let message_id = message_id.trim();
        if message_id.is_empty() {
            log::warn!(
                "conversation_store: set_message_milestone skipped; empty message_id conversation_id={conversation_id}"
            );
            anyhow::bail!("message id is empty");
        }
        let conversation_id = conversation_id.to_string();
        let message_id = message_id.to_string();
        self.db.execute_write(move |conn| {
            if !persist::conversation_in_scope(conn, &conversation_id, scope.filter_uid())? {
                anyhow::bail!("conversation not found");
            }
            persist::set_message_milestone(conn, &conversation_id, &message_id, milestone)
        })
    }

    /// Alias for tool registration / tests.
    pub fn dispatch_tool(&self, args: &serde_json::Value) -> Result<String> {
        self.dispatch_search_tool(args)
    }

    // ---- runs table (dispatcher) ----

    /// Insert a new run row with `status = queued`. Returns false if a row
    /// with the same `run_id` already exists (INSERT OR IGNORE).
    pub fn runs_insert_queued(
        &self,
        run_id: &str,
        conversation_id: &str,
        trigger_source: crate::dispatcher::TriggerSource,
        trigger_meta_json: &str,
        idempotency_key: Option<&str>,
    ) -> Result<bool> {
        self.db.execute_write(|conn| {
            runs::insert_queued(
                conn,
                run_id,
                conversation_id,
                trigger_source,
                trigger_meta_json,
                idempotency_key,
            )
        })
    }

    pub fn runs_find_by_idempotency_key(&self, key: &str) -> Result<Option<(String, String)>> {
        let conn = self.db.conn.lock();
        runs::find_by_idempotency_key(&conn, key)
    }

    pub fn runs_set_status(
        &self,
        run_id: &str,
        status: runs::RunStatus,
        error: Option<&str>,
    ) -> Result<()> {
        self.db
            .execute_write(|conn| runs::set_status(conn, run_id, status, error))
    }

    pub fn runs_get(&self, run_id: &str) -> Result<Option<runs::RunRecord>> {
        let conn = self.db.conn.lock();
        runs::get(&conn, run_id)
    }

    pub fn runs_list_queued(&self, limit: usize) -> Result<Vec<runs::RunRecord>> {
        let conn = self.db.conn.lock();
        runs::list_by_status(&conn, "queued", limit)
    }

    pub fn runs_list_non_terminal_for_conversation(
        &self,
        conversation_id: &str,
        limit: usize,
    ) -> Result<Vec<runs::RunRecord>> {
        let conn = self.db.conn.lock();
        runs::list_non_terminal_by_conversation(&conn, conversation_id, limit)
    }

    /// Cancel queued/running rows left over from a prior process (no live task).
    pub fn runs_reconcile_interrupted(&self) -> Result<u32> {
        self.db
            .execute_write(|conn| runs::reconcile_interrupted(conn))
    }

    // ---- cron_jobs (Phase 5 scheduler) ----

    pub fn cron_jobs_insert(&self, job: &cron_jobs::NewCronJob<'_>) -> Result<bool> {
        let conn = self.db.conn.lock();
        cron_jobs::insert(&conn, job)
    }

    pub fn cron_jobs_list_due(&self, now_ms: i64) -> Result<Vec<cron_jobs::CronJobRecord>> {
        let conn = self.db.conn.lock();
        cron_jobs::list_due(&conn, now_ms)
    }

    pub fn cron_jobs_list_all(&self) -> Result<Vec<cron_jobs::CronJobRecord>> {
        let conn = self.db.conn.lock();
        cron_jobs::list_all(&conn)
    }

    pub fn cron_jobs_get(&self, id: &str) -> Result<Option<cron_jobs::CronJobRecord>> {
        let conn = self.db.conn.lock();
        cron_jobs::get(&conn, id)
    }

    pub fn cron_jobs_mark_ran<Z: chrono::TimeZone>(
        &self,
        id: &str,
        ran_at: chrono::DateTime<Z>,
    ) -> Result<()> {
        let conn = self.db.conn.lock();
        cron_jobs::mark_ran(&conn, id, ran_at)
    }

    pub fn cron_jobs_set_enabled(&self, id: &str, enabled: bool) -> Result<bool> {
        let conn = self.db.conn.lock();
        cron_jobs::set_enabled(&conn, id, enabled)
    }

    pub fn cron_jobs_set_current_session_id(&self, id: &str, session_id: &str) -> Result<()> {
        let conn = self.db.conn.lock();
        cron_jobs::set_current_session_id(&conn, id, session_id)
    }

    pub fn cron_jobs_delete(&self, id: &str) -> Result<bool> {
        let conn = self.db.conn.lock();
        cron_jobs::delete(&conn, id)
    }

    pub fn cron_jobs_update_deliver(&self, id: &str, deliver: Option<&str>) -> Result<bool> {
        let conn = self.db.conn.lock();
        cron_jobs::update_deliver(&conn, id, deliver)
    }

    pub fn cron_jobs_set_last_delivery_error(&self, id: &str, err: Option<&str>) -> Result<()> {
        let conn = self.db.conn.lock();
        cron_jobs::set_last_delivery_error(&conn, id, err)
    }

    // ---- app_secrets (encrypted app-level secrets, e.g. webhook token) ----

    pub fn app_secret_get(&self, label: &str) -> Result<Option<Vec<u8>>> {
        let conn = self.db.conn.lock();
        app_secrets::get(&conn, label)
    }

    pub fn app_secret_has(&self, label: &str) -> Result<bool> {
        let conn = self.db.conn.lock();
        app_secrets::has(&conn, label)
    }

    /// First-write-only insert. Returns false if a secret already exists.
    pub fn app_secret_try_insert(&self, label: &str, value: &[u8]) -> Result<bool> {
        let conn = self.db.conn.lock();
        app_secrets::try_insert(&conn, label, value)
    }

    pub fn app_secret_delete(&self, label: &str) -> Result<bool> {
        let conn = self.db.conn.lock();
        app_secrets::delete(&conn, label)
    }

    pub fn app_secret_list_by_prefix(&self, prefix: &str) -> Result<Vec<(String, i64)>> {
        let conn = self.db.conn.lock();
        app_secrets::list_by_prefix(&conn, prefix)
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
    pub fn dispatch_read_tool_for_test(&self, args: &serde_json::Value) -> Result<String> {
        self.dispatch_read_tool(args)
    }

    #[cfg(test)]
    pub fn message_agent_instance_id_col(
        &self,
        conversation_id: &str,
        message_id: &str,
    ) -> Result<Option<String>> {
        let conn = self.db.conn.lock();
        conn.query_row(
            "SELECT agent_instance_id FROM messages
             WHERE conversation_id = ?1 AND message_id = ?2",
            rusqlite::params![conversation_id, message_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(Into::into)
        .map(|v: Option<Option<String>>| v.flatten())
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
           workspace_inherit_disabled INTEGER NOT NULL DEFAULT 0,
           lead_agent_id TEXT NOT NULL DEFAULT 'general',
           agent_mode TEXT NOT NULL DEFAULT 'single',
           performance_mode TEXT,
           im_session_epoch INTEGER NOT NULL DEFAULT 0,
           im_active_conversation_id TEXT,
           im_last_interaction_at_ms INTEGER NOT NULL DEFAULT 0,
           session_user_id TEXT NOT NULL DEFAULT '',
           is_pinned INTEGER NOT NULL DEFAULT 0,
           lead_agent_instance_id TEXT
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
           is_system_generated INTEGER NOT NULL DEFAULT 0,
           context_included INTEGER NOT NULL DEFAULT 1,
           tool_name TEXT,
           agent_instance_id TEXT,
           is_scoped INTEGER NOT NULL DEFAULT 0,
           is_milestone INTEGER NOT NULL DEFAULT 0,
           UNIQUE(conversation_id, message_id)
         );
         CREATE INDEX IF NOT EXISTS idx_conversations_updated
           ON conversations(updated_at_ms DESC);
         CREATE INDEX IF NOT EXISTS idx_messages_conv_pos
           ON messages(conversation_id, position);",
    )?;
    // `runs` table lives in the same db; idempotent CREATE.
    runs::ensure_schema(conn)?;
    cron_jobs::ensure_schema(conn)?;
    webhook_sources::ensure_schema(conn)?;
    app_secrets::ensure_schema(conn)?;
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
    // Always-run: existing DBs already at SCHEMA_VERSION still need new columns.
    add_column_if_missing(
        conn,
        "conversations",
        "is_pinned",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    add_column_if_missing(conn, "conversations", "lead_agent_instance_id", "TEXT")?;
    // v21 messages anchor flag — same always-run guard for DBs that somehow
    // reached the version without the column (schema_version was pre-written).
    add_column_if_missing(
        conn,
        "messages",
        "is_system_generated",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_messages_conv_anchor
           ON messages(conversation_id, position) WHERE role = 'user' AND is_system_generated = 0;",
    )?;
    // v22: materialized lead-context flag (mirrors is_context_included).
    add_column_if_missing(
        conn,
        "messages",
        "context_included",
        "INTEGER NOT NULL DEFAULT 1",
    )?;
    crate::conversation_store::persist::backfill_context_included(conn)?;
    add_column_if_missing(conn, "messages", "tool_name", "TEXT")?;
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_messages_conv_included_pos
           ON messages(conversation_id, position) WHERE context_included = 1;",
    )?;
    ensure_messages_agent_instance_id(conn)?;
    ensure_messages_is_scoped(conn)?;
    crate::conversation_store::persist::backfill_thread_context_identity(conn)?;
    // User-set nav milestone. Kept off the message payload so ordinary
    // transcript upserts do not clear it. Full replaces snapshot/restore it.
    add_column_if_missing(
        conn,
        "messages",
        "is_milestone",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    // After column migrations, create indexes that depend on newer columns.
    ensure_conversations_user_updated_index(conn)?;
    ensure_projects_schema(conn)?;
    // project_id is added in ensure_projects_schema; pin indexes need it.
    ensure_conversations_pinned_updated_index(conn)?;
    Ok(())
}

/// Project migration is intentionally gated by the `projects` table itself,
/// rather than the shared schema version. This allows an app upgrade to safely
/// add project support even when an older build already advanced that version.
fn ensure_projects_schema(conn: &Connection) -> Result<()> {
    let exists: bool = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'projects' LIMIT 1",
            [],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    add_column_if_missing(conn, "conversations", "project_id", "TEXT")?;
    if !exists {
        conn.execute_batch(
            "CREATE TABLE projects (
               id TEXT PRIMARY KEY,
               name TEXT NOT NULL,
               workspace_root TEXT NOT NULL DEFAULT '',
               is_default INTEGER NOT NULL DEFAULT 0,
               is_pinned INTEGER NOT NULL DEFAULT 0,
               is_archived INTEGER NOT NULL DEFAULT 0,
               created_at_ms INTEGER NOT NULL,
               updated_at_ms INTEGER NOT NULL,
               session_user_id TEXT NOT NULL DEFAULT ''
             );",
        )?;
        log::info!("conversation_store: created projects table");
    }
    add_column_if_missing(conn, "projects", "is_pinned", "INTEGER NOT NULL DEFAULT 0")?;
    add_column_if_missing(
        conn,
        "projects",
        "is_archived",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    add_column_if_missing(
        conn,
        "projects",
        "session_user_id",
        "TEXT NOT NULL DEFAULT ''",
    )?;
    // Recover databases opened by an intermediate build that created the table
    // before project backfill existed. A populated table is authoritative and
    // is never re-migrated.
    let project_count: i64 =
        conn.query_row("SELECT COUNT(*) FROM projects", [], |row| row.get(0))?;
    if project_count == 0 {
        log::info!(
            "conversation_store: projects table is empty; backfilling historical conversations"
        );
        migrate_projects_from_conversations(conn)?;
    }
    reconcile_defaults_for_known_users(conn)?;
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_conversations_project_updated
           ON conversations(project_id, updated_at_ms DESC);
         CREATE INDEX IF NOT EXISTS idx_projects_session_user
           ON projects(session_user_id, is_archived, is_pinned DESC, updated_at_ms DESC);",
    )?;
    Ok(())
}

fn migrate_projects_from_conversations(conn: &Connection) -> Result<()> {
    let now = chrono::Utc::now().timestamp_millis();
    let mut uids = std::collections::BTreeSet::new();
    {
        let mut stmt = conn.prepare(
            "SELECT DISTINCT trim(session_user_id) FROM conversations
             WHERE id NOT LIKE 'cron:%' AND id NOT LIKE 'webhook:%' AND id NOT LIKE 'im:%'",
        )?;
        for uid in stmt.query_map([], |row| row.get::<_, String>(0))? {
            uids.insert(uid?);
        }
    }
    if uids.is_empty() {
        uids.insert(String::new());
    }

    let mut project_total = 0usize;
    let mut assigned_total = 0usize;
    for uid in uids {
        let default_root = if uid.is_empty() {
            String::new()
        } else {
            crate::session_sandbox::SessionSandbox::default_path("", &uid)?
                .to_string_lossy()
                .into_owned()
        };
        let default_id = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO projects (id, name, workspace_root, is_default, is_pinned, is_archived,
                                  created_at_ms, updated_at_ms, session_user_id)
             VALUES (?1, '默认项目', ?2, 1, 0, 0, ?3, ?3, ?4)",
            params![default_id, default_root, now, uid],
        )?;
        project_total += 1;

        let mut roots = std::collections::BTreeSet::new();
        {
            let mut stmt = conn.prepare(
                "SELECT DISTINCT trim(workspace_root) FROM conversations
                 WHERE id NOT LIKE 'cron:%' AND id NOT LIKE 'webhook:%' AND id NOT LIKE 'im:%'
                   AND trim(session_user_id) = ?1
                   AND trim(workspace_root) != '' AND trim(workspace_root) != ?2",
            )?;
            for root in stmt.query_map(params![uid, default_root], |row| row.get::<_, String>(0))? {
                roots.insert(root?);
            }
        }
        for root in roots {
            let id = uuid::Uuid::new_v4().to_string();
            conn.execute(
                "INSERT INTO projects (id, name, workspace_root, is_default, is_pinned, is_archived,
                                      created_at_ms, updated_at_ms, session_user_id)
                 VALUES (?1, ?2, ?3, 0, 0, 0, ?4, ?4, ?5)",
                params![id, project_name_for_root(&root), root, now, uid],
            )?;
            project_total += 1;
        }

        assigned_total += conn.execute(
            "UPDATE conversations SET project_id = ?1
             WHERE id NOT LIKE 'cron:%' AND id NOT LIKE 'webhook:%' AND id NOT LIKE 'im:%'
               AND trim(session_user_id) = ?3
               AND (trim(workspace_root) = '' OR trim(workspace_root) = ?2)",
            params![default_id, default_root, uid],
        )?;
        assigned_total += conn.execute(
            "UPDATE conversations SET project_id = (
               SELECT id FROM projects p
               WHERE p.session_user_id = trim(conversations.session_user_id)
                 AND p.workspace_root = trim(conversations.workspace_root)
               LIMIT 1
             )
             WHERE id NOT LIKE 'cron:%' AND id NOT LIKE 'webhook:%' AND id NOT LIKE 'im:%'
               AND trim(session_user_id) = ?1
               AND trim(workspace_root) != '' AND trim(workspace_root) != ?2",
            params![uid, default_root],
        )?;
    }
    log::info!(
        "conversation_store: project migration projects={project_total} assigned={assigned_total}"
    );
    Ok(())
}

fn reconcile_defaults_for_known_users(conn: &Connection) -> Result<()> {
    let mut uids = std::collections::BTreeSet::new();
    {
        let mut stmt = conn.prepare(
            "SELECT DISTINCT trim(session_user_id) FROM conversations
             WHERE trim(session_user_id) != ''
               AND id NOT LIKE 'cron:%' AND id NOT LIKE 'webhook:%' AND id NOT LIKE 'im:%'",
        )?;
        for uid in stmt.query_map([], |row| row.get::<_, String>(0))? {
            uids.insert(uid?);
        }
    }
    {
        let mut stmt = conn.prepare(
            "SELECT DISTINCT trim(session_user_id) FROM projects
             WHERE trim(session_user_id) != ''",
        )?;
        for uid in stmt.query_map([], |row| row.get::<_, String>(0))? {
            uids.insert(uid?);
        }
    }
    for uid in uids {
        reconcile_default_project_for_user(conn, &uid)?;
    }
    Ok(())
}

/// Ensure the signed-in user has exactly one default project pointing at their
/// `{session-sandboxes}/{session_user_id}` directory.
fn reconcile_default_project_for_user(conn: &Connection, session_user_id: &str) -> Result<()> {
    let uid = session_user::normalize_session_user_id(session_user_id);
    if uid.is_empty() {
        return Ok(());
    }
    let root = crate::session_sandbox::SessionSandbox::default_path("", uid)?
        .to_string_lossy()
        .into_owned();
    if root.is_empty() {
        return Ok(());
    }
    let now = chrono::Utc::now().timestamp_millis();

    let by_root: Option<String> = conn
        .query_row(
            "SELECT id FROM projects
             WHERE session_user_id = ?1 AND workspace_root = ?2
             ORDER BY is_default DESC, updated_at_ms DESC, id DESC
             LIMIT 1",
            params![uid, root],
            |row| row.get(0),
        )
        .optional()?;

    let project_id = match by_root {
        Some(id) => id,
        None => {
            // Claim legacy unowned project that already points at this sandbox.
            let orphan: Option<String> = conn
                .query_row(
                    "SELECT id FROM projects
                     WHERE session_user_id = '' AND workspace_root = ?1
                     ORDER BY is_default DESC, updated_at_ms DESC, id DESC
                     LIMIT 1",
                    params![root],
                    |row| row.get(0),
                )
                .optional()?;
            if let Some(id) = orphan {
                conn.execute(
                    "UPDATE projects SET session_user_id = ?2, name = '默认项目', updated_at_ms = ?3
                     WHERE id = ?1",
                    params![id, uid, now],
                )?;
                log::info!(
                    "conversation_store: claimed legacy default project id={id} for session_user_id={uid}"
                );
                id
            } else {
                let existing_default: Option<String> = conn
                    .query_row(
                        "SELECT id FROM projects
                         WHERE session_user_id = ?1 AND is_default != 0
                         LIMIT 1",
                        params![uid],
                        |row| row.get(0),
                    )
                    .optional()?;
                match existing_default {
                    Some(id) => {
                        conn.execute(
                            "UPDATE projects SET name = '默认项目', workspace_root = ?2, updated_at_ms = ?3
                             WHERE id = ?1",
                            params![id, root, now],
                        )?;
                        id
                    }
                    None => {
                        let id = uuid::Uuid::new_v4().to_string();
                        conn.execute(
                            "INSERT INTO projects (id, name, workspace_root, is_default, is_pinned,
                                                  is_archived, created_at_ms, updated_at_ms, session_user_id)
                             VALUES (?1, '默认项目', ?2, 1, 0, 0, ?3, ?3, ?4)",
                            params![id, root, now, uid],
                        )?;
                        id
                    }
                }
            }
        }
    };

    conn.execute(
        "UPDATE projects SET is_default = 0
         WHERE session_user_id = ?1 AND is_default != 0 AND id != ?2",
        params![uid, project_id],
    )?;
    conn.execute(
        "UPDATE projects SET name = '默认项目', is_default = 1, workspace_root = ?2, updated_at_ms = ?3
         WHERE id = ?1",
        params![project_id, root, now],
    )?;
    conn.execute(
        "UPDATE conversations SET project_id = ?1
         WHERE workspace_root = ?2 AND trim(session_user_id) = ?3",
        params![project_id, root, uid],
    )?;
    log::info!(
        "conversation_store: reconciled default project session_user_id={uid} sandbox={root}"
    );
    Ok(())
}

fn project_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Project> {
    let updated_at = row.get(7)?;
    Ok(Project {
        id: row.get(0)?,
        name: row.get(1)?,
        workspace_root: row.get(2)?,
        is_default: row.get::<_, i64>(3)? != 0,
        is_pinned: row.get::<_, i64>(4)? != 0,
        is_archived: row.get::<_, i64>(5)? != 0,
        created_at: row.get(6)?,
        updated_at,
        session_user_id: row.get(8).unwrap_or_default(),
        last_activity_at: updated_at,
    })
}

fn normalize_workspace_root(root: &str) -> String {
    root.trim().trim_end_matches(['/', '\\']).to_string()
}

fn project_name_for_root(root: &str) -> String {
    let normalized = root.trim_end_matches(['/', '\\']);
    normalized
        .rsplit(['/', '\\'])
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("Default project")
        .to_string()
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
    add_column_if_missing(
        conn,
        "conversations",
        "workspace_inherit_disabled",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    add_column_if_missing(
        conn,
        "conversations",
        "im_last_interaction_at_ms",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    // v10: cron jobs track the active cron session id (advanced on daily
    // rollover). Existing rows backfill to NULL — the scheduler lazily sets it
    // on the next fire.
    add_column_if_missing(conn, "cron_jobs", "current_session_id", "TEXT")?;
    // v17: cron jobs carry an optional `deliver` string (Run → IM 出站总线).
    // Empty/NULL means no IM push after the run; otherwise the ImDeliverHook
    // parses it (e.g. "feishu", "feishu:ou_xxx", "feishu:group:chatid",
    // comma-separated, "all") and routes the final reply to IM. Backfills to
    // NULL for existing rows (no behavior change).
    add_column_if_missing(conn, "cron_jobs", "deliver", "TEXT")?;
    // v18: track last IM delivery failure for cron jobs (cleared on success).
    add_column_if_missing(conn, "cron_jobs", "last_delivery_error", "TEXT")?;
    // v19: one-shot schedules (Hermes-aligned soft-complete).
    add_column_if_missing(
        conn,
        "cron_jobs",
        "schedule_kind",
        "TEXT NOT NULL DEFAULT 'cron'",
    )?;
    add_column_if_missing(conn, "cron_jobs", "schedule_raw", "TEXT")?;
    webhook_sources::ensure_schema(conn)?;
    add_column_if_missing(conn, "webhook_sources", "auth_header_name", "TEXT")?;
    add_column_if_missing(
        conn,
        "webhook_sources",
        "session_mode",
        "TEXT NOT NULL DEFAULT 'per_delivery'",
    )?;
    add_column_if_missing(conn, "conversations", "last_lead_prompt_tokens", "INTEGER")?;
    add_column_if_missing(
        conn,
        "conversations",
        "session_user_id",
        "TEXT NOT NULL DEFAULT ''",
    )?;
    // v20: sidebar conversation pin state (pinned first, then updated_at).
    add_column_if_missing(
        conn,
        "conversations",
        "is_pinned",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    // v23: per-conversation performance tier override (Composer picker).
    // NULL = no override → global agentPerformanceModes default applies.
    add_column_if_missing(conn, "conversations", "performance_mode", "TEXT")?;
    // v24: lead agent thread id. One id per conversation; not rotated on agent switch.
    add_column_if_missing(conn, "conversations", "lead_agent_instance_id", "TEXT")?;
    // v21: materialized turn-anchor flag on messages. Backfill only touches
    // user rows that should be flagged (idempotent + store_meta gated);
    // afterwards writes compute it at insert time.
    add_column_if_missing(
        conn,
        "messages",
        "is_system_generated",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    crate::conversation_store::persist::backfill_is_system_generated(conn)?;
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_messages_conv_anchor
           ON messages(conversation_id, position) WHERE role = 'user' AND is_system_generated = 0;",
    )?;
    // v22: lead LLM working-set flag. Set-based JSON backfill (store_meta gated);
    // writes compute it at insert/upsert time.
    add_column_if_missing(
        conn,
        "messages",
        "context_included",
        "INTEGER NOT NULL DEFAULT 1",
    )?;
    crate::conversation_store::persist::backfill_context_included(conn)?;
    add_column_if_missing(conn, "messages", "tool_name", "TEXT")?;
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_messages_conv_included_pos
           ON messages(conversation_id, position) WHERE context_included = 1;",
    )?;
    // v25: materialize payload.agentInstanceId for instance-scoped recall.
    ensure_messages_agent_instance_id(conn)?;
    ensure_messages_is_scoped(conn)?;
    crate::conversation_store::persist::backfill_thread_context_identity(conn)?;
    conn.execute(
        "UPDATE conversations SET session_user_id = trim(session_user_id)
         WHERE session_user_id != trim(session_user_id)",
        [],
    )?;
    ensure_conversations_user_updated_index(conn)?;
    conn.execute(
        "UPDATE conversations SET im_last_interaction_at_ms = updated_at_ms
         WHERE im_last_interaction_at_ms = 0 AND updated_at_ms > 0",
        [],
    )?;
    conn.execute(
        "UPDATE schema_version SET version = ?1",
        params![SCHEMA_VERSION],
    )?;
    log::info!("conversation_store: migrated schema to v{}", SCHEMA_VERSION);
    Ok(())
}

fn ensure_conversations_user_updated_index(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_conversations_user_updated
         ON conversations(session_user_id, updated_at_ms DESC);",
    )?;
    Ok(())
}

fn ensure_conversations_pinned_updated_index(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_conversations_pinned_updated
           ON conversations(is_pinned DESC, updated_at_ms DESC, id DESC);
         CREATE INDEX IF NOT EXISTS idx_conversations_project_pinned_updated
           ON conversations(project_id, is_pinned DESC, updated_at_ms DESC, id DESC);",
    )?;
    Ok(())
}

fn ensure_messages_agent_instance_id(conn: &Connection) -> Result<()> {
    add_column_if_missing(conn, "messages", "agent_instance_id", "TEXT")?;
    crate::conversation_store::persist::backfill_agent_instance_id_column(conn)?;
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_messages_conv_instance_pos
           ON messages(conversation_id, agent_instance_id, position)
           WHERE agent_instance_id IS NOT NULL;
         CREATE INDEX IF NOT EXISTS idx_messages_agent_instance_id
           ON messages(agent_instance_id)
           WHERE agent_instance_id IS NOT NULL;",
    )?;
    Ok(())
}

fn ensure_messages_is_scoped(conn: &Connection) -> Result<()> {
    add_column_if_missing(conn, "messages", "is_scoped", "INTEGER NOT NULL DEFAULT 0")?;
    crate::conversation_store::persist::backfill_is_scoped(conn)?;
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_messages_conv_lead_pos
           ON messages(conversation_id, position) WHERE is_scoped = 0;",
    )?;
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
    const TOKENIZER: &str = "cjk_bigram";
    /// `role` is tokenized so sidebar MATCH can use `NOT {role}: tool`.
    const SCHEMA: &str = "role_indexed";

    let tokenizer: Option<String> = conn
        .query_row(
            "SELECT value FROM store_meta WHERE key = 'fts_tokenizer'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    let schema: Option<String> = conn
        .query_row(
            "SELECT value FROM store_meta WHERE key = 'fts_schema'",
            [],
            |row| row.get(0),
        )
        .optional()?;

    let schema_ok = tokenizer.as_deref() == Some(TOKENIZER)
        && schema.as_deref() == Some(SCHEMA)
        && fts_table_exists(conn)?;
    let index_empty = fts_docsize_empty(conn)?;
    let has_messages = messages_exist(conn)?;

    if schema_ok && !(index_empty && has_messages) {
        return Ok(());
    }

    if !schema_ok && fts_table_exists(conn)? {
        conn.execute_batch(
            "DROP TRIGGER IF EXISTS messages_ai;
             DROP TRIGGER IF EXISTS messages_ad;
             DROP TRIGGER IF EXISTS messages_au;
             DROP TABLE IF EXISTS messages_fts;",
        )?;
        log::info!("conversation_store: rebuilding FTS tokenizer={TOKENIZER} schema={SCHEMA}");
        create_fts_table(conn)?;
    } else if !fts_table_exists(conn)? {
        create_fts_table(conn)?;
    } else {
        log::warn!("conversation_store: FTS index empty while messages exist; rebuilding in place");
    }

    rebuild_fts_index(conn)?;
    if has_messages && fts_docsize_empty(conn)? {
        anyhow::bail!("FTS rebuild left an empty index while messages exist");
    }

    conn.execute(
        "INSERT INTO store_meta(key, value) VALUES ('fts_tokenizer', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![TOKENIZER],
    )?;
    conn.execute(
        "INSERT INTO store_meta(key, value) VALUES ('fts_schema', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![SCHEMA],
    )?;
    Ok(())
}

fn fts_docsize_empty(conn: &Connection) -> Result<bool> {
    if !fts_table_exists(conn)? {
        return Ok(true);
    }
    let n: i64 = conn
        .query_row("SELECT COUNT(*) FROM messages_fts_docsize", [], |row| {
            row.get(0)
        })
        .optional()?
        .unwrap_or(0);
    Ok(n == 0)
}

fn messages_exist(conn: &Connection) -> Result<bool> {
    let n: i64 = conn.query_row("SELECT EXISTS(SELECT 1 FROM messages LIMIT 1)", [], |row| {
        row.get(0)
    })?;
    Ok(n != 0)
}

fn rebuild_fts_index(conn: &Connection) -> Result<()> {
    let n = conn.execute(
        "INSERT INTO messages_fts(messages_fts) VALUES('rebuild')",
        [],
    )?;
    log::info!("conversation_store: FTS rebuilt from messages, rows={n}");
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
           role,
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
