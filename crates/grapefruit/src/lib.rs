#![forbid(unsafe_code)]
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
//! Argon2id password hashing/verification with caller-injected cost params.
//!
//! [`Hasher`] wraps a configured `Argon2id v0x13` instance. Cost parameters
//! (memory/time/parallelism) are the caller's policy, passed in once at
//! construction — this crate owns the hashing *mechanics* only:
//!
//! - [`Hasher::hash`] — PHC string with a fresh OS-RNG salt per call.
//! - [`Hasher::verify`] — fail-closed: an **empty stored hash returns
//!   `false`** (the not-yet-activated-account sentinel — an empty DB column
//!   must never match any password), and an unparseable PHC string returns
//!   `false` rather than erroring.
//!
//! ## The dummy-verify anti-enumeration pattern
//!
//! Callers with a login route should burn the same Argon2 cost on the
//! no-such-user path as on the real-user path, or response timing leaks
//! which usernames exist. The pattern: precompute one real PHC string at
//! startup (e.g. `hasher.hash("__nobody_should_ever_match_this__")` in a
//! `LazyLock` static) and `verify` the submitted password against it
//! whenever there is no real stored hash to check. The static belongs to
//! the caller — it binds app policy (params, lifecycle) — but the timing
//! discipline only works if every branch of the auth path runs a real
//! verify.
//!
//! ## Cost-parameter floor
//!
//! [`Hasher::new`] panics unless the params meet at least one OWASP Password
//! Storage (2024) Argon2id config: **m >= 19456 KiB with t >= 2**, or
//! **m >= 47104 KiB with t >= 1** (p >= 1 throughout — argon2's own `Params`
//! already rejects p < 1). argon2's built-in minimum (m = 8 KiB) is far below
//! a safe cost; and a memory-only floor would still admit m=19456 with t=1,
//! about half the intended work — so the floor is the 2-D OWASP condition, not
//! a single `m` threshold. Sub-floor params are a misconfiguration, not a
//! policy choice: fail closed and loud at construction (the same stance as the
//! invalid-params panic). The panic fires when the hasher is *constructed* — a
//! caller that builds it lazily (e.g. behind a `LazyLock`) defers that from
//! process start to first use.
//!
//! ## No pepper (server-side secret) by default
//!
//! This crate hashes without a pepper (argon2's keyed secret). Deliberate
//! downgrade, not an oversight: a pepper only protects if it is stored
//! outside the database and a consumer operates its lifecycle, and there is
//! no such consumer today. A pepper is not recorded in the PHC string, so
//! rotating it invalidates every existing hash (no re-hash without the
//! plaintext) and losing it locks out every account — that lifecycle is a
//! consumer's, not these mechanics'. When a consumer needs one, add a
//! `new_with_pepper` over `Argon2::new_with_secret` and resolve, with that
//! consumer in hand: the secret's provenance (the held `Argon2` is
//! `'static`, so the pepper must be too), whether peppers are versioned so
//! they can rotate, and the loss-of-pepper availability stance — then
//! re-audit. Until then, a caller needing a pepper must wrap `argon2`
//! directly.
//!
//! ## Memory hygiene / zeroization
//!
//! This crate enables argon2's `zeroize` feature, so argon2 wipes its
//! password-derived initial hash and its intermediate block buffers after
//! use. Two things it does *not* wipe: the large (~`m_cost` KiB) Argon2
//! memory matrix, which argon2 frees without zeroing (it has no
//! `Drop`/`ZeroizeOnDrop`), and the `plaintext: &str` that [`Hasher::hash`] /
//! [`Hasher::verify`] borrow — this crate cannot zero memory it does not own,
//! so the password bytes live until their owner drops them. A caller pinning
//! secrets in memory must zero the plaintext at the call site (e.g. own it as
//! a `Zeroizing<String>`); the memory matrix is not addressable from here.
//!
//! ## No upgrade-on-verify (forward-debt)
//!
//! [`Hasher::verify`] returns only pass/fail, not "verified but below current
//! params" — there is no `needs_rehash`. So raising the floor or retuning
//! params does not re-cost existing stored hashes: they verify forever at
//! their embedded (older) cost. Closing this needs both a crate method
//! (`needs_rehash(stored) -> bool`, comparing the stored PHC params to the
//! instance) and a caller flow that re-hashes on successful login. Deferred
//! until a consumer drives that flow — none re-hashes on login today — so the
//! method's shape gets fixed by real use rather than guessed now.

