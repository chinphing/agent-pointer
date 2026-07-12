use serde::Serialize;
use tauri::{AppHandle, Emitter};

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

    let Some(update) = app
        .updater()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| e.to_string())?
    else {
        return Ok(UpdateCheckResult {
            available: false,
            version: None,
            notes: None,
            current_version,
        });
    };

    Ok(UpdateCheckResult {
        available: true,
        version: Some(update.version.clone()),
        notes: update.body.clone(),
        current_version,
    })
}

#[tauri::command]
pub async fn download_update(app: AppHandle) -> Result<(), String> {
    use tauri_plugin_updater::UpdaterExt;

    let Some(update) = app
        .updater()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| e.to_string())?
    else {
        return Err("no_update_available".into());
    };

    let mut downloaded = 0;
    update
        .download_and_install(
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
            || {},
        )
        .await
        .map_err(|e| e.to_string())?;

    let _ = app.emit(
        "updater://status",
        serde_json::json!({ "phase": "ready" }),
    );
    Ok(())
}

#[tauri::command]
pub async fn restart_app(app: AppHandle) {
    app.restart();
}
