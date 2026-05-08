mod commands;

use pointer_core::chat_service::AppState;
use std::sync::Arc;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = env_logger::try_init();
    let state = Arc::new(AppState::new());

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            commands::send_chat,
            commands::cancel_chat,
            commands::approve_tool_call,
            commands::get_settings,
            commands::update_settings,
            commands::set_api_key,
            commands::clear_api_key,
            commands::test_connection,
            commands::list_skills,
            commands::list_tools,
            commands::load_conversations,
            commands::save_conversations,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
