mod commands;

use pointer_core::{chat_service::AppState, skills::external::skills_dir};
use std::{fs, path::Path, sync::Arc};
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    pointer_core::logging::init_backtrace_defaults();

    // `pointer_core::provider=debug`：流式/非流式请求结束后在 stderr 打印模型原始正文（含 XML 工具块），便于调试。
    const DEFAULT_LOG_FILTER: &str =
        "warn,pointer_core=info,pointer_core::provider=debug,pointer_app_lib=info";
    let log_dir = pointer_core::logging::desktop_log_dir();
    if let Err(err) =
        pointer_core::logging::init_runtime_logging(&log_dir, DEFAULT_LOG_FILTER)
    {
        eprintln!(
            "Pointer: file logging unavailable ({err}); logs are stderr-only. log_dir={}",
            log_dir.display()
        );
        let _ = env_logger::Builder::from_env(
            env_logger::Env::default().default_filter_or(DEFAULT_LOG_FILTER),
        )
        .try_init();
        pointer_core::logging::install_panic_hook();
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            if let Err(err) = install_bundled_skills(app) {
                log::warn!("install bundled skills failed: {err}");
            }
            app.manage(Arc::new(AppState::new()));
            Ok(())
        })
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
            commands::import_skill_zip,
            commands::list_tools,
            commands::list_agents,
            commands::preview_computer_annotated_screen,
            commands::load_conversations,
            commands::save_conversations,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn install_bundled_skills(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let Ok(resource_dir) = app.path().resource_dir() else {
        return Ok(());
    };
    let bundled_skills = resource_dir.join("skills");
    if !bundled_skills.exists() {
        return Ok(());
    }

    let target_root = skills_dir()?;
    for entry in fs::read_dir(bundled_skills)? {
        let entry = entry?;
        let source = entry.path();
        if !source.is_dir() {
            continue;
        }

        let target = target_root.join(entry.file_name());
        if target.exists() {
            continue;
        }
        copy_dir_all(&source, &target)?;
    }

    Ok(())
}

fn copy_dir_all(source: &Path, target: &Path) -> std::io::Result<()> {
    fs::create_dir_all(target)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());
        if source_path.is_dir() {
            copy_dir_all(&source_path, &target_path)?;
        } else if source_path.is_file() {
            fs::copy(&source_path, &target_path)?;
        }
    }
    Ok(())
}
