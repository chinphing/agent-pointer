//! Bounded curated memory (MEMORY.md + USER.md) with frozen system-prompt snapshot.

use anyhow::{anyhow, Context, Result};
use parking_lot::{Mutex, RwLock};
use serde_json::{json, Value};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::user_storage::{memories_root_dir, user_storage_segment};

pub const ENTRY_DELIMITER: &str = "\n§\n";
pub const DEFAULT_MEMORY_CHAR_LIMIT: usize = 2200;
pub const DEFAULT_USER_CHAR_LIMIT: usize = 1375;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryTarget {
    Memory,
    User,
}

impl MemoryTarget {
    fn parse(s: &str) -> Option<Self> {
        match s.trim() {
            "memory" => Some(Self::Memory),
            "user" => Some(Self::User),
            _ => None,
        }
    }

    fn file_name(self) -> &'static str {
        match self {
            Self::Memory => "MEMORY.md",
            Self::User => "USER.md",
        }
    }

    fn block_title(self) -> &'static str {
        match self {
            Self::Memory => "MEMORY (agent notes)",
            Self::User => "USER PROFILE (who the user is)",
        }
    }
}

#[derive(Debug, Clone, Default)]
struct MemoryStoreInner {
    memory_entries: Vec<String>,
    user_entries: Vec<String>,
    snapshot_memory: String,
    snapshot_user: String,
}

pub struct MemoryStore {
    inner: RwLock<MemoryStoreInner>,
    memories_root: PathBuf,
    active_session_user_id: Mutex<String>,
}

impl MemoryStore {
    pub fn open_default() -> Result<Self> {
        let memories_root = memories_root_dir()?;
        Ok(Self {
            inner: RwLock::new(MemoryStoreInner::default()),
            memories_root,
            active_session_user_id: Mutex::new(String::new()),
        })
    }

    pub fn open_in_dir(memories_root: PathBuf) -> Self {
        let _ = fs::create_dir_all(&memories_root);
        Self {
            inner: RwLock::new(MemoryStoreInner::default()),
            memories_root,
            active_session_user_id: Mutex::new(String::new()),
        }
    }

    /// Switch the in-memory snapshot to one user's on-disk files.
    pub fn ensure_session_user(&self, session_user_id: &str) -> Result<()> {
        let key = session_user_id.trim().to_string();
        let mut active = self.active_session_user_id.lock();
        if *active == key {
            return Ok(());
        }
        *active = key.clone();
        drop(active);
        self.reload_snapshot_for(session_user_id)
    }

    pub fn reload_snapshot_for_conversation(&self, conversation_id: &str) -> Result<()> {
        let uid = crate::user_storage::session_user_id_for_conversation(conversation_id);
        self.ensure_session_user(&uid)
    }

    pub fn memories_dir(&self) -> PathBuf {
        let active = self.active_session_user_id.lock().clone();
        self.user_base_dir(&active)
    }

    fn user_base_dir(&self, session_user_id: &str) -> PathBuf {
        self.memories_root
            .join(user_storage_segment(session_user_id))
    }

    /// Load live entries from disk and rebuild the frozen system-prompt snapshot.
    pub fn reload_snapshot(&self) -> Result<()> {
        let active = self.active_session_user_id.lock().clone();
        self.reload_snapshot_for(&active)
    }

    fn reload_snapshot_for(&self, session_user_id: &str) -> Result<()> {
        let base_dir = self.user_base_dir(session_user_id);
        fs::create_dir_all(&base_dir)
            .with_context(|| format!("create memories dir {}", base_dir.display()))?;
        let memory_entries = Self::read_entries_with_legacy(
            &base_dir.join(MemoryTarget::Memory.file_name()),
            &self
                .memories_root
                .join(MemoryTarget::Memory.file_name()),
        )?;
        let user_entries = Self::read_entries_with_legacy(
            &base_dir.join(MemoryTarget::User.file_name()),
            &self.memories_root.join(MemoryTarget::User.file_name()),
        )?;
        let snapshot_memory = Self::render_snapshot_block(
            MemoryTarget::Memory,
            &memory_entries,
            DEFAULT_MEMORY_CHAR_LIMIT,
        );
        let snapshot_user = Self::render_snapshot_block(
            MemoryTarget::User,
            &user_entries,
            DEFAULT_USER_CHAR_LIMIT,
        );
        let mut g = self.inner.write();
        g.memory_entries = memory_entries;
        g.user_entries = user_entries;
        g.snapshot_memory = snapshot_memory;
        g.snapshot_user = snapshot_user;
        log::info!(
            "memory: reloaded snapshot memory_entries={} user_entries={}",
            g.memory_entries.len(),
            g.user_entries.len()
        );
        Ok(())
    }

