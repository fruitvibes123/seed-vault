#![cfg_attr(not(test), deny(clippy::unwrap_used))]
//! Generic per-key attempt budget — storage trait, burn semantics,
//! framework-free.
//!
//! ## Concepts
//!
//! - **[`AttemptKind`]** — marker trait per use case. Names the key type,
//!   the budget, the soft-lock retry window, the wire-visible exhaustion
//!   status code + header. The recipes app's `PairAttempt` (a per-pairing-
//!   code attempt budget) is the first consumer; the same shape fits
//!   biometric re-pair prompts, password-reset attempts, etc.
//!
//! - **[`BudgetStorage<A>`]** — trait for the persistent state behind the
//!   budget. Two methods (`check` + `decrement`) deliberately split so a
//!   request guard can do ONLY `check` (read-only, no side effects on every
//!   request) and route handlers call `decrement` only when the attempt
//!   counted (e.g., a credential validation failed).
//!
//! - A **router adapter** (per framework, owned by the consumer) reads
//!   `A::Key` from the request — conventionally the route's first dynamic
//!   path-param segment — calls `BudgetStorage::check`, and fails the
//!   request with `A::EXHAUSTION_STATUS_CODE` when the budget is gone. The
//!   recipes `AttemptBudget<A>` Rocket guard is the reference adapter.
//!
//! ## Threat-model note
//!
//! `A::Key` is typically a `Secret`-class path parameter — the path bytes
//! ARE the credential. Consumers MUST:
//!
//! - Redact the key's route prefix from request logs (else the key
//!   surfaces in proxy + journal logs).
//! - Maintain per-IP throttling on the route as a separate axis —
//!   the attempt budget is per-KEY only; per-IP brute-force defense
//!   (and any `Retry-After` rate-limit signaling) lives elsewhere.
//! - Pick `BUDGET` so the brute-force cost of guessing `A::Key` is
//!   infeasible at the implied attempt rate.

/// Marker trait per use case. See crate docs.
pub trait AttemptKind: Sized + Send + Sync + 'static {
    /// The budget key type. Router adapters parse it from the request
    /// (e.g. a path parameter), so it must be constructible from a path
    /// segment in the adapter's framework; any parse bound (Rocket's
    /// `FromParam`, etc.) belongs to the adapter, not this trait.
    ///
    /// Bounds intentionally minimal: any storage backend that needs
    /// `Eq + Hash` (e.g., a HashMap-keyed in-memory storage) adds
    /// those bounds in its own impl where-clause, not at the trait
    /// level. Keep the trait surface to the minimum the generic
    /// abstraction requires; defer broader bounds until a concrete
    /// consumer signals the need.
    type Key: Clone + Send + Sync + 'static;

    /// Total attempts allowed against a single key before exhaustion.
    /// The 1st exhausting attempt is the BURN; subsequent attempts
    /// hit the adapter's `Exhausted` path before the route runs.
    const BUDGET: u32;

    /// HTTP status code (bare u16 — no framework status type) the
    /// adapter returns when `check` fails with `Exhausted`; the adapter
    /// maps it into its framework's type (Rocket: `Status::new(code)`).
    /// Conventionally 410 Gone for codes/tokens (the resource IS gone
    /// after burning) or 429 for rate-limited surfaces.
    const EXHAUSTION_STATUS_CODE: u16;

    /// Optional response header to set on the BURN (the attempt that
    /// exhausts the budget). Route handlers SHOULD read this via
    /// `<A as AttemptKind>::EXHAUSTION_HEADER` and add it to the burn
    /// response. The adapter's exhausted-path (subsequent attempts after
    /// the burn already happened) does NOT add the header; the burn
    /// happened on a prior request.
    const EXHAUSTION_HEADER: Option<(&'static str, &'static str)>;

    /// Short identifier for logs / debug-asserts. Lowercase, no-spaces.
    const NAME: &'static str;
}

