//! Unified in-process conversation session facade.
//!
//! Business code that mutates a conversation transcript should go through this
//! module (not `ConversationStore` message writes directly). The facade owns:
//! - lead LLM working-set cache (cross-`run_chat`)
//! - generation tokens bumped on every transcript mutation
//! - DB persistence via [`crate::conversation_store`]
//!
//! UI paging / FTS / meta listing may still read SQLite directly.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::OnceLock;

use anyhow::Result;
use parking_lot::Mutex;

use crate::conversation_store::{self, conversation_preview};
use crate::models::ChatMessage;

const MAX_CACHED_CONVERSATIONS: usize = 16;

#[derive(Debug, Clone)]
struct WorkingSetEntry {
    generation: u64,
    db_count: u32,
    working: Vec<ChatMessage>,
}

struct SessionHub {
    /// Monotonic per-conversation generation; bumped on any transcript write.
    generations: HashMap<String, u64>,
    cache: HashMap<String, WorkingSetEntry>,
    /// LRU order (front = oldest).
    lru: VecDeque<String>,
}

impl SessionHub {
    fn new() -> Self {
        Self {
            generations: HashMap::new(),
            cache: HashMap::new(),
            lru: VecDeque::new(),
        }
    }

    fn generation(&self, conversation_id: &str) -> u64 {
        self.generations.get(conversation_id).copied().unwrap_or(0)
    }

    fn bump(&mut self, conversation_id: &str) -> u64 {
        let g = self.generation(conversation_id).saturating_add(1);
        self.generations.insert(conversation_id.to_string(), g);
        // Any mutation invalidates the cached working set for that id.
        self.cache.remove(conversation_id);
        self.lru.retain(|id| id != conversation_id);
        g
    }

    fn touch_lru(&mut self, conversation_id: &str) {
        self.lru.retain(|id| id != conversation_id);
        self.lru.push_back(conversation_id.to_string());
        while self.lru.len() > MAX_CACHED_CONVERSATIONS {
            if let Some(old) = self.lru.pop_front() {
                self.cache.remove(&old);
                log::info!("conversation_session: evicted working-set cache conversation_id={old}");
            }
        }
    }

    fn put_cache(&mut self, conversation_id: &str, entry: WorkingSetEntry) {
        self.touch_lru(conversation_id);
        self.cache.insert(conversation_id.to_string(), entry);
    }

    fn drop_conversation(&mut self, conversation_id: &str) {
        self.generations.remove(conversation_id);
        self.cache.remove(conversation_id);
        self.lru.retain(|id| id != conversation_id);
    }
}

fn hub() -> &'static Mutex<SessionHub> {
    static HUB: OnceLock<Mutex<SessionHub>> = OnceLock::new();
    HUB.get_or_init(|| Mutex::new(SessionHub::new()))
}

/// Called from [`crate::conversation_store`] write paths so every transcript
/// mutation invalidates the facade cache (including external crate callers).
pub fn note_transcript_mutated(conversation_id: &str) {
    let id = conversation_id.trim();
    if id.is_empty() {
        return;
    }
    let g = hub().lock().bump(id);
    log::debug!("conversation_session: generation bump conversation_id={id} generation={g}");
}

/// Drop cache + generation when a conversation is deleted.
pub fn drop_conversation(conversation_id: &str) {
    let id = conversation_id.trim();
    if id.is_empty() {
        return;
    }
    hub().lock().drop_conversation(id);
    log::info!("conversation_session: dropped conversation_id={id}");
}

/// Current generation (0 if never mutated in this process).
pub fn transcript_generation(conversation_id: &str) -> u64 {
    hub().lock().generation(conversation_id.trim())
}

fn filter_lead_working_history(
    messages: impl IntoIterator<Item = ChatMessage>,
) -> Vec<ChatMessage> {
    messages
        .into_iter()
        .filter(|m| {
            !crate::models::is_scoped_sub_message(m)
                && crate::message_context::is_context_included(m)
        })
        .collect()
}

fn meta_message_count(conversation_id: &str, working_len: u32) -> u32 {
    conversation_store::global_store()
        .ok()
        .and_then(|store| store.message_count(conversation_id).ok())
        .map(|db| db.max(working_len))
        .unwrap_or(working_len)
}

/// Append rows missing from SQLite, then refresh meta. Preferred write entry.
pub fn append_missing(
    conversation_id: &str,
    messages: &[ChatMessage],
) -> Result<Vec<conversation_store::AppendedMessageRow>> {
    let store = conversation_store::global_store()?;
    let appended = store.append_missing_messages(conversation_id, messages)?;
    if !appended.is_empty() {
        let count = store.message_count(conversation_id)?;
        let preview = conversation_preview(messages);
        store.flush_conversation_meta(conversation_id, count, &preview)?;
        log::info!(
            "conversation_session: append_missing conversation_id={conversation_id} written={} db_count={count}",
            appended.len()
        );
    }
    Ok(appended)
}

