use axum::routing::post;
use axum::Router;
use std::sync::Arc;

use super::handler::{handle_channel_webhook, WebhookState};

pub fn webhook_routes(state: Arc<WebhookState>) -> Router {
    Router::new()
        .route(
            "/webhooks/:channel/:account_id",
            post(handle_channel_webhook).get(handle_channel_webhook),
        )
        .with_state(state)
}