use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::{Algorithm, Argon2, Params, Version};

/// OWASP Password Storage (2024) Argon2id memory floor (KiB) at `t_cost >= 2` —
/// the first recommended config (m=19456, t=2, p=1). See the crate-level
/// "Cost-parameter floor" note; [`Hasher::new`] enforces this *with* `t >= 2`.
pub const OWASP_MIN_M_COST: u32 = 19_456;

/// OWASP Password Storage (2024) Argon2id memory floor (KiB) at `t_cost == 1` —
/// the second recommended config (m=47104, t=1, p=1). The higher memory floor
/// compensates for the single time-cost pass.
pub const OWASP_MIN_M_COST_T1: u32 = 47_104;

/// A configured Argon2id hasher. Construct once (params are policy,
/// injected by the caller), reuse for every hash/verify.
pub struct Hasher(Argon2<'static>);

impl Hasher {
    /// Argon2id v0x13 with caller-supplied costs. Panics on invalid params,
    /// or on params below the OWASP 2024 floor (see the crate-level
    /// "Cost-parameter floor" note) — a misconfigured cost set must be loud at
    /// construction, not silently weakened.
    pub fn new(m_cost: u32, t_cost: u32, p_cost: u32) -> Self {
        assert!(
            (m_cost >= OWASP_MIN_M_COST && t_cost >= 2)
                || (m_cost >= OWASP_MIN_M_COST_T1 && t_cost >= 1),
            "argon2 params (m={m_cost}, t={t_cost}, p={p_cost}) are below the OWASP 2024 \
             minimum: require m>={OWASP_MIN_M_COST} with t>=2, or m>={OWASP_MIN_M_COST_T1} with t>=1"
        );
        let params = Params::new(m_cost, t_cost, p_cost, None).expect("argon2 params");
        Self(Argon2::new(Algorithm::Argon2id, Version::V0x13, params))
    }

    /// Hash `plaintext` into a PHC string with a fresh OS-RNG salt.
    pub fn hash(&self, plaintext: &str) -> String {
        let salt = SaltString::generate(&mut rand::rngs::OsRng);
        self.0
            .hash_password(plaintext.as_bytes(), &salt)
            .expect("argon2 hash")
            .to_string()
    }

    /// Verify `plaintext` against a stored PHC string. Empty `stored`
    /// always returns `false` (placeholder-account sentinel); a PHC string
    /// that fails to parse returns `false`.
    ///
    /// Timing: any stored value that cannot be verified — empty, unparseable,
    /// or a PHC string that parses but carries no salt/hash — returns in
    /// sub-microsecond time, while a verifiable hash runs the full Argon2
    /// computation (milliseconds; ~9 ms at the app's params). The gap is
    /// observable only to an attacker who can already write the `stored`
    /// column (inject a non-verifiable value for a chosen account, then time
    /// logins to confirm the write). Not independently exploitable — it
    /// presupposes prior write access; a caller that wants to equalize it runs
    /// a dummy verify on the no-real-hash paths, as the dummy-verify pattern
    /// above already does for the no-such-user branch.
    pub fn verify(&self, plaintext: &str, stored: &str) -> bool {
        if stored.is_empty() {
            return false;
        }
        let parsed = match PasswordHash::new(stored) {
            Ok(p) => p,
            Err(_) => return false,
        };
        self.0
            .verify_password(plaintext.as_bytes(), &parsed)
            .is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_round_trip() {
                                                                         
                                                                             
                                                                       
                                                                          
                                                                      
                                                                      
                                                                 
        let hasher = Hasher::new(19_456, 2, 1);
        let h = hasher.hash("hunter2");
        assert!(hasher.verify("hunter2", &h));
        assert!(!hasher.verify("wrong", &h));
        assert!(!hasher.verify("hunter2", ""));            
    }

    #[test]
    #[should_panic(expected = "below the OWASP 2024")]
    fn sub_floor_params_panic() {
                                                                        
                                                                      
        let _ = Hasher::new(8, 1, 1);
    }

    #[test]
    #[should_panic(expected = "below the OWASP 2024")]
    fn m_floor_with_single_pass_panics() {
                                                                               
                                                                               
                          
        let _ = Hasher::new(19_456, 1, 1);
    }

    #[test]
    fn owasp_second_config_t1_is_accepted() {
                                                                               
                                                           
        let hasher = Hasher::new(47_104, 1, 1);
        let h = hasher.hash("hunter2");
        assert!(hasher.verify("hunter2", &h));
    }
}
