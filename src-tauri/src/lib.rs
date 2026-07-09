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
mod popup_windows;
mod cloud_webview;
mod cloud_commands;
mod window_chrome_commands;

use pointer_channels::adapters::register_builtin_channels;
use pointer_channels::{ChannelGateway, ChannelRegistry};
use pointer_core::chat_service::AppState;
use pointer_core::models::StreamEvent;
use pointer_core::{
    skills::external::{system_skills_dir, sync_bundled_skill_dirs},
};
use std::{
    path::PathBuf,
    sync::Arc,
};
#[cfg(target_os = "macos")]
use std::sync::atomic::{AtomicU64, Ordering};
use tauri::{Emitter, Manager, RunEvent};
#[cfg(target_os = "macos")]
use tauri::WindowEvent;

#[cfg(target_os = "macos")]
fn traffic_light_inset_log_level(label: &'static str) -> Option<log::Level> {
    match label {
        // Routine repair / delayed passes — too noisy at INFO during resize and reapply storms.
        "reapply-delayed-50"
        | "reapply-delayed-200"
        | "reapply-delayed-500"
        | "window-resized"
        | "scale-factor-changed"
        | "window-focused" => None,
        _ => Some(log::Level::Debug),
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn apply_macos_traffic_light_inset(
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
    if let Some(level) = traffic_light_inset_log_level(label) {
        log::log!(
            level,
            "macOS traffic lights inset applied ({label}, x={}, y={})",
            macos_traffic_lights::INSET_X,
            macos_traffic_lights::INSET_Y
        );
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn reapply_macos_window_chrome(win: &tauri::WebviewWindow<tauri::Wry>) {
    if window_chrome_commands::is_computer_compact_chrome_active() {
        return;
    }
    apply_macos_overlay_chrome_api(win);

    schedule_macos_overlay_chrome_pass(win, "reapply-delayed-50", 50);
    schedule_macos_overlay_chrome_pass(win, "reapply-delayed-200", 200);
    schedule_macos_overlay_chrome_pass(win, "reapply-delayed-500", 500);
}

#[cfg(target_os = "macos")]
static MACOS_CHROME_REPAIR_GEN: AtomicU64 = AtomicU64::new(0);

/// Re-apply overlay title bar + traffic-light inset without toggling decorations.
#[cfg(target_os = "macos")]
pub(crate) fn repair_macos_overlay_chrome(win: &tauri::WebviewWindow<tauri::Wry>, label: &'static str) {
    if window_chrome_commands::is_computer_compact_chrome_active() {
        return;
    }

    let win_thread = win.clone();
    let win_inset = win.clone();
    if let Err(e) = win_thread.run_on_main_thread(move || {
        if window_chrome_commands::is_computer_compact_chrome_active() {
            return;
        }
        if let Ok(ns_window) = win_inset.ns_window() {
            macos_traffic_lights::apply_overlay_titlebar(ns_window);
            macos_traffic_lights::set_traffic_lights_visible(ns_window, true);
            macos_traffic_lights::enable_window_dragging(ns_window);
            apply_macos_traffic_light_inset(&win_inset, label);
        }
    }) {
        log::warn!("macOS window chrome: repair run_on_main_thread failed ({label}): {e}");
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn schedule_macos_overlay_chrome_repair(
    win: &tauri::WebviewWindow<tauri::Wry>,
    reason: &'static str,
) {
    if window_chrome_commands::is_computer_compact_chrome_active() {
        return;
    }

    let gen = MACOS_CHROME_REPAIR_GEN.fetch_add(1, Ordering::SeqCst) + 1;
    let win = win.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(120)).await;
        if MACOS_CHROME_REPAIR_GEN.load(Ordering::SeqCst) != gen {
            return;
        }
        repair_macos_overlay_chrome(&win, reason);
    });
}

#[cfg(target_os = "macos")]
fn apply_macos_overlay_chrome_api(win: &tauri::WebviewWindow<tauri::Wry>) {
    if window_chrome_commands::is_computer_compact_chrome_active() {
        return;
    }
    use tauri::TitleBarStyle;

    if let Err(e) = win.set_decorations(true) {
        log::warn!("macOS window chrome: set_decorations(true) failed: {e}");
    }
    if let Err(e) = win.set_title_bar_style(TitleBarStyle::Overlay) {
        log::warn!("macOS window chrome: set_title_bar_style(Overlay) failed: {e}");
    }
    if let Err(e) = win.set_title("Pointer Render") {
        log::warn!("macOS window chrome: set_title failed: {e}");
    }

    let win_initial = win.clone();
    if let Err(e) = win.run_on_main_thread(move || {
        if window_chrome_commands::is_computer_compact_chrome_active() {
            return;
        }
        if let Ok(ns_window) = win_initial.ns_window() {
            macos_traffic_lights::set_compact_surface(ns_window, false);
            macos_traffic_lights::apply_overlay_titlebar(ns_window);
            macos_traffic_lights::set_traffic_lights_visible(ns_window, true);
            apply_macos_traffic_light_inset(&win_initial, "reapply");
        }
    }) {
        log::warn!("macOS window chrome: run_on_main_thread failed: {e}");
    }
}

#[cfg(target_os = "macos")]
fn schedule_macos_overlay_chrome_pass(
    win: &tauri::WebviewWindow<tauri::Wry>,
    label: &'static str,
    delay_ms: u64,
) {
    use std::time::Duration;
    use tauri::TitleBarStyle;

    let win_delayed = win.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(delay_ms)).await;
        if window_chrome_commands::is_computer_compact_chrome_active() {
            return;
        }
        let _ = win_delayed.set_title_bar_style(TitleBarStyle::Overlay);
        let win_apply = win_delayed.clone();
        let _ = win_delayed.run_on_main_thread(move || {
            if window_chrome_commands::is_computer_compact_chrome_active() {
                return;
            }
            if let Ok(ns_window) = win_apply.ns_window() {
                macos_traffic_lights::apply_overlay_titlebar(ns_window);
                macos_traffic_lights::set_traffic_lights_visible(ns_window, true);
            }
            apply_macos_traffic_light_inset(&win_apply, label);
        });
    });
}

