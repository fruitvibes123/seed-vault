//! Compile-time IDOR-safe resource loading.
//!
//! A framework-agnostic core (the [`Resource`] trait + [`PathParamKind`]) plus
//! per-framework `Scoped<T>` adapters selected by feature flags:
//!
//! - `rocket` (default) — `pomelo::Scoped`, a Rocket `FromRequest` request guard.
//! - `axum` — `pomelo::axum::Scoped`, an Axum `FromRequestParts` extractor
//!   (also exposing `ScopeCache` and `GuardRejection`).
//!
//! Both adapters enforce the SAME contract: the resource id comes from the
//! route's FIRST dynamic path segment; the scope comes from a request-local
//! cache the consumer's auth guard populates; and `load_scoped` returning
//! `Ok(None)` maps to 404 (the 404-not-403 discipline). See README.md for usage.

#[cfg(feature = "rocket")]
mod guard;

#[cfg(feature = "rocket")]
pub use guard::Scoped;

#[cfg(feature = "axum")]
mod axum_guard;

/// Axum adapter surface (feature = "axum"):
/// `pomelo::axum::{Scoped, ScopeCache, GuardRejection}` — the Axum
/// `FromRequestParts` extractor twin of the Rocket `Scoped` guard.
///
/// The module is deliberately named `axum` to mirror the axum crate's shape;
/// inside `axum_guard.rs` the axum CRATE is referenced as `::axum` so this
/// submodule name does not shadow it.
#[cfg(feature = "axum")]
pub mod axum {
    pub use crate::axum_guard::{GuardRejection, ScopeCache, Scoped};
}

use async_trait::async_trait;
use sqlx::SqlitePool;

/// Declares the threat-model intent of a Resource's path parameter.
///
/// Contract: every
/// Resource impl must explicitly state whether the path-param-index-0
/// it reads is an opaque identifier (`Identifier`) or a load-bearing
/// secret (`Secret`). The two carry very different threat models:
///
/// - `Identifier` — the path-param is a non-secret database id. The
///   resource's secrecy comes from the SQL `WHERE id=? AND scope=?`
///   predicate (which `load_scoped` is required to apply). An attacker
///   guessing the id gets a 404 unless they're already in the right
///   scope. Logging the id, putting it in HTTP `Referer`, surfacing it
///   in HAProxy access logs, etc. is acceptable.
///
/// - `Secret` — the path-param's BYTES are the access credential
///   (e.g., an 8-char Crockford pairing code, a share-link token).
///   Anyone who learns the bytes can act. Logging requires `redact_tokens`
///   coverage of the route prefix; brute-force requires per-attempt
///   throttling; entropy requirements bound the URL-shape design.
///
/// Why this lives at compile time on the trait: a future maintainer
/// adding a Resource impl is forced to pick a kind. The choice is
/// audit-grep-discoverable; a discipline-only convention (the prior
/// shape) gets eroded silently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathParamKind {
    /// Non-secret database identifier. Secrecy comes from the
    /// `WHERE scope=?` predicate, not from id-bytes confidentiality.
    Identifier,
    /// Load-bearing secret (token, code, capability). Requires log
    /// redaction at the route prefix + per-attempt throttle + brute-
    /// force entropy in the URL-shape design.
    Secret,
}

/// Implemented by every scope-owned resource type.
///
/// `load_scoped` returns `Ok(None)` for BOTH "resource doesn't exist" AND
/// "resource exists in a different scope" — the 404-not-403 discipline
/// is structural.
#[async_trait]
pub trait Resource: Sized + Send {
    /// The scope type this resource is owned by. For a household-scoped
    /// app this is `HouseholdId`. Bound `sqlx::Type<sqlx::Sqlite>` lets
    /// scope values bind directly in `query!` parameters. `Debug` is
    /// required so the `Scoped<T>` guard can log the scope on the
    /// `InternalServerError` path.
    type Scope: std::fmt::Debug + Copy + Send + Sync + sqlx::Type<sqlx::Sqlite> + 'static;

    /// Declares whether `Scoped<Self>`'s path-param-0 is a
    /// non-secret identifier (the default for scope-owned database rows)
    /// or a load-bearing secret. See [`PathParamKind`] for the threat-model
    /// implications. Today's `Scoped<T>` use cases are exclusively
    /// `Identifier`; the const exists so a future use case that wants
    /// `Scoped<SecretFoo>`-style semantics has to declare intent upfront
    /// rather than silently inherit the identifier-conventions of every
    /// other Resource impl.
    const ID_PARAM_KIND: PathParamKind;

    async fn load_scoped(
        id: i64,
        scope: Self::Scope,
        pool: &SqlitePool,
    ) -> sqlx::Result<Option<Self>>;
}
