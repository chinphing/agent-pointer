//! 统一错误响应：`{ "error": "<code>", "message": "<中文说明>" }`。

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

#[derive(Debug)]
pub enum ApiError {
    BadRequest(String),
    Unauthorized,
    Forbidden(String),
    NotFound(String),
    ServiceUnavailable(String),
    Internal(String),
}

impl ApiError {
    /// 把任意错误转成 500，同时把完整信息打到日志（不回给前端）。
    pub fn internal(err: impl std::fmt::Display) -> Self {
        log::error!("admin internal error: {err}");
        Self::Internal(err.to_string())
    }

    fn parts(&self) -> (StatusCode, &'static str, String) {
        match self {
            Self::BadRequest(msg) => (StatusCode::BAD_REQUEST, "bad_request", msg.clone()),
            Self::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                "未登录或会话已过期".to_string(),
            ),
            Self::Forbidden(msg) => (StatusCode::FORBIDDEN, "forbidden", msg.clone()),
            Self::NotFound(msg) => (StatusCode::NOT_FOUND, "not_found", msg.clone()),
            Self::ServiceUnavailable(msg) => (
                StatusCode::SERVICE_UNAVAILABLE,
                "service_unavailable",
                msg.clone(),
            ),
            Self::Internal(msg) => (StatusCode::INTERNAL_SERVER_ERROR, "internal", msg.clone()),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code, message) = self.parts();
        (status, Json(json!({ "error": code, "message": message }))).into_response()
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(err: anyhow::Error) -> Self {
        Self::internal(err)
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(err: sqlx::Error) -> Self {
        Self::internal(err)
    }
}

pub type ApiResult<T> = Result<T, ApiError>;

/// 请求体解析失败时给中文提示，而不是 serde 的英文原文。
pub fn bad_json(err: impl std::fmt::Display) -> ApiError {
    ApiError::BadRequest(format!("请求格式不正确：{err}"))
}
