use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use crate::agents::register_builtin_agents;
use crate::extensions::ExtensionRegistry;
use crate::models::{
    ensure_agent_model_refs_have_provider, EffectiveSettingsView, ModelSettings, PlatformSettings,
    UserSettings,
};
use crate::platform_auth::{PlatformLoginCredentials, SharedPlatformAuth};
use crate::platform_config::{
    apply_login_llm_credentials, apply_login_llm_provider_api_keys, apply_login_media_oss,
    finalize_merged_settings,
    merge_platform_preferences, persist_local_platform_settings, PlatformConfigManager,
    SharedPlatformConfig,
};
use crate::skills::SkillRegistry;
use crate::storage;
use crate::tools::ToolRegistry;

fn open_conversation_store_with_fallback() -> Arc<crate::conversation_store::ConversationStore> {
    match crate::conversation_store::global_store() {
        Ok(store) => store,
        Err(e) => {
            log::warn!("conversation_store: open failed ({e:#}); using temp db");
            let temp_path = std::env::temp_dir().join(format!(
                "pointer-conversations-{}.db",
                uuid::Uuid::new_v4()
            ));
            match crate::conversation_store::ConversationStore::open(temp_path) {
                Ok(store) => Arc::new(store),
                Err(e2) => {
                    log::error!(
                        "conversation_store: temp db failed ({e2:#}); using in-memory db"
                    );
                    Arc::new(
                        crate::conversation_store::ConversationStore::open(
                            std::path::PathBuf::from(":memory:"),
                        )
                        .expect("conversation_store in-memory db"),
                    )
                }
            }
        }
    }
}

pub struct AppState {
    pub tools: Arc<ToolRegistry>,
    pub skills: Arc<SkillRegistry>,
    pub agents: Arc<crate::agents::AgentRegistry>,
    pub computer_state: Arc<crate::agents::computer::ComputerState>,
    pub platform_auth: SharedPlatformAuth,
    pub platform_config: SharedPlatformConfig,
    pub task_board_store: Arc<crate::task_board::TaskBoardStore>,
    pub memory_store: Arc<crate::memory::MemoryStore>,
    pub session_index: Arc<crate::session_search::SessionIndex>,
    /// Lifecycle hooks aligned with Python `call_extensions(extension_point, …)`.
    pub extensions: Arc<ExtensionRegistry>,
    pub cancels: Mutex<HashMap<String, CancellationToken>>,
    /// When set, the in-flight `terminal` tool for that conversation kills its subprocess (host-only; does not cancel the LLM turn).
    /// Outer key: conversation_id; inner key: tool_call_id.
    pub terminal_run_abort: Mutex<HashMap<String, HashMap<String, Arc<AtomicBool>>>>,
    pub approvals: Mutex<HashMap<String, oneshot::Sender<bool>>>,
    /// Blocks `run_subagent` → computer until the UI confirms monitor selection.
    pub monitor_picks: Mutex<HashMap<String, oneshot::Sender<Result<(), String>>>>,
    /// Pending terminal stdin submissions keyed by `request_id`.
    pub terminal_input_pending:
        Mutex<HashMap<String, std::sync::mpsc::Sender<crate::tools::terminal::TerminalInputResolution>>>,
    /// Active main-agent task board key per conversation.
    pub active_main_task_boards: Mutex<HashMap<String, String>>,
    /// Main task board anchor bindings: conversation -> (store_key -> user_message_id).
    pub task_board_anchor_by_store_key: Mutex<HashMap<String, HashMap<String, String>>>,
    /// Last user/chat activity for curator idle detection.
    pub last_activity_at: Mutex<Instant>,
    /// Prevents overlapping curator LLM passes.
    pub curator_llm_running: AtomicBool,
}

