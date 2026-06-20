pub mod attachment_lookup;
pub mod access;
pub mod apply;
pub mod audio;
pub mod dashscope_audio;
pub mod dashscope_video;
pub mod image_dir;
pub mod manifest;
pub mod oss;
pub mod outbound_reply;
pub mod media_ref;
pub mod reply_attachments;
pub mod resolve;
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

pub use oss::{
    delete_temp_object, resolve_media_oss_config, upload_composer_video_bytes,
    upload_composer_video_from_path, upload_temp_video, ComposerVideoUploadResult, OssTempVideo,
};
pub use image_dir::{
    format_image_dir_scope_notice, list_image_files_in_dir, ImageDirRange, DEFAULT_IMAGE_BATCH,
    MAX_IMAGES_PER_CALL,
};
pub use apply::apply_media_to_history;
pub use attachment_lookup::find_attachment_by_media_ref;
pub use understand::{
    describe_image_with_model, describe_images_with_model, describe_pdf_pages_with_model,
    describe_video_with_model, focus_extracted_pdf_text_with_goal, transcribe_audio_with_model,
};
pub use pdf::{
    extract_pdf_page_images_base64, extract_pdf_page_images_base64_range,
    extract_pdf_text_sorted, extract_pdf_text_sorted_range, format_pdf_scope_notice,
    pdf_page_count, PdfPageRange, DEFAULT_PDF_PAGE_END, MAX_PDF_PAGES_PER_CALL,
};
pub use capabilities::model_supports_vision;
pub use deps_hint::MediaDepsStatus;
pub use ffmpeg::{ffmpeg_available, probe_ffmpeg_tools, FfmpegToolProbe, FfmpegToolStatus};
pub use access::is_user_filesystem_path;
pub use outbound_reply::{
    im_outbound_reply_source, reply_media_source, split_reply_media, strip_outbound_media_markers,
};
pub use manifest::{
    append_user_attachments_api_context, attachment_summaries_json, attachment_summary_json,
    format_user_attachments_api_manifest, ATTACHMENT_NEEDS_INTENT_MARKER,
    USER_ATTACHMENTS_MARKER,
};
pub use media_ref::{read_media_ref_bytes, resolve_media_ref};
pub use resolve::{is_storage_rel_path, resolve_local_media_path};
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
pub use video::{
    extract_video_frame_base64s, extract_video_frame_base64s_with_range,
    format_video_scope_notice, format_video_shrink_notice, is_video_file_name,
    prepare_video_bytes_for_range, probe_video_duration, download_video_from_url,
    remux_video_faststart, shrink_video_to_max, video_mime_from_file_name, VideoShrinkReport, VideoTimeRange,
    DEFAULT_FRAMES_PER_SECOND, MAX_VIDEO_API_BASE64_BYTES, MAX_VIDEO_BYTES,
    MAX_VISION_FRAMES_PER_CALL, COMPOSER_VIDEO_ADVISORY_BYTES, max_raw_bytes_for_native_video_api,
    video_data_url_encoded_len,
};
