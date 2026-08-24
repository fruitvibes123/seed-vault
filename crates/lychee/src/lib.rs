#![cfg_attr(not(test), deny(clippy::unwrap_used))]
//! URL-safe random token mint + deterministic BLAKE3 storage hash.
//!
//! The raw-token-shown-once / hash-stored pattern: [`mint_token`] draws
//! `n_bytes` from the OS RNG and returns the URL-safe base64 raw token
//! together with its storage hash. The hash is plain BLAKE3 hex —
//! **deterministic**, so it works under a database UNIQUE index and as a
//! lookup key; the raw token never touches storage.
//!
//! [`hash_token_namespaced`] is the domain-separated variant
//! (`blake3::derive_key`): two token families hashed under different domain
//! strings live in disjoint hash namespaces, so a leaked hash from one
//! family can never be replayed against another's lookup table.
//!
//! Token policy — which families exist, their byte lengths, TTLs, and
//! uniqueness rules — belongs to the caller. This crate only mints and
//! hashes.

use base64::Engine as _;

/// Generate a high-entropy token of `n_bytes` from the OS RNG. Returns
/// `(raw, hash)`: `raw` is the URL-safe no-pad base64 form (shown once,
/// never stored); `hash` is `hash_token(&raw)` (stored).
pub fn mint_token(n_bytes: usize) -> (String, String) {
    use rand::RngCore;
    let mut bytes = vec![0u8; n_bytes];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    let hash = hash_token(&raw);
    (raw, hash)
}

/// Storage hash of a raw token: BLAKE3, hex-encoded. Deterministic (so a
/// UNIQUE index and point lookups work), fast (so per-request verification
/// is cheap).
pub fn hash_token(raw: &str) -> String {
    blake3::hash(raw.as_bytes()).to_hex().to_string()
}

/// Domain-separated storage hash: `blake3::derive_key(domain, raw)`,
/// hex-encoded. Separates one token family's hash namespace from every
/// other's, so a hash leaked from one table can never be replayed against
/// another. Hygiene on top of preimage resistance, not a substitute for it.
pub fn hash_token_namespaced(domain: &str, raw: &str) -> String {
    blake3::derive_key(domain, raw.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_hash_is_deterministic() {
        let (raw, hash) = mint_token(16);
        assert_eq!(hash_token(&raw), hash);
    }

    #[test]
    fn minted_raw_token_is_22_url_safe_chars_for_16_bytes() {
                                                                             
        let (raw, _) = mint_token(16);
        assert_eq!(raw.len(), 22);
        assert!(
            raw.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
            "URL_SAFE_NO_PAD alphabet only, got {raw:?}"
        );
    }

    #[test]
    fn namespaced_hash_differs_from_plain_hash() {
        let raw = "some-raw-token";
        assert_ne!(hash_token_namespaced("a", raw), hash_token(raw));
    }
}
