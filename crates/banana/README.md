# banana

Framework-agnostic HTTP byte-range parser and mtime-keyed BLAKE3 ETag cache. The crate has no framework dependencies; it works with Rocket, Axum, any other HTTP server, or non-HTTP code that wants the underlying logic.

## API

- `parse_range_str`: RFC 9110 §14.2 single-range parser. Returns `RangeSpec::Explicit { start, end }` or `RangeSpec::Suffix(n)`. A comma-separated multi-range value fails with `RangeError`, per the `single-range-only → 400` policy that recipes' download spec pins; multi-range responses (RFC 9110 §14.6) are not supported.
- `RangeError`: unit-struct error returned by `parse_range_str` on any malformed input (missing `bytes=` prefix, multi-range, empty, non-numeric, inverted). Callers map it to HTTP 400.
- `RangeSpec::resolve(file_size)`: resolves a parsed range against the file's known size. Returns inclusive `(start, end)` byte indices, or `None` for an out-of-bounds range (caller maps to 416).
- `EtagCache`: mtime+size-keyed BLAKE3 hash cache that hashes through an open `tokio::fs::File` handle. One instance holds the entry for one file path; a multi-file server keeps a `HashMap<PathBuf, EtagCache>` or equivalent.
- `quote_etag`: renders a hex digest as the quoted strong-ETag header value per RFC 9110 §8.8.3.
- `matches_if_none_match`: RFC 9110 §13.1.2 conditional-GET matcher. Handles single-value, comma-separated list, `W/`-weak prefix, and `*` wildcard. Returns `true` when the consumer should short-circuit to 304.

## Usage

Handler sketch for Rocket; an Axum handler follows the same steps.

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

## Guarantees and limits

- Hash-vs-serve TOCTOU: the ETag is hashed through a file handle the caller has already opened. The file descriptor pins the inode even if the path is rewritten via `rename` mid-request, so the bytes whose hash is the ETag are the bytes the streaming path serves.
- Lock scope: the mutex is held only across the cache lookup and store, and the BLAKE3 hash runs outside the lock. Two concurrent first requests can both hash; they produce the same value, so the second store is benign.
- Mutex poisoning: the locked region holds only infallible operations (clone, equality, store), so a poison can come only from unrelated panicking code. The cache recovers through `into_inner()` and returns no error to the caller, so a poison can't leak into the response body.
- Cache key `(mtime, size)`: the cache trusts that any change of file content observably bumps mtime or size. Standard operator rotation tools (`rsync`, `cp`, `mv`, atomic `rename`) all satisfy this. A deployment that preserves both mtime and size across a content swap (e.g., `touch -d <prior-mtime>` after a same-size replacement) makes the cache serve a stale ETag forever, and conditional-GET clients stay stuck on 304s describing the old bytes. Operator SOP must avoid this. A caller whose threat model includes that workflow needs the cache keyed on a content sentinel (BLAKE3 of the first/last 4 KiB) instead, at the cost of per-request work.
- Whitespace (Postel-side leniency): `parse_range_str` trims whitespace around the hyphen, so `bytes=0 - 99` parses as `bytes=0-99`. RFC 9110 §14.1.2's BNF doesn't permit this. The tolerance rejects no legitimate strict-RFC input, and a strict-compliance variant is a one-line change if a consumer needs it.

Extracted from a private download-service implementation as its two highest-leverage reusable primitives.

## License

MIT OR Apache-2.0.