#[cfg(target_os = "macos")]
fn configure_macos_window_chrome(app: &tauri::App) {
    let Some(win) = app.get_webview_window("main") else {
        log::warn!("macOS window chrome: main window not found");
        return;
    };
    reapply_macos_window_chrome(&win);

    let win_for_events = win.clone();
    win.on_window_event(move |event| {
        match event {
            WindowEvent::Resized(_) => {
                schedule_macos_overlay_chrome_repair(&win_for_events, "window-resized");
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                schedule_macos_overlay_chrome_repair(&win_for_events, "scale-factor-changed");
            }
            WindowEvent::Focused(true) => {
                schedule_macos_overlay_chrome_repair(&win_for_events, "window-focused");
            }
            _ => {}
        }
    });

    log::info!("macOS window chrome: native traffic lights enabled (decorations + overlay)");
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    pointer_core::logging::init_backtrace_defaults();
    pointer_core::tls::ensure_rustls_crypto_provider();

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
            popup_windows::create_main_window(app)
                .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;

            #[cfg(target_os = "macos")]
            configure_macos_window_chrome(app);

            #[cfg(any(target_os = "windows", target_os = "linux"))]
            {
                use tauri::Manager;
                if let Some(win) = app.get_webview_window("main") {
                    window_chrome_commands::configure_frameless_window_chrome(&win);
                } else {
                    log::warn!("frameless window chrome: main window not found at startup");
                }
            }

            if let Err(err) = install_bundled_skills(app) {
                log::warn!("install bundled skills failed: {err}");
            }
            let app_state = Arc::new(AppState::new());
            let auth = app_state.platform_auth.clone();
            let app_for_creds = app_state.clone();
            tauri::async_runtime::spawn(async move {
                app_for_creds.start_background_tasks();
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
            // Build the singleton run dispatcher (unified callable / event
            // trigger entry) backed by this AppState. All trigger sources
            // (IPC `send_chat`, HTTP Runs API, webhooks, cron, IM, internal)
            // route through it. Built-in lifecycle hooks are pre-registered
            // inside `build_dispatcher`.
            let dispatcher = Arc::new(app_state.build_dispatcher());
            app.manage(dispatcher.clone());
            // Phase 5: cron scheduler. Desktop defaults ON (same as the
            // server / web host). Disable via `POINTER_SCHEDULER_ENABLED=0`.
            // The scheduler reuses the same dispatcher + store.
            let scheduler_on = std::env::var("POINTER_SCHEDULER_ENABLED")
                .map(|v| v != "0" && v.to_ascii_lowercase() != "false")
                .unwrap_or(true);
            if scheduler_on {
                // Spawn the ticker on Tauri's async runtime. `Scheduler::start`
                // uses `tokio::spawn`, which panics here because Tauri's
                // `setup` closure runs on the UI thread outside the tokio
                // runtime context. `tauri::async_runtime::spawn` is the correct
                // entry for desktop.
                let scheduler = Arc::new(pointer_core::scheduler::Scheduler::new(
                    app_state.clone(),
                    dispatcher.clone(),
                ));
                tauri::async_runtime::spawn(scheduler.clone().run());
                app.manage(scheduler);
                log::info!("desktop: cron scheduler enabled (ticker started)");
            } else {
                log::info!("desktop: cron scheduler disabled by POINTER_SCHEDULER_ENABLED=0");
            }
            let stream_app = app.handle().clone();
            pointer_core::stream_broadcast::subscribe_stream(Arc::new(move |ev| {
                if let Err(e) = stream_app.emit(commands::STREAM_EVENT, ev) {
                    log::warn!("stream broadcast emit failed: {e}");
                }
            }));
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
                channel_monitor::ChannelMonitorHandle::new();
            let monitor_supervisor = monitor_handle.supervisor();
            monitor_handle.start(channel_gateway.clone());
            app.manage(channel_gateway.clone());
            app.manage(Arc::new(monitor_handle));
            app.manage(Arc::new(
                pointer_channels::adapters::weixin::qr_login::QrLoginState::new(),
            ));
            app.manage(Arc::new(
                pointer_channels::registration::ChannelRegistrationState::with_completion(
                    channel_gateway,
                    monitor_supervisor,
                ),
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
            commands::submit_terminal_input,
            commands::dismiss_terminal_input,
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
            commands::probe_external_skills,
            commands::import_external_skills,
            commands::dismiss_external_skills_prompt,
            commands::list_tools,
            commands::list_agents,
            commands::get_task_board_snapshot,
            commands::list_work_items,
            commands::work_item_stats,
            commands::preview_computer_annotated_screen,
            commands::preview_computer_round_screen,
            commands::preview_chat_media,
            commands::get_chat_media_local_path,
            commands::preview_media_ref,
            commands::save_chat_attachment,
            commands::check_media_deps,
            commands::reveal_in_finder,
            commands::open_path_with_default_app,
            commands::open_chat_media,
            commands::get_media_oss_upload_status,
            commands::upload_composer_video_to_oss,
            commands::upload_composer_video_bytes_to_oss,
            commands::read_local_file_for_attachment,
            commands::get_local_file_size,
            commands::list_computer_monitors,
            commands::set_computer_conversation_monitor,
            commands::confirm_computer_monitor_pick,
            commands::cancel_computer_monitor_pick,
            commands::load_conversations,
            commands::load_conversation_metas,
            commands::load_conversation_messages,
            commands::delete_conversation,
            commands::save_conversations,
            commands::save_conversation_meta,
            commands::append_conversation_messages,
            commands::list_pinned_experiences,
            commands::list_experience_home,
            commands::search_experiences,
            commands::get_experience_detail,
            commands::list_cron_jobs,
            commands::get_dispatcher_queue_snapshot,
            commands::create_cron_job,
            commands::update_cron_job,
            commands::delete_cron_job,
            commands::get_webhook_config,
            commands::set_webhook_source_token,
            commands::patch_webhook_source,
            commands::reveal_webhook_source_token,
            commands::clear_webhook_source_token,
            commands::clear_webhook_legacy_token,
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
            cloud_commands::get_cloud_platform_me,
            cloud_commands::list_cloud_shop_regions,
            cloud_commands::get_cloud_shop_pricing,
            cloud_commands::preview_cloud_shop,
            cloud_commands::purchase_cloud_agent,
            cloud_commands::list_cloud_agents,
            cloud_commands::get_cloud_agent,
            cloud_commands::renew_cloud_agent,
            cloud_commands::release_cloud_agent,
            cloud_commands::create_cloud_agent_oauth_code,
            cloud_commands::open_cloud_agent,
            cloud_commands::focus_cloud_agent,
            cloud_commands::close_cloud_agent,
            cloud_commands::is_cloud_agent_window_open,
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
            window_chrome_commands::set_computer_compact_chrome,
            window_chrome_commands::begin_computer_compact_window,
            window_chrome_commands::restore_computer_compact_window,
            window_chrome_commands::reapply_window_chrome,
            window_chrome_commands::place_computer_compact_window,
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
        log::info!("bundled skills: all present under {}", system_skills_dir()?.display());
    } else {
        log::info!("bundled skills: installed {:?}", installed);
    }
    Ok(())
}
