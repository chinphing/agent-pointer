pub mod access;
pub mod apply;
pub mod audio;
pub mod dashscope_audio;
pub mod dashscope_video;
pub mod manifest;
pub mod outbound_reply;
pub mod media_ref;
pub mod reply_attachments;
pub mod capabilities;
pub mod deps_hint;
pub mod filename;
pub mod path_hint;
pub mod retry;
pub mod ffmpeg;
pub mod office;
pub mod pdf;
pub mod store;
pub mod token;
pub mod understand;
pub mod video;

pub use apply::apply_media_to_history;
pub use understand::{
    describe_image_with_model, describe_pdf_pages_with_model, describe_video_with_model,
    focus_extracted_pdf_text_with_goal, transcribe_audio_with_model,
};
pub use pdf::{extract_pdf_page_images_base64, extract_pdf_text_sorted};
pub use capabilities::model_supports_vision;
pub use deps_hint::MediaDepsStatus;
pub use ffmpeg::{ffmpeg_available, probe_ffmpeg_tools, FfmpegToolProbe, FfmpegToolStatus};
pub use access::is_user_filesystem_path;
pub use outbound_reply::{
    im_outbound_reply_source, reply_media_source, split_reply_media, strip_outbound_media_markers,
};
pub use manifest::{
    append_user_attachments_api_context, format_user_attachments_api_manifest,
    ATTACHMENT_NEEDS_INTENT_MARKER, USER_ATTACHMENTS_MARKER,
};
pub use media_ref::{read_media_ref_bytes, resolve_media_ref};
pub use path_hint::{
    append_attachment_paths, append_recovery_paths, attachment_path_lines,
    attachment_recovery_path_lines, MEDIA_URI_SCHEME,
};
pub use filename::{
    merge_inbound_filename, normalize_inbound_filename, recovery_mode_for_filename,
    RecoveryPathMode,
};
pub use reply_attachments::attachments_from_reply_paths;
pub use store::{
    media_abs_path, read_chat_media_preview, read_media_bytes, read_media_ref_preview,
    save_attachment_bytes,
};
pub use video::{extract_video_frame_base64s, remux_video_faststart};
