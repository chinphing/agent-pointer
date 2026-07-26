//! Agent extension hooks (Python `python.helpers.extension` analogue).
//!
//! Extension **points** match the Pointer Python agent: hooks run at fixed lifecycle stages.
//! Profile-specific built-ins register from their own crates/modules (e.g. Computer screen inject under
//! [`crate::agents::computer::extension_hooks`]). Each hook has an `override_key` (Python: source file basename);
//! **re-registering** the same key **replaces** the previous hook so callers can override defaults
//! without forked init order.
//!
//! This crate does not scan arbitrary directories at runtime; add hooks by calling
//! [`ExtensionRegistry::register_message_loop_prompts_after`] (or builtins via
//! [`register_builtin_extensions`]) when building [`crate::chat_service::AppState`].

use crate::agents::computer::ComputerState;
use crate::agents::AgentProfile;
use crate::models::{ChatMessage, ChatStreamSender};
use crate::task_board::TaskBoardStore;
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;

pub mod common_user_dynamic_inject_hook;
pub mod task_board_hook;

/// Per-turn context for [`ExtensionPoint::MessageLoopPromptsAfter`].
///
/// Canonical transcript is **read-only** (`base_messages`). Hooks may only append
/// ephemeral rows to [`Self::injected_tail`] (never mutate / push onto session history).
pub struct MessageLoopPromptsAfterContext<'a> {
    pub computer_state: &'a ComputerState,
    pub lead_agent_profile: AgentProfile,
    /// Authoritative history for this round (session `history` / sub-agent `local_history`).
    pub base_messages: &'a [ChatMessage],
    /// Ephemeral API-only rows for this request (`[CUR_SCREEN]`, task-board inject, …).
    pub injected_tail: &'a mut Vec<ChatMessage>,
    pub conversation_id: &'a str,
    /// When set, hooks may emit thread events (e.g. [`crate::models::StreamEvent::InjectedAssistantMessage`])
    /// or legacy [`crate::models::StreamEvent::UiToast`]; neither is part of the model API payload.
    pub stream: Option<&'a ChatStreamSender>,
    /// Assistant message id for the LLM round in progress (UI row exists after [`crate::models::StreamEvent::MessageStart`]).
    /// Screen inject uses this to attach per-turn annotated previews.
    pub round_assistant_message_id: Option<String>,
    /// File-name prefix for capture dumps; defaults to `round_assistant_message_id` when unset.
    /// Supervisor sub-agents set this to a per-iteration id while UI events stay on the parent message.
    pub round_screen_dump_prefix: Option<String>,
    /// Shared task board store for dynamic user inject hooks.
    pub task_board_store: Arc<TaskBoardStore>,
    /// Store key for current round (main conversation id or sub-agent derived key).
    pub task_board_store_key: &'a str,
    /// Feature flag for common user dynamic inject migration.
    pub user_dynamic_inject_enabled: bool,
}

impl MessageLoopPromptsAfterContext<'_> {
    /// Scan base history then already-queued injects (for hint cadence, etc.).
    pub fn messages_for_scan(&self) -> impl Iterator<Item = &ChatMessage> {
        self.base_messages.iter().chain(self.injected_tail.iter())
    }
}

/// Context for [`ExtensionPoint::BeforeMainLlmCall`] immediately before [`crate::provider::OpenAIProvider::stream_chat`].
///
/// `system_prompts_cacheable` already includes communication inject, agent/skills, tool appendix,
/// and **`[Environment]`**. Hooks here **append only to `system_prompts_dynamic`**
/// (e.g. `[LOCKED GOAL]`). **Full date+time** for Computer is in `[CUR_SCREEN]`, not here.
pub struct BeforeMainLlmCallContext<'a> {
    pub computer_state: &'a ComputerState,
    pub lead_agent_profile: AgentProfile,
    pub system_prompts_dynamic: &'a mut Vec<String>,
    /// Main chat session id (stream / logs). Not necessarily equal to [`Self::task_board_store_key`].
    pub conversation_id: &'a str,
    pub task_board_store: Arc<crate::task_board::TaskBoardStore>,
    /// Key for [`crate::task_board::TaskBoardStore::snapshot_for_prompt`] / host `task_board` binding.
    pub task_board_store_key: &'a str,
}

#[async_trait]
pub trait MessageLoopPromptsAfterHook: Send + Sync {
    /// Stable id for **deduplication** (Python: source file basename). Re-registering replaces the prior hook.
    fn override_key(&self) -> &'static str;

    /// Lexicographic run order within the extension point (Python: `_10_…`, `_75_…`).
    fn sort_key(&self) -> &'static str;

    async fn execute(&self, ctx: &mut MessageLoopPromptsAfterContext<'_>) -> Result<()>;
}

#[async_trait]
pub trait BeforeMainLlmCallHook: Send + Sync {
    fn override_key(&self) -> &'static str;
    fn sort_key(&self) -> &'static str;
    async fn execute(&self, ctx: &mut BeforeMainLlmCallContext<'_>) -> Result<()>;
}

/// Registry of extension hooks. Intended as `Arc<ExtensionRegistry>` on [`crate::chat_service::AppState`].
#[derive(Default)]
pub struct ExtensionRegistry {
    message_loop_prompts_after: Vec<Arc<dyn MessageLoopPromptsAfterHook>>,
    before_main_llm_call: Vec<Arc<dyn BeforeMainLlmCallHook>>,
}

impl ExtensionRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a hook, replacing any existing hook with the same `override_key`.
    pub fn register_message_loop_prompts_after(
        &mut self,
        hook: Arc<dyn MessageLoopPromptsAfterHook>,
    ) {
        let key = hook.override_key();
        self.message_loop_prompts_after
            .retain(|h| h.override_key() != key);
        self.message_loop_prompts_after.push(hook);
    }

