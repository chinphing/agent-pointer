//! Canonical SQLite conversation store (Hermes-style) with embedded FTS search.

pub mod app_secrets;
mod cjk_fts;
pub mod cron_jobs;
mod db;
pub mod im_session;
mod migrate;
mod persist;
pub mod runs;
mod search;
mod session_user;
#[cfg(test)]
mod tests;
pub mod webhook_sources;
mod write;

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use crate::models::{
    ChatMessage, Conversation, ConversationMeta, ConversationSearchHit, Project,
    ProjectCreationResult, ProjectCursor, ProjectPage,
};
use crate::storage::app_data_dir;

const DB_FILE: &str = "conversations.db";
const SCHEMA_VERSION: i32 = 20;

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

    /// Cursor-paginated meta-only list (no messages). Sort order is
    /// `(is_pinned DESC, updated_at_ms DESC, id DESC)`. Pass `None` for the first page.
    pub fn load_metas(
        &self,
        cursor: Option<persist::MetaCursor>,
        limit: i64,
    ) -> Result<Vec<ConversationMeta>> {
        let conn = self.db.conn.lock();
        persist::load_metas_from_conn(&conn, cursor, limit)
    }

    /// Load one conversation's meta by id (no messages). O(log n) via PK.
    pub fn load_meta(&self, id: &str) -> Result<Option<ConversationMeta>> {
        let conn = self.db.conn.lock();
        persist::load_meta_from_conn(&conn, id)
    }

    pub fn load_projects(&self, cursor: Option<ProjectCursor>, limit: i64) -> Result<ProjectPage> {
        let conn = self.db.conn.lock();
        persist::load_project_page_from_conn(&conn, cursor, limit)
    }

    pub fn load_sidebar_projects(&self) -> Result<Vec<Project>> {
        let conn = self.db.conn.lock();
        persist::load_sidebar_projects_from_conn(&conn)
    }

    pub fn load_project(&self, id: &str) -> Result<Option<Project>> {
        let conn = self.db.conn.lock();
        persist::load_project_from_conn(&conn, id)
    }

    pub fn load_project_metas(
        &self,
        project_id: &str,
        cursor: Option<persist::MetaCursor>,
        limit: i64,
    ) -> Result<Vec<ConversationMeta>> {
        let conn = self.db.conn.lock();
        persist::load_project_metas_from_conn(&conn, project_id, cursor, limit)
    }

    pub fn create_project(
        &self,
        name: &str,
        workspace_root: &str,
    ) -> Result<ProjectCreationResult> {
        let name = name.trim();
        if name.is_empty() {
            anyhow::bail!("project name is required");
        }
        let workspace_root = normalize_workspace_root(workspace_root);
        if workspace_root.is_empty() {
            anyhow::bail!("project workspace root is required");
        }
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
        };
        let mut result = self.db.execute_write(|conn| {
            let existing = conn.query_row(
                "SELECT id, name, workspace_root, is_default, is_pinned, is_archived,
                        created_at_ms, updated_at_ms
                 FROM projects
                 WHERE rtrim(trim(workspace_root), '/\\') = ?1
                 ORDER BY is_archived ASC, updated_at_ms DESC, id DESC
                 LIMIT 1",
                params![workspace_root],
                project_from_row,
            ).optional()?;
            if let Some(existing) = existing {
                return Ok(ProjectCreationResult { project: existing, reused_existing: true });
            }
            conn.execute(
                "INSERT INTO projects (id, name, workspace_root, is_default, is_pinned, is_archived, created_at_ms, updated_at_ms)
                 VALUES (?1, ?2, ?3, 0, 0, 0, ?4, ?4)",
                params![project.id, project.name, project.workspace_root, now],
            )?;
            Ok(ProjectCreationResult { project: project.clone(), reused_existing: false })
        })?;
        if result.reused_existing {
            let conn = self.db.conn.lock();
            if let Some(project) = persist::load_project_from_conn(&conn, &result.project.id)? {
                result.project = project;
            }
            log::info!(
                "conversation_store: reused project id={}",
                result.project.id
            );
        } else {
            log::info!(
                "conversation_store: created project id={}",
                result.project.id
            );
        }
        Ok(result)
    }

    pub fn update_project(
        &self,
        id: &str,
        name: Option<&str>,
        workspace_root: Option<&str>,
        is_pinned: Option<bool>,
        is_archived: Option<bool>,
    ) -> Result<Project> {
        if is_archived == Some(true) {
            let conn = self.db.conn.lock();
            let is_default: Option<i64> = conn
                .query_row(
                    "SELECT is_default FROM projects WHERE id = ?1",
                    params![id],
                    |row| row.get(0),
                )
                .optional()?;
            if is_default == Some(1) {
                anyhow::bail!("default project cannot be archived");
            }
        }
        let now = chrono::Utc::now().timestamp_millis();
        self.db.execute_write(|conn| {
            let changed = conn.execute(
                "UPDATE projects SET
                   name = COALESCE(?2, name), workspace_root = COALESCE(?3, workspace_root),
                   is_pinned = COALESCE(?4, is_pinned), is_archived = COALESCE(?5, is_archived),
                   updated_at_ms = ?6 WHERE id = ?1",
                params![
                    id,
                    name.map(str::trim),
                    workspace_root.map(str::trim),
                    is_pinned.map(i64::from),
                    is_archived.map(i64::from),
                    now
                ],
            )?;
            if changed == 0 {
                anyhow::bail!("project not found");
            }
            Ok(())
        })?;
        let conn = self.db.conn.lock();
        persist::load_project_from_conn(&conn, id)?
            .ok_or_else(|| anyhow::anyhow!("project not found"))
    }

    pub fn delete_project(&self, id: &str) -> Result<()> {
        self.db.execute_write(|conn| {
            let default: Option<i64> = conn
                .query_row(
                    "SELECT is_default FROM projects WHERE id = ?1",
                    params![id],
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
            conn.execute("DELETE FROM projects WHERE id = ?1", params![id])?;
            log::info!("conversation_store: deleted project id={id} conversations={count}");
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

    pub fn save_all(&self, list: &[Conversation]) -> Result<()> {
        // Snapshot old IDs before the write so we can detect deletions.
        let old_ids: Vec<String> = self.list_all_ids().unwrap_or_default();
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
            reconcile_default_project_from_sandbox(conn)
        })?;
        Ok(())
    }

    /// Explicitly delete one conversation and its messages, and clean up its
    /// sandbox directory. Use this instead of relying on `save_meta_all` to
    /// diff against a (now paginated, incomplete) in-memory list.
    pub fn delete_conversation(&self, id: &str) -> Result<()> {
        self.db
            .execute_write(|conn| persist::delete_conversation_from_conn(conn, id))?;
        log::info!("conversation_store: deleted conversation id={id}");
        if let Err(e) = crate::session_sandbox::SessionSandbox::cleanup_for_conversation(id) {
            log::warn!("session_sandbox cleanup failed for {id}: {e}");
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

    /// Soft-exclude payloads + shift suffix + insert summary at the cut point.
    pub fn persist_context_compression(
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
        })
    }

    /// P2b: replace full transcript from client-held messages.
    pub fn replace_messages(&self, conversation_id: &str, messages: &[ChatMessage]) -> Result<()> {
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
        search::dispatch_tool(&self.db, args)
    }

    /// Sidebar search: FTS over full message bodies (+ title/preview supplement).
    pub fn search_conversations(
        &self,
        query: &str,
        limit: i64,
    ) -> Result<Vec<ConversationSearchHit>> {
        search::search_conversations_for_ui(&self.db, query, limit)
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
           im_session_epoch INTEGER NOT NULL DEFAULT 0,
           im_active_conversation_id TEXT,
           im_last_interaction_at_ms INTEGER NOT NULL DEFAULT 0,
           session_user_id TEXT NOT NULL DEFAULT '',
           is_pinned INTEGER NOT NULL DEFAULT 0
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
               updated_at_ms INTEGER NOT NULL
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
    reconcile_default_project_from_sandbox(conn)?;
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_conversations_project_updated
           ON conversations(project_id, updated_at_ms DESC);",
    )?;
    Ok(())
}

fn migrate_projects_from_conversations(conn: &Connection) -> Result<()> {
    let default_root = default_sandbox_root_from_conversations(conn)?;
    let now = chrono::Utc::now().timestamp_millis();
    let default_id = uuid::Uuid::new_v4().to_string();
    let default_name = "默认项目".to_string();
    conn.execute(
        "INSERT INTO projects (id, name, workspace_root, is_default, is_pinned, is_archived, created_at_ms, updated_at_ms)
         VALUES (?1, ?2, ?3, 1, 0, 0, ?4, ?4)",
        params![default_id, default_name, default_root, now],
    )?;
    let mut roots = std::collections::BTreeSet::new();
    let mut stmt = conn.prepare(
        "SELECT DISTINCT trim(workspace_root) FROM conversations
         WHERE id NOT LIKE 'cron:%' AND id NOT LIKE 'webhook:%' AND id NOT LIKE 'im:%'
           AND trim(workspace_root) != '' AND trim(workspace_root) != ?1",
    )?;
    for root in stmt.query_map(params![default_root], |row| row.get::<_, String>(0))? {
        roots.insert(root?);
    }
    let additional_projects = roots.len();
    for root in roots {
        let id = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO projects (id, name, workspace_root, is_default, is_pinned, is_archived, created_at_ms, updated_at_ms)
             VALUES (?1, ?2, ?3, 0, 0, 0, ?4, ?4)",
            params![id, project_name_for_root(&root), root, now],
        )?;
    }
    let assigned_default = conn.execute(
        "UPDATE conversations SET project_id = ?1
         WHERE id NOT LIKE 'cron:%' AND id NOT LIKE 'webhook:%' AND id NOT LIKE 'im:%'
           AND (trim(workspace_root) = '' OR trim(workspace_root) = ?2)",
        params![default_id, default_root],
    )?;
    let assigned_other = conn.execute(
        "UPDATE conversations SET project_id = (
           SELECT id FROM projects p WHERE p.workspace_root = trim(conversations.workspace_root)
         )
         WHERE id NOT LIKE 'cron:%' AND id NOT LIKE 'webhook:%' AND id NOT LIKE 'im:%'
           AND trim(workspace_root) != '' AND trim(workspace_root) != ?1",
        params![default_root],
    )?;
    log::info!(
        "conversation_store: project migration default_root={} projects={} assigned_default={} assigned_other={}",
        default_root, additional_projects + 1, assigned_default, assigned_other
    );
    Ok(())
}

