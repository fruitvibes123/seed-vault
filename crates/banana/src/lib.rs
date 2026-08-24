//! Framework-agnostic HTTP byte-range parser + mtime-keyed BLAKE3 ETag cache.
//!
//! Extracted from the recipes project's download-service implementation
//! after the 2026-05-18 agent-productivity experiment selected these two
//! primitives as the highest-leverage reusable pieces. See `README.md` for
//! a usage sketch + design notes.

use std::sync::Mutex;
use std::time::SystemTime;

use tokio::io::AsyncReadExt;

                                                                                 

/// A parsed `Range` header value. `Explicit` covers `bytes=A-B` and
/// `bytes=A-` (with `end: None`); `Suffix(N)` covers `bytes=-N`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeSpec {
    /// `bytes=A-B` (inclusive end) or `bytes=A-` (`end: None` = through EOF).
    Explicit { start: u64, end: Option<u64> },
    /// `bytes=-N` — the last N bytes of the file.
    Suffix(u64),
}

/// Range syntax error: malformed input, non-`bytes` unit, multi-range, or
/// non-numeric endpoints. Callers map this to HTTP 400.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RangeError;

impl std::fmt::Display for RangeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("invalid range syntax")
    }
}

impl std::error::Error for RangeError {}

/// Parse a single-range `Range` header value (the raw bytes after the
/// header name, e.g., `bytes=0-99`). Returns the parsed `RangeSpec` on
/// success.
///
/// Errors on:
/// - Missing `bytes=` prefix
/// - Multi-range (contains `,`)
/// - Empty body (`bytes=`)
/// - Empty start AND empty end (`bytes=-`)
/// - Non-numeric start or end
/// - Inverted range (`bytes=B-A` where B < A)
///
/// Does NOT error on `bytes=-0` (well-formed syntactically); the
/// resolution step maps that to `None` (416 territory).
pub fn parse_range_str(raw: &str) -> Result<RangeSpec, RangeError> {
    let body = raw.strip_prefix("bytes=").ok_or(RangeError)?;
    if body.contains(',') {
        return Err(RangeError);
    }
    let (start_s, end_s) = body.split_once('-').ok_or(RangeError)?;
    let start_s = start_s.trim();
    let end_s = end_s.trim();
    match (start_s.is_empty(), end_s.is_empty()) {
        (true, true) => Err(RangeError),
        (true, false) => {
            let n: u64 = end_s.parse().map_err(|_| RangeError)?;
            Ok(RangeSpec::Suffix(n))
        }
        (false, true) => {
            let start: u64 = start_s.parse().map_err(|_| RangeError)?;
            Ok(RangeSpec::Explicit { start, end: None })
        }
        (false, false) => {
            let start: u64 = start_s.parse().map_err(|_| RangeError)?;
            let end: u64 = end_s.parse().map_err(|_| RangeError)?;
            if end < start {
                return Err(RangeError);
            }
            Ok(RangeSpec::Explicit {
                start,
                end: Some(end),
            })
        }
    }
}

impl RangeSpec {
    /// Resolve against the file's known size. Returns the inclusive
    /// `(start, end)` byte indices on success, or `None` for an
    /// out-of-bounds range (caller maps to HTTP 416).
    pub fn resolve(self, file_size: u64) -> Option<(u64, u64)> {
        if file_size == 0 {
            return None;
        }
        match self {
            RangeSpec::Explicit { start, end } => {
                if start >= file_size {
                    return None;
                }
                let end = match end {
                    Some(e) => e.min(file_size - 1),
                    None => file_size - 1,
                };
                Some((start, end))
            }
            RangeSpec::Suffix(n) => {
                if n == 0 {
                    return None;
                }
                let n = n.min(file_size);
                Some((file_size - n, file_size - 1))
            }
        }
    }
}

                                                                                 

#[derive(Debug, Clone)]
struct EtagCacheEntry {
    mtime: SystemTime,
    size: u64,
    etag_hex: String,
}

