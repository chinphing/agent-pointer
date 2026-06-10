//! Cross-session curated memory (MEMORY.md / USER.md) and background review.

mod background_review;
mod store;
mod tool;

pub use background_review::{
    memory_review_due_for, resolve_review_kind, should_run_memory_review,
    skill_review_due_for, spawn_background_review, spawn_memory_background_review, ReviewKind,
};
pub(crate) use background_review::{allowed_tools_for, dispatch_review_tool};
pub use store::{
    count_real_user_turns, memories_dir, memory_review_due, skill_review_due, MemoryStore,
    DEFAULT_MEMORY_CHAR_LIMIT, DEFAULT_USER_CHAR_LIMIT,
};
pub use tool::{plan_includes_memory, register as register_memory_tool};

pub fn push_memory_to_cacheable(
    cacheable: &mut Vec<String>,
    store: &MemoryStore,
    memory_enabled: bool,
    user_profile_enabled: bool,
) {
    cacheable.extend(store.snapshot_blocks(memory_enabled, user_profile_enabled));
}
