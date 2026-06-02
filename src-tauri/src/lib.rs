mod commands;
#[cfg(target_os = "macos")]
mod macos_computer_permissions;
#[cfg(target_os = "macos")]
mod macos_permission_commands;
#[cfg(target_os = "macos")]
mod macos_traffic_lights;
mod platform_commands;

use pointer_core::models::StreamEvent;
use pointer_core::{chat_service::AppState, skills::external::skills_dir};
use std::{fs, path::Path, sync::Arc};
use tauri::{Emitter, Manager, RunEvent};

#[cfg(target_os = "macos")]
fn apply_macos_traffic_light_inset(
    win: &tauri::WebviewWindow<tauri::Wry>,
    label: &'static str,
) {
    use tauri::LogicalPosition;

    let Ok(ns_window) = win.ns_window() else {
        log::warn!("macOS window chrome: ns_window unavailable ({label})");
        return;
    };
    macos_traffic_lights::apply_inset(
        ns_window,
        LogicalPosition::new(
            macos_traffic_lights::INSET_X,
            macos_traffic_lights::INSET_Y,
        ),
    );
    log::info!(
        "macOS traffic lights inset applied ({label}, x={}, y={})",
        macos_traffic_lights::INSET_X,
        macos_traffic_lights::INSET_Y
    );
}

#[cfg(target_os = "macos")]
fn configure_macos_window_chrome(app: &tauri::App) {
    use std::time::Duration;
    use tauri::{Manager, TitleBarStyle};

    let Some(win) = app.get_webview_window("main") else {
        log::warn!("macOS window chrome: main window not found");
        return;
    };

    if let Err(e) = win.set_decorations(true) {
        log::warn!("macOS window chrome: set_decorations(true) failed: {e}");
    }
    if let Err(e) = win.set_title_bar_style(TitleBarStyle::Overlay) {
        log::warn!("macOS window chrome: set_title_bar_style(Overlay) failed: {e}");
    }
    if let Err(e) = win.set_title(" ") {
        log::warn!("macOS window chrome: set_title failed: {e}");
    }

    let win_initial = win.clone();
    if let Err(e) = win.run_on_main_thread(move || {
        apply_macos_traffic_light_inset(&win_initial, "initial");
    }) {
        log::warn!("macOS window chrome: run_on_main_thread failed: {e}");
    }

    let win_delayed = win.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        let win_apply = win_delayed.clone();
        let _ = win_delayed.run_on_main_thread(move || {
            apply_macos_traffic_light_inset(&win_apply, "delayed");
        });
    });

    log::info!("macOS window chrome: native traffic lights enabled (decorations + overlay)");
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    pointer_core::logging::init_backtrace_defaults();

    // `pointer_core::provider=debug`：流式/非流式请求结束后在 stderr 打印模型原始正文（含 XML 工具块），便于调试。
    // `pointer_core::llm_token_stats=debug`：每轮 LLM 的 usage token 调试行（见 docs/llm/llm-token-usage-logging.md）。
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
        pointer_core::logging::init_stderr_only_logging(DEFAULT_LOG_FILTER);
        pointer_core::logging::install_panic_hook();
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            #[cfg(target_os = "macos")]
            configure_macos_window_chrome(app);

            if let Err(err) = install_bundled_skills(app) {
                log::warn!("install bundled skills failed: {err}");
            }
            let app_state = Arc::new(AppState::new());
            let auth = app_state.platform_auth.clone();
            let app_for_creds = app_state.clone();
            tauri::async_runtime::spawn(async move {
                match auth.load_persisted_session().await {
                    Ok(Some(creds)) => app_for_creds.apply_login_credentials(&creds),
                    Ok(None) => {}
                    Err(e) => log::warn!("platform_auth: persisted session load failed: {e}"),
                }
                if let Err(e) = pointer_core::token_usage_store::finalize_all_stale_accum() {
                    log::warn!("token_usage_store: startup finalize stale failed: {e}");
                }
                if let Err(e) = pointer_core::token_usage_store::flush_pending_reports(&auth).await {
                    log::warn!("token_usage_store: startup flush failed: {e}");
                }
            });
            app.manage(app_state);
            let handle = app.handle().clone();
            match pointer_core::agents::computer::capture_debug::purge_computer_captures_older_than_days(
                pointer_core::agents::computer::capture_debug::CAPTURE_RETENTION_DAYS,
            ) {
                Ok(removed) if removed > 0 => {
                    if let Err(e) = handle.emit(
                        commands::STREAM_EVENT,
                        StreamEvent::UiToast {
                            conversation_id: String::new(),
                            message: "截图过期已清理".into(),
                            level: "warning".into(),
                        },
                    ) {
                        log::warn!("emit capture purged toast failed: {e}");
                    }
                }
                Ok(_) => {}
                Err(e) => log::warn!("computer capture purge failed: {e}"),
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::send_chat,
            commands::cancel_chat,
            commands::abort_terminal_command,
            commands::approve_tool_call,
            commands::get_settings,
            commands::update_settings,
            commands::update_agent_settings,
            commands::update_user_settings,
            commands::update_platform_settings,
            commands::set_api_key,
            commands::clear_api_key,
            commands::test_connection,
            commands::list_skills,
            commands::import_skill_zip,
            commands::list_tools,
            commands::list_agents,
            commands::get_task_board_snapshot,
            commands::preview_computer_annotated_screen,
            commands::preview_computer_round_screen,
            commands::list_computer_monitors,
            commands::set_computer_conversation_monitor,
            commands::load_conversations,
            commands::save_conversations,
            platform_commands::get_platform_session,
            platform_commands::open_platform_login,
            platform_commands::cancel_platform_login,
            platform_commands::refresh_platform_session,
            platform_commands::logout_platform,
            platform_commands::flush_platform_token_usage,
            platform_commands::load_platform_session_persisted,
            platform_commands::load_platform_session_from_keyring,
            #[cfg(target_os = "macos")]
            macos_permission_commands::register_macos_screen_recording_access,
            #[cfg(target_os = "macos")]
            macos_permission_commands::get_macos_computer_permissions,
            #[cfg(target_os = "macos")]
            macos_permission_commands::open_macos_computer_permission_settings,
            #[cfg(target_os = "macos")]
            macos_permission_commands::begin_macos_permission_drag_flow,
            #[cfg(target_os = "macos")]
            macos_permission_commands::dismiss_macos_permission_drag_guide,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            if let RunEvent::Exit = event {
                if let Some(state) = app.try_state::<Arc<AppState>>() {
                    let auth = state.platform_auth.clone();
                    tauri::async_runtime::block_on(async {
                        if let Err(e) =
                            pointer_core::token_usage_store::finalize_all_stale_accum()
                        {
                            log::warn!("token_usage_store: exit finalize stale failed: {e}");
                        }
                        if let Err(e) =
                            pointer_core::token_usage_store::flush_pending_reports(&auth).await
                        {
                            log::warn!("token_usage_store: exit flush failed: {e}");
                        }
                    });
                }
            }
        });
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
