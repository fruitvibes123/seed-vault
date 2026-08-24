# pomelo

Compile-time IDOR-safe resource loading for Rocket routes.

## What it does

Provides a generic `Scoped<T>` request guard that loads a resource by id within the current request's scope (household / team / org / tenant — anything that owns resources). Cross-scope OR nonexistent ids both return 404, never 403 — the 404-not-403 discipline is encoded at the trait level, not at the route.

## Quick example

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

The guard reads `T::Scope` from Rocket's request-local cache. Your auth guard (whatever resolves the current household/team/org for the request) is responsible for populating it:

```rust
// In your MemberOf guard's FromRequest impl:
let _ = req.local_cache(|| Some(self.0));  // self.0: HouseholdId
```

## Path-param convention

`Scoped<T>` reads the resource id from the **first dynamic path segment** of the matched route. The position is computed structurally from the route's URI template at request time (via `route.uri.unmounted_origin.path().raw_segments()`), then resolved against `request.routed_segment(N)` — so static segments (numeric or not) cannot shadow the resource id. Your route MUST have the resource id as the first dynamic param.

**Convention discipline (semantic).** The build-time check (see the recipes project's `build.rs` + `tests/path_param_convention.rs` for a reference implementation) enforces a NAME shape (`id`, `_id`, or `<x>_id`). It cannot verify that the chosen name actually corresponds to the resource type `T`. A route like `/orgs/<org_id>/recipes/<recipe_id>` with `Scoped<Recipe>` would pass the convention check but bind `<org_id>` as the recipe id at runtime. The SQL 404-not-403 contract (`WHERE id = ? AND household_id = ?`) still neutralizes the security outcome — the wrong id resolves to zero rows → 404, not the wrong resource — but the route's CORRECTNESS would be broken. Reviewers must verify at PR time that a multi-dynamic-i64-param route's first dynamic param matches `Scoped<T>`'s resource type.

## Path-param threat-model intent — `PathParamKind`

Every `Resource` impl declares an `ID_PARAM_KIND: PathParamKind` const that names the threat-model intent of `Scoped<Self>`'s path parameter:

```rust
#[async_trait]
impl Resource for Recipe {
    type Scope = HouseholdId;
    const ID_PARAM_KIND: PathParamKind = PathParamKind::Identifier;
    // ...
}
```

The two variants carry very different threat models:

- **`Identifier`** — the path param is a non-secret database id. Resource secrecy comes from the SQL `WHERE id=? AND scope=?` predicate (which `load_scoped` is required to apply). An attacker guessing the id gets a 404 unless they're already in the right scope. Logging the id, putting it in HTTP `Referer`, surfacing it in HAProxy access logs, etc. is acceptable. **This is the default for every scope-owned database row.**

- **`Secret`** — the path param's BYTES are the access credential (e.g., an 8-char Crockford pairing code, a share-link token). Anyone who learns the bytes can act. Adopting `Secret` for a new Resource implies the consumer is also taking on:
  - **Log redaction** at the route prefix (mask the secret in HAProxy access logs + the project's `RequestLogger` fairing).
  - **Per-attempt throttle** on the route (per-IP rate limit; budget-bound brute-force).
  - **Entropy requirements** in the URL-shape design (don't move from a 40-bit secret to a 0-bit sequential id "for REST cleanliness").

Why on the trait (compile-time): a future maintainer adding a Resource impl must pick a kind — leaving the const off yields `error[E0046]: not all trait items implemented, missing: ID_PARAM_KIND`. The choice is also audit-grep-discoverable: `grep -rn 'ID_PARAM_KIND.*Secret'` finds every secret-keyed Resource in the consumer's tree. A discipline-only convention (the prior shape) gets eroded silently; a compile-time const can't.

### Consider this also — `Resource::ID_PARAM_NAME` const for structural enforcement

The "human reviewer at PR time" rule above could become a build-time check the same way the framework's path-param-name check did. Sketch:

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

The recipes project's `build.rs` would then parse each `Scoped<T>` handler's generic argument (the `T`), look up `T::ID_PARAM_NAME` via the type system… which is the catch: `build.rs` runs against source text, not against the compiled type-system. The const isn't resolvable at the syn-walk layer.

Two paths around it:

1. **A proc-macro `#[scoped_route]` attribute** on each handler that captures both the `Scoped<T>` choice AND the route attribute, and emits a `#[doc = "scope_id_param: id"]` (or similar `cfg`-keyed marker) the build.rs scan can read syntactically.
2. **Conventionally pair each `Resource` impl with a doc-test or compile-time assertion** that the impl's `ID_PARAM_NAME` matches the URI templates of every route using `Scoped<Self>`. Requires the consuming project to list its `(T, &[uri])` pairs somewhere structural.

Both add machinery that's heavier than the current consumers justify (every retro-ported route uses `<_id>` as the first dynamic; the foot-gun route shape isn't on the path). Worth revisiting when:

- A consumer ships a multi-resource hierarchical route where `<a_id>` shadows `<b_id>` at runtime.
- OR a different scope dimension (team / org / project) gets added and the convention starts compiling against multiple `T::Scope` types in the same handler.

Until then, the doc-comment + PR review is the documented contract.

## License

MIT OR Apache-2.0 (your choice). Workspace-shared license files at the repo root.

## Status

Workspace sibling crate of the recipes project. Designed for extraction to its own repo when needed; the move is mechanical.
