# banana

Framework-agnostic HTTP byte-range parser + mtime-keyed BLAKE3 ETag cache, extracted from a private download-service implementation as its two highest-leverage reusable primitives.

## What's here

- **`parse_range_str`** — RFC 9110 §14.2 single-range parser. Returns `RangeSpec::Explicit { start, end }` or `RangeSpec::Suffix(n)`; multi-range is explicitly rejected per the same `single-range-only → 400` policy that recipes' download spec pins.
- **`RangeError`** — single-variant error type returned by `parse_range_str` on any malformed input (missing `bytes=` prefix, multi-range, empty, non-numeric, inverted). Callers map directly to HTTP 400.
- **`RangeSpec::resolve(file_size)`** — resolves a parsed range against the file's known size. Returns inclusive `(start, end)` byte indices, or `None` for an out-of-bounds range (caller maps to 416).
- **`EtagCache`** — mtime+size-keyed BLAKE3 hash cache. Hashes through an open `tokio::fs::File` handle (TOCTOU-safe for the hash-vs-serve axis: the bytes whose hash is the ETag are the same bytes the streaming path serves). Mutex-poisoning is recovered via `into_inner()` so a poison can't leak into the response body.
- **`quote_etag`** — render a hex digest as the quoted strong-ETag header value per RFC 9110 §8.8.3.
- **`matches_if_none_match`** — RFC 9110 §13.1.2 conditional-GET matcher. Handles single-value, comma-separated list, `W/`-weak prefix, and `*` wildcard. Returns `true` when the consumer should short-circuit to 304.

No framework dependencies. Works with Rocket, Axum, any HTTP server, or non-HTTP code that wants the underlying logic.

## Usage sketch (Rocket; Axum is symmetric)

```rust
use banana::{parse_range_str, RangeSpec, EtagCache, quote_etag};

async fn download_handler(/* framework extractors */) -> /* framework response */ {
    let cache: &EtagCache = state.cache();

    // 1. Parse Range header (framework-specific extraction).
    let range = match request.headers().get("Range") {
        None => None,
        Some(raw) => Some(parse_range_str(raw).map_err(|_| /* 400 */)?),
    };

    // 2. Open the file ONCE, take metadata from the open handle (TOCTOU-safe).
    let mut file = tokio::fs::File::open(&path).await.map_err(/* 404 / 500 */)?;
    let meta = file.metadata().await.map_err(/* 500 */)?;
    let size = meta.len();
    let mtime = meta.modified().map_err(/* 500 */)?;

    // 3. Resolve range against file size (skip if no Range header).
    let slice = match range {
        Some(spec) => spec.resolve(size),
        None => Some((0, size.saturating_sub(1))),
    };
    let Some((start, end)) = slice else {
        return /* 416 with Content-Range: bytes */{size} */;
    };

    // 4. ETag — hash through the open handle on cache miss.
    let etag_hex = cache.get_or_compute(&mut file, mtime, size).await.map_err(/* 500 */)?;
    let etag_header = quote_etag(&etag_hex);

    // 5. If-None-Match check — RFC 9110 §13.1.2 weak comparison.
    if let Some(inm_raw) = request.headers().get("If-None-Match")
        && matches_if_none_match(inm_raw, &etag_header)
    {
        return /* 304 with ETag */;
    }

    // 6. Seek + stream the slice (framework-specific body type).
    use tokio::io::AsyncSeekExt;
    file.seek(std::io::SeekFrom::Start(start)).await.map_err(/* 500 */)?;
    // stream `end - start + 1` bytes from `file` ...
}
```

## Design notes

- **TOCTOU consolidation (hash-vs-serve):** the ETag is hashed through an open file handle the caller has already opened. The file descriptor pins the inode even if the path is rewritten via `rename` mid-request, so the hash represents the same bytes the streaming code is about to send.
- **Cache contention shape:** the mutex is held only across the cache lookup and store. The expensive BLAKE3 hash happens outside the lock, so concurrent first-request callers may race (worst case: two concurrent first-requests both hash; they produce the same value; the second overwrite is benign).
- **Mutex poisoning:** recovered via `into_inner()`. The crate doesn't return an error to the caller on poison — the only operations inside the locked region are infallible (clone, equality, store), so poisoning would only arise from unrelated panicking code.
- **Single-range only:** multi-range responses (RFC 9110 §14.6) are not supported. The parser explicitly rejects comma-separated ranges with `RangeError`.
- **Cache-key constraint (`(mtime, size)`):** the cache trusts that any change of file content observably bumps mtime OR size. Standard operator rotation tools (`rsync`, `cp`, `mv`, atomic `rename`) all satisfy this. If a deployment can preserve BOTH mtime AND size across a content swap (e.g., `touch -d <prior-mtime>` after a same-size replacement), the cache will serve a stale ETag forever, and conditional-GET clients will be stuck on 304s describing the old bytes. Operator SOP must avoid this; if a future caller's threat model includes that workflow, key the cache on a content sentinel (BLAKE3 of the first/last 4 KiB) instead, at the cost of per-request work.
- **Postel-side leniency on whitespace:** `parse_range_str` trims whitespace around the hyphen, so `bytes=0 - 99` parses as `bytes=0-99`. RFC 9110 §14.1.2's BNF doesn't permit this; the parser accepts it as a conservative tolerance (no legitimate strict-RFC input gets rejected). A strict-compliance variant is a one-line change if a future consumer needs it.

## License

MIT OR Apache-2.0.
