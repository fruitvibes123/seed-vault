# pomelo

Compile-time IDOR-safe resource loading, with Rocket and Axum adapters behind
feature flags. Published on crates.io as `pomelo-guard` (the bare name was
taken); the library target stays `pomelo`, so code writes `use pomelo::…`
regardless.

`Scoped<T>` loads a resource by id within the current request's scope (household / team / org / tenant: anything that owns resources). A cross-scope id and a nonexistent id both return 404, never 403. The 404-not-403 discipline is encoded at the trait level instead of at the route.

Feature flags select the adapter:

- `rocket` feature (default): `pomelo::Scoped`, a Rocket `FromRequest` request guard.
- `axum` feature: `pomelo::axum::Scoped`, an Axum `FromRequestParts` extractor, with `ScopeCache` and `GuardRejection`.

`load_scoped` takes a `SqlitePool`, so the storage layer is currently tied to sqlx/SQLite; decoupling the pool type is planned.

## Example

```rust
use pomelo::{PathParamKind, Resource, Scoped};

#[async_trait::async_trait]
impl Resource for Recipe {
    type Scope = HouseholdId;  // your scope type
    // Threat-model intent of the path param. Almost every scope-owned
    // database row picks `Identifier`. See "Path-param threat-model
    // intent" below for when `Secret` is the right call.
    const ID_PARAM_KIND: PathParamKind = PathParamKind::Identifier;

    async fn load_scoped(id: i64, scope: HouseholdId, pool: &SqlitePool) -> sqlx::Result<Option<Self>> {
        sqlx::query_as!(Recipe, "SELECT * FROM recipes WHERE id = ? AND household_id = ?", id, scope)
            .fetch_optional(pool).await
    }
}

#[get("/recipes/<id>")]
async fn get(_id: i64, recipe: Scoped<Recipe>) -> Json<Recipe> {
    let Scoped(recipe) = recipe;
    Json(recipe)
}
```

## Scope population

The Rocket guard reads `T::Scope` from Rocket's request-local cache. Your auth guard (whatever resolves the current household/team/org for the request) populates it:

```rust
// In your MemberOf guard's FromRequest impl:
let _ = req.local_cache(|| Some(self.0));  // self.0: HouseholdId
```

The Axum extractor reads a `ScopeCache<T::Scope>` request extension, which the auth extractor inserts before `Scoped<T>` runs.

## Path-param convention

`Scoped<T>` reads the resource id from the first dynamic path segment of the matched route, so your route must have the resource id as its first dynamic param. The Rocket guard computes the position structurally from the route's URI template at request time (via `route.uri.unmounted_origin.path().raw_segments()`) and resolves it against `request.routed_segment(N)`, so static segments (numeric or not) cannot shadow the resource id.

The build-time check (reference implementation: the recipes project's `build.rs` + `tests/path_param_convention.rs`) enforces a name shape (`id`, `_id`, or `<x>_id`). It cannot verify that the chosen name corresponds to the resource type `T`. A route like `/orgs/<org_id>/recipes/<recipe_id>` with `Scoped<Recipe>` passes the convention check and binds `<org_id>` as the recipe id at runtime. The SQL 404-not-403 contract (`WHERE id = ? AND household_id = ?`) still neutralizes the security outcome: the wrong id resolves to zero rows and a 404, never to the wrong resource. The route's correctness is still broken, so reviewers must verify at PR time that a multi-dynamic-i64-param route's first dynamic param matches `Scoped<T>`'s resource type.

## Path-param threat-model intent

Every `Resource` impl declares an `ID_PARAM_KIND: PathParamKind` const that names the threat-model intent of `Scoped<Self>`'s path parameter:

```rust
#[async_trait]
impl Resource for Recipe {
    type Scope = HouseholdId;
    const ID_PARAM_KIND: PathParamKind = PathParamKind::Identifier;
    // ...
}
```

- `Identifier`: the path param is a non-secret database id. Resource secrecy comes from the SQL `WHERE id=? AND scope=?` predicate, which `load_scoped` is required to apply. An attacker guessing the id gets a 404 unless they're already in the right scope. Logging the id, putting it in HTTP `Referer`, surfacing it in HAProxy access logs, etc. is acceptable. `Identifier` is the default for every scope-owned database row.
- `Secret`: the path param's bytes are the access credential (e.g., an 8-char Crockford pairing code, a share-link token). Anyone who learns the bytes can act. A consumer that adopts `Secret` for a new Resource also takes on:
  - Log redaction at the route prefix (mask the secret in HAProxy access logs + the project's `RequestLogger` fairing).
  - A per-attempt throttle on the route (per-IP rate limit; budget-bound brute-force).
  - Entropy requirements in the URL-shape design (don't move from a 40-bit secret to a 0-bit sequential id "for REST cleanliness").

The const sits on the trait, so a future maintainer adding a Resource impl must pick a kind: leaving the const off yields `error[E0046]: not all trait items implemented, missing: ID_PARAM_KIND`. The choice is also audit-grep-discoverable: `grep -rn 'ID_PARAM_KIND.*Secret'` finds every secret-keyed Resource in the consumer's tree. A compile-time const cannot erode unnoticed, as a discipline-only convention (the prior shape) does.

## Proposed `Resource::ID_PARAM_NAME` const

An `ID_PARAM_NAME` const could turn the PR-time review rule of the path-param convention into a build-time check, the way the framework's path-param-name check did for the name shape. Sketch:

```rust
#[async_trait]
pub trait Resource: Sized + Send {
    type Scope: …;

    /// The expected name of the route path-param that carries this
    /// resource's id (e.g., `"id"` for the recipe route
    /// `/api/v1/recipes/<id>`, `"recipe_id"` for the nested route
    /// `/recipes/<recipe_id>/steps/<step_id>`). The build.rs scan
    /// asserts that every `Scoped<Self>` route's first dynamic param's
    /// name equals this value, closing the semantic gap above
    /// structurally.
    const ID_PARAM_NAME: &'static str = "id";

    async fn load_scoped(id: i64, scope: Self::Scope, pool: &SqlitePool)
        -> sqlx::Result<Option<Self>>;
}
```

The recipes project's `build.rs` would then parse each `Scoped<T>` handler's generic argument (the `T`) and look up `T::ID_PARAM_NAME` through the type system. That lookup is the obstacle: `build.rs` runs against source text, so the const isn't resolvable at the syn-walk layer.

Two paths around it:

1. A proc-macro `#[scoped_route]` attribute on each handler that captures both the `Scoped<T>` choice and the route attribute, and emits a `#[doc = "scope_id_param: id"]` (or similar `cfg`-keyed marker) the build.rs scan can read syntactically.
2. A doc-test or compile-time assertion paired by convention with each `Resource` impl, checking that the impl's `ID_PARAM_NAME` matches the URI templates of every route using `Scoped<Self>`. This requires the consuming project to list its `(T, &[uri])` pairs somewhere structural.

Both add machinery heavier than the current consumers justify: every retro-ported route uses `<_id>` as the first dynamic, and the foot-gun route shape isn't on the path. The question reopens when a consumer ships a multi-resource hierarchical route where `<a_id>` shadows `<b_id>` at runtime, or when a different scope dimension (team / org / project) gets added and the convention starts compiling against multiple `T::Scope` types in the same handler. Until then, the doc comment and PR review are the documented contract.

Extracted from a production web application (the reference tenant of the
fruit-basket appliance OS), where every scope-owned route loads through
`Scoped<T>`.

## License

MIT OR Apache-2.0 (your choice). Workspace-shared license files at the repo root.
