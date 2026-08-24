//! Axum response adapter (feature = "axum").
//!
//! `impl IntoResponse for ApiError`, byte-identical to the rocket `Responder`:
//! the same shared [`crate::ApiError::status_code`] drives the `StatusCode`,
//! the same shared [`crate::Body`] is serialized, and the exact wire bytes
//! `Content-Type: application/json; charset=utf-8` (+ `Retry-After` /
//! `Content-Range` where applicable) are emitted.

use crate::ApiError;
use crate::responder::Body;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = StatusCode::from_u16(self.status_code())
            .expect("fig maps every variant to a standard HTTP status code");
        let body = serde_json::to_vec(&Body {
            error: &self.to_string(),
            status: status.as_u16(),
        })
        .expect("serializing a String + u16 cannot fail");

        let mut resp = (status, body).into_response();
                                                                                
                                                                                  
        resp.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json; charset=utf-8"),
        );

        match &self {
            ApiError::TooManyRequests {
                retry_after: Some(d),
                ..
            } => {
                resp.headers_mut().insert(
                    header::RETRY_AFTER,
                    d.as_secs()
                        .to_string()
                        .parse()
                        .expect("digits are a valid header value"),
                );
            }
            ApiError::RangeNotSatisfiable {
                content_range: Some(cr),
                ..
            } => {
                resp.headers_mut().insert(
                    header::CONTENT_RANGE,
                    cr.parse().expect("valid Content-Range header value"),
                );
            }
            _ => {}
        }

        resp
    }
}

#[cfg(test)]
mod tests {
    use crate::ApiError;
    use axum::response::{IntoResponse, Response};
    use http_body_util::BodyExt;
    use std::time::Duration;

    struct ResponseFacts {
        status: u16,
        content_type: Option<String>,
        retry_after: Option<String>,
        content_range: Option<String>,
        body: String,
    }

    fn header_str(resp: &Response, name: &str) -> Option<String> {
        resp.headers()
            .get(name)
            .map(|v| v.to_str().expect("ascii header value").to_string())
    }

    async fn facts(err: ApiError) -> ResponseFacts {
        let resp = err.into_response();
                                                           
        let status = resp.status().as_u16();
        let content_type = header_str(&resp, "content-type");
        let retry_after = header_str(&resp, "retry-after");
        let content_range = header_str(&resp, "content-range");
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let body = String::from_utf8(bytes.to_vec()).unwrap();
        ResponseFacts {
            status,
            content_type,
            retry_after,
            content_range,
            body,
        }
    }

    /// Every variant's (status, body-json, Content-Type) must match the
    /// rocket `Responder` byte-for-byte. The expected bodies below are the
    /// SAME literals asserted by the rocket integration tests
    /// (`tests/integration_test.rs`) — this table is the drift guard.
    #[tokio::test]
    async fn every_variant_status_body_content_type_parity() {
        let cases: Vec<(ApiError, u16, &str)> = vec![
            (
                ApiError::not_found(),
                404,
                r#"{"error":"not found","status":404}"#,
            ),
            (
                ApiError::NotFound {
                    message: Some("file not found".to_string()),
                },
                404,
                r#"{"error":"file not found","status":404}"#,
            ),
            (
                ApiError::BadRequest("missing field 'x'".to_string()),
                400,
                r#"{"error":"missing field 'x'","status":400}"#,
            ),
            (
                ApiError::forbidden(),
                403,
                r#"{"error":"forbidden","status":403}"#,
            ),
            (
                ApiError::unauthorized(),
                401,
                r#"{"error":"unauthorized","status":401}"#,
            ),
            (
                ApiError::Unauthorized {
                    message: Some("authentication required".to_string()),
                },
                401,
                r#"{"error":"authentication required","status":401}"#,
            ),
            (
                ApiError::Conflict("Operation blocked by a related record".to_string()),
                409,
                r#"{"error":"Operation blocked by a related record","status":409}"#,
            ),
            (
                ApiError::Gone(
                    "The resource you're looking for has been revoked or removed".to_string(),
                ),
                410,
                r#"{"error":"The resource you're looking for has been revoked or removed","status":410}"#,
            ),
            (
                ApiError::PayloadTooLarge("File too large (max 10 MiB)".to_string()),
                413,
                r#"{"error":"File too large (max 10 MiB)","status":413}"#,
            ),
            (
                ApiError::Unprocessable("unprocessable entity".to_string()),
                422,
                r#"{"error":"unprocessable entity","status":422}"#,
            ),
            (
                ApiError::TooManyRequests {
                    message: "Too many login attempts".to_string(),
                    retry_after: None,
                },
                429,
                r#"{"error":"Too many login attempts","status":429}"#,
            ),
            (
                ApiError::RangeNotSatisfiable {
                    message: None,
                    content_range: None,
                },
                416,
                r#"{"error":"range not satisfiable","status":416}"#,
            ),
            (
                ApiError::internal(),
                500,
                r#"{"error":"internal error","status":500}"#,
            ),
        ];

        for (err, want_status, want_body) in cases {
            let f = facts(err).await;
            assert_eq!(f.status, want_status, "status for body {want_body}");
            assert_eq!(
                f.content_type.as_deref(),
                Some("application/json; charset=utf-8"),
                "exact Content-Type wire bytes for body {want_body}"
            );
            assert_eq!(f.body, want_body, "body json");
        }
    }

    #[tokio::test]
    async fn too_many_requests_with_window_emits_retry_after() {
        let f = facts(ApiError::TooManyRequests {
            message: "Too many pairing attempts".to_string(),
            retry_after: Some(Duration::from_secs(60)),
        })
        .await;
        assert_eq!(f.status, 429);
        assert_eq!(f.retry_after.as_deref(), Some("60"));
        assert_eq!(
            f.body,
            r#"{"error":"Too many pairing attempts","status":429}"#
        );
    }

    #[tokio::test]
    async fn too_many_requests_without_window_emits_no_retry_after() {
        let f = facts(ApiError::TooManyRequests {
            message: "Too many login attempts".to_string(),
            retry_after: None,
        })
        .await;
        assert_eq!(f.retry_after, None);
    }

    #[tokio::test]
    async fn range_not_satisfiable_emits_content_range() {
        let f = facts(ApiError::RangeNotSatisfiable {
            message: None,
            content_range: Some("bytes */1024".to_string()),
        })
        .await;
        assert_eq!(f.status, 416);
        assert_eq!(f.content_range.as_deref(), Some("bytes */1024"));
        assert_eq!(f.body, r#"{"error":"range not satisfiable","status":416}"#);
    }

    #[tokio::test]
    async fn range_not_satisfiable_without_content_range_emits_none() {
        let f = facts(ApiError::RangeNotSatisfiable {
            message: None,
            content_range: None,
        })
        .await;
        assert_eq!(f.content_range, None);
    }
}