impl AppState {
    pub fn new() -> Self {
        crate::shell_env::bootstrap_process_path_from_login_shell();

        let platform_mgr = PlatformConfigManager::new();
        storage::ensure_legacy_settings_migrated();
        match storage::load_local_platform_settings() {
            Ok(Some(local)) => {
                log::info!("storage: loaded persisted agent settings from disk");
                platform_mgr.replace(local);
            }
            Ok(None) => {}
            Err(e) => log::warn!("storage: load local platform settings failed: {e}"),
        }

        let platform_config = platform_mgr.shared();

        let tools = Arc::new(ToolRegistry::new());
        let task_board_store = match crate::task_board::open_default_persistence() {
            Some(db) => {
                let wi_store = std::sync::Arc::new(crate::task_board::WorkItemStore::new());
                if let Some(wi_db) = crate::task_board::open_default_work_item_persistence() {
                    wi_store.set_persistence(Some(wi_db));
                }
                std::sync::Arc::new(crate::task_board::TaskBoardStore::with_persistence_and_work_items(
                    db,
                    wi_store,
                ))
            }
            None => {
                log::warn!("task_board: sqlite persistence unavailable; in-memory only");
                std::sync::Arc::new(crate::task_board::TaskBoardStore::new())
            }
        };
        let memory_store = match crate::memory::MemoryStore::open_default() {
            Ok(store) => {
                let arc = Arc::new(store);
                if let Err(e) = arc.reload_snapshot() {
                    log::warn!("memory: initial load failed: {e:#}");
                }
                arc
            }
            Err(e) => {
                log::warn!("memory: open failed ({e:#}); using empty in-memory store");
                let arc = Arc::new(crate::memory::MemoryStore::open_in_dir(
                    crate::storage::app_data_dir()
                        .unwrap_or_else(|_| std::env::temp_dir())
                        .join("memories"),
                ));
                if let Err(re) = arc.reload_snapshot() {
                    log::warn!("memory: fallback load failed: {re:#}");
                }
                arc
            }
        };
        crate::tools::builtin::register_all(&tools, task_board_store.clone());
        crate::memory::register_memory_tool(&tools, memory_store.clone());
        let session_index = open_conversation_store_with_fallback();
        crate::session_search::register_session_search_tool(&tools, session_index.clone());
        crate::tools::cron_job::register(&tools, session_index.clone());
        let skills = Arc::new(SkillRegistry::new());
        crate::skills::builtin::register_all(&skills);
        crate::tools::builtin::register_skill_tools(&tools, skills.clone());
        if let Err(err) = skills.reload_meta() {
            log::warn!("load external skills failed: {err}");
        }
        let user_settings = storage::load_user_settings().unwrap_or_default();
        crate::tools::parallel::ParallelLimits::from_settings(
            &platform_mgr.effective_model_settings(&user_settings),
        )
        .log_startup(&platform_mgr.effective_model_settings(&user_settings));
        let agents = Arc::new(crate::agents::AgentRegistry::new());
        register_builtin_agents(&agents);
        if let Err(err) = agents.reload_external() {
            log::warn!("load external agents failed: {err}");
        }
        let platform_auth = Arc::new(crate::platform_auth::PlatformAuthManager::new());
        let computer_state = Arc::new(crate::agents::computer::ComputerState::new(
            &agents,
            platform_auth.clone(),
            platform_config.clone(),
        ));
        crate::tools::builtin::register_computer_tools(&tools, computer_state.clone());
        let mut extension_registry = ExtensionRegistry::new();
        crate::extensions::register_builtin_extensions(&mut extension_registry);
        crate::platform_config::register_global_platform_config(platform_config.clone());
        Self {
            tools,
            skills,
            agents,
            computer_state,
            platform_auth,
            platform_config,
            task_board_store,
            memory_store,
            session_index,
            extensions: Arc::new(extension_registry),
            cancels: Mutex::new(HashMap::new()),
            terminal_run_abort: Mutex::new(HashMap::new()),
            approvals: Mutex::new(HashMap::new()),
            monitor_picks: Mutex::new(HashMap::new()),
            terminal_input_pending: Mutex::new(HashMap::new()),
            active_main_task_boards: Mutex::new(HashMap::new()),
            task_board_anchor_by_store_key: Mutex::new(HashMap::new()),
            last_activity_at: Mutex::new(Instant::now()),
            curator_llm_running: AtomicBool::new(false),
        }
    }

    pub fn touch_activity(&self) {
        *self.last_activity_at.lock() = Instant::now();
    }

    pub fn idle_duration(&self) -> std::time::Duration {
        self.last_activity_at.lock().elapsed()
    }

    pub fn start_background_tasks(self: &Arc<Self>) {
        crate::skills::curator::start_background_loop(self.clone());
    }

    /// Build a [`crate::dispatcher::RunDispatcher`] backed by this state.
    /// Hosts (Tauri / server) construct one dispatcher after wrapping
    /// `AppState` in `Arc` and share it across all trigger sources. The
    /// dispatcher reuses this `AppState` (and its `run_chat` entry), so the
    /// existing execution path is unchanged. Built-in lifecycle hooks
    /// (structured logging) are pre-registered.
    pub fn build_dispatcher(self: &Arc<Self>) -> crate::dispatcher::RunDispatcher {
        let mut hooks = crate::dispatcher::HookRegistry::new();
        crate::dispatcher::hooks::register_builtin_hooks(&mut hooks);
        crate::dispatcher::RunDispatcher::with_hooks(self.clone(), Arc::new(hooks))
    }

