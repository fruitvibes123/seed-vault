//! Axum `Scoped<T>` extractor (feature = "axum") — the same contract as the
//! Rocket guard:
//!
//! - **id** — the FIRST dynamic path param of the matched route.
//!   `RawPathParams` yields params in path-template declaration order, so
//!   position 0 is the axum equivalent of Rocket's first `<…>` segment. Static
//!   prefixes (`/api/v1/things/{id}`) are NOT params, so they cannot shadow the
//!   id — the class of bug the pre-`routed_segment` Rocket draft had.
//! - **scope** — read from a `ScopeCache<T::Scope>` request extension, which
//!   exactly one populator guard (e.g. the consumer's `MemberOf`) inserts
//!   before `Scoped<T>` runs. Axum extensions are LAST-write-wins (the inverse
//!   of Rocket's `local_cache` first-write-wins) — the one-populator invariant
//!   is re-expressed by the consumer's convention enforcer, not here.
//! - **load** — `T::load_scoped(id, scope, pool)`; `Ok(None)` → 404, covering
//!   both "absent" AND "other scope" (404-not-403, indistinguishable by design).
//!
//! `pool` is pulled from the router state via `FromRef<S>` for `SqlitePool`.
//!
//! The axum CRATE is referenced as `::axum` throughout so the `pomelo::axum`
//! re-export module (lib.rs) does not shadow it.

use crate::Resource;
use ::axum::extract::{FromRef, FromRequestParts, RawPathParams};
use ::axum::http::StatusCode;
use ::axum::http::request::Parts;
use ::axum::response::{IntoResponse, Response};
use sqlx::SqlitePool;

/// Type-keyed scope cache. A populator extractor inserts it into the request
/// extensions; `Scoped<T>` reads it. Mirrors the Rocket guard's
/// `request.local_cache::<Option<T::Scope>>()` channel.
#[derive(Clone, Copy, Debug)]
pub struct ScopeCache<S: Copy + Send + Sync + 'static>(pub S);

/// Axum extractor: loads `T` by id within the cached scope.
///
/// Destructure `Scoped(resource)` in handlers. See the module docs for the
/// id / scope / load contract it enforces.
pub struct Scoped<T: Resource>(pub T);

/// Rejection ladder — mirrors the Rocket guard's statuses EXACTLY.
#[derive(Debug)]
pub enum GuardRejection {
    /// 401 — no `ScopeCache` in the request extensions (the populator guard was
    /// absent or failed silently; must be unreachable in prod by construction).
    NoScope,
    /// 400 — no dynamic path param, or the first one is not an `i64`.
    BadId,
    /// 404 — absent OR other-scope (indistinguishable, by design: 404-not-403).
    NotFound,
    /// 500 — a `load_scoped` DB error.
    Internal,
}

impl IntoResponse for GuardRejection {
    fn into_response(self) -> Response {
                                                                              
                                                                      
        let status = match self {
            GuardRejection::NoScope => StatusCode::UNAUTHORIZED,
            GuardRejection::BadId => StatusCode::BAD_REQUEST,
            GuardRejection::NotFound => StatusCode::NOT_FOUND,
            GuardRejection::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        };
        status.into_response()
    }
}