/// mtime+size-keyed BLAKE3 ETag cache. Hashes through an open
/// `tokio::fs::File` handle (TOCTOU-safe), caches `(mtime, size) →
/// hex-digest`, and returns the cached value on matching mtime AND size.
///
/// One cache instance per file path. Multi-file caches need a separate
/// `HashMap<PathBuf, EtagCache>` or equivalent — see README.
///
/// Safe to share across tasks via `Arc<EtagCache>`. The internal mutex
/// is held only across cache lookup/store; the BLAKE3 hash work happens
/// outside the lock.
pub struct EtagCache {
    inner: Mutex<Option<EtagCacheEntry>>,
}

impl Default for EtagCache {
    fn default() -> Self {
        Self::new()
    }
}

impl EtagCache {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(None),
        }
    }

    /// Return the cached ETag if `(mtime, size)` matches; otherwise
    /// hash the open handle's bytes from current position to EOF, store
    /// the result, and return it.
    ///
    /// The file handle is read from its current cursor position. Callers
    /// SHOULD have the cursor at 0 to hash the whole file. On return the
    /// cursor is at EOF — the caller is responsible for seeking back if
    /// it intends to stream from the same handle.
    ///
    /// Peak-allocates ~64 KiB regardless of file size.
    pub async fn get_or_compute(
        &self,
        file: &mut tokio::fs::File,
        mtime: SystemTime,
        size: u64,
    ) -> std::io::Result<String> {
                                                                   
                                                                    
                                                                        
                                                                
                                                               
                                                  
        {
            let guard = match self.inner.lock() {
                Ok(g) => g,
                Err(poisoned) => poisoned.into_inner(),
            };
            if let Some(entry) = guard.as_ref()
                && entry.mtime == mtime
                && entry.size == size
            {
                return Ok(entry.etag_hex.clone());
            }
        }
                                                                       
                                                                       
                                                                        
                                                      
        let mut hasher = blake3::Hasher::new();
        let mut buf = [0u8; 64 * 1024];
        loop {
            let n = file.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }
        let etag_hex = hasher.finalize().to_hex().to_string();
        {
            let mut guard = match self.inner.lock() {
                Ok(g) => g,
                Err(poisoned) => poisoned.into_inner(),
            };
            *guard = Some(EtagCacheEntry {
                mtime,
                size,
                etag_hex: etag_hex.clone(),
            });
        }
        Ok(etag_hex)
    }
}

/// Render a hex digest as the quoted strong-ETag header value per
/// RFC 9110 §8.8.3: `"<hex>"`. Use the returned `String` as-is in the
/// `ETag` response header.
pub fn quote_etag(hex: &str) -> String {
    format!("\"{hex}\"")
}

/// Match an `If-None-Match` header value against a resource's current
/// strong ETag using RFC 9110 §13.1.2 weak comparison semantics.
///
/// `if_none_match_raw` is the raw header value, e.g. `"abc"`, `W/"abc"`,
/// `"abc", "def"`, or the wildcard `*`. `current_etag` is the resource's
/// current quoted ETag value (the [`quote_etag`] output, e.g.
/// `"af3b1c..."`).
///
/// Returns `true` if the conditional GET should short-circuit to 304 —
/// i.e., the wildcard matches, or any listed entity-tag matches the
/// current ETag under weak comparison (RFC 9110 §8.8.3.2: the opaque
/// tag portions are byte-identical regardless of either side being
/// tagged `W/` weak).
///
/// **Wildcard placement (R2 audit I2):** `*` is honored only when it is
/// the entire trimmed header value. RFC 9110 §8.8.3.1's ABNF is
/// `If-None-Match = "*" / 1#entity-tag` — the wildcard is mutually
/// exclusive with the entity-tag list. A header like `"\"abc\", *"`
/// returns `false` (the `*` is treated as a malformed list token rather
/// than a top-level wildcard).
///
/// **Comma boundary (R2 audit I3):** the comma split does NOT track
/// DQUOTE pairing. RFC 9110 §8.8.3 permits commas inside an opaque-tag
/// value, but this matcher's `split(',')` would mis-tokenize such a
/// value. The [`quote_etag`] helper that this crate ships produces
/// BLAKE3 hex digests (`0-9a-f` only), so no comma-bearing ETag is
/// emitted by any in-tree consumer. A future caller minting
/// comma-bearing opaque tags would trip this gap; that workflow is a
/// known niche the crate doesn't promise to support.
///
/// Folded into the crate at R1 audit L3 (2026-05-19) so each consumer
/// doesn't hand-roll a narrower single-value matcher that diverges from
/// the RFC. The recipes download route uses this directly; the impl-5
/// experiment ancestor's `request_matches_etag` was the design source.
pub fn matches_if_none_match(if_none_match_raw: &str, current_etag: &str) -> bool {
    let trimmed = if_none_match_raw.trim();
    if trimmed == "*" {
        return true;
    }
    let current_opaque = strip_weak_prefix(current_etag);
    for token in trimmed.split(',') {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }
        if strip_weak_prefix(token) == current_opaque {
            return true;
        }
    }
    false
}

