use serde::Serialize;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, State};

/// Downloaded updater package waiting for the user to confirm install/restart.
#[derive(Default)]
pub struct PendingUpdateState {
    inner: Mutex<Option<PendingUpdate>>,
}

struct PendingUpdate {
    version: String,
    bytes: Vec<u8>,
}

#[tauri::command]
pub fn updater_log(msg: String) {
    log::info!("[updater-fe] {}", msg);
}

#[derive(Serialize)]
pub struct UpdateCheckResult {
    pub available: bool,
    pub version: Option<String>,
    pub notes: Option<String>,
    pub current_version: String,
}

#[tauri::command]
pub async fn check_for_update(app: AppHandle) -> Result<UpdateCheckResult, String> {
    use tauri_plugin_updater::UpdaterExt;

    let current_version = app.package_info().version.to_string();
    log::info!("[updater] checking for update: current={current_version}");

    let Some(update) = app
        .updater()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| {
            log::error!("[updater] check failed: {e}");
            e.to_string()
        })?
    else {
        log::info!("[updater] no update available");
        return Ok(UpdateCheckResult {
            available: false,
            version: None,
            notes: None,
            current_version,
        });
    };

    log::info!(
        "[updater] update available: {} → {}",
        current_version,
        update.version
    );
    Ok(UpdateCheckResult {
        available: true,
        version: Some(update.version.clone()),
        notes: update.body.clone(),
        current_version,
    })
}

/// Download and verify the update package only. Do **not** install yet.
///
/// Windows `install()` launches the MSI/NSIS installer and then exits the
/// process (`process::exit(0)`), so installing before the user confirms would
/// look like a silent auto-upgrade.
#[tauri::command]
pub async fn download_update(
    app: AppHandle,
    pending: State<'_, PendingUpdateState>,
) -> Result<(), String> {
    use tauri_plugin_updater::UpdaterExt;

    log::info!("[updater] starting download (install deferred until user confirms)");

    let Some(update) = app
        .updater()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| {
            log::error!("[updater] download check failed: {e}");
            e.to_string()
        })?
    else {
        return Err("no_update_available".into());
    };

    let version = update.version.clone();
    log::info!("[updater] downloading version {version}");

    let mut downloaded = 0usize;
    let bytes = update
        .download(
            |chunk_len, content_len| {
                downloaded += chunk_len;
                let _ = app.emit(
                    "updater://download-progress",
                    serde_json::json!({
                        "downloaded": downloaded,
                        "total": content_len,
                    }),
                );
            },
            || log::info!("[updater] download finished, awaiting install confirmation"),
        )
        .await
        .map_err(|e| {
            log::error!("[updater] download failed: {e}");
            e.to_string()
        })?;

    let byte_len = bytes.len();
    {
        let mut guard = pending.inner.lock().map_err(|e| e.to_string())?;
        *guard = Some(PendingUpdate { version: version.clone(), bytes });
    }

    log::info!(
        "[updater] {version} downloaded ({byte_len} bytes), ready for user confirm"
    );
    let _ = app.emit(
        "updater://status",
        serde_json::json!({ "phase": "ready" }),
    );
    Ok(())
}

/// Return the version of a package already downloaded and waiting for install.
#[tauri::command]
pub fn pending_update_version(pending: State<'_, PendingUpdateState>) -> Option<String> {
    match pending.inner.lock() {
        Ok(guard) => guard.as_ref().map(|p| p.version.clone()),
        Err(e) => {
            log::warn!("[updater] pending_update_version lock poisoned: {e}");
            None
        }
    }
}

/// Install the previously downloaded package, then restart.
///
/// On Windows the installer path calls `process::exit(0)` after launching
/// MSI/NSIS (often with auto-relaunch). On macOS/Linux this returns and we
/// restart via Tauri.
#[tauri::command]
pub async fn install_and_restart(
    app: AppHandle,
    pending: State<'_, PendingUpdateState>,
) -> Result<(), String> {
    use tauri_plugin_updater::UpdaterExt;

    let PendingUpdate { version, bytes } = {
        let mut guard = pending.inner.lock().map_err(|e| e.to_string())?;
        guard.take().ok_or_else(|| {
            log::warn!("[updater] install requested but no pending package");
            "no_pending_update".to_string()
        })?
    };

    log::info!("[updater] user confirmed install of {version}");

    let Some(update) = app
        .updater()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| {
            log::error!("[updater] install check failed: {e}");
            e.to_string()
        })?
    else {
        // Put bytes back so the user can retry after a transient check miss.
        let mut guard = pending.inner.lock().map_err(|e| e.to_string())?;
        *guard = Some(PendingUpdate {
            version: version.clone(),
            bytes,
        });
        log::warn!("[updater] install aborted: update no longer reported as available");
        return Err("no_update_available".into());
    };

    if update.version != version {
        log::warn!(
            "[updater] pending version {version} != latest {}, keeping pending package",
            update.version
        );
        let mut guard = pending.inner.lock().map_err(|e| e.to_string())?;
        *guard = Some(PendingUpdate { version, bytes });
        return Err("update_version_mismatch".into());
    }

    log::info!("[updater] installing version {version}");
    update.install(bytes).map_err(|e| {
        log::error!("[updater] install failed: {e}");
        e.to_string()
    })?;

    // Windows installer path usually exits the process; macOS/Linux need restart.
    log::info!("[updater] install complete, restarting app");
    app.restart();
}

#[tauri::command]
pub async fn restart_app(app: AppHandle) {
    log::info!("[updater] restarting app");
    app.restart();
}