impl<S, T> FromRequestParts<S> for Scoped<T>
where
    T: Resource + 'static,
    T::Scope: Copy + Send + Sync + 'static,
    SqlitePool: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = GuardRejection;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
                                                                               
                                                                                  
        let scope = parts
            .extensions
            .get::<ScopeCache<T::Scope>>()
            .map(|c| c.0)
            .ok_or(GuardRejection::NoScope)?;

                                                                
        let params = RawPathParams::from_request_parts(parts, state)
            .await
            .map_err(|_| GuardRejection::BadId)?;
        let first = params.iter().next().ok_or(GuardRejection::BadId)?;
        let id: i64 = first.1.parse().map_err(|_| GuardRejection::BadId)?;

        let pool = SqlitePool::from_ref(state);
        match T::load_scoped(id, scope, &pool).await {
            Ok(Some(t)) => Ok(Scoped(t)),
            Ok(None) => Err(GuardRejection::NotFound),
            Err(e) => {
                tracing::error!("pomelo: load_scoped failed for id={id}: {e}");
                Err(GuardRejection::Internal)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ScopeCache, Scoped};
    use crate::{PathParamKind, Resource};
    use ::axum::Router;
    use ::axum::body::Body;
    use ::axum::http::{Request, StatusCode};
    use ::axum::routing::get;
    use async_trait::async_trait;
    use sqlx::SqlitePool;
    use tower::ServiceExt;

    /// Synthetic scope newtype (mirrors a real consumer's `HouseholdId`).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::Type)]
    #[sqlx(transparent)]
    struct TestScope(i64);

    #[derive(Debug)]
    struct Widget {
        #[allow(dead_code)]
        id: i64,
        #[allow(dead_code)]
        scope: TestScope,
    }

    #[async_trait]
    impl Resource for Widget {
        type Scope = TestScope;
        const ID_PARAM_KIND: PathParamKind = PathParamKind::Identifier;

        async fn load_scoped(
            id: i64,
            scope: TestScope,
            pool: &SqlitePool,
        ) -> sqlx::Result<Option<Self>> {
            let row: Option<(i64, i64)> = sqlx::query_as(
                "SELECT id, household_id FROM widgets WHERE id = ? AND household_id = ?",
            )
            .bind(id)
            .bind(scope.0)
            .fetch_optional(pool)
            .await?;
            Ok(row.map(|(id, hh)| Widget {
                id,
                scope: TestScope(hh),
            }))
        }
    }

    /// Seeded in-memory pool: widget 1 in scope 100, widget 2 in scope 200.
    async fn seeded_pool() -> SqlitePool {
        let pool = SqlitePool::connect(":memory:").await.unwrap();
        sqlx::query("CREATE TABLE widgets (id INTEGER PRIMARY KEY, household_id INTEGER NOT NULL)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO widgets (id, household_id) VALUES (1, 100), (2, 200)")
            .execute(&pool)
            .await
            .unwrap();
        pool
    }

    /// Handler that only succeeds if `Scoped<Widget>` extracted — returns 200,
    /// otherwise the extractor's `GuardRejection` renders its bare status.
    async fn handler(_scoped: Scoped<Widget>) -> StatusCode {
        StatusCode::OK
    }

    /// Build a request for `uri`, inserting each scope in `scopes` as a
    /// `ScopeCache` extension (in order — the LAST wins, per axum extensions).
    fn request(uri: &str, scopes: &[i64]) -> Request<Body> {
        let mut builder = Request::builder().uri(uri);
        for s in scopes {
            builder = builder.extension(ScopeCache(TestScope(*s)));
        }
        builder.body(Body::empty()).unwrap()
    }

    async fn status_for(router: Router, req: Request<Body>) -> StatusCode {
        router.oneshot(req).await.unwrap().status()
    }

    /// (1) The id is read from the first dynamic param even when the route has
    /// STATIC prefix segments — `/api/v1/things/{id}` reads `{id}`, not `api`.
    #[tokio::test]
    async fn reads_first_dynamic_param_past_static_prefix() {
        let router = Router::new()
            .route("/api/v1/things/{id}", get(handler))
            .with_state(seeded_pool().await);
        let status = status_for(router, request("/api/v1/things/1", &[100])).await;
        assert_eq!(status, StatusCode::OK);
    }

    /// (2) A multi-dynamic-param route reads POSITION 0 only — `/{id}/steps/{step_id}`
    /// scopes on `{id}`, ignoring `{step_id}`.
    #[tokio::test]
    async fn multi_param_route_reads_position_zero_only() {
        let router = Router::new()
            .route("/{id}/steps/{step_id}", get(handler))
            .with_state(seeded_pool().await);
                                                                          
        let status = status_for(router, request("/1/steps/9", &[100])).await;
        assert_eq!(status, StatusCode::OK);
    }

    /// (3) No `ScopeCache` in extensions → 401 (NoScope). Mirrors the Rocket
    /// guard's missing-scope-cache case.
    #[tokio::test]
    async fn missing_scope_cache_returns_401() {
        let router = Router::new()
            .route("/{id}", get(handler))
            .with_state(seeded_pool().await);
        let status = status_for(router, request("/1", &[])).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    /// (4) A non-numeric id → 400 (BadId).
    #[tokio::test]
    async fn non_numeric_id_returns_400() {
        let router = Router::new()
            .route("/{id}", get(handler))
            .with_state(seeded_pool().await);
        let status = status_for(router, request("/abc", &[100])).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    /// (5) An id that exists in ANOTHER scope → 404 (NotFound), never 403 —
    /// the 404-not-403 discipline. Widget 2 is in scope 200; we ask as 100.
    #[tokio::test]
    async fn other_scope_id_returns_404() {
        let router = Router::new()
            .route("/{id}", get(handler))
            .with_state(seeded_pool().await);
        let status = status_for(router, request("/2", &[100])).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    /// (6) Two `ScopeCache` inserts → the LAST wins. This documents the axum
    /// extensions last-write-wins semantics, the INVERSE of Rocket's
    /// `local_cache` first-write-wins (the G10 invariant the consumer's
    /// convention enforcer guards). Insert scope 100 then 200; widget 2 lives
    /// in scope 200, so a 200 status proves the last insert won.
    #[tokio::test]
    async fn last_scope_cache_insert_wins() {
        let router = Router::new()
            .route("/{id}", get(handler))
            .with_state(seeded_pool().await);
        let status = status_for(router, request("/2", &[100, 200])).await;
        assert_eq!(
            status,
            StatusCode::OK,
            "the LAST ScopeCache insert (200) must win; widget 2 is in scope 200"
        );
    }

    /// (bonus, migrated from the Rocket guard's 500 coverage) A `load_scoped`
    /// DB error → 500 (Internal). Here the pool has no `widgets` table, so the
    /// query errors — exercising the `tracing::error!` + `Internal` arm.
    #[tokio::test]
    async fn load_scoped_db_error_returns_500() {
        let pool = SqlitePool::connect(":memory:").await.unwrap();
        let router = Router::new().route("/{id}", get(handler)).with_state(pool);
        let status = status_for(router, request("/1", &[100])).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    }
}
