use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
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
    apply_login_llm_credentials, apply_login_llm_provider_api_keys, finalize_merged_settings,
    merge_platform_preferences, persist_local_platform_settings, PlatformConfigManager,
    SharedPlatformConfig,
};
use crate::skills::SkillRegistry;
use crate::storage;
use crate::tools::ToolRegistry;

pub struct AppState {
    pub tools: Arc<ToolRegistry>,
    pub skills: Arc<SkillRegistry>,
    pub agents: Arc<crate::agents::AgentRegistry>,
    pub computer_state: Arc<crate::agents::computer::ComputerState>,
    pub platform_auth: SharedPlatformAuth,
    pub platform_config: SharedPlatformConfig,
    pub task_board_store: Arc<crate::task_board::TaskBoardStore>,
    /// Lifecycle hooks aligned with Python `call_extensions(extension_point, …)`.
    pub extensions: Arc<ExtensionRegistry>,
    pub cancels: Mutex<HashMap<String, CancellationToken>>,
    /// When set, the in-flight `terminal` tool for that conversation kills its subprocess (host-only; does not cancel the LLM turn).
    pub terminal_run_abort: Mutex<HashMap<String, Arc<AtomicBool>>>,
    pub approvals: Mutex<HashMap<String, oneshot::Sender<bool>>>,
    /// Blocks `run_subagent` → computer until the UI confirms monitor selection.
    pub monitor_picks: Mutex<HashMap<String, oneshot::Sender<Result<(), String>>>>,
    /// Active main-agent task board key per conversation.
    pub active_main_task_boards: Mutex<HashMap<String, String>>,
    /// Main task board anchor bindings: conversation -> (store_key -> user_message_id).
    pub task_board_anchor_by_store_key: Mutex<HashMap<String, HashMap<String, String>>>,
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
            Some(db) => Arc::new(crate::task_board::TaskBoardStore::with_persistence(db)),
            None => {
                log::warn!("task_board: sqlite persistence unavailable; in-memory only");
                Arc::new(crate::task_board::TaskBoardStore::new())
            }
        };
        crate::tools::builtin::register_all(&tools, task_board_store.clone());
        let skills = Arc::new(SkillRegistry::new());
        crate::skills::builtin::register_all(&skills);
        crate::tools::builtin::register_skill_tools(&tools, skills.clone());
        if let Err(err) = skills.reload_meta() {
            log::warn!("load external skills failed: {err}");
        }
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
            extensions: Arc::new(extension_registry),
            cancels: Mutex::new(HashMap::new()),
            terminal_run_abort: Mutex::new(HashMap::new()),
            approvals: Mutex::new(HashMap::new()),
            monitor_picks: Mutex::new(HashMap::new()),
            active_main_task_boards: Mutex::new(HashMap::new()),
            task_board_anchor_by_store_key: Mutex::new(HashMap::new()),
        }
    }

    pub fn load_user_settings(&self) -> UserSettings {
        storage::load_user_settings().unwrap_or_default()
    }

    pub fn save_user_settings(&self, user: &UserSettings) -> anyhow::Result<()> {
        storage::save_user_settings(user)
    }

    pub fn effective_settings(&self) -> crate::models::ModelSettings {
        let user = self.load_user_settings();
        let platform = self.platform_config.read().clone();
        finalize_merged_settings(crate::models::merge_user_platform(&user, &platform))
    }

    pub fn effective_settings_view(&self) -> EffectiveSettingsView {
        let user = self.load_user_settings();
        let platform = self.platform_config.read().clone();
        let merged = finalize_merged_settings(crate::models::merge_user_platform(&user, &platform));
        let is_platform_admin = self.platform_auth.is_platform_admin();
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
        if !self.platform_auth.is_platform_admin() {
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
    }

    pub fn cancel(&self, conversation_id: &str) {
        if let Some(token) = self.cancels.lock().get(conversation_id) {
            token.cancel();
        }
        let approvals: Vec<_> = self.approvals.lock().drain().collect();
        for (_, tx) in approvals {
            let _ = tx.send(false);
        }
        let monitor_picks: Vec<_> = self.monitor_picks.lock().drain().collect();
        for (_, tx) in monitor_picks {
            let _ = tx.send(Err("已停止生成".into()));
        }
    }

    /// Kill only the subprocess for the current **`terminal`** tool in this conversation.
    /// Does **not** cancel the LLM stream or the rest of the turn. Returns **true** if a run was registered.
    pub fn abort_terminal_command(&self, conversation_id: &str) -> bool {
        self.terminal_run_abort
            .lock()
            .get(conversation_id)
            .map(|f| {
                f.store(true, Ordering::SeqCst);
                true
            })
            .unwrap_or(false)
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

    pub fn set_active_main_task_board_key(&self, conversation_id: &str, store_key: &str) {
        if conversation_id.trim().is_empty() || store_key.trim().is_empty() {
            return;
        }
        self.active_main_task_boards
            .lock()
            .insert(conversation_id.to_string(), store_key.to_string());
    }

    pub fn get_active_main_task_board_key(&self, conversation_id: &str) -> Option<String> {
        if let Some(k) = self
            .active_main_task_boards
            .lock()
            .get(conversation_id)
            .cloned()
        {
            return Some(k);
        }
        let prefix = format!(
            "{}{}",
            conversation_id.trim(),
            crate::task_board::coordination::main_turn::MAIN_TURN_KEY_SEP
        );
        let mut keys = self.task_board_store.list_store_keys_by_prefix(&prefix);
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

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}