    pub fn load_user_settings(&self) -> UserSettings {
        storage::load_user_settings().unwrap_or_default()
    }

    /// Globally enabled skill ids for automated runs (IM, cron) that omit an explicit list.
    pub fn default_run_enabled_skill_ids(&self) -> Vec<String> {
        self.load_user_settings().enabled_skill_ids
    }

    pub fn active_platform_auth(&self) -> Arc<crate::platform_auth::PlatformAuthManager> {
        crate::web_request_auth::scoped_auth(&self.platform_auth)
    }

    pub fn save_user_settings(&self, user: &UserSettings) -> anyhow::Result<()> {
        storage::save_user_settings(user)
    }

    pub fn effective_settings(&self) -> crate::models::ModelSettings {
        let user = self.load_user_settings();
        let platform = self.platform_config.read().clone();
        let mut settings =
            finalize_merged_settings(crate::models::merge_user_platform(&user, &platform));
        if let Some(creds) = crate::web_request_auth::scoped_login_creds() {
            crate::platform_config::apply_login_credentials_to_model_settings(&mut settings, &creds);
        }
        settings
    }

    pub fn effective_settings_view(&self) -> EffectiveSettingsView {
        let user = self.load_user_settings();
        let platform = self.platform_config.read().clone();
        let merged = self.effective_settings();
        let is_platform_admin = self.active_platform_auth().is_platform_admin();
        EffectiveSettingsView {
            user,
            platform,
            merged,
            can_edit_platform: is_platform_admin,
            is_platform_admin,
        }
    }

    pub fn update_platform_settings(
        &self,
        mut patch: PlatformSettings,
    ) -> anyhow::Result<EffectiveSettingsView> {
        if !self.active_platform_auth().is_platform_admin() {
            anyhow::bail!("only platform admins may edit platform settings");
        }
        let mut tmp = crate::models::merge_user_platform(&UserSettings::default(), &patch);
        ensure_agent_model_refs_have_provider(&mut tmp);
        patch.agent_default_models = tmp.agent_default_models;
        *self.platform_config.write() = patch.clone();
        Ok(self.effective_settings_view())
    }

    pub fn update_agent_settings(
        &self,
        incoming: &ModelSettings,
    ) -> anyhow::Result<EffectiveSettingsView> {
        self.apply_session_platform_preferences(incoming)?;
        let platform = self.platform_config.read().clone();
        persist_local_platform_settings(&platform);
        Ok(self.effective_settings_view())
    }

    pub fn apply_session_platform_preferences(
        &self,
        incoming: &ModelSettings,
    ) -> anyhow::Result<()> {
        let current = self.platform_config.read().clone();
        let next = merge_platform_preferences(incoming, &current);
        *self.platform_config.write() = next;
        Ok(())
    }

    pub fn apply_login_credentials(&self, creds: &PlatformLoginCredentials) {
        let mut platform = self.platform_config.write();
        apply_login_llm_provider_api_keys(&mut platform, &creds.provider_api_keys);
        apply_login_llm_credentials(
            &mut platform,
            creds.api_key.as_deref(),
            creds.llm_provider.as_deref(),
        );
        if let Some(media) = creds.media_oss.as_ref() {
            apply_login_media_oss(&mut platform, Some(media));
        }
    }

    pub fn cancel(&self, conversation_id: &str) {
        if let Some(token) = self.cancels.lock().get(conversation_id) {
            token.cancel();
        }
        if self.abort_terminal_command(conversation_id, None) {
            log::info!(
                "cancel: aborted in-flight terminal commands conversation_id={conversation_id}"
            );
        }
        let approvals: Vec<_> = self.approvals.lock().drain().collect();
        for (_, tx) in approvals {
            let _ = tx.send(false);
        }
        let monitor_picks: Vec<_> = self.monitor_picks.lock().drain().collect();
        for (_, tx) in monitor_picks {
            let _ = tx.send(Err("已停止生成".into()));
        }
        let pending_inputs: Vec<_> = self.terminal_input_pending.lock().drain().collect();
        for (_, tx) in pending_inputs {
            let _ = tx.send(crate::tools::terminal::TerminalInputResolution::Cancelled);
        }
    }

