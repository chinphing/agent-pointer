//! Cross-session conversation recall via the canonical SQLite store.

mod index;
mod tool;

pub use index::{open, open_default, SessionIndex};
pub use tool::{plan_includes_session_search, register as register_session_search_tool};
