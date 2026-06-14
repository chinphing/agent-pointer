use serde::{Deserialize, Serialize};

use super::ffmpeg::{probe_ffmpeg_tools, FfmpegToolStatus};
use super::path_hint::append_attachment_paths;

pub const MEDIA_DEPS_MARKER: &str = "<!-- pointer-media-deps -->";
pub const UNSUPPORTED_ATTACHMENT_MARKER: &str = "<!-- pointer-unsupported-attachment -->";
pub const MEDIA_PROCESSING_FAILED_MARKER: &str = "<!-- pointer-media-processing-failed -->";

fn skill_recovery_block(marker: &str) -> String {
    format!(
        "{marker}\n\
若用户希望处理该附件，请先征得同意，按以下**优先级**执行（勿跳步；具体 skill/工具由你根据文件名、MIME 与「可用 Skills」索引自行判断）：\n\
**① 已安装/已启用的 Skill**\n\
- 查阅「可用 Skills」索引，判断是否有技能可处理该附件；有则 skill_read 并按技能正文执行（用下方 Saved attachment 的 Local path 配合 file_read）\n\
- 已有匹配技能时**禁止** npx skills find\n\
**② 查找并安装 Skill（无匹配时）**\n\
- skill_read(find-skills)，按需搜索并安装合适技能\n\
**③ 写代码 / 临时脚本（最后手段）**\n\
- 仅当 ①② 均不可行：terminal 一次性脚本或 run_subagent(coder)\n\
无需重发文件，可说「重试上一条附件」或由技能/工具直接给出结果。\n\
IM 渠道需回 Pointer 客户端继续。"
    )
}

fn with_paths(block: String, storage_rel_path: Option<&str>) -> String {
    append_attachment_paths(&block, storage_rel_path)
}

pub fn unsupported_attachment_hint(
    file_name: &str,
    mime: &str,
    storage_rel_path: Option<&str>,
) -> String {
    with_paths(
        format!(
            "[Attachment: {file_name}] 无法直接解析（类型 {mime}；未送入模型）。\n\n{}",
            skill_recovery_block(UNSUPPORTED_ATTACHMENT_MARKER)
        ),
        storage_rel_path,
    )
}

pub fn attachment_processing_failed_hint(
    file_name: &str,
    err: &str,
    storage_rel_path: Option<&str>,
) -> String {
    with_paths(
        format!(
            "[Attachment: {file_name}] 处理失败：{err}\n\n{}",
            skill_recovery_block(MEDIA_PROCESSING_FAILED_MARKER)
        ),
        storage_rel_path,
    )
}

pub fn audio_transcription_failed_hint(
    file_name: &str,
    err: &str,
    storage_rel_path: Option<&str>,
) -> String {
    with_paths(
        format!(
            "[Audio: {file_name}] 转写失败：{err}\n\n{}",
            skill_recovery_block(MEDIA_PROCESSING_FAILED_MARKER)
        ),
        storage_rel_path,
    )
}

pub fn pdf_processing_failed_hint(
    file_name: &str,
    detail: &str,
    storage_rel_path: Option<&str>,
) -> String {
    with_paths(
        format!(
            "[PDF: {file_name}] {detail}\n\n{}",
            skill_recovery_block(MEDIA_PROCESSING_FAILED_MARKER)
        ),
        storage_rel_path,
    )
}

pub fn image_understanding_failed_hint(
    file_name: &str,
    err: &str,
    storage_rel_path: Option<&str>,
) -> String {
    with_paths(
        format!(
            "[Image: {file_name}] 理解失败：{err}\n\n{}",
            skill_recovery_block(MEDIA_PROCESSING_FAILED_MARKER)
        ),
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
    with_paths(
        format!(
            "[Video: {file_name}] 无法处理：本机未检测到可用的 ffmpeg/ffprobe。\n\n\
{MEDIA_DEPS_MARKER}\n\
若用户希望处理 IM 视频，请先征得同意，然后：\n\
1. 调用 skill_read，skill_id=dev-env-setup\n\
2. 读取 references/ffmpeg.md 中对应操作系统章节\n\
3. 用 terminal 执行安装命令并验证：ffmpeg -version && ffprobe -version\n\
4. 安装成功后无需重发视频，请用户说「重试上一条视频」；ffmpeg 就绪后也可能自动重试\n\
IM 渠道会话中若用户不在 Pointer 客户端，回复简短说明：\
请在 Pointer 客户端中说「帮我安装 ffmpeg」。"
        ),
        storage_rel_path,
    )
}

pub fn video_frame_extraction_failed(
    file_name: &str,
    err: &str,
    storage_rel_path: Option<&str>,
) -> String {
    with_paths(
        format!(
            "[Video: {file_name}] ffmpeg 已安装，但未能从该视频提取画面帧：{err}\n\n\
说明：设置页显示「已就绪」仅表示 ffmpeg/ffprobe 可执行；单个视频仍可能因编码、文件损坏或格式不兼容而失败。\n\
请让用户说「重试上一条视频」、换用常见格式（如 H.264 MP4），无需重发文件。不要建议安装 ffmpeg。"
        ),
        storage_rel_path,
    )
}

pub fn im_user_hint_for_ffmpeg() -> &'static str {
    "处理视频需要在本机安装 ffmpeg。请在 Pointer 客户端打开对话并说「帮我安装 ffmpeg」。"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_hint_prioritizes_installed_skills_without_specific_skill_id() {
        let hint =
            unsupported_attachment_hint("report.docx", "application/octet-stream", None);
        assert!(hint.contains(UNSUPPORTED_ATTACHMENT_MARKER));
        assert!(hint.contains("可用 Skills"));
        assert!(hint.contains("禁止"));
        assert!(hint.contains("find-skills"));
        assert!(!hint.contains("excel-handler"));
        let pos_installed = hint.find("已安装").unwrap_or(0);
        let pos_find = hint.find("find-skills").unwrap_or(0);
        assert!(pos_installed < pos_find);
    }
}