/// Persistent state behind an `AttemptKind`'s budget. Split into
/// `check` (read-only) + `decrement` (mutating) so a request guard can
/// gate cheaply per-request while route handlers control when an
/// attempt actually counts.
#[async_trait::async_trait]
pub trait BudgetStorage<A: AttemptKind>: Send + Sync {
    /// Returns `Ok(())` if `key` has remaining budget; `Err(Exhausted)`
    /// if the budget is zero (or the key doesn't exist — same wire
    /// outcome). Read-only; safe to call on every request.
    ///
    /// **Storage-returnable variants**:
    /// `check` and `decrement` impls MAY return ONLY
    /// [`AttemptError::Exhausted`] or [`AttemptError::StorageFailure`].
    /// The guard-internal variants (`NoRoute`, `NoDynamicParam`,
    /// `BadRoutedSegment`, `BadKey`, `MissingState`) are reserved for the
    /// router adapter's own request-resolution paths and MUST NOT be
    /// returned by storage. Future storage impls that surface a new
    /// failure mode (e.g., race-distinguished sentinel — see `decrement`
    /// doc below) extend `AttemptError` and update this constraint
    /// accordingly.
    async fn check(&self, key: &A::Key) -> Result<(), AttemptError>;

    /// Records an attempt and decrements budget for `key`. Returns
    /// `BurnStatus::JustBurned` if THIS decrement exhausted the
    /// budget; `BurnStatus::StillAlive` if attempts remain — OR if
    /// the key was already gone at decrement time (the
    /// check-then-decrement race; see below). Same storage-returnable
    /// variant constraint as `check` (`Exhausted` | `StorageFailure`).
    ///
    /// ## Race semantics
    ///
    /// The trait does NOT promise `Err(Exhausted)` on the
    /// check-decrement race (the guard's check passed but a
    /// concurrent attempt burned the key first). An impl MAY collapse
    /// "decremented-still-alive" and "key-no-longer-exists" onto the
    /// same `Ok(StillAlive)` wire value (the recipes DB-backed impl
    /// does). Callers MUST treat `Ok(StillAlive)` as advisory, not
    /// authoritative — the next `check` on the same key WILL surface
    /// the exhaustion (return `Err(Exhausted)`). The race self-heals
    /// on retry; the client gets one extra round-trip in the rare
    /// concurrent-burn case.
    ///
    /// Future storage impls MAY distinguish the race (e.g., return
    /// a dedicated sentinel) if a consumer needs to log "race lost";
    /// the trait surface deliberately leaves room for both shapes.
    async fn decrement(&self, key: &A::Key) -> Result<BurnStatus, AttemptError>;

