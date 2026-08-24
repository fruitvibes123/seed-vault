# fig

JSON error contract for Rocket JSON APIs. Provides an `ApiError` responder + catchers for the standard `{"error":"...","status":N}` body shape.

## Usage as a route error type

```rust
use fig::{ApiError, catchers};
use rocket::{get, routes, serde::json::Json, Build, Rocket};

#[get("/recipes/<id>")]
async fn get_recipe(id: i64) -> Result<Json<Recipe>, ApiError> {
    // Convenience constructor uses the crate's default message.
    let recipe = load_recipe(id).await.ok_or_else(ApiError::not_found)?;
    Ok(Json(recipe))
}

#[get("/download/<name>")]
async fn download(name: &str) -> Result<Vec<u8>, ApiError> {
    // Struct-variant form passes a route-specific message override.
    let bytes = tokio::fs::read(name).await.map_err(|_| ApiError::NotFound {
        message: Some("file not found".into()),
    })?;
    Ok(bytes)
}

fn rocket() -> Rocket<Build> {
    rocket::build()
        .mount("/api/v1", routes![get_recipe])
        .register("/api/v1", catchers())  // 400, 404, 422, 500 with JSON contract
}
```

### Convenience constructors vs. struct-variant form

`ApiError::not_found() / ::forbidden() / ::unauthorized() / ::internal()` are zero-argument constructors that use the crate-default message (`"not found"`, etc.). When a route needs to pin a different message (e.g., the download spec's `"file not found"` to distinguish "no APK staged" from "unknown route"), use the struct-variant form directly: `ApiError::NotFound { message: Some("…".into()) }`. The message override flows into the JSON body's `error` field; the HTTP status code is fixed per variant.

## Usage as a body-builder for an existing app error type

For projects that already have an `AppError` enum and want the same JSON
contract on API-shaped requests, implement `From<AppError> for ApiError` and
delegate from `respond_to`:

```rust
use fig::ApiError;
use rocket::request::Request;
use rocket::response::{self, Responder};

impl From<MyAppError> for ApiError { /* ... */ }

impl<'r> Responder<'r, 'static> for MyAppError {
    fn respond_to(self, req: &'r Request<'_>) -> response::Result<'static> {
        if req.uri().path().starts_with("/api/") {
            return ApiError::from(self).respond_to(req);
        }
        // ... HTML render path ...
    }
}
```

## ApiError variants

Variants with overridable messages carry `message: Option<String>` — `None` uses the default; `Some("…")` pins a custom wording. Variants with embedded `String` always require a message.

| Variant | Status | Default message | Override? | Special headers |
|---|---|---|---|---|
| `NotFound { message }` | 404 | `"not found"` | yes | |
| `BadRequest(String)` | 400 | — | (always required) | |
| `Forbidden { message }` | 403 | `"forbidden"` | yes | |
| `Unauthorized { message }` | 401 | `"unauthorized"` | yes | |
| `Conflict(String)` | 409 | — | (always required) | |
| `Gone(String)` | 410 | — | (always required) | for revoked / removed resources |
| `PayloadTooLarge(String)` | 413 | — | (always required) | for body-size limits |
| `Unprocessable(String)` | 422 | — | (always required) | for semantically invalid bodies (e.g., missing required JSON field) |
| `TooManyRequests { message, retry_after }` | 429 | — | (always required) | emits `Retry-After` when `retry_after: Some(_)` |
| `RangeNotSatisfiable { message, content_range }` | 416 | `"range not satisfiable"` | yes | emits `Content-Range` when `content_range: Some(_)` (RFC 9110 §14.4 — typically `"bytes */<total>"`) |
| `Internal { message }` | 500 | `"internal error"` | yes | |

All variants serialize to a `{"error":"<message>","status":<code>}` JSON body with `Content-Type: application/json; charset=utf-8`. The wire HTTP status line AND the body's `status` field always agree on the variant's status code.

### Catcher set

`catchers()` returns Rocket catchers for 400 / 404 / 422 / 500 that emit the same `ApiError` shape — register at the JSON-API prefix(es):

```rust
rocket::build()
    .register("/api/v1", fig::catchers())
    .register("/download", fig::catchers()) // for non-/api/ JSON surfaces
```

401 / 403 are intentionally not in the catcher set — guard-failure dispatch typically goes through the host project's catcher (which then constructs `ApiError::unauthorized()` / `ApiError::forbidden()` itself) so that page-shaped requests can branch to a login redirect.

## License

MIT OR Apache-2.0.
