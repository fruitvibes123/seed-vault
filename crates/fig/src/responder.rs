use serde::Serialize;
use std::time::Duration;
use thiserror::Error;

/// JSON error contract for HTTP JSON APIs.
///
/// Each variant maps to an HTTP status ([`ApiError::status_code`]); the
/// per-framework response adapters (rocket `Responder` / axum `IntoResponse`)
/// emit a `{error, status}` JSON body with
/// `Content-Type: application/json; charset=utf-8`.
///
/// Variants that previously hardcoded a fixed message
/// (`NotFound`, `Forbidden`, `Unauthorized`, `Internal`) now carry an
/// `Option<String>` override. Pass `None` to use the default message;
/// pass `Some("…")` to override (e.g., when a spec pins exact wording
/// like `"file not found"` or `"authentication required"`). Added
/// 2026-05-19 after the download-service agent-productivity experiment
/// surfaced that 3 of 3 Rocket agents rejected the crate over
/// fixed-message inflexibility.
#[derive(Debug, Error)]
pub enum ApiError {
    #[error("{}", message.as_deref().unwrap_or("not found"))]
    NotFound { message: Option<String> },

    #[error("{0}")]
    BadRequest(String),

    #[error("{}", message.as_deref().unwrap_or("forbidden"))]
    Forbidden { message: Option<String> },

    #[error("{}", message.as_deref().unwrap_or("unauthorized"))]
    Unauthorized { message: Option<String> },

    #[error("{0}")]
    Conflict(String),

    #[error("{0}")]
    Gone(String),

    #[error("{0}")]
    PayloadTooLarge(String),

    #[error("{0}")]
    Unprocessable(String),

    #[error("{message}")]
    TooManyRequests {
        message: String,
        retry_after: Option<Duration>,
    },

    /// 416 Range Not Satisfiable per RFC 9110 §15.5.17. `content_range`
    /// emits the unsatisfied-range indicator `Content-Range: bytes */<total>`
    /// (`Some("bytes */<total>")`) which RFC 9110 §14.4 says SHOULD accompany
    /// 416 responses to a Range request. Added 2026-05-19 after the
    /// download-service experiment surfaced that the crate lacked a 416
    /// variant; all three Rocket agents that needed it rolled their own.
    #[error("{}", message.as_deref().unwrap_or("range not satisfiable"))]
    RangeNotSatisfiable {
        message: Option<String>,
        content_range: Option<String>,
    },

    #[error("{}", message.as_deref().unwrap_or("internal error"))]
    Internal { message: Option<String> },
}

/// Convenience constructors using the default message. Pre-2026-05-19
/// the unit-variant forms `ApiError::NotFound` etc. existed; these
/// associated functions preserve the same call-site ergonomics
/// (`ApiError::not_found()`) for callers that don't need a custom
/// message. Constructors with custom messages use the struct-variant
/// form directly: `ApiError::NotFound { message: Some("file not found".into()) }`.
impl ApiError {
    pub fn not_found() -> Self {
        Self::NotFound { message: None }
    }
    pub fn forbidden() -> Self {
        Self::Forbidden { message: None }
    }
    pub fn unauthorized() -> Self {
        Self::Unauthorized { message: None }
    }
    pub fn internal() -> Self {
        Self::Internal { message: None }
    }
}

/// The shared `{error, status}` JSON body shape. Kept crate-private: it is the
/// wire contract, not public API. Both framework adapters serialize THIS type
/// so their body bytes cannot drift.
#[derive(Serialize)]
pub(crate) struct Body<'a> {
    pub(crate) error: &'a str,
    pub(crate) status: u16,
}

impl ApiError {
    /// The raw HTTP status code for this error — the SINGLE source of truth
    /// shared by every framework adapter (rocket + axum), so their status
    /// mapping cannot drift. The adapters build their framework-native status
    /// value from this `u16`.
    pub(crate) fn status_code(&self) -> u16 {
        match self {
            ApiError::NotFound { .. } => 404,
            ApiError::BadRequest(_) => 400,
            ApiError::Forbidden { .. } => 403,
            ApiError::Unauthorized { .. } => 401,
            ApiError::Conflict(_) => 409,
            ApiError::Gone(_) => 410,
            ApiError::PayloadTooLarge(_) => 413,
            ApiError::Unprocessable(_) => 422,
            ApiError::TooManyRequests { .. } => 429,
            ApiError::RangeNotSatisfiable { .. } => 416,
            ApiError::Internal { .. } => 500,
        }
    }
}

/// Rocket response adapter (feature = "rocket"). Builds its `Status` from the
/// shared [`ApiError::status_code`] and serializes the shared [`Body`], so it
/// stays byte-identical to the axum adapter.
#[cfg(feature = "rocket")]
mod rocket_responder {
    use super::{ApiError, Body};
    use rocket::http::Status;
    use rocket::request::Request;
    use rocket::response::{self, Responder, Response};
    use std::io::Cursor;

    impl<'r> Responder<'r, 'static> for ApiError {
        fn respond_to(self, _req: &Request<'_>) -> response::Result<'static> {
            let code = self.status_code();
                                                                               
                                                                           
                                                                            
                                                           
            let status = Status::new(code);
            let msg = self.to_string();
            let body = serde_json::to_vec(&Body {
                error: &msg,
                status: code,
            })
            .expect("serializing a String + u16 cannot fail");

            let mut resp = Response::build();
            resp.status(status)
                                                                              
                                                                            
                                                                       
                                                                             
                                                     
                .raw_header("Content-Type", "application/json; charset=utf-8")
                .sized_body(body.len(), Cursor::new(body));

            match &self {
                ApiError::TooManyRequests {
                    retry_after: Some(d),
                    ..
                } => {
                    resp.raw_header("Retry-After", d.as_secs().to_string());
                }
                ApiError::RangeNotSatisfiable {
                    content_range: Some(cr),
                    ..
                } => {
                    resp.raw_header("Content-Range", cr.clone());
                }
                _ => {}
            }

            resp.ok()
        }
    }
}