/// Derive the active user's default workspace from persisted session identity,
/// never from the global workspace preference. A signed-in user shares
/// `{session-sandboxes}/{session_user_id}` across their default conversations.
fn default_sandbox_root_from_conversations(conn: &Connection) -> Result<String> {
    let session_user_id: Option<String> = conn
        .query_row(
            "SELECT trim(session_user_id) FROM conversations
             WHERE trim(session_user_id) != ''
               AND id NOT LIKE 'cron:%' AND id NOT LIKE 'webhook:%' AND id NOT LIKE 'im:%'
             ORDER BY updated_at_ms DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()?;
    let Some(session_user_id) = session_user_id else {
        return Ok(String::new());
    };
    Ok(
        crate::session_sandbox::SessionSandbox::default_path("", &session_user_id)?
            .to_string_lossy()
            .into_owned(),
    )
}

/// Repair projects created by early builds that treated the default user
/// sandbox as an ordinary directory project. This is idempotent and preserves
/// all conversations and local files.
fn reconcile_default_project_from_sandbox(conn: &Connection) -> Result<()> {
    let root = default_sandbox_root_from_conversations(conn)?;
    if root.is_empty() {
        return Ok(());
    }
    let now = chrono::Utc::now().timestamp_millis();
    let project_id: Option<String> = conn
        .query_row(
            "SELECT id FROM projects WHERE workspace_root = ?1 LIMIT 1",
            params![root],
            |row| row.get(0),
        )
        .optional()?;
    let project_id = match project_id {
        Some(id) => id,
        None => {
            let existing_default: Option<String> = conn
                .query_row(
                    "SELECT id FROM projects WHERE is_default != 0 LIMIT 1",
                    [],
                    |row| row.get(0),
                )
                .optional()?;
            let Some(id) = existing_default else {
                return Ok(());
            };
            conn.execute(
                "UPDATE projects SET name = '默认项目', workspace_root = ?2, updated_at_ms = ?3 WHERE id = ?1",
                params![id, root, now],
            )?;
            id
        }
    };
    conn.execute(
        "UPDATE projects SET is_default = 0 WHERE is_default != 0",
        [],
    )?;
    conn.execute(
        "UPDATE projects SET name = '默认项目', is_default = 1, updated_at_ms = ?2 WHERE id = ?1",
        params![project_id, now],
    )?;
    conn.execute(
        "UPDATE conversations SET project_id = ?1
         WHERE workspace_root = ?2",
        params![project_id, root],
    )?;
    log::info!("conversation_store: reconciled default project sandbox={root}");
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
    // The FTS table is an external-content table (`content='messages'`); creating
    // it leaves the index empty, so existing messages (e.g. after a restore or
    // corruption recovery) would not be searchable. Repopulate from `messages`.
    // This only runs when the table was (re)created, not on every open.
    match conn.execute(
        "INSERT INTO messages_fts(messages_fts) VALUES('rebuild')",
        [],
    ) {
        Ok(n) => log::info!("conversation_store: FTS rebuilt from messages, rows={n}"),
        Err(e) => log::warn!("conversation_store: FTS rebuild failed: {e}"),
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
