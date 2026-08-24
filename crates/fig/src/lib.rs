//! JSON error contract for Rust JSON APIs.
//!
//! A framework-agnostic [`ApiError`] enum with ONE shared source of truth for
//! its `{error, status}` JSON body and HTTP status mapping, plus per-framework
//! response adapters selected by feature flags:
//!
//! - `rocket` (default) — `impl Responder for ApiError` + [`catchers`] for
//!   JSON-API prefix registration.
//! - `axum` — `impl axum::response::IntoResponse for ApiError`.
//!
//! Both adapters emit byte-identical bodies and headers — including the exact
//! `Content-Type: application/json; charset=utf-8` wire bytes plus `Retry-After`
//! (`TooManyRequests`) and `Content-Range` (`RangeNotSatisfiable`). The shared
//! `status_code()` + `Body` core is the drift guard this crate exists to
//! provide: a divergence between the two adapters is exactly the failure it
//! prevents.
//!
//! See README.md for usage.

mod responder;

#[cfg(feature = "rocket")]
mod catchers;

#[cfg(feature = "axum")]
mod axum_impl;

#[cfg(feature = "rocket")]
pub use catchers::catchers;
pub use responder::ApiError;
