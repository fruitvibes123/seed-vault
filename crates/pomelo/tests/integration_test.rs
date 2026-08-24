//! Integration tests for `pomelo`'s public surface.
//!
//! The battery pins:
//! - end-to-end Rocket-route tests exercise the `Scoped<T>`
//!   `FromRequest` guard, not just `Resource::load_scoped`. Cross-scope
//!   404, missing-scope 401, non-numeric path-param 400, and missing
//!   `SqlitePool` state 500 cases are all pinned.
//! - the tests depend on no consumer type crate; the synthetic
//!   `TestScope` newtype below stands in for any consumer's scope type,
//!   keeping the crate's tests portable for a future Level-2 extraction.
//!
//! sqlx is exercised via the non-macro `query_as` form so the crate has
//! no compile-time offline cache requirement and no live `DATABASE_URL`
//! dependency. The contract under test (the `Option<Self>` return shape
//! plus the guard's outcome mapping) is identical either way.

                                                                        
                                                                        
                                                                         
#![cfg(feature = "rocket")]

use async_trait::async_trait;
use pomelo::{PathParamKind, Resource, Scoped};
use rocket::http::Status;
use rocket::local::asynchronous::Client;
use rocket::request::{FromRequest, Outcome, Request};
use rocket::{get, routes};
use sqlx::SqlitePool;

/// Synthetic scope newtype for the test fixtures. Mirrors the shape a
/// real consumer would adopt (e.g., `HouseholdId` in the recipes app)
/// without pulling that consumer's crate in. Bound shape matches the
/// `Resource::Scope` trait bounds exactly: `Debug + Copy + Send + Sync
/// + sqlx::Type<Sqlite> + 'static`.
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

/// Regression: `PathParamKind` const declaration is part of
/// the Resource trait. Widget declares Identifier; a synthetic
/// SecretWidget declares Secret. Both compile + the const reads back
/// correctly. The compile-time enforcement (every Resource impl must
/// pick a kind) is exercised structurally by the fact that this file
/// compiles at all — leaving the const out yields E0046.
///
/// Per [[secret-location-entropy-check]] the threat-model distinction
/// matters: future consumers picking Secret are implicitly opting into
/// log redaction + per-attempt throttle + entropy requirements. This
/// test pins the API surface so a future maintainer can't silently
/// remove the const without breaking the compile.
#[derive(Debug)]
struct SecretWidget {
    #[allow(dead_code)]
    id: i64,
    #[allow(dead_code)]
    scope: TestScope,
}

#[async_trait]
impl Resource for SecretWidget {
    type Scope = TestScope;
    const ID_PARAM_KIND: PathParamKind = PathParamKind::Secret;

    async fn load_scoped(
        _id: i64,
        _scope: TestScope,
        _pool: &SqlitePool,
    ) -> sqlx::Result<Option<Self>> {
                                                            
        Ok(None)
    }
}

#[test]
fn path_param_kind_const_is_declared_per_resource() {
                                                                       
                                                                     
                                                                     
    assert_eq!(
        <Widget as Resource>::ID_PARAM_KIND,
        PathParamKind::Identifier
    );
    assert_eq!(
        <SecretWidget as Resource>::ID_PARAM_KIND,
        PathParamKind::Secret
    );
}

/// `Resource::load_scoped` returns `Ok(None)` for cross-scope AND for
/// nonexistent. This is the 404-not-403 discipline the guard layers on
/// top of — the trait's own contract is exercised here at the helper
/// level. The Rocket-route tests below pin the guard's mapping of those
/// `None` returns to `Status::NotFound`.
#[tokio::test]
async fn load_scoped_cross_scope_returns_none() {
    let pool = bootstrap_pool().await;
    let s100 = TestScope(100);

    let r = Widget::load_scoped(1, s100, &pool).await.unwrap();
    assert!(r.is_some(), "own-scope resource should load");

    let r = Widget::load_scoped(2, s100, &pool).await.unwrap();
    assert!(
        r.is_none(),
        "cross-scope resource must return None, not an error variant"
    );

    let r = Widget::load_scoped(999, s100, &pool).await.unwrap();
    assert!(r.is_none(), "nonexistent id must return None");
}

                                                                                                 

