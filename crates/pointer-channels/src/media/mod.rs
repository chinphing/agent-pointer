pub mod attachment;
mod resolve;

pub use attachment::{to_media_attachment, DownloadedMedia, CHANNEL_MEDIA_MAX_BYTES};
pub use resolve::resolve_inbound_attachments;
