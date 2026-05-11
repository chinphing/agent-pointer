//! Persist full screen-capture bundles for debugging under the app data directory.

use crate::agents::computer::screen;
use crate::models::ComputerAnnotatedPreview;
use crate::agents::computer::ScreenCaptureResult;
use anyhow::Context;
use std::fs;
use std::path::PathBuf;

/// Day-level folders older than this are removed on app startup (desktop).
pub const CAPTURE_RETENTION_DAYS: i64 = 7;

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

/// Read a saved annotated PNG and return the same shape as live preview.
pub fn read_computer_capture_preview(rel: &str) -> anyhow::Result<ComputerAnnotatedPreview> {
    let path = safe_capture_file_path(rel).context("invalid or disallowed capture path")?;
    let bytes = fs::read(&path).with_context(|| format!("read {:?}", path))?;
    Ok(ComputerAnnotatedPreview {
        image_base64: screen::encode_image_to_base64(&bytes),
        caption: "本圈标注画面".into(),
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

/// Writes JPEG/PNG files as `{prefix}_{type}_{timestamp_ms}.{ext}` under
/// `{data_dir}/PointerApp/computer-captures/{YYYY-MM-DD}/{conversation_id}/` — same app root as settings and skills ([`crate::storage::app_data_dir`]).
///
/// Returns the **path relative to `computer-captures/`** of the annotated PNG (for lazy UI load), e.g.
/// `2026-05-11/my_conv/msg_abc_annotated_1715423.png`.
pub fn save_computer_capture_debug(
    conversation_id: &str,
    file_prefix: &str,
    cap: &ScreenCaptureResult,
) -> Option<String> {
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
    // Aligns with `[Screen after action]` in `[CUR_SCREEN]` copy: full-frame marked JPEG for this turn.
    const SLOT_SCREEN_AFTER_ACTION: &str = "screen_after_action";
    let write_one = |name: &str, ext: &str, bytes: &[u8]| {
        let path = dir.join(format!("{pfx}_{name}_{ts}.{ext}"));
        if let Err(e) = fs::write(&path, bytes) {
            log::warn!("computer capture dump: write {:?}: {e}", path);
        }
    };

    if let Some(prev) = &cap.inject_previous_raw_jpeg {
        write_one("screen_before_action", "jpg", prev);
    }
    write_one(SLOT_SCREEN_AFTER_ACTION, "jpg", &cap.raw_marked_jpeg);
    write_one("annotated", "png", &cap.annotated_marked_png);
    write_one("zoom_top", "png", &cap.zoom_menu_bar_png);
    write_one("zoom_bottom", "png", &cap.zoom_task_bar_png);
    write_one("zoom_pointer", "png", &cap.zoom_pointer_png);

    let annotated_name = format!("{pfx}_annotated_{ts}.png");
    let rel = format!("{date}/{conv_seg}/{annotated_name}");
    log::info!("computer capture dump: wrote capture set under {:?}, annotated rel={rel}", dir);
    Some(rel)
}
