//! Scoped<T> request guard implementation.

use crate::Resource;
use rocket::http::Status;
use rocket::request::{FromRequest, Outcome, Request};
use sqlx::SqlitePool;

/// Request guard: loads `T` by id within the current scope.
///
/// Destructure `let Scoped(resource) = ...;` in route handlers.
///
/// # Convention
///
/// Reads the resource id from the **first dynamic path segment** of the
/// matched route (e.g., the `<id>` in `/api/v1/recipes/<id>`). The
/// dynamic-segment position is computed from
/// `route.uri.unmounted_origin.path()` at request time, then resolved
/// against `request.routed_segment(N)` — so a numeric static segment
/// elsewhere in the path (`/api/v2/...`, `/2/recipes/<id>`) cannot shadow
/// the resource id.
///
/// Routes adopting this guard MUST have the resource id as the first
/// dynamic param; the build-time check at
/// `tests/path_param_convention.rs` inspects every `#[get/post/...]`
/// handler whose signature mentions `Scoped<…>` or `AttemptBudget<…>`.
///
/// **Convention discipline (semantic).** The build-time check enforces
/// a NAME shape (`id`, `_id`, or `<x>_id`); it cannot verify that the
/// chosen name actually corresponds to the resource type `T`. A route
/// like `/orgs/<org_id>/recipes/<recipe_id>` with `Scoped<Recipe>`
/// passes the convention (both names end in `_id`) but the guard reads
/// `<org_id>` as the recipe id. The 404-not-403 SQL contract
/// (`WHERE id = ? AND household_id = ?`) still neutralizes the security
/// outcome — the wrong id resolves to zero rows → 404, not the wrong
/// resource — but the route's CORRECTNESS is broken. Reviewers must
/// verify at PR time that a multi-i64-dynamic-param route's first
/// dynamic param matches `Scoped<T>`'s resource type. The recipes
/// codebase has no such shape today (every retro-ported route's first
/// dynamic is `<_id>`, the household-scoped resource).
///
/// Note: an earlier draft of this guard used `request.param::<i64>(0)`,
/// which in Rocket 0.5 returns the n-th URI segment after the mount
/// (static OR dynamic) — so a static-prefix route like
/// `/api/v1/recipes/<id>` would pick up `"api"` at index 0, fail to
/// parse, and return `BadId`. A later draft walked routed segments
/// looking for the first `i64`-parseable one; that worked for every
/// currently retro-ported route but was order-of-appearance fragile (a
/// static numeric segment, or a hypothetical sibling-resource route with
/// two `i64` dynamic params, could silently scope-confuse). The current
/// shape is structurally tied to the route's dynamic-param slot.
pub struct Scoped<T: Resource>(pub T);

#[derive(Debug)]
pub enum GuardError {
    NoScope,
    BadId,
    NoRoute,
    PoolMissing,
    NotFound,
    Db(sqlx::Error),
}

#[rocket::async_trait]
impl<'r, T: Resource + Send + 'static> FromRequest<'r> for Scoped<T> {
    type Error = GuardError;

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
                                                                            
                                                            
        let scope_opt: &Option<T::Scope> = req.local_cache(|| None::<T::Scope>);
        let scope: T::Scope = match scope_opt {
            Some(s) => *s,
            None => return Outcome::Error((Status::Unauthorized, GuardError::NoScope)),
        };

                                                                       
                                                                       
                                                      
        let route = match req.route() {
            Some(r) => r,
            None => return Outcome::Error((Status::InternalServerError, GuardError::NoRoute)),
        };
        let dynamic_idx = route
            .uri
            .unmounted_origin
            .path()
            .raw_segments()
            .position(|seg| seg.as_str().starts_with('<'));
        let id_idx = match dynamic_idx {
            Some(i) => i,
            None => return Outcome::Error((Status::BadRequest, GuardError::BadId)),
        };

        let id: i64 = match req
            .routed_segment(id_idx)
            .and_then(|s| s.parse::<i64>().ok())
        {
            Some(id) => id,
            None => return Outcome::Error((Status::BadRequest, GuardError::BadId)),
        };

        let pool = match req.rocket().state::<SqlitePool>() {
            Some(p) => p,
            None => {
                rocket::error!("pomelo: SqlitePool not in Rocket state; Scoped<T> cannot load");
                return Outcome::Error((Status::InternalServerError, GuardError::PoolMissing));
            }
        };

        match T::load_scoped(id, scope, pool).await {
            Ok(Some(t)) => Outcome::Success(Scoped(t)),
            Ok(None) => Outcome::Error((Status::NotFound, GuardError::NotFound)),
            Err(e) => {
                rocket::error!("pomelo: load_scoped failed for id={id}, scope={scope:?}: {e}");
                Outcome::Error((Status::InternalServerError, GuardError::Db(e)))
            }
        }
    }
}