fn strip_weak_prefix(etag: &str) -> &str {
    etag.strip_prefix("W/").unwrap_or(etag)
}

                                                                                 

#[cfg(test)]
mod tests {
    use super::*;

                            

    #[test]
    fn parses_explicit_range() {
        assert_eq!(
            parse_range_str("bytes=0-99").unwrap(),
            RangeSpec::Explicit {
                start: 0,
                end: Some(99)
            }
        );
    }

    #[test]
    fn parses_open_ended_range() {
        assert_eq!(
            parse_range_str("bytes=500-").unwrap(),
            RangeSpec::Explicit {
                start: 500,
                end: None
            }
        );
    }

    #[test]
    fn parses_suffix_range() {
        assert_eq!(
            parse_range_str("bytes=-100").unwrap(),
            RangeSpec::Suffix(100)
        );
    }

    #[test]
    fn rejects_missing_unit_prefix() {
        assert!(parse_range_str("0-99").is_err());
    }

    #[test]
    fn rejects_wrong_unit() {
        assert!(parse_range_str("items=0-99").is_err());
    }

    #[test]
    fn rejects_multi_range() {
        assert!(parse_range_str("bytes=0-99,200-299").is_err());
    }

    #[test]
    fn rejects_empty_body() {
        assert!(parse_range_str("bytes=").is_err());
    }

    #[test]
    fn rejects_empty_start_and_end() {
        assert!(parse_range_str("bytes=-").is_err());
    }

    #[test]
    fn rejects_non_numeric() {
        assert!(parse_range_str("bytes=garbage").is_err());
        assert!(parse_range_str("bytes=a-b").is_err());
    }

    #[test]
    fn rejects_inverted_range() {
        assert!(parse_range_str("bytes=99-0").is_err());
    }

    #[test]
    fn parses_zero_suffix_as_well_formed_syntax() {
        assert_eq!(parse_range_str("bytes=-0").unwrap(), RangeSpec::Suffix(0));
    }

    #[test]
    fn parses_whitespace_around_hyphen() {
        assert_eq!(
            parse_range_str("bytes=0 - 99").unwrap(),
            RangeSpec::Explicit {
                start: 0,
                end: Some(99)
            }
        );
    }

                               

    #[test]
    fn resolve_explicit_in_bounds() {
        let spec = RangeSpec::Explicit {
            start: 0,
            end: Some(99),
        };
        assert_eq!(spec.resolve(1024), Some((0, 99)));
    }

    #[test]
    fn resolve_explicit_open_ended() {
        let spec = RangeSpec::Explicit {
            start: 500,
            end: None,
        };
        assert_eq!(spec.resolve(1024), Some((500, 1023)));
    }

    #[test]
    fn resolve_explicit_end_clamps_to_file_size() {
        let spec = RangeSpec::Explicit {
            start: 0,
            end: Some(9999),
        };
        assert_eq!(spec.resolve(1024), Some((0, 1023)));
    }

    #[test]
    fn resolve_explicit_start_past_eof_returns_none() {
        let spec = RangeSpec::Explicit {
            start: 9999,
            end: None,
        };
        assert_eq!(spec.resolve(1024), None);
    }

    #[test]
    fn resolve_suffix_in_bounds() {
        assert_eq!(RangeSpec::Suffix(100).resolve(1024), Some((924, 1023)));
    }

    #[test]
    fn resolve_suffix_larger_than_file_clamps_to_whole_file() {
        assert_eq!(RangeSpec::Suffix(9999).resolve(1024), Some((0, 1023)));
    }

