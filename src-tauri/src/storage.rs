//! Tauri app data paths — delegate to `pointer_core::storage` for a single schema (theme, UI overrides, etc.).

pub use pointer_core::storage::{
    append_conversation_messages, clear_api_key, create_project, delete_project, has_key,
    load_api_key, load_conversation_messages, load_conversation_messages_page,
    load_conversation_meta, load_conversation_metas, load_conversations, load_project,
    load_project_conversation_metas, load_projects, load_settings, load_sidebar_projects,
    save_api_key, save_conversation_meta, save_settings, update_project,
};
