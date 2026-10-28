# fig

JSON error contract for Rust JSON APIs: an `ApiError` type with the standard `{"error":"...","status":N}` body shape, plus per-framework adapters behind feature flags. The `rocket` feature (default) provides the `Responder` impl and catchers; the `axum` feature provides an `IntoResponse` impl. Both adapters emit the same body and headers.

## Route error type

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

`ApiError::not_found() / ::forbidden() / ::unauthorized() / ::internal()` are zero-argument constructors that use the crate-default message (`"not found"`, etc.). A route that needs to pin a different message uses the struct-variant form, `ApiError::NotFound { message: Some("…".into()) }`. An example is the download spec's `"file not found"`, which distinguishes "no APK staged" from "unknown route".

## Existing app error type

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

## Variants

Variants with a `message: Option<String>` field use the default message on `None` and the given wording on `Some("…")`. Variants that hold a `String` message require one. The message flows into the JSON body's `error` field; the HTTP status code is fixed per variant.

| Variant | Status | Message | Notes |
|---|---|---|---|
| `NotFound { message }` | 404 | default `"not found"` | |
| `BadRequest(String)` | 400 | required | |
| `Forbidden { message }` | 403 | default `"forbidden"` | |
| `Unauthorized { message }` | 401 | default `"unauthorized"` | |
| `Conflict(String)` | 409 | required | |
| `Gone(String)` | 410 | required | for revoked / removed resources |
| `PayloadTooLarge(String)` | 413 | required | for body-size limits |
| `Unprocessable(String)` | 422 | required | for semantically invalid bodies (e.g., missing required JSON field) |
| `TooManyRequests { message, retry_after }` | 429 | required | emits `Retry-After` when `retry_after: Some(_)` |
| `RangeNotSatisfiable { message, content_range }` | 416 | default `"range not satisfiable"` | emits `Content-Range` when `content_range: Some(_)` (RFC 9110 §14.4; typically `"bytes */<total>"`) |
| `Internal { message }` | 500 | default `"internal error"` | |

All variants serialize to a `{"error":"<message>","status":<code>}` JSON body with `Content-Type: application/json; charset=utf-8`. The wire HTTP status line and the body's `status` field always carry the variant's status code.

## Catchers

`catchers()` returns Rocket catchers for 400 / 404 / 422 / 500 that emit the same `ApiError` shape. Register them at each JSON-API prefix:

```rust
rocket::build()
    .register("/api/v1", fig::catchers())
    .register("/download", fig::catchers()) // for non-/api/ JSON surfaces
```

Guard-failure dispatch for 401 / 403 typically goes through the host project's own catcher, which constructs `ApiError::unauthorized()` / `ApiError::forbidden()` itself so that page-shaped requests can branch to a login redirect. The catcher set leaves out 401 and 403 by design.

## License

MIT OR Apache-2.0.