    pub fn register_before_main_llm_call(&mut self, hook: Arc<dyn BeforeMainLlmCallHook>) {
        let key = hook.override_key();
        self.before_main_llm_call
            .retain(|h| h.override_key() != key);
        self.before_main_llm_call.push(hook);
    }

    pub async fn run_message_loop_prompts_after(
        &self,
        ctx: &mut MessageLoopPromptsAfterContext<'_>,
    ) -> Result<()> {
        let mut hooks: Vec<_> = self.message_loop_prompts_after.iter().cloned().collect();
        hooks.sort_by(|a, b| a.sort_key().cmp(b.sort_key()));
        for h in hooks {
            h.execute(ctx).await?;
        }
        Ok(())
    }

    pub async fn run_before_main_llm_call(
        &self,
        ctx: &mut BeforeMainLlmCallContext<'_>,
    ) -> Result<()> {
        let mut hooks: Vec<_> = self.before_main_llm_call.iter().cloned().collect();
        hooks.sort_by(|a, b| a.sort_key().cmp(b.sort_key()));
        for h in hooks {
            h.execute(ctx).await?;
        }
        Ok(())
    }
}

/// Register framework defaults by delegating to profile-specific modules (Computer, …).
pub fn register_builtin_extensions(registry: &mut ExtensionRegistry) {
    crate::agents::computer::extension_hooks::register(registry);
    crate::agents::research::extension_hooks::register(registry);
    registry.register_message_loop_prompts_after(Arc::new(
        common_user_dynamic_inject_hook::CommonUserDynamicInjectHook,
    ));
}

pub(crate) fn new_extension_message_id(prefix: &str) -> String {
    format!("{prefix}_{}", uuid::Uuid::new_v4().simple())
}

pub(crate) fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::computer::ComputerState;
    use crate::agents::AgentProfile;
    use std::sync::atomic::{AtomicU8, Ordering};

    struct CountingHook {
        key: &'static str,
        order: &'static str,
        counter: Arc<AtomicU8>,
    }

    #[async_trait::async_trait]
    impl MessageLoopPromptsAfterHook for CountingHook {
        fn override_key(&self) -> &'static str {
            self.key
        }

        fn sort_key(&self) -> &'static str {
            self.order
        }

        async fn execute(&self, _ctx: &mut MessageLoopPromptsAfterContext<'_>) -> Result<()> {
            self.counter.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    #[tokio::test]
    async fn same_override_key_replaces_earlier_hook() {
        let c1 = Arc::new(AtomicU8::new(0));
        let c2 = Arc::new(AtomicU8::new(0));
        let mut reg = ExtensionRegistry::new();
        reg.register_message_loop_prompts_after(Arc::new(CountingHook {
            key: "_dup",
            order: "_10_a",
            counter: c1.clone(),
        }));
        reg.register_message_loop_prompts_after(Arc::new(CountingHook {
            key: "_dup",
            order: "_10_b",
            counter: c2.clone(),
        }));
        let computer = ComputerState::with_annotate_url("http://127.0.0.1:9");
        let base: &[ChatMessage] = &[];
        let mut tail = Vec::new();
        let mut ctx = MessageLoopPromptsAfterContext {
            computer_state: &computer,
            lead_agent_profile: AgentProfile::General,
            base_messages: base,
            injected_tail: &mut tail,
            conversation_id: "test",
            stream: None,
            round_assistant_message_id: None,
            round_screen_dump_prefix: None,
            task_board_store: Arc::new(crate::task_board::TaskBoardStore::new()),
            task_board_store_key: "test",
            user_dynamic_inject_enabled: true,
        };
        reg.run_message_loop_prompts_after(&mut ctx).await.unwrap();
        assert_eq!(c1.load(Ordering::SeqCst), 0);
        assert_eq!(c2.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn hooks_run_in_sort_key_order() {
        let run = Arc::new(std::sync::Mutex::new(String::new()));

        struct TagHook {
            tag: char,
            key: &'static str,
            sk: &'static str,
            run: Arc<std::sync::Mutex<String>>,
        }
        #[async_trait::async_trait]
        impl MessageLoopPromptsAfterHook for TagHook {
            fn override_key(&self) -> &'static str {
                self.key
            }
            fn sort_key(&self) -> &'static str {
                self.sk
            }
            async fn execute(&self, _ctx: &mut MessageLoopPromptsAfterContext<'_>) -> Result<()> {
                self.run.lock().unwrap().push(self.tag);
                Ok(())
            }
        }

        let mut reg = ExtensionRegistry::new();
        reg.register_message_loop_prompts_after(Arc::new(TagHook {
            tag: 'b',
            key: "_b",
            sk: "_20_b",
            run: run.clone(),
        }));
        reg.register_message_loop_prompts_after(Arc::new(TagHook {
            tag: 'a',
            key: "_a",
            sk: "_10_a",
            run: run.clone(),
        }));

        let computer = ComputerState::with_annotate_url("http://127.0.0.1:9");
        let base: &[ChatMessage] = &[];
        let mut tail = Vec::new();
        let mut ctx = MessageLoopPromptsAfterContext {
            computer_state: &computer,
            lead_agent_profile: AgentProfile::General,
            base_messages: base,
            injected_tail: &mut tail,
            conversation_id: "test",
            stream: None,
            round_assistant_message_id: None,
            round_screen_dump_prefix: None,
            task_board_store: Arc::new(crate::task_board::TaskBoardStore::new()),
            task_board_store_key: "test",
            user_dynamic_inject_enabled: true,
        };
        reg.run_message_loop_prompts_after(&mut ctx).await.unwrap();
        assert_eq!(*run.lock().unwrap(), "ab");
    }
}