/// Upsert one message and keep meta roughly in sync.
pub fn upsert_message(conversation_id: &str, msg: &ChatMessage) -> Result<()> {
    let store = conversation_store::global_store()?;
    upsert_message_in_store(&store, conversation_id, msg)
}

/// Same as [`upsert_message`] but through an explicit store handle, so
/// callers holding a non-global store (tests, isolated DBs) stay consistent.
pub fn upsert_message_in_store(
    store: &conversation_store::ConversationStore,
    conversation_id: &str,
    msg: &ChatMessage,
) -> Result<()> {
    store.upsert_message_no_refresh(conversation_id, msg)?;
    let count = store.message_count(conversation_id)?;
    let preview = match store.stored_conversation_preview(conversation_id) {
        Ok(p) if !p.is_empty() => p,
        _ => conversation_preview(std::slice::from_ref(msg)),
    };
    store.flush_conversation_meta(conversation_id, count, &preview)?;
    Ok(())
}

/// Ordered sync of the lead working-set list (preserve soft-excluded orphans).
pub fn sync_ordered(conversation_id: &str, history: &[ChatMessage]) -> Result<()> {
    let store = conversation_store::global_store()?;
    let preview = conversation_preview(history);
    let count = meta_message_count(conversation_id, history.len() as u32);
    store.sync_messages_ordered_with_meta(conversation_id, history, count, &preview)?;
    let gen = transcript_generation(conversation_id);
    let db_count = store.message_count(conversation_id).unwrap_or(count);
    hub().lock().put_cache(
        conversation_id,
        WorkingSetEntry {
            generation: gen,
            db_count,
            working: history.to_vec(),
        },
    );
    Ok(())
}

/// Persist compression splice; cache is invalidated via store write bump, then
/// callers should publish the post-drain working set with [`publish_working_set`].
pub fn persist_compression_splice(
    conversation_id: &str,
    excluded_messages: &[ChatMessage],
    summary: &ChatMessage,
    insert_before_message_id: &str,
    preview: &str,
) -> Result<()> {
    let store = conversation_store::global_store()?;
    store.persist_context_compression(
        conversation_id,
        excluded_messages,
        summary,
        insert_before_message_id,
        preview,
    )?;
    Ok(())
}

/// Replace the cached lead working set after a successful in-memory drain/sync.
pub fn publish_working_set(conversation_id: &str, working: &[ChatMessage], db_count: Option<u32>) {
    let id = conversation_id.trim();
    if id.is_empty() {
        return;
    }
    let gen = transcript_generation(id);
    let db_count = db_count.unwrap_or_else(|| meta_message_count(id, working.len() as u32));
    hub().lock().put_cache(
        id,
        WorkingSetEntry {
            generation: gen,
            db_count,
            working: working.to_vec(),
        },
    );
    log::debug!(
        "conversation_session: published working set conversation_id={id} generation={gen} working={} db_count={db_count}",
        working.len()
    );
}

