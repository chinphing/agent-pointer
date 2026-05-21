use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use crate::agents::register_builtin_agents;
use crate::extensions::ExtensionRegistry;
use crate::platform_auth::SharedPlatformAuth;
use crate::skills::SkillRegistry;
use crate::tools::ToolRegistry;

pub struct AppState {
    pub tools: Arc<ToolRegistry>,
    pub skills: Arc<SkillRegistry>,
    pub agents: Arc<crate::agents::AgentRegistry>,
    pub computer_state: Arc<crate::agents::computer::ComputerState>,
    pub platform_auth: SharedPlatformAuth,
    pub task_board_store: Arc<crate::tools::task_board::TaskBoardStore>,
    /// Lifecycle hooks aligned with Python `call_extensions(extension_point, …)`.
    pub extensions: Arc<ExtensionRegistry>,
    pub cancels: Mutex<HashMap<String, CancellationToken>>,
    /// When set, the in-flight `terminal` tool for that conversation kills its subprocess (host-only; does not cancel the LLM turn).
    pub terminal_run_abort: Mutex<HashMap<String, Arc<AtomicBool>>>,
    pub approvals: Mutex<HashMap<String, oneshot::Sender<bool>>>,
}

impl AppState {
    pub fn new() -> Self {
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
        if let Err(err) = skills.reload_external() {
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
        ));
        crate::tools::builtin::register_computer_tools(&tools, computer_state.clone());
        let mut extension_registry = ExtensionRegistry::new();
        crate::extensions::register_builtin_extensions(&mut extension_registry);
        extension_registry.register_before_main_llm_call(Arc::new(
            crate::extensions::task_board_hook::TaskBoardSnapshotHook,
        ));
        Self {
            tools,
            skills,
            agents,
            computer_state,
            platform_auth,
            task_board_store,
            extensions: Arc::new(extension_registry),
            cancels: Mutex::new(HashMap::new()),
            terminal_run_abort: Mutex::new(HashMap::new()),
            approvals: Mutex::new(HashMap::new()),
        }
    }

    pub fn cancel(&self, conversation_id: &str) {
        if let Some(token) = self.cancels.lock().get(conversation_id) {
            token.cancel();
        }
        let approvals: Vec<_> = self.approvals.lock().drain().collect();
        for (_, tx) in approvals {
            let _ = tx.send(false);
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
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}
