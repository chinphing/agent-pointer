//! Re-export shared `MEDIA:` parsing from pointer-core.

pub use pointer_core::media::outbound_reply::{
    im_outbound_reply_source, split_reply_media, strip_outbound_media_markers,
};