/// Test-only auth guard that mirrors a real consumer's MemberOf /
/// DeviceCertAuth shape: parses a `X-Scope` header into TestScope and
/// populates the request-local scope cache. Routes that use
/// `Scoped<Widget>` must list `TestAuth` first so the cache is populated
/// before `Scoped::from_request` reads it.
struct TestAuth(#[allow(dead_code)] TestScope);

#[rocket::async_trait]
impl<'r> FromRequest<'r> for TestAuth {
    type Error = ();
    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        let Some(raw) = req.headers().get_one("X-Scope") else {
            return Outcome::Error((Status::Unauthorized, ()));
        };
        let Ok(value) = raw.parse::<i64>() else {
            return Outcome::Error((Status::Unauthorized, ()));
        };
        let scope = TestScope(value);
        let _ = req.local_cache(|| Some(scope));
        Outcome::Success(TestAuth(scope))
    }
}

#[get("/widgets/<_id>")]
fn get_widget(_id: i64, _auth: TestAuth, widget: Scoped<Widget>) -> String {
    let Scoped(w) = widget;
    format!("widget id={}", w.id)
}

/// Route that uses `Scoped<Widget>` but takes the auth guard AFTER it,
/// reproducing the wrong-order-by-construction case. Used to assert
/// `Scoped<T>` 401's when the scope cache hasn't been populated yet.
#[get("/wrong-order/<_id>")]
fn wrong_order(_id: i64, widget: Scoped<Widget>, _auth: TestAuth) -> String {
    let Scoped(w) = widget;
    format!("widget id={}", w.id)
}

/// Route registered without a pool in Rocket state — exercises the
/// structured `PoolMissing` error path.
#[get("/pool-missing/<_id>")]
fn pool_missing(_id: i64, _auth: TestAuth, widget: Scoped<Widget>) -> String {
    let Scoped(w) = widget;
    format!("widget id={}", w.id)
}

async fn bootstrap_pool() -> SqlitePool {
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

async fn bootstrap_client() -> Client {
    let pool = bootstrap_pool().await;
    let rocket = rocket::build()
        .manage(pool)
        .mount("/", routes![get_widget, wrong_order]);
    Client::tracked(rocket).await.expect("rocket client")
}

#[tokio::test]
async fn guard_own_scope_returns_200() {
    let c = bootstrap_client().await;
    let resp = c
        .get("/widgets/1")
        .header(rocket::http::Header::new("X-Scope", "100"))
        .dispatch()
        .await;
    assert_eq!(resp.status(), Status::Ok);
}

#[tokio::test]
async fn guard_cross_scope_returns_404() {
    let c = bootstrap_client().await;
                                                                        
                                                                     
                                                                   
                        
    let resp = c
        .get("/widgets/2")
        .header(rocket::http::Header::new("X-Scope", "100"))
        .dispatch()
        .await;
    assert_eq!(resp.status(), Status::NotFound);
}

#[tokio::test]
async fn guard_nonexistent_id_returns_404() {
    let c = bootstrap_client().await;
    let resp = c
        .get("/widgets/999")
        .header(rocket::http::Header::new("X-Scope", "100"))
        .dispatch()
        .await;
    assert_eq!(resp.status(), Status::NotFound);
}

#[tokio::test]
async fn guard_missing_scope_cache_returns_401() {
    let c = bootstrap_client().await;
                                                                         
                                                                     
    let resp = c
        .get("/wrong-order/1")
        .header(rocket::http::Header::new("X-Scope", "100"))
        .dispatch()
        .await;
    assert_eq!(resp.status(), Status::Unauthorized);
}

#[tokio::test]
async fn guard_non_numeric_path_param_returns_400() {
    let c = bootstrap_client().await;
                                                                      
                                                                       
                                                                    
                                                                      
                                                                 
                                                                       
    let resp = c
        .get("/widgets/abc")
        .header(rocket::http::Header::new("X-Scope", "100"))
        .dispatch()
        .await;
    assert_eq!(resp.status(), Status::BadRequest);
}

#[tokio::test]
async fn guard_missing_pool_state_returns_500() {
                                                               
                                                                   
                                                                 
                                               
    let rocket = rocket::build().mount("/", routes![pool_missing]);
    let c = Client::tracked(rocket).await.expect("rocket client");
    let resp = c
        .get("/pool-missing/1")
        .header(rocket::http::Header::new("X-Scope", "100"))
        .dispatch()
        .await;
    assert_eq!(resp.status(), Status::InternalServerError);
}
