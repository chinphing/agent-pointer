//! Cross-session conversation recall via SQLite FTS5 (`sessions.db`).

mod cjk_fts;
mod index;
mod tool;

pub use index::SessionIndex;
pub use tool::{plan_includes_session_search, register as register_session_search_tool};
