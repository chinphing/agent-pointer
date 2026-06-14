//! Immutable session identity refs shared across agent orchestration phases.

use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use super::super::app_state::AppState;
use super::super::StreamTx;

/// Shared session handles for one chat run (stream, app state, conversation id, cancel).
pub struct SessionRefs<'a> {
    pub stream: &'a StreamTx,
    pub state: &'a AppState,
    pub conversation_id: &'a str,
    pub cancel: &'a CancellationToken,
}

/// Lead single-agent loop: `AppState` is held as `Arc` at the call site.
pub struct SessionRefsArc<'a> {
    pub stream: &'a StreamTx,
    pub state: Arc<AppState>,
    pub conversation_id: &'a str,
    pub cancel: CancellationToken,
}