    /// Cacheable system slices to append after `[Environment]`.
    pub fn snapshot_blocks(
        &self,
        memory_enabled: bool,
        user_profile_enabled: bool,
    ) -> Vec<String> {
        let g = self.inner.read();
        let mut out = Vec::new();
        if memory_enabled && !g.snapshot_memory.is_empty() {
            out.push(g.snapshot_memory.clone());
        }
        if user_profile_enabled && !g.snapshot_user.is_empty() {
            out.push(g.snapshot_user.clone());
        }
        out
    }

    pub fn dispatch_tool(&self, args: &Value) -> Result<String> {
        let action = args
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        let target = args
            .get("target")
            .and_then(|v| v.as_str())
            .unwrap_or("memory");
        let target = MemoryTarget::parse(target)
            .ok_or_else(|| anyhow!("Invalid target '{target}'. Use 'memory' or 'user'."))?;
        let content = args.get("content").and_then(|v| v.as_str());
        let old_text = args.get("old_text").and_then(|v| v.as_str());

        let memory_limit = args
            .get("_memory_char_limit")
            .and_then(|v| v.as_u64())
            .map(|n| n as usize)
            .unwrap_or(DEFAULT_MEMORY_CHAR_LIMIT);
        let user_limit = args
            .get("_user_char_limit")
            .and_then(|v| v.as_u64())
            .map(|n| n as usize)
            .unwrap_or(DEFAULT_USER_CHAR_LIMIT);

        let result = match action {
            "add" => {
                let content = content
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| anyhow!("Content is required for 'add'."))?;
                self.add(target, content, memory_limit, user_limit)?
            }
            "replace" => {
                let old_text = old_text
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| anyhow!("old_text is required for 'replace'."))?;
                let content = content
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| anyhow!("content is required for 'replace'."))?;
                self.replace(target, old_text, content, memory_limit, user_limit)?
            }
            "remove" => {
                let old_text = old_text
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| anyhow!("old_text is required for 'remove'."))?;
                self.remove(target, old_text)?
            }
            _ => {
                return Err(anyhow!(
                    "Unknown action '{action}'. Use: add, replace, remove."
                ))
            }
        };
        Ok(serde_json::to_string(&result)?)
    }

    fn add(
        &self,
        target: MemoryTarget,
        content: &str,
        memory_limit: usize,
        user_limit: usize,
    ) -> Result<Value> {
        let limit = self.char_limit(target, memory_limit, user_limit);
        let mut g = self.inner.write();
        self.reload_target_under_lock(&mut g, target)?;
        let entries = self.entries_mut(&mut g, target);
        if entries.iter().any(|e| e == content) {
            return Ok(self.success_response(target, entries, limit, Some("Entry already exists (no duplicate added).")));
        }
        let mut trial = entries.clone();
        trial.push(content.to_string());
        let new_total = Self::joined_len(&trial);
        if new_total > limit {
            let current = Self::joined_len(entries);
            return Ok(json!({
                "success": false,
                "error": format!(
                    "Memory at {current}/{limit} chars. Adding this entry ({} chars) would exceed the limit. Consolidate with replace/remove, then retry.",
                    content.chars().count()
                ),
                "current_entries": entries.clone(),
                "usage": format!("{current}/{limit}"),
            }));
        }
        entries.push(content.to_string());
        let out_entries = entries.clone();
        self.persist_under_lock(&g, target)?;
        Ok(self.success_response(target, &out_entries, limit, Some("Entry added.")))
    }

    fn replace(
        &self,
        target: MemoryTarget,
        old_text: &str,
        new_content: &str,
        memory_limit: usize,
        user_limit: usize,
    ) -> Result<Value> {
        let limit = self.char_limit(target, memory_limit, user_limit);
        let mut g = self.inner.write();
        self.reload_target_under_lock(&mut g, target)?;
        let entries = self.entries_mut(&mut g, target);
        let matches: Vec<usize> = entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.contains(old_text))
            .map(|(i, _)| i)
            .collect();
        if matches.is_empty() {
            return Ok(json!({
                "success": false,
                "error": format!("No entry matched '{old_text}'."),
            }));
        }
        if matches.len() > 1 {
            let unique: std::collections::HashSet<_> = matches.iter().map(|i| &entries[*i]).collect();
            if unique.len() > 1 {
                let previews: Vec<String> = matches
                    .iter()
                    .map(|i| truncate_preview(&entries[*i], 80))
                    .collect();
                return Ok(json!({
                    "success": false,
                    "error": format!("Multiple entries matched '{old_text}'. Be more specific."),
                    "matches": previews,
                }));
            }
        }
        let idx = matches[0];
        let mut trial = entries.clone();
        trial[idx] = new_content.to_string();
        let new_total = Self::joined_len(&trial);
        if new_total > limit {
            let current = Self::joined_len(entries);
            return Ok(json!({
                "success": false,
                "error": format!(
                    "Replacement would put memory at {new_total}/{limit} chars. Shorten or remove other entries first."
                ),
                "current_entries": entries.clone(),
                "usage": format!("{current}/{limit}"),
            }));
        }
        entries[idx] = new_content.to_string();
        let out_entries = entries.clone();
        self.persist_under_lock(&g, target)?;
        Ok(self.success_response(target, &out_entries, limit, Some("Entry replaced.")))
    }

    fn remove(&self, target: MemoryTarget, old_text: &str) -> Result<Value> {
        let limit = DEFAULT_MEMORY_CHAR_LIMIT; // usage string only
        let user_limit = DEFAULT_USER_CHAR_LIMIT;
        let mut g = self.inner.write();
        self.reload_target_under_lock(&mut g, target)?;
        let entries = self.entries_mut(&mut g, target);
        let matches: Vec<usize> = entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.contains(old_text))
            .map(|(i, _)| i)
            .collect();
        if matches.is_empty() {
            return Ok(json!({
                "success": false,
                "error": format!("No entry matched '{old_text}'."),
            }));
        }
        if matches.len() > 1 {
            let unique: std::collections::HashSet<_> = matches.iter().map(|i| &entries[*i]).collect();
            if unique.len() > 1 {
                let previews: Vec<String> = matches
                    .iter()
                    .map(|i| truncate_preview(&entries[*i], 80))
                    .collect();
                return Ok(json!({
                    "success": false,
                    "error": format!("Multiple entries matched '{old_text}'. Be more specific."),
                    "matches": previews,
                }));
            }
        }
        entries.remove(matches[0]);
        let out_entries = entries.clone();
        self.persist_under_lock(&g, target)?;
        let lim = self.char_limit(target, limit, user_limit);
        Ok(self.success_response(target, &out_entries, lim, Some("Entry removed.")))
    }

    fn success_response(
        &self,
        target: MemoryTarget,
        entries: &[String],
        limit: usize,
        message: Option<&str>,
    ) -> Value {
        let current = Self::joined_len(entries);
        let pct = if limit > 0 {
            ((current as f64 / limit as f64) * 100.0).min(100.0) as u32
        } else {
            0
        };
        let mut o = json!({
            "success": true,
            "target": match target { MemoryTarget::Memory => "memory", MemoryTarget::User => "user" },
            "entries": entries,
            "usage": format!("{pct}% — {current}/{limit} chars"),
            "entry_count": entries.len(),
        });
        if let Some(m) = message {
            o["message"] = json!(m);
        }
        o
    }

    fn path_for(&self, target: MemoryTarget) -> PathBuf {
        let active = self.active_session_user_id.lock().clone();
        self.user_base_dir(&active).join(target.file_name())
    }

    fn legacy_path_for(&self, target: MemoryTarget) -> PathBuf {
        self.memories_root.join(target.file_name())
    }

    fn char_limit(&self, target: MemoryTarget, memory_limit: usize, user_limit: usize) -> usize {
        match target {
            MemoryTarget::Memory => memory_limit,
            MemoryTarget::User => user_limit,
        }
    }

    fn entries_mut<'a>(
        &self,
        g: &'a mut MemoryStoreInner,
        target: MemoryTarget,
    ) -> &'a mut Vec<String> {
        match target {
            MemoryTarget::Memory => &mut g.memory_entries,
            MemoryTarget::User => &mut g.user_entries,
        }
    }

    fn reload_target_under_lock(&self, g: &mut MemoryStoreInner, target: MemoryTarget) -> Result<()> {
        let path = self.path_for(target);
        let legacy = self.legacy_path_for(target);
        if let Some(bak) = Self::detect_external_drift(&path, self.char_limit(target, DEFAULT_MEMORY_CHAR_LIMIT, DEFAULT_USER_CHAR_LIMIT))? {
            return Err(anyhow!(
                "Refusing to write {}: external drift detected. Backup: {}",
                path.display(),
                bak.display()
            ));
        }
        let fresh = Self::read_entries_with_legacy(&path, &legacy)?;
        match target {
            MemoryTarget::Memory => g.memory_entries = fresh,
            MemoryTarget::User => g.user_entries = fresh,
        }
        Ok(())
    }

    fn persist_under_lock(&self, g: &MemoryStoreInner, target: MemoryTarget) -> Result<()> {
        let entries = match target {
            MemoryTarget::Memory => &g.memory_entries,
            MemoryTarget::User => &g.user_entries,
        };
        Self::write_entries(&self.path_for(target), entries)
    }

    fn read_entries_with_legacy(user_path: &Path, legacy_path: &Path) -> Result<Vec<String>> {
        if user_path.exists() {
            return Self::read_entries(user_path);
        }
        if legacy_path.exists() {
            log::info!(
                "memory: using legacy root file {} for user dir {}",
                legacy_path.display(),
                user_path.parent().map(|p| p.display().to_string()).unwrap_or_default()
            );
            return Self::read_entries(legacy_path);
        }
        Ok(vec![])
    }

    fn read_entries(path: &Path) -> Result<Vec<String>> {
        if !path.exists() {
            return Ok(vec![]);
        }
        let raw = fs::read_to_string(path)
            .with_context(|| format!("read memory file {}", path.display()))?;
        if raw.trim().is_empty() {
            return Ok(vec![]);
        }
        let entries: Vec<String> = raw
            .split(ENTRY_DELIMITER)
            .map(str::trim)
            .filter(|e| !e.is_empty())
            .map(String::from)
            .collect();
        Ok(dedupe_preserve_order(entries))
    }

    fn write_entries(path: &Path, entries: &[String]) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = if entries.is_empty() {
            String::new()
        } else {
            entries.join(ENTRY_DELIMITER)
        };
        let tmp = path.with_extension("md.tmp");
        {
            let mut f = fs::File::create(&tmp)
                .with_context(|| format!("create temp memory file {}", tmp.display()))?;
            f.write_all(content.as_bytes())?;
            f.sync_all().ok();
        }
        fs::rename(&tmp, path).with_context(|| format!("atomic rename to {}", path.display()))?;
        Ok(())
    }

    fn render_snapshot_block(target: MemoryTarget, entries: &[String], limit: usize) -> String {
        if entries.is_empty() {
            return String::new();
        }
        let content = entries.join(ENTRY_DELIMITER);
        let current = content.chars().count();
        let pct = if limit > 0 {
            ((current as f64 / limit as f64) * 100.0).min(100.0) as u32
        } else {
            0
        };
        let sep = "═".repeat(46);
        format!(
            "{sep}\n{} [{pct}% — {current}/{limit} chars]\n{sep}\n{content}",
            target.block_title()
        )
    }

    fn joined_len(entries: &[String]) -> usize {
        if entries.is_empty() {
            return 0;
        }
        entries.join(ENTRY_DELIMITER).chars().count()
    }

    fn detect_external_drift(path: &Path, char_limit: usize) -> Result<Option<PathBuf>> {
        if !path.exists() {
            return Ok(None);
        }
        let raw = fs::read_to_string(path)?;
        if raw.trim().is_empty() {
            return Ok(None);
        }
        let parsed: Vec<String> = raw
            .split(ENTRY_DELIMITER)
            .map(str::trim)
            .filter(|e| !e.is_empty())
            .map(String::from)
            .collect();
        let roundtrip = parsed.join(ENTRY_DELIMITER);
        let max_entry = parsed.iter().map(|e| e.chars().count()).max().unwrap_or(0);
        let drift = raw.trim() != roundtrip || max_entry > char_limit;
        if !drift {
            return Ok(None);
        }
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let bak = path.with_extension(format!("md.bak.{ts}"));
        fs::copy(path, &bak)?;
        log::warn!(
            "memory: external drift on {}; backup at {}",
            path.display(),
            bak.display()
        );
        Ok(Some(bak))
    }
}

