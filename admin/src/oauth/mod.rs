//! OAuth 2.0 授权码 + PKCE，以及 partner 接口。

pub mod authorize;
pub mod clients;
pub mod code;
pub mod partner;
pub mod token;

pub use code::CodeStore;

use axum::routing::{get, post};
use axum::Router;

use crate::state::Shared;

pub fn router() -> Router<Shared> {
    Router::new()
        .route(
            "/oauth/authorize",
            get(authorize::page).post(authorize::submit),
        )
        .route("/auth/oauth/exchange-code", post(token::exchange_code))
        .route("/auth/app/token", post(token::app_token))
        .route("/auth/app/logout", post(token::app_logout))
        .route("/auth/partner/llm-credentials", get(partner::llm_credentials))
        .route("/auth/partner/balance", get(partner::balance))
        .route("/auth/partner/token-usage", post(partner::token_usage))
        .route("/auth/me", get(partner::me))
}
