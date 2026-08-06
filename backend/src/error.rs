use axum::{
    Json,
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Serialize;

#[derive(Debug, Serialize)]
struct ErrorResponse {
    detail: String,
}

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    detail: String,
    retry_after: Option<u64>,
}

impl ApiError {
    pub fn bad_request(detail: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            detail: detail.into(),
            retry_after: None,
        }
    }

    pub fn conflict(detail: impl Into<String>) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            detail: detail.into(),
            retry_after: None,
        }
    }

    pub fn forbidden(detail: impl Into<String>) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            detail: detail.into(),
            retry_after: None,
        }
    }

    pub fn not_found(detail: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            detail: detail.into(),
            retry_after: None,
        }
    }

    pub fn rate_limited(retry_after_secs: u64) -> Self {
        Self {
            status: StatusCode::TOO_MANY_REQUESTS,
            detail: "请求过于频繁，请稍后再试".to_string(),
            retry_after: Some(retry_after_secs),
        }
    }

    pub fn service_unavailable(detail: impl Into<String>) -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            detail: detail.into(),
            retry_after: None,
        }
    }

    pub fn unauthorized(detail: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            detail: detail.into(),
            retry_after: None,
        }
    }

    pub fn upstream(detail: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_GATEWAY,
            detail: detail.into(),
            retry_after: None,
        }
    }

    pub fn status(&self) -> StatusCode {
        self.status
    }

    pub fn detail(&self) -> &str {
        &self.detail
    }
}

impl From<mongodb::error::Error> for ApiError {
    fn from(error: mongodb::error::Error) -> Self {
        tracing::error!("数据库操作失败: {error}");
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            detail: "数据库操作失败".to_string(),
            retry_after: None,
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut response = (
            self.status,
            Json(ErrorResponse {
                detail: self.detail,
            }),
        )
            .into_response();
        if let Some(retry_after) = self.retry_after
            && let Ok(value) = HeaderValue::from_str(&retry_after.to_string())
        {
            response.headers_mut().insert(header::RETRY_AFTER, value);
        }
        response
    }
}

pub type ApiResult<T> = Result<Json<T>, ApiError>;
