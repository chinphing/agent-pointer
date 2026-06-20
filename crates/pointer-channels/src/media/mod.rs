pub mod attachment;
pub mod audio_normalize;
mod resolve;

pub use attachment::{
    channel_media_max_bytes, enforce_max_bytes_for_kind, to_media_attachment, DownloadedMedia,
    CHANNEL_MEDIA_MAX_BYTES,
};
pub use resolve::resolve_inbound_attachments;
