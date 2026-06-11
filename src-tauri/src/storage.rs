//! Tauri app data paths — delegate to `pointer_core::storage` for a single schema (theme, UI overrides, etc.).

pub use pointer_core::storage::{
    append_conversation_messages, clear_api_key, has_key, load_api_key, load_conversations, load_settings,
    save_api_key, save_conversation_meta, save_conversations, save_settings,
};