    /// Kill the subprocess for a **`terminal`** tool invocation.
    /// When `tool_call_id` is set, only that invocation is aborted; otherwise all terminals in the conversation.
    /// Does **not** cancel the LLM stream or the rest of the turn. Returns **true** if at least one run was registered.
    pub fn abort_terminal_command(&self, conversation_id: &str, tool_call_id: Option<&str>) -> bool {
        let mut outer = self.terminal_run_abort.lock();
        let Some(inner) = outer.get_mut(conversation_id) else {
            return false;
        };
        match tool_call_id {
            Some(tc_id) => inner.get(tc_id).map(|f| {
                f.store(true, Ordering::SeqCst);
                true
            }).unwrap_or(false),
            None => {
                let mut any = false;
                for f in inner.values() {
                    f.store(true, Ordering::SeqCst);
                    any = true;
                }
                any
            }
        }
    }

    pub fn register_terminal_abort_flag(
        &self,
        conversation_id: &str,
        tool_call_id: &str,
        flag: Arc<AtomicBool>,
    ) {
        let mut outer = self.terminal_run_abort.lock();
        outer
            .entry(conversation_id.to_string())
            .or_default()
            .insert(tool_call_id.to_string(), flag);
    }

    pub fn clear_terminal_abort_flag(&self, conversation_id: &str, tool_call_id: &str) {
        let mut outer = self.terminal_run_abort.lock();
        if let Some(inner) = outer.get_mut(conversation_id) {
            inner.remove(tool_call_id);
            if inner.is_empty() {
                outer.remove(conversation_id);
            }
        }
    }

    pub fn approve_tool_call(&self, tool_call_id: &str, approved: bool) -> bool {
        if let Some(tx) = self.approvals.lock().remove(tool_call_id) {
            let _ = tx.send(approved);
            true
        } else {
            false
        }
    }

    pub fn confirm_computer_monitor_pick(&self, conversation_id: &str) -> bool {
        if let Some(tx) = self.monitor_picks.lock().remove(conversation_id) {
            let _ = tx.send(Ok(()));
            true
        } else {
            false
        }
    }

    pub fn cancel_computer_monitor_pick(&self, conversation_id: &str) -> bool {
        if let Some(tx) = self.monitor_picks.lock().remove(conversation_id) {
            let _ = tx.send(Err("屏幕选择已取消".into()));
            true
        } else {
            false
        }
    }

    pub fn register_terminal_input_wait(
        &self,
        request_id: String,
        tx: std::sync::mpsc::Sender<crate::tools::terminal::TerminalInputResolution>,
    ) {
        self.terminal_input_pending.lock().insert(request_id, tx);
    }

    pub fn submit_terminal_input(&self, request_id: &str, text: String) -> bool {
        if let Some(tx) = self.terminal_input_pending.lock().remove(request_id) {
            let _ = tx.send(crate::tools::terminal::TerminalInputResolution::Submit(text));
            true
        } else {
            false
        }
    }

    pub fn dismiss_terminal_input(&self, request_id: &str) -> bool {
        if let Some(tx) = self.terminal_input_pending.lock().remove(request_id) {
            let _ = tx.send(crate::tools::terminal::TerminalInputResolution::Dismiss);
            true
        } else {
            false
        }
    }

    pub fn set_main_task_board_binding(
        &self,
        conversation_id: &str,
        store_key: &str,
        anchor_message_id: &str,
    ) {
        if conversation_id.trim().is_empty()
            || store_key.trim().is_empty()
            || anchor_message_id.trim().is_empty()
        {
            log::warn!(
                "task_board_binding: invalid args conversation_id={} store_key={} anchor_message_id={}",
                conversation_id,
                store_key,
                anchor_message_id
            );
            return;
        }
        let mut map = self.task_board_anchor_by_store_key.lock();
        map.entry(conversation_id.to_string())
            .or_default()
            .insert(store_key.to_string(), anchor_message_id.to_string());
    }

    pub fn get_main_task_board_anchor(
        &self,
        conversation_id: &str,
        store_key: &str,
    ) -> Option<String> {
        self.task_board_anchor_by_store_key
            .lock()
            .get(conversation_id)
            .and_then(|m| m.get(store_key).cloned())
            .or_else(|| crate::task_board::anchor_message_id_from_main_turn_key(store_key))
    }

