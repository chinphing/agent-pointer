use serde::{Deserialize, Serialize};

use super::ffmpeg::{probe_ffmpeg_tools, FfmpegToolStatus};
use super::filename::recovery_mode_for_filename;
use super::path_hint::append_recovery_paths;

pub const MEDIA_DEPS_MARKER: &str = "<!-- pointer-media-deps -->";
pub const UNSUPPORTED_ATTACHMENT_MARKER: &str = "<!-- pointer-unsupported-attachment -->";
pub const MEDIA_PROCESSING_FAILED_MARKER: &str = "<!-- pointer-media-processing-failed -->";

fn skill_recovery_block(marker: &str) -> String {
    format!(
        "{marker}\n\
If the user wants this attachment processed, ask for consent first, then follow this priority \
(do not skip steps; pick skills/tools from the filename, MIME, and Available Skills index):\n\
**1. Installed / enabled Skills**\n\
- Check the Available Skills index for a skill that handles this attachment; if found, skill_read \
and follow it (use the Saved attachment Local path below)\n\
- When a matching skill exists, do **not** run npx skills find\n\
**2. Find and install a Skill (when none match)**\n\
- skill_read(find-skills), search and install a suitable skill as needed\n\
**3. Code / one-off script (last resort)**\n\
- Only if 1 and 2 are unavailable: terminal one-off script or run_subagent(coder)\n\
The user does not need to resend the file; they can say \"retry last attachment\" or you can \
finish via skill/tool.\n\
For IM channel sessions, the user may need to continue in the Pointer desktop client."
    )
}

fn with_recovery_paths(block: String, file_name: &str, storage_rel_path: Option<&str>) -> String {
    append_recovery_paths(
        &block,
        storage_rel_path,
        recovery_mode_for_filename(file_name),
    )
}

pub fn unsupported_attachment_hint(
    file_name: &str,
    mime: &str,
    storage_rel_path: Option<&str>,
) -> String {
    with_recovery_paths(
        format!(
            "[Attachment: {file_name}] Could not parse directly (type {mime}; not sent to the model).\n\n{}",
            skill_recovery_block(UNSUPPORTED_ATTACHMENT_MARKER)
        ),
        file_name,
        storage_rel_path,
    )
}

pub fn attachment_processing_failed_hint(
    file_name: &str,
    err: &str,
    storage_rel_path: Option<&str>,
) -> String {
    with_recovery_paths(
        format!(
            "[Attachment: {file_name}] Processing failed: {err}\n\n{}",
            skill_recovery_block(MEDIA_PROCESSING_FAILED_MARKER)
        ),
        file_name,
        storage_rel_path,
    )
}

pub fn audio_transcription_failed_hint(
    file_name: &str,
    err: &str,
    storage_rel_path: Option<&str>,
) -> String {
    with_recovery_paths(
        format!(
            "[Audio: {file_name}] Transcription failed: {err}\n\n{}",
            skill_recovery_block(MEDIA_PROCESSING_FAILED_MARKER)
        ),
        file_name,
        storage_rel_path,
    )
}

pub fn pdf_processing_failed_hint(
    file_name: &str,
    detail: &str,
    storage_rel_path: Option<&str>,
) -> String {
    with_recovery_paths(
        format!(
            "[PDF: {file_name}] {detail}\n\n{}",
            skill_recovery_block(MEDIA_PROCESSING_FAILED_MARKER)
        ),
        file_name,
        storage_rel_path,
    )
}

pub fn image_understanding_failed_hint(
    file_name: &str,
    err: &str,
    storage_rel_path: Option<&str>,
) -> String {
    with_recovery_paths(
        format!(
            "[Image: {file_name}] Understanding failed: {err}\n\n{}",
            skill_recovery_block(MEDIA_PROCESSING_FAILED_MARKER)
        ),
        file_name,
        storage_rel_path,
    )
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaDepsStatus {
    /// True only when ffmpeg and ffprobe are found and respond to `-version`.
    pub ffmpeg_available: bool,
    pub status: FfmpegToolStatus,
    pub ffprobe_available: bool,
    pub ffmpeg_path: Option<String>,
    pub ffprobe_path: Option<String>,
    pub detail: Option<String>,
}

impl MediaDepsStatus {
    pub fn probe() -> Self {
        let tools = probe_ffmpeg_tools();
        Self {
            ffmpeg_available: tools.ffmpeg_available,
            status: tools.status,
            ffprobe_available: tools.ffprobe_available,
            ffmpeg_path: tools.ffmpeg_path,
            ffprobe_path: tools.ffprobe_path,
            detail: tools.detail,
        }
    }
}

pub fn video_ffmpeg_missing(file_name: &str, storage_rel_path: Option<&str>) -> String {
    with_recovery_paths(
        format!(
            "[Video: {file_name}] Cannot process: ffmpeg/ffprobe not available on this machine.\n\n\
{MEDIA_DEPS_MARKER}\n\
If the user wants IM video processed, ask for consent, then:\n\
1. skill_read with skill_id=dev-env-setup\n\
2. Read the OS chapter in references/ffmpeg.md\n\
3. Run install commands via terminal and verify: ffmpeg -version && ffprobe -version\n\
4. After install, the user need not resend the video; ask them to say \"retry last video\" \
(auto-retry may also run once ffmpeg is ready)\n\
If the user is not in the Pointer desktop client on an IM channel, reply briefly that they \
should open Pointer and ask to install ffmpeg."
        ),
        file_name,
        storage_rel_path,
    )
}

pub fn video_frame_extraction_failed(
    file_name: &str,
    err: &str,
    storage_rel_path: Option<&str>,
) -> String {
    with_recovery_paths(
        format!(
            "[Video: {file_name}] ffmpeg is installed but frame extraction failed: {err}\n\n\
Note: settings \"ready\" only means ffmpeg/ffprobe run; a single file may still fail due to \
codec, corruption, or format.\n\
Ask the user to say \"retry last video\" or use a common format (e.g. H.264 MP4); no need to \
resend. Do not suggest installing ffmpeg."
        ),
        file_name,
        storage_rel_path,
    )
}

pub fn im_user_hint_for_ffmpeg() -> &'static str {
    "Video processing requires ffmpeg on this machine. Open the conversation in the Pointer \
desktop client and ask to install ffmpeg."
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_hint_prioritizes_installed_skills_without_specific_skill_id() {
        let hint =
            unsupported_attachment_hint("report.docx", "application/octet-stream", None);
        assert!(hint.contains(UNSUPPORTED_ATTACHMENT_MARKER));
        assert!(hint.contains("Available Skills"));
        assert!(hint.contains("do **not** run npx skills find"));
        assert!(hint.contains("find-skills"));
        assert!(!hint.contains("excel-handler"));
        let pos_installed = hint.find("Installed / enabled Skills").unwrap_or(0);
        let pos_find = hint.find("find-skills").unwrap_or(0);
        assert!(pos_installed < pos_find);
    }

    #[test]
    fn unsupported_hint_does_not_suggest_file_read_for_binary() {
        let hint = unsupported_attachment_hint("paper.doc", "application/msword", None);
        assert!(!hint.contains("file_read"));
    }
}
