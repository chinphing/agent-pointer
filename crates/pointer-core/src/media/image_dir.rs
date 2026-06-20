//! Directory of images as input for `media_understand` (mode=image).

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use super::video::MAX_VISION_FRAMES_PER_CALL;

/// Max images processed per `media_understand` call (same cap as vision frame batch).
pub const MAX_IMAGES_PER_CALL: usize = MAX_VISION_FRAMES_PER_CALL;
/// Default batch size when the user does not specify `imageStart` / `imageEnd`.
pub const DEFAULT_IMAGE_BATCH: usize = MAX_IMAGES_PER_CALL;

const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp", "heic", "heif"];

/// 1-based inclusive index range over a sorted directory image list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageDirRange {
    pub start: usize,
    pub end: usize,
    pub user_specified: bool,
}

impl ImageDirRange {
    pub fn image_count(&self) -> usize {
        self.end.saturating_sub(self.start).saturating_add(1)
    }

    pub fn normalize(total_images: usize, start: usize, end: usize) -> Result<Self> {
        if total_images == 0 {
            anyhow::bail!("image directory contains no supported image files");
        }
        if start == 0 || end == 0 {
            anyhow::bail!("imageStart and imageEnd are 1-based and must be >= 1");
        }
        if start > end {
            anyhow::bail!("imageStart ({start}) must be <= imageEnd ({end})");
        }
        if end > total_images {
            anyhow::bail!(
                "imageEnd ({end}) exceeds image file count ({total_images})"
            );
        }
        Ok(Self {
            start,
            end,
            user_specified: true,
        })
    }

    pub fn default_first_batch(total_images: usize) -> Result<Self> {
        if total_images == 0 {
            anyhow::bail!("image directory contains no supported image files");
        }
        Ok(Self {
            start: 1,
            end: total_images.min(DEFAULT_IMAGE_BATCH),
            user_specified: false,
        })
    }

    pub fn ensure_within_per_call_limit(&self) -> Result<()> {
        let count = self.image_count();
        if count > MAX_IMAGES_PER_CALL {
            anyhow::bail!(
                "requested {count} images ({}-{}); max {MAX_IMAGES_PER_CALL} per call — split into multiple media_understand calls with different imageStart/imageEnd",
                self.start,
                self.end
            );
        }
        Ok(())
    }
}

pub fn is_supported_image_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| {
            let lower = e.to_ascii_lowercase();
            IMAGE_EXTENSIONS.iter().any(|ext| *ext == lower)
        })
        .unwrap_or(false)
}

/// List image files directly under `dir` (non-recursive), sorted by file name.
pub fn list_image_files_in_dir(dir: &Path) -> Result<Vec<PathBuf>> {
    if !dir.is_dir() {
        anyhow::bail!("not a directory: {}", dir.display());
    }
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .with_context(|| format!("read image directory {}", dir.display()))?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| p.is_file() && is_supported_image_file(p))
        .collect();
    files.sort_by(|a, b| {
        a.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
            .cmp(
                &b.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_ascii_lowercase(),
            )
    });
    Ok(files)
}

pub fn format_image_dir_scope_notice(
    range: &ImageDirRange,
    total_images: usize,
    dir_display: &str,
) -> String {
    let scope = if range.start == range.end {
        format!("image {}", range.start)
    } else {
        format!("images {}-{}", range.start, range.end)
    };
    if range.user_specified {
        format!(
            "[Image directory scope: {scope} of {total_images} total in \"{dir_display}\" — extracted as requested.]"
        )
    } else {
        format!(
            "[Image directory scope: {scope} of {total_images} total in \"{dir_display}\" — user did not specify an index range; only the first {} image(s) were processed. Call again with imageStart/imageEnd for other images, or split into batches of at most {MAX_IMAGES_PER_CALL} images per call.]",
            range.end
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn image_dir_scope_notice_default_vs_user() {
        let default = ImageDirRange {
            start: 1,
            end: 200,
            user_specified: false,
        };
        let notice = format_image_dir_scope_notice(&default, 500, "/photos");
        assert!(notice.contains("did not specify an index range"));
        assert!(notice.contains("500 total"));

        let user = ImageDirRange {
            start: 5,
            end: 8,
            user_specified: true,
        };
        let notice = format_image_dir_scope_notice(&user, 50, "/photos");
        assert!(notice.contains("as requested"));
        assert!(notice.contains("images 5-8"));
    }

    #[test]
    fn list_image_files_sorts_and_filters() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("b.png"), b"x").unwrap();
        fs::write(dir.path().join("a.jpg"), b"x").unwrap();
        fs::write(dir.path().join("skip.txt"), b"x").unwrap();
        let files = list_image_files_in_dir(dir.path()).unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].file_name().unwrap(), "a.jpg");
        assert_eq!(files[1].file_name().unwrap(), "b.png");
    }
}
