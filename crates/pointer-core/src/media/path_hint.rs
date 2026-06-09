//! Inject saved attachment locations into model context (OpenClaw-style media path hints).

use super::store::media_abs_path;

pub const MEDIA_URI_SCHEME: &str = "pointer-media://";

/// Lines describing where a saved attachment lives on disk.
pub fn attachment_path_lines(storage_rel_path: Option<&str>) -> String {
    let Some(rel) = storage_rel_path.map(str::trim).filter(|s| !s.is_empty()) else {
        return String::new();
    };
    let uri = format!("{MEDIA_URI_SCHEME}{rel}");
    match media_abs_path(rel) {
        Ok(abs) => format!(
            "Saved attachment:\n- URI: {uri}\n- Local path: {}\n- Read with file_read using the local path.",
            abs.display()
        ),
        Err(e) => {
            log::warn!("media path_hint: resolve abs path for {rel}: {e:#}");
            format!("Saved attachment:\n- URI: {uri}")
        }
    }
}

pub fn append_attachment_paths(block: &str, storage_rel_path: Option<&str>) -> String {
    let paths = attachment_path_lines(storage_rel_path);
    if paths.is_empty() {
        block.to_string()
    } else {
        format!("{block}\n\n{paths}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_when_no_storage_path() {
        assert!(attachment_path_lines(None).is_empty());
        assert!(attachment_path_lines(Some("")).is_empty());
    }

    #[test]
    fn append_preserves_block_without_path() {
        let out = append_attachment_paths("[Attachment: a.zip] failed", None);
        assert_eq!(out, "[Attachment: a.zip] failed");
    }
}