    /// Default: atomic-ish check + decrement. Most consumers should
    /// call `check` from the guard + `decrement` from the route
    /// handler on the failure path. This combined method is here for
    /// consumers that want one-shot accounting (e.g., a biometric
    /// re-pair flow where every failure-or-success is an attempt
    /// against the same budget).
    async fn check_and_decrement(&self, key: &A::Key) -> Result<BurnStatus, AttemptError> {
        self.check(key).await?;
        self.decrement(key).await
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BurnStatus {
    /// Attempts remain after this decrement.
    StillAlive,
    /// This decrement exhausted the budget. Route handler should add
    /// `EXHAUSTION_HEADER` to the response if set.
    JustBurned,
}

/// Error surface shared by storage impls and router adapters. The five
/// guard-internal variants are framework-generic failure CLASSES — any
/// router adapter resolving a key from a matched route hits the same
/// five ways to fail — but only `Exhausted` and `StorageFailure` may be
/// returned by `BudgetStorage` impls (see the trait docs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttemptError {
    /// Budget exhausted (or key doesn't exist). Adapter returns
    /// `EXHAUSTION_STATUS_CODE`.
    Exhausted,
    /// Storage backend error (sqlite, network, etc.). Adapter returns
    /// 500. NOT meant to convey rate-limit info to the client.
    StorageFailure,
    /// The framework dispatched the request to the guard without a
    /// matched route (e.g. from a fairing / catcher), so the adapter
    /// cannot resolve the dynamic-segment slot. Adapter returns 500.
    NoRoute,
    /// The matched route has no dynamic path-param segment (`<...>`).
    /// The adapter cannot extract a key; returns 400. Indicates a route
    /// registration that took the budget guard without a dynamic param —
    /// consumers should also catch this at build/test time (the recipes
    /// app pins it via a path-param convention test).
    NoDynamicParam,
    /// The route's URI template promised a segment at index N but the
    /// actual request URI doesn't have one. Adapter returns 400
    /// (malformed request from the caller's POV; routing-layer mismatch
    /// from the framework's POV).
    BadRoutedSegment,
    /// The dynamic segment failed to parse as `A::Key`. Adapter returns
    /// 400 (caller submitted a value of the wrong shape).
    BadKey,
    /// The `BudgetStorage<A>` impl is not registered in the framework's
    /// state container, so the adapter cannot consult the budget;
    /// returns 500. Indicates a missing registration (Rocket: a missing
    /// `.manage(...)`) in the consumer's app builder.
    MissingState,
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::hash::Hash;

    /// Synthetic AttemptKind for trait-shape unit tests.
    struct TestKind;
    impl AttemptKind for TestKind {
        type Key = i64;
        const BUDGET: u32 = 3;
        const EXHAUSTION_STATUS_CODE: u16 = 410;
        const EXHAUSTION_HEADER: Option<(&'static str, &'static str)> =
            Some(("X-Test-Burned", "1"));
        const NAME: &'static str = "test";
    }

    #[test]
    fn attempt_kind_const_surface_is_inspectable() {
                                                                       
                                                                
                                                                    
                                                              
        assert_eq!(<TestKind as AttemptKind>::BUDGET, 3);
        assert_eq!(<TestKind as AttemptKind>::EXHAUSTION_STATUS_CODE, 410);
        assert_eq!(
            <TestKind as AttemptKind>::EXHAUSTION_HEADER,
            Some(("X-Test-Burned", "1"))
        );
        assert_eq!(<TestKind as AttemptKind>::NAME, "test");
    }

    /// An in-process `BudgetStorage` for trait-level testing. Useful as a
    /// reference impl and as a future per-process (no DB) storage shape.
    ///
    /// Storage-impl-local bounds: the HashMap-keyed shape requires
    /// `Eq + Hash` on `A::Key`. Per the trait's minimal-bounds
    /// discipline, the bounds live HERE (storage where-clause) rather
    /// than on the trait itself, so future consumers with non-hashable
    /// keys aren't locked out of `AttemptKind`.
    struct InMemoryStorage<A: AttemptKind>
    where
        A::Key: Eq + Hash,
    {
        counts: std::sync::Mutex<std::collections::HashMap<A::Key, u32>>,
    }

    impl<A: AttemptKind> InMemoryStorage<A>
    where
        A::Key: Eq + Hash,
    {
        fn new() -> Self {
            Self {
                counts: std::sync::Mutex::new(std::collections::HashMap::new()),
            }
        }
    }

    #[async_trait::async_trait]
    impl<A: AttemptKind> BudgetStorage<A> for InMemoryStorage<A>
    where
        A::Key: Eq + Hash,
    {
        async fn check(&self, key: &A::Key) -> Result<(), AttemptError> {
            let counts = self.counts.lock().expect("test mutex");
            let used = counts.get(key).copied().unwrap_or(0);
            if used >= A::BUDGET {
                Err(AttemptError::Exhausted)
            } else {
                Ok(())
            }
        }
        async fn decrement(&self, key: &A::Key) -> Result<BurnStatus, AttemptError> {
            let mut counts = self.counts.lock().expect("test mutex");
            let used = counts.entry(key.clone()).or_insert(0);
            *used += 1;
            if *used >= A::BUDGET {
                Ok(BurnStatus::JustBurned)
            } else {
                Ok(BurnStatus::StillAlive)
            }
        }
    }

    #[tokio::test]
    async fn in_memory_storage_burns_after_budget_attempts() {
        let s: InMemoryStorage<TestKind> = InMemoryStorage::new();
                                                             
        assert_eq!(s.decrement(&42).await.unwrap(), BurnStatus::StillAlive);
        assert_eq!(s.decrement(&42).await.unwrap(), BurnStatus::StillAlive);
                                   
        assert_eq!(s.decrement(&42).await.unwrap(), BurnStatus::JustBurned);
                                                  
        assert_eq!(s.check(&42).await, Err(AttemptError::Exhausted));
                                             
        assert_eq!(s.check(&999).await, Ok(()));
    }

    #[tokio::test]
    async fn check_and_decrement_default_impl_atomic_ish() {
        let s: InMemoryStorage<TestKind> = InMemoryStorage::new();
                                                                  
        assert_eq!(
            s.check_and_decrement(&42).await.unwrap(),
            BurnStatus::StillAlive
        );
                   
        s.decrement(&42).await.unwrap();
        assert_eq!(
            s.check_and_decrement(&42).await.unwrap(),
            BurnStatus::JustBurned
        );
                                                                      
                                               
        assert_eq!(
            s.check_and_decrement(&42).await,
            Err(AttemptError::Exhausted)
        );
    }
}