    #[test]
    fn resolve_zero_suffix_returns_none() {
        assert_eq!(RangeSpec::Suffix(0).resolve(1024), None);
    }

    #[test]
    fn resolve_any_range_against_zero_byte_file_returns_none() {
        assert_eq!(
            RangeSpec::Explicit {
                start: 0,
                end: None
            }
            .resolve(0),
            None
        );
        assert_eq!(RangeSpec::Suffix(100).resolve(0), None);
    }

                       

    #[test]
    fn quotes_strong_etag() {
        assert_eq!(quote_etag("af3b1c"), r#""af3b1c""#);
    }

                                  

    #[test]
    fn inm_single_strong_etag_matches() {
        assert!(matches_if_none_match(r#""abc""#, r#""abc""#));
    }

    #[test]
    fn inm_single_strong_etag_mismatches() {
        assert!(!matches_if_none_match(r#""abc""#, r#""def""#));
    }

    #[test]
    fn inm_wildcard_matches_any_existing_resource() {
        assert!(matches_if_none_match("*", r#""whatever""#));
    }

    #[test]
    fn inm_weak_matches_strong_under_weak_comparison() {
                                                                        
                                                                    
        assert!(matches_if_none_match(r#"W/"abc""#, r#""abc""#));
    }

    #[test]
    fn inm_comma_separated_list_matches_any_member() {
        assert!(matches_if_none_match(r#""def", "abc", "ghi""#, r#""abc""#));
    }

    #[test]
    fn inm_comma_separated_list_misses_when_no_member_matches() {
        assert!(!matches_if_none_match(r#""def", "ghi", "jkl""#, r#""abc""#));
    }

    #[test]
    fn inm_empty_tokens_are_ignored() {
                                                              
        assert!(matches_if_none_match(r#""abc", "#, r#""abc""#));
        assert!(!matches_if_none_match(r#",,,"#, r#""abc""#));
    }

    #[test]
    fn inm_wildcard_in_list_is_not_honoured() {
                                                                         
                                                                        
                                                                       
                                                                         
                                                     
        assert!(!matches_if_none_match(r#""foo", *, "bar""#, r#""xyz""#));
        assert!(!matches_if_none_match(r#""abc", *"#, r#""xyz""#));
                                                                  
        assert!(matches_if_none_match("*", r#""xyz""#));
    }

                      

    #[tokio::test]
    async fn etag_hashes_open_file_and_caches() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let path = tmp.path().to_owned();
        let payload: Vec<u8> = (0..1024u32).map(|i| (i % 256) as u8).collect();
        tokio::fs::write(&path, &payload).await.unwrap();

        let mut file = tokio::fs::File::open(&path).await.unwrap();
        let meta = file.metadata().await.unwrap();
        let mtime = meta.modified().unwrap();
        let size = meta.len();

        let cache = EtagCache::new();
        let first = cache.get_or_compute(&mut file, mtime, size).await.unwrap();
        assert!(!first.is_empty());
        assert_eq!(first.len(), 64);

        let mut file2 = tokio::fs::File::open(&path).await.unwrap();
        let second = cache.get_or_compute(&mut file2, mtime, size).await.unwrap();
        assert_eq!(first, second);
    }

    #[tokio::test]
    async fn etag_recomputes_on_mtime_change() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let path = tmp.path().to_owned();
        tokio::fs::write(&path, b"v1").await.unwrap();

        let mut file = tokio::fs::File::open(&path).await.unwrap();
        let meta = file.metadata().await.unwrap();
        let mtime1 = meta.modified().unwrap();
        let size1 = meta.len();

        let cache = EtagCache::new();
        let etag1 = cache
            .get_or_compute(&mut file, mtime1, size1)
            .await
            .unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
        tokio::fs::write(&path, b"v2-different").await.unwrap();

        let mut file2 = tokio::fs::File::open(&path).await.unwrap();
        let meta2 = file2.metadata().await.unwrap();
        let mtime2 = meta2.modified().unwrap();
        let size2 = meta2.len();
        assert_ne!(mtime1, mtime2, "test relies on mtime granularity");

        let etag2 = cache
            .get_or_compute(&mut file2, mtime2, size2)
            .await
            .unwrap();
        assert_ne!(etag1, etag2);
    }
}
