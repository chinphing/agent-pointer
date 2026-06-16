//! Inject saved attachment locations into model context when recovery is needed.

use super::filename::RecoveryPathMode;
use super::store::media_abs_path;

pub const MEDIA_URI_SCHEME: &str = "pointer-media://";

/// Lines describing where a saved attachment lives on disk (recovery / unsupported only).
pub fn attachment_recovery_path_lines(
    storage_rel_path: Option<&str>,
    mode: RecoveryPathMode,
) -> String {
    let Some(rel) = storage_rel_path.map(str::trim).filter(|s| !s.is_empty()) else {
        return String::new();
    };
    let uri = format!("{MEDIA_URI_SCHEME}{rel}");
    match media_abs_path(rel) {
        Ok(abs) => {
            let guidance = match mode {
                RecoveryPathMode::Binary => {
                    "For further processing, use a matching Skill or a terminal script with the local path below. \
Do not use file_read on binary files."
                }
                RecoveryPathMode::TextLike => {
                    "If this is UTF-8 plain text, you may try file_read; otherwise use a Skill or terminal."
                }
            };
            format!(
                "Saved attachment:\n- URI: {uri}\n- Local path: {}\n{guidance}",
                abs.display()
            )
        }
        Err(e) => {
            log::warn!("media path_hint: resolve abs path for {rel}: {e:#}");
            format!("Saved attachment:\n- URI: {uri}")
        }
    }
}

/// Legacy helper — prefer [`append_recovery_paths`] for failed/unsupported attachments.
pub fn attachment_path_lines(storage_rel_path: Option<&str>) -> String {
    attachment_recovery_path_lines(storage_rel_path, RecoveryPathMode::Binary)
}

pub fn append_recovery_paths(
    block: &str,
    storage_rel_path: Option<&str>,
    mode: RecoveryPathMode,
) -> String {
    let paths = attachment_recovery_path_lines(storage_rel_path, mode);
    if paths.is_empty() {
        block.to_string()
    } else {
        format!("{block}\n\n{paths}")
    }
}

pub fn append_attachment_paths(block: &str, storage_rel_path: Option<&str>) -> String {
    append_recovery_paths(block, storage_rel_path, RecoveryPathMode::Binary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_when_no_storage_path() {
        assert!(attachment_recovery_path_lines(None, RecoveryPathMode::Binary).is_empty());
        assert!(attachment_recovery_path_lines(Some(""), RecoveryPathMode::Binary).is_empty());
    }

    #[test]
    fn recovery_binary_mentions_no_file_read() {
        let out = append_recovery_paths(
            "[Attachment: a.zip] failed",
            None,
            RecoveryPathMode::Binary,
        );
        assert_eq!(out, "[Attachment: a.zip] failed");
    }
}
