use serde::{Deserialize, Serialize};

pub const MEDIA_DEPS_MARKER: &str = "<!-- pointer-media-deps -->";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaDepsStatus {
    pub ffmpeg_available: bool,
}

impl MediaDepsStatus {
    pub fn probe() -> Self {
        Self {
            ffmpeg_available: crate::media::ffmpeg::ffmpeg_available(),
        }
    }
}

pub fn video_ffmpeg_missing(file_name: &str) -> String {
    format!(
        "[Video: {file_name}] 无法处理：本机未检测到 ffmpeg/ffprobe。\n\n\
{MEDIA_DEPS_MARKER}\n\
若用户希望处理 IM 视频，请先征得同意，然后：\n\
1. 调用 skill_load_instructions，skill_id=dev-env-setup\n\
2. 读取 references/ffmpeg.md 中对应操作系统章节\n\
3. 用 terminal 执行安装命令并验证：ffmpeg -version && ffprobe -version\n\
4. 安装成功后请用户重新发送该视频，或说「重试上一条视频」\n\
IM 渠道会话中若用户不在 Pointer 客户端，回复简短说明：\
请在 Pointer 客户端中说「帮我安装 ffmpeg」。"
    )
}

pub fn im_user_hint_for_ffmpeg() -> &'static str {
    "处理视频需要在本机安装 ffmpeg。请在 Pointer 客户端打开对话并说「帮我安装 ffmpeg」。"
}
