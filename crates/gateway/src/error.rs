use axum::extract::rejection::{JsonRejection, PathRejection, QueryRejection};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;
use social_core::Error;

/// Maps domain errors to the error envelope in docs/SPEC.md §4.3.
pub struct ApiError(pub Error);

impl From<Error> for ApiError {
    fn from(e: Error) -> Self {
        Self(e)
    }
}

impl From<JsonRejection> for ApiError {
    fn from(r: JsonRejection) -> Self {
        Self(Error::Invalid(r.body_text()))
    }
}

impl From<PathRejection> for ApiError {
    fn from(r: PathRejection) -> Self {
        Self(Error::Invalid(r.body_text()))
    }
}

impl From<QueryRejection> for ApiError {
    fn from(r: QueryRejection) -> Self {
        Self(Error::Invalid(r.body_text()))
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code) = match &self.0 {
            Error::Invalid(_) => (StatusCode::BAD_REQUEST, "invalid_request"),
            Error::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized"),
            Error::BadCredentials => (StatusCode::UNAUTHORIZED, "invalid_credentials"),
            Error::Forbidden => (StatusCode::FORBIDDEN, "forbidden"),
            Error::NotFound(_) => (StatusCode::NOT_FOUND, "not_found"),
            Error::Conflict(_) => (StatusCode::CONFLICT, "conflict"),
            Error::RateLimited { .. } => (StatusCode::TOO_MANY_REQUESTS, "rate_limited"),
            Error::Unavailable => (StatusCode::SERVICE_UNAVAILABLE, "unavailable"),
            Error::Internal(detail) => {
                tracing::error!(error = %detail, "internal error");
                (StatusCode::INTERNAL_SERVER_ERROR, "internal")
            }
        };
        let message = match &self.0 {
            Error::Internal(_) => "internal error".to_owned(),
            e => e.to_string(),
        };
        let mut res = (status, Json(json!({ "error": { "code": code, "message": message } }))).into_response();
        if let Error::RateLimited { retry_after_secs } = self.0 {
            res.headers_mut().insert(header::RETRY_AFTER, HeaderValue::from(retry_after_secs));
        }
        res
    }
}