/// Canonicalize lead history for `run_chat`: append caller deltas, then serve
/// from working-set cache when generation still matches, else reload from DB.
pub fn prepare_lead_history(conversation_id: &str, history: &mut Vec<ChatMessage>) -> Result<u32> {
    let Ok(store) = conversation_store::global_store() else {
        *history = filter_lead_working_history(std::mem::take(history));
        log::warn!(
            "conversation_session: store unavailable; filtered caller history conversation_id={conversation_id} working_history={}",
            history.len()
        );
        return Ok(history.len() as u32);
    };

    let gen_before = transcript_generation(conversation_id);
    let cached = hub().lock().cache.get(conversation_id).cloned();

    let appended = store.append_missing_messages(conversation_id, history)?;
    let gen_after = transcript_generation(conversation_id);

    if appended.is_empty() {
        if let Some(entry) = cached {
            if entry.generation == gen_before && entry.generation == gen_after {
                *history = entry.working;
                hub().lock().touch_lru(conversation_id);
                log::info!(
                    "conversation_session: working-set cache hit conversation_id={conversation_id} db_messages={} working_history={} generation={}",
                    entry.db_count,
                    history.len(),
                    gen_after
                );
                return Ok(entry.db_count);
            }
        }
    } else if let Some(entry) = cached {
        // Incremental: reuse prior working set and append newly persisted lead rows.
        if entry.generation == gen_before {
            let mut working = entry.working;
            let known: HashSet<String> = working.iter().map(|m| m.id.clone()).collect();
            let mut added = 0u32;
            for msg in history.iter() {
                if known.contains(&msg.id) {
                    continue;
                }
                if !crate::message_context::is_context_included(msg) {
                    continue;
                }
                if crate::models::is_scoped_sub_message(msg) {
                    continue;
                }
                working.push(msg.clone());
                added += 1;
            }
            let db_count = store
                .message_count(conversation_id)
                .unwrap_or(entry.db_count.saturating_add(appended.len() as u32));
            *history = working;
            publish_working_set(conversation_id, history, Some(db_count));
            log::info!(
                "conversation_session: working-set cache incremental conversation_id={conversation_id} written={} added_working={added} db_messages={db_count} working_history={}",
                appended.len(),
                history.len()
            );
            return Ok(db_count);
        }
    }

    let (working, db_count) = store.load_lead_working_messages(conversation_id)?;
    if working.is_empty() && db_count == 0 && !history.is_empty() {
        anyhow::bail!(
            "conversation session reload returned empty after appending {} message(s)",
            history.len()
        );
    }
    *history = working;
    publish_working_set(conversation_id, history, Some(db_count));
    log::info!(
        "conversation_session: working-set cache miss reload conversation_id={conversation_id} db_messages={} working_history={} generation={} written={}",
        db_count,
        history.len(),
        transcript_generation(conversation_id),
        appended.len()
    );
    Ok(db_count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversation_store::ConversationStore;
    use crate::models::{ChatMessage, Conversation, ExcludedReason, MessageContextState, Role};
    use tempfile::TempDir;

    fn sample_conv(id: &str) -> Conversation {
        Conversation {
            id: id.into(),
            title: "T".into(),
            created_at: 1,
            updated_at: 2,
            is_pinned: false,
            messages: vec![
                ChatMessage::user_text("hello"),
                ChatMessage {
                    id: "a1".into(),
                    role: Role::Assistant,
                    content: "ok".into(),
                    status: "done".into(),
                    created_at: 2,
                    tool_calls: None,
                    tool_call_id: None,
                    tool_name: None,
                    error_message: None,
                    reasoning: None,
                    thoughts: None,
                    headline: None,
                    raw_content: None,
                    tool_raw_output: None,
                    agent_id: None,
                    agent_instance_id: None,
                    agent_name: None,
                    agent_trace: None,
                    image_slot_labels: None,
                    images_base64: None,
                    computer_round_screen_rel_path: None,
                    ui_bindings: None,
                    context_state: None,
                    attachments: None,
                    anchor_message_id: None,
                    trace_id: None,
                    task_id: None,
                    spawn_depth: None,
                    agent_chain: None,
                },
            ],
            skill_ids: vec![],
            tool_rounds_used: 0,
            tool_rounds_used_supervisor: 0,
            computer_monitor_id: None,
            project_id: None,
            workspace_root: String::new(),
            workspace_user_set: false,
            workspace_inherit_disabled: false,
            lead_agent_id: crate::agents::DEFAULT_LEAD_AGENT_ID.to_string(),
            agent_mode: crate::agents::AGENT_MODE_SINGLE.to_string(),
            performance_mode: None,
            session_user_id: String::new(),
        }
    }

    #[test]
    fn bump_invalidates_cache_entry() {
        let mut hub = SessionHub::new();
        hub.put_cache(
            "c1",
            WorkingSetEntry {
                generation: 0,
                db_count: 1,
                working: vec![],
            },
        );
        assert!(hub.cache.contains_key("c1"));
        hub.bump("c1");
        assert!(!hub.cache.contains_key("c1"));
        assert_eq!(hub.generation("c1"), 1);
    }

    #[test]
    fn store_upsert_bumps_and_clears_published_cache() {
        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        let mut conv = sample_conv("sess-bump");
        // Stable ids for upsert.
        conv.messages[0].id = "u1".into();
        store.save_all(&[conv.clone()]).unwrap();

        let (working, db_count) = store.load_lead_working_messages("sess-bump").unwrap();
        let gen = transcript_generation("sess-bump");
        hub().lock().put_cache(
            "sess-bump",
            WorkingSetEntry {
                generation: gen,
                db_count,
                working,
            },
        );
        assert!(hub().lock().cache.contains_key("sess-bump"));

        conv.messages[0].context_state = Some(MessageContextState {
            included: false,
            excluded_reason: Some(ExcludedReason::ContextCompression),
        });
        store
            .upsert_message_no_refresh("sess-bump", &conv.messages[0])
            .unwrap();
        assert!(hub().lock().cache.get("sess-bump").is_none());
        assert!(transcript_generation("sess-bump") > gen);
    }
}
