#![cfg_attr(not(test), deny(clippy::unwrap_used))]
//! Session-bound CSRF tokens: BLAKE3 keyed mint + constant-time verify.
//!
//! Tokens are deterministic per binding value (typically a session id):
//! `token_for` is a BLAKE3 keyed-hash of the binding, keyed by a
//! process-local [`CsrfKey`] derived (via `blake3::derive_key`, so
//! domain-separated) from a caller-supplied secret. The key never leaves
//! the process, so an attacker at a different origin can't predict the
//! token even if they observe a victim's cookies.
//!
//! [`CsrfKey::verify`] is fail-closed: wrong length, non-hex input, or a
//! mismatched binding all return `false`, and the comparison runs in
//! constant time (`blake3::Hash` equality).
//!
//! What stays with the caller: the derivation context string (version it —
//! rotating the context invalidates every in-flight token without touching
//! the underlying secret), the secret's storage/encoding, and the framework
//! plumbing that threads tokens into forms and checks them on submit.

/// Derived CSRF signing key. Hold one per process (e.g. in the web
/// framework's managed state) and mint/verify per-request.
pub struct CsrfKey([u8; 32]);

impl CsrfKey {
    /// `blake3::derive_key(context, secret)`. Context versioning is the
    /// caller's (rotate context → invalidate in-flight tokens). `secret`
    /// is raw bytes — decoding from on-disk encodings (base64 etc.) is
    /// the caller's.
    pub fn derive(context: &str, secret: &[u8]) -> Self {
        CsrfKey(blake3::derive_key(context, secret))
    }

    /// Compute the canonical CSRF token for a binding value (e.g. a
    /// session id). Hex-encoded 32-byte BLAKE3 keyed hash; safe to embed
    /// in HTML and URLs.
    pub fn token_for(&self, binding: &str) -> String {
        blake3::keyed_hash(&self.0, binding.as_bytes())
            .to_hex()
            .to_string()
    }

    /// Constant-time check of a submitted token against the canonical
    /// token for `binding`. Hex-decodes the input, compares as
    /// `blake3::Hash` (which uses constant-time eq), and fails closed on
    /// any malformedness.
    pub fn verify(&self, binding: &str, provided: &str) -> bool {
        let provided = provided.trim();
        if provided.len() != 64 {
            return false;
        }
        let mut bytes = [0u8; 32];
        for (i, chunk) in provided.as_bytes().chunks_exact(2).enumerate() {
            let Some(hi) = hex_digit(chunk[0]) else {
                return false;
            };
            let Some(lo) = hex_digit(chunk[1]) else {
                return false;
            };
            bytes[i] = (hi << 4) | lo;
        }
        let provided_hash = blake3::Hash::from(bytes);
        let expected = blake3::keyed_hash(&self.0, binding.as_bytes());
                                                   
        expected == provided_hash
    }
}

fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_key() -> CsrfKey {
                                                                    
                                                    
        CsrfKey::derive("test ctx", &[7u8; 64])
    }

    #[test]
    fn token_round_trips() {
        let k = fixture_key();
        let t = k.token_for("session-abc");
        assert!(k.verify("session-abc", &t));
    }

    #[test]
    fn token_rejects_other_session() {
        let k = fixture_key();
        let t = k.token_for("session-abc");
        assert!(!k.verify("session-xyz", &t));
    }

    #[test]
    fn token_rejects_wrong_length_hex() {
        let k = fixture_key();
        assert!(!k.verify("session-abc", ""));
        assert!(!k.verify("session-abc", "deadbeef"));
        assert!(!k.verify("session-abc", &"a".repeat(63)));
        assert!(!k.verify("session-abc", &"a".repeat(65)));
    }

    #[test]
    fn token_rejects_non_hex() {
        let k = fixture_key();
        let t: String = "z".repeat(64);
        assert!(!k.verify("session-abc", &t));
    }

    #[test]
    fn keys_from_different_cookie_keys_differ() {
        let ka = CsrfKey::derive("test ctx", &[1u8; 64]);
        let kb = CsrfKey::derive("test ctx", &[2u8; 64]);
        let ta = ka.token_for("s");
        let tb = kb.token_for("s");
        assert_ne!(ta, tb);
        assert!(!ka.verify("s", &tb));
        assert!(!kb.verify("s", &ta));
    }

    #[test]
    fn keys_from_different_contexts_differ() {
        let ka = CsrfKey::derive("ctx a", &[7u8; 64]);
        let kb = CsrfKey::derive("ctx b", &[7u8; 64]);
        assert_ne!(ka.token_for("s"), kb.token_for("s"));
    }
}
