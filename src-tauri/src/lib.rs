mod channel_commands;
mod channel_monitor;
mod commands;
#[cfg(target_os = "macos")]
mod macos_computer_permissions;
#[cfg(target_os = "macos")]
mod macos_permission_commands;
#[cfg(target_os = "macos")]
mod macos_traffic_lights;
mod platform_commands;

use pointer_channels::adapters::register_builtin_channels;
use pointer_channels::{ChannelGateway, ChannelRegistry};
use pointer_core::models::StreamEvent;
use pointer_core::{
    chat_service::AppState,
    skills::external::{skills_dir, sync_bundled_skill_dirs},
};
use std::{path::PathBuf, sync::Arc};
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
    // Keep a non-empty NSWindow title so Force Quit / Activity Monitor show
    // "Pointer Render" instead of the custom-protocol URL (tauri://localhost).
    // hiddenTitle still hides this text in the title bar overlay.
    if let Err(e) = win.set_title("Pointer Render") {
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

    // 发布版默认不含 `pointer_core::provider=debug`；调试模式或 dev 构建见 `logging::default_runtime_log_filter`。
    let default_log_filter = pointer_core::logging::default_runtime_log_filter();
    let log_dir = pointer_core::logging::desktop_log_dir();
    if let Err(err) =
        pointer_core::logging::init_runtime_logging(&log_dir, default_log_filter)
    {
        eprintln!(
            "Pointer: file logging unavailable ({err}); logs are stderr-only. log_dir={}",
            log_dir.display()
        );
        pointer_core::logging::init_stderr_only_logging(default_log_filter);
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
            app.manage(app_state.clone());
            let mut channel_registry = ChannelRegistry::new();
            register_builtin_channels(&mut channel_registry);
            let channel_gateway = Arc::new(
                ChannelGateway::new(app_state.clone(), channel_registry)
                    .map_err(|e| format!("channel gateway init failed: {e:#}"))?,
            );
            pointer_channels::install_channel_outbound_bridge(
                channel_gateway.clone(),
                app_state.tools.clone(),
            );
            let monitor_handle =
                channel_monitor::ChannelMonitorHandle::new(channel_gateway.clone());
            monitor_handle.start();
            app.manage(channel_gateway);
            app.manage(Arc::new(monitor_handle));
            app.manage(Arc::new(
                pointer_channels::adapters::weixin::qr_login::QrLoginState::new(),
            ));
            app.manage(Arc::new(
                pointer_channels::registration::ChannelRegistrationState::new(),
            ));
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
            commands::reload_skill_meta,
            commands::import_skill_zip,
            commands::list_tools,
            commands::list_agents,
            commands::get_task_board_snapshot,
            commands::preview_computer_annotated_screen,
            commands::preview_computer_round_screen,
            commands::preview_chat_media,
            commands::preview_media_ref,
            commands::save_chat_attachment,
            commands::check_media_deps,
            commands::list_computer_monitors,
            commands::set_computer_conversation_monitor,
            commands::confirm_computer_monitor_pick,
            commands::cancel_computer_monitor_pick,
            commands::load_conversations,
            commands::save_conversations,
            commands::list_pinned_experiences,
            commands::list_experience_home,
            commands::search_experiences,
            commands::get_experience_detail,
            channel_commands::get_channels_config,
            channel_commands::update_channels_config,
            channel_commands::list_channel_status,
            channel_commands::get_channel_webhook_url,
            channel_commands::start_weixin_login,
            channel_commands::get_weixin_login_status,
            channel_commands::has_weixin_credentials,
            channel_commands::start_channel_registration,
            channel_commands::get_channel_registration_status,
            channel_commands::approve_channel_pairing,
            channel_commands::list_channel_pairing_pending,
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
    let mut sources = Vec::new();
    if let Ok(resource_dir) = app.path().resource_dir() {
        let bundled = resource_dir.join("skills");
        if bundled.exists() {
            sources.push(bundled);
        }
    }
    // `tauri dev` 时 resource_dir 可能无 skills；回退到仓库 skills/
    let dev_skills = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../skills");
    if dev_skills.exists() {
        sources.push(dev_skills);
    }
    if sources.is_empty() {
        log::warn!("bundled skills: no source directory found");
        return Ok(());
    }
    let installed = sync_bundled_skill_dirs(&sources)?;
    if installed.is_empty() {
        log::info!("bundled skills: all present under {}", skills_dir()?.display());
    } else {
        log::info!("bundled skills: installed {:?}", installed);
    }
    Ok(())
}
