//! Persist screen-capture bundles for debugging under the app data directory.
//! Only writes assets the active tier uses — no extra JPEG/PNG generation on disk.

use crate::agents::computer::state::ScreenCaptureResult;
use crate::agents::computer::tier::ComputerTier;
use crate::agents::computer::vision::screen;
use crate::models::ComputerAnnotatedPreview;
use anyhow::Context;
use std::fs;
use std::path::PathBuf;

/// Day-level folders older than this are removed on app startup (desktop).
pub const CAPTURE_RETENTION_DAYS: i64 = 7;

/// On-disk capture dumps are debug-only (title-bar bug icon → 调试模式).
pub fn computer_capture_dump_enabled() -> bool {
    crate::platform_config::effective_settings_global().debug_menus_enabled
}

fn sanitize_path_segment(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn capture_root_dir() -> Option<PathBuf> {
    match crate::storage::app_data_dir() {
        Ok(base) => Some(base.join("computer-captures")),
        Err(e) => {
            log::warn!("computer capture: app_data_dir unavailable: {e}");
            None
        }
    }
}

/// Join `rel` under the capture root; rejects `..` and absolute paths.
pub fn safe_capture_file_path(rel: &str) -> Option<PathBuf> {
    let root = capture_root_dir()?;
    let rel = rel.trim().trim_start_matches(['/', '\\']);
    if rel.is_empty() || rel.contains("..") {
        return None;
    }
    let full = root.join(rel);
    let root_str = root.to_string_lossy();
    let full_str = full.to_string_lossy();
    if !full_str.starts_with(root_str.as_ref()) {
        return None;
    }
    Some(full)
}

/// Read a saved annotated JPEG/PNG (legacy) and return the same shape as live preview.
pub fn read_computer_capture_preview(rel: &str) -> anyhow::Result<ComputerAnnotatedPreview> {
    let path = safe_capture_file_path(rel).context("invalid or disallowed capture path")?;
    let bytes = fs::read(&path).with_context(|| format!("read {:?}", path))?;
    Ok(ComputerAnnotatedPreview {
        image_base64: screen::encode_image_to_base64(&bytes),
        image_mime: screen::image_data_url_mime(&bytes).to_string(),
        caption: "本圈截图画面".into(),
    })
}

/// Deletes `{root}/{YYYY-MM-DD}/` directories whose date is strictly before today − `days`.
/// Returns how many day folders were removed.
pub fn purge_computer_captures_older_than_days(days: i64) -> std::io::Result<usize> {
    let Some(root) = capture_root_dir() else {
        log::warn!("computer capture purge: could not resolve app data directory");
        return Ok(0);
    };
    if !root.is_dir() {
        return Ok(0);
    }
    let cutoff = chrono::Local::now().date_naive() - chrono::Duration::days(days);
    let mut removed = 0usize;
    for entry in fs::read_dir(&root)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if let Ok(d) = chrono::NaiveDate::parse_from_str(&name_str, "%Y-%m-%d") {
            if d < cutoff {
                fs::remove_dir_all(&path)?;
                removed += 1;
                log::info!("computer capture purge: removed {:?}", path);
            }
        }
    }
    Ok(removed)
}

fn write_bytes(dir: &PathBuf, pfx: &str, name: &str, ext: &str, ts: i64, bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    let path = dir.join(format!("{pfx}_{name}_{ts}.{ext}"));
    if let Err(e) = fs::write(&path, bytes) {
        log::warn!("computer capture dump: write {:?}: {e}", path);
    }
}

/// Writes tier-selected files under `{data_dir}/computer-captures/{YYYY-MM-DD}/{conversation_id}/`.
///
/// Returns the **path relative to `computer-captures/`** of the annotated JPEG (for lazy UI load).
pub fn save_computer_capture_debug(
    conversation_id: &str,
    file_prefix: &str,
    cap: &ScreenCaptureResult,
    tier: ComputerTier,
) -> Option<String> {
    if !computer_capture_dump_enabled() {
        return None;
    }
    if cap.annotated_marked_jpeg.is_empty() {
        log::warn!("computer capture dump: skip — empty annotated frame");
        return None;
    }

    let Some(root) = capture_root_dir() else {
        log::warn!("computer capture dump: could not resolve app data directory");
        return None;
    };
    let date = chrono::Local::now().format("%Y-%m-%d").to_string();
    let ts = chrono::Local::now().timestamp_millis();
    let conv_seg = sanitize_path_segment(conversation_id);
    let dir = root.join(&date).join(&conv_seg);
    if let Err(e) = fs::create_dir_all(&dir) {
        log::warn!("computer capture dump: create_dir_all {:?}: {e}", dir);
        return None;
    }

    let pfx = sanitize_path_segment(file_prefix);

    match tier {
        ComputerTier::Primary | ComputerTier::Intermediate => {
            write_bytes(&dir, &pfx, "annotated", "jpg", ts, &cap.annotated_marked_jpeg);
        }
        ComputerTier::Advanced => {
            if let Some(prev) = &cap.inject_before_action {
                write_bytes(&dir, &pfx, "screen_before_action", "jpg", ts, &prev.screen_jpeg);
                write_bytes(
                    &dir,
                    &pfx,
                    "zoom_pointer_before_action",
                    "png",
                    ts,
                    &prev.zoom_pointer_png,
                );
            }
            write_bytes(&dir, &pfx, "screen_raw_unmarked", "jpg", ts, &cap.raw_unmarked_jpeg);
            write_bytes(&dir, &pfx, "screen_after_action", "jpg", ts, &cap.raw_marked_jpeg);
            write_bytes(&dir, &pfx, "annotated", "jpg", ts, &cap.annotated_marked_jpeg);
            write_bytes(&dir, &pfx, "zoom_top", "png", ts, &cap.zoom_menu_bar_png);
            write_bytes(&dir, &pfx, "zoom_bottom", "png", ts, &cap.zoom_task_bar_png);
            write_bytes(&dir, &pfx, "zoom_pointer", "png", ts, &cap.zoom_pointer_png);
        }
    }

    let annotated_name = format!("{pfx}_annotated_{ts}.jpg");
    let rel = format!("{date}/{conv_seg}/{annotated_name}");
    log::info!(
        "computer capture dump: tier={} wrote under {:?}, annotated rel={rel}",
        tier.label(),
        dir
    );
    Some(rel)
}