    pub fn clear_active_main_task_board_key(&self, conversation_id: &str) {
        if conversation_id.trim().is_empty() {
            return;
        }
        self.active_main_task_boards.lock().remove(conversation_id);
    }

    pub fn set_active_main_task_board_key(&self, conversation_id: &str, store_key: &str) {
        if conversation_id.trim().is_empty() || store_key.trim().is_empty() {
            return;
        }
        if crate::task_board::is_child_store_key(store_key) {
            log::warn!(
                "task_board_main_key: reject_child_active conversation_id={} store_key={}",
                conversation_id,
                store_key
            );
            return;
        }
        self.active_main_task_boards
            .lock()
            .insert(conversation_id.to_string(), store_key.to_string());
    }

    pub fn get_active_main_task_board_key(&self, conversation_id: &str) -> Option<String> {
        {
            let mut active = self.active_main_task_boards.lock();
            if let Some(k) = active.get(conversation_id).cloned() {
                if !crate::task_board::is_child_store_key(&k) {
                    return Some(k);
                }
                log::warn!(
                    "task_board_main_key: drop_invalid_active conversation_id={} store_key={}",
                    conversation_id,
                    k
                );
                active.remove(conversation_id);
            }
        }
        let prefix = format!(
            "{}{}",
            conversation_id.trim(),
            crate::task_board::coordination::main_turn::MAIN_TURN_KEY_SEP
        );
        let mut keys = self.task_board_store.list_store_keys_by_prefix(&prefix);
        keys.retain(|k| !crate::task_board::is_child_store_key(k));
        if !keys.is_empty() {
            let pick = keys
                .iter()
                .find(|k| {
                    let doc = self.task_board_store.document(k);
                    !matches!(
                        doc.meta.status,
                        crate::task_board::MetaStatus::Completed | crate::task_board::MetaStatus::Failed
                    )
                })
                .cloned()
                .unwrap_or_else(|| keys.remove(0));
            self.active_main_task_boards
                .lock()
                .insert(conversation_id.to_string(), pick.clone());
            return Some(pick);
        }
        let legacy = self.task_board_store.document(conversation_id);
        if !legacy.board_is_empty() || !legacy.meta.goal.trim().is_empty() {
            return Some(conversation_id.to_string());
        }
        None
    }
}

#[cfg(test)]
mod active_main_task_board_tests {
    use super::AppState;
    use crate::task_board::{
        main_turn_task_board_store_key, sub_agent_task_board_store_key,
    };
    use serde_json::json;

    fn seed_running_child_board(
        state: &AppState,
        conv: &str,
        turn: &str,
        task_id: &str,
    ) -> String {
        let parent = main_turn_task_board_store_key(conv, turn);
        let child = sub_agent_task_board_store_key(&parent, task_id);
        state
            .task_board_store
            .apply(
                &child,
                "init",
                &json!({
                    "goal": "child goal",
                    "items": [{"id": "m1", "title": "S", "status": "pending"}]
                }),
            )
            .expect("init child");
        child
    }

    #[test]
    fn get_active_main_task_board_key_skips_child_from_prefix_scan() {
        let state = AppState::new();
        let conv = "conv-active-child";
        seed_running_child_board(&state, conv, "user-1", "sub_task_x");
        assert_eq!(state.get_active_main_task_board_key(conv), None);
    }

    #[test]
    fn get_active_main_task_board_key_prefers_unfinished_parent_over_child() {
        let state = AppState::new();
        let conv = "conv-parent-wins";
        let turn = "user-1";
        let parent = main_turn_task_board_store_key(conv, turn);
        state
            .task_board_store
            .apply(
                &parent,
                "init",
                &json!({
                    "goal": "parent",
                    "items": [{"id": "m1", "title": "P", "status": "pending"}]
                }),
            )
            .expect("init parent");
        let child = seed_running_child_board(&state, conv, turn, "sub_task_y");
        assert_eq!(
            state.get_active_main_task_board_key(conv).as_deref(),
            Some(parent.as_str())
        );
        assert_ne!(
            state.get_active_main_task_board_key(conv).as_deref(),
            Some(child.as_str())
        );
    }

    #[test]
    fn set_active_main_task_board_key_rejects_child() {
        let state = AppState::new();
        let conv = "conv-reject-child";
        let parent = main_turn_task_board_store_key(conv, "user-1");
        let child = sub_agent_task_board_store_key(&parent, "sub1");
        state.set_active_main_task_board_key(conv, &child);
        assert_eq!(state.get_active_main_task_board_key(conv), None);
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}