pub fn memories_dir() -> Result<PathBuf> {
    memories_root_dir()
}

fn dedupe_preserve_order(entries: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    entries
        .into_iter()
        .filter(|e| seen.insert(e.clone()))
        .collect()
}

fn truncate_preview(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    format!("{}...", s.chars().take(max).collect::<String>())
}

/// Count non-synthetic user turns in history (for memory review nudge).
pub fn count_real_user_turns(history: &[crate::models::ChatMessage]) -> u32 {
    history
        .iter()
        .filter(|m| matches!(m.role, crate::models::Role::User))
        .filter(|m| !crate::message_context::is_synthetic_user_content(&m.content))
        .count() as u32
}

pub fn memory_review_due(user_turns: u32, interval: u32) -> bool {
    interval > 0 && user_turns > 0 && user_turns % interval == 0
}

pub fn skill_review_due(cumulative_tool_iters: u32, interval: u32) -> bool {
    interval > 0 && cumulative_tool_iters > 0 && cumulative_tool_iters % interval == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ChatMessage, Role};

    #[test]
    fn add_replace_remove_roundtrip() {
        let store = MemoryStore::open_in_dir(std::env::temp_dir().join(format!(
            "pointer_mem_test_{}",
            uuid::Uuid::new_v4()
        )));
        store.reload_snapshot().unwrap();
        let add = store
            .dispatch_tool(&json!({"action": "add", "target": "memory", "content": "Prefers Rust"}))
            .unwrap();
        assert!(add.contains("\"success\":true"));
        let rep = store
            .dispatch_tool(&json!({
                "action": "replace",
                "target": "memory",
                "old_text": "Rust",
                "content": "Prefers Rust and TypeScript"
            }))
            .unwrap();
        assert!(rep.contains("\"success\":true"));
        let rem = store
            .dispatch_tool(&json!({
                "action": "remove",
                "target": "memory",
                "old_text": "TypeScript"
            }))
            .unwrap();
        assert!(rem.contains("\"success\":true"));
        store.reload_snapshot().unwrap();
        let blocks = store.snapshot_blocks(true, false);
        assert!(blocks.is_empty());
    }

    #[test]
    fn per_user_memory_dirs_are_isolated() {
        let root = std::env::temp_dir().join(format!("pointer_mem_users_{}", uuid::Uuid::new_v4()));
        let store = MemoryStore::open_in_dir(root.clone());
        store.ensure_session_user("user-a").unwrap();
        store
            .dispatch_tool(&json!({"action": "add", "target": "memory", "content": "A note"}))
            .unwrap();
        store.ensure_session_user("user-b").unwrap();
        store
            .dispatch_tool(&json!({"action": "add", "target": "memory", "content": "B note"}))
            .unwrap();
        store.ensure_session_user("user-a").unwrap();
        let blocks = store.snapshot_blocks(true, false);
        assert_eq!(blocks.len(), 1);
        assert!(blocks[0].contains("A note"));
        assert!(!blocks[0].contains("B note"));
        assert!(root.join(user_storage_segment("user-a")).join("MEMORY.md").is_file());
        assert!(root.join(user_storage_segment("user-b")).join("MEMORY.md").is_file());
    }

    #[test]
    fn review_due_every_n_turns() {
        assert!(!memory_review_due(0, 10));
        assert!(!memory_review_due(9, 10));
        assert!(memory_review_due(10, 10));
        assert!(!memory_review_due(11, 10));
    }

    #[test]
    fn count_skips_synthetic_users() {
        let history = vec![
            ChatMessage {
                id: "1".into(),
                role: Role::User,
                content: "hello".into(),
                status: "done".into(),
                created_at: 0,
                tool_calls: None,
                tool_call_id: None,
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
            },
            ChatMessage {
                id: "2".into(),
                role: Role::User,
                content: "[Conversation summary (auto-compression)] foo".into(),
                status: "done".into(),
                created_at: 0,
                tool_calls: None,
                tool_call_id: None,
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
            },
        ];
        assert_eq!(count_real_user_turns(&history), 1);
    }
}
